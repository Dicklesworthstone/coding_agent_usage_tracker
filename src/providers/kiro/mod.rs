//! Kiro provider (AWS `kiro-cli`).
//!
//! Ports `CodexBar`'s Kiro provider (`KiroStatusProbe`,
//! `KiroUsageLimitsAPI`):
//!
//! - `kiro-cli` ([`FetchKind::Cli`]): runs `kiro-cli whoami` and
//!   `kiro-cli chat --no-interactive /usage` concurrently, strips ANSI codes
//!   and parses the credit report, then enriches it best-effort with the
//!   `GetUsageLimits` API (overage cap and charges the CLI cannot state).
//! - `kiro-api` ([`FetchKind::OAuth`]): `GetUsageLimits` alone, with the
//!   OIDC token the CLI stores in its `SQLite` state (or a selected token
//!   account's token), for when the CLI cannot run.
//!
//! The binary is `KIRO_CLI_PATH`, else `kiro-cli` on `PATH`, else
//! `~/.local/bin`, `/opt/homebrew/bin` or `/usr/local/bin`. The state
//! database is `data.sqlite3` in `KIRO_DATA_DIR`, else
//! `~/Library/Application Support/kiro-cli` (macOS),
//! `$XDG_DATA_HOME/kiro-cli` / `~/.local/share/kiro-cli` (Linux) or
//! `%LOCALAPPDATA%\kiro-cli` (Windows).

mod cli;
mod limits;
mod report;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use chrono::{Local, Utc};

use self::cli::{CliError, KiroAccountInfo};
use self::limits::KiroUsageLimits;
use self::report::KiroUsageReport;
use crate::core::cli_runner::{CliOutput, run_command};
use crate::core::fetch_plan::{FetchContext, FetchKind, FetchPlan, FetchStrategy};
use crate::core::models::UsageSnapshot;
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::common;

/// Strategy id for the CLI probe.
pub const STRATEGY_CLI: &str = "kiro-cli";

/// Strategy id for the usage-limits API.
pub const STRATEGY_API: &str = "kiro-api";

/// CLI binary name.
const CLI_NAME: &str = "kiro-cli";

/// Environment override for the CLI binary path.
const CLI_PATH_ENV: &str = "KIRO_CLI_PATH";

/// `kiro-cli whoami` deadline, as in `CodexBar`.
const ACCOUNT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// `kiro-cli chat --no-interactive /usage` deadline. `CodexBar` allows 20s;
/// caut's whole Kiro fetch runs under a 15s budget, so the probe gets 10s
/// and the optional API enrichment the rest.
const USAGE_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Budget for the best-effort API enrichment after a CLI probe.
const ENRICHMENT_TIMEOUT: Duration = Duration::from_secs(4);

// =============================================================================
// Fetch Plan
// =============================================================================

/// Create the fetch plan for Kiro.
#[must_use]
pub fn fetch_plan() -> FetchPlan {
    FetchPlan::new(
        Provider::Kiro,
        vec![
            FetchStrategy {
                id: STRATEGY_CLI,
                kind: FetchKind::Cli,
                // The CLI reports whichever account it is logged into, so it
                // cannot stand in for an explicitly selected token account.
                is_available: |ctx| ctx.account_token().is_none() && resolve_cli_binary().is_some(),
                // "Not logged in" is the answer; the API would only fail
                // with the same stale token and a vaguer message.
                should_fallback: |err| !matches!(err, CautError::AuthInvalid { .. }),
            },
            FetchStrategy {
                id: STRATEGY_API,
                kind: FetchKind::OAuth,
                // Needs the CLI's state database for the profile ARN (and,
                // without a token account, the access token).
                is_available: |_| limits::state_database_path().is_some_and(|p| p.is_file()),
                should_fallback: |_| true,
            },
        ],
    )
}

