//! Application paths for config, cache, and data.

use directories::ProjectDirs;
use std::path::PathBuf;

/// Application paths.
pub struct AppPaths {
    /// Configuration directory.
    pub config: PathBuf,
    /// Cache directory.
    pub cache: PathBuf,
    /// Data directory.
    pub data: PathBuf,
}

impl AppPaths {
    /// Create paths for the caut application.
    #[must_use]
    pub fn new() -> Self {
        if let Some(proj_dirs) = ProjectDirs::from("com", "steipete", "caut") {
            Self {
                config: proj_dirs.config_dir().to_path_buf(),
                cache: proj_dirs.cache_dir().to_path_buf(),
                data: proj_dirs.data_dir().to_path_buf(),
            }
        } else {
            // Fallback to home directory
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            Self {
                config: home.join(".config/caut"),
                cache: home.join(".cache/caut"),
                data: home.join(".local/share/caut"),
            }
        }
    }

    /// Path to token accounts file.
    #[must_use]
    pub fn token_accounts_file(&self) -> PathBuf {
        self.config.join("token-accounts.json")
    }

    /// Path to CodexBar-compatible token accounts file (macOS only).
    #[must_use]
    pub fn codexbar_token_accounts_file() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .map(|h| h.join("Library/Application Support/CodexBar/token-accounts.json"))
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    /// Path to OpenAI dashboard cache.
    #[must_use]
    pub fn openai_dashboard_cache(&self) -> PathBuf {
        self.cache.join("openai-dashboard.json")
    }

    /// Path to cost usage cache for a provider.
    #[must_use]
    pub fn cost_usage_cache(&self, provider: &str) -> PathBuf {
        self.cache.join(format!("cost-usage/{}-v1.json", provider))
    }

    /// Path to history database file.
    #[must_use]
    pub fn history_db_file(&self) -> PathBuf {
        self.data.join("usage-history.sqlite")
    }

    /// Path to shell prompt cache file.
    #[must_use]
    pub fn prompt_cache_file(&self) -> PathBuf {
        self.cache.join("prompt-cache.json")
    }

    /// Ensure all directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config)?;
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.data)?;
        std::fs::create_dir_all(self.cache.join("cost-usage"))?;
        Ok(())
    }
}

impl Default for AppPaths {
    fn default() -> Self {
        Self::new()
    }
}

/// Module-level function for accessing dirs crate.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
    }
}
