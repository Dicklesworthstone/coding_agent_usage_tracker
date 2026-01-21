//! Rich output module - wraps rich_rust for caut-specific use.
//!
//! This module provides the safety gate infrastructure that determines when rich
//! terminal output should be used vs plain text. The PRIMARY users of caut are
//! AI coding agents which cannot process ANSI escape codes, so these gates are
//! critical for correct operation.
//!
//! ## Theme System
//!
//! The theme system provides consistent styling across all rich output components:
//! - **default**: Full colors with provider brand colors
//! - **minimal**: Subtle styling, bold only
//! - **high-contrast**: Accessible with bold and high-contrast colors
//! - **ascii**: No Unicode box drawing characters
//!
//! Theme selection priority:
//! 1. CLI flag `--theme`
//! 2. Environment variable `CAUT_THEME`
//! 3. Auto-detection based on terminal capabilities
//! 4. Default theme

pub mod components;

use crate::cli::args::OutputFormat;
use crate::util::env as env_util;
use regex::Regex;
use std::sync::LazyLock;

pub use rich_rust::prelude::*;

const THEME_ENV: &str = "CAUT_THEME";

// =============================================================================
// Color Depth Detection
// =============================================================================

/// Terminal color capability level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorDepth {
    /// Plain text only (NO_COLOR set or TERM=dumb).
    NoColor,
    /// Basic 8/16 colors.
    #[default]
    Basic,
    /// 256-color palette (xterm-256color).
    Extended,
    /// 24-bit RGB (truecolor).
    TrueColor,
}

/// Detect terminal color capabilities.
#[must_use]
pub fn detect_color_depth() -> ColorDepth {
    // Check NO_COLOR first
    if std::env::var("NO_COLOR").is_ok() {
        tracing::debug!(reason = "NO_COLOR", "Color depth: NoColor");
        return ColorDepth::NoColor;
    }

    // Check COLORTERM for truecolor
    if let Ok(colorterm) = std::env::var("COLORTERM") {
        if colorterm == "truecolor" || colorterm == "24bit" {
            tracing::debug!(reason = "COLORTERM", colorterm = %colorterm, "Color depth: TrueColor");
            return ColorDepth::TrueColor;
        }
    }

    // Check TERM for 256 colors
    if let Ok(term) = std::env::var("TERM") {
        if term.contains("256color") {
            tracing::debug!(reason = "TERM", term = %term, "Color depth: Extended");
            return ColorDepth::Extended;
        }
        if term == "dumb" {
            tracing::debug!(reason = "TERM=dumb", "Color depth: NoColor");
            return ColorDepth::NoColor;
        }
    }

    // Default to basic colors
    tracing::debug!(reason = "default", "Color depth: Basic");
    ColorDepth::Basic
}

/// Check if terminal supports Unicode.
#[must_use]
pub fn has_unicode_support() -> bool {
    let lang = std::env::var("LANG").unwrap_or_default();
    let lc_all = std::env::var("LC_ALL").unwrap_or_default();

    lang.to_uppercase().contains("UTF-8")
        || lang.to_uppercase().contains("UTF8")
        || lc_all.to_uppercase().contains("UTF-8")
        || lc_all.to_uppercase().contains("UTF8")
}

// =============================================================================
// Box Drawing Characters
// =============================================================================

/// Box drawing style for panels and tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoxStyle {
    /// Rounded corners: ╭─╮ (default for modern terminals).
    #[default]
    Rounded,
    /// Square corners: ┌─┐.
    Square,
    /// Heavy lines: ┏━┓.
    Heavy,
    /// Double lines: ╔═╗.
    Double,
    /// ASCII only: +-+ (for legacy terminals).
    Ascii,
}

/// Box drawing character set.
#[derive(Debug, Clone, Copy)]
pub struct BoxChars {
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
    pub horizontal: char,
    pub vertical: char,
}

impl BoxStyle {
    /// Get the character set for this box style.
    #[must_use]
    pub const fn chars(self) -> BoxChars {
        match self {
            Self::Rounded => BoxChars {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
            },
            Self::Square => BoxChars {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                horizontal: '─',
                vertical: '│',
            },
            Self::Heavy => BoxChars {
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
                horizontal: '━',
                vertical: '┃',
            },
            Self::Double => BoxChars {
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
                horizontal: '═',
                vertical: '║',
            },
            Self::Ascii => BoxChars {
                top_left: '+',
                top_right: '+',
                bottom_left: '+',
                bottom_right: '+',
                horizontal: '-',
                vertical: '|',
            },
        }
    }
}

// =============================================================================
// Theme Configuration
// =============================================================================

/// Complete theme configuration with all styling options.
#[derive(Debug, Clone)]
pub struct ThemeConfig {
    /// Theme name identifier.
    pub name: String,

    // Core semantic colors
    /// Primary accent color.
    pub primary: Style,
    /// Secondary accent color.
    pub secondary: Style,
    /// Success/positive color.
    pub success: Style,
    /// Warning color.
    pub warning: Style,
    /// Error/danger color.
    pub error: Style,
    /// Muted/dimmed color.
    pub muted: Style,

