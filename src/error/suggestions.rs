//! Fix suggestion database for caut errors.
//!
//! Provides actionable fix suggestions mapped to specific error types,
//! including commands, context explanations, and prevention tips.

use std::time::Duration;

// =============================================================================
// Fix Suggestion Types
// =============================================================================

/// A fix suggestion for an error.
///
/// Contains actionable information to help users resolve errors.
#[derive(Debug, Clone)]
pub struct FixSuggestion {
    /// Primary fix commands in order of preference.
    /// These should be copy-paste ready for the terminal.
    pub commands: Vec<String>,

    /// Explanation of why this error occurred.
    /// Should help users understand the root cause.
    pub context: String,

    /// Tips to prevent this error in the future.
    pub prevention: Option<String>,

    /// Link to documentation for more information.
    pub doc_url: Option<String>,

    /// Whether this can potentially be auto-fixed.
    pub auto_fixable: bool,
}

impl FixSuggestion {
    /// Creates a new fix suggestion with required fields.
    #[must_use]
    pub fn new(commands: Vec<String>, context: impl Into<String>) -> Self {
        Self {
            commands,
            context: context.into(),
            prevention: None,
            doc_url: None,
            auto_fixable: false,
        }
    }

    /// Builder: adds prevention tips.
    #[must_use]
    pub fn with_prevention(mut self, prevention: impl Into<String>) -> Self {
        self.prevention = Some(prevention.into());
        self
    }

    /// Builder: adds documentation URL.
    #[must_use]
    pub fn with_doc_url(mut self, url: impl Into<String>) -> Self {
        self.doc_url = Some(url.into());
        self
    }

    /// Builder: marks as auto-fixable.
    #[must_use]
    pub const fn auto_fixable(mut self) -> Self {
        self.auto_fixable = true;
        self
    }
}

// =============================================================================
// CLI Installation Helpers
// =============================================================================

/// Returns installation commands for a CLI tool.
#[must_use]
pub fn install_commands_for_cli(name: &str) -> Vec<String> {
    match name.to_lowercase().as_str() {
        "claude" | "claude-code" => vec![
            "npm install -g @anthropic-ai/claude-code".to_string(),
            "# Or via homebrew: brew install claude-code".to_string(),
        ],
        "codex" => vec![
            "npm install -g @openai/codex".to_string(),
            "# Or download from: https://github.com/openai/codex-cli".to_string(),
        ],
        "cursor" => vec![
            "# Download Cursor from: https://cursor.sh".to_string(),
            "# Cursor CLI is bundled with the app".to_string(),
        ],
        "gemini" | "gemini-cli" => vec![
            "pip install google-generativeai".to_string(),
            "# Or: npm install -g @google/generative-ai".to_string(),
        ],
        "aider" => vec![
            "pip install aider-chat".to_string(),
            "# Or: pipx install aider-chat".to_string(),
        ],
        "copilot" | "github-copilot" => vec![
            "# GitHub Copilot is a VS Code / IDE extension".to_string(),
            "code --install-extension GitHub.copilot".to_string(),
        ],
        "windsurf" | "codeium" => {
            vec!["# Download Windsurf from: https://codeium.com/windsurf".to_string()]
        }
        "roo" | "roo-cline" => vec![
            "# Roo Cline is a VS Code extension".to_string(),
            "code --install-extension RooCline.roo-cline".to_string(),
        ],
        "amp" | "sourcegraph" => {
            vec!["# Download Amp from: https://sourcegraph.com/amp".to_string()]
        }
        _ => vec![format!(
            "# Install {} following its official documentation",
            name
        )],
    }
}

/// Returns documentation URL for a CLI tool.
#[must_use]
pub fn install_doc_for_cli(name: &str) -> Option<String> {
    match name.to_lowercase().as_str() {
        "claude" | "claude-code" => Some("https://docs.anthropic.com/claude-code".to_string()),
        "codex" => Some("https://platform.openai.com/docs/guides/code".to_string()),
        "cursor" => Some("https://cursor.sh/docs".to_string()),
        "gemini" | "gemini-cli" => Some("https://ai.google.dev/docs".to_string()),
        "aider" => Some("https://aider.chat/docs/install.html".to_string()),
        "copilot" | "github-copilot" => Some("https://docs.github.com/en/copilot".to_string()),
        "windsurf" | "codeium" => Some("https://codeium.com/windsurf/docs".to_string()),
        _ => None,
    }
}

// =============================================================================
// Suggestion Generators
// =============================================================================