/// Run one strategy from [`fetch_plan`].
///
/// # Errors
/// Returns the strategy's error, or an error for an unknown strategy id.
pub async fn fetch(strategy_id: &str, ctx: &FetchContext) -> Result<UsageSnapshot> {
    // Type-erasing the strategy futures (process I/O, `SQLite`, HTTP) keeps
    // the pipeline's future shallow; without it, proving that future `Send`
    // overflows the compiler's recursion limit.
    let fetch: BoxedFetch<'_> = match strategy_id {
        STRATEGY_CLI => Box::pin(fetch_cli()),
        STRATEGY_API => Box::pin(fetch_api(ctx)),
        _ => return Err(super::unknown_strategy(Provider::Kiro, strategy_id)),
    };
    fetch.await
}

/// A strategy's fetch future with its concrete type erased.
type BoxedFetch<'a> = Pin<Box<dyn Future<Output = Result<UsageSnapshot>> + Send + 'a>>;

// =============================================================================
// Errors
// =============================================================================

fn provider_name() -> String {
    Provider::Kiro.cli_name().to_string()
}

fn not_logged_in() -> CautError {
    CautError::AuthInvalid {
        provider: provider_name(),
        reason: "Not logged in to Kiro. Run 'kiro-cli login' first.".to_string(),
    }
}

fn cli_not_found() -> CautError {
    CautError::CliNotFound {
        name: CLI_NAME.to_string(),
    }
}

fn from_cli_error(err: CliError) -> CautError {
    match err {
        CliError::NotLoggedIn => not_logged_in(),
        CliError::Failed(reason) => CautError::FetchFailed {
            provider: provider_name(),
            reason,
        },
        CliError::Parse(message) => CautError::ParseResponse(format!(
            "{}: Failed to parse Kiro usage: {message}",
            provider_name()
        )),
    }
}

/// Attribute a `run_command` failure to Kiro rather than to the binary path.
fn from_run_error(err: CautError) -> CautError {
    match err {
        CautError::Timeout(seconds) => CautError::TimeoutWithProvider {
            provider: provider_name(),
            seconds,
        },
        CautError::ProviderNotFound(_) => cli_not_found(),
        CautError::FetchFailed { reason, .. } => CautError::FetchFailed {
            provider: provider_name(),
            reason,
        },
        other => other,
    }
}

// =============================================================================
// CLI
// =============================================================================

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Install locations checked after `PATH`, as in `CodexBar`.
fn well_known_cli_paths(home: Option<&Path>) -> Vec<PathBuf> {
    home.map(|h| h.join(".local").join("bin").join(CLI_NAME))
        .into_iter()
        .chain([
            PathBuf::from("/opt/homebrew/bin").join(CLI_NAME),
            PathBuf::from("/usr/local/bin").join(CLI_NAME),
        ])
        .collect()
}

/// Pick the CLI binary: an executable override, then the `PATH` hit, then
/// the first executable well-known install location.
fn resolve_cli_binary_from(
    override_path: Option<&Path>,
    on_path: Option<PathBuf>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = override_path.filter(|p| is_executable(p)) {
        return Some(path.to_path_buf());
    }
    on_path.or_else(|| {
        well_known_cli_paths(home)
            .into_iter()
            .find(|p| is_executable(p))
    })
}

fn resolve_cli_binary() -> Option<PathBuf> {
    let override_path = std::env::var_os(CLI_PATH_ENV).map(PathBuf::from);
    let home = common::home_dir();
    resolve_cli_binary_from(
        override_path.as_deref(),
        which::which(CLI_NAME).ok(),
        home.as_deref(),
    )
}

/// What `kiro-cli whoami` said about the account.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AccountStatus {
    Account(KiroAccountInfo),
    NotLoggedIn,
    /// The probe failed or timed out; usage can still be reported.
    Unavailable,
}

