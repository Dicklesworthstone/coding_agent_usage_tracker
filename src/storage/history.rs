//! Usage history storage layer.
//!
//! Provides persistence for usage snapshots and query helpers for historical
//! analysis. Built on top of the usage history schema and migrations.
//!
//! ## Retention Policy
//!
//! The history layer supports a tiered retention policy:
//! - **Detailed retention**: Individual snapshots kept for N days (default 30)
//! - **Aggregate retention**: Daily summaries kept for N days (default 365)
//! - **Size limit**: Maximum database size in bytes (default 100MB)
//!
//! Use `HistoryStore::prune()` or `maybe_prune()` to enforce the policy.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use rusqlite::{Connection, Row, params};

use crate::core::models::UsageSnapshot;
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::storage::history_schema::{
    DEFAULT_RETENTION_DAYS, cleanup_old_snapshots, run_migrations,
};

/// Default retention for detailed snapshots (days).
pub const DEFAULT_DETAILED_RETENTION_DAYS: i64 = 30;

/// Default retention for daily aggregates (days).
pub const DEFAULT_AGGREGATE_RETENTION_DAYS: i64 = 365;

/// Default maximum database size (100 MB).
pub const DEFAULT_MAX_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// Default minimum interval between prunes (hours).
pub const DEFAULT_PRUNE_INTERVAL_HOURS: i64 = 24;

/// Retention policy configuration.
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// Days to keep detailed snapshots before aggregating.
    pub detailed_retention_days: i64,
    /// Days to keep daily aggregates.
    pub aggregate_retention_days: i64,
    /// Maximum database size in bytes.
    pub max_size_bytes: u64,
    /// Minimum hours between automatic prune runs.
    pub prune_interval_hours: i64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            detailed_retention_days: DEFAULT_DETAILED_RETENTION_DAYS,
            aggregate_retention_days: DEFAULT_AGGREGATE_RETENTION_DAYS,
            max_size_bytes: DEFAULT_MAX_SIZE_BYTES,
            prune_interval_hours: DEFAULT_PRUNE_INTERVAL_HOURS,
        }
    }
}

impl RetentionPolicy {
    /// Create a policy with custom detailed retention days.
    #[must_use]
    pub const fn with_detailed_days(mut self, days: i64) -> Self {
        self.detailed_retention_days = days;
        self
    }

    /// Create a policy with custom aggregate retention days.
    #[must_use]
    pub const fn with_aggregate_days(mut self, days: i64) -> Self {
        self.aggregate_retention_days = days;
        self
    }

    /// Create a policy with custom max size.
    #[must_use]
    pub const fn with_max_size(mut self, bytes: u64) -> Self {
        self.max_size_bytes = bytes;
        self
    }

