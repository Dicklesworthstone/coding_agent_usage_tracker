//! Usage command implementation.

use crate::cli::args::{OutputFormat, UsageArgs};
use crate::cli::prompt::{ProviderPromptData, update_cache as update_prompt_cache};
use crate::cli::watch;
use crate::core::credential_health::AuthHealthAggregator;
use crate::core::models::{ProviderPayload, RobotOutput};
use crate::core::pipeline::fetch_providers_with_timeout;
use crate::core::provider::ProviderSelection;
use crate::core::status::StatusFetcher;
use crate::error::{CautError, Result};
use crate::render::{human, robot};
use crate::storage::{AppPaths, HistoryStore, RetentionPolicy};
use tokio::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct UsageResults {
    pub payloads: Vec<ProviderPayload>,
    pub errors: Vec<String>,
}

/// Execute the usage command.
///
/// # Errors
/// Returns an error if argument validation fails, provider fetching fails,
/// or output rendering encounters an error.
pub async fn execute(
    args: &UsageArgs,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    // Validate arguments
    args.validate()?;

    // Prune history on startup
    let paths = AppPaths::new();
    if let Ok(store) = HistoryStore::open(&paths.history_db_file())
        && let Err(e) = store.maybe_prune(&RetentionPolicy::default())
    {
        tracing::warn!("Failed to prune history: {}", e);
    }

    // TUI mode implies watch mode
    if args.tui {
        let interval = args.interval;
        if interval == 0 {
            return Err(CautError::Config(
                "Watch interval must be greater than 0 seconds".to_string(),
            ));
        }
        return crate::tui::run_dashboard(args, interval).await;
    }

    if args.watch {
        let interval = Duration::from_secs(args.interval);
        if interval.is_zero() {
            return Err(CautError::Config(
                "Watch interval must be greater than 0 seconds".to_string(),
            ));
        }
        return watch::run_watch(args, format, pretty, no_color, interval).await;
    }

    let results = fetch_usage(args).await?;
    render_usage_results(&results, format, pretty, no_color)?;

    if !results.errors.is_empty() {
        return Err(CautError::PartialFailure {
            failed: results.errors.len(),
        });
    }

    Ok(())
}

pub(crate) async fn fetch_usage(args: &UsageArgs) -> Result<UsageResults> {
    // Parse provider selection
    let selection = args
        .provider
        .as_deref()
        .map(ProviderSelection::from_arg)
        .transpose()?
        .unwrap_or_default();

    let providers = selection.providers();
    let source_mode = args.effective_source();

    tracing::debug!(?providers, ?source_mode, "Starting usage fetch");

    // Fetch usage data from providers
    let timeout_override = args.effective_timeout_override().map(Duration::from_secs);
    let outcomes = fetch_providers_with_timeout(&providers, source_mode, timeout_override).await;

    // Optionally fetch status
    let status_fetcher = if args.status {
        Some(StatusFetcher::new())
    } else {
        None
    };

    // Build payloads
    let mut payloads = Vec::new();
    let mut errors = Vec::new();

    let paths = AppPaths::new();
    let auth_checker = AuthHealthAggregator::new();

    for outcome in outcomes {
        match outcome.result {
            Ok(snapshot) => {
                // Record to history
                if let Ok(store) = HistoryStore::open(&paths.history_db_file())
                    && let Err(e) = store.record_snapshot(&snapshot, &outcome.provider)
                {
                    tracing::warn!("Failed to record snapshot: {}", e);
                }

                // Get status if requested
                let status = if let Some(ref fetcher) = status_fetcher {
                    if let Some(status_url) = outcome.provider.status_page_url() {
                        fetcher.fetch(status_url).await.ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Check auth health for this provider
                let auth_health = auth_checker.check_provider(outcome.provider);
                let auth_warning = auth_health.warning_message();

                let payload = ProviderPayload {
                    provider: outcome.provider.cli_name().to_string(),
                    account: snapshot
                        .identity
                        .as_ref()
                        .and_then(|i| i.account_email.clone()),
                    version: None, // TODO: Get from CLI version
                    source: outcome.source_label,
                    status,
                    usage: snapshot,
                    credits: None, // TODO: Fetch credits
                    antigravity_plan_info: None,
                    openai_dashboard: None,
                    auth_warning,
                };
                payloads.push(payload);
            }
            Err(e) => {
                errors.push(format!("{}: {}", outcome.provider.cli_name(), e));
            }
        }
    }

    // Update prompt cache with successful results
    if !payloads.is_empty() {
        let prompt_data: Vec<ProviderPromptData> = payloads
            .iter()
            .map(|p| ProviderPromptData {
                provider: p.provider.clone(),
                primary_pct: p.usage.primary.as_ref().map(|w| w.used_percent),
                secondary_pct: p.usage.secondary.as_ref().map(|w| w.used_percent),
                credits_remaining: p.credits.as_ref().map(|c| c.remaining),
                cost_today_usd: None, // TODO: Extract cost from payload if available
            })
            .collect();

        if let Err(e) = update_prompt_cache(&prompt_data) {
            tracing::warn!("Failed to update prompt cache: {}", e);
        }
    }

    Ok(UsageResults { payloads, errors })
}

pub(crate) fn render_usage_results(
    results: &UsageResults,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    match format {
        OutputFormat::Human => {
            let output = human::render_usage(&results.payloads, no_color)?;
            println!("{output}");

            for error in &results.errors {
                eprintln!("Error: {error}");
            }
        }
        OutputFormat::Json => {
            let robot_output = RobotOutput::usage(results.payloads.clone(), results.errors.clone());
            let output = if pretty {
                robot::render_json_pretty(&robot_output)?
            } else {
                robot::render_json(&robot_output)?
            };
            println!("{output}");
        }
        OutputFormat::Md => {
            let output = robot::render_markdown_usage(&results.payloads)?;
            println!("{output}");

            if !results.errors.is_empty() {
                println!("\n## Errors\n");
                for error in &results.errors {
                    println!("- {error}");
                }
            }
        }
    }

    Ok(())
}
