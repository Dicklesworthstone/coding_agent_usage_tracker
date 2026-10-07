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
use crate::core::fetch_plan::{FetchContext, FetchKind, FetchPlan, FetchStrategy};
use crate::core::http::{DEFAULT_TIMEOUT, build_client};
use crate::core::models::{ProviderIdentity, RateWindow, ScopedWindow, UsageSnapshot};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::common;

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
                // OAuth requires a token from the selected token account, the
                // keyring, Claude Code's credentials file, or the macOS Keychain.
                is_available: |ctx| oauth_token_for(ctx).is_some(),
                should_fallback: |_| true,
            },
            FetchStrategy {
                id: "claude-web",
                kind: FetchKind::Web,
                // Web requires a claude.ai `sessionKey` cookie, from the
                // selected token account or the environment.
                is_available: |ctx| session_key_for(ctx).is_some(),
                should_fallback: |_| true,
            },
            FetchStrategy {
                id: "claude-cli-pty",
                kind: FetchKind::Cli,
                // The CLI reports whichever account it is logged into, so it
                // cannot stand in for an explicitly selected token account.
                is_available: |ctx| ctx.account_token().is_none() && is_cli_available(),
                should_fallback: |_| false,
            },
        ],
    )
}

/// Run one strategy from [`fetch_plan`].
///
/// # Errors
/// Returns the strategy's error, or an error for an unknown strategy id.
pub async fn fetch(strategy_id: &str, ctx: &FetchContext) -> Result<UsageSnapshot> {
    match strategy_id {
        "claude-oauth" => {
            let token = oauth_token_for(ctx).ok_or_else(|| {
                CautError::Config(
                    "No Claude OAuth token found (checked token accounts, keyring, \
                     <claude_dir>/.credentials.json, and macOS Keychain)"
                        .to_string(),
                )
            })?;
            fetch_oauth(&token).await
        }
        "claude-web" => {
            let session_key = session_key_for(ctx).ok_or_else(|| {
                common::missing_credential(
                    Provider::Claude,
                    "set CLAUDE_SESSION_KEY or add a token account with the claude.ai sessionKey cookie",
                )
            })?;
            fetch_web(&session_key).await
        }
        "claude-cli-pty" => fetch_cli().await,
        _ => Err(crate::providers::unknown_strategy(
            Provider::Claude,
            strategy_id,
        )),
    }
}

/// Check if the Claude CLI is available.
fn is_cli_available() -> bool {
    which::which(CLI_NAME).is_ok()
}

/// Whether a credential is an Anthropic OAuth access token (`sk-ant-oat…`)
/// rather than a claude.ai session cookie.
fn is_oauth_access_token(token: &str) -> bool {
    common::strip_bearer(token).starts_with("sk-ant-oat")
}

/// The OAuth token to use for this fetch.
///
/// A selected token account wins when it holds an OAuth token; a selected
/// session-cookie account disables OAuth so the fetch reports that account
/// rather than the locally logged-in one. Without a selection, local sources
/// come first, then the active token account.
fn oauth_token_for(ctx: &FetchContext) -> Option<String> {
    if let Some(token) = ctx.account_token() {
        return is_oauth_access_token(token).then(|| common::strip_bearer(token).to_string());
    }
    get_oauth_token().or_else(|| {
        common::active_token_account(Provider::Claude).filter(|t| is_oauth_access_token(t))
    })
}

/// The claude.ai `sessionKey` cookie to use for this fetch.
fn session_key_for(ctx: &FetchContext) -> Option<String> {
    if let Some(token) = ctx.account_token() {
        return (!is_oauth_access_token(token))
            .then(|| session_key_from_credential(token))
            .flatten();
    }
    common::env_secret(&["CLAUDE_SESSION_KEY", "CLAUDE_COOKIE"])
        .and_then(|raw| session_key_from_credential(&raw))
        .or_else(|| {
            common::active_token_account(Provider::Claude)
                .filter(|t| !is_oauth_access_token(t))
                .and_then(|t| session_key_from_credential(&t))
        })
}

