//! Error types for caut.
//!
//! Uses `thiserror` for structured error types that map to exit codes.
//!
//! ## Error Taxonomy
//!
//! Errors are categorized into six main categories:
//! - **Authentication**: Issues with credentials, tokens, or login state
//! - **Network**: Connection, timeout, DNS, or SSL/TLS issues
//! - **Configuration**: Config file parsing, validation, or missing values
//! - **Provider**: Rate limits, service unavailability, or API issues
//! - **Environment**: Missing tools, permissions, or system requirements
//! - **Internal**: Unexpected errors, bugs, or unclassified issues
//!
//! Each error has a stable error code (e.g., `CAUT-A001`) for programmatic handling.
//!
//! ## Fix Suggestions
//!
//! Each error type can provide actionable fix suggestions via the
//! [`CautError::fix_suggestions()`] method. Suggestions include:
//! - Commands to run (copy-paste ready)
//! - Context explaining why the error occurred
//! - Prevention tips for the future
//! - Documentation links when available

pub mod suggestions;

use std::time::Duration;
use thiserror::Error;

pub use suggestions::FixSuggestion;

// =============================================================================
// Error Categories
// =============================================================================

/// High-level error categories for classification and routing.
///
/// Used to determine fix suggestions and error handling strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// Authentication issues (expired, missing, invalid tokens/credentials).
    Authentication,
    /// Network issues (timeout, DNS, SSL, connection refused).
    Network,
    /// Configuration issues (parse errors, invalid values, missing files).
    Configuration,
    /// Provider-specific issues (rate limits, unavailable, API errors).
    Provider,
    /// Environment issues (missing CLIs, permissions, system requirements).
    Environment,
    /// Internal errors (bugs, unexpected state, unclassified).
    Internal,
}

impl ErrorCategory {
    /// Returns a human-readable description of the category.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Authentication => "Authentication error",
            Self::Network => "Network error",
            Self::Configuration => "Configuration error",
            Self::Provider => "Provider error",
            Self::Environment => "Environment error",
            Self::Internal => "Internal error",
        }
    }

    /// Returns a short code prefix for this category.
    #[must_use]
    pub const fn code_prefix(&self) -> &'static str {
        match self {
            Self::Authentication => "A",
            Self::Network => "N",
            Self::Configuration => "C",
            Self::Provider => "P",
            Self::Environment => "E",
            Self::Internal => "X",
        }
    }
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

// =============================================================================
// Exit Codes
// =============================================================================

/// Exit codes matching CodexBar CLI semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    /// Success
    Success = 0,
    /// Unexpected failure
    GeneralError = 1,
    /// Binary not found, provider CLI not installed
    BinaryNotFound = 2,
    /// Parse/format errors, unsupported provider, missing rate limits
    ParseError = 3,
    /// Timeout
    Timeout = 4,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as i32
    }
}

/// Main error type for caut operations.
///
/// Each variant has:
/// - A stable error code (e.g., `CAUT-A001`)
/// - A category for classification
/// - A retryable flag for retry logic
#[derive(Error, Debug)]
pub enum CautError {
    // ==========================================================================
    // Authentication errors (Category: Authentication)
    // ==========================================================================
    /// Authentication token has expired and needs refresh.
    #[error("authentication expired for {provider}")]
    AuthExpired {
        provider: String,
    },

    /// Authentication is not configured for the provider.
    #[error("authentication not configured for {provider}")]
    AuthNotConfigured {
        provider: String,
    },

    /// Authentication credentials are invalid.
    #[error("invalid credentials for {provider}: {reason}")]
    AuthInvalid {
        provider: String,
        reason: String,
    },

    // ==========================================================================
    // Network errors (Category: Network)
    // ==========================================================================
    /// Request timed out after specified duration.
    #[error("request timeout after {seconds}s for {provider}")]
    TimeoutWithProvider {
        provider: String,
        seconds: u64,
    },

    /// DNS resolution failed.
    #[error("DNS resolution failed for {host}")]
    DnsFailure {
        host: String,
    },

    /// SSL/TLS handshake or certificate error.
    #[error("SSL/TLS error: {message}")]
    SslError {
        message: String,
    },

    /// Connection refused by remote server.
    #[error("connection refused: {host}")]
    ConnectionRefused {
        host: String,
    },

    // ==========================================================================
    // Configuration errors (Category: Configuration)
    // ==========================================================================
    /// Configuration file not found at expected path.
    #[error("config file not found: {path}")]
    ConfigNotFound {
        path: String,
    },

    /// Error parsing configuration file.
    #[error("config parse error at {path}: {message}")]
    ConfigParse {
        path: String,
        line: Option<usize>,
        message: String,
    },

    /// Invalid value in configuration.
    #[error("invalid config value for '{key}': {message}")]
    ConfigInvalid {
        key: String,
        value: String,
        message: String,
    },

