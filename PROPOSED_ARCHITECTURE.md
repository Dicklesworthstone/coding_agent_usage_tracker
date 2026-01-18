# Proposed Architecture for `caut` (coding_agent_usage_tracker)

> Objective: Port CodexBar’s CLI + core provider pipeline into a Rust CLI with **human mode** (rich console) and
> **robot mode** (agent‑optimized JSON/Markdown). This doc defines Rust modules, CLI contracts, and output schemas.

---

## 1) Goals

1. **CLI parity with CodexBar** for usage + cost.
2. **Human mode** using `rich_rust` for readable, dense output.
3. **Robot mode** with stable, token‑efficient JSON/Markdown.
4. **Strict output contracts** (schema versioned, deterministic ordering when possible).
5. **No GUI dependencies**; cross‑platform CLI behavior.

---

## 2) Project Layout (Rust)

```
src/
├── main.rs                # CLI entry, argument parsing, command dispatch
├── lib.rs                 # Library root (re-exports modules)
├── error.rs               # CautError enum with exit codes
├── cli/
│   ├── mod.rs
│   ├── usage.rs           # usage command
│   ├── cost.rs            # cost command
│   └── args.rs            # clap definitions, validation
├── core/
│   ├── mod.rs
│   ├── models.rs          # RateWindow, UsageSnapshot, CreditsSnapshot, etc.
│   ├── provider.rs        # ProviderDescriptor + registry
│   ├── fetch_plan.rs      # ProviderFetchPlan, Strategy, Outcome
│   ├── status.rs          # status page fetch + mapping
│   └── logging.rs         # JSONL stderr logging (optional)
├── providers/             # per‑provider fetchers + parsers
│   ├── mod.rs
│   ├── codex/...
│   ├── claude/...
│   └── ...
├── render/
│   ├── mod.rs
│   ├── human.rs           # rich_rust output
│   └── robot.rs           # JSON/Markdown output
├── storage/
│   ├── mod.rs
│   ├── token_accounts.rs  # token-accounts.json
│   ├── cache.rs           # caches for web/cost scans
│   └── paths.rs           # config/cache/app‑support paths
└── util/
    ├── time.rs
    ├── format.rs
    └── env.rs
```

---

## 3) Key Crates (Cargo.toml)

```toml
[dependencies]
# CLI argument parsing
clap = { version = "4.5", features = ["derive", "env"] }
clap_complete = "4.5"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Error handling
anyhow = "1.0"
thiserror = "2.0"

# Async runtime + HTTP
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.13", features = ["json", "webpki-roots"] }

# Date/time
chrono = { version = "0.4", features = ["serde"] }

# Platform paths and secrets
directories = "6.0"
keyring = { version = "3.6", features = ["apple-native", "windows-native", "linux-native"] }

# Terminal output - using rich_rust (local port of Python Rich)
rich_rust = { path = "/dp/rich_rust", features = ["full"] }
crossterm = "0.29"
colored = "3.1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# CLI detection
which = "8.0.0"

[build-dependencies]
vergen-gix = { version = "9.1", features = ["build", "cargo", "rustc"] }
```

---

## 4) CLI Contract (caut)

### Commands

- `caut` (no args): **Quick‑start** summary.
- `caut usage` (default for explicit command usage)
- `caut cost`

### Shared Flags

- `--format human|json|md` (default: `human`)
- `--robot` (alias for `--format json` unless `--format md` is set)
- `--pretty` (pretty JSON)
- `--no-color`
- `--log-level <trace|debug|info|warn|error>`
- `--json-logs` (emit JSONL logs to stderr)
- `-v/--verbose`

### `usage` Flags (parity with CodexBar)

- `--provider <name|both|all>`
- `--account <label>`
- `--account-index <n>` (1‑based)
- `--all-accounts`
- `--no-credits`
- `--status`
- `--source <auto|web|cli|oauth>`
- `--web-timeout <seconds>`
- `--web-debug-dump-html`

### `cost` Flags (parity with CodexBar)

- `--provider <name|both|all>`
- `--refresh`

### Quick‑start output (no args)

- **Single screen, token‑dense**:
  - What the tool is
  - Top 3 commands
  - Provider selection example
  - Robot output example

---

## 5) Human Mode Output (rich_rust)

### Goals
- Equivalent information to CodexBar CLI text.
- More readable via tables/panels and aligned bars.

### Proposed Layout

```
╭─ Codex (openai-web) ───────────────────────────────╮
│ Session  72% left   [========----]  resets in 2h   │
│ Weekly   41% left   [====--------]  resets Fri 9am │
│ Pace     6% in reserve | Expected 47% used         │
│ Credits  112.4 left                                  │
│ Account  user@example.com                           │
│ Plan     Pro                                        │
│ Status   Operational                                │
╰─────────────────────────────────────────────────────╯
```

`rich_rust` components to use:
- `Panel` for per‑provider blocks.
- `Table` for multi‑provider summaries.
- Styled bars with color thresholds (green/yellow/red).

---

## 6) Robot Mode Output

### JSON (default robot)

Stable schema with versioning:

