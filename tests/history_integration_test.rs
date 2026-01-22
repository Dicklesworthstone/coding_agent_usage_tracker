//! Comprehensive integration tests for the History subsystem.
//!
//! Tests the full history workflow including:
//! - Schema and migrations
//! - Storage layer operations
//! - Time range queries
//! - Data retention and pruning
//! - Performance benchmarks
//! - Concurrent access patterns

use std::sync::Arc;
use std::thread;
use std::time::Instant;

use chrono::{Duration, Utc};
use tempfile::TempDir;

use caut::core::models::{ProviderIdentity, RateWindow, UsageSnapshot};
use caut::core::provider::Provider;
use caut::storage::history::{
    DEFAULT_AGGREGATE_RETENTION_DAYS, DEFAULT_DETAILED_RETENTION_DAYS, HistoryStore, PruneResult,
    RetentionPolicy, StatsPeriod, StoredSnapshot,
};

mod common;

// =============================================================================
// Test Helpers
// =============================================================================

/// Create a test snapshot with given parameters.
fn make_snapshot(at: chrono::DateTime<Utc>, primary_pct: f64) -> UsageSnapshot {
    UsageSnapshot {
        primary: Some(RateWindow {
            used_percent: primary_pct,
            window_minutes: Some(180),
            resets_at: Some(at + Duration::minutes(30)),
            reset_description: Some("resets in 30m".to_string()),
        }),
        secondary: None,
        tertiary: None,
        updated_at: at,
        identity: Some(ProviderIdentity {
            account_email: Some("test@example.com".to_string()),
            account_organization: Some("Test Org".to_string()),
            login_method: Some("oauth".to_string()),
        }),
    }
}

/// Create a snapshot with secondary and tertiary windows.
fn make_full_snapshot(
    at: chrono::DateTime<Utc>,
    primary_pct: f64,
    secondary_pct: f64,
    tertiary_pct: f64,
) -> UsageSnapshot {
    UsageSnapshot {
        primary: Some(RateWindow {
            used_percent: primary_pct,
            window_minutes: Some(180),
            resets_at: Some(at + Duration::minutes(30)),
            reset_description: Some("resets in 30m".to_string()),
        }),
        secondary: Some(RateWindow {
            used_percent: secondary_pct,
            window_minutes: Some(10080),
            resets_at: Some(at + Duration::days(7)),
            reset_description: Some("resets in 7d".to_string()),
        }),
        tertiary: Some(RateWindow {
            used_percent: tertiary_pct,
            window_minutes: Some(10080),
            resets_at: Some(at + Duration::days(7)),
            reset_description: Some("Opus tier".to_string()),
        }),
        updated_at: at,
        identity: Some(ProviderIdentity {
            account_email: Some("claude@example.com".to_string()),
            account_organization: Some("Anthropic".to_string()),
            login_method: Some("oauth".to_string()),
        }),
    }
}

/// Open a temporary history store for testing.
fn open_temp_store() -> (HistoryStore, TempDir) {
    let temp = TempDir::new().expect("create temp dir");
    let db_path = temp.path().join("test-history.sqlite");
    let store = HistoryStore::open(&db_path).expect("open store");
    (store, temp)
}

// =============================================================================
// Schema and Migration Tests
// =============================================================================

#[test]
fn test_fresh_database_creation() {
    let (store, _temp) = open_temp_store();

    // Verify tables exist by counting rows
    let snapshot_count = store
        .count_rows("usage_snapshots")
        .expect("count snapshots");
    let agg_count = store
        .count_rows("daily_aggregates")
        .expect("count aggregates");

    assert_eq!(snapshot_count, 0);
    assert_eq!(agg_count, 0);
}

#[test]
fn test_in_memory_database() {
    let store = HistoryStore::open_in_memory().expect("open in-memory");

    let now = Utc::now();
    let snapshot = make_snapshot(now, 50.0);

    let id = store
        .record_snapshot(&snapshot, &Provider::Claude)
        .expect("record");
    assert!(id > 0);

    let results = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert_eq!(results.len(), 1);
}

