//! `caut mcp`: a Model Context Protocol server on stdio.
//!
//! Lets MCP-aware agents (Claude Code, Codex, Cursor, ...) query usage
//! mid-task instead of shelling out and parsing CLI output:
//!
//! ```text
//! claude mcp add caut -- caut mcp
//! ```
//!
//! Transport: newline-delimited JSON-RPC 2.0 on stdin/stdout. Logs go to
//! stderr, so stdout carries protocol messages only.
//!
//! Tools: `caut_usage_status`, `caut_recommend_account`,
//! `caut_switch_account`, `caut_forecast`, `caut_cost`.
//! Resources: `caut://providers`.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cli::args::UsageArgs;
use crate::cli::usage::{UsageResults, fetch_usage};
use crate::core::cost_scanner::CostScanner;
use crate::core::models::{ProviderPayload, RateWindow};
use crate::core::prediction::UsagePace;
use crate::core::provider::{Provider, ProviderSelection};
use crate::error::{CautError, Result};
use crate::storage::paths::AppPaths;
use crate::storage::token_accounts::TokenAccountStore;
use crate::storage::{HistoryStore, StoredSnapshot};

/// Protocol revisions this server speaks, newest first.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Default `min_remaining_percent` for account recommendations.
const DEFAULT_MIN_REMAINING: f64 = 20.0;

/// Usage at or above which an account is reported as `warning`.
const WARNING_PERCENT: f64 = 75.0;

/// Usage at or above which an account is reported as `critical`.
const CRITICAL_PERCENT: f64 = 90.0;

/// JSON-RPC error codes.
mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
}

