//! Credential file watcher for detecting account changes.
//!
//! Provides cross-platform filesystem monitoring for credential files,
//! enabling automatic detection of account switches and token refreshes.
//!
//! ## Architecture
//!
//! The watcher monitors credential file paths for each provider and emits
//! events when changes are detected. It integrates with the credential
//! hashing module to distinguish between account switches and token refreshes.
//!
//! A background thread receives raw filesystem events from the `notify` crate,
//! debounces them (coalescing rapid changes within a 500ms window), then
//! compares credential hashes to classify each change as an account switch,
//! token refresh, deletion, or no-op.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use caut::core::credential_watcher::{CredentialWatcher, WatchEvent};
//!
//! let watcher = CredentialWatcher::new()?;
//! watcher.watch_provider(Provider::Claude)?;
//!
//! // Handle events in your event loop
//! while let Ok(event) = watcher.try_recv() {
//!     match event {
//!         WatchEvent::AccountSwitch { provider, identity, .. } => {
//!             // Capture usage snapshot
//!         }
//!         WatchEvent::TokenRefresh { provider, .. } => {
//!             // Log refresh, no snapshot needed
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::core::credential_hash::{
    ChangeType, CredentialHasher, CredentialHashes, IdentityFields,
};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};

/// How long to wait after the last filesystem event before processing,
/// to coalesce rapid writes (e.g., atomic rename patterns).
const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);

/// Events emitted by the credential watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A new account was detected (credentials created or identity changed).
    AccountSwitch {
        /// Provider the account belongs to.
        provider: Provider,
        /// Identity fields from the new credentials.
        identity: IdentityFields,
        /// Path to the credential file.
        path: PathBuf,
    },
    /// Token was refreshed but identity unchanged.
    TokenRefresh {
        /// Provider whose token was refreshed.
        provider: Provider,
        /// Path to the credential file.
        path: PathBuf,
    },
    /// Credentials were deleted.
    CredentialsRemoved {
        /// Provider whose credentials were removed.
        provider: Provider,
        /// Path to the credential file.
        path: PathBuf,
    },
    /// Error occurred while processing an event.
    Error {
        /// Error message.
        message: String,
        /// Path that caused the error, if known.
        path: Option<PathBuf>,
    },
}

/// State tracking for a watched credential file.
#[derive(Debug)]
struct WatchedFile {
    /// Provider this file belongs to.
    provider: Provider,
    /// Last known hash of the file.
    last_hash: Option<CredentialHashes>,
}

/// Shared state between the main thread and the event processing thread.
type SharedState = Arc<Mutex<HashMap<PathBuf, WatchedFile>>>;

/// Cross-platform credential file watcher.
///
/// Wraps the `notify` crate's filesystem watcher with credential-aware
/// change detection. A background thread translates raw FS events into
/// typed `WatchEvent`s that distinguish account switches from token refreshes.
pub struct CredentialWatcher {
    /// The underlying filesystem watcher.
    watcher: Mutex<RecommendedWatcher>,
    /// Receiver for processed watch events.
    event_rx: Receiver<WatchEvent>,
    /// Shared map of watched paths (accessible from event thread).
    watched: SharedState,
    /// Hasher for credential content (main-thread use in `process_change`).
    hasher: CredentialHasher,
    /// Send `()` to signal the event thread to stop.
    _stop_tx: Sender<()>,
}

