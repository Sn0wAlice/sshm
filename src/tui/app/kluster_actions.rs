//! Action handlers for the Kluster tab — bridges the `KlusterAction` enum
//! emitted by the tab to actual `docker`/`kubectl` invocations and the
//! `inquire`-driven cluster CRUD flows.

use std::io::stdout;

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::Backend, Terminal};

use crate::t;
use crate::tui::ssh::toast::Toast;
use crate::tui::tabs::kluster_tab::{KlusterTabState, KlusterTarget};

use super::cluster_form::{run_cluster_delete_confirm, run_cluster_form};
use super::kluster_worker::{KlusterTargets, WorkerTargets};

fn enter_foreground<B: Backend>(terminal: &mut Terminal<B>) {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
    let _ = terminal.clear();
}

fn restore_tui<B: Backend>(terminal: &mut Terminal<B>) {
    let _ = enable_raw_mode();
    let _ = execute!(stdout(), EnterAlternateScreen);
    let _ = terminal.clear();
}

fn report_shell_exit(status: std::io::Result<std::process::ExitStatus>) -> Option<Toast> {
    // We only surface "could not even spawn the CLI" — typical reason is
    // that `docker`/`kubectl`/`incus` is not on PATH. Non-zero exit from a
    // running shell (e.g. last-command-failed-then-Ctrl-D, sigint, missing
    // /bin/sh in distroless) is too noisy to toast every time.
    match status {
        Ok(_) => None,
        Err(e) => Some(Toast::error(format!("{e}"))),
    }
}

pub fn handle_kluster_open_shell<B: Backend>(
    state: &mut KlusterTabState,
    terminal: &mut Terminal<B>,
    toast: &mut Option<Toast>,
) {
    let Some(target) = state.current_target() else {
        *toast = Some(Toast::error(t!("kluster.no_target")));
        return;
    };
    enter_foreground(terminal);
    let res = match target {
        KlusterTarget::Docker(c) => crate::kluster::docker::exec_shell(&c.id),
        KlusterTarget::Pod { cluster, pod, container } => {
            // First container in the pod by default if none specified.
            let ctn = container.or_else(|| pod.containers.first().map(|s| s.as_str()));
            crate::kluster::kube::exec_shell(cluster, &pod.namespace, &pod.name, ctn)
        }
        KlusterTarget::Incus { instance, remote } => {
            crate::kluster::incus::exec_shell(&instance.name, remote)
        }
    };
    restore_tui(terminal);
    if let Some(t) = report_shell_exit(res) {
        *toast = Some(t);
    }
}

pub fn handle_kluster_open_logs<B: Backend>(
    state: &mut KlusterTabState,
    tail: u32,
    follow: bool,
    terminal: &mut Terminal<B>,
    toast: &mut Option<Toast>,
) {
    let Some(target) = state.current_target() else {
        *toast = Some(Toast::error(t!("kluster.no_target")));
        return;
    };
    enter_foreground(terminal);
    let res = match target {
        KlusterTarget::Docker(c) => crate::kluster::docker::logs(&c.id, tail, follow),
        KlusterTarget::Pod { cluster, pod, container } => {
            let ctn = container.or_else(|| pod.containers.first().map(|s| s.as_str()));
            crate::kluster::kube::logs(cluster, &pod.namespace, &pod.name, ctn, tail, follow)
        }
        KlusterTarget::Incus { instance, remote } => {
            crate::kluster::incus::logs(&instance.name, remote, tail, follow)
        }
    };
    restore_tui(terminal);
    if let Err(e) = res {
        *toast = Some(Toast::error(t!("kluster.logs_failed", "error" => e)));
    }
}

pub fn sync_kluster_targets(targets: &KlusterTargets, state: &KlusterTabState) {
    if let Ok(mut g) = targets.lock() {
        *g = WorkerTargets {
            clusters: state.db.clusters.clone(),
            incus_remotes: state.db.incus_remotes.clone(),
        };
    }
}

// -------- CRUD flows (TUI form / modal) --------

/// Open the cluster form to create a new entry. Saves to `kluster.json`
/// and updates the in-memory state on confirm.
pub fn kluster_add_cluster_flow<B: Backend>(
    state: &mut KlusterTabState,
    terminal: &mut Terminal<B>,
) -> Result<()> {
    enter_foreground(terminal);
    let new_cluster = run_cluster_form(None);
    restore_tui(terminal);
    let new_cluster = match new_cluster {
        Some(c) => c,
        None => return Ok(()),
    };

    if state.db.clusters.iter().any(|c| c.name == new_cluster.name) {
        return Err(anyhow::anyhow!(
            "cluster '{}' already exists",
            new_cluster.name
        ));
    }
    state.db.clusters.push(new_cluster);
    state.cluster_pods.push(None);
    crate::kluster::db::save(&state.db).context("saving kluster.json")?;
    state.rebuild_rows();
    Ok(())
}

