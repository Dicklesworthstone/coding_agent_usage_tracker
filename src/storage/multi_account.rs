//! Multi-account storage layer.
//!
//! Provides types and database operations for multi-account tracking,
//! including account registry, switch logging, and provider health.

use chrono::{DateTime, Utc};
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
    pub fn from_str(s: &str) -> Self {
        match s {
            "open" => Self::Open,
            "half_open" => Self::HalfOpen,
            _ => Self::Closed,
        }
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
                        circuit_state: CircuitState::from_str(&row.get::<_, String>(4)?),
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
}
