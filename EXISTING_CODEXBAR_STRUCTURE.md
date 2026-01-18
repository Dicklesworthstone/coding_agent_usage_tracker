# Existing CodexBar Structure (Spec Extraction)

This document captures CodexBar’s **CLI + core provider pipeline** behavior as the canonical spec to port. It focuses on
data models, CLI semantics, fetch strategies, output formats, and configuration sources. UI and macOS app lifecycle are
explicitly out of scope.

---

## 1) Module Layout & Entry Points

### Modules
- `Sources/CodexBarCore`
  - Provider descriptors + fetch strategies + parsing + shared utilities.
  - Core data models used by CLI and the menu bar app.
- `Sources/CodexBarCLI`
  - Commander-based CLI (`codexbar`).
  - Commands: `usage` (default), `cost`.
- Other modules (`CodexBar`, `Widget`, `Macros`, helpers) are not required for CLI parity.

### CLI Entry Point
- `Sources/CodexBarCLI/CLIEntry.swift`
  - Defines `@main` entry and parses argv.
  - Default command is `usage` when no explicit command is provided.
  - Handles `-h/--help` and `-V/--version` before building command descriptors.

---

## 2) CLI Commands & Flags

### Command: `usage` (default)
Prints usage for selected providers in **text** or **JSON**.

**Arguments / Flags** (Commander):
- `--format text|json` (default: `text`)
- `--json` (alias for `--format json`)
- `--json-output` (emit JSON logs to stderr)
- `--log-level <trace|verbose|debug|info|warning|error|critical>`
- `-v/--verbose`
- `--provider <name|both|all>`
- `--account <label>`
- `--account-index <n>` (1-based; converted to 0-based)
- `--all-accounts`
- `--no-credits` (text output only; JSON always includes credits if available)
- `--no-color`
- `--pretty` (pretty JSON output)
- `--status` (fetch status page)
- `--source <auto|web|cli|oauth>`
- `--web` (alias for `--source web`)
- `--web-timeout <seconds>` (Codex web)
- `--web-debug-dump-html` (Codex web debug)
- `--antigravity-plan-debug` (stderr diagnostic)
- `--augment-debug` (stderr diagnostic)

**Important validation rules**
- `--source` must be one of `auto|web|cli|oauth` (else exit 1).
- On non-macOS: `--source web|auto` is rejected.
- Token account selection:
  - `--all-accounts` cannot be combined with `--account` or `--account-index`.
  - Any account selection requires a **single provider**.
  - Providers without token account support reject account selection.

### Command: `cost`
Prints **local cost usage** (Claude + Codex only) as text or JSON.

**Arguments / Flags**
- `--format text|json` (default: `text`)
- `--json` (alias for `--format json`)
- `--json-output` (emit JSON logs to stderr)
- `--log-level <trace|verbose|debug|info|warning|error|critical>`
- `-v/--verbose`
- `--provider <name|both|all>`
- `--no-color`
- `--pretty`
- `--refresh` (ignore cached scan)

**Rules**
- Only Claude + Codex are supported; others are skipped with stderr warning.
- If no supported providers remain, exits with error.

---

## 3) Provider Selection Logic

From `CLIEntry.swift`:

1) If `--provider` is passed:
   - Parsed with `ProviderSelection(argument:)`.
2) Else:
   - Load enabled providers from UserDefaults:
     - Prefer `com.steipete.codexbar` or `com.steipete.codexbar.debug` (key: `providerToggles`).
     - Fallback to standard defaults.
3) Selection fallback:
   - If >= 3 enabled → `.all`.
   - If exactly 2 enabled:
     - If those are the **primary providers** → `.both`.
     - Else → `.custom([providers])`.
   - If 1 enabled → that provider.
   - If none → Codex.

`both` resolves to “primary providers” from metadata (`isPrimaryProvider`).

---

## 4) Output Formats

### Text output (human)
Rendered by `CLIRenderer.renderText`:

