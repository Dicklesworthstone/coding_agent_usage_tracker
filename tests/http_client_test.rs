//! Integration tests for HTTP client with mock server.
//!
//! Tests HTTP operations against wiremock mock endpoints to verify:
//! - Success responses with valid JSON
//! - Error response handling (401, 403, 429, 500)
//! - Timeout handling
//! - Response parsing

mod common;

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use caut::core::http::{build_client, fetch_json, DEFAULT_TIMEOUT};
use caut::error::CautError;

use common::logger::TestLogger;

// =============================================================================
// Test Data Structures
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestPayload {
    status: String,
    value: i32,
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RateLimitResponse {
    used: i32,
    limit: i32,
    remaining: i32,
    reset_at: String,
}

// =============================================================================
// Success Response Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_success_with_valid_json() {
    let log = TestLogger::new("fetch_json_success_with_valid_json");
    log.phase("setup");

    let mock_server = MockServer::start().await;
    let payload = TestPayload {
        status: "ok".to_string(),
        value: 42,
        message: Some("Success".to_string()),
    };

    Mock::given(method("GET"))
        .and(path("/api/test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&payload))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/test", mock_server.uri());
    log.http_request("GET", &url);

    let result: TestPayload = fetch_json(&client, &url).await.expect("fetch should succeed");

    log.phase("verify");
    assert_eq!(result, payload);
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_success_with_minimal_json() {
    let log = TestLogger::new("fetch_json_success_with_minimal_json");
    log.phase("setup");

    let mock_server = MockServer::start().await;
    let payload = TestPayload {
        status: "ok".to_string(),
        value: 0,
        message: None,
    };

    Mock::given(method("GET"))
        .and(path("/api/minimal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&payload))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/minimal", mock_server.uri());
    let result: TestPayload = fetch_json(&client, &url).await.expect("fetch should succeed");

    log.phase("verify");
    assert_eq!(result.status, "ok");
    assert_eq!(result.value, 0);
    assert!(result.message.is_none());
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_with_rate_limit_headers() {
    let log = TestLogger::new("fetch_json_with_rate_limit_headers");
    log.phase("setup");

    let mock_server = MockServer::start().await;
    let rate_limit_response = RateLimitResponse {
        used: 50,
        limit: 100,
        remaining: 50,
        reset_at: "2026-01-19T00:00:00Z".to_string(),
    };

    Mock::given(method("GET"))
        .and(path("/api/rate-limit"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(&rate_limit_response)
                .insert_header("X-RateLimit-Limit", "100")
                .insert_header("X-RateLimit-Remaining", "50")
                .insert_header("X-RateLimit-Reset", "1737244800"),
        )
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/rate-limit", mock_server.uri());
    let result: RateLimitResponse = fetch_json(&client, &url).await.expect("fetch should succeed");

    log.phase("verify");
    assert_eq!(result.used, 50);
    assert_eq!(result.remaining, 50);
    log.finish_ok();
}

// =============================================================================
// HTTP Error Response Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_401_unauthorized() {
    let log = TestLogger::new("fetch_json_401_unauthorized");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/protected"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/protected", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            assert!(msg.contains("401"), "Error should mention 401: {}", msg);
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_403_forbidden() {
    let log = TestLogger::new("fetch_json_403_forbidden");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/forbidden"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/forbidden", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            assert!(msg.contains("403"), "Error should mention 403: {}", msg);
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_429_rate_limited() {
    let log = TestLogger::new("fetch_json_429_rate_limited");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/limited"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_string("Rate limit exceeded")
                .insert_header("Retry-After", "60"),
        )
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/limited", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            assert!(msg.contains("429"), "Error should mention 429: {}", msg);
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_500_server_error() {
    let log = TestLogger::new("fetch_json_500_server_error");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/error"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/error", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            assert!(msg.contains("500"), "Error should mention 500: {}", msg);
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_502_bad_gateway() {
    let log = TestLogger::new("fetch_json_502_bad_gateway");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/gateway"))
        .respond_with(ResponseTemplate::new(502).set_body_string("Bad Gateway"))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/gateway", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            assert!(msg.contains("502"), "Error should mention 502: {}", msg);
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_503_service_unavailable() {
    let log = TestLogger::new("fetch_json_503_service_unavailable");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/unavailable"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/unavailable", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            assert!(msg.contains("503"), "Error should mention 503: {}", msg);
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

// =============================================================================
// Response Parsing Error Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_invalid_json_response() {
    let log = TestLogger::new("fetch_json_invalid_json_response");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/invalid"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not valid json"))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/invalid", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::ParseResponse(msg) => {
            log.debug(&format!("Got expected parse error: {}", msg));
        }
        other => panic!("Expected ParseResponse error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_empty_response() {
    let log = TestLogger::new("fetch_json_empty_response");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/empty"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/empty", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    match &result.unwrap_err() {
        CautError::ParseResponse(_) => {}
        other => panic!("Expected ParseResponse error, got: {:?}", other),
    }
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_wrong_json_structure() {
    let log = TestLogger::new("fetch_json_wrong_json_structure");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    // Return valid JSON but wrong structure for TestPayload
    Mock::given(method("GET"))
        .and(path("/api/wrong"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(&serde_json::json!({
                "different": "structure",
                "unexpected": 123
            })),
        )
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/wrong", mock_server.uri());
    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    match &result.unwrap_err() {
        CautError::ParseResponse(_) => {}
        other => panic!("Expected ParseResponse error, got: {:?}", other),
    }
    log.finish_ok();
}

// =============================================================================
// Timeout Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_timeout_on_slow_response() {
    let log = TestLogger::new("fetch_json_timeout_on_slow_response");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    // Respond after 5 seconds (longer than our 1 second timeout)
    Mock::given(method("GET"))
        .and(path("/api/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(&TestPayload {
                    status: "ok".to_string(),
                    value: 1,
                    message: None,
                })
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&mock_server)
        .await;

    log.phase("execute");
    // Use a very short timeout
    let client = build_client(Duration::from_secs(1)).expect("client build");
    let url = format!("{}/api/slow", mock_server.uri());
    log.info("Making request with 1s timeout to slow endpoint");

    let result: Result<TestPayload, CautError> = fetch_json(&client, &url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Timeout(secs) => {
            // Note: The implementation uses DEFAULT_TIMEOUT in the error regardless of actual timeout
            // The important thing is that we got a Timeout error, not the specific value
            log.debug(&format!("Got expected timeout error (reports {} seconds)", secs));
        }
        other => panic!("Expected Timeout error, got: {:?}", other),
    }
    log.finish_ok();
}

// =============================================================================
// Network Condition Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_connection_refused() {
    let log = TestLogger::new("fetch_json_connection_refused");
    log.phase("setup");

    // Use a port that's definitely not listening
    let url = "http://127.0.0.1:59999/api/test";

    log.phase("execute");
    let client = build_client(Duration::from_secs(2)).expect("client build");
    let result: Result<TestPayload, CautError> = fetch_json(&client, url).await;

    log.phase("verify");
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CautError::Network(msg) => {
            log.debug(&format!("Got expected network error: {}", msg));
        }
        other => panic!("Expected Network error, got: {:?}", other),
    }
    log.finish_ok();
}

// =============================================================================
// Client Configuration Tests
// =============================================================================

#[tokio::test]
async fn build_client_with_custom_timeout() {
    let log = TestLogger::new("build_client_with_custom_timeout");
    log.phase("execute");

    let result = build_client(Duration::from_secs(60));

    log.phase("verify");
    assert!(result.is_ok());
    log.finish_ok();
}

#[tokio::test]
async fn build_client_with_zero_timeout() {
    let log = TestLogger::new("build_client_with_zero_timeout");
    log.phase("execute");

    // Zero timeout is valid for reqwest (means no timeout)
    let result = build_client(Duration::from_secs(0));

    log.phase("verify");
    assert!(result.is_ok());
    log.finish_ok();
}

// =============================================================================
// User-Agent Header Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_sends_user_agent() {
    let log = TestLogger::new("fetch_json_sends_user_agent");
    log.phase("setup");

    let mock_server = MockServer::start().await;
    let payload = TestPayload {
        status: "ok".to_string(),
        value: 1,
        message: None,
    };

    // Expect User-Agent header containing "caut/"
    Mock::given(method("GET"))
        .and(path("/api/ua"))
        .and(header("User-Agent", format!("caut/{}", env!("CARGO_PKG_VERSION")).as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(&payload))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/ua", mock_server.uri());
    let result: TestPayload = fetch_json(&client, &url).await.expect("request should match UA");

    log.phase("verify");
    assert_eq!(result.status, "ok");
    log.finish_ok();
}

// =============================================================================
// Provider-Specific Response Format Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_claude_rate_limit_format() {
    let log = TestLogger::new("fetch_json_claude_rate_limit_format");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    // Claude-style rate limit response
    let claude_response = serde_json::json!({
        "rate_limit": {
            "requests": {
                "used": 30,
                "limit": 100,
                "remaining": 70
            },
            "tokens": {
                "used": 50000,
                "limit": 200000,
                "remaining": 150000
            },
            "resets_at": "2026-01-18T12:00:00Z"
        }
    });

    Mock::given(method("GET"))
        .and(path("/v1/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&claude_response))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/v1/rate_limit", mock_server.uri());
    let result: serde_json::Value = fetch_json(&client, &url).await.expect("fetch should succeed");

    log.phase("verify");
    assert!(result["rate_limit"]["requests"]["remaining"].as_i64().unwrap() == 70);
    log.finish_ok();
}

#[tokio::test]
async fn fetch_json_openai_rate_limit_format() {
    let log = TestLogger::new("fetch_json_openai_rate_limit_format");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    // OpenAI-style rate limit response
    let openai_response = serde_json::json!({
        "user": {
            "id": "user-123",
            "email": "test@example.com",
            "subscription": {
                "plan": "pro",
                "status": "active"
            }
        },
        "limits": {
            "requests_per_minute": 60,
            "tokens_per_minute": 100000
        }
    });

    Mock::given(method("GET"))
        .and(path("/dashboard/user/api_keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&openai_response))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/dashboard/user/api_keys", mock_server.uri());
    let result: serde_json::Value = fetch_json(&client, &url).await.expect("fetch should succeed");

    log.phase("verify");
    assert!(result["user"]["email"].as_str().unwrap() == "test@example.com");
    log.finish_ok();
}

// =============================================================================
// StatusPage Response Format Tests
// =============================================================================

#[tokio::test]
async fn fetch_json_statuspage_format() {
    let log = TestLogger::new("fetch_json_statuspage_format");
    log.phase("setup");

    let mock_server = MockServer::start().await;

    // StatusPage.io style response
    let status_response = serde_json::json!({
        "page": {
            "id": "abc123",
            "name": "Provider Status"
        },
        "status": {
            "indicator": "none",
            "description": "All Systems Operational"
        },
        "components": [
            {
                "name": "API",
                "status": "operational"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/api/v2/status.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&status_response))
        .mount(&mock_server)
        .await;

    log.phase("execute");
    let client = build_client(DEFAULT_TIMEOUT).expect("client build");
    let url = format!("{}/api/v2/status.json", mock_server.uri());
    let result: serde_json::Value = fetch_json(&client, &url).await.expect("fetch should succeed");

    log.phase("verify");
    assert_eq!(result["status"]["indicator"].as_str().unwrap(), "none");
    assert_eq!(
        result["status"]["description"].as_str().unwrap(),
        "All Systems Operational"
    );
    log.finish_ok();
}
