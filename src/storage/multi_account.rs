//! Multi-account storage layer.
//!
//! Provides types and database operations for multi-account tracking,
//! including account registry, switch logging, provider health, and
//! account-linked usage snapshots.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{CautError, Result};

/// A registered account in the multi-account system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Unique identifier (UUID).
    pub id: String,
    /// Provider name (claude, codex, gemini).
    pub provider: String,
    /// Account email or identifier.
    pub email: String,
    /// User-defined friendly label.
    pub label: Option<String>,
    /// Hash of credential content for change detection.
    pub credential_hash: Option<String>,
    /// When the account was first seen.
    pub added_at: DateTime<Utc>,
    /// Last activity timestamp.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Whether the account is active.
    pub is_active: bool,
    /// Provider-specific metadata (JSON).
    pub metadata: Option<String>,
}

impl Account {
    /// Create a new account with generated UUID.
    #[must_use]
    pub fn new(provider: &str, email: &str) -> Self {
        Self {
            id: uuid_v4(),
            provider: provider.to_string(),
            email: email.to_string(),
            label: None,
            credential_hash: None,
            added_at: Utc::now(),
            last_seen_at: None,
            is_active: true,
            metadata: None,
        }
    }

    /// Set a user-defined label.
    #[must_use]
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// Set the credential hash.
    #[must_use]
    pub fn with_credential_hash(mut self, hash: &str) -> Self {
        self.credential_hash = Some(hash.to_string());
        self
    }
}

/// Entry in the switch log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchLogEntry {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub from_account_id: Option<String>,
    pub to_account_id: String,
    pub trigger_type: String,
    pub trigger_details: Option<String>,
    pub success: bool,
    pub rollback: bool,
    pub error_message: Option<String>,
}

/// Trigger type for account switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchTrigger {
    /// User-initiated manual switch.
    Manual,
    /// Automatic switch due to usage threshold.
    Threshold,
    /// Automatic switch based on usage forecast.
    Forecast,
    /// Automatic switch after hitting rate limit.
    RateLimit,
    /// Scheduled rotation.
    Schedule,
}

impl SwitchTrigger {
    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Threshold => "threshold",
            Self::Forecast => "forecast",
            Self::RateLimit => "rate_limit",
            Self::Schedule => "schedule",
        }
    }
}

/// Provider health and circuit breaker state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub provider: String,
    pub last_success: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    pub circuit_state: CircuitState,
    pub opened_at: Option<DateTime<Utc>>,
    pub avg_latency_ms: Option<i32>,
    pub p95_latency_ms: Option<i32>,
    pub total_requests: i64,
    pub total_failures: i64,
}

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// Normal operation, requests pass through.
    #[default]
    Closed,
    /// Circuit tripped, requests fail fast.
    Open,
    /// Testing if service recovered.
    HalfOpen,
}

impl CircuitState {
    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::HalfOpen => "half_open",
        }
    }

    /// Parse from database string.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "open" => Self::Open,
            "half_open" => Self::HalfOpen,
            _ => Self::Closed,
        }
    }
}

/// Trigger type for usage snapshot captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotTrigger {
    /// User-initiated manual capture.
    #[default]
    Manual,
    /// Automatic capture on account switch.
    Switch,
    /// Periodic/scheduled capture.
    Periodic,
}

impl SnapshotTrigger {
    /// Convert to database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Switch => "switch",
            Self::Periodic => "periodic",
        }
    }

    /// Parse from database string.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "switch" => Self::Switch,
            "periodic" => Self::Periodic,
            _ => Self::Manual,
        }
    }
}

/// A usage snapshot record linked to an account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshotRecord {
    /// Database row ID.
    pub id: i64,
    /// Account ID this snapshot belongs to (optional for legacy data).
    pub account_id: Option<String>,
    /// Provider name.
    pub provider: String,
    /// When the snapshot was captured.
    pub fetched_at: DateTime<Utc>,
    /// How this snapshot was triggered.
    pub trigger_type: SnapshotTrigger,
    /// Source of the data (oauth, cli, web, etc.).
    pub source: String,

    // Primary rate window
    pub primary_used_pct: Option<f64>,
    pub primary_window_minutes: Option<i32>,
    pub primary_resets_at: Option<DateTime<Utc>>,

    // Secondary rate window
    pub secondary_used_pct: Option<f64>,
    pub secondary_window_minutes: Option<i32>,
    pub secondary_resets_at: Option<DateTime<Utc>>,

    // Tertiary rate window
    pub tertiary_used_pct: Option<f64>,
    pub tertiary_window_minutes: Option<i32>,
    pub tertiary_resets_at: Option<DateTime<Utc>>,

    // Cost data
    pub cost_today_usd: Option<f64>,
    pub cost_mtd_usd: Option<f64>,
    pub credits_remaining: Option<f64>,

    // Identity info
    pub account_email: Option<String>,
    pub account_org: Option<String>,

    // Metadata
    pub fetch_duration_ms: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
}

/// Builder for creating new usage snapshots.
#[derive(Debug, Clone, Default)]
pub struct NewUsageSnapshot {
    pub account_id: Option<String>,
    pub provider: String,
    pub fetched_at: DateTime<Utc>,
    pub trigger_type: SnapshotTrigger,
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
}

impl NewUsageSnapshot {
    /// Create a new snapshot builder for a provider.
    #[must_use]
    pub fn new(provider: &str) -> Self {
        Self {
            provider: provider.to_string(),
            fetched_at: Utc::now(),
            source: "cli".to_string(),
            ..Default::default()
        }
    }

    /// Set the account ID.
    #[must_use]
    pub fn with_account(mut self, account_id: &str) -> Self {
        self.account_id = Some(account_id.to_string());
        self
    }

    /// Set the trigger type.
    #[must_use]
    pub fn with_trigger(mut self, trigger: SnapshotTrigger) -> Self {
        self.trigger_type = trigger;
        self
    }

    /// Set the source.
    #[must_use]
    pub fn with_source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    /// Set the primary rate window.
    #[must_use]
    pub fn with_primary(mut self, used_pct: f64, window_minutes: Option<i32>, resets_at: Option<DateTime<Utc>>) -> Self {
        self.primary_used_pct = Some(used_pct);
        self.primary_window_minutes = window_minutes;
        self.primary_resets_at = resets_at;
        self
    }

