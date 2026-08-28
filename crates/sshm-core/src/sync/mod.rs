//! Git-backed sync of the sshm configuration across machines.
//!
//! The user points sshm at an **SSH** git remote of their own (`git@host:me/
//! sshm-config.git`) and an SSH private key; sshm keeps a private working copy
//! in `~/.config/sshm/sync-repo` and, on demand or on a schedule, reconciles
//! `host.json`, `kluster.json`, `theme.toml` and (opt-in) `settings.toml`
//! with it. Nothing is sent anywhere the user did not configure, and the
//! `[sync]` block itself — which holds the key path — never leaves the machine.
//!
//! Three properties this module is built around:
//!
//! * **One syncer at a time.** Several sshm processes (two TUIs, the GUI, a
//!   cron entry) share the same config dir. [`lock::SyncLock`] is an atomic
//!   `O_EXCL` file lock; whoever wins runs, everybody else skips this tick
//!   instead of queueing up behind it.
//! * **One schedule, not N.** The last-run timestamp lives in a shared state
//!   file ([`state::SyncState`]), so five open instances on a 15-minute
//!   interval still produce one sync every 15 minutes.
//! * **Merges, not overwrites.** Hosts and clusters are reconciled
//!   entry-by-entry against the last synced state, so two machines editing
//!   different hosts both keep their work. See [`merge`].
//!
//! Typical use:
//!
//! ```ignore
//! let cfg = load_settings();
//! match sshm_core::sync::sync_if_due(&cfg.sync)? {
//!     SyncRun::Done(report) => println!("{}", report.summary()),
//!     SyncRun::Busy(who)    => println!("another instance (pid {}) is syncing", who.pid),
//!     SyncRun::Skipped(why) => println!("{why}"),
//! }
//! ```

pub mod engine;
pub mod git;
pub mod lock;
pub mod merge;
pub mod state;

use std::time::Duration;

use anyhow::Result;

pub use engine::{preflight, Direction, SyncReport};
pub use lock::{LockInfo, SyncLock};
pub use merge::MergeStats;
pub use state::SyncState;

use crate::config::settings::SyncConfig;

/// How long an interactive `sshm sync` waits for another instance to finish
/// before giving up. Long enough to cover a normal fetch/push round trip.
const CLI_LOCK_WAIT: Duration = Duration::from_secs(20);

/// Outcome of asking sshm to sync.
#[derive(Debug)]
pub enum SyncRun {
    /// A sync actually ran.
    Done(SyncReport),
    /// Another instance holds the lock; this tick was skipped on purpose.
    Busy(LockInfo),
    /// Nothing to do: sync is off, or the interval hasn't elapsed.
    Skipped(&'static str),
}

impl SyncRun {
    /// Human-readable one-liner for a toast or a CLI line.
    pub fn summary(&self) -> String {
        match self {
            SyncRun::Done(r) => r.summary(),
            SyncRun::Busy(info) => format!(
                "another sshm instance is syncing (pid {} on {}, {}s ago)",
                info.pid, info.host, info.age_secs()
            ),
            SyncRun::Skipped(why) => (*why).to_string(),
        }
    }

    pub fn ran(&self) -> bool {
        matches!(self, SyncRun::Done(_))
    }