/// Open the cluster form pre-filled with the currently selected cluster.
pub fn kluster_edit_cluster_flow<B: Backend>(
    state: &mut KlusterTabState,
    terminal: &mut Terminal<B>,
) -> Result<()> {
    use crate::tui::tabs::kluster_tab::KlusterRow;
    let cluster_idx = match state.flat_rows.get(state.selected) {
        Some(KlusterRow::ClusterHeader { cluster_idx, .. }) => *cluster_idx,
        Some(KlusterRow::ClusterPod { cluster_idx, .. }) => *cluster_idx,
        _ => return Err(anyhow::anyhow!("select a cluster row first")),
    };
    let current = state.db.clusters[cluster_idx].clone();
    let original_name = current.name.clone();

    enter_foreground(terminal);
    let updated = run_cluster_form(Some(&current));
    restore_tui(terminal);
    let updated = match updated {
        Some(c) => c,
        None => return Ok(()),
    };

    // Reject rename collisions with another existing cluster.
    if updated.name != original_name
        && state.db.clusters.iter().any(|c| c.name == updated.name)
    {
        return Err(anyhow::anyhow!(
            "cluster '{}' already exists",
            updated.name
        ));
    }
    state.db.clusters[cluster_idx] = updated;
    crate::kluster::db::save(&state.db).context("saving kluster.json")?;
    state.rebuild_rows();
    Ok(())
}

/// `kubectl delete pod` on the currently selected pod, after a confirm
/// modal. Returns `Ok(Some(name))` on actual deletion, `Ok(None)` on cancel
/// or when the row isn't a pod.
pub fn kluster_delete_pod_flow<B: Backend>(
    state: &mut KlusterTabState,
    terminal: &mut Terminal<B>,
) -> Result<Option<String>> {
    use crate::kluster::Cluster;
    use crate::tui::tabs::kluster_tab::KlusterRow;
    let (cluster, namespace, pod_name): (Cluster, String, String) = match state
        .flat_rows
        .get(state.selected)
    {
        Some(KlusterRow::ClusterPod { cluster_idx, pod_idx, .. }) => {
            let cluster = state.db.clusters.get(*cluster_idx)
                .ok_or_else(|| anyhow::anyhow!("cluster index out of range"))?
                .clone();
            let pods = state.cluster_pods.get(*cluster_idx)
                .and_then(|x| x.as_ref())
                .ok_or_else(|| anyhow::anyhow!("pod list not loaded"))?;
            let pod = pods.get(*pod_idx)
                .ok_or_else(|| anyhow::anyhow!("pod index out of range"))?;
            (cluster, pod.namespace.clone(), pod.name.clone())
        }
        _ => return Ok(None),
    };

    enter_foreground(terminal);
    let confirmed = super::cluster_form::run_cluster_delete_confirm(
        &format!("pod {}/{}", namespace, pod_name),
    );
    if !confirmed {
        restore_tui(terminal);
        return Ok(None);
    }
    let out = crate::kluster::kube::delete_pod(&cluster, &namespace, &pod_name)
        .context("running kubectl delete pod")?;
    restore_tui(terminal);

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(anyhow::anyhow!(
            "kubectl delete pod {}/{} failed: {}",
            namespace,
            pod_name,
            if stderr.is_empty() { "non-zero exit".to_string() } else { stderr }
        ));
    }
    // Optimistically drop the entry from the cached list so the UI updates
    // immediately; the next refresh will re-confirm the state.
    let cluster_idx = state.db.clusters.iter().position(|c| c.name == cluster.name);
    if let Some(ci) = cluster_idx {
        if let Some(Some(list)) = state.cluster_pods.get_mut(ci) {
            list.retain(|p| !(p.namespace == namespace && p.name == pod_name));
        }
    }
    state.rebuild_rows();
    Ok(Some(format!("{}/{}", namespace, pod_name)))
}

/// Confirm delete via a centred modal, then drop the cluster + its cached pods.
pub fn kluster_delete_cluster_flow<B: Backend>(
    state: &mut KlusterTabState,
    terminal: &mut Terminal<B>,
) -> Result<()> {
    use crate::tui::tabs::kluster_tab::KlusterRow;
    let cluster_idx = match state.flat_rows.get(state.selected) {
        Some(KlusterRow::ClusterHeader { cluster_idx, .. }) => *cluster_idx,
        Some(KlusterRow::ClusterPod { cluster_idx, .. }) => *cluster_idx,
        _ => return Err(anyhow::anyhow!("select a cluster row first")),
    };
    let name = state.db.clusters[cluster_idx].name.clone();

    enter_foreground(terminal);
    let confirmed = run_cluster_delete_confirm(&name);
    restore_tui(terminal);

    if confirmed {
        state.db.clusters.remove(cluster_idx);
        if cluster_idx < state.cluster_pods.len() {
            state.cluster_pods.remove(cluster_idx);
        }
        crate::kluster::db::save(&state.db).context("saving kluster.json")?;
        state.rebuild_rows();
        if state.selected >= state.flat_rows.len() {
            state.selected = state.flat_rows.len().saturating_sub(1);
        }
    }
    Ok(())
}
