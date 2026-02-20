//! Event handling for the TUI dashboard.

use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};

/// TUI events.
#[derive(Debug, Clone)]
pub enum Event {
    /// Terminal tick event for refresh.
    Tick,
    /// Keyboard input.
    Key(KeyEvent),
    /// Mouse input.
    Mouse(event::MouseEvent),
    /// Terminal resize.
    Resize(u16, u16),
}

/// Event handler for the TUI.
pub struct EventHandler {
    /// Tick rate in milliseconds.
    tick_rate: Duration,
}

impl EventHandler {
    /// Create a new event handler with the given tick rate.
    #[must_use]
    pub const fn new(tick_rate_ms: u64) -> Self {
        Self {
            tick_rate: Duration::from_millis(tick_rate_ms),
        }
    }

    /// Poll for the next event with timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if event polling fails.
    pub fn next(&self) -> std::io::Result<Event> {
        if event::poll(self.tick_rate)? {
            match event::read()? {
                CrosstermEvent::Key(key) => Ok(Event::Key(key)),
                CrosstermEvent::Mouse(mouse) => Ok(Event::Mouse(mouse)),
                CrosstermEvent::Resize(w, h) => Ok(Event::Resize(w, h)),
                CrosstermEvent::FocusGained
                | CrosstermEvent::FocusLost
                | CrosstermEvent::Paste(_) => Ok(Event::Tick),
            }
        } else {
            Ok(Event::Tick)
        }
    }
}

/// Key action resulting from a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Quit the application.
    Quit,
    /// Refresh data immediately.
    Refresh,
    /// Navigate up.
    Up,
    /// Navigate down.
    Down,
    /// Navigate left.
    Left,
    /// Navigate right.
    Right,
    /// Select/enter.
    Select,
    /// Toggle help.
    Help,
    /// No action.
    None,
}

impl KeyAction {
    /// Parse a key event into an action.
    #[must_use]
    pub const fn from_key_event(key: KeyEvent) -> Self {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Self::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Self::Quit,
            KeyCode::Char('r') | KeyCode::F(5) => Self::Refresh,
            KeyCode::Up | KeyCode::Char('k') => Self::Up,
            KeyCode::Down | KeyCode::Char('j') => Self::Down,
            KeyCode::Left | KeyCode::Char('h') => Self::Left,
            KeyCode::Right | KeyCode::Char('l') => Self::Right,
            KeyCode::Enter | KeyCode::Char(' ') => Self::Select,
            KeyCode::Char('?') | KeyCode::F(1) => Self::Help,
            _ => Self::None,
        }
    }
}
