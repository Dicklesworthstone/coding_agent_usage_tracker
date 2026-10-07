//! `codex app-server` JSON-RPC client.
//!
//! The Codex CLI exposes account rate limits over a newline-delimited JSON-RPC
//! session on stdio. This is the same source `CodexBar` falls back to when the
//! `auth.json` token is missing or stale: the CLI refreshes its own tokens.
//!
//! Session: `initialize` → `initialized` (notification) →
//! `account/rateLimits/read` → `account/read`.

use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

use super::{CLI_NAME, credit_limit_cost, non_empty, normalize_windows, number};
use crate::core::fetch_plan::ProviderFetch;
use crate::core::models::{CreditsSnapshot, ProviderIdentity, RateWindow, UsageSnapshot};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::common;

/// Arguments that start the app-server without granting it write access.
const APP_SERVER_ARGS: &[&str] = &["-s", "read-only", "-a", "never", "app-server"];

/// Time allowed for `initialize` (the CLI may refresh tokens here).
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);

/// Time allowed for each subsequent request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);

/// Longest stdout line accepted before the session is abandoned.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Fetch usage by running `codex app-server`.
///
/// # Errors
/// Returns `ProviderNotFound` when the CLI is missing, a timeout or RPC error
/// from the session, or `FetchFailed` when neither windows nor credits come
/// back.
pub(super) async fn fetch_via_app_server() -> Result<ProviderFetch> {
    let program =
        which::which(CLI_NAME).map_err(|_| CautError::ProviderNotFound(CLI_NAME.to_string()))?;
    let mut command = Command::new(program);
    command.args(APP_SERVER_ARGS);
    let mut session = RpcSession::spawn(command)?;
    let result = session.read_usage().await;
    session.shutdown().await;
    let (limits, account) = result?;
    map_rpc_usage(&limits, account.as_ref(), Utc::now())
}

/// A running app-server child and its stdio.
struct RpcSession {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl RpcSession {
    /// Spawn the app-server.
    fn spawn(mut command: Command) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    CautError::ProviderNotFound(CLI_NAME.to_string())
                } else {
                    common::fetch_failed(Provider::Codex, format!("codex app-server: {e}"))
                }
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| common::fetch_failed(Provider::Codex, "codex app-server: no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| common::fetch_failed(Provider::Codex, "codex app-server: no stdout"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        })
    }

    /// Run the session: rate limits are required, the account is best effort.
    async fn read_usage(&mut self) -> Result<(Value, Option<Value>)> {
        self.request(
            "initialize",
            json!({"clientInfo": {"name": "caut", "version": env!("CARGO_PKG_VERSION")}}),
            INITIALIZE_TIMEOUT,
        )
        .await?;
        self.notify("initialized").await?;
        // The app-server answers on one stdout stream, so requests stay serial.
        let limits = self
            .request("account/rateLimits/read", json!({}), REQUEST_TIMEOUT)
            .await?;
        let account = self
            .request("account/read", json!({}), REQUEST_TIMEOUT)
            .await
            .ok();
        Ok((limits, account))
    }

    /// Send a request and wait for its result.
    async fn request(&mut self, method: &str, params: Value, limit: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"id": id, "method": method, "params": params}))
            .await?;
        timeout(limit, self.read_response(id, method))
            .await
            .map_err(|_| CautError::TimeoutWithProvider {
                provider: Provider::Codex.cli_name().to_string(),
                seconds: limit.as_secs(),
            })?
    }

    /// Send a notification (no id, no reply).
    async fn notify(&mut self, method: &str) -> Result<()> {
        self.send(&json!({"method": method, "params": {}})).await
    }

    async fn send(&mut self, message: &Value) -> Result<()> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.map_err(|e| {
            common::fetch_failed(Provider::Codex, format!("codex app-server stdin: {e}"))
        })?;
        self.stdin.flush().await.map_err(|e| {
            common::fetch_failed(Provider::Codex, format!("codex app-server stdin: {e}"))
        })
    }

    /// Read lines until the reply to `id`, skipping notifications and noise.
    async fn read_response(&mut self, id: u64, method: &str) -> Result<Value> {
        loop {
            let line = self
                .stdout
                .next_line()
                .await
                .map_err(|e| {
                    common::fetch_failed(Provider::Codex, format!("codex app-server stdout: {e}"))
                })?
                .ok_or_else(|| {
                    common::fetch_failed(
                        Provider::Codex,
                        format!("codex app-server exited before answering `{method}`"),
                    )
                })?;
            if line.len() > MAX_LINE_BYTES {
                return Err(common::fetch_failed(
                    Provider::Codex,
                    "codex app-server line exceeded the size limit",
                ));
            }
            if let Some(result) = match_response(&line, id, method)? {
                return Ok(result);
            }
        }
    }

    /// Close stdin and make sure the child is gone.
    async fn shutdown(mut self) {
        let _ = self.stdin.shutdown().await;
        let _ = self.child.start_kill();
        let _ = timeout(Duration::from_secs(2), self.child.wait()).await;
    }
}

