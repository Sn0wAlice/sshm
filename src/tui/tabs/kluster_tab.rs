//! Kluster tab — Docker containers + saved k8s/k3s cluster pods.
//!
//! State holds the in-memory snapshot returned by the background discovery
//! worker; rendering and event handling are stateless and pure (apart from
//! the `selected` cursor).

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use std::collections::HashMap;

use crate::kluster::{Cluster, ContainerInfo, IncusInstance, KlusterDb, PodInfo};
use crate::tui::theme::Theme;

/// One renderable row in the left pane. Indices reference the live snapshot
/// stored alongside on `KlusterTabState`.
#[derive(Debug, Clone)]
pub enum KlusterRow {
    DockerHeader { count: usize, available: bool },
    DockerContainer(usize),
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
    pub selected: usize,
    pub flat_rows: Vec<KlusterRow>,
    /// True after the very first refresh round-trip; gates "no daemon" toasts.
    pub bootstrapped: bool,
    pub bootstrap_imported: usize,
}

impl KlusterTabState {
    pub fn new() -> Self {
        let (db, imported) = crate::kluster::db::load_or_bootstrap();
        let cluster_pods = vec![None; db.clusters.len()];
        let mut state = KlusterTabState {
            db,
            docker_available: false,
            docker_containers: Vec::new(),
            cluster_pods,
            incus_local_available: false,
            incus_local_instances: Vec::new(),
            incus_remote_instances: HashMap::new(),
            selected: 0,
            flat_rows: Vec::new(),
            bootstrapped: false,
            bootstrap_imported: imported,
        };
        state.rebuild_rows();
        state
    }

    /// Recompute `flat_rows` from the current snapshot. Called every time
    /// the worker pushes new data.
    pub fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        rows.push(KlusterRow::DockerHeader {
            count: self.docker_containers.len(),
            available: self.docker_available,
        });
        if self.docker_available {
            for i in 0..self.docker_containers.len() {
                rows.push(KlusterRow::DockerContainer(i));
            }
        }
        // Local Incus section (only if `incus` available locally).
        rows.push(KlusterRow::IncusLocalHeader {
            count: self.incus_local_instances.len(),
            available: self.incus_local_available,
        });
        if self.incus_local_available {
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
            rows.push(KlusterRow::IncusRemoteHeader { remote_idx: ri, count });
            if let Some(list) = self.incus_remote_instances.get(remote) {
                for ii in 0..list.len() {
                    rows.push(KlusterRow::IncusRemoteInstance { remote_idx: ri, instance_idx: ii });
                }
            }
        }
        for (ci, cluster) in self.db.clusters.iter().enumerate() {
            let pods = self.cluster_pods.get(ci).and_then(|x| x.as_ref());
            let count = pods.map(|p| p.len()).unwrap_or(0);
            rows.push(KlusterRow::ClusterHeader { cluster_idx: ci, count });
            if let Some(pods) = pods {
                for (pi, pod) in pods.iter().enumerate() {
                    rows.push(KlusterRow::ClusterPod {
                        cluster_idx: ci,
                        pod_idx: pi,
                        container: None,
                    });
                    let _ = cluster; // future: per-container expansion
                    let _ = pod;
                }
            }
        }
        self.flat_rows = rows;
        if self.selected >= self.flat_rows.len() {
            self.selected = self.flat_rows.len().saturating_sub(1);
        }
    }

    /// Returns the actionable target on the current row, or None for headers.
    pub fn current_target(&self) -> Option<KlusterTarget<'_>> {
        let row = self.flat_rows.get(self.selected)?;
        match row {
            KlusterRow::DockerContainer(i) => {
                self.docker_containers.get(*i).map(KlusterTarget::Docker)
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
}

pub fn handle_kluster_event(key: KeyCode, state: &mut KlusterTabState) -> KlusterAction {
    match key {
        KeyCode::Up | KeyCode::Char('k') => { state.move_up(); KlusterAction::None }
        KeyCode::Down | KeyCode::Char('j') => { state.move_down(); KlusterAction::None }
        KeyCode::Enter => KlusterAction::OpenShell,
        KeyCode::Char('l') => KlusterAction::OpenLogsFollow,
        KeyCode::Char('r') => KlusterAction::Refresh,
        KeyCode::Char('n') => KlusterAction::AddCluster,
        KeyCode::Char('e') => KlusterAction::EditCluster,
        KeyCode::Char('d') => KlusterAction::DeleteCluster,
        _ => KlusterAction::None,
    }
}

pub fn draw_kluster_tab(f: &mut Frame, area: Rect, state: &KlusterTabState, theme: &Theme) {
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // ----- Left pane -----
    let mut items: Vec<ListItem> = Vec::new();
    for row in &state.flat_rows {
        items.push(render_row(row, state, theme));
    }

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
    f.render_stateful_widget(list, hchunks[0], &mut ls);

    // ----- Right pane: details -----
    let detail_text = render_details(state);
    let detail = Paragraph::new(detail_text).block(
        Block::default()
            .title("Details")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().bg(theme.bg).fg(theme.fg)),
    );
    f.render_widget(detail, hchunks[1]);
}

