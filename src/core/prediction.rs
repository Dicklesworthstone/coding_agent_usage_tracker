//! Prediction utilities for usage trends.
//!
//! Provides velocity calculations over recent history snapshots. Velocity is
//! measured as percentage points per hour.

use chrono::{Duration, Utc};

use crate::storage::StoredSnapshot;

/// Calculate usage velocity over a time window.
///
/// Returns percent-per-hour (can be negative). Returns None when there is
/// insufficient data or the window is invalid.
#[must_use]
pub fn calculate_velocity(history: &[StoredSnapshot], window: Duration) -> Option<f64> {
    if history.len() < 2 || window <= Duration::zero() {
        return None;
    }

    let recent = recent_points(history, window);
    if recent.len() < 2 {
        return None;
    }

    let segment = strip_resets(&recent);
    if segment.len() < 2 {
        return None;
    }

    let slope_per_second = linear_regression_slope(&segment)?;
    Some(slope_per_second * 3600.0)
}

/// Compute a smoothed velocity using an exponential moving average.
///
/// `alpha` is the smoothing factor (0.0 < alpha <= 1.0).
#[must_use]
pub fn smoothed_velocity(history: &[StoredSnapshot], window: Duration, alpha: f64) -> Option<f64> {
    if !(0.0 < alpha && alpha <= 1.0) {
        return None;
    }

    let recent = recent_points(history, window);
    if recent.len() < 2 {
        return None;
    }

    let segment = strip_resets(&recent);
    if segment.len() < 2 {
        return None;
    }

    let velocities = interval_velocities(&segment);
    if velocities.is_empty() {
        return None;
    }

    let mut ema = velocities[0];
    for v in &velocities[1..] {
        ema = alpha * v + (1.0 - alpha) * ema;
    }

    Some(ema)
}

/// Detect a likely usage reset between two snapshots.
#[must_use]
pub fn detect_reset(prev: &StoredSnapshot, curr: &StoredSnapshot) -> bool {
    let prev_pct = prev.primary_used_pct.unwrap_or(0.0);
    let curr_pct = curr.primary_used_pct.unwrap_or(0.0);

    prev_pct > 50.0 && curr_pct < 10.0 && (prev_pct - curr_pct) > 40.0
}

fn recent_points<'a>(history: &'a [StoredSnapshot], window: Duration) -> Vec<&'a StoredSnapshot> {
    let cutoff = Utc::now() - window;
    let mut points: Vec<&StoredSnapshot> = history
        .iter()
        .filter(|s| s.fetched_at >= cutoff && s.primary_used_pct.is_some())
        .collect();
    points.sort_by(|a, b| a.fetched_at.cmp(&b.fetched_at));
    points
}