/// Generates fix suggestions for authentication expired errors.
#[must_use]
pub fn auth_expired_suggestions(provider: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                format!("caut auth refresh {}", provider),
                format!("caut auth login {}", provider),
            ],
            format!(
                "Your OAuth token for {} has expired. Tokens are typically valid for \
                 24 hours. The token may have been revoked if you logged out elsewhere \
                 or changed your password.",
                provider
            ),
        )
        .with_prevention(
            "Use `caut usage --watch` to monitor session health and get alerts \
             before tokens expire. Consider setting up token refresh automation.",
        ),
    ]
}

/// Generates fix suggestions for authentication not configured errors.
#[must_use]
pub fn auth_not_configured_suggestions(provider: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            format!("caut auth login {}", provider),
            format!("caut setup {}", provider),
        ],
        format!(
            "No credentials found for {}. This provider requires authentication \
                 to access usage data. You may need to complete the OAuth flow or \
                 provide an API key.",
            provider
        ),
    )]
}

/// Generates fix suggestions for invalid authentication errors.
#[must_use]
pub fn auth_invalid_suggestions(provider: &str, reason: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                format!("caut auth logout {}", provider),
                format!("caut auth login {}", provider),
            ],
            format!(
                "The credentials for {} are invalid: {}. This typically happens when \
                 API keys are revoked, passwords change, or OAuth scopes are modified.",
                provider, reason
            ),
        )
        .with_prevention(
            "Verify your API keys and OAuth credentials are current. Use `caut auth status` \
             to check credential validity.",
        ),
    ]
}

/// Generates fix suggestions for timeout errors.
#[must_use]
pub fn timeout_suggestions(provider: &str, seconds: u64) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                format!(
                    "caut usage --provider {} --timeout {}",
                    provider,
                    seconds * 2
                ),
                "caut doctor".to_string(),
            ],
            format!(
                "The {} provider did not respond within {}s. This could be due to \
                 network issues, provider slowness, or the CLI tool being unresponsive.",
                provider, seconds
            ),
        )
        .with_prevention(
            "Increase timeout with `--timeout` or in config file. Consider disabling \
             slow providers if this persists. Check your network connection.",
        ),
    ]
}

/// Generates fix suggestions for DNS failure errors.
#[must_use]
pub fn dns_failure_suggestions(host: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                format!("nslookup {}", host),
                format!("ping {}", host),
                "cat /etc/resolv.conf".to_string(),
            ],
            format!(
                "DNS resolution failed for {}. The hostname could not be resolved to \
                 an IP address. This may indicate network configuration issues.",
                host
            ),
        )
        .with_prevention(
            "Verify your DNS settings and network connectivity. Try using a different \
             DNS server (e.g., 8.8.8.8 or 1.1.1.1).",
        ),
    ]
}

/// Generates fix suggestions for SSL/TLS errors.
#[must_use]
pub fn ssl_error_suggestions(message: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                "# Check system certificate store".to_string(),
                "openssl s_client -connect api.example.com:443 -showcerts".to_string(),
            ],
            format!(
                "SSL/TLS handshake or certificate verification failed: {}. This may \
                 indicate certificate issues, proxy interference, or outdated CA certs.",
                message
            ),
        )
        .with_prevention(
            "Ensure your system certificates are up to date. If behind a corporate \
             proxy, you may need to add its certificate to your trust store.",
        ),
    ]
}

/// Generates fix suggestions for connection refused errors.
#[must_use]
pub fn connection_refused_suggestions(host: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                format!("curl -v {}", host),
                "# Check if service is running".to_string(),
                "netstat -tuln | grep LISTEN".to_string(),
            ],
            format!(
                "Connection to {} was refused. The server is not accepting connections \
                 on the expected port. This may indicate the service is down.",
                host
            ),
        )
        .with_prevention(
            "Check if the target service is running. Verify firewall rules and \
             network connectivity. The provider may be experiencing an outage.",
        ),
    ]
}

/// Generates fix suggestions for config not found errors.
#[must_use]
pub fn config_not_found_suggestions(path: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec!["caut config init".to_string(), format!("touch {}", path)],
            format!(
                "Configuration file not found at {}. This file is needed for caut \
                 to know which providers to query and how to authenticate.",
                path
            ),
        )
        .auto_fixable(),
    ]
}

