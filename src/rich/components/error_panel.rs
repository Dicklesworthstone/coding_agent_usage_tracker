//! Error panel component for displaying errors with suggestions.

use crate::rich::{Renderable, ThemeConfig};
use rich_rust::prelude::*;

/// A styled panel for displaying error messages with optional suggestions.
#[derive(Debug, Clone)]
pub struct ErrorPanel {
    title: String,
    message: String,
    suggestions: Vec<String>,
    details: Option<String>,
}

impl ErrorPanel {
    /// Create a new error panel with a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            title: "Error".to_string(),
            message: message.into(),
            suggestions: Vec::new(),
            details: None,
        }
    }

    /// Create an error panel from a `CautError`.
    #[must_use]
    pub fn from_error(err: &crate::error::CautError) -> Self {
        let mut panel = Self::new(err.to_string());
        panel.title = err.category().to_string();

        // Add suggestions from the error if available
        for fix_suggestion in err.fix_suggestions() {
            // Add the context as a suggestion
            if !fix_suggestion.context.is_empty() {
                panel = panel.with_suggestion(&fix_suggestion.context);
            }
            // Add any commands as suggestions
            for cmd in &fix_suggestion.commands {
                panel = panel.with_suggestion(format!("Run: {cmd}"));
            }
        }

        panel
    }

    /// Set a custom title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Add a suggestion.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Add multiple suggestions.
    #[must_use]
    pub fn with_suggestions(
        mut self,
        suggestions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.suggestions
            .extend(suggestions.into_iter().map(Into::into));
        self
    }

    /// Add additional details.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Render as a rich Panel.
    #[must_use]
    pub fn render_panel(&self, theme: &ThemeConfig) -> Panel<'static> {
        let mut content_lines: Vec<Vec<Segment<'static>>> = Vec::new();

        // Message line
        content_lines.push(vec![Segment::plain(self.message.clone())]);

        // Details if present
        if let Some(details) = &self.details {
            content_lines.push(vec![]); // Empty line
            content_lines.push(vec![Segment::styled(details.clone(), theme.muted.clone())]);
        }

        // Suggestions
        if !self.suggestions.is_empty() {
            content_lines.push(vec![]); // Empty line
            content_lines.push(vec![Segment::styled(
                "Suggestions:".to_string(),
                theme.primary.clone(),
            )]);

            for suggestion in &self.suggestions {
                content_lines.push(vec![
                    Segment::styled("  • ".to_string(), theme.warning.clone()),
                    Segment::plain(suggestion.clone()),
                ]);
            }
        }

        let title = Text::new(self.title.clone());
        let mut panel = Panel::new(content_lines).title(title);
        panel = panel.border_style(theme.panel_error_border.clone());
        panel
    }

    /// Render as styled segments.
    #[must_use]
    pub fn render_segments(&self, theme: &ThemeConfig) -> Vec<Vec<Segment<'static>>> {
        let mut lines = Vec::new();

        // Title line
        lines.push(vec![
            Segment::styled("✗ ".to_string(), theme.status_error.clone()),
            Segment::styled(self.title.clone(), theme.error.clone()),
        ]);

        // Message line
        lines.push(vec![Segment::plain(self.message.clone())]);

        // Details if present
        if let Some(details) = &self.details {
            lines.push(vec![Segment::styled(details.clone(), theme.muted.clone())]);
        }

        // Suggestions
        if !self.suggestions.is_empty() {
            lines.push(vec![Segment::styled(
                "Suggestions:".to_string(),
                theme.primary.clone(),
            )]);

            for suggestion in &self.suggestions {
                lines.push(vec![
                    Segment::styled("  💡 ".to_string(), theme.warning.clone()),
                    Segment::plain(suggestion.clone()),
                ]);
            }
        }

        lines
    }
}

impl Renderable for ErrorPanel {
    fn render(&self) -> String {
        let mut lines = Vec::new();

        // Title with error icon
        lines.push(format!("✗ {}", self.title));
        lines.push("─".repeat(40));

        // Message
        lines.push(self.message.clone());

        // Details
        if let Some(details) = &self.details {
            lines.push(String::new());
            lines.push(details.clone());
        }

        // Suggestions
        if !self.suggestions.is_empty() {
            lines.push(String::new());
            lines.push("Suggestions:".to_string());
            for suggestion in &self.suggestions {
                lines.push(format!("  💡 {suggestion}"));
            }
        }

        lines.join("\n")
    }

    fn render_plain(&self) -> String {
        let mut lines = Vec::new();

        // Title with ASCII error indicator
        lines.push(format!("[X] {}", self.title));
        lines.push("-".repeat(40));

        // Message
        lines.push(self.message.clone());

        // Details
        if let Some(details) = &self.details {
            lines.push(String::new());
            lines.push(details.clone());
        }

        // Suggestions with ASCII bullet
        if !self.suggestions.is_empty() {
            lines.push(String::new());
            lines.push("Suggestions:".to_string());
            for suggestion in &self.suggestions {
                lines.push(format!("  * {suggestion}"));
            }
        }

        lines.join("\n")
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
    fn test_error_panel_shows_message() {
        let panel = ErrorPanel::new("Test error message");
        let out = panel.render();
        assert!(out.contains("Test error message"));
    }

    #[test]
    fn test_error_panel_shows_title() {
        let panel = ErrorPanel::new("Error").with_title("Network Error");
        let out = panel.render();
        assert!(out.contains("Network Error"));
    }

    #[test]
    fn test_error_panel_shows_suggestions() {
        let panel = ErrorPanel::new("Connection failed")
            .with_suggestion("Check your internet connection")
            .with_suggestion("Try again later");
        let out = panel.render();
        assert!(out.contains("Suggestions"));
        assert!(out.contains("Check your internet connection"));
        assert!(out.contains("Try again later"));
    }

    #[test]
    fn test_error_panel_shows_details() {
        let panel =
            ErrorPanel::new("Error occurred").with_details("Additional context about the error");
        let out = panel.render();
        assert!(out.contains("Additional context"));
    }

    #[test]
    fn test_error_panel_plain_no_ansi() {
        let panel = ErrorPanel::new("Test error")
            .with_title("Test")
            .with_suggestion("Fix it")
            .with_details("Details here");
        let plain = panel.render_plain();
        assert_no_ansi(&plain);
        assert!(plain.contains("[X]"));
        assert!(plain.contains('*'));
    }

    #[test]
    fn test_error_panel_render_panel() {
        let theme = create_default_theme();
        let panel = ErrorPanel::new("Test error");
        let _ = panel.render_panel(&theme); // Should not panic
    }

    #[test]
    fn test_error_panel_render_segments() {
        let theme = create_default_theme();
        let panel = ErrorPanel::new("Test error").with_suggestion("Try this");
        let segments = panel.render_segments(&theme);
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_error_panel_with_multiple_suggestions() {
        let suggestions = vec!["First", "Second", "Third"];
        let panel = ErrorPanel::new("Error").with_suggestions(suggestions);
        let out = panel.render();
        assert!(out.contains("First"));
        assert!(out.contains("Second"));
        assert!(out.contains("Third"));
    }
}
