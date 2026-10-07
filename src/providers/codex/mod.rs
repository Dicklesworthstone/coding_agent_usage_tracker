//! Codex (`OpenAI`) provider implementation.
//!
//! Strategies, in `CodexBar`'s auto order:
//!
//! 1. `codex-oauth` — the `ChatGPT` access token in Codex's `auth.json` (or a
//!    token-account bearer token) against the `wham/usage` endpoint.
//! 2. `codex-cli-rpc` — `codex app-server` over JSON-RPC
//!    (`account/rateLimits/read`, `account/read`). The CLI refreshes its own
//!    tokens, so this is also the recovery path for an expired `auth.json`.
//! 3. `codex-web` (`--source web` or a cookie credential) — a `chatgpt.com`
//!    session cookie exchanged for an access token via `/api/auth/session`,
//!    then the same `wham/usage` call.
//!
//! Source labels: `oauth`, `cli`, `web`.

mod rpc;

use std::fs;
use std::path::PathBuf;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::core::fetch_plan::{FetchContext, FetchKind, FetchPlan, FetchStrategy, ProviderFetch};
use crate::core::models::{
    CreditsSnapshot, ProviderCostSnapshot, ProviderIdentity, RateWindow, ScopedWindow,
    UsageSnapshot,
};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::common;

/// CLI binary name.
const CLI_NAME: &str = "codex";

/// Default `ChatGPT` backend base URL.
const DEFAULT_CHATGPT_BASE_URL: &str = "https://chatgpt.com/backend-api/";

/// Usage path under a `ChatGPT` `/backend-api` base.
const CHATGPT_USAGE_PATH: &str = "/wham/usage";

/// Usage path under any other (Codex API) base.
const CODEX_USAGE_PATH: &str = "/api/codex/usage";

/// `ChatGPT` session endpoint that trades a session cookie for an access token.
const CHATGPT_SESSION_URL: &str = "https://chatgpt.com/api/auth/session";

/// Refresh margin: a token this close to `exp` is treated as expired.
const TOKEN_REFRESH_MARGIN_SECS: i64 = 5 * 60;

/// Session window length (5 hours).
const SESSION_WINDOW_MINUTES: i32 = 300;

/// Weekly window length.
const WEEKLY_WINDOW_MINUTES: i32 = 10_080;

/// Environment variables holding a `chatgpt.com` cookie header.
const COOKIE_ENV_VARS: &[&str] = &["CODEX_COOKIE", "CHATGPT_COOKIE"];

// =============================================================================
// Fetch Plan
// =============================================================================

/// Create fetch plan for Codex.
#[must_use]
pub fn fetch_plan() -> FetchPlan {
    FetchPlan::new(
        Provider::Codex,
        vec![
            FetchStrategy {
                id: "codex-oauth",
                kind: FetchKind::OAuth,
                is_available: |ctx| oauth_credentials_for(ctx).is_some(),
                should_fallback: oauth_should_fallback,
            },
            FetchStrategy {
                id: "codex-cli-rpc",
                kind: FetchKind::Cli,
                // The CLI answers for whichever account it is logged into, so it
                // cannot stand in for an explicitly selected token account.
                is_available: |ctx| ctx.account_token().is_none() && is_cli_available(),
                should_fallback: |_| true,
            },
            FetchStrategy {
                id: "codex-web",
                kind: FetchKind::WebDashboard,
                is_available: |ctx| cookie_header_for(ctx).is_some(),
                should_fallback: |_| false,
            },
        ],
    )
}

/// Run one strategy from [`fetch_plan`].
///
/// # Errors
/// Returns the strategy's error, or an error for an unknown strategy id.
pub async fn fetch(strategy_id: &str, ctx: &FetchContext) -> Result<ProviderFetch> {
    match strategy_id {
        "codex-oauth" => {
            let creds = oauth_credentials_for(ctx).ok_or_else(|| {
                common::missing_credential(
                    Provider::Codex,
                    "run `codex login` (no ChatGPT tokens in <CODEX_HOME>/auth.json)",
                )
            })?;
            fetch_oauth(&creds).await
        }
        "codex-cli-rpc" => rpc::fetch_via_app_server().await,
        "codex-web" => {
            let cookie = cookie_header_for(ctx).ok_or_else(|| {
                common::missing_credential(
                    Provider::Codex,
                    "set CODEX_COOKIE to a chatgpt.com Cookie header or add a token account",
                )
            })?;
            fetch_web(&cookie).await
        }
        _ => Err(crate::providers::unknown_strategy(
            Provider::Codex,
            strategy_id,
        )),
    }
}

