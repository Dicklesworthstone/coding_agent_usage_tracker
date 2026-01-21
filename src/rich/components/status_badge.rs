//! Status badge component for inline indicators.

use crate::rich::{Renderable, ThemeConfig};
use rich_rust::prelude::*;

/// Status level for a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    /// Success (green check).
    Success,
    /// Warning (yellow).
    Warning,
    /// Error (red X).
    Error,
    /// Info (blue).
    Info,
    /// Neutral/unknown.
    Neutral,
}

impl StatusLevel {
    /// Get the icon for this status level.
    #[must_use]
    pub const fn icon(&self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Warning => "⚠",
            Self::Error => "✗",
            Self::Info => "ℹ",
            Self::Neutral => "•",
        }
    }

    /// Get the plain text icon for this status level.
    #[must_use]
    pub const fn plain_icon(&self) -> &'static str {
        match self {
            Self::Success => "[OK]",
            Self::Warning => "[!]",
            Self::Error => "[X]",
            Self::Info => "[i]",
            Self::Neutral => "[-]",
        }
    }
}

/// A styled status badge with icon and optional label.
#[derive(Debug, Clone)]
pub struct StatusBadge {
    level: StatusLevel,
    label: Option<String>,
    use_unicode: bool,
}

impl StatusBadge {
    /// Create a new status badge with the given level.
    #[must_use]
    pub fn new(level: StatusLevel) -> Self {
        Self {
            level,
            label: None,
            use_unicode: true,
        }
    }

    /// Create a success badge.
    #[must_use]
    pub fn success() -> Self {
        Self::new(StatusLevel::Success)
    }

    /// Create a warning badge.
    #[must_use]
    pub fn warning() -> Self {
        Self::new(StatusLevel::Warning)
    }

    /// Create an error badge.
    #[must_use]
    pub fn error() -> Self {
        Self::new(StatusLevel::Error)
    }

    /// Create an info badge.
    #[must_use]
    pub fn info() -> Self {
        Self::new(StatusLevel::Info)
    }

    /// Add a label to the badge.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Disable Unicode icons (use ASCII fallback).
    #[must_use]
    pub fn ascii(mut self) -> Self {
        self.use_unicode = false;
        self
    }

    /// Get the style for this badge's level.
    #[must_use]
    pub fn style<'a>(&self, theme: &'a ThemeConfig) -> &'a Style {
        match self.level {
            StatusLevel::Success => &theme.status_success,
            StatusLevel::Warning => &theme.status_warning,
            StatusLevel::Error => &theme.status_error,
            StatusLevel::Info => &theme.primary,
            StatusLevel::Neutral => &theme.muted,
        }
    }

    /// Render the badge as styled segments.
    #[must_use]
    pub fn render_segments(&self, theme: &ThemeConfig) -> Vec<Segment<'static>> {
        let style = self.style(theme).clone();
        let icon = if self.use_unicode {
            self.level.icon()
        } else {
            self.level.plain_icon()
        };

        let mut segments = vec![Segment::styled(icon.to_string(), style.clone())];

        if let Some(label) = &self.label {
            segments.push(Segment::plain(" "));
            segments.push(Segment::styled(label.clone(), style));
        }

        segments
    }
}

impl Renderable for StatusBadge {
    fn render(&self) -> String {
        let icon = if self.use_unicode {
            self.level.icon()
        } else {
            self.level.plain_icon()
        };

        if let Some(label) = &self.label {
            format!("{} {}", icon, label)
        } else {
            icon.to_string()
        }
    }

    fn render_plain(&self) -> String {
        let icon = self.level.plain_icon();
        if let Some(label) = &self.label {
            format!("{} {}", icon, label)
        } else {
            icon.to_string()
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
    fn test_status_badge_success() {
        let badge = StatusBadge::success();
        assert_eq!(badge.level, StatusLevel::Success);
        assert!(badge.render().contains("✓"));
    }

    #[test]
    fn test_status_badge_with_label() {
        let badge = StatusBadge::success().with_label("OK");
        assert!(badge.render().contains("OK"));
    }

    #[test]
    fn test_status_badge_ascii_mode() {
        let badge = StatusBadge::error().ascii();
        assert!(badge.render().contains("[X]"));
    }

    #[test]
    fn test_status_badge_plain_no_ansi() {
        let badge = StatusBadge::warning().with_label("Warning");
        let plain = badge.render_plain();
        assert_no_ansi(&plain);
        assert!(plain.contains("[!]"));
        assert!(plain.contains("Warning"));
    }

    #[test]
    fn test_status_badge_all_levels_have_icons() {
        let levels = [
            StatusLevel::Success,
            StatusLevel::Warning,
            StatusLevel::Error,
            StatusLevel::Info,
            StatusLevel::Neutral,
        ];
        for level in levels {
            assert!(!level.icon().is_empty());
            assert!(!level.plain_icon().is_empty());
        }
    }

    #[test]
    fn test_status_badge_segments() {
        let theme = create_default_theme();
        let badge = StatusBadge::success().with_label("Done");
        let segments = badge.render_segments(&theme);
        assert!(!segments.is_empty());
    }
}