    /// True when the run actually moved something (worth a toast).
    pub fn changed_anything(&self) -> bool {
        match self {
            SyncRun::Done(r) => !r.updated_locally.is_empty() || r.pushed,
            _ => false,
        }
    }
}

/// Sync right now, whatever the schedule says. Waits briefly for the lock, so
/// an explicit `sshm sync` typed while a background run is in flight does the
/// obvious thing instead of bailing out.
pub fn sync_now(cfg: &SyncConfig) -> Result<SyncRun> {
    sync_with(cfg, Direction::Both, true)
}

/// Sync in one direction only (`sshm sync pull` / `push`).
pub fn sync_direction(cfg: &SyncConfig, direction: Direction) -> Result<SyncRun> {
    sync_with(cfg, direction, true)
}

/// Sync only if enabled, in interval mode, and the interval has elapsed since
/// the last attempt by *any* instance. Never waits for the lock: a tick that
/// collides with another instance's run is simply skipped — that instance is
/// doing the exact same work.
///
/// This is what the TUI's background worker and a `sshm sync --if-due` cron
/// entry both call.
pub fn sync_if_due(cfg: &SyncConfig) -> Result<SyncRun> {
    if !cfg.is_active() {
        return Ok(SyncRun::Skipped("config sync is disabled"));
    }
    let Some(interval) = cfg.effective_interval() else {
        return Ok(SyncRun::Skipped("config sync is set to manual"));
    };
    if !SyncState::load().is_due(interval) {
        return Ok(SyncRun::Skipped("not due yet"));
    }
    sync_with(cfg, Direction::Both, false)
}

/// Shared body of every entry point: take the lock, run, record the outcome.
fn sync_with(cfg: &SyncConfig, direction: Direction, wait_for_lock: bool) -> Result<SyncRun> {
    if !cfg.is_active() {
        return Ok(SyncRun::Skipped("config sync is disabled"));
    }

    let what = match direction {
        Direction::Both => "sync",
        Direction::Pull => "pull",
        Direction::Push => "push",
    };
    let lock = if wait_for_lock {
        SyncLock::acquire_waiting(what, CLI_LOCK_WAIT)?
    } else {
        SyncLock::try_acquire(what)?
    };
    let Some(_lock) = lock else {
        // Report who has it, falling back to a placeholder if they released it
        // between our failed acquire and this read.
        let info = SyncLock::holder().unwrap_or(LockInfo {
            pid: 0,
            host: lock::hostname(),
            acquired_at: chrono::Utc::now().timestamp(),
            what: what.to_string(),
        });
        return Ok(SyncRun::Busy(info));
    };

    // Re-check the schedule now that we hold the lock: another instance may
    // have synced while we were waiting for it.
    if !wait_for_lock {
        if let Some(interval) = cfg.effective_interval() {
            if !SyncState::load().is_due(interval) {
                return Ok(SyncRun::Skipped("not due yet"));
            }
        }
    }

    let mut st = SyncState::load();
    st.mark_attempt();
    let _ = st.save();

    match engine::run(cfg, direction) {
        Ok(report) => {
            let mut st = SyncState::load();
            st.mark_success(report.summary());
            let _ = st.save();
            Ok(SyncRun::Done(report))
        }
        Err(e) => {
            let mut st = SyncState::load();
            st.mark_error(format!("{e:#}"));
            let _ = st.save();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{SyncConfig, SyncMode};

    #[test]
    fn a_disabled_sync_never_runs() {
        let cfg = SyncConfig {
            enabled: false,
            repo_url: "git@example.com:me/conf.git".into(),
            ..SyncConfig::default()
        };
        assert!(matches!(sync_if_due(&cfg).unwrap(), SyncRun::Skipped(_)));
        assert!(matches!(sync_now(&cfg).unwrap(), SyncRun::Skipped(_)));
    }

    #[test]
    fn an_enabled_but_unconfigured_sync_never_runs() {
        let cfg = SyncConfig { enabled: true, ..SyncConfig::default() };
        assert!(matches!(sync_if_due(&cfg).unwrap(), SyncRun::Skipped(_)));
    }

    #[test]
    fn manual_mode_is_skipped_by_the_scheduler() {
        let cfg = SyncConfig {
            enabled: true,
            repo_url: "git@example.com:me/conf.git".into(),
            mode: SyncMode::Manual,
            ..SyncConfig::default()
        };
        match sync_if_due(&cfg).unwrap() {
            SyncRun::Skipped(why) => assert!(why.contains("manual")),
            other => panic!("expected a skip, got {other:?}"),
        }
    }

    #[test]
    fn a_busy_run_summarizes_the_holder() {
        let run = SyncRun::Busy(LockInfo {
            pid: 4242,
            host: "laptop".into(),
            acquired_at: chrono::Utc::now().timestamp(),
            what: "sync".into(),
        });
        let s = run.summary();
        assert!(s.contains("4242") && s.contains("laptop"), "got: {s}");
        assert!(!run.ran());
        assert!(!run.changed_anything());
    }
}