```json
{
  "schemaVersion": "caut.v1",
  "generatedAt": "<ISO8601>",
  "command": "usage",
  "providers": [
    {
      "provider": "codex",
      "account": "user@example.com",
      "version": "0.6.0",
      "source": "openai-web",
      "status": { "indicator": "none", "description": "Operational", "updatedAt": "...", "url": "..." },
      "usage": { "primary": {...}, "secondary": {...}, "tertiary": null, "updatedAt": "...", "identity": {...} },
      "credits": { "remaining": 112.4, "events": [...], "updatedAt": "..." },
      "openaiDashboard": { ... }
    }
  ],
  "errors": [],
  "meta": { "format": "json", "flags": ["--status"], "runtime": "cli" }
}
```

### Markdown (optional robot)

- One provider block per section.
- Bullet list with **key/value** pairs to minimize tokens.

Example:
```
## Codex (openai-web)
- session_left: 72%
- weekly_left: 41%
- resets_session: 2025-12-04T19:15:00Z
- resets_weekly: 2025-12-05T17:00:00Z
- credits_left: 112.4
```

---

## 7) Provider Pipeline (Rust)

Mirror CodexBar’s descriptor + strategy pipeline:

- `ProviderDescriptor` → metadata + branding + CLI name + fetch plan.
- `ProviderFetchPlan` → allowed source modes + ordered strategy list.
- `ProviderFetchStrategy` → `is_available`, `fetch`, `should_fallback`.
- `ProviderFetchOutcome` → result + attempts (for `--verbose`).

This allows consistent provider selection logic and agent‑friendly diagnostics.

---

## 8) Token Accounts

- Support **both** formats:
  - CodexBar-compatible file (default path on macOS):
    `~/Library/Application Support/CodexBar/token-accounts.json`
  - `caut` native file:
    `~/.config/caut/token-accounts.json`
- Add **convert** subcommand to translate between formats:
  - `caut token-accounts convert --from codexbar --to caut`
  - `caut token-accounts convert --from caut --to codexbar`
- Preserve selection semantics (`--account`, `--account-index`, `--all-accounts`).

---

## 9) Caches & Storage

- **Config**: `~/.config/caut/` (or OS‑specific via `directories`).
  - `caut` owns config (does not read CodexBar defaults by default).
  - Import helper: `caut config import codexbar` (reads CodexBar provider toggles when available).
- **Cache**: `~/.cache/caut/`
  - `openai-dashboard.json` (web dashboard cache)
  - `cost-usage/<provider>-v1.json`

---

## 10) Error Handling Pattern

The error module (`src/error.rs`) uses `thiserror` for structured error types that map directly
to CodexBar-compatible exit codes.

### Error Enum (`CautError`)

```rust
#[derive(Error, Debug)]
pub enum CautError {
    // Configuration errors (exit 3)
    #[error("configuration error: {0}")]
    Config(String),

    #[error("invalid provider: {0}")]
    InvalidProvider(String),

    #[error("unsupported source for provider {provider}: {source_type}")]
    UnsupportedSource { provider: String, source_type: String },

    // Provider errors (exit 2 or 3)
    #[error("provider CLI not found: {0}")]
    ProviderNotFound(String),

    #[error("no available fetch strategy for provider: {0}")]
    NoAvailableStrategy(String),

    #[error("fetch failed for {provider}: {reason}")]
    FetchFailed { provider: String, reason: String },

    // Account errors (exit 3)
    #[error("account selection requires a single provider")]
    AccountRequiresSingleProvider,

    #[error("--all-accounts cannot be combined with --account or --account-index")]
    AllAccountsConflict,

    // Parse errors (exit 3)
    #[error("failed to parse response: {0}")]
    ParseResponse(String),

    // Network errors (exit 4 for timeout, 1 otherwise)
    #[error("request timeout after {0} seconds")]
    Timeout(u64),

    #[error("network error: {0}")]
    Network(String),

    // I/O errors (exit 1)
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    // Generic wrapper
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

### Exit Code Mapping

```rust
impl CautError {
    pub const fn exit_code(&self) -> ExitCode {
        match self {
            Self::ProviderNotFound(_) => ExitCode::BinaryNotFound, // 2
            Self::Config(_) | Self::InvalidProvider(_) | ... => ExitCode::ParseError, // 3
            Self::Timeout(_) => ExitCode::Timeout, // 4
            Self::Network(_) | Self::Io(_) | ... => ExitCode::GeneralError, // 1
        }
    }
}
```

---

## 11) Incremental Delivery Strategy

1. **Phase 1** ✅: CLI skeleton + models + human/robot renderers.
   - `src/main.rs` - CLI entry with tokio async main
   - `src/cli/args.rs` - Full clap definitions
   - `src/core/models.rs` - All data models (RateWindow, UsageSnapshot, etc.)
   - `src/core/provider.rs` - Provider enum + registry (all 16 providers)
   - `src/render/human.rs` - rich_rust placeholder
   - `src/render/robot.rs` - JSON/Markdown output
   - `src/error.rs` - Error types with exit codes
   - Project compiles with `cargo check`
2. **Phase 2**: Provider fetch strategies + impl.
   - Implement `FetchStrategy` trait for each source type
   - Add provider-specific fetchers in `src/providers/`
3. **Phase 3**: Cost usage scanning.
   - Scan Claude/Codex local files for cost data
   - Implement cache layer
4. **Phase 4**: Status polling + diagnostics.
   - Fetch status from provider status pages
   - Add `--verbose` diagnostics

---

## 12) Open Questions

1. Any additional conversion formats beyond CodexBar ↔ `caut`?
