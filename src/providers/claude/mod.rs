//! Claude (Anthropic) provider implementation.
//!
//! Supports:
//! - OAuth API (token from the keyring, Claude Code's `.credentials.json`,
//!   or the macOS Keychain)
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
                    // OAuth requires a token from the keyring, Claude Code's
                    // credentials file, or the macOS Keychain.
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

/// Check if an OAuth token is available from any supported source.
fn has_oauth_token() -> bool {
    get_oauth_token().is_some()
}

/// Get an OAuth access token for the Anthropic API.
///
/// Tries, in order:
/// 1. The system keyring entry (`caut` / `claude-oauth-token`).
/// 2. Claude Code's credentials file (`<claude_dir>/.credentials.json`, key
///    `claudeAiOauth.accessToken`) — `<claude_dir>` honors `CLAUDE_CONFIG_DIR`
///    via [`get_claude_dir`].
/// 3. On macOS, the `Claude Code-credentials` Keychain entry (which holds the
///    same JSON payload as the credentials file).
///
/// Tokens whose `claudeAiOauth.expiresAt` (epoch milliseconds) is in the past
/// are skipped. See issue #8.
pub(crate) fn get_oauth_token() -> Option<String> {
    get_keyring_token()
        .or_else(get_credentials_file_token)
        .or_else(get_macos_keychain_token)
}

/// Get OAuth token from the caut keyring entry.
fn get_keyring_token() -> Option<String> {
    let entry = keyring::Entry::new("caut", "claude-oauth-token").ok()?;
    entry.get_password().ok().filter(|t| !t.is_empty())
}

/// Get OAuth token from Claude Code's `.credentials.json`.
fn get_credentials_file_token() -> Option<String> {
    let creds_path = get_claude_dir()?.join(".credentials.json");
    let content = fs::read_to_string(creds_path).ok()?;
    token_from_credentials_json(&content)
}

/// On macOS, extract an OAuth token from the `Claude Code-credentials`
/// Keychain entry, which stores the same JSON payload that Linux/Windows
/// installs write to `.credentials.json`.
#[cfg(target_os = "macos")]
fn get_macos_keychain_token() -> Option<String> {
    let user = std::env::var("USER").ok();
    let try_entry = |account: &str| -> Option<String> {
        keyring::Entry::new("Claude Code-credentials", account)
            .ok()?
            .get_password()
            .ok()
    };
    let payload = user
        .as_deref()
        .and_then(try_entry)
        .or_else(|| try_entry(""))?;
    token_from_credentials_json(&payload)
}

#[cfg(not(target_os = "macos"))]
const fn get_macos_keychain_token() -> Option<String> {
    None
}

// =============================================================================
// Local Config Types
// =============================================================================

/// Get the Claude config directory path.
///
/// Resolution order (first match wins):
/// 1. `CLAUDE_CONFIG_DIR` environment variable — the same knob Anthropic's
///    Claude Code CLI uses to relocate its config, so honoring it lets users
///    run side-by-side accounts with separate config directories.
/// 2. `~/.claude` — the documented default.
///
/// See issue #6.
fn get_claude_dir() -> Option<PathBuf> {
    if let Ok(env_dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let trimmed = env_dir.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    directories::BaseDirs::new().map(|d| d.home_dir().join(".claude"))
}

/// Check if Claude is configured locally.
///
/// Considers:
/// - A `.credentials.json` or `settings.json` under the resolved Claude dir.
/// - On macOS, the system Keychain entry Claude Code writes on OAuth login
///   (service `Claude Code-credentials`). The Claude Code CLI on macOS
///   stores OAuth credentials in the Keychain, not in `.credentials.json`,
///   so a file-only probe reports the user as unauthenticated even when the
///   CLI itself is fully logged in. See issue #6.
fn has_local_config() -> bool {
    if get_claude_dir()
        .is_some_and(|d| d.join(".credentials.json").exists() || d.join("settings.json").exists())
    {
        return true;
    }
    macos_keychain_has_claude_credentials()
}

/// On macOS, look up the Claude Code keychain entry. Returns false on other
/// platforms and on lookup failure.
#[cfg(target_os = "macos")]
fn macos_keychain_has_claude_credentials() -> bool {
    // Prefer the current-user keychain; fall back to the generic entry.
    let user = std::env::var("USER").ok();
    let try_entry = |account: &str| -> bool {
        keyring::Entry::new("Claude Code-credentials", account)
            .ok()
            .and_then(|e| e.get_password().ok())
            .is_some_and(|s| !s.is_empty())
    };
    user.as_deref().is_some_and(try_entry) || try_entry("")
}

#[cfg(not(target_os = "macos"))]
const fn macos_keychain_has_claude_credentials() -> bool {
    false
}

/// Shape of Claude Code's `.credentials.json` (and the macOS Keychain
/// payload): OAuth tokens live under a top-level `claudeAiOauth` object
/// (alongside `mcpOAuth`). This matches what the doctor's auth check parses
/// in `src/core/doctor/checks.rs`. See issue #8.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredentialsFile {
    #[serde(default)]
    claude_ai_oauth: Option<ClaudeOauthCredentials>,
}

