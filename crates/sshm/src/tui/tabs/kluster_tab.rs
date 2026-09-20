//! Kluster tab — Docker containers + saved k8s/k3s cluster pods.
//!
//! State holds the in-memory snapshot returned by the background discovery
//! worker; rendering and event handling are stateless and pure (apart from
//! the `selected` cursor).

use crossterm::event::KeyCode;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use std::collections::{HashMap, HashSet};

use crate::kluster::{Cluster, ContainerInfo, IncusInstance, KlusterDb, LifecycleAction, PodInfo};
use crate::tui::theme::Theme;

/// Stable string key used in [`KlusterTabState::collapsed`] to identify a
/// section header. Stable across refreshes (doesn't depend on row index).
fn header_key(row: &KlusterRow) -> Option<String> {
    match row {
        KlusterRow::DockerHeader { .. } => Some("docker".into()),
        KlusterRow::AppleHeader { .. } => Some("apple".into()),
        KlusterRow::PodmanHeader { .. } => Some("podman".into()),
        KlusterRow::DockerRemoteHeader { remote_idx, .. } => {
            Some(format!("docker_remote_{}", remote_idx))
        }
        KlusterRow::IncusLocalHeader { .. } => Some("incus_local".into()),
        KlusterRow::IncusRemoteHeader { remote_idx, .. } => {
            Some(format!("incus_remote_{}", remote_idx))
        }
        KlusterRow::ClusterHeader { cluster_idx, .. } => Some(format!("cluster_{}", cluster_idx)),
        _ => None,
    }
}

/// True for the five section-header row variants.
fn is_header(row: &KlusterRow) -> bool {
    matches!(
        row,
        KlusterRow::DockerHeader { .. }
            | KlusterRow::AppleHeader { .. }
            | KlusterRow::PodmanHeader { .. }
            | KlusterRow::DockerRemoteHeader { .. }
            | KlusterRow::IncusLocalHeader { .. }
            | KlusterRow::IncusRemoteHeader { .. }
            | KlusterRow::ClusterHeader { .. }
    )
}

/// Fuzzy match `text` against `filter` (smart-case, fzf-style). An empty
/// filter matches everything.
fn item_matches(text: &str, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    SkimMatcherV2::default()
        .smart_case()
        .fuzzy_match(text, filter)
        .is_some()
}

/// One renderable row in the left pane. Indices reference the live snapshot
/// stored alongside on `KlusterTabState`.
#[derive(Debug, Clone)]
pub enum KlusterRow {
    DockerHeader {
        count: usize,
        available: bool,
    },
    DockerContainer(usize),
    /// Apple `container` runtime (macOS) — local only.
    AppleHeader {
        count: usize,
        available: bool,
    },
    AppleContainer(usize),
    /// Podman, local daemon. Podman speaks Docker's CLI, so it is the same
    /// snapshot type — only the binary behind it differs.
    PodmanHeader {
        count: usize,
        available: bool,
    },
    PodmanContainer(usize),
    /// One header per saved Docker remote (over SSH). `remote_idx` indexes
    /// `db.docker_remotes`, `reachable` is the last status reported by the
    /// worker.
    DockerRemoteHeader {
        remote_idx: usize,
        count: usize,
        reachable: bool,
    },
    DockerRemoteContainer {
        remote_idx: usize,
        container_idx: usize,
    },
    ClusterHeader {
        cluster_idx: usize,
        count: usize,
    },
    ClusterPod {
        cluster_idx: usize,
        pod_idx: usize,
        /// `Some(name)` when the pod has multiple containers and the user
        /// has expanded a specific one. `None` = use the first container.
        container: Option<String>,
    },
    IncusLocalHeader {
        count: usize,
        available: bool,
    },
    IncusLocalInstance(usize),
    IncusRemoteHeader {
        remote_idx: usize,
        count: usize,
    },
    IncusRemoteInstance {
        remote_idx: usize,
        instance_idx: usize,
    },
}

pub struct KlusterTabState {
    pub db: KlusterDb,
    pub docker_available: bool,
    pub docker_containers: Vec<ContainerInfo>,
    pub apple_available: bool,
    pub apple_containers: Vec<ContainerInfo>,
    pub podman_available: bool,
    pub podman_containers: Vec<ContainerInfo>,
    /// Indexed by `db.clusters[i].name`. `None` = not refreshed yet.
    pub cluster_pods: Vec<Option<Vec<PodInfo>>>,
    pub incus_local_available: bool,
    pub incus_local_instances: Vec<IncusInstance>,
    /// Keyed by remote alias (entries from `db.incus_remotes`).
    pub incus_remote_instances: HashMap<String, Vec<IncusInstance>>,
    /// Resolved `ssh://…` URI for each saved Docker remote (keyed by host_alias).
    /// Filled by `sync_kluster_targets` whenever the host DB or kluster DB changes.
    pub docker_remote_uris: HashMap<String, String>,
    /// Containers reported by each Docker remote in the last refresh round.
    pub docker_remote_containers: HashMap<String, Vec<ContainerInfo>>,
    pub docker_remote_reachable: HashMap<String, bool>,
    pub selected: usize,
    pub flat_rows: Vec<KlusterRow>,
    /// True after the very first refresh round-trip; gates "no daemon" toasts.
    pub bootstrapped: bool,
    pub bootstrap_imported: usize,
    /// Header keys (see [`header_key`]) that are currently collapsed.
    pub collapsed: HashSet<String>,
    /// Fuzzy filter applied to container / pod / instance rows. Empty = no
    /// filter. While non-empty, sections are force-expanded and headers with
    /// no matching child are hidden.
    pub filter: String,
    /// True while the user is typing into [`Self::filter`] (entered with `/`).
    pub input_mode: bool,
}

impl Default for KlusterTabState {
    fn default() -> Self {
        Self::new()
    }
}

impl KlusterTabState {
    pub fn new() -> Self {
        let (db, imported) = crate::kluster::db::load_or_bootstrap();
        let mut state = Self::from_db(db);
        state.bootstrap_imported = imported;
        state
    }