/// `CodexBar` falls back from OAuth to the CLI only for states the CLI can
/// repair (missing, expired or rejected credentials). Network, server and
/// decode failures surface as-is instead of spawning `codex app-server`.
const fn oauth_should_fallback(error: &CautError) -> bool {
    matches!(
        error,
        CautError::AuthExpired { .. }
            | CautError::AuthInvalid { .. }
            | CautError::AuthNotConfigured { .. }
    )
}

/// Check if the Codex CLI is available.
fn is_cli_available() -> bool {
    which::which(CLI_NAME).is_ok()
}

// =============================================================================
// Credentials
// =============================================================================

/// The Codex home directory: `CODEX_HOME`, else `~/.codex`.
fn codex_home() -> Option<PathBuf> {
    common::dir_from_env_or_home("CODEX_HOME", ".codex")
}

/// `ChatGPT` OAuth credentials for the usage API.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCredentials {
    access_token: String,
    id_token: Option<String>,
    account_id: Option<String>,
    /// Access-token expiry from its JWT `exp` claim.
    expires_at: Option<DateTime<Utc>>,
}

impl CodexCredentials {
    /// Credentials from a bare access token (a token account's bearer token).
    fn from_access_token(token: &str) -> Self {
        let token = common::strip_bearer(token).to_string();
        let claims = decode_jwt_claims(&token);
        Self {
            account_id: claims.as_ref().and_then(account_id_from_claims),
            expires_at: claims.as_ref().and_then(expiry_from_claims),
            access_token: token,
            id_token: None,
        }
    }

    /// Whether the access token is expired or about to be.
    fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        self.expires_at
            .is_some_and(|exp| (exp - now).num_seconds() <= TOKEN_REFRESH_MARGIN_SECS)
    }
}

/// Parse Codex's `auth.json`.
///
/// Returns `None` for an API-key-only file: `wham/usage` needs a `ChatGPT`
/// login, so such a setup is served by the CLI strategy instead.
fn parse_auth_json(content: &str) -> Option<CodexCredentials> {
    let json: Value = serde_json::from_str(content).ok()?;
    let tokens = json.get("tokens")?;
    let access_token = string_field(tokens, &["access_token", "accessToken"])?;
    let id_token = string_field(tokens, &["id_token", "idToken"]);
    let access_claims = decode_jwt_claims(&access_token);
    let id_claims = id_token.as_deref().and_then(decode_jwt_claims);
    let account_id = string_field(tokens, &["account_id", "accountId"])
        .or_else(|| id_claims.as_ref().and_then(account_id_from_claims))
        .or_else(|| access_claims.as_ref().and_then(account_id_from_claims));
    Some(CodexCredentials {
        expires_at: access_claims.as_ref().and_then(expiry_from_claims),
        access_token,
        id_token,
        account_id,
    })
}

/// Read OAuth credentials from `<codex_home>/auth.json`.
fn read_local_credentials() -> Option<CodexCredentials> {
    let path = codex_home()?.join("auth.json");
    let content = fs::read_to_string(path).ok()?;
    parse_auth_json(&content)
}

/// Whether a token-account credential is a cookie header rather than a
/// bearer token. JWTs and opaque tokens never contain `=` or `;`.
fn is_cookie_credential(token: &str) -> bool {
    let trimmed = common::strip_bearer(token);
    trimmed.contains('=') || trimmed.contains(';')
}

/// OAuth credentials for this fetch: a selected bearer-token account, else
/// the local `auth.json`, else an active bearer-token account.
fn oauth_credentials_for(ctx: &FetchContext) -> Option<CodexCredentials> {
    if let Some(token) = ctx.account_token() {
        return (!is_cookie_credential(token)).then(|| CodexCredentials::from_access_token(token));
    }
    read_local_credentials().or_else(|| {
        common::active_token_account(Provider::Codex)
            .filter(|t| !is_cookie_credential(t))
            .map(|t| CodexCredentials::from_access_token(&t))
    })
}

/// A `chatgpt.com` cookie header for the web strategy.
fn cookie_header_for(ctx: &FetchContext) -> Option<String> {
    if let Some(token) = ctx.account_token() {
        return is_cookie_credential(token).then(|| common::normalize_cookie_header(token));
    }
    common::env_secret(COOKIE_ENV_VARS)
        .or_else(|| {
            common::active_token_account(Provider::Codex).filter(|t| is_cookie_credential(t))
        })
        .map(|raw| common::normalize_cookie_header(&raw))
}

// =============================================================================
// JWT helpers
// =============================================================================

/// Decode a JWT's payload (no signature check: the claims only label the
/// account, they grant nothing).
fn decode_jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (_header, payload, _signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.is_object().then_some(value)
}