/// Run the MCP server until stdin closes.
///
/// # Errors
/// Returns an error only if stdout cannot be written.
pub async fn execute() -> Result<()> {
    let server = McpServer::new(AppPaths::new().token_accounts_file());
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    tracing::info!("caut MCP server listening on stdio");
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(&line).await {
            let mut text = response.to_string();
            text.push('\n');
            stdout.write_all(text.as_bytes()).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

/// The protocol handler, independent of the transport.
pub struct McpServer {
    token_accounts_path: PathBuf,
}

impl McpServer {
    /// A server that manages token accounts at `token_accounts_path`.
    #[must_use]
    pub const fn new(token_accounts_path: PathBuf) -> Self {
        Self {
            token_accounts_path,
        }
    }

    /// Handle one line of input; `None` for notifications.
    pub async fn handle_line(&self, line: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                let mut responses = Vec::new();
                for message in batch {
                    if let Some(response) = self.handle_message(message).await {
                        responses.push(response);
                    }
                }
                (!responses.is_empty()).then_some(Value::Array(responses))
            }
            Ok(message) => self.handle_message(message).await,
            Err(e) => Some(error_response(
                &Value::Null,
                code::PARSE_ERROR,
                &format!("Parse error: {e}"),
            )),
        }
    }

    /// Handle one JSON-RPC message.
    pub async fn handle_message(&self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response from the client (we send no requests) or garbage.
            return id.map(|id| error_response(&id, code::INVALID_REQUEST, "Missing method"));
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (no id) never get a reply.
        let id = id?;

        let result = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.call_tool(&params).await,
            "resources/list" => Ok(json!({ "resources": resource_definitions() })),
            "resources/read" => read_resource(&params),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            _ => Err((
                code::METHOD_NOT_FOUND,
                format!("Method not found: {method}"),
            )),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_response(&id, code, &message),
        })
    }

    /// `tools/call`: tool failures are results with `isError`, so the
    /// agent sees them; only a malformed call is a protocol error.
    async fn call_tool(&self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
            (
                code::INVALID_PARAMS,
                "tools/call needs a tool name".to_string(),
            )
        })?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let outcome = match name {
            "caut_usage_status" => self.usage_status(&args).await,
            "caut_recommend_account" => self.recommend_account(&args).await,
            "caut_switch_account" => self.switch_account(&args),
            "caut_forecast" => self.forecast(&args).await,
            "caut_cost" => cost(&args).await,
            _ => return Err((code::INVALID_PARAMS, format!("Unknown tool: {name}"))),
        };
        Ok(match outcome {
            Ok(value) => tool_result(&value, false),
            Err(error) => tool_result(&json!({ "error": error.to_string() }), true),
        })
    }

    // -------------------------------------------------------------------------
    // Tools
    // -------------------------------------------------------------------------

    async fn usage_status(&self, args: &Value) -> Result<Value> {
        let usage_args = UsageArgs {
            provider: str_arg(args, "provider"),
            account: str_arg(args, "account"),
            all_accounts: args
                .get("all_accounts")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            source: str_arg(args, "source"),
            status: args.get("status").and_then(Value::as_bool).unwrap_or(false),
            ..base_usage_args()
        };
        usage_args.validate()?;
        let results = fetch_usage(&usage_args).await?;
        Ok(usage_summary(&results, Utc::now()))
    }

    async fn recommend_account(&self, args: &Value) -> Result<Value> {
        let provider = required_provider(args)?;
        let min_remaining = args
            .get("min_remaining_percent")
            .and_then(Value::as_f64)
            .unwrap_or(DEFAULT_MIN_REMAINING);
        let store = TokenAccountStore::load(&self.token_accounts_path)?;
        let has_accounts =
            provider.supports_token_accounts() && !store.get_all(provider).is_empty();
        let usage_args = UsageArgs {
            provider: Some(provider.cli_name().to_string()),
            all_accounts: has_accounts,
            ..base_usage_args()
        };
        let results = fetch_usage(&usage_args).await?;
        let active = store.get_active(provider).map(|a| a.label.clone());
        Ok(recommendation(
            provider,
            &results,
            min_remaining,
            active.as_deref(),
        ))
    }

    fn switch_account(&self, args: &Value) -> Result<Value> {
        let provider = required_provider(args)?;
        let label = str_arg(args, "account").ok_or_else(|| {
            CautError::Config("`account` (a token account label) is required".into())
        })?;
        let mut store = TokenAccountStore::load(&self.token_accounts_path)?;
        store.set_active(provider, &label)?;
        store.save()?;
        Ok(json!({
            "provider": provider.cli_name(),
            "activeAccount": label.trim(),
            "note": "caut now uses this token account by default for this provider. \
                     It does not change which account the provider's own CLI is logged into."
        }))
    }

    async fn forecast(&self, args: &Value) -> Result<Value> {
        let usage_args = UsageArgs {
            provider: str_arg(args, "provider"),
            ..base_usage_args()
        };
        let results = fetch_usage(&usage_args).await?;
        let now = Utc::now();
        let history = HistoryStore::open(&AppPaths::new().history_db_file()).ok();
        let forecasts: Vec<Value> = results
            .payloads
            .iter()
            .map(|payload| {
                let snapshots = history.as_ref().and_then(|store| {
                    Provider::from_cli_name(&payload.provider)
                        .ok()
                        .and_then(|p| {
                            store
                                .get_snapshots(&p, now - chrono::Duration::hours(6), now)
                                .ok()
                        })
                });
                forecast_payload(payload, snapshots.as_deref(), now)
            })
            .collect();
        Ok(json!({ "forecasts": forecasts, "errors": results.errors }))
    }
}

