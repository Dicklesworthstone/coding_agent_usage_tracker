//! Reusable rich output components for caut.
//!
//! This module provides a library of styled components that wrap rich_rust
//! primitives with caut-specific functionality. All components implement
//! the `Renderable` trait for both rich and plain text output.
//!
//! ## Components
//!
//! - [`ProviderCard`] - Styled card displaying a provider's usage data
//! - [`UsageBar`] - Visual progress bar showing usage percentage
//! - [`UsageTable`] - Multi-provider comparison table with totals
//! - [`StatusBadge`] - Inline status indicators (success/warning/error)
//! - [`ErrorPanel`] - Error messages with suggestions
//! - [`ProgressIndicator`] - Multi-provider fetch progress
//! - [`Spinner`] - Indeterminate progress spinner
//!
//! ## Formatter Functions
//!
//! - [`format_token_count`] - Token count with units (5.7M)
//! - [`format_token_count_full`] - Token count with commas (5,678,901)
//! - [`format_cost`] - Currency formatting ($10.50)
//! - [`format_percentage`] - Percentage formatting (75%)
//! - [`format_duration_short`] - Duration formatting (2h 30m)

mod error_panel;
mod formatters;
mod progress_indicator;
mod provider_card;
mod status_badge;
mod usage_bar;
mod usage_table;

pub use error_panel::ErrorPanel;
pub use formatters::*;
pub use progress_indicator::{ProgressIndicator, Spinner};
pub use provider_card::ProviderCard;
pub use status_badge::{StatusBadge, StatusLevel};
pub use usage_bar::UsageBar;
pub use usage_table::UsageTable;