    /// Build a state around an explicit [`KlusterDb`], touching no disk.
    /// [`Self::new`] is this plus `load_or_bootstrap()`. Kept separate so the
    /// row/selection logic can be exercised without a config directory.
    pub fn from_db(db: KlusterDb) -> Self {
        let cluster_pods = vec![None; db.clusters.len()];
        // Collapse k8s/k3s cluster sections by default — they often hold 50+
        // pods and the noise hides everything else. Docker/Incus stay open.
        let collapsed: HashSet<String> = (0..db.clusters.len())
            .map(|i| format!("cluster_{}", i))
            .collect();
        let mut state = KlusterTabState {
            db,
            docker_available: false,
            docker_containers: Vec::new(),
            apple_available: false,
            apple_containers: Vec::new(),
            podman_available: false,
            podman_containers: Vec::new(),
            cluster_pods,
            incus_local_available: false,
            incus_local_instances: Vec::new(),
            incus_remote_instances: HashMap::new(),
            docker_remote_uris: HashMap::new(),
            docker_remote_containers: HashMap::new(),
            docker_remote_reachable: HashMap::new(),
            selected: 0,
            flat_rows: Vec::new(),
            bootstrapped: false,
            bootstrap_imported: 0,
            collapsed,
            filter: String::new(),
            input_mode: false,
        };
        state.rebuild_rows();
        state
    }

    /// Recompute `flat_rows` from the current snapshot. Called every time
    /// the worker pushes new data, and after a collapse toggle.
    pub fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        // While a filter is active, every section is force-expanded so matches
        // hidden inside collapsed sections still surface.
        let filtering = !self.filter.is_empty();
        let docker_h = KlusterRow::DockerHeader {
            count: self.docker_containers.len(),
            available: self.docker_available,
        };
        let docker_collapsed = !filtering && self.collapsed.contains("docker");
        rows.push(docker_h);
        if self.docker_available && !docker_collapsed {
            for i in 0..self.docker_containers.len() {
                rows.push(KlusterRow::DockerContainer(i));
            }
        }
        // Apple `container` (macOS). Only shown once the runtime reports as
        // available, so Linux users never see an "unavailable" line.
        if self.apple_available {
            let apple_collapsed = !filtering && self.collapsed.contains("apple");
            rows.push(KlusterRow::AppleHeader {
                count: self.apple_containers.len(),
                available: true,
            });
            if !apple_collapsed {
                for i in 0..self.apple_containers.len() {
                    rows.push(KlusterRow::AppleContainer(i));
                }
            }
        }
        // Podman, same rule as Apple: only shown when it is actually there, so
        // the far more common "no podman" machine sees nothing at all rather
        // than a permanent "(unavailable)" line.
        if self.podman_available {
            let podman_collapsed = !filtering && self.collapsed.contains("podman");
            rows.push(KlusterRow::PodmanHeader {
                count: self.podman_containers.len(),
                available: true,
            });
            if !podman_collapsed {
                for i in 0..self.podman_containers.len() {
                    rows.push(KlusterRow::PodmanContainer(i));
                }
            }
        }
        // Local Incus section. Kept with the other locals, above every remote.
        let incus_local_h = KlusterRow::IncusLocalHeader {
            count: self.incus_local_instances.len(),
            available: self.incus_local_available,
        };
        let incus_local_collapsed = !filtering && self.collapsed.contains("incus_local");
        rows.push(incus_local_h);
        if self.incus_local_available && !incus_local_collapsed {
            for i in 0..self.incus_local_instances.len() {
                rows.push(KlusterRow::IncusLocalInstance(i));
            }
        }