#[test]
fn test_migration_idempotence() {
    let temp = TempDir::new().expect("create temp dir");
    let db_path = temp.path().join("test.db");

    // Open multiple times to verify migrations don't run twice
    for i in 0..3 {
        let store = HistoryStore::open(&db_path).expect(&format!("open {}", i));
        let count = store
            .count_rows("schema_migrations")
            .expect("count migrations");
        assert_eq!(count, 2, "Should have exactly 2 migrations after run {}", i);
    }
}

#[test]
fn test_database_persists_across_reopens() {
    let temp = TempDir::new().expect("create temp dir");
    let db_path = temp.path().join("persist.db");
    let now = Utc::now();

    // First session: write data
    {
        let store = HistoryStore::open(&db_path).expect("open first");
        store
            .record_snapshot(&make_snapshot(now, 42.0), &Provider::Codex)
            .expect("record");
    }

    // Second session: read data
    {
        let store = HistoryStore::open(&db_path).expect("open second");
        let latest = store.get_latest_all().expect("get latest");

        assert_eq!(latest.len(), 1);
        assert_eq!(latest[&Provider::Codex].primary_used_pct, Some(42.0));
    }
}

// =============================================================================
// Storage Layer Tests
// =============================================================================

#[test]
fn test_record_and_retrieve_snapshot() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();
    let snapshot = make_snapshot(now, 45.0);

    let id = store
        .record_snapshot(&snapshot, &Provider::Claude)
        .expect("record");
    assert!(id > 0);

    let results = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert_eq!(results.len(), 1);
    let stored = &results[0];
    assert_eq!(stored.provider, Provider::Claude);
    assert!((stored.primary_used_pct.unwrap() - 45.0).abs() < f64::EPSILON);
    assert_eq!(stored.account_email.as_deref(), Some("test@example.com"));
}

#[test]
fn test_full_snapshot_all_tiers() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();
    let snapshot = make_full_snapshot(now, 30.0, 45.0, 60.0);

    store
        .record_snapshot(&snapshot, &Provider::Claude)
        .expect("record");

    let results = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert_eq!(results.len(), 1);
    let stored = &results[0];

    assert!((stored.primary_used_pct.unwrap() - 30.0).abs() < f64::EPSILON);
    assert!((stored.secondary_used_pct.unwrap() - 45.0).abs() < f64::EPSILON);
    assert!((stored.tertiary_used_pct.unwrap() - 60.0).abs() < f64::EPSILON);
}

#[test]
fn test_time_range_queries() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert snapshots at different times
    for i in 0..10 {
        let time = now - Duration::hours(i);
        let pct = (i * 10) as f64;
        store
            .record_snapshot(&make_snapshot(time, pct), &Provider::Claude)
            .expect("record");
    }

    // Query last 5 hours
    let results = store
        .get_snapshots(&Provider::Claude, now - Duration::hours(5), now)
        .expect("query 5h");

    // Should get snapshots at hours 0,1,2,3,4,5 = 6 snapshots
    assert_eq!(results.len(), 6);

    // Query last 2 hours
    let results = store
        .get_snapshots(&Provider::Claude, now - Duration::hours(2), now)
        .expect("query 2h");

    assert_eq!(results.len(), 3);
}

#[test]
fn test_time_range_invalid_order() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Start > end should error
    let result = store.get_snapshots(&Provider::Claude, now, now - Duration::hours(1));

    assert!(result.is_err());
}

#[test]
fn test_multiple_providers_isolation() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    store
        .record_snapshot(&make_snapshot(now, 30.0), &Provider::Claude)
        .expect("record claude");
    store
        .record_snapshot(&make_snapshot(now, 50.0), &Provider::Codex)
        .expect("record codex");
    store
        .record_snapshot(&make_snapshot(now, 70.0), &Provider::Gemini)
        .expect("record gemini");

    let all = store.get_latest_all().expect("latest all");
    assert_eq!(all.len(), 3);
    assert!((all[&Provider::Claude].primary_used_pct.unwrap() - 30.0).abs() < f64::EPSILON);
    assert!((all[&Provider::Codex].primary_used_pct.unwrap() - 50.0).abs() < f64::EPSILON);
    assert!((all[&Provider::Gemini].primary_used_pct.unwrap() - 70.0).abs() < f64::EPSILON);

    // Query single provider
    let claude_only = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("claude only");
    assert_eq!(claude_only.len(), 1);
    assert_eq!(claude_only[0].provider, Provider::Claude);
}