- Header: `== <Provider> <Version> (<source>) ==`
- Primary window: `<SessionLabel>: <percent left> [bar]`
- Reset line: `Resets <countdown or absolute>`
- Weekly window: `<WeeklyLabel>: <percent left> [bar]`
- Pace line (Codex + Claude only) when conditions met:
  - `Pace: <On pace | x% in reserve/deficit> | Expected <N>% used | Runs out in <eta>`
- Tertiary (Opus/Sonnet) when supported.
- Credits line for Codex: `Credits: <value> left`.
- Identity:
  - `Account: <email>`
  - `Plan: <plan>`
- Status (if `--status`): `Status: <indicator label> – <description>`

### JSON output (robot)
`ProviderPayload` encodes as JSON array (one entry per provider/account).

**ProviderPayload fields**
- `provider: String`
- `account: String?`
- `version: String?`
- `source: String`
- `status: ProviderStatusPayload?`
- `usage: UsageSnapshot`
- `credits: CreditsSnapshot?`
- `antigravityPlanInfo: AntigravityPlanInfoSummary?`
- `openaiDashboard: OpenAIDashboardSnapshot?`

**Status payload**
- `indicator: none|minor|major|critical|maintenance|unknown`
- `description: String?`
- `updatedAt: Date?`
- `url: String`

### Cost output (text)
- Header: `<Provider> Cost (local)`
- `Today: <cost> · <tokens> tokens` (tokens optional)
- `Last 30 days: <cost> · <tokens> tokens` (tokens optional)

### Cost output (JSON)
`CostPayload` fields:
- `provider: String`
- `source: String` (always `local`)
- `updatedAt: Date`
- `sessionTokens: Int?`
- `sessionCostUSD: Double?`
- `last30DaysTokens: Int?`
- `last30DaysCostUSD: Double?`
- `daily: [CostDailyEntryPayload]`
- `totals: CostTotalsPayload?`

`CostDailyEntryPayload`:
- `date: String`
- `inputTokens: Int?`
- `outputTokens: Int?`
- `cacheReadTokens: Int?`
- `cacheCreationTokens: Int?`
- `totalTokens: Int?`
- `totalCost: Double?` (encoded as `costUSD` internally)
- `modelsUsed: [String]?`
- `modelBreakdowns: [{ modelName, cost }]?`

`CostTotalsPayload`:
- `inputTokens: Int?`
- `outputTokens: Int?`
- `cacheReadTokens: Int?`
- `cacheCreationTokens: Int?`
- `totalTokens: Int?`
- `totalCost: Double?`

---

## 5) Core Data Models (Fields & Defaults)

### `RateWindow`
- `usedPercent: Double`
- `windowMinutes: Int?`
- `resetsAt: Date?`
- `resetDescription: String?`
- `remainingPercent = max(0, 100 - usedPercent)`

### `ProviderIdentitySnapshot`
- `providerID: UsageProvider?`
- `accountEmail: String?`
- `accountOrganization: String?`
- `loginMethod: String?`
- `scoped(to:)` sets providerID when missing.

### `UsageSnapshot`
- `primary: RateWindow?`
- `secondary: RateWindow?`
- `tertiary: RateWindow?`
- `providerCost: ProviderCostSnapshot?`
- `zaiUsage: ZaiUsageSnapshot?` (not persisted)
- `minimaxUsage: MiniMaxUsageSnapshot?` (not persisted)
- `cursorRequests: CursorRequestUsage?` (not persisted)
- `updatedAt: Date`
- `identity: ProviderIdentitySnapshot?`

Encoding behavior:
- Always encodes window keys (`nil` encoded as `null`).
- Also encodes legacy identity fields (`accountEmail`, `accountOrganization`, `loginMethod`).

### `CreditsSnapshot`
- `remaining: Double`
- `events: [CreditEvent]`
- `updatedAt: Date`

### `OpenAIDashboardSnapshot`
- `signedInEmail: String?`
- `codeReviewRemainingPercent: Double?`
- `creditEvents: [CreditEvent]`
- `dailyBreakdown: [OpenAIDashboardDailyBreakdown]`
- `usageBreakdown: [OpenAIDashboardDailyBreakdown]`
- `creditsPurchaseURL: String?`
- `primaryLimit: RateWindow?`
- `secondaryLimit: RateWindow?`
- `creditsRemaining: Double?`
- `accountPlan: String?`
- `updatedAt: Date`

