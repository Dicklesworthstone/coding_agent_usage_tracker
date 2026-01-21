//! Provider card component for displaying a single provider's usage.

use crate::core::models::{ProviderPayload, RateWindow, StatusIndicator};
use crate::core::provider::Provider;
use crate::rich::{Renderable, ThemeConfig};
use rich_rust::prelude::*;

use super::formatters::{format_duration_short, format_percentage};
use super::status_badge::{StatusBadge, StatusLevel};
use super::usage_bar::UsageBar;

/// Convert a provider string to display name using Provider enum if possible.
fn provider_display_name(provider: &str) -> String {
    Provider::from_cli_name(provider)
        .map(|p| p.display_name().to_string())
        .unwrap_or_else(|_| {
            // Fallback: title case the provider name
            let mut chars = provider.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
}

/// A styled card displaying a provider's usage data.
#[derive(Debug)]
pub struct ProviderCard<'a> {
    payload: &'a ProviderPayload,
    theme: &'a ThemeConfig,
    compact: bool,
}

impl<'a> ProviderCard<'a> {
    /// Create a new provider card.
    #[must_use]
    pub fn new(payload: &'a ProviderPayload, theme: &'a ThemeConfig) -> Self {
        Self {
            payload,
            theme,
            compact: false,
        }
    }

    /// Use compact rendering mode.
    #[must_use]
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Get the provider's display name.
    #[must_use]
    pub fn provider_name(&self) -> String {
        provider_display_name(&self.payload.provider)
    }

    /// Get the source label.
    #[must_use]
    pub fn source_label(&self) -> &str {
        &self.payload.source
    }

    /// Render a rate window as segments.
    fn render_rate_window_segments(
        &self,
        window: &RateWindow,
        label: &str,
    ) -> Vec<Segment<'static>> {
        let mut segments = Vec::new();

        // Label
        segments.push(Segment::styled(
            format!("{}: ", label),
            self.theme.muted.clone(),
        ));

        // Usage bar
        let bar = UsageBar::new(window.used_percent).width(15);
        segments.extend(bar.render_segments(self.theme));

        // Reset info
        if let Some(desc) = &window.reset_description {
            segments.push(Segment::styled(
                format!(" ({})", desc),
                self.theme.muted.clone(),
            ));
        } else if let Some(minutes) = window.window_minutes {
            segments.push(Segment::styled(
                format!(" ({})", format_duration_short(minutes)),
                self.theme.muted.clone(),
            ));
        }

        segments
    }

    /// Render rate window as plain text.
    fn render_rate_window_plain(&self, window: &RateWindow, label: &str) -> String {
        let bar = UsageBar::new(window.used_percent).width(15);
        let bar_str = bar.render_plain();

        let reset_info = if let Some(desc) = &window.reset_description {
            format!(" ({})", desc)
        } else if let Some(minutes) = window.window_minutes {
            format!(" ({})", format_duration_short(minutes))
        } else {
            String::new()
        };

        format!("{}: {}{}", label, bar_str, reset_info)
    }

    /// Render the card header as segments.
    fn render_header_segments(&self) -> Vec<Segment<'static>> {
        let provider_style = self.theme.provider_style(&self.provider_name()).clone();

        vec![
            Segment::styled(self.provider_name(), provider_style),
            Segment::styled(
                format!(" ({})", self.source_label()),
                self.theme.muted.clone(),
            ),
        ]
    }

    /// Render the card content as segments.
    fn render_content_segments(&self) -> Vec<Vec<Segment<'static>>> {
        let mut lines = Vec::new();

        // Primary rate window (Session)
        if let Some(primary) = &self.payload.usage.primary {
            lines.push(self.render_rate_window_segments(primary, "Session"));
        }

        // Secondary rate window (Weekly)
        if let Some(secondary) = &self.payload.usage.secondary {
            lines.push(self.render_rate_window_segments(secondary, "Weekly"));
        }

        // Tertiary rate window (Opus tier, etc.)
        if let Some(tertiary) = &self.payload.usage.tertiary {
            lines.push(self.render_rate_window_segments(tertiary, "Tier"));
        }

        // Credits info (Codex)
        if let Some(credits) = &self.payload.credits {
            let mut credit_line = Vec::new();
            credit_line.push(Segment::styled("Credits: ", self.theme.muted.clone()));
            credit_line.push(Segment::styled(
                format!("${:.2} remaining", credits.remaining),
                self.theme.cost.clone(),
            ));
            lines.push(credit_line);
        }

        // Identity info
        if let Some(identity) = &self.payload.usage.identity {
            if let Some(email) = &identity.account_email {
                let mut id_line = Vec::new();
                id_line.push(Segment::styled("Account: ", self.theme.muted.clone()));
                id_line.push(Segment::plain(email.clone()));
                lines.push(id_line);
            }
        }

        // Status info
        if let Some(status) = &self.payload.status {
            let is_operational = status.indicator == StatusIndicator::None;
            let status_level = if is_operational {
                StatusLevel::Success
            } else {
                StatusLevel::Warning
            };
            let badge = StatusBadge::new(status_level).with_label(
                status.description.as_deref().unwrap_or(if is_operational {
                    "Operational"
                } else {
                    "Issues"
                }),
            );

            let mut status_line = Vec::new();
            status_line.push(Segment::styled("Status: ", self.theme.muted.clone()));
            status_line.extend(badge.render_segments(self.theme));
            lines.push(status_line);
        }

        lines
    }

    /// Render the card as a rich panel.
    #[must_use]
    pub fn render_panel(&self) -> Panel<'static> {
        // Build header
        let header = self.provider_name();

        // Build content lines with segments (preserves styling)
        let content_lines = self.render_content_segments();

        let title = Text::new(header.clone());
        let mut panel = Panel::new(content_lines).title(title);

        // Apply provider color to border
        let provider_style = self.theme.provider_style(&self.provider_name()).clone();
        panel = panel.border_style(provider_style);

        panel
    }
}

