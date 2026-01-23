//! Provider panel widget for the TUI dashboard.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Widget},
};

use crate::core::models::ProviderPayload;

/// A panel displaying a single provider's usage information.
pub struct ProviderPanel<'a> {
    /// The provider data to display.
    payload: &'a ProviderPayload,
    /// Whether this panel is currently selected.
    selected: bool,
}

impl<'a> ProviderPanel<'a> {
    /// Create a new provider panel.
    #[must_use]
    pub fn new(payload: &'a ProviderPayload, selected: bool) -> Self {
        Self { payload, selected }
    }

    /// Get the color for a usage percentage.
    fn usage_color(percent: f64) -> Color {
        if percent <= 25.0 {
            Color::Red
        } else if percent <= 50.0 {
            Color::Yellow
        } else {
            Color::Green
        }
    }

    /// Format duration to human-readable string.
    fn format_reset_time(minutes: Option<i32>) -> String {
        match minutes {
            Some(m) if m < 60 => format!("{m}m"),
            Some(m) if m < 1440 => format!("{}h {}m", m / 60, m % 60),
            Some(m) => format!("{}d", m / 1440),
            None => "unknown".to_string(),
        }
    }

    /// Build usage lines for display.
    fn build_usage_lines(&self) -> Vec<Line<'a>> {
        let mut lines = Vec::new();
        let usage = &self.payload.usage;

        // Primary rate window (session)
        if let Some(primary) = &usage.primary {
            let remaining = primary.remaining_percent();
            let color = Self::usage_color(remaining);
            let reset = Self::format_reset_time(primary.window_minutes);
            lines.push(Line::from(vec![
                Span::raw("Session: "),
                Span::styled(
                    format!("{remaining:.0}%"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" (resets {reset})")),
            ]));
        }

        // Secondary rate window (weekly)
        if let Some(secondary) = &usage.secondary {
            let remaining = secondary.remaining_percent();
            let color = Self::usage_color(remaining);
            let reset = Self::format_reset_time(secondary.window_minutes);
            lines.push(Line::from(vec![
                Span::raw("Weekly:  "),
                Span::styled(
                    format!("{remaining:.0}%"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" (resets {reset})")),
            ]));
        }

        // Tertiary rate window (opus tier for Claude)
        if let Some(tertiary) = &usage.tertiary {
            let remaining = tertiary.remaining_percent();
            let color = Self::usage_color(remaining);
            lines.push(Line::from(vec![
                Span::raw("Opus:    "),
                Span::styled(
                    format!("{remaining:.0}%"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        // Identity info
        if let Some(identity) = &usage.identity {
            if let Some(email) = &identity.account_email {
                lines.push(Line::from(vec![
                    Span::styled("Account: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(email.clone()),
                ]));
            }
        }

        if usage.primary.is_none() && usage.secondary.is_none() {
            lines.push(Line::from(Span::styled(
                "No usage data",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Credits info (Codex)
        if let Some(credits) = &self.payload.credits {
            let remaining = credits.remaining;
            lines.push(Line::from(vec![
                Span::raw("Credits: "),
                Span::styled(
                    format!("${remaining:.2}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        // Status info
        if let Some(status) = &self.payload.status {
            use crate::core::models::StatusIndicator;
            let (status_icon, status_color) = match &status.indicator {
                StatusIndicator::None => ("✓", Color::Green),
                StatusIndicator::Minor => ("⚠", Color::Yellow),
                StatusIndicator::Major | StatusIndicator::Critical => ("✗", Color::Red),
                StatusIndicator::Maintenance => ("🔧", Color::Blue),
                StatusIndicator::Unknown => ("?", Color::DarkGray),
            };
            let description = status
                .description
                .as_deref()
                .unwrap_or("Unknown status");
            lines.push(Line::from(vec![
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::raw(" "),
                Span::raw(description.to_string()),
            ]));
        }

        lines
    }
}

impl Widget for ProviderPanel<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Build the provider name for the title
        let provider_name = &self.payload.provider;
        let source = &self.payload.source;
        let title = format!(" {provider_name} ({source}) ");

        // Create the block with appropriate styling
        let border_style = if self.selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        // Render the main content
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(inner);

        // Render usage gauge if available
        let usage = &self.payload.usage;
        if let Some(primary) = &usage.primary {
            let remaining = primary.remaining_percent();
            let used = 100.0 - remaining;
            let color = Self::usage_color(remaining);

            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(color))
                .percent(used as u16)
                .label(format!("{used:.0}% used"));

            gauge.render(chunks[0], buf);
        }

        // Render usage details
        let lines = self.build_usage_lines();
        let paragraph = Paragraph::new(lines);
        paragraph.render(chunks[1], buf);
    }
}