    // ==========================================================================
    // Provider errors (Category: Provider)
    // ==========================================================================
    /// Rate limited by provider.
    #[error("rate limited by {provider}: {message}")]
    RateLimited {
        provider: String,
        retry_after: Option<Duration>,
        message: String,
    },

    /// Provider service is temporarily unavailable.
    #[error("provider {provider} unavailable: {message}")]
    ProviderUnavailable {
        provider: String,
        message: String,
    },

    /// Provider API returned an error.
    #[error("provider {provider} API error: {message}")]
    ProviderApiError {
        provider: String,
        status_code: Option<u16>,
        message: String,
    },

    // ==========================================================================
    // Environment errors (Category: Environment)
    // ==========================================================================
    /// Required CLI tool not found in PATH.
    #[error("CLI tool not found: {name}")]
    CliNotFound {
        name: String,
    },

    /// Permission denied accessing file or resource.
    #[error("permission denied: {path}")]
    PermissionDenied {
        path: String,
    },

    /// Required environment variable not set.
    #[error("environment variable not set: {name}")]
    EnvVarMissing {
        name: String,
    },

    // ==========================================================================
    // Legacy errors (maintained for backward compatibility)
    // ==========================================================================
    /// Generic configuration error (legacy - prefer specific variants).
    #[error("configuration error: {0}")]
    Config(String),

    /// Invalid provider name (legacy).
    #[error("invalid provider: {0}")]
    InvalidProvider(String),

    /// Unsupported source type for provider (legacy).
    #[error("unsupported source for provider {provider}: {source_type}")]
    UnsupportedSource {
        provider: String,
        source_type: String,
    },

    /// Provider CLI not found (legacy - prefer CliNotFound).
    #[error("provider CLI not found: {0}")]
    ProviderNotFound(String),

    /// No fetch strategy available (legacy).
    #[error("no available fetch strategy for provider: {0}")]
    NoAvailableStrategy(String),

    /// Generic fetch failure (legacy - prefer specific variants).
    #[error("fetch failed for {provider}: {reason}")]
    FetchFailed { provider: String, reason: String },

    // ==========================================================================
    // Account errors (Category: Configuration)
    // ==========================================================================
    /// Account selection requires single provider.
    #[error("account selection requires a single provider")]
    AccountRequiresSingleProvider,

    /// Conflicting account flags.
    #[error("--all-accounts cannot be combined with --account or --account-index")]
    AllAccountsConflict,

    /// Provider doesn't support token accounts.
    #[error("provider {0} does not support token accounts")]
    ProviderNoTokenAccounts(String),

    /// Specified account not found.
    #[error("account not found: {0}")]
    AccountNotFound(String),

    /// No accounts configured for provider.
    #[error("no accounts configured for provider: {0}")]
    NoAccountsConfigured(String),

    // ==========================================================================
    // Parse errors (Category: Provider/Internal)
    // ==========================================================================
    /// Failed to parse provider response.
    #[error("failed to parse response: {0}")]
    ParseResponse(String),

    /// Rate limit data missing from response.
    #[error("missing rate limit data in response")]
    MissingRateLimit,

    // ==========================================================================
    // Partial failure (Category: Provider)
    // ==========================================================================
    /// Some providers succeeded, some failed.
    #[error("partial failure: {failed} provider(s) failed")]
    PartialFailure { failed: usize },

    // ==========================================================================
    // Network errors (Category: Network, legacy)
    // ==========================================================================
    /// Request timeout (legacy - prefer TimeoutWithProvider).
    #[error("request timeout after {0} seconds")]
    Timeout(u64),

    /// Generic network error (legacy).
    #[error("network error: {0}")]
    Network(String),

    // ==========================================================================
    // I/O errors (Category: Internal)
    // ==========================================================================
    /// I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    // ==========================================================================
    // Generic wrapper (Category: Internal)
    // ==========================================================================
    /// Catch-all for other errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CautError {
    /// Map error to exit code following CodexBar semantics.
    #[must_use]
    pub const fn exit_code(&self) -> ExitCode {
        match self {
            // Environment errors -> Binary not found
            Self::ProviderNotFound(_)
            | Self::CliNotFound { .. } => ExitCode::BinaryNotFound,

            // Configuration and parse errors -> Parse error
            Self::Config(_)
            | Self::ConfigNotFound { .. }
            | Self::ConfigParse { .. }
            | Self::ConfigInvalid { .. }
            | Self::InvalidProvider(_)
            | Self::UnsupportedSource { .. }
            | Self::AccountRequiresSingleProvider
            | Self::AllAccountsConflict
            | Self::ProviderNoTokenAccounts(_)
            | Self::AccountNotFound(_)
            | Self::NoAccountsConfigured(_)
            | Self::ParseResponse(_)
            | Self::MissingRateLimit
            | Self::NoAvailableStrategy(_)
            | Self::AuthNotConfigured { .. }
            | Self::AuthInvalid { .. } => ExitCode::ParseError,

            // Timeout errors
            Self::Timeout(_)
            | Self::TimeoutWithProvider { .. } => ExitCode::Timeout,

            // Everything else -> General error
            Self::AuthExpired { .. }
            | Self::DnsFailure { .. }
            | Self::SslError { .. }
            | Self::ConnectionRefused { .. }
            | Self::RateLimited { .. }
            | Self::ProviderUnavailable { .. }
            | Self::ProviderApiError { .. }
            | Self::PermissionDenied { .. }
            | Self::EnvVarMissing { .. }
            | Self::Network(_)
            | Self::FetchFailed { .. }
            | Self::PartialFailure { .. }
            | Self::Io(_)
            | Self::Json(_)
            | Self::Other(_) => ExitCode::GeneralError,
        }
    }

