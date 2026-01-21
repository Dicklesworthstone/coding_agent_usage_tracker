//! Error rendering for caut.
//!
//! Provides rich error rendering with fix suggestions for terminal output,
//! as well as simple text output for non-TTY environments.

use crate::cli::args::OutputFormat;
use crate::error::{CautError, FixSuggestion};
use crate::rich::{ThemeConfig, create_default_theme, should_use_rich_output};
use rich_rust::prelude::*;
use rich_rust::{ColorSystem, Segment, Style};

// =============================================================================
// Public API
// =============================================================================

/// Render an error with appropriate formatting based on terminal capabilities.
///
/// Uses rich output if:
/// - Output format is Human
/// - `no_color` flag is not set
/// - stderr is a TTY
/// - Not in a CI environment
///
/// Otherwise falls back to simple text output.
///
/// When format is JSON or Md, outputs structured JSON for machine consumption.
#[must_use]
pub fn render_error(error: &CautError, format: OutputFormat, no_color: bool) -> String {
    render_error_full(error, format, no_color, false)
}

/// Render an error with full control over all formatting options.
///
/// This is the main entry point for error rendering with explicit `pretty` control.
/// Use this when you need to respect the `--pretty` flag for JSON output.
#[must_use]
pub fn render_error_full(
    error: &CautError,
    format: OutputFormat,
    no_color: bool,
    pretty: bool,
) -> String {
    // JSON/Md formats get structured JSON error output for AI agents
    match format {
        OutputFormat::Json => return render_error_json(error, pretty),
        OutputFormat::Md => return render_error_json(error, true), // Md always pretty
        OutputFormat::Human => {}
    }

    // Check if we should use rich output (respects all safety gates)
    // Note: should_use_rich_output checks stdout, but errors go to stderr
    // We still use it for format/no_color checks but also check stderr TTY
    let use_rich = should_use_rich_output(format, no_color) && crate::util::env::stderr_is_tty();

    if use_rich {
        render_rich(error)
    } else {
        render_simple(error)
    }
}

/// Render error as structured JSON for machine consumption.
#[must_use]
pub fn render_error_json(error: &CautError, pretty: bool) -> String {
    let error_json = ErrorJson::from_error(error);
    if pretty {
        serde_json::to_string_pretty(&error_json).unwrap_or_else(|_| render_simple(error))
    } else {
        serde_json::to_string(&error_json).unwrap_or_else(|_| render_simple(error))
    }
}

// =============================================================================
// Rich Terminal Rendering
// =============================================================================

/// Render error with rich terminal formatting.
fn render_rich(error: &CautError) -> String {
    let theme = create_default_theme();
    let suggestions = error.fix_suggestions();

    let mut lines: Vec<String> = Vec::new();

    // Header with error code
    let header = render_header(error, &theme);
    lines.push(header);

    // Blank line
    lines.push(String::new());

    // Fix suggestions section
    if !suggestions.is_empty() {
        lines.push(render_suggestions_section(&suggestions, &theme));
    }

    // Context section (why this happened)
    if let Some(context) = suggestions.first().map(|s| &s.context) {
        if !context.is_empty() {
            lines.push(String::new());
            lines.push(render_context_section(context, &theme));
        }
    }

    // Prevention tips
    if let Some(prevention) = suggestions.first().and_then(|s| s.prevention.as_ref()) {
        lines.push(String::new());
        lines.push(render_prevention_section(prevention, &theme));
    }

    // Documentation link
    if let Some(doc_url) = suggestions.first().and_then(|s| s.doc_url.as_ref()) {
        lines.push(String::new());
        lines.push(render_doc_link(doc_url, &theme));
    }

    // Wrap in a panel
    let panel_content = lines.join("\n");
    render_error_panel(&panel_content, error, &theme)
}

/// Render the error header line with code.
fn render_header(error: &CautError, theme: &ThemeConfig) -> String {
    let error_style = &theme.error;
    let muted_style = &theme.muted;

    let header_text = format!("{}", error);
    let code_text = format!(" [{}]", error.error_code());

    let segments = vec![
        Segment::styled(header_text, error_style.clone()),
        Segment::styled(code_text, muted_style.clone()),
    ];

    segments_to_string(&segments, false)
}

/// Render the fix suggestions section.
fn render_suggestions_section(suggestions: &[FixSuggestion], theme: &ThemeConfig) -> String {
    let mut lines = Vec::new();

    let header = Segment::styled("How to fix:".to_string(), theme.primary.clone());
    lines.push(segments_to_string(&[header], false));

    for (i, suggestion) in suggestions.iter().enumerate() {
        for (j, cmd) in suggestion.commands.iter().enumerate() {
            let prefix = if j == 0 {
                format!("  {}. ", i + 1)
            } else {
                "     Or: ".to_string()
            };

            let prefix_seg = Segment::plain(prefix);
            let cmd_seg = Segment::styled(
                cmd.clone(),
                Style::new().color(Color::parse("cyan").unwrap()),
            );

            lines.push(segments_to_string(&[prefix_seg, cmd_seg], false));
        }
    }

    lines.join("\n")
}

