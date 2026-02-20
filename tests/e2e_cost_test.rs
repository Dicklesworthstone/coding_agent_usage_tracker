//! E2E tests for caut cost command.
//!
//! Tests the full CLI flow from invocation to output, verifying:
//! - Command execution and exit codes
//! - Output format correctness (human, JSON, markdown)
//! - Cost value validation (non-negative)
//! - Token count validation (integer)
//! - Flag handling (--no-color, --verbose, --pretty, --refresh)
//!
//! These tests run against the compiled binary and verify real CLI behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;

mod common;

use common::logger::TestLogger;

// =============================================================================
// Test Helpers
// =============================================================================

/// Get the caut binary command.
#[allow(deprecated)]
fn caut_cmd() -> Command {
    // Try standard cargo_bin first
    if let Ok(cmd) = Command::cargo_bin("caut") {
        return cmd;
    }

    // Fallback to hardcoded path seen in environment
    let path = PathBuf::from("/tmp/cargo-target/debug/caut");
    if path.exists() {
        return Command::new(path);
    }

    panic!("Could not find caut binary");
}

/// Get the path to the test artifacts directory.
fn artifacts_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-logs/artifacts");
    fs::create_dir_all(&dir).expect("Failed to create artifacts directory");
    dir
}

/// Save command output to an artifact file.
fn save_artifact(name: &str, content: &[u8]) {
    let path = artifacts_dir().join(name);
    fs::write(&path, content).expect("Failed to write artifact");
}

// =============================================================================
// Basic Invocation Tests
// =============================================================================

#[test]
fn cost_basic_invocation() {
    let log = TestLogger::new("cost_basic_invocation");
    log.phase("execute");

    let output = caut_cmd().arg("cost").output().expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_basic_stdout.txt", &output.stdout);
    save_artifact("e2e_cost_basic_stderr.txt", &output.stderr);

    // Note: exit code may be non-zero if no cost data available
    log.info(&format!("Exit status: {:?}", output.status));
    log.finish_ok();
}

#[test]
fn cost_help_displays_options() {
    let log = TestLogger::new("cost_help_displays_options");
    log.phase("execute");

    caut_cmd()
        .arg("cost")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("--provider"))
        .stdout(predicate::str::contains("--format"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--no-color"))
        .stdout(predicate::str::contains("--refresh"));

    log.finish_ok();
}

// =============================================================================
// Output Format Tests
// =============================================================================

#[test]
fn cost_json_output_is_valid() {
    let log = TestLogger::new("cost_json_output_is_valid");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_json_output.json", &output.stdout);

    // Parse as JSON
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout_str).expect("Output should be valid JSON");

    // Verify schema structure
    assert!(
        json.get("schemaVersion").is_some(),
        "Missing schemaVersion field"
    );
    assert_eq!(
        json.get("command").and_then(|v| v.as_str()),
        Some("cost"),
        "Command should be 'cost'"
    );
    assert!(
        json.get("generatedAt").is_some(),
        "Missing generatedAt timestamp"
    );
    assert!(json.get("data").is_some(), "Missing data field");
    assert!(json.get("errors").is_some(), "Missing errors field");

    log.finish_ok();
}

