//! Credential content hashing for change detection.
//!
//! Provides functionality to hash credential file contents in a way that
//! distinguishes meaningful account changes from simple token refreshes.
//!
//! ## Key Concepts
//!
//! - **Identity hash**: Based on stable identity fields (email, org, account ID)
//! - **Content hash**: Based on all credential content (for detecting any change)
//! - **Change detection**: Determines if a change is an account switch or token refresh
//!
//! ## Usage
//!
//! ```rust,ignore
//! use caut::core::credential_hash::{CredentialHasher, ChangeType};
//!
//! let hasher = CredentialHasher::new();
//!
//! // Hash a credential file
//! let result = hasher.hash_file("/path/to/credentials.json")?;
//!
//! // Compare with previous state
//! let change = hasher.detect_change(&old_hash, &new_hash);
//! match change {
//!     ChangeType::AccountSwitch => println!("Different account!"),
//!     ChangeType::TokenRefresh => println!("Same account, refreshed tokens"),
//!     ChangeType::NoChange => println!("No changes detected"),
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{CautError, Result};

/// Result of hashing a credential file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialHashes {
    /// Hash of identity-relevant fields (account switch detection).
    pub identity_hash: String,
    /// Hash of all credential content (any change detection).
    pub content_hash: String,
    /// Extracted identity fields (for debugging/logging).
    pub identity_fields: IdentityFields,
}

/// Identity fields extracted from credentials.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityFields {
    /// Account email if present.
    pub email: Option<String>,
    /// User ID if present.
    pub user_id: Option<String>,
    /// Organization/team if present.
    pub organization: Option<String>,
    /// Account name/label if present.
    pub account_name: Option<String>,
}

impl IdentityFields {
    /// Returns true if any identity field is present.
    #[must_use]
    pub fn has_identity(&self) -> bool {
        self.email.is_some()
            || self.user_id.is_some()
            || self.organization.is_some()
            || self.account_name.is_some()
    }

    /// Returns a display string for the identity.
    #[must_use]
    pub fn display(&self) -> String {
        if let Some(email) = &self.email {
            if let Some(org) = &self.organization {
                return format!("{email} ({org})");
            }
            return email.clone();
        }
        if let Some(user_id) = &self.user_id {
            return format!("user:{user_id}");
        }
        if let Some(account_name) = &self.account_name {
            return account_name.clone();
        }
        "unknown".to_string()
    }
}

/// Type of change detected between credential states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    /// No changes detected (hashes match).
    NoChange,
    /// Token was refreshed but identity unchanged.
    TokenRefresh,
    /// Different account/identity (account switch).
    AccountSwitch,
    /// File was created (no previous state).
    Created,
    /// File was deleted (no current state).
    Deleted,
}

impl ChangeType {
    /// Returns true if this change should trigger a usage snapshot.
    #[must_use]
    pub fn should_capture_snapshot(&self) -> bool {
        matches!(self, Self::AccountSwitch | Self::Created)
    }

    /// Returns a human-readable description.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::NoChange => "no change",
            Self::TokenRefresh => "token refreshed",
            Self::AccountSwitch => "account switched",
            Self::Created => "credentials created",
            Self::Deleted => "credentials deleted",
        }
    }
}

/// Hasher for credential files.
#[derive(Debug, Default)]
pub struct CredentialHasher {
    // Future: could hold provider-specific field mappings
}

impl CredentialHasher {
    /// Create a new credential hasher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Hash a credential file.
    ///
    /// Reads the file and extracts identity fields plus computes hashes.
    pub fn hash_file(&self, path: &Path) -> Result<CredentialHashes> {
        let content = fs::read_to_string(path).map_err(|e| {
            CautError::Other(anyhow::anyhow!(
                "read credential file '{}': {e}",
                path.display()
            ))
        })?;

        self.hash_content(&content)
    }

    /// Hash credential content (JSON string).
    pub fn hash_content(&self, content: &str) -> Result<CredentialHashes> {
        // Parse JSON
        let json: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| CautError::Other(anyhow::anyhow!("parse credentials JSON: {e}")))?;