/// `caut_cost`: local cost scan (Claude + Codex logs).
async fn cost(args: &Value) -> Result<Value> {
    let selection = str_arg(args, "provider")
        .as_deref()
        .map(ProviderSelection::from_arg)
        .transpose()?
        .unwrap_or_default();
    let providers: Vec<Provider> = selection
        .providers()
        .into_iter()
        .filter(|p| p.supports_cost_scan())
        .collect();
    if providers.is_empty() {
        return Err(CautError::Config(
            "Local cost scanning supports claude and codex only".to_string(),
        ));
    }
    let scanner = CostScanner::new();
    let mut costs = Vec::new();
    let mut errors = Vec::new();
    for provider in providers {
        match scanner.scan(provider, false).await {
            Ok(payload) => costs.push(serde_json::to_value(payload)?),
            Err(e) => errors.push(format!("{}: {e}", provider.cli_name())),
        }
    }
    Ok(json!({ "costs": costs, "errors": errors }))
}

// =============================================================================
// Summaries
// =============================================================================

/// `UsageArgs` with every flag off.
const fn base_usage_args() -> UsageArgs {
    UsageArgs {
        provider: None,
        account: None,
        account_index: None,
        all_accounts: false,
        no_credits: false,
        status: false,
        source: None,
        web: false,
        timeout: None,
        web_timeout: None,
        web_debug_dump_html: false,
        watch: false,
        interval: 30,
        tui: false,
    }
}

/// One named window in a summary.
fn window_json(name: &str, window: &RateWindow, now: DateTime<Utc>) -> Value {
    json!({
        "name": name,
        "usedPercent": round1(window.used_percent),
        "remainingPercent": round1(window.remaining_percent()),
        "windowMinutes": window.window_minutes,
        "resetsAt": window.resets_at,
        "resetsInSeconds": window.resets_at.map(|at| (at - now).num_seconds().max(0)),
    })
}

/// Every window of a payload with provider labels: primary, secondary,
/// tertiary, then scoped.
fn named_windows(payload: &ProviderPayload) -> Vec<(String, &RateWindow)> {
    let provider = Provider::from_cli_name(&payload.provider).ok();
    let usage = &payload.usage;
    let mut windows = Vec::new();
    if let Some(w) = &usage.primary {
        windows.push((
            provider
                .map_or("Session", Provider::session_label)
                .to_string(),
            w,
        ));
    }
    if let Some(w) = &usage.secondary {
        windows.push((
            provider
                .map_or("Weekly", Provider::weekly_label)
                .to_string(),
            w,
        ));
    }
    if let Some(w) = &usage.tertiary {
        let label = provider
            .and_then(Provider::tertiary_label)
            .unwrap_or("Tier 3");
        windows.push((label.to_string(), w));
    }
    for scoped in &usage.scoped {
        windows.push((scoped.label.clone(), &scoped.window));
    }
    windows
}

/// The highest usage percent across every window and the spend budget.
fn worst_used_percent(payload: &ProviderPayload) -> Option<f64> {
    named_windows(payload)
        .iter()
        .map(|(_, w)| w.used_percent)
        .chain(
            payload
                .usage
                .provider_cost
                .as_ref()
                .and_then(crate::core::models::ProviderCostSnapshot::used_percent),
        )
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        })
}

/// `ok` / `warning` / `critical` / `exhausted` / `unknown` from the worst window.
fn health(worst: Option<f64>) -> &'static str {
    match worst {
        None => "unknown",
        Some(w) if w >= 100.0 => "exhausted",
        Some(w) if w >= CRITICAL_PERCENT => "critical",
        Some(w) if w >= WARNING_PERCENT => "warning",
        Some(_) => "ok",
    }
}

/// Compact per-provider summary for agents.
fn payload_summary(payload: &ProviderPayload, now: DateTime<Utc>) -> Value {
    let worst = worst_used_percent(payload);
    let windows: Vec<Value> = named_windows(payload)
        .iter()
        .map(|(name, w)| window_json(name, w, now))
        .collect();
    let identity = payload.usage.identity.as_ref();
    json!({
        "provider": payload.provider,
        "account": payload.account,
        "email": identity.and_then(|i| i.account_email.clone()),
        "plan": identity.and_then(crate::core::models::ProviderIdentity::plan),
        "source": payload.source,
        "health": health(worst),
        "worstUsedPercent": worst.map(round1),
        "windows": windows,
        "providerCost": payload.usage.provider_cost,
        "creditsRemaining": payload.credits.as_ref().map(|c| c.remaining),
        "status": payload.status.as_ref().map(|s| s.indicator),
        "authWarning": payload.auth_warning,
    })
}

