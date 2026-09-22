# AGENTS.md — caut (Coding Agent Usage Tracker)

> Guidelines for AI coding agents working in this Rust codebase.

---

## RULE 0 - THE FUNDAMENTAL OVERRIDE PREROGATIVE

If I tell you to do something, even if it goes against what follows below, YOU MUST LISTEN TO ME. I AM IN CHARGE, NOT YOU.

---

## RULE 0.5 - SUITE-WIDE RULES LIVE IN /data/projects/AGENTS.md

The suite-wide rules in **`/data/projects/AGENTS.md`** bind you here too. Read it. Two sections
are load-bearing for perf work and are NOT duplicated below, so they cannot drift out of sync:

- **`## Named Reward-Hacking Patterns (ALL FORBIDDEN)`** — 12 named patterns, several already
  observed in this suite: gate self-weakening (and the exact price of a legitimate gate fix),
  proof-class inflation, golden regeneration reflex, commit-stream pumping, tautological tests,
  easy-lever cherry-picking, close-pump abuse, scope-splitting, spec-editing as progress,
  conformance metastasis, dependency smuggling, bench-path hardcoding.
- **`### Work-Graph Discipline`** — JSONL is truth and `beads.db` is disposable, `br sync
  --import-only` after every pull, single-writer on graph structure, closure on cited evidence
  with blocker beads gated on their named probe, `br dep cycles` stays empty.

The three that most often decide whether a number here is real: a **self-speedup is
MAINTENANCE, not a win** — a win needs the incumbent live in the SAME invocation; **never
weaken a gate to land a change**, and if a gate is genuinely defective, meet the evidence
standard and publish the win/lose split of what the fix admits; and **reporting a loss is a
success** — one line, revert, next lever, no retraction narrative.

---

## RULE NUMBER 1: NO FILE DELETION

**YOU ARE NEVER ALLOWED TO DELETE A FILE WITHOUT EXPRESS PERMISSION.** Even a new file that you yourself created, such as a test code file. You have a horrible track record of deleting critically important files or otherwise throwing away tons of expensive work. As a result, you have permanently lost any and all rights to determine that a file or folder should be deleted.

**YOU MUST ALWAYS ASK AND RECEIVE CLEAR, WRITTEN PERMISSION BEFORE EVER DELETING A FILE OR FOLDER OF ANY KIND.**

---

## Irreversible Git & Filesystem Actions — DO NOT EVER BREAK GLASS

1. **Absolutely forbidden commands:** `git reset --hard`, `git clean -fd`, `rm -rf`, or any command that can delete or overwrite code/data must never be run unless the user explicitly provides the exact command and states, in the same message, that they understand and want the irreversible consequences.
2. **No guessing:** If there is any uncertainty about what a command might delete or overwrite, stop immediately and ask the user for specific approval. "I think it's safe" is never acceptable.
3. **Safer alternatives first:** When cleanup or rollbacks are needed, request permission to use non-destructive options (`git status`, `git diff`, `git stash`, copying to backups) before ever considering a destructive command.
4. **Mandatory explicit plan:** Even after explicit user authorization, restate the command verbatim, list exactly what will be affected, and wait for a confirmation that your understanding is correct. Only then may you execute it—if anything remains ambiguous, refuse and escalate.
5. **Document the confirmation:** When running any approved destructive command, record (in the session notes / final response) the exact user text that authorized it, the command actually run, and the execution time. If that record is absent, the operation did not happen.

---

## Git Branch: ONLY Use `main`, NEVER `master`

**The default branch is `main`. The `master` branch exists only for legacy URL compatibility.**

- **All work happens on `main`** — commits, PRs, feature branches all merge to `main`
- **Never reference `master` in code or docs** — if you see `master` anywhere, it's a bug that needs fixing
- **The `master` branch must stay synchronized with `main`** — after pushing to `main`, also push to `master`:
  ```bash
  git push origin main:master
  ```

**If you see `master` referenced anywhere:**
1. Update it to `main`
2. Ensure `master` is synchronized: `git push origin main:master`

---

## Toolchain: Rust & Cargo

We only use **Cargo** in this project, NEVER any other package manager.