#[test]
fn test_latest_snapshot_per_provider() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert older then newer for same provider
    store
        .record_snapshot(
            &make_snapshot(now - Duration::minutes(10), 10.0),
            &Provider::Codex,
        )
        .expect("record old");
    store
        .record_snapshot(&make_snapshot(now, 20.0), &Provider::Codex)
        .expect("record new");

    let latest = store.get_latest_all().expect("latest");
    assert_eq!(latest.len(), 1);
    // Should get the newest one
    assert!((latest[&Provider::Codex].primary_used_pct.unwrap() - 20.0).abs() < f64::EPSILON);
}

// =============================================================================
// Velocity Computation Tests
// =============================================================================

#[test]
fn test_velocity_computation() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // 10% used 2 hours ago, 30% now = 10 pct/hour velocity
    store
        .record_snapshot(
            &make_snapshot(now - Duration::hours(2), 10.0),
            &Provider::Codex,
        )
        .expect("record old");
    store
        .record_snapshot(&make_snapshot(now, 30.0), &Provider::Codex)
        .expect("record new");

    let velocity = store
        .get_velocity(&Provider::Codex, Duration::hours(3))
        .expect("velocity")
        .expect("some velocity");

    // 20 pct over 2 hours = 10 pct/hour
    assert!((velocity - 10.0).abs() < 0.1);
}

#[test]
fn test_velocity_insufficient_data() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Only one snapshot
    store
        .record_snapshot(&make_snapshot(now, 50.0), &Provider::Claude)
        .expect("record");

    let velocity = store
        .get_velocity(&Provider::Claude, Duration::hours(1))
        .expect("velocity");

    assert!(velocity.is_none());
}

#[test]
fn test_velocity_no_data() {
    let store = HistoryStore::open_in_memory().expect("open store");

    let velocity = store
        .get_velocity(&Provider::Claude, Duration::hours(1))
        .expect("velocity");

    assert!(velocity.is_none());
}

#[test]
fn test_velocity_negative_window() {
    let store = HistoryStore::open_in_memory().expect("open store");

    let result = store.get_velocity(&Provider::Claude, Duration::zero());
    assert!(result.is_err());

    let result = store.get_velocity(&Provider::Claude, Duration::hours(-1));
    assert!(result.is_err());
}

// =============================================================================
// Statistics Tests
// =============================================================================

#[test]
fn test_stats_computation() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert varied data
    for pct in [10.0, 30.0, 50.0, 70.0, 90.0] {
        let time = now - Duration::hours(1);
        store
            .record_snapshot(&make_snapshot(time, pct), &Provider::Claude)
            .expect("record");
    }

    let stats = store
        .get_stats(&Provider::Claude, StatsPeriod::Today)
        .expect("stats");

    assert_eq!(stats.sample_count, 5);
    assert!((stats.average_primary_pct - 50.0).abs() < f64::EPSILON);
    assert!((stats.max_primary_pct - 90.0).abs() < f64::EPSILON);
    assert!((stats.min_primary_pct - 10.0).abs() < f64::EPSILON);
}