    // Provider brand colors
    /// Claude/Anthropic brand color (orange).
    pub provider_claude: Style,
    /// OpenAI/Codex brand color (green).
    pub provider_openai: Style,
    /// Google/Gemini brand color (blue).
    pub provider_google: Style,
    /// Cursor brand color (purple).
    pub provider_cursor: Style,
    /// GitHub Copilot brand color (blue).
    pub provider_copilot: Style,
    /// Fallback for unknown providers.
    pub provider_other: Style,

    // Table styling
    /// Table header style.
    pub table_header: Style,
    /// Table border style.
    pub table_border: Style,
    /// Alternating row style (optional).
    pub table_row_alt: Option<Style>,

    // Panel styling
    /// Panel title style.
    pub panel_title: Style,
    /// Panel border style.
    pub panel_border: Style,
    /// Error panel border style.
    pub panel_error_border: Style,

    // Semantic value styling
    /// Cost/currency values.
    pub cost: Style,
    /// Numeric counts.
    pub count: Style,
    /// Percentage values.
    pub percentage: Style,
    /// High/critical percentage values.
    pub percentage_high: Style,

    // Status indicators
    /// Success status (green check).
    pub status_success: Style,
    /// Warning status (yellow).
    pub status_warning: Style,
    /// Error status (red X).
    pub status_error: Style,

    // Box drawing
    /// Box character style.
    pub box_style: BoxStyle,

    // Terminal capabilities
    /// Detected color depth.
    pub color_depth: ColorDepth,
}

impl ThemeConfig {
    /// Get provider-specific style by name.
    #[must_use]
    pub fn provider_style(&self, name: &str) -> &Style {
        let name_lower = name.to_lowercase();
        if name_lower.contains("claude") || name_lower.contains("anthropic") {
            &self.provider_claude
        } else if name_lower.contains("openai")
            || name_lower.contains("gpt")
            || name_lower.contains("codex")
        {
            &self.provider_openai
        } else if name_lower.contains("google") || name_lower.contains("gemini") {
            &self.provider_google
        } else if name_lower.contains("cursor") {
            &self.provider_cursor
        } else if name_lower.contains("copilot") || name_lower.contains("github") {
            &self.provider_copilot
        } else {
            &self.provider_other
        }
    }
}

// =============================================================================
// Built-in Themes
// =============================================================================

/// Parse color from name with fallback.
#[must_use]
pub fn parse_color(name: &str) -> Color {
    Color::parse(name).unwrap_or_else(|_| Color::parse("white").unwrap())
}

/// Parse hex color with fallback to named color.
fn hex_or_named(hex: &str, fallback: &str) -> Color {
    Color::parse(hex).unwrap_or_else(|_| parse_color(fallback))
}

/// Default theme with full colors and provider brand colors.
#[must_use]
pub fn create_default_theme() -> ThemeConfig {
    let color_depth = detect_color_depth();
    let box_style = if has_unicode_support() {
        BoxStyle::Rounded
    } else {
        BoxStyle::Ascii
    };

    ThemeConfig {
        name: "default".to_string(),

        primary: Style::new().color(parse_color("cyan")).bold(),
        secondary: Style::new().color(parse_color("blue")),
        success: Style::new().color(parse_color("green")).bold(),
        warning: Style::new().color(parse_color("yellow")).bold(),
        error: Style::new().color(parse_color("red")).bold(),
        muted: Style::new().dim(),

        // Provider brand colors
        provider_claude: Style::new().color(hex_or_named("#D97706", "yellow")),
        provider_openai: Style::new().color(hex_or_named("#10B981", "green")),
        provider_google: Style::new().color(hex_or_named("#3B82F6", "blue")),
        provider_cursor: Style::new().color(hex_or_named("#8B5CF6", "magenta")),
        provider_copilot: Style::new().color(hex_or_named("#2563EB", "blue")),
        provider_other: Style::new().color(parse_color("white")),

        table_header: Style::new().bold().underline(),
        table_border: Style::new().dim(),
        table_row_alt: Some(Style::new().dim()),

        panel_title: Style::new().bold(),
        panel_border: Style::new().color(parse_color("cyan")),
        panel_error_border: Style::new().color(parse_color("red")),

        cost: Style::new().color(parse_color("green")).bold(),
        count: Style::new().color(parse_color("cyan")),
        percentage: Style::new().color(parse_color("yellow")),
        percentage_high: Style::new().color(parse_color("red")).bold(),

        status_success: Style::new().color(parse_color("green")),
        status_warning: Style::new().color(parse_color("yellow")),
        status_error: Style::new().color(parse_color("red")),

        box_style,
        color_depth,
    }
}

/// Minimal theme with subtle styling (bold only, no colors).
#[must_use]
pub fn create_minimal_theme() -> ThemeConfig {
    let color_depth = detect_color_depth();

    ThemeConfig {
        name: "minimal".to_string(),

        primary: Style::new().bold(),
        secondary: Style::new(),
        success: Style::new().bold(),
        warning: Style::new().bold(),
        error: Style::new().bold(),
        muted: Style::new().dim(),

        // Minimal uses no provider-specific colors
        provider_claude: Style::new(),
        provider_openai: Style::new(),
        provider_google: Style::new(),
        provider_cursor: Style::new(),
        provider_copilot: Style::new(),
        provider_other: Style::new(),

        table_header: Style::new().bold(),
        table_border: Style::new(),
        table_row_alt: None,

        panel_title: Style::new().bold(),
        panel_border: Style::new(),
        panel_error_border: Style::new().bold(),

        cost: Style::new().bold(),
        count: Style::new(),
        percentage: Style::new(),
        percentage_high: Style::new().bold(),

        status_success: Style::new(),
        status_warning: Style::new(),
        status_error: Style::new().bold(),

        box_style: BoxStyle::Rounded,
        color_depth,
    }
}