    /// Validate the policy configuration.
    ///
    /// # Errors
    /// Returns an error if retention days are non-positive, detailed retention
    /// is not less than aggregate retention, or max size is zero.
    pub fn validate(&self) -> Result<()> {
        if self.detailed_retention_days <= 0 {
            return Err(CautError::Config(
                "Detailed retention days must be greater than 0".to_string(),
            ));
        }
        if self.aggregate_retention_days <= 0 {
            return Err(CautError::Config(
                "Aggregate retention days must be greater than 0".to_string(),
            ));
        }
        if self.detailed_retention_days >= self.aggregate_retention_days {
            return Err(CautError::Config(
                "Detailed retention must be less than aggregate retention".to_string(),
            ));
        }
        if self.max_size_bytes == 0 {
            return Err(CautError::Config(
                "Max size must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Result of a prune operation.
#[derive(Debug, Clone, Default)]
pub struct PruneResult {
    /// Number of detailed snapshots deleted.
    pub detailed_deleted: usize,
    /// Number of daily aggregates created.
    pub aggregates_created: usize,
    /// Number of old aggregates deleted.
    pub aggregates_deleted: usize,
    /// Approximate bytes freed.
    pub bytes_freed: u64,
    /// Duration of the prune operation.
    pub duration_ms: u64,
    /// Whether this was a dry run.
    pub dry_run: bool,
    /// Whether size limit triggered additional cleanup.
    pub size_limit_triggered: bool,
}

/// History database access layer.
pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    /// Create or open a history database at the given path.
    ///
    /// # Errors
    /// Returns an error if the parent directory cannot be created, the database
    /// cannot be opened, or schema migrations fail.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conn = Connection::open(path)
            .map_err(|e| CautError::Other(anyhow::anyhow!("open history db: {e}")))?;

        run_migrations(&mut conn)?;

        Ok(Self { conn })
    }

    /// Open an in-memory history database (for testing).
    ///
    /// # Errors
    /// Returns an error if the in-memory database cannot be opened or migrations fail.
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()
            .map_err(|e| CautError::Other(anyhow::anyhow!("open in-memory db: {e}")))?;

        run_migrations(&mut conn)?;

        Ok(Self { conn })
    }

    /// Record a usage snapshot for a provider.
    ///
    /// # Errors
    /// Returns an error if the INSERT statement cannot be prepared or executed.
    pub fn record_snapshot(&self, snapshot: &UsageSnapshot, provider: &Provider) -> Result<i64> {
        let primary = snapshot.primary.as_ref();
        let secondary = snapshot.secondary.as_ref();
        let tertiary = snapshot.tertiary.as_ref();

        let identity = snapshot.identity.as_ref();

        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO usage_snapshots ( \
                provider, fetched_at, source, \
                primary_used_pct, primary_window_minutes, primary_resets_at, \
                secondary_used_pct, secondary_window_minutes, secondary_resets_at, \
                tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at, \
                cost_today_usd, cost_mtd_usd, credits_remaining, \
                account_email, account_org, fetch_duration_ms \
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"
        )
        .map_err(|e| CautError::Other(anyhow::anyhow!("prepare insert: {e}")))?;

        stmt.execute(params![
            provider.cli_name(),
            snapshot.updated_at.to_rfc3339(),
            "unknown",
            primary.map(|p| p.used_percent),
            primary.and_then(|p| p.window_minutes),
            primary
                .and_then(|p| p.resets_at.as_ref())
                .map(chrono::DateTime::to_rfc3339),
            secondary.map(|p| p.used_percent),
            secondary.and_then(|p| p.window_minutes),
            secondary
                .and_then(|p| p.resets_at.as_ref())
                .map(chrono::DateTime::to_rfc3339),
            tertiary.map(|p| p.used_percent),
            tertiary.and_then(|p| p.window_minutes),
            tertiary
                .and_then(|p| p.resets_at.as_ref())
                .map(chrono::DateTime::to_rfc3339),
            Option::<f64>::None,
            Option::<f64>::None,
            Option::<f64>::None,
            identity.and_then(|i| i.account_email.clone()),
            identity.and_then(|i| i.account_organization.clone()),
            Option::<i64>::None,
        ])
        .map_err(|e| CautError::Other(anyhow::anyhow!("insert snapshot: {e}")))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get snapshots for a provider within a time range.
    ///
    /// # Errors
    /// Returns an error if the time range is invalid (`from > to`) or the query fails.
    pub fn get_snapshots(
        &self,
        provider: &Provider,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<StoredSnapshot>> {
        if from > to {
            return Err(CautError::Config(
                "Time range start must be before end".to_string(),
            ));
        }

        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT \
                id, provider, fetched_at, source, \
                primary_used_pct, primary_window_minutes, primary_resets_at, \
                secondary_used_pct, secondary_window_minutes, secondary_resets_at, \
                tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at, \
                cost_today_usd, cost_mtd_usd, credits_remaining, \
                account_email, account_org, fetch_duration_ms, created_at \
            FROM usage_snapshots \
            WHERE provider = ?1 AND fetched_at BETWEEN ?2 AND ?3 \
            ORDER BY fetched_at DESC",
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare select: {e}")))?;

        let rows = stmt
            .query_map(
                params![provider.cli_name(), from.to_rfc3339(), to.to_rfc3339()],
                map_row,
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("query snapshots: {e}")))?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(row.map_err(|e| CautError::Other(anyhow::anyhow!("map row: {e}")))?);
        }

        Ok(snapshots)
    }

    /// Get the latest snapshot for each provider.
    ///
    /// # Errors
    /// Returns an error if the SELECT query cannot be prepared or executed.
    pub fn get_latest_all(&self) -> Result<HashMap<Provider, StoredSnapshot>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT \
                id, provider, fetched_at, source, \
                primary_used_pct, primary_window_minutes, primary_resets_at, \
                secondary_used_pct, secondary_window_minutes, secondary_resets_at, \
                tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at, \
                cost_today_usd, cost_mtd_usd, credits_remaining, \
                account_email, account_org, fetch_duration_ms, created_at \
            FROM usage_snapshots \
            ORDER BY fetched_at DESC",
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare select: {e}")))?;

        let rows = stmt
            .query_map([], map_row)
            .map_err(|e| CautError::Other(anyhow::anyhow!("query latest: {e}")))?;

        let mut latest = HashMap::new();
        for row in rows {
            let snapshot = row.map_err(|e| CautError::Other(anyhow::anyhow!("map row: {e}")))?;
            latest.entry(snapshot.provider).or_insert(snapshot);
        }

        Ok(latest)
    }

    /// Get usage velocity (% change per hour) over a recent window.
    ///
    /// # Errors
    /// Returns an error if the velocity window is non-positive or the snapshot query fails.
    pub fn get_velocity(&self, provider: &Provider, window: Duration) -> Result<Option<f64>> {
        if window <= Duration::zero() {
            return Err(CautError::Config(
                "Velocity window must be greater than 0".to_string(),
            ));
        }

        let to = Utc::now();
        let from = to - window;
        let snapshots = self.get_snapshots(provider, from, to)?;

        Ok(crate::core::prediction::calculate_velocity(
            &snapshots, window,
        ))
    }

    /// Get aggregated stats for a time period.
    ///
    /// # Errors
    /// Returns an error if the snapshot query for the given period fails.
    pub fn get_stats(&self, provider: &Provider, period: &StatsPeriod) -> Result<UsageStats> {
        let (from, to) = period.to_range();
        let snapshots = self.get_snapshots(provider, from, to)?;

        let mut values = Vec::new();
        let mut total_cost = 0.0_f64;

        for snapshot in &snapshots {
            if let Some(value) = snapshot.primary_used_pct {
                values.push(value);
            }
            if let Some(cost) = snapshot.cost_today_usd {
                total_cost += cost;
            }
        }

        let sample_count = values.len();
        if sample_count == 0 {
            return Ok(UsageStats {
                average_primary_pct: 0.0,
                max_primary_pct: 0.0,
                min_primary_pct: 0.0,
                total_cost,
                sample_count: 0,
            });
        }

        let sum: f64 = values.iter().sum();
        #[allow(clippy::cast_precision_loss)] // sample count is small in practice
        let average_primary_pct = sum / sample_count as f64;
        let max_primary_pct = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let min_primary_pct = values.iter().copied().fold(f64::INFINITY, f64::min);

        Ok(UsageStats {
            average_primary_pct,
            max_primary_pct,
            min_primary_pct,
            total_cost,
            sample_count,
        })
    }

    /// Cleanup old snapshots using the retention window.
    ///
    /// # Errors
    /// Returns an error if the retention days are non-positive or the cleanup query fails.
    pub fn cleanup(&self, retention_days: i64) -> Result<usize> {
        cleanup_old_snapshots(&self.conn, retention_days)
    }

    /// Cleanup old snapshots with the default retention window.
    ///
    /// # Errors
    /// Returns an error if the cleanup query fails.
    pub fn cleanup_default(&self) -> Result<usize> {
        self.cleanup(DEFAULT_RETENTION_DAYS)
    }

    /// Prune the database according to the retention policy.
    ///
    /// Executes a 4-phase prune:
    /// 1. Aggregate old detailed snapshots into daily summaries
    /// 2. Delete very old aggregates
    /// 3. Check size limit and delete more if needed
    /// 4. Vacuum if significant data was removed
    ///
    /// # Errors
    /// Returns an error if the policy is invalid or any database operation
    /// during aggregation, deletion, or vacuuming fails.
    pub fn prune(&self, policy: &RetentionPolicy, dry_run: bool) -> Result<PruneResult> {
        policy.validate()?;
        let start = Instant::now();
        let mut result = PruneResult {
            dry_run,
            ..Default::default()
        };

        // Get initial size for comparison
        let initial_size = self.get_db_size()?;

        // Phase 1: Aggregate old detailed data into daily summaries
        let cutoff = Utc::now() - Duration::days(policy.detailed_retention_days);
        result.aggregates_created = self.aggregate_old_snapshots(&cutoff, dry_run)?;

        // Delete the detailed snapshots that were aggregated
        if dry_run {
            result.detailed_deleted = self.count_old_snapshots(&cutoff)?;
        } else {
            result.detailed_deleted = self.delete_old_snapshots(&cutoff)?;
        }

        // Phase 2: Delete very old aggregates
        let agg_cutoff = Utc::now() - Duration::days(policy.aggregate_retention_days);
        if dry_run {
            result.aggregates_deleted = self.count_old_aggregates(&agg_cutoff)?;
        } else {
            result.aggregates_deleted = self.delete_old_aggregates(&agg_cutoff)?;
        }

        // Phase 3: Check size limit
        let current_size = if dry_run {
            initial_size
        } else {
            self.get_db_size()?
        };
        if current_size > policy.max_size_bytes {
            result.size_limit_triggered = true;
            if !dry_run {
                self.enforce_size_limit(policy.max_size_bytes)?;
            }
        }

        // Phase 4: Vacuum if significant data removed
        let significant_change = result.detailed_deleted > 100 || result.aggregates_deleted > 10;
        if !dry_run && significant_change {
            self.conn
                .execute_batch("VACUUM")
                .map_err(|e| CautError::Other(anyhow::anyhow!("vacuum failed: {e}")))?;
        }

        // Calculate bytes freed
        let final_size = if dry_run {
            initial_size
        } else {
            self.get_db_size()?
        };
        result.bytes_freed = initial_size.saturating_sub(final_size);
        #[allow(clippy::cast_possible_truncation)] // prune duration in millis always fits u64
        {
            result.duration_ms = start.elapsed().as_millis() as u64;
        }

        // Record prune in history (if not dry run)
        if !dry_run {
            self.record_prune(&result)?;
        }

        Ok(result)
    }

    /// Prune with the default policy.
    ///
    /// # Errors
    /// Returns an error if any database operation during pruning fails.
    pub fn prune_default(&self, dry_run: bool) -> Result<PruneResult> {
        self.prune(&RetentionPolicy::default(), dry_run)
    }

    /// Check if pruning is needed based on the interval.
    ///
    /// # Errors
    /// Returns an error if the policy is invalid or any database operation fails.
    pub fn maybe_prune(&self, policy: &RetentionPolicy) -> Result<Option<PruneResult>> {
        policy.validate()?;

        // Check last prune time
        let should_prune = self.get_last_prune_time().is_none_or(|last| {
            let elapsed = Utc::now() - last;
            elapsed.num_hours() >= policy.prune_interval_hours
        });

        // Also check size - always prune if over limit
        let current_size = self.get_db_size()?;
        let over_size = current_size > policy.max_size_bytes;

        if should_prune || over_size {
            Ok(Some(self.prune(policy, false)?))
        } else {
            Ok(None)
        }
    }

    /// Count rows in a specified table.
    ///
    /// # Errors
    /// Returns an error if the table name is invalid or the COUNT query fails.
    pub fn count_rows(&self, table: &str) -> Result<i64> {
        // Validate table name to prevent SQL injection
        let valid_tables = [
            "usage_snapshots",
            "daily_aggregates",
            "prune_history",
            "schema_migrations",
        ];
        if !valid_tables.contains(&table) {
            return Err(CautError::Config(format!("Invalid table name: {table}")));
        }

        let query = format!("SELECT COUNT(*) FROM {table}");
        let count: i64 = self
            .conn
            .query_row(&query, [], |row| row.get(0))
            .map_err(|e| CautError::Other(anyhow::anyhow!("count {table}: {e}")))?;

        Ok(count)
    }

    /// Get the approximate database size in bytes.
    ///
    /// # Errors
    /// Returns an error if the PRAGMA queries for page count or page size fail.
    pub fn get_db_size(&self) -> Result<u64> {
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(|e| CautError::Other(anyhow::anyhow!("page_count: {e}")))?;

        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(|e| CautError::Other(anyhow::anyhow!("page_size: {e}")))?;

        #[allow(clippy::cast_sign_loss)] // page count and page size are non-negative
        Ok((page_count * page_size) as u64)
    }

    /// Get the last prune timestamp.
    fn get_last_prune_time(&self) -> Option<DateTime<Utc>> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT pruned_at FROM prune_history WHERE dry_run = 0 ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();

        result.and_then(|s| parse_optional_timestamp(Some(s)))
    }

