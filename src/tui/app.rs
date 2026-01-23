//! Application state and main event loop for the TUI dashboard.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

use crate::cli::args::UsageArgs;
use crate::cli::usage::{UsageResults, fetch_usage};
use crate::core::models::ProviderPayload;
use crate::error::{CautError, Result};

use super::dashboard::Dashboard;
use super::event::{Event, EventHandler, KeyAction};
use super::Tui;

/// Result type for the app.
pub type AppResult<T> = std::result::Result<T, CautError>;

/// Application state for the TUI dashboard.
pub struct App {
    /// Usage command arguments.
    args: UsageArgs,
    /// Current provider payloads.
    payloads: Vec<ProviderPayload>,
    /// Current error messages.
    errors: Vec<String>,
    /// Currently selected panel index.
    selected: usize,
    /// Last data update timestamp.
    last_update: Option<DateTime<Utc>>,
    /// Refresh interval in seconds.
    refresh_interval: Duration,
    /// Last refresh time.
    last_refresh: Instant,
    /// Whether to show help overlay.
    show_help: bool,
    /// Whether the app should quit.
    should_quit: bool,
    /// Whether a refresh is pending.
    refresh_pending: bool,
}

impl App {
    /// Create a new application instance.
    #[must_use]
    pub fn new(args: UsageArgs, refresh_interval_secs: u64) -> Self {
        Self {
            args,
            payloads: Vec::new(),
            errors: Vec::new(),
            selected: 0,
            last_update: None,
            refresh_interval: Duration::from_secs(refresh_interval_secs),
            last_refresh: Instant::now() - Duration::from_secs(refresh_interval_secs + 1),
            show_help: false,
            should_quit: false,
            refresh_pending: true, // Start with a refresh
        }
    }

    /// Run the application event loop.
    ///
    /// # Errors
    ///
    /// Returns an error if rendering or event handling fails.
    pub async fn run(mut self, terminal: &mut Tui) -> Result<()> {
        let event_handler = EventHandler::new(100); // 100ms tick rate
        let (tx, mut rx) = mpsc::channel::<UsageResults>(1);

        // Initial fetch
        self.spawn_fetch(tx.clone());

        while !self.should_quit {
            // Render the current state
            terminal
                .draw(|frame| {
                    let dashboard = Dashboard::new(
                        &self.payloads,
                        &self.errors,
                        self.selected,
                        self.last_update,
                        self.show_help,
                    );
                    frame.render_widget(dashboard, frame.area());
                })
                .map_err(|e| CautError::Io(e.into()))?;

            // Handle events
            match event_handler.next() {
                Ok(Event::Key(key)) => {
                    let action = KeyAction::from_key_event(key);
                    self.handle_action(action, tx.clone());
                }
                Ok(Event::Tick) => {
                    // Check for fetch results
                    if let Ok(results) = rx.try_recv() {
                        self.update_from_results(results);
                    }

                    // Check if we need to refresh
                    if self.last_refresh.elapsed() >= self.refresh_interval {
                        self.spawn_fetch(tx.clone());
                    }
                }
                Ok(Event::Resize(_, _)) => {
                    // Terminal will be redrawn on next iteration
                }
                Ok(Event::Mouse(_)) => {
                    // Ignore mouse events for now
                }
                Err(e) => {
                    // Log but don't crash on event errors
                    tracing::warn!("Event error: {e}");
                }
            }
        }

        Ok(())
    }

    /// Handle a key action.
    fn handle_action(&mut self, action: KeyAction, tx: mpsc::Sender<UsageResults>) {
        // If help is shown, any key dismisses it
        if self.show_help && action != KeyAction::None {
            self.show_help = false;
            return;
        }

        match action {
            KeyAction::Quit => {
                self.should_quit = true;
            }
            KeyAction::Refresh => {
                self.spawn_fetch(tx);
            }
            KeyAction::Up => {
                self.move_selection_vertical(-1);
            }
            KeyAction::Down => {
                self.move_selection_vertical(1);
            }
            KeyAction::Left => {
                self.move_selection_horizontal(-1);
            }
            KeyAction::Right => {
                self.move_selection_horizontal(1);
            }
            KeyAction::Help => {
                self.show_help = !self.show_help;
            }
            KeyAction::Select | KeyAction::None => {}
        }
    }

    /// Move selection vertically (assumes 2-column grid by default).
    fn move_selection_vertical(&mut self, delta: i32) {
        if self.payloads.is_empty() {
            return;
        }

        // Assume 2 columns for vertical navigation
        let cols = 2;
        let current = self.selected as i32;
        let new_selection = current + (delta * cols);

        if new_selection >= 0 && new_selection < self.payloads.len() as i32 {
            self.selected = new_selection as usize;
        }
    }

    /// Move selection horizontally.
    fn move_selection_horizontal(&mut self, delta: i32) {
        if self.payloads.is_empty() {
            return;
        }

        let current = self.selected as i32;
        let new_selection = current + delta;

        if new_selection >= 0 && new_selection < self.payloads.len() as i32 {
            self.selected = new_selection as usize;
        }
    }

    /// Spawn a background fetch task.
    fn spawn_fetch(&mut self, tx: mpsc::Sender<UsageResults>) {
        if self.refresh_pending {
            return; // Already fetching
        }

        self.refresh_pending = true;
        self.last_refresh = Instant::now();

        let args = self.args.clone();
        tokio::spawn(async move {
            let results = fetch_usage(&args).await;
            match results {
                Ok(results) => {
                    let _ = tx.send(results).await;
                }
                Err(e) => {
                    // Send empty results with error
                    let _ = tx
                        .send(UsageResults {
                            payloads: Vec::new(),
                            errors: vec![e.to_string()],
                        })
                        .await;
                }
            }
        });
    }

    /// Update state from fetch results.
    fn update_from_results(&mut self, results: UsageResults) {
        self.refresh_pending = false;

        // Only update payloads if we got some (preserve stale data on error)
        if !results.payloads.is_empty() {
            self.payloads = results.payloads;
            self.last_update = Some(Utc::now());

            // Ensure selected is in bounds
            if self.selected >= self.payloads.len() && !self.payloads.is_empty() {
                self.selected = self.payloads.len() - 1;
            }
        }

        self.errors = results.errors;
    }
}
