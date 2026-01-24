//! Codex (OpenAI) provider implementation.
//!
//! Supports:
//! - Web dashboard scraping (macOS only)
//! - CLI local config (auth.json with JWT tokens)
//! - CLI RPC
//!
//! Source labels: `openai-web`, `codex-cli`
//!
//! The CLI local config reads identity and subscription info from
//! `~/.codex/auth.json`, which contains JWT tokens with embedded claims
//! about the user's plan type and subscription status.

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use serde::Deserialize;

use crate::core::cli_runner::{CLI_TIMEOUT, run_command, run_json_command};
use crate::core::fetch_plan::{FetchKind, FetchPlan, FetchStrategy};
use crate::core::models::{CreditsSnapshot, ProviderIdentity, RateWindow, UsageSnapshot};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};

/// Source label for web dashboard.
pub const SOURCE_WEB: &str = "openai-web";

/// Source label for CLI.
pub const SOURCE_CLI: &str = "codex-cli";

/// CLI binary name.
const CLI_NAME: &str = "codex";

// =============================================================================
// Fetch Plan
// =============================================================================

/// Create fetch plan for Codex.
#[must_use]
pub fn fetch_plan() -> FetchPlan {
    FetchPlan::new(
        Provider::Codex,
        vec![
            FetchStrategy {
                id: "codex-web-dashboard",
                kind: FetchKind::WebDashboard,
                is_available: || {
                    // Web dashboard requires macOS with cookies
                    cfg!(target_os = "macos")
                },
                should_fallback: |_| true,
            },
            FetchStrategy {
                id: "codex-cli-rpc",
                kind: FetchKind::Cli,
                is_available: is_cli_available,
                should_fallback: |_| false,
            },
        ],
    )
}

/// Check if the Codex CLI is available.
fn is_cli_available() -> bool {
    which::which(CLI_NAME).is_ok()
}

// =============================================================================
// Local Config Types
// =============================================================================

/// Get the Codex config directory path.
fn get_codex_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".codex"))
}

/// Auth.json structure from ~/.codex/auth.json
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CodexAuthJson {
    #[serde(rename = "OPENAI_API_KEY")]
    #[serde(default)]
    openai_api_key: Option<String>,
    #[serde(default)]
    last_refresh: Option<String>,
    #[serde(default)]
    tokens: Option<CodexAuthTokens>,
}

/// OAuth tokens structure in auth.json
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CodexAuthTokens {
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

/// JWT claims from the id_token (decoded from base64).
/// Contains OpenAI-specific auth information.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JwtClaims {
    /// User email
    #[serde(default)]
    email: Option<String>,
    /// Email verified flag
    #[serde(default)]
    email_verified: Option<bool>,
    /// OpenAI-specific auth claims
    #[serde(default, rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaims>,
}

/// OpenAI-specific claims embedded in JWT.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAiAuthClaims {
    /// ChatGPT account ID
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    /// Plan type (e.g., "pro", "plus", "free")
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
    /// User ID
    #[serde(default)]
    chatgpt_user_id: Option<String>,
    /// Subscription active start date
    #[serde(default)]
    chatgpt_subscription_active_start: Option<String>,
    /// Subscription active until date
    #[serde(default)]
    chatgpt_subscription_active_until: Option<String>,
    /// Last subscription check timestamp
    #[serde(default)]
    chatgpt_subscription_last_checked: Option<String>,
    /// Organizations the user belongs to
    #[serde(default)]
    organizations: Option<Vec<OpenAiOrganization>>,
}

/// Organization info from JWT claims.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAiOrganization {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    is_default: Option<bool>,
}

/// Read auth info from local ~/.codex/auth.json
fn read_local_auth() -> Option<CodexAuthJson> {
    let codex_dir = get_codex_dir()?;
    let auth_path = codex_dir.join("auth.json");

    if !auth_path.exists() {
        tracing::debug!("Codex auth.json not found at {:?}", auth_path);
        return None;
    }

    let content = fs::read_to_string(&auth_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Check if user is authenticated with Codex
fn is_authenticated() -> bool {
    read_local_auth()
        .map(|auth| auth.openai_api_key.is_some() || auth.tokens.is_some())
        .unwrap_or(false)
}

/// Decode JWT payload (the middle part between the two dots).
/// JWTs are base64url encoded, so we need to handle URL-safe base64.
fn decode_jwt_payload(token: &str) -> Option<JwtClaims> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        tracing::debug!("Invalid JWT format: expected 3 parts");
        return None;
    }

    let payload = parts[1];

    // Base64url decode: replace - with + and _ with /
    let mut payload_std = payload.replace('-', "+").replace('_', "/");

    // Add padding if needed
    let padding = (4 - payload_std.len() % 4) % 4;
    for _ in 0..padding {
        payload_std.push('=');
    }

    // Decode base64
    let decoded = match base64_decode(&payload_std) {
        Some(d) => d,
        None => {
            tracing::debug!("Failed to decode JWT payload as base64");
            return None;
        }
    };

    // Parse JSON
    match serde_json::from_slice::<JwtClaims>(&decoded) {
        Ok(claims) => Some(claims),
        Err(e) => {
            tracing::debug!(error = %e, "Failed to parse JWT claims as JSON");
            None
        }
    }
}

