//! Kiro credit limits from the `GetUsageLimits` service the official CLI
//! itself calls.
//!
//! Ports `KiroUsageLimitsAPI` from `CodexBar`. The CLI's `/usage` report
//! states credits against the plan alone and omits the overage section for
//! organization accounts; `GetUsageLimits` carries the overage allowance on
//! top of the plan, which is the ceiling an account actually spends against.
//!
//! Credentials come from the CLI's own `SQLite` state, opened read-only: the
//! CLI owns the token and its refresh.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;

use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::common;

/// `X-Amz-Target` for the usage-limits operation.
pub const TARGET: &str = "AmazonCodeWhispererService.GetUsageLimits";

/// AWS JSON 1.0 protocol content type.
pub const CONTENT_TYPE_AMZ_JSON: &str = "application/x-amz-json-1.0";

/// Request timeout, as in `CodexBar`.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The state database file inside the CLI's data directory.
const STATE_DB_FILE: &str = "data.sqlite3";

/// The CLI's data directory name.
const DATA_DIR_NAME: &str = "kiro-cli";

const TOKEN_SQL: &str = "SELECT value FROM auth_kv WHERE key = 'kirocli:odic:token'";
const PROFILE_SQL: &str = "SELECT value FROM state WHERE key = 'api.codewhisperer.profile'";

const CREDIT_RESOURCE: &str = "CREDIT";

/// Plausible Unix seconds for a billing reset: 2001-09-09 through
/// 2100-01-01. Anything outside is a unit change (e.g. milliseconds), not a
/// date.
const RESET_RANGE: std::ops::RangeInclusive<f64> = 1_000_000_000.0..=4_102_444_800.0;

/// Plan and overage ceilings from `GetUsageLimits`.
#[derive(Debug, Clone, PartialEq)]
pub struct KiroUsageLimits {
    /// Credits included in the plan.
    pub plan_limit: f64,
    /// Plan credits spent, excluding overage.
    pub plan_used: f64,
    /// Overage credits spent beyond the plan.
    pub overage_used: f64,
    /// Maximum overage credits the account may spend; `None` when overage is
    /// not enabled.
    pub overage_cap: Option<f64>,
    /// `Some` when the API stated ENABLED (with a cap) or DISABLED; `None`
    /// when omitted, unrecognized, or ENABLED without a cap.
    pub overage_enabled: Option<bool>,
    /// Charges accrued for `overage_used`, in `currency_code`.
    pub overage_charges: Option<f64>,
    /// Price per overage credit, in `currency_code`.
    pub overage_rate: Option<f64>,
    pub currency_code: String,
    pub resets_at: DateTime<Utc>,
    /// True when `bonuses[]` was non-empty, so plan usage cannot be split from
    /// bonus spend.
    pub has_unseparated_bonus: bool,
    /// `subscriptionInfo.subscriptionTitle`, e.g. `KIRO POWER`.
    pub subscription_title: Option<String>,
}

impl KiroUsageLimits {
    /// The overage budget in currency: `cap × rate`.
    #[must_use]
    pub fn overage_charge_limit(&self) -> Option<f64> {
        match (self.overage_cap, self.overage_rate) {
            (Some(cap), Some(rate)) if cap > 0.0 && rate > 0.0 => Some(cap * rate),
            _ => None,
        }
    }
}

// =============================================================================
// Response parsing
// =============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageLimitsResponse {
    usage_breakdown_list: Vec<UsageBreakdown>,
    #[serde(default)]
    overage_configuration: Option<OverageConfiguration>,
    #[serde(default)]
    next_date_reset: Option<f64>,
    #[serde(default)]
    subscription_info: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBreakdown {
    #[serde(default)]
    resource_type: Option<String>,
    #[serde(default)]
    current_usage_with_precision: Option<f64>,
    #[serde(default)]
    usage_limit_with_precision: Option<f64>,
    #[serde(default)]
    current_overages_with_precision: Option<f64>,
    #[serde(default)]
    overage_cap_with_precision: Option<f64>,
    #[serde(default)]
    overage_charges: Option<f64>,
    #[serde(default)]
    overage_rate: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    next_date_reset: Option<f64>,
    #[serde(default)]
    bonuses: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverageConfiguration {
    #[serde(default)]
    overage_status: Option<String>,
}

fn usable_credits(value: Option<f64>, field: &str) -> std::result::Result<f64, String> {
    value
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(|| format!("no usable {field}"))
}

/// `Some(true)` for ENABLED, `Some(false)` for DISABLED, `None` otherwise.
fn overage_availability(status: Option<&str>) -> Option<bool> {
    match status?.to_uppercase().as_str() {
        "ENABLED" => Some(true),
        "DISABLED" => Some(false),
        _ => None,
    }
}

fn reset_date(value: Option<f64>) -> Option<DateTime<Utc>> {
    let seconds = value.filter(|v| v.is_finite() && RESET_RANGE.contains(v))?;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "range-checked to 2001..2100 in seconds, far inside i64 milliseconds"
    )]
    let millis = (seconds * 1000.0).round() as i64;
    Utc.timestamp_millis_opt(millis).single()
}

