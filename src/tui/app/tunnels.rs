//! Background SSH tunnels — spawn `ssh -N` port-forwards that keep running
//! while you use the rest of the TUI, plus a dashboard popup to watch and
//! stop them.
//!
//! ## Lifetime & persistence
//!
//! The [`TunnelManager`] is owned by `main` and threaded through every
//! `run_tui` call, so tunnels survive connecting to a host and coming back.
//!
//! Each instance also mirrors its live tunnels to a **per-instance** state
//! file `~/.config/sshm/tunnels/<sshm-pid>.json`. Per-instance (not shared)
//! means two SSHM processes never race on the same file. On startup
//! [`recover_orphans`] scans the *other* files: if the owning SSHM is gone
//! (crash / SIGKILL — which skips our cleanup), every tunnel PID it listed is
//! verified to still be an `ssh -N` process and SIGTERM'd, then the stale
//! file is removed. The verification guards against PID reuse killing an
//! unrelated process.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use serde::{Deserialize, Serialize};

use crate::models::{Host, Tunnel, TunnelKind};
use crate::ssh::proxy::resolve_proxy_jump;
use crate::tui::ssh::portforward::build_forward_arg;
use crate::tui::theme::Theme;

/// PIDs of every live background tunnel of *this* process. Used by
/// [`kill_all`] for the clean-quit cleanup (`q::press` calls `process::exit`,
/// which skips destructors).
static ACTIVE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn register_pid(pid: u32) {
    if let Ok(mut g) = ACTIVE_PIDS.lock() {
        g.push(pid);
    }
}

fn unregister_pid(pid: u32) {
    if let Ok(mut g) = ACTIVE_PIDS.lock() {
        g.retain(|&p| p != pid);
    }
}

/// SIGTERM every background tunnel of this process and drop our state file.
/// Call this right before the process exits.
pub fn kill_all() {
    if let Ok(g) = ACTIVE_PIDS.lock() {
        for &pid in g.iter() {
            // SAFETY: kill() with a plain signal is safe; a stale PID just
            // yields ESRCH, which we ignore.
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
    let _ = fs::remove_file(state_file_for(std::process::id()));
}

// ============================================================================
// State-file persistence
// ============================================================================

/// `~/.config/sshm/tunnels/`.
fn tunnels_dir() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("sshm");
    p.push("tunnels");
    p
}

/// Path of the state file owned by the SSHM process with PID `sshm_pid`.
fn state_file_for(sshm_pid: u32) -> PathBuf {
    tunnels_dir().join(format!("{}.json", sshm_pid))
}

/// One tunnel as serialized to the per-instance state file.
#[derive(Serialize, Deserialize)]
struct PersistEntry {
    /// PID of the `ssh -N` process.
    pid: u32,
    host_name: String,
    host_display: String,
    tunnel: Tunnel,
    started: DateTime<Utc>,
}

/// The command line of `pid`, via `ps`. `None` when the process is gone.
fn process_cmdline(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// True when `pid` is alive *and* looks like an `sshm` process.
fn process_is_sshm(pid: u32) -> bool {
    process_cmdline(pid)
        .map(|c| c.contains("sshm"))
        .unwrap_or(false)
}

/// True when `pid` is alive *and* looks like one of our `ssh -N` tunnels.
/// This guards a SIGTERM against PID reuse hitting an unrelated process.
fn process_is_ssh_tunnel(pid: u32) -> bool {
    process_cmdline(pid)
        .map(|c| {
            let has_ssh = c
                .split_whitespace()
                .any(|w| w == "ssh" || w.ends_with("/ssh"));
            let has_n = c.split_whitespace().any(|w| w == "-N");
            has_ssh && has_n
        })
        .unwrap_or(false)
}

/// Scan the tunnels dir for state files left behind by SSHM instances that
/// are no longer running (crash / SIGKILL) — plus a stale file from a prior
/// process that reused our PID — verify their tunnels are still live `ssh -N`
/// processes, SIGTERM those, and delete the files. Returns the kill count.
fn recover_orphans() -> usize {
    let our_pid = std::process::id();
    let dir = tunnels_dir();
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return 0,
    };

    let mut killed = 0usize;
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let owner_pid: u32 = match path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        // A file whose owner SSHM is still running belongs to a live instance
        // — leave it alone. Our own PID's file is always stale here (we run
        // recovery before writing anything), so it is treated as an orphan.
        if owner_pid != our_pid && process_is_sshm(owner_pid) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<PersistEntry>>(&content) {
                for e in list {
                    if process_is_ssh_tunnel(e.pid) {
                        // SAFETY: see kill_all.
                        unsafe {
                            libc::kill(e.pid as i32, libc::SIGTERM);
                        }
                        killed += 1;
                    }
                }
            }
        }
        let _ = fs::remove_file(&path);
    }
    killed
}