#[test]
fn test_stats_empty_period() {
    let store = HistoryStore::open_in_memory().expect("open store");

    let stats = store
        .get_stats(&Provider::Claude, StatsPeriod::Yesterday)
        .expect("stats");

    assert_eq!(stats.sample_count, 0);
    assert!((stats.average_primary_pct - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_stats_last_7_days() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert data across multiple days
    for day in 0..7 {
        let time = now - Duration::days(day);
        let pct = ((day + 1) * 10) as f64;
        store
            .record_snapshot(&make_snapshot(time, pct), &Provider::Codex)
            .expect("record");
    }

    let stats = store
        .get_stats(&Provider::Codex, StatsPeriod::Last7Days)
        .expect("stats");

    assert_eq!(stats.sample_count, 7);
}

// =============================================================================
// Retention Policy Tests
// =============================================================================

#[test]
fn test_retention_policy_validation() {
    // Valid policy
    let policy = RetentionPolicy::default();
    assert!(policy.validate().is_ok());

    // Invalid: detailed >= aggregate
    let bad_policy = RetentionPolicy::default()
        .with_detailed_days(365)
        .with_aggregate_days(30);
    assert!(bad_policy.validate().is_err());

    // Invalid: zero retention
    let bad_policy = RetentionPolicy::default().with_detailed_days(0);
    assert!(bad_policy.validate().is_err());

    // Invalid: zero max size
    let bad_policy = RetentionPolicy::default().with_max_size(0);
    assert!(bad_policy.validate().is_err());
}

#[test]
fn test_prune_aggregates_old_data() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert old snapshot (10 days old)
    let old_time = now - Duration::days(10);
    store
        .record_snapshot(&make_snapshot(old_time, 50.0), &Provider::Codex)
        .expect("record old");

    // Insert recent snapshot
    store
        .record_snapshot(
            &make_snapshot(now - Duration::days(1), 60.0),
            &Provider::Codex,
        )
        .expect("record recent");

    // Policy: keep detailed for 5 days
    let policy = RetentionPolicy::default().with_detailed_days(5);

    let result = store.prune(&policy, false).expect("prune");

    assert_eq!(result.detailed_deleted, 1);
    assert_eq!(result.aggregates_created, 1);
    assert!(!result.dry_run);

    // Verify detailed count
    let count = store.count_rows("usage_snapshots").expect("count");
    assert_eq!(count, 1); // Only recent one remains
}

#[test]
fn test_prune_dry_run() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert old snapshot
    store
        .record_snapshot(
            &make_snapshot(now - Duration::days(10), 50.0),
            &Provider::Claude,
        )
        .expect("record old");

    let policy = RetentionPolicy::default().with_detailed_days(5);

    // Dry run
    let result = store.prune(&policy, true).expect("dry run");

    assert!(result.dry_run);
    assert_eq!(result.detailed_deleted, 1); // Would delete 1

    // Verify nothing actually deleted
    let count = store.count_rows("usage_snapshots").expect("count");
    assert_eq!(count, 1);
}

#[test]
fn test_maybe_prune_interval() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let policy = RetentionPolicy::default()
        .with_detailed_days(5)
        .with_aggregate_days(10);

    // First prune should run
    let result1 = store.maybe_prune(&policy).expect("first");
    assert!(result1.is_some());

    // Immediate second should be skipped
    let result2 = store.maybe_prune(&policy).expect("second");
    assert!(result2.is_none());
}

#[test]
fn test_cleanup_old_data() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert old and new snapshots
    store
        .record_snapshot(
            &make_snapshot(now - Duration::days(100), 30.0),
            &Provider::Claude,
        )
        .expect("old");
    store
        .record_snapshot(
            &make_snapshot(now - Duration::days(50), 40.0),
            &Provider::Claude,
        )
        .expect("medium");
    store
        .record_snapshot(&make_snapshot(now, 50.0), &Provider::Claude)
        .expect("new");

    // Cleanup with 60 day retention
    let deleted = store.cleanup(60).expect("cleanup");

    // Should delete the 100-day-old one
    assert_eq!(deleted, 1);

    let count = store.count_rows("usage_snapshots").expect("count");
    assert_eq!(count, 2);
}

// =============================================================================
// Concurrent Access Tests
// =============================================================================

#[test]
fn test_concurrent_writes() {
    let temp = TempDir::new().expect("create temp dir");
    let db_path = temp.path().join("concurrent.db");
    let now = Utc::now();

    // Initialize database
    {
        let _ = HistoryStore::open(&db_path).expect("init");
    }

    let path = Arc::new(db_path);
    let mut handles = Vec::new();

    // Spawn multiple threads writing concurrently
    for i in 0..10 {
        let path = Arc::clone(&path);
        let handle = thread::spawn(move || {
            let store = HistoryStore::open(&path).expect("open");
            let snapshot = make_snapshot(now - Duration::minutes(i), (i * 10) as f64);
            store
                .record_snapshot(&snapshot, &Provider::Claude)
                .expect("record");
        });
        handles.push(handle);
    }

    // Wait for all threads
    for h in handles {
        h.join().expect("thread join");
    }

    // Verify all writes succeeded
    let store = HistoryStore::open(&path).expect("open final");
    let results = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert_eq!(results.len(), 10);
}