/// Parse a `GetUsageLimits` response body.
///
/// # Errors
/// Returns a message when the body is not the expected JSON, when there is
/// not exactly one `CREDIT` balance, when a component is negative or not
/// finite, when overage exceeds total usage, when plan usage exceeds the plan
/// (unless bonus spend is folded in), or when the reset date is implausible.
pub fn parse_usage_limits(body: &str) -> std::result::Result<KiroUsageLimits, String> {
    let response: UsageLimitsResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;

    let mut credits = response
        .usage_breakdown_list
        .iter()
        .filter(|b| b.resource_type.as_deref() == Some(CREDIT_RESOURCE));
    let credit = credits
        .next()
        .ok_or_else(|| "no credit balance reported".to_string())?;
    if credits.next().is_some() {
        return Err("several credit balances reported".to_string());
    }

    // Validate each component rather than the sum: a negative one would
    // otherwise hide inside a positive total.
    let plan_limit = usable_credits(credit.usage_limit_with_precision, "plan limit")?;
    let total_used = usable_credits(credit.current_usage_with_precision, "usage")?;
    let overage_used = usable_credits(
        Some(credit.current_overages_with_precision.unwrap_or(0.0)),
        "overage usage",
    )?;
    // `currentUsage` includes overage, so overage above it is impossible.
    if total_used < overage_used {
        return Err("overage exceeds total usage".to_string());
    }
    let plan_used = total_used - overage_used;
    let has_unseparated_bonus = credit.bonuses.as_ref().is_some_and(|b| !b.is_empty());
    // Bonus spend is folded into currentUsage, so only then may plan usage
    // exceed the plan ceiling.
    if !has_unseparated_bonus && plan_used > plan_limit {
        return Err("plan usage exceeds plan limit".to_string());
    }

    let availability = overage_availability(
        response
            .overage_configuration
            .as_ref()
            .and_then(|c| c.overage_status.as_deref()),
    );
    let overage_cap = match (availability, credit.overage_cap_with_precision) {
        (Some(true), Some(cap)) => Some(usable_credits(Some(cap), "overage cap")?),
        _ => None,
    };
    // ENABLED without a cap is incomplete, not disabled.
    let overage_enabled = if availability == Some(true) && overage_cap.is_none() {
        None
    } else {
        availability
    };
    let resets_at = reset_date(credit.next_date_reset.or(response.next_date_reset))
        .ok_or_else(|| "no plausible reset date reported".to_string())?;

    let subscription_title = response
        .subscription_info
        .as_ref()
        .and_then(|info| info.get("subscriptionTitle"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);

    Ok(KiroUsageLimits {
        plan_limit,
        plan_used,
        overage_used,
        overage_cap,
        overage_enabled,
        overage_charges: credit
            .overage_charges
            .filter(|v| v.is_finite() && *v >= 0.0),
        overage_rate: credit.overage_rate.filter(|v| v.is_finite() && *v > 0.0),
        currency_code: credit.currency.clone().unwrap_or_else(|| "USD".to_string()),
        resets_at,
        has_unseparated_bonus,
        subscription_title,
    })
}

// =============================================================================
// State database
// =============================================================================

/// Which platform convention locates the CLI's data directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// `~/Library/Application Support/kiro-cli`.
    MacOs,
    /// `%LOCALAPPDATA%\kiro-cli`.
    Windows,
    /// `$XDG_DATA_HOME/kiro-cli`, default `~/.local/share/kiro-cli`.
    Unix,
}