    /// Returns the error category for classification and routing.
    #[must_use]
    pub const fn category(&self) -> ErrorCategory {
        match self {
            // Authentication errors
            Self::AuthExpired { .. }
            | Self::AuthNotConfigured { .. }
            | Self::AuthInvalid { .. } => ErrorCategory::Authentication,

            // Network errors
            Self::Timeout(_)
            | Self::TimeoutWithProvider { .. }
            | Self::DnsFailure { .. }
            | Self::SslError { .. }
            | Self::ConnectionRefused { .. }
            | Self::Network(_) => ErrorCategory::Network,

            // Configuration errors
            Self::Config(_)
            | Self::ConfigNotFound { .. }
            | Self::ConfigParse { .. }
            | Self::ConfigInvalid { .. }
            | Self::InvalidProvider(_)
            | Self::UnsupportedSource { .. }
            | Self::AccountRequiresSingleProvider
            | Self::AllAccountsConflict
            | Self::ProviderNoTokenAccounts(_)
            | Self::AccountNotFound(_)
            | Self::NoAccountsConfigured(_) => ErrorCategory::Configuration,

            // Provider errors
            Self::RateLimited { .. }
            | Self::ProviderUnavailable { .. }
            | Self::ProviderApiError { .. }
            | Self::FetchFailed { .. }
            | Self::PartialFailure { .. }
            | Self::NoAvailableStrategy(_)
            | Self::ParseResponse(_)
            | Self::MissingRateLimit => ErrorCategory::Provider,

            // Environment errors
            Self::ProviderNotFound(_)
            | Self::CliNotFound { .. }
            | Self::PermissionDenied { .. }
            | Self::EnvVarMissing { .. } => ErrorCategory::Environment,

            // Internal errors
            Self::Io(_)
            | Self::Json(_)
            | Self::Other(_) => ErrorCategory::Internal,
        }
    }

