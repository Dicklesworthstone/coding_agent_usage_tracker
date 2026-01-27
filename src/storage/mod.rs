//! Storage for configuration, caches, and token accounts.

pub mod cache;
pub mod config;
pub mod history;
pub mod history_schema;
pub mod multi_account;
pub mod paths;
pub mod token_accounts;

pub use cache::{CacheSource, CacheStaleness, OfflineCache, OfflineCacheConfig, OfflineCacheEntry};
pub use config::{
    Config, ConfigSource, ConfigSources, ENV_CONFIG, ENV_FORMAT, ENV_NO_COLOR, ENV_NO_COLOR_STD,
    ENV_PRETTY, ENV_PROVIDERS, ENV_TIMEOUT, ENV_VERBOSE, ResolvedConfig,
};
pub use history::{
    DEFAULT_AGGREGATE_RETENTION_DAYS, DEFAULT_DETAILED_RETENTION_DAYS, DEFAULT_MAX_SIZE_BYTES,
    DEFAULT_PRUNE_INTERVAL_HOURS, HistoryStore, PruneResult, RetentionPolicy, StatsPeriod,
    StoredSnapshot, UsageStats,
};
pub use history_schema::{DEFAULT_RETENTION_DAYS, cleanup_old_snapshots, run_migrations};
pub use multi_account::{
    Account, CircuitState, MultiAccountDb, ProviderHealth, SwitchLogEntry, SwitchTrigger,
};
pub use paths::AppPaths;
pub use token_accounts::{TokenAccount, TokenAccountStore};