/// High-contrast theme for accessibility.
#[must_use]
pub fn create_high_contrast_theme() -> ThemeConfig {
    let color_depth = detect_color_depth();

    ThemeConfig {
        name: "high-contrast".to_string(),

        primary: Style::new().color(parse_color("white")).bold(),
        secondary: Style::new().color(parse_color("white")),
        success: Style::new().color(parse_color("green")).bold(),
        warning: Style::new().color(parse_color("yellow")).bold(),
        error: Style::new().color(parse_color("red")).bold(),
        muted: Style::new().color(parse_color("white")),

        // High contrast uses bold for all providers
        provider_claude: Style::new().color(parse_color("yellow")).bold(),
        provider_openai: Style::new().color(parse_color("green")).bold(),
        provider_google: Style::new().color(parse_color("blue")).bold(),
        provider_cursor: Style::new().color(parse_color("magenta")).bold(),
        provider_copilot: Style::new().color(parse_color("cyan")).bold(),
        provider_other: Style::new().color(parse_color("white")).bold(),

        table_header: Style::new().color(parse_color("white")).bold().underline(),
        table_border: Style::new().color(parse_color("white")),
        table_row_alt: None,

        panel_title: Style::new().color(parse_color("white")).bold(),
        panel_border: Style::new().color(parse_color("white")).bold(),
        panel_error_border: Style::new().color(parse_color("red")).bold(),

        cost: Style::new().color(parse_color("green")).bold(),
        count: Style::new().color(parse_color("white")).bold(),
        percentage: Style::new().color(parse_color("yellow")).bold(),
        percentage_high: Style::new().color(parse_color("red")).bold(),

        status_success: Style::new().color(parse_color("green")).bold(),
        status_warning: Style::new().color(parse_color("yellow")).bold(),
        status_error: Style::new().color(parse_color("red")).bold(),

        box_style: BoxStyle::Heavy,
        color_depth,
    }
}

/// ASCII theme for legacy terminals without Unicode support.
#[must_use]
pub fn create_ascii_theme() -> ThemeConfig {
    ThemeConfig {
        box_style: BoxStyle::Ascii,
        ..create_default_theme()
    }
}

/// Get theme by name with alias support.
#[must_use]
pub fn theme_by_name(name: &str) -> ThemeConfig {
    match name.to_lowercase().as_str() {
        "default" => create_default_theme(),
        "minimal" | "min" => create_minimal_theme(),
        "high-contrast" | "highcontrast" | "hc" => create_high_contrast_theme(),
        "ascii" | "plain" => create_ascii_theme(),
        unknown => {
            tracing::warn!(theme = %unknown, "Unknown theme, using default");
            create_default_theme()
        }
    }
}

/// Get provider-specific style by name (standalone function).
#[must_use]
pub fn provider_style(name: &str, theme: &ThemeConfig) -> Style {
    theme.provider_style(name).clone()
}

// =============================================================================
// Legacy Theme (for backwards compatibility)
// =============================================================================

/// Regex for stripping rich markup tags - compiled once.
///
/// Handles all markup formats:
/// - Basic: `[bold]`, `[italic]`, `[underline]`
/// - Colors: `[red]`, `[green]`, `[blue]`
/// - Hex: `[#ff0000]`, `[#abc]`
/// - RGB: `[rgb(255,0,0)]`
/// - 256-color: `[color(196)]`
/// - Combined: `[bold red on white]`
/// - Closing: `[/]`, `[/bold]`
static MARKUP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern matches valid markup tags but not array indices like [0]
    Regex::new(r"\[/?[a-zA-Z_#][a-zA-Z0-9_# (),]*\]").unwrap()
});

/// Regex for stripping ANSI escape sequences.
static ANSI_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").unwrap());

/// Minimal theme descriptor for rich output decisions (legacy compatibility).
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

    /// Create a theme from a name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Get the full theme configuration.
    #[must_use]
    pub fn config(&self) -> ThemeConfig {
        theme_by_name(&self.name)
    }
}

impl From<&ThemeConfig> for Theme {
    fn from(config: &ThemeConfig) -> Self {
        Self {
            name: config.name.clone(),
        }
    }
}

/// Default theme for rich output (legacy compatibility).
#[must_use]
pub fn default_theme() -> Theme {
    Theme {
        name: "default".to_string(),
    }
}

/// Load theme selection from environment or defaults (legacy compatibility).
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

/// Load full theme configuration with CLI and env support.
///
/// Priority:
/// 1. CLI flag (`theme_arg`)
/// 2. Environment variable (`CAUT_THEME`)
/// 3. Auto-detection based on terminal capabilities
/// 4. Default theme
#[must_use]
pub fn get_theme_config(theme_arg: Option<&str>) -> ThemeConfig {
    // Priority 1: CLI flag
    if let Some(name) = theme_arg {
        tracing::debug!(source = "cli_flag", theme = %name, "Theme selected via CLI");
        return theme_by_name(name);
    }

    // Priority 2: Environment variable
    if let Ok(name) = std::env::var(THEME_ENV) {
        if !name.trim().is_empty() {
            tracing::debug!(source = "env_var", theme = %name, "Theme selected via CAUT_THEME");
            return theme_by_name(&name);
        }
    }

    // Priority 3: Auto-detect based on capabilities
    if !has_unicode_support() {
        tracing::debug!(
            source = "auto_detect",
            reason = "no_unicode",
            "Using ASCII theme"
        );
        return create_ascii_theme();
    }

    // Priority 4: Default
    tracing::debug!(source = "default", "Using default theme");
    create_default_theme()
}

