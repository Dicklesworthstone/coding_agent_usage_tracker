//! Integration tests for Provider → Render pipeline.
//!
//! Tests the full data flow from provider extraction through rendering,
//! verifying that data transforms correctly at each stage.

mod common;

use caut::core::models::{CostPayload, ProviderPayload, RobotOutput};
use caut::render::{human, robot};
use caut::test_utils::{
    make_test_cost_payload, make_test_cost_payload_minimal, make_test_provider_payload,
    make_test_provider_payload_minimal, make_test_usage_snapshot_with_tertiary,
};
use caut::{assert_contains, assert_json_valid};

use common::logger::TestLogger;

// =============================================================================
// Usage Pipeline Tests
// =============================================================================

#[test]
fn usage_pipeline_single_provider_to_human_output() {
    let log = TestLogger::new("usage_pipeline_single_provider_to_human_output");
    log.phase("setup");

    // Stage 1: Create provider payload (simulating provider extraction)
    let provider = "codex";
    let source = "cli";
    let payload = make_test_provider_payload(provider, source);

    log.phase("render_human");
    // Stage 2: Render to human output
    let human_output = human::render_usage(&[payload], false).expect("Human render should succeed");

    log.phase("verify");
    // Stage 3: Verify output contains expected data
    assert_contains!(&human_output, provider);
    assert_contains!(&human_output, source);
    assert_contains!(&human_output, "Session");
    assert_contains!(&human_output, "Credits:");
    log.finish_ok();
}

#[test]
fn usage_pipeline_single_provider_to_robot_json() {
    let log = TestLogger::new("usage_pipeline_single_provider_to_robot_json");
    log.phase("setup");

    let provider = "claude";
    let source = "oauth";
    let payload = make_test_provider_payload(provider, source);

    log.phase("render_robot");
    let json_output =
        robot::render_usage_json(&[payload], false).expect("Robot JSON render should succeed");

    log.phase("verify");
    assert_json_valid!(&json_output);

    // Verify JSON structure
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    assert_eq!(parsed["command"], "usage");
    assert_eq!(parsed["schemaVersion"], "caut.v1");

    let data = &parsed["data"][0];
    assert_eq!(data["provider"], provider);
    assert_eq!(data["source"], source);
    log.finish_ok();
}

#[test]
fn usage_pipeline_single_provider_to_robot_markdown() {
    let log = TestLogger::new("usage_pipeline_single_provider_to_robot_markdown");
    log.phase("setup");

    let provider = "codex";
    let source = "cli";
    let payload = make_test_provider_payload(provider, source);

    log.phase("render_markdown");
    let md_output =
        robot::render_markdown_usage(&[payload]).expect("Markdown render should succeed");

    log.phase("verify");
    assert_contains!(&md_output, &format!("## {provider} ({source})"));
    assert_contains!(&md_output, "session_left:");
    assert_contains!(&md_output, "weekly_left:");
    assert_contains!(&md_output, "credits_left:");
    log.finish_ok();
}