    /// Record a prune operation in history.
    #[allow(clippy::cast_possible_wrap)] // prune counts and sizes are small enough to fit in i64
    fn record_prune(&self, result: &PruneResult) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO prune_history (\
                    pruned_at, detailed_deleted, aggregates_deleted, \
                    aggregates_created, bytes_freed, duration_ms, dry_run\
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    Utc::now().to_rfc3339(),
                    result.detailed_deleted as i64,
                    result.aggregates_deleted as i64,
                    result.aggregates_created as i64,
                    result.bytes_freed as i64,
                    result.duration_ms as i64,
                    i32::from(result.dry_run),
                ],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("record prune: {e}")))?;
        Ok(())
    }

    /// Aggregate old snapshots into daily summaries.
    fn aggregate_old_snapshots(&self, cutoff: &DateTime<Utc>, dry_run: bool) -> Result<usize> {
        // Find all days with snapshots older than cutoff that don't have aggregates yet
        let days_to_aggregate: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT DISTINCT provider, date(fetched_at) as day \
                 FROM usage_snapshots \
                 WHERE fetched_at < ?1 \
                 AND NOT EXISTS (\
                     SELECT 1 FROM daily_aggregates \
                     WHERE daily_aggregates.provider = usage_snapshots.provider \
                     AND daily_aggregates.date = date(fetched_at)\
                 )",
                )
                .map_err(|e| CautError::Other(anyhow::anyhow!("prepare aggregate query: {e}")))?;

            let rows = stmt
                .query_map([cutoff.to_rfc3339()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| CautError::Other(anyhow::anyhow!("query days: {e}")))?;

            let mut days = Vec::new();
            for row in rows {
                days.push(row.map_err(|e| CautError::Other(anyhow::anyhow!("map day: {e}")))?);
            }
            days
        };

        if dry_run {
            return Ok(days_to_aggregate.len());
        }

        // Create aggregates for each day/provider combo
        let mut created = 0;
        for (provider, day) in days_to_aggregate {
            if self.create_daily_aggregate(&provider, &day)? {
                created += 1;
            }
        }

        Ok(created)
    }

    /// Create a daily aggregate from snapshots for a specific provider/day.
    fn create_daily_aggregate(&self, provider: &str, day: &str) -> Result<bool> {
        // Calculate aggregate stats
        let stats: Option<AggregateStats> = self
            .conn
            .query_row(
                "SELECT \
                AVG(primary_used_pct), MAX(primary_used_pct), MIN(primary_used_pct), \
                AVG(secondary_used_pct), MAX(secondary_used_pct), MIN(secondary_used_pct), \
                AVG(tertiary_used_pct), MAX(tertiary_used_pct), MIN(tertiary_used_pct), \
                SUM(cost_today_usd), COUNT(*), MIN(fetched_at), MAX(fetched_at), \
                account_email, account_org \
             FROM usage_snapshots \
             WHERE provider = ?1 AND date(fetched_at) = ?2 \
             GROUP BY provider, date(fetched_at)",
                params![provider, day],
                |row| {
                    Ok(AggregateStats {
                        primary_avg: row.get(0)?,
                        primary_max: row.get(1)?,
                        primary_min: row.get(2)?,
                        secondary_avg: row.get(3)?,
                        secondary_max: row.get(4)?,
                        secondary_min: row.get(5)?,
                        tertiary_avg: row.get(6)?,
                        tertiary_max: row.get(7)?,
                        tertiary_min: row.get(8)?,
                        total_cost: row.get(9)?,
                        sample_count: row.get(10)?,
                        first_fetch: row.get(11)?,
                        last_fetch: row.get(12)?,
                        account_email: row.get(13)?,
                        account_org: row.get(14)?,
                    })
                },
            )
            .ok();

        let Some(stats) = stats else {
            return Ok(false);
        };

        // Insert the aggregate
        self.conn
            .execute(
                "INSERT OR REPLACE INTO daily_aggregates (\
                provider, date, \
                primary_avg_used_pct, primary_max_used_pct, primary_min_used_pct, \
                secondary_avg_used_pct, secondary_max_used_pct, secondary_min_used_pct, \
                tertiary_avg_used_pct, tertiary_max_used_pct, tertiary_min_used_pct, \
                total_cost_usd, sample_count, first_fetch, last_fetch, \
                account_email, account_org\
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    provider,
                    day,
                    stats.primary_avg,
                    stats.primary_max,
                    stats.primary_min,
                    stats.secondary_avg,
                    stats.secondary_max,
                    stats.secondary_min,
                    stats.tertiary_avg,
                    stats.tertiary_max,
                    stats.tertiary_min,
                    stats.total_cost,
                    stats.sample_count,
                    stats.first_fetch,
                    stats.last_fetch,
                    stats.account_email,
                    stats.account_org,
                ],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("insert aggregate: {e}")))?;

        Ok(true)
    }

    /// Delete old snapshots (after aggregation).
    fn delete_old_snapshots(&self, cutoff: &DateTime<Utc>) -> Result<usize> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM usage_snapshots WHERE fetched_at < ?1",
                [cutoff.to_rfc3339()],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("delete snapshots: {e}")))?;
        Ok(deleted)
    }

    /// Count old snapshots that would be deleted.
    fn count_old_snapshots(&self, cutoff: &DateTime<Utc>) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_snapshots WHERE fetched_at < ?1",
                [cutoff.to_rfc3339()],
                |row| row.get(0),
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("count snapshots: {e}")))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // COUNT(*) is non-negative and small
        Ok(count as usize)
    }

    /// Delete old aggregates.
    fn delete_old_aggregates(&self, cutoff: &DateTime<Utc>) -> Result<usize> {
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();
        let deleted = self
            .conn
            .execute(
                "DELETE FROM daily_aggregates WHERE date < ?1",
                [cutoff_date],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("delete aggregates: {e}")))?;
        Ok(deleted)
    }

    /// Count old aggregates that would be deleted.
    fn count_old_aggregates(&self, cutoff: &DateTime<Utc>) -> Result<usize> {
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM daily_aggregates WHERE date < ?1",
                [cutoff_date],
                |row| row.get(0),
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("count aggregates: {e}")))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // COUNT(*) is non-negative and small
        Ok(count as usize)
    }

    /// Enforce size limit by deleting oldest data.
    fn enforce_size_limit(&self, max_bytes: u64) -> Result<()> {
        // Delete oldest snapshots in batches until under limit
        loop {
            let current_size = self.get_db_size()?;
            if current_size <= max_bytes {
                break;
            }

            // Delete oldest 100 snapshots
            let deleted = self
                .conn
                .execute(
                    "DELETE FROM usage_snapshots WHERE id IN (\
                    SELECT id FROM usage_snapshots ORDER BY fetched_at ASC LIMIT 100\
                )",
                    [],
                )
                .map_err(|e| CautError::Other(anyhow::anyhow!("delete batch: {e}")))?;

            if deleted == 0 {
                // No more snapshots, try aggregates
                let agg_deleted = self
                    .conn
                    .execute(
                        "DELETE FROM daily_aggregates WHERE id IN (\
                        SELECT id FROM daily_aggregates ORDER BY date ASC LIMIT 100\
                    )",
                        [],
                    )
                    .map_err(|e| CautError::Other(anyhow::anyhow!("delete agg batch: {e}")))?;

                if agg_deleted == 0 {
                    break; // Nothing left to delete
                }
            }
        }

        self.conn
            .execute_batch("VACUUM")
            .map_err(|e| CautError::Other(anyhow::anyhow!("vacuum after size limit: {e}")))?;

        Ok(())
    }
}

