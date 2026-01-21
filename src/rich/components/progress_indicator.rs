//! Progress indicator component for multi-provider fetch progress.

use crate::rich::{Renderable, ThemeConfig};
use rich_rust::prelude::*;

/// Progress indicator for showing multi-provider fetch status.
#[derive(Debug, Clone)]
pub struct ProgressIndicator {
    total: usize,
    completed: usize,
    current_provider: Option<String>,
    failed: usize,
    show_percentage: bool,
    width: usize,
}

impl ProgressIndicator {
    /// Create a new progress indicator with a total count.
    #[must_use]
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
            current_provider: None,
            failed: 0,
            show_percentage: true,
            width: 20,
        }
    }

    /// Tick progress by one.
    #[must_use]
    pub fn tick(mut self) -> Self {
        if self.completed < self.total {
            self.completed += 1;
        }
        self
    }

    /// Set the current provider being processed.
    #[must_use]
    pub fn with_current(mut self, provider: impl Into<String>) -> Self {
        self.current_provider = Some(provider.into());
        self
    }

    /// Clear the current provider.
    #[must_use]
    pub fn clear_current(mut self) -> Self {
        self.current_provider = None;
        self
    }

    /// Record a failure.
    #[must_use]
    pub fn with_failure(mut self) -> Self {
        self.failed += 1;
        self
    }

    /// Set the number of failures.
    #[must_use]
    pub fn with_failures(mut self, count: usize) -> Self {
        self.failed = count;
        self
    }

    /// Set completed count directly.
    #[must_use]
    pub fn with_completed(mut self, completed: usize) -> Self {
        self.completed = completed.min(self.total);
        self
    }

    /// Set whether to show percentage.
    #[must_use]
    pub fn show_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    /// Set the bar width.
    #[must_use]
    pub fn width(mut self, width: usize) -> Self {
        self.width = width.max(5);
        self
    }

    /// Get the completion percentage.
    #[must_use]
    pub fn percentage(&self) -> f64 {
        if self.total == 0 {
            100.0
        } else {
            (self.completed as f64 / self.total as f64) * 100.0
        }
    }

    /// Check if all items are complete.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.completed >= self.total
    }

    /// Get the number of successful completions.
    #[must_use]
    pub fn successful(&self) -> usize {
        self.completed.saturating_sub(self.failed)
    }

    /// Render as styled segments.
    #[must_use]
    pub fn render_segments(&self, theme: &ThemeConfig) -> Vec<Segment<'static>> {
        let mut segments = Vec::new();

        // Progress bar
        let pct = self.percentage();
        let filled = ((pct / 100.0) * self.width as f64).round() as usize;
        let empty = self.width.saturating_sub(filled);

        let fill_color = if self.failed > 0 {
            theme.status_warning.clone()
        } else if self.is_complete() {
            theme.status_success.clone()
        } else {
            theme.primary.clone()
        };

        segments.push(Segment::styled("█".repeat(filled), fill_color));
        segments.push(Segment::styled("░".repeat(empty), theme.muted.clone()));

        // Percentage/count
        if self.show_percentage {
            segments.push(Segment::styled(
                format!(" {}/{}", self.completed, self.total),
                theme.muted.clone(),
            ));
        }

        // Current provider
        if let Some(provider) = &self.current_provider {
            segments.push(Segment::styled(
                format!(" ({})", provider),
                theme.primary.clone(),
            ));
        }

        // Failures
        if self.failed > 0 {
            segments.push(Segment::styled(
                format!(" [{} failed]", self.failed),
                theme.status_error.clone(),
            ));
        }

        segments
    }
}

impl Renderable for ProgressIndicator {
    fn render(&self) -> String {
        let pct = self.percentage();
        let filled = ((pct / 100.0) * self.width as f64).round() as usize;
        let empty = self.width.saturating_sub(filled);

        let mut result = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        if self.show_percentage {
            result.push_str(&format!(" {}/{}", self.completed, self.total));
        }

        if let Some(provider) = &self.current_provider {
            result.push_str(&format!(" ({})", provider));
        }

        if self.failed > 0 {
            result.push_str(&format!(" [{} failed]", self.failed));
        }

        result
    }

