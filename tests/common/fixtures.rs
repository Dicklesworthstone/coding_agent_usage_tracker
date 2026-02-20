//! Test fixtures and factory functions for integration tests.
//!
//! Provides reusable test data generators and fixture loaders for testing
//! caut functionality across providers.
//!
//! # Usage
//!
//! ```rust,ignore
//! use common::fixtures::*;
//!
//! // Load a JSON fixture
//! let creds: serde_json::Value = load_fixture("claude/credentials_full.json");
//!
//! // Create test objects with factories
//! let usage = usage_snapshot(30.0, Some(45.0));
//! let status = status_payload(StatusIndicator::None, "All Systems Operational");
//! ```

use std::fs;
use std::path::PathBuf;

use chrono::{TimeDelta, Utc};
use serde::de::DeserializeOwned;

// Re-export types we use in factories
pub use caut::core::models::{
    CostDailyEntry, CostPayload, CostTotals, CreditEvent, CreditsSnapshot, ProviderIdentity,
    ProviderPayload, RateWindow, StatusIndicator, StatusPayload, UsageSnapshot,
};

// =============================================================================
// Fixture Loading
// =============================================================================

/// Get the path to the fixtures directory.
fn fixtures_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("tests/fixtures")
}

/// Load a JSON fixture file and deserialize it.
///
/// # Arguments
///
/// * `path` - Relative path within the `tests/fixtures/` directory
///
/// # Panics
///
/// Panics if the file cannot be read or parsed.
///
/// # Examples
///
/// ```rust,ignore
/// use common::fixtures::load_fixture;
///
/// let creds: serde_json::Value = load_fixture("claude/credentials_full.json");
/// ```
pub fn load_fixture<T: DeserializeOwned>(path: &str) -> T {
    let full_path = fixtures_dir().join(path);
    let content = fs::read_to_string(&full_path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", full_path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse fixture {}: {}", full_path.display(), e))
}