        // ---- Remotes, below every local section ----
        // Remote Docker daemons (over SSH).
        for (ri, remote) in self.db.docker_remotes.iter().enumerate() {
            let containers = self.docker_remote_containers.get(&remote.host_alias);
            let count = containers.map(|v| v.len()).unwrap_or(0);
            let reachable = self
                .docker_remote_reachable
                .get(&remote.host_alias)
                .copied()
                .unwrap_or(false);
            let key = format!("docker_remote_{}", ri);
            let is_collapsed = !filtering && self.collapsed.contains(&key);
            rows.push(KlusterRow::DockerRemoteHeader {
                remote_idx: ri,
                count,
                reachable,
            });
            if !is_collapsed && reachable {
                if let Some(list) = containers {
                    for ii in 0..list.len() {
                        rows.push(KlusterRow::DockerRemoteContainer {
                            remote_idx: ri,
                            container_idx: ii,
                        });
                    }
                }
            }
        }
        // Remote Incus sections.
        for (ri, remote) in self.db.incus_remotes.iter().enumerate() {
            let count = self
                .incus_remote_instances
                .get(remote)
                .map(|v| v.len())
                .unwrap_or(0);
            let key = format!("incus_remote_{}", ri);
            let is_collapsed = !filtering && self.collapsed.contains(&key);
            rows.push(KlusterRow::IncusRemoteHeader {
                remote_idx: ri,
                count,
            });
            if !is_collapsed {
                if let Some(list) = self.incus_remote_instances.get(remote) {
                    for ii in 0..list.len() {
                        rows.push(KlusterRow::IncusRemoteInstance {
                            remote_idx: ri,
                            instance_idx: ii,
                        });
                    }
                }
            }
        }
        for (ci, _cluster) in self.db.clusters.iter().enumerate() {
            let pods = self.cluster_pods.get(ci).and_then(|x| x.as_ref());
            let count = pods.map(|p| p.len()).unwrap_or(0);
            let key = format!("cluster_{}", ci);
            let is_collapsed = !filtering && self.collapsed.contains(&key);
            rows.push(KlusterRow::ClusterHeader {
                cluster_idx: ci,
                count,
            });
            if !is_collapsed {
                if let Some(pods) = pods {
                    for (pi, _pod) in pods.iter().enumerate() {
                        rows.push(KlusterRow::ClusterPod {
                            cluster_idx: ci,
                            pod_idx: pi,
                            container: None,
                        });
                    }
                }
            }
        }
        if filtering {
            rows = self.apply_row_filter(rows);
        }
        self.flat_rows = rows;
        if self.selected >= self.flat_rows.len() {
            self.selected = self.flat_rows.len().saturating_sub(1);
        }
    }

    /// Drop item rows that don't fuzzy-match [`Self::filter`], and drop any
    /// section header left with no matching child. Assumes `rows` is the fully
    /// expanded layout (headers immediately followed by their items).
    fn apply_row_filter(&self, rows: Vec<KlusterRow>) -> Vec<KlusterRow> {
        let mut out: Vec<KlusterRow> = Vec::new();
        let mut pending_header: Option<KlusterRow> = None;
        for row in rows {
            if is_header(&row) {
                // A new header supersedes any previous header that never got
                // a match (so empty sections are dropped while filtering).
                pending_header = Some(row);
            } else if self.row_item_matches(&row) {
                if let Some(h) = pending_header.take() {
                    out.push(h);
                }
                out.push(row);
            }
        }
        out
    }

    /// True when the item on `row` fuzzy-matches the current filter. Headers
    /// and unknown rows return false.
    fn row_item_matches(&self, row: &KlusterRow) -> bool {
        let text: Option<String> = match row {
            KlusterRow::DockerContainer(i) => self
                .docker_containers
                .get(*i)
                .map(|c| format!("{} {}", c.name, c.image)),
            KlusterRow::AppleContainer(i) => self
                .apple_containers
                .get(*i)
                .map(|c| format!("{} {}", c.name, c.image)),
            KlusterRow::PodmanContainer(i) => self
                .podman_containers
                .get(*i)
                .map(|c| format!("{} {}", c.name, c.image)),
            KlusterRow::DockerRemoteContainer {
                remote_idx,
                container_idx,
            } => self
                .db
                .docker_remotes
                .get(*remote_idx)
                .and_then(|r| self.docker_remote_containers.get(&r.host_alias))
                .and_then(|v| v.get(*container_idx))
                .map(|c| format!("{} {}", c.name, c.image)),
            KlusterRow::ClusterPod {
                cluster_idx,
                pod_idx,
                ..
            } => self
                .cluster_pods
                .get(*cluster_idx)
                .and_then(|x| x.as_ref())
                .and_then(|p| p.get(*pod_idx))
                .map(|p| format!("{} {}", p.namespace, p.name)),
            KlusterRow::IncusLocalInstance(i) => self
                .incus_local_instances
                .get(*i)
                .map(|inst| format!("{} {}", inst.name, inst.image)),
            KlusterRow::IncusRemoteInstance {
                remote_idx,
                instance_idx,
            } => self
                .db
                .incus_remotes
                .get(*remote_idx)
                .and_then(|r| self.incus_remote_instances.get(r))
                .and_then(|v| v.get(*instance_idx))
                .map(|inst| format!("{} {}", inst.name, inst.image)),
            _ => return false,
        };
        match text {
            Some(t) => item_matches(&t, &self.filter),
            None => false,
        }
    }

    /// Re-pack `collapsed` keys after a deletion at `deleted_idx` for entries
    /// matching `prefix` (e.g. `"cluster_"`). Drops the deleted key and
    /// shifts higher indices down by one. Other unrelated keys are kept.
    pub fn shift_collapsed_after_delete(&mut self, prefix: &str, deleted_idx: usize) {
        let mut next = HashSet::new();
        for key in self.collapsed.drain() {
            if let Some(rest) = key.strip_prefix(prefix) {
                if let Ok(n) = rest.parse::<usize>() {
                    if n == deleted_idx {
                        continue;
                    }
                    let new_n = if n > deleted_idx { n - 1 } else { n };
                    next.insert(format!("{}{}", prefix, new_n));
                    continue;
                }
            }
            next.insert(key);
        }
        self.collapsed = next;
    }

    /// Toggle the collapsed state of the header on the current row.
    /// No-op if the cursor isn't on a header.
    pub fn toggle_collapsed_at_selected(&mut self) {
        let key = match self.flat_rows.get(self.selected) {
            Some(row) => header_key(row),
            None => None,
        };
        if let Some(k) = key {
            if !self.collapsed.remove(&k) {
                self.collapsed.insert(k);
            }
            self.rebuild_rows();
        }
    }

    /// Returns the actionable target on the current row, or None for headers.
    pub fn current_target(&self) -> Option<KlusterTarget<'_>> {
        let row = self.flat_rows.get(self.selected)?;
        match row {
            KlusterRow::DockerContainer(i) => {
                self.docker_containers.get(*i).map(KlusterTarget::Docker)
            }
            KlusterRow::AppleContainer(i) => {
                self.apple_containers.get(*i).map(KlusterTarget::Apple)
            }
            KlusterRow::PodmanContainer(i) => {
                self.podman_containers.get(*i).map(KlusterTarget::Podman)
            }
            KlusterRow::DockerRemoteContainer {
                remote_idx,
                container_idx,
            } => {
                let remote = self.db.docker_remotes.get(*remote_idx)?;
                let host_uri = self.docker_remote_uris.get(&remote.host_alias)?;
                let containers = self.docker_remote_containers.get(&remote.host_alias)?;
                let container = containers.get(*container_idx)?;
                Some(KlusterTarget::DockerRemote {
                    container,
                    host_uri,
                })
            }
            KlusterRow::ClusterPod {
                cluster_idx,
                pod_idx,
                container,
            } => {
                let cluster = self.db.clusters.get(*cluster_idx)?;
                let pod = self
                    .cluster_pods
                    .get(*cluster_idx)?
                    .as_ref()?
                    .get(*pod_idx)?;
                Some(KlusterTarget::Pod {
                    cluster,
                    pod,
                    container: container.as_deref(),
                })
            }
            KlusterRow::IncusLocalInstance(i) => {
                self.incus_local_instances
                    .get(*i)
                    .map(|inst| KlusterTarget::Incus {
                        instance: inst,
                        remote: None,
                    })
            }
            KlusterRow::IncusRemoteInstance {
                remote_idx,
                instance_idx,
            } => {
                let remote = self.db.incus_remotes.get(*remote_idx)?;
                let instance = self
                    .incus_remote_instances
                    .get(remote)?
                    .get(*instance_idx)?;
                Some(KlusterTarget::Incus {
                    instance,
                    remote: Some(remote.as_str()),
                })
            }
            _ => None,
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.flat_rows.len() {
            self.selected += 1;
        }
    }
    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

