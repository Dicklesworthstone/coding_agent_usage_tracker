//! Caching utilities for web dashboard and cost data.

use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Serialize, de::DeserializeOwned};

use crate::error::Result;

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

/// Read cached data if fresh.
pub fn read_if_fresh<T: DeserializeOwned>(path: &Path, max_age: Duration) -> Result<Option<T>> {
    if !is_fresh(path, max_age) {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)?;
    let data: T = serde_json::from_str(&content)?;
    Ok(Some(data))
}

/// Write data to cache.
pub fn write<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(data)?;
    std::fs::write(path, content)?;
    Ok(())
}
