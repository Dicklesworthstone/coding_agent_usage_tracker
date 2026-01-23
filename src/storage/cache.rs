//! Caching utilities for web dashboard and cost data.
//!
//! This module provides high-performance caching optimized for shell prompt
//! integration where latency is critical (<50ms target, <10ms for reads).
//!
//! # Features
//! - Atomic writes using temp file + rename (prevents corruption)
//! - Non-blocking async writes for prompt performance
//! - Staleness tracking with configurable thresholds
//! - Graceful degradation on missing/corrupt cache

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::core::models::ProviderPayload;
use crate::error::Result;
use crate::storage::paths::AppPaths;

/// Staleness thresholds for cache data.
pub const STALENESS_FRESH_SECS: u64 = 300; // 5 minutes
pub const STALENESS_STALE_SECS: u64 = 1800; // 30 minutes
pub const STALENESS_VERY_STALE_SECS: u64 = 3600; // 1 hour

/// Default TTL for offline provider cache entries.
pub const DEFAULT_OFFLINE_TTL_SECS: u64 = 3600; // 1 hour
/// Default stale threshold multiplier for offline cache entries.
pub const DEFAULT_OFFLINE_STALE_MULTIPLIER: f64 = 2.0;
/// Default very stale threshold multiplier for offline cache entries.
pub const DEFAULT_OFFLINE_VERY_STALE_MULTIPLIER: f64 = 4.0;

/// Cache staleness level for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// Data is fresh (< 5 minutes old).
    Fresh,
    /// Data is somewhat stale (5-30 minutes old) - display with "~" prefix.
    Stale,
    /// Data is very stale (30+ minutes old) - display with "?" prefix.
    VeryStale,
    /// Cache is missing or expired beyond use.
    Missing,
}

impl Staleness {
    /// Get the display prefix for this staleness level.
    #[must_use]
    pub const fn prefix(&self) -> &'static str {
        match self {
            Self::Fresh => "",
            Self::Stale => "~",
            Self::VeryStale => "?",
            Self::Missing => "-",
        }
    }

    /// Check if the data is usable (Fresh, Stale, or VeryStale).
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        !matches!(self, Self::Missing)
    }

    /// Determine staleness from age in seconds.
    #[must_use]
    pub const fn from_age_secs(age_secs: u64) -> Self {
        if age_secs < STALENESS_FRESH_SECS {
            Self::Fresh
        } else if age_secs < STALENESS_STALE_SECS {
            Self::Stale
        } else if age_secs < STALENESS_VERY_STALE_SECS {
            Self::VeryStale
        } else {
            Self::Missing
        }
    }
}

/// Performance metrics for cache operations.
#[derive(Debug, Default)]
pub struct CacheMetrics {
    /// Number of cache reads.
    pub reads: AtomicU64,
    /// Number of cache writes.
    pub writes: AtomicU64,
    /// Total read time in microseconds.
    pub read_time_us: AtomicU64,
    /// Total write time in microseconds.
    pub write_time_us: AtomicU64,
}

