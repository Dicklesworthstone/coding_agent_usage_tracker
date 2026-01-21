//! Configuration file loading and management.
//!
//! Loads configuration from:
//! - Linux/macOS: `~/.config/caut/config.toml`
//! - Windows: `%APPDATA%/caut/config.toml`
//!
//! ## Precedence
//!
//! Settings are resolved with the following precedence (highest first):
//! 1. CLI flags
//! 2. Environment variables
//! 3. Config file
//! 4. Built-in defaults
//!
//! ## Environment Variables
//!
//! - `CAUT_PROVIDERS`: Comma-separated provider list (e.g., "claude,codex")
//! - `CAUT_FORMAT`: Output format (human, json, md)
//! - `CAUT_TIMEOUT`: Default timeout in seconds
//! - `CAUT_NO_COLOR` or `NO_COLOR`: Disable colors (1, true, yes)
//! - `CAUT_VERBOSE`: Enable verbose output (1, true, yes)
//! - `CAUT_PRETTY`: Pretty-print JSON output (1, true, yes)
//! - `CAUT_CONFIG`: Override config file path

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::AppPaths;
use crate::cli::args::{Cli, OutputFormat, UsageArgs};
use crate::core::provider::Provider;
use crate::error::Result;

// =============================================================================
// Environment Variable Names
// =============================================================================

/// Environment variable for comma-separated provider list.
pub const ENV_PROVIDERS: &str = "CAUT_PROVIDERS";
/// Environment variable for output format.
pub const ENV_FORMAT: &str = "CAUT_FORMAT";
/// Environment variable for timeout in seconds.
pub const ENV_TIMEOUT: &str = "CAUT_TIMEOUT";
/// Environment variable to disable colors.
pub const ENV_NO_COLOR: &str = "CAUT_NO_COLOR";
/// Standard environment variable to disable colors.
pub const ENV_NO_COLOR_STD: &str = "NO_COLOR";
/// Environment variable for verbose output.
pub const ENV_VERBOSE: &str = "CAUT_VERBOSE";
/// Environment variable for pretty JSON output.
pub const ENV_PRETTY: &str = "CAUT_PRETTY";
/// Environment variable to override config file path.
pub const ENV_CONFIG: &str = "CAUT_CONFIG";

// =============================================================================
// Resolved Configuration
// =============================================================================

/// Fully resolved configuration after merging CLI, env vars, and config file.
///
/// This struct represents the final, validated configuration to be used
/// by the application. All values have been resolved according to the
/// precedence rules.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// Providers to query.
    pub providers: Vec<Provider>,
    /// Output format.
    pub format: OutputFormat,
    /// Request timeout.
    pub timeout: Duration,
    /// Whether to disable colored output.
    pub no_color: bool,
    /// Whether verbose logging is enabled.
    pub verbose: bool,
    /// Whether to pretty-print JSON output.
    pub pretty: bool,
    /// Whether to include provider status.
    pub include_status: bool,
    /// Source of each setting for debugging.
    pub sources: ConfigSources,
}

/// Tracks the source of each configuration value.
#[derive(Debug, Clone, Default)]
pub struct ConfigSources {
    pub providers: ConfigSource,
    pub format: ConfigSource,
    pub timeout: ConfigSource,
    pub no_color: ConfigSource,
    pub verbose: ConfigSource,
    pub pretty: ConfigSource,
    pub include_status: ConfigSource,
}

/// Where a configuration value came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConfigSource {
    /// Value from CLI flag.
    Cli,
    /// Value from environment variable.
    Env,
    /// Value from config file.
    ConfigFile,
    /// Built-in default.
    #[default]
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cli => write!(f, "CLI flag"),
            Self::Env => write!(f, "environment variable"),
            Self::ConfigFile => write!(f, "config file"),
            Self::Default => write!(f, "default"),
        }
    }
}

impl ResolvedConfig {
    /// Resolve final configuration from CLI args, environment variables, and config file.
    ///
    /// # Precedence
    ///
    /// 1. CLI flags (highest priority)
    /// 2. Environment variables
    /// 3. Config file
    /// 4. Built-in defaults (lowest priority)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The config file exists but is invalid
    /// - Any resolved value is invalid (e.g., unknown provider)
    pub fn resolve(cli: &Cli, usage_args: Option<&UsageArgs>) -> Result<Self> {
        let config = Self::load_config()?;
        config.validate()?;

        let mut sources = ConfigSources::default();

        let providers = Self::resolve_providers(usage_args, &config, &mut sources.providers)?;
        let format = Self::resolve_format(cli, &config, &mut sources.format)?;
        let timeout = Self::resolve_timeout(usage_args, &config, &mut sources.timeout);
        let no_color = Self::resolve_no_color(cli, &config, &mut sources.no_color);
        let verbose = Self::resolve_verbose(cli, &mut sources.verbose);
        let pretty = Self::resolve_pretty(cli, &config, &mut sources.pretty);
        let include_status =
            Self::resolve_include_status(usage_args, &config, &mut sources.include_status);

        Ok(Self {
            providers,
            format,
            timeout,
            no_color,
            verbose,
            pretty,
            include_status,
            sources,
        })
    }

