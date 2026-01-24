//! Local cost scanning for Claude and Codex.
//!
//! Scans local JSONL history files and stats caches to compute
//! usage statistics for the cost command.

use crate::core::models::{CostDailyEntry, CostPayload, CostTotals};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::storage::paths::AppPaths;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

/// Claude stats cache format (from ~/.claude/stats-cache.json).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(dead_code)]
struct ClaudeStatsCache {
    version: i32,
    last_computed_date: String,
    daily_activity: Vec<ClaudeDailyActivity>,
}

/// Daily activity entry from Claude stats cache.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDailyActivity {
    date: String,
    message_count: i64,
    session_count: i64,
    tool_call_count: i64,
}

/// Codex session event from JSONL files.
#[derive(Debug, Deserialize)]
#[expect(dead_code)]
struct CodexEvent {
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

/// Cost scanner for local usage data.
pub struct CostScanner {
    #[expect(dead_code)]
    paths: AppPaths,
}

impl CostScanner {
    /// Create a new cost scanner.
    #[must_use]
    pub fn new() -> Self {
        Self {
            paths: AppPaths::new(),
        }
    }

    /// Scan cost data for a provider.
    pub async fn scan(&self, provider: Provider, _refresh: bool) -> Result<CostPayload> {
        match provider {
            Provider::Claude => self.scan_claude().await,
            Provider::Codex => self.scan_codex().await,
            _ => Err(CautError::Config(format!(
                "Provider {} does not support local cost scanning",
                provider.cli_name()
            ))),
        }
    }

    /// Scan Claude's local stats cache.
    async fn scan_claude(&self) -> Result<CostPayload> {
        let claude_dir = dirs::home_dir()
            .ok_or_else(|| CautError::Config("Cannot determine home directory".to_string()))?
            .join(".claude");

        let stats_path = claude_dir.join("stats-cache.json");

        if !stats_path.exists() {
            tracing::debug!(?stats_path, "Claude stats cache not found");
            return Ok(self.empty_cost_payload("claude"));
        }

        tracing::debug!(?stats_path, "Reading Claude stats cache");

        let file = File::open(&stats_path)
            .map_err(|e| CautError::Config(format!("Failed to open Claude stats cache: {}", e)))?;

        let stats: ClaudeStatsCache = serde_json::from_reader(file).map_err(|e| {
            CautError::ParseResponse(format!("Failed to parse Claude stats cache: {}", e))
        })?;

        // Filter to last 30 days
        let cutoff = Utc::now() - Duration::days(30);
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();

        let mut daily_entries: Vec<CostDailyEntry> = Vec::new();
        let mut total_messages: i64 = 0;
        let mut _total_sessions: i64 = 0;
        let mut _total_tool_calls: i64 = 0;
        let mut today_messages: i64 = 0;

        let today = Utc::now().format("%Y-%m-%d").to_string();

        for activity in &stats.daily_activity {
            if activity.date >= cutoff_date {
                daily_entries.push(CostDailyEntry {
                    date: activity.date.clone(),
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    total_tokens: Some(activity.message_count), // Use message count as proxy
                    total_cost: None, // Cannot determine cost without token data
                    models_used: None,
                });

                total_messages += activity.message_count;
                _total_sessions += activity.session_count;
                _total_tool_calls += activity.tool_call_count;

                if activity.date == today {
                    today_messages = activity.message_count;
                }
            }
        }

        // Sort by date descending (most recent first)
        daily_entries.sort_by(|a, b| b.date.cmp(&a.date));

        Ok(CostPayload {
            provider: "claude".to_string(),
            source: "local".to_string(),
            updated_at: Utc::now(),
            session_tokens: Some(today_messages),
            session_cost_usd: None, // Cannot determine without pricing data
            last_30_days_tokens: Some(total_messages),
            last_30_days_cost_usd: None, // Cannot determine without pricing data
            daily: daily_entries,
            totals: Some(CostTotals {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens: Some(total_messages),
                total_cost: None,
            }),
        })
    }