/// Render the context section.
fn render_context_section(context: &str, theme: &ThemeConfig) -> String {
    let mut lines = Vec::new();

    let header = Segment::styled("Why this happened:".to_string(), theme.secondary.clone());
    lines.push(segments_to_string(&[header], false));

    // Wrap text to ~60 chars for readability
    for line in wrap_text(context, 60) {
        lines.push(format!("  {}", line));
    }

    lines.join("\n")
}

/// Render the prevention section.
fn render_prevention_section(prevention: &str, theme: &ThemeConfig) -> String {
    let mut lines = Vec::new();

    let header = Segment::styled("Prevention:".to_string(), theme.success.clone());
    lines.push(segments_to_string(&[header], false));

    for line in wrap_text(prevention, 60) {
        lines.push(format!("  {}", line));
    }

    lines.join("\n")
}

/// Render the documentation link.
fn render_doc_link(url: &str, theme: &ThemeConfig) -> String {
    let label = Segment::styled("Docs: ".to_string(), theme.muted.clone());
    let link = Segment::styled(url.to_string(), Style::new().underline());
    segments_to_string(&[label, link], false)
}

/// Wrap the error content in a styled panel.
fn render_error_panel(content: &str, error: &CautError, theme: &ThemeConfig) -> String {
    // Build panel using rich_rust
    let content_lines: Vec<Vec<Segment>> = content
        .lines()
        .map(|line| vec![Segment::plain(line.to_string())])
        .collect();

    let title = Text::new(error.category().to_string());
    let panel = Panel::new(content_lines)
        .title(title)
        .border_style(theme.panel_error_border.clone())
        .padding((1, 2));

    // Render to segments, then to string
    let segments = panel.render(70);
    segments_to_string(&segments, false)
}

// =============================================================================
// Simple Text Rendering
// =============================================================================

/// Render error as simple text (no ANSI codes, no Unicode).
fn render_simple(error: &CautError) -> String {
    let suggestions = error.fix_suggestions();

    let mut lines = Vec::new();

    // Header
    lines.push(format!("Error [{}]: {}", error.error_code(), error));

    // First command suggestion
    if let Some(suggestion) = suggestions.first() {
        if let Some(cmd) = suggestion.commands.first() {
            // Skip comments
            if !cmd.starts_with('#') {
                lines.push(format!("Fix: {}", cmd));
            } else if suggestion.commands.len() > 1 {
                // Try second command if first is a comment
                if let Some(cmd2) = suggestion.commands.get(1) {
                    if !cmd2.starts_with('#') {
                        lines.push(format!("Fix: {}", cmd2));
                    }
                }
            }
        }
    }

    lines.join("\n")
}

// =============================================================================
// JSON Rendering
// =============================================================================

/// JSON representation of an error for machine consumption.
#[derive(serde::Serialize)]
struct ErrorJson {
    error_code: String,
    category: String,
    message: String,
    is_retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u64>,
    suggestions: Vec<SuggestionJson>,
}

#[derive(serde::Serialize)]
struct SuggestionJson {
    commands: Vec<String>,
    context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prevention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_url: Option<String>,
    auto_fixable: bool,
}