/// Resolved target the action handlers in `app::mod` work with.
pub enum KlusterTarget<'a> {
    Docker(&'a ContainerInfo),
    /// Container on Apple's macOS `container` runtime.
    Apple(&'a ContainerInfo),
    /// Container on a local Podman engine.
    Podman(&'a ContainerInfo),
    /// Container running on a remote Docker daemon reached via SSH.
    /// `host_uri` is the `ssh://user@host:port` value to set as `DOCKER_HOST`.
    DockerRemote {
        container: &'a ContainerInfo,
        host_uri: &'a str,
    },
    Pod {
        cluster: &'a Cluster,
        pod: &'a PodInfo,
        container: Option<&'a str>,
    },
    /// `remote = None` ⇒ local Incus daemon (no `<remote>:` prefix).
    Incus {
        instance: &'a IncusInstance,
        remote: Option<&'a str>,
    },
}

#[derive(Debug)]
pub enum KlusterAction {
    None,
    Refresh,
    OpenShell,
    /// Stream logs with `-f` (follow). The only logs hotkey — `l` — uses
    /// this; Ctrl+C in the foreground returns to the TUI.
    OpenLogsFollow,
    AddCluster,
    EditCluster,
    DeleteCluster,
    /// `kubectl delete pod` — only fired on terminated pods (Succeeded / Failed).
    DeletePod,
    /// Open a picker to register a new Docker remote (a saved Host that runs Docker).
    AddDockerRemote,
    /// Remove a Docker remote entry (the SSH host itself is unaffected).
    DeleteDockerRemote,
    /// Start / stop / restart the selected Docker container or Incus instance.
    Lifecycle(LifecycleAction),
    /// Open the rich detail view (inspect) for the selected item.
    OpenDetail,
}

/// `Some(running)` for a Docker container or Incus instance under the cursor
/// (i.e. a row that supports start/stop/restart), `None` for pods and headers.
fn lifecycle_running(state: &KlusterTabState) -> Option<bool> {
    match state.current_target()? {
        KlusterTarget::Docker(c) => Some(c.running),
        KlusterTarget::Apple(c) => Some(c.running),
        KlusterTarget::Podman(c) => Some(c.running),
        KlusterTarget::DockerRemote { container, .. } => Some(container.running),
        KlusterTarget::Incus { instance, .. } => Some(instance.running),
        KlusterTarget::Pod { .. } => None,
    }
}

pub fn handle_kluster_event(key: KeyCode, state: &mut KlusterTabState) -> KlusterAction {
    // While typing a filter, keystrokes edit the query; arrows still navigate.
    if state.input_mode {
        match key {
            KeyCode::Esc => {
                state.input_mode = false;
                state.filter.clear();
                state.selected = 0;
                state.rebuild_rows();
            }
            KeyCode::Enter => state.input_mode = false,
            KeyCode::Backspace => {
                state.filter.pop();
                state.selected = 0;
                state.rebuild_rows();
            }
            KeyCode::Up => state.move_up(),
            KeyCode::Down => state.move_down(),
            KeyCode::Char(c) => {
                state.filter.push(c);
                state.selected = 0;
                state.rebuild_rows();
            }
            _ => {}
        }
        return KlusterAction::None;
    }

    // `/` opens the filter; Esc clears an already-applied filter.
    if key == KeyCode::Char('/') {
        state.input_mode = true;
        state.filter.clear();
        state.selected = 0;
        state.rebuild_rows();
        return KlusterAction::None;
    }
    if key == KeyCode::Esc && !state.filter.is_empty() {
        state.filter.clear();
        state.selected = 0;
        state.rebuild_rows();
        return KlusterAction::None;
    }

    let row = state.flat_rows.get(state.selected);
    let on_item = matches!(
        row,
        Some(KlusterRow::DockerContainer(_))
            | Some(KlusterRow::AppleContainer(_))
            | Some(KlusterRow::PodmanContainer(_))
            | Some(KlusterRow::DockerRemoteContainer { .. })
            | Some(KlusterRow::ClusterPod { .. })
            | Some(KlusterRow::IncusLocalInstance(_))
            | Some(KlusterRow::IncusRemoteInstance { .. })
    );
    let on_header = matches!(
        row,
        Some(KlusterRow::DockerHeader { .. })
            | Some(KlusterRow::AppleHeader { .. })
            | Some(KlusterRow::PodmanHeader { .. })
            | Some(KlusterRow::DockerRemoteHeader { .. })
            | Some(KlusterRow::IncusLocalHeader { .. })
            | Some(KlusterRow::IncusRemoteHeader { .. })
            | Some(KlusterRow::ClusterHeader { .. })
    );
    let on_docker_remote_header = matches!(row, Some(KlusterRow::DockerRemoteHeader { .. }));
    let on_cluster_header = matches!(row, Some(KlusterRow::ClusterHeader { .. }));
    let on_terminal_pod = matches!(row, Some(KlusterRow::ClusterPod { .. }))
        && state
            .current_target()
            .as_ref()
            .map(|t| {
                if let KlusterTarget::Pod { pod, .. } = t {
                    pod.phase.eq_ignore_ascii_case("Succeeded")
                        || pod.phase.eq_ignore_ascii_case("Failed")
                } else {
                    false
                }
            })
            .unwrap_or(false);

    match key {
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_up();
            KlusterAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_down();
            KlusterAction::None
        }
        KeyCode::Char('r') => KlusterAction::Refresh,
        // `n` is context-aware: on a docker (local or remote) header, register
        // a new Docker remote; everywhere else it adds a k8s/k3s cluster.
        KeyCode::Char('n') => match row {
            Some(KlusterRow::DockerHeader { .. })
            | Some(KlusterRow::DockerRemoteHeader { .. })
            | Some(KlusterRow::DockerContainer(_))
            | Some(KlusterRow::DockerRemoteContainer { .. }) => KlusterAction::AddDockerRemote,
            _ => KlusterAction::AddCluster,
        },
        // Headers: Enter (and Space) toggles collapse.
        KeyCode::Enter | KeyCode::Char(' ') if on_header => {
            state.toggle_collapsed_at_selected();
            KlusterAction::None
        }
        // Item-only actions
        KeyCode::Enter if on_item => KlusterAction::OpenShell,
        KeyCode::Char('i') if on_item => KlusterAction::OpenDetail,
        KeyCode::Char('l') if on_item => KlusterAction::OpenLogsFollow,
        // `s` toggles start/stop on a Docker/Incus item; `R` restarts it.
        // Both no-op on pods (k8s has no equivalent — use `d` to delete).
        KeyCode::Char('s') if on_item => match lifecycle_running(state) {
            Some(true) => KlusterAction::Lifecycle(LifecycleAction::Stop),
            Some(false) => KlusterAction::Lifecycle(LifecycleAction::Start),
            None => KlusterAction::None,
        },
        KeyCode::Char('R') if on_item => match lifecycle_running(state) {
            Some(_) => KlusterAction::Lifecycle(LifecycleAction::Restart),
            None => KlusterAction::None,
        },
        // Cluster header CRUD
        KeyCode::Char('e') if on_cluster_header => KlusterAction::EditCluster,
        KeyCode::Char('d') if on_cluster_header => KlusterAction::DeleteCluster,
        // Docker remote: `d` on its header removes the entry (SSH host stays).
        KeyCode::Char('d') if on_docker_remote_header => KlusterAction::DeleteDockerRemote,
        // Pod-level cleanup: `d` on a Succeeded/Failed pod deletes it.
        KeyCode::Char('d') if on_terminal_pod => KlusterAction::DeletePod,
        _ => KlusterAction::None,
    }
}

pub fn draw_kluster_tab(f: &mut Frame, area: Rect, state: &KlusterTabState, theme: &Theme) {
    let items: Vec<ListItem> = state
        .flat_rows
        .iter()
        .map(|row| render_row(row, state, theme))
        .collect();

    let mut ls = ListState::default();
    if !state.flat_rows.is_empty() {
        ls.select(Some(state.selected));
    }
    let title = if state.input_mode {
        format!("Kluster — filter: {}▏", state.filter)
    } else if !state.filter.is_empty() {
        let matches = state.flat_rows.iter().filter(|r| !is_header(r)).count();
        format!("Kluster — filter: {} ({} match)", state.filter, matches)
    } else {
        "Kluster — Docker + clusters".to_string()
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg)),
        )
        .highlight_symbol("➜ ")
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, area, &mut ls);
}