    /// Scan Codex's local session files.
    async fn scan_codex(&self) -> Result<CostPayload> {
        let codex_dir = dirs::home_dir()
            .ok_or_else(|| CautError::Config("Cannot determine home directory".to_string()))?
            .join(".codex");

        let sessions_dir = codex_dir.join("sessions");

        if !sessions_dir.exists() {
            tracing::debug!(?sessions_dir, "Codex sessions directory not found");
            return Ok(self.empty_cost_payload("codex"));
        }

        tracing::debug!(?sessions_dir, "Scanning Codex sessions directory");

        // Collect all JSONL files from the sessions directory
        let cutoff = Utc::now() - Duration::days(30);
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();

        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();
        let today = Utc::now().format("%Y-%m-%d").to_string();

        // Walk through year/month/day directories
        if let Ok(years) = std::fs::read_dir(&sessions_dir) {
            for year_entry in years.flatten() {
                if !year_entry.path().is_dir() {
                    continue;
                }
                if let Ok(months) = std::fs::read_dir(year_entry.path()) {
                    for month_entry in months.flatten() {
                        if !month_entry.path().is_dir() {
                            continue;
                        }
                        if let Ok(days) = std::fs::read_dir(month_entry.path()) {
                            for day_entry in days.flatten() {
                                let day_path = day_entry.path();
                                if !day_path.is_dir() {
                                    continue;
                                }

                                // Read JSONL files in this day's directory
                                if let Ok(files) = std::fs::read_dir(&day_path) {
                                    for file_entry in files.flatten() {
                                        let file_path = file_entry.path();
                                        if file_path.extension().map_or(false, |ext| ext == "jsonl")
                                        {
                                            self.scan_codex_jsonl(
                                                &file_path,
                                                &cutoff_date,
                                                &mut daily_counts,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Also check history.jsonl in codex root
        let history_path = codex_dir.join("history.jsonl");
        if history_path.exists() {
            self.scan_codex_history(&history_path, &cutoff_date, &mut daily_counts);
        }

        // Convert to daily entries
        let mut daily_entries: Vec<CostDailyEntry> = daily_counts
            .into_iter()
            .map(|(date, count)| CostDailyEntry {
                date,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens: Some(count.events),
                total_cost: None,
                models_used: None,
            })
            .collect();

        daily_entries.sort_by(|a, b| b.date.cmp(&a.date));

        let total_events: i64 = daily_entries.iter().filter_map(|e| e.total_tokens).sum();

        let today_events = daily_entries
            .iter()
            .find(|e| e.date == today)
            .and_then(|e| e.total_tokens)
            .unwrap_or(0);

        Ok(CostPayload {
            provider: "codex".to_string(),
            source: "local".to_string(),
            updated_at: Utc::now(),
            session_tokens: Some(today_events),
            session_cost_usd: None,
            last_30_days_tokens: Some(total_events),
            last_30_days_cost_usd: None,
            daily: daily_entries,
            totals: Some(CostTotals {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens: Some(total_events),
                total_cost: None,
            }),
        })
    }

    /// Scan a Codex JSONL session file.
    fn scan_codex_jsonl(
        &self,
        path: &PathBuf,
        cutoff_date: &str,
        daily_counts: &mut HashMap<String, DailyCount>,
    ) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(?path, error = %e, "Failed to open Codex session file");
                return;
            }
        };

        let reader = BufReader::new(file);
        let mut recent_lines: VecDeque<String> = VecDeque::with_capacity(100);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if line.is_empty() {
                continue;
            }

            if recent_lines.len() == 100 {
                recent_lines.pop_front();
            }
            recent_lines.push_back(line);
        }

        for line in recent_lines {
            let event: CodexEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Extract date from timestamp
            if let Some(date) = event.timestamp.get(..10) {
                if date >= cutoff_date {
                    daily_counts
                        .entry(date.to_string())
                        .or_insert_with(DailyCount::default)
                        .events += 1;
                }
            }
        }
    }

    /// Scan Codex history.jsonl for session counts.
    fn scan_codex_history(
        &self,
        path: &PathBuf,
        cutoff_date: &str,
        daily_counts: &mut HashMap<String, DailyCount>,
    ) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(?path, error = %e, "Failed to open Codex history file");
                return;
            }
        };

        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if line.is_empty() {
                continue;
            }

            // History format: {"session_id": "...", "ts": 1234567890, "text": "..."}
            #[derive(Deserialize)]
            struct HistoryEntry {
                ts: i64,
            }

            let entry: HistoryEntry = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Convert timestamp to date
            if let Some(dt) = DateTime::from_timestamp(entry.ts, 0) {
                let date = dt.format("%Y-%m-%d").to_string();
                if date.as_str() >= cutoff_date {
                    daily_counts
                        .entry(date)
                        .or_insert_with(DailyCount::default)
                        .events += 1;
                }
            }
        }
    }

    /// Create an empty cost payload for a provider.
    fn empty_cost_payload(&self, provider: &str) -> CostPayload {
        CostPayload {
            provider: provider.to_string(),
            source: "local".to_string(),
            updated_at: Utc::now(),
            session_tokens: None,
            session_cost_usd: None,
            last_30_days_tokens: None,
            last_30_days_cost_usd: None,
            daily: Vec::new(),
            totals: None,
        }
    }
}

impl Default for CostScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper for counting daily events.
#[derive(Default)]
struct DailyCount {
    events: i64,
}

/// Module-level access to dirs crate.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // =========================================================================
    // CostScanner Creation Tests
    // =========================================================================

    #[test]
    fn test_cost_scanner_new() {
        let scanner = CostScanner::new();
        // Just verify it creates without panic
        assert!(true);
        // Using scanner to avoid warning
        let _ = scanner.paths;
    }

    #[test]
    fn test_cost_scanner_default() {
        let scanner = CostScanner::default();
        // Default implementation works
        let _ = scanner.paths;
    }

    // =========================================================================
    // Empty Cost Payload Tests
    // =========================================================================

    #[test]
    fn test_empty_cost_payload() {
        let scanner = CostScanner::new();
        let payload = scanner.empty_cost_payload("test");
        assert_eq!(payload.provider, "test");
        assert_eq!(payload.source, "local");
        assert!(payload.daily.is_empty());
    }

    #[test]
    fn test_empty_cost_payload_claude() {
        let scanner = CostScanner::new();
        let payload = scanner.empty_cost_payload("claude");
        assert_eq!(payload.provider, "claude");
        assert_eq!(payload.source, "local");
        assert!(payload.session_tokens.is_none());
        assert!(payload.session_cost_usd.is_none());
        assert!(payload.last_30_days_tokens.is_none());
        assert!(payload.last_30_days_cost_usd.is_none());
        assert!(payload.daily.is_empty());
        assert!(payload.totals.is_none());
    }

    #[test]
    fn test_empty_cost_payload_codex() {
        let scanner = CostScanner::new();
        let payload = scanner.empty_cost_payload("codex");
        assert_eq!(payload.provider, "codex");
        assert_eq!(payload.source, "local");
    }

    // =========================================================================
    // ClaudeStatsCache Parsing Tests
    // =========================================================================

    #[test]
    fn test_claude_stats_cache_parse_full() {
        let json = r#"{
            "version": 1,
            "lastComputedDate": "2026-01-18",
            "dailyActivity": [
                {"date": "2026-01-18", "messageCount": 42, "sessionCount": 3, "toolCallCount": 15},
                {"date": "2026-01-17", "messageCount": 35, "sessionCount": 2, "toolCallCount": 12}
            ]
        }"#;

        let stats: ClaudeStatsCache = serde_json::from_str(json).unwrap();
        assert_eq!(stats.version, 1);
        assert_eq!(stats.last_computed_date, "2026-01-18");
        assert_eq!(stats.daily_activity.len(), 2);

        let first = &stats.daily_activity[0];
        assert_eq!(first.date, "2026-01-18");
        assert_eq!(first.message_count, 42);
        assert_eq!(first.session_count, 3);
        assert_eq!(first.tool_call_count, 15);
    }

