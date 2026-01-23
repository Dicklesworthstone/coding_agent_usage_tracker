//! Shell prompt integration command.
//!
//! Provides fast, cached usage output for shell prompt integration.
//! Designed for <50ms execution time by reading from cache only.
//!
//! # Staleness Handling
//!
//! The prompt command handles stale cache data gracefully:
//! - **Fresh** (< 5 min): Display normally
//! - **Stale** (5-30 min): Display with "~" prefix
//! - **Very stale** (30+ min): Display with "?" prefix
//! - **Missing**: Display nothing (graceful degradation)

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cli::args::{PromptArgs, PromptFormat, ShellType};
use crate::error::Result;
use crate::storage::AppPaths;
use crate::storage::cache::{
    Staleness, is_fresh, read_if_fresh, read_with_staleness, write, write_async,
};

/// Cached prompt data for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCache {
    /// Timestamp when the cache was written.
    pub cached_at: DateTime<Utc>,
    /// Provider entries with usage data.
    pub providers: Vec<ProviderPromptData>,
}

/// Usage data for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPromptData {
    /// Provider name (cli_name).
    pub provider: String,
    /// Primary usage percentage (session window).
    pub primary_pct: Option<f64>,
    /// Secondary usage percentage (weekly window).
    pub secondary_pct: Option<f64>,
    /// Credits remaining (if applicable).
    pub credits_remaining: Option<f64>,
    /// Today's cost in USD (if applicable).
    pub cost_today_usd: Option<f64>,
}

/// Execute the prompt command.
pub fn execute(args: &PromptArgs) -> Result<()> {
    // Handle --install flag for shell snippets
    if let Some(shell) = args.install {
        print_install_snippet(shell);
        return Ok(());
    }

    // Read from cache only - never do network fetch
    let paths = AppPaths::new();
    let cache_path = paths.prompt_cache_file();

    // Read cache with staleness tracking
    // We use read_with_staleness for graceful degradation - showing stale data
    // with a prefix is better than showing nothing
    let cache_result: Option<(PromptCache, Staleness)> =
        read_with_staleness(&cache_path).unwrap_or(None);

    // For strict mode (legacy behavior), check max_age
    let (cache, staleness) = if args.strict_freshness {
        // Legacy behavior: use max_age for strict freshness check
        let max_age = Duration::from_secs(args.cache_max_age);
        match read_if_fresh(&cache_path, max_age).unwrap_or(None) {
            Some(cache) => (cache, Staleness::Fresh),
            None => return Ok(()), // Strict mode: no output if stale
        }
    } else {
        // Graceful degradation: show stale data with indicator
        match cache_result {
            Some((cache, staleness)) => (cache, staleness),
            None => return Ok(()), // No cache at all
        }
    };

    // Filter providers if specified
    let providers: Vec<_> = if let Some(ref provider_name) = args.provider {
        cache
            .providers
            .iter()
            .filter(|p| p.provider == *provider_name)
            .collect()
    } else {
        cache.providers.iter().collect()
    };

    if providers.is_empty() {
        return Ok(());
    }

    // Determine if we should use color
    let use_color = if args.no_color {
        false
    } else if args.color {
        true
    } else {
        // Auto-detect: disable color in non-TTY
        atty::is(atty::Stream::Stdout) && std::env::var("NO_COLOR").is_err()
    };

    // Format output with staleness prefix
    let output = format_prompt_with_staleness(&providers, args.prompt_format, use_color, staleness);
    print!("{output}");

    Ok(())
}

/// Format prompt output according to the requested format.
fn format_prompt(
    providers: &[&ProviderPromptData],
    format: PromptFormat,
    use_color: bool,
) -> String {
    format_prompt_with_staleness(providers, format, use_color, Staleness::Fresh)
}

/// Format prompt output with staleness indicator.
fn format_prompt_with_staleness(
    providers: &[&ProviderPromptData],
    format: PromptFormat,
    use_color: bool,
    staleness: Staleness,
) -> String {
    let base_output = match format {
        PromptFormat::Minimal => format_minimal(providers, use_color),
        PromptFormat::Compact => format_compact(providers, use_color),
        PromptFormat::Full => format_full(providers, use_color),
        PromptFormat::Icon => format_icon(providers, use_color),
    };

    // Add staleness prefix if not fresh
    if staleness == Staleness::Fresh || base_output.is_empty() {
        base_output
    } else {
        format!("{}{}", staleness.prefix(), base_output)
    }
}