/// Extract a `sessionKey` from either a full cookie header or a bare value.
fn session_key_from_credential(raw: &str) -> Option<String> {
    if let Some(value) = common::cookie_value(raw, "sessionKey") {
        return (!value.is_empty()).then(|| value.to_string());
    }
    let trimmed = raw.trim();
    (trimmed.starts_with("sk-ant-") && !trimmed.contains('=') && !trimmed.contains(';'))
        .then(|| trimmed.to_string())
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
/// are skipped; a missing, zero, or negative `expiresAt` is a "no expiry
/// recorded" sentinel and the token is used. See issues #8 and #10.
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
    let content = read_credentials_file()?;
    token_from_credentials_json(&content)
}

/// Read the raw contents of `<claude_dir>/.credentials.json`, if present.
fn read_credentials_file() -> Option<String> {
    let creds_path = get_claude_dir()?.join(".credentials.json");
    fs::read_to_string(creds_path).ok()
}

/// On macOS, extract an OAuth token from the `Claude Code-credentials`
/// Keychain entry, which stores the same JSON payload that Linux/Windows
/// installs write to `.credentials.json`.
#[cfg(target_os = "macos")]
fn get_macos_keychain_token() -> Option<String> {
    let payload = read_macos_keychain_payload()?;
    token_from_credentials_json(&payload)
}

#[cfg(not(target_os = "macos"))]
const fn get_macos_keychain_token() -> Option<String> {
    None
}

/// Where a Claude credentials payload was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialsSource {
    /// `<claude_dir>/.credentials.json`.
    File,
    /// The macOS login Keychain (`Claude Code-credentials`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    MacosKeychain,
}

impl CredentialsSource {
    /// Human-readable label for diagnostics.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::File => "credentials file",
            Self::MacosKeychain => "macOS Keychain",
        }
    }
}

/// Read Claude Code's raw credentials JSON payload from wherever the CLI
/// stored it: the credentials file first, then (macOS only) the Keychain.
///
/// Used by `caut doctor`, which previously only looked at the file and so
/// told every macOS user to re-authenticate. See issue #10.
pub(crate) fn read_credentials_payload() -> Option<(CredentialsSource, String)> {
    if let Some(content) = read_credentials_file() {
        return Some((CredentialsSource::File, content));
    }
    read_macos_keychain_payload().map(|payload| (CredentialsSource::MacosKeychain, payload))
}

/// The Keychain service name Claude Code uses for its OAuth credentials.
#[cfg(target_os = "macos")]
const MACOS_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Read the raw JSON payload Claude Code stores in the macOS login Keychain.
///
/// Claude Code writes the item with the `security` CLI, so `/usr/bin/security`
/// is the only application on the item's access-control list. An in-process
/// Security.framework lookup (what the `keyring` crate does) from a different
/// binary is therefore not trusted by the item: depending on the session it
/// either fails outright or would pop a Keychain consent dialog. Reading
/// through the same `security find-generic-password` tool Claude Code uses
/// works without any prompt, so that is tried first and the `keyring` crate
/// is kept as a fallback for items whose ACL does allow it. See issue #10.
#[cfg(target_os = "macos")]
fn read_macos_keychain_payload() -> Option<String> {
    let user = std::env::var("USER").ok().filter(|u| !u.trim().is_empty());

    // Prefer the current user's account; fall back to any account.
    for account in [user.as_deref(), None] {
        if let Some(payload) = security_cli_find_generic_password(account) {
            tracing::debug!(
                user_account = account.is_some(),
                "Read Claude Code credentials from the macOS Keychain via `security`"
            );
            return Some(payload);
        }
    }
    for account in user.as_deref().into_iter().chain(std::iter::once("")) {
        if let Some(payload) = keyring_find_generic_password(account) {
            tracing::debug!(
                user_account = !account.is_empty(),
                "Read Claude Code credentials from the macOS Keychain via keyring"
            );
            return Some(payload);
        }
    }
    tracing::debug!("No readable Claude Code credentials in the macOS Keychain");
    None
}

