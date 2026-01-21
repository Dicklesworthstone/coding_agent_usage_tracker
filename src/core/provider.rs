//! Provider descriptors and registry.
//!
//! Defines all supported providers and their metadata.
//! See EXISTING_CODEXBAR_STRUCTURE.md section 6-7.

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
        matches!(
            self,
            Self::Claude
                | Self::Zai
                | Self::Cursor
                | Self::OpenCode
                | Self::Factory
                | Self::MiniMax
        )
    }

    /// Whether this provider supports local cost scanning.
    #[must_use]
    pub const fn supports_cost_scan(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }

    /// Default timeout for provider fetch operations.
    #[must_use]
    pub const fn default_timeout(self) -> Duration {
        match self {
            // API/OAuth providers can be a bit slower
            Self::Gemini | Self::VertexAI => Duration::from_secs(15),
            // Local CLIs or lightweight sources
            Self::Cursor | Self::Copilot | Self::Kiro | Self::JetBrainsAI | Self::Amp => {
                Duration::from_secs(8)
            }
            // Default for most providers
            _ => Duration::from_secs(10),
        }
    }

    /// Get the status page URL for this provider.
    #[must_use]
    pub const fn status_page_url(self) -> Option<&'static str> {
        match self {
            Self::Codex => Some("https://status.openai.com"),
            Self::Claude => Some("https://status.anthropic.com"),
            Self::Gemini | Self::VertexAI => Some("https://status.cloud.google.com"),
            Self::Cursor => Some("https://status.cursor.com"),
            Self::Copilot => Some("https://www.githubstatus.com"),
            _ => None,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSelection {
    /// Single provider.
    Single(Provider),
    /// Primary providers only (Codex + Claude).
    Both,
    /// All providers.
    All,
    /// Custom list of providers.
    Custom(Vec<Provider>),
}

impl ProviderSelection {
    /// Parse from CLI argument string.
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

impl Default for ProviderSelection {
    fn default() -> Self {
        Self::Both
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
        let mut descriptors = HashMap::new();

        // Codex
        descriptors.insert(
            Provider::Codex,
            ProviderDescriptor {
                id: Provider::Codex,
                metadata: ProviderMetadata {
                    display_name: "Codex",
                    session_label: "Session",
                    weekly_label: "Weekly",
                    supports_opus: false,
                    opus_label: None,
                    supports_credits: true,
                    status_page_url: Some("https://status.openai.com"),
                    dashboard_url: Some("https://platform.openai.com/usage"),
                },
                branding: ProviderBranding {
                    primary_color: "#10A37F",
                    icon: "󰧑",
                },
            },
        );

        // Claude
        descriptors.insert(
            Provider::Claude,
            ProviderDescriptor {
                id: Provider::Claude,
                metadata: ProviderMetadata {
                    display_name: "Claude",
                    session_label: "Chat",
                    weekly_label: "Weekly",
                    supports_opus: true,
                    opus_label: Some("Opus/Sonnet"),
                    supports_credits: false,
                    status_page_url: Some("https://status.anthropic.com"),
                    dashboard_url: Some("https://claude.ai/settings/usage"),
                },
                branding: ProviderBranding {
                    primary_color: "#D97706",
                    icon: "󰚩",
                },
            },
        );

        // Gemini
        descriptors.insert(
            Provider::Gemini,
            ProviderDescriptor {
                id: Provider::Gemini,
                metadata: ProviderMetadata {
                    display_name: "Gemini",
                    session_label: "Session",
                    weekly_label: "Weekly",
                    supports_opus: false,
                    opus_label: None,
                    supports_credits: false,
                    status_page_url: Some("https://status.cloud.google.com"),
                    dashboard_url: Some("https://aistudio.google.com"),
                },
                branding: ProviderBranding {
                    primary_color: "#4285F4",
                    icon: "󰊭",
                },
            },
        );

        // Add remaining providers with sensible defaults
        for provider in [
            Provider::Antigravity,
            Provider::Cursor,
            Provider::OpenCode,
            Provider::Factory,
            Provider::Zai,
            Provider::MiniMax,
            Provider::Kimi,
            Provider::Copilot,
            Provider::KimiK2,
            Provider::Kiro,
            Provider::VertexAI,
            Provider::JetBrainsAI,
            Provider::Amp,
        ] {
            descriptors.insert(
                provider,
                ProviderDescriptor {
                    id: provider,
                    metadata: ProviderMetadata {
                        display_name: provider.display_name(),
                        session_label: "Session",
                        weekly_label: "Weekly",
                        supports_opus: false,
                        opus_label: None,
                        supports_credits: false,
                        status_page_url: None,
                        dashboard_url: None,
                    },
                    branding: ProviderBranding {
                        primary_color: "#888888",
                        icon: "●",
                    },
                },
            );
        }

        Self { descriptors }
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
    fn provider_default_timeout_values() {
        assert_eq!(Provider::Claude.default_timeout().as_secs(), 10);
        assert_eq!(Provider::Codex.default_timeout().as_secs(), 10);
        assert_eq!(Provider::Gemini.default_timeout().as_secs(), 15);
        assert_eq!(Provider::Cursor.default_timeout().as_secs(), 8);
    }
}