/// Interpret one stdout line: `Some(result)` for the reply to `id`, `None` for
/// anything else (notifications, other ids, non-JSON output).
fn match_response(line: &str, id: u64, method: &str) -> Result<Option<Value>> {
    let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
        return Ok(None);
    };
    let matches_id = match message.get("id") {
        Some(Value::Number(n)) => n.as_u64() == Some(id),
        Some(Value::String(s)) => s.parse::<u64>().ok() == Some(id),
        _ => false,
    };
    if !matches_id {
        return Ok(None);
    }
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(rpc_error(method, text));
    }
    message.get("result").cloned().map(Some).ok_or_else(|| {
        common::fetch_failed(Provider::Codex, format!("`{method}` reply has no result"))
    })
}

/// Classify an RPC error message.
fn rpc_error(method: &str, text: &str) -> CautError {
    let lower = text.to_lowercase();
    let auth_markers = [
        "login",
        "logged in",
        "log in",
        "sign in",
        "unauthorized",
        "auth",
    ];
    if auth_markers.iter().any(|marker| lower.contains(marker)) {
        CautError::AuthInvalid {
            provider: Provider::Codex.cli_name().to_string(),
            reason: format!("`{method}`: {text} (run `codex login`)"),
        }
    } else {
        common::fetch_failed(Provider::Codex, format!("`{method}` failed: {text}"))
    }
}

/// Map `account/rateLimits/read` (+ `account/read`) into a fetch result.
fn map_rpc_usage(
    limits_result: &Value,
    account_result: Option<&Value>,
    now: DateTime<Utc>,
) -> Result<ProviderFetch> {
    let limits = limits_result.get("rateLimits").ok_or_else(|| {
        common::fetch_failed(Provider::Codex, "no rateLimits in app-server reply")
    })?;

    let account = account_result.and_then(|a| a.get("account"));
    let is_chatgpt = account
        .and_then(|a| a.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|t| t.eq_ignore_ascii_case("chatgpt"));
    let email = account
        .filter(|_| is_chatgpt)
        .and_then(|a| non_empty(a.get("email")));
    let plan =
        non_empty(limits.get("planType").or_else(|| limits.get("plan_type"))).or_else(|| {
            account
                .filter(|_| is_chatgpt)
                .and_then(|a| non_empty(a.get("planType")))
        });

    let (primary, secondary) = normalize_windows(
        limits.get("primary").and_then(rpc_window),
        limits.get("secondary").and_then(rpc_window),
    );

    let by_limit_id = limits_result
        .get("rateLimitsByLimitId")
        .or_else(|| limits_result.get("rate_limits_by_limit_id"))
        .and_then(Value::as_object);
    let mut candidates = vec![limits];
    if let Some(map) = by_limit_id {
        let mut others: Vec<&Value> = map.values().collect();
        others.sort_by_key(|v| {
            non_empty(v.get("limitName").or_else(|| v.get("limitId"))).unwrap_or_default()
        });
        candidates.extend(others);
    }
    let provider_cost = candidates.iter().find_map(|candidate| {
        candidate
            .get("individualLimit")
            .or_else(|| candidate.get("individual_limit"))
            .and_then(|l| credit_limit_cost(l, now))
    });

    let credits = limits.get("credits").and_then(|c| {
        // A missing or unparseable balance reads as zero, as in `CodexBar`.
        c.is_object().then(|| CreditsSnapshot {
            remaining: number(c.get("balance")).unwrap_or(0.0),
            events: Vec::new(),
            updated_at: now,
        })
    });

    let identity = (email.is_some() || plan.is_some()).then_some(ProviderIdentity {
        account_email: email,
        account_organization: None,
        login_method: plan,
    });

    let usage = UsageSnapshot {
        primary,
        secondary,
        provider_cost,
        updated_at: now,
        identity,
        ..UsageSnapshot::empty()
    };
    if !usage.has_quota() && usage.identity.is_none() && credits.is_none() {
        return Err(common::fetch_failed(
            Provider::Codex,
            "codex app-server returned no rate limits",
        ));
    }
    Ok(ProviderFetch { usage, credits })
}