    /// Set the secondary rate window.
    #[must_use]
    pub fn with_secondary(mut self, used_pct: f64, window_minutes: Option<i32>, resets_at: Option<DateTime<Utc>>) -> Self {
        self.secondary_used_pct = Some(used_pct);
        self.secondary_window_minutes = window_minutes;
        self.secondary_resets_at = resets_at;
        self
    }

    /// Set account identity info.
    #[must_use]
    pub fn with_identity(mut self, email: Option<&str>, org: Option<&str>) -> Self {
        self.account_email = email.map(String::from);
        self.account_org = org.map(String::from);
        self
    }
}

/// Multi-account database operations.
pub struct MultiAccountDb<'a> {
    conn: &'a Connection,
}

impl<'a> MultiAccountDb<'a> {
    /// Create a new database handle.
    #[must_use]
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    // ===== Account CRUD =====

    /// Insert a new account.
    pub fn insert_account(&self, account: &Account) -> Result<()> {
        self.conn
            .execute(
                r"INSERT INTO accounts (id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata)
                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    account.id,
                    account.provider,
                    account.email,
                    account.label,
                    account.credential_hash,
                    account.added_at.to_rfc3339(),
                    account.last_seen_at.map(|t| t.to_rfc3339()),
                    account.is_active,
                    account.metadata,
                ],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("insert account: {e}")))?;
        Ok(())
    }

    /// Get an account by ID.
    pub fn get_account(&self, id: &str) -> Result<Option<Account>> {
        let result = self
            .conn
            .query_row(
                r"SELECT id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata
                  FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok(Account {
                        id: row.get(0)?,
                        provider: row.get(1)?,
                        email: row.get(2)?,
                        label: row.get(3)?,
                        credential_hash: row.get(4)?,
                        added_at: parse_datetime(row.get::<_, String>(5)?),
                        last_seen_at: row.get::<_, Option<String>>(6)?.map(parse_datetime),
                        is_active: row.get(7)?,
                        metadata: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|e| CautError::Other(anyhow::anyhow!("get account: {e}")))?;
        Ok(result)
    }

    /// Find an account by provider and email.
    pub fn find_account(&self, provider: &str, email: &str) -> Result<Option<Account>> {
        let result = self
            .conn
            .query_row(
                r"SELECT id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata
                  FROM accounts WHERE provider = ?1 AND email = ?2",
                [provider, email],
                |row| {
                    Ok(Account {
                        id: row.get(0)?,
                        provider: row.get(1)?,
                        email: row.get(2)?,
                        label: row.get(3)?,
                        credential_hash: row.get(4)?,
                        added_at: parse_datetime(row.get::<_, String>(5)?),
                        last_seen_at: row.get::<_, Option<String>>(6)?.map(parse_datetime),
                        is_active: row.get(7)?,
                        metadata: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|e| CautError::Other(anyhow::anyhow!("find account: {e}")))?;
        Ok(result)
    }

    /// List all accounts, optionally filtered by provider.
    pub fn list_accounts(&self, provider: Option<&str>) -> Result<Vec<Account>> {
        let mut accounts = Vec::new();

        let sql = match provider {
            Some(_) => {
                r"SELECT id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata
                  FROM accounts WHERE provider = ?1 AND is_active = 1 ORDER BY email"
            }
            None => {
                r"SELECT id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata
                  FROM accounts WHERE is_active = 1 ORDER BY provider, email"
            }
        };

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare list accounts: {e}")))?;

        let rows = if let Some(p) = provider {
            stmt.query([p])
        } else {
            stmt.query([])
        }
        .map_err(|e| CautError::Other(anyhow::anyhow!("query accounts: {e}")))?;

        let mapped = rows.mapped(|row| {
            Ok(Account {
                id: row.get(0)?,
                provider: row.get(1)?,
                email: row.get(2)?,
                label: row.get(3)?,
                credential_hash: row.get(4)?,
                added_at: parse_datetime(row.get::<_, String>(5)?),
                last_seen_at: row.get::<_, Option<String>>(6)?.map(parse_datetime),
                is_active: row.get(7)?,
                metadata: row.get(8)?,
            })
        });

        for account in mapped {
            accounts.push(
                account.map_err(|e| CautError::Other(anyhow::anyhow!("read account row: {e}")))?,
            );
        }

        Ok(accounts)
    }

    /// Update account's last_seen_at timestamp.
    pub fn touch_account(&self, id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET last_seen_at = ?1 WHERE id = ?2",
                params![Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("touch account: {e}")))?;
        Ok(())
    }

    /// Update account's credential hash.
    pub fn update_credential_hash(&self, id: &str, hash: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET credential_hash = ?1, last_seen_at = ?2 WHERE id = ?3",
                params![hash, Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("update credential hash: {e}")))?;
        Ok(())
    }

    /// Deactivate an account.
    pub fn deactivate_account(&self, id: &str) -> Result<()> {
        self.conn
            .execute("UPDATE accounts SET is_active = 0 WHERE id = ?1", [id])
            .map_err(|e| CautError::Other(anyhow::anyhow!("deactivate account: {e}")))?;
        Ok(())
    }

    /// Reactivate a deactivated account.
    pub fn reactivate_account(&self, id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET is_active = 1, last_seen_at = ?1 WHERE id = ?2",
                params![Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("reactivate account: {e}")))?;
        Ok(())
    }

    /// Update an account's label.
    pub fn update_label(&self, id: &str, label: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET label = ?1, last_seen_at = ?2 WHERE id = ?3",
                params![label, Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("update label: {e}")))?;
        Ok(())
    }

    /// Update an account's metadata.
    pub fn update_metadata(&self, id: &str, metadata: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET metadata = ?1, last_seen_at = ?2 WHERE id = ?3",
                params![metadata, Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("update metadata: {e}")))?;
        Ok(())
    }

    /// Upsert an account: creates if not exists, updates if exists.
    ///
    /// Uses (provider, email) as the unique key. On conflict, updates:
    /// - label (if provided in the new account)
    /// - credential_hash (if provided)
    /// - last_seen_at (always updated)
    /// - is_active (set to true)
    /// - metadata (if provided)
    ///
    /// Returns the account ID (existing or new).
    pub fn upsert_account(&self, account: &Account) -> Result<String> {
        // Try to find existing account first
        if let Some(existing) = self.find_account(&account.provider, &account.email)? {
            // Update existing account
            self.conn
                .execute(
                    r"UPDATE accounts SET
                        label = COALESCE(?1, label),
                        credential_hash = COALESCE(?2, credential_hash),
                        last_seen_at = ?3,
                        is_active = 1,
                        metadata = COALESCE(?4, metadata)
                      WHERE id = ?5",
                    params![
                        account.label,
                        account.credential_hash,
                        Utc::now().to_rfc3339(),
                        account.metadata,
                        existing.id,
                    ],
                )
                .map_err(|e| CautError::Other(anyhow::anyhow!("upsert update: {e}")))?;

            Ok(existing.id)
        } else {
            // Insert new account
            self.insert_account(account)?;
            Ok(account.id.clone())
        }
    }

    /// Count accounts, optionally filtered by provider.
    pub fn count_accounts(&self, provider: Option<&str>) -> Result<i64> {
        let count: i64 = match provider {
            Some(p) => self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM accounts WHERE provider = ?1 AND is_active = 1",
                    [p],
                    |row| row.get(0),
                )
                .map_err(|e| CautError::Other(anyhow::anyhow!("count accounts: {e}")))?,
            None => self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM accounts WHERE is_active = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| CautError::Other(anyhow::anyhow!("count accounts: {e}")))?,
        };

        Ok(count)
    }

    /// List all accounts including inactive ones.
    pub fn list_all_accounts(&self, provider: Option<&str>) -> Result<Vec<Account>> {
        let mut accounts = Vec::new();

        let sql = match provider {
            Some(_) => {
                r"SELECT id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata
                  FROM accounts WHERE provider = ?1 ORDER BY email"
            }
            None => {
                r"SELECT id, provider, email, label, credential_hash, added_at, last_seen_at, is_active, metadata
                  FROM accounts ORDER BY provider, email"
            }
        };

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare list all accounts: {e}")))?;

        let rows = if let Some(p) = provider {
            stmt.query([p])
        } else {
            stmt.query([])
        }
        .map_err(|e| CautError::Other(anyhow::anyhow!("query all accounts: {e}")))?;

        let mapped = rows.mapped(|row| {
            Ok(Account {
                id: row.get(0)?,
                provider: row.get(1)?,
                email: row.get(2)?,
                label: row.get(3)?,
                credential_hash: row.get(4)?,
                added_at: parse_datetime(row.get::<_, String>(5)?),
                last_seen_at: row.get::<_, Option<String>>(6)?.map(parse_datetime),
                is_active: row.get(7)?,
                metadata: row.get(8)?,
            })
        });

        for account in mapped {
            accounts.push(
                account.map_err(|e| CautError::Other(anyhow::anyhow!("read account row: {e}")))?,
            );
        }

        Ok(accounts)
    }

    // ===== Switch Log =====

    /// Log an account switch.
    pub fn log_switch(
        &self,
        provider: &str,
        from_account_id: Option<&str>,
        to_account_id: &str,
        trigger: SwitchTrigger,
        trigger_details: Option<&str>,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<i64> {
        self.conn
            .execute(
                r"INSERT INTO switch_log (timestamp, provider, from_account_id, to_account_id, trigger_type, trigger_details, success, error_message)
                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Utc::now().to_rfc3339(),
                    provider,
                    from_account_id,
                    to_account_id,
                    trigger.as_str(),
                    trigger_details,
                    success,
                    error_message,
                ],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("log switch: {e}")))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get recent switch log entries.
    pub fn get_switch_log(&self, limit: i64) -> Result<Vec<SwitchLogEntry>> {
        let mut entries = Vec::new();

        let mut stmt = self
            .conn
            .prepare(
                r"SELECT id, timestamp, provider, from_account_id, to_account_id, trigger_type, trigger_details, success, rollback, error_message
                  FROM switch_log ORDER BY timestamp DESC LIMIT ?1",
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare switch log: {e}")))?;

        let rows = stmt
            .query([limit])
            .map_err(|e| CautError::Other(anyhow::anyhow!("query switch log: {e}")))?;

        let mapped = rows.mapped(|row| {
            Ok(SwitchLogEntry {
                id: row.get(0)?,
                timestamp: parse_datetime(row.get::<_, String>(1)?),
                provider: row.get(2)?,
                from_account_id: row.get(3)?,
                to_account_id: row.get(4)?,
                trigger_type: row.get(5)?,
                trigger_details: row.get(6)?,
                success: row.get(7)?,
                rollback: row.get(8)?,
                error_message: row.get(9)?,
            })
        });

        for entry in mapped {
            entries.push(
                entry.map_err(|e| CautError::Other(anyhow::anyhow!("read switch log row: {e}")))?,
            );
        }

        Ok(entries)
    }

    // ===== Provider Health =====

    /// Get or create provider health record.
    pub fn get_provider_health(&self, provider: &str) -> Result<ProviderHealth> {
        let result = self
            .conn
            .query_row(
                r"SELECT provider, last_success, last_failure, consecutive_failures, circuit_state, opened_at, avg_latency_ms, p95_latency_ms, total_requests, total_failures
                  FROM provider_health WHERE provider = ?1",
                [provider],
                |row| {
                    Ok(ProviderHealth {
                        provider: row.get(0)?,
                        last_success: row.get::<_, Option<String>>(1)?.map(parse_datetime),
                        last_failure: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                        consecutive_failures: row.get(3)?,
                        circuit_state: CircuitState::parse(&row.get::<_, String>(4)?),
                        opened_at: row.get::<_, Option<String>>(5)?.map(parse_datetime),
                        avg_latency_ms: row.get(6)?,
                        p95_latency_ms: row.get(7)?,
                        total_requests: row.get(8)?,
                        total_failures: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(|e| CautError::Other(anyhow::anyhow!("get provider health: {e}")))?;

        Ok(result.unwrap_or_else(|| ProviderHealth {
            provider: provider.to_string(),
            last_success: None,
            last_failure: None,
            consecutive_failures: 0,
            circuit_state: CircuitState::Closed,
            opened_at: None,
            avg_latency_ms: None,
            p95_latency_ms: None,
            total_requests: 0,
            total_failures: 0,
        }))
    }

    /// Record a successful request.
    pub fn record_success(&self, provider: &str, latency_ms: i32) -> Result<()> {
        self.conn
            .execute(
                r"INSERT INTO provider_health (provider, last_success, consecutive_failures, circuit_state, total_requests, avg_latency_ms, updated_at)
                  VALUES (?1, ?2, 0, 'closed', 1, ?3, ?2)
                  ON CONFLICT(provider) DO UPDATE SET
                    last_success = ?2,
                    consecutive_failures = 0,
                    circuit_state = 'closed',
                    opened_at = NULL,
                    total_requests = total_requests + 1,
                    avg_latency_ms = (COALESCE(avg_latency_ms, 0) * total_requests + ?3) / (total_requests + 1),
                    updated_at = ?2",
                params![provider, Utc::now().to_rfc3339(), latency_ms],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("record success: {e}")))?;
        Ok(())
    }

    /// Record a failed request.
    pub fn record_failure(&self, provider: &str) -> Result<()> {
        self.conn
            .execute(
                r"INSERT INTO provider_health (provider, last_failure, consecutive_failures, total_requests, total_failures, updated_at)
                  VALUES (?1, ?2, 1, 1, 1, ?2)
                  ON CONFLICT(provider) DO UPDATE SET
                    last_failure = ?2,
                    consecutive_failures = consecutive_failures + 1,
                    total_requests = total_requests + 1,
                    total_failures = total_failures + 1,
                    updated_at = ?2",
                params![provider, Utc::now().to_rfc3339()],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("record failure: {e}")))?;
        Ok(())
    }

    /// Open the circuit breaker for a provider.
    pub fn open_circuit(&self, provider: &str) -> Result<()> {
        self.conn
            .execute(
                r"UPDATE provider_health SET circuit_state = 'open', opened_at = ?1, updated_at = ?1 WHERE provider = ?2",
                params![Utc::now().to_rfc3339(), provider],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("open circuit: {e}")))?;
        Ok(())
    }

    /// Set circuit to half-open for testing.
    pub fn half_open_circuit(&self, provider: &str) -> Result<()> {
        self.conn
            .execute(
                r"UPDATE provider_health SET circuit_state = 'half_open', updated_at = ?1 WHERE provider = ?2",
                params![Utc::now().to_rfc3339(), provider],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("half-open circuit: {e}")))?;
        Ok(())
    }

    // ===== Usage Snapshots =====

    /// Insert a new usage snapshot linked to an account.
    ///
    /// Returns the row ID of the inserted snapshot.
    pub fn insert_snapshot(&self, snapshot: &NewUsageSnapshot) -> Result<i64> {
        self.conn
            .execute(
                r"INSERT INTO usage_snapshots (
                    account_id, provider, fetched_at, trigger_type, source,
                    primary_used_pct, primary_window_minutes, primary_resets_at,
                    secondary_used_pct, secondary_window_minutes, secondary_resets_at,
                    tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at,
                    cost_today_usd, cost_mtd_usd, credits_remaining,
                    account_email, account_org, fetch_duration_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
                params![
                    snapshot.account_id,
                    snapshot.provider,
                    snapshot.fetched_at.to_rfc3339(),
                    snapshot.trigger_type.as_str(),
                    snapshot.source,
                    snapshot.primary_used_pct,
                    snapshot.primary_window_minutes,
                    snapshot.primary_resets_at.map(|t| t.to_rfc3339()),
                    snapshot.secondary_used_pct,
                    snapshot.secondary_window_minutes,
                    snapshot.secondary_resets_at.map(|t| t.to_rfc3339()),
                    snapshot.tertiary_used_pct,
                    snapshot.tertiary_window_minutes,
                    snapshot.tertiary_resets_at.map(|t| t.to_rfc3339()),
                    snapshot.cost_today_usd,
                    snapshot.cost_mtd_usd,
                    snapshot.credits_remaining,
                    snapshot.account_email,
                    snapshot.account_org,
                    snapshot.fetch_duration_ms,
                ],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("insert snapshot: {e}")))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get the latest snapshot for an account.
    pub fn get_latest_snapshot(&self, account_id: &str) -> Result<Option<UsageSnapshotRecord>> {
        let result = self
            .conn
            .query_row(
                r"SELECT
                    id, account_id, provider, fetched_at, trigger_type, source,
                    primary_used_pct, primary_window_minutes, primary_resets_at,
                    secondary_used_pct, secondary_window_minutes, secondary_resets_at,
                    tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at,
                    cost_today_usd, cost_mtd_usd, credits_remaining,
                    account_email, account_org, fetch_duration_ms, created_at
                FROM usage_snapshots
                WHERE account_id = ?1
                ORDER BY fetched_at DESC
                LIMIT 1",
                [account_id],
                map_snapshot_row,
            )
            .optional()
            .map_err(|e| CautError::Other(anyhow::anyhow!("get latest snapshot: {e}")))?;

        Ok(result)
    }

    /// Get the latest snapshot for each account of a provider.
    pub fn get_latest_snapshots_by_provider(&self, provider: &str) -> Result<Vec<UsageSnapshotRecord>> {
        let mut snapshots = Vec::new();

        // Use a subquery with both MAX(fetched_at) and MAX(id) to handle ties
        // in fetched_at timestamps (picks the row with highest id in case of tie)
        let mut stmt = self
            .conn
            .prepare(
                r"SELECT
                    s.id, s.account_id, s.provider, s.fetched_at, s.trigger_type, s.source,
                    s.primary_used_pct, s.primary_window_minutes, s.primary_resets_at,
                    s.secondary_used_pct, s.secondary_window_minutes, s.secondary_resets_at,
                    s.tertiary_used_pct, s.tertiary_window_minutes, s.tertiary_resets_at,
                    s.cost_today_usd, s.cost_mtd_usd, s.credits_remaining,
                    s.account_email, s.account_org, s.fetch_duration_ms, s.created_at
                FROM usage_snapshots s
                WHERE s.provider = ?1
                  AND s.account_id IS NOT NULL
                  AND s.id = (
                      SELECT id FROM usage_snapshots s2
                      WHERE s2.account_id = s.account_id AND s2.provider = ?1
                      ORDER BY s2.fetched_at DESC, s2.id DESC
                      LIMIT 1
                  )",
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare latest by provider: {e}")))?;

        let rows = stmt
            .query_map([provider], map_snapshot_row)
            .map_err(|e| CautError::Other(anyhow::anyhow!("query latest by provider: {e}")))?;

        for row in rows {
            snapshots.push(row.map_err(|e| CautError::Other(anyhow::anyhow!("map snapshot row: {e}")))?);
        }

        Ok(snapshots)
    }

    /// Get snapshots for an account in a time range.
    pub fn get_snapshots_in_range(
        &self,
        account_id: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<UsageSnapshotRecord>> {
        if from > to {
            return Err(CautError::Config(
                "Time range start must be before end".to_string(),
            ));
        }

        let mut snapshots = Vec::new();

        let mut stmt = self
            .conn
            .prepare(
                r"SELECT
                    id, account_id, provider, fetched_at, trigger_type, source,
                    primary_used_pct, primary_window_minutes, primary_resets_at,
                    secondary_used_pct, secondary_window_minutes, secondary_resets_at,
                    tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at,
                    cost_today_usd, cost_mtd_usd, credits_remaining,
                    account_email, account_org, fetch_duration_ms, created_at
                FROM usage_snapshots
                WHERE account_id = ?1 AND fetched_at BETWEEN ?2 AND ?3
                ORDER BY fetched_at DESC",
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare snapshots in range: {e}")))?;

        let rows = stmt
            .query_map(
                params![account_id, from.to_rfc3339(), to.to_rfc3339()],
                map_snapshot_row,
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("query snapshots in range: {e}")))?;

        for row in rows {
            snapshots.push(row.map_err(|e| CautError::Other(anyhow::anyhow!("map snapshot row: {e}")))?);
        }

        Ok(snapshots)
    }

    /// Get all snapshots for an account (most recent first, with limit).
    pub fn get_account_snapshots(&self, account_id: &str, limit: i64) -> Result<Vec<UsageSnapshotRecord>> {
        let mut snapshots = Vec::new();

        let mut stmt = self
            .conn
            .prepare(
                r"SELECT
                    id, account_id, provider, fetched_at, trigger_type, source,
                    primary_used_pct, primary_window_minutes, primary_resets_at,
                    secondary_used_pct, secondary_window_minutes, secondary_resets_at,
                    tertiary_used_pct, tertiary_window_minutes, tertiary_resets_at,
                    cost_today_usd, cost_mtd_usd, credits_remaining,
                    account_email, account_org, fetch_duration_ms, created_at
                FROM usage_snapshots
                WHERE account_id = ?1
                ORDER BY fetched_at DESC
                LIMIT ?2",
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("prepare account snapshots: {e}")))?;

        let rows = stmt
            .query_map(params![account_id, limit], map_snapshot_row)
            .map_err(|e| CautError::Other(anyhow::anyhow!("query account snapshots: {e}")))?;

        for row in rows {
            snapshots.push(row.map_err(|e| CautError::Other(anyhow::anyhow!("map snapshot row: {e}")))?);
        }

        Ok(snapshots)
    }

    /// Delete snapshots older than a cutoff date for an account.
    ///
    /// Returns the number of rows deleted.
    pub fn cleanup_account_snapshots(&self, account_id: &str, retention_days: i64) -> Result<usize> {
        if retention_days <= 0 {
            return Err(CautError::Config(
                "Retention days must be greater than 0".to_string(),
            ));
        }

        let cutoff = Utc::now() - Duration::days(retention_days);

        let deleted = self
            .conn
            .execute(
                "DELETE FROM usage_snapshots WHERE account_id = ?1 AND fetched_at < ?2",
                params![account_id, cutoff.to_rfc3339()],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("cleanup account snapshots: {e}")))?;

        Ok(deleted)
    }

    /// Delete all snapshots for an account.
    ///
    /// Returns the number of rows deleted.
    pub fn delete_account_snapshots(&self, account_id: &str) -> Result<usize> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM usage_snapshots WHERE account_id = ?1",
                [account_id],
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("delete account snapshots: {e}")))?;

        Ok(deleted)
    }

    /// Count snapshots for an account.
    pub fn count_account_snapshots(&self, account_id: &str) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_snapshots WHERE account_id = ?1",
                [account_id],
                |row| row.get(0),
            )
            .map_err(|e| CautError::Other(anyhow::anyhow!("count account snapshots: {e}")))?;

        Ok(count)
    }
}

