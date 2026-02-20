//! Integration tests for test fixtures.
//!
//! Verifies that all fixtures can be loaded and factory functions work correctly.

mod common;

use common::fixtures::{
    StatusIndicator, cost_payload, credits_snapshot, load_fixture, load_fixture_text,
    provider_identity, provider_identity_full, provider_payload, provider_payload_default,
    rate_window, status_critical, status_major, status_minor, status_operational, status_payload,
    usage_snapshot, usage_snapshot_full, usage_snapshot_minimal,
};

// =============================================================================
// Fixture Loading Tests
// =============================================================================

#[test]
fn test_load_claude_credentials_full() {
    let creds: serde_json::Value = load_fixture("claude/credentials_full.json");
    assert!(creds.get("credentials").is_some());
    let credentials = &creds["credentials"];
    assert_eq!(credentials["email"], "user@example.com");
}

#[test]
fn test_load_claude_credentials_minimal() {
    let creds: serde_json::Value = load_fixture("claude/credentials_minimal.json");
    let credentials = &creds["credentials"];
    assert_eq!(credentials["email"], "user@example.com");
    assert!(credentials.get("accountUuid").is_none());
}

#[test]
fn test_load_claude_credentials_empty() {
    let creds: serde_json::Value = load_fixture("claude/credentials_empty.json");
    assert!(creds["credentials"].is_null());
}

#[test]
fn test_load_claude_cli_output() {
    let output = load_fixture_text("claude/cli_output_v0.2.105.txt");
    assert!(output.contains("Claude CLI v0.2.105"));
    assert!(output.contains("70% remaining"));
    assert!(output.contains("user@example.com"));
}

#[test]
fn test_load_claude_rate_limit_full() {
    let rate: serde_json::Value = load_fixture("claude/rate_limit_full.json");
    assert!(rate.get("rate_limit").is_some());
    assert!(rate.get("identity").is_some());

    let primary = &rate["rate_limit"]["primary"];
    assert_eq!(primary["used_percent"], 30.0);
    assert_eq!(primary["remaining_percent"], 70.0);
}

#[test]
fn test_load_codex_auth_oauth() {
    let auth: serde_json::Value = load_fixture("codex/auth_oauth.json");
    assert!(auth.get("tokens").is_some());
    let tokens = &auth["tokens"];
    assert!(tokens.get("id_token").is_some());
    assert!(tokens.get("access_token").is_some());
}

#[test]
fn test_load_codex_auth_api_key() {
    let auth: serde_json::Value = load_fixture("codex/auth_api_key.json");
    let api_key = auth["OPENAI_API_KEY"].as_str().unwrap();
    assert!(api_key.starts_with("sk-test-"));
}

#[test]
fn test_load_codex_jwt_claims() {
    let claims: serde_json::Value = load_fixture("codex/jwt_claims_sample.json");
    assert_eq!(claims["email"], "user@example.com");
    assert_eq!(claims["email_verified"], true);

    let openai_auth = &claims["https://api.openai.com/auth"];
    assert_eq!(openai_auth["chatgpt_plan_type"], "pro");
}

#[test]
fn test_load_status_fixtures() {
    let operational: serde_json::Value = load_fixture("status/statuspage_operational.json");
    assert_eq!(operational["status"]["indicator"], "none");

    let minor: serde_json::Value = load_fixture("status/statuspage_minor.json");
    assert_eq!(minor["status"]["indicator"], "minor");

    let major: serde_json::Value = load_fixture("status/statuspage_major.json");
    assert_eq!(major["status"]["indicator"], "major");

    let critical: serde_json::Value = load_fixture("status/statuspage_critical.json");
    assert_eq!(critical["status"]["indicator"], "critical");

    let maintenance: serde_json::Value = load_fixture("status/statuspage_maintenance.json");
    assert_eq!(maintenance["status"]["indicator"], "maintenance");
}

#[test]
fn test_load_cost_fixtures() {
    let cache: serde_json::Value = load_fixture("cost/claude_stats_cache.json");
    assert_eq!(cache["version"], 1);
    assert!(!cache["daily"].as_array().unwrap().is_empty());

    let events: serde_json::Value = load_fixture("cost/codex_event_log.json");
    assert!(!events["events"].as_array().unwrap().is_empty());
}

#[test]
fn test_load_token_account_fixtures() {
    let single: serde_json::Value = load_fixture("token_accounts/single_account.json");
    assert_eq!(single["version"], 1);
    assert!(single.get("providers").is_some());

    let multi: serde_json::Value = load_fixture("token_accounts/multi_account.json");
    let providers = &multi["providers"];
    assert!(providers.get("claude").is_some());
    assert!(providers.get("codex").is_some());

    let empty: serde_json::Value = load_fixture("token_accounts/empty.json");
    let empty_providers = empty["providers"].as_object().unwrap();
    assert!(empty_providers.is_empty());
}

