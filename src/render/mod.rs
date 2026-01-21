//! Output rendering for human and robot modes.

pub mod doctor;
pub mod error;
pub mod human;
pub mod robot;

use crate::cli::args::OutputFormat;
use crate::core::doctor::DoctorReport;
use crate::core::models::{CostPayload, ProviderPayload};
use crate::error::Result;
pub use human::{HistoryDay, HistoryRenderOptions, render_history_chart};

/// Render usage results.
pub fn render_usage(
    results: &[ProviderPayload],
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<String> {
    match format {
        OutputFormat::Human => human::render_usage(results, no_color),
        OutputFormat::Json => robot::render_usage_json(results, pretty),
        OutputFormat::Md => robot::render_usage_md(results),
    }
}

/// Render cost results.
pub fn render_cost(
    results: &[CostPayload],
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<String> {
    match format {
        OutputFormat::Human => human::render_cost(results, no_color),
        OutputFormat::Json => robot::render_cost_json(results, pretty),
        OutputFormat::Md => robot::render_cost_md(results),
    }
}

/// Render doctor report.
pub fn render_doctor(
    report: &DoctorReport,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<String> {
    match format {
        OutputFormat::Human => doctor::render_human(report, no_color),
        OutputFormat::Json => doctor::render_json(report, pretty),
        OutputFormat::Md => doctor::render_md(report),
    }
}
