//! Query client for the background caut server.
//!
//! Implements `caut query` which connects to a running `caut serve`
//! instance and prints the response as JSON to stdout.

use crate::cli::args::QueryArgs;
use crate::error::{CautError, Result};
use crate::storage::AppPaths;
use tokio::time::Duration;

/// Execute the `query` command: fetch data from a running caut server.
///
/// # Errors
/// Returns an error if the server is unreachable, the endpoint is invalid,
/// or the response cannot be read.
pub async fn execute(args: &QueryArgs, pretty: bool) -> Result<()> {
    let endpoint = normalize_endpoint(&args.endpoint);

    let url = format!("http://{}:{}{endpoint}", args.host, args.port);

    tracing::debug!(%url, "Querying caut server");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .map_err(|e| CautError::Config(format!("Failed to build HTTP client: {e}")))?;

    let response = client.get(&url).send().await.map_err(|e| {
        if e.is_connect() {
            CautError::Config(format!(
                "Cannot connect to caut server at {}:{}. Is `caut serve` running?",
                args.host, args.port
            ))
        } else if e.is_timeout() {
            CautError::Config(format!(
                "Timeout connecting to caut server at {}:{}",
                args.host, args.port
            ))
        } else {
            CautError::Config(format!("Request failed: {e}"))
        }
    })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| CautError::Config(format!("Failed to read response body: {e}")))?;

    if !status.is_success() {
        return Err(CautError::Config(format!(
            "Server returned {status}: {body}"
        )));
    }

    // Pretty-print if requested
    if pretty {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
            let pretty_json = serde_json::to_string_pretty(&value).unwrap_or(body);
            println!("{pretty_json}");
        } else {
            println!("{body}");
        }
    } else {
        println!("{body}");
    }

    Ok(())
}

/// Normalize the endpoint path (add leading slash if missing).
fn normalize_endpoint(endpoint: &str) -> String {
    let e = endpoint.trim().to_lowercase();
    if e.starts_with('/') {
        e
    } else {
        format!("/{e}")
    }
}

/// Read the server info file to auto-discover host/port.
/// This is used when the user doesn't specify --host/--port.
#[must_use]
#[allow(dead_code)]
pub fn read_server_info() -> Option<(String, u16)> {
    let paths = AppPaths::new();
    let info_path = paths.data.join("caut-server.json");

    let content = std::fs::read_to_string(&info_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;

    let bind = value.get("bind")?.as_str()?.to_string();
    let port_u64 = value.get("port")?.as_u64()?;
    let port = u16::try_from(port_u64).ok()?;

    Some((bind, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_endpoint_adds_slash() {
        assert_eq!(normalize_endpoint("usage"), "/usage");
        assert_eq!(normalize_endpoint("/usage"), "/usage");
        assert_eq!(normalize_endpoint("  health  "), "/health");
        assert_eq!(normalize_endpoint("/COST"), "/cost");
    }
}
