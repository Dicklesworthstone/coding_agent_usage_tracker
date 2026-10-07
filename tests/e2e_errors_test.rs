//! E2E tests for caut error scenarios and edge cases.
//!
//! Covers:
//! - Invalid command handling
//! - Invalid provider handling
//! - Conflicting flags
//! - Help/version output
//! - Corrupted config behavior (no panic)

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

mod common;

use common::logger::TestLogger;

#[test]
#[allow(deprecated)]
fn invalid_command_is_rejected() {
    let log = TestLogger::new("invalid_command_is_rejected");
    log.phase("execute");

    Command::cargo_bin("caut")
        .unwrap()
        .arg("notacommand")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unknown")
                .or(predicate::str::contains("unrecognized"))
                .or(predicate::str::contains("error"))
                .or(predicate::str::contains("invalid")),
        );

    log.finish_ok();
}

#[test]
#[allow(deprecated)]
fn invalid_provider_is_rejected() {
    let log = TestLogger::new("invalid_provider_is_rejected");
    log.phase("execute");

    Command::cargo_bin("caut")
        .unwrap()
        .arg("usage")
        .arg("--provider=nonexistent_provider_xyz")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid")
                .or(predicate::str::contains("unknown"))
                .or(predicate::str::contains("not found")),
        );

    log.finish_ok();
}

#[test]
#[allow(deprecated)]
fn conflicting_flags_are_rejected() {
    let log = TestLogger::new("conflicting_flags_are_rejected");
    log.phase("execute");

    Command::cargo_bin("caut")
        .unwrap()
        .arg("usage")
        .arg("--all-accounts")
        .arg("--account=test")
        .assert()
        .failure()
        .stderr(predicate::str::contains("all-accounts"));

    log.finish_ok();
}

#[test]
#[allow(deprecated)]
fn help_exits_zero() {
    let log = TestLogger::new("help_exits_zero");
    log.phase("execute");

    Command::cargo_bin("caut")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));

    log.finish_ok();
}

#[test]
#[allow(deprecated)]
fn version_format_is_valid() {
    let log = TestLogger::new("version_format_is_valid");
    log.phase("execute");

    Command::cargo_bin("caut")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"caut \d+\.\d+\.\d+").unwrap());

    log.finish_ok();
}

#[test]
#[allow(deprecated)]
fn corrupted_config_does_not_panic() {
    let log = TestLogger::new("corrupted_config_does_not_panic");
    log.phase("setup");

    let mut temp_config = NamedTempFile::new().expect("create temp config");
    writeln!(temp_config, "this is not valid toml {{{{").expect("write temp config");

    log.phase("execute");
    // Point only the child at the corrupted config. Setting CAUT_CONFIG on
    // this test process instead would leak it into every `caut` subprocess
    // the other tests in this binary spawn concurrently.
    let output = Command::cargo_bin("caut")
        .unwrap()
        .env("CAUT_CONFIG", temp_config.path())
        .arg("usage")
        .output()
        .expect("run caut with corrupted config");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.to_lowercase().contains("panic"),
        "Should not panic on corrupted config"
    );

    log.finish_ok();
}