/// Helper struct for aggregate calculation.
#[derive(Debug)]
struct AggregateStats {
    primary_avg: Option<f64>,
    primary_max: Option<f64>,
    primary_min: Option<f64>,
    secondary_avg: Option<f64>,
    secondary_max: Option<f64>,
    secondary_min: Option<f64>,
    tertiary_avg: Option<f64>,
    tertiary_max: Option<f64>,
    tertiary_min: Option<f64>,
    total_cost: Option<f64>,
    sample_count: i64,
    first_fetch: Option<String>,
    last_fetch: Option<String>,
    account_email: Option<String>,
    account_org: Option<String>,
}

/// Stored snapshot record.
#[derive(Debug, Clone)]
pub struct StoredSnapshot {
    pub id: i64,
    pub provider: Provider,
    pub fetched_at: DateTime<Utc>,
    pub source: String,

    pub primary_used_pct: Option<f64>,
    pub primary_window_minutes: Option<i32>,
    pub primary_resets_at: Option<DateTime<Utc>>,

    pub secondary_used_pct: Option<f64>,
    pub secondary_window_minutes: Option<i32>,
    pub secondary_resets_at: Option<DateTime<Utc>>,

    pub tertiary_used_pct: Option<f64>,
    pub tertiary_window_minutes: Option<i32>,
    pub tertiary_resets_at: Option<DateTime<Utc>>,

