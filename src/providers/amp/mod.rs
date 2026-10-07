//! Amp provider.
//!
//! Not yet ported: the fetch plan is empty, so the pipeline reports that no
//! strategy is available.

use crate::core::fetch_plan::{FetchContext, FetchPlan};
use crate::core::models::UsageSnapshot;
use crate::core::provider::Provider;
use crate::error::Result;

/// Create the fetch plan for Amp.
#[must_use]
pub const fn fetch_plan() -> FetchPlan {
    FetchPlan::new(Provider::Amp, vec![])
}

/// Run one strategy from [`fetch_plan`].
///
/// # Errors
/// Returns an error for any strategy id, since none are defined yet.
pub async fn fetch(strategy_id: &str, _ctx: &FetchContext) -> Result<UsageSnapshot> {
    Err(super::unknown_strategy(Provider::Amp, strategy_id))
}