impl Renderable for ProviderCard<'_> {
    fn render(&self) -> String {
        let mut lines = Vec::new();

        // Header
        lines.push(format!(
            "─── {} ({}) ───",
            self.provider_name(),
            self.source_label()
        ));

        // Rate windows
        if let Some(primary) = &self.payload.usage.primary {
            lines.push(format!(
                "  Session: {} {}",
                format_percentage(primary.used_percent),
                primary.reset_description.as_deref().unwrap_or("")
            ));
        }

        if let Some(secondary) = &self.payload.usage.secondary {
            lines.push(format!(
                "  Weekly:  {} {}",
                format_percentage(secondary.used_percent),
                secondary.reset_description.as_deref().unwrap_or("")
            ));
        }

        // Credits
        if let Some(credits) = &self.payload.credits {
            lines.push(format!("  Credits: ${:.2} remaining", credits.remaining));
        }

        lines.join("\n")
    }

    fn render_plain(&self) -> String {
        let mut lines = Vec::new();

        // Header
        lines.push(format!(
            "--- {} ({}) ---",
            self.provider_name(),
            self.source_label()
        ));

        // Rate windows
        if let Some(primary) = &self.payload.usage.primary {
            lines.push(self.render_rate_window_plain(primary, "Session"));
        }

        if let Some(secondary) = &self.payload.usage.secondary {
            lines.push(self.render_rate_window_plain(secondary, "Weekly"));
        }

        if let Some(tertiary) = &self.payload.usage.tertiary {
            lines.push(self.render_rate_window_plain(tertiary, "Tier"));
        }

        // Credits
        if let Some(credits) = &self.payload.credits {
            lines.push(format!("Credits: ${:.2} remaining", credits.remaining));
        }

        // Identity
        if let Some(identity) = &self.payload.usage.identity {
            if let Some(email) = &identity.account_email {
                lines.push(format!("Account: {}", email));
            }
        }

        // Status
        if let Some(status) = &self.payload.status {
            let is_operational = status.indicator == StatusIndicator::None;
            let indicator = if is_operational { "[OK]" } else { "[!]" };
            lines.push(format!(
                "Status: {} {}",
                indicator,
                status.description.as_deref().unwrap_or("")
            ));
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::create_default_theme;
    use crate::test_utils::make_test_provider_payload;

    fn assert_no_ansi(s: &str) {
        assert!(
            !s.contains("\x1b["),
            "Contains ANSI: {}",
            &s[..100.min(s.len())]
        );
    }

    #[test]
    fn test_provider_card_new() {
        let payload = make_test_provider_payload("claude", "oauth");
        let theme = create_default_theme();
        let card = ProviderCard::new(&payload, &theme);
        assert_eq!(card.provider_name(), "Claude");
        assert_eq!(card.source_label(), "oauth");
    }

    #[test]
    fn test_provider_card_render() {
        let payload = make_test_provider_payload("claude", "oauth");
        let theme = create_default_theme();
        let card = ProviderCard::new(&payload, &theme);
        let rendered = card.render();
        assert!(rendered.contains("Claude"));
    }

    #[test]
    fn test_provider_card_render_plain_no_ansi() {
        let payload = make_test_provider_payload("claude", "oauth");
        let theme = create_default_theme();
        let card = ProviderCard::new(&payload, &theme);
        let plain = card.render_plain();
        assert_no_ansi(&plain);
        assert!(plain.contains("Claude"));
    }

    #[test]
    fn test_provider_card_shows_rate_windows() {
        let payload = make_test_provider_payload("claude", "cli");
        let theme = create_default_theme();
        let card = ProviderCard::new(&payload, &theme);
        let rendered = card.render_plain();
        // The test payload should have rate windows
        assert!(rendered.contains("Session") || rendered.contains("Weekly"));
    }

    #[test]
    fn test_provider_card_compact() {
        let payload = make_test_provider_payload("codex", "web");
        let theme = create_default_theme();
        let card = ProviderCard::new(&payload, &theme).compact();
        assert!(card.compact);
    }
}