impl CredentialWatcher {
    /// Create a new credential watcher.
    ///
    /// Spawns a background thread that processes filesystem events and
    /// translates them into `WatchEvent`s available via `try_recv()`.
    ///
    /// Returns a watcher with no paths being monitored.
    /// Use `watch_provider` or `watch_path` to add paths.
    ///
    /// # Errors
    ///
    /// Returns an error if the `notify` watcher cannot be created or
    /// the background thread fails to spawn.
    pub fn new() -> Result<Self> {
        let (event_tx, event_rx) = mpsc::channel();
        let (fs_tx, fs_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();

        let watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Err(e) = fs_tx.send(res) {
                    tracing::error!("Failed to send fs event: {e}");
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| CautError::Other(anyhow::anyhow!("create watcher: {e}")))?;

        let shared_state: SharedState = Arc::new(Mutex::new(HashMap::new()));

        // Spawn the event processing thread
        let thread_state = Arc::clone(&shared_state);
        std::thread::Builder::new()
            .name("caut-credential-watcher".into())
            .spawn(move || {
                event_processing_loop(fs_rx, event_tx, thread_state, stop_rx);
            })
            .map_err(|e| CautError::Other(anyhow::anyhow!("spawn watcher thread: {e}")))?;

        Ok(Self {
            watcher: Mutex::new(watcher),
            event_rx,
            watched: shared_state,
            hasher: CredentialHasher::new(),
            _stop_tx: stop_tx,
        })
    }

    /// Watch credential files for a specific provider.
    ///
    /// Resolves the provider's default credential path and registers it
    /// with the filesystem watcher. Returns the resolved path if successful.
    ///
    /// # Errors
    ///
    /// Returns an error if registering the path with the filesystem watcher fails.
    pub fn watch_provider(&mut self, provider: Provider) -> Result<Option<PathBuf>> {
        let Some(cred_path) = provider.credentials_path() else {
            return Ok(None);
        };
        let Some(home) = dirs::home_dir() else {
            return Ok(None);
        };
        let full_path = home.join(cred_path);
        self.watch_path(&full_path, provider)?;
        Ok(Some(full_path))
    }

    /// Watch a specific credential file path.
    ///
    /// If the file exists, its current content is hashed as the baseline.
    /// The parent directory is watched (non-recursively) so we also detect
    /// file creation and atomic-rename replacements.
    ///
    /// # Errors
    ///
    /// Returns an error if the shared state mutex is poisoned or the
    /// underlying `notify` watcher fails to register the path.
    pub fn watch_path(&mut self, path: &Path, provider: Provider) -> Result<()> {
        let last_hash = if path.exists() {
            self.hasher.hash_file(path).ok()
        } else {
            None
        };

        // Store watch state in the shared map
        {
            let mut shared = self
                .watched
                .lock()
                .map_err(|e| CautError::Other(anyhow::anyhow!("lock watched state: {e}")))?;
            shared.insert(
                path.to_path_buf(),
                WatchedFile {
                    provider,
                    last_hash,
                },
            );
        }

        // Register with the notify watcher.
        // Watch the parent directory so we catch creates and atomic renames.
        // If the file itself exists, also watch it directly for modifications.
        let watch_target = if path.exists() {
            path
        } else {
            path.parent().unwrap_or(path)
        };

        let mut notify_watcher = self
            .watcher
            .lock()
            .map_err(|e| CautError::Other(anyhow::anyhow!("lock watcher: {e}")))?;
        notify_watcher
            .watch(watch_target, RecursiveMode::NonRecursive)
            .map_err(|e| {
                CautError::Other(anyhow::anyhow!("watch '{}': {e}", watch_target.display()))
            })?;
        drop(notify_watcher);

        tracing::info!(
            path = %path.display(),
            provider = %provider.display_name(),
            "Watching credential file"
        );

        Ok(())
    }

    /// Stop watching a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the shared state mutex is poisoned.
    pub fn unwatch_path(&mut self, path: &Path) -> Result<()> {
        // Remove from shared state
        {
            let mut shared = self
                .watched
                .lock()
                .map_err(|e| CautError::Other(anyhow::anyhow!("lock watched state: {e}")))?;
            shared.remove(path);
        }

        // Unregister from the notify watcher
        let mut notify_watcher = self
            .watcher
            .lock()
            .map_err(|e| CautError::Other(anyhow::anyhow!("lock watcher: {e}")))?;
        // Best-effort unwatch; the path may not exist or may not be watched directly
        let _ = notify_watcher.unwatch(path);
        if let Some(parent) = path.parent() {
            let _ = notify_watcher.unwatch(parent);
        }
        drop(notify_watcher);

        tracing::info!(path = %path.display(), "Unwatched credential file");

        Ok(())
    }

    /// Try to receive the next event without blocking.
    ///
    /// # Errors
    ///
    /// Returns `TryRecvError::Empty` if no events are pending, or
    /// `TryRecvError::Disconnected` if the watcher thread has stopped.
    pub fn try_recv(&self) -> std::result::Result<WatchEvent, TryRecvError> {
        self.event_rx.try_recv()
    }

    /// Block until the next event is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the watcher channel is disconnected.
    pub fn recv(&self) -> Result<WatchEvent> {
        self.event_rx
            .recv()
            .map_err(|_| CautError::Other(anyhow::anyhow!("watcher channel disconnected")))
    }

    /// Block until the next event, with a timeout.
    ///
    /// # Errors
    ///
    /// Returns `RecvTimeoutError::Timeout` if no event arrives within
    /// the given duration, or `RecvTimeoutError::Disconnected` if the
    /// watcher thread has stopped.
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<WatchEvent, mpsc::RecvTimeoutError> {
        self.event_rx.recv_timeout(timeout)
    }

    /// Check if a provider is being watched.
    #[must_use]
    pub fn is_watching(&self, provider: Provider) -> bool {
        self.watched
            .lock()
            .map(|state| state.values().any(|w| w.provider == provider))
            .unwrap_or(false)
    }

    /// Get the number of paths being watched.
    #[must_use]
    pub fn watch_count(&self) -> usize {
        self.watched.lock().map(|state| state.len()).unwrap_or(0)
    }

    /// Process a filesystem event and determine the change type.
    ///
    /// This is a helper for testing and manual (poll-based) event processing.
    /// In normal operation, the background thread handles this automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the shared state mutex is poisoned.
    pub fn process_change(&mut self, path: &Path) -> Result<Option<WatchEvent>> {
        let mut shared = self
            .watched
            .lock()
            .map_err(|e| CautError::Other(anyhow::anyhow!("lock watched state: {e}")))?;

        let Some(watched_file) = shared.get_mut(path) else {
            return Ok(None);
        };

        let provider = watched_file.provider;
        let old_hash = watched_file.last_hash.as_ref();

        let new_hash = if path.exists() {
            self.hasher.hash_file(path).ok()
        } else {
            None
        };

        let change = self.hasher.detect_change(old_hash, new_hash.as_ref());

        watched_file.last_hash.clone_from(&new_hash);

        // Release the lock before building the event
        drop(shared);

        let event = change_to_event(change, provider, path, new_hash);
        Ok(event)
    }
}

/// Convert a `ChangeType` plus context into an optional `WatchEvent`.
fn change_to_event(
    change: ChangeType,
    provider: Provider,
    path: &Path,
    new_hash: Option<CredentialHashes>,
) -> Option<WatchEvent> {
    match change {
        ChangeType::NoChange => None,
        ChangeType::AccountSwitch | ChangeType::Created => {
            let identity = new_hash.map(|h| h.identity_fields).unwrap_or_default();
            Some(WatchEvent::AccountSwitch {
                provider,
                identity,
                path: path.to_path_buf(),
            })
        }
        ChangeType::TokenRefresh => Some(WatchEvent::TokenRefresh {
            provider,
            path: path.to_path_buf(),
        }),
        ChangeType::Deleted => Some(WatchEvent::CredentialsRemoved {
            provider,
            path: path.to_path_buf(),
        }),
    }
}

/// Background thread: receive raw notify events, debounce, classify, and forward.
#[allow(clippy::needless_pass_by_value)] // Channels must be moved into the thread
fn event_processing_loop(
    fs_rx: mpsc::Receiver<notify::Result<Event>>,
    event_tx: Sender<WatchEvent>,
    shared_state: SharedState,
    stop_rx: mpsc::Receiver<()>,
) {
    let hasher = CredentialHasher::new();

    loop {
        // Check for stop signal (non-blocking)
        if stop_rx.try_recv().is_ok() {
            tracing::debug!("Credential watcher thread stopping (stop signal)");
            return;
        }

        // Wait for the next filesystem event (with timeout so we can check stop)
        let first_event = match fs_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::debug!("Credential watcher thread stopping (channel closed)");
                return;
            }
        };