#[test]
fn usage_pipeline_multi_provider_aggregation() {
    let log = TestLogger::new("usage_pipeline_multi_provider_aggregation");
    log.phase("setup");

    // Simulate multiple providers returning data
    let providers = vec![
        make_test_provider_payload("codex", "cli"),
        make_test_provider_payload("claude", "oauth"),
        make_test_provider_payload_minimal("gemini", "web"),
    ];

    log.phase("render_human");
    let human_output =
        human::render_usage(&providers, false).expect("Multi-provider human render should succeed");

    log.phase("verify_human");
    assert_contains!(&human_output, "codex");
    assert_contains!(&human_output, "claude");
    assert_contains!(&human_output, "gemini");

    log.phase("render_robot");
    let json_output = robot::render_usage_json(&providers, false)
        .expect("Multi-provider JSON render should succeed");

    log.phase("verify_robot");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    let data = parsed["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    log.finish_ok();
}

#[test]
fn usage_pipeline_preserves_all_rate_windows() {
    let log = TestLogger::new("usage_pipeline_preserves_all_rate_windows");
    log.phase("setup");

    let mut payload = make_test_provider_payload("claude", "oauth");
    payload.usage = make_test_usage_snapshot_with_tertiary();

    log.phase("render");
    let json_output = robot::render_usage_json(&[payload], false).expect("Render should succeed");

    log.phase("verify");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    let usage = &parsed["data"][0]["usage"];

    assert!(
        usage["primary"].is_object(),
        "Primary window should be present"
    );
    assert!(
        usage["secondary"].is_object(),
        "Secondary window should be present"
    );
    assert!(
        usage["tertiary"].is_object(),
        "Tertiary window should be present"
    );
    log.finish_ok();
}

#[test]
fn usage_pipeline_handles_minimal_data() {
    let log = TestLogger::new("usage_pipeline_handles_minimal_data");
    log.phase("setup");

    let payload = make_test_provider_payload_minimal("test", "minimal");

    log.phase("render_human");
    let human_output = human::render_usage(std::slice::from_ref(&payload), false)
        .expect("Minimal data human render should succeed");
    assert_contains!(&human_output, "test");

    log.phase("render_robot");
    let json_output = robot::render_usage_json(std::slice::from_ref(&payload), false)
        .expect("Minimal data JSON render should succeed");
    assert_json_valid!(&json_output);
    log.finish_ok();
}

// =============================================================================
// Cost Pipeline Tests
// =============================================================================

#[test]
fn cost_pipeline_single_provider_to_human_output() {
    let log = TestLogger::new("cost_pipeline_single_provider_to_human_output");
    log.phase("setup");

    let provider = "claude";
    let payload = make_test_cost_payload(provider);

    log.phase("render_human");
    let human_output =
        human::render_cost(&[payload], false).expect("Cost human render should succeed");

    log.phase("verify");
    assert_contains!(&human_output, &format!("{provider} Cost"));
    assert_contains!(&human_output, "Today:");
    assert_contains!(&human_output, "Last 30 days:");
    log.finish_ok();
}

#[test]
fn cost_pipeline_single_provider_to_robot_json() {
    let log = TestLogger::new("cost_pipeline_single_provider_to_robot_json");
    log.phase("setup");

    let provider = "claude";
    let payload = make_test_cost_payload(provider);

    log.phase("render_robot");
    let json_output =
        robot::render_cost_json(&[payload], false).expect("Cost JSON render should succeed");

    log.phase("verify");
    assert_json_valid!(&json_output);

    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    assert_eq!(parsed["command"], "cost");

    let data = &parsed["data"][0];
    assert_eq!(data["provider"], provider);
    assert!(data["sessionCostUsd"].is_number());
    assert!(data["last30DaysCostUsd"].is_number());
    log.finish_ok();
}

#[test]
fn cost_pipeline_single_provider_to_robot_markdown() {
    let log = TestLogger::new("cost_pipeline_single_provider_to_robot_markdown");
    log.phase("setup");

    let provider = "codex";
    let payload = make_test_cost_payload(provider);

    log.phase("render_markdown");
    let md_output =
        robot::render_markdown_cost(&[payload]).expect("Cost markdown render should succeed");

    log.phase("verify");
    assert_contains!(&md_output, &format!("## {provider} Cost"));
    assert_contains!(&md_output, "### Summary");
    assert_contains!(&md_output, "today_cost_usd:");
    log.finish_ok();
}

#[test]
fn cost_pipeline_multi_provider_aggregation() {
    let log = TestLogger::new("cost_pipeline_multi_provider_aggregation");
    log.phase("setup");

    let providers = vec![
        make_test_cost_payload("claude"),
        make_test_cost_payload("codex"),
    ];

    log.phase("render_human");
    let human_output =
        human::render_cost(&providers, false).expect("Multi-provider cost render should succeed");

    log.phase("verify_human");
    assert_contains!(&human_output, "claude Cost");
    assert_contains!(&human_output, "codex Cost");

    log.phase("render_robot");
    let json_output = robot::render_cost_json(&providers, false)
        .expect("Multi-provider cost JSON render should succeed");

    log.phase("verify_robot");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    let data = parsed["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    log.finish_ok();
}

#[test]
fn cost_pipeline_preserves_daily_breakdown() {
    let log = TestLogger::new("cost_pipeline_preserves_daily_breakdown");
    log.phase("setup");

    let payload = make_test_cost_payload("claude");

    log.phase("render");
    let json_output = robot::render_cost_json(&[payload], false).expect("Render should succeed");

    log.phase("verify");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    let daily = &parsed["data"][0]["daily"];

    assert!(daily.is_array());
    assert!(!daily.as_array().unwrap().is_empty());

    let first_day = &daily[0];
    assert!(first_day["date"].is_string());
    assert!(first_day["totalCost"].is_number());
    log.finish_ok();
}

#[test]
fn cost_pipeline_preserves_totals() {
    let log = TestLogger::new("cost_pipeline_preserves_totals");
    log.phase("setup");

    let payload = make_test_cost_payload("claude");

    log.phase("render");
    let json_output = robot::render_cost_json(&[payload], false).expect("Render should succeed");

    log.phase("verify");
    let parsed: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    let totals = &parsed["data"][0]["totals"];

    assert!(totals.is_object());
    assert!(totals["inputTokens"].is_number());
    assert!(totals["outputTokens"].is_number());
    assert!(totals["totalCost"].is_number());
    log.finish_ok();
}

#[test]
fn cost_pipeline_handles_minimal_data() {
    let log = TestLogger::new("cost_pipeline_handles_minimal_data");
    log.phase("setup");

    let payload = make_test_cost_payload_minimal("test");

    log.phase("render_human");
    let human_output = human::render_cost(std::slice::from_ref(&payload), false)
        .expect("Minimal cost human render should succeed");
    assert_contains!(&human_output, "test Cost");
    assert_contains!(&human_output, "No activity");

    log.phase("render_robot");
    let json_output = robot::render_cost_json(std::slice::from_ref(&payload), false)
        .expect("Minimal cost JSON render should succeed");
    assert_json_valid!(&json_output);
    log.finish_ok();
}

// =============================================================================
// Error Propagation Tests
// =============================================================================

#[test]
fn error_propagation_in_robot_output() {
    let log = TestLogger::new("error_propagation_in_robot_output");
    log.phase("setup");

    let providers: Vec<ProviderPayload> = vec![make_test_provider_payload("codex", "cli")];
    let errors = vec![
        "Failed to fetch from claude: timeout after 30s".to_string(),
        "Failed to fetch from gemini: authentication required".to_string(),
    ];

    log.phase("create_output");
    let output = RobotOutput::usage(providers, errors);

    log.phase("render");
    let json = serde_json::to_string(&output).expect("Serialization should succeed");

    log.phase("verify");
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should have one successful provider
    assert_eq!(parsed["data"].as_array().unwrap().len(), 1);

    // Should have two errors
    let error_array = parsed["errors"].as_array().unwrap();
    assert_eq!(error_array.len(), 2);
    assert!(error_array[0].as_str().unwrap().contains("claude"));
    assert!(error_array[1].as_str().unwrap().contains("gemini"));
    log.finish_ok();
}

#[test]
fn empty_results_with_errors_only() {
    let log = TestLogger::new("empty_results_with_errors_only");
    log.phase("setup");

    let providers: Vec<ProviderPayload> = vec![];
    let errors = vec!["All providers failed".to_string()];

    log.phase("create_output");
    let output = RobotOutput::usage(providers, errors);

    log.phase("render");
    let json = serde_json::to_string(&output).expect("Serialization should succeed");

    log.phase("verify");
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(parsed["data"].as_array().unwrap().is_empty());
    assert_eq!(parsed["errors"].as_array().unwrap().len(), 1);
    log.finish_ok();
}

// =============================================================================
// Round-Trip Serialization Tests
// =============================================================================

#[test]
fn usage_round_trip_serialization() {
    let log = TestLogger::new("usage_round_trip_serialization");
    log.phase("setup");

    let original = make_test_provider_payload("codex", "cli");

    log.phase("serialize");
    let json = robot::render_usage_json(std::slice::from_ref(&original), false)
        .expect("Serialization should succeed");

    log.phase("deserialize");
    let parsed: RobotOutput<Vec<ProviderPayload>> =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    log.phase("verify");
    assert_eq!(parsed.data.len(), 1);
    assert_eq!(parsed.data[0].provider, original.provider);
    assert_eq!(parsed.data[0].source, original.source);
    log.finish_ok();
}

#[test]
fn cost_round_trip_serialization() {
    let log = TestLogger::new("cost_round_trip_serialization");
    log.phase("setup");

    let original = make_test_cost_payload("claude");

    log.phase("serialize");
    let json = robot::render_cost_json(std::slice::from_ref(&original), false)
        .expect("Serialization should succeed");

    log.phase("deserialize");
    let parsed: RobotOutput<Vec<CostPayload>> =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    log.phase("verify");
    assert_eq!(parsed.data.len(), 1);
    assert_eq!(parsed.data[0].provider, original.provider);
    assert_eq!(parsed.data[0].session_cost_usd, original.session_cost_usd);
    log.finish_ok();
}

// =============================================================================
// Cross-Format Consistency Tests
// =============================================================================

#[test]
fn same_data_renders_consistently() {
    let log = TestLogger::new("same_data_renders_consistently");
    log.phase("setup");

    let payload = make_test_provider_payload("codex", "cli");

    log.phase("render_all_formats");
    let human_output = human::render_usage(std::slice::from_ref(&payload), true)
        .expect("Human render should succeed");
    let json_output = robot::render_usage_json(std::slice::from_ref(&payload), false)
        .expect("JSON render should succeed");
    let md_output = robot::render_markdown_usage(std::slice::from_ref(&payload))
        .expect("Markdown render should succeed");

    log.phase("verify");
    // All formats should contain the provider name
    assert_contains!(&human_output, "codex");
    assert_contains!(&json_output, "codex");
    assert_contains!(&md_output, "codex");
    log.finish_ok();
}
