//! Budget precedence resolution for usage and cost limits.
//!
//! This module implements the configuration merging logic for budgets with
//! multiple priority levels. Higher priority configurations override lower ones.
//!
//! ## Priority Levels
//!
//! From lowest to highest:
//! 1. `Global` (0) - Default limits applying to all providers
//! 2. `ProviderDefault` (1) - Provider-specific defaults
//! 3. `ProviderSpecific` (2) - User-configured provider limits
//! 4. `Override` (3) - Temporary or CLI overrides
//!
//! ## TOML Configuration Format
//!
//! ```toml
//! [global]
//! daily_cost_usd = 10.0
//! weekly_cost_usd = 50.0
//! monthly_cost_usd = 150.0
//! alert_at_percent = [50, 75, 90]
//!
//! [claude]
//! daily_usage_percent = 80
//! weekly_cost_usd = 30.0
//!
//! [claude.override]
//! daily_cost_usd = 5.0  # Temporary stricter limit
//! ```

use crate::core::Provider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Budget Priority
// =============================================================================

/// Priority levels for budget configurations.
///
/// Higher values override lower values when resolving conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BudgetPriority {
    /// Default limits applying to all providers.
    Global = 0,
    /// Provider-specific defaults (built-in).
    ProviderDefault = 1,
    /// User-configured provider limits.
    ProviderSpecific = 2,
    /// Temporary or CLI overrides.
    Override = 3,
}

impl BudgetPriority {
    /// All priority levels in order from lowest to highest.
    pub const ALL: &'static [Self] = &[
        Self::Global,
        Self::ProviderDefault,
        Self::ProviderSpecific,
        Self::Override,
    ];

    /// Human-readable name for this priority level.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::ProviderDefault => "Provider Default",
            Self::ProviderSpecific => "Provider Specific",
            Self::Override => "Override",
        }
    }
}

impl std::fmt::Display for BudgetPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// =============================================================================
// Budget Limits
// =============================================================================

/// Budget limits that can be configured at any priority level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetLimits {
    /// Daily cost limit in USD.
    pub daily_cost_usd: Option<f64>,
    /// Weekly cost limit in USD.
    pub weekly_cost_usd: Option<f64>,
    /// Monthly cost limit in USD.
    pub monthly_cost_usd: Option<f64>,
    /// Daily usage percentage limit (0-100).
    pub daily_usage_percent: Option<f64>,
    /// Weekly usage percentage limit (0-100).
    pub weekly_usage_percent: Option<f64>,
    /// Daily credit limit.
    pub daily_credits: Option<f64>,
    /// Alert thresholds as percentages (e.g., [50, 75, 90]).
    #[serde(default)]
    pub alert_at_percent: Vec<u8>,
}

impl BudgetLimits {
    /// Check if all limits are None/empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.daily_cost_usd.is_none()
            && self.weekly_cost_usd.is_none()
            && self.monthly_cost_usd.is_none()
            && self.daily_usage_percent.is_none()
            && self.weekly_usage_percent.is_none()
            && self.daily_credits.is_none()
            && self.alert_at_percent.is_empty()
    }
}

// =============================================================================
// Budget Configuration
// =============================================================================

/// A single budget configuration entry with priority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Provider this config applies to (None = global).
    pub provider: Option<Provider>,
    /// Priority level for conflict resolution.
    pub priority: BudgetPriority,
    /// The actual budget limits.
    pub limits: BudgetLimits,
}

impl BudgetConfig {
    /// Create a global budget configuration.
    #[must_use]
    pub fn global(limits: BudgetLimits) -> Self {
        Self {
            provider: None,
            priority: BudgetPriority::Global,
            limits,
        }
    }

    /// Create a provider-specific budget configuration.
    #[must_use]
    pub fn for_provider(provider: Provider, limits: BudgetLimits) -> Self {
        Self {
            provider: Some(provider),
            priority: BudgetPriority::ProviderSpecific,
            limits,
        }
    }

    /// Create a provider override configuration.
    #[must_use]
    pub fn override_for_provider(provider: Provider, limits: BudgetLimits) -> Self {
        Self {
            provider: Some(provider),
            priority: BudgetPriority::Override,
            limits,
        }
    }
}

// =============================================================================
// Budget Sources (Tracking Origin)
// =============================================================================

