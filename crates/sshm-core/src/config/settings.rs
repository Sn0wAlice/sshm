use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_port")]
    pub default_port: u16,
    #[serde(default = "default_username")]
    pub default_username: String,
    #[serde(default)]
    pub default_identity_file: String,
    #[serde(default)]
    pub export_path: String,
    /// Auto-refresh reachability/latency for every host in the background.
    /// Enabled by default; user can turn it off from the Settings tab.
    #[serde(default = "default_auto_health_check")]
    pub auto_health_check: bool,
    /// Pause the background health worker while an interactive SSH session
    /// is in the foreground, and resume it on return. Avoids the network
    /// noise of probing every host while you're actually working on one.
    /// Enabled by default; toggle from the Settings tab.
    #[serde(default = "default_pause_health_on_session")]
    pub pause_health_on_session: bool,
    /// How often (seconds) the background worker re-probes every host.
    /// Doubles as the cache TTL — entries older than this are re-probed.
    #[serde(default = "default_health_ttl_secs")]
    pub health_ttl_secs: u64,
    /// TCP connect timeout (ms) used by each probe. SSH banner read is
    /// derived from this (~1/3, capped at 750ms).
    #[serde(default = "default_health_probe_timeout_ms")]
    pub health_probe_timeout_ms: u64,
    /// How often (seconds) the Kluster tab refreshes the docker container
    /// list and the pods of every saved cluster.
    #[serde(default = "default_kluster_refresh_secs")]
    pub kluster_refresh_secs: u64,
    /// Default `--tail N` for `docker logs` / `kubectl logs` from the
    /// Kluster tab.
    #[serde(default = "default_kluster_log_tail_lines")]
    pub kluster_log_tail_lines: u32,
    /// Command prefix used to open an SSH session in a new terminal window
    /// (the `o` hotkey). Empty = auto-detect. Example: `kitty -e`,
    /// `wezterm start --`, `gnome-terminal --`, `alacritty -e`.
    #[serde(default)]
    pub external_terminal: String,
    /// Emit native desktop notifications (tunnel dropped, host up/down).
    /// Enabled by default; toggle from the Settings tab.
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
    /// Custom icon for desktop notifications (path, `~` allowed). Empty = OS
    /// default. On macOS this needs `terminal-notifier` installed — plain
    /// `osascript` can't override the notification icon.
    #[serde(default)]
    pub notification_icon: String,
    /// Git-backed configuration sync. **Must stay the last field**: it
    /// serializes as a `[sync]` TOML table, and anything declared after it
    /// would end up nested inside that table.
    #[serde(default)]
    pub sync: SyncConfig,
}

fn default_port() -> u16 { 22 }
fn default_username() -> String { "root".to_string() }
fn default_auto_health_check() -> bool { true }
fn default_pause_health_on_session() -> bool { true }
fn default_health_ttl_secs() -> u64 { 30 }
fn default_health_probe_timeout_ms() -> u64 { 1500 }
fn default_kluster_refresh_secs() -> u64 { 10 }
fn default_kluster_log_tail_lines() -> u32 { 100 }
fn default_notifications_enabled() -> bool { true }

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            default_port: 22,
            default_username: "root".to_string(),
            default_identity_file: String::new(),
            export_path: String::new(),
            auto_health_check: true,
            pause_health_on_session: true,
            health_ttl_secs: default_health_ttl_secs(),
            health_probe_timeout_ms: default_health_probe_timeout_ms(),
            kluster_refresh_secs: default_kluster_refresh_secs(),
            kluster_log_tail_lines: default_kluster_log_tail_lines(),
            external_terminal: String::new(),
            notifications_enabled: true,
            notification_icon: String::new(),
            sync: SyncConfig::default(),
        }
    }
}

pub fn settings_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });
    base.join("sshm").join("settings.toml")
}

pub fn load_settings() -> AppConfig {
    let path = settings_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<AppConfig>(&content) {
            return cfg;
        }
    }
    AppConfig::default()
}

pub fn try_save_settings(config: &AppConfig) -> Result<()> {
    let path = settings_path();
    let toml_str = toml::to_string_pretty(config).context("serializing settings")?;
    super::io::atomic_write(&path, toml_str.as_bytes())
        .with_context(|| format!("saving settings {}", path.display()))
}

pub fn save_settings(config: &AppConfig) {
    if let Err(e) = try_save_settings(config) {
        eprintln!("save_settings: {e:#}");
    }
}

// -----------------------------------------------------------------------------
// Git-backed configuration sync
// -----------------------------------------------------------------------------

/// Which shared config file a sync run carries.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncItem {
    /// `host.json` — hosts, folders, tunnels. Merged entry-by-entry.
    Hosts,
    /// `kluster.json` — saved clusters and Incus remotes.
    Kluster,
    /// `settings.toml` — app settings, minus the `[sync]` table which always
    /// stays machine-local (see [`crate::sync`]).
    Settings,
    /// `theme.toml` — colors.
    Theme,
}

impl SyncItem {
    /// Every item, in a stable order.
    pub const ALL: &'static [SyncItem] =
        &[SyncItem::Hosts, SyncItem::Kluster, SyncItem::Settings, SyncItem::Theme];

