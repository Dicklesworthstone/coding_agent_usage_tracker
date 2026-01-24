//! Claude (Anthropic) provider implementation.
//!
//! Supports:
//! - OAuth API (with stored token)
//! - Web scraping (macOS only)
//! - CLI local config reading
//! - CLI PTY
//!
//! Source labels: `oauth`, `web`, `claude`, `cli-local`

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use serde::Deserialize;

use crate::core::cli_runner::{CLI_TIMEOUT, run_command, run_json_command};
use crate::core::fetch_plan::{FetchKind, FetchPlan, FetchStrategy};
use crate::core::http::{DEFAULT_TIMEOUT, build_client};
use crate::core::models::{ProviderIdentity, RateWindow, UsageSnapshot};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};

/// Source label for OAuth.
pub const SOURCE_OAUTH: &str = "oauth";

/// Source label for web.
pub const SOURCE_WEB: &str = "web";

/// Source label for CLI.
pub const SOURCE_CLI: &str = "claude";

/// CLI binary name.
const CLI_NAME: &str = "claude";

/// Anthropic API base URL.
const API_BASE: &str = "https://api.anthropic.com";

// =============================================================================
// Fetch Plan
// =============================================================================

/// Create fetch plan for Claude.
#[must_use]
pub fn fetch_plan() -> FetchPlan {
    FetchPlan::new(
        Provider::Claude,
        vec![
            FetchStrategy {
                id: "claude-oauth",
                kind: FetchKind::OAuth,
                is_available: || {
                    // OAuth requires stored token
                    // Check keyring for token
                    has_oauth_token()
                },
                should_fallback: |_| true,
            },
            FetchStrategy {
                id: "claude-web",
                kind: FetchKind::Web,
                is_available: || {
                    // Web requires macOS with cookies
                    cfg!(target_os = "macos")
                },
                should_fallback: |_| true,
            },
            FetchStrategy {
                id: "claude-cli-pty",
                kind: FetchKind::Cli,
                is_available: is_cli_available,
                should_fallback: |_| false,
            },
        ],
    )
}

/// Check if the Claude CLI is available.
fn is_cli_available() -> bool {
    which::which(CLI_NAME).is_ok()
}

/// Check if OAuth token is available.
fn has_oauth_token() -> bool {
    // Try to get token from keyring
    get_oauth_token().is_some()
}

/// Get OAuth token from keyring.
fn get_oauth_token() -> Option<String> {
    let entry = keyring::Entry::new("caut", "claude-oauth-token").ok()?;
    entry.get_password().ok()
}

// =============================================================================
// Local Config Types
// =============================================================================

/// Get the Claude config directory path.
fn get_claude_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".claude"))
}

/// Check if Claude is configured locally.
fn has_local_config() -> bool {
    get_claude_dir()
        .is_some_and(|d| d.join(".credentials.json").exists() || d.join("settings.json").exists())
}

/// Credentials.json structure from ~/.claude/.credentials.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredentials {
    #[serde(default)]
    credentials: Option<ClaudeCredentialData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredentialData {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    account_uuid: Option<String>,
}

/// Read credentials from local ~/.claude/.credentials.json
fn read_local_credentials() -> Option<ClaudeCredentials> {
    let claude_dir = get_claude_dir()?;
    let creds_path = claude_dir.join(".credentials.json");

    if !creds_path.exists() {
        tracing::debug!("Claude credentials not found at {:?}", creds_path);
        return None;
    }

    let content = fs::read_to_string(&creds_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Get identity information from local credentials.
fn get_local_identity() -> Option<ProviderIdentity> {
    let creds = read_local_credentials()?;
    let data = creds.credentials?;

    Some(ProviderIdentity {
        account_email: data.email,
        account_organization: data.account_uuid,
        login_method: Some("cli-local".to_string()),
    })
}

// =============================================================================
// API Response Types
// =============================================================================

/// Response from Anthropic rate limit API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeRateLimitResponse {
    #[serde(default)]
    rate_limit: Option<ClaudeRateLimit>,
    #[allow(dead_code)]
    #[serde(default)]
    usage: Option<ClaudeUsage>,
    #[serde(default)]
    account: Option<ClaudeAccount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeRateLimit {
    #[serde(default)]
    requests_remaining: Option<i64>,
    #[serde(default)]
    requests_limit: Option<i64>,
    #[serde(default)]
    tokens_remaining: Option<i64>,
    #[serde(default)]
    tokens_limit: Option<i64>,
    #[serde(default)]
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct ClaudeAccount {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    plan: Option<String>,
}

// =============================================================================
// Fetch Implementations
// =============================================================================

/// Fetch usage via OAuth API.
///
/// Requires a valid OAuth token stored in the system keyring.
pub async fn fetch_oauth(token: &str) -> Result<UsageSnapshot> {
    let client = build_client(DEFAULT_TIMEOUT)?;

    // Make authenticated request to rate limit endpoint
    let url = format!("{}/v1/rate_limits", API_BASE);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", token)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                CautError::Timeout(DEFAULT_TIMEOUT.as_secs())
            } else {
                CautError::Network(e.to_string())
            }
        })?;

    if !response.status().is_success() {
        return Err(CautError::FetchFailed {
            provider: "claude".to_string(),
            reason: format!("HTTP {}", response.status()),
        });
    }

    let data: ClaudeRateLimitResponse = response
        .json()
        .await
        .map_err(|e| CautError::ParseResponse(e.to_string()))?;

    parse_api_response(data)
}

