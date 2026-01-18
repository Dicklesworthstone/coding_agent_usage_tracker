//! Provider-specific fetchers.
//!
//! Each provider has its own submodule implementing fetch strategies.

pub mod claude;
pub mod codex;

// Re-export common types
pub use crate::core::fetch_plan::{FetchKind, FetchOutcome, FetchPlan, FetchStrategy, SourceMode};
pub use crate::core::provider::Provider;
