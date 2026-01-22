//! Core data models and provider infrastructure.

pub mod cli_runner;
pub mod cost_scanner;
pub mod credential_health;
pub mod doctor;
pub mod fetch_plan;
pub mod http;
pub mod logging;
pub mod models;
pub mod pipeline;
pub mod prediction;
pub mod provider;
pub mod status;

pub use cost_scanner::CostScanner;
pub use credential_health::{
    check_oauth_file, check_oauth_json, get_reauth_instructions, AuthHealthAggregator,
    CredentialHealth, CredentialHealthReport, CredentialType, HealthSeverity, JwtHealth,
    JwtHealthChecker, OAuthHealth, OverallHealth, ProviderAuthHealth, SourceHealth,
};
pub use doctor::{CheckStatus, DiagnosticCheck, DoctorReport, ProviderHealth};
pub use fetch_plan::{FetchAttempt, FetchOutcome, FetchStrategy};
pub use models::{
    CostDailyEntry, CostPayload, CostTotals, CreditEvent, CreditsSnapshot, OpenAIDashboardSnapshot,
    ProviderIdentity, ProviderPayload, RateWindow, RobotOutput, StatusIndicator, StatusPayload,
    UsageSnapshot,
};
pub use prediction::{calculate_velocity, detect_reset, smoothed_velocity};
pub use provider::{Provider, ProviderDescriptor, ProviderRegistry, ProviderSelection};
pub use status::StatusFetcher;