fn render_row<'a>(row: &KlusterRow, state: &KlusterTabState, theme: &Theme) -> ListItem<'a> {
    match row {
        KlusterRow::DockerHeader { count, available } => {
            let glyph = if state.collapsed.contains("docker") {
                "▸"
            } else {
                "▾"
            };
            let label = if *available {
                format!("{} Docker (local) ({})", glyph, count)
            } else {
                format!("{} Docker (local) (unavailable)", glyph)
            };
            let style = if *available {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        }
        KlusterRow::DockerContainer(i) => {
            let c = &state.docker_containers[*i];
            render_docker_container(c, theme)
        }
        KlusterRow::AppleHeader { count, available } => {
            let glyph = if state.collapsed.contains("apple") {
                "▸"
            } else {
                "▾"
            };
            let label = if *available {
                format!("{} Apple container (local) ({})", glyph, count)
            } else {
                format!("{} Apple container (local) (unavailable)", glyph)
            };
            let style = if *available {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        }
        KlusterRow::AppleContainer(i) => {
            let c = &state.apple_containers[*i];
            render_docker_container(c, theme)
        }
        KlusterRow::PodmanHeader { count, available } => {
            let glyph = if state.collapsed.contains("podman") {
                "▸"
            } else {
                "▾"
            };
            let label = if *available {
                format!("{} Podman (local) ({})", glyph, count)
            } else {
                format!("{} Podman (local) (unavailable)", glyph)
            };
            let style = if *available {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        }
        KlusterRow::PodmanContainer(i) => {
            // Podman's `ps` output is Docker's, so the row renders identically.
            let c = &state.podman_containers[*i];
            render_docker_container(c, theme)
        }
        KlusterRow::DockerRemoteHeader {
            remote_idx,
            count,
            reachable,
        } => {
            let remote = &state.db.docker_remotes[*remote_idx];
            let key = format!("docker_remote_{}", remote_idx);
            let glyph = if state.collapsed.contains(&key) {
                "▸"
            } else {
                "▾"
            };
            let suffix = if *reachable {
                format!("({})", count)
            } else {
                "(unreachable)".to_string()
            };
            let style = if *reachable {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{} Docker (remote {}) {}", glyph, remote.host_alias, suffix),
                style,
            )))
        }
        KlusterRow::DockerRemoteContainer {
            remote_idx,
            container_idx,
        } => {
            let remote = &state.db.docker_remotes[*remote_idx];
            let containers = state.docker_remote_containers.get(&remote.host_alias);
            match containers.and_then(|v| v.get(*container_idx)) {
                Some(c) => render_docker_container(c, theme),
                None => ListItem::new(Span::raw("    ?")),
            }
        }
        KlusterRow::ClusterHeader { cluster_idx, count } => {
            let cluster = &state.db.clusters[*cluster_idx];
            let key = format!("cluster_{}", cluster_idx);
            let glyph = if state.collapsed.contains(&key) {
                "▸"
            } else {
                "▾"
            };
            let label = format!(
                "{} Cluster: {} ({})  [{}]",
                glyph,
                cluster.name,
                count,
                cluster.kind.label()
            );
            ListItem::new(Line::from(Span::styled(
                label,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )))
        }
        KlusterRow::ClusterPod {
            cluster_idx,
            pod_idx,
            ..
        } => {
            let pods = state.cluster_pods[*cluster_idx].as_ref().unwrap();
            let pod = &pods[*pod_idx];
            let phase_style = match pod.phase.as_str() {
                "Running" => Style::default().fg(theme.success),
                "Pending" => Style::default().fg(theme.muted),
                _ => Style::default().fg(theme.error),
            };
            let containers_repr = if pod.containers.is_empty() {
                String::new()
            } else {
                format!("[{}]", pod.containers.join(", "))
            };
            ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("{}/", pod.namespace),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(
                    pod.name.clone(),
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(format!("● {} ", pod.phase), phase_style),
                Span::styled(containers_repr, Style::default().fg(theme.muted)),
            ]))
        }
        KlusterRow::IncusLocalHeader { count, available } => {
            let glyph = if state.collapsed.contains("incus_local") {
                "▸"
            } else {
                "▾"
            };
            let label = if *available {
                format!("{} Incus (local) ({})", glyph, count)
            } else {
                format!("{} Incus (local) (unavailable)", glyph)
            };
            let style = if *available {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        }
        KlusterRow::IncusLocalInstance(i) => {
            let inst = &state.incus_local_instances[*i];
            render_incus_instance(inst, theme)
        }
        KlusterRow::IncusRemoteHeader { remote_idx, count } => {
            let remote = &state.db.incus_remotes[*remote_idx];
            let key = format!("incus_remote_{}", remote_idx);
            let glyph = if state.collapsed.contains(&key) {
                "▸"
            } else {
                "▾"
            };
            ListItem::new(Line::from(Span::styled(
                format!("{} Incus (remote {}) ({})", glyph, remote, count),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )))
        }
        KlusterRow::IncusRemoteInstance {
            remote_idx,
            instance_idx,
        } => {
            let remote = &state.db.incus_remotes[*remote_idx];
            let inst = &state.incus_remote_instances[remote][*instance_idx];
            render_incus_instance(inst, theme)
        }
    }
}

fn render_docker_container<'a>(c: &ContainerInfo, theme: &Theme) -> ListItem<'a> {
    let glyph = if c.running { "●" } else { "○" };
    let glyph_style = if c.running {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.muted)
    };
    ListItem::new(Line::from(vec![
        Span::raw("    "),
        Span::styled(format!("{} ", glyph), glyph_style),
        Span::styled(
            c.name.clone(),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(c.image.clone(), Style::default().fg(theme.muted)),
        Span::raw("  "),
        Span::styled(c.status.clone(), Style::default().fg(theme.muted)),
    ]))
}