fn account_status(result: Result<CliOutput>) -> AccountStatus {
    match result {
        Ok(out) => match cli::validate_whoami(&out.stdout, &out.stderr, out.exit_code) {
            Ok(info) => AccountStatus::Account(info),
            Err(CliError::NotLoggedIn) => AccountStatus::NotLoggedIn,
            Err(err) => {
                tracing::debug!(error = ?err, "Kiro account probe failed");
                AccountStatus::Unavailable
            }
        },
        Err(err) => {
            tracing::debug!(error = %err, "Kiro account probe failed");
            AccountStatus::Unavailable
        }
    }
}

/// The combined `/usage` output, or why the command failed.
fn usage_output(result: Result<CliOutput>) -> Result<String> {
    let out = result.map_err(from_run_error)?;
    let combined = cli::combine_output(&out.stdout, &out.stderr);
    if cli::is_login_required(&combined) {
        return Err(not_logged_in());
    }
    if out.exit_code != 0 {
        let message = cli::strip_ansi(&combined).trim().to_string();
        return Err(from_cli_error(CliError::Failed(if message.is_empty() {
            format!("Kiro CLI usage failed with status {}.", out.exit_code)
        } else {
            message
        })));
    }
    Ok(combined)
}

/// Run `whoami` and `/usage` side by side and parse the report.
///
/// A failed or unparseable usage report from a logged-out CLI is reported
/// as "not logged in", since that is the cause the user can act on.
async fn run_cli_probe(binary: &Path) -> Result<KiroUsageReport> {
    let program = binary.to_str().ok_or_else(cli_not_found)?;
    let (account, usage) = tokio::join!(
        run_command(program, &["whoami"], ACCOUNT_PROBE_TIMEOUT),
        run_command(
            program,
            &["chat", "--no-interactive", "/usage"],
            USAGE_PROBE_TIMEOUT
        ),
    );
    let account = account_status(account);
    let logged_out = account == AccountStatus::NotLoggedIn;
    let output =
        usage_output(usage).map_err(|err| if logged_out { not_logged_in() } else { err })?;
    let info = match account {
        AccountStatus::Account(info) => info,
        AccountStatus::NotLoggedIn | AccountStatus::Unavailable => KiroAccountInfo::default(),
    };
    match cli::parse_usage_output(&output, &info, &Local::now()) {
        Ok(report) => Ok(report),
        Err(CliError::Parse(_)) if logged_out => Err(not_logged_in()),
        Err(err) => Err(from_cli_error(err)),
    }
}

/// `GetUsageLimits` as an optional extra: any failure leaves the CLI's own
/// plan-relative numbers standing. The CLI ran first, so a token it
/// refreshed along the way is already on disk.
async fn enrich_best_effort() -> Option<KiroUsageLimits> {
    let db = limits::state_database_path().filter(|p| p.is_file())?;
    match tokio::time::timeout(ENRICHMENT_TIMEOUT, limits::fetch_usage_limits(&db, None)).await {
        Ok(Ok(limits)) => Some(limits),
        Ok(Err(err)) => {
            tracing::debug!(error = %err, "Kiro usage API unavailable");
            None
        }
        Err(_) => {
            tracing::debug!("Kiro usage API timed out");
            None
        }
    }
}

async fn fetch_cli() -> Result<UsageSnapshot> {
    let binary = resolve_cli_binary().ok_or_else(cli_not_found)?;
    let report = run_cli_probe(&binary).await?;
    let limits = enrich_best_effort().await;
    Ok(report
        .with_usage_limits(limits)
        .to_usage_snapshot(Utc::now()))
}

// =============================================================================
// API
// =============================================================================

/// A report built from the API alone; the plan name is the subscription
/// title, and the plan gauge only appears when the API's numbers are
/// unambiguous.
fn report_from_limits(limits: KiroUsageLimits) -> KiroUsageReport {
    let title = limits.subscription_title.clone();
    KiroUsageReport::plan_only(title.as_deref()).with_usage_limits(Some(limits))
}

