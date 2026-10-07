//! Provider descriptors and registry.
//!
//! Defines all supported providers and their metadata.
//! See `EXISTING_CODEXBAR_STRUCTURE.md` section 6-7.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::error::{CautError, Result};

// =============================================================================
// Provider Enum
// =============================================================================

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Codex,
    Claude,
    Gemini,
    Antigravity,
    Cursor,
    OpenCode,
    Factory,
    Zai,
    MiniMax,
    Kimi,
    Copilot,
    KimiK2,
    Kiro,
    VertexAI,
    JetBrainsAI,
    Amp,
}

impl Provider {
    /// All providers in display order.
    pub const ALL: &'static [Self] = &[
        Self::Codex,
        Self::Claude,
        Self::Gemini,
        Self::Antigravity,
        Self::Cursor,
        Self::OpenCode,
        Self::Factory,
        Self::Zai,
        Self::MiniMax,
        Self::Kimi,
        Self::Copilot,
        Self::KimiK2,
        Self::Kiro,
        Self::VertexAI,
        Self::JetBrainsAI,
        Self::Amp,
    ];

    /// Primary providers (Codex + Claude).
    pub const PRIMARY: &'static [Self] = &[Self::Codex, Self::Claude];

    /// CLI name for this provider.
    #[must_use]
    pub const fn cli_name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Antigravity => "antigravity",
            Self::Cursor => "cursor",
            Self::OpenCode => "opencode",
            Self::Factory => "factory",
            Self::Zai => "zai",
            Self::MiniMax => "minimax",
            Self::Kimi => "kimi",
            Self::Copilot => "copilot",
            Self::KimiK2 => "kimik2",
            Self::Kiro => "kiro",
            Self::VertexAI => "vertexai",
            Self::JetBrainsAI => "jetbrains",
            Self::Amp => "amp",
        }
    }

    /// Display name for human output.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Antigravity => "Antigravity",
            Self::Cursor => "Cursor",
            Self::OpenCode => "OpenCode",
            Self::Factory => "Factory",
            Self::Zai => "z.ai",
            Self::MiniMax => "MiniMax",
            Self::Kimi => "Kimi",
            Self::Copilot => "Copilot",
            Self::KimiK2 => "Kimi K2",
            Self::Kiro => "Kiro",
            Self::VertexAI => "Vertex AI",
            Self::JetBrainsAI => "JetBrains AI",
            Self::Amp => "Amp",
        }
    }

    /// Parse from CLI argument.
    ///
    /// # Errors
    /// Returns an error if the name does not match any known provider.
    pub fn from_cli_name(name: &str) -> Result<Self> {
        let lower = name.to_lowercase();
        Self::ALL
            .iter()
            .find(|p| p.cli_name() == lower)
            .copied()
            .ok_or_else(|| CautError::InvalidProvider(name.to_string()))
    }

    /// Whether this provider is a primary provider.
    #[must_use]
    pub const fn is_primary(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }

    /// Whether this provider supports credits.
    #[must_use]
    pub const fn supports_credits(self) -> bool {
        matches!(self, Self::Codex)
    }

    /// Whether this provider supports token accounts.
    #[must_use]
    pub const fn supports_token_accounts(self) -> bool {
        // Providers whose strategies take a credential from the selected
        // token account (API key, cookie header or bearer token).
        matches!(
            self,
            Self::Codex
                | Self::Claude
                | Self::Zai
                | Self::Cursor
                | Self::OpenCode
                | Self::Factory
                | Self::MiniMax
                | Self::Kimi
                | Self::KimiK2
                | Self::Copilot
                | Self::Amp
                | Self::Kiro
        )
    }

    /// Whether this provider supports local cost scanning.
    #[must_use]
    pub const fn supports_cost_scan(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }

    /// Default timeout for provider fetch operations.
    ///
    /// Windows process spawning is significantly slower than Unix (cmd.exe
    /// overhead, antivirus hooks, etc.), so each CLI subprocess invocation
    /// can take 1-3s even for trivial commands. The fetch pipeline may
    /// invoke several subprocesses sequentially (version check, multiple
    /// rate-limit probes), so we use generous timeouts to avoid false
    /// "request timeout" errors on slower machines.
    #[must_use]
    pub const fn default_timeout(self) -> Duration {
        match self {
            // Primary providers and API/OAuth: CLI fetch tries multiple
            // subprocess invocations sequentially (version + JSON probes +
            // fallbacks), each needing time to spawn. 30s accommodates slow
            // Windows environments where doctor --help succeeds in ~700ms
            // but the full fetch pipeline can exceed 10s.
            Self::Gemini | Self::VertexAI | Self::Claude | Self::Codex => Duration::from_secs(30),
            // Local CLIs or lightweight sources
            Self::Cursor | Self::Copilot | Self::Kiro | Self::JetBrainsAI | Self::Amp => {
                Duration::from_secs(15)
            }
            // Default for other providers
            _ => Duration::from_secs(20),
        }
    }

    /// Default priority for provider ordering (lower = higher priority).
    ///
    /// Used when no explicit priority is configured in the config file.
    /// Primary providers (Claude, Codex) have highest priority.
    #[must_use]
    pub const fn default_priority(self) -> i32 {
        match self {
            // Primary providers - highest priority
            Self::Claude => 1,
            Self::Codex => 2,
            // Popular secondary providers
            Self::Gemini => 3,
            Self::Cursor => 4,
            Self::Copilot => 5,
            // Other providers
            Self::VertexAI => 6,
            Self::Kiro => 7,
            Self::JetBrainsAI => 8,
            Self::Amp => 9,
            Self::Zai => 10,
            Self::MiniMax => 11,
            Self::Kimi => 12,
            Self::KimiK2 => 13,
            Self::Antigravity => 14,
            Self::OpenCode => 15,
            Self::Factory => 16,
        }
    }

    /// Get the Statuspage (`<url>/api/v2/status.json`) base URL for this
    /// provider. Google's providers publish no Statuspage feed, so they have
    /// none.
    #[must_use]
    pub const fn status_page_url(self) -> Option<&'static str> {
        match self {
            Self::Codex => Some("https://status.openai.com"),
            Self::Claude => Some("https://status.claude.com"),
            Self::Cursor => Some("https://status.cursor.com"),
            Self::Copilot => Some("https://www.githubstatus.com"),
            Self::Factory => Some("https://status.factory.ai"),
            _ => None,
        }
    }

    /// Label for the `primary` window, as `CodexBar` names it.
    #[must_use]
    pub const fn session_label(self) -> &'static str {
        match self {
            Self::Codex | Self::Claude => "Session",
            Self::Gemini => "Pro",
            Self::Antigravity => "Gemini Models",
            Self::Cursor => "Total",
            Self::OpenCode | Self::Zai => "5-hour",
            Self::Factory => "Standard",
            Self::MiniMax => "Prompts",
            Self::Kimi => "7-day usage",
            Self::Copilot => "Premium",
            Self::KimiK2 | Self::Kiro => "Credits",
            Self::VertexAI => "Requests",
            Self::JetBrainsAI => "Current",
            Self::Amp => "Amp Free",
        }
    }

    /// Label for the `secondary` window, as `CodexBar` names it.
    #[must_use]
    pub const fn weekly_label(self) -> &'static str {
        match self {
            Self::Codex | Self::Claude | Self::OpenCode | Self::Zai | Self::KimiK2 => "Weekly",
            Self::Gemini => "Flash",
            Self::Antigravity => "Claude and GPT",
            Self::Cursor => "Cursor",
            Self::Factory => "Premium",
            Self::MiniMax => "Window",
            Self::Kimi => "5-hour usage",
            Self::Copilot => "Chat",
            Self::Kiro => "Bonus",
            Self::VertexAI => "Tokens",
            Self::JetBrainsAI => "Refill",
            Self::Amp => "Balance",
        }
    }

    /// Label for the `tertiary` window, for providers that report one.
    #[must_use]
    pub const fn tertiary_label(self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("Opus/Sonnet"),
            Self::Gemini => Some("Flash Lite"),
            Self::Cursor => Some("Third Party"),
            _ => None,
        }
    }

    /// The provider's usage / billing dashboard.
    #[must_use]
    pub const fn dashboard_url(self) -> Option<&'static str> {
        match self {
            Self::Codex => Some("https://chatgpt.com/codex/cloud/settings/analytics#usage"),
            Self::Claude => Some("https://claude.ai/settings/usage"),
            Self::Gemini => Some("https://gemini.google.com"),
            Self::Cursor => Some("https://cursor.com/dashboard?tab=usage"),
            Self::OpenCode => Some("https://opencode.ai/auth"),
            Self::Factory => Some("https://app.factory.ai/settings/billing"),
            Self::Zai => Some("https://z.ai/manage-apikey/coding-plan/personal/my-plan"),
            Self::MiniMax => {
                Some("https://platform.minimax.io/user-center/payment/coding-plan?cycle_type=3")
            }
            Self::Kimi => Some("https://www.kimi.com/code/console"),
            Self::Copilot => Some("https://github.com/settings/copilot"),
            Self::Kiro => Some("https://app.kiro.dev/account/usage"),
            Self::VertexAI => Some("https://console.cloud.google.com/vertex-ai"),
            Self::Amp => Some("https://ampcode.com/settings/usage"),
            Self::Antigravity | Self::KimiK2 | Self::JetBrainsAI => None,
        }
    }

    /// Get installation suggestion for this provider's CLI.
    #[must_use]
    pub const fn install_suggestion(self) -> &'static str {
        match self {
            Self::Codex => "Install with: npm install -g @openai/codex",
            Self::Claude => "Install with: npm install -g @anthropic-ai/claude-code",
            Self::Gemini => "Install with: npm install -g @google/gemini-cli",
            Self::Cursor => "Install Cursor from: https://cursor.sh",
            Self::Copilot => "Install GitHub Copilot extension in your editor",
            Self::VertexAI => "Install with: gcloud components install vertex-ai",
            Self::JetBrainsAI => "Enable JetBrains AI Assistant in your IDE",
            _ => "Check provider documentation for installation instructions",
        }
    }

    /// Get authentication suggestion for this provider.
    #[must_use]
    pub const fn auth_suggestion(self) -> &'static str {
        match self {
            Self::Codex => "Run: codex auth login",
            Self::Claude => "Run: claude auth login",
            Self::Gemini => "Run: gemini auth login",
            Self::Cursor => "Open Cursor and sign in",
            Self::Copilot => "Sign in with your GitHub account",
            Self::VertexAI => "Run: gcloud auth application-default login",
            Self::JetBrainsAI => "Configure in IDE Settings > AI Assistant",
            _ => "Check provider documentation for authentication",
        }
    }

    /// Get the credentials file path for this provider (relative to home).
    #[must_use]
    pub const fn credentials_path(self) -> Option<&'static str> {
        match self {
            Self::Claude => Some(".claude/.credentials.json"),
            Self::Codex => Some(".codex/auth.json"),
            Self::Gemini => Some(".config/gemini/credentials.json"),
            Self::Cursor => Some(".cursor/auth.json"),
            _ => None,
        }
    }
}