/// Run `security find-generic-password -s <service> [-a <account>] -w` and
/// return the stored secret, if any. Never logs the secret.
#[cfg(target_os = "macos")]
fn security_cli_find_generic_password(account: Option<&str>) -> Option<String> {
    let mut cmd = std::process::Command::new("/usr/bin/security");
    cmd.arg("find-generic-password")
        .arg("-s")
        .arg(MACOS_KEYCHAIN_SERVICE);
    if let Some(account) = account {
        cmd.arg("-a").arg(account);
    }
    cmd.arg("-w");
    cmd.stdin(std::process::Stdio::null());
    let output = match cmd.output() {
        Ok(output) => output,
        Err(err) => {
            tracing::debug!(error = %err, "Could not run `security find-generic-password`");
            return None;
        }
    };
    if !output.status.success() {
        tracing::debug!(
            status = %output.status,
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "`security find-generic-password` did not return the Claude Code item"
        );
        return None;
    }
    let payload = String::from_utf8(output.stdout).ok()?;
    let payload = payload.trim();
    if payload.is_empty() {
        return None;
    }
    Some(payload.to_string())
}

/// Read the Keychain item through the `keyring` crate (Security.framework).
#[cfg(target_os = "macos")]
fn keyring_find_generic_password(account: &str) -> Option<String> {
    match keyring::Entry::new(MACOS_KEYCHAIN_SERVICE, account).and_then(|e| e.get_password()) {
        Ok(payload) if !payload.trim().is_empty() => Some(payload),
        Ok(_) => None,
        Err(err) => {
            tracing::debug!(error = %err, "keyring lookup of the Claude Code item failed");
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
const fn read_macos_keychain_payload() -> Option<String> {
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
fn macos_keychain_has_claude_credentials() -> bool {
    read_macos_keychain_payload().is_some()
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
    token_from_credentials_json_at(content, Utc::now().timestamp_millis())
}

/// [`token_from_credentials_json`] with an injectable clock (epoch ms).
fn token_from_credentials_json_at(content: &str, now_ms: i64) -> Option<String> {
    let creds: ClaudeCredentialsFile = serde_json::from_str(content).ok()?;
    let oauth = creds.claude_ai_oauth?;
    let token = oauth.access_token.filter(|t| !t.is_empty())?;
    if oauth_token_is_expired(oauth.expires_at, now_ms) {
        tracing::debug!("Claude OAuth token is expired (expiresAt in the past), skipping");
        return None;
    }
    Some(token)
}

/// Whether a `claudeAiOauth.expiresAt` value (epoch ms) says the token is
/// stale at `now_ms`.
///
/// Claude Code writes `expiresAt: 0` when it manages refresh itself (the real
/// deadline then lives in `refreshTokenExpiresAt`). Zero and negative values
/// are therefore sentinels meaning "no expiry recorded", not timestamps in
/// 1970 — treat them like a missing field. See issue #10.
const fn oauth_token_is_expired(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    match expires_at_ms {
        Some(expires_at_ms) if expires_at_ms > 0 => expires_at_ms <= now_ms,
        _ => false,
    }
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
    /// One entry per rate limit window, including the `weekly_scoped`
    /// per-model allowances that have no top-level field of their own. Without
    /// these an account whose Fable quota is spent reads as idle (issue #11).
    #[serde(default)]
    limits: Vec<ClaudeLimit>,
}

/// One entry of the usage response's `limits[]` array.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeLimit {
    /// `session`, `weekly_all` or `weekly_scoped`.
    #[serde(default)]
    kind: Option<String>,
    /// `session` or `weekly`; the window length this limit is measured over.
    #[serde(default)]
    group: Option<String>,
    /// Percent of the allowance consumed, 0-100.
    #[serde(default)]
    percent: Option<f64>,
    /// The provider's own grading: `normal`, `warning`, `critical`.
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    resets_at: Option<String>,
    /// Present when the limit binds one model (or surface) rather than the
    /// whole account.
    #[serde(default)]
    scope: Option<ClaudeLimitScope>,
    /// Set on the limit currently binding the account — not on the set of
    /// limits that apply, so it must not be used to filter entries out.
    /// Optional so an explicit `null` cannot fail the whole response.
    #[serde(default)]
    is_active: Option<bool>,
}

/// The `scope` object of a `limits[]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeLimitScope {
    #[serde(default)]
    model: Option<ClaudeLimitModel>,
}

/// The model a scoped limit applies to. `id` is currently always null in the
/// responses observed, so the display name is the usable identifier.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ClaudeLimitModel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
}

impl ClaudeLimit {
    /// The model this limit is scoped to, or `None` when it applies to the
    /// whole account.
    fn model_label(&self) -> Option<String> {
        let model = self.scope.as_ref()?.model.as_ref()?;
        model
            .display_name
            .as_ref()
            .or(model.id.as_ref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// The window length this limit is measured over, in minutes.
    fn window_minutes(&self) -> Option<i32> {
        match self.group.as_deref() {
            Some("session") => Some(FIVE_HOUR_WINDOW_MINUTES),
            Some("weekly") => Some(SEVEN_DAY_WINDOW_MINUTES),
            _ => None,
        }
    }

    /// Render this limit as a [`RateWindow`], or `None` when it carries no
    /// percentage.
    ///
    /// A limit with no `percent` says nothing about how much is left, and
    /// reporting it as 0% would be the very failure this parsing exists to
    /// prevent — an unknown quota reading as spare capacity. Skipping it
    /// matches how the top-level windows treat a missing `utilization`.
    fn rate_window(&self) -> Option<RateWindow> {
        let used_percent = self.percent?;
        let resets_at = self.resets_at.as_ref().and_then(|s| s.parse().ok());
        Some(RateWindow {
            used_percent,
            window_minutes: self.window_minutes(),
            resets_at,
            reset_description: resets_at.map(crate::util::time::format_countdown),
        })
    }
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

/// claude.ai web API base URL.
const WEB_API_BASE: &str = "https://claude.ai/api";

/// One entry of `GET /api/organizations`.
#[derive(Debug, Deserialize)]
struct ClaudeWebOrganization {
    uuid: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

impl ClaudeWebOrganization {
    fn has_capability(&self, wanted: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(wanted))
    }

    /// An organization whose only capability is `api` (a Console org).
    fn is_api_only(&self) -> bool {
        !self.capabilities.is_empty()
            && self
                .capabilities
                .iter()
                .all(|c| c.eq_ignore_ascii_case("api"))
    }
}

/// Pick the organization whose usage the web UI shows: the first chat-capable
/// one, else the first that is not API-only, else the first.
fn select_web_organization(orgs: &[ClaudeWebOrganization]) -> Option<&ClaudeWebOrganization> {
    orgs.iter()
        .find(|o| o.has_capability("chat"))
        .or_else(|| orgs.iter().find(|o| !o.is_api_only()))
        .or_else(|| orgs.first())
}

/// `GET /api/account`: identity for the session.
#[derive(Debug, Deserialize)]
struct ClaudeWebAccount {
    #[serde(default)]
    email_address: Option<String>,
}

/// Fetch usage from the claude.ai web API with a `sessionKey` cookie.
///
/// Calls `GET /api/organizations` to find the chat organization, then
/// `GET /api/organizations/{uuid}/usage`, which returns the same window shape
/// as the OAuth usage endpoint. `GET /api/account` supplies the email; its
/// failure only drops identity.
///
/// # Errors
/// Returns an error if the session key is rejected, Cloudflare challenges the
/// request, or a response cannot be parsed.
pub async fn fetch_web(session_key: &str) -> Result<UsageSnapshot> {
    let client = common::http_client()?;
    let cookie = format!("sessionKey={session_key}");
    let get = |path: &str| {
        client
            .get(format!("{WEB_API_BASE}{path}"))
            .header("Cookie", cookie.clone())
            .header("Accept", "application/json")
    };

    let orgs: Vec<ClaudeWebOrganization> =
        common::send_json(Provider::Claude, get("/organizations")).await?;
    let org = select_web_organization(&orgs).ok_or_else(|| {
        common::fetch_failed(Provider::Claude, "claude.ai session has no organizations")
    })?;

    let usage: ClaudeOauthUsageResponse = common::send_json(
        Provider::Claude,
        get(&format!("/organizations/{}/usage", org.uuid)),
    )
    .await?;
    let mut snapshot = parse_oauth_usage_response(&usage);

    let account: Option<ClaudeWebAccount> = common::send_json(Provider::Claude, get("/account"))
        .await
        .ok();
    snapshot.identity = Some(ProviderIdentity {
        account_email: account.and_then(|a| a.email_address),
        account_organization: org.name.clone(),
        login_method: Some(SOURCE_WEB.to_string()),
    });
    Ok(snapshot)
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
        updated_at: now,
        identity,
        ..UsageSnapshot::empty()
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

    let mut primary = parse_usage_window(response.five_hour.as_ref(), FIVE_HOUR_WINDOW_MINUTES);
    let mut secondary = parse_usage_window(response.seven_day.as_ref(), SEVEN_DAY_WINDOW_MINUTES);
    let tertiary = parse_usage_window(response.seven_day_opus.as_ref(), SEVEN_DAY_WINDOW_MINUTES)
        .or_else(|| {
            parse_usage_window(response.seven_day_sonnet.as_ref(), SEVEN_DAY_WINDOW_MINUTES)
        });

    // `limits[]` is the current shape. Every entry is read, not only the ones
    // flagged `is_active`: that flag marks the window binding the account right
    // now, so filtering on it would drop the general windows exactly when a
    // scoped quota is the one that is spent (issue #11).
    let mut scoped = Vec::new();
    for limit in &response.limits {
        let Some(window) = limit.rate_window() else {
            continue; // no percentage: nothing this entry can tell us
        };
        if let Some(label) = limit.model_label() {
            scoped.push(ScopedWindow {
                label,
                kind: limit.kind.clone(),
                severity: limit.severity.clone(),
                is_active: limit.is_active.unwrap_or(false),
                window,
            });
            continue;
        }
        // Account-wide entries backfill the top-level windows for responses
        // that report them only here.
        match limit.kind.as_deref() {
            Some("session") if primary.is_none() => primary = Some(window),
            Some("weekly_all") if secondary.is_none() => secondary = Some(window),
            _ => {}
        }
    }

    // Worst first, so the quota that decides whether the account is usable is
    // the one a reader sees first.
    scoped.sort_by(|a, b| {
        b.window
            .used_percent
            .partial_cmp(&a.window.used_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });

    UsageSnapshot {
        primary,
        secondary,
        tertiary,
        scoped,
        provider_cost: None,
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
        scoped: Vec::new(),
        provider_cost: None,
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
    fn selected_session_cookie_account_routes_to_web_only() {
        let plan = fetch_plan();
        let ctx = FetchContext::with_account("work", "Cookie: foo=1; sessionKey=sk-ant-sid01-abc");
        assert!(!(plan.strategies[0].is_available)(&ctx), "oauth");
        assert!((plan.strategies[1].is_available)(&ctx), "web");
        assert!(!(plan.strategies[2].is_available)(&ctx), "cli");
        assert_eq!(session_key_for(&ctx).as_deref(), Some("sk-ant-sid01-abc"));
    }

    #[test]
    fn selected_oauth_account_routes_to_oauth_only() {
        let plan = fetch_plan();
        let ctx = FetchContext::with_account("work", "Bearer sk-ant-oat01-xyz");
        assert!((plan.strategies[0].is_available)(&ctx), "oauth");
        assert!(!(plan.strategies[1].is_available)(&ctx), "web");
        assert!(!(plan.strategies[2].is_available)(&ctx), "cli");
        assert_eq!(oauth_token_for(&ctx).as_deref(), Some("sk-ant-oat01-xyz"));
    }

    #[test]
    fn session_key_from_credential_accepts_bare_and_header_forms() {
        assert_eq!(
            session_key_from_credential("sk-ant-sid01-abc").as_deref(),
            Some("sk-ant-sid01-abc")
        );
        assert_eq!(
            session_key_from_credential("a=1; sessionKey=sk-ant-sid01-def").as_deref(),
            Some("sk-ant-sid01-def")
        );
        assert_eq!(session_key_from_credential("a=1; b=2"), None);
        assert_eq!(session_key_from_credential("random"), None);
        assert_eq!(session_key_from_credential("sessionKey="), None);
    }

    #[test]
    fn select_web_organization_prefers_chat_then_non_api() {
        let orgs: Vec<ClaudeWebOrganization> = serde_json::from_str(
            r#"[
                {"uuid": "api-org", "name": "Console", "capabilities": ["api"]},
                {"uuid": "chat-org", "name": "Personal", "capabilities": ["chat", "claude_pro"]}
            ]"#,
        )
        .unwrap();
        assert_eq!(select_web_organization(&orgs).unwrap().uuid, "chat-org");

        let orgs: Vec<ClaudeWebOrganization> = serde_json::from_str(
            r#"[
                {"uuid": "api-org", "capabilities": ["api"]},
                {"uuid": "other", "capabilities": ["raven"]}
            ]"#,
        )
        .unwrap();
        assert_eq!(select_web_organization(&orgs).unwrap().uuid, "other");

        let orgs: Vec<ClaudeWebOrganization> =
            serde_json::from_str(r#"[{"uuid": "only-api", "capabilities": ["api"]}]"#).unwrap();
        assert_eq!(select_web_organization(&orgs).unwrap().uuid, "only-api");
        assert!(select_web_organization(&[]).is_none());
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
        assert_ne!(desc.as_str(), "");

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
    // Model-Scoped Quota Tests (issue #11)
    // =========================================================================

    /// The routing-risk case: the general windows are below threshold while the
    /// weekly Fable allowance is spent, so the account looks available and is
    /// not. Shape taken from a real `GET /api/oauth/usage` response.
    fn exhausted_fable_json() -> &'static str {
        r#"{
            "five_hour": {"utilization": 0.0, "resets_at": "2030-01-01T05:00:00Z"},
            "seven_day": {"utilization": 56.0, "resets_at": "2030-01-04T00:00:00Z"},
            "seven_day_opus": null,
            "seven_day_sonnet": null,
            "limits": [
                {"kind": "session", "group": "session", "percent": 0, "severity": "normal",
                 "resets_at": "2030-01-01T05:00:00Z", "scope": null, "is_active": false},
                {"kind": "weekly_all", "group": "weekly", "percent": 56, "severity": "normal",
                 "resets_at": "2030-01-04T00:00:00Z", "scope": null, "is_active": false},
                {"kind": "weekly_scoped", "group": "weekly", "percent": 100, "severity": "critical",
                 "resets_at": "2030-01-04T19:59:59Z",
                 "scope": {"model": {"display_name": "Fable", "id": null}, "surface": null},
                 "is_active": true}
            ]
        }"#
    }

    #[test]
    fn parse_usage_response_exposes_scoped_quota() {
        let response: ClaudeOauthUsageResponse =
            serde_json::from_str(exhausted_fable_json()).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        // The general windows are unchanged and still look healthy.
        let primary = snapshot.primary.as_ref().expect("primary");
        assert!(primary.used_percent.abs() < f64::EPSILON);
        let secondary = snapshot.secondary.as_ref().expect("secondary");
        assert!((secondary.used_percent - 56.0).abs() < f64::EPSILON);

        // The scoped quota is what says the account cannot do Fable work.
        assert_eq!(snapshot.scoped.len(), 1);
        let fable = &snapshot.scoped[0];
        assert_eq!(fable.label, "Fable");
        assert_eq!(fable.kind.as_deref(), Some("weekly_scoped"));
        assert_eq!(fable.severity.as_deref(), Some("critical"));
        assert!(fable.is_active);
        assert!((fable.window.used_percent - 100.0).abs() < f64::EPSILON);
        assert_eq!(fable.window.window_minutes, Some(SEVEN_DAY_WINDOW_MINUTES));
        assert!(fable.window.resets_at.is_some());
        assert!(fable.window.reset_description.is_some());
        assert!(fable.is_exhausted());
        assert!(fable.is_near_limit(80.0));

        assert_eq!(
            snapshot.worst_scoped().map(|s| s.label.as_str()),
            Some("Fable")
        );
        assert_eq!(snapshot.exhausted_scoped().len(), 1);
    }

    #[test]
    fn parse_usage_response_keeps_healthy_scoped_quotas() {
        let json = r#"{
            "five_hour": {"utilization": 71.0},
            "seven_day": {"utilization": 56.0},
            "limits": [
                {"kind": "weekly_scoped", "group": "weekly", "percent": 5, "severity": "normal",
                 "scope": {"model": {"display_name": "Fable"}}, "is_active": false},
                {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "severity": "normal",
                 "scope": {"model": {"display_name": "Opus"}}, "is_active": false}
            ]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        // Worst first, so the binding quota reads first.
        let labels: Vec<&str> = snapshot.scoped.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["Fable", "Opus"]);
        assert_eq!(snapshot.exhausted_scoped().len(), 0);
    }

    /// `is_active` marks the window binding the account right now, not the set
    /// of windows that apply — filtering on it would drop the general windows
    /// exactly when a scoped quota is spent.
    #[test]
    fn parse_usage_response_keeps_inactive_limits() {
        let response: ClaudeOauthUsageResponse =
            serde_json::from_str(exhausted_fable_json()).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert!(snapshot.primary.is_some(), "inactive session limit dropped");
        assert!(
            snapshot.secondary.is_some(),
            "inactive weekly limit dropped"
        );
    }

    /// Accounts that report the general windows only through `limits[]`.
    #[test]
    fn parse_usage_response_backfills_windows_from_limits() {
        let json = r#"{
            "limits": [
                {"kind": "session", "group": "session", "percent": 33, "resets_at": "2030-01-01T05:00:00Z"},
                {"kind": "weekly_all", "group": "weekly", "percent": 44}
            ]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        let primary = snapshot.primary.expect("primary from limits[]");
        assert!((primary.used_percent - 33.0).abs() < f64::EPSILON);
        assert_eq!(primary.window_minutes, Some(FIVE_HOUR_WINDOW_MINUTES));
        let secondary = snapshot.secondary.expect("secondary from limits[]");
        assert!((secondary.used_percent - 44.0).abs() < f64::EPSILON);
        assert_eq!(snapshot.scoped.as_slice(), [] as [ScopedWindow; 0]);
    }

    /// A top-level window wins over the limits[] entry for the same thing: the
    /// former carries a float utilization, the latter a rounded percent.
    #[test]
    fn parse_usage_response_prefers_top_level_precision() {
        let json = r#"{
            "five_hour": {"utilization": 33.7},
            "limits": [{"kind": "session", "group": "session", "percent": 34}]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 33.7).abs() < f64::EPSILON);
    }

    /// A limit scoped to something other than a model (a surface) is not a
    /// model quota and must not be reported as one.
    #[test]
    fn parse_usage_response_ignores_non_model_scopes() {
        let json = r#"{
            "limits": [
                {"kind": "weekly_scoped", "group": "weekly", "percent": 90,
                 "scope": {"model": null, "surface": {"display_name": "Claude Code"}}}
            ]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert_eq!(snapshot.scoped.as_slice(), [] as [ScopedWindow; 0]);
    }

    /// A response with no `limits[]` at all still parses, and older payloads
    /// keep the behavior they had.
    #[test]
    fn parse_usage_response_without_limits_is_unchanged() {
        let response: ClaudeOauthUsageResponse =
            serde_json::from_str(sample_usage_json()).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert_eq!(snapshot.scoped.as_slice(), [] as [ScopedWindow; 0]);
        assert!(snapshot.worst_scoped().is_none());
        assert!(snapshot.tertiary.is_some());
    }

    /// A limit with no `percent` says nothing about remaining capacity, and
    /// must not be reported as 0% used.
    #[test]
    fn parse_usage_response_skips_limits_without_a_percentage() {
        let json = r#"{
            "limits": [
                {"kind": "weekly_scoped", "group": "weekly", "severity": "normal",
                 "scope": {"model": {"display_name": "Fable"}}, "is_active": true},
                {"kind": "session", "group": "session"}
            ]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert_eq!(snapshot.scoped.as_slice(), [] as [ScopedWindow; 0]);
        assert!(snapshot.primary.is_none());
    }

    /// An explicit `null` in a field the API normally sends as a bool must not
    /// fail the whole response.
    #[test]
    fn parse_usage_response_tolerates_null_is_active() {
        let json = r#"{
            "limits": [
                {"kind": "weekly_scoped", "group": "weekly", "percent": 100, "is_active": null,
                 "scope": {"model": {"display_name": "Fable"}}}
            ]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert_eq!(snapshot.scoped.len(), 1);
        assert!(!snapshot.scoped[0].is_active);
        assert!(snapshot.scoped[0].is_exhausted());
    }

    /// The model `id` stands in when the API sends no display name.
    #[test]
    fn parse_usage_response_falls_back_to_model_id() {
        let json = r#"{
            "limits": [
                {"kind": "weekly_scoped", "group": "weekly", "percent": 40,
                 "scope": {"model": {"id": "claude-fable-5", "display_name": null}}}
            ]
        }"#;
        let response: ClaudeOauthUsageResponse = serde_json::from_str(json).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        assert_eq!(snapshot.scoped.len(), 1);
        assert_eq!(snapshot.scoped[0].label, "claude-fable-5");
    }

    #[test]
    fn scoped_quota_survives_json_round_trip() {
        let response: ClaudeOauthUsageResponse =
            serde_json::from_str(exhausted_fable_json()).expect("deserialize");
        let snapshot = parse_oauth_usage_response(&response);

        let json = serde_json::to_value(&snapshot).expect("serialize");
        let scoped = json["scoped"].as_array().expect("scoped array");
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0]["label"], "Fable");
        assert_eq!(scoped[0]["severity"], "critical");
        assert_eq!(scoped[0]["isActive"], true);
        assert!((scoped[0]["window"]["usedPercent"].as_f64().expect("pct") - 100.0).abs() < 1e-9);
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
    fn token_from_credentials_json_treats_zero_expiry_as_sentinel() {
        // Claude Code writes `expiresAt: 0` when it handles refresh itself;
        // the real deadline sits in `refreshTokenExpiresAt`. See issue #10.
        let content = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-zero","expiresAt":0,"refreshTokenExpiresAt":1790727709073,"subscriptionType":"max"}}"#;

        assert_eq!(
            token_from_credentials_json(content).as_deref(),
            Some("sk-ant-oat01-zero")
        );
    }

    #[test]
    fn oauth_token_is_expired_sentinels_and_boundaries() {
        let now = 1_700_000_000_000;
        assert!(!oauth_token_is_expired(None, now));
        assert!(!oauth_token_is_expired(Some(0), now));
        assert!(!oauth_token_is_expired(Some(-1), now));
        assert!(!oauth_token_is_expired(Some(i64::MIN), now));
        assert!(!oauth_token_is_expired(Some(now + 1), now));
        assert!(oauth_token_is_expired(Some(now), now));
        assert!(oauth_token_is_expired(Some(now - 1), now));
        assert!(oauth_token_is_expired(Some(1), now));
    }

    #[test]
    fn token_from_credentials_json_at_uses_injected_clock() {
        let content = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-clock","expiresAt":1000}}"#;
        assert_eq!(
            token_from_credentials_json_at(content, 999).as_deref(),
            Some("sk-ant-oat01-clock")
        );
        assert!(token_from_credentials_json_at(content, 1000).is_none());
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