- **Edition:** Rust 2024 (stable 1.88+ — see `rust-toolchain.toml`)
- **Dependency versions:** Explicit versions for stability
- **Configuration:** Cargo.toml only (single crate, not a workspace)
- **Unsafe code:** Denied (`#![deny(unsafe_code)]`) — tests may use `#[allow(unsafe_code)]` for env var manipulation

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` + `clap_complete` | CLI argument parsing with derive macros and shell completions |
| `serde` + `serde_json` + `toml` | Serialization (JSON provider responses, TOML config) |
| `tokio` | Async runtime (full features) |
| `reqwest` | HTTP client for provider API calls (JSON + webpki-roots) |
| `chrono` | Date/time handling with serde support |
| `rusqlite` | SQLite for usage history and daily aggregates |
| `keyring` | Platform-native credential storage (macOS, Linux, Windows) |
| `rich_rust` | Terminal rich text rendering (Rust port of Python Rich) |
| `ratatui` + `crossterm` | TUI dashboard for live usage monitoring |
| `colored` + `atty` | Terminal colors with TTY detection |
| `fancy-regex` / `regex` | Pattern matching and markup stripping |
| `tracing` + `tracing-subscriber` | Structured logging with env-filter and JSON output |
| `anyhow` + `thiserror` | Error handling (anyhow for main, thiserror for library errors) |
| `sha2` + `hex` + `base64` | Cryptography for credential hashing |
| `notify` | Filesystem watching for credential daemon |
| `vergen-gix` | Build metadata embedding (build.rs) |
| `which` | CLI binary discovery for provider detection |
| `futures` | Async combinators for concurrent provider fetches |

### Release Profile

The release build optimizes for binary size:

```toml
[profile.release]
opt-level = "z"     # Optimize for size (lean binary for distribution)
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit for better optimization
panic = "abort"     # Smaller binary, no unwinding overhead
strip = true        # Remove debug symbols
```

---

## Code Editing Discipline

### No Script-Based Changes

**NEVER** run a script that processes/changes code files in this repo. Brittle regex-based transformations create far more problems than they solve.

- **Always make code changes manually**, even when there are many instances
- For many simple changes: use parallel subagents
- For subtle/complex changes: do them methodically yourself

### No File Proliferation

If you want to change something or add a feature, **revise existing code files in place**.

**NEVER** create variations like:
- `mainV2.rs`
- `main_improved.rs`
- `main_enhanced.rs`

New files are reserved for **genuinely new functionality** that makes zero sense to include in any existing file. The bar for creating new files is **incredibly high**.

---

## Backwards Compatibility

We do not care about backwards compatibility—we're in early development with no users. We want to do things the **RIGHT** way with **NO TECH DEBT**.

- Never create "compatibility shims"
- Never create wrapper functions for deprecated APIs
- Just fix the code directly

---

## Compiler Checks (CRITICAL)

**After any substantive code changes, you MUST verify no errors were introduced:**

```bash
# Check for compiler errors and warnings
cargo check --all-targets

# Check for clippy lints (pedantic + nursery are enabled)
cargo clippy --all-targets -- -D warnings

# Verify formatting
cargo fmt --check
```

If you see errors, **carefully understand and resolve each issue**. Read sufficient context to fix them the RIGHT way.

---

## Testing

### Testing Policy

Every module includes inline `#[cfg(test)]` unit tests alongside the implementation. Tests must cover:
- Happy path
- Edge cases (empty input, max values, boundary conditions)
- Error conditions

Integration tests live in the `tests/` directory. E2E tests live in `scripts/`.

### Unit Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run all tests including doc tests
cargo test --all-features --verbose
cargo test --doc

# Run specific test file
cargo test --test schema_contract_test
cargo test --test fixtures_test
cargo test --test http_client_test
cargo test --test history_integration_test
```

### End-to-End Testing

```bash
# Run E2E test scripts
./scripts/history_e2e.sh
./scripts/e2e_rich_logging.sh

# Verify rich_rust dependency
./scripts/verify_rich_rust_dep.sh
```

### Test Categories

| Test File | Focus Areas |
|-----------|-------------|
| `tests/schema_contract_test.rs` | JSON schema validation (caut.v1), output contract stability |
| `tests/fixtures_test.rs` | Provider response fixture parsing |
| `tests/http_client_test.rs` | HTTP client behavior, timeout handling, retry logic |
| `tests/history_integration_test.rs` | SQLite history storage, daily aggregates, migrations |
| `tests/provider_render_pipeline_test.rs` | Provider data through render pipeline (human + robot) |
| `tests/e2e_usage_test.rs` | End-to-end usage command validation |
| `tests/e2e_cost_test.rs` | End-to-end cost command validation |
| `tests/e2e_errors_test.rs` | Error handling and exit code validation |
| `tests/e2e_history_test.rs` | History command end-to-end tests |
| `tests/log_capture_test.rs` | Structured logging capture and format |
| `tests/logging_format_test.rs` | Log output format validation |
| `src/test_utils.rs` | Shared test utilities (conditionally compiled) |

### Test Utilities

The `test-utils` feature flag exposes `src/test_utils.rs` for integration test crates:

```toml
[features]
test-utils = ["tempfile"]