impl Platform {
    /// The platform this binary was built for.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Inputs that locate the state database.
#[derive(Debug, Clone, Default)]
pub struct StateDbLocation<'a> {
    pub home: Option<&'a Path>,
    /// `KIRO_DATA_DIR`: overrides the data directory on every platform.
    pub kiro_data_dir: Option<&'a str>,
    /// `XDG_DATA_HOME` (Linux / other Unix).
    pub xdg_data_home: Option<&'a str>,
    /// `%LOCALAPPDATA%` (Windows).
    pub local_app_data: Option<&'a Path>,
}

fn cleaned_path(raw: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    let trimmed = raw.map(str::trim).filter(|s| !s.is_empty())?;
    if trimmed == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
        && let Some(home) = home
    {
        return Some(home.join(rest));
    }
    Some(PathBuf::from(trimmed))
}

/// Path of the CLI's `data.sqlite3` for a platform convention.
#[must_use]
pub fn state_database_path_for(platform: Platform, loc: &StateDbLocation<'_>) -> Option<PathBuf> {
    if let Some(dir) = cleaned_path(loc.kiro_data_dir, loc.home) {
        return Some(dir.join(STATE_DB_FILE));
    }
    let data_home = match platform {
        Platform::MacOs => loc.home?.join("Library").join("Application Support"),
        Platform::Windows => loc
            .local_app_data
            .map(Path::to_path_buf)
            .or_else(|| loc.home.map(|h| h.join("AppData").join("Local")))?,
        Platform::Unix => cleaned_path(loc.xdg_data_home, loc.home)
            .or_else(|| loc.home.map(|h| h.join(".local").join("share")))?,
    };
    Some(data_home.join(DATA_DIR_NAME).join(STATE_DB_FILE))
}

/// The CLI's state database on this machine, honouring `KIRO_DATA_DIR` and
/// `XDG_DATA_HOME`.
#[must_use]
pub fn state_database_path() -> Option<PathBuf> {
    let home = common::home_dir();
    let kiro_data_dir = std::env::var("KIRO_DATA_DIR").ok();
    let xdg_data_home = std::env::var("XDG_DATA_HOME").ok();
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    state_database_path_for(
        Platform::current(),
        &StateDbLocation {
            home: home.as_deref(),
            kiro_data_dir: kiro_data_dir.as_deref(),
            xdg_data_home: xdg_data_home.as_deref(),
            local_app_data: local_app_data.as_deref(),
        },
    )
}

/// The bearer token and profile the CLI is signed in with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KiroCliIdentity {
    pub access_token: String,
    pub profile_arn: String,
}

fn credentials_unavailable(reason: &str) -> CautError {
    CautError::AuthInvalid {
        provider: Provider::Kiro.cli_name().to_string(),
        reason: format!("Kiro CLI credentials unavailable: {reason}; run `kiro-cli login`"),
    }
}

fn query_value(conn: &Connection, sql: &str, what: &str) -> Result<String> {
    let value = conn
        .query_row(sql, [], |row| {
            Ok(match row.get_ref(0)? {
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
                    Some(String::from_utf8_lossy(bytes).into_owned())
                }
                _ => None,
            })
        })
        .optional()
        .map_err(|e| credentials_unavailable(&format!("read {what}: {e}")))?;
    value
        .flatten()
        .ok_or_else(|| credentials_unavailable(&format!("{what} not found in Kiro CLI state")))
}

fn json_string(json: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Read the CLI's access token and profile ARN from its state database,
/// read-only. `token_override` (a selected token account) replaces the
/// stored token; the profile still comes from the database.
///
/// # Errors
/// Returns an auth error when the database is unreadable or lacks the token
/// or profile.
pub fn read_identity(db_path: &Path, token_override: Option<&str>) -> Result<KiroCliIdentity> {
    if !db_path.is_file() {
        return Err(credentials_unavailable(&format!(
            "Kiro CLI state database not readable at {}",
            db_path.display()
        )));
    }
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| credentials_unavailable(&format!("open state database: {e}")))?;
    // Best effort: the CLI may hold a write lock briefly.
    let _ = conn.busy_timeout(Duration::from_millis(250));

    let access_token = if let Some(token) = token_override {
        common::strip_bearer(token).to_string()
    } else {
        let token_json = query_value(&conn, TOKEN_SQL, "token")?;
        json_string(&token_json, "access_token")
            .ok_or_else(|| credentials_unavailable("token has no access_token"))?
    };
    let profile_json = query_value(&conn, PROFILE_SQL, "profile")?;
    let profile_arn = json_string(&profile_json, "arn")
        .ok_or_else(|| credentials_unavailable("profile has no arn"))?;
    Ok(KiroCliIdentity {
        access_token,
        profile_arn,
    })
}