/// Fetch usage via web scraping.
///
/// This requires macOS with browser cookies available.
pub async fn fetch_web() -> Result<UsageSnapshot> {
    #[cfg(not(target_os = "macos"))]
    {
        Err(CautError::UnsupportedSource {
            provider: "claude".to_string(),
            source_type: "web".to_string(),
        })
    }

    #[cfg(target_os = "macos")]
    {
        // TODO: Implement actual web scraping
        // This would involve:
        // 1. Reading browser cookies for claude.ai
        // 2. Making authenticated request to the web dashboard
        // 3. Parsing the response
        Err(CautError::FetchFailed {
            provider: "claude".to_string(),
            reason: "Web scraping not yet implemented".to_string(),
        })
    }
}

/// Fetch usage via CLI PTY.
///
/// Calls the `claude` CLI to get rate limit information.
/// Falls back to reading local config files for identity info.
pub async fn fetch_cli() -> Result<UsageSnapshot> {
    // First check version to confirm CLI is working
    let version = get_cli_version().await.ok();
    let now = Utc::now();

    tracing::debug!(
        ?version,
        cli_available = is_cli_available(),
        has_local_config = has_local_config(),
        "Claude CLI fetch starting"
    );

    // Try to get rate limit info via JSON output (unlikely to work - Claude CLI doesn't expose this)
    if let Ok(response) = try_json_rate_limit().await {
        return parse_api_response(response);
    }

    // Try the /limits subcommand if available
    if let Ok(output) = run_command(CLI_NAME, &["limits"], CLI_TIMEOUT).await {
        if output.success() {
            return parse_cli_limits_output(&output.stdout);
        }
    }

    // Fallback: Read identity from local config files
    let identity = get_local_identity().or_else(|| {
        Some(ProviderIdentity {
            account_email: None,
            account_organization: None,
            login_method: if has_local_config() {
                Some("cli-local".to_string())
            } else {
                Some("cli-unauthenticated".to_string())
            },
        })
    });

    // Return snapshot with what we know
    // Note: Claude CLI doesn't expose rate limit info directly via CLI
    // Rate limit data needs to come from OAuth API or web dashboard
    Ok(UsageSnapshot {
        primary: None,
        secondary: None,
        tertiary: None,
        updated_at: now,
        identity,
    })
}

/// Try to get rate limit via JSON output.
async fn try_json_rate_limit() -> Result<ClaudeRateLimitResponse> {
    // Try various command patterns that CLI tools commonly use
    let commands = [
        &["rate-limit", "--json"][..],
        &["limits", "--json"][..],
        &["status", "--json"][..],
    ];

    for args in commands {
        if let Ok(response) =
            run_json_command::<ClaudeRateLimitResponse>(CLI_NAME, args, CLI_TIMEOUT).await
        {
            return Ok(response);
        }
    }

    Err(CautError::FetchFailed {
        provider: "claude".to_string(),
        reason: "No rate limit command found".to_string(),
    })
}

