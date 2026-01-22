//! Doctor command output rendering.
//!
//! Renders diagnostic reports for human and machine consumption.

use crate::core::doctor::{CheckStatus, DiagnosticCheck, DoctorReport, ProviderHealth};
use crate::error::Result;
use rich_rust::prelude::*;
use rich_rust::{Color, ColorSystem, Segment, Style};
use std::time::Instant;
use tracing::Level;

// =============================================================================
// Human-Readable Output
// =============================================================================

/// Render a doctor report for human consumption.
pub fn render_human(report: &DoctorReport, no_color: bool) -> Result<String> {
    let start = if tracing::enabled!(Level::DEBUG) {
        Some(Instant::now())
    } else {
        None
    };
    let _theme = crate::rich::get_theme();
    let mut output = String::new();

    // Header
    output.push_str(&render_header(report, no_color));
    output.push('\n');

    // Installation section
    output.push_str(&render_installation(report, no_color));
    output.push('\n');

    // Providers section
    if !report.providers.is_empty() {
        output.push_str(&render_section_header("Providers", no_color));
        output.push('\n');

        for provider_health in &report.providers {
            output.push_str(&render_provider_health(provider_health, no_color));
            output.push('\n');
        }
    }

    // Summary
    output.push_str(&render_summary(report, no_color));

    if let Some(start) = start {
        tracing::debug!(
            component = "doctor_report",
            render_time_ms = start.elapsed().as_millis(),
            "Rendered doctor report"
        );
    }

    Ok(output)
}

/// Render the report header.
fn render_header(_report: &DoctorReport, no_color: bool) -> String {
    let title = "caut doctor - System Diagnostic Report";

    if no_color {
        let border = "-".repeat(title.len() + 4);
        format!("{}\n| {} |\n{}", border, title, border)
    } else {
        let title_text = Text::styled(
            title,
            Style::new().bold().color(Color::parse("cyan").unwrap()),
        );
        let panel = Panel::new(vec![vec![Segment::plain("")]])
            .title(title_text)
            .padding((0, 1));
        segments_to_string(&panel.render(65), no_color)
    }
}

/// Render the installation section.
fn render_installation(report: &DoctorReport, no_color: bool) -> String {
    let mut output = String::new();

    output.push_str(&render_section_header("Installation", no_color));
    output.push('\n');

    // caut version
    let version_line = format!(
        "  {} caut v{} ({})",
        status_icon(true, no_color),
        report.caut_version,
        short_sha(&report.caut_git_sha)
    );
    output.push_str(&colorize_line(&version_line, true, no_color));
    output.push('\n');

    // Config status
    output.push_str(&render_check_line(&report.config_status, "  ", no_color));
    output.push('\n');

    output
}

/// Render a section header.
fn render_section_header(title: &str, no_color: bool) -> String {
    if no_color {
        format!("{}\n{}", title, "-".repeat(60))
    } else {
        let style = Style::new().bold().color(Color::parse("white").unwrap());
        let styled = style.render(title, ColorSystem::TrueColor);
        format!("{}\n{}", styled, "-".repeat(60))
    }
}

/// Render a provider's health status.
fn render_provider_health(health: &ProviderHealth, no_color: bool) -> String {
    let mut output = String::new();

    // Provider name
    let provider_name = health.provider.display_name();
    if no_color {
        output.push_str(&format!("{}\n", provider_name));
    } else {
        let style = Style::new().bold();
        output.push_str(&style.render(provider_name, ColorSystem::TrueColor));
        output.push('\n');
    }

    // CLI installed check
    output.push_str(&render_check_line(&health.cli_installed, "  ", no_color));
    if let Some(version) = &health.cli_version {
        output.push_str(&format!("      {}\n", version));
    } else {
        output.push('\n');
    }

    // Authenticated check
    output.push_str(&render_check_line(&health.authenticated, "  ", no_color));
    output.push('\n');

    // Credential health check (if present)
    if let Some(ref cred_health) = health.credential_health {
        output.push_str(&render_check_line(cred_health, "  ", no_color));
        output.push('\n');
    }

    // API reachable check
    output.push_str(&render_check_line(&health.api_reachable, "  ", no_color));
    if let Some(duration) = health.api_reachable.duration {
        output.push_str(&format!("      {}ms\n", duration.as_millis()));
    } else {
        output.push('\n');
    }

    output
}

