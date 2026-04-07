//! Background HTTP server for programmatic queries.
//!
//! Implements `caut serve` which starts a lightweight HTTP daemon that
//! caches usage data and responds to queries from other processes.
//! This enables plugins and scripts to query caut data without launching
//! the full TUI or paying cold-start penalties on each invocation.

use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};

use crate::cli::args::ServeArgs;
use crate::cli::usage::fetch_usage;
use crate::core::models::ProviderPayload;
use crate::error::{CautError, Result};
use crate::storage::AppPaths;

/// Cached state shared between the refresh loop and HTTP handlers.
#[derive(Debug, Clone, Default)]
struct ServerState {
    payloads: Vec<ProviderPayload>,
    errors: Vec<String>,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
    refresh_count: u64,
}

/// Health check response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
    refresh_count: u64,
    cached_providers: usize,
    cached_errors: usize,
}

/// Server info written to the PID/info file so clients can discover the server.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    pid: u32,
    bind: String,
    port: u16,
    started_at: chrono::DateTime<chrono::Utc>,
}

/// Write the server info file so `caut query` can auto-discover the server.
fn write_server_info(args: &ServeArgs, paths: &AppPaths) -> Result<()> {
    let info = ServerInfo {
        pid: std::process::id(),
        bind: args.bind.clone(),
        port: args.port,
        started_at: chrono::Utc::now(),
    };

    let info_path = args
        .pid_file
        .clone()
        .unwrap_or_else(|| paths.data.join("caut-server.json"));

    if let Some(parent) = info_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(&info)
        .map_err(|e| CautError::Config(format!("Failed to serialize server info: {e}")))?;
    std::fs::write(&info_path, json)?;

    tracing::info!("Server info written to {}", info_path.display());
    Ok(())
}


/// Handle an incoming HTTP request.
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    state: Arc<RwLock<ServerState>>,
    started_at: std::time::Instant,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    tracing::debug!(%method, %path, "Handling request");

    let response = match (method, path.as_str()) {
        (Method::GET, "/usage") => {
            let state = state.read().await;
            let output = crate::core::models::RobotOutput::usage(
                state.payloads.clone(),
                state.errors.clone(),
            );
            drop(state);
            json_response(StatusCode::OK, &output)
        }

        (Method::GET, "/cost") => {
            // Cost requires a fresh scan, not cached usage data.
            // Run it on demand.
            let cost_result = fetch_cost_data().await;
            match cost_result {
                Ok(output_json) => ok_raw_json(output_json),
                Err(e) => json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &serde_json::json!({ "error": e.to_string() }),
                ),
            }
        }

        (Method::GET, "/session") => {
            let session_result = fetch_session_data().await;
            match session_result {
                Ok(output_json) => ok_raw_json(output_json),
                Err(e) => json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &serde_json::json!({ "error": e.to_string() }),
                ),
            }
        }

        (Method::GET, "/health") => {
            let state = state.read().await;
            let health = HealthResponse {
                status: "ok",
                version: env!("CARGO_PKG_VERSION"),
                uptime_seconds: started_at.elapsed().as_secs(),
                last_refresh: state.last_refresh,
                refresh_count: state.refresh_count,
                cached_providers: state.payloads.len(),
                cached_errors: state.errors.len(),
            };
            drop(state);
            json_response(StatusCode::OK, &health)
        }

        (Method::GET, "/") => {
            let endpoints = serde_json::json!({
                "endpoints": [
                    { "path": "/usage", "method": "GET", "description": "Cached provider usage data (JSON)" },
                    { "path": "/cost", "method": "GET", "description": "On-demand local cost scan (JSON)" },
                    { "path": "/session", "method": "GET", "description": "Recent session cost data (JSON)" },
                    { "path": "/health", "method": "GET", "description": "Server health and uptime" },
                ],
                "version": env!("CARGO_PKG_VERSION"),
            });
            json_response(StatusCode::OK, &endpoints)
        }

        _ => json_response(
            StatusCode::NOT_FOUND,
            &serde_json::json!({ "error": "not found", "path": path }),
        ),
    };

    Ok(response)
}

/// Build a JSON HTTP response.
fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<Full<Bytes>> {
    let json = serde_json::to_string(body).unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string());
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(Full::new(Bytes::from(json)))
        .expect("response builder should not fail")
}

/// Build a response from pre-serialized JSON.
fn ok_raw_json(json: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(Full::new(Bytes::from(json)))
        .expect("response builder should not fail")
}

/// Fetch cost data on demand.
async fn fetch_cost_data() -> Result<String> {
    use crate::core::cost_scanner::CostScanner;
    use crate::core::models::RobotOutput;
    use crate::core::provider::ProviderSelection;

    let selection = ProviderSelection::default();
    let providers: Vec<_> = selection
        .providers()
        .into_iter()
        .filter(|p| p.supports_cost_scan())
        .collect();

    let scanner = CostScanner::new();
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for provider in &providers {
        match scanner.scan(*provider, false).await {
            Ok(payload) => results.push(payload),
            Err(e) => errors.push(format!("{}: {}", provider.cli_name(), e)),
        }
    }

    let output = RobotOutput::cost(results, errors);
    serde_json::to_string(&output)
        .map_err(|e| CautError::Config(format!("Failed to serialize cost data: {e}")))
}