#[test]
fn cost_json_format_flag_works() {
    let log = TestLogger::new("cost_json_format_flag_works");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--format")
        .arg("json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // Should be valid JSON
    let result: Result<serde_json::Value, _> = serde_json::from_str(&stdout_str);
    assert!(
        result.is_ok(),
        "Output should be valid JSON with --format json"
    );

    log.finish_ok();
}

#[test]
fn cost_markdown_output() {
    let log = TestLogger::new("cost_markdown_output");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--format")
        .arg("md")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_md_output.md", &output.stdout);

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    log.debug(&format!("Markdown output length: {}", stdout_str.len()));

    log.finish_ok();
}

#[test]
fn cost_pretty_json_is_formatted() {
    let log = TestLogger::new("cost_pretty_json_is_formatted");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .arg("--pretty")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_pretty_json.json", &output.stdout);

    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // Pretty JSON should have newlines
    let line_count = stdout_str.lines().count();
    assert!(
        line_count > 1,
        "Pretty JSON should have multiple lines, got {line_count}"
    );

    // Should still be valid JSON
    let result: Result<serde_json::Value, _> = serde_json::from_str(&stdout_str);
    assert!(result.is_ok(), "Pretty JSON should still be valid JSON");

    log.finish_ok();
}

// =============================================================================
// Flag Tests
// =============================================================================

#[test]
fn cost_no_color_removes_ansi_codes() {
    let log = TestLogger::new("cost_no_color_removes_ansi_codes");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--no-color")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_nocolor_stdout.txt", &output.stdout);

    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // Check for ANSI escape sequences
    assert!(
        !stdout_str.contains('\x1b'),
        "Output should not contain ANSI escape codes with --no-color"
    );

    log.finish_ok();
}

#[test]
fn cost_verbose_mode_runs() {
    let log = TestLogger::new("cost_verbose_mode_runs");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--verbose")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_verbose_stdout.txt", &output.stdout);
    save_artifact("e2e_cost_verbose_stderr.txt", &output.stderr);

    log.debug(&format!(
        "stdout: {} bytes, stderr: {} bytes",
        output.stdout.len(),
        output.stderr.len()
    ));

    log.finish_ok();
}

#[test]
fn cost_refresh_flag_runs() {
    let log = TestLogger::new("cost_refresh_flag_runs");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--refresh")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_refresh_stdout.txt", &output.stdout);

    // Should run without crashing
    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

// =============================================================================
// Provider Tests
// =============================================================================

#[test]
fn cost_provider_filter_claude() {
    let log = TestLogger::new("cost_provider_filter_claude");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--provider=claude")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_claude_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn cost_provider_filter_codex() {
    let log = TestLogger::new("cost_provider_filter_codex");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--provider=codex")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_codex_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn cost_provider_all() {
    let log = TestLogger::new("cost_provider_all");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--provider=all")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_all_providers_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn cost_provider_both() {
    let log = TestLogger::new("cost_provider_both");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--provider=both")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_cost_both_providers_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

// =============================================================================
// Value Validation Tests
// =============================================================================

#[test]
fn cost_json_session_cost_is_non_negative() {
    let log = TestLogger::new("cost_json_session_cost_is_non_negative");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str)
        && let Some(data) = json.get("data").and_then(|d| d.as_array())
    {
        for provider in data {
            if let Some(cost) = provider
                .get("sessionCostUsd")
                .and_then(serde_json::Value::as_f64)
            {
                assert!(
                    cost >= 0.0,
                    "Session cost should be non-negative, got {cost}"
                );
            }
        }
    }

    log.finish_ok();
}

#[test]
fn cost_json_monthly_cost_is_non_negative() {
    let log = TestLogger::new("cost_json_monthly_cost_is_non_negative");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str)
        && let Some(data) = json.get("data").and_then(|d| d.as_array())
    {
        for provider in data {
            if let Some(cost) = provider
                .get("last30DaysCostUsd")
                .and_then(serde_json::Value::as_f64)
            {
                assert!(
                    cost >= 0.0,
                    "Monthly cost should be non-negative, got {cost}"
                );
            }
        }
    }

    log.finish_ok();
}

#[test]
fn cost_json_token_counts_are_integers() {
    let log = TestLogger::new("cost_json_token_counts_are_integers");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str)
        && let Some(data) = json.get("data").and_then(|d| d.as_array())
    {
        for provider in data {
            // Check session tokens
            if let Some(tokens) = provider.get("sessionTokens")
                && !tokens.is_null()
            {
                assert!(
                    tokens.is_i64() || tokens.is_u64(),
                    "Session tokens should be an integer, got {tokens:?}"
                );
                let count = tokens.as_i64().unwrap_or(0);
                assert!(
                    count >= 0,
                    "Token count should be non-negative, got {count}"
                );
            }

            // Check last 30 days tokens
            if let Some(tokens) = provider.get("last30DaysTokens")
                && !tokens.is_null()
            {
                assert!(
                    tokens.is_i64() || tokens.is_u64(),
                    "Monthly tokens should be an integer, got {tokens:?}"
                );
            }
        }
    }

    log.finish_ok();
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn cost_json_has_errors_array() {
    let log = TestLogger::new("cost_json_has_errors_array");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        let errors = json.get("errors");
        assert!(errors.is_some(), "JSON output should have 'errors' field");
        assert!(errors.unwrap().is_array(), "'errors' should be an array");
    }

    log.finish_ok();
}