/// `caut_usage_status` result.
fn usage_summary(results: &UsageResults, now: DateTime<Utc>) -> Value {
    json!({
        "providers": results
            .payloads
            .iter()
            .map(|p| payload_summary(p, now))
            .collect::<Vec<_>>(),
        "errors": results.errors,
        "generatedAt": now,
    })
}

/// `caut_recommend_account` result: the account with the most headroom
/// (`100 - worst window`) that clears `min_remaining`.
fn recommendation(
    provider: Provider,
    results: &UsageResults,
    min_remaining: f64,
    active: Option<&str>,
) -> Value {
    let mut candidates: Vec<(Option<String>, f64, &ProviderPayload)> = results
        .payloads
        .iter()
        .filter_map(|p| {
            worst_used_percent(p).map(|worst| (p.account.clone(), (100.0 - worst).max(0.0), p))
        })
        .collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let best = candidates
        .first()
        .filter(|(_, headroom, _)| *headroom >= min_remaining);
    let reason = match (best, candidates.first()) {
        (Some((account, headroom, _)), _) => format!(
            "{} has the most headroom: {:.0}% left in its tightest window",
            account.as_deref().unwrap_or("The current login"),
            headroom
        ),
        (None, Some((_, headroom, _))) => {
            format!("No account has at least {min_remaining:.0}% left; the best has {headroom:.0}%")
        }
        (None, None) => "No usage data could be fetched for any account".to_string(),
    };
    json!({
        "provider": provider.cli_name(),
        "recommended": best.map(|(account, headroom, _)| json!({
            "account": account,
            "remainingPercent": round1(*headroom),
            "isActive": account.as_deref().is_some_and(|a| Some(a) == active),
        })),
        "activeAccount": active,
        "minRemainingPercent": min_remaining,
        "reason": reason,
        "candidates": candidates
            .iter()
            .map(|(account, headroom, payload)| json!({
                "account": account,
                "remainingPercent": round1(*headroom),
                "health": health(worst_used_percent(payload)),
            }))
            .collect::<Vec<_>>(),
        "errors": results.errors,
        "howToSwitch": "Call caut_switch_account (or `caut token-accounts use`) to make it the default.",
    })
}

/// Forecast for one provider: per-window pace plus, with enough history,
/// the primary window's recent burn rate.
fn forecast_payload(
    payload: &ProviderPayload,
    history: Option<&[StoredSnapshot]>,
    now: DateTime<Utc>,
) -> Value {
    let windows: Vec<Value> = named_windows(payload)
        .iter()
        .map(|(name, window)| {
            let pace = UsagePace::of(window, now);
            json!({
                "name": name,
                "usedPercent": round1(window.used_percent),
                "resetsAt": window.resets_at,
                "pace": pace,
                "summary": pace.map(|p| p.summary()),
            })
        })
        .collect();
    let velocity = history
        .and_then(|h| crate::core::prediction::calculate_velocity(h, chrono::Duration::hours(6)));
    let hours_to_limit = match (velocity, payload.usage.primary.as_ref()) {
        (Some(v), Some(primary)) if v > 0.0 => {
            Some(round1((100.0 - primary.used_percent).max(0.0) / v))
        }
        _ => None,
    };
    json!({
        "provider": payload.provider,
        "account": payload.account,
        "windows": windows,
        "primaryVelocityPercentPerHour": velocity.map(round1),
        "primaryHoursToLimit": hours_to_limit,
    })
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

// =============================================================================
// Protocol helpers
// =============================================================================

fn initialize_result(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "listChanged": false, "subscribe": false },
        },
        "serverInfo": { "name": "caut", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "caut reports rate-limit and quota usage for LLM coding providers \
            (Claude, Codex, Gemini, Cursor, Copilot, ...). Call caut_usage_status before long \
            tasks; when a provider is near its limit, call caut_recommend_account and \
            caut_switch_account to move to the account with the most headroom.",
    })
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_result(value: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": is_error,
    })
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn required_provider(args: &Value) -> Result<Provider> {
    let name = str_arg(args, "provider")
        .ok_or_else(|| CautError::Config("`provider` is required".to_string()))?;
    Provider::from_cli_name(&name)
}