// =============================================================================
// Request
// =============================================================================

/// The regional endpoint for a CLI profile ARN
/// (`arn:aws:codewhisperer:<region>:<account>:profile/<id>`), or `None` for
/// a malformed ARN or an unsupported region, so credentials are never sent
/// to a default endpoint.
#[must_use]
pub fn endpoint_for_profile_arn(arn: &str) -> Option<&'static str> {
    if arn.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    let [prefix, partition, service, region, _account, resource] = parts.as_slice() else {
        return None;
    };
    let profile_id = resource.strip_prefix("profile/")?;
    if *prefix != "arn"
        || *partition != "aws"
        || *service != "codewhisperer"
        || profile_id.is_empty()
    {
        return None;
    }
    match *region {
        "us-east-1" => Some("https://codewhisperer.us-east-1.amazonaws.com/"),
        "eu-central-1" => Some("https://q.eu-central-1.amazonaws.com/"),
        _ => None,
    }
}

fn parse_error(message: &str) -> CautError {
    CautError::ParseResponse(format!(
        "{}: Failed to parse Kiro usage API response: {message}",
        Provider::Kiro.cli_name()
    ))
}

/// POST `GetUsageLimits` to `endpoint` and parse the reply.
///
/// # Errors
/// Returns a transport error, a status-mapped error (401/403 → auth), or a
/// parse error.
pub async fn request_usage_limits(
    client: &Client,
    endpoint: &str,
    identity: &KiroCliIdentity,
) -> Result<KiroUsageLimits> {
    let body = serde_json::json!({ "profileArn": identity.profile_arn }).to_string();
    let request = client
        .post(endpoint)
        .header(CONTENT_TYPE, CONTENT_TYPE_AMZ_JSON)
        .header("X-Amz-Target", TARGET)
        .header(AUTHORIZATION, format!("Bearer {}", identity.access_token))
        .body(body);
    let response = common::send_checked(Provider::Kiro, request).await?;
    let text = response
        .text()
        .await
        .map_err(|e| common::map_send_error(Provider::Kiro, &e))?;
    parse_usage_limits(&text).map_err(|m| parse_error(&m))
}

/// Read the CLI's credentials from `db_path` and fetch the usage limits.
///
/// # Errors
/// Returns an auth error for missing credentials or an unsupported profile
/// ARN, otherwise the request's error.
pub async fn fetch_usage_limits(
    db_path: &Path,
    token_override: Option<&str>,
) -> Result<KiroUsageLimits> {
    let identity = read_identity(db_path, token_override)?;
    let endpoint = endpoint_for_profile_arn(&identity.profile_arn)
        .ok_or_else(|| credentials_unavailable("unsupported profile ARN"))?;
    let client = crate::core::http::build_client(REQUEST_TIMEOUT)?;
    request_usage_limits(&client, endpoint, &identity).await
}