/// Render a single check line with status icon and optional suggestion.
fn render_check_line(check: &DiagnosticCheck, indent: &str, no_color: bool) -> String {
    let mut output = String::new();

    let is_ok = check.status.is_ready();
    let is_warning = check.status.is_warning();
    let icon = status_icon_detailed(&check.status, no_color);

    // Main line
    let main_line = format!("{}{} {}", indent, icon, check.name);
    output.push_str(&colorize_line_status(&main_line, &check.status, no_color));

    // Details or suggestion
    match &check.status {
        CheckStatus::Pass { details } => {
            if let Some(details) = details {
                output.push_str(&format!("  {}", details));
            }
        }
        CheckStatus::Warning { details, suggestion } => {
            output.push('\n');
            output.push_str(&colorize_line_status(
                &format!("{}    {}", indent, details),
                &check.status,
                no_color
            ));
            if let Some(suggestion) = suggestion {
                let arrow = if no_color { "->" } else { "\u{2192}" };
                output.push_str(&format!("\n{}    {} {}", indent, arrow, suggestion));
            }
        }
        CheckStatus::Fail { reason, suggestion } => {
            output.push('\n');
            output.push_str(&format!("{}    {}\n", indent, reason));
            if let Some(suggestion) = suggestion {
                let arrow = if no_color { "->" } else { "\u{2192}" };
                output.push_str(&format!("{}    {} {}", indent, arrow, suggestion));
            }
        }
        CheckStatus::Skipped { reason } => {
            output.push_str(&format!("  ({})", reason));
        }
        CheckStatus::Timeout { after } => {
            output.push_str(&format!("  (timeout after {}s)", after.as_secs()));
        }
    }

    output
}

/// Render the summary footer.
fn render_summary(report: &DoctorReport, no_color: bool) -> String {
    let mut output = String::new();

    output.push_str(&"-".repeat(60));
    output.push('\n');

    let (ready, needs_attention) = report.summary();
    let duration_ms = report.total_duration.as_millis();

    let summary_text = format!(
        "Summary: {} ready, {} need attention",
        ready, needs_attention
    );

    let time_text = format!("[{:.1}s]", duration_ms as f64 / 1000.0);

    if no_color {
        output.push_str(&format!("{:<50} {}\n", summary_text, time_text));
    } else {
        let color = if needs_attention > 0 {
            "yellow"
        } else {
            "green"
        };
        let style = Style::new().color(Color::parse(color).unwrap());
        let styled_summary = style.render(&summary_text, ColorSystem::TrueColor);
        let dim_time = Style::new()
            .dim()
            .render(&time_text, ColorSystem::TrueColor);
        output.push_str(&format!("{:<50} {}\n", styled_summary, dim_time));
    }

    output
}

/// Get status icon for pass/fail.
fn status_icon(is_ok: bool, no_color: bool) -> &'static str {
    if no_color {
        if is_ok { "[OK]" } else { "[!!]" }
    } else {
        if is_ok { "\u{2713}" } else { "\u{2717}" } // ✓ ✗
    }
}

/// Get status icon with warning support.
fn status_icon_detailed(status: &CheckStatus, no_color: bool) -> &'static str {
    match status {
        CheckStatus::Pass { .. } => {
            if no_color { "[OK]" } else { "\u{2713}" } // ✓
        }
        CheckStatus::Warning { .. } => {
            if no_color { "[!!]" } else { "\u{26A0}" } // ⚠
        }
        CheckStatus::Fail { .. } | CheckStatus::Timeout { .. } => {
            if no_color { "[!!]" } else { "\u{2717}" } // ✗
        }
        CheckStatus::Skipped { .. } => {
            if no_color { "[--]" } else { "\u{23ED}" } // ⏭
        }
    }
}

/// Colorize a line based on status.
fn colorize_line(line: &str, is_ok: bool, no_color: bool) -> String {
    if no_color {
        line.to_string()
    } else {
        let color = if is_ok { "green" } else { "red" };
        let style = Style::new().color(Color::parse(color).unwrap());
        style.render(line, ColorSystem::TrueColor)
    }
}

/// Colorize a line based on CheckStatus (with warning support).
fn colorize_line_status(line: &str, status: &CheckStatus, no_color: bool) -> String {
    if no_color {
        line.to_string()
    } else {
        let color = match status {
            CheckStatus::Pass { .. } => "green",
            CheckStatus::Warning { .. } => "yellow",
            CheckStatus::Fail { .. } | CheckStatus::Timeout { .. } => "red",
            CheckStatus::Skipped { .. } => "bright_black",
        };
        let style = Style::new().color(Color::parse(color).unwrap());
        style.render(line, ColorSystem::TrueColor)
    }
}

