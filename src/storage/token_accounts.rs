//! Token account storage.
//!
//! A token account is a named credential (API key, cookie header or OAuth
//! token) for one provider. `caut usage --account <label>` /
//! `--account-index <n>` / `--all-accounts` fetch with these instead of the
//! locally discovered login.
//!
//! The file format is `CodexBar`'s `token-accounts.json`. `CodexBar` writes
//! timestamps as epoch seconds and carries provider-specific extras
//! (`usageScope`, `organizationId`, ...); caut reads both its own RFC 3339
//! timestamps and epoch seconds, and keeps unknown fields so a round trip
//! through caut loses nothing. See `EXISTING_CODEXBAR_STRUCTURE.md` section 8.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::core::provider::Provider;
use crate::error::{CautError, Result};

/// A single token account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenAccount {
    /// Unique ID (a UUID string).
    pub id: String,
    /// User-friendly label, matched case-insensitively by `--account`.
    pub label: String,
    /// The actual token/cookie.
    pub token: String,
    /// When this account was added.
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub added_at: DateTime<Utc>,
    /// When this account was last used.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_timestamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used: Option<DateTime<Utc>>,
    /// Provider-specific fields caut does not interpret, kept verbatim.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl TokenAccount {
    /// A new account with a fresh id, added now.
    #[must_use]
    pub fn new(label: impl Into<String>, token: impl Into<String>) -> Self {
        let label = label.into();
        let token = token.into();
        Self {
            id: generate_id(&label, &token),
            label,
            token,
            added_at: Utc::now(),
            last_used: None,
            extra: serde_json::Map::new(),
        }
    }

    /// The token with all but its last four characters masked, for display.
    #[must_use]
    pub fn masked_token(&self) -> String {
        let chars: Vec<char> = self.token.trim().chars().collect();
        if chars.len() <= 8 {
            return "*".repeat(chars.len());
        }
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}…{tail}", "*".repeat(4))
    }
}

/// Provider token account data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTokenAccountData {
    pub version: u32,
    pub accounts: Vec<TokenAccount>,
    #[serde(default)]
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

impl ProviderTokenAccountData {
    /// The active index clamped into range (0 when there are no accounts).
    #[must_use]
    pub fn clamped_active_index(&self) -> usize {
        self.active_index.min(self.accounts.len().saturating_sub(1))
    }
}

/// Root token accounts file structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Which accounts a command asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountSelection {
    /// `--account <label>`.
    pub label: Option<String>,
    /// `--account-index <n>`, already converted to 0-based.
    pub index: Option<usize>,
    /// `--all-accounts`.
    pub all: bool,
}