/// Generates fix suggestions for config parse errors.
#[must_use]
pub fn config_parse_suggestions(
    path: &str,
    line: Option<usize>,
    message: &str,
) -> Vec<FixSuggestion> {
    let line_info = line.map_or(String::new(), |l| format!(" on line {}", l));
    vec![
        FixSuggestion::new(
            vec![
                format!("$EDITOR {}", path),
                "caut config validate".to_string(),
                "caut config init --force".to_string(),
            ],
            format!(
                "The config file has a syntax error{}. The TOML parser reported: {}",
                line_info, message
            ),
        )
        .with_prevention(
            "Use `caut config validate` after editing to check config. Consider using \
             a TOML-aware editor with syntax highlighting.",
        ),
    ]
}

/// Generates fix suggestions for invalid config value errors.
#[must_use]
pub fn config_invalid_suggestions(key: &str, value: &str, message: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            format!("caut config set {} <valid_value>", key),
            "caut config show".to_string(),
        ],
        format!(
            "Invalid config value for '{}': '{}'. {}",
            key, value, message
        ),
    )]
}

/// Generates fix suggestions for rate limit errors.
#[must_use]
pub fn rate_limited_suggestions(
    provider: &str,
    retry_after: Option<Duration>,
    message: &str,
) -> Vec<FixSuggestion> {
    let wait_cmd = retry_after.map_or_else(
        || "# Wait before retrying".to_string(),
        |d| {
            format!(
                "sleep {} && caut usage --provider {}",
                d.as_secs(),
                provider
            )
        },
    );

    let wait_info = retry_after.map_or_else(
        || "Wait before retrying.".to_string(),
        |d| format!("Try again in {} seconds.", d.as_secs()),
    );

    vec![
        FixSuggestion::new(
            vec![wait_cmd],
            format!(
                "You have been rate limited by {}: {}. {}",
                provider, message, wait_info
            ),
        )
        .with_prevention(
            "Use `caut usage --watch` with longer intervals to avoid hitting rate \
             limits. Consider using cached results when available.",
        ),
    ]
}

/// Generates fix suggestions for provider unavailable errors.
#[must_use]
pub fn provider_unavailable_suggestions(provider: &str, message: &str) -> Vec<FixSuggestion> {
    vec![
        FixSuggestion::new(
            vec![
                format!("caut doctor --provider {}", provider),
                "# Check provider status page".to_string(),
            ],
            format!(
                "The {} provider is temporarily unavailable: {}. This is usually \
                 a transient issue on the provider's side.",
                provider, message
            ),
        )
        .with_prevention(
            "This is typically a temporary issue. Wait and retry. Check the \
             provider's status page for known outages.",
        ),
    ]
}

/// Generates fix suggestions for provider API errors.
#[must_use]
pub fn provider_api_error_suggestions(
    provider: &str,
    status_code: Option<u16>,
    message: &str,
) -> Vec<FixSuggestion> {
    let status_info = status_code.map_or(String::new(), |c| format!(" (HTTP {})", c));
    vec![FixSuggestion::new(
        vec![
            format!("caut doctor --provider {}", provider),
            "caut auth status".to_string(),
        ],
        format!(
            "The {} API returned an error{}: {}",
            provider, status_info, message
        ),
    )]
}

/// Generates fix suggestions for CLI not found errors.
#[must_use]
pub fn cli_not_found_suggestions(name: &str) -> Vec<FixSuggestion> {
    let commands = install_commands_for_cli(name);
    let doc_url = install_doc_for_cli(name);

    let mut suggestion = FixSuggestion::new(
        commands,
        format!(
            "The {} CLI tool is not installed or not in PATH. This provider \
             requires the CLI to fetch usage data.",
            name
        ),
    );

    if let Some(url) = doc_url {
        suggestion = suggestion.with_doc_url(url);
    }

    vec![suggestion]
}

/// Generates fix suggestions for permission denied errors.
#[must_use]
pub fn permission_denied_suggestions(path: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            format!("ls -la {}", path),
            format!("chmod 644 {}", path),
            format!("# Or: sudo chown $USER {}", path),
        ],
        format!(
            "Permission denied accessing {}. The file or directory may have \
                 restrictive permissions or be owned by another user.",
            path
        ),
    )]
}

/// Generates fix suggestions for missing environment variable errors.
#[must_use]
pub fn env_var_missing_suggestions(name: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            format!("export {}=\"your_value_here\"", name),
            format!("# Or add to ~/.bashrc: export {}=\"...\"", name),
        ],
        format!(
            "The environment variable {} is required but not set. This may be \
                 needed for authentication or configuration.",
            name
        ),
    )]
}