#[test]
fn test_load_error_fixtures() {
    let timeout: serde_json::Value = load_fixture("errors/network_timeout.json");
    assert_eq!(timeout["error"]["type"], "timeout");

    let unauthorized: serde_json::Value = load_fixture("errors/http_401.json");
    assert_eq!(unauthorized["error"]["code"], 401);

    let forbidden: serde_json::Value = load_fixture("errors/http_403.json");
    assert_eq!(forbidden["error"]["code"], 403);

    let server_error: serde_json::Value = load_fixture("errors/http_500.json");
    assert_eq!(server_error["error"]["code"], 500);
}

#[test]
fn test_load_malformed_json() {
    let malformed = load_fixture_text("errors/malformed_json.txt");
    // Should not parse as valid JSON
    let result: Result<serde_json::Value, _> = serde_json::from_str(&malformed);
    assert!(result.is_err());
}

// =============================================================================
// Factory Function Tests
// =============================================================================

#[test]
fn test_usage_snapshot_factory() {
    let usage = usage_snapshot(30.0, Some(45.0));
    assert!(usage.primary.is_some());
    assert!(usage.secondary.is_some());
    assert!(usage.tertiary.is_none());

    let primary = usage.primary.unwrap();
    assert!((primary.used_percent - 30.0).abs() < f64::EPSILON);
    assert!((primary.remaining_percent() - 70.0).abs() < f64::EPSILON);
}

#[test]
fn test_usage_snapshot_full_factory() {
    let usage = usage_snapshot_full(25.0, 40.0, 55.0);
    assert!(usage.primary.is_some());
    assert!(usage.secondary.is_some());
    assert!(usage.tertiary.is_some());

    let tertiary = usage.tertiary.unwrap();
    assert!((tertiary.used_percent - 55.0).abs() < f64::EPSILON);
}

#[test]
fn test_usage_snapshot_minimal_factory() {
    let usage = usage_snapshot_minimal(50.0);
    assert!(usage.primary.is_some());
    assert!(usage.secondary.is_none());
    assert!(usage.tertiary.is_none());
}

#[test]
fn test_rate_window_factory() {
    let window = rate_window(35.0, 180);
    assert!((window.used_percent - 35.0).abs() < f64::EPSILON);
    assert_eq!(window.window_minutes, Some(180));
    assert!(window.resets_at.is_some());
    assert!(window.reset_description.is_some());
}

#[test]
fn test_provider_payload_factory() {
    let usage = usage_snapshot(30.0, None);
    let payload = provider_payload("codex", "cli", usage);

    assert_eq!(payload.provider, "codex");
    assert_eq!(payload.source, "cli");
    assert!(payload.credits.is_some()); // Codex has credits
    assert!(payload.status.is_some());
}

#[test]
fn test_provider_payload_no_credits_for_claude() {
    let payload = provider_payload_default("claude", "oauth");
    assert!(payload.credits.is_none()); // Claude doesn't have credits
}

#[test]
fn test_status_factories() {
    let op = status_operational();
    assert_eq!(op.indicator, StatusIndicator::None);
    assert!(op.description.as_ref().unwrap().contains("Operational"));

    let minor = status_minor();
    assert_eq!(minor.indicator, StatusIndicator::Minor);

    let major = status_major();
    assert_eq!(major.indicator, StatusIndicator::Major);

    let critical = status_critical();
    assert_eq!(critical.indicator, StatusIndicator::Critical);
}

#[test]
fn test_status_payload_factory() {
    let status = status_payload(StatusIndicator::Maintenance, "Scheduled maintenance");
    assert_eq!(status.indicator, StatusIndicator::Maintenance);
    assert_eq!(status.description.unwrap(), "Scheduled maintenance");
}

#[test]
fn test_credits_snapshot_factory() {
    let credits = credits_snapshot(75.0);
    assert!((credits.remaining - 75.0).abs() < f64::EPSILON);
    assert!(!credits.events.is_empty());
    // Should have purchase and usage events
    assert!(credits.events.iter().any(|e| e.event_type == "purchase"));
    assert!(credits.events.iter().any(|e| e.event_type == "usage"));
}

#[test]
fn test_cost_payload_factory() {
    let cost = cost_payload("claude", 2.50, 50.00, 100_000);
    assert_eq!(cost.provider, "claude");
    assert!((cost.session_cost_usd.unwrap() - 2.50).abs() < f64::EPSILON);
    assert!((cost.last_30_days_cost_usd.unwrap() - 50.00).abs() < f64::EPSILON);
    assert_eq!(cost.session_tokens.unwrap(), 100_000);
    assert!(!cost.daily.is_empty());
    assert!(cost.totals.is_some());
}

#[test]
fn test_provider_identity_factories() {
    let simple = provider_identity("test@example.com");
    assert_eq!(simple.account_email.unwrap(), "test@example.com");
    assert!(simple.account_organization.is_none());

    let full = provider_identity_full("test@example.com", "Test Org", "oauth");
    assert_eq!(full.account_email.unwrap(), "test@example.com");
    assert_eq!(full.account_organization.unwrap(), "Test Org");
    assert_eq!(full.login_method.unwrap(), "oauth");
}
