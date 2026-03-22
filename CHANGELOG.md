# Changelog

All notable changes to **caut** (Coding Agent Usage Tracker) are documented here.

This project has no formal releases or git tags. The timeline below is
reconstructed from the full commit history on `main`, organized by capability
rather than raw diff order. Commit links point to the canonical GitHub repository.

Repository: <https://github.com/Dicklesworthstone/coding_agent_usage_tracker>

---

## [Unreleased] -- 0.1.0-dev

### 2026-03-20 -- Stable Rust build support

The project previously required nightly Rust due to `#![feature(let_chains)]`
in `rich_rust` and a `const fn` calling non-const `dirs::home_dir()`. Both
blockers were resolved, making `caut` buildable on **stable Rust 1.88+**.

**Stable toolchain enablement**

- Remove non-const `const` from `codexbar_token_accounts_file()` ([`19731e9`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/19731e9e9bbe99e5a50e652bf67594255df76186))
- Upgrade `rich_rust` 0.2.0 to 0.2.1, which drops `#![feature(let_chains)]` ([`19731e9`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/19731e9e9bbe99e5a50e652bf67594255df76186))
- Update README requirements table to reflect stable toolchain support ([`19731e9`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/19731e9e9bbe99e5a50e652bf67594255df76186))
- Fixes [#2](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/issues/2)

**CI lint fixes**

- Use `mul_add` for fused multiply-add in linear regression to satisfy `suboptimal_flops` ([`5ec6808`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5ec68083436d5ce266d46d0d0ffd75506656f740))
- Allow `missing_const_for_fn` on platform-conditional function ([`5ec6808`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5ec68083436d5ce266d46d0d0ffd75506656f740))

---

### 2026-02-25 -- Documentation: cass tool reference

- Document the `cass` (Cross-Agent Session Search) CLI tool in AGENTS.md, enabling agents to search prior conversations across Claude Code, Codex, Cursor, Gemini, and ChatGPT ([`57de595`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/57de5953801c6c61bcb73efddd9fc171acd4cb2a))

---

### 2026-02-21 -- License and social preview

**License update**

- Replace plain MIT license with MIT + OpenAI/Anthropic Rider restricting use by those companies without express written permission ([`968fc98`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/968fc9849e519777f8c8c8b14eea1d8b490b7231))
- Update README license references to match ([`43bfda1`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/43bfda1af787e0746a2a3fc2ed7739e3168cab96))

**Social preview**

- Add 1280x640 GitHub social preview image for link sharing ([`5d2bf61`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5d2bf6105c7306d9077bd81fdb4a0863449b277f))

---

### 2026-02-20 -- Major dependency upgrade and nightly lint sweep

**17 dependency upgrades**

- `clap` 4.5.54 to 4.5.60, `serde_json` 1.0.140 to 1.0.149, `toml` 0.9 to 1.0.3, `tokio` 1.44 to 1.49, `reqwest` 0.13.1 to 0.13.2, `chrono` 0.4.42 to 0.4.43, `rich_rust` =0.1.0 to 0.2.0, `tracing` 0.1.41 to 0.1.44, `jsonschema` 0.28 to 0.42, and more ([`fbae5bc`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/fbae5bc9869c3d09921a3b109157e7cf9f00dd2f))
- Temporarily switched toolchain to nightly 1.95.0 (later reverted to stable in March)

**Comprehensive clippy lint fixes (source modules)**

- Format string interpolation (`format!("{x}")` style) across all modules ([`24b1878`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/24b18784611dc21febf919497c3fc96a2df04ba2))
- Promote pure functions to `const fn` (budget limits, CLI helpers) ([`24b1878`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/24b18784611dc21febf919497c3fc96a2df04ba2))
- Collapse nested `if let` into `if-let-chain` expressions ([`24b1878`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/24b18784611dc21febf919497c3fc96a2df04ba2))
- Add `#[must_use]`, `/// # Errors` doc sections, and `Default` impls where required ([`24b1878`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/24b18784611dc21febf919497c3fc96a2df04ba2))

**Clippy lint fixes (test modules)**

- Same format-string, dead-code, and cast-truncation fixes applied to all 12 test files ([`44f0274`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/44f02746df3583a2f289e2312ac0c8fee04f1966))

---

### 2026-02-16 -- Bug fix: Claude credential detection (issue #1)

- `caut doctor` was looking for a `credentials` key in `~/.claude/.credentials.json`, but the file uses `claudeAiOauth` as its top-level key. This caused "Invalid credentials format" even for properly authenticated users ([`4df8345`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/4df83457de33c9bd7aa6c101d76528e5711678da))
- Now verifies `accessToken` is present and non-empty, extracts `subscriptionType` for richer status output
- Closes [#1](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/issues/1)

---

### 2026-02-15 -- rich_rust dependency stabilization

- Migrate `rich_rust` from pre-release/git reference to crates.io v0.2.0 for reproducible builds ([`75365ec`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/75365ecdd52bc9e0dbca8379cd3ce15c82ae4ed3))

---

### 2026-02-14 -- AGENTS.md refresh

- Fix typo ("PEROGATIVE" to "PREROGATIVE"), add `main:master` push instructions, expand dependency tables, remove sections now covered by README ([`c786a1a`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/c786a1a1fc13cc1db349caf6cec378ecc7147d93))

---

### 2026-02-11 -- Credential file watcher core

- Complete the `credential_watcher.rs` stub with a fully functional filesystem watcher using the `notify` crate ([`a6f36dd`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a6f36dd7478d12cf4e23504bfaaa716494f9bbe1))
- Background thread with 500ms debouncing for event processing
- Thread-safe shared state via `Arc<Mutex<>>` for watched file registry
- Clean shutdown via drop-based stop channel
- `recv()` and `recv_timeout()` methods for blocking event consumption

---

### 2026-01-27 -- Multi-account daemon monitoring system

A major feature branch delivering the storage and detection infrastructure for
monitoring multiple accounts per provider with automatic switch detection.

**Multi-account SQLite schema**

- Migration 003: `accounts`, `switch_log`, `provider_health`, `notification_history` tables ([`99c70bd`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/99c70bd80e72480c7f02c6957114e28121cd644c))
- WAL mode for concurrent daemon + CLI access
- `account_id` and `trigger_type` columns added to `usage_snapshots`

**Account-linked usage snapshot storage**

- `NewUsageSnapshot` builder, `UsageSnapshotRecord`, `SnapshotTrigger` enum (manual/switch/periodic) ([`22e78fc`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/22e78fc0f950e76b0c53371ac473f8de5a7b09c5))
- Insert, query-latest, range-query, cleanup, and delete operations with 12 unit tests

**Account registry CRUD**

- `upsert_account` using `(provider, email)` as natural key, label/metadata updates, reactivation, active/inactive listing ([`60ead18`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/60ead186560006923913273a09dd15a76e8b545b))
- 8 unit tests; total 22 multi-account tests passing

**Credential content hashing**

- SHA-256 identity hashing on stable fields (email, user_id, organization) that remains constant across token refreshes but changes on account switches ([`a405804`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a405804361449b13d2f0ed06e9c6d915256d0eb0))
- JWT claims extraction from `id_token` for identity fields
- `ChangeType` enum: `NoChange`, `TokenRefresh`, `AccountSwitch`, `Created`, `Deleted`
- 15 unit tests

**Credential watcher foundation**

- `notify` v8.0 dependency for cross-platform filesystem monitoring ([`bbf0fd7`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/bbf0fd78715f35c9bcf04e9818f37570b0d51db9))
- `WatchEvent` enum (`AccountSwitch`, `TokenRefresh`, `CredentialsRemoved`, `Error`)
- Integration with `CredentialHasher` for change classification
- 7 unit tests

**Code review fixes**

- Rename `from_str` to `parse` to avoid shadowing `std::str::FromStr` ([`a886b78`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a886b78a7ea6334e2f8ce199c5a666d1a09b633c))
- Fix `from_combined_hash` to use `split_once` for robustness
- Fix `get_latest_snapshots_by_provider` duplicate row bug caused by timestamp ties

**Dependency and warning cleanup**

- Update `toml` 0.8 to 0.9, pin `rich_rust` to =0.1.0 for stable compatibility ([`d491dbb`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/d491dbbe327dca8f3171f2dd4e22c54d083aa862))
- Resolve `dead_code` warnings in `build_info`, `cost_scanner`, `lib.rs`
- Create 29 beads issues for the multi-account daemon epic

---

### 2026-01-25 -- Expanded doctor health checks

- Additional diagnostic checks for improved system status monitoring ([`a464b9f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a464b9f3cc9bd5ce72ba61438e0cd97b4beec56b))

---

### 2026-01-23 -- CLI, storage, and error infrastructure expansion

**Major infrastructure expansion**

- Comprehensive argument parsing (136+ lines), session management commands, detailed history querying (275+ lines), TUI module scaffold ([`71d73fa`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/71d73fad34f484f2fea10c0b4f8c781bd0d168c4))
- Centralized path management (158 lines), multi-provider cache expansion (368+ lines), config validation and migration support ([`71d73fa`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/71d73fad34f484f2fea10c0b4f8c781bd0d168c4))
- Typed error module covering storage, CLI, provider, and TUI error conditions (678 lines) ([`71d73fa`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/71d73fad34f484f2fea10c0b4f8c781bd0d168c4))
- Improved prediction accuracy tracking and additional provider metadata

**Toolchain: stable to avoid nightly ICE**

- Switch `rust-toolchain` and CI to stable channel, update AGENTS.md guidance ([`a02d5ac`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a02d5ac5aaa91fc985e7ae652470ba1a4409a91c))

**CI warning fixes**

- `#[allow(dead_code)]` on serde-only deserialization structs (false positive warnings) ([`aa697d8`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/aa697d87f66198f1590a8416bfc097c851778339))
- Fix mismatched lifetime syntax in `format_status_segments` ([`aa697d8`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/aa697d87f66198f1590a8416bfc097c851778339))
- Prefix unused parameters with underscore, add `#[expect(dead_code)]` annotations ([`c741cf3`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/c741cf360d53a09cf90e90f9f2f5c3512276a515))

---

### 2026-01-22 -- Budget management, credential health, and cache freshness

**Budget management**

- New `budgets.rs` module (30KB) for usage quota management, budget allocation and consumption tracking, threshold alerts, and limit enforcement ([`3e7d969`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/3e7d9697725bf42c378291ec57502b76e4acf3b6))

**Credential health monitoring enhancements**

- Extend prompt handling with better CLI interaction ([`0574419`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/057441929bab9ede1153a3596c002b2dec160872))
- Enhanced credential health checking with more providers
- Session logs module for improved tracking
- Improved doctor rendering for clearer output

**Strict freshness mode**

- `--strict-freshness` flag for hard cache TTL enforcement ([`acd8d13`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/acd8d130cd0a2ff34a3797837d1f4a3d138379e2))
- Without the flag, stale data is shown with a staleness indicator (`~/`?)
- Extensive cache staleness detection and reporting with graceful degradation

---

### 2026-01-21 -- Rich TUI, error rendering, history expansion, and credential health

**Rich TUI component library**

- Seven reusable components in `src/rich/components/`: error panel, formatters, progress indicator, provider card, status badge, usage bar, usage table ([`90f5582`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/90f5582a6ed080562b55fcfeb71b6afe5d16215c))
- Comprehensive E2E tests for usage command with rich output

**Error rendering**

- Dedicated error rendering module for usage display ([`e220f78`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/e220f78e4dcffbe43c0a0ef597ab9f9ca750f1d8))
- E2E error handling tests covering error scenario validation ([`3dc5867`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/3dc586762d402de23406ea7d40b1bb8efbeb0300))

**History command expansion**

- New history subcommands with filtering options ([`4f3363d`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/4f3363d0b417c704e546599575e4421863f78a5e))
- Provider-specific enhancements for Claude and Codex history
- Improved error rendering with better formatting
- E2E test coverage for history features

**Provider-level usage aggregation**

- Provider grouping for usage reports ([`cb0bdf0`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/cb0bdf008ce40abd7fd9184101554ae9097458de))
- Improved pipeline error handling and edge cases
- New CLI args for filtering and formatting

**Credential health monitoring and prediction**

- `credential_health.rs` for credential health monitoring and reporting ([`372ac90`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/372ac903bbbb06f4dedb21075d91bdab66d3e6db))
- `prediction.rs` for usage prediction algorithms
- History integration tests and E2E testing script

**CLI improvements**

- Improved argument handling and UX refactoring ([`ac7727f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/ac7727f7f3dd2d5356299ace6fe98527e7f5e242))
- Enhanced main entry point ([`f7fc784`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/f7fc784ab5d8d7ecf7a3731cc12a28b427fe3240))
- Improved human-readable output formatting ([`f8e860f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/f8e860f5124b897c2a41320f74f8cc412805d4f2))

**License**

- Add MIT License (Copyright 2026 Jeffrey Emanuel) ([`535a3a9`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/535a3a9e173d4e268a10931231e7b145389983d5))

---

### 2026-01-20 -- Rich terminal output and enhanced logging

**rich_rust integration**

- New `src/rich/` module wrapping `rich_rust` for caut-specific use ([`e000dd0`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/e000dd074a57ed5d60886f526e1a0a89c4daa624))
- Theme configuration via `CAUT_THEME` environment variable
- Conditional rich output based on output format and terminal capabilities
- Updated `human.rs` and `doctor.rs` renderers to use rich rendering
- Graceful fallback for non-TTY contexts

**Enhanced logging system**

- `LogFormat` enum: Human (default), Json, Compact ([`acdd404`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/acdd4040bf89ed665220a67eb75a7b8bd09a22d2))
- Environment variable configuration: `CAUT_LOG`, `CAUT_LOG_FORMAT`, `CAUT_LOG_FILE`
- File logging support for persistent log capture
- JSON logging for machine parsing (CI, log aggregation)

**E2E and logging format tests**

- `e2e_history_test.rs` for history command validation ([`44bb568`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/44bb568fe6495d495781d8b8fd78d9c22dc86c8a))
- Log capture and log format switching tests

---

### 2026-01-19 -- History persistence and rich output planning

**History retention and pruning**

- Retention policy logic in `HistoryStore` with configurable limits ([`83acc24`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/83acc2453ba9323964e5f46b5aadb72963ae1c47))
- Pruning capabilities: delete old snapshots, aggregate daily stats
- Integrated pruning into CLI startup; integrated history recording into usage fetch pipeline
- Extensive unit tests for history pruning and retention

**SQLite dependency**

- Add `rusqlite` 0.38 for history tracking persistence ([`4225f12`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/4225f12dd090468596c8f6b0f59b11c29c1cd905))
- Enables daily/weekly/monthly aggregation, cost trend analysis, and provider comparison across time periods

**rich_rust integration plan**

- Detailed 1,700-line integration plan for premium CLI experience ([`00e6e0a`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/00e6e0a159e6bcce47b28bdc1f60730d219eb3d4))
- Dual-track architecture (human vs robot mode), safety gates, color theming, 4-phase roadmap

**Safety gate testing**

- CI/GITHUB_ACTIONS env var detection, color depth tests, all 7 safety gate conditions validated ([`d37180a`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/d37180af51dac235932548f4f1743dd1e2068fa6))

---

### 2026-01-18 -- Initial release: full CodexBar port to Rust

The foundational release of **caut**, a cross-platform Rust port of
[CodexBar](https://github.com/steipete/codexbar) (macOS-only Swift app) for
monitoring LLM provider usage from a single CLI command.

#### Core architecture

**Project initialization**

- Rust 2024 edition, Cargo project with `caut` binary target ([`84aa293`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/84aa293265a472f9df434c773656dabf5333a485))
- Release profile optimized for size: `opt-level = "z"`, LTO, single codegen unit, `panic = "abort"`, stripped symbols
- Build metadata embedding via `vergen-gix` (git SHA, build timestamp, rustc version)
- `unsafe_code = "deny"`, clippy pedantic + nursery warnings enabled

**CLI command layer (clap derive API)**

- Four commands: `usage`, `cost`, `token-accounts`, `doctor` ([`04252cf`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/04252cf7a18b0db50c87c3b856ddebac2bda2398))
- Global options: `--format`, `--json`, `--pretty`, `--no-color`, `--log-level`, `--verbose` ([`b33c1ea`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/b33c1ea887ee5fee57301bd57001838541636460))
- `usage` command: `--provider`, `--account`, `--account-index`, `--all-accounts`, `--no-credits`, `--status`, `--source`, `--web`, `--web-timeout`
- `cost` command: `--provider`, `--refresh`
- `token-accounts` subcommands: `list`, `convert`
- `doctor` command: parallel health checks with pass/warn/fail per provider
- `watch` command: continuous monitoring with configurable refresh interval
- CodexBar-compatible exit codes: 0 (success), 1 (error), 2 (binary not found), 3 (parse), 4 (timeout)

#### Provider support

**16-provider registry**

- Codex, Claude, Gemini, Antigravity, Cursor, OpenCode, Factory, z.ai, MiniMax, Kimi, KimiK2, Kiro, VertexAI, JetBrains AI, Amp, Copilot ([`044410e`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/044410e63b72b0b11ae842d977ab1073bbc14664))
- Provider metadata: `cli_name()`, `display_name()`, `supports_credits()`, `status_page_url()`, `credentials_path()`, `install_suggestion()`
- `ProviderSelection` enum: Primary, Both, All, Specific

**Claude provider fetcher**

- Three fetch strategies in priority order: OAuth token, web dashboard (macOS), CLI via PTY ([`ef3ceab`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/ef3ceabdfefcb98910246126f99baf2d7f2ed4c4))
- Session %, weekly %, and Opus/Sonnet tier tracking (tertiary rate window)
- Multi-account support via `token-accounts.json`

**Codex provider fetcher**

- Two fetch strategies: web dashboard with cookie auth (macOS), CLI RPC with JSON output ([`ef3ceab`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/ef3ceabdfefcb98910246126f99baf2d7f2ed4c4))
- Credits tracking, plan type detection (Pro, Plus), JWT token parsing
- Session and weekly rate limits with credit transaction history

#### Fetch pipeline

**Strategy-based fetch system**

- Four fetch strategy types: CLI (PTY), Web (cookies), OAuth (tokens), API (keys) ([`044410e`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/044410e63b72b0b11ae842d977ab1073bbc14664))
- Automatic fallback chain between strategies per provider
- Parallel fetching with configurable per-provider timeout
- All attempts recorded for diagnostics

#### Domain models

**Rate limit and cost tracking types**

- `RateWindow`: usage percentage, window duration, reset time, human-readable description ([`044410e`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/044410e63b72b0b11ae842d977ab1073bbc14664))
- `UsageSnapshot`: primary/secondary/tertiary rate windows with identity info
- `CreditsSnapshot`: remaining balance with transaction history
- `CostPayload`: session/30-day costs with per-day breakdown and input/output/cache token counts
- `RobotOutput`: JSON schema wrapper with `schemaVersion: "caut.v1"`

#### Rendering

**Dual-mode rendering system**

- Human mode: rich terminal formatting with colored panels, ASCII progress bars, color-coded status ([`47c9eba`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/47c9eba5e88fea3f94a64f8bdffacdaac01331bb))
- JSON mode: compact or pretty-printed, `caut.v1` schema-versioned envelopes ([`47c9eba`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/47c9eba5e88fea3f94a64f8bdffacdaac01331bb))
- Markdown mode: token-efficient format for AI agent consumption
- Doctor output: pass/warn/fail with timing and actionable suggestions
- Color thresholds: green (>= 25%), yellow (>= 10%), red (< 10%)

#### Storage

**Configuration, cache, and token accounts**

- TOML config at platform-specific XDG paths (macOS, Linux, Windows) ([`dd5c8de`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/dd5c8de881d3ac8e30295ab10186e6bf54ca2a8d))
- File-based cache with TTL invalidation for cost scan results
- Multi-account token storage in `token-accounts.json` with UUID-identified accounts, active account selection, and atomic writes with 600 permissions

**Local cost scanning**

- JSONL log scanning for `~/.codex/logs/` and `~/.claude/stats-cache.json` ([`044410e`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/044410e63b72b0b11ae842d977ab1073bbc14664))
- Daily aggregation with token pricing models for cost estimation

#### Error handling

**Typed errors with actionable suggestions**

- `CautError` variants: `Config`, `ProviderNotFound`, `UnsupportedSource`, `FetchFailed`, `Timeout`, `NoAvailableStrategy`, `PartialFailure`, `ParseError`, `IoError` ([`0537466`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/0537466ba22d708ae93228b6e34f5c655b051f40))
- `FixSuggestion` with description, copy-paste command, and documentation link
- Error category classification: Authentication, Network, Configuration, Provider, Environment, Internal

#### Utilities

**Time, format, and environment helpers**

- Duration formatting ("2h 15m", "3d 2h"), relative timestamps, ISO 8601 parsing ([`0537466`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/0537466ba22d708ae93228b6e34f5c655b051f40))
- Number formatting (thousand separators), percentage display, byte size formatting
- TTY detection, color capability detection, terminal width, CI environment detection

#### Testing

**Comprehensive test suite**

- Integration tests: schema contract validation against `caut-v1.schema.json`, HTTP mocking with `wiremock`, provider render pipeline tests, fixture validation ([`96b2651`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/96b26518efeb5252342cb077fca0f26ee5ec53cd))
- E2E tests: usage command (9 scenarios), cost command (7 scenarios) via `assert_cmd`
- Test utilities: factory functions, assertion helpers, mock builders (fluent API), tempdir helpers
- Coverage targets: core 90%+, render 85%+, CLI 80%+, providers 75%+

#### Documentation and CI

**JSON Schema contract**

- JSON Schema 2020-12 specification for robot mode output ([`034b5b7`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/034b5b76ed3970e5e2b4ae033aa9c2f9477c4336))
- Covers all output types: `RateWindow`, `UsageSnapshot`, `ProviderPayload`, `CostPayload`, `CreditsSnapshot`, `StatusPayload`
- Used by E2E tests for contract validation

**Project documentation suite**

- README, AGENTS.md, CONFIG_AND_IMPORTS.md, architecture docs, porting plan, upgrade log ([`a14042f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a14042f993fc9b4aca58583f5ec4cc5b7e9ead38))
- EXISTING_CODEXBAR_STRUCTURE.md: complete Swift-to-Rust mapping tables

**CI/CD pipeline**

- Three-tier GitHub Actions: unit tests, integration tests, E2E tests ([`a39813c`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a39813ca0bec042bddca45be7daf07e7c53e1ca1))
- Unified `ci.yml` with lint, multi-platform tests, security audit, coverage ([`9792ff3`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/9792ff3c6a319de6a8f950e860c7010b9250cc55))
- `release.yml` for cross-platform binary releases (5 targets: Linux/macOS/Windows, x86_64 + ARM) with SHA256 checksums

**History tracking**

- New history module for tracking usage over time with database migrations and CLI commands ([`f95755b`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/f95755b6ff2398fd26d68b938ebd08909b3f3136))

**README**

- Hero image (webp, 172KB), centered layout, problem/solution framing ([`69027de`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/69027defaf3cd24cb95be3dff55b212ad46397b0))
- Comparison tables, performance metrics, architecture diagram, FAQ

**Project illustration**

- Visual overview image for documentation ([`5f66546`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5f665469396a23ae8b8f240f42cbb23649453907))

**Beads issue tracking**

- 155 tracked issues (113 open, 42 closed) across all modules ([`a1fd67c`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a1fd67cfa44fa7f10d48f7dd4958498fe92aea8a))
- JSONL-based issue database with custom git merge driver