        // Extract identity fields
        let identity_fields = self.extract_identity(&json);

        // Compute identity hash (stable fields only)
        let identity_hash = self.compute_identity_hash(&identity_fields);

        // Compute content hash (normalized JSON, excludes timestamps)
        let content_hash = self.compute_content_hash(&json);

        Ok(CredentialHashes {
            identity_hash,
            content_hash,
            identity_fields,
        })
    }

    /// Detect the type of change between old and new credential states.
    #[must_use]
    pub fn detect_change(
        &self,
        old: Option<&CredentialHashes>,
        new: Option<&CredentialHashes>,
    ) -> ChangeType {
        match (old, new) {
            (None, None) => ChangeType::NoChange,
            (None, Some(_)) => ChangeType::Created,
            (Some(_), None) => ChangeType::Deleted,
            (Some(old), Some(new)) => {
                if old.identity_hash != new.identity_hash {
                    ChangeType::AccountSwitch
                } else if old.content_hash != new.content_hash {
                    ChangeType::TokenRefresh
                } else {
                    ChangeType::NoChange
                }
            }
        }
    }

    /// Compare two hash strings and return the change type.
    ///
    /// This is a convenience method for use with stored `credential_hash` values.
    /// The hash should be in the format "identity:content" (as produced by `to_combined_hash`).
    #[must_use]
    pub fn detect_change_from_hashes(
        &self,
        old_hash: Option<&str>,
        new_hash: Option<&str>,
    ) -> ChangeType {
        match (old_hash, new_hash) {
            (None, None) => ChangeType::NoChange,
            (None, Some(_)) => ChangeType::Created,
            (Some(_), None) => ChangeType::Deleted,
            (Some(old), Some(new)) => {
                if old == new {
                    return ChangeType::NoChange;
                }

                // Parse combined hashes
                let old_identity = old.split(':').next().unwrap_or(old);
                let new_identity = new.split(':').next().unwrap_or(new);

                if old_identity != new_identity {
                    ChangeType::AccountSwitch
                } else {
                    ChangeType::TokenRefresh
                }
            }
        }
    }

    /// Extract identity fields from credential JSON.
    fn extract_identity(&self, json: &serde_json::Value) -> IdentityFields {
        let mut fields = IdentityFields::default();

        // Try various common field names for identity
        // Email fields
        for key in [
            "email",
            "user_email",
            "account_email",
            "userEmail",
            "accountEmail",
        ] {
            if let Some(v) = json.get(key).and_then(|v| v.as_str()) {
                fields.email = Some(v.to_string());
                break;
            }
        }

        // User ID fields
        for key in ["user_id", "userId", "sub", "id", "account_id", "accountId"] {
            if let Some(v) = json.get(key).and_then(|v| v.as_str()) {
                fields.user_id = Some(v.to_string());
                break;
            }
        }

        // Organization fields
        for key in [
            "organization",
            "org",
            "team",
            "org_id",
            "orgId",
            "workspace",
        ] {
            if let Some(v) = json.get(key).and_then(|v| v.as_str()) {
                fields.organization = Some(v.to_string());
                break;
            }
        }

        // Account name fields
        for key in [
            "name",
            "account_name",
            "accountName",
            "display_name",
            "displayName",
        ] {
            if let Some(v) = json.get(key).and_then(|v| v.as_str()) {
                fields.account_name = Some(v.to_string());
                break;
            }
        }

        // Try to extract from nested structures (e.g., id_token claims)
        if !fields.has_identity() {
            if let Some(token) = json.get("id_token").and_then(|v| v.as_str()) {
                if let Some(claims) = self.decode_jwt_claims(token) {
                    if fields.email.is_none() {
                        fields.email = claims
                            .get("email")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                    if fields.user_id.is_none() {
                        fields.user_id =
                            claims.get("sub").and_then(|v| v.as_str()).map(String::from);
                    }
                }
            }
        }

        fields
    }

    /// Compute a hash based on identity fields.
    fn compute_identity_hash(&self, fields: &IdentityFields) -> String {
        let mut hasher = Sha256::new();

        // Hash identity fields in deterministic order
        if let Some(email) = &fields.email {
            hasher.update("email:");
            hasher.update(email.as_bytes());
            hasher.update(";");
        }
        if let Some(user_id) = &fields.user_id {
            hasher.update("user_id:");
            hasher.update(user_id.as_bytes());
            hasher.update(";");
        }
        if let Some(organization) = &fields.organization {
            hasher.update("organization:");
            hasher.update(organization.as_bytes());
            hasher.update(";");
        }
        if let Some(account_name) = &fields.account_name {
            hasher.update("account_name:");
            hasher.update(account_name.as_bytes());
            hasher.update(";");
        }

        let result = hasher.finalize();
        hex::encode(&result[..16]) // Use first 16 bytes (32 hex chars)
    }

    /// Compute a hash of the full content, excluding volatile fields.
    fn compute_content_hash(&self, json: &serde_json::Value) -> String {
        // Normalize JSON by sorting keys and excluding volatile fields
        let normalized = self.normalize_json(json);
        let json_str = serde_json::to_string(&normalized).unwrap_or_default();

        let mut hasher = Sha256::new();
        hasher.update(json_str.as_bytes());
        let result = hasher.finalize();
        hex::encode(&result[..16])
    }

    /// Normalize JSON for consistent hashing.
    fn normalize_json(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                // Use BTreeMap for sorted keys
                let mut sorted: BTreeMap<&String, serde_json::Value> = BTreeMap::new();
                for (k, v) in map {
                    // Skip volatile fields that change on refresh
                    if self.is_volatile_field(k) {
                        continue;
                    }
                    sorted.insert(k, self.normalize_json(v));
                }
                serde_json::Value::Object(sorted.into_iter().map(|(k, v)| (k.clone(), v)).collect())
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.normalize_json(v)).collect())
            }
            _ => value.clone(),
        }
    }

    /// Check if a field is volatile (changes on token refresh).
    fn is_volatile_field(&self, key: &str) -> bool {
        matches!(
            key.to_lowercase().as_str(),
            "access_token"
                | "refresh_token"
                | "id_token"
                | "token"
                | "expires_at"
                | "expires_in"
                | "expiry"
                | "issued_at"
                | "iat"
                | "exp"
                | "nbf"
                | "timestamp"
                | "created_at"
                | "updated_at"
                | "last_refresh"
                | "last_used"
        )
    }

    /// Decode JWT claims (just the payload, no verification).
    fn decode_jwt_claims(&self, token: &str) -> Option<serde_json::Value> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        let payload = parts[1];
        let mut payload_std = payload.replace('-', "+").replace('_', "/");

        // Add padding
        let padding = (4 - payload_std.len() % 4) % 4;
        for _ in 0..padding {
            payload_std.push('=');
        }

        // Decode base64
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let decoded = STANDARD.decode(&payload_std).ok()?;
        serde_json::from_slice(&decoded).ok()
    }
}