    /// Load config file, respecting CAUT_CONFIG override.
    fn load_config() -> Result<Config> {
        if let Ok(path) = std::env::var(ENV_CONFIG) {
            Config::load_from(Path::new(&path))
        } else {
            Config::load()
        }
    }

    /// Resolve providers setting.
    fn resolve_providers(
        usage_args: Option<&UsageArgs>,
        config: &Config,
        source: &mut ConfigSource,
    ) -> Result<Vec<Provider>> {
        // 1. CLI flag
        if let Some(args) = usage_args {
            if let Some(ref provider_arg) = args.provider {
                *source = ConfigSource::Cli;
                return Self::parse_provider_arg(provider_arg);
            }
        }

        // 2. Environment variable
        if let Ok(providers_env) = std::env::var(ENV_PROVIDERS) {
            *source = ConfigSource::Env;
            return providers_env
                .split(',')
                .map(|s| Provider::from_cli_name(s.trim()))
                .collect();
        }

        // 3. Config file
        if !config.providers.default_providers.is_empty() {
            *source = ConfigSource::ConfigFile;
            return config
                .providers
                .default_providers
                .iter()
                .map(|s| Provider::from_cli_name(s))
                .collect();
        }

        // 4. Default
        *source = ConfigSource::Default;
        Ok(vec![Provider::Claude, Provider::Codex])
    }

    /// Parse a provider argument (single provider, "both", or "all").
    fn parse_provider_arg(arg: &str) -> Result<Vec<Provider>> {
        match arg.to_lowercase().as_str() {
            "both" => Ok(vec![Provider::Claude, Provider::Codex]),
            "all" => Ok(Provider::ALL.to_vec()),
            name => Ok(vec![Provider::from_cli_name(name)?]),
        }
    }

    /// Resolve output format setting.
    fn resolve_format(
        cli: &Cli,
        config: &Config,
        source: &mut ConfigSource,
    ) -> Result<OutputFormat> {
        // 1. CLI --json flag (shorthand)
        if cli.json {
            *source = ConfigSource::Cli;
            return Ok(OutputFormat::Json);
        }

        // 1. CLI --format flag (if not default)
        // Note: clap sets default_value, so we check env first
        // We can't distinguish "user passed --format human" from default
        // So env has lower priority than CLI default

        // 2. Environment variable
        if let Ok(format_env) = std::env::var(ENV_FORMAT) {
            *source = ConfigSource::Env;
            return Self::parse_format(&format_env);
        }

        // Check if CLI format was explicitly set (not default)
        // Since clap uses default_value, we rely on the presence of
        // other CLI args or env vars to determine source
        if cli.format != OutputFormat::Human {
            *source = ConfigSource::Cli;
            return Ok(cli.format);
        }

        // 3. Config file
        if let Some(ref format_str) = config.output.format {
            *source = ConfigSource::ConfigFile;
            return Self::parse_format(format_str);
        }

        // 4. Default (from clap)
        *source = ConfigSource::Default;
        Ok(OutputFormat::Human)
    }

    /// Parse a format string into OutputFormat.
    fn parse_format(s: &str) -> Result<OutputFormat> {
        match s.to_lowercase().as_str() {
            "human" => Ok(OutputFormat::Human),
            "json" => Ok(OutputFormat::Json),
            "md" | "markdown" => Ok(OutputFormat::Md),
            _ => Err(crate::error::CautError::Config(format!(
                "Invalid format '{}'. Valid formats: human, json, md",
                s
            ))),
        }
    }

    /// Resolve timeout setting.
    fn resolve_timeout(
        usage_args: Option<&UsageArgs>,
        config: &Config,
        source: &mut ConfigSource,
    ) -> Duration {
        // 1. CLI --timeout or --web-timeout flag
        if let Some(args) = usage_args {
            if let Some(timeout) = args.timeout {
                *source = ConfigSource::Cli;
                return Duration::from_secs(timeout);
            }
            if let Some(timeout) = args.web_timeout {
                *source = ConfigSource::Cli;
                return Duration::from_secs(timeout);
            }
        }

        // 2. Environment variable
        if let Ok(timeout_env) = std::env::var(ENV_TIMEOUT) {
            if let Ok(timeout) = timeout_env.parse::<u64>() {
                *source = ConfigSource::Env;
                return Duration::from_secs(timeout);
            }
        }

        // 3. Config file
        *source = ConfigSource::ConfigFile;
        Duration::from_secs(config.general.timeout_seconds)
    }