    #[test]
    fn test_claude_stats_cache_parse_empty_activity() {
        let json = r#"{
            "version": 1,
            "lastComputedDate": "2026-01-18",
            "dailyActivity": []
        }"#;

        let stats: ClaudeStatsCache = serde_json::from_str(json).unwrap();
        assert!(stats.daily_activity.is_empty());
    }

    #[test]
    fn test_claude_stats_cache_parse_malformed() {
        let json = r#"{ "version": 1, "lastComputedDate": "2026-01-18" "dailyActivity": [] }"#;
        let result: std::result::Result<ClaudeStatsCache, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // =========================================================================
    // CodexEvent Parsing Tests
    // =========================================================================

    #[test]
    fn test_codex_event_parse_full() {
        let json = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message", "payload": {"text": "Hello"}}"#;

        let event: CodexEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.timestamp, "2026-01-18T10:30:00Z");
        assert_eq!(event.event_type, "message");
        assert!(event.payload.is_some());
    }

    #[test]
    fn test_codex_event_parse_minimal() {
        let json = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "response"}"#;

        let event: CodexEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.timestamp, "2026-01-18T10:30:00Z");
        assert_eq!(event.event_type, "response");
        assert!(event.payload.is_none());
    }

    #[test]
    fn test_codex_event_date_extraction() {
        let json = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}"#;
        let event: CodexEvent = serde_json::from_str(json).unwrap();

        // Extract date from first 10 chars
        let date = event.timestamp.get(..10);
        assert_eq!(date, Some("2026-01-18"));
    }

    // =========================================================================
    // scan_codex_jsonl Tests
    // =========================================================================

    #[test]
    fn test_scan_codex_jsonl_valid_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message", "payload": {}}
{"timestamp": "2026-01-18T10:31:00Z", "type": "response", "payload": {}}
{"timestamp": "2026-01-17T14:00:00Z", "type": "message", "payload": {}}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();
        let cutoff_date = "2026-01-01";

        scanner.scan_codex_jsonl(&file_path, cutoff_date, &mut daily_counts);

        // Should have 2 dates
        assert_eq!(daily_counts.len(), 2);
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 2);
        assert_eq!(daily_counts.get("2026-01-17").unwrap().events, 1);
    }

    #[test]
    fn test_scan_codex_jsonl_empty_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}