/// Parse API response into UsageSnapshot.
fn parse_api_response(response: ClaudeRateLimitResponse) -> Result<UsageSnapshot> {
    let now = Utc::now();

    let primary = response.rate_limit.as_ref().and_then(|rl| {
        match (rl.requests_remaining, rl.requests_limit) {
            (Some(remaining), Some(limit)) if limit > 0 => {
                let used_percent = ((limit - remaining) as f64 / limit as f64) * 100.0;
                Some(RateWindow {
                    used_percent,
                    window_minutes: None,
                    resets_at: rl.resets_at.as_ref().and_then(|s| s.parse().ok()),
                    reset_description: None,
                })
            }
            _ => None,
        }
    });

    let secondary =
        response
            .rate_limit
            .as_ref()
            .and_then(|rl| match (rl.tokens_remaining, rl.tokens_limit) {
                (Some(remaining), Some(limit)) if limit > 0 => {
                    let used_percent = ((limit - remaining) as f64 / limit as f64) * 100.0;
                    Some(RateWindow {
                        used_percent,
                        window_minutes: None,
                        resets_at: rl.resets_at.as_ref().and_then(|s| s.parse().ok()),
                        reset_description: None,
                    })
                }
                _ => None,
            });

    let identity = Some(ProviderIdentity {
        account_email: response.account.as_ref().and_then(|a| a.email.clone()),
        account_organization: response
            .account
            .as_ref()
            .and_then(|a| a.organization.clone()),
        login_method: Some("oauth".to_string()),
    });

    Ok(UsageSnapshot {
        primary,
        secondary,
        tertiary: None,
        updated_at: now,
        identity,
    })
}

/// Parse CLI limits output (text format).
fn parse_cli_limits_output(output: &str) -> Result<UsageSnapshot> {
    let now = Utc::now();

    // Parse text output like:
    // "Requests: 45/100 remaining (55% used)"
    // "Tokens: 90000/100000 remaining (10% used)"
    let mut primary = None;
    let mut secondary = None;

    for line in output.lines() {
        let line = line.trim().to_lowercase();
        if line.contains("request") {
            if let Some(pct) = extract_percent(&line) {
                primary = Some(RateWindow {
                    used_percent: pct,
                    window_minutes: None,
                    resets_at: None,
                    reset_description: None,
                });
            }
        } else if line.contains("token") {
            if let Some(pct) = extract_percent(&line) {
                secondary = Some(RateWindow {
                    used_percent: pct,
                    window_minutes: None,
                    resets_at: None,
                    reset_description: None,
                });
            }
        }
    }

    Ok(UsageSnapshot {
        primary,
        secondary,
        tertiary: None,
        updated_at: now,
        identity: Some(ProviderIdentity {
            account_email: None,
            account_organization: None,
            login_method: Some("cli".to_string()),
        }),
    })
}

/// Extract percentage from a line like "55% used" or "(55%)".
fn extract_percent(line: &str) -> Option<f64> {
    // Find a number followed by %
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut num_str = String::from(c);
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() || next == '.' {
                    num_str.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&'%') {
                return num_str.parse().ok();
            }
        }
    }
    None
}

/// Get the CLI version.
async fn get_cli_version() -> Result<String> {
    let output = run_command(CLI_NAME, &["--version"], CLI_TIMEOUT).await?;

    if output.success() {
        // Parse version from output
        let version = output
            .stdout
            .trim()
            .split_whitespace()
            .last()
            .unwrap_or("unknown")
            .to_string();
        Ok(version)
    } else {
        Err(CautError::FetchFailed {
            provider: "claude".to_string(),
            reason: "Failed to get version".to_string(),
        })
    }
}

/// Store OAuth token in keyring.
///
/// # Errors
///
/// Returns error if keyring access fails.
pub fn store_oauth_token(token: &str) -> Result<()> {
    let entry = keyring::Entry::new("caut", "claude-oauth-token")
        .map_err(|e| CautError::Config(format!("Keyring error: {}", e)))?;

    entry
        .set_password(token)
        .map_err(|e| CautError::Config(format!("Failed to store token: {}", e)))
}

