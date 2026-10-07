//! Provider-specific fetchers.
//!
//! Each provider has its own submodule exposing two entry points:
//!
//! - `fetch_plan()` — the ordered [`FetchStrategy`] list the pipeline walks.
//! - `fetch(strategy_id, ctx)` — runs one strategy from that list.
//!
//! [`fetch_plan`] and [`execute`] dispatch on [`Provider`] so the pipeline
//! never needs to know about individual providers.

pub mod amp;
pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod common;
pub mod copilot;
pub mod cursor;
pub mod factory;
pub mod gemini;
pub mod jetbrains;
pub mod kimi;
pub mod kimik2;
pub mod kiro;
pub mod minimax;
pub mod opencode;
pub mod vertexai;
pub mod zai;

// Re-export common types
pub use crate::core::fetch_plan::{
    FetchContext, FetchKind, FetchOutcome, FetchPlan, FetchStrategy, ProviderFetch, SourceMode,
};
pub use crate::core::provider::Provider;

use crate::core::models::UsageSnapshot;
use crate::error::Result;

/// The ordered fetch strategies for a provider.
#[must_use]
pub fn fetch_plan(provider: Provider) -> FetchPlan {
    match provider {
        Provider::Codex => codex::fetch_plan(),
        Provider::Claude => claude::fetch_plan(),
        Provider::Gemini => gemini::fetch_plan(),
        Provider::Antigravity => antigravity::fetch_plan(),
        Provider::Cursor => cursor::fetch_plan(),
        Provider::OpenCode => opencode::fetch_plan(),
        Provider::Factory => factory::fetch_plan(),
        Provider::Zai => zai::fetch_plan(),
        Provider::MiniMax => minimax::fetch_plan(),
        Provider::Kimi => kimi::fetch_plan(),
        Provider::Copilot => copilot::fetch_plan(),
        Provider::KimiK2 => kimik2::fetch_plan(),
        Provider::Kiro => kiro::fetch_plan(),
        Provider::VertexAI => vertexai::fetch_plan(),
        Provider::JetBrainsAI => jetbrains::fetch_plan(),
        Provider::Amp => amp::fetch_plan(),
    }
}

/// Run one strategy from a provider's fetch plan.
///
/// # Errors
/// Returns the strategy's fetch error, or an error for an unknown strategy id.
pub async fn execute(
    provider: Provider,
    strategy_id: &str,
    ctx: &FetchContext,
) -> Result<ProviderFetch> {
    match provider {
        // Codex also reports a credit balance alongside its windows.
        Provider::Codex => codex::fetch(strategy_id, ctx).await,
        _ => execute_usage(provider, strategy_id, ctx)
            .await
            .map(ProviderFetch::from),
    }
}

/// Run a strategy for a provider whose fetchers return only a usage snapshot.
async fn execute_usage(
    provider: Provider,
    strategy_id: &str,
    ctx: &FetchContext,
) -> Result<UsageSnapshot> {
    match provider {
        Provider::Codex => codex::fetch(strategy_id, ctx).await.map(|f| f.usage),
        Provider::Claude => claude::fetch(strategy_id, ctx).await,
        Provider::Gemini => gemini::fetch(strategy_id, ctx).await,
        Provider::Antigravity => antigravity::fetch(strategy_id, ctx).await,
        Provider::Cursor => cursor::fetch(strategy_id, ctx).await,
        Provider::OpenCode => opencode::fetch(strategy_id, ctx).await,
        Provider::Factory => factory::fetch(strategy_id, ctx).await,
        Provider::Zai => zai::fetch(strategy_id, ctx).await,
        Provider::MiniMax => minimax::fetch(strategy_id, ctx).await,
        Provider::Kimi => kimi::fetch(strategy_id, ctx).await,
        Provider::Copilot => copilot::fetch(strategy_id, ctx).await,
        Provider::KimiK2 => kimik2::fetch(strategy_id, ctx).await,
        Provider::Kiro => kiro::fetch(strategy_id, ctx).await,
        Provider::VertexAI => vertexai::fetch(strategy_id, ctx).await,
        Provider::JetBrainsAI => jetbrains::fetch(strategy_id, ctx).await,
        Provider::Amp => amp::fetch(strategy_id, ctx).await,
    }
}

/// Error for a strategy id the provider does not define.
#[must_use]
pub fn unknown_strategy(provider: Provider, strategy_id: &str) -> crate::error::CautError {
    common::fetch_failed(provider, format!("Unknown strategy: {strategy_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plan_belongs_to_its_provider_with_unique_ids() {
        for &provider in Provider::ALL {
            let plan = fetch_plan(provider);
            assert_eq!(plan.provider, provider);
            let mut ids: Vec<_> = plan.strategies.iter().map(|s| s.id).collect();
            let total = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), total, "duplicate strategy id for {provider:?}");
        }
    }

    #[tokio::test]
    async fn unknown_strategy_is_an_error_for_every_provider() {
        let ctx = FetchContext::default();
        for &provider in Provider::ALL {
            assert!(
                execute(provider, "no-such-strategy", &ctx).await.is_err(),
                "{provider:?} accepted an unknown strategy"
            );
        }
    }
}