/// The `ChatGPT` account id from JWT claims.
fn account_id_from_claims(claims: &Value) -> Option<String> {
    non_empty(claims.get("chatgpt_account_id"))
        .or_else(|| non_empty(claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")))
        .or_else(|| {
            claims
                .get("organizations")?
                .as_array()?
                .iter()
                .find_map(|org| non_empty(org.get("id")))
        })
}

/// The `exp` claim as a timestamp.
fn expiry_from_claims(claims: &Value) -> Option<DateTime<Utc>> {
    claims
        .get("exp")
        .and_then(Value::as_i64)
        .and_then(|exp| DateTime::from_timestamp(exp, 0))
}

/// Email and plan from id-token claims.
fn identity_from_id_token(id_token: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(claims) = id_token.and_then(decode_jwt_claims) else {
        return (None, None);
    };
    let email = non_empty(claims.get("email"))
        .or_else(|| non_empty(claims.pointer("/https:~1~1api.openai.com~1profile/email")));
    let plan = non_empty(claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type"))
        .or_else(|| non_empty(claims.get("chatgpt_plan_type")));
    (email, plan)
}

// =============================================================================
// Usage API
// =============================================================================

/// Read `chatgpt_base_url` from Codex's `config.toml` text.
fn parse_chatgpt_base_url(config: &str) -> Option<String> {
    config.lines().find_map(|raw| {
        let line = raw.split('#').next()?.trim();
        let (key, value) = line.split_once('=')?;
        if key.trim() != "chatgpt_base_url" {
            return None;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Normalize a base URL: drop trailing slashes and add `/backend-api` to a
/// bare `chatgpt.com` / `chat.openai.com` host.
fn normalize_base_url(value: &str) -> String {
    let mut base = value.trim().to_string();
    if base.is_empty() {
        base = DEFAULT_CHATGPT_BASE_URL.to_string();
    }
    while base.ends_with('/') {
        base.pop();
    }
    if (base.starts_with("https://chatgpt.com") || base.starts_with("https://chat.openai.com"))
        && !base.contains("/backend-api")
    {
        base.push_str("/backend-api");
    }
    base
}

/// The usage endpoint for a base URL.
fn usage_url_for_base(base: &str) -> String {
    let base = normalize_base_url(base);
    let path = if base.contains("/backend-api") {
        CHATGPT_USAGE_PATH
    } else {
        CODEX_USAGE_PATH
    };
    format!("{base}{path}")
}

/// The usage endpoint, honoring `chatgpt_base_url` in Codex's config.
fn usage_url() -> String {
    let base = codex_home()
        .and_then(|home| fs::read_to_string(home.join("config.toml")).ok())
        .and_then(|config| parse_chatgpt_base_url(&config))
        .unwrap_or_else(|| DEFAULT_CHATGPT_BASE_URL.to_string());
    usage_url_for_base(&base)
}

/// Fetch usage from `wham/usage` with OAuth credentials.
///
/// # Errors
/// Returns `AuthExpired` for an expired token (the CLI strategy can refresh
/// it), `AuthInvalid` for a rejected one, or a network/parse error.
async fn fetch_oauth(creds: &CodexCredentials) -> Result<ProviderFetch> {
    if creds.needs_refresh(Utc::now()) {
        return Err(CautError::AuthExpired {
            provider: Provider::Codex.cli_name().to_string(),
        });
    }
    let value = request_usage(&creds.access_token, creds.account_id.as_deref()).await?;
    let (email, plan) = identity_from_id_token(creds.id_token.as_deref());
    Ok(map_usage_response(&value, email, plan, Utc::now()))
}

/// `GET` the usage endpoint.
async fn request_usage(access_token: &str, account_id: Option<&str>) -> Result<Value> {
    let client = common::http_client()?;
    let mut request = client
        .get(usage_url())
        .bearer_auth(access_token)
        .header("Accept", "application/json");
    if let Some(account_id) = account_id.filter(|id| !id.is_empty()) {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    common::send_json(Provider::Codex, request).await
}

/// Fetch usage with a `chatgpt.com` session cookie.
///
/// # Errors
/// Returns `AuthInvalid` when the session yields no access token, or the
/// usage request's error.
async fn fetch_web(cookie_header: &str) -> Result<ProviderFetch> {
    let client = common::http_client()?;
    let session: Value = common::send_json(
        Provider::Codex,
        client
            .get(CHATGPT_SESSION_URL)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json"),
    )
    .await?;
    let access_token =
        non_empty(session.get("accessToken")).ok_or_else(|| CautError::AuthInvalid {
            provider: Provider::Codex.cli_name().to_string(),
            reason: "chatgpt.com session cookie is not signed in (no accessToken)".to_string(),
        })?;
    let account_id = decode_jwt_claims(&access_token)
        .as_ref()
        .and_then(account_id_from_claims);
    let value = request_usage(&access_token, account_id.as_deref()).await?;
    let email = non_empty(session.pointer("/user/email"));
    Ok(map_usage_response(&value, email, None, Utc::now()))
}

// =============================================================================
// Response mapping
// =============================================================================

/// Map a `wham/usage` response into a usage snapshot plus credits.
fn map_usage_response(
    value: &Value,
    email: Option<String>,
    token_plan: Option<String>,
    now: DateTime<Utc>,
) -> ProviderFetch {
    let rate_limit = value.get("rate_limit");
    let (primary, secondary) = normalize_windows(
        rate_limit
            .and_then(|r| r.get("primary_window"))
            .and_then(window_from_snapshot),
        rate_limit
            .and_then(|r| r.get("secondary_window"))
            .and_then(window_from_snapshot),
    );

    let plan = non_empty(value.get("plan_type")).or(token_plan);
    let identity = ProviderIdentity {
        account_email: email,
        account_organization: None,
        login_method: plan,
    };

    let individual_limit = first_object(value, &["individual_limit", "individualLimit"])
        .or_else(|| {
            rate_limit.and_then(|r| first_object(r, &["individual_limit", "individualLimit"]))
        })
        .or_else(|| {
            first_object(value, &["spend_control", "spendControl"])
                .and_then(|s| first_object(s, &["individual_limit", "individualLimit"]))
        });

    let usage = UsageSnapshot {
        primary,
        secondary,
        scoped: additional_windows(value.get("additional_rate_limits")),
        provider_cost: individual_limit.and_then(|l| credit_limit_cost(l, now)),
        updated_at: now,
        identity: Some(identity),
        ..UsageSnapshot::empty()
    };
    let credits = value.get("credits").and_then(|c| credits_from(c, now));
    ProviderFetch { usage, credits }
}

/// One `{used_percent, reset_at, limit_window_seconds}` window.
fn window_from_snapshot(value: &Value) -> Option<RateWindow> {
    let used_percent = number(value.get("used_percent"))?;
    let seconds = number(value.get("limit_window_seconds")).unwrap_or(0.0);
    #[allow(clippy::cast_possible_truncation)] // window lengths are small
    let window_minutes = (seconds > 0.0).then(|| (seconds / 60.0) as i32);
    #[allow(clippy::cast_possible_truncation)] // epoch seconds fit in i64
    let resets_at = number(value.get("reset_at"))
        .filter(|v| *v > 0.0)
        .and_then(|v| DateTime::from_timestamp(v as i64, 0));
    Some(RateWindow {
        used_percent,
        window_minutes,
        resets_at,
        reset_description: None,
    })
}

/// Which lane a window belongs to, by its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowRole {
    Session,
    Weekly,
    Unknown,
}

const fn role(window: &RateWindow) -> WindowRole {
    match window.window_minutes {
        Some(SESSION_WINDOW_MINUTES) => WindowRole::Session,
        Some(WEEKLY_WINDOW_MINUTES) => WindowRole::Weekly,
        _ => WindowRole::Unknown,
    }
}

/// Put the session window in `primary` and the weekly one in `secondary`,
/// whichever slot the server used (`CodexBar`'s `CodexRateWindowNormalizer`).
fn normalize_windows(
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
) -> (Option<RateWindow>, Option<RateWindow>) {
    match (primary, secondary) {
        (Some(p), Some(s)) => match (role(&p), role(&s)) {
            (WindowRole::Weekly, WindowRole::Session | WindowRole::Unknown) => (Some(s), Some(p)),
            _ => (Some(p), Some(s)),
        },
        (Some(p), None) => match role(&p) {
            WindowRole::Weekly => (None, Some(p)),
            _ => (Some(p), None),
        },
        (None, Some(s)) => match role(&s) {
            WindowRole::Weekly => (None, Some(s)),
            _ => (Some(s), None),
        },
        (None, None) => (None, None),
    }
}

/// Map `additional_rate_limits` (model-specific limits such as Codex Spark)
/// into scoped windows.
fn additional_windows(value: Option<&Value>) -> Vec<ScopedWindow> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut used_ids: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for entry in entries {
        let limit_name = non_empty(entry.get("limit_name"));
        let feature = non_empty(entry.get("metered_feature"));
        let rate_limit = entry.get("rate_limit");
        let primary = rate_limit.and_then(|r| r.get("primary_window"));
        let secondary = rate_limit.and_then(|r| r.get("secondary_window"));

        let is_spark = [&limit_name, &feature]
            .iter()
            .filter_map(|v| v.as_deref())
            .any(|v| v.to_lowercase().contains("spark"));

        let mut push = |id: String, title: String, snapshot: &Value| {
            if used_ids.contains(&id) {
                return;
            }
            if let Some(window) = window_from_snapshot(snapshot) {
                used_ids.push(id.clone());
                out.push(ScopedWindow {
                    label: title,
                    kind: Some(id),
                    severity: None,
                    is_active: false,
                    window,
                });
            }
        };

        if is_spark {
            for (snapshot, fallback_weekly) in [(primary, false), (secondary, true)] {
                let Some(snapshot) = snapshot.filter(|s| s.is_object()) else {
                    continue;
                };
                let minutes = number(snapshot.get("limit_window_seconds")).unwrap_or(0.0) / 60.0;
                let weekly = if minutes > 0.0 && minutes <= 6.0 * 60.0 {
                    false
                } else if minutes >= 6.0 * 24.0 * 60.0 {
                    true
                } else {
                    fallback_weekly
                };
                let (id, title) = if weekly {
                    ("codex-spark-weekly", "Codex Spark Weekly")
                } else {
                    ("codex-spark", "Codex Spark 5-hour")
                };
                push(id.to_string(), title.to_string(), snapshot);
            }
            continue;
        }

        // Model-specific limits report utilization in the primary window;
        // the secondary is a fallback.
        let Some(snapshot) = primary.or(secondary).filter(|s| s.is_object()) else {
            continue;
        };
        let Some(source) = feature.clone().or_else(|| limit_name.clone()) else {
            continue;
        };
        let slug = slugify(&source);
        if slug.is_empty() {
            continue;
        }
        let title = limit_name
            .or(feature)
            .unwrap_or_else(|| "Codex extra limit".to_string());
        push(format!("codex-{slug}"), title, snapshot);
    }
    out
}

/// Lowercase alphanumerics joined by single dashes.
fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// A team/business monthly credit cap as a provider cost.
fn credit_limit_cost(limit_value: &Value, now: DateTime<Utc>) -> Option<ProviderCostSnapshot> {
    let limit = number(limit_value.get("limit")).filter(|l| *l > 0.0)?;
    let remaining_percent = number(
        limit_value
            .get("remainingPercent")
            .or_else(|| limit_value.get("remaining_percent")),
    );
    let used = number(limit_value.get("used")).unwrap_or_else(|| {
        remaining_percent.map_or(0.0, |rp| limit * (100.0 - rp).clamp(0.0, 100.0) / 100.0)
    });
    #[allow(clippy::cast_possible_truncation)] // epoch seconds fit in i64
    let resets_at = ["resetsAt", "resets_at", "reset_at"]
        .iter()
        .find_map(|key| number(limit_value.get(*key)))
        .filter(|v| *v > 0.0)
        .and_then(|v| DateTime::from_timestamp(v as i64, 0));
    Some(ProviderCostSnapshot {
        used: used.max(0.0),
        limit,
        currency_code: "credits".to_string(),
        period: Some("Monthly".to_string()),
        resets_at,
        updated_at: now,
    })
}

/// Credits balance from `{has_credits, unlimited, balance}`.
///
/// `None` when the account has no metered credits (unlimited or none) and
/// reports no balance.
fn credits_from(value: &Value, now: DateTime<Utc>) -> Option<CreditsSnapshot> {
    let has_credits = value
        .get("has_credits")
        .or_else(|| value.get("hasCredits"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let unlimited = value
        .get("unlimited")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let balance = number(value.get("balance"));
    let metered = has_credits && !unlimited;
    if balance.is_none() && !metered {
        return None;
    }
    Some(CreditsSnapshot {
        remaining: balance.unwrap_or(0.0),
        events: Vec::new(),
        updated_at: now,
    })
}

// =============================================================================
// JSON helpers
// =============================================================================

/// A number that may be encoded as a JSON number or a numeric string.
fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

/// A trimmed, non-empty string.
fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The first present string field among alternate spellings.
fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| non_empty(value.get(*key)))
}

/// The first present object field among alternate spellings.
fn first_object<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| value.get(*key).filter(|v| v.is_object()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn jwt(claims: &Value) -> String {
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.sig",
            engine.encode(br#"{"alg":"none"}"#),
            engine.encode(claims.to_string())
        )
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_766_900_000, 0).unwrap()
    }

    // ---------------------------------------------------------------------
    // Fetch plan
    // ---------------------------------------------------------------------

    #[test]
    fn plan_order_matches_codexbar_auto_mode() {
        let plan = fetch_plan();
        let ids: Vec<_> = plan.strategies.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["codex-oauth", "codex-cli-rpc", "codex-web"]);
        assert_eq!(plan.strategies[0].kind, FetchKind::OAuth);
        assert_eq!(plan.strategies[1].kind, FetchKind::Cli);
        assert_eq!(plan.strategies[2].kind, FetchKind::WebDashboard);
    }

    #[test]
    fn bearer_account_routes_to_oauth_only() {
        let plan = fetch_plan();
        let ctx = FetchContext::with_account("work", format!("Bearer {}", jwt(&json!({"exp": 1}))));
        assert!((plan.strategies[0].is_available)(&ctx));
        assert!(!(plan.strategies[1].is_available)(&ctx));
        assert!(!(plan.strategies[2].is_available)(&ctx));
    }

    #[test]
    fn cookie_account_routes_to_web_only() {
        let plan = fetch_plan();
        let ctx = FetchContext::with_account(
            "work",
            "Cookie: __Secure-next-auth.session-token=abc; other=1",
        );
        assert!(!(plan.strategies[0].is_available)(&ctx));
        assert!(!(plan.strategies[1].is_available)(&ctx));
        assert!((plan.strategies[2].is_available)(&ctx));
        assert_eq!(
            cookie_header_for(&ctx).as_deref(),
            Some("__Secure-next-auth.session-token=abc; other=1")
        );
    }

    #[test]
    fn oauth_fallback_only_for_repairable_errors() {
        assert!(oauth_should_fallback(&CautError::AuthExpired {
            provider: "codex".into()
        }));
        assert!(oauth_should_fallback(&CautError::AuthInvalid {
            provider: "codex".into(),
            reason: "401".into()
        }));
        assert!(!oauth_should_fallback(&CautError::Network("down".into())));
        assert!(!oauth_should_fallback(&CautError::ParseResponse(
            "bad".into()
        )));
    }

    // ---------------------------------------------------------------------
    // Credentials
    // ---------------------------------------------------------------------

    #[test]
    fn parse_auth_json_reads_tokens_and_jwt_account() {
        let id_token = jwt(&json!({
            "email": "user@example.com",
            "https://api.openai.com/auth": {"chatgpt_account_id": "acct-jwt", "chatgpt_plan_type": "pro"}
        }));
        let access = jwt(&json!({"exp": 2_000_000_000}));
        let content = json!({
            "OPENAI_API_KEY": null,
            "tokens": {"id_token": id_token, "access_token": access, "refresh_token": "r"}
        })
        .to_string();
        let creds = parse_auth_json(&content).unwrap();
        assert_eq!(creds.account_id.as_deref(), Some("acct-jwt"));
        assert_eq!(creds.expires_at, DateTime::from_timestamp(2_000_000_000, 0));
        assert!(!creds.needs_refresh(now()));
        assert_eq!(
            identity_from_id_token(creds.id_token.as_deref()),
            (Some("user@example.com".into()), Some("pro".into()))
        );
    }

    #[test]
    fn parse_auth_json_prefers_explicit_account_id_and_camel_case() {
        let content = json!({
            "tokens": {"accessToken": "opaque", "refreshToken": "r", "accountId": "acct-1"}
        })
        .to_string();
        let creds = parse_auth_json(&content).unwrap();
        assert_eq!(creds.access_token, "opaque");
        assert_eq!(creds.account_id.as_deref(), Some("acct-1"));
        assert_eq!(creds.expires_at, None);
    }

    #[test]
    fn parse_auth_json_rejects_api_key_only_and_garbage() {
        assert!(parse_auth_json(r#"{"OPENAI_API_KEY": "sk-123"}"#).is_none());
        assert!(parse_auth_json(r#"{"tokens": {"access_token": ""}}"#).is_none());
        assert!(parse_auth_json("not json").is_none());
    }

    #[test]
    fn parse_auth_json_reads_repo_fixture() {
        let content = include_str!("../../../tests/fixtures/codex/auth_oauth.json");
        let creds = parse_auth_json(content).unwrap();
        assert_eq!(creds.account_id.as_deref(), Some("acct_test_123456"));
        let (email, plan) = identity_from_id_token(creds.id_token.as_deref());
        assert_eq!(email.as_deref(), Some("user@example.com"));
        assert_eq!(plan.as_deref(), Some("pro"));
    }

    #[test]
    fn expired_token_needs_refresh() {
        let creds = CodexCredentials::from_access_token(&jwt(&json!({"exp": 1_766_900_100})));
        assert!(creds.needs_refresh(now()), "within the 5-minute margin");
        let creds = CodexCredentials::from_access_token(&jwt(&json!({"exp": 1_766_990_000})));
        assert!(!creds.needs_refresh(now()));
        let creds = CodexCredentials::from_access_token("opaque-token");
        assert!(
            !creds.needs_refresh(now()),
            "no exp claim: let the server decide"
        );
    }

    #[test]
    fn account_id_from_org_claims() {
        let claims = json!({"organizations": [{"id": ""}, {"id": "org-2"}]});
        assert_eq!(account_id_from_claims(&claims).as_deref(), Some("org-2"));
        assert_eq!(account_id_from_claims(&json!({})), None);
    }

    #[test]
    fn decode_jwt_rejects_malformed_tokens() {
        assert!(decode_jwt_claims("a.b").is_none());
        assert!(decode_jwt_claims("a.b.c.d").is_none());
        assert!(decode_jwt_claims("a.!!!.c").is_none());
    }

    // ---------------------------------------------------------------------
    // Base URL
    // ---------------------------------------------------------------------

    #[test]
    fn base_url_parsing_and_normalization() {
        let config = "model = \"o3\"\n# chatgpt_base_url = \"ignored\"\nchatgpt_base_url = 'https://chatgpt.com/' # trailing\n";
        assert_eq!(
            parse_chatgpt_base_url(config).as_deref(),
            Some("https://chatgpt.com/")
        );
        assert_eq!(parse_chatgpt_base_url("model = 'x'"), None);
        assert_eq!(
            usage_url_for_base("https://chatgpt.com/"),
            "https://chatgpt.com/backend-api/wham/usage"
        );
        assert_eq!(
            usage_url_for_base(DEFAULT_CHATGPT_BASE_URL),
            "https://chatgpt.com/backend-api/wham/usage"
        );
        assert_eq!(
            usage_url_for_base("https://proxy.example.com/"),
            "https://proxy.example.com/api/codex/usage"
        );
        assert_eq!(
            usage_url_for_base(""),
            "https://chatgpt.com/backend-api/wham/usage"
        );
    }

    // ---------------------------------------------------------------------
    // Usage mapping
    // ---------------------------------------------------------------------

    fn spark_response() -> Value {
        json!({
          "plan_type": "pro",
          "rate_limit": {
            "primary_window": {"used_percent": 22, "reset_at": 1_766_948_068, "limit_window_seconds": 18000},
            "secondary_window": {"used_percent": 43, "reset_at": 1_767_407_914, "limit_window_seconds": 604_800}
          },
          "additional_rate_limits": [{
            "limit_name": "GPT-5.3-Codex-Spark",
            "metered_feature": "gpt_5_3_codex_spark",
            "rate_limit": {
              "primary_window": {"used_percent": 30, "reset_at": 1_766_948_068, "limit_window_seconds": 18000},
              "secondary_window": {"used_percent": 100, "reset_at": 1_767_407_914, "limit_window_seconds": 604_800}
            }
          }],
          "credits": {"has_credits": true, "unlimited": false, "balance": "12.5"}
        })
    }

    #[test]
    fn maps_windows_spark_and_credits() {
        let fetch = map_usage_response(&spark_response(), Some("a@b.c".into()), None, now());
        let usage = fetch.usage;
        let primary = usage.primary.unwrap();
        assert!((primary.used_percent - 22.0).abs() < f64::EPSILON);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(
            primary.resets_at,
            DateTime::from_timestamp(1_766_948_068, 0)
        );
        let secondary = usage.secondary.unwrap();
        assert!((secondary.used_percent - 43.0).abs() < f64::EPSILON);
        assert_eq!(secondary.window_minutes, Some(10_080));

        assert_eq!(usage.scoped.len(), 2);
        assert_eq!(usage.scoped[0].label, "Codex Spark 5-hour");
        assert_eq!(usage.scoped[0].kind.as_deref(), Some("codex-spark"));
        assert_eq!(usage.scoped[1].label, "Codex Spark Weekly");
        assert!(usage.scoped[1].is_exhausted());

        let identity = usage.identity.unwrap();
        assert_eq!(identity.account_email.as_deref(), Some("a@b.c"));
        assert_eq!(identity.login_method.as_deref(), Some("pro"));

        let credits = fetch.credits.unwrap();
        assert!((credits.remaining - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn swaps_windows_reported_in_the_wrong_slots() {
        let value = json!({"rate_limit": {
            "primary_window": {"used_percent": 70, "reset_at": 0, "limit_window_seconds": 604_800},
            "secondary_window": {"used_percent": 10, "reset_at": 0, "limit_window_seconds": 18000}
        }});
        let usage = map_usage_response(&value, None, Some("plus".into()), now()).usage;
        assert_eq!(usage.primary.as_ref().unwrap().window_minutes, Some(300));
        assert_eq!(
            usage.secondary.as_ref().unwrap().window_minutes,
            Some(10_080)
        );
        assert_eq!(
            usage.primary.unwrap().resets_at,
            None,
            "reset_at 0 means unknown"
        );
        assert_eq!(
            usage.identity.unwrap().login_method.as_deref(),
            Some("plus"),
            "token plan fills in when the response has none"
        );
    }

    #[test]
    fn lone_weekly_window_lands_in_secondary() {
        let value = json!({"rate_limit": {
            "primary_window": {"used_percent": 5, "reset_at": 1, "limit_window_seconds": 604_800},
            "secondary_window": null
        }});
        let usage = map_usage_response(&value, None, None, now()).usage;
        assert!(usage.primary.is_none());
        assert!(usage.secondary.is_some());
    }

    #[test]
    fn missing_or_malformed_windows_do_not_fail() {
        let value = json!({"rate_limit": {"primary_window": {"reset_at": 1}}, "plan_type": "free"});
        let fetch = map_usage_response(&value, None, None, now());
        assert!(fetch.usage.primary.is_none());
        assert!(fetch.usage.secondary.is_none());
        assert!(!fetch.usage.has_quota());
        assert!(fetch.credits.is_none());
        let empty = map_usage_response(&json!({}), None, None, now());
        assert!(!empty.usage.has_quota());
    }

    #[test]
    fn generic_additional_limit_uses_slug_and_title() {
        let value = json!({"additional_rate_limits": [
            {"limit_name": "Code Review", "metered_feature": "code_review",
             "rate_limit": {"primary_window": {"used_percent": 12.5, "reset_at": 0, "limit_window_seconds": 0}}},
            {"limit_name": "Dup", "metered_feature": "code_review",
             "rate_limit": {"primary_window": {"used_percent": 99, "reset_at": 0, "limit_window_seconds": 0}}},
            {"limit_name": "", "metered_feature": null, "rate_limit": {"primary_window": {"used_percent": 1}}},
            "not an object"
        ]});
        let scoped = map_usage_response(&value, None, None, now()).usage.scoped;
        assert_eq!(
            scoped.len(),
            1,
            "duplicates and unnamed entries are dropped"
        );
        assert_eq!(scoped[0].label, "Code Review");
        assert_eq!(scoped[0].kind.as_deref(), Some("codex-code-review"));
        assert_eq!(scoped[0].window.window_minutes, None);
    }

    #[test]
    fn individual_limit_becomes_monthly_credit_cost() {
        let value = json!({
            "spend_control": {"individual_limit": {"limit": "1000", "remaining_percent": 25, "reset_at": 1_767_000_000}}
        });
        let cost = map_usage_response(&value, None, None, now())
            .usage
            .provider_cost
            .unwrap();
        assert!((cost.limit - 1000.0).abs() < f64::EPSILON);
        assert!((cost.used - 750.0).abs() < f64::EPSILON);
        assert_eq!(cost.currency_code, "credits");
        assert_eq!(cost.resets_at, DateTime::from_timestamp(1_767_000_000, 0));

        // Root wins over rate_limit, which wins over spend_control.
        let value = json!({
            "individual_limit": {"limit": 10, "used": 3},
            "rate_limit": {"individual_limit": {"limit": 20, "used": 4}},
            "spend_control": {"individual_limit": {"limit": 30, "used": 5}}
        });
        let cost = map_usage_response(&value, None, None, now())
            .usage
            .provider_cost
            .unwrap();
        assert!((cost.limit - 10.0).abs() < f64::EPSILON);

        // A zero cap is no cap.
        let value = json!({"individual_limit": {"limit": 0, "used": 3}});
        assert!(
            map_usage_response(&value, None, None, now())
                .usage
                .provider_cost
                .is_none()
        );
    }

    #[test]
    fn credits_rules() {
        assert!(credits_from(&json!({"has_credits": false, "unlimited": false}), now()).is_none());
        assert!(credits_from(&json!({"has_credits": true, "unlimited": true}), now()).is_none());
        let metered =
            credits_from(&json!({"has_credits": true, "unlimited": false}), now()).unwrap();
        assert!(metered.remaining.abs() < f64::EPSILON);
        let numeric = credits_from(&json!({"balance": 4.25}), now()).unwrap();
        assert!((numeric.remaining - 4.25).abs() < f64::EPSILON);
    }

    #[test]
    fn slugify_collapses_separators() {
        assert_eq!(slugify("GPT-5.3 Codex__Spark!"), "gpt-5-3-codex-spark");
        assert_eq!(slugify("---"), "");
    }

    #[test]
    fn number_accepts_numbers_and_numeric_strings() {
        assert_eq!(number(Some(&json!(3))), Some(3.0));
        assert_eq!(number(Some(&json!(" 2.5 "))), Some(2.5));
        assert_eq!(number(Some(&json!("x"))), None);
        assert_eq!(number(Some(&json!(null))), None);
        assert_eq!(number(None), None);
    }
}