impl CacheMetrics {
    /// Create new metrics tracker.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reads: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            read_time_us: AtomicU64::new(0),
            write_time_us: AtomicU64::new(0),
        }
    }

    /// Record a read operation.
    pub fn record_read(&self, duration: Duration) {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.read_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record a write operation.
    pub fn record_write(&self, duration: Duration) {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.write_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Get average read time in microseconds.
    #[must_use]
    pub fn avg_read_time_us(&self) -> u64 {
        let reads = self.reads.load(Ordering::Relaxed);
        if reads == 0 {
            return 0;
        }
        self.read_time_us.load(Ordering::Relaxed) / reads
    }

    /// Get average write time in microseconds.
    #[must_use]
    pub fn avg_write_time_us(&self) -> u64 {
        let writes = self.writes.load(Ordering::Relaxed);
        if writes == 0 {
            return 0;
        }
        self.write_time_us.load(Ordering::Relaxed) / writes
    }
}

/// Global cache metrics for monitoring.
pub static CACHE_METRICS: CacheMetrics = CacheMetrics::new();

/// Check if a cache file is fresh (exists and not expired).
pub fn is_fresh(path: &Path, max_age: Duration) -> bool {
    if !path.exists() {
        return false;
    }

    path.metadata()
        .and_then(|m| m.modified())
        .map(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .map(|age| age < max_age)
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Get the age of a cache file in seconds.
#[must_use]
pub fn get_age_secs(path: &Path) -> Option<u64> {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|d| d.as_secs())
}

/// Get the staleness level of a cache file.
#[must_use]
pub fn get_staleness(path: &Path) -> Staleness {
    get_age_secs(path).map_or(Staleness::Missing, Staleness::from_age_secs)
}

/// Read cached data if fresh.
/// Optimized for speed - reads synchronously and parses JSON.
pub fn read_if_fresh<T: DeserializeOwned>(path: &Path, max_age: Duration) -> Result<Option<T>> {
    if !is_fresh(path, max_age) {
        return Ok(None);
    }

    read_fast(path).map(Some)
}

/// Read cached data regardless of freshness.
/// Returns the data along with its staleness level.
pub fn read_with_staleness<T: DeserializeOwned>(path: &Path) -> Result<Option<(T, Staleness)>> {
    let staleness = get_staleness(path);
    if !staleness.is_usable() {
        return Ok(None);
    }

    let data = read_fast(path)?;
    Ok(Some((data, staleness)))
}

/// Fast read path optimized for shell prompt integration.
/// Target: <10ms reads.
fn read_fast<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let start = Instant::now();

    // Read file content - this is the hot path
    let content = std::fs::read_to_string(path)?;

    // Parse JSON
    let data: T = serde_json::from_str(&content)?;

    // Record metrics
    CACHE_METRICS.record_read(start.elapsed());

    Ok(data)
}

/// Write data to cache atomically.
/// Uses temp file + rename to prevent corruption.
pub fn write<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let start = Instant::now();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Serialize to JSON (compact for smaller file size)
    let content = serde_json::to_string(data)?;

    // Write atomically using temp file + rename
    write_atomic(path, content.as_bytes())?;

    // Record metrics
    CACHE_METRICS.record_write(start.elapsed());

    Ok(())
}

/// Write bytes atomically using temp file + rename.
/// This prevents corruption if the process is interrupted during write.
fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    // Create temp file in same directory (required for atomic rename)
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("cache"),
        std::process::id()
    ));

    // Write to temp file
    {
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?; // Ensure data is flushed to disk
    }

    // Atomic rename to final path
    std::fs::rename(&temp_path, path)?;

    Ok(())
}

/// Write data to cache asynchronously (non-blocking).
/// Spawns a background task to write the cache.
/// Returns immediately without waiting for write to complete.
pub fn write_async<T: Serialize + Send + 'static>(path: std::path::PathBuf, data: T) {
    // Spawn a background task to handle the write
    std::thread::spawn(move || {
        if let Err(e) = write(&path, &data) {
            // Log error but don't propagate - this is fire-and-forget
            tracing::warn!("Failed to write cache: {}", e);
        }
    });
}

/// Write data to cache asynchronously using tokio.
/// Spawns a tokio task to write the cache.
pub async fn write_async_tokio<T: Serialize + Send + Sync + 'static>(
    path: std::path::PathBuf,
    data: T,
) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = write(&path, &data) {
            tracing::warn!("Failed to write cache: {}", e);
        }
    });
}

// =============================================================================
// Offline cache for provider payloads
// =============================================================================

/// Source of cached data for offline usage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CacheSource {
    /// Cached from a live network fetch.
    NetworkFetch,
    /// Cached from CLI output.
    CliOutput,
    /// Cached from estimated/derived data.
    Estimated,
}

/// Cache entry for provider payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineCacheEntry {
    pub payload: ProviderPayload,
    pub cached_at: DateTime<Utc>,
    pub ttl_seconds: u64,
    pub source: CacheSource,
}

impl OfflineCacheEntry {
    /// Age of the cache entry.
    #[must_use]
    pub fn age(&self) -> Duration {
        let age = Utc::now() - self.cached_at;
        Duration::from_secs(age.num_seconds().max(0) as u64)
    }

    /// Whether the cache entry is still fresh (within TTL).
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        self.age().as_secs() <= self.ttl_seconds
    }

    /// Determine staleness using cache configuration thresholds.
    #[must_use]
    pub fn staleness(&self, config: &OfflineCacheConfig) -> CacheStaleness {
        let age = self.age();
        let age_secs = age.as_secs();
        let ttl_secs = self.ttl_seconds.max(1);

        let stale_limit = config.stale_threshold_secs(ttl_secs);
        let very_stale_limit = config.very_stale_threshold_secs(ttl_secs);

        if age_secs <= ttl_secs {
            CacheStaleness::Fresh { age }
        } else if age_secs <= stale_limit {
            CacheStaleness::Stale { age }
        } else if age_secs <= very_stale_limit {
            CacheStaleness::VeryStale { age }
        } else {
            CacheStaleness::VeryStale { age }
        }
    }
}

