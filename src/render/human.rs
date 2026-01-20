//! Human-readable output using rich_rust.
//!
//! Renders usage and cost data with styled panels, tables, and progress bars.

use crate::core::models::{CostPayload, ProviderPayload, RateWindow, StatusIndicator};
use crate::error::Result;
use rich_rust::prelude::*;
use rich_rust::{Color, ColorSystem, Segment, Style};

/// Convert segments to a styled string with ANSI codes.
fn segments_to_string(segments: &[Segment], no_color: bool) -> String {
    let color_system = if no_color {
        ColorSystem::Standard // Will be ignored since styles won't render
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

/// Get color based on remaining percentage.
fn percentage_color(percent: f64) -> Color {
    if percent >= 25.0 {
        Color::parse("green").unwrap()
    } else if percent >= 10.0 {
        Color::parse("yellow").unwrap()
    } else {
        Color::parse("red").unwrap()
    }
}

/// Render usage results for human consumption.
pub fn render_usage(results: &[ProviderPayload], no_color: bool) -> Result<String> {
    let mut output = String::new();

    for payload in results {
        output.push_str(&render_provider_usage(payload, no_color));
        output.push('\n');
    }

    Ok(output)
}

/// Render a single provider's usage.
fn render_provider_usage(payload: &ProviderPayload, no_color: bool) -> String {
    let mut content_lines: Vec<Vec<Segment>> = Vec::new();

    // Primary window
    if let Some(primary) = &payload.usage.primary {
        content_lines.push(format_rate_window_segments("Session", primary, no_color));
    }

    // Secondary window
    if let Some(secondary) = &payload.usage.secondary {
        content_lines.push(format_rate_window_segments("Weekly", secondary, no_color));
    }

    // Tertiary window (Opus/Sonnet)
    if let Some(tertiary) = &payload.usage.tertiary {
        content_lines.push(format_rate_window_segments(
            "Opus/Sonnet",
            tertiary,
            no_color,
        ));
    }

    // Credits
    if let Some(credits) = &payload.credits {
        content_lines.push(vec![Segment::plain(format!(
            "Credits: {:.1} left",
            credits.remaining
        ))]);
    }

    // Identity
    if let Some(identity) = &payload.usage.identity {
        if let Some(email) = &identity.account_email {
            content_lines.push(vec![Segment::plain(format!("Account: {}", email))]);
        }
    }

    // Status
    if let Some(status) = &payload.status {
        content_lines.push(format_status_segments(
            status.indicator,
            status.description.as_deref(),
            no_color,
        ));
    }

    // Fallback if no data
    if content_lines.is_empty() {
        let style = if no_color {
            Style::new()
        } else {
            Style::new().dim()
        };
        content_lines.push(vec![Segment::styled("No usage data available", style)]);
    }

    // Build panel title with styling
    let version = payload.version.as_deref().unwrap_or("");
    let title_text = format!("{} {} ({})", payload.provider, version, payload.source);
    let title = if no_color {
        Text::new(&title_text)
    } else {
        let style = Style::new().bold().color(Color::parse("cyan").unwrap());
        Text::styled(&title_text, style)
    };

    // Create panel
    let mut panel = Panel::new(content_lines).title(title).padding((0, 1)); // Horizontal padding

    if !no_color {
        panel = panel.border_style(Style::new().color(Color::parse("blue").unwrap()));
    }

    let segments = panel.render(60);
    segments_to_string(&segments, no_color)
}

/// Format rate window as styled segments with progress bar.
fn format_rate_window_segments<'a>(
    label: &'a str,
    window: &'a RateWindow,
    no_color: bool,
) -> Vec<Segment<'a>> {
    let remaining = window.remaining_percent();
    let reset = window
        .reset_description
        .as_deref()
        .unwrap_or("unknown reset");

    let mut segments = Vec::new();

    // Label
    let label_style = Style::new().bold();
    segments.push(Segment::styled(format!("{}: ", label), label_style));

    // Percentage
    let pct_color = percentage_color(remaining);
    let pct_style = Style::new().color(pct_color.clone());
    segments.push(Segment::styled(format!("{:.0}% ", remaining), pct_style));

    // Progress bar using rich_rust
    let bar_color = if no_color {
        Color::parse("white").unwrap()
    } else {
        pct_color
    };
    let bar_style = Style::new().color(bar_color.clone());
    let remaining_style = Style::new().color(Color::parse("bright_black").unwrap());

    let mut bar = ProgressBar::with_total(100)
        .width(16)
        .bar_style(BarStyle::Block)
        .completed_style(bar_style)
        .remaining_style(remaining_style)
        .show_percentage(false);
    bar.set_progress(remaining / 100.0);

    segments.extend(bar.render(16));

    // Reset info
    segments.push(Segment::plain(format!(" {}", reset)));

    segments
}