/// Shorten a git SHA.
fn short_sha(sha: &str) -> &str {
    if sha.len() >= 7 { &sha[..7] } else { sha }
}

/// Convert segments to string.
fn segments_to_string(segments: &[Segment], no_color: bool) -> String {
    let color_system = if no_color {
        ColorSystem::Standard
    } else {
        ColorSystem::TrueColor
    };

    segments
        .iter()
        .map(|seg| {
            if no_color || seg.style.is_none() {
                seg.text.to_string()
            } else {
                seg.style.as_ref().unwrap().render(&seg.text, color_system)
            }
        })
        .collect()
}

// =============================================================================
// JSON Output
// =============================================================================

/// Render a doctor report as JSON.
pub fn render_json(report: &DoctorReport, pretty: bool) -> Result<String> {
    let json = if pretty {
        serde_json::to_string_pretty(report)?
    } else {
        serde_json::to_string(report)?
    };
    Ok(json)
}

// =============================================================================
// Markdown Output
// =============================================================================

/// Render a doctor report as Markdown.
pub fn render_md(report: &DoctorReport) -> Result<String> {
    let mut output = String::new();

    output.push_str("# caut doctor - System Diagnostic Report\n\n");

    // Installation section
    output.push_str("## Installation\n\n");
    output.push_str(&format!(
        "- caut: v{} ({})\n",
        report.caut_version,
        short_sha(&report.caut_git_sha)
    ));
    output.push_str(&format!(
        "- config: {}\n",
        format_check_status_md(&report.config_status)
    ));
    output.push('\n');

    // Providers section
    if !report.providers.is_empty() {
        output.push_str("## Providers\n\n");

        for health in &report.providers {
            output.push_str(&format!("### {}\n\n", health.provider.display_name()));
            output.push_str(&format!("| Check | Status |\n|-------|--------|\n"));
            output.push_str(&format!(
                "| CLI installed | {} |\n",
                format_check_status_md(&health.cli_installed)
            ));
            output.push_str(&format!(
                "| Authenticated | {} |\n",
                format_check_status_md(&health.authenticated)
            ));
            if let Some(ref cred_health) = health.credential_health {
                output.push_str(&format!(
                    "| Credential health | {} |\n",
                    format_check_status_md(cred_health)
                ));
            }
            output.push_str(&format!(
                "| API reachable | {} |\n",
                format_check_status_md(&health.api_reachable)
            ));
            output.push('\n');
        }
    }

    // Summary
    let (ready, needs_attention) = report.summary();
    output.push_str("## Summary\n\n");
    output.push_str(&format!(
        "- **Ready:** {}\n- **Needs attention:** {}\n- **Duration:** {:.1}s\n",
        ready,
        needs_attention,
        report.total_duration.as_millis() as f64 / 1000.0
    ));

    Ok(output)
}

