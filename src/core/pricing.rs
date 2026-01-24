//! Pricing data and cost calculation for session usage.
//!
//! This module provides model pricing information and calculates costs from
//! session token usage data parsed by the session_logs module.

use crate::core::session_logs::SessionUsage;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Per-million token pricing for a specific model.
#[derive(Debug, Clone)]
pub struct ModelPricing {
    /// Model identifier (e.g., "claude-3-opus", "gpt-4").
    pub model: String,
    /// Cost per million input tokens (USD).
    pub input_per_million: f64,
    /// Cost per million output tokens (USD).
    pub output_per_million: f64,
    /// Cost per million cache read tokens (USD).
    pub cache_read_per_million: f64,
    /// Cost per million cache creation tokens (USD).
    pub cache_creation_per_million: f64,
}

impl ModelPricing {
    /// Create a new pricing entry.
    #[must_use]
    pub const fn new(
        _model: &'static str,
        input_per_million: f64,
        output_per_million: f64,
        cache_read_per_million: f64,
        cache_creation_per_million: f64,
    ) -> Self {
        Self {
            model: String::new(), // Will be set via from_static
            input_per_million,
            output_per_million,
            cache_read_per_million,
            cache_creation_per_million,
        }
    }

    fn from_static(
        model: &str,
        input_per_million: f64,
        output_per_million: f64,
        cache_read_per_million: f64,
        cache_creation_per_million: f64,
    ) -> Self {
        Self {
            model: model.to_string(),
            input_per_million,
            output_per_million,
            cache_read_per_million,
            cache_creation_per_million,
        }
    }

    /// Calculate cost for given token counts.
    #[must_use]
    pub fn calculate_cost(
        &self,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
    ) -> TokenCostBreakdown {
        let input_cost = (input as f64 / 1_000_000.0) * self.input_per_million;
        let output_cost = (output as f64 / 1_000_000.0) * self.output_per_million;
        let cache_read_cost = (cache_read as f64 / 1_000_000.0) * self.cache_read_per_million;
        let cache_creation_cost =
            (cache_creation as f64 / 1_000_000.0) * self.cache_creation_per_million;

        TokenCostBreakdown {
            input_cost_usd: input_cost,
            output_cost_usd: output_cost,
            cache_read_cost_usd: cache_read_cost,
            cache_creation_cost_usd: cache_creation_cost,
            total_cost_usd: input_cost + output_cost + cache_read_cost + cache_creation_cost,
        }
    }
}

/// Cost breakdown by token type.
#[derive(Debug, Clone, Default)]
pub struct TokenCostBreakdown {
    pub input_cost_usd: f64,
    pub output_cost_usd: f64,
    pub cache_read_cost_usd: f64,
    pub cache_creation_cost_usd: f64,
    pub total_cost_usd: f64,
}

/// Collection of model pricing data with effective date.
#[derive(Debug, Clone)]
pub struct PricingTable {
    /// Model name to pricing mapping (normalized lowercase).
    models: HashMap<String, ModelPricing>,
    /// When this pricing data became effective.
    pub effective_date: DateTime<Utc>,
}

impl Default for PricingTable {
    fn default() -> Self {
        Self::current()
    }
}

