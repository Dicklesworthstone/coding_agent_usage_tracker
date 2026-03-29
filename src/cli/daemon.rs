//! Resident daemon support for usage watch mode.

use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch as tokio_watch;
use tokio::time::{Duration, Instant, interval_at, timeout};

use crate::cli::args::{DaemonCommand, DaemonStartArgs, OutputFormat, UsageArgs};
use crate::cli::usage::{UsageResults, fetch_usage, render_usage_results};
use crate::cli::watch::WatchState;
use crate::error::{CautError, Result};
use crate::storage::cache;
use crate::storage::paths::AppPaths;

type FetchFuture = Pin<Box<dyn Future<Output = Result<UsageResults>> + Send>>;
type UsageFetcher = Arc<dyn Fn(UsageArgs) -> FetchFuture + Send + Sync>;

const LOOPBACK_HOST: &str = "127.0.0.1";
const PORT_FILE_CONNECT_TIMEOUT_MS: u64 = 200;
const REQUEST_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResidentMetadata {
    pid: u32,
    port: u16,
    nonce: String,
    started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResidentEnvelope {
    nonce: String,
    request: ResidentRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum ResidentRequest {
    Status,
    Refresh,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResidentResponse {
    Status { snapshot: ResidentStatusSnapshot },
    Ack { accepted: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ResidentStatusSnapshot {
    pub last_results: Option<Vec<crate::core::models::ProviderPayload>>,
    pub last_errors: Vec<String>,
    pub last_fetch_at: Option<DateTime<Utc>>,
    pub fetch_count: u64,
    pub error_count: u64,
    pub last_error: Option<String>,
    pub refresh_in_progress: bool,
}

#[derive(Debug, Default)]
struct ResidentState {
    watch_state: WatchState,
    refresh_in_progress: bool,
    refresh_pending: bool,
    nonce: String,
}

impl ResidentState {
    fn snapshot(&self) -> ResidentStatusSnapshot {
        ResidentStatusSnapshot {
            last_results: self.watch_state.last_results.clone(),
            last_errors: self.watch_state.last_errors.clone(),
            last_fetch_at: self.watch_state.last_fetch_at,
            fetch_count: self.watch_state.fetch_count,
            error_count: self.watch_state.error_count,
            last_error: self
                .watch_state
                .last_error
                .as_ref()
                .map(ToString::to_string),
            refresh_in_progress: self.refresh_in_progress,
        }
    }

    const fn request_refresh(&mut self) -> bool {
        if self.refresh_in_progress {
            self.refresh_pending = true;
            false
        } else {
            self.refresh_in_progress = true;
            true
        }
    }

    fn finish_refresh(&mut self, result: Result<UsageResults>) -> bool {
        self.watch_state.update(result);
        if self.refresh_pending {
            self.refresh_pending = false;
            self.refresh_in_progress = true;
            true
        } else {
            self.refresh_in_progress = false;
            false
        }
    }
}

/// Execute a resident-daemon lifecycle command.
///
/// # Errors
/// Returns an error if daemon startup, client communication, rendering, or
/// shutdown handling fails.
pub async fn execute(
    command: &DaemonCommand,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    match command {
        DaemonCommand::Start(args) => start_command(args).await,
        DaemonCommand::Status => status_command(format, pretty, no_color).await,
        DaemonCommand::Refresh => refresh_command().await,
        DaemonCommand::Stop => stop_command().await,
        DaemonCommand::Run(args) => run_command(args).await,
    }
}

async fn start_command(args: &DaemonStartArgs) -> Result<()> {
    args.validate()?;
    let paths = AppPaths::new();
    paths.ensure_dirs()?;

    if let Some(metadata) = read_metadata_if_live(&paths.daemon_metadata_file()).await? {
        return Err(CautError::Config(format!(
            "Resident daemon already running on {}:{}",
            LOOPBACK_HOST, metadata.port
        )));
    }

    remove_stale_metadata(&paths.daemon_metadata_file())?;

    let current_exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(current_exe);
    command.arg("daemon");
    command.arg("run");
    append_usage_args(&mut command, &args.to_usage_args());
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.spawn()?;

    let metadata = wait_for_metadata(&paths.daemon_metadata_file()).await?;
    println!(
        "Resident daemon started on {}:{}",
        LOOPBACK_HOST, metadata.port
    );
    Ok(())
}

async fn run_command(args: &DaemonStartArgs) -> Result<()> {
    args.validate()?;
    let paths = AppPaths::new();
    run_resident_with_fetcher(args.clone(), paths, production_fetcher(), None).await
}

async fn status_command(format: OutputFormat, pretty: bool, no_color: bool) -> Result<()> {
    let snapshot = resident_status().await?;
    if let Some(payloads) = snapshot.last_results {
        let results = UsageResults {
            payloads,
            errors: snapshot.last_errors,
        };
        render_usage_results(&results, format, pretty, no_color)?;
        if !results.errors.is_empty() {
            return Err(CautError::PartialFailure {
                failed: results.errors.len(),
            });
        }
    } else if let Some(ref last_error) = snapshot.last_error {
        if format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        return Err(CautError::Config(format!(
            "Resident daemon has no cached snapshot yet. Last error: {last_error}"
        )));
    } else if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!("Resident daemon has no cached snapshot yet.");
    }
    Ok(())
}

async fn refresh_command() -> Result<()> {
    let response = send_request(ResidentRequest::Refresh).await?;
    match response {
        ResidentResponse::Ack { accepted } => {
            if accepted {
                println!("Resident refresh scheduled.");
            } else {
                println!("Resident refresh already pending.");
            }
            Ok(())
        }
        ResidentResponse::Status { .. } => Err(CautError::Config(
            "Unexpected resident response for refresh".to_string(),
        )),
    }
}

async fn stop_command() -> Result<()> {
    let response = send_request(ResidentRequest::Stop).await?;
    match response {
        ResidentResponse::Ack { .. } => {
            let metadata_path = AppPaths::new().daemon_metadata_file();
            wait_for_metadata_removal(&metadata_path).await?;
            println!("Resident daemon stopped.");
            Ok(())
        }
        ResidentResponse::Status { .. } => Err(CautError::Config(
            "Unexpected resident response for stop".to_string(),
        )),
    }
}

pub(crate) async fn resident_status() -> Result<ResidentStatusSnapshot> {
    match send_request(ResidentRequest::Status).await? {
        ResidentResponse::Status { snapshot } => Ok(snapshot),
        ResidentResponse::Ack { .. } => Err(CautError::Config(
            "Unexpected resident response for status".to_string(),
        )),
    }
}

fn production_fetcher() -> UsageFetcher {
    Arc::new(|args: UsageArgs| Box::pin(async move { fetch_usage(&args).await }))
}

async fn send_request(request: ResidentRequest) -> Result<ResidentResponse> {
    let metadata_path = AppPaths::new().daemon_metadata_file();
    let metadata = read_metadata_if_live(&metadata_path)
        .await?
        .ok_or_else(|| CautError::Config("Resident daemon is not running".to_string()))?;
    send_request_to_metadata(&metadata, request).await
}

async fn send_request_to_metadata(
    metadata: &ResidentMetadata,
    request: ResidentRequest,
) -> Result<ResidentResponse> {
    let address = socket_addr(metadata.port);
    let mut stream = timeout(
        Duration::from_millis(PORT_FILE_CONNECT_TIMEOUT_MS),
        TcpStream::connect(address),
    )
    .await
    .map_err(|_| CautError::ConnectionRefused {
        host: address.to_string(),
    })??;

    let payload = serde_json::to_vec(&ResidentEnvelope {
        nonce: metadata.nonce.clone(),
        request,
    })?;

    timeout(Duration::from_millis(REQUEST_TIMEOUT_MS), async {
        stream.write_all(&payload).await?;
        stream.shutdown().await?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Result::<ResidentResponse>::Ok(serde_json::from_slice(&response)?)
    })
    .await
    .map_err(|_| CautError::Timeout(REQUEST_TIMEOUT_MS / 1_000))?
}

async fn read_metadata_if_live(path: &Path) -> Result<Option<ResidentMetadata>> {
    if !path.exists() {
        return Ok(None);
    }

    let metadata: ResidentMetadata = if let Ok(metadata) = cache::read(path) {
        metadata
    } else {
        remove_stale_metadata(path)?;
        return Ok(None);
    };

    if handshake_metadata(&metadata).await {
        Ok(Some(metadata))
    } else {
        remove_stale_metadata(path)?;
        Ok(None)
    }
}

async fn handshake_metadata(metadata: &ResidentMetadata) -> bool {
    matches!(
        send_request_to_metadata(metadata, ResidentRequest::Status).await,
        Ok(ResidentResponse::Status { .. })
    )
}

fn remove_stale_metadata(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

async fn wait_for_metadata(path: &Path) -> Result<ResidentMetadata> {
    for _ in 0..50 {
        if let Ok(metadata) = cache::read(path) {
            return Ok(metadata);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(CautError::Config(
        "Timed out waiting for resident daemon to write metadata".to_string(),
    ))
}

async fn wait_for_metadata_removal(path: &Path) -> Result<()> {
    for _ in 0..50 {
        if !path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(CautError::Config(
        "Timed out waiting for resident daemon to stop".to_string(),
    ))
}

fn append_usage_args(command: &mut std::process::Command, args: &UsageArgs) {
    if let Some(provider) = &args.provider {
        command.arg("--provider");
        command.arg(provider);
    }
    if let Some(account) = &args.account {
        command.arg("--account");
        command.arg(account);
    }
    if let Some(account_index) = args.account_index {
        command.arg("--account-index");
        command.arg(account_index.to_string());
    }
    if args.all_accounts {
        command.arg("--all-accounts");
    }
    if args.no_credits {
        command.arg("--no-credits");
    }
    if args.status {
        command.arg("--status");
    }
    if let Some(source) = &args.source {
        command.arg("--source");
        command.arg(source);
    }
    if args.web {
        command.arg("--web");
    }
    if let Some(timeout_seconds) = args.timeout {
        command.arg("--timeout");
        command.arg(timeout_seconds.to_string());
    }
    if let Some(web_timeout_seconds) = args.web_timeout {
        command.arg("--web-timeout");
        command.arg(web_timeout_seconds.to_string());
    }
    if args.web_debug_dump_html {
        command.arg("--web-debug-dump-html");
    }
    command.arg("--interval");
    command.arg(args.interval.to_string());
}

fn socket_addr(port: u16) -> SocketAddr {
    format!("{LOOPBACK_HOST}:{port}")
        .parse()
        .expect("loopback socket addr")
}

async fn run_resident_with_fetcher(
    args: DaemonStartArgs,
    paths: AppPaths,
    fetcher: UsageFetcher,
    started_tx: Option<tokio::sync::oneshot::Sender<u16>>,
) -> Result<()> {
    paths.ensure_dirs()?;
    let metadata_path = paths.daemon_metadata_file();

    if let Some(metadata) = read_metadata_if_live(&metadata_path).await? {
        return Err(CautError::Config(format!(
            "Resident daemon already running on {}:{}",
            LOOPBACK_HOST, metadata.port
        )));
    }

    remove_stale_metadata(&metadata_path)?;

    let listener = TcpListener::bind(socket_addr(0)).await?;
    let port = listener.local_addr()?.port();
    let metadata = ResidentMetadata {
        pid: std::process::id(),
        port,
        nonce: generate_nonce(port),
        started_at: Utc::now(),
    };
    cache::write(&metadata_path, &metadata)?;

    if let Some(started_tx) = started_tx {
        let _ = started_tx.send(port);
    }

    let shared = Arc::new(Mutex::new(ResidentState {
        nonce: metadata.nonce.clone(),
        ..ResidentState::default()
    }));
    let (shutdown_tx, mut shutdown_rx) = tokio_watch::channel(false);
    let usage_args = args.to_usage_args();
    let interval_duration = Duration::from_secs(usage_args.interval);

    maybe_start_refresh(&shared, &fetcher, &usage_args);

    let refresh_shared = Arc::clone(&shared);
    let refresh_fetcher = Arc::clone(&fetcher);
    let refresh_usage_args = usage_args.clone();
    let refresh_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        let mut ticker = interval_at(Instant::now() + interval_duration, interval_duration);
        let mut refresh_shutdown = refresh_shutdown;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    maybe_start_refresh(&refresh_shared, &refresh_fetcher, &refresh_usage_args);
                }
                changed = refresh_shutdown.changed() => {
                    if changed.is_err() || *refresh_shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let shared = Arc::clone(&shared);
                let fetcher = Arc::clone(&fetcher);
                let usage_args = usage_args.clone();
                let shutdown_tx = shutdown_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_client(stream, shared, fetcher, usage_args, shutdown_tx).await {
                        tracing::debug!(?error, "resident client request failed");
                    }
                });
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }

    remove_stale_metadata(&metadata_path)?;
    Ok(())
}

fn maybe_start_refresh(
    shared: &Arc<Mutex<ResidentState>>,
    fetcher: &UsageFetcher,
    args: &UsageArgs,
) {
    let should_start = {
        let mut state = shared.lock().expect("resident state lock poisoned");
        state.request_refresh()
    };
    if !should_start {
        return;
    }

    spawn_refresh(shared, fetcher, args.clone());
}

fn spawn_refresh(shared: &Arc<Mutex<ResidentState>>, fetcher: &UsageFetcher, args: UsageArgs) {
    let shared = Arc::clone(shared);
    let fetcher = Arc::clone(fetcher);
    tokio::spawn(async move {
        let result = fetcher(args.clone()).await;
        let should_restart = {
            let mut state = shared.lock().expect("resident state lock poisoned");
            state.finish_refresh(result)
        };
        if should_restart {
            spawn_refresh(&shared, &fetcher, args);
        }
    });
}

async fn handle_client(
    mut stream: TcpStream,
    shared: Arc<Mutex<ResidentState>>,
    fetcher: UsageFetcher,
    usage_args: UsageArgs,
    shutdown_tx: tokio_watch::Sender<bool>,
) -> Result<()> {
    let mut request_bytes = Vec::new();
    stream.read_to_end(&mut request_bytes).await?;
    let envelope: ResidentEnvelope = serde_json::from_slice(&request_bytes)?;
    let expected_nonce = {
        let state = shared.lock().expect("resident state lock poisoned");
        state.nonce.clone()
    };
    if envelope.nonce != expected_nonce {
        return Err(CautError::ConnectionRefused {
            host: LOOPBACK_HOST.to_string(),
        });
    }

    let request = envelope.request;
    let should_shutdown = matches!(request, ResidentRequest::Stop);

    let response = match request {
        ResidentRequest::Status => {
            let snapshot = shared
                .lock()
                .expect("resident state lock poisoned")
                .snapshot();
            ResidentResponse::Status { snapshot }
        }
        ResidentRequest::Refresh => {
            let should_start = {
                let mut state = shared.lock().expect("resident state lock poisoned");
                state.request_refresh()
            };
            if should_start {
                spawn_refresh(&shared, &fetcher, usage_args);
            }
            ResidentResponse::Ack {
                accepted: should_start,
            }
        }
        ResidentRequest::Stop => ResidentResponse::Ack { accepted: true },
    };

    let payload = serde_json::to_vec(&response)?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    stream.shutdown().await?;
    if should_shutdown {
        let _ = shutdown_tx.send(true);
    }
    Ok(())
}

fn generate_nonce(port: u16) -> String {
    let mut hasher = Sha256::new();
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(port.to_le_bytes());
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    hasher.update(now_nanos.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_test_provider_payload;
    use std::path::PathBuf;
    use tokio::sync::{Notify, oneshot};

    fn daemon_args(interval: u64) -> DaemonStartArgs {
        DaemonStartArgs {
            usage: UsageArgs {
                provider: Some("codex".to_string()),
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
                interval,
                tui: false,
            },
        }
    }

    fn metadata_path(root: &Path) -> PathBuf {
        root.join("cache").join("resident-daemon.json")
    }

    fn test_paths(root: &Path) -> AppPaths {
        AppPaths {
            config: root.join("config"),
            cache: root.join("cache"),
            data: root.join("data"),
        }
    }

    fn static_fetcher(results: UsageResults) -> UsageFetcher {
        Arc::new(move |_args: UsageArgs| {
            let results = results.clone();
            Box::pin(async move { Ok(results) })
        })
    }

    fn delayed_fetcher(delay: Duration, notify: Arc<Notify>) -> UsageFetcher {
        Arc::new(move |_args: UsageArgs| {
            let notify = Arc::clone(&notify);
            Box::pin(async move {
                notify.notify_one();
                tokio::time::sleep(delay).await;
                Ok(UsageResults {
                    payloads: vec![make_test_provider_payload("codex", "cli")],
                    errors: Vec::new(),
                })
            })
        })
    }

    async fn spawn_test_daemon(
        root: &Path,
        fetcher: UsageFetcher,
    ) -> (tokio::task::JoinHandle<Result<()>>, u16) {
        let (started_tx, started_rx) = oneshot::channel();
        let args = daemon_args(60);
        let paths = test_paths(root);
        let handle = tokio::spawn(run_resident_with_fetcher(
            args,
            paths,
            fetcher,
            Some(started_tx),
        ));
        let port = started_rx.await.expect("port from resident");
        (handle, port)
    }

    #[tokio::test]
    async fn resident_port_file_written() {
        let temp = tempfile::tempdir().expect("tempdir");
        let results = UsageResults {
            payloads: vec![make_test_provider_payload("codex", "cli")],
            errors: Vec::new(),
        };
        let (handle, port) = spawn_test_daemon(temp.path(), static_fetcher(results)).await;

        let metadata: ResidentMetadata =
            cache::read(&metadata_path(temp.path())).expect("metadata file");
        assert_eq!(metadata.port, port);
        assert_eq!(metadata.pid, std::process::id());

        let _ = send_request_to_metadata(&metadata, ResidentRequest::Stop).await;
        handle.await.expect("resident join").expect("resident exit");
    }

    #[tokio::test]
    async fn resident_replaces_stale_port_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let stale_metadata = ResidentMetadata {
            pid: 999_999,
            port: 9,
            nonce: "stale".to_string(),
            started_at: Utc::now(),
        };
        let path = metadata_path(temp.path());
        cache::write(&path, &stale_metadata).expect("write stale metadata");

        let results = UsageResults {
            payloads: vec![make_test_provider_payload("codex", "cli")],
            errors: Vec::new(),
        };
        let (handle, port) = spawn_test_daemon(temp.path(), static_fetcher(results)).await;

        let metadata: ResidentMetadata = cache::read(&path).expect("fresh metadata");
        assert_eq!(metadata.port, port);
        assert_ne!(metadata.port, stale_metadata.port);

        let _ = send_request_to_metadata(&metadata, ResidentRequest::Stop).await;
        handle.await.expect("resident join").expect("resident exit");
    }

    #[tokio::test]
    async fn resident_status_returns_last_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let expected_payload = make_test_provider_payload("codex", "cli");
        let results = UsageResults {
            payloads: vec![expected_payload.clone()],
            errors: Vec::new(),
        };
        let (handle, _port) = spawn_test_daemon(temp.path(), static_fetcher(results)).await;

        for _ in 0..20 {
            let metadata: ResidentMetadata =
                cache::read(&metadata_path(temp.path())).expect("metadata");
            let response = send_request_to_metadata(&metadata, ResidentRequest::Status)
                .await
                .expect("status response");
            if let ResidentResponse::Status { snapshot } = response
                && let Some(payloads) = snapshot.last_results
            {
                assert_eq!(payloads.len(), 1);
                assert_eq!(payloads[0].provider, expected_payload.provider);
                assert_eq!(payloads[0].source, expected_payload.source);
                assert_eq!(payloads[0].account, expected_payload.account);
                let _ = send_request_to_metadata(&metadata, ResidentRequest::Stop).await;
                handle.await.expect("resident join").expect("resident exit");
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        panic!("resident did not publish last snapshot");
    }

    #[tokio::test]
    async fn resident_refresh_is_non_blocking() {
        let temp = tempfile::tempdir().expect("tempdir");
        let notify = Arc::new(Notify::new());
        let (handle, _port) = spawn_test_daemon(
            temp.path(),
            delayed_fetcher(Duration::from_millis(250), Arc::clone(&notify)),
        )
        .await;

        notify.notified().await;
        let metadata: ResidentMetadata =
            cache::read(&metadata_path(temp.path())).expect("metadata");
        let start = std::time::Instant::now();
        let response = send_request_to_metadata(&metadata, ResidentRequest::Refresh)
            .await
            .expect("refresh response");
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(matches!(response, ResidentResponse::Ack { .. }));

        let _ = send_request_to_metadata(&metadata, ResidentRequest::Stop).await;
        handle.await.expect("resident join").expect("resident exit");
    }

    #[tokio::test]
    async fn resident_stop_removes_port_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let results = UsageResults {
            payloads: vec![make_test_provider_payload("codex", "cli")],
            errors: Vec::new(),
        };
        let (handle, _port) = spawn_test_daemon(temp.path(), static_fetcher(results)).await;

        let path = metadata_path(temp.path());
        assert!(path.exists(), "metadata file should exist while running");

        let metadata: ResidentMetadata = cache::read(&path).expect("metadata");
        let _ = send_request_to_metadata(&metadata, ResidentRequest::Stop).await;
        handle.await.expect("resident join").expect("resident exit");
        assert!(!path.exists(), "metadata file should be removed on stop");
    }
}