/// Tracks where each resolved value came from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetSources {
    /// Source of daily_cost_usd value.
    pub daily_cost_usd: Option<BudgetPriority>,
    /// Source of weekly_cost_usd value.
    pub weekly_cost_usd: Option<BudgetPriority>,
    /// Source of monthly_cost_usd value.
    pub monthly_cost_usd: Option<BudgetPriority>,
    /// Source of daily_usage_percent value.
    pub daily_usage_percent: Option<BudgetPriority>,
    /// Source of weekly_usage_percent value.
    pub weekly_usage_percent: Option<BudgetPriority>,
    /// Source of daily_credits value.
    pub daily_credits: Option<BudgetPriority>,
    /// Source of alert_at_percent value.
    pub alert_at_percent: Option<BudgetPriority>,
}

// =============================================================================
// Resolved Budget
// =============================================================================

/// A fully resolved budget for a specific provider.
///
/// Created by merging all applicable configurations by priority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedBudget {
    /// The provider this budget applies to.
    pub provider: Provider,
    /// The resolved limit values.
    pub limits: BudgetLimits,
    /// Tracks where each value came from.
    pub sources: BudgetSources,
}

impl ResolvedBudget {
    /// Check if any limits are configured.
    #[must_use]
    pub fn has_limits(&self) -> bool {
        !self.limits.is_empty()
    }
}

// =============================================================================
// Budget Resolution
// =============================================================================

/// Resolves budget configurations for a provider by merging in priority order.
///
/// # Algorithm
///
/// 1. Collect all applicable configs (global + provider-specific)
/// 2. Sort by priority (lowest first)
/// 3. For each field, take the first non-None value from highest priority
/// 4. For alert thresholds, take the most conservative (lowest) values
///
/// # Arguments
///
/// * `provider` - The provider to resolve budgets for
/// * `configs` - All budget configurations to consider
///
/// # Returns
///
/// A `ResolvedBudget` with merged limits and source tracking.
#[must_use]
pub fn resolve_budget(provider: Provider, configs: &[BudgetConfig]) -> ResolvedBudget {
    // Collect applicable configs: global (provider=None) or matching provider
    let mut applicable: Vec<&BudgetConfig> = configs
        .iter()
        .filter(|c| c.provider.is_none() || c.provider == Some(provider))
        .collect();

    // Sort by priority (highest first for easy override)
    applicable.sort_by(|a, b| b.priority.cmp(&a.priority));

    let mut limits = BudgetLimits::default();
    let mut sources = BudgetSources::default();

    // Merge each field from highest to lowest priority
    for config in &applicable {
        // Daily cost
        if limits.daily_cost_usd.is_none() {
            if let Some(val) = config.limits.daily_cost_usd {
                limits.daily_cost_usd = Some(val);
                sources.daily_cost_usd = Some(config.priority);
            }
        }

        // Weekly cost
        if limits.weekly_cost_usd.is_none() {
            if let Some(val) = config.limits.weekly_cost_usd {
                limits.weekly_cost_usd = Some(val);
                sources.weekly_cost_usd = Some(config.priority);
            }
        }

        // Monthly cost
        if limits.monthly_cost_usd.is_none() {
            if let Some(val) = config.limits.monthly_cost_usd {
                limits.monthly_cost_usd = Some(val);
                sources.monthly_cost_usd = Some(config.priority);
            }
        }

        // Daily usage percent
        if limits.daily_usage_percent.is_none() {
            if let Some(val) = config.limits.daily_usage_percent {
                limits.daily_usage_percent = Some(val);
                sources.daily_usage_percent = Some(config.priority);
            }
        }

        // Weekly usage percent
        if limits.weekly_usage_percent.is_none() {
            if let Some(val) = config.limits.weekly_usage_percent {
                limits.weekly_usage_percent = Some(val);
                sources.weekly_usage_percent = Some(config.priority);
            }
        }

        // Daily credits
        if limits.daily_credits.is_none() {
            if let Some(val) = config.limits.daily_credits {
                limits.daily_credits = Some(val);
                sources.daily_credits = Some(config.priority);
            }
        }
    }

    // Alert thresholds: take the most conservative (lowest) values
    // Merge all threshold sets and keep the unique lowest values
    let mut all_thresholds: Vec<u8> = applicable
        .iter()
        .flat_map(|c| c.limits.alert_at_percent.iter().copied())
        .collect();

    if !all_thresholds.is_empty() {
        all_thresholds.sort_unstable();
        all_thresholds.dedup();
        // Take the most conservative: prefer lower thresholds
        // Keep up to 5 unique thresholds
        all_thresholds.truncate(5);
        limits.alert_at_percent = all_thresholds;
        // Find which config contributed the lowest threshold
        if let Some(lowest) = limits.alert_at_percent.first() {
            for config in &applicable {
                if config.limits.alert_at_percent.contains(lowest) {
                    sources.alert_at_percent = Some(config.priority);
                    break;
                }
            }
        }
    }

    ResolvedBudget {
        provider,
        limits,
        sources,
    }
}

