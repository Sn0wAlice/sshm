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

/// How many remote probes run at once. Each one shells out and usually opens
/// an SSH connection, so this is a courtesy limit as much as a resource one:
/// somebody with thirty saved remotes should not have sshm open thirty
/// connections every tick.
const MAX_PARALLEL_PROBES: usize = 8;

/// One independent, network-bound lookup for a refresh pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe<'a> {
    DockerRemote { alias: &'a str, uri: &'a str },
    IncusRemote(&'a str),
    Cluster(&'a Cluster),
}

/// Flatten a snapshot into the probes one pass has to make.
///
/// Docker remotes come first: they are the ones a user is most often staring
/// at, and results stream to the UI as each probe finishes.
pub fn collect_probes(snapshot: &WorkerTargets) -> Vec<Probe<'_>> {
    let mut probes = Vec::with_capacity(
        snapshot.docker_remotes.len() + snapshot.incus_remotes.len() + snapshot.clusters.len(),
    );
    for (alias, uri) in &snapshot.docker_remotes {
        probes.push(Probe::DockerRemote { alias, uri });
    }
    for remote in &snapshot.incus_remotes {
        probes.push(Probe::IncusRemote(remote));
    }
    for cluster in &snapshot.clusters {
        probes.push(Probe::Cluster(cluster));
    }
    probes
}

/// Run one probe and push its result. Never fails: an unreachable target
/// reports empty rather than erroring, because the worker polls on a loop and
/// a transient network blip is not news.
fn run_probe(probe: &Probe<'_>, tx: &mpsc::Sender<KlusterUpdate>) {
    match probe {
        Probe::DockerRemote { alias, uri } => {
            let (containers, reachable) = match crate::kluster::docker::list_containers(Some(uri)) {
                Ok(v) => (v, true),
                Err(_) => (Vec::new(), false),
            };
            let _ = tx.send(KlusterUpdate::DockerRemote {
                host_alias: (*alias).to_string(),
                containers,
                reachable,
            });
        }
        Probe::IncusRemote(remote) => {
            let instances =
                crate::kluster::incus::list_instances(Some(remote)).unwrap_or_default();
            let _ = tx.send(KlusterUpdate::IncusRemote {
                remote: (*remote).to_string(),
                instances,
            });
        }
        Probe::Cluster(cluster) => {
            let pods = crate::kluster::kube::list_pods(cluster).unwrap_or_default();
            let _ = tx.send(KlusterUpdate::Cluster {
                cluster_name: cluster.name.clone(),
                pods,
            });
        }
    }
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

                // Everything left is a network round-trip to somewhere else:
                // a Docker daemon over SSH, an Incus remote, a cluster's
                // apiserver. They don't depend on each other, so run them
                // concurrently — serially, one pass cost the *sum* of their
                // latencies, which is what made a handful of remotes visibly
                // slow on a 10s interval.
                let probes = collect_probes(&snapshot);
                for chunk in probes.chunks(MAX_PARALLEL_PROBES) {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::scope(|scope| {
                        for probe in chunk {
                            let tx = result_tx.clone();
                            scope.spawn(move || run_probe(probe, &tx));
                        }
                    });
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                let interval = Duration::from_secs(interval_secs.load(Ordering::Relaxed).max(2));
                next_pass = Instant::now() + interval;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kluster::ClusterKind;

    fn cluster(name: &str) -> Cluster {
        Cluster {
            name: name.to_string(),
            kind: ClusterKind::K8s,
            kubeconfig: None,
            context: None,
            namespace_default: None,
        }
    }

    fn targets() -> WorkerTargets {
        WorkerTargets {
            clusters: vec![cluster("prod"), cluster("stage")],
            incus_remotes: vec!["lab".into()],
            docker_remotes: vec![
                ("web".into(), "ssh://root@10.0.0.1".into()),
                ("db".into(), "ssh://root@10.0.0.2".into()),
            ],
        }
    }

    #[test]
    fn every_target_becomes_exactly_one_probe() {
        let t = targets();
        let probes = collect_probes(&t);
        assert_eq!(probes.len(), 5, "2 docker + 1 incus + 2 clusters");
        assert_eq!(
            probes.iter().filter(|p| matches!(p, Probe::DockerRemote { .. })).count(),
            2
        );
        assert_eq!(probes.iter().filter(|p| matches!(p, Probe::IncusRemote(_))).count(), 1);
        assert_eq!(probes.iter().filter(|p| matches!(p, Probe::Cluster(_))).count(), 2);
    }

    #[test]
    fn docker_remotes_are_probed_first() {
        // Results stream to the UI as each probe lands, so ordering decides
        // what fills in first on the screen.
        let t = targets();
        let probes = collect_probes(&t);
        assert!(matches!(probes[0], Probe::DockerRemote { alias: "web", .. }));
        assert!(matches!(probes[1], Probe::DockerRemote { alias: "db", .. }));
    }

    #[test]
    fn a_docker_probe_carries_the_resolved_uri() {
        // The worker never reads the SSH host DB; the URI is resolved upstream.
        let t = targets();
        let probes = collect_probes(&t);
        match &probes[0] {
            Probe::DockerRemote { alias, uri } => {
                assert_eq!(*alias, "web");
                assert_eq!(*uri, "ssh://root@10.0.0.1");
            }
            other => panic!("expected a docker remote, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_snapshot_produces_no_probes() {
        // A user with only local Docker must not pay for a parallel pass.
        assert!(collect_probes(&WorkerTargets::default()).is_empty());
    }

    #[test]
    fn probes_are_chunked_to_the_parallelism_cap() {
        let t = WorkerTargets {
            clusters: (0..20).map(|i| cluster(&format!("c{i}"))).collect(),
            ..Default::default()
        };
        let probes = collect_probes(&t);
        let chunks: Vec<_> = probes.chunks(MAX_PARALLEL_PROBES).collect();
        assert_eq!(chunks.len(), 3, "20 probes at 8 per pass");
        assert!(chunks.iter().all(|c| c.len() <= MAX_PARALLEL_PROBES));
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 20, "nothing dropped");
    }
}