/// Format a check status for Markdown.
fn format_check_status_md(check: &DiagnosticCheck) -> String {
    match &check.status {
        CheckStatus::Pass { details } => {
            let icon = "\u{2705}"; // ✅
            match details {
                Some(d) => format!("{} {}", icon, d),
                None => format!("{} OK", icon),
            }
        }
        CheckStatus::Warning { details, suggestion } => {
            let icon = "\u{26A0}\u{FE0F}"; // ⚠️
            let mut s = format!("{} {}", icon, details);
            if let Some(sug) = suggestion {
                s.push_str(&format!(" *({})* ", sug));
            }
            s
        }
        CheckStatus::Fail { reason, suggestion } => {
            let icon = "\u{274C}"; // ❌
            let mut s = format!("{} {}", icon, reason);
            if let Some(sug) = suggestion {
                s.push_str(&format!(" *({})* ", sug));
            }
            s
        }
        CheckStatus::Skipped { reason } => {
            format!("\u{23ED} Skipped: {}", reason) // ⏭
        }
        CheckStatus::Timeout { after } => {
            format!("\u{23F1} Timeout after {}s", after.as_secs()) // ⏱
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::Provider;
    use std::time::Duration;

    fn make_test_report() -> DoctorReport {
        let ok_check = DiagnosticCheck::new(
            "Test check",
            CheckStatus::Pass {
                details: Some("all good".to_string()),
            },
        )
        .with_duration(Duration::from_millis(42));

        let fail_check = DiagnosticCheck::new(
            "Auth check",
            CheckStatus::Fail {
                reason: "No credentials".to_string(),
                suggestion: Some("Run: claude auth login".to_string()),
            },
        );

        let provider_ok = ProviderHealth {
            provider: Provider::Codex,
            cli_installed: ok_check.clone(),
            cli_version: Some("0.6.0".to_string()),
            authenticated: ok_check.clone(),
            credential_health: None,
            api_reachable: ok_check.clone(),
        };

        let provider_fail = ProviderHealth {
            provider: Provider::Claude,
            cli_installed: ok_check.clone(),
            cli_version: Some("1.0.30".to_string()),
            authenticated: fail_check,
            credential_health: None,
            api_reachable: ok_check.clone(),
        };

        DoctorReport {
            caut_version: "0.1.0".to_string(),
            caut_git_sha: "a999778deadbeef".to_string(),
            config_status: ok_check,
            providers: vec![provider_ok, provider_fail],
            total_duration: Duration::from_millis(1234),
        }
    }

    #[test]
    fn render_human_output_contains_header() {
        let report = make_test_report();
        let output = render_human(&report, true).unwrap();

        assert!(output.contains("caut doctor"));
        assert!(output.contains("System Diagnostic"));
    }

    #[test]
    fn render_human_output_shows_version() {
        let report = make_test_report();
        let output = render_human(&report, true).unwrap();

        assert!(output.contains("v0.1.0"));
        assert!(output.contains("a999778")); // Short SHA
    }

    #[test]
    fn render_human_output_shows_providers() {
        let report = make_test_report();
        let output = render_human(&report, true).unwrap();

        assert!(output.contains("Codex"));
        assert!(output.contains("Claude"));
    }

    #[test]
    fn render_human_output_shows_summary() {
        let report = make_test_report();
        let output = render_human(&report, true).unwrap();

        assert!(output.contains("Summary"));
        assert!(output.contains("ready"));
        assert!(output.contains("need attention"));
    }

    #[test]
    fn render_human_no_color_uses_ascii() {
        let report = make_test_report();
        let output = render_human(&report, true).unwrap();

        // No ANSI escape codes
        assert!(!output.contains("\x1b["));
        // Uses ASCII alternatives
        assert!(output.contains("[OK]") || output.contains("[!!]"));
    }

    #[test]
    fn render_human_with_color_has_ansi() {
        let report = make_test_report();
        let output = render_human(&report, false).unwrap();

        // Should contain ANSI codes or Unicode icons
        assert!(
            output.contains("\x1b[") || output.contains("\u{2713}") || output.contains("\u{2717}")
        );
    }

    #[test]
    fn render_json_is_valid() {
        let report = make_test_report();
        let json = render_json(&report, false).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("cautVersion").is_some());
        assert!(parsed.get("providers").is_some());
    }

    #[test]
    fn render_json_pretty_has_newlines() {
        let report = make_test_report();
        let json = render_json(&report, true).unwrap();

        assert!(json.contains('\n'));
    }

    #[test]
    fn render_md_has_headers() {
        let report = make_test_report();
        let md = render_md(&report).unwrap();

        assert!(md.contains("# caut doctor"));
        assert!(md.contains("## Installation"));
        assert!(md.contains("## Providers"));
        assert!(md.contains("## Summary"));
    }

    #[test]
    fn render_md_has_provider_tables() {
        let report = make_test_report();
        let md = render_md(&report).unwrap();

        assert!(md.contains("### Codex"));
        assert!(md.contains("### Claude"));
        assert!(md.contains("| Check | Status |"));
    }

    #[test]
    fn render_shows_fail_with_suggestion() {
        let report = make_test_report();
        let output = render_human(&report, true).unwrap();

        assert!(output.contains("No credentials"));
        assert!(output.contains("claude auth login"));
    }

    #[test]
    fn short_sha_truncates() {
        assert_eq!(short_sha("a999778deadbeef"), "a999778");
        assert_eq!(short_sha("abc"), "abc");
    }

    #[test]
    fn status_icon_returns_correct_icons() {
        assert_eq!(status_icon(true, true), "[OK]");
        assert_eq!(status_icon(false, true), "[!!]");
        assert_eq!(status_icon(true, false), "\u{2713}");
        assert_eq!(status_icon(false, false), "\u{2717}");
    }
}