#[test]
fn test_concurrent_read_write() {
    let temp = TempDir::new().expect("create temp dir");
    let db_path = temp.path().join("rw.db");
    let now = Utc::now();

    // Initialize with some data
    {
        let store = HistoryStore::open(&db_path).expect("init");
        for i in 0..5 {
            store
                .record_snapshot(
                    &make_snapshot(now - Duration::hours(i), (i * 10) as f64),
                    &Provider::Claude,
                )
                .expect("seed");
        }
    }

    let path = Arc::new(db_path);
    let mut handles = Vec::new();

    // Writers
    for i in 0..5 {
        let path = Arc::clone(&path);
        let handle = thread::spawn(move || {
            let store = HistoryStore::open(&path).expect("open");
            let snapshot = make_snapshot(now, (50 + i * 5) as f64);
            store
                .record_snapshot(&snapshot, &Provider::Codex)
                .expect("write");
        });
        handles.push(handle);
    }

    // Readers
    for _ in 0..5 {
        let path = Arc::clone(&path);
        let handle = thread::spawn(move || {
            let store = HistoryStore::open(&path).expect("open");
            let _ = store.get_latest_all().expect("read");
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("join");
    }
}

// =============================================================================
// Performance Benchmark Tests
// =============================================================================

#[test]
fn bench_snapshot_insert_1000() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    let start = Instant::now();
    for i in 0..1000 {
        let snapshot = make_snapshot(now - Duration::minutes(i), (i % 100) as f64);
        store
            .record_snapshot(&snapshot, &Provider::Claude)
            .expect("record");
    }
    let elapsed = start.elapsed();

    // Should complete 1000 inserts in < 2 seconds
    assert!(
        elapsed.as_secs() < 2,
        "Too slow: {:?} for 1000 inserts ({:?}/insert)",
        elapsed,
        elapsed / 1000
    );

    println!(
        "Performance: 1000 inserts in {:?} ({:?}/insert)",
        elapsed,
        elapsed / 1000
    );
}

#[test]
fn bench_time_range_query_10k_rows() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert 10000 snapshots
    for i in 0..10000 {
        let snapshot = make_snapshot(now - Duration::minutes(i), (i % 100) as f64);
        store
            .record_snapshot(&snapshot, &Provider::Claude)
            .expect("record");
    }

    let start = Instant::now();
    let results = store
        .get_snapshots(&Provider::Claude, now - Duration::days(30), now)
        .expect("query");
    let elapsed = start.elapsed();

    assert!(!results.is_empty());

    // Query should be < 500ms even with 10k rows
    assert!(
        elapsed.as_millis() < 500,
        "Query too slow: {:?} for {} rows",
        elapsed,
        results.len()
    );

    println!("Performance: query {} rows in {:?}", results.len(), elapsed);
}

#[test]
fn bench_get_latest_all_100_providers() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert data for multiple providers
    let providers = [
        Provider::Claude,
        Provider::Codex,
        Provider::Gemini,
        Provider::Cursor,
        Provider::Copilot,
    ];

    for provider in &providers {
        for i in 0..100 {
            let snapshot = make_snapshot(now - Duration::minutes(i), (i % 100) as f64);
            store.record_snapshot(&snapshot, provider).expect("record");
        }
    }

    let start = Instant::now();
    let latest = store.get_latest_all().expect("latest");
    let elapsed = start.elapsed();

    assert_eq!(latest.len(), providers.len());

    // Should be fast
    assert!(
        elapsed.as_millis() < 100,
        "get_latest_all too slow: {:?}",
        elapsed
    );

    println!(
        "Performance: get_latest_all for {} providers in {:?}",
        providers.len(),
        elapsed
    );
}

