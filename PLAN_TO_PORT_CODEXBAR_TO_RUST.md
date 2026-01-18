# Plan: Port CodexBar "guts" to Rust (caut)

## Executive Summary

We will port CodexBar’s CLI + core provider pipeline (usage, credits, cost scans, status) into a Rust CLI called `caut`
(coding_agent_usage_tracker). This is a **spec-first** port: extract behavior from CodexBar, design a Rust architecture,
then implement without referencing the legacy code during implementation. The goal is a dual‑mode CLI:

- **Human mode**: rich, readable console output using `rich_rust`.
- **Robot mode**: highly structured, token-efficient JSON or Markdown designed for coding agents.

## Scope (In)

1. **CLI parity with CodexBar (full provider coverage)**
   - Commands: `usage` (default) and `cost`.
   - Flags: provider selection, source selection, output format, status fetch, token accounts, verbosity/logging, timeouts.
2. **Core data model parity**
   - `RateWindow`, `UsageSnapshot`, `CreditsSnapshot`, `OpenAIDashboardSnapshot`, cost usage models.
3. **Provider fetch pipeline**
   - Descriptor registry, fetch strategy pipeline, ordered fallbacks with debug attempts.
4. **Token accounts**
   - `token-accounts.json` format + selection semantics.
   - Add **conversion mode** between CodexBar and `caut` formats.
5. **Status polling**
   - Statuspage and Workspace incident checks (same behavior, used by CLI `--status`).
6. **Local cost usage scanning**
   - Claude + Codex JSONL scanning with caching and a 30‑day rolling window.
7. **Human/robot output contracts**
   - Rich text UI for humans, stable JSON/Markdown for agents.
   - Add **compatibility output** to round-trip CodexBar JSON when needed.

## Explicit Exclusions (Out of Scope for Initial Port)

- **Menu bar UI** (SwiftUI app, widgets, status items, icon rendering).
- **App settings UI** (Preferences panes, toggles, and in‑app features).
- **Sparkle updates, macOS packaging, and notarization.**
- **Swift macros, SwiftPM, and macOS‑specific UI frameworks.**
- **Anything requiring WebKit in a GUI** (use HTTP/cookie import/headless only; no GUI webview).
- **Non‑CLI helper processes** (e.g., `CodexBarClaudeWatchdog`, `CodexBarClaudeWebProbe`).

If you want parity with those later, we can add a second phase.

## Deliverables

- `EXISTING_CODEXBAR_STRUCTURE.md` — exhaustive spec of CodexBar CLI + core behavior.
- `PROPOSED_ARCHITECTURE.md` — Rust architecture and CLI contracts for `caut`.
- (Later) Rust implementation + tests.

## Phases

### Phase 1 — Spec Extraction (current)
- Deep‑dive CodexBar docs and source.
- Extract data models, CLI flags, output formats, and provider pipeline.
- Produce `EXISTING_CODEXBAR_STRUCTURE.md`.

### Phase 2 — Architecture Synthesis
- Design Rust module boundaries.
- Define `caut` CLI commands/flags and output contracts.
- Document human + robot output schemas.
- Produce `PROPOSED_ARCHITECTURE.md`.

### Phase 3 — Implementation (later)
- Implement CLI skeleton and core types.
- Implement provider registry.
- Implement **all providers** (full parity).
- Add token accounts + conversion mode.
- Add cost usage scan.

## Risks / Open Questions

- **Provider coverage**: full parity vs. staged rollout (Codex + Claude first).
- **Web scraping**: CodexBar uses WebKit; Rust CLI will need HTTP‑only or headless browser approach.
- **Auth storage**: Keychain integration is macOS‑specific; for Rust CLI we should use keyring crate + file fallback.
- **Cross‑platform**: Linux should skip web/cookie sources by default (CodexBar exits with error for web on non‑macOS).
- **Robot mode contract**: choose JSON vs Markdown defaults and ensure strict stability.

## Decisions (Confirmed)

1. **Full provider parity** from the start.
2. **Conversion mode** between CodexBar and `caut` token-account formats.
3. **Robot mode default: JSON**, with Markdown supported.
4. **Config**: `caut` owns its config, but can import CodexBar defaults.
5. **Conversion formats**: CodexBar ↔ `caut` only (for now).