fn provider_names() -> Vec<&'static str> {
    Provider::ALL.iter().map(|p| p.cli_name()).collect()
}

fn tool_definitions() -> Value {
    let providers = provider_names();
    let mut selection = providers.clone();
    selection.extend(["both", "all"]);
    json!([
        {
            "name": "caut_usage_status",
            "title": "Usage status",
            "description": "Current rate-limit / quota usage per provider: every window with percent used, \
                reset time and a health grade (ok, warning >=75%, critical >=90%, exhausted).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "enum": selection,
                        "description": "One provider, 'both' (codex+claude) or 'all'. Defaults to the configured providers." },
                    "account": { "type": "string", "description": "Token account label (needs a single provider)." },
                    "all_accounts": { "type": "boolean", "description": "Report every token account of the provider." },
                    "source": { "type": "string", "enum": ["auto", "web", "cli", "oauth", "api", "local"] },
                    "status": { "type": "boolean", "description": "Also fetch the provider status page." }
                }
            }
        },
        {
            "name": "caut_recommend_account",
            "title": "Recommend account",
            "description": "Fetch every token account of a provider and recommend the one with the most \
                headroom in its tightest window.",
            "inputSchema": {
                "type": "object",
                "required": ["provider"],
                "properties": {
                    "provider": { "type": "string", "enum": providers },
                    "min_remaining_percent": { "type": "number", "minimum": 0, "maximum": 100, "default": DEFAULT_MIN_REMAINING }
                }
            }
        },
        {
            "name": "caut_switch_account",
            "title": "Switch account",
            "description": "Make a token account caut's default for a provider (same as `caut token-accounts use`).",
            "inputSchema": {
                "type": "object",
                "required": ["provider", "account"],
                "properties": {
                    "provider": { "type": "string", "enum": providers },
                    "account": { "type": "string", "description": "Token account label." }
                }
            }
        },
        {
            "name": "caut_forecast",
            "title": "Forecast",
            "description": "Pace per window (on pace / in deficit / in reserve, when it runs out before reset) \
                and the recent burn rate from usage history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "enum": selection }
                }
            }
        },
        {
            "name": "caut_cost",
            "title": "Local cost",
            "description": "Token and dollar cost from local Claude Code / Codex session logs (today, last 30 days, daily).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "enum": ["claude", "codex", "both"] }
                }
            }
        }
    ])
}

fn resource_definitions() -> Value {
    json!([{
        "uri": "caut://providers",
        "name": "providers",
        "title": "Supported providers",
        "description": "Every provider caut can query, its window labels, token-account support and dashboard.",
        "mimeType": "application/json"
    }])
}