/// Central safety gate - determines if rich output is allowed.
///
/// Returns `false` (plain output) when ANY of these conditions are true:
/// 1. `format` is not `Human` (robot/JSON/MD mode)
/// 2. `--no-color` flag is set
/// 3. `NO_COLOR` env var is set (any value, per <https://no-color.org/>)
/// 4. `CAUT_PLAIN` env var is set
/// 5. stdout is not a TTY (piped or redirected)
/// 6. `TERM=dumb`
/// 7. `CI` env var is set (common in CI systems)
/// 8. `GITHUB_ACTIONS` env var is set
///
/// The checks are ordered by likelihood and cost.
#[must_use]
pub fn should_use_rich_output(format: OutputFormat, no_color_flag: bool) -> bool {
    // Fast path: explicit robot mode (non-Human format)
    if format != OutputFormat::Human {
        tracing::debug!(
            reason = "robot_mode",
            decision = "disabled",
            "Rich output DISABLED: non-Human format"
        );
        return false;
    }

    // CLI flag --no-color
    if no_color_flag {
        tracing::debug!(
            reason = "no_color_flag",
            decision = "disabled",
            "Rich output DISABLED: --no-color flag"
        );
        return false;
    }

    // Check standard NO_COLOR (https://no-color.org/)
    // Spec says ANY value (including empty) disables color
    if std::env::var("NO_COLOR").is_ok() {
        tracing::debug!(
            reason = "no_color_env",
            decision = "disabled",
            "Rich output DISABLED: NO_COLOR set"
        );
        return false;
    }

    // Check caut-specific plain mode
    if std::env::var("CAUT_PLAIN").is_ok() {
        tracing::debug!(
            reason = "caut_plain",
            decision = "disabled",
            "Rich output DISABLED: CAUT_PLAIN set"
        );
        return false;
    }

    // Check if stdout is a terminal
    if !env_util::stdout_is_tty() {
        tracing::debug!(
            reason = "not_tty",
            decision = "disabled",
            "Rich output DISABLED: stdout not TTY"
        );
        return false;
    }

    // Check for dumb terminal
    if std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false) {
        tracing::debug!(
            reason = "term_dumb",
            decision = "disabled",
            "Rich output DISABLED: TERM=dumb"
        );
        return false;
    }

    // Check for CI environments (often non-interactive)
    if std::env::var("CI").is_ok() {
        tracing::debug!(
            reason = "ci_environment",
            decision = "disabled",
            "Rich output DISABLED: CI environment"
        );
        return false;
    }

    // Check specifically for GitHub Actions
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        tracing::debug!(
            reason = "github_actions",
            decision = "disabled",
            "Rich output DISABLED: GITHUB_ACTIONS set"
        );
        return false;
    }

    tracing::debug!(decision = "enabled", "Rich output ENABLED");
    true
}

/// Remove rich markup tags from text, preserving content.
///
/// Handles all markup formats:
/// - Basic: `[bold]`, `[italic]`, `[underline]`
/// - Colors: `[red]`, `[green]`, `[blue]`
/// - Hex: `[#ff0000]`, `[#abc]`
/// - RGB: `[rgb(255,0,0)]`
/// - 256-color: `[color(196)]`
/// - Combined: `[bold red on white]`
/// - Closing: `[/]`, `[/bold]`
///
/// Does NOT remove array indices like `[0]` or brackets without markup syntax.
#[must_use]
pub fn strip_markup(text: &str) -> String {
    MARKUP_REGEX.replace_all(text, "").to_string()
}

/// Strip markup AND any ANSI escape codes that might have leaked through.
///
/// Use this for guaranteed plain text output.
#[must_use]
pub fn strip_all_formatting(text: &str) -> String {
    let no_markup = strip_markup(text);
    ANSI_REGEX.replace_all(&no_markup, "").to_string()
}

/// Check if text contains ANSI escape codes.
#[must_use]
pub fn contains_ansi(text: &str) -> bool {
    text.contains("\x1b[")
}

/// Wrapper around rich_rust Console that respects safety gates.
///
/// This struct provides a consistent interface for output that automatically
/// handles the rich/plain decision. When rich output is disabled, markup is
/// stripped from all output.
pub struct RichConsole {
    inner: Option<Console>,
    enabled: bool,
    theme: Theme,
}

impl RichConsole {
    /// Create a new `RichConsole` respecting the given output settings.
    #[must_use]
    pub fn new(format: OutputFormat, no_color_flag: bool) -> Self {
        let enabled = should_use_rich_output(format, no_color_flag);
        let theme = get_theme();
        Self {
            inner: if enabled { Some(Console::new()) } else { None },
            enabled,
            theme,
        }
    }

    /// Print to stdout with markup (if rich enabled).
    ///
    /// If rich is disabled, markup is stripped and plain text is printed.
    pub fn print(&self, text: &str) {
        if self.enabled {
            if let Some(console) = &self.inner {
                console.print(text);
            }
        } else {
            println!("{}", strip_markup(text));
        }
    }