// =============================================================================
// Provider Selection
// =============================================================================

/// Provider selection from CLI arguments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProviderSelection {
    /// Single provider.
    Single(Provider),
    /// Primary providers only (Codex + Claude).
    #[default]
    Both,
    /// All providers.
    All,
    /// Custom list of providers.
    Custom(Vec<Provider>),
}

impl ProviderSelection {
    /// Parse from CLI argument string.
    ///
    /// # Errors
    /// Returns an error if the argument does not match a valid provider name.
    pub fn from_arg(arg: &str) -> Result<Self> {
        match arg.to_lowercase().as_str() {
            "both" => Ok(Self::Both),
            "all" => Ok(Self::All),
            name => Ok(Self::Single(Provider::from_cli_name(name)?)),
        }
    }

    /// Get the providers in this selection.
    #[must_use]
    pub fn providers(&self) -> Vec<Provider> {
        match self {
            Self::Single(p) => vec![*p],
            Self::Both => Provider::PRIMARY.to_vec(),
            Self::All => Provider::ALL.to_vec(),
            Self::Custom(ps) => ps.clone(),
        }
    }

    /// Whether this selection is a single provider.
    #[must_use]
    pub const fn is_single(&self) -> bool {
        matches!(self, Self::Single(_))
    }
}

