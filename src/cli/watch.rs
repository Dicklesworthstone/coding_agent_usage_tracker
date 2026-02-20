//! Watch mode support for the usage command.
//!
//! Provides the core loop and state management for continuous updates.

use chrono::{DateTime, Utc};
use tokio::time::{Duration, interval};

use crate::cli::args::{OutputFormat, UsageArgs};
use crate::cli::usage::{UsageResults, fetch_usage, render_usage_results};
use crate::error::{CautError, Result};

/// State tracking across watch iterations.
#[derive(Debug, Default)]
pub struct WatchState {
    pub last_results: Option<Vec<crate::core::models::ProviderPayload>>,
    pub last_errors: Vec<String>,
    pub last_fetch_at: Option<DateTime<Utc>>,
    pub fetch_count: u64,
    pub error_count: u64,
    pub last_error: Option<CautError>,
}

impl WatchState {
    /// Create a new watch state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update state with the latest fetch results.
    pub(crate) fn update(&mut self, results: Result<UsageResults>) {
        self.fetch_count += 1;
        match results {
            Ok(results) => {
                self.last_results = Some(results.payloads);
                self.last_errors = results.errors;
                self.last_fetch_at = Some(Utc::now());
                if self.last_errors.is_empty() {
                    self.last_error = None;
                } else {
                    self.error_count += 1;
                    self.last_error = Some(CautError::PartialFailure {
                        failed: self.last_errors.len(),
                    });
                }
            }
            Err(e) => {
                self.error_count += 1;
                self.last_error = Some(e);
                // Preserve last_results/last_errors for stale display.
            }
        }
    }
}

/// Run watch mode for the usage command.
///
/// # Errors
/// Returns an error if the initial fetch fails or if rendering encounters
/// an I/O or serialization error.
pub async fn run_watch(
    args: &UsageArgs,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
    interval_duration: Duration,
) -> Result<()> {
    let mut state = WatchState::new();
    let mut ticker = interval(interval_duration);

    // Ctrl+C handler for clean shutdown.
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(());
    });

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let results = fetch_usage(args).await;
                state.update(results);
                render_watch_frame(&state, format, pretty, no_color)?;
            }
            _ = &mut shutdown_rx => {
                render_final_snapshot(&state, format, pretty, no_color)?;
                break;
            }
        }
    }

    Ok(())
}

fn render_watch_frame(
    state: &WatchState,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    if let Some(payloads) = &state.last_results {
        let results = UsageResults {
            payloads: payloads.clone(),
            errors: state.last_errors.clone(),
        };
        render_usage_results(&results, format, pretty, no_color)?;
    }

    if let Some(err) = &state.last_error {
        eprintln!("Error: {err}");
    }

    Ok(())
}

fn render_final_snapshot(
    state: &WatchState,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    render_watch_frame(state, format, pretty, no_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_test_provider_payload;

    #[test]
    fn watch_state_updates_on_success() {
        let mut state = WatchState::new();
        let results = UsageResults {
            payloads: vec![make_test_provider_payload("codex", "cli")],
            errors: vec!["codex: warning".to_string()],
        };

        state.update(Ok(results));

        assert_eq!(state.fetch_count, 1);
        assert_eq!(state.error_count, 1);
        assert!(matches!(
            state.last_error,
            Some(CautError::PartialFailure { failed: 1 })
        ));
        assert!(state.last_fetch_at.is_some());
        assert_eq!(state.last_results.as_ref().unwrap().len(), 1);
        assert_eq!(state.last_errors.len(), 1);
    }

    #[test]
    fn watch_state_preserves_results_on_error() {
        let mut state = WatchState::new();
        let results = UsageResults {
            payloads: vec![make_test_provider_payload("codex", "cli")],
            errors: Vec::new(),
        };

        state.update(Ok(results));
        let before_len = state.last_results.as_ref().map_or(0, std::vec::Vec::len);

        state.update(Err(CautError::Config("boom".to_string())));

        assert_eq!(state.fetch_count, 2);
        assert_eq!(state.error_count, 1);
        assert!(state.last_error.is_some());
        assert_eq!(
            state.last_results.as_ref().map(std::vec::Vec::len),
            Some(before_len)
        );
    }
}
