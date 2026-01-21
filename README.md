<p align="center">
  <img src="caut_illustration.webp" alt="caut - Coding Agent Usage Tracker" width="800">
</p>

<h1 align="center">caut</h1>

<p align="center">
  <strong>Coding Agent Usage Tracker — monitor LLM provider usage from a single CLI</strong>
</p>

<p align="center">
  <a href="https://github.com/Dicklesworthstone/coding_agent_usage_tracker/actions/workflows/ci.yml"><img src="https://github.com/Dicklesworthstone/coding_agent_usage_tracker/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.88%2B-orange.svg" alt="Rust 1.88+"></a>
</p>

---

## Quick Install

```bash
# From source (recommended)
cargo install --git https://github.com/Dicklesworthstone/coding_agent_usage_tracker

# Then run
caut usage
```

---

## TL;DR

### The Problem

You're using multiple AI coding assistants—Codex, Claude, Gemini, Cursor, Copilot—each with different rate limits, credits, and usage dashboards. Tracking your remaining quota means:

- Opening 5+ browser tabs
- Remembering different login flows
- Mentally aggregating disparate data formats

### The Solution

**caut** fetches usage data from **16+ LLM providers** through a single CLI command. Human-readable tables for your terminal, or structured JSON/Markdown for AI agents to consume.

```bash
$ caut usage

╭─ Codex (openai-web) ─────────────────────────────────────╮
│ Session  72% left   [========----]  resets in 2h 15m     │
│ Weekly   41% left   [====--------]  resets Fri 9am       │
│ Credits  112.4 left                                      │
│ Account  user@example.com                                │
│ Plan     Pro                                             │
╰──────────────────────────────────────────────────────────╯

╭─ Claude (oauth) ─────────────────────────────────────────╮
│ Chat     85% left   [==========--]  resets in 4h         │
│ Weekly   62% left   [======------]  resets Mon 12am      │
│ Opus     45% left   [====--------]  separate tier        │
│ Account  claude@example.com                              │
╰──────────────────────────────────────────────────────────╯
```

### Why caut?

| Feature | caut | Manual Checking |
|---------|------|-----------------|
| **Single command** | `caut usage --provider all` | Open 5+ dashboards |
| **Rate limit tracking** | Session + weekly + opus tiers | Mental math |
| **Credits monitoring** | Real-time balance | Refresh each page |
| **Status awareness** | Provider outage alerts | Check status pages |
| **Robot mode** | JSON/Markdown for agents | N/A |
| **Cross-platform** | macOS, Linux, Windows | Browser-only |
| **Cost tracking** | Local JSONL scanning | Export CSVs manually |

---

## AGENTS.md

This project includes an [`AGENTS.md`](AGENTS.md) file with detailed guidelines for AI coding agents. If you're an AI assistant working on this codebase, read that file for:

- Toolchain requirements (Rust 2024, nightly)
- Code editing discipline and patterns
- Testing and CI/CD requirements
- Multi-agent coordination via MCP Agent Mail
- Beads workflow integration

---

## Design Philosophy

### 1. Dual-Mode Output

Human mode uses **rich terminal formatting** with colored bars, panels, and progress indicators. Robot mode emits **stable JSON/Markdown schemas** designed for token efficiency when consumed by AI agents.

```bash
# Human mode (default)
caut usage

# Robot mode for AI agents
caut usage --json
caut usage --format md
```

### 2. Provider Abstraction

Each of the 16 providers has a **descriptor** with metadata, branding, and fetch strategies. Adding a new provider means implementing one trait—not touching core logic.

```
┌─────────────────────────────────────────────────────────┐
│                   Provider Registry                      │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐       │
│  │  Codex  │ │ Claude  │ │ Gemini  │ │   ...   │       │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘       │
└─────────────────────────────────────────────────────────┘
```

### 3. Fail Gracefully

Network timeouts, missing credentials, and provider outages are handled with **clear error messages and partial results**—never crash, always inform.

### 4. Zero Configuration

Works out of the box by detecting installed CLI tools and browser cookies. Optional config file for power users who want to customize behavior.

### 5. CodexBar Parity

