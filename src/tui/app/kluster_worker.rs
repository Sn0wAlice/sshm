//! Background discovery worker for the Kluster tab.
//!
//! Periodically polls `docker ps` and `kubectl get pods` for every saved
//! cluster and pushes results to the main loop via an `mpsc::Sender`. The
//! pattern mirrors `health_worker`: an Arc<AtomicU64> drives the interval
//! so the user can change it from Settings without restarting the app.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::kluster::{Cluster, ContainerInfo, IncusInstance, PodInfo};

/// Shared snapshot of what the worker should poll. The main loop refreshes
/// this whenever `kluster.json` changes; the worker reads it once per cycle.
#[derive(Debug, Clone, Default)]
pub struct WorkerTargets {
    pub clusters: Vec<Cluster>,
    pub incus_remotes: Vec<String>,
    /// Docker remotes resolved at sync time: `(alias, ssh:// URI)`.
    /// We resolve eagerly so the worker doesn't need to know about the SSH
    /// host DB.
    pub docker_remotes: Vec<(String, String)>,
}

pub type KlusterTargets = Arc<Mutex<WorkerTargets>>;

/// One result chunk sent from the worker to the UI.
#[derive(Debug)]
pub enum KlusterUpdate {
    Docker {
        available: bool,
        containers: Vec<ContainerInfo>,
    },
    /// Result for Apple's macOS `container` runtime.
    Apple {
        available: bool,
        containers: Vec<ContainerInfo>,
    },
    /// Result for a remote Docker daemon (keyed by Host alias).
    DockerRemote {
        host_alias: String,
        containers: Vec<ContainerInfo>,
        reachable: bool,
    },
    Cluster {
        cluster_name: String,
        pods: Vec<PodInfo>,
    },
    IncusLocal {
        available: bool,
        instances: Vec<IncusInstance>,
    },
    IncusRemote {
        remote: String,
        instances: Vec<IncusInstance>,
    },
}

pub fn spawn_kluster_worker(
    targets: KlusterTargets,
    stop: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    poke: Arc<AtomicBool>,
    result_tx: mpsc::Sender<KlusterUpdate>,
    interval_secs: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        let mut next_pass = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Paused (e.g. while an interactive SSH session is in the
            // foreground) — hold off polling and clear any pending poke.
            if !enabled.load(Ordering::Relaxed) {
                poke.store(false, Ordering::Relaxed);
                next_pass = Instant::now();
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            let due = Instant::now() >= next_pass;
            let poked = poke.swap(false, Ordering::Relaxed);
            if due || poked {
                // Docker — local
                crate::kluster::docker::invalidate_daemon_cache();
                let available = crate::kluster::docker::daemon_running();
                let containers = if available {
                    crate::kluster::docker::list_containers(None).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let _ = result_tx.send(KlusterUpdate::Docker { available, containers });

                // Apple `container` (macOS) — local only, no remotes.
                crate::kluster::apple::invalidate_cache();
                let apple_avail = crate::kluster::apple::available();
                let apple_containers = if apple_avail {
                    crate::kluster::apple::list_containers().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let _ = result_tx.send(KlusterUpdate::Apple {
                    available: apple_avail,
                    containers: apple_containers,
                });

                // Snapshot the worker targets once per cycle (avoid holding the
                // lock across slow shell-outs).
                let snapshot: WorkerTargets = match targets.lock() {
                    Ok(g) => g.clone(),
                    Err(_) => break,
                };

                // Docker — remote (over SSH)
                for (alias, uri) in &snapshot.docker_remotes {
                    if stop.load(Ordering::Relaxed) { return; }
                    let (containers, reachable) =
                        match crate::kluster::docker::list_containers(Some(uri)) {
                            Ok(v) => (v, true),
                            Err(_) => (Vec::new(), false),
                        };
                    let _ = result_tx.send(KlusterUpdate::DockerRemote {
                        host_alias: alias.clone(),
                        containers,
                        reachable,
                    });
                }

                // Local Incus
                crate::kluster::incus::invalidate_cache();
                let incus_avail = crate::kluster::incus::local_available();
                let incus_local = if incus_avail {
                    crate::kluster::incus::list_instances(None).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let _ = result_tx.send(KlusterUpdate::IncusLocal {
                    available: incus_avail,
                    instances: incus_local,
                });

                // Remote Incus
                for remote in &snapshot.incus_remotes {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let instances = crate::kluster::incus::list_instances(Some(remote))
                        .unwrap_or_default();
                    let _ = result_tx.send(KlusterUpdate::IncusRemote {
                        remote: remote.clone(),
                        instances,
                    });
                }

                // Clusters
                for cluster in &snapshot.clusters {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let pods = crate::kluster::kube::list_pods(cluster).unwrap_or_default();
                    let _ = result_tx.send(KlusterUpdate::Cluster {
                        cluster_name: cluster.name.clone(),
                        pods,
                    });
                }

                let interval = Duration::from_secs(interval_secs.load(Ordering::Relaxed).max(2));
                next_pass = Instant::now() + interval;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}