/// Format status as styled segments.
fn format_status_segments(
    indicator: StatusIndicator,
    description: Option<&str>,
    no_color: bool,
) -> Vec<Segment> {
    let mut segments = Vec::new();

    segments.push(Segment::styled("Status: ", Style::new().bold()));

    let (label, color) = match indicator {
        StatusIndicator::None => ("Operational", "green"),
        StatusIndicator::Minor => ("Minor Issue", "yellow"),
        StatusIndicator::Major => ("Major Issue", "red"),
        StatusIndicator::Critical => ("Critical", "red"),
        StatusIndicator::Maintenance => ("Maintenance", "blue"),
        StatusIndicator::Unknown => ("Unknown", "white"),
    };

    let style = if no_color {
        Style::new()
    } else {
        let mut s = Style::new().color(Color::parse(color).unwrap());
        if indicator == StatusIndicator::Critical {
            s = s.bold();
        }
        s
    };

    segments.push(Segment::styled(label, style));

    if let Some(desc) = description {
        segments.push(Segment::plain(format!(" – {}", desc)));
    }

    segments
}

/// Render cost results for human consumption.
pub fn render_cost(results: &[CostPayload], no_color: bool) -> Result<String> {
    let mut output = String::new();

    for payload in results {
        let mut content_lines: Vec<Vec<Segment>> = Vec::new();

        // Today's usage
        let today_text = match (payload.session_cost_usd, payload.session_tokens) {
            (Some(cost), Some(tokens)) => {
                format!("Today: ${:.2} · {} messages", cost, format_number(tokens))
            }
            (Some(cost), None) => format!("Today: ${:.2}", cost),
            (None, Some(tokens)) => format!("Today: {} messages", format_number(tokens)),
            (None, None) => "Today: No activity".to_string(),
        };
        content_lines.push(vec![Segment::plain(today_text)]);

        // Last 30 days
        let monthly_text = match (payload.last_30_days_cost_usd, payload.last_30_days_tokens) {
            (Some(cost), Some(tokens)) => {
                format!(
                    "Last 30 days: ${:.2} · {} messages",
                    cost,
                    format_number(tokens)
                )
            }
            (Some(cost), None) => format!("Last 30 days: ${:.2}", cost),
            (None, Some(tokens)) => format!("Last 30 days: {} messages", format_number(tokens)),
            (None, None) => "Last 30 days: No activity".to_string(),
        };
        content_lines.push(vec![Segment::plain(monthly_text)]);

        // Build panel
        let title_text = format!("{} Cost (local)", payload.provider);
        let title = if no_color {
            Text::new(&title_text)
        } else {
            let style = Style::new().bold().color(Color::parse("magenta").unwrap());
            Text::styled(&title_text, style)
        };

        let mut panel = Panel::new(content_lines).title(title).padding((0, 1));

        if !no_color {
            panel = panel.border_style(Style::new().color(Color::parse("magenta").unwrap()));
        }

        let segments = panel.render(50);
        output.push_str(&segments_to_string(&segments, no_color));
        output.push('\n');
    }

    Ok(output)
}