/// A state database shaped like the CLI's, for tests.
#[cfg(test)]
pub(super) fn make_state_db(dir: &Path, profile_arn: &str, token: Option<&str>) -> PathBuf {
    let path = dir.join(STATE_DB_FILE);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE auth_kv(key TEXT PRIMARY KEY, value TEXT);
         CREATE TABLE state(key TEXT PRIMARY KEY, value TEXT);",
    )
    .unwrap();
    if let Some(token) = token {
        conn.execute(
            "INSERT INTO auth_kv VALUES ('kirocli:odic:token', ?1)",
            [serde_json::json!({ "access_token": token, "refresh_token": "r" }).to_string()],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO state VALUES ('api.codewhisperer.profile', ?1)",
        [serde_json::json!({ "arn": profile_arn, "profile_name": "p" }).to_string()],
    )
    .unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERAGE_RESPONSE: &str =
        include_str!("../../../tests/fixtures/kiro/get_usage_limits_overage.json");

    fn parse(json: &str) -> KiroUsageLimits {
        parse_usage_limits(json).unwrap()
    }

    #[test]
    fn parses_plan_and_overage_without_double_counting() {
        let limits = parse(OVERAGE_RESPONSE);
        assert!((limits.plan_limit - 10000.0).abs() < 1e-9);
        assert!((limits.plan_used - 10000.0).abs() < 1e-9);
        assert!((limits.overage_used - 3603.49).abs() < 1e-9);
        assert_eq!(limits.overage_cap, Some(10000.0));
        assert_eq!(limits.overage_enabled, Some(true));
        assert_eq!(limits.overage_charges, Some(144.139_711_109_352));
        assert_eq!(limits.overage_rate, Some(0.04));
        assert_eq!(limits.currency_code, "USD");
        assert!((limits.overage_charge_limit().unwrap() - 400.0).abs() < 1e-9);
        assert_eq!(
            limits.resets_at,
            Utc.timestamp_opt(1_788_220_800, 0).unwrap()
        );
        assert!(!limits.has_unseparated_bonus);
        assert_eq!(limits.subscription_title.as_deref(), Some("KIRO POWER"));
    }

    #[test]
    fn rejects_overage_above_total_usage() {
        let json = OVERAGE_RESPONSE.replace(
            "\"currentUsageWithPrecision\":13603.49",
            "\"currentUsageWithPrecision\":100",
        );
        assert!(parse_usage_limits(&json).is_err());
    }

    #[test]
    fn keeps_overage_above_the_cap() {
        let json = OVERAGE_RESPONSE.replace(
            "\"overageCapWithPrecision\":10000.0",
            "\"overageCapWithPrecision\":100",
        );
        let limits = parse(&json);
        assert_eq!(limits.overage_cap, Some(100.0));
        assert!((limits.overage_used - 3603.49).abs() < 1e-9);
    }

    #[test]
    fn disabled_overage_has_no_cap() {
        let json = OVERAGE_RESPONSE.replace(
            "\"overageStatus\":\"ENABLED\"",
            "\"overageStatus\":\"DISABLED\"",
        );
        let limits = parse(&json);
        assert_eq!(limits.overage_cap, None);
        assert_eq!(limits.overage_enabled, Some(false));
        assert_eq!(limits.overage_charge_limit(), None);
        // Disabling overage does not un-spend it.
        assert!((limits.overage_used - 3603.49).abs() < 1e-9);
        assert!(limits.plan_used <= limits.plan_limit);
    }

    #[test]
    fn unknown_or_missing_overage_status_is_not_disabled() {
        let json = OVERAGE_RESPONSE.replace(
            "\"overageStatus\":\"ENABLED\"",
            "\"overageStatus\":\"FUTURE_STATUS\"",
        );
        let limits = parse(&json);
        assert_eq!(limits.overage_enabled, None);
        assert_eq!(limits.overage_cap, None);

        let json = OVERAGE_RESPONSE.replace("\"overageStatus\":\"ENABLED\"", "\"x\":1");
        assert_eq!(parse(&json).overage_enabled, None);
        let json =
            OVERAGE_RESPONSE.replace("\"overageStatus\":\"ENABLED\"", "\"overageStatus\":\"\"");
        assert_eq!(parse(&json).overage_enabled, None);
        let json = OVERAGE_RESPONSE.replace(
            "\"overageStatus\":\"ENABLED\"",
            "\"overageStatus\":\"enabled\"",
        );
        assert_eq!(parse(&json).overage_enabled, Some(true));
    }

    #[test]
    fn enabled_without_cap_is_incomplete() {
        let json = OVERAGE_RESPONSE.replace("\"overageCapWithPrecision\":10000.0,", "");
        let limits = parse(&json);
        assert_eq!(limits.overage_enabled, None);
        assert_eq!(limits.overage_cap, None);
        assert!((limits.overage_used - 3603.49).abs() < 1e-9);
    }

    #[test]
    fn rejects_implausible_reset_dates() {
        let json = OVERAGE_RESPONSE.replace("1.7882208E9", "1.7882208E12");
        assert!(parse_usage_limits(&json).is_err());
        let json =
            OVERAGE_RESPONSE.replace("\"nextDateReset\":1.7882208E9", "\"nextDateReset\":null");
        assert!(parse_usage_limits(&json).is_err());
    }

    #[test]
    fn falls_back_to_top_level_reset_date() {
        let json = OVERAGE_RESPONSE.replacen(
            "\"displayName\":\"Credit\",\"nextDateReset\":1.7882208E9,",
            "\"displayName\":\"Credit\",",
            1,
        );
        assert!(!json.contains("\"displayName\":\"Credit\",\"nextDateReset\""));
        assert_eq!(
            parse(&json).resets_at,
            Utc.timestamp_opt(1_788_220_800, 0).unwrap()
        );
    }

    #[test]
    fn rejects_zero_or_several_credit_balances() {
        let several = r#"{"nextDateReset":1.7882208E9,"usageBreakdownList":[
            {"resourceType":"CREDIT","currentUsageWithPrecision":1.0,"usageLimitWithPrecision":10.0},
            {"resourceType":"CREDIT","currentUsageWithPrecision":2.0,"usageLimitWithPrecision":20.0}]}"#;
        assert_eq!(
            parse_usage_limits(several).unwrap_err(),
            "several credit balances reported"
        );
        let none = r#"{"nextDateReset":1.7882208E9,"usageBreakdownList":[
            {"resourceType":"AGENTIC_REQUEST","currentUsageWithPrecision":1.0,"usageLimitWithPrecision":10.0}]}"#;
        assert_eq!(
            parse_usage_limits(none).unwrap_err(),
            "no credit balance reported"
        );
        assert!(parse_usage_limits(r#"{"usageBreakdownList":[]}"#).is_err());
        assert!(parse_usage_limits("{}").is_err());
        assert!(parse_usage_limits("not json").is_err());
    }

    #[test]
    fn minimal_credit_row_defaults_currency_and_overage() {
        let json = r#"{"nextDateReset":1790812800,"usageBreakdownList":[
            {"resourceType":"CREDIT","currentUsageWithPrecision":12.5,"usageLimitWithPrecision":50}]}"#;
        let limits = parse(json);
        assert!((limits.plan_used - 12.5).abs() < f64::EPSILON);
        assert!(limits.overage_used.abs() < f64::EPSILON);
        assert_eq!(limits.currency_code, "USD");
        assert_eq!(limits.overage_cap, None);
        assert_eq!(limits.overage_enabled, None);
        assert_eq!(limits.subscription_title, None);
    }

    #[test]
    fn rejects_negative_or_missing_components() {
        let json = OVERAGE_RESPONSE.replace(
            "\"usageLimitWithPrecision\":10000.0",
            "\"usageLimitWithPrecision\":-1",
        );
        assert_eq!(
            parse_usage_limits(&json).unwrap_err(),
            "no usable plan limit"
        );
        let json = OVERAGE_RESPONSE.replace("\"currentUsageWithPrecision\":13603.49,", "");
        assert_eq!(parse_usage_limits(&json).unwrap_err(), "no usable usage");
        let json = OVERAGE_RESPONSE.replace(
            "\"currentOveragesWithPrecision\":3603.49",
            "\"currentOveragesWithPrecision\":-3",
        );
        assert_eq!(
            parse_usage_limits(&json).unwrap_err(),
            "no usable overage usage"
        );
        let json = OVERAGE_RESPONSE.replace(
            "\"overageCapWithPrecision\":10000.0",
            "\"overageCapWithPrecision\":-5",
        );
        assert_eq!(
            parse_usage_limits(&json).unwrap_err(),
            "no usable overage cap"
        );
    }

    #[test]
    fn rejects_plan_usage_above_plan_unless_bonus_is_folded_in() {
        let json = OVERAGE_RESPONSE.replace(
            "\"currentOveragesWithPrecision\":3603.49",
            "\"currentOveragesWithPrecision\":0",
        );
        assert_eq!(
            parse_usage_limits(&json).unwrap_err(),
            "plan usage exceeds plan limit"
        );

        let json = OVERAGE_RESPONSE
            .replace("\"bonuses\":[]", "\"bonuses\":[{}]")
            .replace(
                "\"currentUsageWithPrecision\":13603.49",
                "\"currentUsageWithPrecision\":14603.49",
            );
        let limits = parse(&json);
        assert!(limits.has_unseparated_bonus);
        assert!((limits.plan_used - 11000.0).abs() < 1e-9);
        assert_eq!(limits.overage_cap, Some(10000.0));
    }

    #[test]
    fn drops_invalid_charges_and_rates() {
        let json = OVERAGE_RESPONSE
            .replace(
                "\"overageCharges\":144.139711109352",
                "\"overageCharges\":-1",
            )
            .replace("\"overageRate\":0.04", "\"overageRate\":0");
        let limits = parse(&json);
        assert_eq!(limits.overage_charges, None);
        assert_eq!(limits.overage_rate, None);
        assert_eq!(limits.overage_charge_limit(), None);
    }

    #[test]
    fn endpoint_follows_the_profile_region() {
        assert_eq!(
            endpoint_for_profile_arn("arn:aws:codewhisperer:us-east-1:123456789012:profile/test"),
            Some("https://codewhisperer.us-east-1.amazonaws.com/")
        );
        assert_eq!(
            endpoint_for_profile_arn("arn:aws:codewhisperer:eu-central-1:123456789012:profile/x"),
            Some("https://q.eu-central-1.amazonaws.com/")
        );
        for arn in [
            "",
            "not-an-arn",
            "arn:aws:codewhisperer:eu-central-1:123456789012",
            "arn:aws-cn:codewhisperer:eu-central-1:123456789012:profile/test",
            "arn:aws:s3:eu-central-1:123456789012:profile/test",
            "arn:aws:codewhisperer::123456789012:profile/test",
            "arn:aws:codewhisperer:ap-southeast-1:123456789012:profile/test",
            "arn:aws:codewhisperer:EU-CENTRAL-1:123456789012:profile/test",
            "arn:aws:codewhisperer:eu-central-1.example:123456789012:profile/test",
            "arn:aws:codewhisperer:-eu-central-1:123456789012:profile/test",
            "arn:aws:codewhisperer:eu-central-1-:123456789012:profile/test",
            "arn:aws:codewhisperer:eu-central-1:123456789012:profile/",
            "arn:aws:codewhisperer:eu-central-1:123456789012:other/test",
            "arn:aws:codewhisperer:eu-central-1:123456789012:profile/test ",
            "arn:aws:codewhisperer:eu-central-1:123456789012:profile/te\nst",
        ] {
            assert_eq!(endpoint_for_profile_arn(arn), None, "accepted {arn:?}");
        }
    }

    #[test]
    fn state_database_path_per_platform_and_overrides() {
        let home = Path::new("/tmp/caut-kiro-home");
        let base = StateDbLocation {
            home: Some(home),
            ..StateDbLocation::default()
        };
        assert_eq!(
            state_database_path_for(Platform::MacOs, &base).unwrap(),
            home.join("Library/Application Support/kiro-cli/data.sqlite3")
        );
        assert_eq!(
            state_database_path_for(Platform::Unix, &base).unwrap(),
            home.join(".local/share/kiro-cli/data.sqlite3")
        );
        assert_eq!(
            state_database_path_for(Platform::Windows, &base).unwrap(),
            home.join("AppData")
                .join("Local")
                .join("kiro-cli")
                .join("data.sqlite3")
        );
        let windows = StateDbLocation {
            local_app_data: Some(Path::new("/tmp/localappdata")),
            ..base.clone()
        };
        assert_eq!(
            state_database_path_for(Platform::Windows, &windows).unwrap(),
            Path::new("/tmp/localappdata")
                .join("kiro-cli")
                .join("data.sqlite3")
        );
        let xdg = StateDbLocation {
            xdg_data_home: Some("/tmp/xdg-data"),
            ..base.clone()
        };
        assert_eq!(
            state_database_path_for(Platform::Unix, &xdg).unwrap(),
            Path::new("/tmp/xdg-data/kiro-cli/data.sqlite3")
        );
        let blank_xdg = StateDbLocation {
            xdg_data_home: Some("   "),
            ..base.clone()
        };
        assert_eq!(
            state_database_path_for(Platform::Unix, &blank_xdg).unwrap(),
            home.join(".local/share/kiro-cli/data.sqlite3")
        );
        let override_dir = StateDbLocation {
            kiro_data_dir: Some("/tmp/kiro-data"),
            ..base.clone()
        };
        for platform in [Platform::MacOs, Platform::Unix, Platform::Windows] {
            assert_eq!(
                state_database_path_for(platform, &override_dir).unwrap(),
                Path::new("/tmp/kiro-data/data.sqlite3")
            );
        }
        let tilde = StateDbLocation {
            kiro_data_dir: Some("~/kiro"),
            ..base
        };
        assert_eq!(
            state_database_path_for(Platform::Unix, &tilde).unwrap(),
            home.join("kiro/data.sqlite3")
        );
        assert_eq!(
            state_database_path_for(Platform::MacOs, &StateDbLocation::default()),
            None
        );
    }

    const ARN: &str = "arn:aws:codewhisperer:us-east-1:123456789012:profile/test-profile";

    #[test]
    fn reads_identity_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let db = make_state_db(dir.path(), ARN, Some("synthetic-kiro-token"));
        let before = std::fs::read(&db).unwrap();
        let identity = read_identity(&db, None).unwrap();
        assert_eq!(identity.access_token, "synthetic-kiro-token");
        assert_eq!(identity.profile_arn, ARN);
        assert_eq!(std::fs::read(&db).unwrap(), before);

        let overridden = read_identity(&db, Some("Bearer account-token")).unwrap();
        assert_eq!(overridden.access_token, "account-token");
        assert_eq!(overridden.profile_arn, ARN);
    }

    #[test]
    fn identity_errors_are_auth_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.sqlite3");
        assert!(matches!(
            read_identity(&missing, None),
            Err(CautError::AuthInvalid { .. })
        ));

        let db = make_state_db(dir.path(), ARN, None);
        assert!(matches!(
            read_identity(&db, None),
            Err(CautError::AuthInvalid { reason, .. }) if reason.contains("token not found")
        ));
        // A token account supplies the token; the profile still comes from disk.
        assert!(read_identity(&db, Some("tok")).is_ok());

        let other = tempfile::tempdir().unwrap();
        let empty_profile = make_state_db(other.path(), "", Some("tok"));
        assert!(matches!(
            read_identity(&empty_profile, None),
            Err(CautError::AuthInvalid { reason, .. }) if reason.contains("profile has no arn")
        ));

        let not_sqlite = dir.path().join("garbage.sqlite3");
        std::fs::write(&not_sqlite, b"this is not a database").unwrap();
        assert!(matches!(
            read_identity(&not_sqlite, None),
            Err(CautError::AuthInvalid { .. })
        ));
    }

    #[tokio::test]
    async fn unsupported_profile_never_sends_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let db = make_state_db(
            dir.path(),
            "arn:aws:codewhisperer:ap-southeast-1:123456789012:profile/test",
            Some("tok"),
        );
        match fetch_usage_limits(&db, None).await {
            Err(CautError::AuthInvalid { reason, .. }) => {
                assert!(reason.contains("unsupported profile ARN"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_sends_aws_json_and_parses_reply() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("content-type", CONTENT_TYPE_AMZ_JSON))
            .and(header("x-amz-target", TARGET))
            .and(header("authorization", "Bearer synthetic-kiro-token"))
            .and(body_json(serde_json::json!({ "profileArn": ARN })))
            .respond_with(ResponseTemplate::new(200).set_body_string(OVERAGE_RESPONSE))
            .expect(1)
            .mount(&server)
            .await;

        let identity = KiroCliIdentity {
            access_token: "synthetic-kiro-token".to_string(),
            profile_arn: ARN.to_string(),
        };
        let client = crate::core::http::build_client(REQUEST_TIMEOUT).unwrap();
        let limits = request_usage_limits(&client, &format!("{}/", server.uri()), &identity)
            .await
            .unwrap();
        assert!((limits.plan_used - 10000.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn request_maps_http_errors() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let identity = KiroCliIdentity {
            access_token: "expired".to_string(),
            profile_arn: ARN.to_string(),
        };
        let client = crate::core::http::build_client(REQUEST_TIMEOUT).unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string(r#"{"__type":"AccessDeniedException","message":"expired"}"#),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            request_usage_limits(&client, &server.uri(), &identity).await,
            Err(CautError::AuthInvalid { .. })
        ));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;
        assert!(matches!(
            request_usage_limits(&client, &server.uri(), &identity).await,
            Err(CautError::ParseResponse(_))
        ));
    }
}
