//! Core data models ported from CodexBar.
//!
//! These types represent the canonical usage data structures.
//! See EXISTING_CODEXBAR_STRUCTURE.md section 5 for field semantics.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// =============================================================================
// Rate Window
// =============================================================================

/// A rate-limited usage window (session, weekly, etc.).
///
/// # Fields
/// - `used_percent`: Percentage of the window consumed (0-100).
/// - `window_minutes`: Duration of the window in minutes (if known).
/// - `resets_at`: When the window resets (if known).
/// - `reset_description`: Human-readable reset description (e.g., "in 2 hours").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateWindow {
    pub used_percent: f64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_description: Option<String>,
}

impl RateWindow {
    /// Percentage remaining in this window.
    #[must_use]
    pub fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).max(0.0)
    }

    /// Create a new rate window with the given usage percentage.
    #[must_use]
    pub fn new(used_percent: f64) -> Self {
        Self {
            used_percent,
            window_minutes: None,
            resets_at: None,
            reset_description: None,
        }
    }
}

// =============================================================================
// Provider Identity
// =============================================================================

/// Identity information for a provider account.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_email: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_organization: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_method: Option<String>,
}

// =============================================================================
// Usage Snapshot
// =============================================================================

/// Complete usage snapshot for a provider.
///
/// Contains primary (session), secondary (weekly), and optionally tertiary
/// (Opus/Sonnet tier) rate windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    /// Primary rate window (usually session-based).
    pub primary: Option<RateWindow>,

    /// Secondary rate window (usually weekly).
    pub secondary: Option<RateWindow>,

    /// Tertiary rate window (Opus/Sonnet tier for Claude).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tertiary: Option<RateWindow>,

    /// When this snapshot was captured.
    pub updated_at: DateTime<Utc>,

    /// Account identity information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<ProviderIdentity>,
}

impl UsageSnapshot {
    /// Create a new usage snapshot with only primary window.
    #[must_use]
    pub fn new(primary: RateWindow) -> Self {
        Self {
            primary: Some(primary),
            secondary: None,
            tertiary: None,
            updated_at: Utc::now(),
            identity: None,
        }
    }
}

// =============================================================================
// Credits
// =============================================================================

/// A credit balance event (purchase, usage, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreditEvent {
    pub amount: f64,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Credits snapshot for providers that support credit balances (Codex).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsSnapshot {
    /// Remaining credit balance.
    pub remaining: f64,

    /// Recent credit events.
    #[serde(default)]
    pub events: Vec<CreditEvent>,

    /// When this snapshot was captured.
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// OpenAI Dashboard (Codex-specific)
// =============================================================================

/// Daily breakdown from OpenAI dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIDashboardDailyBreakdown {
    pub date: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i64>,
}

/// Extended dashboard data from OpenAI web interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIDashboardSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_in_email: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_review_remaining_percent: Option<f64>,

    #[serde(default)]
    pub credit_events: Vec<CreditEvent>,

    #[serde(default)]
    pub daily_breakdown: Vec<OpenAIDashboardDailyBreakdown>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_purchase_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_limit: Option<RateWindow>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_limit: Option<RateWindow>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_remaining: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_plan: Option<String>,

    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Status
// =============================================================================

/// Status indicator from provider status pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StatusIndicator {
    #[default]
    None,
    Minor,
    Major,
    Critical,
    Maintenance,
    Unknown,
}

impl StatusIndicator {
    /// Parse from statuspage.io indicator string.
    #[must_use]
    pub fn from_statuspage(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "none" | "operational" => Self::None,
            "minor" => Self::Minor,
            "major" => Self::Major,
            "critical" => Self::Critical,
            "maintenance" | "under_maintenance" => Self::Maintenance,
            _ => Self::Unknown,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "Operational",
            Self::Minor => "Minor Issue",
            Self::Major => "Major Issue",
            Self::Critical => "Critical",
            Self::Maintenance => "Maintenance",
            Self::Unknown => "Unknown",
        }
    }
}