A faithful port of [CodexBar](https://github.com/steipete/codexbar)'s CLI functionality to cross-platform Rust, preserving all commands, flags, and output formats.

---

## Comparison

| Feature | caut | CodexBar | Manual |
|---------|------|----------|--------|
| **Platform** | macOS, Linux, Windows | macOS only | Any |
| **Language** | Rust | Swift | N/A |
| **Providers** | 16+ | 16 | 1 per tab |
| **Robot mode** | JSON + Markdown | JSON | None |
| **Installation** | Single binary | App bundle | N/A |
| **Menu bar UI** | No (CLI only) | Yes | No |
| **Memory usage** | ~10MB | ~50MB | Varies |
| **Startup time** | ~10ms | ~500ms | N/A |

---

## Origins

**caut** is a cross-platform Rust port of [CodexBar](https://github.com/steipete/codexbar) by Peter Steinberger. CodexBar is a beautiful macOS menu bar app for tracking AI coding tool usage. caut brings that functionality to the command line, running anywhere Rust compiles.

**Author:** [Jeffrey Emanuel](https://github.com/Dicklesworthstone)

---

## Installation

### From Source (Recommended)

```bash
git clone https://github.com/Dicklesworthstone/coding_agent_usage_tracker
cd coding_agent_usage_tracker
cargo install --path .
```

### From Git

```bash
cargo install --git https://github.com/Dicklesworthstone/coding_agent_usage_tracker
```

### Requirements

| Requirement | Version | Notes |
|-------------|---------|-------|
| **Rust** | 1.88+ | Nightly required for edition 2024 |
| **OpenSSL** | Any | Linux only, for TLS |

### Verify Installation

```bash
caut --version
# caut 0.1.0 (abc1234 2026-01-18)
```

---

## Quick Start

### 1. Check Your Usage

```bash
# Primary providers (Codex + Claude)
caut usage

# All configured providers
caut usage --provider all

# Specific provider
caut usage --provider gemini
```

### 2. View Cost Data

```bash
# Claude and Codex local cost scan
caut cost

# Force refresh cached data
caut cost --refresh
```

### 3. Robot Mode for AI Agents

```bash
# Compact JSON (one line)
caut usage --json

# Pretty-printed JSON
caut usage --json --pretty

# Markdown output
caut usage --format md
```

### 4. Include Status

```bash
# Check provider operational status
caut usage --status
```

---

## Commands

### `caut` (no args)

Prints quickstart help with top commands and examples.

### `caut usage`

Show rate limit usage for selected providers.

```
USAGE:
    caut usage [OPTIONS]

OPTIONS:
    --provider <NAME|both|all>  Provider selection (default: both)
    --account <LABEL>           Use specific account
    --account-index <N>         Use account by index (1-based)
    --all-accounts              Query all configured accounts
    --no-credits                Hide credits in human output
    --status                    Fetch provider status
    --source <auto|web|cli|oauth>  Data source preference
    --web                       Shorthand for --source web
    --timeout <SECONDS>         Per-provider fetch timeout override
    --web-timeout <SECONDS>     Web fetch timeout (default: 30)
```

### `caut cost`

Show local cost usage from JSONL logs.

```
USAGE:
    caut cost [OPTIONS]

OPTIONS:
    --provider <NAME|both|all>  Provider selection (default: both)
    --refresh                   Ignore cache, rescan files
```

### `caut token-accounts`

Manage multi-account configurations.

```
USAGE:
    caut token-accounts list [--provider <NAME>]
    caut token-accounts convert --from <FORMAT> --to <FORMAT>
```

### Global Options

```
--format <human|json|md>  Output format (default: human)
--json                    Shorthand for --format json
--pretty                  Pretty-print JSON
--no-color                Disable colored output
--log-level <LEVEL>       Log level (trace|debug|info|warn|error)
--json-output             Emit JSONL logs to stderr
-v, --verbose             Enable debug logging
```

---

## Supported Providers

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

---

## Data Sources

caut uses multiple strategies to fetch usage data:

| Source | How it works | Platform |
|--------|--------------|----------|
| **CLI** | Invokes provider CLI via PTY | All |
| **Web** | Reads browser cookies | macOS |
| **OAuth** | Uses stored tokens | All |
| **API** | Direct API calls | All |
| **Local** | Scans local JSONL logs | All |

Priority order: CLI → Web → OAuth → API → Local (configurable via `--source`)

---

## Configuration

### Config File Location

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/caut/config.toml` |
| Linux | `~/.config/caut/config.toml` |
| Windows | `%APPDATA%\caut\config.toml` |

### Example Config

```toml
# Default provider selection
default_provider = "both"

# Output preferences
[output]
format = "human"
color = true

# Provider toggles
[providers]
codex = true
claude = true
gemini = false
cursor = false

# Web fetch settings
[web]
timeout_seconds = 30
```

### Token Accounts

Multi-account support uses `token-accounts.json`:

```json
{
  "version": 1,
  "providers": {
    "claude": {
      "accounts": [
        {
          "id": "uuid-here",
          "label": "personal",
          "token": "sk-ant-...",
          "addedAt": 1704067200
        }
      ],
      "activeIndex": 0
    }
  }
}
```

---

## JSON Output Schema

### Schema Version

All JSON output includes `schemaVersion: "caut.v1"` for forward compatibility.

### Usage Response

```json
{
  "schemaVersion": "caut.v1",
  "generatedAt": "2026-01-18T12:00:00Z",
  "command": "usage",
  "data": [
    {
      "provider": "codex",
      "account": "user@example.com",
      "version": "0.6.0",
      "source": "openai-web",
      "status": {
        "indicator": "none",
        "description": "Operational",
        "updatedAt": "2026-01-18T11:00:00Z",
        "url": "https://status.openai.com"
      },
      "usage": {
        "primary": {
          "usedPercent": 28.0,
          "remainingPercent": 72.0,
          "windowMinutes": 180,
          "resetsAt": "2026-01-18T14:15:00Z"
        },
        "secondary": {
          "usedPercent": 59.0,
          "remainingPercent": 41.0,
          "windowMinutes": 10080,
          "resetsAt": "2026-01-24T09:00:00Z"
        },
        "tertiary": null,
        "updatedAt": "2026-01-18T12:00:00Z",
        "identity": {
          "accountEmail": "user@example.com",
          "loginMethod": "google"
        }
      },
      "credits": {
        "remaining": 112.4,
        "events": [],
        "updatedAt": "2026-01-18T12:00:00Z"
      }
    }
  ],
  "errors": [],
  "meta": {
    "format": "json",
    "runtime": "cli"
  }
}
```

### Cost Response

```json
{
  "schemaVersion": "caut.v1",
  "generatedAt": "2026-01-18T12:00:00Z",
  "command": "cost",
  "data": [
    {
      "provider": "claude",
      "source": "local",
      "updatedAt": "2026-01-18T12:00:00Z",
      "sessionCostUSD": 2.45,
      "sessionTokens": 124500,
      "last30DaysCostUSD": 47.82,
      "last30DaysTokens": 2400000,
      "daily": [
        {
          "date": "2026-01-18",
          "totalTokens": 124500,
          "totalCost": 2.45,
          "modelsUsed": ["claude-3-opus", "claude-3-sonnet"]
        }
      ],
      "totals": {
        "inputTokens": 1800000,
        "outputTokens": 600000,
        "totalTokens": 2400000,
        "totalCost": 47.82
      }
    }
  ],
  "errors": [],
  "meta": {
    "format": "json",
    "runtime": "cli"
  }
}
```

---

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                         CLI Entry                             │
│                        (src/main.rs)                          │
└──────────────────────────┬───────────────────────────────────┘
                           │
              ┌────────────┴────────────┐
              │                         │
              ▼                         ▼
    ┌─────────────────┐       ┌─────────────────┐
    │  Usage Command  │       │  Cost Command   │
    │ (cli/usage.rs)  │       │  (cli/cost.rs)  │
    └────────┬────────┘       └────────┬────────┘
             │                         │
             └──────────┬──────────────┘
                        │
                        ▼
    ┌──────────────────────────────────────────────────────────┐
    │                   Provider Registry                       │
    │                  (core/provider.rs)                       │
    │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐        │
    │  │  Codex  │ │ Claude  │ │ Gemini  │ │   ...   │        │
    │  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘        │
    └───────┼───────────┼───────────┼───────────┼─────────────┘
            │           │           │           │
            └─────┬─────┴─────┬─────┴─────┬─────┘
                  │           │           │
                  ▼           ▼           ▼
    ┌─────────────────────────────────────────────────────────┐
    │                    Fetch Strategies                      │
    │                   (core/fetch_plan.rs)                   │
    │                                                          │
    │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
    │  │   CLI    │  │   Web    │  │  OAuth   │  │   API   │ │
    │  │  (PTY)   │  │ (cookies)│  │ (tokens) │  │ (keys)  │ │
    │  └──────────┘  └──────────┘  └──────────┘  └─────────┘ │
    └─────────────────────────────────────────────────────────┘
                              │
                              ▼
    ┌─────────────────────────────────────────────────────────┐
    │                      Renderers                           │
    │                    (render/*.rs)                         │
    │                                                          │
    │  ┌──────────────────┐    ┌──────────────────┐           │
    │  │   Human Mode     │    │   Robot Mode     │           │
    │  │   (rich_rust)    │    │   (JSON/MD)      │           │
    │  └──────────────────┘    └──────────────────┘           │
    └─────────────────────────────────────────────────────────┘
```

---

## Exit Codes

| Code | Meaning | Example |
|------|---------|---------|
| `0` | Success | Normal operation |
| `1` | General error | Network failure, I/O error |
| `2` | Binary not found | Provider CLI not installed |
| `3` | Parse/config error | Invalid arguments, bad JSON |
| `4` | Timeout | Web fetch exceeded limit |

---

## Performance

caut is designed for **fast startup** and **minimal resource usage**:

| Metric | Value |
|--------|-------|
| Binary size | ~3MB (release, stripped) |
| Startup time | ~10ms |
| Memory usage | ~10MB peak |
| First response | <500ms (cached) |

Release builds use aggressive optimization:

```toml
[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Better optimization
panic = "abort"     # Smaller binary
strip = true        # Remove symbols
```

---

## Troubleshooting

### "Provider CLI not found" (Exit 2)

The provider's CLI tool isn't installed or not in PATH.

```bash
# Check if codex is installed
which codex

# Check if claude is installed
which claude
```

### "No available fetch strategy" (Exit 3)

No data source is available for the provider:

- **Web source**: Requires browser cookies (macOS only)
- **CLI source**: Requires provider CLI installed
- **OAuth source**: Requires token configuration

Try specifying a source: `caut usage --source cli`

### "Request timeout" (Exit 4)

Web fetch took too long. Increase timeout:

```bash
caut usage --web-timeout 60
```

### Colors not showing

TTY detection may fail in some terminals:

```bash
# Force colors
TERM=xterm-256color caut usage

# Or disable if corrupted
caut usage --no-color
```

### Cache issues

Force refresh cached data:

```bash
caut cost --refresh
```

---

## Limitations

| Limitation | Workaround |
|------------|------------|
| **No GUI** | Use [CodexBar](https://github.com/steipete/codexbar) for menu bar app |
| **Web sources (macOS only)** | Use CLI or OAuth sources on Linux/Windows |
| **Local cost scanning** | Only Codex and Claude support JSONL log scanning |
| **Token account sync** | Manual token management required |

---

## FAQ

### How is this different from CodexBar?

caut is a cross-platform CLI port of CodexBar's core functionality. CodexBar is a macOS-native menu bar app with a GUI. caut runs anywhere Rust compiles.

### Can I use this with my AI agent?

Yes. Use `--json` or `--format md` for structured output. The schema is stable and versioned (`caut.v1`).

### Does it store my credentials?

Token accounts are stored locally in `token-accounts.json`. Passwords/cookies are read from your system keychain or browser profile—never stored by caut.

### How do I add a new provider?

Implement the `FetchStrategy` trait in `src/providers/` and register in the provider registry. See existing providers for examples.

### Why Rust?

- Single static binary, no runtime dependencies
- Cross-platform (macOS, Linux, Windows)
- Memory safety without GC overhead
- Fast startup (~10ms) for CLI tool ergonomics

### Why not just use each provider's API?

Most providers don't expose rate limit data via API. caut uses the same data sources as official dashboards: CLI RPC, browser cookies, and OAuth tokens.

---

## About Contributions

Please don't take this the wrong way, but I do not accept outside contributions for any of my projects. I simply don't have the mental bandwidth to review anything, and it's my name on the thing, so I'm responsible for any problems it causes; thus, the risk-reward is highly asymmetric from my perspective. I'd also have to worry about other "stakeholders," which seems unwise for tools I mostly make for myself for free. Feel free to submit issues, and even PRs if you want to illustrate a proposed fix, but know I won't merge them directly. Instead, I'll have Claude or Codex review submissions via `gh` and independently decide whether and how to address them. Bug reports in particular are welcome. Sorry if this offends, but I want to avoid wasted time and hurt feelings. I understand this isn't in sync with the prevailing open-source ethos that seeks community contributions, but it's the only way I can move at this velocity and keep my sanity.

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Acknowledgments

- [CodexBar](https://github.com/steipete/codexbar) by Peter Steinberger — the original macOS app this project ports
- [rich_rust](https://crates.io/crates/rich_rust) — Rust port of Python Rich for terminal output
- [clap](https://crates.io/crates/clap) — CLI argument parsing
- [tokio](https://tokio.rs) — async runtime
- [serde](https://serde.rs) — serialization framework
