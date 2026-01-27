-- Migration 003: Multi-account schema
--
-- Adds normalized account tracking, switch logging, and provider health
-- for the multi-account daemon monitoring system.

-- Enable WAL mode for better concurrency (daemon + CLI access)
PRAGMA journal_mode = WAL;

-- Accounts table: normalized account registry
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,              -- UUID
    provider TEXT NOT NULL,           -- claude, codex, gemini
    email TEXT NOT NULL,              -- Account identifier (email or ID)
    label TEXT,                       -- User-defined friendly label
    credential_hash TEXT,             -- Hash of credential content for change detection
    added_at TEXT NOT NULL,           -- ISO8601 timestamp when first seen
    last_seen_at TEXT,                -- ISO8601 timestamp of last activity
    is_active INTEGER DEFAULT 1,      -- 1=active, 0=disabled
    metadata TEXT,                    -- JSON blob for provider-specific data
    UNIQUE(provider, email)
);

CREATE INDEX IF NOT EXISTS idx_accounts_provider ON accounts(provider);
CREATE INDEX IF NOT EXISTS idx_accounts_active ON accounts(is_active) WHERE is_active = 1;

-- Add account_id to usage_snapshots for multi-account tracking
-- This references the accounts table but allows NULL for legacy snapshots
ALTER TABLE usage_snapshots ADD COLUMN account_id TEXT REFERENCES accounts(id);
ALTER TABLE usage_snapshots ADD COLUMN trigger_type TEXT DEFAULT 'manual';  -- switch, periodic, manual

CREATE INDEX IF NOT EXISTS idx_snapshots_account ON usage_snapshots(account_id);

-- Switch log: audit trail for account switches
CREATE TABLE IF NOT EXISTS switch_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,          -- ISO8601 timestamp
    provider TEXT NOT NULL,           -- claude, codex, gemini
    from_account_id TEXT REFERENCES accounts(id),  -- NULL if no previous account
    to_account_id TEXT NOT NULL REFERENCES accounts(id),
    trigger_type TEXT NOT NULL,       -- threshold, forecast, rate_limit, manual, schedule
    trigger_details TEXT,             -- JSON with trigger context (threshold %, forecast minutes, etc.)
    success INTEGER NOT NULL,         -- 1=success, 0=failure
    rollback INTEGER DEFAULT 0,       -- 1 if this was a rollback from failed switch
    error_message TEXT,               -- Error details if success=0
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_switch_time ON switch_log(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_switch_provider ON switch_log(provider);

-- Provider health: circuit breaker state and health metrics
CREATE TABLE IF NOT EXISTS provider_health (
    provider TEXT PRIMARY KEY,        -- claude, codex, gemini
    last_success TEXT,                -- ISO8601 timestamp of last successful fetch
    last_failure TEXT,                -- ISO8601 timestamp of last failed fetch
    consecutive_failures INTEGER DEFAULT 0,
    circuit_state TEXT DEFAULT 'closed',  -- closed, open, half_open
    opened_at TEXT,                   -- ISO8601 timestamp when circuit opened
    avg_latency_ms INTEGER,           -- Rolling average latency
    p95_latency_ms INTEGER,           -- 95th percentile latency
    total_requests INTEGER DEFAULT 0,
    total_failures INTEGER DEFAULT 0,
    updated_at TEXT DEFAULT (datetime('now'))
);

-- Notification history: track sent notifications to prevent spam
CREATE TABLE IF NOT EXISTS notification_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    notification_type TEXT NOT NULL,  -- threshold_70, threshold_85, threshold_95, forecast, anomaly
    sent_at TEXT NOT NULL,            -- ISO8601 timestamp
    channel TEXT NOT NULL,            -- desktop, webhook, terminal
    payload TEXT,                     -- JSON notification content
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_notification_account ON notification_history(account_id);
CREATE INDEX IF NOT EXISTS idx_notification_type_time ON notification_history(notification_type, sent_at DESC);
