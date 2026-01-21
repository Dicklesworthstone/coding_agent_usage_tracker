//! History command implementation.
//!
//! Manages usage history database: pruning old data, showing statistics,
//! and displaying usage trends with ASCII/Unicode visualizations.

use chrono::{Duration, Utc};

use crate::cli::args::{HistoryCommand, HistoryPruneArgs, HistoryShowArgs, OutputFormat};
use crate::core::provider::Provider;
use crate::error::Result;
use crate::render::human::{HistoryDay, HistoryRenderOptions, render_history_chart};
use crate::storage::{
    AppPaths, DEFAULT_AGGREGATE_RETENTION_DAYS, DEFAULT_DETAILED_RETENTION_DAYS,
    DEFAULT_MAX_SIZE_BYTES, HistoryStore, RetentionPolicy,
};

/// Execute history commands.
pub fn execute(
    cmd: &HistoryCommand,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    match cmd {
        HistoryCommand::Show(args) => execute_show(args, format, pretty, no_color),
        HistoryCommand::Prune(args) => execute_prune(args, format, pretty),
        HistoryCommand::Stats => execute_stats(format, pretty),
    }
}

/// Execute the show subcommand - display usage history with trend visualization.
fn execute_show(
    args: &HistoryShowArgs,
    format: OutputFormat,
    _pretty: bool,
    no_color: bool,
) -> Result<()> {
    let paths = AppPaths::new();
    let history_path = paths.history_db_file();

    // Check if database exists
    if !history_path.exists() {
        match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "schemaVersion": "caut.v1",
                    "command": "history show",
                    "data": null,
                    "message": "No history data available. Run `caut usage` to start collecting data."
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Md => {
                println!("# Usage History\n");
                println!("No history data available. Run `caut usage` to start collecting data.");
            }
            OutputFormat::Human => {
                println!("No history data available.");
                println!("Run `caut usage` to start collecting usage data.");
            }
        }
        return Ok(());
    }

    let store = HistoryStore::open(&history_path)?;

    // Parse provider filter
    let providers: Vec<Provider> = if let Some(ref provider_name) = args.provider {
        vec![Provider::from_cli_name(provider_name)?]
    } else {
        Provider::ALL.to_vec()
    };

    // Calculate time range
    let to = Utc::now();
    let from = to - Duration::days(i64::from(args.days));

    // Render options
    let mut options = HistoryRenderOptions::default();
    options.no_color = no_color || format != OutputFormat::Human;
    options.max_width = None;
    options.use_unicode = options.use_unicode && !args.ascii;

    let mut any_data = false;

    // Handle JSON format
    if format == OutputFormat::Json {
        let mut provider_data = Vec::new();

        for provider in &providers {
            let days = get_daily_history(&store, provider, from, to)?;
            if !days.is_empty() {
                let day_json: Vec<_> = days
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "date": d.label,
                            "avgUsagePct": d.avg_primary_pct,
                            "totalCost": d.total_cost,
                            "hitLimit": d.hit_limit
                        })
                    })
                    .collect();

                provider_data.push(serde_json::json!({
                    "provider": provider.cli_name(),
                    "days": day_json
                }));
            }
        }

        let output = serde_json::json!({
            "schemaVersion": "caut.v1",
            "command": "history show",
            "data": {
                "period": {
                    "from": from.format("%Y-%m-%d").to_string(),
                    "to": to.format("%Y-%m-%d").to_string(),
                    "days": args.days
                },
                "providers": provider_data
            }
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Handle Markdown format
    if format == OutputFormat::Md {
        println!("# Usage History\n");
        println!(
            "**Period:** {} to {} ({} days)\n",
            from.format("%Y-%m-%d"),
            to.format("%Y-%m-%d"),
            args.days
        );

        for provider in &providers {
            let days = get_daily_history(&store, provider, from, to)?;
            if !days.is_empty() {
                any_data = true;
                println!("## {}\n", provider.display_name());
                println!("| Date | Usage % | Cost | Limit Hit |");
                println!("|------|---------|------|-----------|");
                for day in &days {
                    let cost = day
                        .total_cost
                        .map(|c| format!("${:.2}", c))
                        .unwrap_or_else(|| "-".to_string());
                    let limit = if day.hit_limit { "Yes" } else { "-" };
                    println!(
                        "| {} | {:.1}% | {} | {} |",
                        day.label, day.avg_primary_pct, cost, limit
                    );
                }
                println!();
            }
        }

        if !any_data {
            println!("No usage data found for the specified period.");
        }
        return Ok(());
    }

    // Human format - use the render_history_chart function
    for provider in &providers {
        let days = get_daily_history(&store, provider, from, to)?;
        if !days.is_empty() {
            any_data = true;
            let chart = render_history_chart(provider.display_name(), &days, &options);
            println!("{}", chart);
            println!();
        }
    }

    if !any_data {
        println!("No usage data found for the specified period.");
        println!("Run `caut usage` to start collecting usage data.");
    }

    Ok(())
}

/// Get daily history data for a provider, suitable for chart rendering.
fn get_daily_history(
    store: &HistoryStore,
    provider: &Provider,
    from: chrono::DateTime<Utc>,
    to: chrono::DateTime<Utc>,
) -> Result<Vec<HistoryDay>> {
    use std::collections::HashMap;

    let snapshots = store.get_snapshots(provider, from, to)?;

    if snapshots.is_empty() {
        return Ok(Vec::new());
    }

    // Group snapshots by day
    let mut daily_data: HashMap<String, Vec<(f64, Option<f64>)>> = HashMap::new();

    for snapshot in &snapshots {
        let day_key = snapshot.fetched_at.format("%Y-%m-%d").to_string();
        let used_pct = snapshot.primary_used_pct.unwrap_or(0.0);
        let cost = snapshot.cost_today_usd;

        daily_data
            .entry(day_key)
            .or_default()
            .push((used_pct, cost));
    }

    // Convert to HistoryDay structs
    let mut days: Vec<HistoryDay> = daily_data
        .into_iter()
        .map(|(date_str, values)| {
            let avg_pct = values.iter().map(|(p, _)| *p).sum::<f64>() / values.len() as f64;
            let max_pct = values
                .iter()
                .map(|(p, _)| *p)
                .fold(f64::NEG_INFINITY, f64::max);
            let total_cost = values
                .iter()
                .filter_map(|(_, c)| *c)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            // Parse the date to get a nice label
            let label = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .map(|d| d.format("%a %m/%d").to_string())
                .unwrap_or(date_str.clone());

            HistoryDay {
                label,
                avg_primary_pct: avg_pct,
                total_cost,
                hit_limit: max_pct >= 95.0,
            }
        })
        .collect();

    // Sort by date (the label is "Day MM/DD" format, so we need to sort by the original key)
    days.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(days)
}

/// Execute the prune subcommand.
fn execute_prune(args: &HistoryPruneArgs, format: OutputFormat, pretty: bool) -> Result<()> {
    let paths = AppPaths::new();
    let history_path = paths.history_db_file();

    // Check if database exists
    if !history_path.exists() {
        if format == OutputFormat::Json {
            let output = serde_json::json!({
                "schemaVersion": "caut.v1",
                "command": "history prune",
                "data": null,
                "message": "No history database found"
            });
            if pretty {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{}", serde_json::to_string(&output)?);
            }
        } else {
            println!("No history database found at: {}", history_path.display());
            println!("Nothing to prune.");
        }
        return Ok(());
    }

    let store = HistoryStore::open(&history_path)?;

    // Build retention policy from args
    let mut policy = RetentionPolicy::default();

    if let Some(days) = args.keep_days {
        policy = policy.with_detailed_days(days);
    }

    if let Some(days) = args.keep_aggregates {
        policy = policy.with_aggregate_days(days);
    }

    if let Some(mb) = args.max_size_mb {
        policy = policy.with_max_size(mb * 1024 * 1024);
    }

    // Get pre-prune stats
    let db_size_before = store.get_db_size()?;

    // Execute prune
    let result = store.prune(&policy, args.dry_run)?;

    // Output results
    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "schemaVersion": "caut.v1",
                "command": "history prune",
                "data": {
                    "dryRun": result.dry_run,
                    "detailedDeleted": result.detailed_deleted,
                    "aggregatesCreated": result.aggregates_created,
                    "aggregatesDeleted": result.aggregates_deleted,
                    "bytesFreed": result.bytes_freed,
                    "durationMs": result.duration_ms,
                    "sizeLimitTriggered": result.size_limit_triggered,
                    "policy": {
                        "detailedRetentionDays": policy.detailed_retention_days,
                        "aggregateRetentionDays": policy.aggregate_retention_days,
                        "maxSizeBytes": policy.max_size_bytes,
                    },
                    "dbSizeBefore": db_size_before,
                    "dbSizeAfter": if result.dry_run { db_size_before } else { store.get_db_size()? },
                }
            });

            if pretty {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{}", serde_json::to_string(&output)?);
            }
        }
        OutputFormat::Md => {
            println!("# History Prune Results\n");
            if result.dry_run {
                println!("**Mode:** Dry run (no changes made)\n");
            }
            println!("## Summary\n");
            println!("| Metric | Value |");
            println!("|--------|-------|");
            println!(
                "| Detailed snapshots {} | {} |",
                if result.dry_run {
                    "would be deleted"
                } else {
                    "deleted"
                },
                result.detailed_deleted
            );
            println!(
                "| Daily aggregates {} | {} |",
                if result.dry_run {
                    "would be created"
                } else {
                    "created"
                },
                result.aggregates_created
            );
            println!(
                "| Old aggregates {} | {} |",
                if result.dry_run {
                    "would be deleted"
                } else {
                    "deleted"
                },
                result.aggregates_deleted
            );
            println!("| Duration | {} ms |", result.duration_ms);
            if !result.dry_run {
                println!("| Bytes freed | {} |", format_bytes(result.bytes_freed));
            }
            println!("\n## Policy\n");
            println!(
                "- Keep detailed snapshots: {} days",
                policy.detailed_retention_days
            );
            println!(
                "- Keep daily aggregates: {} days",
                policy.aggregate_retention_days
            );
            println!(
                "- Max database size: {}",
                format_bytes(policy.max_size_bytes)
            );
        }
        OutputFormat::Human => {
            if result.dry_run {
                println!("Dry run - no changes made\n");
            }

            println!("History Prune Results");
            println!("---------------------");
            println!(
                "Detailed snapshots {}: {}",
                if result.dry_run {
                    "to delete"
                } else {
                    "deleted"
                },
                result.detailed_deleted
            );
            println!(
                "Daily aggregates {}: {}",
                if result.dry_run {
                    "to create"
                } else {
                    "created"
                },
                result.aggregates_created
            );
            println!(
                "Old aggregates {}: {}",
                if result.dry_run {
                    "to delete"
                } else {
                    "deleted"
                },
                result.aggregates_deleted
            );
            println!("Duration: {} ms", result.duration_ms);

            if !result.dry_run && result.bytes_freed > 0 {
                println!("Bytes freed: {}", format_bytes(result.bytes_freed));
            }

            if result.size_limit_triggered {
                println!(
                    "\nSize limit ({}) was exceeded and triggered additional cleanup.",
                    format_bytes(policy.max_size_bytes)
                );
            }

            println!("\nPolicy:");
            println!(
                "  Keep detailed snapshots: {} days",
                policy.detailed_retention_days
            );
            println!(
                "  Keep daily aggregates: {} days",
                policy.aggregate_retention_days
            );
            println!(
                "  Max database size: {}",
                format_bytes(policy.max_size_bytes)
            );
        }
    }

    Ok(())
}