impl ErrorJson {
    fn from_error(error: &CautError) -> Self {
        let suggestions = error.fix_suggestions();

        Self {
            error_code: error.error_code().to_string(),
            category: error.category().to_string(),
            message: error.to_string(),
            is_retryable: error.is_retryable(),
            provider: error.provider().map(String::from),
            retry_after_seconds: error.retry_after().map(|d| d.as_secs()),
            suggestions: suggestions
                .into_iter()
                .map(|s| SuggestionJson {
                    commands: s.commands,
                    context: s.context,
                    prevention: s.prevention,
                    doc_url: s.doc_url,
                    auto_fixable: s.auto_fixable,
                })
                .collect(),
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Convert segments to a styled string.
fn segments_to_string(segments: &[Segment], no_color: bool) -> String {
    let color_system = if no_color {
        ColorSystem::Standard
    } else {
        ColorSystem::TrueColor
    };

    segments
        .iter()
        .map(|seg| {
            if no_color || seg.style.is_none() {
                seg.text.to_string()
            } else {
                seg.style.as_ref().unwrap().render(&seg.text, color_system)
            }
        })
        .collect()
}

/// Simple text wrapping (no external dependency).
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::strip_all_formatting;
    use std::time::Duration;

    fn assert_no_ansi(s: &str) {
        assert!(
            !s.contains("\x1b["),
            "Contains ANSI codes: {}",
            &s[..100.min(s.len())]
        );
    }

    // -------------------------------------------------------------------------
    // Simple rendering tests
    // -------------------------------------------------------------------------

    #[test]
    fn simple_render_includes_error_code() {
        let err = CautError::CliNotFound {
            name: "claude".to_string(),
        };
        let output = render_simple(&err);
        assert!(output.contains("CAUT-E001"));
        assert!(output.contains("claude"));
    }

    #[test]
    fn simple_render_includes_fix_command() {
        let err = CautError::CliNotFound {
            name: "claude".to_string(),
        };
        let output = render_simple(&err);
        assert!(output.contains("Fix:") || output.contains("npm install"));
    }

    #[test]
    fn simple_render_no_ansi_codes() {
        let err = CautError::AuthExpired {
            provider: "claude".to_string(),
        };
        let output = render_simple(&err);
        assert_no_ansi(&output);
    }

    #[test]
    fn simple_render_skips_comment_commands() {
        let err = CautError::PermissionDenied {
            path: "/etc/secret".to_string(),
        };
        let output = render_simple(&err);
        // Should have a real command, not just a comment
        if output.contains("Fix:") {
            // If there's a fix line, it shouldn't start with #
            for line in output.lines() {
                if line.starts_with("Fix:") {
                    assert!(!line.contains("Fix: #"));
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // JSON rendering tests
    // -------------------------------------------------------------------------

    #[test]
    fn json_render_valid_json() {
        let err = CautError::AuthExpired {
            provider: "claude".to_string(),
        };
        let output = render_error_json(&err, false);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn json_render_includes_all_fields() {
        let err = CautError::RateLimited {
            provider: "claude".to_string(),
            retry_after: Some(Duration::from_secs(60)),
            message: "Too many requests".to_string(),
        };
        let output = render_error_json(&err, true);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["error_code"], "CAUT-P001");
        assert_eq!(parsed["category"], "Provider error");
        assert_eq!(parsed["is_retryable"], true);
        assert_eq!(parsed["provider"], "claude");
        assert_eq!(parsed["retry_after_seconds"], 60);
        assert!(parsed["suggestions"].is_array());
    }

    #[test]
    fn json_render_omits_null_fields() {
        let err = CautError::Timeout(30);
        let output = render_error_json(&err, false);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // Provider should be absent for timeout error
        assert!(parsed.get("provider").is_none());
        assert!(parsed.get("retry_after_seconds").is_none());
    }

    // -------------------------------------------------------------------------
    // Rich rendering tests
    // -------------------------------------------------------------------------

    #[test]
    fn rich_render_includes_error_code() {
        let err = CautError::CliNotFound {
            name: "claude".to_string(),
        };
        let output = render_rich(&err);
        // Strip ANSI to check content
        let plain = strip_all_formatting(&output);
        assert!(plain.contains("CAUT-E001"));
    }

    #[test]
    fn rich_render_includes_suggestions() {
        let err = CautError::AuthExpired {
            provider: "claude".to_string(),
        };
        let output = render_rich(&err);
        let plain = strip_all_formatting(&output);
        assert!(plain.contains("How to fix"));
        assert!(plain.contains("caut auth"));
    }

    #[test]
    fn rich_render_includes_context() {
        let err = CautError::AuthExpired {
            provider: "claude".to_string(),
        };
        let output = render_rich(&err);
        let plain = strip_all_formatting(&output);
        assert!(plain.contains("Why this happened") || plain.contains("expired"));
    }

    // -------------------------------------------------------------------------
    // Text wrapping tests
    // -------------------------------------------------------------------------

    #[test]
    fn wrap_text_respects_width() {
        let text = "This is a somewhat long line that should be wrapped at the specified width";
        let wrapped = wrap_text(text, 20);
        for line in &wrapped {
            assert!(
                line.len() <= 25, // Allow some overflow for long words
                "Line too long: {}",
                line
            );
        }
    }

    #[test]
    fn wrap_text_handles_empty() {
        let wrapped = wrap_text("", 60);
        assert_eq!(wrapped.len(), 1);
        assert!(wrapped[0].is_empty());
    }

    #[test]
    fn wrap_text_preserves_words() {
        let text = "one two three";
        let wrapped = wrap_text(text, 100);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0], "one two three");
    }

    // -------------------------------------------------------------------------
    // Format selection tests
    // -------------------------------------------------------------------------

    #[test]
    fn render_error_json_format_returns_json() {
        let err = CautError::Timeout(30);
        let output = render_error(&err, OutputFormat::Json, false);
        // Should be valid JSON when format is Json
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["error_code"].is_string());
    }

    #[test]
    fn render_error_full_respects_pretty() {
        let err = CautError::Timeout(30);

        // Non-pretty should be compact (no newlines in structure)
        let compact = render_error_full(&err, OutputFormat::Json, false, false);
        assert!(!compact.contains("\n  "));

        // Pretty should have indentation
        let pretty = render_error_full(&err, OutputFormat::Json, false, true);
        assert!(pretty.contains("\n  "));
    }

    #[test]
    fn render_error_md_format_always_pretty() {
        let err = CautError::Timeout(30);
        let output = render_error(&err, OutputFormat::Md, false);
        // Md format should always be pretty (indented)
        assert!(output.contains("\n  "));
    }

    #[test]
    fn render_error_with_no_color_returns_plain() {
        let err = CautError::Timeout(30);
        let output = render_error(&err, OutputFormat::Human, true);
        // With no_color=true, should be plain text
        assert_no_ansi(&output);
    }
}