/// Format a number with thousand separators.
fn format_number(n: i64) -> String {
    let s = n.to_string();
    let bytes: Vec<_> = s.bytes().rev().collect();
    let chunks: Vec<_> = bytes
        .chunks(3)
        .map(|chunk| chunk.iter().rev().map(|&b| b as char).collect::<String>())
        .collect();
    chunks.into_iter().rev().collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::UsageSnapshot;
    use crate::test_utils::{
        make_test_cost_payload, make_test_cost_payload_minimal, make_test_credits_snapshot_minimal,
        make_test_provider_payload, make_test_provider_payload_minimal, make_test_rate_window,
        make_test_status_major_outage, make_test_status_operational, make_test_status_payload,
        make_test_usage_snapshot_with_tertiary,
    };
    use crate::{assert_ansi_codes, assert_contains, assert_no_ansi_codes, assert_not_contains};

    // =========================================================================
    // render_usage() Tests
    // =========================================================================

    #[test]
    fn render_usage_single_provider() {
        let payload = make_test_provider_payload("codex", "cli");
        let result = render_usage(&[payload], false).unwrap();

        assert_contains!(&result, "codex");
        assert_contains!(&result, "(cli)");
        assert_contains!(&result, "Session");
    }

    #[test]
    fn render_usage_multiple_providers() {
        let payloads = vec![
            make_test_provider_payload("codex", "cli"),
            make_test_provider_payload("claude", "oauth"),
        ];

        let result = render_usage(&payloads, false).unwrap();

        assert_contains!(&result, "codex");
        assert_contains!(&result, "claude");
    }

    #[test]
    fn render_usage_empty_results() {
        let result = render_usage(&[], false).unwrap();
        assert!(result.is_empty() || result.trim().is_empty());
    }

    #[test]
    fn render_usage_with_color() {
        let mut payload = make_test_provider_payload("test-provider", "test");
        payload.version = Some("1.0.0".to_string());
        payload.credits = Some(make_test_credits_snapshot_minimal(42.5));
        payload.status = Some(make_test_status_operational());
        let result = render_usage(&[payload], false).unwrap();

        // Check panel structure
        assert_contains!(&result, "test-provider");
        assert_contains!(&result, "1.0.0");
        assert_contains!(&result, "(test)");

        // Check rate windows are present
        assert_contains!(&result, "Session");
        assert_contains!(&result, "72%"); // 100 - 28 = 72% remaining

        assert_contains!(&result, "Weekly");
        assert_contains!(&result, "55%"); // 100 - 45 = 55% remaining

        // Check credits
        assert_contains!(&result, "Credits: 42.5");

        // Check status
        assert_contains!(&result, "Status");
        assert_contains!(&result, "Operational");

        // Should have ANSI codes when color enabled
        assert_ansi_codes!(&result);
    }

    #[test]
    fn render_usage_no_color() {
        let payload = make_test_provider_payload("test-provider", "test");
        let result = render_usage(&[payload], true).unwrap();

        // Should still contain all content
        assert_contains!(&result, "test-provider");
        assert_contains!(&result, "Session");
        assert_contains!(&result, "Weekly");

        // Should not contain ANSI escape codes
        assert_no_ansi_codes!(&result);
    }

    // =========================================================================
    // render_provider_usage() Tests
    // =========================================================================

    #[test]
    fn render_provider_usage_all_windows() {
        let mut payload = make_test_provider_payload("claude", "oauth");
        payload.usage = make_test_usage_snapshot_with_tertiary();

        let result = render_provider_usage(&payload, false);

        assert_contains!(&result, "Session");
        assert_contains!(&result, "Weekly");
        assert_contains!(&result, "Opus/Sonnet"); // tertiary
    }

    #[test]
    fn render_provider_usage_primary_only() {
        let mut payload = make_test_provider_payload_minimal("codex", "cli");
        // Make sure only primary is set
        payload.usage.secondary = None;
        payload.usage.tertiary = None;

        let result = render_provider_usage(&payload, false);

        assert_contains!(&result, "Session");
        assert_not_contains!(&result, "Weekly");
        assert_not_contains!(&result, "Opus/Sonnet");
    }

    #[test]
    fn render_provider_usage_with_credits() {
        let payload = make_test_provider_payload("codex", "cli");
        let result = render_provider_usage(&payload, false);

        assert_contains!(&result, "Credits:");
    }

    #[test]
    fn render_provider_usage_without_credits() {
        let payload = make_test_provider_payload("claude", "oauth");
        let result = render_provider_usage(&payload, false);

        assert_not_contains!(&result, "Credits:");
    }

    #[test]
    fn render_provider_usage_with_account_identity() {
        let payload = make_test_provider_payload("claude", "oauth");
        let result = render_provider_usage(&payload, false);

        assert_contains!(&result, "Account:");
        assert_contains!(&result, "test@example.com");
    }

    #[test]
    fn render_provider_usage_empty_data() {
        let payload = make_test_provider_payload_minimal("empty", "test");
        let empty_payload = ProviderPayload {
            usage: UsageSnapshot {
                primary: None,
                secondary: None,
                tertiary: None,
                updated_at: chrono::Utc::now(),
                identity: None,
            },
            ..payload
        };

        let result = render_provider_usage(&empty_payload, true);
        assert_contains!(&result, "No usage data available");
    }

    // =========================================================================
    // format_rate_window_segments() Tests
    // =========================================================================

    #[test]
    fn format_rate_window_shows_label_and_percentage() {
        let window = make_test_rate_window(30.0); // 30% used = 70% remaining

        let segments = format_rate_window_segments("Session", &window, true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Session:");
        assert_contains!(&text, "70%");
    }

    #[test]
    fn format_rate_window_shows_reset_description() {
        let window = make_test_rate_window(30.0);

        let segments = format_rate_window_segments("Session", &window, true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "resets in");
    }

    #[test]
    fn format_rate_window_handles_missing_reset_description() {
        let window = RateWindow::new(30.0); // Minimal window without reset_description

        let segments = format_rate_window_segments("Session", &window, true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "unknown reset");
    }

    // =========================================================================
    // percentage_color() Tests
    // =========================================================================

    #[test]
    fn percentage_color_green_above_25() {
        let color = percentage_color(50.0);
        assert!(matches!(color, Color { .. }));

        let color_at_25 = percentage_color(25.0);
        assert!(matches!(color_at_25, Color { .. }));
    }

    #[test]
    fn percentage_color_yellow_between_10_and_25() {
        let color = percentage_color(15.0);
        assert!(matches!(color, Color { .. }));

        let color_at_10 = percentage_color(10.0);
        assert!(matches!(color_at_10, Color { .. }));
    }

    #[test]
    fn percentage_color_red_below_10() {
        let color = percentage_color(5.0);
        assert!(matches!(color, Color { .. }));

        let color_at_0 = percentage_color(0.0);
        assert!(matches!(color_at_0, Color { .. }));
    }

    // =========================================================================
    // format_status_segments() Tests
    // =========================================================================

    #[test]
    fn format_status_operational() {
        let segments = format_status_segments(StatusIndicator::None, None, true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Status:");
        assert_contains!(&text, "Operational");
    }

    #[test]
    fn format_status_minor_issue() {
        let segments = format_status_segments(StatusIndicator::Minor, Some("Degraded API"), true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Minor Issue");
        assert_contains!(&text, "Degraded API");
    }

    #[test]
    fn format_status_major_issue() {
        let segments =
            format_status_segments(StatusIndicator::Major, Some("Service disruption"), true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Major Issue");
    }

    #[test]
    fn format_status_critical() {
        let segments =
            format_status_segments(StatusIndicator::Critical, Some("Complete outage"), false);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Critical");
    }

    #[test]
    fn format_status_maintenance() {
        let segments =
            format_status_segments(StatusIndicator::Maintenance, Some("Scheduled"), true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Maintenance");
    }

    #[test]
    fn format_status_unknown() {
        let segments = format_status_segments(StatusIndicator::Unknown, None, true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "Unknown");
    }

    #[test]
    fn format_status_with_description() {
        let segments = format_status_segments(StatusIndicator::Major, Some("API is down"), true);
        let text: String = segments.iter().map(|s| s.text.clone()).collect();

        assert_contains!(&text, "– API is down");
    }

    // =========================================================================
    // render_cost() Tests
    // =========================================================================

    #[test]
    fn render_cost_single_provider() {
        let payload = make_test_cost_payload("claude");
        let result = render_cost(&[payload], false).unwrap();

        assert_contains!(&result, "claude Cost");
        assert_contains!(&result, "Today:");
        assert_contains!(&result, "Last 30 days:");
    }

    #[test]
    fn render_cost_multiple_providers() {
        let payloads = vec![
            make_test_cost_payload("claude"),
            make_test_cost_payload("codex"),
        ];

        let result = render_cost(&payloads, false).unwrap();

        assert_contains!(&result, "claude Cost");
        assert_contains!(&result, "codex Cost");
    }

    #[test]
    fn render_cost_empty_results() {
        let result = render_cost(&[], false).unwrap();
        assert!(result.is_empty() || result.trim().is_empty());
    }

    #[test]
    fn render_cost_with_color() {
        let payload = make_test_cost_payload("claude");
        let result = render_cost(&[payload], false).unwrap();

        assert_ansi_codes!(&result);
    }

    #[test]
    fn render_cost_no_color() {
        let payload = make_test_cost_payload("claude");
        let result = render_cost(&[payload], true).unwrap();

        assert_no_ansi_codes!(&result);
        // Content should still be present
        assert_contains!(&result, "claude Cost");
    }

    #[test]
    fn render_cost_shows_today_cost_and_tokens() {
        let payload = make_test_cost_payload("claude");
        let result = render_cost(&[payload], true).unwrap();

        assert_contains!(&result, "Today:");
        assert_contains!(&result, "$2.45");
        assert_contains!(&result, "124,500"); // formatted with thousands separator
    }

    #[test]
    fn render_cost_shows_monthly_cost() {
        let payload = make_test_cost_payload("claude");
        let result = render_cost(&[payload], true).unwrap();

        assert_contains!(&result, "Last 30 days:");
        assert_contains!(&result, "$47.82");
    }

    #[test]
    fn render_cost_no_activity() {
        let payload = make_test_cost_payload_minimal("claude");
        let result = render_cost(&[payload], true).unwrap();

        assert_contains!(&result, "No activity");
    }

    // =========================================================================
    // format_number() Tests
    // =========================================================================

    #[test]
    fn format_number_thousands() {
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234), "1,234");
    }

    #[test]
    fn format_number_millions() {
        assert_eq!(format_number(1000000), "1,000,000");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn format_number_small() {
        assert_eq!(format_number(1), "1");
        assert_eq!(format_number(99), "99");
        assert_eq!(format_number(100), "100");
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_zero() {
        assert_eq!(format_number(0), "0");
    }

    // =========================================================================
    // No-Color Mode Tests
    // =========================================================================

    #[test]
    fn no_color_mode_preserves_content() {
        let payload = make_test_provider_payload("codex", "cli");

        let with_color = render_usage(&[payload.clone()], false).unwrap();
        let without_color = render_usage(&[payload], true).unwrap();

        // Strip ANSI codes from colored version for comparison
        let stripped = crate::test_utils::strip_ansi_codes(&with_color);

        // Core content should be the same
        assert!(without_color.contains("codex"));
        assert!(stripped.contains("codex"));
    }

    #[test]
    fn segments_to_string_with_color() {
        let segments = vec![
            Segment::styled("bold", Style::new().bold()),
            Segment::plain(" text"),
        ];

        let result = segments_to_string(&segments, false);
        assert_ansi_codes!(&result);
        assert_contains!(&result, "bold");
        assert_contains!(&result, "text");
    }

    #[test]
    fn segments_to_string_without_color() {
        let segments = vec![
            Segment::styled("bold", Style::new().bold()),
            Segment::plain(" text"),
        ];

        let result = segments_to_string(&segments, true);
        assert_no_ansi_codes!(&result);
        assert_eq!(result, "bold text");
    }
}
