//! Usage table component for multi-provider comparison.

use crate::core::models::{ProviderPayload, StatusIndicator};
use crate::core::provider::Provider;
use crate::rich::{Renderable, ThemeConfig};
use rich_rust::prelude::*;

use super::formatters::{format_cost, format_percentage};

/// Convert a provider string to display name using Provider enum if possible.
fn provider_display_name(provider: &str) -> String {
    Provider::from_cli_name(provider).map_or_else(
        |_| {
            // Fallback: title case the provider name
            let mut chars = provider.chars();
            chars.next().map_or_else(String::new, |c| {
                c.to_uppercase().collect::<String>() + chars.as_str()
            })
        },
        |p| p.display_name().to_string(),
    )
}

/// A table displaying usage data for multiple providers.
#[derive(Debug)]
pub struct UsageTable<'a> {
    providers: &'a [ProviderPayload],
    theme: &'a ThemeConfig,
    show_totals: bool,
    compact: bool,
}

impl<'a> UsageTable<'a> {
    /// Create a new usage table.
    #[must_use]
    pub const fn new(providers: &'a [ProviderPayload], theme: &'a ThemeConfig) -> Self {
        Self {
            providers,
            theme,
            show_totals: false,
            compact: false,
        }
    }

    /// Enable totals row.
    #[must_use]
    pub const fn with_totals(mut self) -> Self {
        self.show_totals = true;
        self
    }

    /// Use compact rendering mode.
    #[must_use]
    pub const fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Check if the table is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Get the number of providers.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.providers.len()
    }

    /// Build a `rich_rust` Table with styled content.
    #[must_use]
    pub fn build_table(&self) -> Table {
        let mut table = Table::new();

        // Add header row
        table.add_row_cells(["Provider", "Session", "Weekly", "Credits", "Status"]);

        // Add provider rows
        for payload in self.providers {
            let provider_name = provider_display_name(&payload.provider);

            let session = payload
                .usage
                .primary
                .as_ref()
                .map_or_else(|| "—".to_string(), |w| format_percentage(w.used_percent));

            let weekly = payload
                .usage
                .secondary
                .as_ref()
                .map_or_else(|| "—".to_string(), |w| format_percentage(w.used_percent));

            let credits = payload
                .credits
                .as_ref()
                .map_or_else(|| "—".to_string(), |c| format_cost(c.remaining));

            let status = payload.status.as_ref().map_or_else(
                || "—".to_string(),
                |s| {
                    if s.indicator == StatusIndicator::None {
                        "✓".to_string()
                    } else {
                        "⚠".to_string()
                    }
                },
            );

            table.add_row_cells([provider_name.as_str(), &session, &weekly, &credits, &status]);
        }

        // Add totals row if enabled
        if self.show_totals && !self.providers.is_empty() {
            let total_credits: f64 = self
                .providers
                .iter()
                .filter_map(|p| p.credits.as_ref().map(|c| c.remaining))
                .sum();

            table.add_row_cells(["Total", "—", "—", &format_cost(total_credits), "—"]);
        }

        table
    }

    /// Render the table as styled segments for each row.
    #[must_use]
    pub fn render_segments(&self) -> Vec<Vec<Segment<'static>>> {
        let mut rows = Vec::new();

        // Header row
        let header_style = self.theme.table_header.clone();
        rows.push(vec![
            Segment::styled("Provider".to_string(), header_style.clone()),
            Segment::styled("Session".to_string(), header_style.clone()),
            Segment::styled("Weekly".to_string(), header_style.clone()),
            Segment::styled("Credits".to_string(), header_style.clone()),
            Segment::styled("Status".to_string(), header_style),
        ]);

        // Provider rows
        for (i, payload) in self.providers.iter().enumerate() {
            let provider_style = self
                .theme
                .provider_style(&provider_display_name(&payload.provider));
            let row_style = if i % 2 == 1 {
                self.theme.table_row_alt.clone().unwrap_or_default()
            } else {
                Style::new()
            };

            let provider_name = provider_display_name(&payload.provider);

            let session = payload
                .usage
                .primary
                .as_ref()
                .map_or_else(|| "—".to_string(), |w| format_percentage(w.used_percent));

            let weekly = payload
                .usage
                .secondary
                .as_ref()
                .map_or_else(|| "—".to_string(), |w| format_percentage(w.used_percent));

            let credits = payload
                .credits
                .as_ref()
                .map_or_else(|| "—".to_string(), |c| format_cost(c.remaining));

            let status_text = payload.status.as_ref().map_or("—", |s| {
                if s.indicator == StatusIndicator::None {
                    "✓"
                } else {
                    "⚠"
                }
            });

            let status_style = payload.status.as_ref().map_or_else(
                || row_style.clone(),
                |s| {
                    if s.indicator == StatusIndicator::None {
                        self.theme.status_success.clone()
                    } else {
                        self.theme.status_warning.clone()
                    }
                },
            );

            rows.push(vec![
                Segment::styled(provider_name, provider_style.clone()),
                Segment::styled(session, row_style.clone()),
                Segment::styled(weekly, row_style.clone()),
                Segment::styled(credits, self.theme.cost.clone()),
                Segment::styled(status_text.to_string(), status_style),
            ]);
        }

        rows
    }
}