/// Provider status payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub indicator: StatusIndicator,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,

    pub url: String,
}

// =============================================================================
// Provider Payload (JSON output)
// =============================================================================

/// Complete provider payload for JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPayload {
    pub provider: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    pub source: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusPayload>,

    pub usage: UsageSnapshot,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<CreditsSnapshot>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "antigravityPlanInfo")]
    pub antigravity_plan_info: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "openaiDashboard")]
    pub openai_dashboard: Option<OpenAIDashboardSnapshot>,

    /// Authentication health warning message (if credentials need attention).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_warning: Option<String>,
}

// =============================================================================
// Cost Usage Models
// =============================================================================

/// Daily cost entry for local cost scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostDailyEntry {
    pub date: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_used: Option<Vec<String>>,
}

/// Aggregated cost totals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostTotals {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,
}

/// Cost payload for the `cost` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostPayload {
    pub provider: String,
    pub source: String,
    pub updated_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_cost_usd: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_30_days_tokens: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_30_days_cost_usd: Option<f64>,

    #[serde(default)]
    pub daily: Vec<CostDailyEntry>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<CostTotals>,
}

// =============================================================================
// Robot Output Envelope
// =============================================================================

/// Serializable fix suggestion for robot output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixSuggestionReport {
    pub commands: Vec<String>,
    pub context: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prevention: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_url: Option<String>,

    pub auto_fixable: bool,
}

/// Report for a single strategy attempt during fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyAttemptReport {
    pub strategy_id: String,
    pub kind: String,
    pub duration_ms: u64,
    pub success: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Structured error report for a provider failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderErrorReport {
    pub provider: String,
    pub final_error: String,
    pub error_code: String,
    pub retryable: bool,
    pub attempts: Vec<StrategyAttemptReport>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<FixSuggestionReport>,
}

/// Top-level JSON envelope for robot mode output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotOutput<T> {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub command: String,
    pub data: T,

    #[serde(default)]
    pub errors: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_details: Option<Vec<ProviderErrorReport>>,

    pub meta: RobotMeta,
}

/// Metadata for robot output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotMeta {
    pub format: String,
    pub flags: Vec<String>,
    pub runtime: String,
}

impl<T> RobotOutput<T> {
    /// Create a new robot output envelope.
    pub fn new(command: impl Into<String>, data: T) -> Self {
        Self {
            schema_version: "caut.v1".to_string(),
            generated_at: Utc::now(),
            command: command.into(),
            data,
            errors: Vec::new(),
            error_details: None,
            meta: RobotMeta {
                format: "json".to_string(),
                flags: Vec::new(),
                runtime: "cli".to_string(),
            },
        }
    }

    /// Create with errors.
    pub fn with_errors(command: impl Into<String>, data: T, errors: Vec<String>) -> Self {
        Self {
            schema_version: "caut.v1".to_string(),
            generated_at: Utc::now(),
            command: command.into(),
            data,
            errors,
            error_details: None,
            meta: RobotMeta {
                format: "json".to_string(),
                flags: Vec::new(),
                runtime: "cli".to_string(),
            },
        }
    }

    /// Create with errors and structured error details.
    pub fn with_errors_and_details(
        command: impl Into<String>,
        data: T,
        errors: Vec<String>,
        error_details: Option<Vec<ProviderErrorReport>>,
    ) -> Self {
        Self {
            schema_version: "caut.v1".to_string(),
            generated_at: Utc::now(),
            command: command.into(),
            data,
            errors,
            error_details,
            meta: RobotMeta {
                format: "json".to_string(),
                flags: Vec::new(),
                runtime: "cli".to_string(),
            },
        }
    }
}

impl RobotOutput<Vec<ProviderPayload>> {
    /// Create a usage output envelope.
    pub fn usage(providers: Vec<ProviderPayload>, errors: Vec<String>) -> Self {
        Self::with_errors("usage", providers, errors)
    }

