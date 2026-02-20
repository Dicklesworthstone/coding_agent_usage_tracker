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
        ProjectDirs::from("com", "steipete", "caut").map_or_else(
            || {
                // Fallback to home directory
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                Self {
                    config: home.join(".config/caut"),
                    cache: home.join(".cache/caut"),
                    data: home.join(".local/share/caut"),
                }
            },
            |proj_dirs| Self {
                config: proj_dirs.config_dir().to_path_buf(),
                cache: proj_dirs.cache_dir().to_path_buf(),
                data: proj_dirs.data_dir().to_path_buf(),
            },
        )
    }

    /// Path to token accounts file.
    #[must_use]
    pub fn token_accounts_file(&self) -> PathBuf {
        self.config.join("token-accounts.json")
    }

    /// Path to CodexBar-compatible token accounts file (macOS only).
    #[must_use]
    pub const fn codexbar_token_accounts_file() -> Option<PathBuf> {
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

    /// Path to `OpenAI` dashboard cache.
    #[must_use]
    pub fn openai_dashboard_cache(&self) -> PathBuf {
        self.cache.join("openai-dashboard.json")
    }

    /// Path to cost usage cache for a provider.
    #[must_use]
    pub fn cost_usage_cache(&self, provider: &str) -> PathBuf {
        self.cache.join(format!("cost-usage/{provider}-v1.json"))
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
    ///
    /// # Errors
    /// Returns an error if any directory cannot be created.
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

#[cfg(test)]
mod tests {
    use super::AppPaths;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use crate::test_utils::TestDir;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[allow(unsafe_code)]
    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
    }

    impl EnvGuard {
        #[allow(unsafe_code)]
        fn set(key: &'static str, value: &PathBuf) -> Self {
            let prior = std::env::var(key).ok();
            // SAFETY: Tests guard env mutation with a global mutex.
            unsafe { std::env::set_var(key, value) };
            Self { key, prior }
        }
    }

    impl Drop for EnvGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            match &self.prior {
                Some(value) => {
                    // SAFETY: Tests guard env mutation with a global mutex.
                    unsafe { std::env::set_var(self.key, value) };
                }
                None => {
                    // SAFETY: Tests guard env mutation with a global mutex.
                    unsafe { std::env::remove_var(self.key) };
                }
            }
        }
    }

    #[test]
    fn token_accounts_file_is_under_config_dir() {
        let paths = AppPaths::new();
        let token_path = paths.token_accounts_file();
        assert!(
            token_path.ends_with("token-accounts.json"),
            "token accounts path should end with token-accounts.json"
        );
        assert_eq!(
            token_path.parent(),
            Some(paths.config.as_path()),
            "token accounts file should live under config dir"
        );
    }

    #[test]
    fn cache_and_data_paths_are_rooted_in_expected_dirs() {
        let paths = AppPaths::new();

        let dashboard = paths.openai_dashboard_cache();
        assert!(dashboard.starts_with(&paths.cache));
        assert!(dashboard.ends_with("openai-dashboard.json"));

        let cost_cache = paths.cost_usage_cache("codex");
        assert!(cost_cache.starts_with(&paths.cache));
        let cost_suffix = PathBuf::from("cost-usage").join("codex-v1.json");
        assert!(cost_cache.ends_with(cost_suffix));

        let history_db = paths.history_db_file();
        assert!(history_db.starts_with(&paths.data));
        assert!(history_db.ends_with("usage-history.sqlite"));

        let prompt_cache = paths.prompt_cache_file();
        assert!(prompt_cache.starts_with(&paths.cache));
        assert!(prompt_cache.ends_with("prompt-cache.json"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn codexbar_token_accounts_path_is_mac_only() {
        let path = AppPaths::codexbar_token_accounts_file();
        assert!(
            path.is_some(),
            "expected CodexBar token accounts path on macOS"
        );
        let path = path.unwrap();
        assert!(
            path.ends_with("Library/Application Support/CodexBar/token-accounts.json"),
            "unexpected CodexBar token accounts path: {}",
            path.display()
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn codexbar_token_accounts_path_is_none_off_macos() {
        assert!(AppPaths::codexbar_token_accounts_file().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn app_paths_respect_xdg_overrides() {
        let _lock = env_lock().lock().expect("env lock poisoned");
        let dir = TestDir::new();

        let config_home = dir.path().join("xdg-config");
        let cache_home = dir.path().join("xdg-cache");
        let data_home = dir.path().join("xdg-data");
        std::fs::create_dir_all(&config_home).expect("config dir");
        std::fs::create_dir_all(&cache_home).expect("cache dir");
        std::fs::create_dir_all(&data_home).expect("data dir");

        let _guard_config = EnvGuard::set("XDG_CONFIG_HOME", &config_home);
        let _guard_cache = EnvGuard::set("XDG_CACHE_HOME", &cache_home);
        let _guard_data = EnvGuard::set("XDG_DATA_HOME", &data_home);

        let paths = AppPaths::new();
        assert_eq!(paths.config, config_home.join("caut"));
        assert_eq!(paths.cache, cache_home.join("caut"));
        assert_eq!(paths.data, data_home.join("caut"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ensure_dirs_creates_expected_tree() {
        let _lock = env_lock().lock().expect("env lock poisoned");
        let dir = TestDir::new();

        let config_home = dir.path().join("xdg-config");
        let cache_home = dir.path().join("xdg-cache");
        let data_home = dir.path().join("xdg-data");
        std::fs::create_dir_all(&config_home).expect("config dir");
        std::fs::create_dir_all(&cache_home).expect("cache dir");
        std::fs::create_dir_all(&data_home).expect("data dir");

        let _guard_config = EnvGuard::set("XDG_CONFIG_HOME", &config_home);
        let _guard_cache = EnvGuard::set("XDG_CACHE_HOME", &cache_home);
        let _guard_data = EnvGuard::set("XDG_DATA_HOME", &data_home);

        let paths = AppPaths::new();
        paths.ensure_dirs().expect("ensure dirs");

        assert!(paths.config.exists(), "config dir should exist");
        assert!(paths.cache.exists(), "cache dir should exist");
        assert!(paths.data.exists(), "data dir should exist");
        assert!(
            paths.cache.join("cost-usage").exists(),
            "cost-usage cache dir should exist"
        );
    }
}