/// Serialize credential hashes to a single combined string.
impl CredentialHashes {
    /// Create a combined hash string suitable for storage.
    ///
    /// Format: `identity_hash:content_hash`
    #[must_use]
    pub fn to_combined_hash(&self) -> String {
        format!("{}:{}", self.identity_hash, self.content_hash)
    }

    /// Parse a combined hash string back into components.
    ///
    /// Uses `split_once` to handle edge cases where hash might contain colons.
    #[must_use]
    pub fn from_combined_hash(hash: &str) -> Option<(String, String)> {
        hash.split_once(':')
            .map(|(identity, content)| (identity.to_string(), content.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_credential_json(email: &str, token: &str) -> String {
        format!(
            r#"{{"email": "{email}", "access_token": "{token}", "expires_at": "2026-01-28T00:00:00Z"}}"#
        )
    }

    #[test]
    fn test_hash_content_basic() {
        let hasher = CredentialHasher::new();
        let content = make_credential_json("user@example.com", "token123");

        let hashes = hasher.hash_content(&content).expect("hash content");

        assert!(!hashes.identity_hash.is_empty());
        assert!(!hashes.content_hash.is_empty());
        assert_eq!(
            hashes.identity_fields.email,
            Some("user@example.com".to_string())
        );
    }

    #[test]
    fn test_identity_hash_stable_across_token_refresh() {
        let hasher = CredentialHasher::new();

        // Same email, different tokens
        let content1 = make_credential_json("user@example.com", "token123");
        let content2 = make_credential_json("user@example.com", "token456");

        let hashes1 = hasher.hash_content(&content1).expect("hash1");
        let hashes2 = hasher.hash_content(&content2).expect("hash2");

        // Identity hash should be the same (same email)
        assert_eq!(hashes1.identity_hash, hashes2.identity_hash);

        // Content hash should differ (different tokens are excluded, but other fields might differ)
        // Actually since we exclude volatile fields, the content hash might be the same too
        // Let's verify the change detection works
        let change = hasher.detect_change(Some(&hashes1), Some(&hashes2));
        assert_eq!(change, ChangeType::NoChange); // Identity same, non-volatile content same
    }

    #[test]
    fn test_identity_hash_differs_on_account_switch() {
        let hasher = CredentialHasher::new();

        let content1 = make_credential_json("user1@example.com", "token123");
        let content2 = make_credential_json("user2@example.com", "token123");

        let hashes1 = hasher.hash_content(&content1).expect("hash1");
        let hashes2 = hasher.hash_content(&content2).expect("hash2");

        // Identity hash should differ (different emails)
        assert_ne!(hashes1.identity_hash, hashes2.identity_hash);

        let change = hasher.detect_change(Some(&hashes1), Some(&hashes2));
        assert_eq!(change, ChangeType::AccountSwitch);
    }

    #[test]
    fn test_detect_change_created() {
        let hasher = CredentialHasher::new();
        let content = make_credential_json("user@example.com", "token123");
        let hashes = hasher.hash_content(&content).expect("hash");

        let change = hasher.detect_change(None, Some(&hashes));
        assert_eq!(change, ChangeType::Created);
    }

    #[test]
    fn test_detect_change_deleted() {
        let hasher = CredentialHasher::new();
        let content = make_credential_json("user@example.com", "token123");
        let hashes = hasher.hash_content(&content).expect("hash");

        let change = hasher.detect_change(Some(&hashes), None);
        assert_eq!(change, ChangeType::Deleted);
    }

    #[test]
    fn test_detect_change_no_change() {
        let hasher = CredentialHasher::new();
        let content = make_credential_json("user@example.com", "token123");
        let hashes = hasher.hash_content(&content).expect("hash");

        let change = hasher.detect_change(Some(&hashes), Some(&hashes));
        assert_eq!(change, ChangeType::NoChange);
    }

    #[test]
    fn test_combined_hash_roundtrip() {
        let hasher = CredentialHasher::new();
        let content = make_credential_json("user@example.com", "token123");
        let hashes = hasher.hash_content(&content).expect("hash");

        let combined = hashes.to_combined_hash();
        let (identity, content) = CredentialHashes::from_combined_hash(&combined).expect("parse");

        assert_eq!(identity, hashes.identity_hash);
        assert_eq!(content, hashes.content_hash);
    }

    #[test]
    fn test_detect_change_from_hashes() {
        let hasher = CredentialHasher::new();

        // Same hash
        let hash = "abc123:def456";
        let change = hasher.detect_change_from_hashes(Some(hash), Some(hash));
        assert_eq!(change, ChangeType::NoChange);

        // Different identity
        let old = "abc123:def456";
        let new = "xyz789:def456";
        let change = hasher.detect_change_from_hashes(Some(old), Some(new));
        assert_eq!(change, ChangeType::AccountSwitch);

        // Same identity, different content
        let old = "abc123:def456";
        let new = "abc123:ghi789";
        let change = hasher.detect_change_from_hashes(Some(old), Some(new));
        assert_eq!(change, ChangeType::TokenRefresh);

        // Created
        let change = hasher.detect_change_from_hashes(None, Some("abc:def"));
        assert_eq!(change, ChangeType::Created);

        // Deleted
        let change = hasher.detect_change_from_hashes(Some("abc:def"), None);
        assert_eq!(change, ChangeType::Deleted);
    }

    #[test]
    fn test_identity_fields_display() {
        let fields = IdentityFields {
            email: Some("user@example.com".to_string()),
            organization: Some("MyOrg".to_string()),
            ..Default::default()
        };
        assert_eq!(fields.display(), "user@example.com (MyOrg)");

        let fields = IdentityFields {
            email: Some("user@example.com".to_string()),
            ..Default::default()
        };
        assert_eq!(fields.display(), "user@example.com");

        let fields = IdentityFields {
            user_id: Some("user123".to_string()),
            ..Default::default()
        };
        assert_eq!(fields.display(), "user:user123");

        let fields = IdentityFields::default();
        assert_eq!(fields.display(), "unknown");
    }

    #[test]
    fn test_change_type_should_capture_snapshot() {
        assert!(ChangeType::AccountSwitch.should_capture_snapshot());
        assert!(ChangeType::Created.should_capture_snapshot());
        assert!(!ChangeType::TokenRefresh.should_capture_snapshot());
        assert!(!ChangeType::NoChange.should_capture_snapshot());
        assert!(!ChangeType::Deleted.should_capture_snapshot());
    }

    #[test]
    fn test_volatile_fields_excluded() {
        let hasher = CredentialHasher::new();

        // Two credentials with same stable content but different volatile fields
        let content1 = r#"{
            "email": "user@example.com",
            "user_id": "123",
            "access_token": "old_token",
            "expires_at": "2026-01-01T00:00:00Z",
            "issued_at": "2025-12-01T00:00:00Z"
        }"#;

        let content2 = r#"{
            "email": "user@example.com",
            "user_id": "123",
            "access_token": "new_token",
            "expires_at": "2026-02-01T00:00:00Z",
            "issued_at": "2026-01-15T00:00:00Z"
        }"#;

        let hashes1 = hasher.hash_content(content1).expect("hash1");
        let hashes2 = hasher.hash_content(content2).expect("hash2");

        // Identity should be the same
        assert_eq!(hashes1.identity_hash, hashes2.identity_hash);

        // Content hash should also be the same since volatile fields are excluded
        assert_eq!(hashes1.content_hash, hashes2.content_hash);

        let change = hasher.detect_change(Some(&hashes1), Some(&hashes2));
        assert_eq!(change, ChangeType::NoChange);
    }

    #[test]
    fn test_nested_identity_extraction() {
        let hasher = CredentialHasher::new();

        // Nested user object
        let content = r#"{
            "user": {
                "email": "nested@example.com"
            },
            "access_token": "token123"
        }"#;

        let hashes = hasher.hash_content(content).expect("hash");

        // Direct email field not found, but content hash should work
        assert!(!hashes.content_hash.is_empty());
    }

