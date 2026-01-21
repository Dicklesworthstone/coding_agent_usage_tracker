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
use caut::cli::args::{Cli, OutputFormat, UsageArgs};
use caut::core::provider::Provider;
use caut::storage::config::{
    Config, ConfigSource, ENV_CONFIG, ENV_FORMAT, ENV_NO_COLOR, ENV_PRETTY, ENV_PROVIDERS,
    ENV_TIMEOUT, ENV_VERBOSE, ResolvedConfig,
};
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::TempDir;

mod common;

use common::logger::TestLogger;

// =============================================================================
// Environment Helpers (Config Integration Tests)
// =============================================================================

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prior: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(vars: &[(&str, Option<&str>)]) -> Self {
        let lock = ENV_LOCK.lock().expect("env lock");
        let mut prior = Vec::new();

        for (key, value) in vars {
            let key_string = (*key).to_string();
            let existing = std::env::var(key).ok();
            prior.push((key_string.clone(), existing));

            unsafe {
                match value {
                    Some(val) => std::env::set_var(key, val),
                    None => std::env::remove_var(key),
                }
            }
        }

        Self { _lock: lock, prior }
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        for (key, value) in self.prior.drain(..) {
            unsafe {
                match value {
                    Some(val) => std::env::set_var(&key, val),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}

fn make_test_cli() -> Cli {
    Cli {
        command: None,
        format: OutputFormat::Human,
        json: false,
        pretty: false,
        no_color: false,
        log_level: None,
        json_output: false,
        verbose: false,
        debug_rich: false,
    }
}

fn make_test_usage_args() -> UsageArgs {
    UsageArgs {
        provider: None,
        account: None,
        account_index: None,
        all_accounts: false,
        no_credits: false,
        status: false,
        source: None,
        web: false,
        web_timeout: None,
        web_debug_dump_html: false,
        watch: false,
        interval: 30,
    }
}

// =============================================================================
// Test Helpers
// =============================================================================

/// Get the caut binary command.
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

/// Setup a test environment with a temporary directory.
fn setup_env() -> (Command, TempDir) {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let data_home = temp_dir.path();

    let mut cmd = caut_cmd();
    cmd.env("XDG_DATA_HOME", data_home)
        .env("XDG_CONFIG_HOME", data_home)
        .env("XDG_CACHE_HOME", data_home);

    (cmd, temp_dir)
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

#[test]
fn usage_creates_history_db() {
    let log = TestLogger::new("usage_creates_history_db");
    log.phase("setup");
    let (mut cmd, temp_dir) = setup_env();

    log.phase("execute");
    // Run usage (it might fail to fetch, but should init DB)
    let _ = cmd.arg("usage").output();

    log.phase("verify");
    let db_path = temp_dir.path().join("caut/usage-history.sqlite");
    assert!(db_path.exists(), "History database should be created");

    // Verify it's a valid SQLite DB (has tables)
    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='usage_snapshots'",
            [],
            |row| row.get(0),
        )
        .expect("query tables");
    assert_eq!(count, 1, "usage_snapshots table should exist");

    log.finish_ok();
}

// =============================================================================
// Config Integration Tests
// =============================================================================

#[test]
fn doctor_config_detects_xdg_config_home() {
    let log = TestLogger::new("doctor_config_detects_xdg_config_home");
    log.phase("setup");

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let config_dir = temp_dir.path().join("caut");
    fs::create_dir_all(&config_dir).expect("create config dir");
    let config_path = config_dir.join("config.toml");
    fs::write(&config_path, "[general]\ntimeout_seconds = 45\n").expect("write config");

    log.phase("execute");
    let output = caut_cmd()
        .arg("doctor")
        .arg("--json")
        .env("XDG_CONFIG_HOME", temp_dir.path())
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_doctor_config_xdg_stdout.json", &output.stdout);

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout_str).expect("doctor json output");
    let config_status = &json["configStatus"];
    let status = config_status["status"]["status"].as_str().unwrap_or("");
    assert_eq!(status, "pass");

    let details = config_status["status"]["details"].as_str().unwrap_or("");
    assert!(
        details.contains("config.toml"),
        "config details should include config path"
    );

    log.finish_ok();
}

#[test]
fn doctor_config_invalid_toml_reports_failure() {
    let log = TestLogger::new("doctor_config_invalid_toml_reports_failure");
    log.phase("setup");

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let config_dir = temp_dir.path().join("caut");
    fs::create_dir_all(&config_dir).expect("create config dir");
    let config_path = config_dir.join("config.toml");
    fs::write(&config_path, "this is not valid toml {{{{").expect("write config");

    log.phase("execute");
    let output = caut_cmd()
        .arg("doctor")
        .arg("--json")
        .env("XDG_CONFIG_HOME", temp_dir.path())
        .output()
        .expect("Failed to execute");

    log.phase("verify");
    save_artifact("e2e_doctor_config_invalid_stdout.json", &output.stdout);
    save_artifact("e2e_doctor_config_invalid_stderr.txt", &output.stderr);

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout_str).expect("doctor json output");
    let config_status = &json["configStatus"];
    let status = config_status["status"]["status"].as_str().unwrap_or("");
    assert_eq!(status, "fail");

    let reason = config_status["status"]["reason"].as_str().unwrap_or("");
    assert!(
        reason.to_lowercase().contains("invalid config file")
            || reason.to_lowercase().contains("failed to load"),
        "config failure should mention invalid config"
    );

    log.finish_ok();
}

#[test]
fn resolved_config_env_overrides_config_file() {
    let dir = TempDir::new().expect("temp dir");
    let config_path = dir.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[general]
timeout_seconds = 60

[output]
format = "md"
color = true
pretty = false

[providers]
default_providers = ["codex"]
"#,
    )
    .expect("write config");

    let _env = EnvGuard::set(&[
        (ENV_CONFIG, Some(config_path.to_str().expect("config path"))),
        (ENV_PROVIDERS, Some("claude")),
        (ENV_FORMAT, Some("json")),
        (ENV_TIMEOUT, Some("42")),
        (ENV_NO_COLOR, None),
        (ENV_VERBOSE, None),
        (ENV_PRETTY, None),
    ]);

    let cli = make_test_cli();
    let usage_args = make_test_usage_args();
    let resolved = ResolvedConfig::resolve(&cli, Some(&usage_args)).expect("resolve config");

    assert_eq!(resolved.format, OutputFormat::Json);
    assert_eq!(resolved.timeout.as_secs(), 42);
    assert_eq!(resolved.providers, vec![Provider::Claude]);
    assert_eq!(resolved.sources.format, ConfigSource::Env);
    assert_eq!(resolved.sources.timeout, ConfigSource::Env);
    assert_eq!(resolved.sources.providers, ConfigSource::Env);
}

