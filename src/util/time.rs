//! Time formatting utilities.

use chrono::{DateTime, Utc};

/// Format a countdown to a future time.
#[must_use]
pub fn format_countdown(target: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = target.signed_duration_since(now);

    if duration.num_seconds() <= 0 {
        return "now".to_string();
    }

    let hours = duration.num_hours();
    let minutes = duration.num_minutes() % 60;

    if hours > 24 {
        let days = hours / 24;
        format!("in {days} day{}", if days == 1 { "" } else { "s" })
    } else if hours > 0 {
        format!("in {hours}h {minutes}m")
    } else if minutes > 0 {
        format!("in {minutes}m")
    } else {
        let seconds = duration.num_seconds();
        format!("in {seconds}s")
    }
}

/// Format a relative time (past or future).
#[must_use]
pub fn format_relative_time(target: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(target);

    if duration.num_seconds().abs() < 60 {
        return "just now".to_string();
    }

    let minutes = duration.num_minutes().abs();
    let hours = duration.num_hours().abs();
    let days = duration.num_days().abs();

    let suffix = if duration.num_seconds() > 0 {
        "ago"
    } else {
        "from now"
    };

    if days > 0 {
        format!("{days} day{} {suffix}", if days == 1 { "" } else { "s" })
    } else if hours > 0 {
        format!("{hours} hour{} {suffix}", if hours == 1 { "" } else { "s" })
    } else {
        format!(
            "{minutes} minute{} {suffix}",
            if minutes == 1 { "" } else { "s" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn countdown_hours() {
        let target = Utc::now() + Duration::hours(3) + Duration::minutes(30);
        let result = format_countdown(target);
        assert!(result.contains("3h"));
    }

    #[test]
    fn countdown_days() {
        // Use 3 days to avoid edge case at exactly 48 hours
        let target = Utc::now() + Duration::days(3);
        let result = format_countdown(target);
        assert!(
            result.contains("day"),
            "Expected 'day' in result, got: {result}"
        );
        assert!(
            result.starts_with("in "),
            "Expected countdown to start with 'in ', got: {result}"
        );
    }
}