### Cost usage models
- `CostUsageTokenSnapshot`: session + last30 days + daily + updatedAt
- `CostUsageDailyReport.Entry`: date + token/cost fields + model breakdowns
- `CostUsageDailyReport.Summary`: totals

---

## 6) Provider Architecture

### Descriptor + Registry
- `ProviderDescriptor` fields:
  - `id: UsageProvider`
  - `metadata: ProviderMetadata`
  - `branding: ProviderBranding`
  - `tokenCost: ProviderTokenCostConfig`
  - `fetchPlan: ProviderFetchPlan`
  - `cli: ProviderCLIConfig`

- `ProviderDescriptorRegistry`
  - Static list of descriptors per `UsageProvider`.
  - `all` preserves registration order.
  - `cliNameMap` maps cli names + aliases to providers.

### Provider Metadata
- `displayName`, `sessionLabel`, `weeklyLabel`, `opusLabel`
- `supportsOpus`, `supportsCredits`
- `toggleTitle`, `cliName`, `defaultEnabled`
- `isPrimaryProvider`, `usesAccountFallback`
- `browserCookieOrder`, `dashboardURL`, `statusPageURL`, `statusLinkURL`, `statusWorkspaceProductID`

### Fetch Plan + Pipeline
- `ProviderSourceMode`: `auto`, `web`, `cli`, `oauth`.
- `ProviderFetchKind`: `cli`, `web`, `oauth`, `apiToken`, `localProbe`, `webDashboard`.
- `ProviderFetchStrategy`:
  - `id`, `kind`, `isAvailable`, `fetch`, `shouldFallback`.
- `ProviderFetchPipeline.fetch`:
  - Resolves ordered strategies.
  - For each: if available → try fetch.
  - On error: if strategy allows fallback, continue; else stop.
  - If none succeeded → `ProviderFetchError.noAvailableStrategy`.
- `ProviderFetchOutcome`:
  - `result: Result<ProviderFetchResult, Error>`
  - `attempts: [ProviderFetchAttempt]`

---

## 7) Providers & Data Sources (Summary)

**Legend:** web = browser cookies/WebView, cli = RPC/PTy, oauth = API, api = token API, local = local files/probe.

| Provider | Auto Strategy Order | Source Labels |
| --- | --- | --- |
| Codex | Web dashboard → CLI RPC/PTy | `openai-web`, `codex-cli` |
| Claude | OAuth → Web → CLI PTY | `oauth`, `web`, `claude` |
| Gemini | OAuth API | `api` |
| Antigravity | Local probe | `local` |
| Cursor | Web cookies | `web` |
| OpenCode | Web cookies | `web` |
| Factory | Web cookies + tokens | `web` |
| z.ai | API token | `api` |
| MiniMax | API token or web cookies | `api` / `web` |
| Kimi | API token | `api` |
| Copilot | API token | `api` |
| Kimi K2 | API token | `api` |
| Kiro | CLI command | `cli` |
| Vertex AI | OAuth | `oauth` |
| JetBrains AI | Local file | `local` |
| Amp | Web cookies | `web` |

---

## 8) Token Accounts (Multi‑Account Support)

### File Format
- Path: `~/Library/Application Support/CodexBar/token-accounts.json`
- Root object:
  - `version: Int`
  - `providers: { <providerID>: ProviderTokenAccountData }`

`ProviderTokenAccountData`:
- `version: Int`
- `accounts: [ProviderTokenAccount]`
- `activeIndex: Int`

`ProviderTokenAccount`:
- `id: UUID`
- `label: String`
- `token: String`
- `addedAt: TimeInterval`
- `lastUsed: TimeInterval?`

### CLI Selection Semantics
- `--account <label>`: match `label` case‑insensitively.
- `--account-index <n>`: 1‑based → 0‑based internal index.
- `--all-accounts`: iterate all accounts.
- If overrides are requested but no accounts exist → error.

