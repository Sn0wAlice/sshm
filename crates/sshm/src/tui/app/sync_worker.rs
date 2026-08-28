//! Background config-sync worker for the TUI.
//!
//! Wakes up on a slow tick and asks the engine whether a sync is due. The
//! schedule and the "only one syncer" rule both live in `sshm_core::sync`, so
//! running several TUIs (plus the GUI, plus a cron entry) still produces one
//! sync per interval, by whichever instance happens to get there first.
//!
//! Results come back over an `mpsc` channel as a ready-to-show toast message.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::settings::SyncConfig;
use sshm_core::sync::{self, SyncRun};

/// The live sync settings, re-read by the worker on every tick so a change in
/// the Settings tab takes effect without a restart.
pub type SharedSyncConfig = Arc<Mutex<SyncConfig>>;

/// A finished sync, as the event loop wants to display it.
pub enum SyncMsg {
    /// Something actually moved — worth a toast.
    Changed(String),
    /// A sync ran and found nothing to do. Only surfaced for manual runs.
    Quiet(String),
    Failed(String),
}

/// How often the worker re-checks the schedule. The interval itself is
/// enforced by the shared state file, so this only bounds how late a sync can
/// be, and costs a couple of file reads.
const TICK: Duration = Duration::from_secs(5);

/// Spawn the worker. It exits within one tick of `stop` being set.
///
/// Setting `poke` requests an immediate sync regardless of the schedule — used
/// for "sync on start" and for a manual sync triggered from the UI.
pub fn spawn_sync_worker(
    cfg: SharedSyncConfig,
    stop: Arc<AtomicBool>,
    poke: Arc<AtomicBool>,
    tx: mpsc::Sender<SyncMsg>,
) {
    thread::spawn(move || loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let snapshot = match cfg.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => break,
        };
        let forced = poke.swap(false, Ordering::Relaxed);

        if snapshot.is_active() && (forced || snapshot.effective_interval().is_some()) {
            let outcome = if forced {
                sync::sync_now(&snapshot)
            } else {
                sync::sync_if_due(&snapshot)
            };
            match outcome {
                Ok(run) => {
                    let msg = run.summary();
                    let send = match &run {
                        // A skipped tick is the normal case; never toast it.
                        SyncRun::Skipped(_) => None,
                        SyncRun::Busy(_) => forced.then_some(SyncMsg::Quiet(msg)),
                        SyncRun::Done(_) if run.changed_anything() => Some(SyncMsg::Changed(msg)),
                        SyncRun::Done(_) => forced.then_some(SyncMsg::Quiet(msg)),
                    };
                    if let Some(m) = send {
                        if tx.send(m).is_err() {
                            break; // event loop gone
                        }
                    }
                }
                Err(e) => {
                    if tx.send(SyncMsg::Failed(format!("{e:#}"))).is_err() {
                        break;
                    }
                }
            }
        }

        // Sleep in short slices so quitting stays snappy.
        let mut slept = Duration::ZERO;
        while slept < TICK {
            if stop.load(Ordering::Relaxed) || poke.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(250));
            slept += Duration::from_millis(250);
        }
    });
}
