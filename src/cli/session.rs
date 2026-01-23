//! Session cost attribution command.
//!
//! Implements `caut session` to show per-session cost breakdowns from
//! Claude Code and Codex session logs.

use crate::cli::args::{OutputFormat, SessionArgs};
use crate::core::pricing::SessionCostCalculator;
use crate::core::provider::Provider;
use crate::core::session_logs::{ClaudeSessionParser, CodexSessionParser, SessionLogFinder, SessionLogPath, SessionUsage};
use crate::error::{CautError, Result};
use chrono::{DateTime, Duration, Local, Utc};
use serde::Serialize;

/// Single session summary for output.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub provider: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_minutes: Option<i64>,
    pub total_cost_usd: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub models_used: Vec<String>,
    pub primary_model: String,
    pub confidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
}

impl SessionSummary {
    fn from_usage(usage: &SessionUsage, provider: Provider) -> Self {
        let calc = SessionCostCalculator::new();
        let cost = calc.calculate(usage);

        let duration_minutes = match (usage.started_at, usage.ended_at) {
            (Some(start), Some(end)) => Some((end - start).num_minutes()),
            _ => None,
        };

        let mut models: Vec<String> = usage.models_used.iter().cloned().collect();
        models.sort();

        Self {
            session_id: usage.session_id.clone(),
            provider: provider.cli_name().to_string(),
            started_at: usage.started_at,
            ended_at: usage.ended_at,
            duration_minutes,
            total_cost_usd: cost.total_usd,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
            models_used: models,
            primary_model: cost.model,
            confidence: cost.confidence.description().to_string(),
            project_path: usage.project_path.as_ref().map(|p| p.display().to_string()),
        }
    }
}

/// Output payload for session command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOutput {
    pub schema_version: &'static str,
    pub generated_at: DateTime<Utc>,
    pub command: &'static str,
    pub sessions: Vec<SessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<SessionTotals>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Aggregate totals for multiple sessions.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTotals {
    pub session_count: usize,
    pub total_cost_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_duration_minutes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_hour_usd: Option<f64>,
}

/// Execute the session command.
pub async fn execute(
    args: &SessionArgs,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    let finder = SessionLogFinder::new()?;

    // Determine time range
    let (since, until) = if args.today {
        let now = Local::now();
        let start_of_day = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_local_timezone(now.timezone()).single())
            .flatten()
            .map(|dt| dt.with_timezone(&Utc));
        (start_of_day, None)
    } else {
        // Default: last 24 hours for recent sessions
        let since = Utc::now() - Duration::hours(24);
        (Some(since), None)
    };

    // Determine which providers to query
    let providers = match &args.provider {
        Some(name) => vec![Provider::from_cli_name(name)?],
        None => vec![Provider::Claude, Provider::Codex],
    };

    let mut sessions = Vec::new();
    let mut errors = Vec::new();

    // Collect sessions from each provider
    for provider in providers {
        let logs = finder.find_sessions(provider, since, until);

        for log in logs {
            match parse_log(&log) {
                Ok(usage) => {
                    // Filter by session ID if specified
                    if let Some(ref id) = args.id {
                        if !usage.session_id.contains(id) {
                            continue;
                        }
                    }
                    sessions.push(SessionSummary::from_usage(&usage, provider));
                }
                Err(e) => {
                    errors.push(format!("{}: {}", log.path.display(), e));
                }
            }
        }
    }

    // Sort by end time (most recent first)
    sessions.sort_by(|a, b| b.ended_at.cmp(&a.ended_at));

    // Apply limit for list mode
    if args.list && sessions.len() > args.limit {
        sessions.truncate(args.limit);
    }

    // Calculate totals if showing multiple sessions
    let totals = if sessions.len() > 1 || args.list || args.today {
        Some(calculate_totals(&sessions))
    } else {
        None
    };

    let output = SessionOutput {
        schema_version: "caut.v1",
        generated_at: Utc::now(),
        command: "session",
        sessions,
        totals,
        errors,
    };

    // Render output
    match format {
        OutputFormat::Json => {
            let json = if pretty {
                serde_json::to_string_pretty(&output)?
            } else {
                serde_json::to_string(&output)?
            };
            println!("{json}");
        }
        OutputFormat::Md => {
            println!("{}", render_markdown(&output));
        }
        OutputFormat::Human => {
            println!("{}", render_human(&output, no_color));
        }
    }

    Ok(())
}