/// The `claudeAiOauth` object inside the credentials payload.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOauthCredentials {
    #[serde(default)]
    access_token: Option<String>,
    /// Token expiry as epoch milliseconds.
    #[serde(default)]
    expires_at: Option<i64>,
}

/// Extract a non-expired access token from a credentials JSON payload
/// (`claudeAiOauth.accessToken`, honoring `claudeAiOauth.expiresAt`).
fn token_from_credentials_json(content: &str) -> Option<String> {
    let creds: ClaudeCredentialsFile = serde_json::from_str(content).ok()?;
    let oauth = creds.claude_ai_oauth?;
    let token = oauth.access_token.filter(|t| !t.is_empty())?;
    if let Some(expires_at_ms) = oauth.expires_at
        && expires_at_ms <= Utc::now().timestamp_millis()
    {
        tracing::debug!("Claude OAuth token is expired (expiresAt in the past), skipping");
        return None;
    }
    Some(token)
}

/// Shape of Claude Code's main config (`~/.claude.json`): account identity
/// lives under the top-level `oauthAccount` object.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeMainConfig {
    #[serde(default)]
    oauth_account: Option<ClaudeOauthAccount>,
}

/// The `oauthAccount` object inside Claude Code's main config.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOauthAccount {
    #[serde(default)]
    email_address: Option<String>,
    #[serde(default)]
    organization_name: Option<String>,
}

/// Parse the `oauthAccount` identity object out of main-config JSON.
fn oauth_account_from_json(content: &str) -> Option<ClaudeOauthAccount> {
    serde_json::from_str::<ClaudeMainConfig>(content)
        .ok()?
        .oauth_account
}

/// Read the `oauthAccount` identity from Claude Code's main config.
///
/// Checks `<claude_dir>/.claude.json` first (where Claude Code writes it when
/// `CLAUDE_CONFIG_DIR` relocates the config), then the documented default
/// `~/.claude.json`.
fn read_oauth_account() -> Option<ClaudeOauthAccount> {
    let mut candidates = Vec::new();
    if let Some(dir) = get_claude_dir() {
        candidates.push(dir.join(".claude.json"));
    }
    if let Some(base) = directories::BaseDirs::new() {
        candidates.push(base.home_dir().join(".claude.json"));
    }
    for path in candidates {
        if let Ok(content) = fs::read_to_string(&path)
            && let Some(account) = oauth_account_from_json(&content)
        {
            return Some(account);
        }
    }
    None
}

/// Build a [`ProviderIdentity`] from the local `oauthAccount` config with the
/// given login method. Email/org are `None` when no account info is found.
fn local_identity_with_method(method: &str) -> ProviderIdentity {
    let account = read_oauth_account();
    ProviderIdentity {
        account_email: account.as_ref().and_then(|a| a.email_address.clone()),
        account_organization: account.and_then(|a| a.organization_name),
        login_method: Some(method.to_string()),
    }
}

/// Get identity information from local Claude Code config.
fn get_local_identity() -> Option<ProviderIdentity> {
    let account = read_oauth_account()?;
    Some(ProviderIdentity {
        account_email: account.email_address,
        account_organization: account.organization_name,
        login_method: Some("cli-local".to_string()),
    })
}

// =============================================================================
// API Response Types
// =============================================================================

