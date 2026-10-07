//! Helpers shared by provider fetchers.
//!
//! Credential discovery, HTTP status mapping and the timestamp shapes that
//! provider APIs return are the same problem for every provider, so they live
//! here instead of being re-solved per module.

use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::core::fetch_plan::FetchContext;
use crate::core::http::{DEFAULT_TIMEOUT, build_client};
use crate::core::models::RateWindow;
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::storage::paths::AppPaths;
use crate::storage::token_accounts::TokenAccountStore;

/// Longest response-body excerpt kept in an error message.
const ERROR_BODY_EXCERPT: usize = 200;

// =============================================================================
// Paths
// =============================================================================

/// The user's home directory.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

/// A directory from an environment variable, falling back to `~/<default>`.
///
/// Mirrors how CLIs such as `codex` (`CODEX_HOME`) and `gemini` let a user
/// relocate their config directory.
#[must_use]
pub fn dir_from_env_or_home(env_var: &str, default_relative: &str) -> Option<PathBuf> {
    if let Ok(value) = std::env::var(env_var) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    home_dir().map(|h| h.join(default_relative))
}

// =============================================================================
// Credentials
// =============================================================================

/// The first non-empty value among the given environment variables.
#[must_use]
pub fn env_secret(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// The keyring service caut stores provider secrets under.
pub const KEYRING_SERVICE: &str = "caut";

/// Keyring account name for a provider's API key: `<cli-name>-api-key`.
#[must_use]
pub fn keyring_account(provider: Provider) -> String {
    format!("{}-api-key", provider.cli_name())
}

/// A provider secret stored in the platform keyring by caut.
#[must_use]
pub fn keyring_secret(provider: Provider) -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_account(provider)).ok()?;
    entry
        .get_password()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The active token account's credential for a provider, if any are stored.
#[must_use]
pub fn active_token_account(provider: Provider) -> Option<String> {
    let store = TokenAccountStore::load(&AppPaths::new().token_accounts_file()).ok()?;
    store
        .get_active(provider)
        .map(|a| a.token.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Resolve a provider credential (API key, cookie header or token).
///
/// Order, first match wins:
/// 1. The token account selected for this fetch (`--account`, `--all-accounts`).
/// 2. The given environment variables, in order.
/// 3. The caut keyring entry (`caut` / `<provider>-api-key`).
/// 4. The provider's active entry in `token-accounts.json`.
#[must_use]
pub fn resolve_secret(
    ctx: &FetchContext,
    provider: Provider,
    env_names: &[&str],
) -> Option<String> {
    if let Some(token) = ctx.account_token() {
        return Some(token.to_string());
    }
    env_secret(env_names)
        .or_else(|| keyring_secret(provider))
        .or_else(|| active_token_account(provider))
}

/// Strip a leading `Bearer ` from a pasted token.
#[must_use]
pub fn strip_bearer(token: &str) -> &str {
    let trimmed = token.trim();
    trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .map_or(trimmed, str::trim)
}

/// Look up one cookie's value in a `Cookie:` header string.
#[must_use]
pub fn cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    let header = cookie_header.trim();
    let header = header
        .strip_prefix("Cookie:")
        .or_else(|| header.strip_prefix("cookie:"))
        .unwrap_or(header);
    header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key.trim() == name).then(|| value.trim())
    })
}

