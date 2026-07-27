//! Change detection for the sshm config directory.
//!
//! Both frontends (the TUI and the desktop GUI) can run against the same
//! `~/.config/sshm/` files at once. This module watches that directory with the
//! `notify` crate and, after debouncing the inevitable event bursts (editors —
//! and our own atomic write-temp-then-rename — each fire several raw events),
//! forwards one [`DbChanged`] per touched file over a plain `std::sync::mpsc`
//! channel. No async runtime is required, so it stays usable from either
//! frontend (poll it from an event loop, or block on the receiver in a thread).

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher};

/// How long the directory must stay quiet before a burst of raw events is
/// flushed as debounced [`DbChanged`]s.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Which shared file changed on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbChanged {
    /// `host.json` — the host database.
    Hosts,
    /// `kluster.json` — saved clusters / Incus remotes.
    Kluster,
    /// `settings.toml` — app settings.
    Settings,
}

impl DbChanged {
    /// Classify a touched path by its file name. Temp files (`.host.json.tmp-*`),
    /// backups (`host.json.bak`) and anything else in the directory return
    /// `None` and are ignored.
    fn classify(path: &Path) -> Option<DbChanged> {
        match path.file_name().and_then(|s| s.to_str()) {
            Some("host.json") => Some(DbChanged::Hosts),
            Some("kluster.json") => Some(DbChanged::Kluster),
            Some("settings.toml") => Some(DbChanged::Settings),
            _ => None,
        }
    }
}

/// A running watch over the config directory. Dropping it stops the watch; the
/// debounce thread then observes its input close and exits on its own.
pub struct ConfigWatcher {
    // Drop order matters: dropping `_watcher` first ends the raw event stream,
    // which lets the debounce thread notice its input closed and terminate.
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<DbChanged>,
}

impl ConfigWatcher {
    /// Start watching the real sshm config directory (the parent of
    /// `host.json`).
    pub fn start() -> Result<Self> {
        let cfg = crate::config::path::config_path();
        let dir = cfg.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        Self::start_in(&dir)
    }

    /// Start watching an arbitrary directory. Exposed for tests.
    pub fn start_in(dir: &Path) -> Result<Self> {
        let (raw_tx, raw_rx) = mpsc::channel::<DbChanged>();

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    // Ignore pure access/metadata reads; keep create/modify/
                    // remove/rename — those are what an atomic save produces.
                    if matches!(event.kind, EventKind::Access(_)) {
                        return;
                    }
                    for p in &event.paths {
                        if let Some(kind) = DbChanged::classify(p) {
                            let _ = raw_tx.send(kind);
                        }
                    }
                }
            })
            .context("creating filesystem watcher")?;

        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .with_context(|| format!("watching {}", dir.display()))?;

        let (tx, rx) = mpsc::channel::<DbChanged>();
        std::thread::Builder::new()
            .name("sshm-config-debounce".into())
            .spawn(move || debounce_loop(&raw_rx, &tx, DEBOUNCE))
            .context("spawning debounce thread")?;

        Ok(ConfigWatcher { _watcher: watcher, rx })
    }

    /// Non-blocking: drain and return every debounced change pending right now,
    /// deduplicated. Empty when nothing changed. Intended to be called from the
    /// frontend's event loop each tick.
    pub fn poll(&self) -> Vec<DbChanged> {
        let mut seen = Vec::new();
        while let Ok(change) = self.rx.try_recv() {
            if !seen.contains(&change) {
                seen.push(change);
            }
        }
        seen
    }

    /// Borrow the underlying receiver, e.g. to block on it in a dedicated
    /// thread instead of polling.
    pub fn receiver(&self) -> &Receiver<DbChanged> {
        &self.rx
    }
}

/// Coalesce raw events into one [`DbChanged`] per distinct file per quiet
/// window, forwarding on `tx`. Exits when `raw_rx` disconnects (watcher
/// dropped) or the consumer goes away.
fn debounce_loop(raw_rx: &Receiver<DbChanged>, tx: &Sender<DbChanged>, window: Duration) {
    loop {
        let Some(batch) = collect_batch(raw_rx, window) else {
            return; // input closed
        };
        for change in batch {
            if tx.send(change).is_err() {
                return; // consumer gone
            }
        }
    }
}

/// Block for the first raw event, then keep draining until the stream stays
/// quiet for `window`. Returns the deduplicated set of files touched during the
/// burst, or `None` if the input closed before any event arrived.
fn collect_batch(raw_rx: &Receiver<DbChanged>, window: Duration) -> Option<HashSet<DbChanged>> {
    let first = raw_rx.recv().ok()?;
    let mut batch = HashSet::new();
    batch.insert(first);
    loop {
        match raw_rx.recv_timeout(window) {
            Ok(change) => {
                batch.insert(change);
            }
            // Quiet window elapsed, or the source closed mid-burst: flush what
            // we have. (On disconnect the next collect_batch returns None.)
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                return Some(batch)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_matches_only_the_three_db_files() {
        assert_eq!(DbChanged::classify(Path::new("/x/host.json")), Some(DbChanged::Hosts));
        assert_eq!(DbChanged::classify(Path::new("/x/kluster.json")), Some(DbChanged::Kluster));
        assert_eq!(DbChanged::classify(Path::new("/x/settings.toml")), Some(DbChanged::Settings));
    }

    #[test]
    fn classify_ignores_temp_and_backup_files() {
        // The dotfile temp names atomic_write leaves behind, and .bak backups.
        assert_eq!(DbChanged::classify(Path::new("/x/.host.json.tmp-1234-7")), None);
        assert_eq!(DbChanged::classify(Path::new("/x/host.json.bak")), None);
        assert_eq!(DbChanged::classify(Path::new("/x/other.txt")), None);
    }

    #[test]
    fn debounce_collapses_a_burst_to_one_event() {
        let (tx, rx) = mpsc::channel();
        for _ in 0..5 {
            tx.send(DbChanged::Hosts).unwrap();
        }
        let batch = collect_batch(&rx, Duration::from_millis(30)).expect("a batch");
        assert_eq!(batch.len(), 1);
        assert!(batch.contains(&DbChanged::Hosts));
    }

    #[test]
    fn debounce_keeps_distinct_files_separate() {
        let (tx, rx) = mpsc::channel();
        tx.send(DbChanged::Hosts).unwrap();
        tx.send(DbChanged::Settings).unwrap();
        tx.send(DbChanged::Hosts).unwrap();
        let batch = collect_batch(&rx, Duration::from_millis(30)).expect("a batch");
        assert_eq!(batch.len(), 2);
        assert!(batch.contains(&DbChanged::Hosts));
        assert!(batch.contains(&DbChanged::Settings));
    }

    #[test]
    fn debounce_returns_none_when_input_closed() {
        let (tx, rx) = mpsc::channel::<DbChanged>();
        drop(tx);
        assert!(collect_batch(&rx, Duration::from_millis(10)).is_none());
    }

    // Real filesystem + notify + timing: excluded from the normal run (can be
    // flaky under load / in sandboxes). Run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "touches the filesystem and depends on notify timing"]
    fn end_to_end_detects_a_real_write() {
        let dir = tempfile::tempdir().unwrap();
        let w = ConfigWatcher::start_in(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(150)); // let the backend arm

        std::fs::write(dir.path().join("host.json"), "{}").unwrap();

        // Wait up to ~2s for the debounced event to surface.
        let mut got = Vec::new();
        for _ in 0..40 {
            got.extend(w.poll());
            if got.contains(&DbChanged::Hosts) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(got.contains(&DbChanged::Hosts), "expected a Hosts change, got {got:?}");
    }
}
