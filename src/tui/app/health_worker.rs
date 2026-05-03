//! Background reachability worker for the TUI.
//!
//! Owns a list of probe targets (kept in sync with the database) and a
//! detached thread that probes each one on a configurable interval.
//! Results stream back via an `mpsc::Sender<(name, HostStatus)>`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::models::Database;
use crate::tui::app::HostStatus;

/// Shared list of probe targets (name, host, port).
pub type HealthTargets = Arc<Mutex<Vec<(String, String, u16)>>>;

/// RAII guard that flips the worker's stop flag on drop. Ensures the worker
/// shuts down when `run_tui` returns from any code path.
pub struct WorkerGuard(pub Arc<AtomicBool>);
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Refresh the shared target list from the current database.
pub fn sync_health_targets(targets: &HealthTargets, db: &Database) {
    if let Ok(mut t) = targets.lock() {
        t.clear();
        for h in db.hosts.values() {
            t.push((h.name.clone(), h.host.clone(), h.port));
        }
    }
}

/// Spawn the detached background worker that periodically probes every
/// known host and streams results back through `result_tx`. The worker
/// exits within one tick of `stop` being set to `true`.
pub fn spawn_health_worker(
    targets: HealthTargets,
    stop: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    result_tx: mpsc::Sender<(String, HostStatus)>,
    interval_secs: Arc<AtomicU64>,
    probe_timeout_ms: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        // Force an immediate first pass by pretending we're due.
        let mut next_pass = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if !enabled.load(Ordering::Relaxed) {
                next_pass = Instant::now();
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            if Instant::now() >= next_pass {
                let snapshot: Vec<(String, String, u16)> = match targets.lock() {
                    Ok(guard) => guard.clone(),
                    Err(_) => break,
                };
                let probe_timeout = Duration::from_millis(
                    probe_timeout_ms.load(Ordering::Relaxed).max(100),
                );
                for (name, host, port) in snapshot {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let tx = result_tx.clone();
                    let probe_stop = Arc::clone(&stop);
                    thread::spawn(move || {
                        if probe_stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let status = crate::tui::health::probe_host(&host, port, probe_timeout);
                        let _ = tx.send((name, status));
                    });
                }
                let interval = Duration::from_secs(
                    interval_secs.load(Ordering::Relaxed).max(1),
                );
                next_pass = Instant::now() + interval;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}