/// Fetch session data on demand.
async fn fetch_session_data() -> Result<String> {
    use crate::cli::args::SessionArgs;

    // Use default session args (shows most recent session)
    let args = SessionArgs::default();
    let output = crate::cli::session::build_session_output(&args)?;
    serde_json::to_string(&output)
        .map_err(|e| CautError::Config(format!("Failed to serialize session data: {e}")))
}

/// Perform the initial usage fetch and populate the shared state.
async fn initial_fetch(
    usage_args: &crate::cli::args::UsageArgs,
    state: &Arc<RwLock<ServerState>>,
) {
    tracing::info!("Performing initial usage fetch...");
    match fetch_usage(usage_args).await {
        Ok(results) => {
            let provider_count = results.payloads.len();
            let mut s = state.write().await;
            s.payloads = results.payloads;
            s.errors = results.errors;
            s.last_refresh = Some(chrono::Utc::now());
            s.refresh_count = 1;
            drop(s);
            eprintln!("Initial fetch complete: {provider_count} provider(s)");
        }
        Err(e) => {
            eprintln!("Warning: initial fetch failed: {e}");
            tracing::warn!("Initial fetch failed: {}", e);
        }
    }
}

/// Spawn the background refresh task that periodically re-fetches usage data.
fn spawn_refresh_task(
    state: Arc<RwLock<ServerState>>,
    refresh_interval: Duration,
    usage_args: crate::cli::args::UsageArgs,
) {
    tokio::spawn(async move {
        let mut ticker = interval(refresh_interval);
        // Skip the first tick (we already did the initial fetch)
        ticker.tick().await;

        loop {
            ticker.tick().await;
            tracing::debug!("Background refresh tick");
            match fetch_usage(&usage_args).await {
                Ok(results) => {
                    let mut s = state.write().await;
                    s.payloads = results.payloads;
                    s.errors = results.errors;
                    s.last_refresh = Some(chrono::Utc::now());
                    s.refresh_count += 1;
                    drop(s);
                    tracing::debug!("Refresh complete");
                }
                Err(e) => {
                    tracing::warn!("Background refresh failed: {}", e);
                }
            }
        }
    });
}

/// Spawn the shutdown handler that cleans up the server info file on Ctrl+C.
fn spawn_shutdown_handler(
    pid_file: Option<std::path::PathBuf>,
    paths: AppPaths,
) -> tokio::sync::oneshot::Receiver<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("\nShutting down...");
        let info_path = pid_file.unwrap_or_else(|| paths.data.join("caut-server.json"));
        if info_path.exists() {
            let _ = std::fs::remove_file(&info_path);
        }
        let _ = shutdown_tx.send(());
    });
    shutdown_rx
}

/// Run the accept loop, dispatching connections to the request handler.
async fn accept_loop(
    listener: TcpListener,
    state: Arc<RwLock<ServerState>>,
    started_at: std::time::Instant,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, peer_addr)) => {
                        tracing::debug!(%peer_addr, "Accepted connection");
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);
                            let svc = service_fn(move |req| {
                                let state = Arc::clone(&state);
                                handle_request(req, state, started_at)
                            });
                            if let Err(e) = http1::Builder::new()
                                .serve_connection(io, svc)
                                .await
                            {
                                tracing::warn!(%peer_addr, error = %e, "Connection error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Accept error");
                    }
                }
            }
            _ = &mut shutdown_rx => {
                tracing::info!("Server shutdown complete");
                break;
            }
        }
    }
}

/// Execute the `serve` command: start the background HTTP server.
///
/// # Errors
/// Returns an error if the server cannot bind to the specified address,
/// or if the initial usage fetch fails.
pub async fn execute(args: &ServeArgs) -> Result<()> {
    let paths = AppPaths::new();
    paths.ensure_dirs()?;

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .map_err(|e| CautError::Config(format!("Invalid bind address: {e}")))?;

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| CautError::Config(format!("Failed to bind to {addr}: {e}")))?;

    eprintln!("caut server listening on http://{addr}");
    eprintln!("Endpoints: /usage /cost /session /health");
    eprintln!("Refresh interval: {}s", args.interval);
    eprintln!("Press Ctrl+C to stop.");

    write_server_info(args, &paths)?;

    let state = Arc::new(RwLock::new(ServerState::default()));
    let started_at = std::time::Instant::now();
    let usage_args = args.to_usage_args();

    initial_fetch(&usage_args, &state).await;
    spawn_refresh_task(Arc::clone(&state), Duration::from_secs(args.interval), usage_args);
    let shutdown_rx = spawn_shutdown_handler(args.pid_file.clone(), paths);

    accept_loop(listener, state, started_at, shutdown_rx).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_response_serializes() {
        let health = HealthResponse {
            status: "ok",
            version: "0.1.0",
            uptime_seconds: 42,
            last_refresh: None,
            refresh_count: 0,
            cached_providers: 0,
            cached_errors: 0,
        };
        let json = serde_json::to_string(&health).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"uptimeSeconds\":42"));
    }

    #[test]
    fn server_info_serializes() {
        let info = ServerInfo {
            pid: 12345,
            bind: "127.0.0.1".to_string(),
            port: 19485,
            started_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"pid\":12345"));
        assert!(json.contains("\"port\":19485"));
    }
}