/// Execute the stats subcommand.
fn execute_stats(format: OutputFormat, pretty: bool) -> Result<()> {
    let paths = AppPaths::new();
    let history_path = paths.history_db_file();

    if !history_path.exists() {
        if format == OutputFormat::Json {
            let output = serde_json::json!({
                "schemaVersion": "caut.v1",
                "command": "history stats",
                "data": null,
                "message": "No history database found"
            });
            if pretty {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{}", serde_json::to_string(&output)?);
            }
        } else {
            println!("No history database found at: {}", history_path.display());
        }
        return Ok(());
    }

    let store = HistoryStore::open(&history_path)?;
    let db_size = store.get_db_size()?;

    // Count snapshots and aggregates (use raw SQL for efficiency)
    let snapshot_count = count_table(&store, "usage_snapshots")?;
    let aggregate_count = count_table(&store, "daily_aggregates")?;
    let prune_count = count_table(&store, "prune_history")?;

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "schemaVersion": "caut.v1",
                "command": "history stats",
                "data": {
                    "databasePath": history_path.to_string_lossy(),
                    "databaseSizeBytes": db_size,
                    "snapshotCount": snapshot_count,
                    "aggregateCount": aggregate_count,
                    "pruneHistoryCount": prune_count,
                    "defaults": {
                        "detailedRetentionDays": DEFAULT_DETAILED_RETENTION_DAYS,
                        "aggregateRetentionDays": DEFAULT_AGGREGATE_RETENTION_DAYS,
                        "maxSizeBytes": DEFAULT_MAX_SIZE_BYTES,
                    }
                }
            });
            if pretty {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{}", serde_json::to_string(&output)?);
            }
        }
        OutputFormat::Md => {
            println!("# History Database Statistics\n");
            println!("| Property | Value |");
            println!("|----------|-------|");
            println!("| Database path | `{}` |", history_path.display());
            println!("| Database size | {} |", format_bytes(db_size));
            println!("| Snapshots | {} |", snapshot_count);
            println!("| Daily aggregates | {} |", aggregate_count);
            println!("| Prune operations | {} |", prune_count);
            println!("\n## Default Retention Policy\n");
            println!(
                "- Detailed retention: {} days",
                DEFAULT_DETAILED_RETENTION_DAYS
            );
            println!(
                "- Aggregate retention: {} days",
                DEFAULT_AGGREGATE_RETENTION_DAYS
            );
            println!("- Max size: {}", format_bytes(DEFAULT_MAX_SIZE_BYTES));
        }
        OutputFormat::Human => {
            println!("History Database Statistics");
            println!("---------------------------");
            println!("Database: {}", history_path.display());
            println!("Size: {}", format_bytes(db_size));
            println!();
            println!("Records:");
            println!("  Snapshots: {}", snapshot_count);
            println!("  Daily aggregates: {}", aggregate_count);
            println!("  Prune history: {}", prune_count);
            println!();
            println!("Default retention policy:");
            println!("  Detailed: {} days", DEFAULT_DETAILED_RETENTION_DAYS);
            println!("  Aggregates: {} days", DEFAULT_AGGREGATE_RETENTION_DAYS);
            println!("  Max size: {}", format_bytes(DEFAULT_MAX_SIZE_BYTES));
        }
    }

    Ok(())
}

/// Count rows in a table.
fn count_table(store: &HistoryStore, table: &str) -> Result<i64> {
    store.count_rows(table)
}

/// Format bytes in human-readable form.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}