### Supported Providers
Token account support is defined in `TokenAccountSupportCatalog`:
- **Claude**: sessionKey cookie or OAuth access token (`sk-ant-oat...`).
- **z.ai**: API token, injected via env var.
- **Cursor**: cookie header.
- **OpenCode**: cookie header.
- **Factory**: cookie header.
- **MiniMax**: cookie header.
- **Augment**: cookie header.

Special handling:
- Claude OAuth tokens (sk‑ant‑oat) are normalized (strip `Bearer `) and routed to OAuth fetch.

---

## 9) Defaults & Presentation Rules

- **Color**: only for text output when stdout is a TTY and `--no-color` not set; `TERM=dumb` disables.
- **Reset display**: `resetTimesShowAbsolute` in UserDefaults toggles absolute vs countdown.
- **Formatting**:
  - Usage bar width = 12.
  - Percent coloring: green (>=25), yellow (10–24), red (<10).
  - Credits formatting uses `en_US_POSIX` locale.

---

## 10) Status Fetching

- CLI `--status` fetches `<statusPageURL>/api/v2/status.json` with 10s timeout.
- Response maps to `ProviderStatusPayload`.
- Unknown or error maps to `indicator: unknown` with error description.

---

## 11) Logging

`CodexBarLog.bootstrapIfNeeded`:
- `--json-output` writes JSONL to stderr with fields:
  - `timestamp`, `level`, `label`, `message`, `source`, `file`, `function`, `line`, `metadata`.
- Log level selection:
  - explicit `--log-level` overrides.
  - `--verbose` maps to debug.
  - default to error.

---

## 12) Error Handling & Exit Codes

`mapError` in CLI:
- Exit `2`: binary not found, provider CLI not installed.
- Exit `4`: timeouts.
- Exit `3`: parse/format errors, unsupported provider, missing rate limits.
- Exit `1`: unexpected failures.

Exit code `0` on success.

---

## 13) Web Dashboard Cache (Codex)

`OpenAIDashboardCacheStore`:
- Path: `~/Library/Application Support/com.steipete.codexbar/openai-dashboard.json`.
- Cache is best‑effort (write errors ignored).
- Used by CLI JSON output when web fetch is not run but a matching account email exists.

---

## 14) Rust Port Mapping (`caut`)

This section maps CodexBar Swift constructs to their Rust equivalents in `caut`.

### Module Structure

| Swift | Rust |
|-------|------|
| `Sources/CodexBarCore/` | `src/core/` |
| `Sources/CodexBarCLI/` | `src/cli/` |
| Provider descriptors | `src/core/provider.rs` |
| Data models | `src/core/models.rs` |
| Fetch strategies | `src/core/fetch_plan.rs` |
| CLI entry | `src/main.rs` |

### Core Files

| Purpose | Rust File |
|---------|-----------|
| CLI argument parsing | `src/cli/args.rs` |
| Usage command impl | `src/cli/usage.rs` |
| Cost command impl | `src/cli/cost.rs` |
| Error types + exit codes | `src/error.rs` |
| Provider enum + registry | `src/core/provider.rs` |
| Data models (RateWindow, etc.) | `src/core/models.rs` |
| Fetch plan + pipeline | `src/core/fetch_plan.rs` |
| Status fetching | `src/core/status.rs` |
| Logging bootstrap | `src/core/logging.rs` |
| Human output rendering | `src/render/human.rs` |
| JSON/Markdown output | `src/render/robot.rs` |
| Token accounts storage | `src/storage/token_accounts.rs` |
| Path utilities | `src/storage/paths.rs` |
| Cache storage | `src/storage/cache.rs` |
| Time formatting | `src/util/time.rs` |
| Number formatting | `src/util/format.rs` |
| Environment detection | `src/util/env.rs` |

### Type Mappings