/// Map a database row to a `UsageSnapshotRecord`.
fn map_snapshot_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSnapshotRecord> {
    Ok(UsageSnapshotRecord {
        id: row.get(0)?,
        account_id: row.get(1)?,
        provider: row.get(2)?,
        fetched_at: parse_datetime(row.get::<_, String>(3)?),
        trigger_type: SnapshotTrigger::parse(&row.get::<_, String>(4)?),
        source: row.get(5)?,
        primary_used_pct: row.get(6)?,
        primary_window_minutes: row.get(7)?,
        primary_resets_at: row.get::<_, Option<String>>(8)?.map(parse_datetime),
        secondary_used_pct: row.get(9)?,
        secondary_window_minutes: row.get(10)?,
        secondary_resets_at: row.get::<_, Option<String>>(11)?.map(parse_datetime),
        tertiary_used_pct: row.get(12)?,
        tertiary_window_minutes: row.get(13)?,
        tertiary_resets_at: row.get::<_, Option<String>>(14)?.map(parse_datetime),
        cost_today_usd: row.get(15)?,
        cost_mtd_usd: row.get(16)?,
        credits_remaining: row.get(17)?,
        account_email: row.get(18)?,
        account_org: row.get(19)?,
        fetch_duration_ms: row.get(20)?,
        created_at: row.get::<_, Option<String>>(21)?.map(parse_datetime),
    })
}