/// Staleness levels for offline cache entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStaleness {
    Fresh { age: Duration },
    Stale { age: Duration },
    VeryStale { age: Duration },
}

impl CacheStaleness {
    /// Get the age associated with this staleness.
    #[must_use]
    pub const fn age(self) -> Duration {
        match self {
            Self::Fresh { age } | Self::Stale { age } | Self::VeryStale { age } => age,
        }
    }
}

/// Configuration for offline caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OfflineCacheConfig {
    pub default_ttl_seconds: u64,
    pub stale_threshold_multiplier: f64,
    pub very_stale_threshold_multiplier: f64,
    pub provider_ttls: HashMap<String, u64>,
}

impl Default for OfflineCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl_seconds: DEFAULT_OFFLINE_TTL_SECS,
            stale_threshold_multiplier: DEFAULT_OFFLINE_STALE_MULTIPLIER,
            very_stale_threshold_multiplier: DEFAULT_OFFLINE_VERY_STALE_MULTIPLIER,
            provider_ttls: HashMap::new(),
        }
    }
}

impl OfflineCacheConfig {
    /// Get TTL for a provider (falls back to default).
    #[must_use]
    pub fn ttl_for(&self, provider: &str) -> Duration {
        let ttl = self
            .provider_ttls
            .get(provider)
            .copied()
            .unwrap_or(self.default_ttl_seconds)
            .max(1);
        Duration::from_secs(ttl)
    }

    /// Override TTL for a provider.
    #[must_use]
    pub fn with_provider_ttl(mut self, provider: &str, ttl_seconds: u64) -> Self {
        self.provider_ttls
            .insert(provider.to_string(), ttl_seconds.max(1));
        self
    }

    fn stale_threshold_secs(&self, ttl_secs: u64) -> u64 {
        ((ttl_secs as f64) * self.stale_threshold_multiplier.max(1.0)).round() as u64
    }

    fn very_stale_threshold_secs(&self, ttl_secs: u64) -> u64 {
        ((ttl_secs as f64) * self.very_stale_threshold_multiplier.max(1.0)).round() as u64
    }
}

/// Offline cache for provider payloads with TTL and staleness tracking.
pub struct OfflineCache {
    cache_dir: PathBuf,
    config: OfflineCacheConfig,
}

impl OfflineCache {
    /// Create a new offline cache with default configuration.
    #[must_use]
    pub fn new() -> Self {
        let cache_dir = AppPaths::new().cache.join("offline");
        Self::with_dir(cache_dir, OfflineCacheConfig::default())
    }

    /// Create a new offline cache with custom configuration.
    #[must_use]
    pub fn with_config(config: OfflineCacheConfig) -> Self {
        let cache_dir = AppPaths::new().cache.join("offline");
        Self::with_dir(cache_dir, config)
    }

