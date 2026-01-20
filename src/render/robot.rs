//! Robot-mode output (JSON and Markdown).
//!
//! Provides stable, token-efficient output for AI agents.

use crate::core::models::{CostPayload, ProviderPayload, RobotOutput};
use crate::error::Result;

/// Render any RobotOutput as JSON.
pub fn render_json<T: serde::Serialize>(output: &T) -> Result<String> {
    Ok(serde_json::to_string(output)?)
}

/// Render any RobotOutput as pretty JSON.
pub fn render_json_pretty<T: serde::Serialize>(output: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(output)?)
}

/// Render usage as JSON (legacy).
pub fn render_usage_json(results: &[ProviderPayload], pretty: bool) -> Result<String> {
    let output = RobotOutput::usage(results.to_vec(), vec![]);

    if pretty {
        render_json_pretty(&output)
    } else {
        render_json(&output)
    }
}

/// Render usage as Markdown.
pub fn render_markdown_usage(results: &[ProviderPayload]) -> Result<String> {
    render_usage_md(results)
}

/// Render usage as Markdown (legacy).
pub fn render_usage_md(results: &[ProviderPayload]) -> Result<String> {
    let mut output = String::new();

    for payload in results {
        output.push_str(&format!("## {} ({})\n", payload.provider, payload.source));

        if let Some(primary) = &payload.usage.primary {
            output.push_str(&format!(
                "- session_left: {:.0}%\n",
                primary.remaining_percent()
            ));
            if let Some(resets_at) = &primary.resets_at {
                output.push_str(&format!("- resets_session: {}\n", resets_at));
            }
        }

        if let Some(secondary) = &payload.usage.secondary {
            output.push_str(&format!(
                "- weekly_left: {:.0}%\n",
                secondary.remaining_percent()
            ));
            if let Some(resets_at) = &secondary.resets_at {
                output.push_str(&format!("- resets_weekly: {}\n", resets_at));
            }
        }

        if let Some(credits) = &payload.credits {
            output.push_str(&format!("- credits_left: {:.1}\n", credits.remaining));
        }

        if let Some(status) = &payload.status {
            output.push_str(&format!("- status: {:?}\n", status.indicator));
        }

        output.push('\n');
    }

    Ok(output)
}

/// Render cost as JSON.
pub fn render_cost_json(results: &[CostPayload], pretty: bool) -> Result<String> {
    let output = RobotOutput::new("cost", results);

    let json = if pretty {
        serde_json::to_string_pretty(&output)?
    } else {
        serde_json::to_string(&output)?
    };

    Ok(json)
}

/// Render cost as Markdown (public interface).
pub fn render_markdown_cost(results: &[CostPayload]) -> Result<String> {
    render_cost_md(results)
}