/// Response from the Anthropic OAuth usage endpoint
/// (`GET {API_BASE}/api/oauth/usage`) — the same endpoint the Claude Code
/// CLI's `/usage` screen queries. See issue #8.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeOauthUsageResponse {
    #[serde(default)]
    five_hour: Option<ClaudeUsageWindow>,
    #[serde(default)]
    seven_day: Option<ClaudeUsageWindow>,
    #[serde(default)]
    seven_day_opus: Option<ClaudeUsageWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<ClaudeUsageWindow>,
}

/// A single usage window from the OAuth usage endpoint.
///
/// `utilization` is already percent-scale (e.g. `18.0` means 18% used);
/// `resets_at` is an RFC 3339 timestamp.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeUsageWindow {
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
}

// =============================================================================
// Fetch Implementations
// =============================================================================

/// Fetch usage via the Anthropic OAuth usage endpoint.
///
/// Sends `GET {API_BASE}/api/oauth/usage` with `Authorization: Bearer <token>`
/// and the `anthropic-beta: oauth-2025-04-20` header. The token comes from
/// [`get_oauth_token`] (keyring, Claude Code's credentials file, or the macOS
/// Keychain).
///
/// # Errors
/// Returns an error if the HTTP client cannot be built, the request times out,
/// the server returns a non-success status, or the response cannot be parsed.
pub async fn fetch_oauth(token: &str) -> Result<UsageSnapshot> {
    let client = build_client(DEFAULT_TIMEOUT)?;

    let url = format!("{API_BASE}/api/oauth/usage");

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
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

    let data: ClaudeOauthUsageResponse = response
        .json()
        .await
        .map_err(|e| CautError::ParseResponse(e.to_string()))?;

    Ok(parse_oauth_usage_response(&data))
}

/// Fetch usage via web scraping.
///
/// This requires macOS with browser cookies available.
///
/// # Errors
/// Returns an error if web scraping is not supported on the current platform
/// or if the scraping operation fails.
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
///
/// # Errors
/// Returns an error if no rate limit data can be obtained from the CLI
/// or local config files.
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
        let snapshot = parse_oauth_usage_response(&response);
        // Only accept the parsed output if it actually carried quota data;
        // an unrelated-but-valid JSON object deserializes to all-None fields.
        if snapshot.primary.is_some() || snapshot.secondary.is_some() || snapshot.tertiary.is_some()
        {
            return Ok(snapshot);
        }
    }

    // Try the /limits subcommand if available
    if let Ok(output) = run_command(CLI_NAME, &["limits"], CLI_TIMEOUT).await
        && output.success()
    {
        return Ok(parse_cli_limits_output(&output.stdout));
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
async fn try_json_rate_limit() -> Result<ClaudeOauthUsageResponse> {
    // Try various command patterns that CLI tools commonly use
    let commands = [
        &["rate-limit", "--json"][..],
        &["limits", "--json"][..],
        &["status", "--json"][..],
    ];

    for args in commands {
        if let Ok(response) =
            run_json_command::<ClaudeOauthUsageResponse>(CLI_NAME, args, CLI_TIMEOUT).await
        {
            return Ok(response);
        }
    }

    Err(CautError::FetchFailed {
        provider: "claude".to_string(),
        reason: "No rate limit command found".to_string(),
    })
}

/// Minutes in the 5-hour session window.
const FIVE_HOUR_WINDOW_MINUTES: i32 = 5 * 60;

/// Minutes in the 7-day window.
const SEVEN_DAY_WINDOW_MINUTES: i32 = 7 * 24 * 60;

/// Convert one OAuth usage window into a [`RateWindow`].
///
/// `utilization` is already percent-scale, so it maps directly onto
/// `used_percent`. `resets_at` (RFC 3339) is parsed and also humanized into
/// `reset_description` (e.g. "in 2h 15m"), which is what the human renderer
/// displays.
fn parse_usage_window(
    window: Option<&ClaudeUsageWindow>,
    window_minutes: i32,
) -> Option<RateWindow> {
    let window = window?;
    let used_percent = window.utilization?;
    let resets_at = window.resets_at.as_ref().and_then(|s| s.parse().ok());
    let reset_description = resets_at.map(crate::util::time::format_countdown);
    Some(RateWindow {
        used_percent,
        window_minutes: Some(window_minutes),
        resets_at,
        reset_description,
    })
}

/// Parse the OAuth usage response into a `UsageSnapshot`.
///
/// Window mapping: primary = `five_hour`, secondary = `seven_day`,
/// tertiary = `seven_day_opus` (falling back to `seven_day_sonnet`).
/// Identity comes from the local `oauthAccount` config, since the usage
/// endpoint does not return account info.
fn parse_oauth_usage_response(response: &ClaudeOauthUsageResponse) -> UsageSnapshot {
    let now = Utc::now();

    let primary = parse_usage_window(response.five_hour.as_ref(), FIVE_HOUR_WINDOW_MINUTES);
    let secondary = parse_usage_window(response.seven_day.as_ref(), SEVEN_DAY_WINDOW_MINUTES);
    let tertiary = parse_usage_window(response.seven_day_opus.as_ref(), SEVEN_DAY_WINDOW_MINUTES)
        .or_else(|| {
            parse_usage_window(response.seven_day_sonnet.as_ref(), SEVEN_DAY_WINDOW_MINUTES)
        });

    UsageSnapshot {
        primary,
        secondary,
        tertiary,
        updated_at: now,
        identity: Some(local_identity_with_method("oauth")),
    }
}

/// Parse CLI limits output (text format).
fn parse_cli_limits_output(output: &str) -> UsageSnapshot {
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
        } else if line.contains("token")
            && let Some(pct) = extract_percent(&line)
        {
            secondary = Some(RateWindow {
                used_percent: pct,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
            });
        }
    }

    UsageSnapshot {
        primary,
        secondary,
        tertiary: None,
        updated_at: now,
        identity: Some(ProviderIdentity {
            account_email: None,
            account_organization: None,
            login_method: Some("cli".to_string()),
        }),
    }
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
        .map_err(|e| CautError::Config(format!("Keyring error: {e}")))?;

    entry
        .set_password(token)
        .map_err(|e| CautError::Config(format!("Failed to store token: {e}")))
}