    /// Resolve no_color setting.
    fn resolve_no_color(cli: &Cli, config: &Config, source: &mut ConfigSource) -> bool {
        // 1. CLI --no-color flag
        if cli.no_color {
            *source = ConfigSource::Cli;
            return true;
        }

        // 2. Environment variable (CAUT_NO_COLOR or standard NO_COLOR)
        if Self::is_env_truthy(ENV_NO_COLOR) || std::env::var(ENV_NO_COLOR_STD).is_ok() {
            *source = ConfigSource::Env;
            return true;
        }

        // 3. Config file (inverted: config.output.color = false means no_color = true)
        if !config.output.color {
            *source = ConfigSource::ConfigFile;
            return true;
        }

        // 4. Default
        *source = ConfigSource::Default;
        false
    }

    /// Resolve verbose setting.
    fn resolve_verbose(cli: &Cli, source: &mut ConfigSource) -> bool {
        // 1. CLI --verbose flag
        if cli.verbose {
            *source = ConfigSource::Cli;
            return true;
        }

        // 2. Environment variable
        if Self::is_env_truthy(ENV_VERBOSE) {
            *source = ConfigSource::Env;
            return true;
        }

        // 3. Default (no config file setting for verbose)
        *source = ConfigSource::Default;
        false
    }

    /// Resolve pretty setting.
    fn resolve_pretty(cli: &Cli, config: &Config, source: &mut ConfigSource) -> bool {
        // 1. CLI --pretty flag
        if cli.pretty {
            *source = ConfigSource::Cli;
            return true;
        }

        // 2. Environment variable
        if Self::is_env_truthy(ENV_PRETTY) {
            *source = ConfigSource::Env;
            return true;
        }

        // 3. Config file
        if config.output.pretty {
            *source = ConfigSource::ConfigFile;
            return true;
        }

        // 4. Default
        *source = ConfigSource::Default;
        false
    }

    /// Resolve include_status setting.
    fn resolve_include_status(
        usage_args: Option<&UsageArgs>,
        config: &Config,
        source: &mut ConfigSource,
    ) -> bool {
        // 1. CLI --status flag
        if let Some(args) = usage_args {
            if args.status {
                *source = ConfigSource::Cli;
                return true;
            }
        }

        // 2. Config file
        if config.general.include_status {
            *source = ConfigSource::ConfigFile;
            return true;
        }

        // 3. Default
        *source = ConfigSource::Default;
        false
    }

    /// Check if an environment variable is set to a truthy value.
    fn is_env_truthy(var: &str) -> bool {
        std::env::var(var)
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false)
    }
}

/// Application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// General settings.
    pub general: GeneralConfig,
    /// Provider-specific settings.
    pub providers: ProvidersConfig,
    /// Output settings.
    pub output: OutputConfig,
}

/// General application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Default timeout for network requests in seconds.
    pub timeout_seconds: u64,
    /// Whether to include provider status by default.
    pub include_status: bool,
    /// Default log level (error, warn, info, debug, trace).
    pub log_level: Option<String>,
}

/// Provider-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProvidersConfig {
    /// Default providers to query when none specified.
    pub default_providers: Vec<String>,
    /// Claude-specific settings.
    pub claude: ProviderSettings,
    /// Codex-specific settings.
    pub codex: ProviderSettings,
}

/// Settings for a specific provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderSettings {
    /// Whether this provider is enabled.
    pub enabled: bool,
    /// Custom API base URL (if different from default).
    pub api_base: Option<String>,
}

/// Output formatting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Default output format (human, json, md).
    pub format: Option<String>,
    /// Whether to use colors in output.
    pub color: bool,
    /// Whether to pretty-print JSON output.
    pub pretty: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            providers: ProvidersConfig::default(),
            output: OutputConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            include_status: false,
            log_level: None,
        }
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            default_providers: vec!["claude".to_string(), "codex".to_string()],
            claude: ProviderSettings {
                enabled: true,
                api_base: None,
            },
            codex: ProviderSettings {
                enabled: true,
                api_base: None,
            },
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: None,
            color: true,
            pretty: false,
        }
    }
}

