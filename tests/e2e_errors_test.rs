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
use std::sync::Mutex;
use tempfile::NamedTempFile;

mod common;

use common::logger::TestLogger;

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

    let _guard = EnvGuard::set(&[(
        "CAUT_CONFIG",
        Some(temp_config.path().to_str().expect("config path")),
    )]);

    log.phase("execute");
    let output = Command::cargo_bin("caut")
        .unwrap()
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
