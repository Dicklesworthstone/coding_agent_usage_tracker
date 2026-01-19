# Rich Rust Integration Plan for caut

> **Goal:** Transform caut's human-mode console output into a visually stunning, premium CLI experience using rich_rust's full feature set, while maintaining perfect compatibility with robot-mode (JSON/Markdown) for AI agent consumers.

---

## Executive Summary

This plan details how to deeply integrate `/dp/rich_rust` throughout the `caut` codebase to create world-class terminal output. The integration follows a **dual-track approach**:

1. **Human Mode**: Rich, colorful, premium visual experience using panels, tables, progress bars, trees, syntax highlighting, and styled text
2. **Robot Mode**: Unchanged - clean JSON/Markdown for AI agents (primary users)

**Critical Constraint:** All rich formatting MUST be gated behind TTY detection and human output mode checks. Agents must never receive ANSI codes in their output streams.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Safety Gates: Protecting Agent Users](#2-safety-gates-protecting-agent-users)
3. [Console Infrastructure](#3-console-infrastructure)
4. [Usage Command Integration](#4-usage-command-integration)
5. [Cost Command Integration](#5-cost-command-integration)
6. [Doctor Command Integration](#6-doctor-command-integration)
7. [History Command Integration](#7-history-command-integration)
8. [Error Display Integration](#8-error-display-integration)
9. [Progress & Loading States](#9-progress--loading-states)
10. [Startup & Branding](#10-startup--branding)
11. [Help & Documentation](#11-help--documentation)
12. [Color Theming System](#12-color-theming-system)
13. [Implementation Phases](#13-implementation-phases)
14. [File-by-File Changes](#14-file-by-file-changes)
15. [Testing Strategy](#15-testing-strategy)
16. [Performance Considerations](#16-performance-considerations)

---

## 1. Architecture Overview

### Current Architecture
```
CLI Args → Command Dispatch → Fetch/Process → Render (human.rs/robot.rs) → stdout
```

### Enhanced Architecture
```
CLI Args → Command Dispatch → Fetch/Process → OutputMode Check
                                                    │
                                    ┌───────────────┴───────────────┐
                                    │                               │
                              Human Mode                       Robot Mode
                                    │                               │
                           ┌────────┴────────┐                      │
                           │                 │                      │
                      TTY Detected      Non-TTY                     │
                           │                 │                      │
                    rich_rust Output   Plain Text              JSON/Markdown
                    (Panels, Tables,   (No ANSI)               (Unchanged)
                     Colors, etc.)
```

### New Module Structure
```
src/
├── render/
│   ├── mod.rs           # OutputMode enum, dispatch logic
│   ├── human.rs         # REWRITE: rich_rust integration
│   ├── robot.rs         # UNCHANGED: JSON/Markdown
│   ├── theme.rs         # NEW: Color theme definitions
│   ├── components.rs    # NEW: Reusable rich components
│   └── branding.rs      # NEW: Logo, banner, startup display
```

---

## 2. Safety Gates: Protecting Agent Users

### Primary Safety Mechanism

```rust
// src/render/mod.rs

/// Determines if rich output should be used
pub fn should_use_rich_output(format: OutputFormat, force_color: bool) -> bool {
    match format {
        OutputFormat::Json | OutputFormat::Md => false,  // NEVER rich for robot mode
        OutputFormat::Human => {
            if std::env::var("NO_COLOR").is_ok() {
                return false;
            }
            if std::env::var("CAUT_PLAIN").is_ok() {
                return false;
            }
            if force_color {
                return true;
            }
            // Only use rich if connected to TTY
            std::io::stdout().is_terminal()
        }
    }
}
```

### Environment Variable Support

| Variable | Effect |
|----------|--------|
| `NO_COLOR` | Disable all ANSI codes (standard) |
| `CAUT_PLAIN` | Force plain text output |
| `FORCE_COLOR=1` | Force colors even without TTY |
| `CAUT_THEME` | Select color theme (see Section 12) |

### CLI Flags

```rust
// Add to src/cli/args.rs

#[derive(Args)]
pub struct GlobalArgs {
    // Existing...

    /// Force plain text output (no colors/styling)
    #[arg(long, global = true)]
    pub plain: bool,

    /// Force colors even when not connected to terminal
    #[arg(long, global = true, conflicts_with = "plain")]
    pub force_color: bool,
}
```

---

## 3. Console Infrastructure

### Create Shared Console Instance

```rust
// src/render/components.rs

use rich_rust::prelude::*;
use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Global console instance for human output
pub static CONSOLE: Lazy<Mutex<Console>> = Lazy::new(|| {
    Mutex::new(
        Console::builder()
            .markup(true)
            .emoji(true)
            .build()
    )
});

/// Get console with proper configuration
pub fn get_console(force_color: bool, no_color: bool) -> Console {
    let mut builder = Console::builder()
        .markup(true)
        .emoji(true);

    if no_color {
        builder = builder.no_color();
    }

    if force_color {
        builder = builder.force_terminal(true);
    }

    builder.build()
}
```

### Reusable Component Library

```rust
// src/render/components.rs

use crate::render::theme::Theme;

/// Create a styled panel for provider data
pub fn provider_panel(
    provider_name: &str,
    source: &str,
    content: Vec<Segment<'_>>,
    theme: &Theme,
) -> Panel<'_> {
    Panel::new(vec![content])
        .title(Text::styled(
            format!(" {} ", provider_name),
            theme.provider_title.clone()
        ))
        .subtitle(Text::styled(
            format!(" {} ", source),
            theme.source_label.clone()
        ))
        .border_style(theme.panel_border.clone())
        .rounded()
        .padding((0, 1))
}

/// Create a usage progress bar
pub fn usage_bar(
    label: &str,
    percent_remaining: f64,
    reset_info: Option<&str>,
    theme: &Theme,
) -> Vec<Segment<'static>> {
    let bar_width = 20;
    let filled = ((percent_remaining / 100.0) * bar_width as f64) as usize;

    let bar_style = if percent_remaining > 25.0 {
        theme.usage_good.clone()
    } else if percent_remaining > 10.0 {
        theme.usage_warning.clone()
    } else {
        theme.usage_critical.clone()
    };

    let mut bar = ProgressBar::with_total(100)
        .width(bar_width)
        .completed_style(bar_style)
        .show_percentage(false);
    bar.set_progress(percent_remaining / 100.0);

    // Build line: "Session  72% left  [========----]  resets in 2h 15m"
    let mut segments = vec![
        Segment::styled(format!("{:8}", label), theme.label.clone()),
        Segment::plain("  "),
        Segment::styled(format!("{:3.0}% left", percent_remaining),
            if percent_remaining > 25.0 { theme.percent_good.clone() }
            else if percent_remaining > 10.0 { theme.percent_warning.clone() }
            else { theme.percent_critical.clone() }
        ),
        Segment::plain("  "),
    ];

    segments.extend(bar.render(bar_width));

    if let Some(reset) = reset_info {
        segments.push(Segment::plain("  "));
        segments.push(Segment::styled(reset.to_string(), theme.reset_time.clone()));
    }

    segments.push(Segment::line());
    segments
}

/// Create a styled table for data display
pub fn data_table(
    title: Option<&str>,
    headers: &[&str],
    rows: Vec<Vec<String>>,
    theme: &Theme,
) -> Table {
    let mut table = Table::new()
        .border_style(theme.table_border.clone())
        .header_style(theme.table_header.clone())
        .rounded();

    if let Some(t) = title {
        table = table.title(Text::styled(t, theme.table_title.clone()));
    }

    for header in headers {
        table = table.with_column(Column::new(*header));
    }

    for row in rows {
        table.add_row_cells(row);
    }

    table
}

/// Create a status indicator
pub fn status_indicator(status: &str, theme: &Theme) -> Segment<'static> {
    match status.to_lowercase().as_str() {
        "operational" | "none" => Segment::styled(
            format!(" {} Operational", theme.icons.check),
            theme.status_ok.clone()
        ),
        "minor" => Segment::styled(
            format!(" {} Minor Issues", theme.icons.warning),
            theme.status_warning.clone()
        ),
        "major" | "critical" => Segment::styled(
            format!(" {} Service Disruption", theme.icons.error),
            theme.status_error.clone()
        ),
        _ => Segment::plain(status.to_string()),
    }
}

/// Create a styled error panel
pub fn error_panel(
    title: &str,
    message: &str,
    suggestions: Option<&[String]>,
    theme: &Theme,
) -> Panel<'static> {
    let mut content = vec![
        Segment::styled(message.to_string(), theme.error_message.clone()),
        Segment::line(),
    ];

    if let Some(suggs) = suggestions {
        content.push(Segment::line());
        content.push(Segment::styled("Suggestions:".to_string(), theme.suggestion_header.clone()));
        content.push(Segment::line());

        for sug in suggs {
            content.push(Segment::styled(format!("  {} {}", theme.icons.arrow_right, sug),
                theme.suggestion_text.clone()));
            content.push(Segment::line());
        }
    }

    Panel::new(vec![content])
        .title(Text::styled(format!(" {} {} ", theme.icons.error, title), theme.error_title.clone()))
        .border_style(theme.error_border.clone())
        .rounded()
}

/// Create a warning panel
pub fn warning_panel(title: &str, message: &str, theme: &Theme) -> Panel<'static> {
    Panel::new(vec![vec![
        Segment::styled(message.to_string(), theme.warning_message.clone()),
    ]])
        .title(Text::styled(format!(" {} {} ", theme.icons.warning, title), theme.warning_title.clone()))
        .border_style(theme.warning_border.clone())
        .rounded()
}

/// Create a success panel
pub fn success_panel(title: &str, message: &str, theme: &Theme) -> Panel<'static> {
    Panel::new(vec![vec![
        Segment::styled(message.to_string(), theme.success_message.clone()),
    ]])
        .title(Text::styled(format!(" {} {} ", theme.icons.check, title), theme.success_title.clone()))
        .border_style(theme.success_border.clone())
        .rounded()
}
```

---

## 4. Usage Command Integration

### Current State (src/render/human.rs)
Basic text output with some color codes.

### Enhanced Implementation

```rust
// src/render/human.rs - COMPLETE REWRITE

use rich_rust::prelude::*;
use crate::core::models::{ProviderPayload, UsageSnapshot, CreditsSnapshot};
use crate::render::theme::Theme;
use crate::render::components::*;

pub fn render_usage(
    payloads: &[ProviderPayload],
    include_status: bool,
    theme: &Theme,
) -> String {
    let console = get_console(false, false);
    let mut output = String::new();

    for (i, payload) in payloads.iter().enumerate() {
        if i > 0 {
            output.push('\n');
        }

        output.push_str(&render_provider_card(payload, include_status, theme));
    }

    output
}

fn render_provider_card(
    payload: &ProviderPayload,
    include_status: bool,
    theme: &Theme,
) -> String {
    let console = get_console(false, false);

    // Build content segments
    let mut content_lines: Vec<Vec<Segment<'static>>> = Vec::new();

    // Usage bars
    if let Some(primary) = &payload.usage.primary {
        let remaining = 100.0 - primary.used_percent;
        let reset_str = primary.reset_description.as_deref();
        content_lines.push(usage_bar("Session", remaining, reset_str, theme));
    }

    if let Some(secondary) = &payload.usage.secondary {
        let remaining = 100.0 - secondary.used_percent;
        let reset_str = secondary.reset_description.as_deref();
        content_lines.push(usage_bar("Weekly", remaining, reset_str, theme));
    }

    if let Some(tertiary) = &payload.usage.tertiary {
        let remaining = 100.0 - tertiary.used_percent;
        let reset_str = tertiary.reset_description.as_deref();
        content_lines.push(usage_bar("Opus", remaining, reset_str, theme));
    }

    // Credits (if available)
    if let Some(credits) = &payload.credits {
        content_lines.push(vec![
            Segment::styled("Credits ".to_string(), theme.label.clone()),
            Segment::plain(" "),
            Segment::styled(
                format!("{:.1} remaining", credits.remaining),
                theme.credits_value.clone()
            ),
            Segment::line(),
        ]);
    }

    // Account info
    if let Some(identity) = &payload.usage.identity {
        if let Some(email) = &identity.account_email {
            content_lines.push(vec![
                Segment::styled("Account ".to_string(), theme.label.clone()),
                Segment::plain(" "),
                Segment::styled(email.clone(), theme.account_value.clone()),
                Segment::line(),
            ]);
        }
    }

    // Status (if requested)
    if include_status {
        if let Some(status) = &payload.status {
            content_lines.push(vec![
                Segment::styled("Status  ".to_string(), theme.label.clone()),
                Segment::plain(" "),
                status_indicator(&status.indicator.to_string(), theme),
                Segment::line(),
            ]);
        }
    }

    // Create provider panel
    let panel = Panel::new(content_lines)
        .title(Text::styled(
            format!(" {} {} ", provider_icon(&payload.provider), payload.provider.to_uppercase()),
            theme.provider_title.clone()
        ))
        .subtitle(Text::styled(
            format!(" {} ", payload.source),
            theme.source_label.clone()
        ))
        .subtitle_align(JustifyMethod::Right)
        .border_style(provider_border_style(&payload.provider, theme))
        .rounded()
        .width(60)
        .padding((0, 1));

    // Render to string
    let segments = panel.render(60);
    segments_to_string(&segments, &console)
}

fn provider_icon(provider: &str) -> &'static str {
    match provider.to_lowercase().as_str() {
        "codex" => "🤖",
        "claude" => "🧠",
        "gemini" => "💎",
        "cursor" => "➜",
        "copilot" => "🚀",
        "vertex" | "vertexai" => "☁️",
        "jetbrains" => "🔧",
        _ => "📊",
    }
}

fn provider_border_style(provider: &str, theme: &Theme) -> Style {
    match provider.to_lowercase().as_str() {
        "codex" => theme.codex_border.clone(),
        "claude" => theme.claude_border.clone(),
        "gemini" => theme.gemini_border.clone(),
        "cursor" => theme.cursor_border.clone(),
        _ => theme.default_border.clone(),
    }
}

fn segments_to_string(segments: &[Segment<'_>], console: &Console) -> String {
    let mut output = String::new();
    for segment in segments {
        if let Some(style) = &segment.style {
            output.push_str(&style.render(&segment.text, console.color_system().unwrap_or(ColorSystem::Standard)));
        } else {
            output.push_str(&segment.text);
        }
    }
    output
}
```

### Visual Output Example

```
╭─ 🤖 CODEX ────────────────────────────────── openai-web ─╮
│                                                          │
│ Session   72% left  ████████████░░░░░░░░  resets in 2h   │
│ Weekly    41% left  ████████░░░░░░░░░░░░  resets Fri 9am │
│ Credits   112.4 remaining                                │
│ Account   user@example.com                               │
│ Status    ✓ Operational                                  │
│                                                          │
╰──────────────────────────────────────────────────────────╯

╭─ 🧠 CLAUDE ───────────────────────────────────── oauth ─╮
│                                                          │
│ Chat      85% left  █████████████████░░░  resets in 4h   │
│ Weekly    62% left  ████████████░░░░░░░░  resets Mon 12am│
│ Opus      45% left  █████████░░░░░░░░░░░  separate tier  │
│ Account   claude@example.com                             │
│                                                          │
╰──────────────────────────────────────────────────────────╯
```

---

## 5. Cost Command Integration

### Enhanced Cost Display

```rust
// In src/render/human.rs

pub fn render_cost(
    payloads: &[CostPayload],
    theme: &Theme,
) -> String {
    let console = get_console(false, false);
    let mut output = String::new();

    for payload in payloads {
        // Summary panel
        let summary_panel = Panel::new(vec![
            vec![
                Segment::styled("Today's Cost    ".to_string(), theme.label.clone()),
                Segment::styled(
                    format!("${:.2}", payload.session_cost_usd.unwrap_or(0.0)),
                    theme.cost_today.clone()
                ),
                Segment::line(),
            ],
            vec![
                Segment::styled("30-Day Total    ".to_string(), theme.label.clone()),
                Segment::styled(
                    format!("${:.2}", payload.last_30_days_cost_usd.unwrap_or(0.0)),
                    theme.cost_total.clone()
                ),
                Segment::line(),
            ],
            vec![
                Segment::styled("Total Tokens    ".to_string(), theme.label.clone()),
                Segment::styled(
                    format_tokens(payload.last_30_days_tokens.unwrap_or(0)),
                    theme.token_count.clone()
                ),
                Segment::line(),
            ],
        ])
            .title(Text::styled(
                format!(" {} Cost Summary ", provider_icon(&payload.provider)),
                theme.provider_title.clone()
            ))
            .rounded()
            .width(50);

        output.push_str(&segments_to_string(&summary_panel.render(50), &console));
        output.push('\n');

        // Daily breakdown table (last 7 days)
        if !payload.daily.is_empty() {
            let mut table = Table::new()
                .title("Recent Activity")
                .with_column(Column::new("Date"))
                .with_column(Column::new("Tokens").justify(JustifyMethod::Right))
                .with_column(Column::new("Cost").justify(JustifyMethod::Right))
                .with_column(Column::new("Models"))
                .border_style(theme.table_border.clone())
                .header_style(theme.table_header.clone())
                .rounded();

            for entry in payload.daily.iter().take(7) {
                table.add_row_cells([
                    entry.date.clone(),
                    format_tokens(entry.total_tokens.unwrap_or(0)),
                    format!("${:.2}", entry.total_cost.unwrap_or(0.0)),
                    entry.models_used.as_ref()
                        .map(|m| m.join(", "))
                        .unwrap_or_default(),
                ]);
            }

            output.push_str(&table.render_plain(80));
            output.push('\n');
        }
    }

    output
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
```

### Visual Output Example

```
╭─ 🤖 Cost Summary ────────────────────────────╮
│                                              │
│ Today's Cost      $2.45                      │
│ 30-Day Total      $47.82                     │
│ Total Tokens      2.4M                       │
│                                              │
╰──────────────────────────────────────────────╯

┌──────────── Recent Activity ─────────────────┐
│ Date       │    Tokens │   Cost │ Models     │
├────────────┼───────────┼────────┼────────────┤
│ 2026-01-19 │    124.5K │  $2.45 │ opus, sonn │
│ 2026-01-18 │    198.2K │  $3.82 │ opus       │
│ 2026-01-17 │     89.1K │  $1.65 │ sonnet     │
└──────────────────────────────────────────────┘
```

---

## 6. Doctor Command Integration

### Enhanced Diagnostics Display

```rust
// In src/render/doctor.rs - REWRITE

pub fn render_doctor_report(
    report: &DoctorReport,
    theme: &Theme,
) -> String {
    let console = get_console(false, false);
    let mut output = String::new();

    // Header
    let header = Rule::with_title(" caut doctor ")
        .style(theme.rule_style.clone())
        .character("─");
    output.push_str(&header.render_plain(60));
    output.push('\n');

    // Config status
    let config_panel = if report.config_status.is_ok() {
        success_panel(
            "Configuration",
            "Config file loaded successfully",
            theme
        )
    } else {
        warning_panel(
            "Configuration",
            "Using default configuration (no config file found)",
            theme
        )
    };
    output.push_str(&segments_to_string(&config_panel.render(60), &console));
    output.push('\n');

    // Provider checks as a tree
    let mut root = TreeNode::new(Text::styled(
        "Provider Health Checks",
        theme.tree_root.clone()
    ));

    for check in &report.providers {
        let (icon, style) = match check.status {
            CheckStatus::Pass => (theme.icons.check, theme.status_ok.clone()),
            CheckStatus::Warn => (theme.icons.warning, theme.status_warning.clone()),
            CheckStatus::Fail => (theme.icons.error, theme.status_error.clone()),
        };

        let mut node = TreeNode::new(Text::styled(
            format!("{} {}", icon, check.provider_name),
            style.clone()
        ));

        // Add details as children
        if let Some(version) = &check.version {
            node = node.child(TreeNode::new(Text::styled(
                format!("Version: {}", version),
                theme.tree_detail.clone()
            )));
        }

        if let Some(msg) = &check.message {
            node = node.child(TreeNode::new(Text::styled(
                msg.clone(),
                theme.tree_detail.clone()
            )));
        }

        root = root.child(node);
    }

    let tree = Tree::new(root)
        .guides(TreeGuides::Rounded)
        .guide_style(theme.tree_guide.clone());

    output.push_str(&tree.render_plain());
    output.push('\n');

    // Summary
    let passed = report.providers.iter().filter(|c| matches!(c.status, CheckStatus::Pass)).count();
    let total = report.providers.len();

    let summary_style = if passed == total {
        theme.success_message.clone()
    } else {
        theme.warning_message.clone()
    };

    output.push_str(&format!(
        "\n{}/{} providers healthy (completed in {:?})\n",
        passed, total, report.total_duration
    ));

    output
}
```

### Visual Output Example

```
───────────────────── caut doctor ─────────────────────

╭─ ✓ Configuration ────────────────────────────────────╮
│ Config file loaded successfully                      │
╰──────────────────────────────────────────────────────╯

Provider Health Checks
├─ ✓ Codex
│  ├─ Version: 0.6.0
│  └─ Authentication valid
├─ ✓ Claude
│  ├─ Version: 1.2.3
│  └─ OAuth token valid
├─ ⚠ Gemini
│  └─ Not configured
╰─ ✗ Cursor
   └─ CLI not found in PATH

3/4 providers healthy (completed in 1.2s)
```

---

## 7. History Command Integration

### Enhanced History Display

```rust
// In src/cli/history.rs

pub fn render_history_stats(
    stats: &HistoryStats,
    theme: &Theme,
) -> String {
    let console = get_console(false, false);

    // Stats table
    let mut table = Table::new()
        .title("Usage History Statistics")
        .with_column(Column::new("Metric"))
        .with_column(Column::new("Value").justify(JustifyMethod::Right))
        .border_style(theme.table_border.clone())
        .rounded();

    table.add_row_cells(["Total Snapshots", &stats.total_snapshots.to_string()]);
    table.add_row_cells(["Aggregated Days", &stats.aggregated_days.to_string()]);
    table.add_row_cells(["Database Size", &format_bytes(stats.db_size_bytes)]);
    table.add_row_cells(["Oldest Record", &stats.oldest_record.format("%Y-%m-%d").to_string()]);
    table.add_row_cells(["Newest Record", &stats.newest_record.format("%Y-%m-%d").to_string()]);

    table.render_plain(50)
}

pub fn render_prune_result(
    result: &PruneResult,
    dry_run: bool,
    theme: &Theme,
) -> String {
    let console = get_console(false, false);

    let title = if dry_run { "Prune Preview (Dry Run)" } else { "Prune Complete" };
    let icon = if dry_run { theme.icons.info } else { theme.icons.check };

    let panel = Panel::new(vec![
        vec![
            Segment::styled("Snapshots to delete: ".to_string(), theme.label.clone()),
            Segment::styled(result.deleted_snapshots.to_string(), theme.value.clone()),
            Segment::line(),
        ],
        vec![
            Segment::styled("Aggregates created:  ".to_string(), theme.label.clone()),
            Segment::styled(result.created_aggregates.to_string(), theme.value.clone()),
            Segment::line(),
        ],
        vec![
            Segment::styled("Space reclaimed:     ".to_string(), theme.label.clone()),
            Segment::styled(format_bytes(result.bytes_reclaimed), theme.value.clone()),
            Segment::line(),
        ],
    ])
        .title(Text::styled(format!(" {} {} ", icon, title), theme.panel_title.clone()))
        .rounded();

    segments_to_string(&panel.render(50), &console)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}
```

---

## 8. Error Display Integration

### Enhanced Error Rendering

```rust
// src/error/display.rs - NEW FILE

use rich_rust::prelude::*;
use crate::error::{CautError, ErrorCategory};
use crate::render::theme::Theme;
use crate::render::components::*;

pub fn render_error(error: &CautError, theme: &Theme) -> String {
    let console = get_console(false, false);

    let (title, icon) = match error.category() {
        ErrorCategory::Authentication => ("Authentication Error", theme.icons.lock),
        ErrorCategory::Network => ("Network Error", theme.icons.network),
        ErrorCategory::Configuration => ("Configuration Error", theme.icons.config),
        ErrorCategory::Provider => ("Provider Error", theme.icons.provider),
        ErrorCategory::Environment => ("Environment Error", theme.icons.env),
        ErrorCategory::Internal => ("Internal Error", theme.icons.bug),
    };

    let suggestions = error.fix_suggestions();

    // Build content
    let mut content_lines: Vec<Vec<Segment<'static>>> = Vec::new();

    // Error message
    content_lines.push(vec![
        Segment::styled(error.to_string(), theme.error_message.clone()),
        Segment::line(),
    ]);

    // Suggestions
    if !suggestions.is_empty() {
        content_lines.push(vec![Segment::line()]);
        content_lines.push(vec![
            Segment::styled("How to fix:", theme.suggestion_header.clone()),
            Segment::line(),
        ]);

        for (i, suggestion) in suggestions.iter().enumerate() {
            content_lines.push(vec![
                Segment::styled(
                    format!("  {}. {}", i + 1, suggestion),
                    theme.suggestion_text.clone()
                ),
                Segment::line(),
            ]);
        }
    }

    // Command examples (if applicable)
    if let Some(commands) = error.suggested_commands() {
        content_lines.push(vec![Segment::line()]);
        content_lines.push(vec![
            Segment::styled("Try running:", theme.command_header.clone()),
            Segment::line(),
        ]);

        for cmd in commands {
            content_lines.push(vec![
                Segment::styled(format!("  $ {}", cmd), theme.command_text.clone()),
                Segment::line(),
            ]);
        }
    }

    let panel = Panel::new(content_lines)
        .title(Text::styled(
            format!(" {} {} ", icon, title),
            theme.error_title.clone()
        ))
        .border_style(theme.error_border.clone())
        .rounded()
        .width(70);

    segments_to_string(&panel.render(70), &console)
}
```

### Visual Output Example

```
╭─ 🔐 Authentication Error ────────────────────────────╮
│                                                      │
│ Claude OAuth token has expired                       │
│                                                      │
│ How to fix:                                          │
│   1. Re-authenticate with Claude                     │
│   2. Check your token in the system keyring          │
│                                                      │
│ Try running:                                         │
│   $ claude auth login                                │
│   $ caut doctor --provider claude                    │
│                                                      │
╰──────────────────────────────────────────────────────╯
```

---

## 9. Progress & Loading States

### Fetch Progress Indicators

```rust
// src/render/progress.rs - NEW FILE

use rich_rust::prelude::*;
use std::sync::{Arc, Mutex};
use std::io::Write;

/// Progress display for multi-provider fetching
pub struct FetchProgress {
    spinner: Spinner,
    providers: Vec<ProviderProgress>,
    console: Console,
    start_time: std::time::Instant,
}

struct ProviderProgress {
    name: String,
    status: FetchStatus,
}

enum FetchStatus {
    Pending,
    Fetching,
    Success(std::time::Duration),
    Failed(String),
}

impl FetchProgress {
    pub fn new(provider_names: &[&str]) -> Self {
        Self {
            spinner: Spinner::dots().style(Style::new().color_str("cyan").unwrap()),
            providers: provider_names.iter().map(|name| ProviderProgress {
                name: name.to_string(),
                status: FetchStatus::Pending,
            }).collect(),
            console: get_console(false, false),
            start_time: std::time::Instant::now(),
        }
    }

    pub fn start_provider(&mut self, name: &str) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.name == name) {
            p.status = FetchStatus::Fetching;
        }
        self.render();
    }

    pub fn complete_provider(&mut self, name: &str, duration: std::time::Duration) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.name == name) {
            p.status = FetchStatus::Success(duration);
        }
        self.render();
    }

    pub fn fail_provider(&mut self, name: &str, error: &str) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.name == name) {
            p.status = FetchStatus::Failed(error.to_string());
        }
        self.render();
    }

    fn render(&mut self) {
        // Clear previous lines and redraw
        print!("\x1B[{}A", self.providers.len() + 1);
        print!("\x1B[J");

        // Header with spinner
        let frame = self.spinner.next_frame();
        println!("{} Fetching usage data...", frame);

        // Provider status lines
        for provider in &self.providers {
            let (icon, status_text, style) = match &provider.status {
                FetchStatus::Pending => ("○", "pending".to_string(), Style::new().dim()),
                FetchStatus::Fetching => ("◐", "fetching...".to_string(), Style::new().color_str("cyan").unwrap()),
                FetchStatus::Success(d) => ("✓", format!("done ({:.0}ms)", d.as_millis()), Style::new().color_str("green").unwrap()),
                FetchStatus::Failed(e) => ("✗", e.clone(), Style::new().color_str("red").unwrap()),
            };

            let line = format!("  {} {:12} {}", icon, provider.name, status_text);
            println!("{}", style.render(&line, self.console.color_system().unwrap_or(ColorSystem::Standard)));
        }

        std::io::stdout().flush().ok();
    }

    pub fn finish(&self) {
        let elapsed = self.start_time.elapsed();
        println!("\nCompleted in {:.1}s", elapsed.as_secs_f64());
    }
}
```

### Visual Output Example (animated)

```
⠋ Fetching usage data...
  ✓ codex        done (245ms)
  ◐ claude       fetching...
  ○ gemini       pending
  ○ cursor       pending
```

---

## 10. Startup & Branding

### Application Banner

```rust
// src/render/branding.rs - NEW FILE

use rich_rust::prelude::*;

/// ASCII art logo (small version for terminal)
const LOGO: &str = r#"
                  _
  ___ __ _ _   _| |_
 / __/ _` | | | | __|
| (_| (_| | |_| | |_
 \___\__,_|\__,_|\__|
"#;

pub fn render_banner(version: &str, theme: &Theme) -> String {
    let console = get_console(false, false);

    let mut segments = Vec::new();

    // Logo in gradient colors
    for (i, line) in LOGO.lines().enumerate() {
        let color = match i % 3 {
            0 => theme.brand_primary.clone(),
            1 => theme.brand_secondary.clone(),
            _ => theme.brand_tertiary.clone(),
        };
        segments.push(Segment::styled(line.to_string(), color));
        segments.push(Segment::line());
    }

    // Version and tagline
    segments.push(Segment::styled(
        format!("v{}", version),
        theme.version_text.clone()
    ));
    segments.push(Segment::plain("  "));
    segments.push(Segment::styled(
        "Coding Agent Usage Tracker",
        theme.tagline.clone()
    ));
    segments.push(Segment::line());

    segments_to_string(&segments, &console)
}

/// Compact one-line version for help/usage
pub fn render_compact_header(version: &str, theme: &Theme) -> String {
    let console = get_console(false, false);

    let segments = vec![
        Segment::styled("caut".to_string(), theme.brand_primary.clone().bold()),
        Segment::plain(" "),
        Segment::styled(format!("v{}", version), theme.version_text.clone()),
        Segment::plain(" — "),
        Segment::styled("Coding Agent Usage Tracker".to_string(), theme.tagline.clone().dim()),
        Segment::line(),
    ];

    segments_to_string(&segments, &console)
}
```

---

## 11. Help & Documentation

### Enhanced Help Display

```rust
// Integration with clap's help system

pub fn render_help(theme: &Theme) -> String {
    let console = get_console(false, false);
    let mut output = String::new();

    // Header
    output.push_str(&render_compact_header(env!("CARGO_PKG_VERSION"), theme));
    output.push('\n');

    // Usage section
    let usage_rule = Rule::with_title(" Usage ")
        .style(theme.rule_style.clone());
    output.push_str(&usage_rule.render_plain(60));
    output.push_str("\n\n");

    output.push_str("  caut [OPTIONS] <COMMAND>\n\n");

    // Commands section
    let commands_rule = Rule::with_title(" Commands ")
        .style(theme.rule_style.clone());
    output.push_str(&commands_rule.render_plain(60));
    output.push_str("\n\n");

    let mut table = Table::new()
        .with_column(Column::new("Command").min_width(15))
        .with_column(Column::new("Description"))
        .show_header(false)
        .show_edge(false)
        .padding(1, 0);

    table.add_row_cells(["usage", "Show rate limit usage for providers"]);
    table.add_row_cells(["cost", "Show local cost usage from JSONL logs"]);
    table.add_row_cells(["doctor", "Run health checks on providers"]);
    table.add_row_cells(["history", "Manage usage history database"]);
    table.add_row_cells(["token-accounts", "Manage multi-account configurations"]);

    output.push_str(&table.render_plain(60));
    output.push('\n');

    // Examples section
    let examples_rule = Rule::with_title(" Examples ")
        .style(theme.rule_style.clone());
    output.push_str(&examples_rule.render_plain(60));
    output.push_str("\n\n");

    let examples = [
        ("caut usage", "Check primary providers (Codex + Claude)"),
        ("caut usage --provider all", "Check all 16 providers"),
        ("caut usage --json", "Output as JSON for AI agents"),
        ("caut cost --refresh", "Force refresh cost data"),
        ("caut doctor", "Run health checks"),
    ];

    for (cmd, desc) in examples {
        output.push_str(&format!("  {} {}\n",
            Style::new().color_str("cyan").unwrap().bold().render(cmd, ColorSystem::TrueColor),
            Style::new().dim().render(&format!("# {}", desc), ColorSystem::TrueColor)
        ));
    }

    output.push('\n');
    output
}
```

---

## 12. Color Theming System

### Theme Definition

```rust
// src/render/theme.rs - NEW FILE

use rich_rust::prelude::*;

/// Icons used throughout the UI
pub struct ThemeIcons {
    pub check: &'static str,
    pub warning: &'static str,
    pub error: &'static str,
    pub info: &'static str,
    pub arrow_right: &'static str,
    pub lock: &'static str,
    pub network: &'static str,
    pub config: &'static str,
    pub provider: &'static str,
    pub env: &'static str,
    pub bug: &'static str,
}

impl Default for ThemeIcons {
    fn default() -> Self {
        Self {
            check: "✓",
            warning: "⚠",
            error: "✗",
            info: "ℹ",
            arrow_right: "→",
            lock: "🔐",
            network: "🌐",
            config: "⚙",
            provider: "📊",
            env: "🔧",
            bug: "🐛",
        }
    }
}

/// Complete theme for caut output
pub struct Theme {
    // Icons
    pub icons: ThemeIcons,

    // Brand colors
    pub brand_primary: Style,
    pub brand_secondary: Style,
    pub brand_tertiary: Style,
    pub version_text: Style,
    pub tagline: Style,

    // Provider-specific borders
    pub codex_border: Style,
    pub claude_border: Style,
    pub gemini_border: Style,
    pub cursor_border: Style,
    pub default_border: Style,

    // Panel styles
    pub provider_title: Style,
    pub source_label: Style,
    pub panel_border: Style,
    pub panel_title: Style,

    // Labels and values
    pub label: Style,
    pub value: Style,
    pub account_value: Style,
    pub credits_value: Style,

    // Usage bar colors
    pub usage_good: Style,
    pub usage_warning: Style,
    pub usage_critical: Style,
    pub percent_good: Style,
    pub percent_warning: Style,
    pub percent_critical: Style,
    pub reset_time: Style,

    // Cost display
    pub cost_today: Style,
    pub cost_total: Style,
    pub token_count: Style,

    // Status indicators
    pub status_ok: Style,
    pub status_warning: Style,
    pub status_error: Style,

    // Tables
    pub table_border: Style,
    pub table_header: Style,
    pub table_title: Style,

    // Trees
    pub tree_root: Style,
    pub tree_guide: Style,
    pub tree_detail: Style,

    // Errors and warnings
    pub error_title: Style,
    pub error_border: Style,
    pub error_message: Style,
    pub warning_title: Style,
    pub warning_border: Style,
    pub warning_message: Style,
    pub success_title: Style,
    pub success_border: Style,
    pub success_message: Style,

    // Suggestions and commands
    pub suggestion_header: Style,
    pub suggestion_text: Style,
    pub command_header: Style,
    pub command_text: Style,

    // Rules
    pub rule_style: Style,
}

impl Theme {
    /// Default theme - vibrant and modern
    pub fn default() -> Self {
        Self {
            icons: ThemeIcons::default(),

            // Brand - cyan/blue gradient
            brand_primary: Style::new().color_str("#00d4ff").unwrap().bold(),
            brand_secondary: Style::new().color_str("#0099cc").unwrap().bold(),
            brand_tertiary: Style::new().color_str("#006699").unwrap().bold(),
            version_text: Style::new().color_str("#888888").unwrap(),
            tagline: Style::new().color_str("#aaaaaa").unwrap().italic(),

            // Provider borders
            codex_border: Style::new().color_str("#10a37f").unwrap(),  // OpenAI green
            claude_border: Style::new().color_str("#cc785c").unwrap(), // Anthropic orange
            gemini_border: Style::new().color_str("#4285f4").unwrap(), // Google blue
            cursor_border: Style::new().color_str("#7c3aed").unwrap(), // Purple
            default_border: Style::new().color_str("#666666").unwrap(),

            // Panels
            provider_title: Style::new().bold(),
            source_label: Style::new().dim().italic(),
            panel_border: Style::new().color_str("#444444").unwrap(),
            panel_title: Style::new().bold(),

            // Labels
            label: Style::new().color_str("#888888").unwrap(),
            value: Style::new().color_str("#ffffff").unwrap(),
            account_value: Style::new().color_str("#aaaaaa").unwrap(),
            credits_value: Style::new().color_str("#ffd700").unwrap().bold(),

            // Usage bars
            usage_good: Style::new().color_str("#00ff00").unwrap(),
            usage_warning: Style::new().color_str("#ffff00").unwrap(),
            usage_critical: Style::new().color_str("#ff0000").unwrap(),
            percent_good: Style::new().color_str("#00ff00").unwrap().bold(),
            percent_warning: Style::new().color_str("#ffff00").unwrap().bold(),
            percent_critical: Style::new().color_str("#ff0000").unwrap().bold(),
            reset_time: Style::new().color_str("#888888").unwrap().italic(),

            // Cost
            cost_today: Style::new().color_str("#00ff00").unwrap().bold(),
            cost_total: Style::new().color_str("#ffffff").unwrap().bold(),
            token_count: Style::new().color_str("#00d4ff").unwrap(),

            // Status
            status_ok: Style::new().color_str("#00ff00").unwrap(),
            status_warning: Style::new().color_str("#ffff00").unwrap(),
            status_error: Style::new().color_str("#ff0000").unwrap(),

            // Tables
            table_border: Style::new().color_str("#444444").unwrap(),
            table_header: Style::new().bold().color_str("#00d4ff").unwrap(),
            table_title: Style::new().bold(),

            // Trees
            tree_root: Style::new().bold(),
            tree_guide: Style::new().color_str("#444444").unwrap(),
            tree_detail: Style::new().color_str("#888888").unwrap(),

            // Errors
            error_title: Style::new().color_str("#ff0000").unwrap().bold(),
            error_border: Style::new().color_str("#ff0000").unwrap(),
            error_message: Style::new().color_str("#ff6666").unwrap(),
            warning_title: Style::new().color_str("#ffff00").unwrap().bold(),
            warning_border: Style::new().color_str("#ffff00").unwrap(),
            warning_message: Style::new().color_str("#ffff88").unwrap(),
            success_title: Style::new().color_str("#00ff00").unwrap().bold(),
            success_border: Style::new().color_str("#00ff00").unwrap(),
            success_message: Style::new().color_str("#88ff88").unwrap(),

            // Suggestions
            suggestion_header: Style::new().bold().underline(),
            suggestion_text: Style::new().color_str("#aaaaaa").unwrap(),
            command_header: Style::new().bold(),
            command_text: Style::new().color_str("#00d4ff").unwrap(),

            // Rules
            rule_style: Style::new().color_str("#444444").unwrap(),
        }
    }

    /// Minimal theme - less color, more subtle
    pub fn minimal() -> Self {
        let mut theme = Self::default();
        theme.brand_primary = Style::new().bold();
        theme.brand_secondary = Style::new().bold();
        theme.brand_tertiary = Style::new().bold();
        theme.codex_border = Style::new().dim();
        theme.claude_border = Style::new().dim();
        theme.gemini_border = Style::new().dim();
        theme.cursor_border = Style::new().dim();
        theme.default_border = Style::new().dim();
        theme
    }

    /// High contrast theme
    pub fn high_contrast() -> Self {
        let mut theme = Self::default();
        // Adjust colors for better visibility
        theme.usage_good = Style::new().color_str("green").unwrap().bold();
        theme.usage_warning = Style::new().color_str("yellow").unwrap().bold();
        theme.usage_critical = Style::new().color_str("red").unwrap().bold();
        theme
    }

    /// ASCII-safe theme (no Unicode)
    pub fn ascii() -> Self {
        let mut theme = Self::default();
        theme.icons = ThemeIcons {
            check: "[OK]",
            warning: "[!]",
            error: "[X]",
            info: "[i]",
            arrow_right: "->",
            lock: "[LOCK]",
            network: "[NET]",
            config: "[CFG]",
            provider: "[PRV]",
            env: "[ENV]",
            bug: "[BUG]",
        };
        theme
    }

    /// Load theme from environment or config
    pub fn from_env() -> Self {
        match std::env::var("CAUT_THEME").as_deref() {
            Ok("minimal") => Self::minimal(),
            Ok("high-contrast") => Self::high_contrast(),
            Ok("ascii") => Self::ascii(),
            _ => Self::default(),
        }
    }
}
```

---

## 13. Implementation Phases

### Phase 1: Foundation (Week 1)
**Priority: Critical**

1. **Add rich_rust dependency**
   - Update `Cargo.toml` with `rich_rust = { version = "0.1", features = ["full"] }`
   - Verify compilation

2. **Create safety infrastructure**
   - Implement `should_use_rich_output()` function
   - Add `--plain` and `--force-color` CLI flags
   - Test with `NO_COLOR` and `CAUT_PLAIN` env vars

3. **Create theme system**
   - Implement `src/render/theme.rs`
   - Create default, minimal, high-contrast, and ASCII themes
   - Add `CAUT_THEME` environment variable support

4. **Create component library**
   - Implement `src/render/components.rs`
   - Create reusable functions: `provider_panel`, `usage_bar`, `data_table`, etc.

### Phase 2: Usage Command (Week 2)
**Priority: High**

1. **Rewrite human.rs for usage**
   - Implement provider cards with panels
   - Add progress bar-based usage display
   - Add status indicators
   - Preserve robot.rs unchanged

2. **Add progress indicators**
   - Implement spinner during fetch
   - Show per-provider fetch status
   - Display completion summary

### Phase 3: Supporting Commands (Week 3)
**Priority: Medium**

1. **Cost command enhancement**
   - Summary panel with key metrics
   - Daily breakdown table
   - Token formatting (K, M suffixes)

2. **Doctor command enhancement**
   - Config status panel
   - Tree-based provider health display
   - Summary with pass/warn/fail counts

3. **History command enhancement**
   - Stats table
   - Prune preview/result panels

### Phase 4: Error & Help (Week 4)
**Priority: Medium**

1. **Error display**
   - Category-based error panels
   - Suggestion lists
   - Command examples

2. **Help system**
   - Compact header with branding
   - Command tables
   - Example sections

3. **Branding**
   - ASCII art logo
   - Version display
   - Tagline

### Phase 5: Polish & Testing (Week 5)
**Priority: High**

1. **Visual testing**
   - Screenshot comparisons
   - Multiple terminal emulators
   - Color system fallback testing

2. **Performance verification**
   - Ensure no slowdown for agent users
   - Benchmark render times

3. **Documentation**
   - Update README with screenshots
   - Document theming system
   - Add troubleshooting section

---

## 14. File-by-File Changes

### Files to Create

| File | Purpose |
|------|---------|
| `src/render/theme.rs` | Theme definitions and loading |
| `src/render/components.rs` | Reusable rich components |
| `src/render/branding.rs` | Logo, banner, startup display |
| `src/render/progress.rs` | Progress indicators for fetching |
| `src/error/display.rs` | Rich error rendering |

### Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `rich_rust` dependency |
| `src/cli/args.rs` | Add `--plain`, `--force-color` flags |
| `src/render/mod.rs` | Add safety gates, theme loading |
| `src/render/human.rs` | Complete rewrite with rich_rust |
| `src/render/doctor.rs` | Enhance with panels and trees |
| `src/cli/usage.rs` | Integrate progress indicators |
| `src/cli/cost.rs` | Enhance output |
| `src/cli/history.rs` | Enhance output |
| `src/error/mod.rs` | Integrate rich error display |
| `src/main.rs` | Theme initialization |

### Files Unchanged

| File | Reason |
|------|--------|
| `src/render/robot.rs` | Robot mode must remain clean JSON/Markdown |
| `src/core/*` | Business logic unchanged |
| `src/providers/*` | Provider logic unchanged |
| `src/storage/*` | Storage logic unchanged |

---

## 15. Testing Strategy

### Unit Tests

```rust
#[test]
fn test_robot_mode_no_ansi() {
    let output = render_usage_json(&payloads, false);
    assert!(!output.contains("\x1B["));  // No ANSI escape codes
}

#[test]
fn test_human_mode_has_ansi_when_tty() {
    // Mock TTY environment
    let output = render_usage_human(&payloads, &Theme::default());
    assert!(output.contains("\x1B["));  // Has ANSI codes
}

#[test]
fn test_no_color_env_disables_colors() {
    std::env::set_var("NO_COLOR", "1");
    assert!(!should_use_rich_output(OutputFormat::Human, false));
    std::env::remove_var("NO_COLOR");
}
```

### Integration Tests

```bash
# Robot mode produces valid JSON
caut usage --json | jq .

# Robot mode has no ANSI codes
caut usage --json | grep -q $'\x1B' && echo "FAIL: ANSI in JSON" || echo "PASS"

# Plain mode has no ANSI codes
caut usage --plain | grep -q $'\x1B' && echo "FAIL: ANSI in plain" || echo "PASS"

# Human mode works when piped (graceful degradation)
caut usage | cat
```

### Visual Regression Tests

Use `insta` for snapshot testing of rendered output (strip ANSI for comparison).

---

## 16. Performance Considerations

### Lazy Initialization

```rust
// Only initialize Console when actually rendering human output
static CONSOLE: Lazy<Mutex<Console>> = Lazy::new(|| {
    Mutex::new(Console::new())
});
```

### Avoid Rich Rendering for Robot Mode

```rust
pub fn render_output(payloads: &[ProviderPayload], format: OutputFormat, ...) -> String {
    match format {
        OutputFormat::Json => {
            // Direct serialization - no rich_rust involvement
            serde_json::to_string(payloads).unwrap()
        }
        OutputFormat::Md => {
            // Direct string building - no rich_rust involvement
            build_markdown(payloads)
        }
        OutputFormat::Human => {
            // Only here do we use rich_rust
            render_human_rich(payloads, theme)
        }
    }
}
```

### Benchmark Targets

| Operation | Target | Rationale |
|-----------|--------|-----------|
| JSON render | <1ms | No styling overhead |
| Markdown render | <2ms | Simple string building |
| Human render (no TTY) | <5ms | Skip ANSI generation |
| Human render (TTY) | <20ms | Full rich rendering |

---

## Summary

This integration plan transforms caut's human output into a visually stunning CLI experience while maintaining perfect compatibility with AI agents. Key principles:

1. **Safety First**: Robot mode (JSON/Markdown) is never touched by rich_rust
2. **Graceful Degradation**: Non-TTY human output falls back to plain text
3. **Theming**: Multiple themes support different environments and preferences
4. **Reusable Components**: Common patterns extracted into a component library
5. **Performance**: Lazy initialization and early-exit for robot mode
6. **Testing**: Comprehensive tests ensure agents never receive ANSI codes

The result will be a CLI that delights human observers with premium visuals while remaining a reliable, parseable tool for AI agents.
