//! Usage command implementation.

use crate::cli::args::{OutputFormat, UsageArgs};
use crate::cli::prompt::{ProviderPromptData, update_cache as update_prompt_cache};
use crate::cli::watch;
use crate::core::credential_health::AuthHealthAggregator;
use crate::core::fetch_plan::FetchContext;
use crate::core::models::{ProviderPayload, RobotOutput};
use crate::core::pipeline::{FetchRequest, fetch_requests};
use crate::core::provider::{Provider, ProviderSelection};
use crate::core::status::StatusFetcher;
use crate::error::{CautError, Result};
use crate::render::{human, robot};
use crate::storage::token_accounts::{AccountSelection, TokenAccountStore};
use crate::storage::{AppPaths, HistoryStore, RetentionPolicy};
use tokio::time::Duration;

/// Turn providers plus an account selection into fetch requests.
///
/// Without a selection every provider is fetched once with its locally
/// discovered credentials. A selection (`--account`, `--account-index`,
/// `--all-accounts`) needs exactly one provider that supports token accounts
/// and yields one request per chosen account, as in `CodexBar`.
///
/// # Errors
/// Returns an error for a selection over several providers, a provider
/// without token-account support, or a selection that matches nothing.
fn build_requests(
    providers: &[Provider],
    selection: &AccountSelection,
    load_store: impl FnOnce() -> Result<TokenAccountStore>,
) -> Result<Vec<FetchRequest>> {
    if !selection.is_explicit() {
        return Ok(providers.iter().map(|&p| FetchRequest::new(p)).collect());
    }
    let [provider] = providers else {
        return Err(CautError::Config(
            "Account selection (--account, --account-index, --all-accounts) needs a single provider: add --provider <name>".to_string(),
        ));
    };
    if !provider.supports_token_accounts() {
        return Err(CautError::Config(format!(
            "{} does not support token accounts",
            provider.display_name()
        )));
    }
    let store = load_store()?;
    let accounts = store.resolve(*provider, selection)?;
    Ok(accounts
        .into_iter()
        .map(|account| FetchRequest {
            provider: *provider,
            ctx: FetchContext::with_account(account.label.clone(), account.token.clone()),
        })
        .collect())
}

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

    let account_selection = AccountSelection {
        label: args.account.clone(),
        index: args.account_index.map(|i| i.saturating_sub(1)),
        all: args.all_accounts,
    };
    let requests = build_requests(&providers, &account_selection, || {
        TokenAccountStore::load(&AppPaths::new().token_accounts_file())
    })?;

    // Fetch usage data from providers
    let timeout_override = args.effective_timeout_override().map(Duration::from_secs);
    let outcomes = fetch_requests(&requests, source_mode, timeout_override).await;

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

    for (request, outcome) in requests.iter().zip(outcomes) {
        let account_label = request.ctx.account_label.clone();
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
                    // A selected token account is named by its label, so
                    // `--all-accounts` rows stay distinguishable.
                    account: account_label.or_else(|| {
                        snapshot
                            .identity
                            .as_ref()
                            .and_then(|i| i.account_email.clone())
                    }),
                    version: None, // TODO: Get from CLI version
                    source: outcome.source_label,
                    status,
                    usage: snapshot,
                    credits: outcome.credits,
                    antigravity_plan_info: None,
                    openai_dashboard: None,
                    auth_warning,
                };
                payloads.push(payload);
            }
            Err(e) => {
                let name = account_label.map_or_else(
                    || outcome.provider.cli_name().to_string(),
                    |label| format!("{} [{label}]", outcome.provider.cli_name()),
                );
                errors.push(format!("{name}: {e}"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::token_accounts::TokenAccount;

    fn store_with_zai_accounts() -> Result<TokenAccountStore> {
        let mut store = TokenAccountStore::empty();
        store.add(Provider::Zai, TokenAccount::new("personal", "tok-1"))?;
        store.add(Provider::Zai, TokenAccount::new("team", "tok-2"))?;
        Ok(store)
    }

    #[test]
    fn no_selection_fetches_each_provider_once_without_loading_accounts() {
        let requests = build_requests(
            &[Provider::Codex, Provider::Claude],
            &AccountSelection::default(),
            || panic!("the store must not be loaded without a selection"),
        )
        .unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|r| r.ctx.account_token().is_none()));
    }

    #[test]
    fn all_accounts_yields_one_request_per_account() {
        let selection = AccountSelection {
            all: true,
            ..Default::default()
        };
        let requests =
            build_requests(&[Provider::Zai], &selection, store_with_zai_accounts).unwrap();
        let labels: Vec<_> = requests
            .iter()
            .map(|r| r.ctx.account_label.as_deref().unwrap())
            .collect();
        assert_eq!(labels, ["personal", "team"]);
        assert_eq!(requests[1].ctx.account_token(), Some("tok-2"));
    }

    #[test]
    fn label_and_index_select_one_account() {
        let by_label = AccountSelection {
            label: Some("TEAM".into()),
            ..Default::default()
        };
        let requests =
            build_requests(&[Provider::Zai], &by_label, store_with_zai_accounts).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].ctx.account_token(), Some("tok-2"));

        let by_index = AccountSelection {
            index: Some(0),
            ..Default::default()
        };
        let requests =
            build_requests(&[Provider::Zai], &by_index, store_with_zai_accounts).unwrap();
        assert_eq!(requests[0].ctx.account_token(), Some("tok-1"));
    }

    #[test]
    fn selection_needs_one_supporting_provider() {
        let selection = AccountSelection {
            all: true,
            ..Default::default()
        };
        let err = build_requests(
            &[Provider::Codex, Provider::Claude],
            &selection,
            store_with_zai_accounts,
        )
        .unwrap_err();
        assert!(err.to_string().contains("single provider"));

        let err = build_requests(
            &[Provider::JetBrainsAI],
            &selection,
            store_with_zai_accounts,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not support token accounts"));
    }

    #[test]
    fn selection_errors_propagate_from_the_store() {
        let selection = AccountSelection {
            label: Some("missing".into()),
            ..Default::default()
        };
        assert!(build_requests(&[Provider::Zai], &selection, store_with_zai_accounts).is_err());
        let selection = AccountSelection {
            all: true,
            ..Default::default()
        };
        assert!(
            build_requests(&[Provider::Zai], &selection, || Ok(
                TokenAccountStore::empty()
            ))
            .is_err()
        );
    }
}
