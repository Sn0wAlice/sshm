//! Small view structs that cross IPC where a core type isn't directly
//! serializable (e.g. it holds `PathBuf`). The big model types (`Host`,
//! `Tunnel`, `AppConfig`, kluster types, `TunnelRecord`) are reused straight
//! from `sshm_core` — no duplication there.

use serde::{Deserialize, Serialize};
use specta::Type;
use sshm_core::ssh::keys::KeyEntry;
use sshm_core::ssh::known_hosts::HostKey;

/// One SSH key under `~/.ssh`, with paths flattened to strings for the webview.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IdentityDto {
    pub private: String,
    pub public: String,
    pub key_type: String,
    pub bits: Option<u32>,
    pub comment: String,
    pub fingerprint: String,
    pub in_agent: bool,
    pub is_hardware: bool,
}

impl From<KeyEntry> for IdentityDto {
    fn from(k: KeyEntry) -> Self {
        IdentityDto {
            private: k.private.to_string_lossy().into_owned(),
            public: k.public.to_string_lossy().into_owned(),
            key_type: k.key_type,
            bits: k.bits,
            comment: k.comment,
            fingerprint: k.fingerprint,
            in_agent: k.in_agent,
            is_hardware: k.is_hardware,
        }
    }
}

/// Reachability of a host: `latency_ms = Some(ms)` when a direct TCP connect to
/// `host:port` succeeded, `None` when it couldn't be reached directly (down, or
/// only reachable through a ProxyJump).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HostPing {
    pub name: String,
    pub latency_ms: Option<u32>,
}

/// One host key (algorithm + SHA256 fingerprint), flattened for the webview.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HostKeyDto {
    pub key_type: String,
    pub fingerprint: String,
}

impl From<HostKey> for HostKeyDto {
    fn from(k: HostKey) -> Self {
        HostKeyDto { key_type: k.key_type, fingerprint: k.fingerprint }
    }
}

/// Verdict when comparing the key pinned in `known_hosts` against the one the
/// server presents right now — mirrors the TUI's `F` inspector.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// Reachable, nothing pinned yet — a trust-on-first-use decision.
    Unpinned,
    /// A key is pinned but the server couldn't be reached to compare.
    Unreachable,
    /// The pinned key matches what the server presents. All good.
    Match,
    /// A key is pinned but it does NOT match the server — a changed key.
    Changed,
    /// No key on either side.
    Unknown,
}

/// The pinned vs. live host key(s) for a saved host, plus the verdict. Feeds the
/// GUI host-key inspector so the user can pin, forget, or replace a stale key.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HostKeyInfo {
    /// The resolved hostname the check ran against.
    pub host: String,
    pub port: u16,
    pub pinned: Vec<HostKeyDto>,
    pub live: Vec<HostKeyDto>,
    pub status: HostKeyStatus,
}

/// A saved cluster/remote plus which runtime it targets, for the Kluster tab.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct KlusterOverview {
    pub clusters: Vec<sshm_core::kluster::Cluster>,
    pub incus_remotes: Vec<String>,
    pub docker_remotes: Vec<sshm_core::kluster::DockerRemote>,
    pub docker_local_available: bool,
    /// Apple's native `container` runtime (macOS 26+, Apple silicon).
    pub apple_local_available: bool,
    pub incus_local_available: bool,
    pub kube_available: bool,
}
