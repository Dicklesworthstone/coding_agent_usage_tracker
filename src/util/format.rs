//! Number formatting utilities.

/// Format a percentage with color threshold hints.
#[must_use]
pub fn format_percent(value: f64) -> String {
    format!("{value:.0}%")
}

/// Format a cost in USD.
#[must_use]
pub fn format_cost(value: f64) -> String {
    format!("${value:.2}")
}

/// Format a token count with thousands separators.
#[must_use]
pub fn format_tokens(value: i64) -> String {
    fn format_compact(value: i64, divisor: i64, suffix: &str) -> String {
        let sign = if value < 0 { "-" } else { "" };
        let abs = value.abs();
        let major = abs / divisor;
        let minor = (abs % divisor) / (divisor / 10);
        format!("{sign}{major}.{minor}{suffix}")
    }

    if value.abs() >= 1_000_000 {
        format_compact(value, 1_000_000, "M")
    } else if value.abs() >= 1_000 {
        format_compact(value, 1_000, "K")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tokens_millions() {
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn format_tokens_thousands() {
        assert_eq!(format_tokens(12_500), "12.5K");
    }

    #[test]
    fn format_tokens_small() {
        assert_eq!(format_tokens(500), "500");
    }
}