/// Delete OAuth token from keyring.
///
/// # Errors
///
/// Returns error if keyring access fails.
pub fn delete_oauth_token() -> Result<()> {
    let entry = keyring::Entry::new("caut", "claude-oauth-token")
        .map_err(|e| CautError::Config(format!("Keyring error: {}", e)))?;

    entry
        .delete_credential()
        .map_err(|e| CautError::Config(format!("Failed to delete token: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Fetch Plan Tests
    // =========================================================================

    #[test]
    fn fetch_plan_has_correct_provider() {
        let plan = fetch_plan();
        assert_eq!(plan.provider, Provider::Claude);
    }

    #[test]
    fn fetch_plan_has_expected_strategies() {
        let plan = fetch_plan();
        assert_eq!(plan.strategies.len(), 3);

        // First strategy should be OAuth
        assert_eq!(plan.strategies[0].id, "claude-oauth");
        assert!(matches!(plan.strategies[0].kind, FetchKind::OAuth));

        // Second strategy should be web
        assert_eq!(plan.strategies[1].id, "claude-web");
        assert!(matches!(plan.strategies[1].kind, FetchKind::Web));

        // Third strategy should be CLI
        assert_eq!(plan.strategies[2].id, "claude-cli-pty");
        assert!(matches!(plan.strategies[2].kind, FetchKind::Cli));
    }

    #[test]
    fn fetch_plan_web_availability_checks_os() {
        let plan = fetch_plan();
        let web_strategy = &plan.strategies[1];

        // On non-macOS, web should not be available
        #[cfg(not(target_os = "macos"))]
        assert!(!(web_strategy.is_available)());

        // On macOS, it should be available
        #[cfg(target_os = "macos")]
        assert!((web_strategy.is_available)());
    }

    #[test]
    fn fetch_plan_fallback_behavior() {
        let plan = fetch_plan();

        // OAuth should fallback on any error
        let oauth_strategy = &plan.strategies[0];
        assert!((oauth_strategy.should_fallback)(
            &crate::error::CautError::FetchFailed {
                provider: "claude".to_string(),
                reason: "test".to_string(),
            }
        ));

        // Web should fallback on any error
        let web_strategy = &plan.strategies[1];
        assert!((web_strategy.should_fallback)(
            &crate::error::CautError::FetchFailed {
                provider: "claude".to_string(),
                reason: "test".to_string(),
            }
        ));

        // CLI should not fallback (it's the last resort)
        let cli_strategy = &plan.strategies[2];
        assert!(!(cli_strategy.should_fallback)(
            &crate::error::CautError::FetchFailed {
                provider: "claude".to_string(),
                reason: "test".to_string(),
            }
        ));
    }

    // =========================================================================
    // API Response Parsing Tests
    // =========================================================================

    #[test]
    fn parse_api_response_full_data() {
        let response = ClaudeRateLimitResponse {
            rate_limit: Some(ClaudeRateLimit {
                requests_remaining: Some(70),
                requests_limit: Some(100),
                tokens_remaining: Some(80_000),
                tokens_limit: Some(100_000),
                resets_at: Some("2026-01-18T12:00:00Z".to_string()),
            }),
            usage: None,
            account: Some(ClaudeAccount {
                email: Some("test@example.com".to_string()),
                organization: Some("Test Org".to_string()),
                plan: Some("pro".to_string()),
            }),
        };

        let snapshot = parse_api_response(response).expect("snapshot");

        // Primary (requests): 30 used out of 100 = 30%
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 30.0).abs() < f64::EPSILON);
        assert!(primary.resets_at.is_some());

        // Secondary (tokens): 20000 used out of 100000 = 20%
        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 20.0).abs() < f64::EPSILON);

        // Identity
        let identity = snapshot.identity.expect("identity");
        assert_eq!(identity.account_email.as_deref(), Some("test@example.com"));
        assert_eq!(identity.account_organization.as_deref(), Some("Test Org"));
        assert_eq!(identity.login_method.as_deref(), Some("oauth"));
    }

    #[test]
    fn parse_api_response_empty_response() {
        let response = ClaudeRateLimitResponse {
            rate_limit: None,
            usage: None,
            account: None,
        };

        let snapshot = parse_api_response(response).expect("snapshot");
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.tertiary.is_none());

        // Identity should still be set
        let identity = snapshot.identity.expect("identity");
        assert_eq!(identity.login_method.as_deref(), Some("oauth"));
    }

    #[test]
    fn parse_api_response_only_requests() {
        let response = ClaudeRateLimitResponse {
            rate_limit: Some(ClaudeRateLimit {
                requests_remaining: Some(50),
                requests_limit: Some(100),
                tokens_remaining: None,
                tokens_limit: None,
                resets_at: None,
            }),
            usage: None,
            account: None,
        };

        let snapshot = parse_api_response(response).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 50.0).abs() < f64::EPSILON);
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_api_response_only_tokens() {
        let response = ClaudeRateLimitResponse {
            rate_limit: Some(ClaudeRateLimit {
                requests_remaining: None,
                requests_limit: None,
                tokens_remaining: Some(25_000),
                tokens_limit: Some(100_000),
                resets_at: None,
            }),
            usage: None,
            account: None,
        };

        let snapshot = parse_api_response(response).expect("snapshot");
        assert!(snapshot.primary.is_none());
        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_api_response_zero_limit_handled() {
        let response = ClaudeRateLimitResponse {
            rate_limit: Some(ClaudeRateLimit {
                requests_remaining: Some(0),
                requests_limit: Some(0), // Edge case: zero limit
                tokens_remaining: Some(0),
                tokens_limit: Some(0),
                resets_at: None,
            }),
            usage: None,
            account: None,
        };

        let snapshot = parse_api_response(response).expect("snapshot");
        // Zero limits should result in None (division by zero protection)
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_api_response_boundary_percentages() {
        // 100% used (0 remaining)
        let response = ClaudeRateLimitResponse {
            rate_limit: Some(ClaudeRateLimit {
                requests_remaining: Some(0),
                requests_limit: Some(100),
                tokens_remaining: Some(100_000),
                tokens_limit: Some(100_000), // 0% used
                resets_at: None,
            }),
            usage: None,
            account: None,
        };

        let snapshot = parse_api_response(response).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        let secondary = snapshot.secondary.expect("secondary");
        assert!((primary.used_percent - 100.0).abs() < f64::EPSILON);
        assert!((secondary.used_percent - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_api_response_invalid_resets_at() {
        let response = ClaudeRateLimitResponse {
            rate_limit: Some(ClaudeRateLimit {
                requests_remaining: Some(50),
                requests_limit: Some(100),
                tokens_remaining: None,
                tokens_limit: None,
                resets_at: Some("not-a-valid-timestamp".to_string()),
            }),
            usage: None,
            account: None,
        };

        let snapshot = parse_api_response(response).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        // Invalid timestamp should result in None
        assert!(primary.resets_at.is_none());
    }

    // =========================================================================
    // CLI Output Parsing Tests
    // =========================================================================

    #[test]
    fn parse_cli_limits_output_full_format() {
        let output =
            "Requests: 45/100 remaining (55% used)\nTokens: 90000/100000 remaining (10% used)";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");

        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 55.0).abs() < f64::EPSILON);

        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_empty() {
        let output = "";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_cli_limits_output_only_requests() {
        let output = "Request limit: 75% used";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 75.0).abs() < f64::EPSILON);
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_cli_limits_output_only_tokens() {
        let output = "Token usage: 33.5% consumed";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");
        assert!(snapshot.primary.is_none());
        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 33.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_case_insensitive() {
        let output = "REQUEST LIMIT: 25% used\nTOKEN LIMIT: 50% used";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        let secondary = snapshot.secondary.expect("secondary");
        assert!((primary.used_percent - 25.0).abs() < f64::EPSILON);
        assert!((secondary.used_percent - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_with_extra_content() {
        let output = r"
Claude CLI v1.2.3
==================
Status: Active
Requests used: 20% of limit
Token usage is at 45%
Plan: Pro
";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        let secondary = snapshot.secondary.expect("secondary");
        assert!((primary.used_percent - 20.0).abs() < f64::EPSILON);
        assert!((secondary.used_percent - 45.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_sets_identity() {
        let output = "Requests: 50% used";

        let snapshot = parse_cli_limits_output(output).expect("snapshot");
        let identity = snapshot.identity.expect("identity");
        assert_eq!(identity.login_method.as_deref(), Some("cli"));
        assert!(identity.account_email.is_none());
    }

    // =========================================================================
    // Percent Extraction Tests
    // =========================================================================

    #[test]
    fn extract_percent_basic() {
        assert_eq!(extract_percent("50% used"), Some(50.0));
        assert_eq!(extract_percent("100%"), Some(100.0));
        assert_eq!(extract_percent("0%"), Some(0.0));
    }

    #[test]
    fn extract_percent_decimal() {
        assert_eq!(extract_percent("33.5% used"), Some(33.5));
        assert_eq!(extract_percent("99.99%"), Some(99.99));
        assert_eq!(extract_percent("0.1%"), Some(0.1));
    }

    #[test]
    fn extract_percent_with_surrounding_text() {
        assert_eq!(extract_percent("Usage is at 75% of limit"), Some(75.0));
        assert_eq!(extract_percent("(45%)"), Some(45.0));
        assert_eq!(extract_percent("Rate: 12.5% consumed"), Some(12.5));
    }

    #[test]
    fn extract_percent_no_percent_sign() {
        assert_eq!(extract_percent("50 used"), None);
        assert_eq!(extract_percent("just text"), None);
        assert_eq!(extract_percent(""), None);
    }

    #[test]
    fn extract_percent_multiple_numbers() {
        // Should extract the first number followed by %
        assert_eq!(extract_percent("123 requests, 45% used"), Some(45.0));
    }

    #[test]
    fn extract_percent_takes_first_match() {
        assert_eq!(extract_percent("25% then 50%"), Some(25.0));
    }

    // =========================================================================
    // Source Constants Tests
    // =========================================================================

    #[test]
    fn source_constants_defined() {
        assert_eq!(SOURCE_OAUTH, "oauth");
        assert_eq!(SOURCE_WEB, "web");
        assert_eq!(SOURCE_CLI, "claude");
    }
}
