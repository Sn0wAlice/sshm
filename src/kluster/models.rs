use serde::{Deserialize, Serialize};

/// Display flavour for a saved cluster — purely cosmetic, drives the badge in
/// the tab. Detection is naïve (substring on context name), users can toggle.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClusterKind {
    K8s,
    K3s,
}

impl Default for ClusterKind {
    fn default() -> Self { ClusterKind::K8s }
}

impl ClusterKind {
    pub fn label(&self) -> &'static str {
        match self {
            ClusterKind::K8s => "k8s",
            ClusterKind::K3s => "k3s",
        }
    }

    /// Naïve auto-detection from a kubeconfig context name. `k3s` is the
    /// usual signal; everything else is plain k8s.
    pub fn from_context_name(name: &str) -> Self {
        if name.to_ascii_lowercase().contains("k3s") {
            ClusterKind::K3s
        } else {
            ClusterKind::K8s
        }
    }
}

/// One saved cluster entry. `kubeconfig` and `context` are both optional;
/// when omitted, `kubectl` is invoked with no `--kubeconfig`/`--context`
/// flags and falls back to the standard env / `current-context` rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cluster {
    pub name: String,
    #[serde(default)]
    pub kind: ClusterKind,
    #[serde(default)]
    pub kubeconfig: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub namespace_default: Option<String>,
}

/// A reference to a remote Docker daemon reached over SSH. The actual
/// connection details are looked up from the saved Host map (`host.json`)
/// at runtime, so renaming/editing the SSH host's user/port flows through.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerRemote {
    /// Name of an entry in the SSH Host DB (`Host.name`).
    pub host_alias: String,
}

/// On-disk representation for `kluster.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct KlusterDb {
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    /// Incus remote aliases (`incus remote list`). Empty when only the
    /// implicit `local` remote is in use.
    #[serde(default)]
    pub incus_remotes: Vec<String>,
    /// Saved Docker daemons reachable over SSH (`DOCKER_HOST=ssh://…`).
    #[serde(default)]
    pub docker_remotes: Vec<DockerRemote>,
}

/// Snapshot of one Docker container at the moment of `docker ps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub running: bool,
}

/// Snapshot of one k8s pod at the moment of `kubectl get pods`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodInfo {
    pub namespace: String,
    pub name: String,
    pub containers: Vec<String>,
    /// e.g. `Running`, `Pending`, `CrashLoopBackOff`.
    pub phase: String,
}

/// A start / stop / restart operation on a Docker container or Incus instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Start,
    Stop,
    Restart,
}

impl LifecycleAction {
    /// The matching `docker` / `incus` subcommand.
    pub fn subcommand(self) -> &'static str {
        match self {
            LifecycleAction::Start => "start",
            LifecycleAction::Stop => "stop",
            LifecycleAction::Restart => "restart",
        }
    }

    /// Past-tense verb for success toasts ("Started nginx").
    pub fn past_tense(self) -> &'static str {
        match self {
            LifecycleAction::Start => "Started",
            LifecycleAction::Stop => "Stopped",
            LifecycleAction::Restart => "Restarted",
        }
    }
}

/// One titled group of label→value rows in the rich detail view (e.g.
/// "Networking" holding `IPv4 → 192.168.64.3`). Purely presentational.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailSection {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

impl DetailSection {
    pub fn new(title: impl Into<String>) -> Self {
        DetailSection { title: title.into(), rows: Vec::new() }
    }
    /// Push a `label → value` row, skipping empty values so the view stays
    /// terse (no rows of blanks for fields the runtime didn't report).
    pub fn push(&mut self, label: impl Into<String>, value: impl Into<String>) {
        let value = value.into();
        if !value.trim().is_empty() {
            self.rows.push((label.into(), value));
        }
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Parsed, runtime-agnostic detail for one container/instance, built from an
/// `inspect` call. Rendered by the Kluster detail popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerDetail {
    /// Header line (usually the container name).
    pub title: String,
    pub sections: Vec<DetailSection>,
    /// Last few log lines, when the runtime could produce them cheaply.
    pub log_tail: Vec<String>,
}

/// Snapshot of one Incus instance (container or VM).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncusInstance {
    pub name: String,
    /// `container` or `virtual-machine`.
    pub kind: String,
    /// `Running`, `Stopped`, …
    pub status: String,
    /// Image alias / fingerprint, when reported.
    pub image: String,
    pub running: bool,
}