/// Minimal format: "45%"
fn format_minimal(providers: &[&ProviderPromptData], use_color: bool) -> String {
    let Some(primary) = providers.first() else {
        return String::new();
    };

    let pct = primary.primary_pct.unwrap_or(0.0);
    let pct_str = format!("{:.0}%", pct);

    if use_color {
        colorize_by_percent(&pct_str, pct)
    } else {
        pct_str
    }
}

/// Compact format: "claude:45%|$12"
fn format_compact(providers: &[&ProviderPromptData], use_color: bool) -> String {
    let parts: Vec<String> = providers
        .iter()
        .map(|p| format_provider_compact(p, use_color))
        .filter(|s| !s.is_empty())
        .collect();

    parts.join(" ")
}

/// Full format: "claude:45%/67% codex:$12.34"
fn format_full(providers: &[&ProviderPromptData], use_color: bool) -> String {
    let parts: Vec<String> = providers
        .iter()
        .map(|p| format_provider_full(p, use_color))
        .filter(|s| !s.is_empty())
        .collect();

    parts.join(" ")
}

/// Icon format: "⚡45%"
fn format_icon(providers: &[&ProviderPromptData], use_color: bool) -> String {
    let Some(primary) = providers.first() else {
        return String::new();
    };

    let pct = primary.primary_pct.unwrap_or(0.0);
    let icon = if pct >= 90.0 {
        "⚠️"
    } else if pct >= 70.0 {
        "⚡"
    } else {
        "✓"
    };

    let pct_str = format!("{}{:.0}%", icon, pct);

    if use_color {
        colorize_by_percent(&pct_str, pct)
    } else {
        pct_str
    }
}