/// Normalize a pasted cookie header: drop a leading `Cookie:` and whitespace.
#[must_use]
pub fn normalize_cookie_header(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

// =============================================================================
// HTTP
// =============================================================================

/// The default HTTP client for provider API calls.
///
/// # Errors
/// Returns an error if the client cannot be constructed.
pub fn http_client() -> Result<Client> {
    build_client(DEFAULT_TIMEOUT)
}

/// Map a transport error into the matching [`CautError`].
#[must_use]
pub fn map_send_error(provider: Provider, err: &reqwest::Error) -> CautError {
    if err.is_timeout() {
        CautError::TimeoutWithProvider {
            provider: provider.cli_name().to_string(),
            seconds: DEFAULT_TIMEOUT.as_secs(),
        }
    } else if err.is_connect() {
        CautError::Network(format!("{}: {err}", provider.cli_name()))
    } else {
        CautError::Network(err.to_string())
    }
}

/// Map a non-success HTTP status into the matching [`CautError`].
#[must_use]
pub fn status_error(provider: Provider, status: StatusCode, body: &str) -> CautError {
    let name = provider.cli_name().to_string();
    let excerpt: String = body.trim().chars().take(ERROR_BODY_EXCERPT).collect();
    match status.as_u16() {
        401 | 403 => CautError::AuthInvalid {
            provider: name,
            reason: if excerpt.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {excerpt}")
            },
        },
        429 => CautError::RateLimited {
            provider: name,
            retry_after: None,
            message: if excerpt.is_empty() {
                "HTTP 429".to_string()
            } else {
                excerpt
            },
        },
        code => CautError::ProviderApiError {
            provider: name,
            status_code: Some(code),
            message: if excerpt.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {excerpt}")
            },
        },
    }
}

/// Send a request and return the response if the status is a success.
///
/// # Errors
/// Returns a transport error, or a status-specific error for a non-2xx reply.
pub async fn send_checked(provider: Provider, request: RequestBuilder) -> Result<Response> {
    let response = request
        .send()
        .await
        .map_err(|e| map_send_error(provider, &e))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(status_error(provider, status, &body))
}

/// Send a request and decode a JSON response body.
///
/// # Errors
/// Returns a transport error, a status-specific error, or a parse error.
pub async fn send_json<T: DeserializeOwned>(
    provider: Provider,
    request: RequestBuilder,
) -> Result<T> {
    let response = send_checked(provider, request).await?;
    let text = response
        .text()
        .await
        .map_err(|e| map_send_error(provider, &e))?;
    serde_json::from_str(&text)
        .map_err(|e| CautError::ParseResponse(format!("{}: {e}", provider.cli_name())))
}

// =============================================================================
// Errors
// =============================================================================

/// "No credential configured" error naming where the user can put one.
#[must_use]
pub fn missing_credential(provider: Provider, hint: &str) -> CautError {
    CautError::AuthInvalid {
        provider: provider.cli_name().to_string(),
        reason: format!("no credential found; {hint}"),
    }
}

/// Generic fetch failure with a reason.
#[must_use]
pub fn fetch_failed(provider: Provider, reason: impl Into<String>) -> CautError {
    CautError::FetchFailed {
        provider: provider.cli_name().to_string(),
        reason: reason.into(),
    }
}

// =============================================================================
// Values
// =============================================================================

/// Parse a timestamp that may be RFC 3339 text, a numeric string, or epoch
/// seconds / milliseconds as a number.
#[must_use]
pub fn parse_timestamp(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    match value {
        serde_json::Value::String(s) => parse_timestamp_str(s),
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| {
                #[allow(clippy::cast_possible_truncation)]
                n.as_f64().map(|f| f as i64)
            })
            .and_then(epoch_to_datetime),
        _ => None,
    }
}

/// Parse an RFC 3339 / ISO 8601 timestamp, or an epoch value written as text.
#[must_use]
pub fn parse_timestamp_str(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|naive| Utc.from_utc_datetime(&naive));
    }
    s.parse::<i64>().ok().and_then(epoch_to_datetime)
}

/// Interpret an epoch value as seconds or milliseconds by magnitude.
#[must_use]
pub fn epoch_to_datetime(value: i64) -> Option<DateTime<Utc>> {
    if value <= 0 {
        return None;
    }
    // Anything past year ~5138 in seconds is really milliseconds.
    if value > 100_000_000_000 {
        Utc.timestamp_millis_opt(value).single()
    } else {
        Utc.timestamp_opt(value, 0).single()
    }
}

/// Percent of `limit` consumed by `used`, clamped to 0..=100.
///
/// Returns `None` when `limit` is not positive (no cap to measure against).
#[must_use]
pub fn percent_of(used: f64, limit: f64) -> Option<f64> {
    (limit > 0.0).then(|| (used / limit * 100.0).clamp(0.0, 100.0))
}