// =============================================================================
// Budget Violations
// =============================================================================

/// Type of budget violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViolationType {
    /// Daily cost limit exceeded.
    DailyCost,
    /// Weekly cost limit exceeded.
    WeeklyCost,
    /// Monthly cost limit exceeded.
    MonthlyCost,
    /// Daily usage percentage exceeded.
    DailyUsage,
    /// Weekly usage percentage exceeded.
    WeeklyUsage,
    /// Daily credits exceeded.
    DailyCredits,
    /// Alert threshold reached.
    AlertThreshold,
}

impl ViolationType {
    /// Human-readable description.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::DailyCost => "Daily cost limit exceeded",
            Self::WeeklyCost => "Weekly cost limit exceeded",
            Self::MonthlyCost => "Monthly cost limit exceeded",
            Self::DailyUsage => "Daily usage limit exceeded",
            Self::WeeklyUsage => "Weekly usage limit exceeded",
            Self::DailyCredits => "Daily credit limit exceeded",
            Self::AlertThreshold => "Alert threshold reached",
        }
    }
}

impl std::fmt::Display for ViolationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// A specific budget violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetViolation {
    /// Type of violation.
    pub violation_type: ViolationType,
    /// The configured limit.
    pub limit: f64,
    /// The current value.
    pub current: f64,
    /// Percentage of limit used (current/limit * 100).
    pub percent_used: f64,
    /// Source of the limit that was violated.
    pub source: BudgetPriority,
}

impl BudgetViolation {
    /// Create a new violation.
    #[must_use]
    pub fn new(
        violation_type: ViolationType,
        limit: f64,
        current: f64,
        source: BudgetPriority,
    ) -> Self {
        let percent_used = if limit > 0.0 {
            (current / limit) * 100.0
        } else {
            100.0
        };
        Self {
            violation_type,
            limit,
            current,
            percent_used,
            source,
        }
    }

    /// Check if this is a hard limit violation (100%+).
    #[must_use]
    pub fn is_exceeded(&self) -> bool {
        self.current >= self.limit
    }

    /// Check if this is an alert threshold (not yet exceeded but close).
    #[must_use]
    pub fn is_warning(&self) -> bool {
        self.violation_type == ViolationType::AlertThreshold && !self.is_exceeded()
    }
}

/// Current usage values for checking against budgets.
#[derive(Debug, Clone, Default)]
pub struct CurrentUsage {
    /// Daily cost spent in USD.
    pub daily_cost_usd: Option<f64>,
    /// Weekly cost spent in USD.
    pub weekly_cost_usd: Option<f64>,
    /// Monthly cost spent in USD.
    pub monthly_cost_usd: Option<f64>,
    /// Daily usage percentage (0-100).
    pub daily_usage_percent: Option<f64>,
    /// Weekly usage percentage (0-100).
    pub weekly_usage_percent: Option<f64>,
    /// Daily credits used.
    pub daily_credits: Option<f64>,
}