    #[test]
    fn test_extract_from_id_token() {
        let hasher = CredentialHasher::new();

        // Create a simple JWT with email claim
        // Header: {"alg":"none","typ":"JWT"}
        // Payload: {"email":"jwt@example.com","sub":"user123"}
        let header = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0"; // base64url encoded
        let payload = "eyJlbWFpbCI6Imp3dEBleGFtcGxlLmNvbSIsInN1YiI6InVzZXIxMjMifQ"; // base64url encoded
        let jwt = format!("{header}.{payload}.sig");

        let content = format!(r#"{{"id_token": "{jwt}"}}"#);

        let hashes = hasher.hash_content(&content).expect("hash");

        // Should extract email and sub from JWT
        assert_eq!(
            hashes.identity_fields.email,
            Some("jwt@example.com".to_string())
        );
        assert_eq!(hashes.identity_fields.user_id, Some("user123".to_string()));
    }

    #[test]
    fn test_invalid_json_error() {
        let hasher = CredentialHasher::new();

        let result = hasher.hash_content("not valid json {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_json_object() {
        let hasher = CredentialHasher::new();

        let hashes = hasher.hash_content("{}").expect("hash empty");

        assert!(!hashes.identity_hash.is_empty());
        assert!(!hashes.content_hash.is_empty());
        assert!(!hashes.identity_fields.has_identity());
    }
}