    /// File name used both locally (in `~/.config/sshm`) and in the repo.
    pub fn file_name(&self) -> &'static str {
        match self {
            SyncItem::Hosts => "host.json",
            SyncItem::Kluster => "kluster.json",
            SyncItem::Settings => "settings.toml",
            SyncItem::Theme => "theme.toml",
        }
    }

    /// Absolute path of the local copy.
    pub fn local_path(&self) -> PathBuf {
        super::path::config_dir().join(self.file_name())
    }

    /// Short CLI/TUI label (`hosts`, `kluster`, …).
    pub fn key(&self) -> &'static str {
        match self {
            SyncItem::Hosts => "hosts",
            SyncItem::Kluster => "kluster",
            SyncItem::Settings => "settings",
            SyncItem::Theme => "theme",
        }
    }

    /// Parse a CLI/TUI label. Case-insensitive; unknown labels return `None`.
    pub fn from_key(s: &str) -> Option<SyncItem> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hosts" | "host" | "host.json" => Some(SyncItem::Hosts),
            "kluster" | "kluster.json" => Some(SyncItem::Kluster),
            "settings" | "settings.toml" => Some(SyncItem::Settings),
            "theme" | "theme.toml" => Some(SyncItem::Theme),
            _ => None,
        }
    }
}

/// What to do when the same file changed on both sides since the last sync.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Merge what can be merged (host/cluster entries), and keep **this**
    /// machine's version of anything that truly collides. The default.
    #[default]
    PreferLocal,
    /// Same merge, but a true collision resolves to the remote version.
    PreferRemote,
}

/// When an sshm process syncs on its own.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Never automatic: only `sshm sync` (or a cron entry calling it) syncs.
    #[default]
    Manual,
    /// The TUI syncs every `interval_secs` in the background, and
    /// `sshm sync --if-due` becomes a no-op until the interval has elapsed.
    Interval,
}

/// Git-backed sync of the sshm config across machines.
///
/// Auth is SSH-key based: `repo_url` is an SSH remote (`git@host:owner/repo.git`)
/// and `ssh_key` the private key handed to git through `GIT_SSH_COMMAND`. No
/// credentials are ever written to the repo.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Master switch. Off = every automatic trigger is skipped and an explicit
    /// `sshm sync` refuses with a hint to run `sshm sync setup`.
    #[serde(default)]
    pub enabled: bool,
    /// SSH remote, e.g. `git@github.com:me/sshm-config.git`.
    #[serde(default)]
    pub repo_url: String,
    /// Private key passed to `ssh -i`. `~` is expanded. Empty = whatever the
    /// user's ssh-agent / `~/.ssh/config` provides.
    #[serde(default)]
    pub ssh_key: String,
    /// Branch to track.
    #[serde(default = "default_sync_branch")]
    pub branch: String,
    #[serde(default)]
    pub mode: SyncMode,
    /// Interval used by [`SyncMode::Interval`], in seconds. Floored at 60 by
    /// [`SyncConfig::effective_interval`] so a typo can't hammer the remote.
    #[serde(default = "default_sync_interval_secs")]
    pub interval_secs: u64,
    /// Sync once, right after the TUI starts.
    #[serde(default)]
    pub on_start: bool,
    /// Sync once more when leaving the TUI, so local edits land upstream.
    #[serde(default)]
    pub on_exit: bool,
    /// Files carried by a sync run.
    #[serde(default = "default_sync_items")]
    pub items: Vec<SyncItem>,
    #[serde(default)]
    pub conflict: ConflictPolicy,
    /// Fail instead of trusting an unknown host key. Off by default, which
    /// maps to ssh's `StrictHostKeyChecking=accept-new` (trust on first use,
    /// refuse on change).
    #[serde(default)]
    pub strict_host_key_checking: bool,
}

fn default_sync_branch() -> String { "main".to_string() }
fn default_sync_interval_secs() -> u64 { 900 }
fn default_sync_items() -> Vec<SyncItem> {
    // `settings.toml` is opt-in: it is the most machine-specific of the four
    // (terminal command, notification icon, health intervals).
    vec![SyncItem::Hosts, SyncItem::Kluster, SyncItem::Theme]
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            enabled: false,
            repo_url: String::new(),
            ssh_key: String::new(),
            branch: default_sync_branch(),
            mode: SyncMode::default(),
            interval_secs: default_sync_interval_secs(),
            on_start: false,
            on_exit: false,
            items: default_sync_items(),
            conflict: ConflictPolicy::default(),
            strict_host_key_checking: false,
        }
    }
}

/// Lower bound on the automatic interval, in seconds.
pub const MIN_SYNC_INTERVAL_SECS: u64 = 60;

impl SyncConfig {
    /// True once there is a remote to talk to.
    pub fn is_configured(&self) -> bool {
        !self.repo_url.trim().is_empty()
    }

    /// Enabled *and* pointed at a remote.
    pub fn is_active(&self) -> bool {
        self.enabled && self.is_configured()
    }

    /// Tilde-expanded private key path, or `None` when unset.
    pub fn expanded_key(&self) -> Option<String> {
        let k = self.ssh_key.trim();
        if k.is_empty() {
            None
        } else {
            Some(shellexpand::tilde(k).to_string())
        }
    }

    /// Branch to track, never empty.
    pub fn effective_branch(&self) -> String {
        let b = self.branch.trim();
        if b.is_empty() { default_sync_branch() } else { b.to_string() }
    }

    /// Items to carry, never empty (falls back to the defaults) and
    /// deduplicated in [`SyncItem::ALL`] order.
    pub fn effective_items(&self) -> Vec<SyncItem> {
        let chosen: Vec<SyncItem> = SyncItem::ALL
            .iter()
            .copied()
            .filter(|i| self.items.contains(i))
            .collect();
        if chosen.is_empty() { default_sync_items() } else { chosen }
    }

    /// Automatic interval in seconds, floored at [`MIN_SYNC_INTERVAL_SECS`],
    /// or `None` when the mode isn't interval-driven.
    pub fn effective_interval(&self) -> Option<u64> {
        match self.mode {
            SyncMode::Interval => Some(self.interval_secs.max(MIN_SYNC_INTERVAL_SECS)),
            SyncMode::Manual => None,
        }
    }
}