impl Renderable for UsageTable<'_> {
    fn render(&self) -> String {
        if self.providers.is_empty() {
            return "No provider data available.".to_string();
        }

        let mut lines = Vec::new();

        // Header
        lines.push(format!(
            "{:<15} {:>10} {:>10} {:>12} {:>8}",
            "Provider", "Session", "Weekly", "Credits", "Status"
        ));
        lines.push("─".repeat(60));

        // Provider rows
        for payload in self.providers {
            let provider_name = provider_display_name(&payload.provider);

            let session = payload
                .usage
                .primary
                .as_ref()
                .map_or_else(|| "—".to_string(), |w| format_percentage(w.used_percent));

            let weekly = payload
                .usage
                .secondary
                .as_ref()
                .map_or_else(|| "—".to_string(), |w| format_percentage(w.used_percent));

            let credits = payload
                .credits
                .as_ref()
                .map_or_else(|| "—".to_string(), |c| format_cost(c.remaining));

            let status = payload.status.as_ref().map_or("—", |s| {
                if s.indicator == StatusIndicator::None {
                    "✓"
                } else {
                    "⚠"
                }
            });

            lines.push(format!(
                "{provider_name:<15} {session:>10} {weekly:>10} {credits:>12} {status:>8}"
            ));
        }

        // Totals row
        if self.show_totals && !self.providers.is_empty() {
            lines.push("─".repeat(60));
            let total_credits: f64 = self
                .providers
                .iter()
                .filter_map(|p| p.credits.as_ref().map(|c| c.remaining))
                .sum();

            lines.push(format!(
                "{:<15} {:>10} {:>10} {:>12} {:>8}",
                "Total",
                "—",
                "—",
                format_cost(total_credits),
                "—"
            ));
        }

        lines.join("\n")
    }

    fn render_plain(&self) -> String {
        if self.providers.is_empty() {
            return "No provider data available.".to_string();
        }

        let mut lines = Vec::new();

        // Header
        lines.push(format!(
            "{:<15} {:>10} {:>10} {:>12} {:>8}",
            "Provider", "Session", "Weekly", "Credits", "Status"
        ));
        lines.push("-".repeat(60));

        // Provider rows
        for payload in self.providers {
            let provider_name = provider_display_name(&payload.provider);

            let session = payload
                .usage
                .primary
                .as_ref()
                .map_or_else(|| "-".to_string(), |w| format_percentage(w.used_percent));

            let weekly = payload
                .usage
                .secondary
                .as_ref()
                .map_or_else(|| "-".to_string(), |w| format_percentage(w.used_percent));

            let credits = payload
                .credits
                .as_ref()
                .map_or_else(|| "-".to_string(), |c| format_cost(c.remaining));

            let status = payload.status.as_ref().map_or("-", |s| {
                if s.indicator == StatusIndicator::None {
                    "[OK]"
                } else {
                    "[!]"
                }
            });

            lines.push(format!(
                "{provider_name:<15} {session:>10} {weekly:>10} {credits:>12} {status:>8}"
            ));
        }

        // Totals row
        if self.show_totals && !self.providers.is_empty() {
            lines.push("-".repeat(60));
            let total_credits: f64 = self
                .providers
                .iter()
                .filter_map(|p| p.credits.as_ref().map(|c| c.remaining))
                .sum();

            lines.push(format!(
                "{:<15} {:>10} {:>10} {:>12} {:>8}",
                "Total",
                "-",
                "-",
                format_cost(total_credits),
                "-"
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
    fn test_usage_table_empty() {
        let theme = create_default_theme();
        let table = UsageTable::new(&[], &theme);
        let out = table.render();
        assert!(out.contains("No") || !out.is_empty());
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn test_usage_table_single_provider() {
        let payloads = vec![make_test_provider_payload("claude", "oauth")];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme);
        let out = table.render();
        assert!(out.contains("Claude"));
        assert!(!table.is_empty());
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_usage_table_multiple_providers() {
        let payloads = vec![
            make_test_provider_payload("claude", "oauth"),
            make_test_provider_payload("codex", "cli"),
        ];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme);
        let out = table.render();
        assert!(out.contains("Claude"));
        assert!(out.contains("Codex"));
    }

    #[test]
    fn test_usage_table_totals() {
        let payloads = vec![
            make_test_provider_payload("claude", "oauth"),
            make_test_provider_payload("codex", "cli"),
        ];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme).with_totals();
        let out = table.render();
        assert!(out.contains("Total"));
    }

    #[test]
    fn test_usage_table_plain_no_ansi() {
        let payloads = vec![
            make_test_provider_payload("claude", "oauth"),
            make_test_provider_payload("codex", "cli"),
        ];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme).with_totals();
        let plain = table.render_plain();
        assert_no_ansi(&plain);
    }

    #[test]
    fn test_usage_table_build_table() {
        let payloads = vec![make_test_provider_payload("claude", "oauth")];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme);
        let _ = table.build_table(); // Should not panic
    }

    #[test]
    fn test_usage_table_render_segments() {
        let payloads = vec![make_test_provider_payload("claude", "oauth")];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme);
        let segments = table.render_segments();
        assert!(!segments.is_empty());
        // Should have header + 1 data row
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn test_usage_table_compact_mode() {
        let payloads = vec![make_test_provider_payload("claude", "oauth")];
        let theme = create_default_theme();
        let table = UsageTable::new(&payloads, &theme).compact();
        assert!(table.compact);
    }
}