/// Delete OAuth token from keyring.
///
/// # Errors
///
/// Returns error if keyring access fails.
pub fn delete_oauth_token() -> Result<()> {
    let entry = keyring::Entry::new("caut", "claude-oauth-token")
        .map_err(|e| CautError::Config(format!("Keyring error: {e}")))?;

    entry
        .delete_credential()
        .map_err(|e| CautError::Config(format!("Failed to delete token: {e}")))
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
    // OAuth Usage Response Parsing Tests
    // =========================================================================

    /// Real-shape payload from `GET /api/oauth/usage` (see issue #8).
    /// `utilization` is already percent-scale; `resets_at` is RFC 3339.
    fn sample_usage_json() -> &'static str {
        r#"{
            "five_hour": {"utilization": 18.0, "resets_at": "2030-01-01T05:00:00Z"},
            "seven_day": {"utilization": 42.0, "resets_at": "2030-01-04T00:00:00+00:00"},
            "seven_day_opus": {"utilization": 7.5, "resets_at": "2030-01-04T00:00:00Z"},
            "seven_day_sonnet": {"utilization": 12.0, "resets_at": "2030-01-04T00:00:00Z"},
            "extra_field_ignored": {"foo": "bar"}
        }"#
    }

    #[test]
    fn parse_usage_response_full_data() {
        let response: ClaudeOauthUsageResponse =
            serde_json::from_str(sample_usage_json()).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        // Primary = five_hour; utilization maps directly to used_percent.
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 18.0).abs() < f64::EPSILON);
        assert_eq!(primary.window_minutes, Some(FIVE_HOUR_WINDOW_MINUTES));
        assert!(primary.resets_at.is_some());
        let desc = primary.reset_description.expect("reset description");
        assert!(!desc.is_empty());

        // Secondary = seven_day.
        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 42.0).abs() < f64::EPSILON);
        assert_eq!(secondary.window_minutes, Some(SEVEN_DAY_WINDOW_MINUTES));

        // Tertiary = seven_day_opus when present (Sonnet is the fallback).
        let tertiary = snapshot.tertiary.expect("tertiary");
        assert!((tertiary.used_percent - 7.5).abs() < f64::EPSILON);

        let identity = snapshot.identity.expect("identity");
        assert_eq!(identity.login_method.as_deref(), Some("oauth"));
    }

    #[test]
    fn parse_usage_response_future_reset_has_countdown_description() {
        let response: ClaudeOauthUsageResponse =
            serde_json::from_str(sample_usage_json()).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        // The fixture timestamps are far in the future, so the humanized
        // description must be a countdown ("in ...").
        let primary = snapshot.primary.expect("primary");
        let desc = primary.reset_description.expect("reset description");
        assert!(
            desc.starts_with("in "),
            "expected countdown description, got: {desc}"
        );
    }

    #[test]
    fn parse_usage_response_empty() {
        let response: ClaudeOauthUsageResponse = serde_json::from_str("{}").expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.tertiary.is_none());

        // Identity should still be set
        let identity = snapshot.identity.expect("identity");
        assert_eq!(identity.login_method.as_deref(), Some("oauth"));
    }

    #[test]
    fn parse_usage_response_tertiary_falls_back_to_sonnet() {
        let json = r#"{
            "five_hour": {"utilization": 0.0, "resets_at": null},
            "seven_day_sonnet": {"utilization": 12.0, "resets_at": "2030-01-04T00:00:00Z"}
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        let tertiary = snapshot.tertiary.expect("tertiary");
        assert!((tertiary.used_percent - 12.0).abs() < f64::EPSILON);
        assert_eq!(tertiary.window_minutes, Some(SEVEN_DAY_WINDOW_MINUTES));
    }

    #[test]
    fn parse_usage_response_missing_utilization_skips_window() {
        let json = r#"{"five_hour": {"resets_at": "2030-01-01T05:00:00Z"}}"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        // A window without utilization carries no usable percentage.
        assert!(snapshot.primary.is_none());
    }

    #[test]
    fn parse_usage_response_invalid_resets_at() {
        let json = r#"{"five_hour": {"utilization": 50.0, "resets_at": "not-a-valid-timestamp"}}"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 50.0).abs() < f64::EPSILON);
        // Invalid timestamp should result in no reset info, but the window
        // itself must survive.
        assert!(primary.resets_at.is_none());
        assert!(primary.reset_description.is_none());
    }

    // =========================================================================
    // Credentials File Parsing Tests (claudeAiOauth schema)
    // =========================================================================

    #[test]
    fn token_from_credentials_json_real_schema() {
        let future_ms = Utc::now().timestamp_millis() + 3_600_000;
        let content = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"sk-ant-oat01-test-token","refreshToken":"sk-ant-ort01-refresh","expiresAt":{future_ms},"scopes":["user:inference"],"subscriptionType":"max"}},"mcpOAuth":{{}}}}"#
        );

        assert_eq!(
            token_from_credentials_json(&content).as_deref(),
            Some("sk-ant-oat01-test-token")
        );
    }

    #[test]
    fn token_from_credentials_json_skips_expired_token() {
        let past_ms = Utc::now().timestamp_millis() - 1_000;
        let content = format!(
            r#"{{"claudeAiOauth":{{"accessToken":"sk-ant-oat01-stale","expiresAt":{past_ms}}}}}"#
        );

        assert!(token_from_credentials_json(&content).is_none());
    }

    #[test]
    fn token_from_credentials_json_allows_missing_expiry() {
        let content = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-no-expiry"}}"#;

        assert_eq!(
            token_from_credentials_json(content).as_deref(),
            Some("sk-ant-oat01-no-expiry")
        );
    }

    #[test]
    fn token_from_credentials_json_rejects_missing_or_empty_token() {
        assert!(token_from_credentials_json(r#"{"claudeAiOauth":{}}"#).is_none());
        assert!(token_from_credentials_json(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
    }

    #[test]
    fn token_from_credentials_json_rejects_other_schemas() {
        // A top-level `credentials` key is NOT what Claude Code writes.
        assert!(token_from_credentials_json(r#"{"credentials":{"email":"a@b.c"}}"#).is_none());
        // MCP-only auth has no primary Claude token.
        assert!(
            token_from_credentials_json(r#"{"mcpOAuth":{"server":{"accessToken":"x"}}}"#).is_none()
        );
        assert!(token_from_credentials_json("not json").is_none());
    }

    // =========================================================================
    // Main Config (oauthAccount) Identity Tests
    // =========================================================================

    #[test]
    fn oauth_account_from_json_real_schema() {
        let content = r#"{
            "oauthAccount": {
                "accountUuid": "123e4567-e89b-12d3-a456-426614174000",
                "emailAddress": "user@example.com",
                "organizationUuid": "223e4567-e89b-12d3-a456-426614174000",
                "organizationName": "User's Organization",
                "organizationRole": "admin"
            },
            "numStartups": 42
        }"#;

        let account = oauth_account_from_json(content).expect("account");
        assert_eq!(account.email_address.as_deref(), Some("user@example.com"));
        assert_eq!(
            account.organization_name.as_deref(),
            Some("User's Organization")
        );
    }

    #[test]
    fn oauth_account_from_json_missing_account() {
        assert!(oauth_account_from_json("{}").is_none());
        assert!(oauth_account_from_json("not json").is_none());
    }

    // =========================================================================
    // CLI Output Parsing Tests
    // =========================================================================

    #[test]
    fn parse_cli_limits_output_full_format() {
        let output =
            "Requests: 45/100 remaining (55% used)\nTokens: 90000/100000 remaining (10% used)";

        let snapshot = parse_cli_limits_output(output);

        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 55.0).abs() < f64::EPSILON);

        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_empty() {
        let output = "";

        let snapshot = parse_cli_limits_output(output);
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_cli_limits_output_only_requests() {
        let output = "Request limit: 75% used";

        let snapshot = parse_cli_limits_output(output);
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 75.0).abs() < f64::EPSILON);
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_cli_limits_output_only_tokens() {
        let output = "Token usage: 33.5% consumed";

        let snapshot = parse_cli_limits_output(output);
        assert!(snapshot.primary.is_none());
        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 33.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_case_insensitive() {
        let output = "REQUEST LIMIT: 25% used\nTOKEN LIMIT: 50% used";

        let snapshot = parse_cli_limits_output(output);
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

        let snapshot = parse_cli_limits_output(output);
        let primary = snapshot.primary.expect("primary");
        let secondary = snapshot.secondary.expect("secondary");
        assert!((primary.used_percent - 20.0).abs() < f64::EPSILON);
        assert!((secondary.used_percent - 45.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_cli_limits_output_sets_identity() {
        let output = "Requests: 50% used";

        let snapshot = parse_cli_limits_output(output);
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

    // =========================================================================
    // CLAUDE_CONFIG_DIR env var tests (issue #6)
    // =========================================================================

    /// Serializes tests that mutate `CLAUDE_CONFIG_DIR` to avoid interleaving.
    /// Other tests in this module do not touch this env var, so the lock
    /// only needs to cover the writers here.
    static CLAUDE_CONFIG_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Sets an env var for the duration of its scope and restores the
    /// previous value (or removes if there was none) on Drop. Ensures the
    /// env var is restored even if the test panics on an assertion, so
    /// one failing test cannot leak state into another.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: all call sites in this module hold CLAUDE_CONFIG_DIR_LOCK
            // for the duration of the guard, so no other test is racing on the
            // same env var.
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            #[allow(unsafe_code)]
            unsafe {
                match self.original.take() {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn get_claude_dir_honors_claude_config_dir_env() {
        let _lock = CLAUDE_CONFIG_DIR_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = EnvVarGuard::set("CLAUDE_CONFIG_DIR", "/tmp/claude-alt-account");

        assert_eq!(
            get_claude_dir(),
            Some(PathBuf::from("/tmp/claude-alt-account"))
        );
    }

    #[test]
    fn get_claude_dir_ignores_empty_env_and_falls_back_to_default() {
        let _lock = CLAUDE_CONFIG_DIR_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Empty / whitespace-only strings (common when shells set but blank)
        // must not shadow the ~/.claude default.
        let _env = EnvVarGuard::set("CLAUDE_CONFIG_DIR", "   ");

        let expected = directories::BaseDirs::new().map(|d| d.home_dir().join(".claude"));
        assert_eq!(get_claude_dir(), expected);
    }
}
