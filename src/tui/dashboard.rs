//! Dashboard widget for the TUI.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::core::models::ProviderPayload;

use super::provider_panel::ProviderPanel;

/// The main dashboard layout.
pub struct Dashboard<'a> {
    /// Provider payloads to display.
    payloads: &'a [ProviderPayload],
    /// Any error messages to display.
    errors: &'a [String],
    /// Currently selected panel index.
    selected: usize,
    /// Last update timestamp.
    last_update: Option<chrono::DateTime<chrono::Utc>>,
    /// Show help overlay.
    show_help: bool,
}

impl<'a> Dashboard<'a> {
    /// Create a new dashboard.
    #[must_use]
    pub fn new(
        payloads: &'a [ProviderPayload],
        errors: &'a [String],
        selected: usize,
        last_update: Option<chrono::DateTime<chrono::Utc>>,
        show_help: bool,
    ) -> Self {
        Self {
            payloads,
            errors,
            selected,
            last_update,
            show_help,
        }
    }

    /// Calculate grid layout based on terminal width.
    fn calculate_grid(&self, area: Rect) -> (usize, Vec<Rect>) {
        let num_providers = self.payloads.len();
        if num_providers == 0 {
            return (0, Vec::new());
        }

        // Determine number of columns based on width
        let cols = if area.width >= 160 {
            4
        } else if area.width >= 120 {
            3
        } else if area.width >= 80 {
            2
        } else {
            1
        };

        // Calculate rows needed
        let rows = (num_providers + cols - 1) / cols;

        // Create row constraints
        let row_constraints: Vec<Constraint> = (0..rows)
            .map(|_| Constraint::Ratio(1, rows as u32))
            .collect();

        let row_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(area);

        // Create column constraints for each row
        let mut panel_areas = Vec::with_capacity(num_providers);
        for (row_idx, row_area) in row_chunks.iter().enumerate() {
            let start_idx = row_idx * cols;
            let end_idx = (start_idx + cols).min(num_providers);
            let items_in_row = end_idx - start_idx;

            if items_in_row == 0 {
                break;
            }

            let col_constraints: Vec<Constraint> = (0..items_in_row)
                .map(|_| Constraint::Ratio(1, items_in_row as u32))
                .collect();

            let col_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints)
                .split(*row_area);

            panel_areas.extend(col_chunks.iter().copied());
        }

        (cols, panel_areas)
    }

    /// Render the header.
    fn render_header(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let title = Line::from(vec![
            Span::styled(
                " caut dashboard ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("                                        "),
            Span::styled("[?] Help  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[r] Refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[q] Quit", Style::default().fg(Color::DarkGray)),
        ]);

        let header = Paragraph::new(title).style(Style::default().bg(Color::DarkGray));
        header.render(area, buf);
    }

    /// Render the footer.
    fn render_footer(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let update_text = if let Some(ts) = self.last_update {
            let now = chrono::Utc::now();
            let secs_ago = (now - ts).num_seconds();
            format!("Last updated: {secs_ago}s ago")
        } else {
            "Fetching...".to_string()
        };

        let footer = Line::from(vec![
            Span::raw(" "),
            Span::styled(update_text, Style::default().fg(Color::DarkGray)),
            Span::raw("    "),
            Span::styled(
                "Press 'r' to refresh now",
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        let paragraph = Paragraph::new(footer);
        paragraph.render(area, buf);
    }

    /// Render errors panel.
    fn render_errors(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if self.errors.is_empty() {
            return;
        }

        let error_lines: Vec<Line> = self
            .errors
            .iter()
            .map(|e| {
                Line::from(vec![
                    Span::styled("⚠ ", Style::default().fg(Color::Yellow)),
                    Span::raw(e.clone()),
                ])
            })
            .collect();

        let block = Block::default()
            .title(" Warnings ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(error_lines).block(block);
        paragraph.render(area, buf);
    }

    /// Render the help overlay.
    fn render_help(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let help_text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  caut dashboard - Keyboard Shortcuts",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  Navigation:"),
            Line::from("    ↑/k, ↓/j      Move selection up/down"),
            Line::from("    ←/h, →/l      Move selection left/right"),
            Line::from(""),
            Line::from("  Actions:"),
            Line::from("    r, F5         Refresh data now"),
            Line::from("    ?, F1         Toggle this help"),
            Line::from("    q, Esc        Quit"),
            Line::from(""),
            Line::from(Span::styled(
                "  Press any key to close",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        // Center the help box
        let help_width = 50;
        let help_height = 15;
        let x = area.x + (area.width.saturating_sub(help_width)) / 2;
        let y = area.y + (area.height.saturating_sub(help_height)) / 2;
        let help_area = Rect::new(x, y, help_width, help_height);

        let block = Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));

        let paragraph = Paragraph::new(help_text).block(block);
        paragraph.render(help_area, buf);
    }
}

impl Widget for Dashboard<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Main layout: header, content, errors (if any), footer
        let has_errors = !self.errors.is_empty();
        let error_height = if has_errors {
            (self.errors.len() + 2).min(5) as u16
        } else {
            0
        };

        let constraints = if has_errors {
            vec![
                Constraint::Length(1),            // Header
                Constraint::Min(10),              // Provider panels
                Constraint::Length(error_height), // Errors
                Constraint::Length(1),            // Footer
            ]
        } else {
            vec![
                Constraint::Length(1), // Header
                Constraint::Min(10),   // Provider panels
                Constraint::Length(1), // Footer
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Render header
        self.render_header(chunks[0], buf);

        // Render provider panels
        let content_area = chunks[1];
        let (_, panel_areas) = self.calculate_grid(content_area);

        for (i, (payload, panel_area)) in self.payloads.iter().zip(panel_areas.iter()).enumerate() {
            let is_selected = i == self.selected;
            let panel = ProviderPanel::new(payload, is_selected);
            panel.render(*panel_area, buf);
        }

        // Render errors if present
        if has_errors {
            self.render_errors(chunks[2], buf);
            self.render_footer(chunks[3], buf);
        } else {
            self.render_footer(chunks[2], buf);
        }

        // Render help overlay if active
        if self.show_help {
            self.render_help(area, buf);
        }
    }
}
