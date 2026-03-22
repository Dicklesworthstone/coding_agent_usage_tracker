# Changelog

All notable changes to **caut** (Coding Agent Usage Tracker) are documented here.

This project has no formal releases or git tags yet. The timeline below is
reconstructed from the commit history on `main`. Each section groups commits by
the capability they deliver, in reverse chronological order.

Repository: <https://github.com/Dicklesworthstone/coding_agent_usage_tracker>

---

## [Unreleased] — 0.1.0-dev

### 2026-03-20 — Stable Rust build support

Resolved an issue where `caut` required nightly Rust, making it buildable on
stable Rust 1.88+. Also fixed new clippy lints (`suboptimal_flops`,
`missing_const_for_fn`) that were breaking CI.

- fix: enable stable Rust builds ([#2](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/pull/2)) — [`19731e9`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/19731e9e9bbe99e5a50e652bf67594255df76186)
- fix: resolve clippy warnings breaking CI — [`5ec6808`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5ec68083436d5ce266d46d0d0ffd75506656f740)

### 2026-02-25 — Documentation: Cross-Agent Session Search

Added documentation for the `cass` (Cross-Agent Session Search) tool to
AGENTS.md.

- docs(AGENTS.md): add cass tool reference — [`57de595`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/57de5953801c6c61bcb73efddd9fc171acd4cb2a)

### 2026-02-21 — License update and social preview

Updated the license from plain MIT to MIT with an OpenAI/Anthropic Rider,
reflecting the project's use of AI-generated code. Added a GitHub social
preview image.

- chore: update license to MIT with OpenAI/Anthropic Rider — [`968fc98`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/968fc9849e519777f8c8c8b14eea1d8b490b7231)
- docs: update README license references — [`43bfda1`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/43bfda1af787e0746a2a3fc2ed7739e3168cab96)
- chore: add GitHub social preview image — [`5d2bf61`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5d2bf6105c7306d9077bd81fdb4a0863449b277f)

### 2026-02-20 — Dependency upgrade and nightly clippy cleanup

Upgraded 17 dependencies and switched the toolchain to nightly. Fixed all
resulting clippy lints across source and test modules.

- Upgrade 17 dependencies, switch to nightly toolchain — [`fbae5bc`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/fbae5bc9869c3d09921a3b109157e7cf9f00dd2f)
- Fix nightly clippy lints in source modules — [`24b1878`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/24b18784611dc21febf919497c3fc96a2df04ba2)
- Fix nightly clippy lints in test modules — [`44f0274`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/44f02746df3583a2f289e2312ac0c8fee04f1966)

### 2026-02-16 — Doctor fix for Claude credentials (closes #1)

The `caut doctor` command was checking the wrong keys in Claude's
`.credentials.json`, causing false-negative health reports.

- fix(doctor): check correct keys in Claude .credentials.json — [`4df8345`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/4df83457de33c9bd7aa6c101d76528e5711678da)

### 2026-02-15 — rich\_rust dependency stabilized

Switched from a pre-release git ref to the published `rich_rust` v0.2.0 crate
on crates.io.

- Update rich\_rust to crates.io v0.2.0 — [`75365ec`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/75365ecdd52bc9e0dbca8379cd3ce15c82ae4ed3)

### 2026-02-11 — Credential file watcher (core)

Implemented the core credential file watcher that uses filesystem notifications
(`notify` crate) to detect when provider credentials change on disk, enabling
automatic re-authentication without manual restarts.

- feat(core): implement credential file watcher core — [`a6f36dd`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a6f36dd7478d12cf4e23504bfaaa716494f9bbe1)

### 2026-01-27 — Multi-account storage and credential watching foundation

Major storage layer expansion adding multi-account support with a new SQLite
schema, account registry CRUD, usage snapshot storage linked to accounts, and
the foundation for credential file watching with content hashing for change
detection.

**Multi-account SQLite storage:**
- feat(storage): implement multi-account SQLite schema — [`99c70bd`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/99c70bd80e72480c7f02c6957114e28121cd644c)
- feat(storage): implement account-linked usage snapshot storage — [`22e78fc`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/22e78fc0f950e76b0c53371ac473f8de5a7b09c5)
- feat(storage): implement account registry CRUD operations — [`60ead18`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/60ead186560006923913273a09dd15a76e8b545b)

**Credential watching foundation:**
- feat(core): implement credential content hashing for change detection — [`a405804`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a405804361449b13d2f0ed06e9c6d915256d0eb0)
- feat(core): add notify crate and credential watcher foundation — [`bbf0fd7`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/bbf0fd78715f35c9bcf04e9818f37570b0d51db9)
- fix: address code review issues in new modules — [`a886b78`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a886b78a7ea6334e2f8ce199c5a666d1a09b633c)
- fix: resolve dead\_code warnings and update dependencies — [`d491dbb`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/d491dbbe327dca8f3171f2dd4e22c54d083aa862)

### 2026-01-25 — Additional doctor health checks

Extended the `caut doctor` command with more diagnostic checks for provider
health.

- Add additional doctor health checks — [`a464b9f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a464b9f3cc9bd5ce72ba61438e0cd97b4beec56b)

### 2026-01-23 — Major infrastructure expansion: TUI, sessions, budgets, error handling

The single largest commit in the project, adding ~4,000 lines across 23 files.
Introduced a TUI dashboard module (`src/tui/`), a session management CLI
subcommand, a comprehensive budget management module, expanded error handling,
and deep storage/cache improvements. Also moved CI to the stable toolchain to
work around a nightly ICE (internal compiler error).

**TUI dashboard:**
- New `src/tui/` module with `app.rs`, `dashboard.rs`, `event.rs`,
  `provider_panel.rs` providing a real-time terminal UI.

**Session management:**
- New `src/cli/session.rs` (529 lines) for session lifecycle commands.

**Budget management:**
- feat(core): add comprehensive budget management module (942 lines) — [`3e7d969`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/3e7d9697725bf42c378291ec57502b76e4acf3b6)

**Infrastructure:**
- feat: major expansion of CLI, storage, and error handling — [`71d73fa`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/71d73fad34f484f2fea10c0b4f8c781bd0d168c4)
- ci: move caut to stable toolchain to avoid nightly ICE — [`a02d5ac`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a02d5ac5aaa91fc985e7ae652470ba1a4409a91c)
- Fix cargo fmt import ordering — [`1884d88`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/1884d880b2763c294df1fa73119dada07e42b142)
- Fix dead code warnings breaking CI builds — [`aa697d8`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/aa697d87f66198f1590a8416bfc097c851778339), [`c741cf3`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/c741cf360d53a09cf90e90f9f2f5c3512276a515)

### 2026-01-22 — Credential health, pricing engine, session logs, cache freshness

Added credential health monitoring with prediction (anticipates credential
expiry), a pricing engine for cost estimation, a session log scanner, strict
freshness mode for the cache, and enhanced doctor diagnostics.

- feat: Enhance credential health monitoring and doctor diagnostics — [`0574419`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/057441929bab9ede1153a3596c002b2dec160872)
  - New `src/core/pricing.rs` (556 lines) — model pricing data
  - New `src/core/session_logs.rs` (571 lines) — JSONL session log scanner
- Add strict freshness mode and enhanced cache staleness handling — [`acd8d13`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/acd8d130cd0a2ff34a3797837d1f4a3d138379e2)
- fix: Improve doctor diagnostic checks — [`655e691`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/655e6913283bd2effd5ea89926ff501633d16c16)

### 2026-01-21 — Rich TUI, error rendering, credential health, provider aggregation

A very active day that delivered multiple capabilities: rich terminal UI
components (usage bars, provider cards, progress indicators, status badges),
a dedicated error rendering module, credential health monitoring and prediction,
provider-level usage aggregation, expanded history commands, improved CLI
argument handling, and the MIT license.

**Rich TUI components:**
- Add rich TUI components and comprehensive E2E tests — [`90f5582`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/90f5582a6ed080562b55fcfeb71b6afe5d16215c)
  - `usage_bar.rs`, `usage_table.rs`, `provider_card.rs`,
    `progress_indicator.rs`, `status_badge.rs`, `error_panel.rs`

**Error rendering:**
- Add error rendering module (550 lines) — [`e220f78`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/e220f78e4dcffbe43c0a0ef597ab9f9ca750f1d8)
- Add E2E error handling tests — [`3dc5867`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/3dc586762d402de23406ea7d40b1bb8efbeb0300)

**Credential health monitoring:**
- Add credential health monitoring and prediction (3,354 lines) — [`372ac90`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/372ac903bbbb06f4dedb21075d91bdab66d3e6db)
  - `src/core/credential_health.rs` — monitors token/cookie expiry
  - `src/core/prediction.rs` — predicts when credentials will expire
  - `tests/history_integration_test.rs` — 991 lines of integration tests
- Update doctor checks and rendering — [`a116f3a`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a116f3adf95abe37f1a31737bc503096552bb407)

**Provider aggregation and pipeline:**
- Add provider-level usage aggregation, improve pipeline robustness — [`cb0bdf0`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/cb0bdf008ce40abd7fd9184101554ae9097458de)

**CLI improvements:**
- Improve CLI argument handling, add prompt module — [`ac7727f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/ac7727f7f3dd2d5356299ace6fe98527e7f5e242)
- Expand history command and improve error rendering — [`4f3363d`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/4f3363d0b417c704e546599575e4421863f78a5e)
- Improve human-readable output — [`f8e860f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/f8e860f5124b897c2a41320f74f8cc412805d4f2)

**License:**
- Add MIT License — [`535a3a9`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/535a3a9e173d4e268a10931231e7b145389983d5)

### 2026-01-20 — rich\_rust integration and enhanced logging

Integrated the `rich_rust` crate for Python Rich-style terminal formatting.
Added multiple logging output formats (JSONL, human, compact) and E2E tests
for the logging subsystem.

- feat: Integrate rich\_rust for enhanced terminal output — [`e000dd0`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/e000dd074a57ed5d60886f526e1a0a89c4daa624)
- feat: Enhanced logging with multiple output formats — [`acdd404`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/acdd4040bf89ed665220a67eb75a7b8bd09a22d2)
- test: Add E2E and logging format tests — [`44bb568`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/44bb568fe6495d495781d8b8fd78d9c22dc86c8a)

### 2026-01-19 — History persistence and retention policies

Added SQLite-backed history tracking with `rusqlite`, including daily
aggregation, a retention policy with configurable pruning, and a plan for
the rich\_rust terminal integration.

- chore(deps): Add rusqlite 0.38 for history tracking persistence — [`4225f12`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/4225f12dd090468596c8f6b0f59b11c29c1cd905)
- Implement history retention policy and pruning logic — [`83acc24`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/83acc2453ba9323964e5f46b5aadb72963ae1c47)
- docs: Add comprehensive rich\_rust terminal integration plan — [`00e6e0a`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/00e6e0a159e6bcce47b28bdc1f60730d219eb3d4)

### 2026-01-18 — Initial release: project scaffold through first working build

The project was created in a single day. The initial commit sequence delivers a
complete, working CLI tool with two provider backends (Claude and Codex),
dual-mode rendering (human/robot), a storage layer, CI, and a comprehensive
integration test suite with 56 test fixture files.

**Core architecture (all landed 2026-01-18):**

| Capability | Representative commit |
|---|---|
| Cargo project init | [`84aa293`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/84aa293265a472f9df434c773656dabf5333a485) |
| Git config, beads integration | [`5ebff8c`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/5ebff8c574092c449bb545a0218268c7e6dd7d2c) |
| GitHub Actions CI + nextest | [`a39813c`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a39813ca0bec042bddca45be7daf07e7c53e1ca1) |
| CLI entry point + lib root | [`b33c1ea`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/b33c1ea887ee5fee57301bd57001838541636460) |
| clap derive CLI commands (`usage`, `cost`, `token-accounts`) | [`04252cf`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/04252cf7a18b0db50c87c3b856ddebac2bda2398) |
| Core models, fetch pipeline, cost scanner, doctor checks | [`044410e`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/044410e63b72b0b11ae842d977ab1073bbc14664) |
| Claude and Codex provider fetchers | [`ef3ceab`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/ef3ceabdfefcb98910246126f99baf2d7f2ed4c4) |
| Dual-mode rendering (human + robot/JSON/MD) | [`47c9eba`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/47c9eba5e88fea3f94a64f8bdffacdaac01331bb) |
| Storage layer (config, cache, token accounts) | [`dd5c8de`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/dd5c8de881d3ac8e30295ab10186e6bf54ca2a8d) |
| Utilities, error handling, test harness | [`0537466`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/0537466ba22d708ae93228b6e34f5c655b051f40) |
| Integration test suite (56 fixtures, 6,827 lines) | [`96b2651`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/96b26518efeb5252342cb077fca0f26ee5ec53cd) |
| JSON Schema contract (`caut-v1`) | [`034b5b7`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/034b5b76ed3970e5e2b4ae033aa9c2f9477c4336) |
| Documentation suite (AGENTS.md, architecture, porting plan) | [`a14042f`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/a14042f993fc9b4aca58583f5ec4cc5b7e9ead38) |
| CI consolidation + release workflow | [`9792ff3`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/9792ff3c6a319de6a8f950e860c7010b9250cc55) |
| README with hero image | [`69027de`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/69027defaf3cd24cb95be3dff55b212ad46397b0) |
| History tracking feature (SQLite migrations, CLI subcommand) | [`f95755b`](https://github.com/Dicklesworthstone/coding_agent_usage_tracker/commit/f95755b6ff2398fd26d68b938ebd08909b3f3136) |

---

## Summary of capabilities (as of `main` HEAD)

| Area | Status |
|---|---|
| **Providers** | 16 supported (Codex, Claude, Gemini, Cursor, Copilot, z.ai, MiniMax, Kimi, Kimi K2, Kiro, Vertex AI, JetBrains AI, Antigravity, OpenCode, Factory, Amp) |
| **Output formats** | Human (rich terminal), JSON (`caut.v1` schema), Markdown |
| **Data sources** | CLI (PTY), Web (cookies), OAuth (tokens), API (keys), Local (JSONL) |
| **History** | SQLite-backed usage snapshots, daily aggregation, retention policies |
| **Multi-account** | SQLite schema, account registry CRUD, account-linked snapshots |
| **Credential health** | File watcher (notify), content hashing, expiry prediction, doctor diagnostics |
| **Budget management** | Comprehensive budget tracking module |
| **TUI** | Dashboard, provider panels, usage bars, status badges, progress indicators |
| **Cache** | Strict freshness mode, staleness detection |
| **CI** | GitHub Actions (stable Rust 1.88+), nextest, release workflow |
| **License** | MIT with OpenAI/Anthropic Rider |
