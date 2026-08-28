//! Machine-wide sync bookkeeping (`~/.config/sshm/sync-state.json`).
//!
//! The schedule lives here rather than in each process so that N running sshm
//! instances behave like one: whoever syncs first writes the timestamp, and the
//! others see the interval as not yet elapsed and skip. It is also what
//! `sshm sync status` and a cron entry (`sshm sync --if-due`) read.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Persisted result of the last sync attempt, shared by every sshm process.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    /// Unix seconds of the last attempt, successful or not.
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
    /// Unix seconds of the last attempt that completed without error.
    #[serde(default)]
    pub last_success_at: Option<i64>,
    /// One-line summary of the last successful run ("2 pulled, 1 pushed").
    #[serde(default)]
    pub last_summary: Option<String>,
    /// Error text of the last failure, cleared by the next success.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Which machine ran it last — handy when the same repo is shared around.
    #[serde(default)]
    pub last_host: Option<String>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl SyncState {
    pub fn load() -> SyncState {
        Self::load_from(&crate::config::path::sync_state_path())
    }

    pub fn load_from(path: &Path) -> SyncState {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&crate::config::path::sync_state_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context("serializing sync state")?;
        crate::config::io::atomic_write(path, json.as_bytes())
            .with_context(|| format!("saving sync state {}", path.display()))
    }

    /// Seconds since the last successful sync, or `None` if there never was one.
    pub fn since_last_success(&self) -> Option<i64> {
        self.last_success_at.map(|t| (now() - t).max(0))
    }

    /// Seconds since the last attempt, successful or not.
    pub fn since_last_attempt(&self) -> Option<i64> {
        self.last_attempt_at.map(|t| (now() - t).max(0))
    }

    /// Whether an interval-scheduled run is due.
    ///
    /// Deliberately keyed on the last *attempt*, not the last success: when a
    /// remote is unreachable we retry on the normal cadence instead of hot
    /// looping on every tick.
    pub fn is_due(&self, interval_secs: u64) -> bool {
        match self.since_last_attempt() {
            None => true,
            Some(elapsed) => elapsed as u64 >= interval_secs,
        }
    }

    pub fn mark_attempt(&mut self) {
        self.last_attempt_at = Some(now());
        self.last_host = Some(super::lock::hostname());
    }

    pub fn mark_success(&mut self, summary: impl Into<String>) {
        let t = now();
        self.last_attempt_at = Some(t);
        self.last_success_at = Some(t);
        self.last_summary = Some(summary.into());
        self.last_error = None;
        self.last_host = Some(super::lock::hostname());
    }

    pub fn mark_error(&mut self, error: impl Into<String>) {
        self.last_attempt_at = Some(now());
        self.last_error = Some(error.into());
        self.last_host = Some(super::lock::hostname());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_never_synced_state_is_always_due() {
        assert!(SyncState::default().is_due(900));
    }

    #[test]
    fn a_recent_attempt_is_not_due() {
        let mut s = SyncState::default();
        s.mark_attempt();
        assert!(!s.is_due(900));
    }

    #[test]
    fn an_old_attempt_is_due_again() {
        let s = SyncState { last_attempt_at: Some(now() - 1000), ..SyncState::default() };
        assert!(s.is_due(900));
        assert!(!s.is_due(2000));
    }

    #[test]
    fn a_failed_attempt_still_pushes_the_schedule_out() {
        // Otherwise an unreachable remote would be retried on every single tick.
        let mut s = SyncState::default();
        s.mark_error("host unreachable");
        assert!(!s.is_due(900));
        assert!(s.last_success_at.is_none());
    }

    #[test]
    fn success_clears_a_previous_error() {
        let mut s = SyncState::default();
        s.mark_error("boom");
        s.mark_success("2 pulled");
        assert!(s.last_error.is_none());
        assert_eq!(s.last_summary.as_deref(), Some("2 pulled"));
        assert!(s.last_success_at.is_some());
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        let mut s = SyncState::default();
        s.mark_success("1 pushed");
        s.save_to(&path).unwrap();

        let back = SyncState::load_from(&path);
        assert_eq!(back.last_summary.as_deref(), Some("1 pushed"));
        assert_eq!(back.last_success_at, s.last_success_at);
    }

    #[test]
    fn a_missing_or_corrupt_state_file_reads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        assert!(SyncState::load_from(&missing).last_attempt_at.is_none());

        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, "{ not json").unwrap();
        assert!(SyncState::load_from(&broken).last_attempt_at.is_none());
    }
}
