//! Prediction utilities for usage trends.
//!
//! Provides velocity calculations over recent history snapshots. Velocity is
//! measured as percentage points per hour.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::core::models::RateWindow;

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
        ema = (1.0 - alpha).mul_add(ema, alpha * v);
    }

    Some(ema)
}

// =============================================================================
// Pace (single-snapshot forecast)
// =============================================================================

/// How far actual usage is from a straight-line burn of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaceStage {
    OnTrack,
    SlightlyAhead,
    Ahead,
    FarAhead,
    SlightlyBehind,
    Behind,
    FarBehind,
}

impl PaceStage {
    /// `CodexBar`'s thresholds on `actual - expected` percentage points.
    #[must_use]
    pub fn for_delta(delta: f64) -> Self {
        let magnitude = delta.abs();
        if magnitude <= 2.0 {
            Self::OnTrack
        } else if magnitude <= 6.0 {
            if delta >= 0.0 {
                Self::SlightlyAhead
            } else {
                Self::SlightlyBehind
            }
        } else if magnitude <= 12.0 {
            if delta >= 0.0 {
                Self::Ahead
            } else {
                Self::Behind
            }
        } else if delta >= 0.0 {
            Self::FarAhead
        } else {
            Self::FarBehind
        }
    }
}

/// Pace of a rate window, computed from one snapshot.
///
/// A port of `CodexBar`'s `UsagePace.weekly`: the share of the window that
/// has elapsed is the usage a steady burn would have reached; the average
/// rate so far projects when the window runs out.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePace {
    pub stage: PaceStage,
    /// `actual - expected`: positive means burning faster than the window allows.
    pub delta_percent: f64,
    pub expected_used_percent: f64,
    pub actual_used_percent: f64,
    /// Seconds until the window is spent at the average rate so far, when
    /// that happens before the reset.
    pub eta_seconds: Option<f64>,
    /// Whether the average rate so far lasts until the reset.
    pub will_last_to_reset: bool,
}

impl UsagePace {
    /// Compute the pace of `window` at `now`.
    ///
    /// `None` without a reset time or window length, after the reset, or
    /// when the reset is further away than one window (the window has not
    /// started).
    #[must_use]
    pub fn of(window: &RateWindow, now: DateTime<Utc>) -> Option<Self> {
        let resets_at = window.resets_at?;
        let minutes = window.window_minutes.filter(|m| *m > 0)?;
        let duration = f64::from(minutes) * 60.0;
        #[allow(clippy::cast_precision_loss)] // sub-second precision is irrelevant here
        let until_reset = (resets_at - now).num_milliseconds() as f64 / 1000.0;
        if until_reset <= 0.0 || until_reset > duration {
            return None;
        }
        let elapsed = (duration - until_reset).clamp(0.0, duration);
        let expected = (elapsed / duration * 100.0).clamp(0.0, 100.0);
        let actual = window.used_percent.clamp(0.0, 100.0);
        if elapsed <= 0.0 && actual > 0.0 {
            return None;
        }
        let delta = actual - expected;

        let mut eta_seconds = None;
        let mut will_last_to_reset = false;
        if actual >= 100.0 {
            eta_seconds = Some(0.0);
        } else if elapsed > 0.0 && actual > 0.0 {
            let rate = actual / elapsed;
            let candidate = (100.0 - actual) / rate;
            if candidate >= until_reset {
                will_last_to_reset = true;
            } else {
                eta_seconds = Some(candidate);
            }
        } else if elapsed > 0.0 {
            will_last_to_reset = true;
        }

        Some(Self {
            stage: PaceStage::for_delta(delta),
            delta_percent: delta,
            expected_used_percent: expected,
            actual_used_percent: actual,
            eta_seconds,
            will_last_to_reset,
        })
    }

    /// `CodexBar`'s summary: `On pace` / `N% in deficit` / `N% in reserve`,
    /// then `Runs out in …` or `Lasts until reset`.
    #[must_use]
    pub fn summary(&self) -> String {
        #[allow(clippy::cast_possible_truncation)] // a percentage
        let points = self.delta_percent.abs().round() as i64;
        let left = if points == 0 || self.stage == PaceStage::OnTrack {
            "On pace".to_string()
        } else if self.delta_percent > 0.0 {
            format!("{points}% in deficit")
        } else {
            format!("{points}% in reserve")
        };
        let right = if self.will_last_to_reset {
            Some("Lasts until reset".to_string())
        } else {
            self.eta_seconds.map(|eta| {
                if eta < 60.0 {
                    "Runs out now".to_string()
                } else {
                    format!("Runs out in {}", format_duration_secs(eta))
                }
            })
        };
        right.map_or_else(|| left.clone(), |right| format!("{left} · {right}"))
    }
}

/// Compact duration: `3d 4h`, `5h 12m`, `42m`.
#[must_use]
pub fn format_duration_secs(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation)] // bounded, non-negative
    let total_minutes = (seconds.max(0.0) / 60.0).round() as i64;
    let days = total_minutes / (24 * 60);
    let hours = (total_minutes % (24 * 60)) / 60;
    let minutes = total_minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Detect a likely usage reset between two snapshots.
#[must_use]
pub fn detect_reset(prev: &StoredSnapshot, curr: &StoredSnapshot) -> bool {
    let prev_pct = prev.primary_used_pct.unwrap_or(0.0);
    let curr_pct = curr.primary_used_pct.unwrap_or(0.0);

    prev_pct > 50.0 && curr_pct < 10.0 && (prev_pct - curr_pct) > 40.0
}