| Swift Type | Rust Type | Location |
|------------|-----------|----------|
| `UsageProvider` enum | `Provider` enum | `src/core/provider.rs` |
| `ProviderSelection` | `ProviderSelection` enum | `src/core/provider.rs` |
| `ProviderDescriptor` | `ProviderDescriptor` struct | `src/core/provider.rs` |
| `ProviderMetadata` | `ProviderMetadata` struct | `src/core/provider.rs` |
| `ProviderBranding` | `ProviderBranding` struct | `src/core/provider.rs` |
| `RateWindow` | `RateWindow` struct | `src/core/models.rs` |
| `UsageSnapshot` | `UsageSnapshot` struct | `src/core/models.rs` |
| `CreditsSnapshot` | `CreditsSnapshot` struct | `src/core/models.rs` |
| `ProviderPayload` | `ProviderPayload` struct | `src/core/models.rs` |
| `CostPayload` | `CostPayload` struct | `src/core/models.rs` |
| `ProviderFetchPlan` | `FetchPlan` struct | `src/core/fetch_plan.rs` |
| `ProviderFetchKind` | `FetchKind` enum | `src/core/fetch_plan.rs` |
| `ProviderFetchStrategy` | `FetchStrategy` trait | `src/core/fetch_plan.rs` |
| `ProviderTokenAccount` | `TokenAccount` struct | `src/storage/token_accounts.rs` |
| `ProviderStatusPayload` | `StatusPayload` struct | `src/core/status.rs` |

### CLI Flag Mappings

| Swift Flag | Rust (clap) | Location |
|------------|-------------|----------|
| `--format text\|json` | `--format` (OutputFormat enum) | `src/cli/args.rs` |
| `--json` | `--json` (bool) | `src/cli/args.rs` |
| `--provider` | `--provider` (String) | `src/cli/args.rs` |
| `--account` | `--account` (String) | `src/cli/args.rs` |
| `--account-index` | `--account-index` (usize) | `src/cli/args.rs` |
| `--all-accounts` | `--all-accounts` (bool) | `src/cli/args.rs` |
| `--status` | `--status` (bool) | `src/cli/args.rs` |
| `--no-color` | `--no-color` (bool) | `src/cli/args.rs` |
| `--pretty` | `--pretty` (bool) | `src/cli/args.rs` |
| `--verbose/-v` | `-v/--verbose` (bool) | `src/cli/args.rs` |
| `--log-level` | `--log-level` (String) | `src/cli/args.rs` |
| `--json-output` | `--json-output` (bool) | `src/cli/args.rs` |

### Error Exit Code Mapping

| Exit Code | Swift | Rust (`CautError`) |
|-----------|-------|-------------------|
| 0 | Success | `Ok(())` |
| 1 | Unexpected failure | `GeneralError` variants |
| 2 | Binary not found | `ProviderNotFound` |
| 3 | Parse/format error | `ParseError`, `InvalidProvider`, etc. |
| 4 | Timeout | `Timeout` |

See `src/error.rs` for the complete `CautError` enum and `exit_code()` mapping.

### Dependencies

| Swift Dependency | Rust Crate | Purpose |
|------------------|------------|---------|
| Commander | `clap` | CLI argument parsing |
| Foundation.JSONEncoder | `serde_json` | JSON serialization |
| os.Logger | `tracing` + `tracing-subscriber` | Logging |
| NSHomeDirectory | `directories` | Platform paths |
| Keychain | `keyring` | Secure credential storage |
| URLSession | `reqwest` | HTTP client |
| - | `rich_rust` | Terminal formatting (replaces ANSI/Foundation) |
| - | `crossterm` | Terminal capabilities |
| - | `chrono` | Date/time handling |
| - | `anyhow` + `thiserror` | Error handling |
| - | `tokio` | Async runtime |

### Cross-Platform Paths

| Platform | Config Dir | Data Dir |
|----------|------------|----------|
| macOS | `~/Library/Application Support/caut/` | Same |
| Linux | `~/.config/caut/` | `~/.local/share/caut/` |
| Windows | `%APPDATA%\caut\` | Same |

Path resolution uses the `directories` crate. See `src/storage/paths.rs`.

