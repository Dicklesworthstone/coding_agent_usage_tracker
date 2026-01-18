//! HTTP client utilities.
//!
//! Provides a shared HTTP client for all provider fetchers.

use std::time::Duration;

use reqwest::{Client, ClientBuilder};

use crate::error::{CautError, Result};

/// Default timeout for HTTP requests.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for status page requests.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// Build a configured HTTP client.
///
/// # Errors
///
/// Returns error if client construction fails.
pub fn build_client(timeout: Duration) -> Result<Client> {
    ClientBuilder::new()
        .timeout(timeout)
        .user_agent(format!("caut/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| CautError::Network(e.to_string()))
}

/// Get or create a default HTTP client.
pub fn default_client() -> Result<Client> {
    build_client(DEFAULT_TIMEOUT)
}

/// Fetch JSON from a URL.
///
/// # Errors
///
/// Returns error on network failure or JSON parse failure.
pub async fn fetch_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            CautError::Timeout(DEFAULT_TIMEOUT.as_secs())
        } else {
            CautError::Network(e.to_string())
        }
    })?;

    if !response.status().is_success() {
        return Err(CautError::Network(format!(
            "HTTP {} from {}",
            response.status(),
            url
        )));
    }

    response
        .json()
        .await
        .map_err(|e| CautError::ParseResponse(e.to_string()))
}