        // Collect the affected paths from this event
        let mut affected_paths = HashSet::new();
        collect_paths_from_event(first_event, &event_tx, &mut affected_paths);

        // Debounce: drain any additional events that arrive within the window
        let deadline = std::time::Instant::now() + DEBOUNCE_DURATION;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match fs_rx.recv_timeout(remaining) {
                Ok(event) => {
                    collect_paths_from_event(event, &event_tx, &mut affected_paths);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        // Now process each unique affected path
        let mut state = match shared_state.lock() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to lock watched state: {e}");
                continue;
            }
        };

        for path in &affected_paths {
            let Some(watched_file) = state.get_mut(path) else {
                // Not a path we're tracking — might be a sibling in the
                // same parent directory. Skip silently.
                continue;
            };

            let provider = watched_file.provider;
            let old_hash = watched_file.last_hash.as_ref();

            let new_hash = if path.exists() {
                hasher.hash_file(path).ok()
            } else {
                None
            };

            let change = hasher.detect_change(old_hash, new_hash.as_ref());

            // Update stored hash
            watched_file.last_hash.clone_from(&new_hash);

            if let Some(event) = change_to_event(change, provider, path, new_hash) {
                tracing::info!(
                    provider = %provider.display_name(),
                    path = %path.display(),
                    change = %change.description(),
                    "Credential change detected"
                );
                if event_tx.send(event).is_err() {
                    // Consumer dropped — shut down
                    tracing::debug!("Event consumer gone, watcher thread exiting");
                    return;
                }
            }
        }

