-- Migration 001: usage snapshots schema
--
-- Core table for usage history snapshots. Stores time-series usage windows,
-- cost/credit fields, and identity metadata for efficient reporting.

CREATE TABLE IF NOT EXISTS usage_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    fetched_at TEXT NOT NULL,  -- ISO8601 timestamp
    source TEXT NOT NULL,      -- fetch source (oauth, cli, web, etc.)

    -- Primary rate window
    primary_used_pct REAL,
    primary_window_minutes INTEGER,
    primary_resets_at TEXT,

    -- Secondary rate window
    secondary_used_pct REAL,
    secondary_window_minutes INTEGER,
    secondary_resets_at TEXT,

    -- Tertiary rate window (if applicable)
    tertiary_used_pct REAL,
    tertiary_window_minutes INTEGER,
    tertiary_resets_at TEXT,

    -- Cost data
    cost_today_usd REAL,
    cost_mtd_usd REAL,

    -- Credits (for providers like Codex)
    credits_remaining REAL,

    -- Identity info (denormalized for query simplicity)
    account_email TEXT,
    account_org TEXT,

    -- Metadata
    fetch_duration_ms INTEGER,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_snapshots_provider_time
    ON usage_snapshots(provider, fetched_at DESC);

CREATE INDEX IF NOT EXISTS idx_snapshots_fetched_at
    ON usage_snapshots(fetched_at DESC);