/// Generate a UUID v4 (pseudo-random, not cryptographically secure).
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = duration.as_nanos();
    let pid = u64::from(std::process::id());
    // Use wrapping operations to avoid overflow
    let random: u64 = (nanos as u64).wrapping_mul(pid).wrapping_add(nanos as u64);
    let time_high = (nanos >> 32) as u32;
    let time_mid = ((nanos >> 16) & 0xFFFF) as u16;
    format!(
        "{time_high:08x}-{time_mid:04x}-4{:03x}-{:04x}-{:012x}",
        random as u16 & 0x0FFF,
        ((random >> 16) as u16 & 0x3FFF) | 0x8000,
        random & 0xFFFF_FFFF_FFFF
    )
}

/// Parse ISO8601 datetime string.
fn parse_datetime(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::history_schema::run_migrations;

    fn open_test_db() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn test_account_crud() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Create account
        let account = Account::new("claude", "test@example.com").with_label("Test Account");
        db.insert_account(&account).expect("insert account");

        // Get by ID
        let fetched = db.get_account(&account.id).expect("get account");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.email, "test@example.com");
        assert_eq!(fetched.label, Some("Test Account".to_string()));

        // Find by provider/email
        let found = db
            .find_account("claude", "test@example.com")
            .expect("find account");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, account.id);

        // List accounts
        let accounts = db.list_accounts(Some("claude")).expect("list accounts");
        assert_eq!(accounts.len(), 1);

        // Touch account
        db.touch_account(&account.id).expect("touch account");
        let updated = db.get_account(&account.id).expect("get updated");
        assert!(updated.unwrap().last_seen_at.is_some());

        // Deactivate
        db.deactivate_account(&account.id)
            .expect("deactivate account");
        let inactive = db.list_accounts(Some("claude")).expect("list after deactivate");
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_upsert_account_creates_new() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Upsert should create new account
        let account = Account::new("claude", "new@example.com").with_label("New Account");
        let id = db.upsert_account(&account).expect("upsert new");
        assert_eq!(id, account.id);

        // Verify it was created
        let fetched = db.get_account(&id).expect("get account");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.email, "new@example.com");
        assert_eq!(fetched.label, Some("New Account".to_string()));
    }

    #[test]
    fn test_upsert_account_updates_existing() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Create initial account
        let initial = Account::new("codex", "user@example.com").with_label("Initial Label");
        db.insert_account(&initial).expect("insert initial");

        // Upsert with new label and credential hash
        let updated = Account::new("codex", "user@example.com")
            .with_label("Updated Label")
            .with_credential_hash("new-hash-123");

        let id = db.upsert_account(&updated).expect("upsert existing");

        // Should return the original ID, not create a new one
        assert_eq!(id, initial.id);

        // Verify updates were applied
        let fetched = db.get_account(&id).expect("get account");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.label, Some("Updated Label".to_string()));
        assert_eq!(fetched.credential_hash, Some("new-hash-123".to_string()));
        assert!(fetched.is_active); // Should be active after upsert
    }

    #[test]
    fn test_upsert_reactivates_deactivated() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Create and deactivate
        let account = Account::new("gemini", "test@example.com");
        db.insert_account(&account).expect("insert");
        db.deactivate_account(&account.id).expect("deactivate");

        // Verify deactivated
        let fetched = db.get_account(&account.id).expect("get");
        assert!(!fetched.unwrap().is_active);

        // Upsert should reactivate
        let new_data = Account::new("gemini", "test@example.com");
        db.upsert_account(&new_data).expect("upsert");

        // Verify reactivated
        let fetched = db.get_account(&account.id).expect("get after upsert");
        assert!(fetched.unwrap().is_active);
    }

    #[test]
    fn test_update_label() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("claude", "test@example.com");
        db.insert_account(&account).expect("insert");

        // Set label
        db.update_label(&account.id, Some("My Work Account"))
            .expect("set label");
        let fetched = db.get_account(&account.id).expect("get");
        assert_eq!(fetched.unwrap().label, Some("My Work Account".to_string()));

        // Clear label
        db.update_label(&account.id, None).expect("clear label");
        let fetched = db.get_account(&account.id).expect("get after clear");
        assert_eq!(fetched.unwrap().label, None);
    }

    #[test]
    fn test_update_metadata() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("codex", "test@example.com");
        db.insert_account(&account).expect("insert");

        // Set metadata
        let metadata = r#"{"tier": "pro", "org_id": "org-123"}"#;
        db.update_metadata(&account.id, Some(metadata))
            .expect("set metadata");
        let fetched = db.get_account(&account.id).expect("get");
        assert_eq!(fetched.unwrap().metadata, Some(metadata.to_string()));
    }

    #[test]
    fn test_reactivate_account() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("claude", "test@example.com");
        db.insert_account(&account).expect("insert");

        // Deactivate
        db.deactivate_account(&account.id).expect("deactivate");
        let count_active = db.count_accounts(Some("claude")).expect("count active");
        assert_eq!(count_active, 0);

        // Reactivate
        db.reactivate_account(&account.id).expect("reactivate");
        let count_active = db.count_accounts(Some("claude")).expect("count after reactivate");
        assert_eq!(count_active, 1);

        let fetched = db.get_account(&account.id).expect("get");
        assert!(fetched.unwrap().is_active);
    }

    #[test]
    fn test_count_accounts() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Empty initially
        assert_eq!(db.count_accounts(None).expect("count all"), 0);

        // Add accounts for different providers
        db.insert_account(&Account::new("claude", "user1@example.com"))
            .expect("insert");
        db.insert_account(&Account::new("claude", "user2@example.com"))
            .expect("insert");
        db.insert_account(&Account::new("codex", "user3@example.com"))
            .expect("insert");

        // Count all
        assert_eq!(db.count_accounts(None).expect("count all"), 3);

        // Count by provider
        assert_eq!(db.count_accounts(Some("claude")).expect("count claude"), 2);
        assert_eq!(db.count_accounts(Some("codex")).expect("count codex"), 1);
        assert_eq!(db.count_accounts(Some("gemini")).expect("count gemini"), 0);
    }

    #[test]
    fn test_list_all_accounts_includes_inactive() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let active = Account::new("claude", "active@example.com");
        let inactive = Account::new("claude", "inactive@example.com");
        db.insert_account(&active).expect("insert active");
        db.insert_account(&inactive).expect("insert inactive");
        db.deactivate_account(&inactive.id).expect("deactivate");

        // list_accounts excludes inactive
        let active_only = db.list_accounts(Some("claude")).expect("list active");
        assert_eq!(active_only.len(), 1);

        // list_all_accounts includes inactive
        let all = db.list_all_accounts(Some("claude")).expect("list all");
        assert_eq!(all.len(), 2);

        // Verify one is inactive
        let inactive_account = all.iter().find(|a| a.id == inactive.id);
        assert!(inactive_account.is_some());
        assert!(!inactive_account.unwrap().is_active);
    }

    #[test]
    fn test_switch_log() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Create accounts
        let from_account = Account::new("claude", "old@example.com");
        let to_account = Account::new("claude", "new@example.com");
        db.insert_account(&from_account).expect("insert from");
        db.insert_account(&to_account).expect("insert to");

        // Log switch
        let id = db
            .log_switch(
                "claude",
                Some(&from_account.id),
                &to_account.id,
                SwitchTrigger::Threshold,
                Some(r#"{"threshold": 90}"#),
                true,
                None,
            )
            .expect("log switch");
        assert!(id > 0);

        // Get log
        let log = db.get_switch_log(10).expect("get switch log");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].provider, "claude");
        assert_eq!(log[0].trigger_type, "threshold");
        assert!(log[0].success);
    }

    #[test]
    fn test_provider_health() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Initial state
        let health = db.get_provider_health("claude").expect("get health");
        assert_eq!(health.consecutive_failures, 0);
        assert_eq!(health.circuit_state, CircuitState::Closed);

        // Record success
        db.record_success("claude", 150).expect("record success");
        let health = db.get_provider_health("claude").expect("get health");
        assert_eq!(health.total_requests, 1);
        assert!(health.last_success.is_some());

        // Record failures
        db.record_failure("claude").expect("record failure 1");
        db.record_failure("claude").expect("record failure 2");
        let health = db.get_provider_health("claude").expect("get health");
        assert_eq!(health.consecutive_failures, 2);
        assert_eq!(health.total_failures, 2);

        // Open circuit
        db.open_circuit("claude").expect("open circuit");
        let health = db.get_provider_health("claude").expect("get health");
        assert_eq!(health.circuit_state, CircuitState::Open);
        assert!(health.opened_at.is_some());

        // Half-open
        db.half_open_circuit("claude").expect("half-open");
        let health = db.get_provider_health("claude").expect("get health");
        assert_eq!(health.circuit_state, CircuitState::HalfOpen);

        // Success closes circuit
        db.record_success("claude", 100).expect("record success");
        let health = db.get_provider_health("claude").expect("get health");
        assert_eq!(health.circuit_state, CircuitState::Closed);
        assert_eq!(health.consecutive_failures, 0);
    }

    #[test]
    fn test_uuid_generation() {
        let id1 = uuid_v4();
        let id2 = uuid_v4();
        assert_ne!(id1, id2);
        assert!(id1.contains('-'));
        assert_eq!(id1.len(), 36);
    }

    // ===== Snapshot Tests =====

    #[test]
    fn test_insert_and_get_snapshot() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Create an account first
        let account = Account::new("claude", "test@example.com");
        db.insert_account(&account).expect("insert account");

        // Insert a snapshot
        let snapshot = NewUsageSnapshot::new("claude")
            .with_account(&account.id)
            .with_trigger(SnapshotTrigger::Switch)
            .with_source("cli")
            .with_primary(42.5, Some(180), Some(Utc::now() + Duration::minutes(30)))
            .with_identity(Some("test@example.com"), Some("test-org"));

        let id = db.insert_snapshot(&snapshot).expect("insert snapshot");
        assert!(id > 0);

        // Get latest snapshot
        let latest = db.get_latest_snapshot(&account.id).expect("get latest");
        assert!(latest.is_some());
        let latest = latest.unwrap();
        assert_eq!(latest.account_id, Some(account.id.clone()));
        assert_eq!(latest.provider, "claude");
        assert_eq!(latest.trigger_type, SnapshotTrigger::Switch);
        assert_eq!(latest.primary_used_pct, Some(42.5));
        assert_eq!(latest.primary_window_minutes, Some(180));
        assert_eq!(latest.account_email, Some("test@example.com".to_string()));
        assert_eq!(latest.account_org, Some("test-org".to_string()));
    }

    #[test]
    fn test_get_latest_snapshots_by_provider() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        // Create two accounts
        let account1 = Account::new("claude", "user1@example.com");
        let account2 = Account::new("claude", "user2@example.com");
        db.insert_account(&account1).expect("insert account1");
        db.insert_account(&account2).expect("insert account2");

        // Insert snapshots for account1 (older then newer)
        let old_snapshot1 = NewUsageSnapshot {
            account_id: Some(account1.id.clone()),
            provider: "claude".to_string(),
            fetched_at: Utc::now() - Duration::hours(2),
            trigger_type: SnapshotTrigger::Periodic,
            source: "cli".to_string(),
            primary_used_pct: Some(10.0),
            ..Default::default()
        };
        db.insert_snapshot(&old_snapshot1).expect("insert old1");

        let new_snapshot1 = NewUsageSnapshot {
            account_id: Some(account1.id.clone()),
            provider: "claude".to_string(),
            fetched_at: Utc::now() - Duration::hours(1),
            trigger_type: SnapshotTrigger::Manual,
            source: "cli".to_string(),
            primary_used_pct: Some(20.0),
            ..Default::default()
        };
        db.insert_snapshot(&new_snapshot1).expect("insert new1");

        // Insert snapshot for account2
        let snapshot2 = NewUsageSnapshot {
            account_id: Some(account2.id.clone()),
            provider: "claude".to_string(),
            fetched_at: Utc::now(),
            trigger_type: SnapshotTrigger::Switch,
            source: "web".to_string(),
            primary_used_pct: Some(30.0),
            ..Default::default()
        };
        db.insert_snapshot(&snapshot2).expect("insert snapshot2");

        // Get latest snapshots by provider
        let latest = db
            .get_latest_snapshots_by_provider("claude")
            .expect("get latest by provider");
        assert_eq!(latest.len(), 2);

        // Find each account's latest
        let acc1_latest = latest.iter().find(|s| s.account_id == Some(account1.id.clone()));
        let acc2_latest = latest.iter().find(|s| s.account_id == Some(account2.id.clone()));

        assert!(acc1_latest.is_some());
        assert!(acc2_latest.is_some());
        assert_eq!(acc1_latest.unwrap().primary_used_pct, Some(20.0)); // Newer one
        assert_eq!(acc2_latest.unwrap().primary_used_pct, Some(30.0));
    }

    #[test]
    fn test_get_snapshots_in_range() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("codex", "test@example.com");
        db.insert_account(&account).expect("insert account");

        let now = Utc::now();

        // Insert snapshots at different times
        for hours_ago in [24, 12, 6, 1] {
            let snapshot = NewUsageSnapshot {
                account_id: Some(account.id.clone()),
                provider: "codex".to_string(),
                fetched_at: now - Duration::hours(hours_ago),
                trigger_type: SnapshotTrigger::Periodic,
                source: "cli".to_string(),
                primary_used_pct: Some(hours_ago as f64),
                ..Default::default()
            };
            db.insert_snapshot(&snapshot).expect("insert snapshot");
        }

        // Query last 8 hours
        let from = now - Duration::hours(8);
        let to = now;
        let snapshots = db
            .get_snapshots_in_range(&account.id, from, to)
            .expect("get in range");

        assert_eq!(snapshots.len(), 2); // 6 hours and 1 hour ago
        // Should be ordered DESC by time
        assert_eq!(snapshots[0].primary_used_pct, Some(1.0)); // Most recent first
        assert_eq!(snapshots[1].primary_used_pct, Some(6.0));
    }

    #[test]
    fn test_get_snapshots_in_range_invalid() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let now = Utc::now();
        let err = db
            .get_snapshots_in_range("test-id", now, now - Duration::hours(1))
            .expect_err("should fail");

        assert!(matches!(err, CautError::Config(_)));
    }

    #[test]
    fn test_get_account_snapshots_with_limit() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("gemini", "test@example.com");
        db.insert_account(&account).expect("insert account");

        // Insert 10 snapshots
        for i in 0..10 {
            let snapshot = NewUsageSnapshot {
                account_id: Some(account.id.clone()),
                provider: "gemini".to_string(),
                fetched_at: Utc::now() - Duration::hours(10 - i),
                trigger_type: SnapshotTrigger::Periodic,
                source: "cli".to_string(),
                primary_used_pct: Some(i as f64 * 10.0),
                ..Default::default()
            };
            db.insert_snapshot(&snapshot).expect("insert snapshot");
        }

        // Get with limit
        let snapshots = db.get_account_snapshots(&account.id, 5).expect("get with limit");
        assert_eq!(snapshots.len(), 5);
        // Should be most recent first
        assert_eq!(snapshots[0].primary_used_pct, Some(90.0));
        assert_eq!(snapshots[4].primary_used_pct, Some(50.0));
    }

    #[test]
    fn test_cleanup_account_snapshots() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("claude", "test@example.com");
        db.insert_account(&account).expect("insert account");

        let now = Utc::now();

        // Insert old snapshot (35 days ago)
        let old_snapshot = NewUsageSnapshot {
            account_id: Some(account.id.clone()),
            provider: "claude".to_string(),
            fetched_at: now - Duration::days(35),
            trigger_type: SnapshotTrigger::Manual,
            source: "cli".to_string(),
            primary_used_pct: Some(10.0),
            ..Default::default()
        };
        db.insert_snapshot(&old_snapshot).expect("insert old");

        // Insert recent snapshot (5 days ago)
        let recent_snapshot = NewUsageSnapshot {
            account_id: Some(account.id.clone()),
            provider: "claude".to_string(),
            fetched_at: now - Duration::days(5),
            trigger_type: SnapshotTrigger::Manual,
            source: "cli".to_string(),
            primary_used_pct: Some(20.0),
            ..Default::default()
        };
        db.insert_snapshot(&recent_snapshot).expect("insert recent");

        // Count before cleanup
        let count_before = db.count_account_snapshots(&account.id).expect("count before");
        assert_eq!(count_before, 2);

        // Cleanup with 30-day retention
        let deleted = db
            .cleanup_account_snapshots(&account.id, 30)
            .expect("cleanup");
        assert_eq!(deleted, 1);

        // Count after cleanup
        let count_after = db.count_account_snapshots(&account.id).expect("count after");
        assert_eq!(count_after, 1);

        // Verify remaining snapshot is the recent one
        let latest = db.get_latest_snapshot(&account.id).expect("get latest");
        assert_eq!(latest.unwrap().primary_used_pct, Some(20.0));
    }

    #[test]
    fn test_cleanup_rejects_invalid_retention() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let err = db
            .cleanup_account_snapshots("test-id", 0)
            .expect_err("should fail");
        assert!(matches!(err, CautError::Config(_)));

        let err = db
            .cleanup_account_snapshots("test-id", -5)
            .expect_err("should fail");
        assert!(matches!(err, CautError::Config(_)));
    }

    #[test]
    fn test_delete_account_snapshots() {
        let conn = open_test_db();
        let db = MultiAccountDb::new(&conn);

        let account = Account::new("claude", "test@example.com");
        db.insert_account(&account).expect("insert account");

        // Insert multiple snapshots
        for i in 0..5 {
            let snapshot = NewUsageSnapshot {
                account_id: Some(account.id.clone()),
                provider: "claude".to_string(),
                fetched_at: Utc::now() - Duration::hours(i),
                trigger_type: SnapshotTrigger::Periodic,
                source: "cli".to_string(),
                primary_used_pct: Some(i as f64 * 10.0),
                ..Default::default()
            };
            db.insert_snapshot(&snapshot).expect("insert snapshot");
        }

        // Verify count
        let count = db.count_account_snapshots(&account.id).expect("count");
        assert_eq!(count, 5);

        // Delete all
        let deleted = db.delete_account_snapshots(&account.id).expect("delete all");
        assert_eq!(deleted, 5);

        // Verify empty
        let count = db.count_account_snapshots(&account.id).expect("count after");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_snapshot_trigger_conversion() {
        assert_eq!(SnapshotTrigger::Manual.as_str(), "manual");
        assert_eq!(SnapshotTrigger::Switch.as_str(), "switch");
        assert_eq!(SnapshotTrigger::Periodic.as_str(), "periodic");

        assert_eq!(SnapshotTrigger::parse("manual"), SnapshotTrigger::Manual);
        assert_eq!(SnapshotTrigger::parse("switch"), SnapshotTrigger::Switch);
        assert_eq!(SnapshotTrigger::parse("periodic"), SnapshotTrigger::Periodic);
        assert_eq!(SnapshotTrigger::parse("unknown"), SnapshotTrigger::Manual);
    }

    #[test]
    fn test_new_usage_snapshot_builder() {
        let resets_at = Utc::now() + Duration::hours(1);
        let snapshot = NewUsageSnapshot::new("codex")
            .with_account("acc-123")
            .with_trigger(SnapshotTrigger::Switch)
            .with_source("oauth")
            .with_primary(75.5, Some(180), Some(resets_at))
            .with_secondary(30.0, Some(60), None)
            .with_identity(Some("user@test.com"), Some("org-name"));

        assert_eq!(snapshot.provider, "codex");
        assert_eq!(snapshot.account_id, Some("acc-123".to_string()));
        assert_eq!(snapshot.trigger_type, SnapshotTrigger::Switch);
        assert_eq!(snapshot.source, "oauth");
        assert_eq!(snapshot.primary_used_pct, Some(75.5));
        assert_eq!(snapshot.primary_window_minutes, Some(180));
        assert!(snapshot.primary_resets_at.is_some());
        assert_eq!(snapshot.secondary_used_pct, Some(30.0));
        assert_eq!(snapshot.secondary_window_minutes, Some(60));
        assert!(snapshot.secondary_resets_at.is_none());
        assert_eq!(snapshot.account_email, Some("user@test.com".to_string()));
        assert_eq!(snapshot.account_org, Some("org-name".to_string()));
    }
}