fn render_incus_instance<'a>(inst: &IncusInstance, theme: &Theme) -> ListItem<'a> {
    let glyph = if inst.running { "●" } else { "○" };
    let glyph_style = if inst.running {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.muted)
    };
    let kind_short = if inst.kind.starts_with("virtual") {
        "vm"
    } else {
        "ct"
    };
    ListItem::new(Line::from(vec![
        Span::raw("    "),
        Span::styled(format!("{} ", glyph), glyph_style),
        Span::styled(
            inst.name.clone(),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("[{}]", kind_short),
            Style::default().fg(theme.muted),
        ),
        Span::raw("  "),
        Span::styled(inst.image.clone(), Style::default().fg(theme.muted)),
        Span::raw("  "),
        Span::styled(inst.status.clone(), Style::default().fg(theme.muted)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kluster::{ClusterKind, DockerRemote};

    // ---- fixtures --------------------------------------------------------

    fn container(name: &str, running: bool) -> ContainerInfo {
        ContainerInfo {
            id: format!("id-{name}"),
            name: name.to_string(),
            image: "alpine".into(),
            status: if running {
                "Up 2 minutes".into()
            } else {
                "Exited (0)".into()
            },
            running,
        }
    }

    fn instance(name: &str, running: bool) -> IncusInstance {
        IncusInstance {
            name: name.to_string(),
            status: if running {
                "RUNNING".into()
            } else {
                "STOPPED".into()
            },
            kind: "container".into(),
            image: String::new(),
            running,
        }
    }

    fn pod(name: &str, phase: &str) -> PodInfo {
        PodInfo {
            namespace: "default".into(),
            name: name.to_string(),
            containers: vec!["app".into()],
            phase: phase.to_string(),
        }
    }

    fn cluster(name: &str) -> Cluster {
        Cluster {
            name: name.to_string(),
            kind: ClusterKind::K8s,
            kubeconfig: None,
            context: None,
            namespace_default: None,
        }
    }

    /// A state with local Docker (2 containers) and one cluster (2 pods),
    /// everything expanded.
    fn state() -> KlusterTabState {
        let db = KlusterDb {
            clusters: vec![cluster("prod")],
            incus_remotes: vec![],
            docker_remotes: vec![],
        };
        let mut s = KlusterTabState::from_db(db);
        s.docker_available = true;
        s.docker_containers = vec![container("web", true), container("cache", false)];
        s.cluster_pods = vec![Some(vec![
            pod("api-1", "Running"),
            pod("job-9", "Succeeded"),
        ])];
        s.collapsed.clear();
        s.rebuild_rows();
        s
    }

    /// Move the cursor to the first row satisfying `pred`.
    fn select<F: Fn(&KlusterRow) -> bool>(s: &mut KlusterTabState, pred: F) {
        s.selected = s
            .flat_rows
            .iter()
            .position(pred)
            .unwrap_or_else(|| panic!("no matching row in {:?}", s.flat_rows));
    }

    fn press(s: &mut KlusterTabState, c: char) -> KlusterAction {
        handle_kluster_event(KeyCode::Char(c), s)
    }

    // ---- cursor → target -------------------------------------------------

    #[test]
    fn a_header_row_has_no_target() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerHeader { .. }));
        assert!(s.current_target().is_none());
    }

    #[test]
    fn the_cursor_resolves_to_the_container_under_it() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerContainer(_)));
        match s.current_target() {
            Some(KlusterTarget::Docker(c)) => assert_eq!(c.name, "web"),
            _ => panic!("expected the first docker container"),
        }
        s.selected += 1;
        match s.current_target() {
            Some(KlusterTarget::Docker(c)) => assert_eq!(c.name, "cache"),
            _ => panic!("expected the second docker container"),
        }
    }

    #[test]
    fn a_stale_row_index_resolves_to_nothing_rather_than_panicking() {
        // The worker can shrink the snapshot between two frames: rows still
        // reference indices that no longer exist.
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerContainer(_)));
        s.selected += 1;
        s.docker_containers.clear();
        assert!(s.current_target().is_none());
    }

    #[test]
    fn a_pod_target_carries_its_cluster() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::ClusterPod { .. }));
        match s.current_target() {
            Some(KlusterTarget::Pod { cluster, pod, .. }) => {
                assert_eq!(cluster.name, "prod");
                assert_eq!(pod.name, "api-1");
            }
            _ => panic!("expected a pod"),
        }
    }

    // ---- per-row-type guards --------------------------------------------

    #[test]
    fn lifecycle_keys_do_nothing_on_a_pod() {
        // k8s has no start/stop equivalent — the keys must be inert, not
        // fire a Docker action against a pod.
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::ClusterPod { .. }));
        assert!(matches!(press(&mut s, 's'), KlusterAction::None));
        assert!(matches!(press(&mut s, 'R'), KlusterAction::None));
    }

    #[test]
    fn s_stops_a_running_container_and_starts_a_stopped_one() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerContainer(0)));
        assert!(matches!(
            press(&mut s, 's'),
            KlusterAction::Lifecycle(LifecycleAction::Stop)
        ));
        s.selected += 1; // "cache", not running
        assert!(matches!(
            press(&mut s, 's'),
            KlusterAction::Lifecycle(LifecycleAction::Start)
        ));
    }

    #[test]
    fn restart_fires_regardless_of_running_state() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerContainer(1)));
        assert!(matches!(
            press(&mut s, 'R'),
            KlusterAction::Lifecycle(LifecycleAction::Restart)
        ));
    }

    #[test]
    fn an_incus_instance_supports_lifecycle_too() {
        let mut s = KlusterTabState::from_db(KlusterDb::default());
        s.incus_local_available = true;
        s.incus_local_instances = vec![instance("lxc-1", true)];
        s.collapsed.clear();
        s.rebuild_rows();
        select(&mut s, |r| matches!(r, KlusterRow::IncusLocalInstance(_)));
        assert!(matches!(
            press(&mut s, 's'),
            KlusterAction::Lifecycle(LifecycleAction::Stop)
        ));
    }

    #[test]
    fn delete_only_fires_on_a_terminated_pod() {
        let mut s = state();
        // `api-1` is Running — `d` must not offer to delete it.
        select(&mut s, |r| {
            matches!(r, KlusterRow::ClusterPod { pod_idx: 0, .. })
        });
        assert!(matches!(press(&mut s, 'd'), KlusterAction::None));
        // `job-9` is Succeeded — that one is cleanup.
        select(&mut s, |r| {
            matches!(r, KlusterRow::ClusterPod { pod_idx: 1, .. })
        });
        assert!(matches!(press(&mut s, 'd'), KlusterAction::DeletePod));
    }

    #[test]
    fn a_failed_pod_also_counts_as_terminated() {
        let mut s = state();
        s.cluster_pods = vec![Some(vec![pod("job-x", "Failed")])];
        s.rebuild_rows();
        select(&mut s, |r| matches!(r, KlusterRow::ClusterPod { .. }));
        assert!(matches!(press(&mut s, 'd'), KlusterAction::DeletePod));
    }

    #[test]
    fn d_on_a_cluster_header_unlinks_the_cluster_not_a_pod() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::ClusterHeader { .. }));
        assert!(matches!(press(&mut s, 'd'), KlusterAction::DeleteCluster));
        assert!(matches!(press(&mut s, 'e'), KlusterAction::EditCluster));
    }

    #[test]
    fn d_on_a_docker_remote_header_unlinks_the_remote() {
        let db = KlusterDb {
            docker_remotes: vec![DockerRemote {
                host_alias: "web".into(),
            }],
            ..Default::default()
        };
        let mut s = KlusterTabState::from_db(db);
        s.collapsed.clear();
        s.rebuild_rows();
        select(&mut s, |r| {
            matches!(r, KlusterRow::DockerRemoteHeader { .. })
        });
        assert!(matches!(
            press(&mut s, 'd'),
            KlusterAction::DeleteDockerRemote
        ));
    }

    #[test]
    fn n_is_context_aware() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerHeader { .. }));
        assert!(matches!(press(&mut s, 'n'), KlusterAction::AddDockerRemote));
        select(&mut s, |r| matches!(r, KlusterRow::ClusterHeader { .. }));
        assert!(matches!(press(&mut s, 'n'), KlusterAction::AddCluster));
    }

    #[test]
    fn item_keys_are_inert_on_a_header() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerHeader { .. }));
        assert!(matches!(press(&mut s, 'i'), KlusterAction::None));
        assert!(matches!(press(&mut s, 'l'), KlusterAction::None));
        assert!(matches!(press(&mut s, 's'), KlusterAction::None));
    }

    #[test]
    fn enter_shells_into_an_item_and_folds_a_header() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerContainer(_)));
        assert!(matches!(
            handle_kluster_event(KeyCode::Enter, &mut s),
            KlusterAction::OpenShell
        ));

        select(&mut s, |r| matches!(r, KlusterRow::DockerHeader { .. }));
        let before = s.flat_rows.len();
        assert!(matches!(
            handle_kluster_event(KeyCode::Enter, &mut s),
            KlusterAction::None
        ));
        assert!(
            s.flat_rows.len() < before,
            "the section should have collapsed"
        );
    }

    // ---- navigation ------------------------------------------------------

    #[test]
    fn navigation_stops_at_both_ends() {
        let mut s = state();
        s.selected = 0;
        handle_kluster_event(KeyCode::Up, &mut s);
        assert_eq!(s.selected, 0, "must not underflow");

        s.selected = s.flat_rows.len() - 1;
        handle_kluster_event(KeyCode::Down, &mut s);
        assert_eq!(
            s.selected,
            s.flat_rows.len() - 1,
            "must not run past the last row"
        );
    }

    #[test]
    fn jk_navigate_like_the_arrows() {
        let mut s = state();
        s.selected = 0;
        press(&mut s, 'j');
        assert_eq!(s.selected, 1);
        press(&mut s, 'k');
        assert_eq!(s.selected, 0);
    }

    // ---- collapse --------------------------------------------------------

    #[test]
    fn collapsing_hides_children_and_keeps_the_header() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerHeader { .. }));
        s.toggle_collapsed_at_selected();
        assert!(!s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::DockerContainer(_))));
        assert!(s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::DockerHeader { .. })));
        s.toggle_collapsed_at_selected();
        assert_eq!(
            s.flat_rows
                .iter()
                .filter(|r| matches!(r, KlusterRow::DockerContainer(_)))
                .count(),
            2
        );
    }

    #[test]
    fn toggling_on_a_non_header_row_is_a_no_op() {
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerContainer(_)));
        let before = s.flat_rows.len();
        s.toggle_collapsed_at_selected();
        assert_eq!(s.flat_rows.len(), before);
    }

    #[test]
    fn deleting_a_cluster_renumbers_the_collapsed_set() {
        // `collapsed` keys embed the cluster index, so removing cluster 1 has
        // to shift 2→1, 3→2 … or the wrong sections stay folded.
        let mut s = state();
        s.collapsed = ["cluster_0", "cluster_2", "cluster_3", "docker"]
            .iter()
            .map(|k| k.to_string())
            .collect();
        s.shift_collapsed_after_delete("cluster_", 1);
        let mut got: Vec<_> = s.collapsed.iter().cloned().collect();
        got.sort();
        assert_eq!(got, vec!["cluster_0", "cluster_1", "cluster_2", "docker"]);
    }

    #[test]
    fn deleting_a_cluster_drops_its_own_collapsed_key() {
        let mut s = state();
        s.collapsed = ["cluster_1"].iter().map(|k| k.to_string()).collect();
        s.shift_collapsed_after_delete("cluster_", 1);
        assert!(s.collapsed.is_empty());
    }

    // ---- filter ----------------------------------------------------------

    #[test]
    fn slash_enters_filter_mode_and_esc_leaves_it() {
        let mut s = state();
        press(&mut s, '/');
        assert!(s.input_mode);
        press(&mut s, 'w');
        assert_eq!(s.filter, "w");
        handle_kluster_event(KeyCode::Esc, &mut s);
        assert!(!s.input_mode);
        assert!(s.filter.is_empty());
    }

    #[test]
    fn filtering_narrows_to_matching_items() {
        let mut s = state();
        press(&mut s, '/');
        for c in "web".chars() {
            press(&mut s, c);
        }
        let names: Vec<_> = s
            .flat_rows
            .iter()
            .filter_map(|r| match r {
                KlusterRow::DockerContainer(i) => Some(s.docker_containers[*i].name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["web"], "only the matching container survives");
    }

    #[test]
    fn a_filter_force_expands_collapsed_sections() {
        // A match hidden inside a folded section would otherwise be invisible.
        let mut s = state();
        select(&mut s, |r| matches!(r, KlusterRow::DockerHeader { .. }));
        s.toggle_collapsed_at_selected();
        assert!(!s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::DockerContainer(_))));

        press(&mut s, '/');
        for c in "web".chars() {
            press(&mut s, c);
        }
        assert!(s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::DockerContainer(_))));
    }

    #[test]
    fn letters_edit_the_query_instead_of_firing_actions_while_filtering() {
        // `d`, `s` and `n` are destructive elsewhere — inside the filter they
        // must be plain characters.
        let mut s = state();
        press(&mut s, '/');
        for c in "dsn".chars() {
            assert!(matches!(press(&mut s, c), KlusterAction::None));
        }
        assert_eq!(s.filter, "dsn");
    }

    #[test]
    fn arrows_still_navigate_while_filtering() {
        let mut s = state();
        press(&mut s, '/');
        s.selected = 0;
        handle_kluster_event(KeyCode::Down, &mut s);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn enter_confirms_the_filter_and_keeps_it_applied() {
        let mut s = state();
        press(&mut s, '/');
        for c in "web".chars() {
            press(&mut s, c);
        }
        handle_kluster_event(KeyCode::Enter, &mut s);
        assert!(!s.input_mode, "typing is over");
        assert_eq!(s.filter, "web", "but the filter stays applied");
    }

    #[test]
    fn backspace_widens_the_filter_again() {
        let mut s = state();
        press(&mut s, '/');
        for c in "web".chars() {
            press(&mut s, c);
        }
        handle_kluster_event(KeyCode::Backspace, &mut s);
        assert_eq!(s.filter, "we");
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        assert!(item_matches("anything", ""));
    }

    #[test]
    fn refresh_works_from_any_row() {
        let mut s = state();
        for i in 0..s.flat_rows.len() {
            s.selected = i;
            assert!(matches!(press(&mut s, 'r'), KlusterAction::Refresh));
        }
    }
}

