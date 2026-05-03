//! Kluster tab — Docker containers + saved k8s/k3s cluster pods.
//!
//! State holds the in-memory snapshot returned by the background discovery
//! worker; rendering and event handling are stateless and pure (apart from
//! the `selected` cursor).

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use std::collections::{HashMap, HashSet};

use crate::kluster::{Cluster, ContainerInfo, IncusInstance, KlusterDb, PodInfo};
use crate::tui::theme::Theme;

/// Stable string key used in [`KlusterTabState::collapsed`] to identify a
/// section header. Stable across refreshes (doesn't depend on row index).
fn header_key(row: &KlusterRow) -> Option<String> {
    match row {
        KlusterRow::DockerHeader { .. } => Some("docker".into()),
        KlusterRow::DockerRemoteHeader { remote_idx, .. } => Some(format!("docker_remote_{}", remote_idx)),
        KlusterRow::IncusLocalHeader { .. } => Some("incus_local".into()),
        KlusterRow::IncusRemoteHeader { remote_idx, .. } => Some(format!("incus_remote_{}", remote_idx)),
        KlusterRow::ClusterHeader { cluster_idx, .. } => Some(format!("cluster_{}", cluster_idx)),
        _ => None,
    }
}

/// One renderable row in the left pane. Indices reference the live snapshot
/// stored alongside on `KlusterTabState`.
#[derive(Debug, Clone)]
pub enum KlusterRow {
    DockerHeader { count: usize, available: bool },
    DockerContainer(usize),
    /// One header per saved Docker remote (over SSH). `remote_idx` indexes
    /// `db.docker_remotes`, `reachable` is the last status reported by the
    /// worker.
    DockerRemoteHeader { remote_idx: usize, count: usize, reachable: bool },
    DockerRemoteContainer { remote_idx: usize, container_idx: usize },
    ClusterHeader { cluster_idx: usize, count: usize },
    ClusterPod {
        cluster_idx: usize,
        pod_idx: usize,
        /// `Some(name)` when the pod has multiple containers and the user
        /// has expanded a specific one. `None` = use the first container.
        container: Option<String>,
    },
    IncusLocalHeader { count: usize, available: bool },
    IncusLocalInstance(usize),
    IncusRemoteHeader { remote_idx: usize, count: usize },
    IncusRemoteInstance { remote_idx: usize, instance_idx: usize },
}

pub struct KlusterTabState {
    pub db: KlusterDb,
    pub docker_available: bool,
    pub docker_containers: Vec<ContainerInfo>,
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
}

