//! Core data models and provider infrastructure.

pub mod cli_runner;
pub mod cost_scanner;
pub mod doctor;
pub mod fetch_plan;
pub mod http;
pub mod logging;
pub mod models;
pub mod pipeline;
pub mod provider;
pub mod status;

pub use cost_scanner::CostScanner;
pub use doctor::{CheckStatus, DiagnosticCheck, DoctorReport, ProviderHealth};
pub use fetch_plan::{FetchAttempt, FetchOutcome, FetchStrategy};
pub use models::{
    CostDailyEntry, CostPayload, CostTotals, CreditEvent, CreditsSnapshot, OpenAIDashboardSnapshot,
    ProviderIdentity, ProviderPayload, RateWindow, RobotOutput, StatusIndicator, StatusPayload,
    UsageSnapshot,
};
pub use provider::{Provider, ProviderDescriptor, ProviderRegistry, ProviderSelection};
pub use status::StatusFetcher;