/// Generates fix suggestions for invalid provider errors.
#[must_use]
pub fn invalid_provider_suggestions(name: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            "caut providers list".to_string(),
            "caut usage --help".to_string(),
        ],
        format!(
            "Unknown provider: '{}'. Use `caut providers list` to see available \
                 providers.",
            name
        ),
    )]
}

/// Generates fix suggestions for unsupported source type errors.
#[must_use]
pub fn unsupported_source_suggestions(provider: &str, source_type: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![format!("caut providers show {}", provider)],
        format!(
            "The source type '{}' is not supported for provider {}. Check which \
                 sources are available for this provider.",
            source_type, provider
        ),
    )]
}

/// Generates fix suggestions for fetch failed errors.
#[must_use]
pub fn fetch_failed_suggestions(provider: &str, reason: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            format!("caut doctor --provider {}", provider),
            "caut auth status".to_string(),
        ],
        format!("Failed to fetch usage data from {}: {}", provider, reason),
    )]
}

/// Generates fix suggestions for no available strategy errors.
#[must_use]
pub fn no_strategy_suggestions(provider: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            format!("caut auth login {}", provider),
            format!("caut setup {}", provider),
        ],
        format!(
            "No fetch strategy is available for {}. This usually means the CLI \
                 is not installed and no web/API authentication is configured.",
            provider
        ),
    )]
}

/// Generates fix suggestions for account not found errors.
#[must_use]
pub fn account_not_found_suggestions(account: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![
            "caut accounts list".to_string(),
            "caut auth status".to_string(),
        ],
        format!(
            "Account '{}' not found. Use `caut accounts list` to see configured \
                 accounts.",
            account
        ),
    )]
}

/// Generates fix suggestions for no accounts configured errors.
#[must_use]
pub fn no_accounts_suggestions(provider: &str) -> Vec<FixSuggestion> {
    vec![FixSuggestion::new(
        vec![format!("caut auth login {}", provider)],
        format!(
            "No accounts are configured for {}. Set up authentication first.",
            provider
        ),
    )]
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_suggestion_builder() {
        let suggestion = FixSuggestion::new(vec!["cmd1".to_string()], "Test context")
            .with_prevention("Prevent tip")
            .with_doc_url("https://example.com")
            .auto_fixable();

        assert_eq!(suggestion.commands, vec!["cmd1"]);
        assert_eq!(suggestion.context, "Test context");
        assert_eq!(suggestion.prevention, Some("Prevent tip".to_string()));
        assert_eq!(suggestion.doc_url, Some("https://example.com".to_string()));
        assert!(suggestion.auto_fixable);
    }

    #[test]
    fn install_commands_for_known_clis() {
        let claude_cmds = install_commands_for_cli("claude");
        assert!(!claude_cmds.is_empty());
        assert!(claude_cmds.iter().any(|c| c.contains("npm install")));

        let codex_cmds = install_commands_for_cli("codex");
        assert!(!codex_cmds.is_empty());

        let unknown_cmds = install_commands_for_cli("unknown_tool");
        assert!(!unknown_cmds.is_empty());
    }

    #[test]
    fn install_docs_for_known_clis() {
        assert!(install_doc_for_cli("claude").is_some());
        assert!(install_doc_for_cli("codex").is_some());
        assert!(install_doc_for_cli("gemini").is_some());
        assert!(install_doc_for_cli("unknown_xyz").is_none());
    }

    #[test]
    fn auth_suggestions_have_commands() {
        let suggestions = auth_expired_suggestions("claude");
        assert!(!suggestions.is_empty());
        assert!(!suggestions[0].commands.is_empty());
        assert!(suggestions[0].commands.iter().any(|c| c.contains("claude")));
    }

    #[test]
    fn timeout_suggestions_include_provider() {
        let suggestions = timeout_suggestions("codex", 30);
        assert!(!suggestions.is_empty());
        assert!(suggestions[0].context.contains("codex"));
        assert!(suggestions[0].context.contains("30"));
    }

    #[test]
    fn rate_limit_suggestions_include_retry_info() {
        let suggestions =
            rate_limited_suggestions("claude", Some(Duration::from_secs(60)), "Too many requests");
        assert!(!suggestions.is_empty());
        assert!(suggestions[0].context.contains("60 seconds"));
    }

    #[test]
    fn cli_not_found_suggestions_have_install_commands() {
        let suggestions = cli_not_found_suggestions("claude");
        assert!(!suggestions.is_empty());
        assert!(
            suggestions[0]
                .commands
                .iter()
                .any(|c| c.contains("npm install"))
        );
        assert!(suggestions[0].doc_url.is_some());
    }
}