/// Check a resolved budget against current usage.
///
/// # Returns
///
/// A vector of all violations found. May include both hard limit violations
/// and alert threshold warnings.
#[must_use]
pub fn check_budget_violations(
    budget: &ResolvedBudget,
    usage: &CurrentUsage,
) -> Vec<BudgetViolation> {
    let mut violations = Vec::new();

    // Check daily cost
    if let (Some(limit), Some(current)) = (budget.limits.daily_cost_usd, usage.daily_cost_usd) {
        if current >= limit {
            violations.push(BudgetViolation::new(
                ViolationType::DailyCost,
                limit,
                current,
                budget.sources.daily_cost_usd.unwrap_or(BudgetPriority::Global),
            ));
        }
    }

    // Check weekly cost
    if let (Some(limit), Some(current)) = (budget.limits.weekly_cost_usd, usage.weekly_cost_usd) {
        if current >= limit {
            violations.push(BudgetViolation::new(
                ViolationType::WeeklyCost,
                limit,
                current,
                budget.sources.weekly_cost_usd.unwrap_or(BudgetPriority::Global),
            ));
        }
    }

    // Check monthly cost
    if let (Some(limit), Some(current)) = (budget.limits.monthly_cost_usd, usage.monthly_cost_usd) {
        if current >= limit {
            violations.push(BudgetViolation::new(
                ViolationType::MonthlyCost,
                limit,
                current,
                budget.sources.monthly_cost_usd.unwrap_or(BudgetPriority::Global),
            ));
        }
    }

    // Check daily usage percent
    if let (Some(limit), Some(current)) =
        (budget.limits.daily_usage_percent, usage.daily_usage_percent)
    {
        if current >= limit {
            violations.push(BudgetViolation::new(
                ViolationType::DailyUsage,
                limit,
                current,
                budget
                    .sources
                    .daily_usage_percent
                    .unwrap_or(BudgetPriority::Global),
            ));
        }
    }

    // Check weekly usage percent
    if let (Some(limit), Some(current)) =
        (budget.limits.weekly_usage_percent, usage.weekly_usage_percent)
    {
        if current >= limit {
            violations.push(BudgetViolation::new(
                ViolationType::WeeklyUsage,
                limit,
                current,
                budget
                    .sources
                    .weekly_usage_percent
                    .unwrap_or(BudgetPriority::Global),
            ));
        }
    }

    // Check daily credits
    if let (Some(limit), Some(current)) = (budget.limits.daily_credits, usage.daily_credits) {
        if current >= limit {
            violations.push(BudgetViolation::new(
                ViolationType::DailyCredits,
                limit,
                current,
                budget.sources.daily_credits.unwrap_or(BudgetPriority::Global),
            ));
        }
    }

    // Check alert thresholds for cost-based limits
    // Use the most relevant limit for threshold checking
    let (primary_limit, primary_current, primary_source) =
        if let (Some(limit), Some(current)) = (budget.limits.daily_cost_usd, usage.daily_cost_usd) {
            (
                limit,
                current,
                budget.sources.daily_cost_usd.unwrap_or(BudgetPriority::Global),
            )
        } else if let (Some(limit), Some(current)) =
            (budget.limits.weekly_cost_usd, usage.weekly_cost_usd)
        {
            (
                limit,
                current,
                budget
                    .sources
                    .weekly_cost_usd
                    .unwrap_or(BudgetPriority::Global),
            )
        } else {
            return violations;
        };

    // Check each alert threshold
    for &threshold in &budget.limits.alert_at_percent {
        let threshold_value = primary_limit * (f64::from(threshold) / 100.0);
        if primary_current >= threshold_value && primary_current < primary_limit {
            violations.push(BudgetViolation::new(
                ViolationType::AlertThreshold,
                threshold_value,
                primary_current,
                primary_source,
            ));
            // Only report the highest threshold reached
            break;
        }
    }

    violations
}

// =============================================================================
// TOML Configuration Support
// =============================================================================

/// Root configuration structure for budgets.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetFileConfig {
    /// Global budget limits.
    pub global: Option<BudgetLimits>,
    /// Per-provider configurations keyed by CLI name.
    #[serde(flatten)]
    pub providers: HashMap<String, ProviderBudgetConfig>,
}

/// Provider-specific budget configuration with optional override section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderBudgetConfig {
    /// Standard provider limits.
    #[serde(flatten)]
    pub limits: BudgetLimits,
    /// Override limits (highest priority).
    #[serde(rename = "override")]
    pub override_limits: Option<BudgetLimits>,
}

impl BudgetFileConfig {
    /// Convert file config to a list of `BudgetConfig` entries.
    #[must_use]
    pub fn to_configs(&self) -> Vec<BudgetConfig> {
        let mut configs = Vec::new();

        // Add global config
        if let Some(ref global_limits) = self.global {
            if !global_limits.is_empty() {
                configs.push(BudgetConfig::global(global_limits.clone()));
            }
        }

        // Add provider-specific configs
        for (name, provider_config) in &self.providers {
            // Skip special keys
            if name == "global" {
                continue;
            }

            // Try to parse provider name
            if let Some(provider) = parse_provider_name(name) {
                // Add provider-specific limits
                if !provider_config.limits.is_empty() {
                    configs.push(BudgetConfig::for_provider(
                        provider,
                        provider_config.limits.clone(),
                    ));
                }

                // Add override limits
                if let Some(ref override_limits) = provider_config.override_limits {
                    if !override_limits.is_empty() {
                        configs.push(BudgetConfig::override_for_provider(
                            provider,
                            override_limits.clone(),
                        ));
                    }
                }
            }
        }

        configs
    }
}

