//! E2E tests for caut history command.
//!
//! Tests the full CLI flow for history management:
//! - history stats
//! - history prune

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use tempfile::TempDir;

// =============================================================================
// Test Helpers
// =============================================================================

/// Get the caut binary command.
/// Handles custom build directory by checking env var or falling back to specific path.
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

/// Setup a test environment with a temporary directory and an initialized DB.
/// Returns the `TempDir` which must be kept alive for the duration of the test.
fn setup_env() -> (Command, TempDir) {
    let temp_dir = TempDir::new().expect("failed to create temp dir");

    // Simulate XDG_DATA_HOME structure: <data_home>/caut/usage-history.sqlite
    let data_home = temp_dir.path();
    let app_data_dir = data_home.join("caut");
    std::fs::create_dir_all(&app_data_dir).expect("failed to create app data dir");

    let db_path = app_data_dir.join("usage-history.sqlite");

    // Create a valid (empty) sqlite database
    {
        let conn = rusqlite::Connection::open(&db_path).expect("failed to open sqlite db");
        // We can optionally create tables here, but HistoryStore::open runs migrations.
        // However, the CLI checks if file exists first.
        // Since HistoryStore::open runs migrations, an empty sqlite file is enough to start.
        drop(conn);
    }

    let mut cmd = caut_cmd();
    cmd.env("XDG_DATA_HOME", data_home)
        .env("XDG_CONFIG_HOME", data_home) // Isolate config as well
        .env("XDG_CACHE_HOME", data_home); // Isolate cache

    (cmd, temp_dir)
}

// =============================================================================
// History Stats Tests
// =============================================================================

#[test]
fn history_stats_human_output() {
    let (mut cmd, _temp) = setup_env();

    cmd.arg("history")
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("History Database Statistics"))
        .stdout(predicate::str::contains("usage-history.sqlite"));
}

#[test]
fn history_stats_json_output() {
    let (mut cmd, _temp) = setup_env();

    let output = cmd
        .arg("history")
        .arg("stats")
        .arg("--json")
        .output()
        .expect("failed to execute");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    assert_eq!(json["command"], "history stats");
    assert_eq!(json["schemaVersion"], "caut.v1");

    // Verify data is present (not null) because DB exists
    assert!(json["data"].is_object());
    assert!(json["data"]["snapshotCount"].is_number());
}

// =============================================================================
// History Prune Tests
// =============================================================================

#[test]
fn history_prune_dry_run() {
    let (mut cmd, _temp) = setup_env();

    cmd.arg("history")
        .arg("prune")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("History Prune Results"))
        .stdout(predicate::str::contains("Dry run"));
}

#[test]
fn history_prune_json_output() {
    let (mut cmd, _temp) = setup_env();

    let output = cmd
        .arg("history")
        .arg("prune")
        .arg("--dry-run")
        .arg("--json")
        .output()
        .expect("failed to execute");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    assert_eq!(json["command"], "history prune");
    assert_eq!(json["data"]["dryRun"], true);
}

// =============================================================================
// History Show Tests
// =============================================================================

#[test]
fn history_show_human_output() {
    let (mut cmd, _temp) = setup_env();

    // With empty database, should show "no data" message
    cmd.arg("history")
        .arg("show")
        .assert()
        .success()
        .stdout(predicate::str::contains("No usage data found"));
}

#[test]
fn history_show_json_output() {
    let (mut cmd, _temp) = setup_env();

    let output = cmd
        .arg("history")
        .arg("show")
        .arg("--json")
        .output()
        .expect("failed to execute");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    assert_eq!(json["command"], "history show");
    assert_eq!(json["schemaVersion"], "caut.v1");
    assert!(json["data"]["period"].is_object());
    assert!(json["data"]["providers"].is_array());
}

#[test]
fn history_show_markdown_output() {
    let (mut cmd, _temp) = setup_env();

    cmd.arg("history")
        .arg("show")
        .arg("--format")
        .arg("md")
        .assert()
        .success()
        .stdout(predicate::str::contains("# Usage History"))
        .stdout(predicate::str::contains("**Period:**"));
}

#[test]
fn history_show_with_days_flag() {
    let (mut cmd, _temp) = setup_env();

    let output = cmd
        .arg("history")
        .arg("show")
        .arg("--days")
        .arg("30")
        .arg("--json")
        .output()
        .expect("failed to execute");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    assert_eq!(json["data"]["period"]["days"], 30);
}

#[test]
fn history_show_ascii_mode() {
    let (mut cmd, _temp) = setup_env();

    // ASCII mode should work without errors
    cmd.arg("history")
        .arg("show")
        .arg("--ascii")
        .assert()
        .success();
}