    /// Create a cache using a specific directory (useful for tests).
    #[must_use]
    pub fn with_dir(cache_dir: PathBuf, config: OfflineCacheConfig) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create offline cache dir: {}", e);
        }
        Self { cache_dir, config }
    }

    /// Path for a provider cache entry.
    #[must_use]
    pub fn cache_path(&self, provider: &str) -> PathBuf {
        self.cache_dir.join(format!("{provider}.json"))
    }

    /// Read cached entry for a provider (gracefully returns None on missing/corrupt).
    pub fn get(&self, provider: &str) -> Option<OfflineCacheEntry> {
        let path = self.cache_path(provider);
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Read cached entry with computed staleness.
    pub fn get_with_staleness(
        &self,
        provider: &str,
    ) -> Option<(OfflineCacheEntry, CacheStaleness)> {
        let entry = self.get(provider)?;
        let staleness = entry.staleness(&self.config);
        Some((entry, staleness))
    }

    /// Write a provider payload to cache with default TTL and source.
    pub fn set(&self, provider: &str, payload: &ProviderPayload) -> Result<OfflineCacheEntry> {
        self.set_with_source(provider, payload, CacheSource::NetworkFetch)
    }

    /// Write a provider payload to cache with a custom source.
    pub fn set_with_source(
        &self,
        provider: &str,
        payload: &ProviderPayload,
        source: CacheSource,
    ) -> Result<OfflineCacheEntry> {
        let ttl = self.config.ttl_for(provider);
        let entry = OfflineCacheEntry {
            payload: payload.clone(),
            cached_at: Utc::now(),
            ttl_seconds: ttl.as_secs(),
            source,
        };
        self.write_entry(provider, &entry)?;
        Ok(entry)
    }

    /// Write a pre-built cache entry (useful for tests).
    pub fn write_entry(&self, provider: &str, entry: &OfflineCacheEntry) -> Result<()> {
        let path = self.cache_path(provider);
        write(&path, entry)
    }

    /// List cached providers (by CLI name).
    #[must_use]
    pub fn list_cached(&self) -> Vec<String> {
        let mut entries = std::fs::read_dir(&self.cache_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension() == Some("json".as_ref()))
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    /// Clear cache for a provider.
    pub fn clear(&self, provider: &str) -> Result<()> {
        let path = self.cache_path(provider);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Clear all cache entries.
    pub fn clear_all(&self) -> Result<()> {
        for provider in self.list_cached() {
            self.clear(&provider)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use serde::{Deserialize, Serialize};
    use std::thread;
    use tempfile::TempDir;

    use crate::test_utils::{make_test_provider_payload, make_test_provider_payload_minimal};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestData {
        value: String,
        count: i32,
    }

    #[test]
    fn test_staleness_from_age() {
        assert_eq!(Staleness::from_age_secs(0), Staleness::Fresh);
        assert_eq!(Staleness::from_age_secs(60), Staleness::Fresh);
        assert_eq!(Staleness::from_age_secs(299), Staleness::Fresh);
        assert_eq!(Staleness::from_age_secs(300), Staleness::Stale);
        assert_eq!(Staleness::from_age_secs(600), Staleness::Stale);
        assert_eq!(Staleness::from_age_secs(1800), Staleness::VeryStale);
        assert_eq!(Staleness::from_age_secs(3600), Staleness::Missing);
    }

    #[test]
    fn test_staleness_prefix() {
        assert_eq!(Staleness::Fresh.prefix(), "");
        assert_eq!(Staleness::Stale.prefix(), "~");
        assert_eq!(Staleness::VeryStale.prefix(), "?");
        assert_eq!(Staleness::Missing.prefix(), "-");
    }

    #[test]
    fn test_staleness_is_usable() {
        assert!(Staleness::Fresh.is_usable());
        assert!(Staleness::Stale.is_usable());
        assert!(Staleness::VeryStale.is_usable());
        assert!(!Staleness::Missing.is_usable());
    }

    #[test]
    fn test_write_and_read() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("test.json");

        let data = TestData {
            value: "hello".to_string(),
            count: 42,
        };

        // Write
        write(&cache_path, &data).unwrap();
        assert!(cache_path.exists());

        // Read back
        let read_data: TestData = read_fast(&cache_path).unwrap();
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_atomic_write_creates_file() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("atomic.json");

        write_atomic(&cache_path, b"test content").unwrap();
        assert!(cache_path.exists());

        let content = std::fs::read_to_string(&cache_path).unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_atomic_write_no_temp_file_left() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("atomic.json");

        write_atomic(&cache_path, b"test").unwrap();

        // No temp files should remain
        let entries: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].as_ref().unwrap().file_name() == "atomic.json");
    }

    #[test]
    fn test_is_fresh_with_fresh_file() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("fresh.json");

        std::fs::write(&cache_path, "test").unwrap();

        assert!(is_fresh(&cache_path, Duration::from_secs(60)));
    }

    #[test]
    fn test_is_fresh_with_missing_file() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("missing.json");

        assert!(!is_fresh(&cache_path, Duration::from_secs(60)));
    }

    #[test]
    fn test_read_if_fresh_returns_none_for_missing() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("missing.json");

        let result: Result<Option<TestData>> = read_if_fresh(&cache_path, Duration::from_secs(60));
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_staleness_for_missing_file() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("missing.json");

        assert_eq!(get_staleness(&cache_path), Staleness::Missing);
    }

    #[test]
    fn test_get_staleness_for_fresh_file() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("fresh.json");

        std::fs::write(&cache_path, "test").unwrap();

        assert_eq!(get_staleness(&cache_path), Staleness::Fresh);
    }

    #[test]
    fn test_read_with_staleness() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("test.json");

        let data = TestData {
            value: "test".to_string(),
            count: 1,
        };
        write(&cache_path, &data).unwrap();

        let result: Option<(TestData, Staleness)> = read_with_staleness(&cache_path).unwrap();
        let (read_data, staleness) = result.unwrap();

        assert_eq!(read_data, data);
        assert_eq!(staleness, Staleness::Fresh);
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = CacheMetrics::new();

        metrics.record_read(Duration::from_micros(100));
        metrics.record_read(Duration::from_micros(200));

        assert_eq!(metrics.reads.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.avg_read_time_us(), 150);

        metrics.record_write(Duration::from_micros(500));
        assert_eq!(metrics.writes.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.avg_write_time_us(), 500);
    }

    #[test]
    fn test_write_async_completes() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("async.json");

        let data = TestData {
            value: "async".to_string(),
            count: 99,
        };

        write_async(cache_path.clone(), data.clone());

        // Wait a bit for async write to complete
        thread::sleep(Duration::from_millis(100));

        assert!(cache_path.exists());
        let read_data: TestData = read_fast(&cache_path).unwrap();
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_read_performance_under_10ms() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("perf.json");

        // Create a moderately sized cache file
        let data = TestData {
            value: "x".repeat(1000),
            count: 42,
        };
        write(&cache_path, &data).unwrap();

        // Measure read time
        let start = Instant::now();
        let _: TestData = read_fast(&cache_path).unwrap();
        let elapsed = start.elapsed();

        // Should be well under 10ms
        assert!(
            elapsed.as_millis() < 10,
            "Read took {}ms, expected <10ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn test_offline_cache_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("offline");
        let cache = OfflineCache::with_dir(cache_dir.clone(), OfflineCacheConfig::default());

        assert!(cache_dir.exists());

        let payload = make_test_provider_payload("codex", "cli");
        let entry = cache.set("codex", &payload).unwrap();

        assert_eq!(entry.payload.provider, "codex");
        assert_eq!(entry.ttl_seconds, DEFAULT_OFFLINE_TTL_SECS);

        let read_entry = cache.get("codex").unwrap();
        assert_eq!(read_entry.payload.provider, "codex");
        assert_eq!(read_entry.source, CacheSource::NetworkFetch);
    }

    #[test]
    fn test_offline_cache_provider_ttl_override() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("offline");
        let config = OfflineCacheConfig::default().with_provider_ttl("claude", 1800);
        let cache = OfflineCache::with_dir(cache_dir, config);

        let payload = make_test_provider_payload("claude", "oauth");
        let entry = cache.set("claude", &payload).unwrap();

        assert_eq!(entry.ttl_seconds, 1800);
    }

    #[test]
    fn test_offline_cache_staleness_levels() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("offline");
        let config = OfflineCacheConfig {
            default_ttl_seconds: 10,
            stale_threshold_multiplier: 2.0,
            very_stale_threshold_multiplier: 4.0,
            provider_ttls: HashMap::new(),
        };
        let cache = OfflineCache::with_dir(cache_dir, config);

        let payload = make_test_provider_payload_minimal("codex", "cli");

        let fresh_entry = OfflineCacheEntry {
            payload: payload.clone(),
            cached_at: Utc::now() - ChronoDuration::seconds(5),
            ttl_seconds: 10,
            source: CacheSource::NetworkFetch,
        };
        cache.write_entry("codex", &fresh_entry).unwrap();
        let (_, staleness) = cache.get_with_staleness("codex").unwrap();
        assert!(matches!(staleness, CacheStaleness::Fresh { .. }));

        let stale_entry = OfflineCacheEntry {
            payload: payload.clone(),
            cached_at: Utc::now() - ChronoDuration::seconds(15),
            ttl_seconds: 10,
            source: CacheSource::NetworkFetch,
        };
        cache.write_entry("codex", &stale_entry).unwrap();
        let (_, staleness) = cache.get_with_staleness("codex").unwrap();
        assert!(matches!(staleness, CacheStaleness::Stale { .. }));

        let very_stale_entry = OfflineCacheEntry {
            payload,
            cached_at: Utc::now() - ChronoDuration::seconds(45),
            ttl_seconds: 10,
            source: CacheSource::NetworkFetch,
        };
        cache.write_entry("codex", &very_stale_entry).unwrap();
        let (_, staleness) = cache.get_with_staleness("codex").unwrap();
        assert!(matches!(staleness, CacheStaleness::VeryStale { .. }));
    }

    #[test]
    fn test_offline_cache_list_and_clear() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("offline");
        let cache = OfflineCache::with_dir(cache_dir, OfflineCacheConfig::default());

        let codex = make_test_provider_payload_minimal("codex", "cli");
        let claude = make_test_provider_payload_minimal("claude", "oauth");

        cache.set("codex", &codex).unwrap();
        cache.set("claude", &claude).unwrap();

        let cached = cache.list_cached();
        assert_eq!(cached, vec!["claude".to_string(), "codex".to_string()]);

        cache.clear("codex").unwrap();
        assert!(cache.get("codex").is_none());

        cache.clear_all().unwrap();
        assert!(cache.list_cached().is_empty());
    }
}