fn strip_resets<'a>(points: &'a [&'a StoredSnapshot]) -> Vec<&'a StoredSnapshot> {
    let mut segment: Vec<&StoredSnapshot> = Vec::new();
    for point in points {
        if let Some(prev) = segment.last().copied() {
            if detect_reset(prev, point) {
                segment.clear();
            }
        }
        segment.push(*point);
    }
    segment
}

fn linear_regression_slope(points: &[&StoredSnapshot]) -> Option<f64> {
    let n = points.len() as f64;
    if n < 2.0 {
        return None;
    }

    let base_time = points[0].fetched_at.timestamp() as f64;

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;

    for point in points {
        let x = point.fetched_at.timestamp() as f64 - base_time;
        let y = point.primary_used_pct?;

        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_xx += x * x;
    }

    let denominator = n * sum_xx - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    Some((n * sum_xy - sum_x * sum_y) / denominator)
}

fn interval_velocities(points: &[&StoredSnapshot]) -> Vec<f64> {
    let mut velocities = Vec::new();

    for window in points.windows(2) {
        let prev = window[0];
        let curr = window[1];

        if detect_reset(prev, curr) {
            continue;
        }

        let prev_pct = match prev.primary_used_pct {
            Some(v) => v,
            None => continue,
        };
        let curr_pct = match curr.primary_used_pct {
            Some(v) => v,
            None => continue,
        };

        let elapsed_secs = (curr.fetched_at - prev.fetched_at).num_seconds();
        if elapsed_secs <= 0 {
            continue;
        }

        let per_second = (curr_pct - prev_pct) / elapsed_secs as f64;
        velocities.push(per_second * 3600.0);
    }

    velocities
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::assert_float_eq;
    use crate::core::provider::Provider;
    use crate::storage::StoredSnapshot;

    fn make_snapshot_at(ts: chrono::DateTime<Utc>, pct: f64) -> StoredSnapshot {
        StoredSnapshot {
            id: 0,
            provider: Provider::Claude,
            fetched_at: ts,
            source: "test".to_string(),
            primary_used_pct: Some(pct),
            primary_window_minutes: None,
            primary_resets_at: None,
            secondary_used_pct: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            tertiary_used_pct: None,
            tertiary_window_minutes: None,
            tertiary_resets_at: None,
            cost_today_usd: None,
            cost_mtd_usd: None,
            credits_remaining: None,
            account_email: None,
            account_org: None,
            fetch_duration_ms: None,
            created_at: None,
        }
    }

    #[test]
    fn calculate_velocity_requires_two_points() {
        let now = Utc::now();
        let history = vec![make_snapshot_at(now, 50.0)];
        assert!(calculate_velocity(&history, Duration::hours(2)).is_none());
    }

    #[test]
    fn calculate_velocity_two_points() {
        let now = Utc::now();
        let history = vec![
            make_snapshot_at(now - Duration::hours(2), 45.0),
            make_snapshot_at(now, 65.0),
        ];
        let velocity = calculate_velocity(&history, Duration::hours(4)).unwrap();
        assert_float_eq!(velocity, 10.0, 0.01);
    }

    #[test]
    fn calculate_velocity_linear_regression() {
        let now = Utc::now();
        let history = vec![
            make_snapshot_at(now - Duration::hours(3), 10.0),
            make_snapshot_at(now - Duration::hours(2), 15.0),
            make_snapshot_at(now - Duration::hours(1), 20.0),
            make_snapshot_at(now, 25.0),
        ];
        let velocity = calculate_velocity(&history, Duration::hours(6)).unwrap();
        assert_float_eq!(velocity, 5.0, 0.01);
    }

    #[test]
    fn calculate_velocity_ignores_resets() {
        let now = Utc::now();
        let history = vec![
            make_snapshot_at(now - Duration::hours(3), 80.0),
            make_snapshot_at(now - Duration::hours(2), 5.0),
            make_snapshot_at(now - Duration::hours(1), 15.0),
        ];
        let velocity = calculate_velocity(&history, Duration::hours(6)).unwrap();
        assert_float_eq!(velocity, 10.0, 0.01);
    }

    #[test]
    fn detect_reset_thresholds() {
        let now = Utc::now();
        let prev = make_snapshot_at(now - Duration::minutes(30), 70.0);
        let curr = make_snapshot_at(now, 5.0);
        assert!(detect_reset(&prev, &curr));
    }

    #[test]
    fn smoothed_velocity_returns_none_for_invalid_alpha() {
        let now = Utc::now();
        let history = vec![
            make_snapshot_at(now - Duration::hours(1), 10.0),
            make_snapshot_at(now, 20.0),
        ];
        assert!(smoothed_velocity(&history, Duration::hours(2), 0.0).is_none());
        assert!(smoothed_velocity(&history, Duration::hours(2), 1.5).is_none());
    }

    #[test]
    fn smoothed_velocity_ema() {
        let now = Utc::now();
        let history = vec![
            make_snapshot_at(now - Duration::hours(2), 10.0),
            make_snapshot_at(now - Duration::hours(1), 30.0),
            make_snapshot_at(now, 40.0),
        ];

        let velocity = smoothed_velocity(&history, Duration::hours(4), 0.5).unwrap();
        // Interval velocities: 20, 10 (pct/hour), EMA with alpha=0.5 => 15
        assert_float_eq!(velocity, 15.0, 0.01);
    }
}