    /// Create a usage output envelope with structured error details.
    pub fn usage_with_details(
        providers: Vec<ProviderPayload>,
        errors: Vec<String>,
        error_details: Vec<ProviderErrorReport>,
    ) -> Self {
        let details = if error_details.is_empty() {
            None
        } else {
            Some(error_details)
        };

        Self::with_errors_and_details("usage", providers, errors, details)
    }
}

impl RobotOutput<Vec<CostPayload>> {
    /// Create a cost output envelope.
    pub fn cost(providers: Vec<CostPayload>, errors: Vec<String>) -> Self {
        Self::with_errors("cost", providers, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        make_test_credits_snapshot, make_test_provider_payload, make_test_rate_window,
        make_test_status_payload, make_test_usage_snapshot, make_test_usage_snapshot_with_tertiary,
    };
    use crate::{assert_contains, assert_float_eq, assert_json_valid};

    #[test]
    fn rate_window_remaining() {
        let window = RateWindow::new(30.0);
        assert_float_eq!(window.remaining_percent(), 70.0);
    }

    #[test]
    fn rate_window_from_test_utils() {
        let window = make_test_rate_window(28.0);
        assert_float_eq!(window.used_percent, 28.0);
        assert_float_eq!(window.remaining_percent(), 72.0);
        assert!(window.window_minutes.is_some());
        assert!(window.resets_at.is_some());
        assert!(window.reset_description.is_some());
    }

    #[test]
    fn usage_snapshot_from_test_utils() {
        let snapshot = make_test_usage_snapshot();
        assert!(snapshot.primary.is_some());
        assert!(snapshot.secondary.is_some());
        assert!(snapshot.tertiary.is_none());
        assert!(snapshot.identity.is_some());

        let identity = snapshot.identity.as_ref().unwrap();
        assert!(identity.account_email.is_some());
    }

    #[test]
    fn usage_snapshot_with_tertiary_from_test_utils() {
        let snapshot = make_test_usage_snapshot_with_tertiary();
        assert!(snapshot.primary.is_some());
        assert!(snapshot.secondary.is_some());
        assert!(snapshot.tertiary.is_some());
    }

    #[test]
    fn credits_snapshot_from_test_utils() {
        let credits = make_test_credits_snapshot(112.50);
        assert_float_eq!(credits.remaining, 112.50);
        assert!(!credits.events.is_empty());
    }

    #[test]
    fn status_payload_from_test_utils() {
        let status = make_test_status_payload(StatusIndicator::Minor);
        assert_eq!(status.indicator, StatusIndicator::Minor);
        assert!(status.description.is_some());
    }

    #[test]
    fn status_indicator_parse() {
        assert_eq!(
            StatusIndicator::from_statuspage("none"),
            StatusIndicator::None
        );
        assert_eq!(
            StatusIndicator::from_statuspage("operational"),
            StatusIndicator::None
        );
        assert_eq!(
            StatusIndicator::from_statuspage("minor"),
            StatusIndicator::Minor
        );
        assert_eq!(
            StatusIndicator::from_statuspage("garbage"),
            StatusIndicator::Unknown
        );
    }

    #[test]
    fn robot_output_serializes() {
        let output = RobotOutput::new("usage", vec!["test"]);
        let json = serde_json::to_string(&output).unwrap();
        assert_json_valid!(&json);
        assert_contains!(&json, "caut.v1");
    }

    #[test]
    fn provider_payload_from_test_utils() {
        let payload = make_test_provider_payload("codex", "cli");
        assert_eq!(payload.provider, "codex");
        assert_eq!(payload.source, "cli");
        // Codex should have credits
        assert!(payload.credits.is_some());

        let claude_payload = make_test_provider_payload("claude", "oauth");
        assert_eq!(claude_payload.provider, "claude");
        // Claude should not have credits
        assert!(claude_payload.credits.is_none());
    }

    #[test]
    fn provider_payload_serializes() {
        let payload = make_test_provider_payload("test", "test-source");
        let json = serde_json::to_string(&payload).unwrap();
        assert_json_valid!(&json);
        assert_contains!(&json, "test");
        assert_contains!(&json, "test-source");
    }
}
