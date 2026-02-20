//! Token account storage.
//!
//! Supports both caut native format and CodexBar-compatible format.
//! See `EXISTING_CODEXBAR_STRUCTURE.md` section 8.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::provider::Provider;
use crate::error::Result;

/// A single token account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenAccount {
    /// Unique ID.
    pub id: String,
    /// User-friendly label.
    pub label: String,
    /// The actual token/cookie.
    pub token: String,
    /// When this account was added.
    pub added_at: DateTime<Utc>,
    /// When this account was last used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<DateTime<Utc>>,
}

/// Provider token account data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTokenAccountData {
    pub version: u32,
    pub accounts: Vec<TokenAccount>,
    pub active_index: usize,
}

impl Default for ProviderTokenAccountData {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: Vec::new(),
            active_index: 0,
        }
    }
}

/// Root token accounts file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenAccountsFile {
    pub version: u32,
    pub providers: HashMap<String, ProviderTokenAccountData>,
}

impl Default for TokenAccountsFile {
    fn default() -> Self {
        Self {
            version: 1,
            providers: HashMap::new(),
        }
    }
}

/// Token account store.
pub struct TokenAccountStore {
    data: TokenAccountsFile,
    path: Option<std::path::PathBuf>,
}

impl TokenAccountStore {
    /// Load from file or create empty.
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be read or contains invalid JSON.
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let data: TokenAccountsFile = serde_json::from_str(&content)?;
            Ok(Self {
                data,
                path: Some(path.to_path_buf()),
            })
        } else {
            Ok(Self {
                data: TokenAccountsFile::default(),
                path: Some(path.to_path_buf()),
            })
        }
    }

    /// Create empty store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            data: TokenAccountsFile::default(),
            path: None,
        }
    }

    /// Get accounts for a provider.
    #[must_use]
    pub fn get_provider(&self, provider: Provider) -> Option<&ProviderTokenAccountData> {
        self.data.providers.get(provider.cli_name())
    }

    /// Get account by label (case-insensitive).
    #[must_use]
    pub fn get_by_label(&self, provider: Provider, label: &str) -> Option<&TokenAccount> {
        let label_lower = label.to_lowercase();
        self.get_provider(provider)?
            .accounts
            .iter()
            .find(|a| a.label.to_lowercase() == label_lower)
    }

    /// Get account by index (0-based).
    #[must_use]
    pub fn get_by_index(&self, provider: Provider, index: usize) -> Option<&TokenAccount> {
        self.get_provider(provider)?.accounts.get(index)
    }

    /// Get active account for provider.
    #[must_use]
    pub fn get_active(&self, provider: Provider) -> Option<&TokenAccount> {
        let data = self.get_provider(provider)?;
        data.accounts.get(data.active_index)
    }

    /// Get all accounts for a provider.
    #[must_use]
    pub fn get_all(&self, provider: Provider) -> Vec<&TokenAccount> {
        self.get_provider(provider)
            .map(|d| d.accounts.iter().collect())
            .unwrap_or_default()
    }

    /// Save to file.
    ///
    /// # Errors
    /// Returns an error if the parent directory cannot be created, serialization fails,
    /// or the file cannot be written.
    pub fn save(&self) -> Result<()> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let content = serde_json::to_string_pretty(&self.data)?;
            std::fs::write(path, content)?;
        }
        Ok(())
    }
}

/// Convert between `CodexBar` and caut formats.
pub mod convert {
    use super::{Result, TokenAccountsFile};

    /// Convert `CodexBar` format to caut format.
    ///
    /// # Errors
    /// Returns an error if the content is not valid JSON or does not match the expected schema.
    pub fn from_codexbar(content: &str) -> Result<TokenAccountsFile> {
        // CodexBar uses the same JSON structure, just different file location
        let data: TokenAccountsFile = serde_json::from_str(content)?;
        Ok(data)
    }

    /// Convert caut format to `CodexBar` format.
    ///
    /// # Errors
    /// Returns an error if serialization to JSON fails.
    pub fn to_codexbar(data: &TokenAccountsFile) -> Result<String> {
        // Same structure, just serialize
        Ok(serde_json::to_string_pretty(data)?)
    }
}