fn render_row<'a>(
    row: &KlusterRow,
    state: &KlusterTabState,
    theme: &Theme,
) -> ListItem<'a> {
    match row {
        KlusterRow::DockerHeader { count, available } => {
            let label = if *available {
                format!("▾ Docker (local) ({})", count)
            } else {
                "▾ Docker (local) (unavailable)".to_string()
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
        KlusterRow::ClusterHeader { cluster_idx, count } => {
            let cluster = &state.db.clusters[*cluster_idx];
            let label = format!("▾ Cluster: {} ({})  [{}]", cluster.name, count, cluster.kind.label());
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
            let label = if *available {
                format!("▾ Incus (local) ({})", count)
            } else {
                "▾ Incus (local) (unavailable)".to_string()
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
            ListItem::new(Line::from(Span::styled(
                format!("▾ Incus (remote {}) ({})", remote, count),
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

fn render_details(state: &KlusterTabState) -> String {
    let Some(row) = state.flat_rows.get(state.selected) else {
        return "No selection.\n\nPress 'r' to refresh.".to_string();
    };
    match row {
        KlusterRow::DockerHeader { count, available } => {
            if *available {
                format!("Docker daemon: running\nContainers: {}\n\nNavigate to a container — Enter shell, l logs (-f).", count)
            } else {
                "Docker daemon: not running\n(or `docker` not on PATH)\n\nStart Docker Desktop / `systemctl start docker`."
                    .to_string()
            }
        }
        KlusterRow::DockerContainer(i) => {
            let c = &state.docker_containers[*i];
            format!(
                "ID:      {}\nName:    {}\nImage:   {}\nStatus:  {}\nRunning: {}\n\nEnter: shell  l: logs (-f)",
                c.id, c.name, c.image, c.status, if c.running { "yes" } else { "no" }
            )
        }
        KlusterRow::ClusterHeader { cluster_idx, count } => {
            let c = &state.db.clusters[*cluster_idx];
            format!(
                "Name:        {}\nKind:        {}\nKubeconfig:  {}\nContext:     {}\nPods:        {}\n\nn: add  e: edit  d: delete  r: refresh",
                c.name,
                c.kind.label(),
                c.kubeconfig.as_deref().unwrap_or("(default)"),
                c.context.as_deref().unwrap_or("(current-context)"),
                count,
            )
        }
        KlusterRow::ClusterPod { cluster_idx, pod_idx, .. } => {
            let cluster = &state.db.clusters[*cluster_idx];
            let pod = &state.cluster_pods[*cluster_idx].as_ref().unwrap()[*pod_idx];
            format!(
                "Cluster:    {}\nNamespace:  {}\nPod:        {}\nPhase:      {}\nContainers: {}\n\nEnter: shell (first container)  l: logs (-f)",
                cluster.name,
                pod.namespace,
                pod.name,
                pod.phase,
                if pod.containers.is_empty() { "—".to_string() } else { pod.containers.join(", ") },
            )
        }
        KlusterRow::IncusLocalHeader { count, available } => {
            if *available {
                format!("Incus daemon: running\nInstances: {}\n\nNavigate to an instance — Enter shell, l logs (journalctl -f).", count)
            } else {
                "Incus daemon: not available\n(or `incus` not on PATH)\n\nInstall Incus from your distro and start it.".to_string()
            }
        }
        KlusterRow::IncusLocalInstance(i) => {
            let inst = &state.incus_local_instances[*i];
            format_incus_details(inst, None)
        }
        KlusterRow::IncusRemoteHeader { remote_idx, count } => {
            let remote = &state.db.incus_remotes[*remote_idx];
            format!(
                "Remote:     {}\nInstances:  {}\n\n`incus list {}:` is used to populate this view.",
                remote, count, remote
            )
        }
        KlusterRow::IncusRemoteInstance { remote_idx, instance_idx } => {
            let remote = &state.db.incus_remotes[*remote_idx];
            let inst = &state.incus_remote_instances[remote][*instance_idx];
            format_incus_details(inst, Some(remote))
        }
    }
}

fn format_incus_details(inst: &IncusInstance, remote: Option<&str>) -> String {
    let qualified = match remote {
        Some(r) => format!("{}:{}", r, inst.name),
        None => inst.name.clone(),
    };
    format!(
        "Name:    {}\nKind:    {}\nStatus:  {}\nImage:   {}\nRunning: {}\n\nEnter: shell  l: logs (journalctl -f, requires systemd)",
        qualified,
        inst.kind,
        inst.status,
        if inst.image.is_empty() { "—" } else { inst.image.as_str() },
        if inst.running { "yes" } else { "no" },
    )
}
