//! Provider fetch pipeline executor.
//!
//! Orchestrates the execution of fetch strategies for providers.

use std::time::Instant;

use chrono::Utc;
use tokio::time::{Duration, timeout};

use super::fetch_plan::{FetchAttempt, FetchKind, FetchOutcome, SourceMode};
use super::models::UsageSnapshot;
use super::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::{claude, codex};

/// Execute the fetch pipeline for a provider.
///
/// Tries strategies in order until one succeeds or all fail.
pub async fn fetch_provider(provider: Provider, mode: SourceMode) -> FetchOutcome {
    let plan = get_fetch_plan(provider);
    let strategies = plan.for_mode(mode);

    if strategies.is_empty() {
        return FetchOutcome::failure(
            provider,
            CautError::UnsupportedSource {
                provider: provider.cli_name().to_string(),
                source_type: format!("{:?}", mode),
            },
            vec![],
        );
    }

    let mut attempts = Vec::new();

    for strategy in strategies {
        // Check availability
        if !(strategy.is_available)() {
            tracing::debug!(
                provider = %provider.cli_name(),
                strategy = strategy.id,
                "Strategy not available, skipping"
            );
            continue;
        }

        tracing::info!(
            provider = %provider.cli_name(),
            strategy = strategy.id,
            "Trying fetch strategy"
        );

        let started_at = Utc::now();
        let start = Instant::now();

        // Execute the fetch
        let result = execute_strategy(provider, strategy.id, strategy.kind).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let attempt = FetchAttempt {
            strategy_id: strategy.id.to_string(),
            kind: strategy.kind,
            started_at,
            duration_ms,
            success: result.is_ok(),
            error: result.as_ref().err().map(std::string::ToString::to_string),
        };
        attempts.push(attempt);

        match result {
            Ok(snapshot) => {
                tracing::info!(
                    provider = %provider.cli_name(),
                    strategy = strategy.id,
                    duration_ms,
                    "Fetch succeeded"
                );
                return FetchOutcome::success(
                    provider,
                    snapshot,
                    strategy.kind.source_label(),
                    attempts,
                );
            }
            Err(ref e) => {
                tracing::warn!(
                    provider = %provider.cli_name(),
                    strategy = strategy.id,
                    error = %e,
                    "Fetch failed"
                );

                // Check if we should fallback
                if !(strategy.should_fallback)(e) {
                    tracing::debug!(
                        provider = %provider.cli_name(),
                        strategy = strategy.id,
                        "Strategy does not allow fallback, stopping"
                    );
                    return FetchOutcome::failure(provider, result.unwrap_err(), attempts);
                }
            }
        }
    }

    // All strategies failed
    FetchOutcome::failure(
        provider,
        CautError::NoAvailableStrategy(provider.cli_name().to_string()),
        attempts,
    )
}

/// Get the fetch plan for a provider.
fn get_fetch_plan(provider: Provider) -> super::fetch_plan::FetchPlan {
    match provider {
        Provider::Codex => codex::fetch_plan(),
        Provider::Claude => claude::fetch_plan(),
        // Add other providers as they're implemented
        _ => super::fetch_plan::FetchPlan::new(provider, vec![]),
    }
}

/// Execute a specific fetch strategy.
async fn execute_strategy(
    provider: Provider,
    strategy_id: &str,
    _kind: FetchKind,
) -> Result<UsageSnapshot> {
    match (provider, strategy_id) {
        // Codex strategies
        (Provider::Codex, "codex-web-dashboard") => codex::fetch_web_dashboard().await,
        (Provider::Codex, "codex-cli-rpc") => codex::fetch_cli().await,

        // Claude strategies
        (Provider::Claude, "claude-oauth") => {
            // Get token from keyring
            let entry = keyring::Entry::new("caut", "claude-oauth-token")
                .map_err(|e| CautError::Config(format!("Keyring error: {}", e)))?;
            let token = entry
                .get_password()
                .map_err(|_| CautError::Config("No OAuth token found".to_string()))?;
            claude::fetch_oauth(&token).await
        }
        (Provider::Claude, "claude-web") => claude::fetch_web().await,
        (Provider::Claude, "claude-cli-pty") => claude::fetch_cli().await,

        // Unknown strategy
        _ => Err(CautError::FetchFailed {
            provider: provider.cli_name().to_string(),
            reason: format!("Unknown strategy: {}", strategy_id),
        }),
    }
}

/// Fetch multiple providers in parallel.
pub async fn fetch_providers(providers: &[Provider], mode: SourceMode) -> Vec<FetchOutcome> {
    fetch_providers_with_timeout(providers, mode, None).await
}

/// Fetch multiple providers in parallel with a per-provider timeout.
pub async fn fetch_providers_with_timeout(
    providers: &[Provider],
    mode: SourceMode,
    timeout_override: Option<Duration>,
) -> Vec<FetchOutcome> {
    let futures: Vec<_> = providers
        .iter()
        .map(|&p| {
            let timeout = timeout_override.unwrap_or_else(|| p.default_timeout());
            fetch_provider_with_timeout(p, mode, timeout)
        })
        .collect();

    futures::future::join_all(futures).await
}

async fn fetch_provider_with_timeout(
    provider: Provider,
    mode: SourceMode,
    timeout_duration: Duration,
) -> FetchOutcome {
    match timeout(timeout_duration, fetch_provider(provider, mode)).await {
        Ok(outcome) => outcome,
        Err(_) => FetchOutcome::failure(
            provider,
            CautError::TimeoutWithProvider {
                provider: provider.cli_name().to_string(),
                seconds: timeout_duration.as_secs(),
            },
            Vec::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_fetch_plan_codex() {
        let plan = get_fetch_plan(Provider::Codex);
        assert_eq!(plan.provider, Provider::Codex);
        assert!(!plan.strategies.is_empty());
    }

    #[test]
    fn test_get_fetch_plan_claude() {
        let plan = get_fetch_plan(Provider::Claude);
        assert_eq!(plan.provider, Provider::Claude);
        assert!(!plan.strategies.is_empty());
    }
}
