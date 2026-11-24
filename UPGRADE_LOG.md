# Dependency Upgrade Log

**Date:** 2026-02-19  |  **Project:** caut (Coding Agent Usage Tracker)  |  **Language:** Rust

## Summary
- **Updated:** 17  |  **Skipped:** 3 (RC only)  |  **Failed:** 0  |  **Needs attention:** 1

## Toolchain

### Rust: stable → nightly (1.95.0-nightly 2026-02-19)
- **Change:** `rust-toolchain.toml` channel changed from `stable` to `nightly`
- **Build:** Passes `cargo check --all-targets`
- **Note:** Nightly clippy introduces ~825 new pedantic/nursery lint warnings not present in stable clippy. These are pre-existing code patterns, not regressions from the upgrade. Fixing them is a separate task.

## Dependency Updates

### clap: 4.5.54 → 4.5.60
- **Breaking:** None
- **Tests:** Passed

### clap_complete: 4.5.44 → 4.5.66
- **Breaking:** None
- **Tests:** Passed

### serde_json: 1.0.140 → 1.0.149
- **Breaking:** None
- **Tests:** Passed

### toml: 0.9 → 1.0.3
- **Breaking:** `from_str` now only parses documents (not values), `to_string` only renders documents. Caut only uses document-level APIs, so no code changes needed.
- **Tests:** Passed

### anyhow: 1.0.100 → 1.0.102
- **Breaking:** None
- **Tests:** Passed

### thiserror: 2.0.17 → 2.0.18
- **Breaking:** None
- **Tests:** Passed

### tokio: 1.44 → 1.49
- **Breaking:** None
- **Tests:** Passed

### reqwest: 0.13.1 → 0.13.2
- **Breaking:** `webpki-roots` feature removed in 0.13.2 (TLS roots now bundled by default)
- **Migration:** Removed `webpki-roots` from features list
- **Tests:** Passed

### chrono: 0.4.42 → 0.4.43
- **Breaking:** None
- **Tests:** Passed

### rich_rust: =0.1.0 → 0.2.0
- **Breaking:** None detected (prelude API unchanged)
- **Note:** Removed exact version pin (`=0.1.0` → `0.2.0`). New dependency: `stdio-override`
- **Tests:** Passed

### colored: 3.1.0 → 3.1.1
- **Breaking:** None
- **Tests:** Passed

### tracing: 0.1.41 → 0.1.44
- **Breaking:** None
- **Tests:** Passed

### tracing-subscriber: 0.3.19 → 0.3.22
- **Breaking:** None
- **Tests:** Passed

### regex: 1.11 → 1.12.3
- **Breaking:** None
- **Tests:** Passed

### futures: 0.3.31 → 0.3.32
- **Breaking:** None
- **Tests:** Passed

### tempfile: 3.15 → 3.25
- **Breaking:** None
- **Tests:** Passed

### jsonschema: 0.28 → 0.42 (dev-dependency)
- **Breaking:** Major version jump, but the two APIs used (`validator_for`, `Validator`) are unchanged
- **Tests:** Passed

### assert_cmd: 2.0 → 2.1 (dev-dependency)
- **Breaking:** `Command::cargo_bin` deprecated (use `cargo::cargo_bin_cmd!` instead). Pre-existing deprecation warning.
- **Tests:** Passed

## Skipped

### keyring: 3.6.3 (latest stable: 4.0.0-rc.3)
- **Reason:** Only RC available; staying on latest stable
- **Action:** No change

### notify: 8.0 (latest stable: 9.0.0-rc.2)
- **Reason:** Only RC available; staying on latest stable
- **Action:** No change

### sha2: 0.10 (latest stable: 0.11.0-rc.5)
- **Reason:** Only RC available; staying on latest stable
- **Action:** No change

## Already Latest

These crates were already at their latest stable versions:
- serde 1.0.228
- directories 6.0.0
- crossterm 0.29.0
- ratatui 0.30
- which 8.0.0
- rusqlite 0.38.0
- hex 0.4, base64 0.22
- vergen-gix 9.1
- wiremock 0.6, tokio-test 0.4, predicates 3.1, tracing-test 0.2
- atty 0.2 (unmaintained but no replacement needed)