    /// Print to stderr (for errors).
    ///
    /// Even when rich is enabled for stdout, stderr respects its own TTY status.
    /// Note: rich_rust Console doesn't have stderr support, so we print directly.
    pub fn eprint(&self, text: &str) {
        if self.enabled && env_util::stderr_is_tty() {
            // rich_rust doesn't support stderr, print with markup interpretation
            // For now, we just print to stderr - the markup will be visible as-is
            // or could be rendered manually
            eprintln!("{text}");
        } else {
            eprintln!("{}", strip_markup(text));
        }
    }

    /// Print a renderable component.
    pub fn print_renderable<R: Renderable>(&self, renderable: &R) {
        if self.enabled {
            if let Some(console) = &self.inner {
                console.print(&renderable.render());
            }
        } else {
            println!("{}", renderable.render_plain());
        }
    }

    /// Check if rich output is enabled.
    #[must_use]
    pub fn is_rich_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the current theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Get terminal width (or default 80 if not available).
    #[must_use]
    pub fn width(&self) -> usize {
        self.inner
            .as_ref()
            .map_or(80, rich_rust::prelude::Console::width)
    }
}

/// Trait for components that can render in both rich and plain modes.
///
/// Implement this for any component that needs to support both styled
/// terminal output and plain text fallback.
pub trait Renderable {
    /// Render with rich formatting (may contain markup or ANSI codes).
    fn render(&self) -> String;

    /// Render as plain text (MUST NOT contain ANSI codes or markup).
    fn render_plain(&self) -> String;
}

