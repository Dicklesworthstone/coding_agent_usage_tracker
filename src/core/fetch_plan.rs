//! Provider fetch pipeline and strategies.
//!
//! Implements the ordered strategy fallback system from `CodexBar`.
//! See `EXISTING_CODEXBAR_STRUCTURE.md` section 6.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};

use super::models::{CreditsSnapshot, UsageSnapshot};
use super::provider::Provider;
use crate::error::Result;

// =============================================================================
// Source Mode
// =============================================================================

/// Data source mode for fetching provider data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceMode {
    /// Automatically select best available source.
    #[default]
    Auto,
    /// Web scraping / cookies.
    Web,
    /// CLI tool / RPC.
    Cli,
    /// OAuth API.
    OAuth,
    /// Direct API with a token / API key.
    Api,
    /// Local files or a local probe (no network credentials).
    Local,
}

impl SourceMode {
    /// Every accepted `--source` value, for error messages.
    pub const ARG_VALUES: &'static str = "auto, web, cli, oauth, api, local";

    /// Parse from CLI argument.
    #[must_use]
    pub fn from_arg(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "web" => Some(Self::Web),
            "cli" => Some(Self::Cli),
            "oauth" => Some(Self::OAuth),
            "api" => Some(Self::Api),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

// =============================================================================
// Fetch Context
// =============================================================================

/// Per-fetch inputs that are not baked into a provider's strategy table.
///
/// The usage command builds one of these per provider (or per token account
/// with `--all-accounts`) so a strategy can prefer an explicitly selected
/// credential over whatever it would discover on its own.
#[derive(Debug, Clone, Default)]
pub struct FetchContext {
    /// Credential from a selected token account: an API key, a cookie header,
    /// or an OAuth access token, depending on the provider.
    pub account_token: Option<String>,
    /// Label of the selected token account, echoed into the payload.
    pub account_label: Option<String>,
    /// API base URL override from the config file (`providers.<name>.api_base`).
    pub api_base: Option<String>,
}

impl FetchContext {
    /// Context carrying a token-account credential.
    #[must_use]
    pub fn with_account(label: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            account_token: Some(token.into()),
            account_label: Some(label.into()),
            api_base: None,
        }
    }

    /// The token-account credential, trimmed, if one was selected and is non-empty.
    #[must_use]
    pub fn account_token(&self) -> Option<&str> {
        self.account_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }
}

// =============================================================================
// Fetch Kind
// =============================================================================

/// Kind of fetch strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    /// CLI tool (e.g., `codex`, `claude`).
    Cli,
    /// Web scraping with cookies.
    Web,
    /// OAuth API.
    OAuth,
    /// API with token.
    ApiToken,
    /// Local file probe.
    LocalProbe,
    /// Web dashboard scraping.
    WebDashboard,
}

impl FetchKind {
    /// Source label for output.
    #[must_use]
    pub const fn source_label(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Web => "web",
            Self::OAuth => "oauth",
            Self::ApiToken => "api",
            Self::LocalProbe => "local",
            Self::WebDashboard => "web-dashboard",
        }
    }
}

// =============================================================================
// Fetch Strategy
// =============================================================================

/// A fetch strategy for a provider.
pub struct FetchStrategy {
    /// Unique ID for this strategy.
    pub id: &'static str,
    /// Kind of fetch.
    pub kind: FetchKind,
    /// Check if this strategy is available for the given context.
    pub is_available: fn(&FetchContext) -> bool,
    /// Whether to fallback on error.
    pub should_fallback: fn(&crate::error::CautError) -> bool,
}

impl std::fmt::Debug for FetchStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchStrategy")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

// =============================================================================
// Fetch Attempt
// =============================================================================

/// Record of a single fetch attempt.
#[derive(Debug, Clone)]
pub struct FetchAttempt {
    pub strategy_id: String,
    pub kind: FetchKind,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

// =============================================================================
// Provider Fetch Result
// =============================================================================

/// What one strategy run produced: the usage snapshot plus any data that
/// does not belong on it, such as a Codex credit balance.
#[derive(Debug, Clone)]
pub struct ProviderFetch {
    pub usage: UsageSnapshot,
    pub credits: Option<CreditsSnapshot>,
}

impl From<UsageSnapshot> for ProviderFetch {
    fn from(usage: UsageSnapshot) -> Self {
        Self {
            usage,
            credits: None,
        }
    }
}

// =============================================================================
// Fetch Outcome
// =============================================================================

/// Result of a fetch pipeline execution.
#[derive(Debug)]
pub struct FetchOutcome {
    pub provider: Provider,
    pub result: Result<UsageSnapshot>,
    /// Credit balance reported alongside the usage (Codex), if any.
    pub credits: Option<CreditsSnapshot>,
    pub attempts: Vec<FetchAttempt>,
    pub source_label: String,
}

impl FetchOutcome {
    /// Create a successful outcome.
    #[must_use]
    pub fn success(
        provider: Provider,
        snapshot: UsageSnapshot,
        source: &str,
        attempts: Vec<FetchAttempt>,
    ) -> Self {
        Self {
            provider,
            result: Ok(snapshot),
            credits: None,
            attempts,
            source_label: source.to_string(),
        }
    }

    /// Attach a credit balance to a successful outcome.
    #[must_use]
    pub fn with_credits(mut self, credits: Option<CreditsSnapshot>) -> Self {
        self.credits = credits;
        self
    }

    /// Create a failed outcome.
    #[must_use]
    pub const fn failure(
        provider: Provider,
        error: crate::error::CautError,
        attempts: Vec<FetchAttempt>,
    ) -> Self {
        Self {
            provider,
            result: Err(error),
            credits: None,
            attempts,
            source_label: String::new(),
        }
    }

    /// Whether the fetch succeeded.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.result.is_ok()
    }
}

// =============================================================================
// Fetch Plan
// =============================================================================

/// Ordered list of strategies to try for a provider.
#[derive(Debug)]
pub struct FetchPlan {
    pub provider: Provider,
    pub strategies: Vec<FetchStrategy>,
}

impl FetchPlan {
    /// Create a new fetch plan.
    #[must_use]
    pub const fn new(provider: Provider, strategies: Vec<FetchStrategy>) -> Self {
        Self {
            provider,
            strategies,
        }
    }

    /// Filter strategies by source mode.
    #[must_use]
    pub fn for_mode(&self, mode: SourceMode) -> Vec<&FetchStrategy> {
        match mode {
            SourceMode::Auto => self.strategies.iter().collect(),
            SourceMode::Web => self
                .strategies
                .iter()
                .filter(|s| matches!(s.kind, FetchKind::Web | FetchKind::WebDashboard))
                .collect(),
            SourceMode::Cli => self
                .strategies
                .iter()
                .filter(|s| s.kind == FetchKind::Cli)
                .collect(),
            SourceMode::OAuth => self
                .strategies
                .iter()
                .filter(|s| s.kind == FetchKind::OAuth)
                .collect(),
            SourceMode::Api => self
                .strategies
                .iter()
                .filter(|s| s.kind == FetchKind::ApiToken)
                .collect(),
            SourceMode::Local => self
                .strategies
                .iter()
                .filter(|s| s.kind == FetchKind::LocalProbe)
                .collect(),
        }
    }
}

/// Type alias for async fetch function.
pub type FetchFn =
    Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<UsageSnapshot>> + Send>> + Send + Sync>;