    pub cost_today_usd: Option<f64>,
    pub cost_mtd_usd: Option<f64>,
    pub credits_remaining: Option<f64>,

    pub account_email: Option<String>,
    pub account_org: Option<String>,

    pub fetch_duration_ms: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
}

/// Aggregated usage statistics.
pub struct UsageStats {
    pub average_primary_pct: f64,
    pub max_primary_pct: f64,
    pub min_primary_pct: f64,
    pub total_cost: f64,
    pub sample_count: usize,
}

/// Statistics time period selector.
#[derive(Debug, Clone)]
pub enum StatsPeriod {
    Today,
    Yesterday,
    Last7Days,
    Last30Days,
    ThisMonth,
    LastMonth,
    Custom {
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
}

impl StatsPeriod {
    fn to_range(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        match self {
            Self::Today => {
                let now = Utc::now();
                let start = start_of_day(now.date_naive());
                (start, now)
            }
            Self::Yesterday => {
                let now = Utc::now();
                let today = now.date_naive();
                let start = start_of_day(today - Duration::days(1));
                let end = start_of_day(today);
                (start, end)
            }
            Self::Last7Days => {
                let now = Utc::now();
                (now - Duration::days(7), now)
            }
            Self::Last30Days => {
                let now = Utc::now();
                (now - Duration::days(30), now)
            }
            Self::ThisMonth => {
                let now = Utc::now();
                let today = now.date_naive();
                let start = start_of_month(today);
                (start, now)
            }
            Self::LastMonth => {
                let now = Utc::now();
                let today = now.date_naive();
                let this_month = start_of_month(today);
                let last_month = start_of_previous_month(today);
                (last_month, this_month)
            }
            Self::Custom { from, to } => (*from, *to),
        }
    }
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<StoredSnapshot> {
    let provider_name: String = row.get(1)?;
    let provider = Provider::from_cli_name(&provider_name).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(StoredSnapshot {
        id: row.get(0)?,
        provider,
        fetched_at: parse_timestamp(&row.get::<_, String>(2)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        source: row.get(3)?,

        primary_used_pct: row.get(4)?,
        primary_window_minutes: row.get(5)?,
        primary_resets_at: parse_optional_timestamp(row.get(6)?),

        secondary_used_pct: row.get(7)?,
        secondary_window_minutes: row.get(8)?,
        secondary_resets_at: parse_optional_timestamp(row.get(9)?),

        tertiary_used_pct: row.get(10)?,
        tertiary_window_minutes: row.get(11)?,
        tertiary_resets_at: parse_optional_timestamp(row.get(12)?),

        cost_today_usd: row.get(13)?,
        cost_mtd_usd: row.get(14)?,
        credits_remaining: row.get(15)?,

        account_email: row.get(16)?,
        account_org: row.get(17)?,

        fetch_duration_ms: row.get(18)?,
        created_at: parse_optional_timestamp(row.get(19)?),
    })
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| CautError::Other(anyhow::anyhow!("invalid timestamp '{value}': {e}")))
}

fn parse_optional_timestamp(value: Option<String>) -> Option<DateTime<Utc>> {
    value.and_then(|v| {
        DateTime::parse_from_rfc3339(&v)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    })
}

fn start_of_day(date: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
}

fn start_of_month(date: NaiveDate) -> DateTime<Utc> {
    let start = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("start of month");
    start_of_day(start)
}

fn start_of_previous_month(date: NaiveDate) -> DateTime<Utc> {
    let (year, month) = if date.month() == 1 {
        (date.year() - 1, 12)
    } else {
        (date.year(), date.month() - 1)
    };

    let start = NaiveDate::from_ymd_opt(year, month, 1).expect("start of previous month");
    start_of_day(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{ProviderIdentity, RateWindow, UsageSnapshot};

    fn open_temp_store() -> HistoryStore {
        HistoryStore::open_in_memory().expect("open store")
    }

    fn make_snapshot(at: DateTime<Utc>, used: f64) -> UsageSnapshot {
        UsageSnapshot {
            primary: Some(RateWindow {
                used_percent: used,
                window_minutes: Some(180),
                resets_at: Some(at + Duration::minutes(30)),
                reset_description: None,
            }),
            secondary: None,
            tertiary: None,
            updated_at: at,
            identity: Some(ProviderIdentity {
                account_email: Some("user@example.com".to_string()),
                account_organization: Some("org".to_string()),
                login_method: Some("test".to_string()),
            }),
        }
    }

    #[test]
    fn record_and_query_snapshot() {
        let store = open_temp_store();
        let now = Utc::now();
        let snapshot = make_snapshot(now, 42.0);

        let id = store
            .record_snapshot(&snapshot, &Provider::Codex)
            .expect("record snapshot");
        assert!(id > 0);

        let results = store
            .get_snapshots(
                &Provider::Codex,
                now - Duration::hours(1),
                now + Duration::hours(1),
            )
            .expect("query snapshots");

        assert_eq!(results.len(), 1);
        let stored = &results[0];
        assert_eq!(stored.provider, Provider::Codex);
        assert_eq!(stored.primary_used_pct, Some(42.0));
        assert_eq!(stored.source, "unknown");
    }

    #[test]
    fn latest_snapshot_per_provider() {
        let store = open_temp_store();
        let now = Utc::now();

        store
            .record_snapshot(
                &make_snapshot(now - Duration::minutes(10), 10.0),
                &Provider::Codex,
            )
            .expect("record codex");
        store
            .record_snapshot(&make_snapshot(now, 20.0), &Provider::Codex)
            .expect("record codex latest");
        store
            .record_snapshot(&make_snapshot(now, 30.0), &Provider::Claude)
            .expect("record claude");

        let latest = store.get_latest_all().expect("latest all");
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[&Provider::Codex].primary_used_pct, Some(20.0));
        assert_eq!(latest[&Provider::Claude].primary_used_pct, Some(30.0));
    }

    #[test]
    fn velocity_computation() {
        let store = open_temp_store();
        let now = Utc::now();

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

        // 20 percentage points over 2 hours = 10 pct/hour
        assert!((velocity - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stats_period_ranges() {
        let store = open_temp_store();
        let now = Utc::now();

        store
            .record_snapshot(
                &make_snapshot(now - Duration::hours(1), 10.0),
                &Provider::Codex,
            )
            .expect("record 1");
        store
            .record_snapshot(
                &make_snapshot(now - Duration::minutes(10), 30.0),
                &Provider::Codex,
            )
            .expect("record 2");

        let stats = store
            .get_stats(&Provider::Codex, &StatsPeriod::Last7Days)
            .expect("stats");
        assert_eq!(stats.sample_count, 2);
        assert!((stats.average_primary_pct - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cleanup_defaults_to_retention() {
        let store = open_temp_store();
        let deleted = store.cleanup_default().expect("cleanup");
        assert_eq!(deleted, 0);
    }

    #[test]
    fn prune_aggregates_old_snapshots() {
        let store = open_temp_store();
        let now = Utc::now();
        let old_date = now - Duration::days(10);

        // Insert old snapshot (10 days old)
        store
            .record_snapshot(&make_snapshot(old_date, 50.0), &Provider::Codex)
            .expect("record old");

        // Insert new snapshot (1 day old)
        store
            .record_snapshot(
                &make_snapshot(now - Duration::days(1), 60.0),
                &Provider::Codex,
            )
            .expect("record new");

        // Policy: keep detailed for 5 days
        let policy = RetentionPolicy::default().with_detailed_days(5);

        let result = store.prune(&policy, false).expect("prune");

        assert_eq!(result.detailed_deleted, 1);
        assert_eq!(result.aggregates_created, 1);

        // Verify detailed counts
        let detailed_count = store
            .count_rows("usage_snapshots")
            .expect("count snapshots");
        assert_eq!(detailed_count, 1); // Only the new one remains

        // Verify aggregate counts
        let agg_count = store
            .count_rows("daily_aggregates")
            .expect("count aggregates");
        assert_eq!(agg_count, 1);

        // Verify aggregate data directly.
        let agg_row: f64 = store
            .conn
            .query_row(
                "SELECT primary_avg_used_pct FROM daily_aggregates",
                [],
                |row| row.get(0),
            )
            .expect("query aggregate");

        assert!((agg_row - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn maybe_prune_respects_interval() {
        let store = open_temp_store();
        let policy = RetentionPolicy::default()
            .with_detailed_days(5)
            .with_aggregate_days(10);

        // First run should happen (no history)
        let result1 = store.maybe_prune(&policy).expect("maybe prune 1");
        assert!(result1.is_some());

        // Second run immediately after should be skipped
        let result2 = store.maybe_prune(&policy).expect("maybe prune 2");
        assert!(result2.is_none());

        // Force update last prune time to be old
        store
            .conn
            .execute(
                "UPDATE prune_history SET pruned_at = ?1",
                [(Utc::now() - Duration::hours(25)).to_rfc3339()],
            )
            .expect("update prune time");

        // Third run should happen
        let result3 = store.maybe_prune(&policy).expect("maybe prune 3");
        assert!(result3.is_some());
    }
}