fn recent_points(history: &[StoredSnapshot], window: Duration) -> Vec<&StoredSnapshot> {
    let cutoff = Utc::now() - window;
    let mut points: Vec<&StoredSnapshot> = history
        .iter()
        .filter(|s| s.fetched_at >= cutoff && s.primary_used_pct.is_some())
        .collect();
    points.sort_by_key(|a| a.fetched_at);
    points
}

fn strip_resets<'a>(points: &'a [&'a StoredSnapshot]) -> Vec<&'a StoredSnapshot> {
    let mut segment: Vec<&StoredSnapshot> = Vec::new();
    for point in points {
        if let Some(prev) = segment.last().copied()
            && detect_reset(prev, point)
        {
            segment.clear();
        }
        segment.push(*point);
    }
    segment
}

#[allow(clippy::similar_names)]
fn linear_regression_slope(points: &[&StoredSnapshot]) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)] // point count will never exceed f64 precision
    let n = points.len() as f64;
    if n < 2.0 {
        return None;
    }

    #[allow(clippy::cast_precision_loss)] // timestamp fits within f64 precision for current era
    let base_time = points[0].fetched_at.timestamp() as f64;

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;

    for point in points {
        #[allow(clippy::cast_precision_loss)] // timestamp fits within f64 precision for current era
        let x = point.fetched_at.timestamp() as f64 - base_time;
        let y = point.primary_used_pct?;

        sum_x += x;
        sum_y += y;
        sum_xy = x.mul_add(y, sum_xy);
        sum_xx = x.mul_add(x, sum_xx);
    }

    let denominator = n.mul_add(sum_xx, -(sum_x * sum_x));
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    Some(n.mul_add(sum_xy, -(sum_x * sum_y)) / denominator)
}

fn interval_velocities(points: &[&StoredSnapshot]) -> Vec<f64> {
    let mut velocities = Vec::new();

    for window in points.windows(2) {
        let prev = window[0];
        let curr = window[1];

        if detect_reset(prev, curr) {
            continue;
        }

        let Some(prev_pct) = prev.primary_used_pct else {
            continue;
        };
        let Some(curr_pct) = curr.primary_used_pct else {
            continue;
        };

        let elapsed_secs = (curr.fetched_at - prev.fetched_at).num_seconds();
        if elapsed_secs <= 0 {
            continue;
        }

        #[allow(clippy::cast_precision_loss)] // elapsed seconds won't exceed f64 precision
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

    fn weekly_window(used: f64, hours_until_reset: i64, now: DateTime<Utc>) -> RateWindow {
        RateWindow {
            used_percent: used,
            window_minutes: Some(10_080),
            resets_at: Some(now + Duration::hours(hours_until_reset)),
            reset_description: None,
        }
    }

    #[test]
    fn pace_halfway_through_week() {
        let now = Utc::now();
        // 84h of 168h elapsed: a steady burn would be at 50%.
        let on_pace = UsagePace::of(&weekly_window(49.0, 84, now), now).unwrap();
        assert_eq!(on_pace.stage, PaceStage::OnTrack);
        assert_float_eq!(on_pace.expected_used_percent, 50.0, 0.01);
        assert_eq!(on_pace.summary(), "On pace · Lasts until reset");

        let ahead = UsagePace::of(&weekly_window(70.0, 84, now), now).unwrap();
        assert_eq!(ahead.stage, PaceStage::FarAhead);
        // 70% in 84h => 30% more takes 36h, before the 84h reset.
        assert_float_eq!(ahead.eta_seconds.unwrap(), 36.0 * 3600.0, 1.0);
        assert!(!ahead.will_last_to_reset);
        assert_eq!(ahead.summary(), "20% in deficit · Runs out in 1d 12h");

        let behind = UsagePace::of(&weekly_window(45.0, 84, now), now).unwrap();
        assert_eq!(behind.stage, PaceStage::SlightlyBehind);
        assert!(behind.summary().starts_with("5% in reserve"));
    }

    #[test]
    fn pace_edge_cases() {
        let now = Utc::now();
        let spent = UsagePace::of(&weekly_window(100.0, 10, now), now).unwrap();
        assert_eq!(spent.eta_seconds, Some(0.0));
        assert!(spent.summary().ends_with("Runs out now"));

        let idle = UsagePace::of(&weekly_window(0.0, 100, now), now).unwrap();
        assert!(idle.will_last_to_reset);

        // No reset time, no window length, past reset, or not yet started.
        let mut window = weekly_window(10.0, 10, now);
        window.resets_at = None;
        assert!(UsagePace::of(&window, now).is_none());
        let mut window = weekly_window(10.0, 10, now);
        window.window_minutes = None;
        assert!(UsagePace::of(&window, now).is_none());
        assert!(UsagePace::of(&weekly_window(10.0, -1, now), now).is_none());
        assert!(UsagePace::of(&weekly_window(10.0, 500, now), now).is_none());
    }

    #[test]
    fn pace_stage_thresholds() {
        assert_eq!(PaceStage::for_delta(2.0), PaceStage::OnTrack);
        assert_eq!(PaceStage::for_delta(-2.0), PaceStage::OnTrack);
        assert_eq!(PaceStage::for_delta(5.0), PaceStage::SlightlyAhead);
        assert_eq!(PaceStage::for_delta(-10.0), PaceStage::Behind);
        assert_eq!(PaceStage::for_delta(13.0), PaceStage::FarAhead);
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration_secs(42.0 * 60.0), "42m");
        assert_eq!(format_duration_secs(18_720.0), "5h 12m");
        assert_eq!(format_duration_secs(273_600.0), "3d 4h");
        assert_eq!(format_duration_secs(-5.0), "0m");
    }
}