/// Parse a provider CLI name to Provider enum.
fn parse_provider_name(name: &str) -> Option<Provider> {
    match name.to_lowercase().as_str() {
        "codex" => Some(Provider::Codex),
        "claude" => Some(Provider::Claude),
        "gemini" => Some(Provider::Gemini),
        "antigravity" => Some(Provider::Antigravity),
        "cursor" => Some(Provider::Cursor),
        "opencode" => Some(Provider::OpenCode),
        "factory" => Some(Provider::Factory),
        "zai" | "z.ai" => Some(Provider::Zai),
        "minimax" => Some(Provider::MiniMax),
        "kimi" => Some(Provider::Kimi),
        "copilot" => Some(Provider::Copilot),
        "kimik2" | "kimi_k2" => Some(Provider::KimiK2),
        "kiro" => Some(Provider::Kiro),
        "vertexai" | "vertex_ai" => Some(Provider::VertexAI),
        "jetbrains" | "jetbrainsai" => Some(Provider::JetBrainsAI),
        "amp" => Some(Provider::Amp),
        _ => None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(BudgetPriority::Override > BudgetPriority::ProviderSpecific);
        assert!(BudgetPriority::ProviderSpecific > BudgetPriority::ProviderDefault);
        assert!(BudgetPriority::ProviderDefault > BudgetPriority::Global);
    }

    #[test]
    fn test_resolve_budget_highest_priority_wins() {
        let global = BudgetConfig::global(BudgetLimits {
            daily_cost_usd: Some(10.0),
            ..Default::default()
        });

        let provider = BudgetConfig::for_provider(
            Provider::Claude,
            BudgetLimits {
                daily_cost_usd: Some(5.0),
                ..Default::default()
            },
        );

        let configs = vec![global, provider];
        let resolved = resolve_budget(Provider::Claude, &configs);

        // Provider-specific should win
        assert_eq!(resolved.limits.daily_cost_usd, Some(5.0));
        assert_eq!(
            resolved.sources.daily_cost_usd,
            Some(BudgetPriority::ProviderSpecific)
        );
    }

    #[test]
    fn test_resolve_budget_fallback_to_global() {
        let global = BudgetConfig::global(BudgetLimits {
            daily_cost_usd: Some(10.0),
            weekly_cost_usd: Some(50.0),
            ..Default::default()
        });

        let provider = BudgetConfig::for_provider(
            Provider::Claude,
            BudgetLimits {
                daily_cost_usd: Some(5.0),
                // No weekly cost - should fall back to global
                ..Default::default()
            },
        );

        let configs = vec![global, provider];
        let resolved = resolve_budget(Provider::Claude, &configs);

        assert_eq!(resolved.limits.daily_cost_usd, Some(5.0));
        assert_eq!(resolved.limits.weekly_cost_usd, Some(50.0));
        assert_eq!(
            resolved.sources.weekly_cost_usd,
            Some(BudgetPriority::Global)
        );
    }

    #[test]
    fn test_resolve_budget_override_wins() {
        let global = BudgetConfig::global(BudgetLimits {
            daily_cost_usd: Some(10.0),
            ..Default::default()
        });

        let provider = BudgetConfig::for_provider(
            Provider::Claude,
            BudgetLimits {
                daily_cost_usd: Some(5.0),
                ..Default::default()
            },
        );

        let override_config = BudgetConfig::override_for_provider(
            Provider::Claude,
            BudgetLimits {
                daily_cost_usd: Some(2.0),
                ..Default::default()
            },
        );

        let configs = vec![global, provider, override_config];
        let resolved = resolve_budget(Provider::Claude, &configs);

        // Override should win
        assert_eq!(resolved.limits.daily_cost_usd, Some(2.0));
        assert_eq!(
            resolved.sources.daily_cost_usd,
            Some(BudgetPriority::Override)
        );
    }

    #[test]
    fn test_resolve_budget_unrelated_provider_ignored() {
        let claude_config = BudgetConfig::for_provider(
            Provider::Claude,
            BudgetLimits {
                daily_cost_usd: Some(5.0),
                ..Default::default()
            },
        );

        let codex_config = BudgetConfig::for_provider(
            Provider::Codex,
            BudgetLimits {
                daily_cost_usd: Some(15.0),
                ..Default::default()
            },
        );

        let configs = vec![claude_config, codex_config];
        let resolved = resolve_budget(Provider::Claude, &configs);

        // Codex config should not affect Claude
        assert_eq!(resolved.limits.daily_cost_usd, Some(5.0));
    }

    #[test]
    fn test_alert_thresholds_most_conservative() {
        let global = BudgetConfig::global(BudgetLimits {
            alert_at_percent: vec![75, 90],
            ..Default::default()
        });

        let provider = BudgetConfig::for_provider(
            Provider::Claude,
            BudgetLimits {
                alert_at_percent: vec![50, 80],
                ..Default::default()
            },
        );

        let configs = vec![global, provider];
        let resolved = resolve_budget(Provider::Claude, &configs);

        // Should merge and keep lowest unique values
        assert_eq!(resolved.limits.alert_at_percent, vec![50, 75, 80, 90]);
    }

    #[test]
    fn test_check_violations_daily_cost() {
        let budget = ResolvedBudget {
            provider: Provider::Claude,
            limits: BudgetLimits {
                daily_cost_usd: Some(10.0),
                ..Default::default()
            },
            sources: BudgetSources {
                daily_cost_usd: Some(BudgetPriority::Global),
                ..Default::default()
            },
        };

        let usage = CurrentUsage {
            daily_cost_usd: Some(12.0),
            ..Default::default()
        };

        let violations = check_budget_violations(&budget, &usage);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].violation_type, ViolationType::DailyCost);
        assert!(violations[0].is_exceeded());
    }

    #[test]
    fn test_check_violations_alert_threshold() {
        let budget = ResolvedBudget {
            provider: Provider::Claude,
            limits: BudgetLimits {
                daily_cost_usd: Some(10.0),
                alert_at_percent: vec![50, 75, 90],
                ..Default::default()
            },
            sources: BudgetSources {
                daily_cost_usd: Some(BudgetPriority::Global),
                ..Default::default()
            },
        };

        let usage = CurrentUsage {
            daily_cost_usd: Some(8.0), // 80% of limit
            ..Default::default()
        };

        let violations = check_budget_violations(&budget, &usage);
        // Should have alert for 75% threshold (not 90% since we're at 80%)
        assert!(violations
            .iter()
            .any(|v| v.violation_type == ViolationType::AlertThreshold));
    }

    #[test]
    fn test_check_violations_no_violation() {
        let budget = ResolvedBudget {
            provider: Provider::Claude,
            limits: BudgetLimits {
                daily_cost_usd: Some(10.0),
                ..Default::default()
            },
            sources: BudgetSources {
                daily_cost_usd: Some(BudgetPriority::Global),
                ..Default::default()
            },
        };

        let usage = CurrentUsage {
            daily_cost_usd: Some(5.0), // Under limit
            ..Default::default()
        };

        let violations = check_budget_violations(&budget, &usage);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_parse_provider_name() {
        assert_eq!(parse_provider_name("claude"), Some(Provider::Claude));
        assert_eq!(parse_provider_name("Claude"), Some(Provider::Claude));
        assert_eq!(parse_provider_name("CODEX"), Some(Provider::Codex));
        assert_eq!(parse_provider_name("z.ai"), Some(Provider::Zai));
        assert_eq!(parse_provider_name("unknown"), None);
    }

    #[test]
    fn test_budget_file_config_to_configs() {
        let mut providers = HashMap::new();
        providers.insert(
            "claude".to_string(),
            ProviderBudgetConfig {
                limits: BudgetLimits {
                    daily_cost_usd: Some(5.0),
                    ..Default::default()
                },
                override_limits: Some(BudgetLimits {
                    daily_cost_usd: Some(2.0),
                    ..Default::default()
                }),
            },
        );

        let file_config = BudgetFileConfig {
            global: Some(BudgetLimits {
                daily_cost_usd: Some(10.0),
                ..Default::default()
            }),
            providers,
        };

        let configs = file_config.to_configs();
        assert_eq!(configs.len(), 3); // global + claude + claude.override
    }
}