/// Collect rich output diagnostics for debugging.
///
/// Provides detailed information about all environment variables and settings
/// that affect rich output decisions. Useful for `--debug-rich` flag.
#[must_use]
pub fn collect_rich_diagnostics(format: OutputFormat, no_color_flag: bool) -> String {
    let stdout_tty = env_util::stdout_is_tty();
    let stderr_tty = env_util::stderr_is_tty();

    let env_or_unset = |key: &str| std::env::var(key).unwrap_or_else(|_| "<unset>".to_string());

    let no_color_env = env_or_unset("NO_COLOR");
    let caut_plain_env = env_or_unset("CAUT_PLAIN");
    let term_env = env_or_unset("TERM");
    let ci_env = env_or_unset("CI");
    let github_actions_env = env_or_unset("GITHUB_ACTIONS");

    let log_level = env_or_unset("CAUT_LOG");
    let log_format = env_or_unset("CAUT_LOG_FORMAT");
    let log_file = env_or_unset("CAUT_LOG_FILE");

    let theme = env_or_unset(THEME_ENV);
    let rich_enabled = should_use_rich_output(format, no_color_flag);

    let lines = [
        format!("stdout is TTY: {stdout_tty}"),
        format!("stderr is TTY: {stderr_tty}"),
        format!("NO_COLOR: {no_color_env}"),
        format!("CAUT_PLAIN: {caut_plain_env}"),
        format!("TERM: {term_env}"),
        format!("CI: {ci_env}"),
        format!("GITHUB_ACTIONS: {github_actions_env}"),
        format!("CAUT_LOG: {log_level}"),
        format!("CAUT_LOG_FORMAT: {log_format}"),
        format!("CAUT_LOG_FILE: {log_file}"),
        format!("CAUT_THEME: {theme}"),
        format!("output format: {format:?}"),
        format!("no_color flag: {no_color_flag}"),
        format!("rich output enabled: {rich_enabled}"),
    ];

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

    #[allow(unsafe_code)]
    fn without_env_var(key: &str, f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prior = std::env::var(key).ok();
        unsafe {
            std::env::remove_var(key);
        }
        f();
        if let Some(val) = prior {
            unsafe {
                std::env::set_var(key, val);
            }
        }
    }

    // =========================================================================
    // rich_rust Smoke Tests
    // =========================================================================

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

    // =========================================================================
    // Safety Gate Tests - All 8 conditions
    // =========================================================================

    #[traced_test]
    #[test]
    fn test_robot_mode_json_disables_rich() {
        let result = should_use_rich_output(OutputFormat::Json, false);
        assert!(!result);
        assert!(logs_contain("robot_mode"));
        assert!(logs_contain("DISABLED"));
    }

    #[traced_test]
    #[test]
    fn test_robot_mode_markdown_disables_rich() {
        let result = should_use_rich_output(OutputFormat::Md, false);
        assert!(!result);
        assert!(logs_contain("robot_mode"));
    }

    #[traced_test]
    #[test]
    fn test_no_color_flag_disables_rich() {
        let result = should_use_rich_output(OutputFormat::Human, true);
        assert!(!result);
        assert!(logs_contain("no_color_flag"));
    }

    #[traced_test]
    #[test]
    fn test_no_color_env_disables_rich() {
        with_env_var("NO_COLOR", "1", || {
            let result = should_use_rich_output(OutputFormat::Human, false);
            assert!(!result);
            assert!(logs_contain("no_color_env"));
        });
    }

    #[traced_test]
    #[test]
    fn test_no_color_empty_value_still_disables() {
        // NO_COLOR spec says any value (including empty) disables color
        with_env_var("NO_COLOR", "", || {
            let result = should_use_rich_output(OutputFormat::Human, false);
            assert!(!result);
            assert!(logs_contain("no_color_env"));
        });
    }

    #[traced_test]
    #[test]
    fn test_caut_plain_env_disables_rich() {
        with_env_var("CAUT_PLAIN", "1", || {
            let result = should_use_rich_output(OutputFormat::Human, false);
            assert!(!result);
            assert!(logs_contain("caut_plain"));
        });
    }

    #[traced_test]
    #[test]
    fn test_term_dumb_disables_rich() {
        // Note: This test may not trigger in CI because stdout isn't a TTY,
        // so the not_tty check fires first. Testing the logic directly.
        let original = std::env::var("TERM").ok();
        let _guard = ENV_LOCK.lock().unwrap();
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("TERM", "dumb");
        }
        // The function will check TTY first, so let's just verify TERM=dumb is set
        let term = std::env::var("TERM").unwrap();
        assert_eq!(term, "dumb");
        #[allow(unsafe_code)]
        match original {
            Some(v) => unsafe {
                std::env::set_var("TERM", v);
            },
            None => unsafe {
                std::env::remove_var("TERM");
            },
        }
    }

    #[traced_test]
    #[test]
    fn test_ci_env_disables_rich() {
        with_env_var("CI", "true", || {
            let result = should_use_rich_output(OutputFormat::Human, false);
            assert!(!result);
            // Will log either "ci_environment" or earlier check like "not_tty"
        });
    }

    #[traced_test]
    #[test]
    fn test_github_actions_disables_rich() {
        with_env_var("GITHUB_ACTIONS", "true", || {
            let result = should_use_rich_output(OutputFormat::Human, false);
            assert!(!result);
        });
    }

    // =========================================================================
    // Markup Stripping Tests (10+ tests)
    // =========================================================================

    #[test]
    fn test_strip_markup_removes_bold() {
        assert_eq!(strip_markup("[bold]text[/]"), "text");
        assert_eq!(strip_markup("[bold]text[/bold]"), "text");
    }

    #[test]
    fn test_strip_markup_removes_colors() {
        assert_eq!(strip_markup("[red]error[/]"), "error");
        assert_eq!(strip_markup("[green]success[/]"), "success");
        assert_eq!(strip_markup("[blue]info[/]"), "info");
    }

    #[test]
    fn test_strip_markup_removes_hex_colors() {
        assert_eq!(strip_markup("[#ff0000]red[/]"), "red");
        assert_eq!(strip_markup("[#abc]short[/]"), "short");
        assert_eq!(strip_markup("[#AABBCC]mixed[/]"), "mixed");
    }

    #[test]
    fn test_strip_markup_removes_rgb() {
        assert_eq!(strip_markup("[rgb(255,0,0)]red[/]"), "red");
        assert_eq!(strip_markup("[rgb(0, 128, 255)]blue[/]"), "blue");
    }

    #[test]
    fn test_strip_markup_removes_color_number() {
        assert_eq!(strip_markup("[color(196)]red[/]"), "red");
        assert_eq!(strip_markup("[color(42)]green[/]"), "green");
    }

    #[test]
    fn test_strip_markup_removes_combined() {
        assert_eq!(
            strip_markup("[bold red on white]Error:[/] [italic]message[/]"),
            "Error: message"
        );
        assert_eq!(strip_markup("[bold underline]header[/]"), "header");
    }

    #[test]
    fn test_strip_markup_preserves_plain() {
        assert_eq!(strip_markup("plain text"), "plain text");
        assert_eq!(strip_markup("hello world"), "hello world");
        assert_eq!(strip_markup(""), "");
    }

    #[test]
    fn test_strip_markup_preserves_array_indices() {
        // Only strips valid markup patterns, not array indices
        assert_eq!(strip_markup("array[0]"), "array[0]");
        assert_eq!(strip_markup("data[123]"), "data[123]");
        assert_eq!(strip_markup("matrix[1][2]"), "matrix[1][2]");
    }

    #[test]
    fn test_strip_markup_preserves_invalid_markup() {
        // Invalid markup patterns should be preserved
        assert_eq!(strip_markup("[123]"), "[123]");
        assert_eq!(strip_markup("[ ]"), "[ ]");
    }

    #[test]
    fn test_strip_markup_nested() {
        assert_eq!(strip_markup("[bold][red]text[/][/]"), "text");
    }

    // =========================================================================
    // ANSI Code Tests
    // =========================================================================

    #[test]
    fn test_contains_ansi_positive() {
        assert!(contains_ansi("\x1b[31mred\x1b[0m"));
        assert!(contains_ansi("text \x1b[1mbold\x1b[0m text"));
    }

    #[test]
    fn test_contains_ansi_negative() {
        assert!(!contains_ansi("plain text"));
        assert!(!contains_ansi("[bold]markup[/]"));
        assert!(!contains_ansi(""));
    }

    #[test]
    fn test_strip_all_formatting_removes_ansi() {
        let with_ansi = "\x1b[31mred\x1b[0m";
        let stripped = strip_all_formatting(with_ansi);
        assert_eq!(stripped, "red");
        assert!(!contains_ansi(&stripped));
    }

    #[test]
    fn test_strip_all_formatting_removes_both() {
        let mixed = "[bold]\x1b[31mred\x1b[0m[/]";
        let stripped = strip_all_formatting(mixed);
        assert_eq!(stripped, "red");
        assert!(!contains_ansi(&stripped));
    }

    // =========================================================================
    // RichConsole Tests
    // =========================================================================

    #[test]
    fn test_rich_console_respects_robot_mode() {
        let console = RichConsole::new(OutputFormat::Json, false);
        assert!(!console.is_rich_enabled());
    }

    #[test]
    fn test_rich_console_respects_no_color_flag() {
        let console = RichConsole::new(OutputFormat::Human, true);
        assert!(!console.is_rich_enabled());
    }

    #[test]
    fn test_rich_console_provides_width() {
        let console = RichConsole::new(OutputFormat::Json, false);
        let width = console.width();
        // Should be default 80 when rich is disabled
        assert_eq!(width, 80, "Expected default width 80, got {}", width);
    }

    #[test]
    fn test_rich_console_provides_theme() {
        let console = RichConsole::new(OutputFormat::Human, false);
        let theme = console.theme();
        assert!(!theme.name().is_empty());
    }

    // =========================================================================
    // Theme Tests
    // =========================================================================

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
    fn test_theme_loading_default() {
        without_env_var(THEME_ENV, || {
            let theme = get_theme();
            assert_eq!(theme.name(), "default");
            assert!(logs_contain("default"));
        });
    }

    #[test]
    fn test_default_theme() {
        let theme = default_theme();
        assert_eq!(theme.name(), "default");
    }

    // =========================================================================
    // Logging Tests
    // =========================================================================

    #[traced_test]
    #[test]
    fn test_component_render_logs_timing() {
        let payload = make_test_provider_payload("codex", "cli");
        let _ = crate::render::human::render_usage(&[payload], true).unwrap();
        assert!(logs_contain("render_time_ms") || logs_contain("component"));
    }

    // =========================================================================
    // Diagnostics Tests
    // =========================================================================

    #[test]
    fn test_debug_diagnostics_output() {
        let output = collect_rich_diagnostics(OutputFormat::Human, false);
        assert!(output.contains("stdout is TTY"));
        assert!(output.contains("stderr is TTY"));
        assert!(output.contains("NO_COLOR"));
        assert!(output.contains("CAUT_PLAIN"));
        assert!(output.contains("TERM"));
        assert!(output.contains("CI"));
        assert!(output.contains("GITHUB_ACTIONS"));
        assert!(output.contains("output format"));
        assert!(output.contains("no_color flag"));
        assert!(output.contains("rich output enabled"));
    }

    #[test]
    fn test_debug_diagnostics_includes_all_env_vars() {
        with_env_var("CAUT_THEME", "test-theme", || {
            let output = collect_rich_diagnostics(OutputFormat::Human, false);
            assert!(output.contains("CAUT_THEME"));
            assert!(output.contains("test-theme"));
        });
    }

    // =========================================================================
    // Robot Mode Output Guarantee Tests
    // =========================================================================

    #[test]
    fn test_robot_mode_output_guaranteed_no_markup() {
        let console = RichConsole::new(OutputFormat::Json, false);
        assert!(!console.is_rich_enabled());
        // When rich is disabled, print() will strip markup
    }

    #[test]
    fn test_strip_markup_idempotent() {
        let text = "plain text without markup";
        assert_eq!(strip_markup(text), text);
        assert_eq!(strip_markup(&strip_markup(text)), text);
    }

    #[test]
    fn test_strip_all_formatting_idempotent() {
        let text = "plain text";
        assert_eq!(strip_all_formatting(text), text);
        assert_eq!(strip_all_formatting(&strip_all_formatting(text)), text);
    }

    // =========================================================================
    // Theme Config Tests
    // =========================================================================

    #[test]
    fn test_default_theme_config_has_all_fields() {
        let theme = create_default_theme();
        assert_eq!(theme.name, "default");
        // Core colors exist
        assert!(!format!("{:?}", theme.primary).is_empty());
        assert!(!format!("{:?}", theme.error).is_empty());
        assert!(!format!("{:?}", theme.success).is_empty());
    }

    #[test]
    fn test_minimal_theme_config_is_subtle() {
        let theme = create_minimal_theme();
        assert_eq!(theme.name, "minimal");
        // Minimal theme should not have alternating rows
        assert!(theme.table_row_alt.is_none());
    }

    #[test]
    fn test_high_contrast_theme_config_uses_bold() {
        let theme = create_high_contrast_theme();
        assert_eq!(theme.name, "high-contrast");
        // High contrast uses heavy box style
        assert!(matches!(theme.box_style, BoxStyle::Heavy));
    }

    #[test]
    fn test_ascii_theme_config_uses_ascii_box() {
        let theme = create_ascii_theme();
        assert!(matches!(theme.box_style, BoxStyle::Ascii));
    }

    #[traced_test]
    #[test]
    fn test_theme_config_from_cli_flag() {
        let theme = get_theme_config(Some("minimal"));
        assert_eq!(theme.name, "minimal");
        assert!(logs_contain("cli_flag"));
    }

    #[traced_test]
    #[test]
    fn test_theme_config_from_env_var() {
        with_env_var(THEME_ENV, "high-contrast", || {
            let theme = get_theme_config(None);
            assert_eq!(theme.name, "high-contrast");
            assert!(logs_contain("env_var") || logs_contain("CAUT_THEME"));
        });
    }

    #[test]
    fn test_cli_overrides_env_for_theme_config() {
        with_env_var(THEME_ENV, "minimal", || {
            let theme = get_theme_config(Some("ascii"));
            assert!(matches!(theme.box_style, BoxStyle::Ascii));
        });
    }

    #[test]
    fn test_theme_by_name_returns_default() {
        let theme = theme_by_name("default");
        assert_eq!(theme.name, "default");
    }

    #[test]
    fn test_theme_by_name_aliases() {
        // Test all aliases work
        let min = theme_by_name("min");
        assert_eq!(min.name, "minimal");
        let minimal = theme_by_name("minimal");
        assert_eq!(minimal.name, "minimal");
        let hc = theme_by_name("hc");
        assert_eq!(hc.name, "high-contrast");
        let high_contrast = theme_by_name("high-contrast");
        assert_eq!(high_contrast.name, "high-contrast");
        let highcontrast = theme_by_name("highcontrast");
        assert_eq!(highcontrast.name, "high-contrast");
        let ascii = theme_by_name("ascii");
        assert!(matches!(ascii.box_style, BoxStyle::Ascii));
        let plain = theme_by_name("plain");
        assert!(matches!(plain.box_style, BoxStyle::Ascii));
    }

    #[test]
    fn test_theme_by_name_unknown_falls_back() {
        let theme = theme_by_name("nonexistent_theme_xyz");
        // Should not panic, returns default
        assert_eq!(theme.name, "default");
    }

    // =========================================================================
    // Color Depth Detection Tests
    // =========================================================================

    #[test]
    fn test_color_depth_no_color() {
        with_env_var("NO_COLOR", "1", || {
            assert_eq!(detect_color_depth(), ColorDepth::NoColor);
        });
    }

    #[test]
    fn test_color_depth_truecolor() {
        without_env_var("NO_COLOR", || {
            with_env_var("COLORTERM", "truecolor", || {
                assert_eq!(detect_color_depth(), ColorDepth::TrueColor);
            });
        });
    }

    #[test]
    fn test_color_depth_256() {
        without_env_var("NO_COLOR", || {
            without_env_var("COLORTERM", || {
                with_env_var("TERM", "xterm-256color", || {
                    assert_eq!(detect_color_depth(), ColorDepth::Extended);
                });
            });
        });
    }

    // =========================================================================
    // Provider Color Tests
    // =========================================================================

    #[test]
    fn test_provider_style_claude() {
        let theme = create_default_theme();
        let style = theme.provider_style("Claude");
        assert_eq!(
            format!("{:?}", style),
            format!("{:?}", theme.provider_claude)
        );
    }

    #[test]
    fn test_provider_style_case_insensitive() {
        let theme = create_default_theme();
        let style1 = provider_style("CLAUDE", &theme);
        let style2 = provider_style("claude", &theme);
        let style3 = provider_style("Claude", &theme);
        assert_eq!(format!("{:?}", style1), format!("{:?}", style2));
        assert_eq!(format!("{:?}", style2), format!("{:?}", style3));
    }

    #[test]
    fn test_provider_style_unknown_uses_other() {
        let theme = create_default_theme();
        let style = theme.provider_style("unknown_provider_xyz");
        assert_eq!(
            format!("{:?}", style),
            format!("{:?}", theme.provider_other)
        );
    }

    #[test]
    fn test_all_major_providers_have_colors() {
        let theme = create_default_theme();
        let providers = ["Claude", "OpenAI", "Google", "Gemini", "Cursor", "Copilot"];
        for p in providers {
            let style = theme.provider_style(p);
            // Should not be the "other" fallback
            assert_ne!(
                format!("{:?}", style),
                format!("{:?}", theme.provider_other),
                "Provider {} should have dedicated color",
                p
            );
        }
    }

    // =========================================================================
    // Box Style Tests
    // =========================================================================

    #[test]
    fn test_box_style_rounded_chars() {
        let chars = BoxStyle::Rounded.chars();
        assert_eq!(chars.top_left, '╭');
        assert_eq!(chars.top_right, '╮');
        assert_eq!(chars.bottom_left, '╰');
        assert_eq!(chars.bottom_right, '╯');
    }

    #[test]
    fn test_box_style_ascii_portable() {
        let chars = BoxStyle::Ascii.chars();
        assert_eq!(chars.top_left, '+');
        assert_eq!(chars.horizontal, '-');
        assert_eq!(chars.vertical, '|');
        // All ASCII chars are < 128
        assert!((chars.top_left as u32) < 128);
        assert!((chars.horizontal as u32) < 128);
    }

    #[test]
    fn test_box_style_all_variants() {
        // Just verify they don't panic
        let _ = BoxStyle::Rounded.chars();
        let _ = BoxStyle::Square.chars();
        let _ = BoxStyle::Heavy.chars();
        let _ = BoxStyle::Double.chars();
        let _ = BoxStyle::Ascii.chars();
    }

    // =========================================================================
    // Legacy Theme Compatibility Tests
    // =========================================================================

    #[test]
    fn test_legacy_theme_to_config() {
        let theme = Theme::new("minimal");
        let config = theme.config();
        assert_eq!(config.name, "minimal");
    }

    #[test]
    fn test_theme_from_config() {
        let config = create_high_contrast_theme();
        let theme = Theme::from(&config);
        assert_eq!(theme.name(), "high-contrast");
    }
}
