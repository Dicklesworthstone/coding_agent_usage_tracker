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

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::{Serialize, de::DeserializeOwned};

use crate::error::Result;

/// Staleness thresholds for cache data.
pub const STALENESS_FRESH_SECS: u64 = 300; // 5 minutes
pub const STALENESS_STALE_SECS: u64 = 1800; // 30 minutes
pub const STALENESS_VERY_STALE_SECS: u64 = 3600; // 1 hour

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
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("cache"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::thread;
    use tempfile::TempDir;

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

        let result: Result<Option<TestData>> =
            read_if_fresh(&cache_path, Duration::from_secs(60));
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
}