/// One `{usedPercent, windowDurationMins, resetsAt}` window.
fn rpc_window(value: &Value) -> Option<RateWindow> {
    let used_percent = number(value.get("usedPercent"))?;
    #[allow(clippy::cast_possible_truncation)] // window lengths are small
    let window_minutes = number(value.get("windowDurationMins"))
        .filter(|m| *m > 0.0)
        .map(|m| m as i32);
    #[allow(clippy::cast_possible_truncation)] // epoch seconds fit in i64
    let resets_at = number(value.get("resetsAt"))
        .filter(|v| *v > 0.0)
        .and_then(|v| DateTime::from_timestamp(v as i64, 0));
    Some(RateWindow {
        used_percent,
        window_minutes,
        resets_at,
        reset_description: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_766_900_000, 0).unwrap()
    }

    #[test]
    fn match_response_skips_noise_and_other_ids() {
        assert!(match_response("not json", 2, "m").unwrap().is_none());
        assert!(
            match_response(
                r#"{"method":"remoteControl/status/changed","params":{}}"#,
                2,
                "m"
            )
            .unwrap()
            .is_none()
        );
        assert!(
            match_response(r#"{"id":1,"result":{}}"#, 2, "m")
                .unwrap()
                .is_none()
        );
        let result = match_response(r#"{"id":2,"result":{"ok":true}}"#, 2, "m")
            .unwrap()
            .unwrap();
        assert_eq!(result, json!({"ok": true}));
        let result = match_response(r#"{"id":"2","result":1}"#, 2, "m")
            .unwrap()
            .unwrap();
        assert_eq!(result, json!(1));
    }

    #[test]
    fn match_response_maps_errors() {
        let err =
            match_response(r#"{"id":2,"error":{"message":"Not logged in"}}"#, 2, "m").unwrap_err();
        assert!(matches!(err, CautError::AuthInvalid { .. }));
        let err = match_response(r#"{"id":2,"error":{"message":"boom"}}"#, 2, "m").unwrap_err();
        assert!(matches!(err, CautError::FetchFailed { .. }));
        let err = match_response(r#"{"id":2}"#, 2, "m").unwrap_err();
        assert!(matches!(err, CautError::FetchFailed { .. }));
    }

    #[test]
    fn maps_rate_limits_and_account() {
        let limits = json!({"rateLimits": {
            "primary": {"usedPercent": 12, "windowDurationMins": 300, "resetsAt": 1_766_948_068},
            "secondary": {"usedPercent": 40.5, "windowDurationMins": 10080, "resetsAt": 1_767_407_914},
            "credits": {"hasCredits": true, "unlimited": false, "balance": "7.5"}
        }});
        let account = json!({"account": {"type": "chatgpt", "email": "stub@example.com", "planType": "pro"}, "requiresOpenaiAuth": false});
        let fetch = map_rpc_usage(&limits, Some(&account), now()).unwrap();
        let primary = fetch.usage.primary.unwrap();
        assert!((primary.used_percent - 12.0).abs() < f64::EPSILON);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(
            primary.resets_at,
            DateTime::from_timestamp(1_766_948_068, 0)
        );
        assert!((fetch.usage.secondary.unwrap().used_percent - 40.5).abs() < f64::EPSILON);
        let identity = fetch.usage.identity.unwrap();
        assert_eq!(identity.account_email.as_deref(), Some("stub@example.com"));
        assert_eq!(identity.login_method.as_deref(), Some("pro"));
        assert!((fetch.credits.unwrap().remaining - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn rate_limit_plan_wins_and_api_key_accounts_have_no_email() {
        let limits = json!({"rateLimits": {"planType": "plus", "primary": {"usedPercent": 1}}});
        let account = json!({"account": {"type": "apiKey"}});
        let fetch = map_rpc_usage(&limits, Some(&account), now()).unwrap();
        let identity = fetch.usage.identity.unwrap();
        assert_eq!(identity.account_email, None);
        assert_eq!(identity.login_method.as_deref(), Some("plus"));
        assert!(fetch.credits.is_none());
    }

    #[test]
    fn credit_limit_found_in_limits_by_id() {
        let limits = json!({
            "rateLimits": {"primary": null},
            "rateLimitsByLimitId": {"b": {"limitName": "b"}, "a": {"limitName": "a", "individualLimit": {"limit": 100, "used": 40}}}
        });
        let fetch = map_rpc_usage(&limits, None, now()).unwrap();
        let cost = fetch.usage.provider_cost.unwrap();
        assert!((cost.used - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_reply_is_an_error() {
        assert!(map_rpc_usage(&json!({}), None, now()).is_err());
        assert!(map_rpc_usage(&json!({"rateLimits": {}}), None, now()).is_err());
    }

    /// Drive a full session against a stub app-server script.
    #[cfg(unix)]
    #[tokio::test]
    async fn session_against_stub_server() {
        let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialized"'*) ;;
    *'"method":"initialize"'*)
      printf '%s\n' '{"method":"remoteControl/status/changed","params":{}}'
      printf '%s\n' 'stub diagnostic'
      printf '%s\n' '{"id":1,"result":{}}' ;;
    *'account/rateLimits/read'*)
      printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":1766948068}}}}' ;;
    *'account/read'*)
      printf '%s\n' '{"id":3,"result":{"account":{"type":"chatgpt","email":"stub@example.com","planType":"pro"}}}' ;;
  esac
done
"#;
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        let mut session = RpcSession::spawn(command).unwrap();
        let result = session.read_usage().await;
        session.shutdown().await;
        let (limits, account) = result.unwrap();
        let fetch = map_rpc_usage(&limits, account.as_ref(), now()).unwrap();
        assert!((fetch.usage.primary.unwrap().used_percent - 12.0).abs() < f64::EPSILON);
        assert_eq!(
            fetch.usage.identity.unwrap().account_email.as_deref(),
            Some("stub@example.com")
        );
    }

    /// A server that exits mid-session is an error, not a hang.
    #[cfg(unix)]
    #[tokio::test]
    async fn session_reports_early_exit() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("read -r line; exit 0");
        let mut session = RpcSession::spawn(command).unwrap();
        let result = session.read_usage().await;
        session.shutdown().await;
        assert!(result.is_err());
    }
}
