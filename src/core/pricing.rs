//! Pricing data and cost calculation for session usage.
//!
//! This module provides model pricing information and calculates costs from
//! session token usage data parsed by the `session_logs` module.

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

    /// Cost per million 1-hour cache-write tokens: 2x base input on every Claude model
    /// (the 5-minute write rate, 1.25x, is `cache_creation_per_million`).
    #[must_use]
    pub fn cache_write_1h_per_million(&self) -> f64 {
        self.input_per_million * 2.0
    }

    /// Calculate cost for given token counts.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // token counts fit within f64 precision
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
    /// Higher rates a model charges when a request's prompt (input plus cache reads and
    /// writes) is over a threshold: canonical model id -> (threshold tokens, pricing).
    long_prompt: HashMap<String, (i64, ModelPricing)>,
    /// When this pricing data became effective.
    pub effective_date: DateTime<Utc>,
}

/// Token counts of one Claude API response, as its `usage` object reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResponseTokens {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    /// Cache writes with the 5-minute TTL (1.25x base input).
    pub cache_write_5m: i64,
    /// Cache writes with the 1-hour TTL (2x base input).
    pub cache_write_1h: i64,
    /// Served in fast mode (`usage.speed == "fast"`): 2x every token rate.
    pub fast: bool,
    /// Pinned to US-only inference (`usage.inference_geo == "us"`): 1.1x every token rate.
    pub us_only_inference: bool,
}

impl ResponseTokens {
    /// Every input-side token the request sent: what long-prompt pricing tiers count.
    #[must_use]
    pub const fn prompt_tokens(&self) -> i64 {
        self.input + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }
}

/// Canonical form of a model id for pricing lookup.
///
/// Lowercases, drops a provider prefix (`anthropic.`, `us.anthropic.`), anything after a
/// Vertex `@` snapshot, a `:` revision or a `[1m]` context tag, a Bedrock `-v1` suffix and a
/// trailing `-YYYYMMDD` snapshot date, so `claude-opus-4-5-20251101`,
/// `us.anthropic.claude-opus-4-5-20251101-v1:0` and `claude-opus-4-5@20251101` all price
/// as `claude-opus-4-5`.
#[must_use]
pub fn canonical_model_id(model: &str) -> String {
    let lower = model.trim().to_lowercase();
    let mut id = lower.as_str();
    if let Some(pos) = id.find("claude-") {
        id = &id[pos..];
    }
    if let Some(pos) = id.find(['@', ':', '[']) {
        id = &id[..pos];
    }
    if let Some((head, tail)) = id.rsplit_once('-')
        && tail.len() > 1
        && tail.starts_with('v')
        && tail[1..].bytes().all(|b| b.is_ascii_digit())
    {
        id = head;
    }
    if let Some((head, tail)) = id.rsplit_once('-')
        && tail.len() == 8
        && tail.bytes().all(|b| b.is_ascii_digit())
    {
        id = head;
    }
    id.to_string()
}

impl Default for PricingTable {
    fn default() -> Self {
        Self::current()
    }
}

