//! Rich output module - wraps rich_rust for caut-specific use.

use crate::cli::args::OutputFormat;
use crate::util::env as env_util;

pub use rich_rust::prelude::*;

const THEME_ENV: &str = "CAUT_THEME";

/// Minimal theme descriptor for rich output decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    name: String,
}

impl Theme {
    /// Theme name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Default theme for rich output.
#[must_use]
pub fn default_theme() -> Theme {
    Theme {
        name: "default".to_string(),
    }
}

/// Load theme selection from environment or defaults.
#[must_use]
pub fn get_theme() -> Theme {
    match std::env::var(THEME_ENV) {
        Ok(value) if !value.trim().is_empty() => {
            let theme = Theme { name: value };
            tracing::debug!(
                source = "env_var",
                env = THEME_ENV,
                theme = %theme.name(),
                "Loaded rich theme"
            );
            theme
        }
        _ => {
            let theme = default_theme();
            tracing::debug!(
                source = "default",
                theme = %theme.name(),
                "Loaded rich theme"
            );
            theme
        }
    }
}

/// Decide whether rich output should be used and log the reason.
#[must_use]
pub fn should_use_rich_output(format: OutputFormat, no_color_flag: bool) -> bool {
    if format != OutputFormat::Human {
        tracing::debug!(reason = "robot_mode", "Rich output disabled");
        return false;
    }

    if no_color_flag {
        tracing::debug!(reason = "no_color_flag", "Rich output disabled");
        return false;
    }

    if std::env::var("NO_COLOR").is_ok() {
        tracing::debug!(reason = "no_color_env", "Rich output disabled");
        return false;
    }

    if !env_util::stdout_is_tty() {
        tracing::debug!(reason = "stdout_not_tty", "Rich output disabled");
        return false;
    }

    tracing::debug!(reason = "enabled", "Rich output enabled");
    true
}

/// Collect rich output diagnostics for debugging.
#[must_use]
pub fn collect_rich_diagnostics(format: OutputFormat, no_color_flag: bool) -> String {
    let stdout_tty = env_util::stdout_is_tty();
    let stderr_tty = env_util::stderr_is_tty();

    let no_color_env = std::env::var("NO_COLOR").unwrap_or_else(|_| "<unset>".to_string());
    let term_env = std::env::var("TERM").unwrap_or_else(|_| "<unset>".to_string());

    let log_level = std::env::var("CAUT_LOG").unwrap_or_else(|_| "<unset>".to_string());
    let log_format = std::env::var("CAUT_LOG_FORMAT").unwrap_or_else(|_| "<unset>".to_string());
    let log_file = std::env::var("CAUT_LOG_FILE").unwrap_or_else(|_| "<unset>".to_string());

    let theme = std::env::var(THEME_ENV).unwrap_or_else(|_| "<unset>".to_string());
    let rich_enabled = should_use_rich_output(format, no_color_flag);

    let mut lines = Vec::new();
    lines.push(format!("stdout is TTY: {}", stdout_tty));
    lines.push(format!("stderr is TTY: {}", stderr_tty));
    lines.push(format!("NO_COLOR: {}", no_color_env));
    lines.push(format!("TERM: {}", term_env));
    lines.push(format!("CAUT_LOG: {}", log_level));
    lines.push(format!("CAUT_LOG_FORMAT: {}", log_format));
    lines.push(format!("CAUT_LOG_FILE: {}", log_file));
    lines.push(format!("CAUT_THEME: {}", theme));
    lines.push(format!("output format: {:?}", format));
    lines.push(format!("no_color flag: {}", no_color_flag));
    lines.push(format!("rich output enabled: {}", rich_enabled));

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_test_provider_payload;
    use tracing_test::traced_test;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[allow(unsafe_code)]
    fn with_env_var(key: &str, value: &str, f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prior = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        f();
        match prior {
            Some(val) => unsafe {
                std::env::set_var(key, val);
            },
            None => unsafe {
                std::env::remove_var(key);
            },
        }
    }

    #[test]
    fn test_rich_rust_imports_work() {
        let console = Console::new();
        assert!(console.width() > 0);
    }

    #[test]
    fn test_style_creation() {
        let style = Style::new().bold().color(Color::parse("red").unwrap());
        assert!(!style.is_null());
    }

    #[test]
    fn test_table_creation() {
        let mut table = Table::new();
        table.add_row_cells(["test", "value"]);
    }

    #[test]
    fn test_panel_creation() {
        let _panel = Panel::from_text("test content");
    }

    #[test]
    fn test_color_parsing() {
        let red = Color::parse("red");
        assert!(red.is_ok());

        let hex = Color::parse("#ff0000");
        assert!(hex.is_ok());
    }

    #[traced_test]
    #[test]
    fn test_robot_mode_logs_reason() {
        let result = should_use_rich_output(OutputFormat::Json, false);
        assert!(!result);
        assert!(logs_contain("robot_mode"));
        assert!(logs_contain("Rich output disabled"));
    }

    #[traced_test]
    #[test]
    fn test_no_color_env_logs_reason() {
        with_env_var("NO_COLOR", "1", || {
            let result = should_use_rich_output(OutputFormat::Human, false);
            assert!(!result);
            assert!(logs_contain("no_color_env") || logs_contain("NO_COLOR"));
        });
    }

    #[traced_test]
    #[test]
    fn test_theme_loading_logs_source() {
        with_env_var(THEME_ENV, "minimal", || {
            let theme = get_theme();
            assert_eq!(theme.name(), "minimal");
            assert!(logs_contain("env_var") || logs_contain(THEME_ENV));
            assert!(logs_contain("minimal"));
        });
    }

    #[traced_test]
    #[test]
    fn test_component_render_logs_timing() {
        let payload = make_test_provider_payload("codex", "cli");
        let _ = crate::render::human::render_usage(&[payload], true).unwrap();
        assert!(logs_contain("render_time_ms") || logs_contain("component"));
    }

    #[test]
    fn test_debug_diagnostics_output() {
        let output = collect_rich_diagnostics(OutputFormat::Human, false);
        assert!(output.contains("stdout is TTY"));
        assert!(output.contains("NO_COLOR"));
        assert!(output.contains("TERM"));
    }
}