impl PricingTable {
    /// Create a pricing table with current (January 2026) pricing.
    ///
    /// Pricing sources:
    /// - Anthropic: https://www.anthropic.com/pricing
    /// - OpenAI: https://openai.com/pricing
    #[must_use]
    pub fn current() -> Self {
        let mut models = HashMap::new();

        // Anthropic Claude models (as of Jan 2026)
        // Claude Opus 4.5: $15/$75 per million
        Self::add_model(
            &mut models,
            "claude-opus-4-5-20251101",
            15.0,
            75.0,
            1.5,
            18.75,
        );
        Self::add_model(&mut models, "claude-opus-4.5", 15.0, 75.0, 1.5, 18.75);
        Self::add_model(&mut models, "claude-4-opus", 15.0, 75.0, 1.5, 18.75);

        // Claude Sonnet 4: $3/$15 per million
        Self::add_model(
            &mut models,
            "claude-sonnet-4-20250514",
            3.0,
            15.0,
            0.3,
            3.75,
        );
        Self::add_model(&mut models, "claude-sonnet-4", 3.0, 15.0, 0.3, 3.75);
        Self::add_model(&mut models, "claude-4-sonnet", 3.0, 15.0, 0.3, 3.75);

        // Claude 3.5 Sonnet: $3/$15 per million
        Self::add_model(
            &mut models,
            "claude-3-5-sonnet-20241022",
            3.0,
            15.0,
            0.3,
            3.75,
        );
        Self::add_model(&mut models, "claude-3.5-sonnet", 3.0, 15.0, 0.3, 3.75);
        Self::add_model(&mut models, "claude-3-5-sonnet", 3.0, 15.0, 0.3, 3.75);

        // Claude 3 Opus: $15/$75 per million
        Self::add_model(
            &mut models,
            "claude-3-opus-20240229",
            15.0,
            75.0,
            1.5,
            18.75,
        );
        Self::add_model(&mut models, "claude-3-opus", 15.0, 75.0, 1.5, 18.75);

        // Claude 3.5 Haiku: $0.80/$4 per million
        Self::add_model(
            &mut models,
            "claude-3-5-haiku-20241022",
            0.8,
            4.0,
            0.08,
            1.0,
        );
        Self::add_model(&mut models, "claude-3.5-haiku", 0.8, 4.0, 0.08, 1.0);
        Self::add_model(&mut models, "claude-3-5-haiku", 0.8, 4.0, 0.08, 1.0);

        // Claude 3 Haiku: $0.25/$1.25 per million
        Self::add_model(
            &mut models,
            "claude-3-haiku-20240307",
            0.25,
            1.25,
            0.03,
            0.30,
        );
        Self::add_model(&mut models, "claude-3-haiku", 0.25, 1.25, 0.03, 0.30);

        // OpenAI GPT models (as of Jan 2026)
        // GPT-4o: $2.50/$10 per million
        Self::add_model(&mut models, "gpt-4o", 2.5, 10.0, 1.25, 2.5);
        Self::add_model(&mut models, "gpt-4o-2024-11-20", 2.5, 10.0, 1.25, 2.5);

        // GPT-4o mini: $0.15/$0.60 per million
        Self::add_model(&mut models, "gpt-4o-mini", 0.15, 0.60, 0.075, 0.15);
        Self::add_model(
            &mut models,
            "gpt-4o-mini-2024-07-18",
            0.15,
            0.60,
            0.075,
            0.15,
        );

        // GPT-4.1: estimated similar to GPT-4o
        Self::add_model(&mut models, "gpt-4.1", 2.5, 10.0, 1.25, 2.5);

        // GPT-5 / GPT-5.2 (estimated - premium tier)
        Self::add_model(&mut models, "gpt-5", 5.0, 20.0, 2.5, 5.0);
        Self::add_model(&mut models, "gpt-5.2", 5.0, 20.0, 2.5, 5.0);
        Self::add_model(&mut models, "gpt-5-codex", 5.0, 20.0, 2.5, 5.0);
        Self::add_model(&mut models, "gpt-5.2-codex", 5.0, 20.0, 2.5, 5.0);

        // o1 reasoning models
        Self::add_model(&mut models, "o1", 15.0, 60.0, 7.5, 15.0);
        Self::add_model(&mut models, "o1-2024-12-17", 15.0, 60.0, 7.5, 15.0);
        Self::add_model(&mut models, "o1-preview", 15.0, 60.0, 7.5, 15.0);

        // o1-mini
        Self::add_model(&mut models, "o1-mini", 3.0, 12.0, 1.5, 3.0);
        Self::add_model(&mut models, "o1-mini-2024-09-12", 3.0, 12.0, 1.5, 3.0);

        // o3-mini (latest reasoning model)
        Self::add_model(&mut models, "o3-mini", 1.1, 4.4, 0.55, 1.1);
        Self::add_model(&mut models, "o3-mini-2025-01-31", 1.1, 4.4, 0.55, 1.1);

        // Google Gemini models
        Self::add_model(&mut models, "gemini-2.0-flash", 0.10, 0.40, 0.025, 0.10);
        Self::add_model(&mut models, "gemini-1.5-pro", 1.25, 5.0, 0.3125, 1.25);
        Self::add_model(&mut models, "gemini-1.5-flash", 0.075, 0.30, 0.01875, 0.075);
        Self::add_model(&mut models, "gemini-3-pro-preview", 2.0, 8.0, 0.5, 2.0);

        Self {
            models,
            effective_date: Utc::now(),
        }
    }