impl Config {
    /// Load configuration from the default config file path.
    ///
    /// Returns default config if the file doesn't exist.
    /// Returns error only if the file exists but is invalid.
    pub fn load() -> Result<Self> {
        let paths = AppPaths::new();
        Self::load_from(&paths.config.join("config.toml"))
    }

    /// Load configuration from a specific path.
    ///
    /// Returns default config if the file doesn't exist.
    /// Returns error only if the file exists but is invalid.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            tracing::debug!(?path, "Config file not found, using defaults");
            return Ok(Self::default());
        }

        tracing::debug!(?path, "Loading config file");
        let content = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| crate::error::CautError::Config(format!("Invalid config file: {e}")))?;

        Ok(config)
    }

    /// Save configuration to the default config file path.
    pub fn save(&self) -> Result<()> {
        let paths = AppPaths::new();
        self.save_to(&paths.config.join("config.toml"))
    }

    /// Save configuration to a specific path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self).map_err(|e| {
            crate::error::CautError::Config(format!("Failed to serialize config: {e}"))
        })?;

        fs::write(path, content)?;
        tracing::debug!(?path, "Config file saved");
        Ok(())
    }

    /// Get the config file path.
    #[must_use]
    pub fn config_path() -> std::path::PathBuf {
        AppPaths::new().config.join("config.toml")
    }

    /// Validate configuration values.
    ///
    /// Checks that:
    /// - Provider names are valid
    /// - Output format is valid (human, json, md)
    /// - Timeout is within reasonable bounds (1-300 seconds)
    pub fn validate(&self) -> Result<()> {
        use crate::core::provider::Provider;
        use crate::error::CautError;

        // Validate default provider names
        let valid_providers = Provider::ALL
            .iter()
            .map(|provider| provider.cli_name())
            .collect::<Vec<_>>()
            .join(", ");

        for name in &self.providers.default_providers {
            Provider::from_cli_name(name).map_err(|_| {
                CautError::Config(format!(
                    "Invalid provider \"{name}\" in default_providers. Valid providers: {valid_providers}",
                ))
            })?;
        }

        // Validate output format
        if let Some(format) = &self.output.format {
            if !["human", "json", "md"].contains(&format.as_str()) {
                return Err(CautError::Config(format!(
                    "Invalid format \"{}\". Valid formats: human, json, md",
                    format
                )));
            }
        }

        // Validate timeout bounds
        if self.general.timeout_seconds == 0 || self.general.timeout_seconds > 300 {
            return Err(CautError::Config(
                "Timeout must be between 1 and 300 seconds".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert_eq!(config.general.timeout_seconds, 30);
        assert!(config.output.color);
        assert!(!config.providers.default_providers.is_empty());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let result = Config::load_from(Path::new("/nonexistent/path/config.toml"));
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.general.timeout_seconds, 30);
    }

    #[test]
    fn load_valid_toml() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r"
[general]
timeout_seconds = 60
include_status = true

[output]
color = false
pretty = true
 "
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();
        assert_eq!(config.general.timeout_seconds, 60);
        assert!(config.general.include_status);
        assert!(!config.output.color);
        assert!(config.output.pretty);
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "this is not valid toml {{{{").unwrap();

        let result = Config::load_from(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = Config::default();
        config.general.timeout_seconds = 120;
        config.output.pretty = true;

        config.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();

        assert_eq!(loaded.general.timeout_seconds, 120);
        assert!(loaded.output.pretty);
    }

    #[test]
    fn validate_default_config_is_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_invalid_provider_name() {
        let mut config = Config::default();
        config.providers.default_providers = vec!["invalid_provider".to_string()];
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid provider"));
    }

    #[test]
    fn validate_invalid_format() {
        let mut config = Config::default();
        config.output.format = Some("invalid_format".to_string());
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid format"));
    }

    #[test]
    fn validate_timeout_zero_is_invalid() {
        let mut config = Config::default();
        config.general.timeout_seconds = 0;
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Timeout must be between"));
    }

    #[test]
    fn validate_timeout_too_high_is_invalid() {
        let mut config = Config::default();
        config.general.timeout_seconds = 500;
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Timeout must be between"));
    }

    #[test]
    fn validate_valid_formats() {
        for format in &["human", "json", "md"] {
            let mut config = Config::default();
            config.output.format = Some(format.to_string());
            assert!(
                config.validate().is_ok(),
                "Format '{format}' should be valid"
            );
        }
    }

    // -------------------------------------------------------------------------
    // ResolvedConfig tests
    // -------------------------------------------------------------------------

    /// Helper to safely set an environment variable in tests.
    /// SAFETY: Tests are run single-threaded with `cargo test -- --test-threads=1`
    /// for tests that modify environment variables.
    #[allow(unsafe_code)]
    fn set_env(key: &str, value: &str) {
        // SAFETY: Tests modifying env vars should run single-threaded
        unsafe { std::env::set_var(key, value) };
    }

    /// Helper to safely remove an environment variable in tests.
    #[allow(unsafe_code)]
    fn remove_env(key: &str) {
        // SAFETY: Tests modifying env vars should run single-threaded
        unsafe { std::env::remove_var(key) };
    }

    /// Helper to create a default CLI struct for testing.
    fn make_test_cli() -> Cli {
        Cli {
            command: None,
            format: OutputFormat::Human,
            json: false,
            pretty: false,
            no_color: false,
            log_level: None,
            json_output: false,
            verbose: false,
            debug_rich: false,
        }
    }

    /// Helper to create default usage args for testing.
    fn make_test_usage_args() -> UsageArgs {
        UsageArgs {
            provider: None,
            account: None,
            account_index: None,
            all_accounts: false,
            no_credits: false,
            status: false,
            source: None,
            web: false,
            timeout: None,
            web_timeout: None,
            web_debug_dump_html: false,
            watch: false,
            interval: 30,
        }
    }

    #[test]
    fn config_source_display() {
        assert_eq!(format!("{}", ConfigSource::Cli), "CLI flag");
        assert_eq!(format!("{}", ConfigSource::Env), "environment variable");
        assert_eq!(format!("{}", ConfigSource::ConfigFile), "config file");
        assert_eq!(format!("{}", ConfigSource::Default), "default");
    }

    #[test]
    fn resolved_config_default_values() {
        // Clear any env vars that might affect the test
        remove_env(ENV_PROVIDERS);
        remove_env(ENV_FORMAT);
        remove_env(ENV_TIMEOUT);
        remove_env(ENV_NO_COLOR);
        remove_env(ENV_VERBOSE);
        remove_env(ENV_PRETTY);
        remove_env(ENV_CONFIG);

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        // Check defaults
        assert_eq!(resolved.format, OutputFormat::Human);
        assert!(!resolved.no_color);
        assert!(!resolved.verbose);
        assert!(!resolved.pretty);
        assert!(!resolved.include_status);
    }

    #[test]
    fn resolved_config_cli_json_flag() {
        remove_env(ENV_FORMAT);

        let mut cli = make_test_cli();
        cli.json = true;

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert_eq!(resolved.format, OutputFormat::Json);
        assert_eq!(resolved.sources.format, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_cli_format_flag() {
        remove_env(ENV_FORMAT);

        let mut cli = make_test_cli();
        cli.format = OutputFormat::Md;

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert_eq!(resolved.format, OutputFormat::Md);
        assert_eq!(resolved.sources.format, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_cli_verbose_flag() {
        remove_env(ENV_VERBOSE);

        let mut cli = make_test_cli();
        cli.verbose = true;

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.verbose);
        assert_eq!(resolved.sources.verbose, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_cli_no_color_flag() {
        remove_env(ENV_NO_COLOR);
        remove_env(ENV_NO_COLOR_STD);

        let mut cli = make_test_cli();
        cli.no_color = true;

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.no_color);
        assert_eq!(resolved.sources.no_color, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_cli_pretty_flag() {
        remove_env(ENV_PRETTY);

        let mut cli = make_test_cli();
        cli.pretty = true;

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.pretty);
        assert_eq!(resolved.sources.pretty, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_usage_args_provider() {
        remove_env(ENV_PROVIDERS);

        let cli = make_test_cli();
        let mut usage_args = make_test_usage_args();
        usage_args.provider = Some("claude".to_string());

        let resolved = ResolvedConfig::resolve(&cli, Some(&usage_args)).unwrap();

        assert_eq!(resolved.providers.len(), 1);
        assert_eq!(resolved.providers[0], Provider::Claude);
        assert_eq!(resolved.sources.providers, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_usage_args_provider_both() {
        remove_env(ENV_PROVIDERS);

        let cli = make_test_cli();
        let mut usage_args = make_test_usage_args();
        usage_args.provider = Some("both".to_string());

        let resolved = ResolvedConfig::resolve(&cli, Some(&usage_args)).unwrap();

        assert_eq!(resolved.providers.len(), 2);
        assert!(resolved.providers.contains(&Provider::Claude));
        assert!(resolved.providers.contains(&Provider::Codex));
    }

    #[test]
    fn resolved_config_usage_args_timeout() {
        remove_env(ENV_TIMEOUT);

        let cli = make_test_cli();
        let mut usage_args = make_test_usage_args();
        usage_args.web_timeout = Some(120);

        let resolved = ResolvedConfig::resolve(&cli, Some(&usage_args)).unwrap();

        assert_eq!(resolved.timeout, Duration::from_secs(120));
        assert_eq!(resolved.sources.timeout, ConfigSource::Cli);
    }

    #[test]
    fn resolved_config_usage_args_status() {
        let cli = make_test_cli();
        let mut usage_args = make_test_usage_args();
        usage_args.status = true;

        let resolved = ResolvedConfig::resolve(&cli, Some(&usage_args)).unwrap();

        assert!(resolved.include_status);
        assert_eq!(resolved.sources.include_status, ConfigSource::Cli);
    }

    #[test]
    fn parse_format_valid_values() {
        assert_eq!(
            ResolvedConfig::parse_format("human").unwrap(),
            OutputFormat::Human
        );
        assert_eq!(
            ResolvedConfig::parse_format("json").unwrap(),
            OutputFormat::Json
        );
        assert_eq!(
            ResolvedConfig::parse_format("md").unwrap(),
            OutputFormat::Md
        );
        assert_eq!(
            ResolvedConfig::parse_format("markdown").unwrap(),
            OutputFormat::Md
        );
        assert_eq!(
            ResolvedConfig::parse_format("JSON").unwrap(),
            OutputFormat::Json
        );
    }

    #[test]
    fn parse_format_invalid_value() {
        let result = ResolvedConfig::parse_format("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn parse_provider_arg_single() {
        let providers = ResolvedConfig::parse_provider_arg("claude").unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0], Provider::Claude);
    }

    #[test]
    fn parse_provider_arg_both() {
        let providers = ResolvedConfig::parse_provider_arg("both").unwrap();
        assert_eq!(providers.len(), 2);
        assert!(providers.contains(&Provider::Claude));
        assert!(providers.contains(&Provider::Codex));
    }

    #[test]
    fn parse_provider_arg_all() {
        let providers = ResolvedConfig::parse_provider_arg("all").unwrap();
        assert_eq!(providers.len(), Provider::ALL.len());
    }

    #[test]
    fn is_env_truthy_values() {
        set_env("TEST_TRUTHY_1", "1");
        set_env("TEST_TRUTHY_TRUE", "true");
        set_env("TEST_TRUTHY_YES", "yes");
        set_env("TEST_TRUTHY_ON", "on");
        set_env("TEST_TRUTHY_FALSE", "false");
        set_env("TEST_TRUTHY_0", "0");

        assert!(ResolvedConfig::is_env_truthy("TEST_TRUTHY_1"));
        assert!(ResolvedConfig::is_env_truthy("TEST_TRUTHY_TRUE"));
        assert!(ResolvedConfig::is_env_truthy("TEST_TRUTHY_YES"));
        assert!(ResolvedConfig::is_env_truthy("TEST_TRUTHY_ON"));
        assert!(!ResolvedConfig::is_env_truthy("TEST_TRUTHY_FALSE"));
        assert!(!ResolvedConfig::is_env_truthy("TEST_TRUTHY_0"));
        assert!(!ResolvedConfig::is_env_truthy("TEST_NONEXISTENT"));

        // Clean up
        remove_env("TEST_TRUTHY_1");
        remove_env("TEST_TRUTHY_TRUE");
        remove_env("TEST_TRUTHY_YES");
        remove_env("TEST_TRUTHY_ON");
        remove_env("TEST_TRUTHY_FALSE");
        remove_env("TEST_TRUTHY_0");
    }

    // -------------------------------------------------------------------------
    // Environment Variable Override Tests
    // -------------------------------------------------------------------------

    #[test]
    fn env_providers_comma_separated() {
        remove_env(ENV_CONFIG);
        set_env(ENV_PROVIDERS, "claude,codex");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert_eq!(resolved.providers.len(), 2);
        assert!(resolved.providers.contains(&Provider::Claude));
        assert!(resolved.providers.contains(&Provider::Codex));
        assert_eq!(resolved.sources.providers, ConfigSource::Env);

        remove_env(ENV_PROVIDERS);
    }

    #[test]
    fn env_providers_single() {
        remove_env(ENV_CONFIG);
        set_env(ENV_PROVIDERS, "claude");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert_eq!(resolved.providers.len(), 1);
        assert_eq!(resolved.providers[0], Provider::Claude);
        assert_eq!(resolved.sources.providers, ConfigSource::Env);

        remove_env(ENV_PROVIDERS);
    }

    #[test]
    fn env_format_override() {
        remove_env(ENV_CONFIG);
        set_env(ENV_FORMAT, "json");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert_eq!(resolved.format, OutputFormat::Json);
        assert_eq!(resolved.sources.format, ConfigSource::Env);

        remove_env(ENV_FORMAT);
    }

    #[test]
    fn env_timeout_override() {
        remove_env(ENV_CONFIG);
        set_env(ENV_TIMEOUT, "90");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert_eq!(resolved.timeout, Duration::from_secs(90));
        assert_eq!(resolved.sources.timeout, ConfigSource::Env);

        remove_env(ENV_TIMEOUT);
    }

    #[test]
    fn env_no_color_override() {
        remove_env(ENV_CONFIG);
        remove_env(ENV_NO_COLOR_STD);
        set_env(ENV_NO_COLOR, "1");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.no_color);
        assert_eq!(resolved.sources.no_color, ConfigSource::Env);

        remove_env(ENV_NO_COLOR);
    }

    #[test]
    fn env_no_color_std_override() {
        remove_env(ENV_CONFIG);
        remove_env(ENV_NO_COLOR);
        set_env(ENV_NO_COLOR_STD, ""); // Any value works for NO_COLOR standard

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.no_color);
        assert_eq!(resolved.sources.no_color, ConfigSource::Env);

        remove_env(ENV_NO_COLOR_STD);
    }

    #[test]
    fn env_verbose_override() {
        remove_env(ENV_CONFIG);
        set_env(ENV_VERBOSE, "true");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.verbose);
        assert_eq!(resolved.sources.verbose, ConfigSource::Env);

        remove_env(ENV_VERBOSE);
    }

    #[test]
    fn env_pretty_override() {
        remove_env(ENV_CONFIG);
        set_env(ENV_PRETTY, "yes");

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        assert!(resolved.pretty);
        assert_eq!(resolved.sources.pretty, ConfigSource::Env);

        remove_env(ENV_PRETTY);
    }

    #[test]
    fn cli_json_flag_overrides_env() {
        remove_env(ENV_CONFIG);
        set_env(ENV_FORMAT, "md");

        let mut cli = make_test_cli();
        cli.json = true; // --json flag explicitly set

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        // CLI --json flag takes precedence over env
        assert_eq!(resolved.format, OutputFormat::Json);
        assert_eq!(resolved.sources.format, ConfigSource::Cli);

        remove_env(ENV_FORMAT);
    }

    #[test]
    fn env_format_has_priority_over_default_cli_format() {
        // Note: When CLI format is default (Human), env var takes precedence
        // This is because clap's default_value means we can't distinguish
        // "user explicitly passed --format human" from "default value"
        remove_env(ENV_CONFIG);
        set_env(ENV_FORMAT, "md");

        let cli = make_test_cli(); // format = Human (default)

        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        // Env takes precedence over CLI default
        assert_eq!(resolved.format, OutputFormat::Md);
        assert_eq!(resolved.sources.format, ConfigSource::Env);

        remove_env(ENV_FORMAT);
    }

    // -------------------------------------------------------------------------
    // Per-Provider Settings Tests
    // -------------------------------------------------------------------------

    #[test]
    fn per_provider_default_settings() {
        let config = Config::default();

        assert!(config.providers.claude.enabled);
        assert!(config.providers.codex.enabled);
        assert!(config.providers.claude.api_base.is_none());
        assert!(config.providers.codex.api_base.is_none());
    }

    #[test]
    fn per_provider_custom_settings() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[providers.claude]
enabled = false
api_base = "https://custom-claude.example.com"

[providers.codex]
enabled = true
api_base = "https://custom-codex.example.com"
"#
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();

        assert!(!config.providers.claude.enabled);
        assert_eq!(
            config.providers.claude.api_base,
            Some("https://custom-claude.example.com".to_string())
        );
        assert!(config.providers.codex.enabled);
        assert_eq!(
            config.providers.codex.api_base,
            Some("https://custom-codex.example.com".to_string())
        );
    }

    #[test]
    fn default_providers_from_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[providers]
default_providers = ["codex"]
"#
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();

        assert_eq!(config.providers.default_providers, vec!["codex"]);
    }

    // -------------------------------------------------------------------------
    // Nested Config Section Tests
    // -------------------------------------------------------------------------

    #[test]
    fn nested_general_config() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[general]
timeout_seconds = 45
include_status = true
log_level = "debug"
"#
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();

        assert_eq!(config.general.timeout_seconds, 45);
        assert!(config.general.include_status);
        assert_eq!(config.general.log_level, Some("debug".to_string()));
    }

    #[test]
    fn nested_output_config() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[output]
format = "json"
color = false
pretty = true
"#
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();

        assert_eq!(config.output.format, Some("json".to_string()));
        assert!(!config.output.color);
        assert!(config.output.pretty);
    }

    #[test]
    fn all_nested_sections_combined() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[general]
timeout_seconds = 60
include_status = true

[providers]
default_providers = ["claude", "codex"]

[providers.claude]
enabled = true

[providers.codex]
enabled = false

[output]
format = "md"
color = true
pretty = false
"#
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();

        // General section
        assert_eq!(config.general.timeout_seconds, 60);
        assert!(config.general.include_status);

        // Providers section
        assert_eq!(config.providers.default_providers, vec!["claude", "codex"]);
        assert!(config.providers.claude.enabled);
        assert!(!config.providers.codex.enabled);

        // Output section
        assert_eq!(config.output.format, Some("md".to_string()));
        assert!(config.output.color);
        assert!(!config.output.pretty);
    }

    // -------------------------------------------------------------------------
    // Forward Compatibility Tests
    // -------------------------------------------------------------------------

    #[test]
    fn unknown_fields_are_ignored() {
        // serde(default) should ignore unknown fields
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[general]
timeout_seconds = 30
future_field = "some_value"
another_unknown = 42

[unknown_section]
foo = "bar"
"#
        )
        .unwrap();

        let config = Config::load_from(file.path());

        // Should load successfully, ignoring unknown fields
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.general.timeout_seconds, 30);
    }

    #[test]
    fn partial_config_uses_defaults() {
        // Only specify some fields, rest should use defaults
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[general]
timeout_seconds = 45
"#
        )
        .unwrap();

        let config = Config::load_from(file.path()).unwrap();

        // Specified value
        assert_eq!(config.general.timeout_seconds, 45);

        // Default values for unspecified fields
        assert!(!config.general.include_status); // default is false
        assert!(config.output.color); // default is true
        assert!(!config.output.pretty); // default is false
        assert!(config.providers.claude.enabled); // default is true
    }

    // -------------------------------------------------------------------------
    // Config Path Override Tests
    // -------------------------------------------------------------------------

    #[test]
    fn caut_config_env_override() {
        // Create a temp config file
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("custom_config.toml");

        // Write custom config
        std::fs::write(
            &config_path,
            r#"
[general]
timeout_seconds = 99

[output]
pretty = true
"#,
        )
        .unwrap();

        // Set CAUT_CONFIG to point to custom file
        set_env(ENV_CONFIG, config_path.to_str().unwrap());

        let cli = make_test_cli();
        let resolved = ResolvedConfig::resolve(&cli, None).unwrap();

        // Should use values from custom config
        assert_eq!(resolved.timeout, Duration::from_secs(99));
        assert!(resolved.pretty);
        assert_eq!(resolved.sources.timeout, ConfigSource::ConfigFile);
        assert_eq!(resolved.sources.pretty, ConfigSource::ConfigFile);

        remove_env(ENV_CONFIG);
    }

    // -------------------------------------------------------------------------
    // Config Validation Edge Cases
    // -------------------------------------------------------------------------

    #[test]
    fn validate_valid_provider_names() {
        let mut config = Config::default();
        config.providers.default_providers = vec!["claude".to_string(), "codex".to_string()];

        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_empty_providers_is_valid() {
        let mut config = Config::default();
        config.providers.default_providers = vec![];

        // Empty providers should be valid (uses defaults at runtime)
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_boundary_timeout() {
        // Minimum valid timeout
        let mut config = Config::default();
        config.general.timeout_seconds = 1;
        assert!(config.validate().is_ok());

        // Maximum valid timeout
        config.general.timeout_seconds = 300;
        assert!(config.validate().is_ok());

        // Just over maximum
        config.general.timeout_seconds = 301;
        assert!(config.validate().is_err());
    }

    // -------------------------------------------------------------------------
    // Default Values Documentation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn default_config_documents_sensible_defaults() {
        let config = Config::default();

        // Document all default values
        assert_eq!(
            config.general.timeout_seconds, 30,
            "Default timeout should be 30 seconds"
        );
        assert!(
            !config.general.include_status,
            "Status should be off by default"
        );
        assert!(
            config.general.log_level.is_none(),
            "Log level should use system default"
        );

        assert!(
            config.output.color,
            "Color output should be enabled by default"
        );
        assert!(
            !config.output.pretty,
            "Pretty output should be off by default"
        );
        assert!(
            config.output.format.is_none(),
            "Format should use CLI default (human)"
        );

        assert_eq!(
            config.providers.default_providers,
            vec!["claude", "codex"],
            "Default providers should be claude and codex"
        );
        assert!(
            config.providers.claude.enabled,
            "Claude should be enabled by default"
        );
        assert!(
            config.providers.codex.enabled,
            "Codex should be enabled by default"
        );
    }
}