impl KlusterTabState {
    pub fn new() -> Self {
        let (db, imported) = crate::kluster::db::load_or_bootstrap();
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
            bootstrap_imported: imported,
            collapsed,
        };
        state.rebuild_rows();
        state
    }

    /// Recompute `flat_rows` from the current snapshot. Called every time
    /// the worker pushes new data, and after a collapse toggle.
    pub fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        let docker_h = KlusterRow::DockerHeader {
            count: self.docker_containers.len(),
            available: self.docker_available,
        };
        let docker_collapsed = self.collapsed.contains("docker");
        rows.push(docker_h);
        if self.docker_available && !docker_collapsed {
            for i in 0..self.docker_containers.len() {
                rows.push(KlusterRow::DockerContainer(i));
            }
        }
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
            let is_collapsed = self.collapsed.contains(&key);
            rows.push(KlusterRow::DockerRemoteHeader { remote_idx: ri, count, reachable });
            if !is_collapsed && reachable {
                if let Some(list) = containers {
                    for ii in 0..list.len() {
                        rows.push(KlusterRow::DockerRemoteContainer { remote_idx: ri, container_idx: ii });
                    }
                }
            }
        }
        // Local Incus section.
        let incus_local_h = KlusterRow::IncusLocalHeader {
            count: self.incus_local_instances.len(),
            available: self.incus_local_available,
        };
        let incus_local_collapsed = self.collapsed.contains("incus_local");
        rows.push(incus_local_h);
        if self.incus_local_available && !incus_local_collapsed {
            for i in 0..self.incus_local_instances.len() {
                rows.push(KlusterRow::IncusLocalInstance(i));
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
            let is_collapsed = self.collapsed.contains(&key);
            rows.push(KlusterRow::IncusRemoteHeader { remote_idx: ri, count });
            if !is_collapsed {
                if let Some(list) = self.incus_remote_instances.get(remote) {
                    for ii in 0..list.len() {
                        rows.push(KlusterRow::IncusRemoteInstance { remote_idx: ri, instance_idx: ii });
                    }
                }
            }
        }
        for (ci, _cluster) in self.db.clusters.iter().enumerate() {
            let pods = self.cluster_pods.get(ci).and_then(|x| x.as_ref());
            let count = pods.map(|p| p.len()).unwrap_or(0);
            let key = format!("cluster_{}", ci);
            let is_collapsed = self.collapsed.contains(&key);
            rows.push(KlusterRow::ClusterHeader { cluster_idx: ci, count });
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
        self.flat_rows = rows;
        if self.selected >= self.flat_rows.len() {
            self.selected = self.flat_rows.len().saturating_sub(1);
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
                    if n == deleted_idx { continue; }
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
            KlusterRow::DockerRemoteContainer { remote_idx, container_idx } => {
                let remote = self.db.docker_remotes.get(*remote_idx)?;
                let host_uri = self.docker_remote_uris.get(&remote.host_alias)?;
                let containers = self.docker_remote_containers.get(&remote.host_alias)?;
                let container = containers.get(*container_idx)?;
                Some(KlusterTarget::DockerRemote { container, host_uri })
            }
            KlusterRow::ClusterPod { cluster_idx, pod_idx, container } => {
                let cluster = self.db.clusters.get(*cluster_idx)?;
                let pod = self.cluster_pods.get(*cluster_idx)?.as_ref()?.get(*pod_idx)?;
                Some(KlusterTarget::Pod {
                    cluster,
                    pod,
                    container: container.as_deref(),
                })
            }
            KlusterRow::IncusLocalInstance(i) => {
                self.incus_local_instances
                    .get(*i)
                    .map(|inst| KlusterTarget::Incus { instance: inst, remote: None })
            }
            KlusterRow::IncusRemoteInstance { remote_idx, instance_idx } => {
                let remote = self.db.incus_remotes.get(*remote_idx)?;
                let instance = self.incus_remote_instances.get(remote)?.get(*instance_idx)?;
                Some(KlusterTarget::Incus { instance, remote: Some(remote.as_str()) })
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
}

pub fn handle_kluster_event(key: KeyCode, state: &mut KlusterTabState) -> KlusterAction {
    let row = state.flat_rows.get(state.selected);
    let on_item = matches!(
        row,
        Some(KlusterRow::DockerContainer(_))
            | Some(KlusterRow::DockerRemoteContainer { .. })
            | Some(KlusterRow::ClusterPod { .. })
            | Some(KlusterRow::IncusLocalInstance(_))
            | Some(KlusterRow::IncusRemoteInstance { .. })
    );
    let on_header = matches!(
        row,
        Some(KlusterRow::DockerHeader { .. })
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
        KeyCode::Up | KeyCode::Char('k') => { state.move_up(); KlusterAction::None }
        KeyCode::Down | KeyCode::Char('j') => { state.move_down(); KlusterAction::None }
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
        KeyCode::Char('l') if on_item => KlusterAction::OpenLogsFollow,
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
    let list = List::new(items)
        .block(
            Block::default()
                .title("Kluster — Docker + clusters")
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

fn render_row<'a>(
    row: &KlusterRow,
    state: &KlusterTabState,
    theme: &Theme,
) -> ListItem<'a> {
    match row {
        KlusterRow::DockerHeader { count, available } => {
            let glyph = if state.collapsed.contains("docker") { "▸" } else { "▾" };
            let label = if *available {
                format!("{} Docker (local) ({})", glyph, count)
            } else {
                format!("{} Docker (local) (unavailable)", glyph)
            };
            let style = if *available {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        }
        KlusterRow::DockerContainer(i) => {
            let c = &state.docker_containers[*i];
            render_docker_container(c, theme)
        }
        KlusterRow::DockerRemoteHeader { remote_idx, count, reachable } => {
            let remote = &state.db.docker_remotes[*remote_idx];
            let key = format!("docker_remote_{}", remote_idx);
            let glyph = if state.collapsed.contains(&key) { "▸" } else { "▾" };
            let suffix = if *reachable {
                format!("({})", count)
            } else {
                "(unreachable)".to_string()
            };
            let style = if *reachable {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.error).add_modifier(Modifier::BOLD)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{} Docker (remote {}) {}", glyph, remote.host_alias, suffix),
                style,
            )))
        }
        KlusterRow::DockerRemoteContainer { remote_idx, container_idx } => {
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
            let glyph = if state.collapsed.contains(&key) { "▸" } else { "▾" };
            let label = format!("{} Cluster: {} ({})  [{}]", glyph, cluster.name, count, cluster.kind.label());
            ListItem::new(Line::from(Span::styled(
                label,
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            )))
        }
        KlusterRow::ClusterPod { cluster_idx, pod_idx, .. } => {
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
                Span::styled(format!("{}/", pod.namespace), Style::default().fg(theme.muted)),
                Span::styled(pod.name.clone(), Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(format!("● {} ", pod.phase), phase_style),
                Span::styled(containers_repr, Style::default().fg(theme.muted)),
            ]))
        }
        KlusterRow::IncusLocalHeader { count, available } => {
            let glyph = if state.collapsed.contains("incus_local") { "▸" } else { "▾" };
            let label = if *available {
                format!("{} Incus (local) ({})", glyph, count)
            } else {
                format!("{} Incus (local) (unavailable)", glyph)
            };
            let style = if *available {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)
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
            let glyph = if state.collapsed.contains(&key) { "▸" } else { "▾" };
            ListItem::new(Line::from(Span::styled(
                format!("{} Incus (remote {}) ({})", glyph, remote, count),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            )))
        }
        KlusterRow::IncusRemoteInstance { remote_idx, instance_idx } => {
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
        Span::styled(c.name.clone(), Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)),
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
    let kind_short = if inst.kind.starts_with("virtual") { "vm" } else { "ct" };
    ListItem::new(Line::from(vec![
        Span::raw("    "),
        Span::styled(format!("{} ", glyph), glyph_style),
        Span::styled(inst.name.clone(), Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(format!("[{}]", kind_short), Style::default().fg(theme.muted)),
        Span::raw("  "),
        Span::styled(inst.image.clone(), Style::default().fg(theme.muted)),
        Span::raw("  "),
        Span::styled(inst.status.clone(), Style::default().fg(theme.muted)),
    ]))
}