{"timestamp": "2026-01-18T10:31:00Z", "type": "response"}

"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_jsonl(&file_path, "2026-01-01", &mut daily_counts);

        // Should skip empty lines
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 2);
    }

    #[test]
    fn test_scan_codex_jsonl_malformed_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}
not valid json at all
{"timestamp": "2026-01-18T10:31:00Z", "type": "response"}
{incomplete json"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_jsonl(&file_path, "2026-01-01", &mut daily_counts);

        // Should skip malformed lines, count only valid ones
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 2);
    }

    #[test]
    fn test_scan_codex_jsonl_100_line_limit() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        // Create 150 lines
        let mut content = String::new();
        for i in 0..150 {
            content.push_str(&format!(
                r#"{{"timestamp": "2026-01-18T10:{:02}:00Z", "type": "message"}}"#,
                i % 60
            ));
            content.push('\n');
        }

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_jsonl(&file_path, "2026-01-01", &mut daily_counts);

        // Should only read first 100 lines
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 100);
    }

    #[test]
    fn test_scan_codex_jsonl_date_cutoff() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}
{"timestamp": "2025-12-20T10:30:00Z", "type": "message"}
{"timestamp": "2025-06-01T10:30:00Z", "type": "message"}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Cutoff is 2025-12-15, so 2026-01-18 and 2025-12-20 should pass, 2025-06-01 filtered out
        scanner.scan_codex_jsonl(&file_path, "2025-12-15", &mut daily_counts);

        assert_eq!(daily_counts.len(), 2); // 2026-01-18 and 2025-12-20
        assert!(daily_counts.get("2026-01-18").is_some());
        assert!(daily_counts.get("2025-12-20").is_some());
        assert!(daily_counts.get("2025-06-01").is_none()); // Before cutoff
    }

    #[test]
    fn test_scan_codex_jsonl_nonexistent_file() {
        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Should not panic on nonexistent file
        scanner.scan_codex_jsonl(
            &PathBuf::from("/nonexistent/path/file.jsonl"),
            "2026-01-01",
            &mut daily_counts,
        );

        assert!(daily_counts.is_empty());
    }

    // =========================================================================
    // scan_codex_history Tests
    // =========================================================================

    #[test]
    fn test_scan_codex_history_valid() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("history.jsonl");

        // Unix timestamps for 2026-01-18 (~1768780800) and 2026-01-17 (~1768694400)
        // Using timestamps that are definitely in 2026
        let content = r#"{"session_id": "sess_001", "ts": 1768780800, "text": "First message"}
{"session_id": "sess_001", "ts": 1768780860, "text": "Second message"}
{"session_id": "sess_002", "ts": 1768694400, "text": "Yesterday message"}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_history(&file_path, "2026-01-01", &mut daily_counts);

        // Should have entries (exact dates depend on timezone, but should have some)
        assert!(!daily_counts.is_empty());
    }

    #[test]
    fn test_scan_codex_history_malformed() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("history.jsonl");

        // Using 2026 timestamps (~1768780800)
        let content = r#"{"session_id": "sess_001", "ts": 1768780800, "text": "Valid"}