/// Parse a session log file.
fn parse_log(log: &SessionLogPath) -> Result<SessionUsage> {
    match log.provider {
        Provider::Claude => ClaudeSessionParser.parse(&log.path),
        Provider::Codex => CodexSessionParser.parse(&log.path),
        _ => Err(CautError::Config(format!(
            "Session logs not supported for provider: {}",
            log.provider.cli_name()
        ))),
    }
}

/// Calculate aggregate totals.
fn calculate_totals(sessions: &[SessionSummary]) -> SessionTotals {
    let total_cost_usd: f64 = sessions.iter().map(|s| s.total_cost_usd).sum();
    let total_input_tokens: i64 = sessions.iter().map(|s| s.input_tokens).sum();
    let total_output_tokens: i64 = sessions.iter().map(|s| s.output_tokens).sum();
    let total_duration_minutes: i64 = sessions
        .iter()
        .filter_map(|s| s.duration_minutes)
        .sum();

    let cost_per_hour_usd = if total_duration_minutes > 0 {
        Some(total_cost_usd / (total_duration_minutes as f64 / 60.0))
    } else {
        None
    };

    SessionTotals {
        session_count: sessions.len(),
        total_cost_usd,
        total_input_tokens,
        total_output_tokens,
        total_duration_minutes,
        cost_per_hour_usd,
    }
}

/// Render human-readable output.
fn render_human(output: &SessionOutput, no_color: bool) -> String {
    use std::fmt::Write;
    let mut buf = String::new();

    if output.sessions.is_empty() {
        writeln!(buf, "No sessions found in the specified time range.").ok();
        writeln!(buf, "\nTip: Use --today to show all sessions from today,").ok();
        writeln!(buf, "     or --list to show recent sessions.").ok();
        return buf;
    }

    // Header
    let title = if output.sessions.len() == 1 {
        "Session Summary"
    } else {
        "Session Summaries"
    };

    if no_color {
        writeln!(buf, "=== {} ===\n", title).ok();
    } else {
        writeln!(buf, "\x1b[1;36m=== {} ===\x1b[0m\n", title).ok();
    }

    // Individual sessions
    for session in &output.sessions {
        render_session(&mut buf, session, no_color);
        writeln!(buf).ok();
    }

    // Totals if available
    if let Some(ref totals) = output.totals {
        render_totals(&mut buf, totals, no_color);
    }

    // Errors
    if !output.errors.is_empty() {
        writeln!(buf, "\nWarnings:").ok();
        for err in &output.errors {
            writeln!(buf, "  - {}", err).ok();
        }
    }

    buf
}

