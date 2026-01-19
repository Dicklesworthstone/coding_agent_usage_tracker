-- Migration 002: daily aggregates for retention
--
-- Stores summarized daily usage data for long-term retention.
-- Detailed snapshots are aggregated into this table before deletion.

CREATE TABLE IF NOT EXISTS daily_aggregates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    date TEXT NOT NULL,           -- YYYY-MM-DD format

    -- Aggregated primary window stats
    primary_avg_used_pct REAL,
    primary_max_used_pct REAL,
    primary_min_used_pct REAL,

    -- Aggregated secondary window stats
    secondary_avg_used_pct REAL,
    secondary_max_used_pct REAL,
    secondary_min_used_pct REAL,

    -- Aggregated tertiary window stats
    tertiary_avg_used_pct REAL,
    tertiary_max_used_pct REAL,
    tertiary_min_used_pct REAL,

    -- Cost aggregates
    total_cost_usd REAL,

    -- Sample metadata
    sample_count INTEGER NOT NULL DEFAULT 0,
    first_fetch TEXT,             -- ISO8601 timestamp of first sample
    last_fetch TEXT,              -- ISO8601 timestamp of last sample

    -- Identity (denormalized, from most recent sample)
    account_email TEXT,
    account_org TEXT,

    created_at TEXT DEFAULT (datetime('now')),

    -- Unique constraint: one aggregate per provider per day
    UNIQUE(provider, date)
);

CREATE INDEX IF NOT EXISTS idx_aggregates_provider_date
    ON daily_aggregates(provider, date DESC);

CREATE INDEX IF NOT EXISTS idx_aggregates_date
    ON daily_aggregates(date DESC);

-- Prune tracking table
CREATE TABLE IF NOT EXISTS prune_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pruned_at TEXT NOT NULL,           -- ISO8601 timestamp
    detailed_deleted INTEGER NOT NULL,  -- rows deleted from usage_snapshots
    aggregates_deleted INTEGER NOT NULL, -- rows deleted from daily_aggregates
    aggregates_created INTEGER NOT NULL, -- rows created in daily_aggregates
    bytes_freed INTEGER,                 -- approximate bytes freed
    duration_ms INTEGER,                 -- how long prune took
    dry_run INTEGER NOT NULL DEFAULT 0   -- 1 if this was a dry run
);