impl AccountSelection {
    /// Whether any explicit selection was made.
    #[must_use]
    pub const fn is_explicit(&self) -> bool {
        self.label.is_some() || self.index.is_some() || self.all
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
        let data = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            serde_json::from_str(&content).map_err(|e| CautError::ConfigParse {
                path: path.display().to_string(),
                line: Some(e.line()),
                message: e.to_string(),
            })?
        } else {
            TokenAccountsFile::default()
        };
        Ok(Self {
            data,
            path: Some(path.to_path_buf()),
        })
    }

    /// Create empty store.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            data: TokenAccountsFile::default(),
            path: None,
        }
    }

    /// The underlying file data.
    #[must_use]
    pub const fn data(&self) -> &TokenAccountsFile {
        &self.data
    }

    /// Get accounts for a provider.
    #[must_use]
    pub fn get_provider(&self, provider: Provider) -> Option<&ProviderTokenAccountData> {
        self.data.providers.get(provider.cli_name())
    }

    /// Get account by label (case-insensitive, surrounding whitespace ignored).
    #[must_use]
    pub fn get_by_label(&self, provider: Provider, label: &str) -> Option<&TokenAccount> {
        let wanted = label.trim().to_lowercase();
        self.get_provider(provider)?
            .accounts
            .iter()
            .find(|a| a.label.trim().to_lowercase() == wanted)
    }

    /// Get account by index (0-based).
    #[must_use]
    pub fn get_by_index(&self, provider: Provider, index: usize) -> Option<&TokenAccount> {
        self.get_provider(provider)?.accounts.get(index)
    }

    /// Get active account for provider (the active index is clamped).
    #[must_use]
    pub fn get_active(&self, provider: Provider) -> Option<&TokenAccount> {
        let data = self.get_provider(provider)?;
        data.accounts.get(data.clamped_active_index())
    }

    /// Get all accounts for a provider.
    #[must_use]
    pub fn get_all(&self, provider: Provider) -> Vec<&TokenAccount> {
        self.get_provider(provider)
            .map(|d| d.accounts.iter().collect())
            .unwrap_or_default()
    }

    /// Resolve an explicit selection into accounts, with `CodexBar`'s errors:
    /// no accounts, unknown label, or index out of range.
    ///
    /// # Errors
    /// Returns a configuration error describing why nothing matched.
    pub fn resolve(
        &self,
        provider: Provider,
        selection: &AccountSelection,
    ) -> Result<Vec<&TokenAccount>> {
        let accounts = self.get_all(provider);
        if accounts.is_empty() {
            return Err(CautError::Config(format!(
                "No token accounts configured for {}. Add one with: caut token-accounts add --provider {} --label <name>",
                provider.display_name(),
                provider.cli_name()
            )));
        }
        if selection.all {
            return Ok(accounts);
        }
        if let Some(label) = selection
            .label
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            return self
                .get_by_label(provider, label)
                .map(|a| vec![a])
                .ok_or_else(|| {
                    CautError::Config(format!(
                        "No {} token account labeled '{label}'. Available: {}",
                        provider.display_name(),
                        accounts
                            .iter()
                            .map(|a| a.label.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                });
        }
        if let Some(index) = selection.index {
            return self
                .get_by_index(provider, index)
                .map(|a| vec![a])
                .ok_or_else(|| {
                    CautError::Config(format!(
                        "Account index {} is out of range for {} (1-{})",
                        index + 1,
                        provider.display_name(),
                        accounts.len()
                    ))
                });
        }
        Ok(self.get_active(provider).into_iter().collect())
    }

    /// Add an account. Labels must be unique per provider (case-insensitive).
    /// The first account for a provider becomes active.
    ///
    /// # Errors
    /// Returns an error for an empty label/token or a duplicate label.
    pub fn add(&mut self, provider: Provider, account: TokenAccount) -> Result<()> {
        if account.label.trim().is_empty() {
            return Err(CautError::Config("Account label must not be empty".into()));
        }
        if account.token.trim().is_empty() {
            return Err(CautError::Config("Account token must not be empty".into()));
        }
        if self.get_by_label(provider, &account.label).is_some() {
            return Err(CautError::Config(format!(
                "A {} token account labeled '{}' already exists",
                provider.display_name(),
                account.label.trim()
            )));
        }
        self.data
            .providers
            .entry(provider.cli_name().to_string())
            .or_default()
            .accounts
            .push(account);
        Ok(())
    }

    /// Remove the account with this label; returns it. The active index is
    /// adjusted so it keeps pointing at the same account where possible.
    ///
    /// # Errors
    /// Returns an error when no account has the label.
    pub fn remove(&mut self, provider: Provider, label: &str) -> Result<TokenAccount> {
        let wanted = label.trim().to_lowercase();
        let data = self
            .data
            .providers
            .get_mut(provider.cli_name())
            .ok_or_else(|| missing_label(provider, label))?;
        let position = data
            .accounts
            .iter()
            .position(|a| a.label.trim().to_lowercase() == wanted)
            .ok_or_else(|| missing_label(provider, label))?;
        let removed = data.accounts.remove(position);
        if position < data.active_index {
            data.active_index -= 1;
        }
        data.active_index = data.clamped_active_index();
        Ok(removed)
    }

    /// Make the account with this label the active one.
    ///
    /// # Errors
    /// Returns an error when no account has the label.
    pub fn set_active(&mut self, provider: Provider, label: &str) -> Result<()> {
        let wanted = label.trim().to_lowercase();
        let data = self
            .data
            .providers
            .get_mut(provider.cli_name())
            .ok_or_else(|| missing_label(provider, label))?;
        let position = data
            .accounts
            .iter()
            .position(|a| a.label.trim().to_lowercase() == wanted)
            .ok_or_else(|| missing_label(provider, label))?;
        data.active_index = position;
        Ok(())
    }

    /// Save to file, readable only by the owner on Unix (it holds secrets).
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
            write_private(path, &content)?;
        }
        Ok(())
    }
}