fn render_session(buf: &mut String, session: &SessionSummary, no_color: bool) {
    use std::fmt::Write;

    let provider_display = match session.provider.as_str() {
        "claude" => "Claude",
        "codex" => "Codex",
        _ => &session.provider,
    };

    // Session header
    if no_color {
        writeln!(buf, "[{}] {} ({})", provider_display, session.session_id, session.confidence).ok();
    } else {
        let color = match session.provider.as_str() {
            "claude" => "\x1b[38;5;208m", // Orange for Claude
            "codex" => "\x1b[38;5;82m",   // Green for Codex
            _ => "\x1b[37m",
        };
        writeln!(buf, "{}[{}]\x1b[0m {} \x1b[2m({})\x1b[0m", color, provider_display, session.session_id, session.confidence).ok();
    }

    // Time info
    if let (Some(start), Some(end)) = (session.started_at, session.ended_at) {
        let local_start = start.with_timezone(&Local);
        let local_end = end.with_timezone(&Local);
        let duration = session.duration_minutes.unwrap_or(0);

        write!(buf, "  Time: {} - {}",
            local_start.format("%H:%M"),
            local_end.format("%H:%M")).ok();

        if duration > 0 {
            let hours = duration / 60;
            let mins = duration % 60;
            if hours > 0 {
                writeln!(buf, " ({}h {}m)", hours, mins).ok();
            } else {
                writeln!(buf, " ({}m)", mins).ok();
            }
        } else {
            writeln!(buf).ok();
        }
    }

    // Cost
    if no_color {
        writeln!(buf, "  Cost: ${:.2}", session.total_cost_usd).ok();
    } else {
        let cost_color = if session.total_cost_usd > 5.0 {
            "\x1b[31m" // Red for expensive
        } else if session.total_cost_usd > 1.0 {
            "\x1b[33m" // Yellow for moderate
        } else {
            "\x1b[32m" // Green for cheap
        };
        writeln!(buf, "  Cost: {}${:.2}\x1b[0m", cost_color, session.total_cost_usd).ok();
    }

    // Tokens
    let total_tokens = session.input_tokens + session.output_tokens;
    writeln!(buf, "  Tokens: {}K in / {}K out ({:.0}K total)",
        session.input_tokens / 1000,
        session.output_tokens / 1000,
        total_tokens as f64 / 1000.0).ok();

    // Cache tokens if present
    if session.cache_read_tokens > 0 || session.cache_creation_tokens > 0 {
        writeln!(buf, "  Cache: {}K read / {}K created",
            session.cache_read_tokens / 1000,
            session.cache_creation_tokens / 1000).ok();
    }

    // Model
    writeln!(buf, "  Model: {}", session.primary_model).ok();

    // Project if available
    if let Some(ref project) = session.project_path {
        // Show just the project name, not full path
        let project_name = std::path::Path::new(project)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(project);
        writeln!(buf, "  Project: {}", project_name).ok();
    }
}

fn render_totals(buf: &mut String, totals: &SessionTotals, no_color: bool) {
    use std::fmt::Write;

    if no_color {
        writeln!(buf, "--- Totals ---").ok();
    } else {
        writeln!(buf, "\x1b[1m--- Totals ---\x1b[0m").ok();
    }

    writeln!(buf, "  Sessions: {}", totals.session_count).ok();
    writeln!(buf, "  Total Cost: ${:.2}", totals.total_cost_usd).ok();

    let total_tokens = totals.total_input_tokens + totals.total_output_tokens;
    writeln!(buf, "  Total Tokens: {:.0}K", total_tokens as f64 / 1000.0).ok();

    if totals.total_duration_minutes > 0 {
        let hours = totals.total_duration_minutes / 60;
        let mins = totals.total_duration_minutes % 60;
        writeln!(buf, "  Total Time: {}h {}m", hours, mins).ok();
    }

    if let Some(cost_per_hour) = totals.cost_per_hour_usd {
        writeln!(buf, "  Avg Cost/Hour: ${:.2}", cost_per_hour).ok();
    }
}

