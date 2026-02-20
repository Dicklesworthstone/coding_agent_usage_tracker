//! Environment detection utilities.

use std::io::IsTerminal;

/// Check if stdout is a TTY.
#[must_use]
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Check if stderr is a TTY.
#[must_use]
pub fn stderr_is_tty() -> bool {
    std::io::stderr().is_terminal()
}

/// Check if color should be enabled.
#[must_use]
pub fn should_use_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return false;
    }

    // Check NO_COLOR environment variable
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }

    // Check TERM=dumb
    if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return false;
    }

    // Only use color if output is a TTY
    stdout_is_tty()
}

/// Check if running on macOS.
#[must_use]
pub const fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

/// Check if running on Linux.
#[must_use]
pub const fn is_linux() -> bool {
    cfg!(target_os = "linux")
}