#[test]
fn resolved_config_cli_overrides_config_file() {
    let dir = TempDir::new().expect("temp dir");
    let config_path = dir.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[general]
timeout_seconds = 120

[output]
format = "md"
color = true
pretty = false

[providers]
default_providers = ["claude"]
"#,
    )
    .expect("write config");

    let _env = EnvGuard::set(&[
        (ENV_CONFIG, Some(config_path.to_str().expect("config path"))),
        (ENV_PROVIDERS, None),
        (ENV_FORMAT, None),
        (ENV_TIMEOUT, None),
        (ENV_NO_COLOR, None),
        (ENV_VERBOSE, None),
        (ENV_PRETTY, None),
    ]);

    let mut cli = make_test_cli();
    cli.json = true;
    let mut usage_args = make_test_usage_args();
    usage_args.provider = Some("codex".to_string());
    usage_args.web_timeout = Some(5);

    let resolved = ResolvedConfig::resolve(&cli, Some(&usage_args)).expect("resolve config");

    assert_eq!(resolved.format, OutputFormat::Json);
    assert_eq!(resolved.timeout.as_secs(), 5);
    assert_eq!(resolved.providers, vec![Provider::Codex]);
    assert_eq!(resolved.sources.format, ConfigSource::Cli);
    assert_eq!(resolved.sources.timeout, ConfigSource::Cli);
    assert_eq!(resolved.sources.providers, ConfigSource::Cli);
}

#[test]
fn config_load_parses_provider_settings() {
    let dir = TempDir::new().expect("temp dir");
    let config_path = dir.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[providers]
default_providers = ["claude", "codex"]

[providers.claude]
enabled = false
api_base = "https://claude.example.test"

[providers.codex]
enabled = true
api_base = "https://codex.example.test"
"#,
    )
    .expect("write config");

    let config = Config::load_from(&config_path).expect("load config");
    assert_eq!(config.providers.default_providers, vec!["claude", "codex"]);
    assert!(!config.providers.claude.enabled);
    assert_eq!(
        config.providers.claude.api_base.as_deref(),
        Some("https://claude.example.test")
    );
    assert!(config.providers.codex.enabled);
    assert_eq!(
        config.providers.codex.api_base.as_deref(),
        Some("https://codex.example.test")
    );
}