    /// Returns a stable error code for programmatic handling.
    ///
    /// Format: `CAUT-{category}{number}` where category is:
    /// - A: Authentication
    /// - N: Network
    /// - C: Configuration
    /// - P: Provider
    /// - E: Environment
    /// - X: Internal
    #[must_use]
    pub const fn error_code(&self) -> &'static str {
        match self {
            // Authentication errors (A001-A099)
            Self::AuthExpired { .. } => "CAUT-A001",
            Self::AuthNotConfigured { .. } => "CAUT-A002",
            Self::AuthInvalid { .. } => "CAUT-A003",

            // Network errors (N001-N099)
            Self::Timeout(_) => "CAUT-N001",
            Self::TimeoutWithProvider { .. } => "CAUT-N002",
            Self::DnsFailure { .. } => "CAUT-N003",
            Self::SslError { .. } => "CAUT-N004",
            Self::ConnectionRefused { .. } => "CAUT-N005",
            Self::Network(_) => "CAUT-N099",

            // Configuration errors (C001-C099)
            Self::ConfigNotFound { .. } => "CAUT-C001",
            Self::ConfigParse { .. } => "CAUT-C002",
            Self::ConfigInvalid { .. } => "CAUT-C003",
            Self::Config(_) => "CAUT-C004",
            Self::InvalidProvider(_) => "CAUT-C010",
            Self::UnsupportedSource { .. } => "CAUT-C011",
            Self::AccountRequiresSingleProvider => "CAUT-C020",
            Self::AllAccountsConflict => "CAUT-C021",
            Self::ProviderNoTokenAccounts(_) => "CAUT-C022",
            Self::AccountNotFound(_) => "CAUT-C023",
            Self::NoAccountsConfigured(_) => "CAUT-C024",

            // Provider errors (P001-P099)
            Self::RateLimited { .. } => "CAUT-P001",
            Self::ProviderUnavailable { .. } => "CAUT-P002",
            Self::ProviderApiError { .. } => "CAUT-P003",
            Self::FetchFailed { .. } => "CAUT-P010",
            Self::NoAvailableStrategy(_) => "CAUT-P011",
            Self::ParseResponse(_) => "CAUT-P020",
            Self::MissingRateLimit => "CAUT-P021",
            Self::PartialFailure { .. } => "CAUT-P030",

            // Environment errors (E001-E099)
            Self::CliNotFound { .. } => "CAUT-E001",
            Self::ProviderNotFound(_) => "CAUT-E002",
            Self::PermissionDenied { .. } => "CAUT-E003",
            Self::EnvVarMissing { .. } => "CAUT-E004",

            // Internal errors (X001-X099)
            Self::Io(_) => "CAUT-X001",
            Self::Json(_) => "CAUT-X002",
            Self::Other(_) => "CAUT-X099",
        }
    }

    /// Returns whether the error is potentially recoverable by retrying.
    ///
    /// Retryable errors include:
    /// - Timeouts
    /// - Transient network errors
    /// - Rate limits (with backoff)
    /// - Temporary provider unavailability
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout(_)
            | Self::TimeoutWithProvider { .. }
            | Self::Network(_)
            | Self::ConnectionRefused { .. }
            | Self::DnsFailure { .. }
            | Self::RateLimited { .. }
            | Self::ProviderUnavailable { .. }
        )
    }

    /// Returns the retry-after duration if this error specifies one.
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Returns the provider name if this error is provider-specific.
    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::AuthExpired { provider }
            | Self::AuthNotConfigured { provider }
            | Self::AuthInvalid { provider, .. }
            | Self::TimeoutWithProvider { provider, .. }
            | Self::RateLimited { provider, .. }
            | Self::ProviderUnavailable { provider, .. }
            | Self::ProviderApiError { provider, .. }
            | Self::FetchFailed { provider, .. }
            | Self::UnsupportedSource { provider, .. } => Some(provider),
            Self::ProviderNotFound(p)
            | Self::InvalidProvider(p)
            | Self::ProviderNoTokenAccounts(p)
            | Self::AccountNotFound(p)
            | Self::NoAccountsConfigured(p)
            | Self::NoAvailableStrategy(p) => Some(p),
            _ => None,
        }
    }

    /// Returns actionable fix suggestions for this error.
    ///
    /// Suggestions include commands to run, context about why the error
    /// occurred, prevention tips, and documentation links when available.
    ///
    /// # Example
    ///
    /// ```
    /// use caut::error::CautError;
    ///
    /// let err = CautError::CliNotFound { name: "claude".to_string() };
    /// let suggestions = err.fix_suggestions();
    ///
    /// if !suggestions.is_empty() {
    ///     println!("Try these commands:");
    ///     for cmd in &suggestions[0].commands {
    ///         println!("  {}", cmd);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn fix_suggestions(&self) -> Vec<FixSuggestion> {
        match self {
            // Authentication errors
            Self::AuthExpired { provider } => {
                suggestions::auth_expired_suggestions(provider)
            }
            Self::AuthNotConfigured { provider } => {
                suggestions::auth_not_configured_suggestions(provider)
            }
            Self::AuthInvalid { provider, reason } => {
                suggestions::auth_invalid_suggestions(provider, reason)
            }

            // Network errors
            Self::Timeout(seconds) => {
                suggestions::timeout_suggestions("unknown", *seconds)
            }
            Self::TimeoutWithProvider { provider, seconds } => {
                suggestions::timeout_suggestions(provider, *seconds)
            }
            Self::DnsFailure { host } => {
                suggestions::dns_failure_suggestions(host)
            }
            Self::SslError { message } => {
                suggestions::ssl_error_suggestions(message)
            }
            Self::ConnectionRefused { host } => {
                suggestions::connection_refused_suggestions(host)
            }
            Self::Network(msg) => {
                vec![FixSuggestion::new(
                    vec!["caut doctor".to_string()],
                    format!("Network error: {}. Check your internet connection.", msg),
                )]
            }

            // Configuration errors
            Self::ConfigNotFound { path } => {
                suggestions::config_not_found_suggestions(path)
            }
            Self::ConfigParse { path, line, message } => {
                suggestions::config_parse_suggestions(path, *line, message)
            }
            Self::ConfigInvalid { key, value, message } => {
                suggestions::config_invalid_suggestions(key, value, message)
            }
            Self::Config(msg) => {
                vec![FixSuggestion::new(
                    vec!["caut config show".to_string()],
                    format!("Configuration error: {}", msg),
                )]
            }
            Self::InvalidProvider(name) => {
                suggestions::invalid_provider_suggestions(name)
            }
            Self::UnsupportedSource { provider, source_type } => {
                suggestions::unsupported_source_suggestions(provider, source_type)
            }
            Self::AccountRequiresSingleProvider => {
                vec![FixSuggestion::new(
                    vec!["caut usage --provider <provider> --account <account>".to_string()],
                    "Account selection requires specifying a single provider.",
                )]
            }
            Self::AllAccountsConflict => {
                vec![FixSuggestion::new(
                    vec![
                        "caut usage --all-accounts".to_string(),
                        "caut usage --account <name>".to_string(),
                    ],
                    "Cannot combine --all-accounts with --account or --account-index.",
                )]
            }
            Self::ProviderNoTokenAccounts(provider) => {
                vec![FixSuggestion::new(
                    vec![format!("caut providers show {}", provider)],
                    format!("Provider {} does not support multiple token accounts.", provider),
                )]
            }
            Self::AccountNotFound(account) => {
                suggestions::account_not_found_suggestions(account)
            }
            Self::NoAccountsConfigured(provider) => {
                suggestions::no_accounts_suggestions(provider)
            }

            // Provider errors
            Self::RateLimited { provider, retry_after, message } => {
                suggestions::rate_limited_suggestions(provider, *retry_after, message)
            }
            Self::ProviderUnavailable { provider, message } => {
                suggestions::provider_unavailable_suggestions(provider, message)
            }
            Self::ProviderApiError { provider, status_code, message } => {
                suggestions::provider_api_error_suggestions(provider, *status_code, message)
            }
            Self::FetchFailed { provider, reason } => {
                suggestions::fetch_failed_suggestions(provider, reason)
            }
            Self::NoAvailableStrategy(provider) => {
                suggestions::no_strategy_suggestions(provider)
            }
            Self::ParseResponse(msg) => {
                vec![FixSuggestion::new(
                    vec!["caut doctor".to_string()],
                    format!("Failed to parse provider response: {}. This may indicate an API change.", msg),
                )]
            }
            Self::MissingRateLimit => {
                vec![FixSuggestion::new(
                    vec!["caut doctor".to_string()],
                    "Rate limit data was missing from the response. The provider API may have changed.",
                )]
            }
            Self::PartialFailure { failed } => {
                vec![FixSuggestion::new(
                    vec!["caut doctor".to_string()],
                    format!("{} provider(s) failed. Run diagnostics to identify issues.", failed),
                )]
            }

            // Environment errors
            Self::CliNotFound { name } => {
                suggestions::cli_not_found_suggestions(name)
            }
            Self::ProviderNotFound(name) => {
                suggestions::cli_not_found_suggestions(name)
            }
            Self::PermissionDenied { path } => {
                suggestions::permission_denied_suggestions(path)
            }
            Self::EnvVarMissing { name } => {
                suggestions::env_var_missing_suggestions(name)
            }

            // Internal errors - generic suggestions
            Self::Io(err) => {
                vec![FixSuggestion::new(
                    vec!["# Check file permissions and disk space".to_string()],
                    format!("I/O error: {}. Check file permissions and available disk space.", err),
                )]
            }
            Self::Json(err) => {
                vec![FixSuggestion::new(
                    vec!["caut doctor".to_string()],
                    format!("JSON parsing error: {}. The data may be corrupted or in an unexpected format.", err),
                )]
            }
            Self::Other(err) => {
                vec![FixSuggestion::new(
                    vec!["caut doctor".to_string()],
                    format!("Unexpected error: {}. Please report this issue.", err),
                )]
            }
        }
    }
}

