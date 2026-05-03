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

/// On-disk representation for `kluster.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct KlusterDb {
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    /// Incus remote aliases (`incus remote list`). Empty when only the
    /// implicit `local` remote is in use.
    #[serde(default)]
    pub incus_remotes: Vec<String>,
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