#[test]
fn bench_velocity_computation() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert data points over 24 hours
    for i in 0..100 {
        let time = now - Duration::minutes(i * 15); // Every 15 minutes
        let pct = 10.0 + (i as f64 * 0.5); // Slowly increasing
        store
            .record_snapshot(&make_snapshot(time, pct), &Provider::Claude)
            .expect("record");
    }

    let start = Instant::now();
    for _ in 0..100 {
        let _ = store
            .get_velocity(&Provider::Claude, Duration::hours(6))
            .expect("velocity");
    }
    let elapsed = start.elapsed();

    // 100 velocity computations should be fast
    assert!(
        elapsed.as_millis() < 500,
        "Velocity computation too slow: {:?} for 100 calls",
        elapsed
    );

    println!("Performance: 100 velocity computations in {:?}", elapsed);
}

#[test]
fn bench_prune_large_dataset() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert old data (to be pruned)
    for i in 0..1000 {
        let time = now - Duration::days(40) - Duration::minutes(i);
        store
            .record_snapshot(&make_snapshot(time, (i % 100) as f64), &Provider::Claude)
            .expect("record old");
    }

    // Insert recent data (to keep)
    for i in 0..100 {
        let time = now - Duration::hours(i);
        store
            .record_snapshot(&make_snapshot(time, (i % 100) as f64), &Provider::Claude)
            .expect("record recent");
    }

    let policy = RetentionPolicy::default().with_detailed_days(30);

    let start = Instant::now();
    let result = store.prune(&policy, false).expect("prune");
    let elapsed = start.elapsed();

    assert_eq!(result.detailed_deleted, 1000);
    assert!(result.aggregates_created > 0);

    // Prune should be fast
    assert!(elapsed.as_secs() < 5, "Prune too slow: {:?}", elapsed);

    println!(
        "Performance: prune {} rows in {:?}",
        result.detailed_deleted, elapsed
    );
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_snapshot_with_no_identity() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    let snapshot = UsageSnapshot {
        primary: Some(RateWindow {
            used_percent: 50.0,
            window_minutes: Some(180),
            resets_at: None,
            reset_description: None,
        }),
        secondary: None,
        tertiary: None,
        updated_at: now,
        identity: None,
    };

    let id = store
        .record_snapshot(&snapshot, &Provider::Claude)
        .expect("record");
    assert!(id > 0);

    let results = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert_eq!(results.len(), 1);
    assert!(results[0].account_email.is_none());
}

#[test]
fn test_snapshot_with_no_rate_windows() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    let snapshot = UsageSnapshot {
        primary: None,
        secondary: None,
        tertiary: None,
        updated_at: now,
        identity: Some(ProviderIdentity {
            account_email: Some("test@test.com".to_string()),
            account_organization: None,
            login_method: None,
        }),
    };

    let id = store
        .record_snapshot(&snapshot, &Provider::Claude)
        .expect("record");
    assert!(id > 0);

    let results = store
        .get_snapshots(
            &Provider::Claude,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert_eq!(results.len(), 1);
    assert!(results[0].primary_used_pct.is_none());
}

#[test]
fn test_empty_query_result() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    let results = store
        .get_snapshots(&Provider::Claude, now - Duration::hours(1), now)
        .expect("query");

    assert!(results.is_empty());
}

#[test]
fn test_query_wrong_provider() {
    let store = HistoryStore::open_in_memory().expect("open store");
    let now = Utc::now();

    // Insert for Claude
    store
        .record_snapshot(&make_snapshot(now, 50.0), &Provider::Claude)
        .expect("record");

    // Query for Codex
    let results = store
        .get_snapshots(
            &Provider::Codex,
            now - Duration::hours(1),
            now + Duration::hours(1),
        )
        .expect("query");

    assert!(results.is_empty());
}

#[test]
fn test_database_size_tracking() {
    let (store, _temp) = open_temp_store();
    let now = Utc::now();

    let initial_size = store.get_db_size().expect("initial size");
    assert!(initial_size > 0); // SQLite has overhead

    // Insert data
    for i in 0..100 {
        store
            .record_snapshot(
                &make_snapshot(now - Duration::minutes(i), (i % 100) as f64),
                &Provider::Claude,
            )
            .expect("record");
    }

    let new_size = store.get_db_size().expect("new size");
    assert!(new_size > initial_size);
}
