//! Core data models and provider infrastructure.

pub mod budgets;
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
pub mod pricing;
pub mod provider;
pub mod session_logs;
pub mod status;

pub use budgets::{
    BudgetConfig, BudgetFileConfig, BudgetLimits, BudgetPriority, BudgetSources, BudgetViolation,
    CurrentUsage, ProviderBudgetConfig, ResolvedBudget, ViolationType, check_budget_violations,
    resolve_budget,
};
pub use cost_scanner::CostScanner;
pub use credential_health::{
    AuthHealthAggregator, CredentialHealth, CredentialHealthReport, CredentialType, HealthSeverity,
    JwtHealth, JwtHealthChecker, OAuthHealth, OverallHealth, ProviderAuthHealth, SourceHealth,
    check_oauth_file, check_oauth_json, get_reauth_instructions,
};
pub use doctor::{CheckStatus, DiagnosticCheck, DoctorReport, ProviderHealth};
pub use fetch_plan::{FetchAttempt, FetchOutcome, FetchStrategy};
pub use models::{
    CostDailyEntry, CostPayload, CostTotals, CreditEvent, CreditsSnapshot, OpenAIDashboardSnapshot,
    ProviderIdentity, ProviderPayload, RateWindow, RobotOutput, StatusIndicator, StatusPayload,
    UsageSnapshot,
};
pub use prediction::{calculate_velocity, detect_reset, smoothed_velocity};
pub use pricing::{
    CostConfidence, ModelPricing, PricingTable, SessionCost, SessionCostCalculator,
    TokenCostBreakdown,
};
pub use provider::{Provider, ProviderDescriptor, ProviderRegistry, ProviderSelection};
pub use session_logs::{
    ClaudeSessionParser, CodexSessionParser, SessionLogFinder, SessionLogPath, SessionUsage,
};
pub use status::StatusFetcher;