[dev-dependencies]
caut = { path = ".", features = ["test-utils"] }
```

---

## CI/CD Pipeline

### Jobs Overview

| Job | Trigger | Purpose | Blocking |
|-----|---------|---------|----------|
| `lint` | PR, push to main | Format + clippy | Yes |
| `test` | PR, push to main | Tests on Ubuntu, macOS, Windows | Yes |
| `security` | PR, push to main | `cargo audit` dependency scan | Yes |
| `coverage` | PR, push to main | `cargo llvm-cov` + Codecov upload | Non-blocking |
| `build` | After lint + test | Release build verification | Yes |

### Lint Job

- `cargo fmt --all -- --check` — Code formatting
- `cargo clippy --all-targets --all-features -- -D warnings` — Lints (pedantic + nursery enabled)

### Test Job (Cross-Platform Matrix)

Runs on `ubuntu-latest`, `macos-latest`, and `windows-latest`:
- `cargo test --all-features --verbose` — Full test suite
- `cargo test --doc` — Documentation tests

### Security Audit

- `cargo audit` — Checks for known vulnerabilities in dependencies

### Coverage Job

- `cargo llvm-cov` with LCOV output, uploaded to Codecov
- Ignores test files (`tests/` and `test_` prefixed)

---

## Third-Party Library Usage

If you aren't 100% sure how to use a third-party library, **SEARCH ONLINE** to find the latest documentation and current best practices.

---

## caut (Coding Agent Usage Tracker) — This Project

**This is the project you're working on.** caut is a cross-platform CLI tool that fetches usage data from 16+ LLM providers through a single command. It is a Rust port of [CodexBar](https://github.com/steipete/codexbar)'s CLI functionality.

### What It Does

Monitors LLM provider usage (rate limits, credits, costs) across Codex, Claude, Gemini, Cursor, Copilot, and 11 more providers. Outputs human-readable rich terminal panels or structured JSON/Markdown for AI agent consumption.

### Architecture

```
CLI Entry (src/main.rs)
        │
        ├── Usage Command (cli/usage.rs)
        ├── Cost Command (cli/cost.rs)
        ├── History Command (cli/history.rs)
        ├── Session Command (cli/session.rs)
        ├── Prompt Command (cli/prompt.rs)
        ├── Doctor Command (cli/doctor.rs)
        └── Watch Command (cli/watch.rs)
                │
                ▼
    Provider Registry (core/provider.rs)
    ┌─────────┬─────────┬─────────┬─────────┐
    │  Codex  │ Claude  │ Gemini  │   ...   │
    └────┬────┴────┬────┴────┬────┴────┬────┘
         │        │         │         │
         ▼        ▼         ▼         ▼
    Fetch Strategies (core/fetch_plan.rs)
    ┌──────────┬──────────┬──────────┬─────────┐
    │   CLI    │   Web    │  OAuth   │   API   │
    │  (PTY)   │(cookies) │(tokens)  │ (keys)  │
    └──────────┴──────────┴──────────┴─────────┘
                     │
                     ▼
    Renderers (render/*.rs)
    ┌──────────────────┬──────────────────┐
    │   Human Mode     │   Robot Mode     │
    │  (rich_rust)     │   (JSON/MD)      │
    └──────────────────┴──────────────────┘
```

### Project Structure

```
coding_agent_usage_tracker/
├── Cargo.toml                    # Single-crate package
├── build.rs                      # Build script (vergen metadata)
├── rust-toolchain.toml           # Stable Rust toolchain
├── src/
│   ├── main.rs                   # CLI entry point, clap argument dispatch
│   ├── lib.rs                    # Library root, module declarations
│   ├── test_utils.rs             # Shared test utilities (feature-gated)
│   ├── cli/                      # Command implementations
│   │   ├── args.rs               # Clap argument definitions
│   │   ├── usage.rs              # `caut usage` command
│   │   ├── cost.rs               # `caut cost` command
│   │   ├── history.rs            # `caut history` command
│   │   ├── session.rs            # `caut session` command
│   │   ├── prompt.rs             # `caut prompt` command
│   │   ├── doctor.rs             # `caut doctor` command
│   │   └── watch.rs              # `caut watch` command (live polling)
│   ├── core/                     # Business logic
│   │   ├── models.rs             # Data models (UsageData, CostData, etc.)
│   │   ├── provider.rs           # Provider registry and descriptors
│   │   ├── fetch_plan.rs         # Fetch strategy selection and execution
│   │   ├── pipeline.rs           # Provider pipeline orchestration
│   │   ├── http.rs               # HTTP client wrapper
│   │   ├── logging.rs            # Structured logging setup
│   │   ├── status.rs             # Provider status checking
│   │   ├── budgets.rs            # Usage budget tracking and alerts
│   │   ├── cost_scanner.rs       # Local JSONL cost log scanning
│   │   ├── pricing.rs            # Model pricing tables
│   │   ├── prediction.rs         # Usage prediction/forecasting
│   │   ├── session_logs.rs       # Session log parsing
│   │   ├── credential_hash.rs    # Credential hashing utilities
│   │   ├── credential_health.rs  # Credential validity checking
│   │   ├── credential_watcher.rs # Filesystem watcher for credential changes
│   │   ├── cli_runner.rs         # PTY-based CLI invocation
│   │   └── doctor/               # Diagnostic checks
│   ├── providers/                # Provider-specific implementations
│   │   ├── claude/               # Claude (Anthropic) provider
│   │   └── codex/                # Codex (OpenAI) provider
│   ├── render/                   # Output rendering
│   │   ├── human.rs              # Rich terminal output (panels, bars, colors)
│   │   ├── robot.rs              # JSON/Markdown structured output
│   │   ├── doctor.rs             # Doctor command renderer
│   │   └── error.rs              # Error rendering
│   ├── rich/                     # Rich text components (rich_rust integration)
│   │   ├── mod.rs                # Rich rendering engine
│   │   └── components/           # Reusable rich text widgets
│   ├── storage/                  # Data persistence
│   │   ├── cache.rs              # Response caching
│   │   ├── config.rs             # Configuration file management
│   │   ├── history.rs            # SQLite usage history
│   │   ├── history_schema.rs     # History DB schema and migrations
│   │   ├── multi_account.rs      # Multi-account token management
│   │   ├── paths.rs              # Platform-specific paths
│   │   └── token_accounts.rs     # Token account storage
│   ├── tui/                      # TUI dashboard
│   │   ├── app.rs                # Application state
│   │   ├── dashboard.rs          # Dashboard layout
│   │   ├── event.rs              # Event handling
│   │   └── provider_panel.rs     # Provider data panels
│   ├── error/                    # Error types
│   │   ├── mod.rs                # CautError enum, ExitCode, Result type
│   │   └── suggestions.rs        # User-facing error suggestions
│   └── util/                     # Utilities
│       ├── env.rs                # Environment variable helpers
│       ├── format.rs             # Number/string formatting
│       └── time.rs               # Time formatting helpers
├── schemas/
│   └── caut-v1.schema.json       # JSON Schema for output contract
├── migrations/                   # SQLite migrations
│   ├── 001_usage_snapshots.sql
│   ├── 002_daily_aggregates.sql
│   └── 003_multi_account.sql
├── tests/                        # Integration tests
├── scripts/                      # E2E test scripts
└── .github/workflows/            # CI: ci.yml + release.yml
```

### Supported Providers (16+)

| Provider | CLI Name | Source Types | Features |
|----------|----------|--------------|----------|
| **Codex** | `codex` | web, cli | Session, weekly, credits |
| **Claude** | `claude` | oauth, web, cli | Chat, weekly, opus tier |
| **Gemini** | `gemini` | oauth | Session, weekly |
| **Cursor** | `cursor` | web | Session limits |
| **Copilot** | `copilot` | api | Request limits |
| **z.ai** | `zai` | api | Token limits |
| **MiniMax** | `minimax` | api, web | Usage tracking |
| **Kimi** | `kimi` | api | Token limits |
| **Kimi K2** | `kimik2` | api | Token limits |
| **Kiro** | `kiro` | cli | Session limits |
| **Vertex AI** | `vertexai` | oauth | Quota tracking |
| **JetBrains AI** | `jetbrains` | local | Local file |
| **Antigravity** | `antigravity` | local | Local probe |
| **OpenCode** | `opencode` | web | Cookie auth |
| **Factory** | `factory` | web | Cookie auth |
| **Amp** | `amp` | web | Cookie auth |

### Data Sources

| Source | How It Works | Platform |
|--------|--------------|----------|
| **CLI** | Invokes provider CLI via PTY | All |
| **Web** | Reads browser cookies | macOS |
| **OAuth** | Uses stored tokens | All |
| **API** | Direct API calls | All |
| **Local** | Scans local JSONL logs | All |

Priority order: CLI > Web > OAuth > API > Local (configurable via `--source`)

### Output Modes

- **Human mode** (default): Rich terminal formatting with colored bars, panels, and progress indicators via `rich_rust`
- **Robot mode** (`--json` or `--format md`): Stable JSON/Markdown schemas (`caut.v1`) for AI agent consumption

### JSON Schema

All JSON output includes `schemaVersion: "caut.v1"` for forward compatibility. Formal schema definition is in `schemas/caut-v1.schema.json`.

### Exit Codes

| Code | Meaning | Example |
|------|---------|---------|
| `0` | Success | Normal operation |
| `1` | General error | Network failure, I/O error |
| `2` | Binary not found | Provider CLI not installed |
| `3` | Parse/config error | Invalid arguments, bad JSON |
| `4` | Timeout | Web fetch exceeded limit |

### Configuration

Config files are stored in platform-specific locations:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/caut/config.toml` |
| Linux | `~/.config/caut/config.toml` |
| Windows | `%APPDATA%\caut\config.toml` |

### Storage

- **SQLite history**: Usage snapshots and daily aggregates (`migrations/` for schema)
- **Response cache**: `storage/cache.rs` for provider response caching
- **Token accounts**: `token-accounts.json` for multi-account management
- **Credentials**: Platform keyring via `keyring` crate (never stored by caut directly)

### Key Design Decisions

- **CodexBar parity**: Faithful port of [CodexBar](https://github.com/steipete/codexbar)'s CLI functionality to cross-platform Rust
- **Single binary**: No runtime dependencies, ~3MB release binary
- **Dual-mode output**: Human-readable rich text and machine-readable JSON/Markdown from the same data path
- **Fail gracefully**: Network timeouts, missing credentials, and provider outages produce clear error messages with partial results — never crash
- **Zero configuration**: Works out of the box by detecting installed CLI tools and browser cookies
- **Provider abstraction**: Adding a new provider means implementing one trait, not touching core logic
- **Aggressive size optimization**: `opt-level = "z"`, LTO, single codegen unit, abort on panic, strip symbols

### Performance Targets

| Metric | Value |
|--------|-------|
| Binary size | ~3MB (release, stripped) |
| Startup time | ~10ms |
| Memory usage | ~10MB peak |
| First response | <500ms (cached) |

---

## MCP Agent Mail — Multi-Agent Coordination

A mail-like layer that lets coding agents coordinate asynchronously via MCP tools and resources. Provides identities, inbox/outbox, searchable threads, and advisory file reservations with human-auditable artifacts in Git.

### Why It's Useful

- **Prevents conflicts:** Explicit file reservations (leases) for files/globs
- **Token-efficient:** Messages stored in per-project archive, not in context
- **Quick reads:** `resource://inbox/...`, `resource://thread/...`

### Same Repository Workflow

1. **Register identity:**
   ```
   ensure_project(project_key=<abs-path>)
   register_agent(project_key, program, model)
   ```

2. **Reserve files before editing:**
   ```
   file_reservation_paths(project_key, agent_name, ["src/**"], ttl_seconds=3600, exclusive=true)
   ```

3. **Communicate with threads:**
   ```
   send_message(..., thread_id="FEAT-123")
   fetch_inbox(project_key, agent_name)
   acknowledge_message(project_key, agent_name, message_id)
   ```

4. **Quick reads:**
   ```
   resource://inbox/{Agent}?project=<abs-path>&limit=20
   resource://thread/{id}?project=<abs-path>&include_bodies=true
   ```

### Macros vs Granular Tools

- **Prefer macros for speed:** `macro_start_session`, `macro_prepare_thread`, `macro_file_reservation_cycle`, `macro_contact_handshake`
- **Use granular tools for control:** `register_agent`, `file_reservation_paths`, `send_message`, `fetch_inbox`, `acknowledge_message`

### Common Pitfalls

- `"from_agent not registered"`: Always `register_agent` in the correct `project_key` first
- `"FILE_RESERVATION_CONFLICT"`: Adjust patterns, wait for expiry, or use non-exclusive reservation
- **Auth errors:** If JWT+JWKS enabled, include bearer token with matching `kid`

---

## Beads (br) — Dependency-Aware Issue Tracking

Beads provides a lightweight, dependency-aware issue database and CLI (`br` - beads_rust) for selecting "ready work," setting priorities, and tracking status. It complements MCP Agent Mail's messaging and file reservations.

**Important:** `br` is non-invasive—it NEVER runs git commands automatically. You must manually commit changes after `br sync --flush-only`.

### Conventions

- **Single source of truth:** Beads for task status/priority/dependencies; Agent Mail for conversation and audit
- **Shared identifiers:** Use Beads issue ID (e.g., `br-123`) as Mail `thread_id` and prefix subjects with `[br-123]`
- **Reservations:** When starting a task, call `file_reservation_paths()` with the issue ID in `reason`

### Typical Agent Flow

1. **Pick ready work (Beads):**
   ```bash
   br ready --json  # Choose highest priority, no blockers
   ```

2. **Reserve edit surface (Mail):**
   ```
   file_reservation_paths(project_key, agent_name, ["src/**"], ttl_seconds=3600, exclusive=true, reason="br-123")
   ```

3. **Announce start (Mail):**
   ```
   send_message(..., thread_id="br-123", subject="[br-123] Start: <title>", ack_required=true)
   ```

4. **Work and update:** Reply in-thread with progress

5. **Complete and release:**
   ```bash
   br close 123 --reason "Completed"
   br sync --flush-only  # Export to JSONL (no git operations)
   ```
   ```
   release_file_reservations(project_key, agent_name, paths=["src/**"])
   ```
   Final Mail reply: `[br-123] Completed` with summary

### Mapping Cheat Sheet

| Concept | Value |
|---------|-------|
| Mail `thread_id` | `br-###` |
| Mail subject | `[br-###] ...` |
| File reservation `reason` | `br-###` |
| Commit messages | Include `br-###` for traceability |

---

## bv — Graph-Aware Triage Engine

bv is a graph-aware triage engine for Beads projects (`.beads/beads.jsonl`). It computes PageRank, betweenness, critical path, cycles, HITS, eigenvector, and k-core metrics deterministically.

**Scope boundary:** bv handles *what to work on* (triage, priority, planning). For agent-to-agent coordination (messaging, work claiming, file reservations), use MCP Agent Mail.

**CRITICAL: Use ONLY `--robot-*` flags. Bare `bv` launches an interactive TUI that blocks your session.**

### The Workflow: Start With Triage

**`bv --robot-triage` is your single entry point.** It returns:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

```bash
bv --robot-triage        # THE MEGA-COMMAND: start here
bv --robot-next          # Minimal: just the single top pick + claim command
```

### Command Reference

**Planning:**
| Command | Returns |
|---------|---------|
| `--robot-plan` | Parallel execution tracks with `unblocks` lists |
| `--robot-priority` | Priority misalignment detection with confidence |

**Graph Analysis:**
| Command | Returns |
|---------|---------|
| `--robot-insights` | Full metrics: PageRank, betweenness, HITS, eigenvector, critical path, cycles, k-core, articulation points, slack |
| `--robot-label-health` | Per-label health: `health_level`, `velocity_score`, `staleness`, `blocked_count` |
| `--robot-label-flow` | Cross-label dependency: `flow_matrix`, `dependencies`, `bottleneck_labels` |
| `--robot-label-attention [--attention-limit=N]` | Attention-ranked labels |

**History & Change Tracking:**
| Command | Returns |
|---------|---------|
| `--robot-history` | Bead-to-commit correlations |
| `--robot-diff --diff-since <ref>` | Changes since ref: new/closed/modified issues, cycles |

**Other:**
| Command | Returns |
|---------|---------|
| `--robot-burndown <sprint>` | Sprint burndown, scope changes, at-risk items |
| `--robot-forecast <id\|all>` | ETA predictions with dependency-aware scheduling |
| `--robot-alerts` | Stale issues, blocking cascades, priority mismatches |
| `--robot-suggest` | Hygiene: duplicates, missing deps, label suggestions |
| `--robot-graph [--graph-format=json\|dot\|mermaid]` | Dependency graph export |
| `--export-graph <file.html>` | Interactive HTML visualization |

### Scoping & Filtering

```bash
bv --robot-plan --label backend              # Scope to label's subgraph
bv --robot-insights --as-of HEAD~30          # Historical point-in-time
bv --recipe actionable --robot-plan          # Pre-filter: ready to work
bv --recipe high-impact --robot-triage       # Pre-filter: top PageRank
bv --robot-triage --robot-triage-by-track    # Group by parallel work streams
bv --robot-triage --robot-triage-by-label    # Group by domain
```

### Understanding Robot Output

**All robot JSON includes:**
- `data_hash` — Fingerprint of source beads.jsonl
- `status` — Per-metric state: `computed|approx|timeout|skipped` + elapsed ms
- `as_of` / `as_of_commit` — Present when using `--as-of`

**Two-phase analysis:**
- **Phase 1 (instant):** degree, topo sort, density
- **Phase 2 (async, 500ms timeout):** PageRank, betweenness, HITS, eigenvector, cycles

### jq Quick Reference

```bash
bv --robot-triage | jq '.quick_ref'                        # At-a-glance summary
bv --robot-triage | jq '.recommendations[0]'               # Top recommendation
bv --robot-plan | jq '.plan.summary.highest_impact'        # Best unblock target
bv --robot-insights | jq '.status'                         # Check metric readiness
bv --robot-insights | jq '.Cycles'                         # Circular deps (must fix!)
```

---

## UBS — Ultimate Bug Scanner

**Golden Rule:** `ubs <changed-files>` before every commit. Exit 0 = safe. Exit >0 = fix & re-run.

### Commands

```bash
ubs file.rs file2.rs                    # Specific files (< 1s) — USE THIS
ubs $(git diff --name-only --cached)    # Staged files — before commit
ubs --only=rust,toml src/               # Language filter (3-5x faster)
ubs --ci --fail-on-warning .            # CI mode — before PR
ubs .                                   # Whole project (ignores target/, Cargo.lock)
```

### Output Format

```
⚠️  Category (N errors)
    file.rs:42:5 – Issue description
    💡 Suggested fix
Exit code: 1
```

Parse: `file:line:col` → location | 💡 → how to fix | Exit 0/1 → pass/fail

### Fix Workflow

1. Read finding → category + fix suggestion
2. Navigate `file:line:col` → view context
3. Verify real issue (not false positive)
4. Fix root cause (not symptom)
5. Re-run `ubs <file>` → exit 0
6. Commit

### Bug Severity

- **Critical (always fix):** Memory safety, use-after-free, data races, SQL injection
- **Important (production):** Unwrap panics, resource leaks, overflow checks
- **Contextual (judgment):** TODO/FIXME, println! debugging

---

## RCH — Remote Compilation Helper

RCH offloads `cargo build`, `cargo test`, `cargo clippy`, and other compilation commands to a fleet of 8 remote Contabo VPS workers instead of building locally. This prevents compilation storms from overwhelming csd when many agents run simultaneously.

**RCH is installed at `~/.local/bin/rch` and is hooked into Claude Code's PreToolUse automatically.** Most of the time you don't need to do anything if you are Claude Code — builds are intercepted and offloaded transparently.

To manually offload a build:
```bash
rch exec -- cargo build --release
rch exec -- cargo test
rch exec -- cargo clippy
```

Quick commands:
```bash
rch doctor                    # Health check
rch workers probe --all       # Test connectivity to all 8 workers
rch status                    # Overview of current state
rch queue                     # See active/waiting builds
```

If rch or its workers are unavailable, it fails open — builds run locally as normal.

**Note for Codex/GPT-5.2:** Codex does not have the automatic PreToolUse hook, but you can (and should) still manually offload compute-intensive compilation commands using `rch exec -- <command>`. This avoids local resource contention when multiple agents are building simultaneously.

---

## ast-grep vs ripgrep

**Use `ast-grep` when structure matters.** It parses code and matches AST nodes, ignoring comments/strings, and can **safely rewrite** code.

- Refactors/codemods: rename APIs, change import forms
- Policy checks: enforce patterns across a repo
- Editor/automation: LSP mode, `--json` output

**Use `ripgrep` when text is enough.** Fastest way to grep literals/regex.

- Recon: find strings, TODOs, log lines, config values
- Pre-filter: narrow candidate files before ast-grep

### Rule of Thumb

- Need correctness or **applying changes** → `ast-grep`
- Need raw speed or **hunting text** → `rg`
- Often combine: `rg` to shortlist files, then `ast-grep` to match/modify

### Rust Examples

```bash
# Find structured code (ignores comments)
ast-grep run -l Rust -p 'fn $NAME($$$ARGS) -> $RET { $$$BODY }'

# Find all unwrap() calls
ast-grep run -l Rust -p '$EXPR.unwrap()'

# Quick textual hunt
rg -n 'println!' -t rust

# Combine speed + precision
rg -l -t rust 'unwrap\(' | xargs ast-grep run -l Rust -p '$X.unwrap()' --json
```

---

## Morph Warp Grep — AI-Powered Code Search

**Use `mcp__morph-mcp__warp_grep` for exploratory "how does X work?" questions.** An AI agent expands your query, greps the codebase, reads relevant files, and returns precise line ranges with full context.

**Use `ripgrep` for targeted searches.** When you know exactly what you're looking for.

**Use `ast-grep` for structural patterns.** When you need AST precision for matching/rewriting.

### When to Use What

| Scenario | Tool | Why |
|----------|------|-----|
| "How does the provider pipeline work?" | `warp_grep` | Exploratory; don't know where to start |
| "Where is the usage data model defined?" | `warp_grep` | Need to understand architecture |
| "Find all uses of `reqwest::Client`" | `ripgrep` | Targeted literal search |
| "Find files with `println!`" | `ripgrep` | Simple pattern |
| "Replace all `unwrap()` with `expect()`" | `ast-grep` | Structural refactor |

### warp_grep Usage

```
mcp__morph-mcp__warp_grep(
  repoPath: "/dp/coding_agent_usage_tracker",
  query: "How does the provider fetch strategy selection work?"
)
```

Returns structured results with file paths, line ranges, and extracted code snippets.

### Anti-Patterns

- **Don't** use `warp_grep` to find a specific function name → use `ripgrep`
- **Don't** use `ripgrep` to understand "how does X work" → wastes time with manual reads
- **Don't** use `ripgrep` for codemods → risks collateral edits

<!-- bv-agent-instructions-v1 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`) for issue tracking. Issues are stored in `.beads/` and tracked in git.

**Important:** `br` is non-invasive—it NEVER executes git commands. After `br sync --flush-only`, you must manually run `git add .beads/ && git commit`.

### Essential Commands

```bash
# View issues (launches TUI - avoid in automated sessions)
bv

# CLI commands for agents (use these instead)
br ready              # Show issues ready to work (no blockers)
br list --status=open # All open issues
br show <id>          # Full issue details with dependencies
br create --title="..." --type=task --priority=2
br update <id> --status=in_progress
br close <id> --reason "Completed"
br close <id1> <id2>  # Close multiple issues at once
br sync --flush-only  # Export to JSONL (NO git operations)
```

### Workflow Pattern

1. **Start**: Run `br ready` to find actionable work
2. **Claim**: Use `br update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id>`
5. **Sync**: Run `br sync --flush-only` then manually commit

### Key Concepts

- **Dependencies**: Issues can block other issues. `br ready` shows only unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers, not words)
- **Types**: task, bug, feature, epic, question, docs
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
br sync --flush-only    # Export beads to JSONL
git add .beads/         # Stage beads changes
git commit -m "..."     # Commit everything together
git push                # Push to remote
```

### Best Practices

- Check `br ready` at session start to find available work
- Update status as you work (in_progress → closed)
- Create new issues with `br create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always `br sync --flush-only && git add .beads/` before ending session

<!-- end-bv-agent-instructions -->

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Sync beads** - `br sync --flush-only` to export to JSONL
5. **Hand off** - Provide context for next session


---

## cass — Cross-Agent Session Search

`cass` indexes prior agent conversations (Claude Code, Codex, Cursor, Gemini, ChatGPT, etc.) so we can reuse solved problems.

**Rules:** Never run bare `cass` (TUI). Always use `--robot` or `--json`.

### Examples

```bash
cass health
cass search "async runtime" --robot --limit 5
cass view /path/to/session.jsonl -n 42 --json
cass expand /path/to/session.jsonl -n 42 -C 3 --json
cass capabilities --json
cass robot-docs guide
```

### Tips

- Use `--fields minimal` for lean output
- Filter by agent with `--agent`
- Use `--days N` to limit to recent history

stdout is data-only, stderr is diagnostics; exit code 0 means success.

Treat cass as a way to avoid re-solving problems other agents already handled.

---

Note for Codex/GPT-5.2:

You constantly bother me and stop working with concerned questions that look similar to this:

```
Unexpected changes (need guidance)

- Working tree still shows edits I did not make in Cargo.toml, Cargo.lock, src/cli/commands/upgrade.rs, src/storage/sqlite.rs, tests/conformance.rs, tests/storage_deps.rs. Please advise whether to keep/commit/revert these before any further work. I did not touch them.

Next steps (pick one)

1. Decide how to handle the unrelated modified files above so we can resume cleanly.
2. Triage beads_rust-orko (clippy/cargo warnings) and beads_rust-ydqr (rustfmt failures).
3. If you want a full suite run later, fix conformance/clippy blockers and re‑run cargo test --all.
```

NEVER EVER DO THAT AGAIN. The answer is literally ALWAYS the same: those are changes created by the potentially dozen of other agents working on the project at the same time. This is not only a common occurence, it happens multiple times PER MINUTE. The way to deal with it is simple: you NEVER, under ANY CIRCUMSTANCE, stash, revert, overwrite, or otherwise disturb in ANY way the work of other agents. Just treat those changes identically to changes that you yourself made. Just fool yourself into thinking YOU made the changes and simply don't recall it for some reason.

---

## Note on Built-in TODO Functionality

Also, if I ask you to explicitly use your built-in TODO functionality, don't complain about this and say you need to use beads. You can use built-in TODOs if I tell you specifically to do so. Always comply with such orders.

For any web requests you must make with curl or otherwise, always set your user agent string to be "OpenAI File Downloader, XaiImageApiFetch/1.0"