// ============================================================================
// Manager
// ============================================================================

/// One running background tunnel.
pub struct ActiveTunnel {
    pub host_name: String,
    /// `user@host:port` for display.
    pub host_display: String,
    pub tunnel: Tunnel,
    pub started: DateTime<Utc>,
    child: Child,
}

/// Registry of background tunnels. Owned by `main`, shared across `run_tui`.
pub struct TunnelManager {
    pub active: Vec<ActiveTunnel>,
    /// Orphan tunnels cleaned from a previous crashed session — surfaced as a
    /// one-time toast by `run_tui`, which then resets this to 0.
    pub recovered_orphans: usize,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    /// Build the manager and clean up tunnels orphaned by a previous crash.
    pub fn new() -> Self {
        TunnelManager {
            active: Vec::new(),
            recovered_orphans: recover_orphans(),
        }
    }

    /// Spawn `ssh -N <forward…>` detached (all stdio to /dev/null) and record
    /// it. Returns an error message when the spawn fails — or when an
    /// equivalent tunnel is already running (exact relaunch, or a local-port
    /// clash that `ssh` would just fail to bind).
    pub fn start(
        &mut self,
        host: &Host,
        tunnel: &Tunnel,
        all_hosts: &HashMap<String, Host>,
    ) -> Result<(), String> {
        // Refresh first: a tunnel may have died while the port-forward modal
        // was open (the main loop — and its reap — was paused).
        self.reap();

        // Refuse an exact relaunch of an already-running tunnel.
        if self
            .active
            .iter()
            .any(|a| a.host_name == host.name && same_route(&a.tunnel, tunnel))
        {
            return Err("an identical tunnel is already running".to_string());
        }
        // Local (-L) and dynamic (-D) tunnels bind a local port — refuse a
        // second one on the same port (ssh would fail with "address in use").
        if matches!(tunnel.kind, TunnelKind::Local | TunnelKind::Dynamic) {
            if let Some(a) = self.active.iter().find(|a| {
                matches!(a.tunnel.kind, TunnelKind::Local | TunnelKind::Dynamic)
                    && a.tunnel.local_port == tunnel.local_port
            }) {
                return Err(format!(
                    "local port {} is already used by a tunnel on {}",
                    tunnel.local_port, a.host_name
                ));
            }
        }

        let mut cmd = Command::new("ssh");
        cmd.arg("-N");
        for a in build_forward_arg(tunnel) {
            cmd.arg(a);
        }
        cmd.arg(format!("{}@{}", host.username, host.host))
            .arg("-p")
            .arg(host.port.to_string());
        if let Some(id) = &host.identity_file {
            if !id.is_empty() {
                cmd.arg("-i").arg(id);
            }
        }
        if let Some(j) = &host.proxy_jump {
            if let Some(resolved) = resolve_proxy_jump(j, all_hosts) {
                cmd.arg("-J").arg(resolved);
            }
        }
        if host.forward_agent {
            cmd.arg("-A");
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = cmd.spawn().map_err(|e| e.to_string())?;
        register_pid(child.id());
        self.active.push(ActiveTunnel {
            host_name: host.name.clone(),
            host_display: format!("{}@{}:{}", host.username, host.host, host.port),
            tunnel: tunnel.clone(),
            started: Utc::now(),
            child,
        });
        self.persist();
        Ok(())
    }

    /// Drop tunnels whose `ssh` process has exited on its own (port clash,
    /// connection lost, remote closed it…).
    pub fn reap(&mut self) {
        let before = self.active.len();
        self.active.retain_mut(|t| match t.child.try_wait() {
            Ok(Some(_)) => {
                unregister_pid(t.child.id());
                false
            }
            _ => true,
        });
        if self.active.len() != before {
            self.persist();
        }
    }

    /// Kill and forget the tunnel at `idx`.
    pub fn stop(&mut self, idx: usize) {
        if idx < self.active.len() {
            let mut t = self.active.remove(idx);
            unregister_pid(t.child.id());
            let _ = t.child.kill();
            let _ = t.child.wait();
            self.persist();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// Mirror the live set to our per-instance state file (atomic write), or
    /// delete the file when there is nothing to record. Best-effort.
    fn persist(&self) {
        let path = state_file_for(std::process::id());
        if self.active.is_empty() {
            let _ = fs::remove_file(&path);
            return;
        }
        let entries: Vec<PersistEntry> = self
            .active
            .iter()
            .map(|t| PersistEntry {
                pid: t.child.id(),
                host_name: t.host_name.clone(),
                host_display: t.host_display.clone(),
                tunnel: t.tunnel.clone(),
                started: t.started,
            })
            .collect();
        let Ok(json) = serde_json::to_string_pretty(&entries) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

// ============================================================================
// Dashboard popup
// ============================================================================

/// True when two tunnels forward the same thing (label aside) — used to spot
/// an exact relaunch. For `Dynamic`, `remote_host`/`remote_port` are unused
/// and compare equal anyway.
fn same_route(a: &Tunnel, b: &Tunnel) -> bool {
    a.kind == b.kind
        && a.local_port == b.local_port
        && a.remote_host == b.remote_host
        && a.remote_port == b.remote_port
}

/// One-line summary of a tunnel's forwarding, e.g. `:8080 → localhost:80`.
fn tunnel_route(t: &Tunnel) -> String {
    match t.kind {
        TunnelKind::Dynamic => format!("SOCKS5 on :{}", t.local_port),
        TunnelKind::Local => {
            let rh = if t.remote_host.is_empty() { "localhost" } else { &t.remote_host };
            format!(":{} → {}:{}", t.local_port, rh, t.remote_port)
        }
        TunnelKind::Remote => {
            let rh = if t.remote_host.is_empty() { "localhost" } else { &t.remote_host };
            format!("remote :{} → {}:{}", t.local_port, rh, t.remote_port)
        }
    }
}

/// `hh:mm:ss` (or `mm:ss`) for an elapsed second count.
fn fmt_uptime(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/// Render the background-tunnels dashboard as a centered popup overlay.
pub fn draw_tunnels_popup(
    f: &mut Frame,
    manager: &TunnelManager,
    selected: usize,
    theme: &Theme,
) {
    let area = f.area();
    let now = Utc::now();

    let mut lines: Vec<ListItem> = Vec::new();
    for t in &manager.active {
        let secs = (now - t.started).num_seconds().max(0) as u64;
        let label = if t.tunnel.label.is_empty() {
            String::new()
        } else {
            format!("  “{}”", t.tunnel.label)
        };
        lines.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!(" {:<3}", t.tunnel.kind.short()),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<26}", tunnel_route(&t.tunnel)),
                Style::default().fg(theme.fg),
            ),
            Span::styled(
                format!("{:<22}", t.host_name),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("up {}", fmt_uptime(secs)),
                Style::default().fg(theme.success),
            ),
            Span::styled(label, Style::default().fg(theme.muted)),
        ])));
    }

    let body_h = manager.active.len().max(1) as u16;
    let w = 78.min(area.width.max(1));
    let h = (body_h + 4).min(area.height.max(1)); // borders + title + footer
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect { x, y, width: w, height: h };

    let block = Block::default()
        .title(format!(" Background tunnels — {} active ", manager.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if manager.is_empty() {
        f.render_widget(
            Paragraph::new("  No active tunnels. Start one with 'p' on a host.")
                .style(Style::default().fg(theme.muted)),
            chunks[0],
        );
    } else {
        let mut ls = ListState::default();
        ls.select(Some(selected.min(manager.active.len().saturating_sub(1))));
        let list = List::new(lines)
            .highlight_symbol("➜ ")
            .highlight_style(
                Style::default()
                    .bg(theme.accent)
                    .fg(theme.bg)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, chunks[0], &mut ls);
    }

    f.render_widget(
        Paragraph::new("  ↑↓ move   d/x stop   Esc/t close")
            .style(Style::default().fg(theme.muted)),
        chunks[1],
    );
}