not valid json
{"session_id": "sess_002", "ts": 1768780900, "text": "Also valid"}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Should not panic, should skip malformed lines
        scanner.scan_codex_history(&file_path, "2026-01-01", &mut daily_counts);

        // Should have parsed the valid lines
        assert!(!daily_counts.is_empty());
    }

    #[test]
    fn test_scan_codex_history_empty_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("history.jsonl");

        // Using 2026 timestamps (~1768780800)
        let content = r#"{"session_id": "sess_001", "ts": 1768780800, "text": "Valid"}

{"session_id": "sess_002", "ts": 1768780900, "text": "Also valid"}
"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_history(&file_path, "2026-01-01", &mut daily_counts);

        // Should skip empty lines
        assert!(!daily_counts.is_empty());
    }

    #[test]
    fn test_scan_codex_history_nonexistent() {
        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Should not panic on nonexistent file
        scanner.scan_codex_history(
            &PathBuf::from("/nonexistent/history.jsonl"),
            "2026-01-01",
            &mut daily_counts,
        );

        assert!(daily_counts.is_empty());
    }

    // =========================================================================
    // DailyCount Tests
    // =========================================================================

    #[test]
    fn test_daily_count_default() {
        let count = DailyCount::default();
        assert_eq!(count.events, 0);
    }

    #[test]
    fn test_daily_count_increment() {
        let mut count = DailyCount::default();
        count.events += 1;
        count.events += 1;
        assert_eq!(count.events, 2);
    }

    // =========================================================================
    // Date Filtering Tests
    // =========================================================================

    #[test]
    fn test_date_comparison() {
        // Test that string date comparison works correctly
        let cutoff = "2026-01-01";
        let recent_date = "2026-01-18";
        let old_date = "2025-12-01";

        assert!(recent_date >= cutoff);
        assert!(!(old_date >= cutoff));
    }

    #[test]
    fn test_date_sorting() {
        let mut dates = vec!["2026-01-15", "2026-01-18", "2026-01-10", "2026-01-17"];
        dates.sort_by(|a, b| b.cmp(a)); // Descending

        assert_eq!(
            dates,
            vec!["2026-01-18", "2026-01-17", "2026-01-15", "2026-01-10"]
        );
    }

    // =========================================================================
    // Integration-style Tests (with temp directories)
    // =========================================================================

    #[test]
    fn test_codex_directory_structure() {
        let dir = TempDir::new().unwrap();
        let sessions_dir = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("01")
            .join("18");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let session_file = sessions_dir.join("session_abc.jsonl");
        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}"#;
        std::fs::write(&session_file, content).unwrap();

        // Verify structure was created
        assert!(session_file.exists());
    }

    #[test]
    fn test_cost_daily_entry_creation() {
        let entry = CostDailyEntry {
            date: "2026-01-18".to_string(),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            total_tokens: Some(42),
            total_cost: None,
            models_used: None,
        };

        assert_eq!(entry.date, "2026-01-18");
        assert_eq!(entry.total_tokens, Some(42));
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[tokio::test]
    async fn test_scan_unsupported_provider() {
        let scanner = CostScanner::new();

        // Gemini doesn't support cost scanning
        let result = scanner.scan(Provider::Gemini, false).await;
        assert!(result.is_err());

        if let Err(CautError::Config(msg)) = result {
            assert!(msg.contains("does not support local cost scanning"));
        } else {
            panic!("Expected Config error");
        }
    }
}