async fn fetch_api(ctx: &FetchContext) -> Result<UsageSnapshot> {
    let db = limits::state_database_path().ok_or_else(|| {
        common::missing_credential(
            Provider::Kiro,
            "log in with `kiro-cli login` or set KIRO_DATA_DIR",
        )
    })?;
    let limits = limits::fetch_usage_limits(&db, ctx.account_token()).await?;
    Ok(report_from_limits(limits).to_usage_snapshot(Utc::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A successful `run_command` result, the input `account_status` and
    /// `usage_output` take.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "mirrors the Result that run_command returns"
    )]
    fn output(stdout: &str, stderr: &str, exit_code: i32) -> Result<CliOutput> {
        Ok(CliOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exit_code,
        })
    }

    #[test]
    fn fetch_plan_has_cli_then_api() {
        let plan = fetch_plan();
        assert_eq!(plan.provider, Provider::Kiro);
        let ids: Vec<_> = plan.strategies.iter().map(|s| (s.id, s.kind)).collect();
        assert_eq!(
            ids,
            vec![
                (STRATEGY_CLI, FetchKind::Cli),
                (STRATEGY_API, FetchKind::OAuth)
            ]
        );
    }

    #[test]
    fn cli_is_unavailable_for_a_selected_token_account() {
        let plan = fetch_plan();
        let ctx = FetchContext::with_account("work", "token");
        assert!(!(plan.strategies[0].is_available)(&ctx));
        // The default context depends on the machine; it must at least not
        // panic and must agree with binary resolution.
        let default = FetchContext::default();
        assert_eq!(
            (plan.strategies[0].is_available)(&default),
            resolve_cli_binary().is_some()
        );
        assert_eq!(
            (plan.strategies[1].is_available)(&default),
            (plan.strategies[1].is_available)(&ctx)
        );
    }

    #[test]
    fn cli_stops_on_auth_errors_only() {
        let plan = fetch_plan();
        let cli = &plan.strategies[0];
        assert!(!(cli.should_fallback)(&not_logged_in()));
        assert!((cli.should_fallback)(&from_cli_error(CliError::Parse(
            "x".into()
        ))));
        assert!((cli.should_fallback)(&CautError::TimeoutWithProvider {
            provider: "kiro".into(),
            seconds: 10
        }));
        assert!((cli.should_fallback)(&cli_not_found()));
        assert!((plan.strategies[1].should_fallback)(&not_logged_in()));
    }

    #[tokio::test]
    async fn unknown_strategy_is_rejected() {
        let err = fetch("kiro.nope", &FetchContext::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Unknown strategy"));
    }

    #[test]
    fn error_mapping() {
        assert!(matches!(not_logged_in(), CautError::AuthInvalid { .. }));
        assert!(matches!(
            from_cli_error(CliError::Failed("boom".into())),
            CautError::FetchFailed { provider, reason } if provider == "kiro" && reason == "boom"
        ));
        assert!(matches!(
            from_cli_error(CliError::Parse("odd".into())),
            CautError::ParseResponse(msg) if msg.starts_with("kiro:") && msg.contains("odd")
        ));
        assert!(matches!(
            from_run_error(CautError::Timeout(10)),
            CautError::TimeoutWithProvider { provider, seconds: 10 } if provider == "kiro"
        ));
        assert!(matches!(
            from_run_error(CautError::ProviderNotFound("/x/kiro-cli".into())),
            CautError::CliNotFound { name } if name == "kiro-cli"
        ));
        assert!(matches!(
            from_run_error(CautError::FetchFailed {
                provider: "/x/kiro-cli".into(),
                reason: "io".into()
            }),
            CautError::FetchFailed { provider, .. } if provider == "kiro"
        ));
    }

    #[test]
    fn account_status_classifies_whoami() {
        assert_eq!(
            account_status(output(
                "Logged in with Google\nEmail: p@example.com\n",
                "",
                0
            )),
            AccountStatus::Account(KiroAccountInfo {
                auth_method: Some("Google".into()),
                email: Some("p@example.com".into()),
            })
        );
        assert_eq!(
            account_status(output("", "error: Not logged in", 1)),
            AccountStatus::NotLoggedIn
        );
        assert_eq!(
            account_status(output("", "", 0)),
            AccountStatus::Unavailable
        );
        assert_eq!(
            account_status(Err(CautError::Timeout(3))),
            AccountStatus::Unavailable
        );
    }

    #[test]
    fn usage_output_validation() {
        assert_eq!(
            usage_output(output("report\n", "warning: telemetry unavailable\n", 0)).unwrap(),
            "report\nwarning: telemetry unavailable"
        );
        assert!(matches!(
            usage_output(output("", "error: OAuth error: callback ports in use", 1)),
            Err(CautError::AuthInvalid { .. })
        ));
        // A failed command is a failure even when its output looks valid.
        assert!(matches!(
            usage_output(output("████ 25%\n(12.50 of 50 covered in plan)", "", 2)),
            Err(CautError::FetchFailed { reason, .. }) if reason.contains("covered in plan")
        ));
        assert!(matches!(
            usage_output(output("", "", 3)),
            Err(CautError::FetchFailed { reason, .. }) if reason.contains("status 3")
        ));
        assert!(matches!(
            usage_output(Err(CautError::Timeout(10))),
            Err(CautError::TimeoutWithProvider { .. })
        ));
    }

    #[test]
    fn report_from_limits_uses_subscription_title() {
        let json = include_str!("../../../tests/fixtures/kiro/get_usage_limits_overage.json");
        let limits = limits::parse_usage_limits(json).unwrap();
        let usage = report_from_limits(limits).to_usage_snapshot(Utc::now());
        assert!((usage.primary.unwrap().used_percent - 100.0).abs() < 1e-9);
        assert_eq!(usage.scoped.len(), 1);
        assert!(usage.provider_cost.is_some());
        assert!(usage.secondary.is_none());
        let identity = usage.identity.unwrap();
        assert_eq!(identity.login_method.as_deref(), Some("Kiro Power"));
        assert_eq!(identity.account_email, None);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        const USAGE_REPORT: &str = "printf 'Estimated Usage | resets on 2026-06-01 | KIRO FREE\\n'\n\
             printf 'Credits (12.50 of 50 covered in plan)\\n'\n\
             printf '████████████████████ 25%%\\n'\n";

        fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn fake_cli(dir: &Path, whoami: &str, usage: &str) -> PathBuf {
            write_script(
                dir,
                "kiro-cli",
                &format!(
                    "if [ \"$1\" = \"whoami\" ]; then\n{whoami}\nfi\n\
                     if [ \"$1\" = \"chat\" ] && [ \"$3\" = \"/usage\" ]; then\n{usage}\nfi\n\
                     exit 1\n"
                ),
            )
        }

        #[test]
        fn resolves_override_then_path_then_well_known() {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path();
            let override_bin = write_script(home, "custom-kiro", "exit 0\n");
            let not_executable = home.join("plain-file");
            std::fs::write(&not_executable, "x").unwrap();
            let path_hit = PathBuf::from("/somewhere/on/path/kiro-cli");

            assert_eq!(
                resolve_cli_binary_from(Some(&override_bin), Some(path_hit.clone()), Some(home)),
                Some(override_bin)
            );
            // A non-executable override is ignored.
            assert_eq!(
                resolve_cli_binary_from(Some(&not_executable), Some(path_hit.clone()), Some(home)),
                Some(path_hit)
            );
            std::fs::create_dir_all(home.join(".local/bin")).unwrap();
            let local_bin = write_script(&home.join(".local/bin"), "kiro-cli", "exit 0\n");
            assert_eq!(
                resolve_cli_binary_from(None, None, Some(home)),
                Some(local_bin)
            );
            let empty = tempfile::tempdir().unwrap();
            let found = resolve_cli_binary_from(None, None, Some(empty.path()));
            assert!(found.is_none_or(|p| !p.starts_with(empty.path())));
        }

        #[tokio::test]
        async fn probe_reports_usage_and_account() {
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                "printf 'Logged in with Google\\nEmail: person@example.com\\n'\nexit 0",
                &format!("{USAGE_REPORT}exit 0"),
            );
            let report = run_cli_probe(&cli).await.unwrap();
            assert_eq!(report.plan_name, "KIRO FREE");
            assert!((report.credits_used - 12.5).abs() < f64::EPSILON);
            assert!((report.credits_percent - 25.0).abs() < f64::EPSILON);
            assert_eq!(report.account_email.as_deref(), Some("person@example.com"));
            assert_eq!(report.auth_method.as_deref(), Some("Google"));
        }

        #[tokio::test]
        async fn probe_survives_a_failed_account_probe() {
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                "echo 'Connection error' >&2\nexit 1",
                &format!("{USAGE_REPORT}printf 'warning: telemetry unavailable\\n' >&2\nexit 0"),
            );
            let report = run_cli_probe(&cli).await.unwrap();
            assert_eq!(report.plan_name, "KIRO FREE");
            assert_eq!(report.account_email, None);
            assert_eq!(report.auth_method, None);
        }

        #[tokio::test]
        async fn logged_out_account_explains_a_failed_usage_command() {
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                "echo 'Not logged in'\nexit 1",
                "echo 'something went wrong' >&2\nexit 1",
            );
            assert!(matches!(
                run_cli_probe(&cli).await,
                Err(CautError::AuthInvalid { reason, .. }) if reason.contains("kiro-cli login")
            ));

            let unparseable = fake_cli(
                dir.path(),
                "echo 'Not logged in'\nexit 1",
                "echo 'Welcome!'\nexit 0",
            );
            assert!(matches!(
                run_cli_probe(&unparseable).await,
                Err(CautError::AuthInvalid { .. })
            ));
        }

        #[tokio::test]
        async fn failed_usage_command_is_reported_when_logged_in() {
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                "printf 'Logged in with Google\\n'\nexit 0",
                &format!("{USAGE_REPORT}exit 4"),
            );
            assert!(matches!(
                run_cli_probe(&cli).await,
                Err(CautError::FetchFailed { provider, .. }) if provider == "kiro"
            ));

            let garbled = fake_cli(
                dir.path(),
                "printf 'Logged in with Google\\n'\nexit 0",
                "echo 'Usage: unknown format'\nexit 0",
            );
            assert!(matches!(
                run_cli_probe(&garbled).await,
                Err(CautError::ParseResponse(_))
            ));
        }

        #[tokio::test]
        async fn login_prompt_in_usage_is_not_logged_in() {
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                "printf 'Logged in with Google\\n'\nexit 0",
                "echo 'Failed to initialize auth portal.' >&2\nexit 0",
            );
            assert!(matches!(
                run_cli_probe(&cli).await,
                Err(CautError::AuthInvalid { .. })
            ));
        }

        #[tokio::test]
        async fn missing_binary_is_cli_not_found() {
            let dir = tempfile::tempdir().unwrap();
            assert!(matches!(
                run_cli_probe(&dir.path().join("kiro-cli")).await,
                Err(CautError::CliNotFound { .. })
            ));
        }

        #[tokio::test]
        async fn api_reads_the_token_account_and_profile_from_disk() {
            let dir = tempfile::tempdir().unwrap();
            let db = limits::make_state_db(
                dir.path(),
                "arn:aws:codewhisperer:ap-southeast-1:123456789012:profile/test",
                None,
            );
            // An unsupported region is refused before any request is sent.
            assert!(matches!(
                limits::fetch_usage_limits(&db, Some("account-token")).await,
                Err(CautError::AuthInvalid { reason, .. }) if reason.contains("unsupported profile ARN")
            ));
        }
    }
}