/// A window from a used/limit pair.
#[must_use]
pub fn window_from_counts(
    used: f64,
    limit: f64,
    window_minutes: Option<i32>,
    resets_at: Option<DateTime<Utc>>,
) -> Option<RateWindow> {
    percent_of(used, limit).map(|used_percent| RateWindow {
        used_percent,
        window_minutes,
        resets_at,
        reset_description: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_bearer_handles_prefix_and_whitespace() {
        assert_eq!(strip_bearer("Bearer abc"), "abc");
        assert_eq!(strip_bearer("  bearer   xyz "), "xyz");
        assert_eq!(strip_bearer("plain"), "plain");
        assert_eq!(strip_bearer(""), "");
    }

    #[test]
    fn cookie_value_finds_named_cookie() {
        let header = "Cookie: a=1; sessionKey=sk-ant-sid01-xyz; b=2";
        assert_eq!(cookie_value(header, "sessionKey"), Some("sk-ant-sid01-xyz"));
        assert_eq!(cookie_value(header, "a"), Some("1"));
        assert_eq!(cookie_value(header, "missing"), None);
        assert_eq!(cookie_value("", "a"), None);
    }

    #[test]
    fn normalize_cookie_header_strips_prefix() {
        assert_eq!(normalize_cookie_header("Cookie: a=1; b=2 "), "a=1; b=2");
        assert_eq!(normalize_cookie_header("a=1"), "a=1");
    }

    #[test]
    fn resolve_secret_prefers_context_token() {
        let ctx = FetchContext::with_account("work", "  tok-123 ");
        assert_eq!(
            resolve_secret(&ctx, Provider::Zai, &["CAUT_TEST_UNSET_VAR_XYZ"]),
            Some("tok-123".to_string())
        );
    }

    #[test]
    fn status_error_maps_auth_and_rate_limit() {
        assert!(matches!(
            status_error(Provider::Zai, StatusCode::UNAUTHORIZED, ""),
            CautError::AuthInvalid { .. }
        ));
        assert!(matches!(
            status_error(Provider::Zai, StatusCode::FORBIDDEN, "nope"),
            CautError::AuthInvalid { .. }
        ));
        assert!(matches!(
            status_error(Provider::Zai, StatusCode::TOO_MANY_REQUESTS, ""),
            CautError::RateLimited { .. }
        ));
        match status_error(Provider::Zai, StatusCode::BAD_GATEWAY, &"x".repeat(1000)) {
            CautError::ProviderApiError {
                status_code,
                message,
                ..
            } => {
                assert_eq!(status_code, Some(502));
                assert!(message.len() < 300);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parse_timestamp_accepts_common_shapes() {
        let expected = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            parse_timestamp(&json!("2026-01-02T03:04:05Z")),
            Some(expected)
        );
        assert_eq!(
            parse_timestamp(&json!("2026-01-02T03:04:05.000+00:00")),
            Some(expected)
        );
        assert_eq!(
            parse_timestamp(&json!(expected.timestamp())),
            Some(expected)
        );
        assert_eq!(
            parse_timestamp(&json!(expected.timestamp_millis())),
            Some(expected)
        );
        assert_eq!(
            parse_timestamp(&json!(expected.timestamp().to_string())),
            Some(expected)
        );
        assert_eq!(
            parse_timestamp(&json!("2026-01-02")),
            Some(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap())
        );
        assert_eq!(parse_timestamp(&json!(null)), None);
        assert_eq!(parse_timestamp(&json!("")), None);
        assert_eq!(parse_timestamp(&json!(0)), None);
        assert_eq!(parse_timestamp(&json!("garbage")), None);
    }

    #[test]
    fn percent_of_clamps_and_rejects_zero_limit() {
        assert_eq!(percent_of(50.0, 200.0), Some(25.0));
        assert_eq!(percent_of(300.0, 200.0), Some(100.0));
        assert_eq!(percent_of(-5.0, 200.0), Some(0.0));
        assert_eq!(percent_of(5.0, 0.0), None);
    }

    #[test]
    fn window_from_counts_builds_window() {
        let w = window_from_counts(10.0, 40.0, Some(60), None).unwrap();
        assert!((w.used_percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(w.window_minutes, Some(60));
        assert!(window_from_counts(1.0, 0.0, None, None).is_none());
    }
}
