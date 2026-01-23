//! TUI dashboard module using ratatui.
//!
//! Provides a real-time terminal dashboard for monitoring provider usage.

mod app;
mod dashboard;
mod event;
mod provider_panel;

pub use app::{App, AppResult};
pub use dashboard::Dashboard;
pub use event::{Event, EventHandler};

use std::io;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

use crate::cli::args::UsageArgs;
use crate::error::Result;

/// Terminal type alias for the TUI backend.
pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Initialize the terminal for TUI mode.
///
/// # Errors
///
/// Returns an error if terminal initialization fails.
pub fn init_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

/// Restore the terminal to normal mode.
///
/// # Errors
///
/// Returns an error if terminal restoration fails.
pub fn restore_terminal(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Run the TUI dashboard.
///
/// # Errors
///
/// Returns an error if the dashboard fails to run.
pub async fn run_dashboard(args: &UsageArgs, refresh_interval_secs: u64) -> Result<()> {
    let mut terminal = init_terminal().map_err(|e| crate::error::CautError::Io(e.into()))?;

    let app_result = App::new(args.clone(), refresh_interval_secs)
        .run(&mut terminal)
        .await;

    // Always try to restore terminal, even if app failed
    if let Err(e) = restore_terminal(&mut terminal) {
        eprintln!("Failed to restore terminal: {e}");
    }

    app_result
}