/// Simple base64 decoder (standard alphabet with padding).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits_collected = 0;

    for c in input.bytes() {
        if c == b'=' {
            break;
        }

        let value = ALPHABET.iter().position(|&x| x == c)?;
        buffer = (buffer << 6) | (value as u32);
        bits_collected += 6;

        if bits_collected >= 8 {
            bits_collected -= 8;
            result.push((buffer >> bits_collected) as u8);
            buffer &= (1 << bits_collected) - 1;
        }
    }

    Some(result)
}

/// Extract identity info from local auth.json (including JWT claims).
fn get_local_identity() -> Option<(ProviderIdentity, Option<SubscriptionInfo>)> {
    let auth = read_local_auth()?;

    // Try to extract from JWT token first
    if let Some(tokens) = &auth.tokens {
        if let Some(id_token) = &tokens.id_token {
            if let Some(claims) = decode_jwt_payload(id_token) {
                let openai_auth = claims.openai_auth.as_ref();

                // Extract organization name from default org
                let org_name = openai_auth.and_then(|a| {
                    a.organizations.as_ref().and_then(|orgs| {
                        orgs.iter()
                            .find(|o| o.is_default == Some(true))
                            .and_then(|o| o.title.clone())
                    })
                });

                let identity = ProviderIdentity {
                    account_email: claims.email.clone(),
                    account_organization: org_name,
                    login_method: Some("oauth".to_string()),
                };

                // Extract subscription info
                let subscription = openai_auth.map(|a| SubscriptionInfo {
                    plan_type: a.chatgpt_plan_type.clone(),
                    active_start: a.chatgpt_subscription_active_start.clone(),
                    active_until: a.chatgpt_subscription_active_until.clone(),
                    last_checked: a.chatgpt_subscription_last_checked.clone(),
                });

                tracing::debug!(
                    email = ?claims.email,
                    plan = ?openai_auth.and_then(|a| a.chatgpt_plan_type.as_ref()),
                    "Extracted identity from JWT"
                );

                return Some((identity, subscription));
            }
        }

        // Fallback: just return account_id if available
        if let Some(account_id) = &tokens.account_id {
            return Some((
                ProviderIdentity {
                    account_email: None,
                    account_organization: Some(account_id.clone()),
                    login_method: Some("oauth-partial".to_string()),
                },
                None,
            ));
        }
    }

    // API key auth (no identity info available)
    if auth.openai_api_key.is_some() {
        return Some((
            ProviderIdentity {
                account_email: None,
                account_organization: None,
                login_method: Some("api-key".to_string()),
            },
            None,
        ));
    }

    None
}

/// Subscription information extracted from JWT.
#[derive(Debug)]
#[allow(dead_code)]
struct SubscriptionInfo {
    plan_type: Option<String>,
    active_start: Option<String>,
    active_until: Option<String>,
    last_checked: Option<String>,
}

// =============================================================================
// CLI Response Types
// =============================================================================