    fn add_model(
        models: &mut HashMap<String, ModelPricing>,
        model: &str,
        input: f64,
        output: f64,
        cache_read: f64,
        cache_creation: f64,
    ) {
        let pricing = ModelPricing::from_static(model, input, output, cache_read, cache_creation);
        models.insert(model.to_lowercase(), pricing);
    }

    /// Look up pricing for a model by name.
    ///
    /// Model names are normalized to lowercase for matching.
    /// Returns None if the model is not in the pricing table.
    #[must_use]
    pub fn get(&self, model: &str) -> Option<&ModelPricing> {
        self.models.get(&model.to_lowercase())
    }

    /// Get pricing for a model, falling back to a conservative estimate.
    ///
    /// If the model is unknown, returns mid-tier pricing as a fallback.
    #[must_use]
    pub fn get_or_estimate(&self, model: &str) -> (ModelPricing, bool) {
        if let Some(pricing) = self.get(model) {
            (pricing.clone(), true)
        } else {
            // Conservative mid-tier estimate for unknown models
            let estimated = ModelPricing::from_static(
                model, 3.0,  // input: assume Sonnet-tier
                15.0, // output: assume Sonnet-tier
                0.3,  // cache read
                3.75, // cache creation
            );
            (estimated, false)
        }
    }

    /// Get all known model names.
    #[must_use]
    pub fn known_models(&self) -> Vec<&str> {
        self.models.keys().map(String::as_str).collect()
    }
}

/// Confidence level for cost calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostConfidence {
    /// Known model, complete session data.
    High,
    /// Known model but incomplete session or minor estimation.
    Medium,
    /// Unknown model or significant estimation involved.
    Low,
    /// Cannot calculate reliably.
    Unknown,
}

impl CostConfidence {
    /// Get a human-readable description.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::High => "accurate",
            Self::Medium => "estimated",
            Self::Low => "rough estimate",
            Self::Unknown => "unknown",
        }
    }
}

/// Calculated cost for a session.
#[derive(Debug, Clone)]
pub struct SessionCost {
    /// Total cost in USD.
    pub total_usd: f64,
    /// Cost breakdown by token type.
    pub breakdown: TokenCostBreakdown,
    /// Primary model used (most expensive or most tokens).
    pub model: String,
    /// Confidence in the calculation.
    pub confidence: CostConfidence,
    /// Whether the model pricing was known.
    pub model_known: bool,
}

/// Calculator for session costs.
pub struct SessionCostCalculator {
    pricing: PricingTable,
}

