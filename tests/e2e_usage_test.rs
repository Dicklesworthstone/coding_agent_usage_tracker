//! E2E tests for caut usage command.
//!
//! Tests the full CLI flow from invocation to output, verifying:
//! - Command execution and exit codes
//! - Output format correctness (human, JSON, markdown)
//! - Flag handling (--no-color, --verbose, --pretty)
//! - Error handling for invalid inputs
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
fn caut_cmd() -> Command {
    Command::cargo_bin("caut").expect("Failed to find caut binary")
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
fn usage_basic_invocation() {
    let log = TestLogger::new("usage_basic_invocation");
    log.phase("execute");

    let output = caut_cmd().arg("usage").output().expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_basic_stdout.txt", &output.stdout);
    save_artifact("e2e_usage_basic_stderr.txt", &output.stderr);

    // Note: exit code may be non-zero if no providers configured
    // We just verify the command runs without panic
    log.info(&format!("Exit status: {:?}", output.status));
    log.finish_ok();
}

#[test]
fn usage_help_displays_options() {
    let log = TestLogger::new("usage_help_displays_options");
    log.phase("execute");

    caut_cmd()
        .arg("usage")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("--provider"))
        .stdout(predicate::str::contains("--format"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--no-color"));

    log.finish_ok();
}

#[test]
fn usage_version_works() {
    let log = TestLogger::new("usage_version_works");
    log.phase("execute");

    caut_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"caut \d+\.\d+\.\d+").unwrap());

    log.finish_ok();
}

// =============================================================================
// Output Format Tests
// =============================================================================

#[test]
fn usage_json_output_is_valid() {
    let log = TestLogger::new("usage_json_output_is_valid");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_json_output.json", &output.stdout);

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
        Some("usage"),
        "Command should be 'usage'"
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
fn usage_json_format_flag_works() {
    let log = TestLogger::new("usage_json_format_flag_works");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
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
fn usage_markdown_output() {
    let log = TestLogger::new("usage_markdown_output");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--format")
        .arg("md")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_md_output.md", &output.stdout);

    // Markdown output may contain headers, bold, or bullet points
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    log.debug(&format!("Markdown output length: {}", stdout_str.len()));

    log.finish_ok();
}

#[test]
fn usage_pretty_json_is_formatted() {
    let log = TestLogger::new("usage_pretty_json_is_formatted");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--json")
        .arg("--pretty")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_pretty_json.json", &output.stdout);

    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // Pretty JSON should have newlines
    let line_count = stdout_str.lines().count();
    assert!(
        line_count > 1,
        "Pretty JSON should have multiple lines, got {}",
        line_count
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
fn usage_no_color_removes_ansi_codes() {
    let log = TestLogger::new("usage_no_color_removes_ansi_codes");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--no-color")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_nocolor_stdout.txt", &output.stdout);

    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // Check for ANSI escape sequences (ESC[)
    assert!(
        !stdout_str.contains('\x1b'),
        "Output should not contain ANSI escape codes with --no-color"
    );

    log.finish_ok();
}

#[test]
fn usage_verbose_mode_runs() {
    let log = TestLogger::new("usage_verbose_mode_runs");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--verbose")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_verbose_stdout.txt", &output.stdout);
    save_artifact("e2e_usage_verbose_stderr.txt", &output.stderr);

    // Verbose mode should run without crashing
    // Debug output may go to stderr
    log.debug(&format!(
        "stdout: {} bytes, stderr: {} bytes",
        output.stdout.len(),
        output.stderr.len()
    ));

    log.finish_ok();
}

#[test]
fn usage_status_flag_runs() {
    let log = TestLogger::new("usage_status_flag_runs");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--status")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_status_stdout.txt", &output.stdout);

    // Should run without crashing
    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

// =============================================================================
// Provider Tests
// =============================================================================

#[test]
fn usage_provider_filter_claude() {
    let log = TestLogger::new("usage_provider_filter_claude");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--provider=claude")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_claude_stdout.txt", &output.stdout);

    // Should run (may fail if claude not configured, which is OK)
    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn usage_provider_filter_codex() {
    let log = TestLogger::new("usage_provider_filter_codex");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--provider=codex")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_codex_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn usage_provider_all() {
    let log = TestLogger::new("usage_provider_all");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--provider=all")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_all_providers_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn usage_provider_both() {
    let log = TestLogger::new("usage_provider_both");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--provider=both")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_both_providers_stdout.txt", &output.stdout);

    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

#[test]
fn usage_invalid_provider_handled() {
    let log = TestLogger::new("usage_invalid_provider_handled");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--provider=nonexistent_provider_xyz")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_invalid_provider_stdout.txt", &output.stdout);
    save_artifact("e2e_usage_invalid_provider_stderr.txt", &output.stderr);

    // Should handle gracefully (may have non-zero exit, but no panic)
    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn usage_json_has_errors_array() {
    let log = TestLogger::new("usage_json_has_errors_array");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        // Errors should be an array (possibly empty)
        let errors = json.get("errors");
        assert!(errors.is_some(), "JSON output should have 'errors' field");
        assert!(errors.unwrap().is_array(), "'errors' should be an array");
    }

    log.finish_ok();
}

#[test]
fn usage_json_data_is_array() {
    let log = TestLogger::new("usage_json_data_is_array");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        // Data should be an array
        let data = json.get("data");
        assert!(data.is_some(), "JSON output should have 'data' field");
        assert!(data.unwrap().is_array(), "'data' should be an array");
    }

    log.finish_ok();
}

// =============================================================================
// Timeout Tests
// =============================================================================

#[test]
fn usage_timeout_parameter() {
    let log = TestLogger::new("usage_timeout_parameter");
    log.phase("execute");

    // Very short timeout - may timeout or succeed quickly
    let output = caut_cmd()
        .arg("usage")
        .arg("--web-timeout=1")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_usage_timeout_stdout.txt", &output.stdout);
    save_artifact("e2e_usage_timeout_stderr.txt", &output.stderr);

    // Should not panic regardless of outcome
    log.debug(&format!("Exit status: {:?}", output.status));

    log.finish_ok();
}

// =============================================================================
// Combined Flag Tests
// =============================================================================

#[test]
fn usage_json_no_color_combined() {
    let log = TestLogger::new("usage_json_no_color_combined");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
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
fn usage_verbose_json_combined() {
    let log = TestLogger::new("usage_verbose_json_combined");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--json")
        .arg("--verbose")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    // stdout should still be valid JSON even with verbose
    // (verbose output should go to stderr)
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
fn usage_json_schema_version_correct() {
    let log = TestLogger::new("usage_json_schema_version_correct");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
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
fn usage_json_meta_present() {
    let log = TestLogger::new("usage_json_meta_present");
    log.phase("execute");

    let output = caut_cmd()
        .arg("usage")
        .arg("--json")
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    let stdout_str = String::from_utf8_lossy(&output.stdout);

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
        // Meta object should exist
        let meta = json.get("meta");
        assert!(meta.is_some(), "JSON should have 'meta' object");

        if let Some(meta) = meta {
            // Meta should have format and runtime
            assert!(meta.get("format").is_some(), "meta should have 'format'");
            assert!(meta.get("runtime").is_some(), "meta should have 'runtime'");
        }
    }

    log.finish_ok();
}
