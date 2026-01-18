//! CLI command runner utilities.
//!
//! Provides async subprocess execution for CLI-based provider fetchers.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::{CautError, Result};

/// Default timeout for CLI commands.
pub const CLI_TIMEOUT: Duration = Duration::from_secs(30);

/// Output from a CLI command.
#[derive(Debug)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CliOutput {
    /// Check if command succeeded (exit code 0).
    #[must_use]
    pub const fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Run a CLI command with timeout.
///
/// # Errors
///
/// Returns error if:
/// - Command not found
/// - Command times out
/// - Command fails to execute
pub async fn run_command(
    program: &str,
    args: &[&str],
    timeout_duration: Duration,
) -> Result<CliOutput> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CautError::ProviderNotFound(program.to_string())
            } else {
                CautError::FetchFailed {
                    provider: program.to_string(),
                    reason: e.to_string(),
                }
            }
        })?;

    let result = timeout(timeout_duration, async {
        // Read stdout and stderr concurrently to avoid deadlock.
        // If we read them sequentially and the child writes a lot to one stream,
        // its pipe buffer can fill up while we're waiting on the other stream,
        // causing the child to block and creating a deadlock.
        let stdout_handle = async {
            let mut stdout = String::new();
            if let Some(mut out) = child.stdout.take() {
                out.read_to_string(&mut stdout).await?;
            }
            Ok::<_, std::io::Error>(stdout)
        };

        let stderr_handle = async {
            let mut stderr = String::new();
            if let Some(mut err) = child.stderr.take() {
                err.read_to_string(&mut stderr).await?;
            }
            Ok::<_, std::io::Error>(stderr)
        };

        let (stdout_result, stderr_result) = tokio::join!(stdout_handle, stderr_handle);
        let stdout = stdout_result?;
        let stderr = stderr_result?;

        let status = child.wait().await?;

        Ok::<_, std::io::Error>(CliOutput {
            stdout,
            stderr,
            exit_code: status.code().unwrap_or(-1),
        })
    })
    .await;

    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(CautError::FetchFailed {
            provider: program.to_string(),
            reason: e.to_string(),
        }),
        Err(_) => {
            // Timeout - kill the process
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(CautError::Timeout(timeout_duration.as_secs()))
        }
    }
}

/// Run a CLI command and parse JSON output.
///
/// # Errors
///
/// Returns error if command fails or output is not valid JSON.
pub async fn run_json_command<T: serde::de::DeserializeOwned>(
    program: &str,
    args: &[&str],
    timeout_duration: Duration,
) -> Result<T> {
    let output = run_command(program, args, timeout_duration).await?;

    if !output.success() {
        return Err(CautError::FetchFailed {
            provider: program.to_string(),
            reason: format!("exit code {}: {}", output.exit_code, output.stderr.trim()),
        });
    }

    serde_json::from_str(&output.stdout).map_err(|e| {
        CautError::ParseResponse(format!(
            "{}: {}",
            e,
            output.stdout.chars().take(200).collect::<String>()
        ))
    })
}