fn read_resource(params: &Value) -> std::result::Result<Value, (i64, String)> {
    let uri = params.get("uri").and_then(Value::as_str).ok_or_else(|| {
        (
            code::INVALID_PARAMS,
            "resources/read needs a uri".to_string(),
        )
    })?;
    if uri != "caut://providers" {
        return Err((code::INVALID_PARAMS, format!("Unknown resource: {uri}")));
    }
    let providers: Vec<Value> = Provider::ALL
        .iter()
        .map(|p| {
            json!({
                "name": p.cli_name(),
                "displayName": p.display_name(),
                "windows": [p.session_label(), p.weekly_label()],
                "tertiaryWindow": p.tertiary_label(),
                "tokenAccounts": p.supports_token_accounts(),
                "localCostScan": p.supports_cost_scan(),
                "dashboard": p.dashboard_url(),
                "statusPage": p.status_page_url(),
            })
        })
        .collect();
    let text = serde_json::to_string_pretty(&providers).unwrap_or_default();
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{ProviderCostSnapshot, ScopedWindow, UsageSnapshot};
    use crate::storage::token_accounts::TokenAccount;

    fn server() -> (McpServer, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (McpServer::new(dir.path().join("token-accounts.json")), dir)
    }

    fn window(used: f64) -> RateWindow {
        RateWindow {
            used_percent: used,
            window_minutes: Some(300),
            resets_at: None,
            reset_description: None,
        }
    }

    fn payload(
        provider: &str,
        account: Option<&str>,
        primary: f64,
        secondary: f64,
    ) -> ProviderPayload {
        ProviderPayload {
            provider: provider.to_string(),
            account: account.map(str::to_string),
            version: None,
            source: "api".to_string(),
            status: None,
            usage: UsageSnapshot {
                primary: Some(window(primary)),
                secondary: Some(window(secondary)),
                ..UsageSnapshot::empty()
            },
            credits: None,
            antigravity_plan_info: None,
            openai_dashboard: None,
            auth_warning: None,
        }
    }

    #[tokio::test]
    async fn initialize_negotiates_protocol_version() {
        let (server, _dir) = server();
        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#)
            .await
            .unwrap();
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "caut");
        assert!(response["result"]["capabilities"]["tools"].is_object());

        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#)
            .await
            .unwrap();
        assert_eq!(
            response["result"]["protocolVersion"],
            SUPPORTED_PROTOCOL_VERSIONS[0]
        );
    }

    #[tokio::test]
    async fn notifications_get_no_reply_and_ping_does() {
        let (server, _dir) = server();
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .await
                .is_none()
        );
        let pong = server
            .handle_line(r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#)
            .await
            .unwrap();
        assert_eq!(pong["id"], "a");
        assert_eq!(pong["result"], json!({}));
    }

    #[tokio::test]
    async fn protocol_errors() {
        let (server, _dir) = server();
        let parse = server.handle_line("{not json").await.unwrap();
        assert_eq!(parse["error"]["code"], code::PARSE_ERROR);
        let missing = server
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#)
            .await
            .unwrap();
        assert_eq!(missing["error"]["code"], code::METHOD_NOT_FOUND);
        let unknown_tool = server
            .handle_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"x"}}"#)
            .await
            .unwrap();
        assert_eq!(unknown_tool["error"]["code"], code::INVALID_PARAMS);
        let batch = server
            .handle_line(r#"[{"jsonrpc":"2.0","id":5,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/x"}]"#)
            .await
            .unwrap();
        assert_eq!(batch.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tools_list_has_schemas() {
        let (server, _dir) = server();
        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .await
            .unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "caut_usage_status",
                "caut_recommend_account",
                "caut_switch_account",
                "caut_forecast",
                "caut_cost"
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[tokio::test]
    async fn providers_resource_lists_every_provider() {
        let (server, _dir) = server();
        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"caut://providers"}}"#)
            .await
            .unwrap();
        let text = response["result"]["contents"][0]["text"].as_str().unwrap();
        let providers: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(providers.len(), Provider::ALL.len());
        let bad = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"caut://x"}}"#,
            )
            .await
            .unwrap();
        assert_eq!(bad["error"]["code"], code::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn switch_account_sets_active_and_reports_errors_as_tool_errors() {
        let (server, _dir) = server();
        let mut store = TokenAccountStore::load(&server.token_accounts_path).unwrap();
        store
            .add(Provider::Zai, TokenAccount::new("personal", "a"))
            .unwrap();
        store
            .add(Provider::Zai, TokenAccount::new("team", "b"))
            .unwrap();
        store.save().unwrap();

        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"caut_switch_account","arguments":{"provider":"zai","account":"team"}}}"#)
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(
            response["result"]["structuredContent"]["activeAccount"],
            "team"
        );
        let store = TokenAccountStore::load(&server.token_accounts_path).unwrap();
        assert_eq!(store.get_active(Provider::Zai).unwrap().label, "team");

        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"caut_switch_account","arguments":{"provider":"zai","account":"nobody"}}}"#)
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], true);
        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"caut_switch_account","arguments":{"provider":"bogus","account":"x"}}}"#)
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn cost_rejects_providers_without_local_logs() {
        let (server, _dir) = server();
        let response = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"caut_cost","arguments":{"provider":"zai"}}}"#)
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], true);
    }

    #[test]
    fn health_grades() {
        assert_eq!(health(None), "unknown");
        assert_eq!(health(Some(10.0)), "ok");
        assert_eq!(health(Some(80.0)), "warning");
        assert_eq!(health(Some(95.0)), "critical");
        assert_eq!(health(Some(100.0)), "exhausted");
    }

    #[test]
    fn worst_window_includes_scoped_and_cost() {
        let mut p = payload("claude", None, 10.0, 20.0);
        assert_eq!(worst_used_percent(&p), Some(20.0));
        p.usage.scoped.push(ScopedWindow {
            label: "Opus".into(),
            kind: None,
            severity: None,
            is_active: false,
            window: window(100.0),
        });
        assert_eq!(worst_used_percent(&p), Some(100.0));
        let mut q = payload("cursor", None, 5.0, 5.0);
        q.usage.provider_cost = Some(ProviderCostSnapshot {
            used: 9.0,
            limit: 10.0,
            currency_code: "USD".into(),
            period: None,
            resets_at: None,
            updated_at: Utc::now(),
        });
        assert_eq!(worst_used_percent(&q), Some(90.0));
        let summary = payload_summary(&q, Utc::now());
        assert_eq!(summary["health"], "critical");
        assert_eq!(summary["windows"][0]["name"], "Total");
    }

    #[test]
    fn recommendation_picks_most_headroom_above_threshold() {
        let results = UsageResults {
            payloads: vec![
                payload("zai", Some("personal"), 80.0, 30.0),
                payload("zai", Some("team"), 10.0, 40.0),
            ],
            errors: vec!["zai [old]: auth".into()],
        };
        let rec = recommendation(Provider::Zai, &results, 20.0, Some("personal"));
        assert_eq!(rec["recommended"]["account"], "team");
        assert_eq!(rec["recommended"]["remainingPercent"], 60.0);
        assert_eq!(rec["recommended"]["isActive"], false);
        assert_eq!(rec["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(rec["errors"][0], "zai [old]: auth");

        let rec = recommendation(Provider::Zai, &results, 70.0, None);
        assert!(rec["recommended"].is_null());
        assert!(
            rec["reason"]
                .as_str()
                .unwrap()
                .contains("No account has at least 70%")
        );

        let empty = UsageResults {
            payloads: vec![],
            errors: vec![],
        };
        let rec = recommendation(Provider::Zai, &empty, 20.0, None);
        assert!(rec["recommended"].is_null());
    }

    #[test]
    fn forecast_reports_pace_and_history_velocity() {
        let now = Utc::now();
        let mut p = payload("codex", None, 50.0, 70.0);
        p.usage.secondary = Some(RateWindow {
            used_percent: 70.0,
            window_minutes: Some(10_080),
            resets_at: Some(now + chrono::Duration::hours(84)),
            reset_description: None,
        });
        let value = forecast_payload(&p, None, now);
        assert_eq!(
            value["windows"][1]["summary"],
            "20% in deficit · Runs out in 1d 12h"
        );
        assert!(value["primaryVelocityPercentPerHour"].is_null());
    }
}
