//! Status page fetching.
//!
//! Fetches provider status from statuspage.io endpoints.

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use super::models::{StatusIndicator, StatusPayload};
use crate::error::{CautError, Result};

/// Default timeout for status fetches.
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// Response from statuspage.io API.
#[derive(Debug, Deserialize)]
struct StatuspageResponse {
    status: StatuspageStatus,
    page: StatuspagePage,
}

#[derive(Debug, Deserialize)]
struct StatuspageStatus {
    indicator: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct StatuspagePage {
    url: String,
    updated_at: Option<DateTime<Utc>>,
}

/// Fetcher for provider status pages.
pub struct StatusFetcher {
    client: Client,
}

impl StatusFetcher {
    /// Create a new status fetcher.
    ///
    /// # Panics
    /// Panics if the HTTP client fails to build (invalid TLS configuration).
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(STATUS_TIMEOUT)
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Fetch status from a statuspage.io-compatible URL.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails due to network issues or timeout.
    pub async fn fetch(&self, base_url: &str) -> Result<StatusPayload> {
        let api_url = format!("{}/api/v2/status.json", base_url.trim_end_matches('/'));

        let response = self.client.get(&api_url).send().await.map_err(|e| {
            if e.is_timeout() {
                CautError::Timeout(STATUS_TIMEOUT.as_secs())
            } else {
                CautError::Network(e.to_string())
            }
        })?;

        if !response.status().is_success() {
            return Ok(StatusPayload {
                indicator: StatusIndicator::Unknown,
                description: Some(format!("HTTP {}", response.status())),
                updated_at: None,
                url: base_url.to_string(),
            });
        }

        let data: StatuspageResponse = response
            .json()
            .await
            .map_err(|e| CautError::ParseResponse(e.to_string()))?;

        Ok(StatusPayload {
            indicator: StatusIndicator::from_statuspage(&data.status.indicator),
            description: Some(data.status.description),
            updated_at: data.page.updated_at,
            url: data.page.url,
        })
    }
}

impl Default for StatusFetcher {
    fn default() -> Self {
        Self::new()
    }
}