/// Format a single provider in compact format.
fn format_provider_compact(provider: &ProviderPromptData, use_color: bool) -> String {
    let mut parts = Vec::new();

    // Add usage percentage
    if let Some(pct) = provider.primary_pct {
        let pct_str = format!("{:.0}%", pct);
        parts.push(if use_color {
            colorize_by_percent(&pct_str, pct)
        } else {
            pct_str
        });
    }

    // Add cost if available
    if let Some(cost) = provider.cost_today_usd {
        let cost_str = if cost >= 100.0 {
            format!("${:.0}", cost)
        } else if cost >= 10.0 {
            format!("${:.1}", cost)
        } else {
            format!("${:.2}", cost)
        };
        parts.push(cost_str);
    }

    // Add credits if available and no cost
    if provider.cost_today_usd.is_none() {
        if let Some(credits) = provider.credits_remaining {
            let credits_str = if credits >= 100.0 {
                format!("${:.0}", credits)
            } else {
                format!("${:.1}", credits)
            };
            parts.push(credits_str);
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    // Short provider name
    let short_name = short_provider_name(&provider.provider);
    format!("{}:{}", short_name, parts.join("|"))
}

/// Format a single provider in full format.
fn format_provider_full(provider: &ProviderPromptData, use_color: bool) -> String {
    let mut parts = Vec::new();

    // Add primary/secondary usage percentages
    if let Some(pct) = provider.primary_pct {
        let pct_str = if let Some(secondary) = provider.secondary_pct {
            format!("{:.0}%/{:.0}%", pct, secondary)
        } else {
            format!("{:.0}%", pct)
        };
        parts.push(if use_color {
            colorize_by_percent(&pct_str, pct)
        } else {
            pct_str
        });
    }

    // Add cost
    if let Some(cost) = provider.cost_today_usd {
        parts.push(format!("${:.2}", cost));
    }

    // Add credits
    if let Some(credits) = provider.credits_remaining {
        parts.push(format!("cr:${:.2}", credits));
    }

    if parts.is_empty() {
        return String::new();
    }

    let short_name = short_provider_name(&provider.provider);
    format!("{}:{}", short_name, parts.join("|"))
}

/// Get short provider name for prompt display.
fn short_provider_name(name: &str) -> &str {
    match name {
        "claude" => "cl",
        "codex" => "cx",
        "gemini" => "gm",
        "cursor" => "cu",
        "copilot" => "cp",
        _ => &name[..2.min(name.len())],
    }
}

/// Colorize text based on percentage (ANSI codes).
fn colorize_by_percent(text: &str, percent: f64) -> String {
    let color_code = if percent >= 90.0 {
        "\x1b[31m" // Red
    } else if percent >= 70.0 {
        "\x1b[33m" // Yellow
    } else {
        "\x1b[32m" // Green
    };
    format!("{}{}\x1b[0m", color_code, text)
}

/// Print shell installation snippet.
fn print_install_snippet(shell: ShellType) {
    match shell {
        ShellType::Bash => print_bash_snippet(),
        ShellType::Zsh => print_zsh_snippet(),
        ShellType::Fish => print_fish_snippet(),
    }
}

/// Bash installation snippet.
fn print_bash_snippet() {
    println!(
        r#"# Add to your ~/.bashrc or ~/.bash_profile
# caut shell prompt integration

_caut_prompt() {{
    local usage
    usage=$(caut prompt 2>/dev/null)
    if [ -n "$usage" ]; then
        echo "[$usage] "
    fi
}}

# Option 1: Simple PS1 modification
PS1='$(_caut_prompt)\u@\h:\w$ '

# Option 2: Use PROMPT_COMMAND for more control
# PROMPT_COMMAND='_caut_prompt_cmd'
# _caut_prompt_cmd() {{
#     local usage=$(caut prompt 2>/dev/null)
#     if [ -n "$usage" ]; then
#         PS1="[$usage] \u@\h:\w$ "
#     else
#         PS1="\u@\h:\w$ "
#     fi
# }}

# Tip: Run 'caut usage' periodically to refresh the cache
# Example: Add to crontab: */5 * * * * caut usage --json > /dev/null 2>&1
"#
    );
}

/// Zsh installation snippet.
fn print_zsh_snippet() {
    println!(
        r#"# Add to your ~/.zshrc
# caut shell prompt integration

_caut_prompt() {{
    local usage
    usage=$(caut prompt 2>/dev/null)
    if [[ -n "$usage" ]]; then
        echo "[$usage] "
    fi
}}

# Option 1: Add to PROMPT
PROMPT='$(_caut_prompt)%n@%m:%~%# '

# Option 2: Use precmd hook
# precmd() {{
#     local usage=$(caut prompt 2>/dev/null)
#     if [[ -n "$usage" ]]; then
#         PROMPT="[$usage] %n@%m:%~%# "
#     else
#         PROMPT="%n@%m:%~%# "
#     fi
# }}

# Option 3: Right-side prompt
# RPROMPT='$(_caut_prompt)'

# Tip: Run 'caut usage' periodically to refresh the cache
# Example: Add to crontab: */5 * * * * caut usage --json > /dev/null 2>&1
"#
    );
}

/// Fish installation snippet.
fn print_fish_snippet() {
    println!(
        r#"# Add to your ~/.config/fish/config.fish
# caut shell prompt integration

function fish_prompt
    set -l usage (caut prompt 2>/dev/null)
    if test -n "$usage"
        set_color yellow
        echo -n "[$usage] "
        set_color normal
    end

    # Your normal prompt follows
    set_color green
    echo -n (whoami)
    set_color normal
    echo -n "@"
    set_color blue
    echo -n (hostname -s)
    set_color normal
    echo -n ":"
    set_color cyan
    echo -n (prompt_pwd)
    set_color normal
    echo -n "> "
end

# Tip: Run 'caut usage' periodically to refresh the cache
# Example: Add to crontab: */5 * * * * caut usage --json > /dev/null 2>&1
"#
    );
}

/// Update the prompt cache from fetch results.
/// Called by the usage command after successful fetches.
pub fn update_cache(providers: &[ProviderPromptData]) -> Result<()> {
    let paths = AppPaths::new();
    let cache_path = paths.prompt_cache_file();

    let cache = PromptCache {
        cached_at: Utc::now(),
        providers: providers.to_vec(),
    };

    write(&cache_path, &cache)
}

/// Update the prompt cache asynchronously (non-blocking).
/// Called by the usage command after successful fetches.
/// Returns immediately without waiting for write to complete.
pub fn update_cache_async(providers: Vec<ProviderPromptData>) {
    let paths = AppPaths::new();
    let cache_path = paths.prompt_cache_file();

    let cache = PromptCache {
        cached_at: Utc::now(),
        providers,
    };

    write_async(cache_path, cache);
}

/// Check if the prompt cache exists and is fresh.
pub fn cache_is_fresh(max_age_secs: u64) -> bool {
    let paths = AppPaths::new();
    let cache_path = paths.prompt_cache_file();
    is_fresh(&cache_path, Duration::from_secs(max_age_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_data() -> ProviderPromptData {
        ProviderPromptData {
            provider: "claude".to_string(),
            primary_pct: Some(45.5),
            secondary_pct: Some(32.0),
            credits_remaining: None,
            cost_today_usd: Some(12.34),
        }
    }

    #[test]
    fn format_minimal_shows_percent() {
        let data = make_test_data();
        let output = format_minimal(&[&data], false);
        assert_eq!(output, "46%");
    }

    #[test]
    fn format_compact_shows_provider_and_usage() {
        let data = make_test_data();
        let output = format_compact(&[&data], false);
        assert!(output.contains("cl:"));
        assert!(output.contains("46%"));
        // Costs >= $10 are formatted with 1 decimal place
        assert!(output.contains("$12.3"));
    }

    #[test]
    fn format_full_shows_both_windows() {
        let data = make_test_data();
        let output = format_full(&[&data], false);
        assert!(output.contains("cl:"));
        assert!(output.contains("46%/32%"));
    }

    #[test]
    fn format_icon_shows_icon_based_on_usage() {
        let low_usage = ProviderPromptData {
            provider: "claude".to_string(),
            primary_pct: Some(30.0),
            secondary_pct: None,
            credits_remaining: None,
            cost_today_usd: None,
        };
        let output = format_icon(&[&low_usage], false);
        assert!(output.contains("✓"));

        let high_usage = ProviderPromptData {
            primary_pct: Some(95.0),
            ..low_usage.clone()
        };
        let output = format_icon(&[&high_usage], false);
        assert!(output.contains("⚠️"));
    }

    #[test]
    fn short_provider_names() {
        assert_eq!(short_provider_name("claude"), "cl");
        assert_eq!(short_provider_name("codex"), "cx");
        assert_eq!(short_provider_name("gemini"), "gm");
        assert_eq!(short_provider_name("cursor"), "cu");
        assert_eq!(short_provider_name("copilot"), "cp");
        assert_eq!(short_provider_name("kimi"), "ki");
    }

    #[test]
    fn colorize_green_for_low_usage() {
        let output = colorize_by_percent("45%", 45.0);
        assert!(output.contains("\x1b[32m")); // Green
    }

    #[test]
    fn colorize_yellow_for_medium_usage() {
        let output = colorize_by_percent("75%", 75.0);
        assert!(output.contains("\x1b[33m")); // Yellow
    }

    #[test]
    fn colorize_red_for_high_usage() {
        let output = colorize_by_percent("95%", 95.0);
        assert!(output.contains("\x1b[31m")); // Red
    }

    // Staleness tests

    #[test]
    fn format_prompt_with_staleness_fresh_no_prefix() {
        let data = make_test_data();
        let output =
            format_prompt_with_staleness(&[&data], PromptFormat::Minimal, false, Staleness::Fresh);
        assert_eq!(output, "46%");
        assert!(!output.starts_with('~'));
        assert!(!output.starts_with('?'));
    }

    #[test]
    fn format_prompt_with_staleness_stale_prefix() {
        let data = make_test_data();
        let output =
            format_prompt_with_staleness(&[&data], PromptFormat::Minimal, false, Staleness::Stale);
        assert!(
            output.starts_with('~'),
            "Expected ~ prefix for stale data: {}",
            output
        );
        assert!(output.contains("46%"));
    }

    #[test]
    fn format_prompt_with_staleness_very_stale_prefix() {
        let data = make_test_data();
        let output = format_prompt_with_staleness(
            &[&data],
            PromptFormat::Minimal,
            false,
            Staleness::VeryStale,
        );
        assert!(
            output.starts_with('?'),
            "Expected ? prefix for very stale data: {}",
            output
        );
        assert!(output.contains("46%"));
    }

    #[test]
    fn format_prompt_with_empty_providers_no_prefix() {
        let providers: Vec<&ProviderPromptData> = vec![];
        let output = format_prompt_with_staleness(
            &providers,
            PromptFormat::Minimal,
            false,
            Staleness::Stale,
        );
        assert_eq!(output, ""); // Empty output should not have prefix
    }
}