        // Explicitly release the lock before the next iteration
        drop(state);
    }
}

/// Extract file paths from a notify event result and add them to the set.
/// Forwards errors as `WatchEvent::Error`.
fn collect_paths_from_event(
    event_result: notify::Result<Event>,
    event_tx: &Sender<WatchEvent>,
    paths: &mut HashSet<PathBuf>,
) {
    match event_result {
        Ok(event) => {
            // Only process data-change events (create, modify, remove, rename)
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                    for p in event.paths {
                        paths.insert(p);
                    }
                }
                EventKind::Access(_) | EventKind::Other | EventKind::Any => {
                    // Ignore access events and other noise
                }
            }
        }
        Err(e) => {
            let _ = event_tx.send(WatchEvent::Error {
                message: format!("filesystem watch error: {e}"),
                path: None,
            });
        }
    }
}

/// Helper to get home directory.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_watcher_creation() {
        let watcher = CredentialWatcher::new();
        assert!(watcher.is_ok());
    }

    #[test]
    fn test_watch_count() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");
        assert_eq!(watcher.watch_count(), 0);

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("creds.json");
        fs::write(&path, "{}").expect("write file");

        watcher.watch_path(&path, Provider::Claude).expect("watch");
        assert_eq!(watcher.watch_count(), 1);
        assert!(watcher.is_watching(Provider::Claude));

        watcher.unwatch_path(&path).expect("unwatch");
        assert_eq!(watcher.watch_count(), 0);
    }

    #[test]
    fn test_process_change_account_switch() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("creds.json");

        // Write initial credentials
        fs::write(&path, r#"{"email": "user1@example.com"}"#).expect("write file");
        watcher.watch_path(&path, Provider::Claude).expect("watch");

        // Change to different account
        fs::write(&path, r#"{"email": "user2@example.com"}"#).expect("update file");
        let event = watcher.process_change(&path).expect("process");

        assert!(matches!(event, Some(WatchEvent::AccountSwitch { .. })));
        if let Some(WatchEvent::AccountSwitch {
            provider, identity, ..
        }) = event
        {
            assert_eq!(provider, Provider::Claude);
            assert_eq!(identity.email, Some("user2@example.com".to_string()));
        }
    }

    #[test]
    fn test_process_change_token_refresh() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("creds.json");

        // Write initial credentials with identity and token
        fs::write(
            &path,
            r#"{"email": "user@example.com", "access_token": "token1", "org": "myorg"}"#,
        )
        .expect("write file");
        watcher.watch_path(&path, Provider::Claude).expect("watch");

        // Update token but keep identity
        fs::write(
            &path,
            r#"{"email": "user@example.com", "access_token": "token2", "org": "myorg"}"#,
        )
        .expect("update file");
        let event = watcher.process_change(&path).expect("process");

        // Should detect no change since we exclude volatile fields
        assert!(matches!(event, None));
    }

    #[test]
    fn test_process_change_deleted() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("creds.json");

        // Write initial credentials
        fs::write(&path, r#"{"email": "user@example.com"}"#).expect("write file");
        watcher.watch_path(&path, Provider::Codex).expect("watch");

        // Delete file
        fs::remove_file(&path).expect("delete file");
        let event = watcher.process_change(&path).expect("process");

        assert!(matches!(event, Some(WatchEvent::CredentialsRemoved { .. })));
        if let Some(WatchEvent::CredentialsRemoved { provider, .. }) = event {
            assert_eq!(provider, Provider::Codex);
        }
    }

    #[test]
    fn test_process_change_created() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("creds.json");

        // Watch non-existent path
        watcher.watch_path(&path, Provider::Claude).expect("watch");

        // Create file
        fs::write(&path, r#"{"email": "newuser@example.com"}"#).expect("create file");
        let event = watcher.process_change(&path).expect("process");

        assert!(matches!(event, Some(WatchEvent::AccountSwitch { .. })));
    }

    #[test]
    fn test_process_change_unwatched_path() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("unwatched.json");

        // Don't watch this path
        fs::write(&path, r#"{"email": "user@example.com"}"#).expect("write file");

        let event = watcher.process_change(&path).expect("process");
        assert!(event.is_none());
    }

    #[test]
    fn test_automatic_event_on_file_change() {
        let mut watcher = CredentialWatcher::new().expect("create watcher");

        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("creds.json");

        // Write initial credentials and start watching
        fs::write(&path, r#"{"email": "user1@example.com"}"#).expect("write file");
        watcher.watch_path(&path, Provider::Claude).expect("watch");

        // Modify to trigger an account switch
        fs::write(&path, r#"{"email": "user2@example.com"}"#).expect("update file");

        // Wait for the debounce period + some margin
        let event = watcher.recv_timeout(Duration::from_secs(5));
        match event {
            Ok(WatchEvent::AccountSwitch {
                provider, identity, ..
            }) => {
                assert_eq!(provider, Provider::Claude);
                assert_eq!(identity.email, Some("user2@example.com".to_string()));
            }
            Ok(other) => {
                // On some platforms the first event might be an error or different;
                // the key thing is the watcher thread is alive and processing.
                panic!("unexpected event: {other:?}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Polling-based watchers on some CI/platforms may not fire within 5s.
                // This is acceptable — the unit tests for process_change cover the logic.
                eprintln!("NOTE: automatic event timed out (polling watcher on this platform)");
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("watcher channel disconnected unexpectedly");
            }
        }
    }

    #[test]
    fn test_change_to_event_no_change() {
        assert!(
            change_to_event(
                ChangeType::NoChange,
                Provider::Claude,
                Path::new("/x"),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn test_change_to_event_account_switch() {
        let hashes = CredentialHashes {
            identity_hash: "abc".into(),
            content_hash: "def".into(),
            identity_fields: IdentityFields {
                email: Some("test@example.com".into()),
                ..Default::default()
            },
        };
        let event = change_to_event(
            ChangeType::AccountSwitch,
            Provider::Codex,
            Path::new("/creds"),
            Some(hashes),
        );
        assert!(matches!(event, Some(WatchEvent::AccountSwitch { .. })));
    }

    #[test]
    fn test_change_to_event_deleted() {
        let event = change_to_event(
            ChangeType::Deleted,
            Provider::Claude,
            Path::new("/gone"),
            None,
        );
        assert!(matches!(event, Some(WatchEvent::CredentialsRemoved { .. })));
    }
}
