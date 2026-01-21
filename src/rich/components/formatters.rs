//! Formatting utilities for rich output components.

use crate::rich::{ThemeConfig, parse_color};
use rich_rust::prelude::*;

/// Format a token count with appropriate units (e.g., 5.6M vs 5,678,901).
#[must_use]
pub fn format_token_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Format a token count with comma separators.
#[must_use]
pub fn format_token_count_full(count: u64) -> String {
    let s = count.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len() + (s.len() - 1) / 3);

    for (i, &c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c as char);
    }
    result
}

/// Format a cost value as currency.
#[must_use]
pub fn format_cost(amount: f64) -> String {
    if amount >= 1.0 {
        format!("${:.2}", amount)
    } else if amount >= 0.01 {
        format!("${:.3}", amount)
    } else {
        format!("${:.4}", amount)
    }
}

/// Format a percentage value.
#[must_use]
pub fn format_percentage(value: f64) -> String {
    if value >= 10.0 {
        format!("{:.0}%", value)
    } else if value >= 1.0 {
        format!("{:.1}%", value)
    } else {
        format!("{:.2}%", value)
    }
}

/// Get appropriate color for a percentage value.
///
/// Returns green for low, yellow for medium, red for high usage.
#[must_use]
pub fn percentage_color(value: f64) -> Color {
    if value >= 80.0 {
        parse_color("red")
    } else if value >= 50.0 {
        parse_color("yellow")
    } else {
        parse_color("green")
    }
}

/// Create styled segments for a key-value pair.
#[must_use]
pub fn key_value_segments(key: &str, value: &str, theme: &ThemeConfig) -> Vec<Segment<'static>> {
    vec![
        Segment::styled(format!("{}: ", key), theme.muted.clone()),
        Segment::styled(value.to_string(), Style::new()),
    ]
}

/// Format a duration in human-readable form.
#[must_use]
pub fn format_duration_short(minutes: i32) -> String {
    if minutes < 60 {
        format!("{}m", minutes)
    } else if minutes < 1440 {
        let hours = minutes / 60;
        let mins = minutes % 60;
        if mins == 0 {
            format!("{}h", hours)
        } else {
            format!("{}h {}m", hours, mins)
        }
    } else {
        let days = minutes / 1440;
        let hours = (minutes % 1440) / 60;
        if hours == 0 {
            format!("{}d", days)
        } else {
            format!("{}d {}h", days, hours)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_token_count_small() {
        assert_eq!(format_token_count(500), "500");
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn test_format_token_count_thousands() {
        assert_eq!(format_token_count(1_000), "1.0K");
        assert_eq!(format_token_count(5_678), "5.7K");
        assert_eq!(format_token_count(999_999), "1000.0K");
    }

    #[test]
    fn test_format_token_count_millions() {
        assert_eq!(format_token_count(1_000_000), "1.0M");
        assert_eq!(format_token_count(5_678_901), "5.7M");
    }

    #[test]
    fn test_format_token_count_full() {
        assert_eq!(format_token_count_full(1000), "1,000");
        assert_eq!(format_token_count_full(1234567), "1,234,567");
        assert_eq!(format_token_count_full(999), "999");
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(10.50), "$10.50");
        assert_eq!(format_cost(0.05), "$0.050");
        assert_eq!(format_cost(0.005), "$0.0050");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(75.0), "75%");
        assert_eq!(format_percentage(5.5), "5.5%");
        assert_eq!(format_percentage(0.25), "0.25%");
    }

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration_short(30), "30m");
        assert_eq!(format_duration_short(90), "1h 30m");
        assert_eq!(format_duration_short(120), "2h");
        assert_eq!(format_duration_short(1500), "1d 1h");
    }
}