/// Result type alias for caut operations.
pub type Result<T> = std::result::Result<T, CautError>;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // ErrorCategory tests
    // -------------------------------------------------------------------------

    #[test]
    fn error_category_description() {
        assert_eq!(ErrorCategory::Authentication.description(), "Authentication error");
        assert_eq!(ErrorCategory::Network.description(), "Network error");
        assert_eq!(ErrorCategory::Configuration.description(), "Configuration error");
        assert_eq!(ErrorCategory::Provider.description(), "Provider error");
        assert_eq!(ErrorCategory::Environment.description(), "Environment error");
        assert_eq!(ErrorCategory::Internal.description(), "Internal error");
    }

    #[test]
    fn error_category_code_prefix() {
        assert_eq!(ErrorCategory::Authentication.code_prefix(), "A");
        assert_eq!(ErrorCategory::Network.code_prefix(), "N");
        assert_eq!(ErrorCategory::Configuration.code_prefix(), "C");
        assert_eq!(ErrorCategory::Provider.code_prefix(), "P");
        assert_eq!(ErrorCategory::Environment.code_prefix(), "E");
        assert_eq!(ErrorCategory::Internal.code_prefix(), "X");
    }

    #[test]
    fn error_category_display() {
        assert_eq!(format!("{}", ErrorCategory::Authentication), "Authentication error");
        assert_eq!(format!("{}", ErrorCategory::Network), "Network error");
    }

    // -------------------------------------------------------------------------
    // CautError category tests
    // -------------------------------------------------------------------------

    #[test]
    fn authentication_errors_have_correct_category() {
        let err = CautError::AuthExpired { provider: "claude".to_string() };
        assert_eq!(err.category(), ErrorCategory::Authentication);

        let err = CautError::AuthNotConfigured { provider: "codex".to_string() };
        assert_eq!(err.category(), ErrorCategory::Authentication);

        let err = CautError::AuthInvalid {
            provider: "gemini".to_string(),
            reason: "Token expired".to_string(),
        };
        assert_eq!(err.category(), ErrorCategory::Authentication);
    }

    #[test]
    fn network_errors_have_correct_category() {
        let err = CautError::Timeout(30);
        assert_eq!(err.category(), ErrorCategory::Network);

        let err = CautError::TimeoutWithProvider {
            provider: "claude".to_string(),
            seconds: 30,
        };
        assert_eq!(err.category(), ErrorCategory::Network);

        let err = CautError::DnsFailure { host: "api.example.com".to_string() };
        assert_eq!(err.category(), ErrorCategory::Network);

        let err = CautError::Network("Connection reset".to_string());
        assert_eq!(err.category(), ErrorCategory::Network);
    }

    #[test]
    fn configuration_errors_have_correct_category() {
        let err = CautError::Config("Invalid setting".to_string());
        assert_eq!(err.category(), ErrorCategory::Configuration);

        let err = CautError::ConfigNotFound { path: "/etc/caut/config.toml".to_string() };
        assert_eq!(err.category(), ErrorCategory::Configuration);

        let err = CautError::InvalidProvider("unknown".to_string());
        assert_eq!(err.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn provider_errors_have_correct_category() {
        let err = CautError::RateLimited {
            provider: "claude".to_string(),
            retry_after: Some(Duration::from_secs(60)),
            message: "Too many requests".to_string(),
        };
        assert_eq!(err.category(), ErrorCategory::Provider);

        let err = CautError::FetchFailed {
            provider: "codex".to_string(),
            reason: "API error".to_string(),
        };
        assert_eq!(err.category(), ErrorCategory::Provider);
    }

    #[test]
    fn environment_errors_have_correct_category() {
        let err = CautError::CliNotFound { name: "codex".to_string() };
        assert_eq!(err.category(), ErrorCategory::Environment);

        let err = CautError::ProviderNotFound("claude".to_string());
        assert_eq!(err.category(), ErrorCategory::Environment);

        let err = CautError::PermissionDenied { path: "/etc/secret".to_string() };
        assert_eq!(err.category(), ErrorCategory::Environment);
    }

    #[test]
    fn internal_errors_have_correct_category() {
        let err = CautError::Json(serde_json::from_str::<()>("invalid").unwrap_err());
        assert_eq!(err.category(), ErrorCategory::Internal);

        let err = CautError::Other(anyhow::anyhow!("Unexpected error"));
        assert_eq!(err.category(), ErrorCategory::Internal);
    }

    // -------------------------------------------------------------------------
    // Error code tests
    // -------------------------------------------------------------------------

    #[test]
    fn error_codes_follow_format() {
        // All error codes should start with "CAUT-"
        let errors: Vec<CautError> = vec![
            CautError::AuthExpired { provider: "test".to_string() },
            CautError::Timeout(30),
            CautError::Config("test".to_string()),
            CautError::RateLimited {
                provider: "test".to_string(),
                retry_after: None,
                message: "test".to_string(),
            },
            CautError::CliNotFound { name: "test".to_string() },
        ];

        for err in errors {
            let code = err.error_code();
            assert!(code.starts_with("CAUT-"), "Error code {} should start with CAUT-", code);
            assert!(code.len() >= 9, "Error code {} should be at least 9 chars", code);
        }
    }

    #[test]
    fn error_codes_are_unique() {
        use std::collections::HashSet;

        let codes: Vec<&str> = vec![
            CautError::AuthExpired { provider: "".to_string() }.error_code(),
            CautError::AuthNotConfigured { provider: "".to_string() }.error_code(),
            CautError::AuthInvalid { provider: "".to_string(), reason: "".to_string() }.error_code(),
            CautError::Timeout(0).error_code(),
            CautError::TimeoutWithProvider { provider: "".to_string(), seconds: 0 }.error_code(),
            CautError::DnsFailure { host: "".to_string() }.error_code(),
            CautError::SslError { message: "".to_string() }.error_code(),
            CautError::ConnectionRefused { host: "".to_string() }.error_code(),
            CautError::Network("".to_string()).error_code(),
            CautError::ConfigNotFound { path: "".to_string() }.error_code(),
            CautError::ConfigParse { path: "".to_string(), line: None, message: "".to_string() }.error_code(),
            CautError::ConfigInvalid { key: "".to_string(), value: "".to_string(), message: "".to_string() }.error_code(),
            CautError::Config("".to_string()).error_code(),
            CautError::InvalidProvider("".to_string()).error_code(),
            CautError::RateLimited { provider: "".to_string(), retry_after: None, message: "".to_string() }.error_code(),
            CautError::ProviderUnavailable { provider: "".to_string(), message: "".to_string() }.error_code(),
            CautError::CliNotFound { name: "".to_string() }.error_code(),
            CautError::ProviderNotFound("".to_string()).error_code(),
        ];

        let unique: HashSet<_> = codes.iter().collect();
        assert_eq!(codes.len(), unique.len(), "Error codes should be unique");
    }

    // -------------------------------------------------------------------------
    // Retryable tests
    // -------------------------------------------------------------------------

    #[test]
    fn retryable_errors() {
        // These should be retryable
        assert!(CautError::Timeout(30).is_retryable());
        assert!(CautError::TimeoutWithProvider {
            provider: "test".to_string(),
            seconds: 30,
        }.is_retryable());
        assert!(CautError::Network("reset".to_string()).is_retryable());
        assert!(CautError::RateLimited {
            provider: "test".to_string(),
            retry_after: None,
            message: "".to_string(),
        }.is_retryable());
        assert!(CautError::ProviderUnavailable {
            provider: "test".to_string(),
            message: "".to_string(),
        }.is_retryable());
    }

    #[test]
    fn non_retryable_errors() {
        // These should NOT be retryable
        assert!(!CautError::Config("test".to_string()).is_retryable());
        assert!(!CautError::InvalidProvider("test".to_string()).is_retryable());
        assert!(!CautError::AuthInvalid {
            provider: "test".to_string(),
            reason: "bad token".to_string(),
        }.is_retryable());
        assert!(!CautError::CliNotFound { name: "test".to_string() }.is_retryable());
    }

    // -------------------------------------------------------------------------
    // Retry-after tests
    // -------------------------------------------------------------------------

    #[test]
    fn retry_after_returns_duration() {
        let err = CautError::RateLimited {
            provider: "claude".to_string(),
            retry_after: Some(Duration::from_secs(60)),
            message: "Too many requests".to_string(),
        };
        assert_eq!(err.retry_after(), Some(Duration::from_secs(60)));
    }

    #[test]
    fn retry_after_returns_none_for_other_errors() {
        let err = CautError::Timeout(30);
        assert_eq!(err.retry_after(), None);

        let err = CautError::Config("test".to_string());
        assert_eq!(err.retry_after(), None);
    }

    // -------------------------------------------------------------------------
    // Provider extraction tests
    // -------------------------------------------------------------------------

    #[test]
    fn provider_extraction() {
        let err = CautError::AuthExpired { provider: "claude".to_string() };
        assert_eq!(err.provider(), Some("claude"));

        let err = CautError::FetchFailed {
            provider: "codex".to_string(),
            reason: "test".to_string(),
        };
        assert_eq!(err.provider(), Some("codex"));

        let err = CautError::ProviderNotFound("gemini".to_string());
        assert_eq!(err.provider(), Some("gemini"));
    }

    #[test]
    fn provider_returns_none_for_non_provider_errors() {
        let err = CautError::Timeout(30);
        assert_eq!(err.provider(), None);

        let err = CautError::Network("test".to_string());
        assert_eq!(err.provider(), None);
    }

    // -------------------------------------------------------------------------
    // Exit code tests
    // -------------------------------------------------------------------------

    #[test]
    fn exit_codes_are_correct() {
        assert_eq!(CautError::ProviderNotFound("test".to_string()).exit_code(), ExitCode::BinaryNotFound);
        assert_eq!(CautError::CliNotFound { name: "test".to_string() }.exit_code(), ExitCode::BinaryNotFound);

        assert_eq!(CautError::Config("test".to_string()).exit_code(), ExitCode::ParseError);
        assert_eq!(CautError::InvalidProvider("test".to_string()).exit_code(), ExitCode::ParseError);

        assert_eq!(CautError::Timeout(30).exit_code(), ExitCode::Timeout);
        assert_eq!(CautError::TimeoutWithProvider {
            provider: "test".to_string(),
            seconds: 30,
        }.exit_code(), ExitCode::Timeout);

        assert_eq!(CautError::Network("test".to_string()).exit_code(), ExitCode::GeneralError);
    }

    // -------------------------------------------------------------------------
    // Fix suggestion tests
    // -------------------------------------------------------------------------

    #[test]
    fn all_error_variants_have_suggestions() {
        // Every error should have at least one suggestion
        let errors: Vec<CautError> = vec![
            CautError::AuthExpired { provider: "claude".to_string() },
            CautError::AuthNotConfigured { provider: "codex".to_string() },
            CautError::AuthInvalid { provider: "gemini".to_string(), reason: "invalid".to_string() },
            CautError::Timeout(30),
            CautError::TimeoutWithProvider { provider: "claude".to_string(), seconds: 30 },
            CautError::DnsFailure { host: "api.example.com".to_string() },
            CautError::SslError { message: "certificate error".to_string() },
            CautError::ConnectionRefused { host: "localhost:8080".to_string() },
            CautError::Network("reset".to_string()),
            CautError::ConfigNotFound { path: "/etc/caut/config.toml".to_string() },
            CautError::ConfigParse { path: "config.toml".to_string(), line: Some(10), message: "syntax error".to_string() },
            CautError::ConfigInvalid { key: "timeout".to_string(), value: "abc".to_string(), message: "must be number".to_string() },
            CautError::Config("invalid".to_string()),
            CautError::InvalidProvider("unknown".to_string()),
            CautError::UnsupportedSource { provider: "claude".to_string(), source_type: "ftp".to_string() },
            CautError::AccountRequiresSingleProvider,
            CautError::AllAccountsConflict,
            CautError::ProviderNoTokenAccounts("codex".to_string()),
            CautError::AccountNotFound("work".to_string()),
            CautError::NoAccountsConfigured("claude".to_string()),
            CautError::RateLimited { provider: "claude".to_string(), retry_after: Some(Duration::from_secs(60)), message: "too many".to_string() },
            CautError::ProviderUnavailable { provider: "codex".to_string(), message: "down".to_string() },
            CautError::ProviderApiError { provider: "claude".to_string(), status_code: Some(500), message: "error".to_string() },
            CautError::FetchFailed { provider: "gemini".to_string(), reason: "network".to_string() },
            CautError::NoAvailableStrategy("claude".to_string()),
            CautError::ParseResponse("unexpected".to_string()),
            CautError::MissingRateLimit,
            CautError::PartialFailure { failed: 2 },
            CautError::CliNotFound { name: "claude".to_string() },
            CautError::ProviderNotFound("codex".to_string()),
            CautError::PermissionDenied { path: "/etc/secret".to_string() },
            CautError::EnvVarMissing { name: "CLAUDE_API_KEY".to_string() },
        ];

        for err in errors {
            let suggestions = err.fix_suggestions();
            assert!(
                !suggestions.is_empty(),
                "Error {:?} should have at least one suggestion",
                err
            );
            assert!(
                !suggestions[0].context.is_empty(),
                "Error {:?} suggestion should have context",
                err
            );
        }
    }

    #[test]
    fn suggestions_include_provider_in_commands() {
        let err = CautError::AuthExpired { provider: "claude".to_string() };
        let suggestions = err.fix_suggestions();

        assert!(!suggestions.is_empty());
        // At least one command should mention the provider
        let has_provider_cmd = suggestions[0].commands.iter().any(|c| c.contains("claude"));
        assert!(has_provider_cmd, "Suggestions should include provider-specific commands");
    }

    #[test]
    fn cli_not_found_has_install_commands() {
        let err = CautError::CliNotFound { name: "claude".to_string() };
        let suggestions = err.fix_suggestions();

        assert!(!suggestions.is_empty());
        // Should have npm install or similar
        let has_install = suggestions[0].commands.iter().any(|c| c.contains("install"));
        assert!(has_install, "CliNotFound should have install commands");
    }

    #[test]
    fn rate_limited_includes_retry_info() {
        let err = CautError::RateLimited {
            provider: "claude".to_string(),
            retry_after: Some(Duration::from_secs(120)),
            message: "Too many requests".to_string(),
        };
        let suggestions = err.fix_suggestions();

        assert!(!suggestions.is_empty());
        // Context should mention retry time
        assert!(
            suggestions[0].context.contains("120"),
            "Rate limited suggestion should mention retry time"
        );
    }

    #[test]
    fn timeout_includes_duration() {
        let err = CautError::TimeoutWithProvider {
            provider: "codex".to_string(),
            seconds: 45,
        };
        let suggestions = err.fix_suggestions();

        assert!(!suggestions.is_empty());
        assert!(
            suggestions[0].context.contains("45"),
            "Timeout suggestion should mention duration"
        );
    }

    #[test]
    fn config_parse_includes_line_number() {
        let err = CautError::ConfigParse {
            path: "config.toml".to_string(),
            line: Some(42),
            message: "unexpected token".to_string(),
        };
        let suggestions = err.fix_suggestions();

        assert!(!suggestions.is_empty());
        assert!(
            suggestions[0].context.contains("42"),
            "Config parse suggestion should mention line number"
        );
    }

    #[test]
    fn suggestions_have_prevention_tips_where_appropriate() {
        // Auth expired should have prevention tips
        let err = CautError::AuthExpired { provider: "claude".to_string() };
        let suggestions = err.fix_suggestions();

        assert!(!suggestions.is_empty());
        assert!(
            suggestions[0].prevention.is_some(),
            "Auth expired should have prevention tips"
        );
    }
}