#[cfg(test)]
mod podman_tests {
    use super::*;

    fn with_podman(running: bool) -> KlusterTabState {
        let mut s = KlusterTabState::from_db(KlusterDb::default());
        s.podman_available = true;
        s.podman_containers = vec![ContainerInfo {
            id: "pod1".into(),
            name: "api".into(),
            image: "alpine".into(),
            status: if running {
                "Up".into()
            } else {
                "Exited (0)".into()
            },
            running,
        }];
        s.collapsed.clear();
        s.rebuild_rows();
        s
    }

    #[test]
    fn the_section_is_hidden_when_podman_is_absent() {
        // Most machines have no podman; they must see nothing at all, not an
        // "(unavailable)" line — the same rule the Apple runtime follows.
        let s = KlusterTabState::from_db(KlusterDb::default());
        assert!(!s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanHeader { .. })));
    }

    #[test]
    fn the_section_appears_with_its_containers_when_podman_is_there() {
        let s = with_podman(true);
        assert!(s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanHeader { count: 1, .. })));
        assert!(s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanContainer(0))));
    }

    #[test]
    fn a_podman_container_resolves_to_its_own_target() {
        // Not `KlusterTarget::Docker`: the action layer picks the binary from
        // the variant, so a mix-up would run `docker` against a podman id.
        let mut s = with_podman(true);
        s.selected = s
            .flat_rows
            .iter()
            .position(|r| matches!(r, KlusterRow::PodmanContainer(_)))
            .unwrap();
        match s.current_target() {
            Some(KlusterTarget::Podman(c)) => assert_eq!(c.name, "api"),
            other => panic!("expected a podman target, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn lifecycle_and_item_keys_work_on_a_podman_container() {
        let mut s = with_podman(true);
        s.selected = s
            .flat_rows
            .iter()
            .position(|r| matches!(r, KlusterRow::PodmanContainer(_)))
            .unwrap();
        assert!(matches!(
            handle_kluster_event(KeyCode::Char('s'), &mut s),
            KlusterAction::Lifecycle(LifecycleAction::Stop)
        ));
        assert!(matches!(
            handle_kluster_event(KeyCode::Enter, &mut s),
            KlusterAction::OpenShell
        ));
        assert!(matches!(
            handle_kluster_event(KeyCode::Char('i'), &mut s),
            KlusterAction::OpenDetail
        ));
        assert!(matches!(
            handle_kluster_event(KeyCode::Char('l'), &mut s),
            KlusterAction::OpenLogsFollow
        ));
    }

    #[test]
    fn a_stopped_podman_container_starts_instead_of_stopping() {
        let mut s = with_podman(false);
        s.selected = s
            .flat_rows
            .iter()
            .position(|r| matches!(r, KlusterRow::PodmanContainer(_)))
            .unwrap();
        assert!(matches!(
            handle_kluster_event(KeyCode::Char('s'), &mut s),
            KlusterAction::Lifecycle(LifecycleAction::Start)
        ));
    }

    #[test]
    fn the_podman_header_folds_like_any_other() {
        let mut s = with_podman(true);
        s.selected = s
            .flat_rows
            .iter()
            .position(|r| matches!(r, KlusterRow::PodmanHeader { .. }))
            .unwrap();
        s.toggle_collapsed_at_selected();
        assert!(!s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanContainer(_))));
        assert!(s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanHeader { .. })));
    }

    #[test]
    fn podman_containers_are_filterable() {
        let mut s = with_podman(true);
        s.filter = "api".into();
        s.rebuild_rows();
        assert!(s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanContainer(_))));
        s.filter = "zzz".into();
        s.rebuild_rows();
        assert!(!s
            .flat_rows
            .iter()
            .any(|r| matches!(r, KlusterRow::PodmanContainer(_))));
    }

    #[test]
    fn docker_and_podman_sections_coexist_without_confusion() {
        // Both present, same container name: each row must resolve to its own
        // engine's snapshot.
        let mut s = with_podman(true);
        s.docker_available = true;
        s.docker_containers = vec![ContainerInfo {
            id: "dock1".into(),
            name: "api".into(),
            image: "alpine".into(),
            status: "Up".into(),
            running: true,
        }];
        s.rebuild_rows();

        let d = s
            .flat_rows
            .iter()
            .position(|r| matches!(r, KlusterRow::DockerContainer(_)))
            .unwrap();
        s.selected = d;
        assert!(matches!(s.current_target(), Some(KlusterTarget::Docker(c)) if c.id == "dock1"));

        let p = s
            .flat_rows
            .iter()
            .position(|r| matches!(r, KlusterRow::PodmanContainer(_)))
            .unwrap();
        s.selected = p;
        assert!(matches!(s.current_target(), Some(KlusterTarget::Podman(c)) if c.id == "pod1"));
    }
}