/// Response from `codex --rate-limit` or similar.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexRateLimitResponse {
    #[serde(default)]
    rate_limit: Option<CodexRateLimit>,
    #[serde(default)]
    credits: Option<CodexCredits>,
    #[serde(default)]
    user: Option<CodexUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexRateLimit {
    #[serde(default)]
    remaining_percent: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
    #[serde(default)]
    weekly_remaining_percent: Option<f64>,
    #[serde(default)]
    weekly_resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexCredits {
    #[serde(default)]
    remaining: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct CodexUser {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    plan: Option<String>,
}

// =============================================================================
// Fetch Implementations
// =============================================================================

/// Fetch usage via web dashboard.
///
/// This requires macOS with browser cookies available.
pub async fn fetch_web_dashboard() -> Result<UsageSnapshot> {
    #[cfg(not(target_os = "macos"))]
    {
        Err(CautError::UnsupportedSource {
            provider: "codex".to_string(),
            source_type: "web".to_string(),
        })
    }

    #[cfg(target_os = "macos")]
    {
        // TODO: Implement actual web dashboard scraping
        // This would involve:
        // 1. Reading browser cookies
        // 2. Making authenticated request to OpenAI dashboard
        // 3. Parsing the response
        Err(CautError::FetchFailed {
            provider: "codex".to_string(),
            reason: "Web dashboard scraping not yet implemented".to_string(),
        })
    }
}

/// Fetch usage via CLI.
///
/// Reads identity and subscription info from local auth.json file.
/// The auth.json contains JWT tokens with embedded claims about
/// the user's plan type and subscription status.
///
/// Note: The Codex CLI doesn't expose rate limit commands directly.
/// Rate limit data would need to come from OpenAI API or web dashboard.
pub async fn fetch_cli() -> Result<UsageSnapshot> {
    // First check version to confirm CLI is working
    let version = get_cli_version().await.ok();
    let now = Utc::now();

    tracing::debug!(
        ?version,
        cli_available = is_cli_available(),
        authenticated = is_authenticated(),
        "Codex CLI fetch starting"
    );

    // Try JSON output if available (Codex CLI may add rate-limit commands in the future)
    if let Ok(response) = try_json_rate_limit().await {
        return parse_rate_limit_response(response, version);
    }

    // Extract identity from local auth.json (includes JWT decoding)
    let (identity, subscription) = match get_local_identity() {
        Some((id, sub)) => {
            tracing::debug!(
                email = ?id.account_email,
                org = ?id.account_organization,
                login_method = ?id.login_method,
                plan = ?sub.as_ref().and_then(|s| s.plan_type.as_ref()),
                "Extracted identity from local auth.json"
            );
            (Some(id), sub)
        }
        None => {
            tracing::debug!("No identity found in local auth.json");
            (
                Some(ProviderIdentity {
                    account_email: None,
                    account_organization: None,
                    login_method: Some("cli-unauthenticated".to_string()),
                }),
                None,
            )
        }
    };

    // Log subscription info if available
    if let Some(ref sub) = subscription {
        tracing::info!(
            plan = ?sub.plan_type,
            until = ?sub.active_until,
            "Codex subscription info"
        );
    }

    // Return snapshot with identity info
    // Note: Rate limit data is not available via CLI - would need API access
    Ok(UsageSnapshot {
        primary: None,
        secondary: None,
        tertiary: None,
        updated_at: now,
        identity,
    })
}

/// Try to get rate limit via JSON output.
async fn try_json_rate_limit() -> Result<CodexRateLimitResponse> {
    // Try various command patterns that CLI tools commonly use
    // The actual command depends on the Codex CLI implementation
    let commands = [
        &["rate-limit", "--json"][..],
        &["status", "--json"][..],
        &["usage", "--json"][..],
    ];

    for args in commands {
        if let Ok(response) =
            run_json_command::<CodexRateLimitResponse>(CLI_NAME, args, CLI_TIMEOUT).await
        {
            return Ok(response);
        }
    }

    Err(CautError::FetchFailed {
        provider: "codex".to_string(),
        reason: "No rate limit command found".to_string(),
    })
}

/// Parse rate limit response into UsageSnapshot.
fn parse_rate_limit_response(
    response: CodexRateLimitResponse,
    _version: Option<String>,
) -> Result<UsageSnapshot> {
    let now = Utc::now();

    let primary = response.rate_limit.as_ref().and_then(|rl| {
        rl.remaining_percent.map(|pct| {
            let used = 100.0 - pct;
            RateWindow {
                used_percent: used,
                window_minutes: None,
                resets_at: rl.resets_at.as_ref().and_then(|s| s.parse().ok()),
                reset_description: None,
            }
        })
    });

    let secondary = response.rate_limit.as_ref().and_then(|rl| {
        rl.weekly_remaining_percent.map(|pct| {
            let used = 100.0 - pct;
            RateWindow {
                used_percent: used,
                window_minutes: Some(10080), // 7 days in minutes
                resets_at: rl.weekly_resets_at.as_ref().and_then(|s| s.parse().ok()),
                reset_description: None,
            }
        })
    });

    let identity = Some(ProviderIdentity {
        account_email: response.user.as_ref().and_then(|u| u.email.clone()),
        account_organization: None,
        login_method: Some("cli".to_string()),
    });

    Ok(UsageSnapshot {
        primary,
        secondary,
        tertiary: None,
        updated_at: now,
        identity,
    })
}

/// Get the CLI version.
async fn get_cli_version() -> Result<String> {
    let output = run_command(CLI_NAME, &["--version"], CLI_TIMEOUT).await?;

    if output.success() {
        // Parse version from output like "codex 0.6.0" or "0.6.0"
        let version = output
            .stdout
            .trim()
            .split_whitespace()
            .last()
            .unwrap_or("unknown")
            .to_string();
        Ok(version)
    } else {
        Err(CautError::FetchFailed {
            provider: "codex".to_string(),
            reason: "Failed to get version".to_string(),
        })
    }
}

/// Get credits information.
pub async fn fetch_credits() -> Result<CreditsSnapshot> {
    // Try to get credits from the CLI
    if let Ok(response) = try_json_rate_limit().await {
        if let Some(credits) = response.credits {
            if let Some(remaining) = credits.remaining {
                return Ok(CreditsSnapshot {
                    remaining,
                    events: vec![],
                    updated_at: Utc::now(),
                });
            }
        }
    }

    Err(CautError::FetchFailed {
        provider: "codex".to_string(),
        reason: "No credits data available".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base64_url_encode(input: &str) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let bytes = input.as_bytes();
        let mut out = String::new();

        let mut i = 0;
        while i < bytes.len() {
            let b0 = u32::from(bytes[i]);
            let b1 = if i + 1 < bytes.len() {
                u32::from(bytes[i + 1])
            } else {
                0
            };
            let b2 = if i + 2 < bytes.len() {
                u32::from(bytes[i + 2])
            } else {
                0
            };

            let triple = (b0 << 16) | (b1 << 8) | b2;

            let idx0 = ((triple >> 18) & 0x3f) as usize;
            let idx1 = ((triple >> 12) & 0x3f) as usize;
            let idx2 = ((triple >> 6) & 0x3f) as usize;
            let idx3 = (triple & 0x3f) as usize;

            out.push(ALPHABET[idx0] as char);
            out.push(ALPHABET[idx1] as char);
            if i + 1 < bytes.len() {
                out.push(ALPHABET[idx2] as char);
            } else {
                out.push('=');
            }
            if i + 2 < bytes.len() {
                out.push(ALPHABET[idx3] as char);
            } else {
                out.push('=');
            }

            i += 3;
        }

        out.replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_string()
    }

    #[test]
    fn base64_decode_basic() {
        let decoded = base64_decode("SGVsbG8=").expect("decode");
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn decode_jwt_payload_parses_claims() {
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let payload_json = r#"{"email":"test@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro","organizations":[{"title":"Acme","is_default":true}]}}"#;
        let payload = base64_url_encode(payload_json);
        let token = format!("{header}.{payload}.signature");

        let claims = decode_jwt_payload(&token).expect("claims");
        assert_eq!(claims.email.as_deref(), Some("test@example.com"));

        let openai_auth = claims.openai_auth.expect("openai_auth");
        assert_eq!(openai_auth.chatgpt_plan_type.as_deref(), Some("pro"));
        let org = openai_auth
            .organizations
            .unwrap()
            .into_iter()
            .find(|o| o.is_default == Some(true))
            .and_then(|o| o.title);
        assert_eq!(org.as_deref(), Some("Acme"));
    }

    #[test]
    fn decode_jwt_payload_invalid_format_returns_none() {
        assert!(decode_jwt_payload("not-a-jwt").is_none());
    }

    #[test]
    fn parse_rate_limit_response_sets_windows() {
        let response = CodexRateLimitResponse {
            rate_limit: Some(CodexRateLimit {
                remaining_percent: Some(60.0),
                resets_at: Some("2026-01-18T00:00:00Z".to_string()),
                weekly_remaining_percent: Some(20.0),
                weekly_resets_at: Some("2026-01-25T00:00:00Z".to_string()),
            }),
            credits: None,
            user: Some(CodexUser {
                email: Some("user@example.com".to_string()),
                plan: Some("pro".to_string()),
            }),
        };

        let snapshot = parse_rate_limit_response(response, None).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        let secondary = snapshot.secondary.expect("secondary");

        assert!((primary.used_percent - 40.0).abs() < f64::EPSILON);
        assert!((secondary.used_percent - 80.0).abs() < f64::EPSILON);
        assert_eq!(
            snapshot.identity.and_then(|i| i.account_email).as_deref(),
            Some("user@example.com")
        );
        assert!(primary.resets_at.is_some());
        assert!(secondary.resets_at.is_some());
    }

    // =========================================================================
    // Additional base64 decode tests
    // =========================================================================

    #[test]
    fn base64_decode_empty_string() {
        let decoded = base64_decode("").expect("decode empty");
        assert!(decoded.is_empty());
    }

    #[test]
    fn base64_decode_with_padding_variations() {
        // No padding needed (multiple of 4)
        let decoded = base64_decode("YWJj").expect("decode abc");
        assert_eq!(decoded, b"abc");

        // Single padding
        let decoded = base64_decode("YWI=").expect("decode ab");
        assert_eq!(decoded, b"ab");

        // Double padding
        let decoded = base64_decode("YQ==").expect("decode a");
        assert_eq!(decoded, b"a");
    }

    #[test]
    fn base64_decode_invalid_character_returns_none() {
        // Invalid character that's not in base64 alphabet
        assert!(base64_decode("!!!").is_none());
        assert!(base64_decode("abc$def").is_none());
    }

    #[test]
    fn base64_decode_longer_text() {
        // "The quick brown fox jumps over the lazy dog"
        let encoded = "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wcyBvdmVyIHRoZSBsYXp5IGRvZw==";
        let decoded = base64_decode(encoded).expect("decode long text");
        assert_eq!(
            String::from_utf8(decoded).unwrap(),
            "The quick brown fox jumps over the lazy dog"
        );
    }

    // =========================================================================
    // Additional JWT decode tests
    // =========================================================================

    #[test]
    fn decode_jwt_payload_too_few_parts() {
        assert!(decode_jwt_payload("only.two").is_none());
        assert!(decode_jwt_payload("just-one-part").is_none());
        assert!(decode_jwt_payload("").is_none());
    }

    #[test]
    fn decode_jwt_payload_too_many_parts() {
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let payload = base64_url_encode(r#"{"email":"test@example.com"}"#);
        let token = format!("{header}.{payload}.sig.extra.parts");
        assert!(decode_jwt_payload(&token).is_none());
    }

    #[test]
    fn decode_jwt_payload_invalid_base64_payload() {
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let token = format!("{header}.!!invalid-base64!!.signature");
        assert!(decode_jwt_payload(&token).is_none());
    }

    #[test]
    fn decode_jwt_payload_invalid_json_payload() {
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let payload = base64_url_encode("not valid json {{{");
        let token = format!("{header}.{payload}.signature");
        assert!(decode_jwt_payload(&token).is_none());
    }

    #[test]
    fn decode_jwt_payload_minimal_claims() {
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let payload = base64_url_encode(r#"{}"#);
        let token = format!("{header}.{payload}.signature");

        let claims = decode_jwt_payload(&token).expect("claims");
        assert!(claims.email.is_none());
        assert!(claims.openai_auth.is_none());
    }

    #[test]
    fn decode_jwt_payload_with_multiple_organizations() {
        let header = base64_url_encode(r#"{"alg":"HS256"}"#);
        let payload_json = r#"{
            "email":"multi@example.com",
            "https://api.openai.com/auth":{
                "chatgpt_plan_type":"plus",
                "organizations":[
                    {"title":"Org1","is_default":false},
                    {"title":"DefaultOrg","is_default":true},
                    {"title":"Org3","is_default":false}
                ]
            }
        }"#;
        let payload = base64_url_encode(payload_json);
        let token = format!("{header}.{payload}.signature");

        let claims = decode_jwt_payload(&token).expect("claims");
        assert_eq!(claims.email.as_deref(), Some("multi@example.com"));

        let openai_auth = claims.openai_auth.expect("openai_auth");
        let orgs = openai_auth.organizations.expect("organizations");
        assert_eq!(orgs.len(), 3);

        let default_org = orgs.iter().find(|o| o.is_default == Some(true));
        assert_eq!(
            default_org.and_then(|o| o.title.as_deref()),
            Some("DefaultOrg")
        );
    }

    // =========================================================================
    // Rate limit response edge case tests
    // =========================================================================

    #[test]
    fn parse_rate_limit_response_empty_response() {
        let response = CodexRateLimitResponse {
            rate_limit: None,
            credits: None,
            user: None,
        };

        let snapshot = parse_rate_limit_response(response, None).expect("snapshot");
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.tertiary.is_none());
        // Identity should still be set with login_method
        let identity = snapshot.identity.expect("identity");
        assert_eq!(identity.login_method.as_deref(), Some("cli"));
    }

    #[test]
    fn parse_rate_limit_response_only_primary() {
        let response = CodexRateLimitResponse {
            rate_limit: Some(CodexRateLimit {
                remaining_percent: Some(75.0),
                resets_at: None,
                weekly_remaining_percent: None,
                weekly_resets_at: None,
            }),
            credits: None,
            user: None,
        };

        let snapshot = parse_rate_limit_response(response, None).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 25.0).abs() < f64::EPSILON);
        assert!(primary.resets_at.is_none());
        assert!(snapshot.secondary.is_none());
    }

    #[test]
    fn parse_rate_limit_response_only_secondary() {
        let response = CodexRateLimitResponse {
            rate_limit: Some(CodexRateLimit {
                remaining_percent: None,
                resets_at: None,
                weekly_remaining_percent: Some(50.0),
                weekly_resets_at: Some("2026-01-25T12:00:00Z".to_string()),
            }),
            credits: None,
            user: None,
        };

        let snapshot = parse_rate_limit_response(response, None).expect("snapshot");
        assert!(snapshot.primary.is_none());
        let secondary = snapshot.secondary.expect("secondary");
        assert!((secondary.used_percent - 50.0).abs() < f64::EPSILON);
        assert_eq!(secondary.window_minutes, Some(10080)); // 7 days
        assert!(secondary.resets_at.is_some());
    }

    #[test]
    fn parse_rate_limit_response_invalid_resets_at_format() {
        let response = CodexRateLimitResponse {
            rate_limit: Some(CodexRateLimit {
                remaining_percent: Some(80.0),
                resets_at: Some("not-a-valid-timestamp".to_string()),
                weekly_remaining_percent: None,
                weekly_resets_at: None,
            }),
            credits: None,
            user: None,
        };

        let snapshot = parse_rate_limit_response(response, None).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        assert!((primary.used_percent - 20.0).abs() < f64::EPSILON);
        // Invalid timestamp should result in None
        assert!(primary.resets_at.is_none());
    }

    #[test]
    fn parse_rate_limit_response_boundary_percentages() {
        // Test 0% remaining (100% used)
        let response = CodexRateLimitResponse {
            rate_limit: Some(CodexRateLimit {
                remaining_percent: Some(0.0),
                resets_at: None,
                weekly_remaining_percent: Some(100.0),
                weekly_resets_at: None,
            }),
            credits: None,
            user: None,
        };

        let snapshot = parse_rate_limit_response(response, None).expect("snapshot");
        let primary = snapshot.primary.expect("primary");
        let secondary = snapshot.secondary.expect("secondary");
        assert!((primary.used_percent - 100.0).abs() < f64::EPSILON);
        assert!((secondary.used_percent - 0.0).abs() < f64::EPSILON);
    }

    // =========================================================================
    // Fetch plan tests
    // =========================================================================

    #[test]
    fn fetch_plan_has_correct_provider() {
        let plan = fetch_plan();
        assert_eq!(plan.provider, Provider::Codex);
    }

    #[test]
    fn fetch_plan_has_expected_strategies() {
        let plan = fetch_plan();
        assert_eq!(plan.strategies.len(), 2);

        // First strategy should be web dashboard
        assert_eq!(plan.strategies[0].id, "codex-web-dashboard");
        assert!(matches!(plan.strategies[0].kind, FetchKind::WebDashboard));

        // Second strategy should be CLI
        assert_eq!(plan.strategies[1].id, "codex-cli-rpc");
        assert!(matches!(plan.strategies[1].kind, FetchKind::Cli));
    }

    #[test]
    fn fetch_plan_web_dashboard_availability_checks_os() {
        let plan = fetch_plan();
        let web_strategy = &plan.strategies[0];

        // On non-macOS, web dashboard should not be available
        #[cfg(not(target_os = "macos"))]
        assert!(!(web_strategy.is_available)());

        // On macOS, it should be available (regardless of cookies)
        #[cfg(target_os = "macos")]
        assert!((web_strategy.is_available)());
    }

    #[test]
    fn fetch_plan_cli_fallback_behavior() {
        let plan = fetch_plan();

        // Web dashboard should fallback on any error
        let web_strategy = &plan.strategies[0];
        assert!((web_strategy.should_fallback)(
            &crate::error::CautError::FetchFailed {
                provider: "codex".to_string(),
                reason: "test".to_string(),
            }
        ));

        // CLI should not fallback (it's the last resort)
        let cli_strategy = &plan.strategies[1];
        assert!(!(cli_strategy.should_fallback)(
            &crate::error::CautError::FetchFailed {
                provider: "codex".to_string(),
                reason: "test".to_string(),
            }
        ));
    }

    // =========================================================================
    // Auth JSON parsing tests
    // =========================================================================

    #[test]
    fn parse_auth_json_with_api_key_only() {
        let json = r#"{"OPENAI_API_KEY": "sk-test-key-123"}"#;
        let auth: CodexAuthJson = serde_json::from_str(json).expect("parse");
        assert_eq!(auth.openai_api_key.as_deref(), Some("sk-test-key-123"));
        assert!(auth.tokens.is_none());
    }

    #[test]
    fn parse_auth_json_with_oauth_tokens() {
        let json = r#"{
            "tokens": {
                "id_token": "test.jwt.token",
                "access_token": "acc_123",
                "refresh_token": "ref_456",
                "account_id": "acct_789"
            }
        }"#;
        let auth: CodexAuthJson = serde_json::from_str(json).expect("parse");
        assert!(auth.openai_api_key.is_none());
        let tokens = auth.tokens.expect("tokens");
        assert_eq!(tokens.id_token.as_deref(), Some("test.jwt.token"));
        assert_eq!(tokens.account_id.as_deref(), Some("acct_789"));
    }

    #[test]
    fn parse_auth_json_with_both_methods() {
        let json = r#"{
            "OPENAI_API_KEY": "sk-test",
            "tokens": {
                "id_token": "jwt.here",
                "account_id": "acct_123"
            }
        }"#;
        let auth: CodexAuthJson = serde_json::from_str(json).expect("parse");
        assert!(auth.openai_api_key.is_some());
        assert!(auth.tokens.is_some());
    }

    #[test]
    fn parse_auth_json_empty() {
        let json = r#"{}"#;
        let auth: CodexAuthJson = serde_json::from_str(json).expect("parse");
        assert!(auth.openai_api_key.is_none());
        assert!(auth.tokens.is_none());
        assert!(auth.last_refresh.is_none());
    }

    #[test]
    fn parse_auth_json_with_last_refresh() {
        let json = r#"{"last_refresh": "2026-01-18T10:00:00.000Z"}"#;
        let auth: CodexAuthJson = serde_json::from_str(json).expect("parse");
        assert_eq!(
            auth.last_refresh.as_deref(),
            Some("2026-01-18T10:00:00.000Z")
        );
    }

    // =========================================================================
    // JWT claims structure tests
    // =========================================================================

    #[test]
    fn parse_jwt_claims_with_all_fields() {
        let json = r#"{
            "email": "user@example.com",
            "email_verified": true,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_123",
                "chatgpt_plan_type": "pro",
                "chatgpt_user_id": "user_456",
                "chatgpt_subscription_active_start": "2026-01-01T00:00:00Z",
                "chatgpt_subscription_active_until": "2026-02-01T00:00:00Z",
                "chatgpt_subscription_last_checked": "2026-01-18T12:00:00Z",
                "organizations": [
                    {"id": "org_1", "title": "Personal", "role": "owner", "is_default": true},
                    {"id": "org_2", "title": "Work", "role": "member", "is_default": false}
                ]
            }
        }"#;
        let claims: JwtClaims = serde_json::from_str(json).expect("parse claims");

        assert_eq!(claims.email.as_deref(), Some("user@example.com"));
        assert_eq!(claims.email_verified, Some(true));

        let auth = claims.openai_auth.expect("openai_auth");
        assert_eq!(auth.chatgpt_plan_type.as_deref(), Some("pro"));
        assert_eq!(auth.chatgpt_account_id.as_deref(), Some("acct_123"));

        let orgs = auth.organizations.expect("organizations");
        assert_eq!(orgs.len(), 2);

        let default_org = orgs.iter().find(|o| o.is_default == Some(true));
        assert!(default_org.is_some());
        assert_eq!(default_org.unwrap().title.as_deref(), Some("Personal"));
    }

    #[test]
    fn parse_jwt_claims_minimal() {
        let json = r#"{"email": "test@test.com"}"#;
        let claims: JwtClaims = serde_json::from_str(json).expect("parse claims");
        assert_eq!(claims.email.as_deref(), Some("test@test.com"));
        assert!(claims.openai_auth.is_none());
    }

    #[test]
    fn parse_jwt_claims_empty_openai_auth() {
        let json = r#"{"https://api.openai.com/auth": {}}"#;
        let claims: JwtClaims = serde_json::from_str(json).expect("parse claims");
        assert!(claims.email.is_none());
        let auth = claims.openai_auth.expect("openai_auth");
        assert!(auth.chatgpt_plan_type.is_none());
        assert!(auth.organizations.is_none());
    }

    // =========================================================================
    // Real fixture JWT decode test
    // =========================================================================

    #[test]
    fn decode_fixture_jwt_token() {
        // This JWT is from tests/fixtures/codex/auth_oauth.json
        // It contains: email=user@example.com, plan=pro, org=Personal
        let token = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfcGxhbl90eXBlIjoicHJvIiwiY2hhdGdwdF9zdWJzY3JpcHRpb25fYWN0aXZlX3VudGlsIjoiMjAyNi0wMi0wMVQwMDowMDowMFoiLCJvcmdhbml6YXRpb25zIjpbeyJ0aXRsZSI6IlBlcnNvbmFsIiwiaXNfZGVmYXVsdCI6dHJ1ZX1dfX0.sig";

        let claims = decode_jwt_payload(token).expect("decode fixture JWT");

        assert_eq!(claims.email.as_deref(), Some("user@example.com"));
        assert_eq!(claims.email_verified, Some(true));

        let openai_auth = claims.openai_auth.expect("openai_auth present");
        assert_eq!(openai_auth.chatgpt_plan_type.as_deref(), Some("pro"));
        assert_eq!(
            openai_auth.chatgpt_subscription_active_until.as_deref(),
            Some("2026-02-01T00:00:00Z")
        );

        let orgs = openai_auth.organizations.expect("orgs present");
        assert_eq!(orgs.len(), 1);
        assert_eq!(orgs[0].title.as_deref(), Some("Personal"));
        assert_eq!(orgs[0].is_default, Some(true));
    }

    // =========================================================================
    // Subscription info extraction tests
    // =========================================================================

    #[test]
    fn subscription_info_from_claims() {
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let payload_json = r#"{
            "email": "sub@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "plus",
                "chatgpt_subscription_active_start": "2026-01-01T00:00:00Z",
                "chatgpt_subscription_active_until": "2026-12-31T23:59:59Z",
                "chatgpt_subscription_last_checked": "2026-01-18T10:00:00Z"
            }
        }"#;
        let payload = base64_url_encode(payload_json);
        let token = format!("{header}.{payload}.sig");

        let claims = decode_jwt_payload(&token).expect("claims");
        let auth = claims.openai_auth.expect("auth");

        // These would be extracted into SubscriptionInfo
        assert_eq!(auth.chatgpt_plan_type.as_deref(), Some("plus"));
        assert_eq!(
            auth.chatgpt_subscription_active_start.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            auth.chatgpt_subscription_active_until.as_deref(),
            Some("2026-12-31T23:59:59Z")
        );
    }

    // =========================================================================
    // Organization structure tests
    // =========================================================================

    #[test]
    fn parse_organization_with_all_fields() {
        let json = r#"{
            "id": "org_abc123",
            "title": "My Company",
            "role": "admin",
            "is_default": false
        }"#;
        let org: OpenAiOrganization = serde_json::from_str(json).expect("parse org");
        assert_eq!(org.id.as_deref(), Some("org_abc123"));
        assert_eq!(org.title.as_deref(), Some("My Company"));
        assert_eq!(org.role.as_deref(), Some("admin"));
        assert_eq!(org.is_default, Some(false));
    }

    #[test]
    fn parse_organization_minimal() {
        let json = r#"{"title": "Test"}"#;
        let org: OpenAiOrganization = serde_json::from_str(json).expect("parse org");
        assert!(org.id.is_none());
        assert_eq!(org.title.as_deref(), Some("Test"));
        assert!(org.is_default.is_none());
    }

    // =========================================================================
    // Rate limit response deserialization tests
    // =========================================================================

    #[test]
    fn parse_rate_limit_response_camel_case() {
        let json = r#"{
            "rateLimit": {
                "remainingPercent": 65.5,
                "resetsAt": "2026-01-18T15:00:00Z",
                "weeklyRemainingPercent": 30.0,
                "weeklyResetsAt": "2026-01-25T00:00:00Z"
            },
            "credits": {
                "remaining": 50.25
            },
            "user": {
                "email": "camel@example.com",
                "plan": "enterprise"
            }
        }"#;
        let response: CodexRateLimitResponse = serde_json::from_str(json).expect("parse");

        let rl = response.rate_limit.expect("rate_limit");
        assert!((rl.remaining_percent.unwrap() - 65.5).abs() < f64::EPSILON);
        assert!((rl.weekly_remaining_percent.unwrap() - 30.0).abs() < f64::EPSILON);

        let credits = response.credits.expect("credits");
        assert!((credits.remaining.unwrap() - 50.25).abs() < f64::EPSILON);

        let user = response.user.expect("user");
        assert_eq!(user.email.as_deref(), Some("camel@example.com"));
        assert_eq!(user.plan.as_deref(), Some("enterprise"));
    }

    #[test]
    fn parse_rate_limit_response_null_fields() {
        let json = r#"{
            "rateLimit": {
                "remainingPercent": null,
                "resetsAt": null,
                "weeklyRemainingPercent": null,
                "weeklyResetsAt": null
            },
            "credits": null,
            "user": null
        }"#;
        let response: CodexRateLimitResponse = serde_json::from_str(json).expect("parse");

        let rl = response.rate_limit.expect("rate_limit");
        assert!(rl.remaining_percent.is_none());
        assert!(rl.resets_at.is_none());
        assert!(response.credits.is_none());
        assert!(response.user.is_none());
    }

    // =========================================================================
    // Source label constants tests
    // =========================================================================

    #[test]
    fn source_labels_are_correct() {
        assert_eq!(SOURCE_WEB, "openai-web");
        assert_eq!(SOURCE_CLI, "codex-cli");
    }
}