#[test]
fn cost_json_data_is_array() {
    let log = TestLogger::new("cost_json_data_is_array");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        let data = json.get("data");
        assert!(data.is_some(), "JSON output should have 'data' field");
        assert!(data.unwrap().is_array(), "'data' should be an array");
    }

    log.finish_ok();
}

// =============================================================================
// Combined Flag Tests
// =============================================================================

#[test]
fn cost_json_no_color_combined() {
    let log = TestLogger::new("cost_json_no_color_combined");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .arg("--no-color")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // Should be valid JSON
    let result: Result<serde_json::Value, _> = serde_json::from_str(&stdout_str);
    assert!(result.is_ok(), "Combined flags should produce valid JSON");

    // No ANSI codes
    assert!(!stdout_str.contains('\x1b'), "No ANSI codes in JSON output");

    log.finish_ok();
}

#[test]
fn cost_verbose_json_combined() {
    let log = TestLogger::new("cost_verbose_json_combined");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .arg("--verbose")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // stdout should still be valid JSON even with verbose
    let result: Result<serde_json::Value, _> = serde_json::from_str(&stdout_str);
    assert!(
        result.is_ok(),
        "JSON output should be valid even with --verbose"
    );

    log.finish_ok();
}

// =============================================================================
// Schema Verification Tests
// =============================================================================

#[test]
fn cost_json_schema_version_correct() {
    let log = TestLogger::new("cost_json_schema_version_correct");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        let schema_version = json
            .get("schemaVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        assert_eq!(
            schema_version, "caut.v1",
            "Schema version should be 'caut.v1'"
        );
    }

    log.finish_ok();
}

#[test]
fn cost_json_meta_present() {
    let log = TestLogger::new("cost_json_meta_present");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        let meta = json.get("meta");
        assert!(meta.is_some(), "JSON should have 'meta' object");

        if let Some(meta) = meta {
            assert!(meta.get("format").is_some(), "meta should have 'format'");
            assert!(meta.get("runtime").is_some(), "meta should have 'runtime'");
        }
    }

    log.finish_ok();
}

// =============================================================================
// Daily Breakdown Tests
// =============================================================================

#[test]
fn cost_json_daily_entries_valid() {
    let log = TestLogger::new("cost_json_daily_entries_valid");
    log.phase("execute");

    let output = caut_cmd()
        .arg("cost")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str)
        && let Some(data) = json.get("data").and_then(|d| d.as_array())
    {
        for provider in data {
            if let Some(daily) = provider.get("daily").and_then(|d| d.as_array()) {
                for entry in daily {
                    // Check date format (YYYY-MM-DD)
                    if let Some(date) = entry.get("date").and_then(|d| d.as_str()) {
                        let parts: Vec<&str> = date.split('-').collect();
                        assert_eq!(parts.len(), 3, "Date should be YYYY-MM-DD format");
                        // Verify year, month, day are numeric
                        for part in parts {
                            assert!(
                                part.chars().all(|c| c.is_ascii_digit()),
                                "Date parts should be numeric"
                            );
                        }
                    }

                    // Check total cost is non-negative
                    if let Some(cost) = entry.get("totalCost").and_then(serde_json::Value::as_f64) {
                        assert!(cost >= 0.0, "Daily cost should be non-negative");
                    }
                }
            }
        }
    }

    log.finish_ok();
}
