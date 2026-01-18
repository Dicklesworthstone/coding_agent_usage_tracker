//! Usage command implementation.

use crate::cli::args::{OutputFormat, UsageArgs};
use crate::cli::watch;
use crate::core::models::{ProviderPayload, RobotOutput};
use crate::core::pipeline::fetch_providers_with_timeout;
use crate::core::provider::ProviderSelection;
use crate::core::status::StatusFetcher;
use crate::error::{CautError, Result};
use crate::render::{human, robot};
use tokio::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct UsageResults {
    pub payloads: Vec<ProviderPayload>,
    pub errors: Vec<String>,
}

/// Execute the usage command.
pub async fn execute(
    args: &UsageArgs,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    // Validate arguments
    args.validate()?;

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
    let timeout_secs = args.web_timeout.unwrap_or(30);
    let outcomes =
        fetch_providers_with_timeout(&providers, source_mode, Duration::from_secs(timeout_secs))
            .await;

    // Optionally fetch status
    let status_fetcher = if args.status {
        Some(StatusFetcher::new())
    } else {
        None
    };

    // Build payloads
    let mut payloads = Vec::new();
    let mut errors = Vec::new();

    for outcome in outcomes {
        match outcome.result {
            Ok(snapshot) => {
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
                };
                payloads.push(payload);
            }
            Err(e) => {
                errors.push(format!("{}: {}", outcome.provider.cli_name(), e));
            }
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
            println!("{}", output);

            for error in &results.errors {
                eprintln!("Error: {}", error);
            }
        }
        OutputFormat::Json => {
            let robot_output = RobotOutput::usage(results.payloads.clone(), results.errors.clone());
            let output = if pretty {
                robot::render_json_pretty(&robot_output)?
            } else {
                robot::render_json(&robot_output)?
            };
            println!("{}", output);
        }
        OutputFormat::Md => {
            let output = robot::render_markdown_usage(&results.payloads)?;
            println!("{}", output);

            if !results.errors.is_empty() {
                println!("\n## Errors\n");
                for error in &results.errors {
                    println!("- {}", error);
                }
            }
        }
    }

    Ok(())
}