## Needs Attention

### Nightly clippy lint explosion
- **Issue:** Switching from stable to nightly clippy adds ~825 new pedantic/nursery warnings
- **Categories:** format string interpolation (189), missing `# Errors` docs (104), `const fn` suggestions (66), missing backticks in docs (66), `format!` appended to String (57), collapsible `if` (48), and misc cast/style warnings
- **Action:** Fix in a separate task, or add specific `#[allow]` attributes for nightly-only lints

## Notes

- All dependencies checked against crates.io as of 2026-02-19
- `cargo check --all-targets` passes cleanly
- `cargo fmt --check` passes cleanly
- Unit tests pass (669 tests, run single-threaded for env-var tests)
- Pre-existing e2e_history_test failures unrelated to upgrades (SQLite WAL migration issue)

---

# 2026-11-23: semver-compatible Cargo.lock refresh

Lock-only (`cargo update` under the 1.88 MSRV-aware resolver), no manifest change.

## Updated (direct dependencies)

anyhow 1.0.104, assert_cmd 2.2.2, chrono 0.4.45, clap 4.6.9, clap_complete 4.6.12,
futures 0.3.34, http-body-util 0.1.5, hyper 1.12.0, hyper-util 0.1.21, notify 8.2.2,
ratatui 0.30.2, reqwest 0.13.5, rich_rust 0.2.2, thiserror 2.0.21, tokio 1.54.1,
tokio-test 0.4.6, toml 1.2.2, which 8.0.6. About 230 transitive packages moved with them.

## Security

`cargo audit` went from 17 vulnerabilities to 1. Fixed by this refresh: h2 (RUSTSEC-2026-0258),
quinn-proto (7 advisories), quinn-udp, rustls (4), rustls-webpki, url and webpki-root-certs.
Unsound/unmaintained warnings cleared: tokio (RUSTSEC-2026-0386), anyhow, faster-hex, memmap2,
rand, rustix, pin-utils.

Remaining: RUSTSEC-2026-0459 in libsqlite3-sys 0.36 applies only to the bundled SQLite build;
caut links the system SQLite, so it is not affected. The fix needs rusqlite 0.41+, which needs
Rust 1.89. Warnings left: atty (dev-dependency), chrono (unmaintained), heapless 0.8 (through
faster-hex, gix-hash, vergen-gix: build-time only), lru 0.16.4.

## Held

- **serde / serde_core / serde_derive 1.0.228, serde_json 1.0.149, regex 1.12.3,
  regex-automata 0.4.14, regex-syntax 0.8.10, aho-corasick 1.1.4, memchr 2.8.0**: these parse
  the session logs the cost scanner reads, and there is no benchmark to compare against.
- **Needs a newer Rust than the 1.88 MSRV**: cargo-platform 0.3.3, kstring 2.0.5, notify 9,
  notify-types 2.3, uuid 1.29, vergen-gix 10.
- **time =0.3.47** stays pinned (see the comment in Cargo.toml); rich_rust 0.2.4 needs
  time 0.3.55, so rich_rust stays at 0.2.2.

## Performance

No benchmark covers the HTTP client, so a representative workload was measured instead: a
release-built test that serves a provider-style JSON response from a local mock server and
fetches it 4000 times through `fetch_json` (after 300 warm-up calls). Base and candidate
locks were built on the same worker and run alternately:

| Worker | Runs each | Base median | Candidate median | Paired ratio (median) | Candidate faster |
|--------|-----------|-------------|------------------|-----------------------|------------------|
| A | 7 | 2320 ms | 2621 ms | 1.010 | 3 of 7 |
| B | 15 | 806 ms | 846 ms | 1.002 | 7 of 15 |
| C | 15 | 1476 ms | 1352 ms | 0.943 | 8 of 15 |

Run-to-run spread on the shared build machines was about 35% of the mean, and the sign of
the difference changes between machines, so no regression is detectable at that resolution.
The HTTP path is network-bound in real use.

## Validation

- `cargo clippy --all-targets -- -D warnings` (pinned nightly): clean.
- `cargo test --no-fail-fast`: 1240 passed, 0 failed, 17 ignored, the same as the parent.