/// Render Markdown output.
fn render_markdown(output: &SessionOutput) -> String {
    use std::fmt::Write;
    let mut buf = String::new();

    writeln!(buf, "# Session Summary\n").ok();
    writeln!(buf, "Generated: {}\n", output.generated_at.format("%Y-%m-%d %H:%M:%S UTC")).ok();

    if output.sessions.is_empty() {
        writeln!(buf, "No sessions found.\n").ok();
        return buf;
    }

    // Sessions table
    writeln!(buf, "| Provider | Session | Duration | Cost | Tokens | Model |").ok();
    writeln!(buf, "|----------|---------|----------|------|--------|-------|").ok();

    for session in &output.sessions {
        let duration = session.duration_minutes
            .map(|m| format!("{}m", m))
            .unwrap_or_else(|| "-".to_string());
        let tokens = format!("{}K", (session.input_tokens + session.output_tokens) / 1000);

        writeln!(buf, "| {} | {} | {} | ${:.2} | {} | {} |",
            session.provider,
            &session.session_id[..session.session_id.len().min(12)],
            duration,
            session.total_cost_usd,
            tokens,
            session.primary_model).ok();
    }

    // Totals
    if let Some(ref totals) = output.totals {
        writeln!(buf, "\n## Totals\n").ok();
        writeln!(buf, "- **Sessions**: {}", totals.session_count).ok();
        writeln!(buf, "- **Total Cost**: ${:.2}", totals.total_cost_usd).ok();
        writeln!(buf, "- **Total Tokens**: {:.0}K",
            (totals.total_input_tokens + totals.total_output_tokens) as f64 / 1000.0).ok();
        if totals.total_duration_minutes > 0 {
            writeln!(buf, "- **Total Time**: {} minutes", totals.total_duration_minutes).ok();
        }
        if let Some(cost_per_hour) = totals.cost_per_hour_usd {
            writeln!(buf, "- **Cost/Hour**: ${:.2}", cost_per_hour).ok();
        }
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_test_usage() -> SessionUsage {
        SessionUsage {
            session_id: "test-session-123".to_string(),
            project_path: Some("/home/user/project".into()),
            started_at: Some(Utc::now() - Duration::hours(1)),
            ended_at: Some(Utc::now()),
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_read_tokens: 10_000,
            cache_creation_tokens: 5_000,
            models_used: {
                let mut set = HashSet::new();
                set.insert("claude-3-opus".to_string());
                set
            },
            message_count: 10,
        }
    }

    #[test]
    fn session_summary_from_usage() {
        let usage = make_test_usage();
        let summary = SessionSummary::from_usage(&usage, Provider::Claude);

        assert_eq!(summary.session_id, "test-session-123");
        assert_eq!(summary.provider, "claude");
        assert_eq!(summary.input_tokens, 100_000);
        assert_eq!(summary.output_tokens, 50_000);
        assert!(summary.total_cost_usd > 0.0);
        assert_eq!(summary.duration_minutes, Some(60));
    }

    #[test]
    fn calculate_totals_aggregates() {
        let usage = make_test_usage();
        let summary1 = SessionSummary::from_usage(&usage, Provider::Claude);
        let summary2 = SessionSummary::from_usage(&usage, Provider::Codex);

        let totals = calculate_totals(&[summary1.clone(), summary2.clone()]);

        assert_eq!(totals.session_count, 2);
        assert_eq!(totals.total_input_tokens, 200_000);
        assert_eq!(totals.total_output_tokens, 100_000);
        assert!(totals.total_cost_usd > 0.0);
    }

    #[test]
    fn render_human_empty_sessions() {
        let output = SessionOutput {
            schema_version: "caut.v1",
            generated_at: Utc::now(),
            command: "session",
            sessions: vec![],
            totals: None,
            errors: vec![],
        };

        let rendered = render_human(&output, true);
        assert!(rendered.contains("No sessions found"));
    }

    #[test]
    fn render_markdown_format() {
        let usage = make_test_usage();
        let summary = SessionSummary::from_usage(&usage, Provider::Claude);
        let totals = calculate_totals(&[summary.clone()]);

        let output = SessionOutput {
            schema_version: "caut.v1",
            generated_at: Utc::now(),
            command: "session",
            sessions: vec![summary],
            totals: Some(totals),
            errors: vec![],
        };

        let rendered = render_markdown(&output);
        assert!(rendered.contains("# Session Summary"));
        assert!(rendered.contains("| Provider |"));
        assert!(rendered.contains("## Totals"));
    }
}
