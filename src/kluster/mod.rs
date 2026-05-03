//! Docker + k8s/k3s container management.
//!
//! - [`docker`] wraps the local `docker` CLI to list, exec into, and tail
//!   logs from containers running on the host's daemon.
//! - [`kube`] wraps `kubectl` for the same operations across saved clusters.
//! - [`shell`] holds the `bash`-or-`sh` fallback string used by both.
//! - [`db`] persists the user's saved cluster definitions in `kluster.json`.

pub mod models;
pub mod shell;
pub mod db;
pub mod docker;
pub mod kube;
pub mod incus;

pub use models::{Cluster, ClusterKind, ContainerInfo, IncusInstance, KlusterDb, PodInfo};