fn missing_label(provider: Provider, label: &str) -> CautError {
    CautError::Config(format!(
        "No {} token account labeled '{}'",
        provider.display_name(),
        label.trim()
    ))
}

/// Write a file with owner-only permissions on Unix.
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` only applies on creation; tighten an existing file too.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
}

/// A UUID-formatted id derived from the account contents and the clock.
fn generate_id(label: &str, token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(label.as_bytes());
    hasher.update(token.as_bytes());
    hasher.update(
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
            .to_le_bytes(),
    );
    hasher.update(std::process::id().to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 4122 version 4 / variant bits, so the id reads as a random UUID.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

// =============================================================================
// Timestamps
// =============================================================================

/// A timestamp written as RFC 3339 text (caut) or epoch seconds (`CodexBar`).
#[derive(Deserialize)]
#[serde(untagged)]
enum FlexibleTimestamp {
    Text(String),
    Seconds(f64),
}

impl FlexibleTimestamp {
    fn into_datetime(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Text(text) => DateTime::parse_from_rfc3339(text.trim())
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|| text.trim().parse::<f64>().ok().and_then(epoch_seconds)),
            Self::Seconds(seconds) => epoch_seconds(seconds),
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole seconds and nanos of a finite timestamp
fn epoch_seconds(seconds: f64) -> Option<DateTime<Utc>> {
    if !seconds.is_finite() {
        return None;
    }
    let whole = seconds.trunc();
    let nanos = ((seconds - whole) * 1e9).round().clamp(0.0, 999_999_999.0) as u32;
    Utc.timestamp_opt(whole as i64, nanos).single()
}

fn deserialize_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<DateTime<Utc>, D::Error> {
    FlexibleTimestamp::deserialize(deserializer)?
        .into_datetime()
        .ok_or_else(|| serde::de::Error::custom("invalid timestamp"))
}

fn deserialize_optional_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<DateTime<Utc>>, D::Error> {
    Ok(Option::<FlexibleTimestamp>::deserialize(deserializer)?
        .and_then(FlexibleTimestamp::into_datetime))
}

/// Convert between `CodexBar` and caut formats.
pub mod convert {
    use super::{Result, TokenAccountsFile};
    use serde_json::{Value, json};

    /// Parse a token accounts file in either format.
    ///
    /// # Errors
    /// Returns an error if the content is not valid JSON or does not match the expected schema.
    pub fn from_codexbar(content: &str) -> Result<TokenAccountsFile> {
        Ok(serde_json::from_str(content)?)
    }

    /// Serialize in `CodexBar`'s format: timestamps as epoch seconds.
    ///
    /// # Errors
    /// Returns an error if serialization to JSON fails.
    pub fn to_codexbar(data: &TokenAccountsFile) -> Result<String> {
        let mut value = serde_json::to_value(data)?;
        if let Some(providers) = value.get_mut("providers").and_then(Value::as_object_mut) {
            for (name, provider) in providers.iter_mut() {
                let Some(accounts) = provider.get_mut("accounts").and_then(Value::as_array_mut)
                else {
                    continue;
                };
                for (account, original) in accounts.iter_mut().zip(
                    data.providers
                        .get(name)
                        .map(|p| &p.accounts)
                        .into_iter()
                        .flatten(),
                ) {
                    #[allow(clippy::cast_precision_loss)] // seconds since 1970 fit f64 exactly
                    let added = original.added_at.timestamp_millis() as f64 / 1000.0;
                    account["addedAt"] = json!(added);
                    if let Some(last_used) = original.last_used {
                        #[allow(clippy::cast_precision_loss)]
                        let used = last_used.timestamp_millis() as f64 / 1000.0;
                        account["lastUsed"] = json!(used);
                    }
                }
            }
        }
        Ok(serde_json::to_string_pretty(&value)?)
    }

    /// Serialize in caut's native format (RFC 3339 timestamps).
    ///
    /// # Errors
    /// Returns an error if serialization to JSON fails.
    pub fn to_caut(data: &TokenAccountsFile) -> Result<String> {
        Ok(serde_json::to_string_pretty(data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEXBAR_FILE: &str = r#"{
      "version": 1,
      "providers": {
        "zai": {
          "version": 1,
          "activeIndex": 1,
          "accounts": [
            {"id": "A", "label": "Personal", "token": "zai-1", "addedAt": 1767225600.5, "lastUsed": null},
            {"id": "B", "label": "Team", "token": "zai-2", "addedAt": 1767312000, "usageScope": "team", "organizationId": "org-9"}
          ]
        }
      }
    }"#;

    fn store_from(content: &str) -> TokenAccountStore {
        TokenAccountStore {
            data: serde_json::from_str(content).unwrap(),
            path: None,
        }
    }

    #[test]
    fn reads_codexbar_epoch_timestamps_and_extras() {
        let store = store_from(CODEXBAR_FILE);
        let team = store.get_by_label(Provider::Zai, "team").unwrap();
        assert_eq!(team.added_at, Utc.timestamp_opt(1_767_312_000, 0).unwrap());
        assert_eq!(team.extra.get("usageScope").unwrap(), "team");
        assert_eq!(team.extra.get("organizationId").unwrap(), "org-9");
        let personal = store.get_by_label(Provider::Zai, " PERSONAL ").unwrap();
        assert_eq!(personal.added_at.timestamp_subsec_millis(), 500);
        assert_eq!(personal.last_used, None);
        assert_eq!(store.get_active(Provider::Zai).unwrap().label, "Team");
    }

    #[test]
    fn reads_caut_rfc3339_fixture() {
        let content = include_str!("../../tests/fixtures/token_accounts/multi_account.json");
        let store = store_from(content);
        let personal = store.get_by_label(Provider::Claude, "personal").unwrap();
        assert_eq!(
            personal.added_at,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
        assert!(personal.last_used.is_some());
    }

    #[test]
    fn codexbar_round_trip_preserves_everything() {
        let original: TokenAccountsFile = serde_json::from_str(CODEXBAR_FILE).unwrap();
        let written = convert::to_codexbar(&original).unwrap();
        let value: serde_json::Value = serde_json::from_str(&written).unwrap();
        let team = &value["providers"]["zai"]["accounts"][1];
        assert_eq!(team["addedAt"], serde_json::json!(1_767_312_000.0));
        assert_eq!(team["usageScope"], "team");
        let reparsed = convert::from_codexbar(&written).unwrap();
        assert_eq!(reparsed, original);

        let native = convert::to_caut(&original).unwrap();
        assert!(native.contains("2026-01-02T00:00:00Z"));
        assert_eq!(convert::from_codexbar(&native).unwrap(), original);
    }

    #[test]
    fn resolve_follows_codexbar_semantics() {
        let store = store_from(CODEXBAR_FILE);
        let pick = |selection: AccountSelection| {
            store
                .resolve(Provider::Zai, &selection)
                .map(|accounts| accounts.iter().map(|a| a.label.clone()).collect::<Vec<_>>())
        };
        assert_eq!(pick(AccountSelection::default()).unwrap(), ["Team"]);
        assert_eq!(
            pick(AccountSelection {
                all: true,
                ..Default::default()
            })
            .unwrap(),
            ["Personal", "Team"]
        );
        assert_eq!(
            pick(AccountSelection {
                label: Some("personal".into()),
                ..Default::default()
            })
            .unwrap(),
            ["Personal"]
        );
        assert_eq!(
            pick(AccountSelection {
                index: Some(0),
                ..Default::default()
            })
            .unwrap(),
            ["Personal"]
        );
        let err = pick(AccountSelection {
            label: Some("nope".into()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("Personal, Team"));
        let err = pick(AccountSelection {
            index: Some(5),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("out of range"));
        let err = store
            .resolve(Provider::Claude, &AccountSelection::default())
            .unwrap_err();
        assert!(err.to_string().contains("token-accounts add"));
    }

    #[test]
    fn active_index_is_clamped() {
        let mut store = store_from(CODEXBAR_FILE);
        store.data.providers.get_mut("zai").unwrap().active_index = 99;
        assert_eq!(store.get_active(Provider::Zai).unwrap().label, "Team");
        assert_eq!(
            ProviderTokenAccountData::default().clamped_active_index(),
            0
        );
    }

    #[test]
    fn add_remove_and_activate() {
        let mut store = TokenAccountStore::empty();
        store
            .add(Provider::Cursor, TokenAccount::new("work", "cookie=1"))
            .unwrap();
        store
            .add(Provider::Cursor, TokenAccount::new("home", "cookie=2"))
            .unwrap();
        assert!(
            store
                .add(Provider::Cursor, TokenAccount::new("WORK", "x"))
                .is_err(),
            "labels are unique case-insensitively"
        );
        assert!(
            store
                .add(Provider::Cursor, TokenAccount::new(" ", "x"))
                .is_err()
        );
        assert!(
            store
                .add(Provider::Cursor, TokenAccount::new("x", " "))
                .is_err()
        );
        assert_eq!(store.get_active(Provider::Cursor).unwrap().label, "work");

        store.set_active(Provider::Cursor, "home").unwrap();
        assert_eq!(store.get_active(Provider::Cursor).unwrap().label, "home");

        // Removing an earlier account keeps the active one selected.
        let removed = store.remove(Provider::Cursor, "Work").unwrap();
        assert_eq!(removed.token, "cookie=1");
        assert_eq!(store.get_active(Provider::Cursor).unwrap().label, "home");

        assert!(store.remove(Provider::Cursor, "work").is_err());
        assert!(store.set_active(Provider::Zai, "x").is_err());
        store.remove(Provider::Cursor, "home").unwrap();
        assert!(store.get_active(Provider::Cursor).is_none());
    }

    #[test]
    fn save_and_load_round_trip_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("token-accounts.json");
        let mut store = TokenAccountStore::load(&path).unwrap();
        store
            .add(Provider::Zai, TokenAccount::new("main", "secret-token"))
            .unwrap();
        store.save().unwrap();

        let loaded = TokenAccountStore::load(&path).unwrap();
        assert_eq!(
            loaded.get_by_label(Provider::Zai, "main").unwrap().token,
            "secret-token"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn load_reports_parse_errors_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token-accounts.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = TokenAccountStore::load(&path).err().unwrap();
        assert!(matches!(err, CautError::ConfigParse { .. }));
    }

    #[test]
    fn generated_ids_look_like_uuids_and_differ() {
        let a = TokenAccount::new("a", "t");
        let b = TokenAccount::new("b", "t");
        assert_eq!(a.id.len(), 36);
        assert_eq!(a.id.chars().nth(14), Some('4'));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn masked_token_hides_secret() {
        assert_eq!(TokenAccount::new("a", "short").masked_token(), "*****");
        assert_eq!(
            TokenAccount::new("a", "sk-ant-abcdefgh1234").masked_token(),
            "****…1234"
        );
    }
}
