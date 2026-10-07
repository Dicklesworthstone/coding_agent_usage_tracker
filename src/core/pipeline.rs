//! Provider fetch pipeline executor.
//!
//! Orchestrates the execution of fetch strategies for providers.

use std::time::Instant;

use chrono::Utc;
use tokio::time::{Duration, timeout};

use super::fetch_plan::{FetchAttempt, FetchContext, FetchOutcome, ProviderFetch, SourceMode};
use super::provider::Provider;
use crate::error::CautError;
use crate::providers;

/// One unit of fetch work: a provider plus the context to fetch it with.
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub provider: Provider,
    pub ctx: FetchContext,
    /// Configured timeout; `None` uses the provider's default.
    pub timeout: Option<Duration>,
}

impl FetchRequest {
    /// A request with an empty context (local credential discovery only).
    #[must_use]
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            ctx: FetchContext::default(),
            timeout: None,
        }
    }
}

/// Execute the fetch pipeline for a provider with an empty context.
pub async fn fetch_provider(provider: Provider, mode: SourceMode) -> FetchOutcome {
    fetch_provider_with_context(provider, mode, &FetchContext::default()).await
}

/// Execute the fetch pipeline for a provider.
///
/// Tries strategies in order until one succeeds or all fail.
pub async fn fetch_provider_with_context(
    provider: Provider,
    mode: SourceMode,
    ctx: &FetchContext,
) -> FetchOutcome {
    let plan = providers::fetch_plan(provider);
    let strategies = plan.for_mode(mode);

    if strategies.is_empty() {
        let error = if plan.strategies.is_empty() {
            CautError::NoAvailableStrategy(provider.cli_name().to_string())
        } else {
            CautError::UnsupportedSource {
                provider: provider.cli_name().to_string(),
                source_type: format!("{mode:?}").to_lowercase(),
            }
        };
        return FetchOutcome::failure(provider, error, vec![]);
    }

    let mut attempts = Vec::new();
    let mut last_error: Option<CautError> = None;

    for strategy in strategies {
        // Check availability
        if !(strategy.is_available)(ctx) {
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
        let result = providers::execute(provider, strategy.id, ctx).await;
        #[allow(clippy::cast_possible_truncation)] // fetch durations won't exceed u64::MAX ms
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
            Ok(ProviderFetch {
                usage: snapshot,
                credits,
            }) => {
                // Differentiate "fetched rate-limit data" from "fetched identity
                // only" so users understand why `primary`/`secondary` may be null
                // on platforms where the CLI doesn't expose quota (see #7).
                if snapshot.has_quota() {
                    tracing::info!(
                        provider = %provider.cli_name(),
                        strategy = strategy.id,
                        duration_ms,
                        "Fetch succeeded (rate-limit data populated)"
                    );
                } else {
                    tracing::info!(
                        provider = %provider.cli_name(),
                        strategy = strategy.id,
                        duration_ms,
                        "Fetch completed (identity only — no rate-limit data available from this strategy)"
                    );
                }
                return FetchOutcome::success(
                    provider,
                    snapshot,
                    strategy.kind.source_label(),
                    attempts,
                )
                .with_credits(credits);
            }
            Err(e) => {
                tracing::warn!(
                    provider = %provider.cli_name(),
                    strategy = strategy.id,
                    error = %e,
                    "Fetch failed"
                );

                // Check if we should fallback
                if !(strategy.should_fallback)(&e) {
                    tracing::debug!(
                        provider = %provider.cli_name(),
                        strategy = strategy.id,
                        "Strategy does not allow fallback, stopping"
                    );
                    return FetchOutcome::failure(provider, e, attempts);
                }
                last_error = Some(e);
            }
        }
    }

    // Every available strategy failed: the last real error says more than
    // "no strategy" does. Nothing was even available otherwise.
    let error = last_error
        .unwrap_or_else(|| CautError::NoAvailableStrategy(provider.cli_name().to_string()));
    FetchOutcome::failure(provider, error, attempts)
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
    let requests: Vec<FetchRequest> = providers.iter().map(|&p| FetchRequest::new(p)).collect();
    fetch_requests(&requests, mode, timeout_override).await
}

/// Run several fetch requests in parallel, each under its own timeout:
/// `timeout_override` (the CLI flag), else the request's configured
/// timeout, else the provider default.
pub async fn fetch_requests(
    requests: &[FetchRequest],
    mode: SourceMode,
    timeout_override: Option<Duration>,
) -> Vec<FetchOutcome> {
    let futures: Vec<_> = requests
        .iter()
        .map(|request| {
            let timeout = timeout_override
                .or(request.timeout)
                .unwrap_or_else(|| request.provider.default_timeout());
            fetch_provider_with_timeout(request.provider, mode, &request.ctx, timeout)
        })
        .collect();

    futures::future::join_all(futures).await
}

async fn fetch_provider_with_timeout(
    provider: Provider,
    mode: SourceMode,
    ctx: &FetchContext,
    timeout_duration: Duration,
) -> FetchOutcome {
    timeout(
        timeout_duration,
        fetch_provider_with_context(provider, mode, ctx),
    )
    .await
    .unwrap_or_else(|_| {
        FetchOutcome::failure(
            provider,
            CautError::TimeoutWithProvider {
                provider: provider.cli_name().to_string(),
                seconds: timeout_duration.as_secs(),
            },
            Vec::new(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_fetch_plan_codex() {
        let plan = providers::fetch_plan(Provider::Codex);
        assert_eq!(plan.provider, Provider::Codex);
        assert!(!plan.strategies.is_empty());
    }

    #[test]
    fn test_get_fetch_plan_claude() {
        let plan = providers::fetch_plan(Provider::Claude);
        assert_eq!(plan.provider, Provider::Claude);
        assert!(!plan.strategies.is_empty());
    }

    #[tokio::test]
    async fn source_mode_without_matching_strategy_is_unsupported() {
        // Claude has no API-token strategy.
        let outcome = fetch_provider(Provider::Claude, SourceMode::Api).await;
        assert!(matches!(
            outcome.result,
            Err(CautError::UnsupportedSource { .. })
        ));
        assert!(outcome.attempts.is_empty());
    }

    #[test]
    fn fetch_request_new_has_empty_context() {
        let request = FetchRequest::new(Provider::Zai);
        assert_eq!(request.provider, Provider::Zai);
        assert!(request.ctx.account_token().is_none());
    }
}