    fn render_plain(&self) -> String {
        let pct = self.percentage();
        let filled = ((pct / 100.0) * self.width as f64).round() as usize;
        let empty = self.width.saturating_sub(filled);

        let mut result = format!("[{}{}]", "#".repeat(filled), "-".repeat(empty));

        if self.show_percentage {
            result.push_str(&format!(" {}/{}", self.completed, self.total));
        }

        if let Some(provider) = &self.current_provider {
            result.push_str(&format!(" ({})", provider));
        }

        if self.failed > 0 {
            result.push_str(&format!(" [{} failed]", self.failed));
        }

        result
    }
}

/// Spinner indicator for indeterminate progress.
#[derive(Debug, Clone)]
pub struct Spinner {
    frames: Vec<&'static str>,
    current: usize,
    label: Option<String>,
}

impl Spinner {
    /// Create a new spinner with default frames.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            current: 0,
            label: None,
        }
    }

    /// Create a spinner with a label.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Advance to the next frame.
    #[must_use]
    pub fn tick(mut self) -> Self {
        self.current = (self.current + 1) % self.frames.len();
        self
    }

    /// Get the current frame.
    #[must_use]
    pub fn frame(&self) -> &str {
        self.frames[self.current]
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for Spinner {
    fn render(&self) -> String {
        if let Some(label) = &self.label {
            format!("{} {}", self.frame(), label)
        } else {
            self.frame().to_string()
        }
    }

    fn render_plain(&self) -> String {
        let plain_frames = [".", "..", "...", "...."];
        let frame = plain_frames[self.current % plain_frames.len()];
        if let Some(label) = &self.label {
            format!("{} {}", frame, label)
        } else {
            frame.to_string()
        }
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
    fn test_progress_indicator_new() {
        let progress = ProgressIndicator::new(5);
        assert_eq!(progress.total, 5);
        assert_eq!(progress.completed, 0);
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_progress_indicator_tick() {
        let progress = ProgressIndicator::new(3).tick().tick();
        assert_eq!(progress.completed, 2);
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_progress_indicator_complete() {
        let progress = ProgressIndicator::new(2).tick().tick();
        assert!(progress.is_complete());
        assert!((progress.percentage() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_progress_indicator_render() {
        let progress = ProgressIndicator::new(4).with_completed(2);
        let out = progress.render();
        assert!(out.contains("2/4"));
    }

    #[test]
    fn test_progress_indicator_plain_no_ansi() {
        let progress = ProgressIndicator::new(4)
            .with_completed(2)
            .with_current("Claude")
            .with_failure();
        let plain = progress.render_plain();
        assert_no_ansi(&plain);
        assert!(plain.contains("["));
        assert!(plain.contains("]"));
    }

    #[test]
    fn test_progress_indicator_with_current() {
        let progress = ProgressIndicator::new(3).with_current("Claude");
        let out = progress.render();
        assert!(out.contains("Claude"));
    }

    #[test]
    fn test_progress_indicator_with_failures() {
        let progress = ProgressIndicator::new(3).with_completed(3).with_failures(1);
        let out = progress.render();
        assert!(out.contains("1 failed"));
        assert_eq!(progress.successful(), 2);
    }

    #[test]
    fn test_progress_indicator_segments() {
        let theme = create_default_theme();
        let progress = ProgressIndicator::new(3).with_completed(1);
        let segments = progress.render_segments(&theme);
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_progress_indicator_edge_zero_total() {
        let progress = ProgressIndicator::new(0);
        assert!(progress.is_complete());
        assert!((progress.percentage() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_spinner_new() {
        let spinner = Spinner::new();
        assert!(!spinner.frame().is_empty());
    }

    #[test]
    fn test_spinner_tick() {
        let spinner = Spinner::new();
        let frame1 = spinner.frame().to_string();
        let spinner = spinner.tick();
        let frame2 = spinner.frame();
        assert_ne!(frame1, frame2);
    }

    #[test]
    fn test_spinner_with_label() {
        let spinner = Spinner::new().with_label("Loading");
        let out = spinner.render();
        assert!(out.contains("Loading"));
    }

    #[test]
    fn test_spinner_plain_no_ansi() {
        let spinner = Spinner::new().with_label("Working");
        let plain = spinner.render_plain();
        assert_no_ansi(&plain);
        assert!(plain.contains("."));
    }
}
