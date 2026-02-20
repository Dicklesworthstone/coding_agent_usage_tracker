//! Usage history schema and migrations.
//!
//! This module defines the `SQLite` schema for usage history snapshots and
//! provides migration and retention helpers. The history storage layer will
//! build on top of this schema.

use chrono::{Duration, Utc};
use rusqlite::Connection;

use crate::error::{CautError, Result};

const HISTORY_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../../migrations/001_usage_snapshots.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../../migrations/002_daily_aggregates.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../../migrations/003_multi_account.sql"),
    },
];

/// Default retention window for usage history.
pub const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Run schema migrations for the usage history database.
///
/// Returns the latest schema version applied.
///
/// # Errors
/// Returns an error if creating the migrations table, reading the schema version,
/// or applying any migration fails.
pub fn run_migrations(conn: &mut Connection) -> Result<i32> {
    ensure_schema_migrations_table(conn)?;

    let mut current_version = get_schema_version(conn)?;

    for migration in HISTORY_MIGRATIONS {
        if migration.version > current_version {
            apply_migration(conn, migration)?;
            current_version = migration.version;
        }
    }

    Ok(current_version)
}

/// Delete snapshots older than the retention window.
///
/// Returns the number of rows deleted.
///
/// # Errors
/// Returns an error if `retention_days` is non-positive, the DELETE query fails,
/// or the post-cleanup VACUUM fails.
pub fn cleanup_old_snapshots(conn: &Connection, retention_days: i64) -> Result<usize> {
    if retention_days <= 0 {
        return Err(CautError::Config(
            "Retention days must be greater than 0".to_string(),
        ));
    }

    let cutoff = Utc::now() - Duration::days(retention_days);
    let cutoff_str = cutoff.to_rfc3339();

    let deleted = conn
        .execute(
            "DELETE FROM usage_snapshots WHERE fetched_at < ?1",
            [cutoff_str],
        )
        .map_err(|e| CautError::Other(anyhow::anyhow!("cleanup failed: {e}")))?;

    if deleted > 0 {
        conn.execute_batch("VACUUM")
            .map_err(|e| CautError::Other(anyhow::anyhow!("vacuum failed: {e}")))?;
    }

    Ok(deleted)
}

#[derive(Debug, Clone, Copy)]
struct Migration {
    version: i32,
    sql: &'static str,
}

fn ensure_schema_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
            version INTEGER PRIMARY KEY,\
            applied_at TEXT DEFAULT (datetime('now'))\
        );",
    )
    .map_err(|e| CautError::Other(anyhow::anyhow!("create schema_migrations: {e}")))?;

    Ok(())
}

fn get_schema_version(conn: &Connection) -> Result<i32> {
    let version: Option<i32> = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .map_err(|e| CautError::Other(anyhow::anyhow!("read schema version: {e}")))?;

    Ok(version.unwrap_or(0))
}

fn apply_migration(conn: &mut Connection, migration: &Migration) -> Result<()> {
    let tx = conn
        .transaction()
        .map_err(|e| CautError::Other(anyhow::anyhow!("begin migration: {e}")))?;

    tx.execute_batch(migration.sql).map_err(|e| {
        CautError::Other(anyhow::anyhow!(
            "apply migration {}: {e}",
            migration.version
        ))
    })?;

    tx.execute(
        "INSERT INTO schema_migrations (version) VALUES (?1)",
        [migration.version],
    )
    .map_err(|e| {
        CautError::Other(anyhow::anyhow!(
            "record migration {}: {e}",
            migration.version
        ))
    })?;

    tx.commit().map_err(|e| {
        CautError::Other(anyhow::anyhow!(
            "commit migration {}: {e}",
            migration.version
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        Connection::open_in_memory().expect("open in-memory db")
    }

    #[test]
    fn migrations_create_schema() {
        let mut conn = open_in_memory();
        let version = run_migrations(&mut conn).expect("run migrations");

        assert_eq!(version, 3);

        let table_exists: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='usage_snapshots'",
                [],
                |row| row.get(0),
            )
            .expect("query table existence");
        assert_eq!(table_exists, 1);

        let index_exists: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_snapshots_provider_time'",
                [],
                |row| row.get(0),
            )
            .expect("query index existence");
        assert_eq!(index_exists, 1);
    }

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = open_in_memory();
        let version_first = run_migrations(&mut conn).expect("first run");
        let version_second = run_migrations(&mut conn).expect("second run");

        assert_eq!(version_first, 3);
        assert_eq!(version_second, 3);

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count migrations");
        assert_eq!(count, 3);
    }

    #[test]
    fn cleanup_removes_old_snapshots() {
        let mut conn = open_in_memory();
        run_migrations(&mut conn).expect("migrations");

        let old_time = (Utc::now() - Duration::days(120)).to_rfc3339();
        let new_time = (Utc::now() - Duration::days(10)).to_rfc3339();

        conn.execute(
            "INSERT INTO usage_snapshots (provider, fetched_at, source) VALUES (?1, ?2, ?3)",
            ("codex", old_time, "cli"),
        )
        .expect("insert old");

        conn.execute(
            "INSERT INTO usage_snapshots (provider, fetched_at, source) VALUES (?1, ?2, ?3)",
            ("codex", new_time, "cli"),
        )
        .expect("insert new");

        let deleted = cleanup_old_snapshots(&conn, 90).expect("cleanup");
        assert_eq!(deleted, 1);

        let remaining: i32 = conn
            .query_row("SELECT COUNT(*) FROM usage_snapshots", [], |row| row.get(0))
            .expect("count rows");
        assert_eq!(remaining, 1);
    }

    #[test]
    fn cleanup_rejects_non_positive_retention() {
        let conn = open_in_memory();
        let err = cleanup_old_snapshots(&conn, 0).expect_err("should error");
        assert!(matches!(err, CautError::Config(_)));
    }
}