// =============================================================================
// Provider Descriptor
// =============================================================================

/// Metadata for a provider.
#[derive(Debug, Clone)]
pub struct ProviderMetadata {
    /// Display name.
    pub display_name: &'static str,
    /// Session window label (e.g., "Session", "Chat").
    pub session_label: &'static str,
    /// Weekly window label.
    pub weekly_label: &'static str,
    /// Whether provider supports Opus-tier tracking.
    pub supports_opus: bool,
    /// Opus tier label.
    pub opus_label: Option<&'static str>,
    /// Whether provider supports credits.
    pub supports_credits: bool,
    /// Status page URL.
    pub status_page_url: Option<&'static str>,
    /// Dashboard URL.
    pub dashboard_url: Option<&'static str>,
}

/// Branding information for display.
#[derive(Debug, Clone)]
pub struct ProviderBranding {
    /// Primary color (hex).
    pub primary_color: &'static str,
    /// Icon character (for terminal).
    pub icon: &'static str,
}

/// Complete provider descriptor.
#[derive(Debug, Clone)]
pub struct ProviderDescriptor {
    pub id: Provider,
    pub metadata: ProviderMetadata,
    pub branding: ProviderBranding,
}

// =============================================================================
// Provider Registry
// =============================================================================

/// Registry of all provider descriptors.
pub struct ProviderRegistry {
    descriptors: HashMap<Provider, ProviderDescriptor>,
}