impl Default for SessionCostCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionCostCalculator {
    /// Create a calculator with current pricing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pricing: PricingTable::current(),
        }
    }

    /// Create a calculator with custom pricing table.
    #[must_use]
    pub const fn with_pricing(pricing: PricingTable) -> Self {
        Self { pricing }
    }

    /// Calculate cost for a session.
    #[must_use]
    pub fn calculate(&self, usage: &SessionUsage) -> SessionCost {
        // Determine the primary model (most expensive or first found)
        let primary_model = self.select_primary_model(usage);

        // Get pricing (known or estimated)
        let (pricing, model_known) = self.pricing.get_or_estimate(&primary_model);

        // Calculate token costs
        let breakdown = pricing.calculate_cost(
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            usage.cache_creation_tokens,
        );

        // Determine confidence level
        let confidence = self.calculate_confidence(usage, model_known);

        SessionCost {
            total_usd: breakdown.total_cost_usd,
            breakdown,
            model: primary_model,
            confidence,
            model_known,
        }
    }

    /// Select the primary model from a session.
    fn select_primary_model(&self, usage: &SessionUsage) -> String {
        // If no models recorded, use unknown
        if usage.models_used.is_empty() {
            return "unknown".to_string();
        }

        // If only one model, use it
        if usage.models_used.len() == 1 {
            return usage.models_used.iter().next().unwrap().clone();
        }

        // For multiple models, prefer the most expensive known model
        let mut best_model = None;
        let mut best_output_rate = 0.0;

        for model in &usage.models_used {
            if let Some(pricing) = self.pricing.get(model) {
                if pricing.output_per_million > best_output_rate {
                    best_output_rate = pricing.output_per_million;
                    best_model = Some(model.clone());
                }
            }
        }

        // Fall back to first model if none were priced
        best_model.unwrap_or_else(|| usage.models_used.iter().next().unwrap().clone())
    }

    /// Determine confidence level for a calculation.
    fn calculate_confidence(&self, usage: &SessionUsage, model_known: bool) -> CostConfidence {
        // Unknown model = low confidence
        if !model_known {
            return CostConfidence::Low;
        }

        // No tokens = unknown
        if usage.input_tokens == 0 && usage.output_tokens == 0 {
            return CostConfidence::Unknown;
        }

        // Multiple unknown models mixed with known = medium
        let known_count = usage
            .models_used
            .iter()
            .filter(|m| self.pricing.get(m).is_some())
            .count();

        if known_count < usage.models_used.len() {
            return CostConfidence::Medium;
        }

        // Complete session with timestamps = high
        if usage.started_at.is_some() && usage.ended_at.is_some() {
            return CostConfidence::High;
        }

        // Known model but incomplete session = medium
        CostConfidence::Medium
    }

    /// Get the underlying pricing table.
    #[must_use]
    pub const fn pricing(&self) -> &PricingTable {
        &self.pricing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_usage(input: i64, output: i64, models: &[&str]) -> SessionUsage {
        SessionUsage {
            session_id: "test".to_string(),
            project_path: None,
            started_at: Some(Utc::now()),
            ended_at: Some(Utc::now()),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            models_used: models.iter().map(|s| (*s).to_string()).collect(),
            message_count: 1,
        }
    }

    #[test]
    fn pricing_table_has_claude_models() {
        let table = PricingTable::current();
        assert!(table.get("claude-3-opus").is_some());
        assert!(table.get("claude-3.5-sonnet").is_some());
        assert!(table.get("claude-opus-4.5").is_some());
    }

    #[test]
    fn pricing_table_has_openai_models() {
        let table = PricingTable::current();
        assert!(table.get("gpt-4o").is_some());
        assert!(table.get("gpt-4o-mini").is_some());
        assert!(table.get("o1").is_some());
    }

    #[test]
    fn pricing_is_case_insensitive() {
        let table = PricingTable::current();
        assert!(table.get("CLAUDE-3-OPUS").is_some());
        assert!(table.get("GPT-4O").is_some());
    }

    #[test]
    fn unknown_model_returns_estimate() {
        let table = PricingTable::current();
        let (pricing, known) = table.get_or_estimate("totally-fake-model");
        assert!(!known);
        assert!(pricing.input_per_million > 0.0);
    }

    #[test]
    fn calculate_cost_for_claude_opus() {
        let calc = SessionCostCalculator::new();
        let usage = make_usage(1_000_000, 100_000, &["claude-3-opus"]);
        let cost = calc.calculate(&usage);

        // 1M input @ $15/M = $15.00
        // 100K output @ $75/M = $7.50
        // Total = $22.50
        assert!((cost.total_usd - 22.5).abs() < 0.01);
        assert_eq!(cost.confidence, CostConfidence::High);
        assert!(cost.model_known);
    }

    #[test]
    fn calculate_cost_for_gpt4o() {
        let calc = SessionCostCalculator::new();
        let usage = make_usage(500_000, 200_000, &["gpt-4o"]);
        let cost = calc.calculate(&usage);

        // 500K input @ $2.50/M = $1.25
        // 200K output @ $10/M = $2.00
        // Total = $3.25
        assert!((cost.total_usd - 3.25).abs() < 0.01);
        assert_eq!(cost.confidence, CostConfidence::High);
    }

    #[test]
    fn calculate_cost_with_cache_tokens() {
        let calc = SessionCostCalculator::new();
        let mut usage = make_usage(100_000, 50_000, &["claude-3.5-sonnet"]);
        usage.cache_read_tokens = 200_000;
        usage.cache_creation_tokens = 50_000;
        let cost = calc.calculate(&usage);

        // 100K input @ $3/M = $0.30
        // 50K output @ $15/M = $0.75
        // 200K cache read @ $0.30/M = $0.06
        // 50K cache creation @ $3.75/M = $0.1875
        // Total = ~$1.2975
        assert!(cost.total_usd > 1.0);
        assert!(cost.total_usd < 2.0);
    }

    #[test]
    fn unknown_model_gives_low_confidence() {
        let calc = SessionCostCalculator::new();
        let usage = make_usage(100_000, 50_000, &["mystery-model-xyz"]);
        let cost = calc.calculate(&usage);

        assert_eq!(cost.confidence, CostConfidence::Low);
        assert!(!cost.model_known);
        assert!(cost.total_usd > 0.0);
    }

    #[test]
    fn empty_usage_gives_unknown_confidence() {
        let calc = SessionCostCalculator::new();
        let usage = make_usage(0, 0, &["claude-3-opus"]);
        let cost = calc.calculate(&usage);

        assert_eq!(cost.confidence, CostConfidence::Unknown);
        assert!((cost.total_usd - 0.0).abs() < 0.0001);
    }

    #[test]
    fn multiple_models_selects_most_expensive() {
        let calc = SessionCostCalculator::new();
        let usage = make_usage(100_000, 50_000, &["claude-3-haiku", "claude-3-opus"]);
        let cost = calc.calculate(&usage);

        // Should select opus as primary (most expensive)
        assert!(cost.model.contains("opus"));
    }

    #[test]
    fn no_models_uses_unknown() {
        let calc = SessionCostCalculator::new();
        let usage = SessionUsage {
            session_id: "test".to_string(),
            project_path: None,
            started_at: Some(Utc::now()),
            ended_at: Some(Utc::now()),
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            models_used: HashSet::new(),
            message_count: 1,
        };
        let cost = calc.calculate(&usage);

        assert_eq!(cost.model, "unknown");
        assert!(!cost.model_known);
    }

    #[test]
    fn model_pricing_calculates_correctly() {
        let pricing = ModelPricing::from_static("test", 10.0, 50.0, 1.0, 12.5);
        let breakdown = pricing.calculate_cost(2_000_000, 500_000, 100_000, 50_000);

        // 2M input @ $10/M = $20
        // 500K output @ $50/M = $25
        // 100K cache read @ $1/M = $0.10
        // 50K cache creation @ $12.5/M = $0.625
        // Total = $45.725
        assert!((breakdown.total_cost_usd - 45.725).abs() < 0.001);
    }
}
