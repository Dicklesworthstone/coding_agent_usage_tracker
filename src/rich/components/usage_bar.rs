//! Usage bar component for displaying percentage usage.

use std::fmt::Write;

use crate::rich::{Renderable, ThemeConfig, parse_color};
use rich_rust::prelude::*;

use super::formatters::percentage_color;

/// A visual progress bar showing usage percentage.
#[derive(Debug, Clone)]
pub struct UsageBar {
    percentage: f64,
    width: usize,
    show_percentage: bool,
    label: Option<String>,
}

impl UsageBar {
    /// Create a new usage bar with the given percentage (0-100).
    #[must_use]
    pub const fn new(percentage: f64) -> Self {
        Self {
            percentage: percentage.clamp(0.0, 100.0),
            width: 20,
            show_percentage: true,
            label: None,
        }
    }

    /// Set the bar width (number of characters).
    #[must_use]
    pub fn width(mut self, width: usize) -> Self {
        self.width = width.max(5);
        self
    }

    /// Set whether to show the percentage value.
    #[must_use]
    pub const fn show_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    /// Set a label to display before the bar.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Get the fill color based on the percentage.
    #[must_use]
    pub fn fill_color(&self) -> Color {
        percentage_color(self.percentage)
    }

    /// Render the bar as styled segments.
    #[must_use]
    pub fn render_segments(&self, theme: &ThemeConfig) -> Vec<Segment<'static>> {
        let mut segments = Vec::new();

        // Label
        if let Some(label) = &self.label {
            segments.push(Segment::styled(format!("{label}: "), theme.muted.clone()));
        }

        // Calculate fill
        #[allow(clippy::cast_precision_loss)] // width is small
        let width_f = self.width as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // percentage is 0-100
        let filled = ((self.percentage / 100.0) * width_f).round() as usize;
        let empty = self.width.saturating_sub(filled);

        // Bar characters
        let fill_char = '█';
        let empty_char = '░';

        // Filled portion
        let fill_style = Style::new().color(self.fill_color());
        segments.push(Segment::styled(
            fill_char.to_string().repeat(filled),
            fill_style,
        ));

        // Empty portion
        let empty_style = Style::new().color(parse_color("bright_black"));
        segments.push(Segment::styled(
            empty_char.to_string().repeat(empty),
            empty_style,
        ));

        // Percentage
        if self.show_percentage {
            let pct_str = format!(" {:>5.1}%", self.percentage);
            let pct_style = if self.percentage >= 80.0 {
                theme.percentage_high.clone()
            } else {
                theme.percentage.clone()
            };
            segments.push(Segment::styled(pct_str, pct_style));
        }

        segments
    }

    /// Render as plain ASCII.
    fn render_ascii(&self) -> String {
        #[allow(clippy::cast_precision_loss)] // width is small
        let width_f = self.width as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // percentage is 0-100
        let filled = ((self.percentage / 100.0) * width_f).round() as usize;
        let empty = self.width.saturating_sub(filled);

        let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(empty));

        let mut result = String::new();

        if let Some(label) = &self.label {
            result.push_str(label);
            result.push_str(": ");
        }

        result.push_str(&bar);

        if self.show_percentage {
            let _ = write!(result, " {:>5.1}%", self.percentage);
        }

        result
    }
}

impl Renderable for UsageBar {
    fn render(&self) -> String {
        #[allow(clippy::cast_precision_loss)] // width is small
        let width_f = self.width as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // percentage is 0-100
        let filled = ((self.percentage / 100.0) * width_f).round() as usize;
        let empty = self.width.saturating_sub(filled);

        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        let mut result = String::new();

        if let Some(label) = &self.label {
            result.push_str(label);
            result.push_str(": ");
        }

        result.push_str(&bar);

        if self.show_percentage {
            let _ = write!(result, " {:>5.1}%", self.percentage);
        }

        result
    }

    fn render_plain(&self) -> String {
        self.render_ascii()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::create_default_theme;

    fn assert_no_ansi(s: &str) {
        assert!(
            !s.contains("\x1b["),
            "Contains ANSI: {}",
            &s[..100.min(s.len())]
        );
    }

    #[test]
    fn test_usage_bar_new() {
        let bar = UsageBar::new(50.0);
        assert!((bar.percentage - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_usage_bar_clamps_percentage() {
        let bar = UsageBar::new(150.0);
        assert!((bar.percentage - 100.0).abs() < 0.001);

        let bar = UsageBar::new(-10.0);
        assert!(bar.percentage.abs() < 0.001);
    }

    #[test]
    fn test_usage_bar_render() {
        let bar = UsageBar::new(50.0).width(10);
        let rendered = bar.render();
        assert!(rendered.contains("█"));
        assert!(rendered.contains("░"));
    }

    #[test]
    fn test_usage_bar_render_plain() {
        let bar = UsageBar::new(50.0).width(10);
        let plain = bar.render_plain();
        assert_no_ansi(&plain);
        assert!(plain.contains('['));
        assert!(plain.contains(']'));
        assert!(plain.contains('#'));
        assert!(plain.contains('-'));
    }

    #[test]
    fn test_usage_bar_with_label() {
        let bar = UsageBar::new(75.0).with_label("Session");
        let rendered = bar.render();
        assert!(rendered.contains("Session"));
    }

    #[test]
    fn test_usage_bar_no_percentage() {
        let bar = UsageBar::new(50.0).show_percentage(false);
        let rendered = bar.render();
        assert!(!rendered.contains('%'));
    }

    #[test]
    fn test_usage_bar_segments() {
        let theme = create_default_theme();
        let bar = UsageBar::new(75.0).with_label("Usage");
        let segments = bar.render_segments(&theme);
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_usage_bar_color_varies_by_percentage() {
        let low = UsageBar::new(20.0);
        let mid = UsageBar::new(60.0);
        let high = UsageBar::new(90.0);

        // Different percentages should have different colors
        let low_color = format!("{:?}", low.fill_color());
        let mid_color = format!("{:?}", mid.fill_color());
        let high_color = format!("{:?}", high.fill_color());

        assert_ne!(low_color, high_color);
        assert_ne!(mid_color, high_color);
    }
}