/// Render cost as Markdown.
pub fn render_cost_md(results: &[CostPayload]) -> Result<String> {
    let mut output = String::new();

    for payload in results {
        output.push_str(&format!(
            "## {} Cost ({})\n\n",
            payload.provider, payload.source
        ));

        // Summary section
        output.push_str("### Summary\n");

        // Today's usage
        match (payload.session_cost_usd, payload.session_tokens) {
            (Some(cost), Some(tokens)) => {
                output.push_str(&format!("- today_cost_usd: {:.2}\n", cost));
                output.push_str(&format!("- today_messages: {}\n", tokens));
            }
            (Some(cost), None) => {
                output.push_str(&format!("- today_cost_usd: {:.2}\n", cost));
            }
            (None, Some(tokens)) => {
                output.push_str(&format!("- today_messages: {}\n", tokens));
            }
            (None, None) => {
                output.push_str("- today: no_activity\n");
            }
        }

        // Last 30 days
        match (payload.last_30_days_cost_usd, payload.last_30_days_tokens) {
            (Some(cost), Some(tokens)) => {
                output.push_str(&format!("- last_30d_cost_usd: {:.2}\n", cost));
                output.push_str(&format!("- last_30d_messages: {}\n", tokens));
            }
            (Some(cost), None) => {
                output.push_str(&format!("- last_30d_cost_usd: {:.2}\n", cost));
            }
            (None, Some(tokens)) => {
                output.push_str(&format!("- last_30d_messages: {}\n", tokens));
            }
            (None, None) => {
                output.push_str("- last_30d: no_activity\n");
            }
        }

        // Totals section (if available with token breakdown)
        if let Some(totals) = &payload.totals {
            let has_token_breakdown = totals.input_tokens.is_some()
                || totals.output_tokens.is_some()
                || totals.cache_read_tokens.is_some();

            if has_token_breakdown {
                output.push_str("\n### Token Breakdown\n");
                if let Some(input) = totals.input_tokens {
                    output.push_str(&format!("- input_tokens: {}\n", input));
                }
                if let Some(out) = totals.output_tokens {
                    output.push_str(&format!("- output_tokens: {}\n", out));
                }
                if let Some(cache_read) = totals.cache_read_tokens {
                    output.push_str(&format!("- cache_read_tokens: {}\n", cache_read));
                }
                if let Some(cache_create) = totals.cache_creation_tokens {
                    output.push_str(&format!("- cache_creation_tokens: {}\n", cache_create));
                }
                if let Some(total) = totals.total_tokens {
                    output.push_str(&format!("- total_tokens: {}\n", total));
                }
                if let Some(cost) = totals.total_cost {
                    output.push_str(&format!("- total_cost_usd: {:.2}\n", cost));
                }
            }
        }

        // Daily breakdown (last 7 days for token efficiency)
        if !payload.daily.is_empty() {
            output.push_str("\n### Daily (last 7 days)\n");
            output.push_str("| date | messages | cost |\n");
            output.push_str("|------|----------|------|\n");

            for entry in payload.daily.iter().take(7) {
                let messages = entry
                    .total_tokens
                    .map_or("-".to_string(), |t| t.to_string());
                let cost = entry
                    .total_cost
                    .map_or("-".to_string(), |c| format!("${:.2}", c));
                output.push_str(&format!("| {} | {} | {} |\n", entry.date, messages, cost));
            }
        }

        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::RobotOutput;
    use crate::test_utils::{
        make_test_cost_payload, make_test_cost_payload_minimal, make_test_provider_payload,
        make_test_provider_payload_minimal, make_test_usage_snapshot_with_tertiary,
    };
    use crate::{assert_contains, assert_json_valid, assert_not_contains};

    // =========================================================================
    // RobotOutput Envelope Tests
    // =========================================================================

    #[test]
    fn usage_json_has_schema_version() {
        let mut payload = make_test_provider_payload_minimal("codex", "openai-web");
        payload.version = Some("0.6.0".to_string());

        let json = render_usage_json(&[payload], false).unwrap();
        assert_json_valid!(&json);
        assert_contains!(&json, "caut.v1");
    }

    #[test]
    fn envelope_has_generated_at_timestamp() {
        let payload = make_test_provider_payload_minimal("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Check generatedAt exists and is valid ISO 8601 timestamp (camelCase due to serde rename)
        let generated_at = parsed["generatedAt"].as_str().unwrap();
        assert!(generated_at.contains("T")); // ISO 8601 format
        assert!(generated_at.ends_with('Z') || generated_at.contains('+')); // UTC or timezone
    }

    #[test]
    fn envelope_has_command_field() {
        let payload = make_test_provider_payload_minimal("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["command"].as_str().unwrap(), "usage");
    }

    #[test]
    fn cost_envelope_has_command_field() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["command"].as_str().unwrap(), "cost");
    }

    #[test]
    fn envelope_has_meta_section() {
        let payload = make_test_provider_payload_minimal("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["meta"].is_object());
        assert!(parsed["meta"]["format"].is_string());
        assert!(parsed["meta"]["runtime"].is_string());
        assert!(parsed["meta"]["flags"].is_array());
    }

    // =========================================================================
    // Provider Serialization Tests
    // =========================================================================

    #[test]
    fn provider_payload_serializes_all_fields() {
        let payload = make_test_provider_payload("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let data = &parsed["data"][0];
        assert_eq!(data["provider"].as_str().unwrap(), "codex");
        assert_eq!(data["source"].as_str().unwrap(), "cli");
        assert!(data["account"].is_string());
        assert!(data["version"].is_string());
        assert!(data["status"].is_object());
        assert!(data["usage"].is_object());
        assert!(data["credits"].is_object()); // Codex has credits
    }

    #[test]
    fn provider_payload_omits_null_optional_fields() {
        let payload = make_test_provider_payload_minimal("claude", "oauth");

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let data = &parsed["data"][0];
        // Optional fields should be null or not present
        assert!(
            data["account"].is_null()
                || !parsed["data"][0]
                    .as_object()
                    .unwrap()
                    .contains_key("account")
        );
        assert!(data["credits"].is_null());
        assert!(data["status"].is_null());
    }

    #[test]
    fn usage_snapshot_uses_camel_case_fields() {
        let payload = make_test_provider_payload("claude", "oauth");

        let json = render_usage_json(&[payload], true).unwrap();

        // Check for camelCase field names
        assert_contains!(&json, "\"usedPercent\"");
        assert_contains!(&json, "\"windowMinutes\"");
        assert_contains!(&json, "\"resetsAt\"");
        assert_contains!(&json, "\"resetDescription\"");
        assert_contains!(&json, "\"updatedAt\"");
    }

    #[test]
    fn credits_snapshot_uses_camel_case_fields() {
        let payload = make_test_provider_payload("codex", "cli");

        let json = render_usage_json(&[payload], true).unwrap();

        assert_contains!(&json, "\"remaining\"");
        assert_contains!(&json, "\"events\"");
        assert_contains!(&json, "\"updatedAt\"");
    }

    #[test]
    fn provider_with_tertiary_rate_window() {
        let mut payload = make_test_provider_payload("claude", "oauth");
        payload.usage = make_test_usage_snapshot_with_tertiary();

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let usage = &parsed["data"][0]["usage"];
        assert!(usage["primary"].is_object());
        assert!(usage["secondary"].is_object());
        assert!(usage["tertiary"].is_object());
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[test]
    fn empty_errors_array_when_no_errors() {
        let payload = make_test_provider_payload_minimal("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["errors"].is_array());
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn errors_array_populated_with_robot_output() {
        let providers: Vec<ProviderPayload> = vec![];
        let errors = vec![
            "Failed to fetch from claude: timeout".to_string(),
            "Failed to fetch from codex: auth expired".to_string(),
        ];

        let output = RobotOutput::usage(providers, errors);
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let error_array = parsed["errors"].as_array().unwrap();
        assert_eq!(error_array.len(), 2);
        assert!(error_array[0].as_str().unwrap().contains("claude"));
        assert!(error_array[1].as_str().unwrap().contains("codex"));
    }

    #[test]
    fn partial_success_with_errors() {
        // One provider succeeds, errors from others
        let providers = vec![make_test_provider_payload_minimal("codex", "cli")];
        let errors = vec!["Failed to fetch from claude: timeout".to_string()];

        let output = RobotOutput::usage(providers, errors);
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Should have both data and errors
        assert_eq!(parsed["data"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 1);
    }

    // =========================================================================
    // Pretty vs Compact Tests
    // =========================================================================

    #[test]
    fn usage_json_pretty_is_valid() {
        let payload = make_test_provider_payload("codex", "cli");

        let json = render_usage_json(&[payload], true).unwrap();
        assert_json_valid!(&json);
        // Pretty JSON should have newlines
        assert_contains!(&json, "\n");
    }

    #[test]
    fn usage_json_compact_has_no_newlines() {
        let payload = make_test_provider_payload("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();
        assert_json_valid!(&json);
        // Compact JSON should not have newlines (except in string values)
        // Split on newlines and check we got one line
        let lines: Vec<&str> = json.lines().collect();
        assert_eq!(lines.len(), 1, "Compact JSON should be a single line");
    }

    #[test]
    fn cost_json_pretty_has_indentation() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], true).unwrap();
        assert_json_valid!(&json);
        assert_contains!(&json, "  "); // Has indentation
    }

    #[test]
    fn cost_json_compact_is_single_line() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], false).unwrap();
        assert_json_valid!(&json);

        let lines: Vec<&str> = json.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    // =========================================================================
    // Round-trip Serialization Tests
    // =========================================================================

    #[test]
    fn usage_json_round_trip_deserialize() {
        let payload = make_test_provider_payload("codex", "cli");

        let json = render_usage_json(&[payload], false).unwrap();

        // Should be able to deserialize back to RobotOutput
        let parsed: RobotOutput<Vec<ProviderPayload>> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, "caut.v1");
        assert_eq!(parsed.command, "usage");
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].provider, "codex");
    }

    #[test]
    fn cost_json_round_trip_deserialize() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], false).unwrap();

        let parsed: RobotOutput<Vec<CostPayload>> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, "caut.v1");
        assert_eq!(parsed.command, "cost");
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].provider, "claude");
    }

    // =========================================================================
    // Multi-Provider Tests
    // =========================================================================

    #[test]
    fn multiple_providers_in_data_array() {
        let payloads = vec![
            make_test_provider_payload("codex", "cli"),
            make_test_provider_payload("claude", "oauth"),
        ];

        let json = render_usage_json(&payloads, false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let data = parsed["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["provider"].as_str().unwrap(), "codex");
        assert_eq!(data[1]["provider"].as_str().unwrap(), "claude");
    }

    #[test]
    fn empty_providers_creates_empty_data_array() {
        let payloads: Vec<ProviderPayload> = vec![];

        let json = render_usage_json(&payloads, false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["data"].is_array());
        assert_eq!(parsed["data"].as_array().unwrap().len(), 0);
    }

    // =========================================================================
    // Markdown Format Tests
    // =========================================================================

    #[test]
    fn usage_md_format() {
        let payload = make_test_provider_payload_minimal("claude", "oauth");

        let md = render_usage_md(&[payload]).unwrap();
        assert_contains!(&md, "## claude (oauth)");
        assert_contains!(&md, "session_left: 50%");
    }

    #[test]
    fn usage_md_with_secondary_window() {
        let payload = make_test_provider_payload("claude", "oauth");

        let md = render_usage_md(&[payload]).unwrap();
        assert_contains!(&md, "session_left:");
        assert_contains!(&md, "weekly_left:");
    }

    #[test]
    fn usage_md_with_credits() {
        let payload = make_test_provider_payload("codex", "cli");

        let md = render_usage_md(&[payload]).unwrap();
        assert_contains!(&md, "credits_left:");
    }

    #[test]
    fn usage_md_without_credits() {
        let payload = make_test_provider_payload("claude", "oauth");

        let md = render_usage_md(&[payload]).unwrap();
        assert_not_contains!(&md, "credits_left:");
    }

    #[test]
    fn usage_md_multiple_providers() {
        let payloads = vec![
            make_test_provider_payload("codex", "cli"),
            make_test_provider_payload("claude", "oauth"),
        ];

        let md = render_usage_md(&payloads).unwrap();
        assert_contains!(&md, "## codex (cli)");
        assert_contains!(&md, "## claude (oauth)");
    }

    #[test]
    fn cost_md_format() {
        let payload = make_test_cost_payload("claude");

        let md = render_cost_md(&[payload]).unwrap();
        assert_contains!(&md, "## claude Cost (local)");
        assert_contains!(&md, "### Summary");
        assert_contains!(&md, "today_cost_usd: 2.45");
        assert_contains!(&md, "today_messages: 124500");
        assert_contains!(&md, "last_30d_cost_usd: 47.82");
        assert_contains!(&md, "last_30d_messages: 2400000");
        assert_contains!(&md, "### Daily");
    }

    #[test]
    fn cost_md_with_token_breakdown() {
        let payload = make_test_cost_payload("claude");

        let md = render_cost_md(&[payload]).unwrap();
        assert_contains!(&md, "### Token Breakdown");
        assert_contains!(&md, "input_tokens:");
        assert_contains!(&md, "output_tokens:");
    }

    #[test]
    fn cost_md_minimal_no_activity() {
        let payload = make_test_cost_payload_minimal("claude");

        let md = render_cost_md(&[payload]).unwrap();
        assert_contains!(&md, "today: no_activity");
        assert_contains!(&md, "last_30d: no_activity");
    }

    #[test]
    fn cost_md_daily_table_format() {
        let payload = make_test_cost_payload("claude");

        let md = render_cost_md(&[payload]).unwrap();
        // Check markdown table formatting
        assert_contains!(&md, "| date | messages | cost |");
        assert_contains!(&md, "|------|----------|------|");
    }

    // =========================================================================
    // Cost JSON Tests
    // =========================================================================

    #[test]
    fn cost_json_has_schema_version() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], false).unwrap();
        assert_json_valid!(&json);
        assert_contains!(&json, "caut.v1");
        assert_contains!(&json, "claude");
    }

    #[test]
    fn cost_json_includes_daily_entries() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let daily = &parsed["data"][0]["daily"];
        assert!(daily.is_array());
        assert!(!daily.as_array().unwrap().is_empty());
    }

    #[test]
    fn cost_json_includes_totals() {
        let payload = make_test_cost_payload("claude");

        let json = render_cost_json(&[payload], false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let totals = &parsed["data"][0]["totals"];
        assert!(totals.is_object());
        assert!(totals["inputTokens"].is_number());
        assert!(totals["outputTokens"].is_number());
    }
}