impl ProviderRegistry {
    /// Create the registry with all providers.
    #[must_use]
    pub fn new() -> Self {
        let descriptors = Provider::ALL
            .iter()
            .map(|&provider| {
                let tertiary = provider.tertiary_label();
                let descriptor = ProviderDescriptor {
                    id: provider,
                    metadata: ProviderMetadata {
                        display_name: provider.display_name(),
                        session_label: provider.session_label(),
                        weekly_label: provider.weekly_label(),
                        supports_opus: tertiary.is_some(),
                        opus_label: tertiary,
                        supports_credits: provider.supports_credits(),
                        status_page_url: provider.status_page_url(),
                        dashboard_url: provider.dashboard_url(),
                    },
                    branding: Self::branding(provider),
                };
                (provider, descriptor)
            })
            .collect();
        Self { descriptors }
    }

    /// Terminal branding for a provider.
    const fn branding(provider: Provider) -> ProviderBranding {
        match provider {
            Provider::Codex => ProviderBranding {
                primary_color: "#10A37F",
                icon: "󰧑",
            },
            Provider::Claude => ProviderBranding {
                primary_color: "#D97706",
                icon: "󰚩",
            },
            Provider::Gemini => ProviderBranding {
                primary_color: "#4285F4",
                icon: "󰊭",
            },
            _ => ProviderBranding {
                primary_color: "#888888",
                icon: "●",
            },
        }
    }

    /// Get descriptor for a provider.
    #[must_use]
    pub fn get(&self, provider: Provider) -> Option<&ProviderDescriptor> {
        self.descriptors.get(&provider)
    }

    /// Iterate all descriptors.
    pub fn iter(&self) -> impl Iterator<Item = &ProviderDescriptor> {
        self.descriptors.values()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_from_cli_name() {
        assert_eq!(Provider::from_cli_name("codex").unwrap(), Provider::Codex);
        assert_eq!(Provider::from_cli_name("CLAUDE").unwrap(), Provider::Claude);
        assert!(Provider::from_cli_name("invalid").is_err());
    }

    #[test]
    fn provider_selection_from_arg() {
        assert_eq!(
            ProviderSelection::from_arg("both").unwrap(),
            ProviderSelection::Both
        );
        assert_eq!(
            ProviderSelection::from_arg("all").unwrap(),
            ProviderSelection::All
        );
        assert!(ProviderSelection::from_arg("codex").unwrap().is_single());
    }

    #[test]
    fn registry_has_all_providers() {
        let registry = ProviderRegistry::new();
        for provider in Provider::ALL {
            assert!(registry.get(*provider).is_some());
        }
    }

    #[test]
    fn registry_metadata_comes_from_provider_labels() {
        let registry = ProviderRegistry::new();
        for &provider in Provider::ALL {
            let meta = &registry.get(provider).unwrap().metadata;
            assert_ne!(meta.session_label, "");
            assert_ne!(meta.weekly_label, "");
            assert_ne!(meta.session_label, meta.weekly_label, "{provider:?}");
            assert_eq!(meta.supports_opus, meta.opus_label.is_some());
            assert_eq!(meta.display_name, provider.display_name());
        }
        let gemini = &registry.get(Provider::Gemini).unwrap().metadata;
        assert_eq!(
            (gemini.session_label, gemini.weekly_label, gemini.opus_label),
            ("Pro", "Flash", Some("Flash Lite"))
        );
        assert_eq!(Provider::Copilot.session_label(), "Premium");
        assert_eq!(Provider::Codex.tertiary_label(), None);
    }

    #[test]
    fn status_pages_are_statuspage_feeds_only() {
        assert_eq!(
            Provider::Claude.status_page_url(),
            Some("https://status.claude.com")
        );
        assert_eq!(Provider::Gemini.status_page_url(), None);
        assert_eq!(Provider::VertexAI.status_page_url(), None);
    }

    #[test]
    fn provider_default_timeout_values() {
        assert_eq!(Provider::Claude.default_timeout().as_secs(), 30);
        assert_eq!(Provider::Codex.default_timeout().as_secs(), 30);
        assert_eq!(Provider::Gemini.default_timeout().as_secs(), 30);
        assert_eq!(Provider::Cursor.default_timeout().as_secs(), 15);
    }
}