impl PricingTable {
    /// Create a pricing table with current pricing.
    ///
    /// Pricing sources:
    /// - Anthropic (Claude API, first-party rates, November 2026):
    ///   <https://platform.claude.com/docs/en/about-claude/pricing>
    /// - `OpenAI`: <https://openai.com/pricing>
    ///
    /// Claude entries are `(input, output, cache read, 5-minute cache write)` per million
    /// tokens; a 1-hour cache write is 2x base input on every Claude model
    /// ([`ModelPricing::cache_write_1h_per_million`]). Dated snapshots and provider-prefixed
    /// ids resolve through [`canonical_model_id`].
    #[must_use]
    #[allow(clippy::too_many_lines)] // one flat rate table reads best in one place
    pub fn current() -> Self {
        let mut models = HashMap::new();
        let mut long_prompt = HashMap::new();

        // Anthropic Claude models (November 2026).
        for (model, input, output, cache_read, cache_write) in [
            ("claude-fable-5-2", 8.0, 40.0, 0.20, 10.0),
            ("claude-mythos-5-2", 8.0, 40.0, 0.20, 10.0),
            ("claude-fable-5-1", 10.0, 50.0, 0.25, 12.5),
            ("claude-mythos-5-1", 10.0, 50.0, 0.25, 12.5),
            ("claude-fable-5", 10.0, 50.0, 1.0, 12.5),
            ("claude-mythos-5", 10.0, 50.0, 1.0, 12.5),
            ("claude-opus-5-5", 4.0, 20.0, 0.20, 5.0),
            ("claude-opus-5", 5.0, 25.0, 0.50, 6.25),
            ("claude-opus-4-8", 5.0, 25.0, 0.50, 6.25),
            ("claude-opus-4-7", 5.0, 25.0, 0.50, 6.25),
            ("claude-opus-4-6", 5.0, 25.0, 0.50, 6.25),
            ("claude-opus-4-5", 5.0, 25.0, 0.50, 6.25),
            ("claude-opus-4-1", 15.0, 75.0, 1.50, 18.75),
            ("claude-opus-4", 15.0, 75.0, 1.50, 18.75),
            ("claude-sonnet-5-5", 2.0, 10.0, 0.10, 2.5),
            ("claude-sonnet-5", 2.0, 10.0, 0.20, 2.5),
            ("claude-sonnet-4-6", 3.0, 15.0, 0.30, 3.75),
            ("claude-sonnet-4-5", 3.0, 15.0, 0.30, 3.75),
            // Claude Haiku 5.5, prompts up to 100,000 tokens (the long-prompt rates follow).
            ("claude-haiku-5-5", 0.10, 0.50, 0.01, 0.125),
            ("claude-haiku-4-5", 1.0, 5.0, 0.10, 1.25),
        ] {
            Self::add_model(&mut models, model, input, output, cache_read, cache_write);
        }
        // A Claude Haiku 5.5 request whose prompt is over 100,000 tokens pays these on
        // every token of that request.
        long_prompt.insert(
            "claude-haiku-5-5".to_string(),
            (
                100_000,
                ModelPricing::from_static("claude-haiku-5-5", 0.50, 2.50, 0.05, 0.625),
            ),
        );

        // Claude Opus 4.5 aliases kept from earlier versions of this table ($5/$25)
        Self::add_model(
            &mut models,
            "claude-opus-4-5-20251101",
            5.0,
            25.0,
            0.5,
            6.25,
        );
        Self::add_model(&mut models, "claude-opus-4.5", 5.0, 25.0, 0.5, 6.25);
        // Claude Opus 4: $15/$75 per million
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
            long_prompt,
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
    /// Model names are normalized to lowercase for matching; a name with no exact entry
    /// is looked up again by its [`canonical_model_id`] (dated snapshots, provider prefixes).
    /// Returns None if the model is not in the pricing table.
    #[must_use]
    pub fn get(&self, model: &str) -> Option<&ModelPricing> {
        self.models
            .get(&model.to_lowercase())
            .or_else(|| self.models.get(&canonical_model_id(model)))
    }

    /// Pricing for one request with `prompt_tokens` of input (including cache reads and
    /// writes): a model's long-prompt rates when the prompt is over its threshold.
    #[must_use]
    pub fn get_for_prompt(&self, model: &str, prompt_tokens: i64) -> Option<&ModelPricing> {
        if let Some((threshold, pricing)) = self.long_prompt.get(&canonical_model_id(model))
            && prompt_tokens > *threshold
        {
            return Some(pricing);
        }
        self.get(model)
    }

    /// API-rate cost in USD of one response, or `None` when the model is not priced.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // token counts fit within f64 precision
    pub fn response_cost(&self, model: &str, tokens: &ResponseTokens) -> Option<f64> {
        let pricing = self.get_for_prompt(model, tokens.prompt_tokens())?;
        let rate = |count: i64, per_million: f64| count as f64 / 1_000_000.0 * per_million;
        let mut cost = rate(tokens.input, pricing.input_per_million)
            + rate(tokens.output, pricing.output_per_million)
            + rate(tokens.cache_read, pricing.cache_read_per_million)
            + rate(tokens.cache_write_5m, pricing.cache_creation_per_million)
            + rate(tokens.cache_write_1h, pricing.cache_write_1h_per_million());
        if tokens.fast {
            cost *= 2.0;
        }
        if tokens.us_only_inference {
            cost *= 1.1;
        }
        Some(cost)
    }

    /// Get pricing for a model, falling back to a conservative estimate.
    ///
    /// If the model is unknown, returns mid-tier pricing as a fallback.
    #[must_use]
    pub fn get_or_estimate(&self, model: &str) -> (ModelPricing, bool) {
        self.get(model).map_or_else(
            || {
                // Conservative mid-tier estimate for unknown models
                let estimated = ModelPricing::from_static(
                    model, 3.0,  // input: assume Sonnet-tier
                    15.0, // output: assume Sonnet-tier
                    0.3,  // cache read
                    3.75, // cache creation
                );
                (estimated, false)
            },
            |pricing| (pricing.clone(), true),
        )
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
            if let Some(pricing) = self.pricing.get(model)
                && pricing.output_per_million > best_output_rate
            {
                best_output_rate = pricing.output_per_million;
                best_model = Some(model.clone());
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
    fn canonical_model_id_strips_snapshots_and_provider_prefixes() {
        for (raw, expected) in [
            ("claude-opus-5-5", "claude-opus-5-5"),
            ("claude-sonnet-4-5-20250929", "claude-sonnet-4-5"),
            (
                "us.anthropic.claude-opus-4-5-20251101-v1:0",
                "claude-opus-4-5",
            ),
            (
                "anthropic.claude-haiku-4-5-20251001-v1:0",
                "claude-haiku-4-5",
            ),
            ("claude-opus-4-5@20251101", "claude-opus-4-5"),
            ("claude-opus-4-6[1m]", "claude-opus-4-6"),
            ("  Claude-Fable-5-2  ", "claude-fable-5-2"),
            ("gpt-4o", "gpt-4o"),
        ] {
            assert_eq!(canonical_model_id(raw), expected, "{raw}");
        }
    }

    #[test]
    fn current_claude_models_are_priced_by_dated_and_prefixed_ids() {
        let table = PricingTable::current();
        let opus_55 = table.get("claude-opus-5-5").expect("Opus 5.5");
        assert!((opus_55.input_per_million - 4.0).abs() < 1e-9);
        assert!((opus_55.output_per_million - 20.0).abs() < 1e-9);
        assert!((opus_55.cache_read_per_million - 0.20).abs() < 1e-9);

        // Opus 4.5 is $5/$25, not the Opus 4.1 price.
        let opus_45 = table
            .get("us.anthropic.claude-opus-4-5-20251101-v1:0")
            .expect("Opus 4.5 via Bedrock id");
        assert!((opus_45.input_per_million - 5.0).abs() < 1e-9);
        assert!((opus_45.output_per_million - 25.0).abs() < 1e-9);

        let sonnet = table
            .get("claude-sonnet-4-6-20260217")
            .expect("dated Sonnet 4.6");
        assert!((sonnet.output_per_million - 15.0).abs() < 1e-9);
        assert!(table.get("claude-fable-5-2").is_some());
        assert!(table.get("claude-haiku-5-5").is_some());
    }

    #[test]
    fn response_cost_prices_cache_ttls_fast_mode_and_us_inference() {
        let table = PricingTable::current();
        let tokens = ResponseTokens {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            ..ResponseTokens::default()
        };
        // Opus 5.5: $4 + $20 + $0.20 + $5 + $8.
        let base = table.response_cost("claude-opus-5-5", &tokens).unwrap();
        assert!((base - 37.2).abs() < 1e-9, "{base}");

        let fast = ResponseTokens {
            fast: true,
            ..tokens
        };
        let fast_cost = table.response_cost("claude-opus-5-5", &fast).unwrap();
        assert!((fast_cost - 74.4).abs() < 1e-9, "{fast_cost}");

        let us = ResponseTokens {
            us_only_inference: true,
            ..tokens
        };
        let us_cost = table.response_cost("claude-opus-5-5", &us).unwrap();
        assert!((us_cost - 40.92).abs() < 1e-9, "{us_cost}");

        assert!(table.response_cost("claude-imaginary-9", &tokens).is_none());
    }

    #[test]
    fn haiku_5_5_long_prompts_use_the_higher_rates() {
        let table = PricingTable::current();
        let short = ResponseTokens {
            input: 100_000,
            output: 1_000,
            ..ResponseTokens::default()
        };
        // 100,000 x $0.10 + 1,000 x $0.50 (per million).
        let short_cost = table.response_cost("claude-haiku-5-5", &short).unwrap();
        assert!((short_cost - 0.0105).abs() < 1e-12, "{short_cost}");

        // One cache-read token tips the prompt over 100,000: every token pays the
        // long-prompt rates ($0.50 input, $2.50 output, $0.05 cache read).
        let long = ResponseTokens {
            cache_read: 1,
            ..short
        };
        let long_cost = table.response_cost("claude-haiku-5-5", &long).unwrap();
        let expected = 100_000.0 * 0.50 / 1e6 + 1_000.0 * 2.50 / 1e6 + 0.05 / 1e6;
        assert!((long_cost - expected).abs() < 1e-12, "{long_cost}");
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