/// Load a text fixture file as a string.
///
/// # Arguments
///
/// * `path` - Relative path within the `tests/fixtures/` directory
///
/// # Panics
///
/// Panics if the file cannot be read.
pub fn load_fixture_text(path: &str) -> String {
    let full_path = fixtures_dir().join(path);
    fs::read_to_string(&full_path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", full_path.display(), e))
}

/// Load a raw JSON fixture file as `serde_json::Value`.
///
/// Useful when you need to inspect or modify the JSON dynamically.
#[allow(dead_code)]
pub fn load_fixture_json(path: &str) -> serde_json::Value {
    load_fixture(path)
}

// =============================================================================
// Usage Factories
// =============================================================================

/// Create a `UsageSnapshot` with given primary and optional secondary percentages.
///
/// # Arguments
///
/// * `primary_pct` - Used percentage for the primary rate window (0-100)
/// * `secondary_pct` - Optional used percentage for the secondary rate window
///
/// # Examples
///
/// ```rust,ignore
/// let usage = usage_snapshot(30.0, Some(45.0));
/// assert!(usage.primary.is_some());
/// assert!(usage.secondary.is_some());
/// ```
#[must_use]
pub fn usage_snapshot(primary_pct: f64, secondary_pct: Option<f64>) -> UsageSnapshot {
    UsageSnapshot {
        primary: Some(rate_window(primary_pct, 180)),
        secondary: secondary_pct.map(|pct| rate_window(pct, 10080)),
        tertiary: None,
        updated_at: Utc::now(),
        identity: Some(ProviderIdentity {
            account_email: Some("test@example.com".to_string()),
            account_organization: None,
            login_method: Some("fixture".to_string()),
        }),
    }
}

/// Create a `UsageSnapshot` with all three rate tiers (primary, secondary, tertiary).
///
/// Useful for testing Claude-like providers with Opus/Sonnet tiers.
#[must_use]
pub fn usage_snapshot_full(
    primary_pct: f64,
    secondary_pct: f64,
    tertiary_pct: f64,
) -> UsageSnapshot {
    UsageSnapshot {
        primary: Some(rate_window(primary_pct, 180)),
        secondary: Some(rate_window(secondary_pct, 10080)),
        tertiary: Some(rate_window(tertiary_pct, 10080)),
        updated_at: Utc::now(),
        identity: Some(ProviderIdentity {
            account_email: Some("test@example.com".to_string()),
            account_organization: Some("Test Org".to_string()),
            login_method: Some("oauth".to_string()),
        }),
    }
}

/// Create a minimal `UsageSnapshot` with only primary window.
#[allow(dead_code)]
#[must_use]
pub fn usage_snapshot_minimal(primary_pct: f64) -> UsageSnapshot {
    UsageSnapshot::new(RateWindow::new(primary_pct))
}

/// Create a `RateWindow` with given used percentage and window duration.
///
/// # Arguments
///
/// * `used_pct` - Used percentage (0-100)
/// * `window_minutes` - Duration of the rate window in minutes
#[must_use]
pub fn rate_window(used_pct: f64, window_minutes: i32) -> RateWindow {
    let hours = window_minutes / 60;
    let reset_desc = if hours >= 24 {
        format!("resets in {}d", hours / 24)
    } else if hours > 0 {
        format!("resets in {hours}h")
    } else {
        format!("resets in {window_minutes}m")
    };

    RateWindow {
        used_percent: used_pct,
        window_minutes: Some(window_minutes),
        resets_at: Some(Utc::now() + TimeDelta::minutes(i64::from(window_minutes))),
        reset_description: Some(reset_desc),
    }
}

// =============================================================================
// Provider Factories
// =============================================================================

/// Create a `ProviderPayload` for a given provider with usage data.
///
/// Automatically adds credits for Codex provider.
///
/// # Arguments
///
/// * `provider` - Provider name (e.g., "claude", "codex")
/// * `source` - Source label (e.g., "oauth", "cli")
/// * `usage` - Usage snapshot to include
#[must_use]
pub fn provider_payload(provider: &str, source: &str, usage: UsageSnapshot) -> ProviderPayload {
    let has_credits = provider.to_lowercase() == "codex";

    ProviderPayload {
        provider: provider.to_string(),
        account: Some("fixture@example.com".to_string()),
        version: Some("0.1.0-fixture".to_string()),
        source: source.to_string(),
        status: Some(status_payload(
            StatusIndicator::None,
            "All Systems Operational",
        )),
        usage,
        credits: if has_credits {
            Some(credits_snapshot(100.0))
        } else {
            None
        },
        antigravity_plan_info: None,
        openai_dashboard: None,
        auth_warning: None,
    }
}

/// Create a `ProviderPayload` with default usage (30% primary, 45% secondary).
#[must_use]
pub fn provider_payload_default(provider: &str, source: &str) -> ProviderPayload {
    provider_payload(provider, source, usage_snapshot(30.0, Some(45.0)))
}

// =============================================================================
// Status Factories
// =============================================================================

/// Create a `StatusPayload` with the given indicator and description.
///
/// # Arguments
///
/// * `indicator` - Status indicator (None, Minor, Major, Critical, Maintenance, Unknown)
/// * `description` - Human-readable status description
#[must_use]
pub fn status_payload(indicator: StatusIndicator, description: &str) -> StatusPayload {
    StatusPayload {
        indicator,
        description: Some(description.to_string()),
        updated_at: Some(Utc::now()),
        url: "https://status.example.com".to_string(),
    }
}

/// Create a `StatusPayload` for operational status.
#[must_use]
pub fn status_operational() -> StatusPayload {
    status_payload(StatusIndicator::None, "All Systems Operational")
}

/// Create a `StatusPayload` for minor outage.
#[must_use]
pub fn status_minor() -> StatusPayload {
    status_payload(StatusIndicator::Minor, "Minor Service Disruption")
}

/// Create a `StatusPayload` for major outage.
#[must_use]
pub fn status_major() -> StatusPayload {
    status_payload(StatusIndicator::Major, "Major Service Disruption")
}

/// Create a `StatusPayload` for critical outage.
#[must_use]
pub fn status_critical() -> StatusPayload {
    status_payload(StatusIndicator::Critical, "Critical System Failure")
}

// =============================================================================
// Credits Factories
// =============================================================================

/// Create a `CreditsSnapshot` with the given remaining balance.
///
/// Includes sample credit events for purchase and usage.
#[must_use]
pub fn credits_snapshot(remaining: f64) -> CreditsSnapshot {
    CreditsSnapshot {
        remaining,
        events: vec![
            CreditEvent {
                amount: 100.0,
                event_type: "purchase".to_string(),
                timestamp: Utc::now() - TimeDelta::days(30),
                description: Some("Monthly credit purchase".to_string()),
            },
            CreditEvent {
                amount: -(100.0 - remaining),
                event_type: "usage".to_string(),
                timestamp: Utc::now() - TimeDelta::hours(1),
                description: Some("API usage".to_string()),
            },
        ],
        updated_at: Utc::now(),
    }
}

/// Create a minimal `CreditsSnapshot` with no events.
#[allow(dead_code)]
#[must_use]
pub fn credits_snapshot_minimal(remaining: f64) -> CreditsSnapshot {
    CreditsSnapshot {
        remaining,
        events: Vec::new(),
        updated_at: Utc::now(),
    }
}

// =============================================================================
// Cost Factories
// =============================================================================

/// Create a `CostPayload` with session and monthly totals.
///
/// # Arguments
///
/// * `provider` - Provider name
/// * `session_cost` - Today's session cost in USD
/// * `monthly_cost` - Last 30 days total cost in USD
/// * `session_tokens` - Today's token count
#[must_use]
pub fn cost_payload(
    provider: &str,
    session_cost: f64,
    monthly_cost: f64,
    session_tokens: i64,
) -> CostPayload {
    let today = Utc::now().format("%Y-%m-%d").to_string();

    CostPayload {
        provider: provider.to_string(),
        source: "fixture".to_string(),
        updated_at: Utc::now(),
        session_tokens: Some(session_tokens),
        session_cost_usd: Some(session_cost),
        last_30_days_tokens: Some(session_tokens * 20), // Approximate monthly
        last_30_days_cost_usd: Some(monthly_cost),
        daily: vec![CostDailyEntry {
            date: today,
            input_tokens: Some(session_tokens * 3 / 4),
            output_tokens: Some(session_tokens / 4),
            cache_read_tokens: None,
            cache_creation_tokens: None,
            total_tokens: Some(session_tokens),
            total_cost: Some(session_cost),
            models_used: Some(vec!["test-model".to_string()]),
        }],
        totals: Some(CostTotals {
            input_tokens: Some(session_tokens * 15),
            output_tokens: Some(session_tokens * 5),
            cache_read_tokens: None,
            cache_creation_tokens: None,
            total_tokens: Some(session_tokens * 20),
            total_cost: Some(monthly_cost),
        }),
    }
}

/// Create a minimal `CostPayload` with no data.
#[allow(dead_code)]
#[must_use]
pub fn cost_payload_minimal(provider: &str) -> CostPayload {
    CostPayload {
        provider: provider.to_string(),
        source: "fixture".to_string(),
        updated_at: Utc::now(),
        session_tokens: None,
        session_cost_usd: None,
        last_30_days_tokens: None,
        last_30_days_cost_usd: None,
        daily: Vec::new(),
        totals: None,
    }
}

// =============================================================================
// Identity Factories
// =============================================================================

/// Create a `ProviderIdentity` with the given email.
#[must_use]
pub fn provider_identity(email: &str) -> ProviderIdentity {
    ProviderIdentity {
        account_email: Some(email.to_string()),
        account_organization: None,
        login_method: Some("fixture".to_string()),
    }
}

/// Create a full `ProviderIdentity` with all fields.
#[must_use]
pub fn provider_identity_full(email: &str, org: &str, method: &str) -> ProviderIdentity {
    ProviderIdentity {
        account_email: Some(email.to_string()),
        account_organization: Some(org.to_string()),
        login_method: Some(method.to_string()),
    }
}

// =============================================================================
// Tests for Fixtures
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_claude_credentials_fixture() {
        let creds: serde_json::Value = load_fixture("claude/credentials_full.json");
        assert!(creds.get("credentials").is_some());
    }

    #[test]
    fn load_codex_auth_fixture() {
        let auth: serde_json::Value = load_fixture("codex/auth_oauth.json");
        assert!(auth.get("tokens").is_some());
    }

    #[test]
    fn load_status_fixture() {
        let status: serde_json::Value = load_fixture("status/statuspage_operational.json");
        assert_eq!(status["status"]["indicator"], "none");
    }

    #[test]
    fn load_text_fixture() {
        let cli_output = load_fixture_text("claude/cli_output_v0.2.105.txt");
        assert!(cli_output.contains("Claude CLI"));
        assert!(cli_output.contains("70% remaining"));
    }

    #[test]
    fn usage_snapshot_factory() {
        let usage = usage_snapshot(30.0, Some(45.0));
        assert!(usage.primary.is_some());
        assert!(usage.secondary.is_some());
        assert!(usage.tertiary.is_none());

        let primary = usage.primary.unwrap();
        assert!((primary.used_percent - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn usage_snapshot_full_factory() {
        let usage = usage_snapshot_full(30.0, 45.0, 60.0);
        assert!(usage.primary.is_some());
        assert!(usage.secondary.is_some());
        assert!(usage.tertiary.is_some());
    }

    #[test]
    fn provider_payload_factory() {
        let payload = provider_payload_default("codex", "cli");
        assert_eq!(payload.provider, "codex");
        assert!(payload.credits.is_some()); // Codex has credits

        let claude = provider_payload_default("claude", "oauth");
        assert!(claude.credits.is_none()); // Claude doesn't have credits
    }

    #[test]
    fn status_factories() {
        let op = status_operational();
        assert_eq!(op.indicator, StatusIndicator::None);

        let minor = status_minor();
        assert_eq!(minor.indicator, StatusIndicator::Minor);

        let major = status_major();
        assert_eq!(major.indicator, StatusIndicator::Major);

        let critical = status_critical();
        assert_eq!(critical.indicator, StatusIndicator::Critical);
    }

    #[test]
    fn credits_factory() {
        let credits = credits_snapshot(75.0);
        assert!((credits.remaining - 75.0).abs() < f64::EPSILON);
        assert!(!credits.events.is_empty());
    }

    #[test]
    fn cost_factory() {
        let cost = cost_payload("claude", 2.50, 50.00, 100_000);
        assert_eq!(cost.provider, "claude");
        assert!((cost.session_cost_usd.unwrap() - 2.50).abs() < f64::EPSILON);
        assert!(!cost.daily.is_empty());
        assert!(cost.totals.is_some());
    }

    #[test]
    fn identity_factories() {
        let simple = provider_identity("test@example.com");
        assert_eq!(simple.account_email.unwrap(), "test@example.com");
        assert!(simple.account_organization.is_none());

        let full = provider_identity_full("test@example.com", "Test Org", "oauth");
        assert_eq!(full.account_organization.unwrap(), "Test Org");
        assert_eq!(full.login_method.unwrap(), "oauth");
    }
}
