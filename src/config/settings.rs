use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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
}

fn default_port() -> u16 { 22 }
fn default_username() -> String { "root".to_string() }
fn default_auto_health_check() -> bool { true }
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
            health_ttl_secs: default_health_ttl_secs(),
            health_probe_timeout_ms: default_health_probe_timeout_ms(),
            kluster_refresh_secs: default_kluster_refresh_secs(),
            kluster_log_tail_lines: default_kluster_log_tail_lines(),
            external_terminal: String::new(),
            notifications_enabled: true,
            notification_icon: String::new(),
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating settings dir {}", parent.display()))?;
    }
    let toml_str = toml::to_string_pretty(config).context("serializing settings")?;

    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, &toml_str)
        .with_context(|| format!("writing temp settings {}", tmp.display()))?;
    let _ = fs::remove_file(&path);
    if let Err(e) = fs::rename(&tmp, &path) {
        fs::write(&path, &toml_str)
            .with_context(|| format!("rename failed ({e}); fallback-write {}", path.display()))?;
    }
    Ok(())
}

pub fn save_settings(config: &AppConfig) {
    if let Err(e) = try_save_settings(config) {
        eprintln!("save_settings: {e:#}");
    }
}
