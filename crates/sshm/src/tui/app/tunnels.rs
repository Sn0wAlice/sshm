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
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use serde::{Deserialize, Serialize};

use crate::models::{Host, Tunnel, TunnelKind};
use crate::tui::theme::Theme;
use crate::tunnels::build_tunnel_argv;

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

// The directory and the file naming are the engine's: `sshm tunnel` reads the
// same records through `sshm_core::tunnels`, and a second definition here is
// how the two would quietly start looking in different places.
use crate::tunnels::{state_file_for, tunnels_dir};

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
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// True when `pid` is alive *and* looks like an `sshm` process.
fn process_is_sshm(pid: u32) -> bool {
    process_cmdline(pid)
        .map(|c| c.contains("sshm"))
        .unwrap_or(false)
}

/// True when `pid` is alive *and* looks like one of our `ssh -N` tunnels.
/// This guards a SIGTERM against PID reuse hitting an unrelated process.
///
/// Public as [`pid_is_ssh_tunnel`] for the `sshm tunnel` CLI, which reads
/// another instance's records and must apply the same guard.
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
        let owner_pid: u32 = match path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok())
        {
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

/// How many times a dropped tunnel is relaunched before sshm stops trying.
/// A tunnel whose local port is taken, or whose host is gone for good, would
/// otherwise respawn forever.
const MAX_RESTARTS: u32 = 5;

/// Grace period before relaunch number `n` (1-based). Backs off so a host that
/// is down for a minute is not hammered once per reap tick.
fn restart_delay(attempt: u32) -> Duration {
    Duration::from_secs(match attempt {
        0 | 1 => 2,
        2 => 5,
        3 => 15,
        _ => 30,
    })
}

/// One running background tunnel.
pub struct ActiveTunnel {
    pub host_name: String,
    /// `user@host:port` for display.
    pub host_display: String,
    pub tunnel: Tunnel,
    pub started: DateTime<Utc>,
    /// Relaunches performed so far, for the backoff and the give-up rule.
    /// Reset when the user starts the tunnel by hand.
    pub restarts: u32,
    child: Child,
}

/// Registry of background tunnels. Owned by `main`, shared across `run_tui`.
/// A dropped `auto_restart` tunnel waiting out its backoff.
struct PendingRestart {
    host_name: String,
    tunnel: Tunnel,
    /// 1-based relaunch number, carried onto the new child on success.
    attempt: u32,
    due: Instant,
}

pub struct TunnelManager {
    pub active: Vec<ActiveTunnel>,
    /// Tunnels queued for relaunch by [`TunnelManager::restart_due`].
    pending_restarts: Vec<PendingRestart>,
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
            pending_restarts: Vec::new(),
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

        // Same argv builder the engine uses, so a background tunnel reaches a
        // host exactly the way an interactive connection does — identity,
        // ProxyJump chain, agent forwarding and per-host `ssh_options`.
        let argv = build_tunnel_argv(host, tunnel, all_hosts);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
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
            restarts: 0,
            child,
        });
        self.persist();
        Ok(())
    }

    /// Drop tunnels whose `ssh` process has exited on its own (port clash,
    /// connection lost, remote closed it…) and desktop-notify for each.
    ///
    /// A tunnel marked `auto_restart` is queued for relaunch instead of being
    /// announced as closed — see [`Self::restart_due`], which the main loop
    /// calls with the host DB it needs to rebuild the command.
    pub fn reap(&mut self) {
        let mut closed: Vec<String> = Vec::new();
        let mut pending: Vec<PendingRestart> = Vec::new();
        self.active.retain_mut(|t| match t.child.try_wait() {
            Ok(Some(_)) => {
                unregister_pid(t.child.id());
                if t.tunnel.auto_restart && t.restarts < MAX_RESTARTS {
                    pending.push(PendingRestart {
                        host_name: t.host_name.clone(),
                        tunnel: t.tunnel.clone(),
                        attempt: t.restarts + 1,
                        due: Instant::now() + restart_delay(t.restarts + 1),
                    });
                } else {
                    let mut msg = format!("{}  ({})", tunnel_route(&t.tunnel), t.host_name);
                    if t.tunnel.auto_restart {
                        msg.push_str(&format!(" — gave up after {MAX_RESTARTS} restarts"));
                    }
                    closed.push(msg);
                }
                false
            }
            _ => true,
        });
        self.pending_restarts.extend(pending);
        if !closed.is_empty() || !self.pending_restarts.is_empty() {
            self.persist();
        }
        for c in &closed {
            crate::os::notify("SSHM — tunnel closed", c);
        }
    }

    /// Start every `auto_start` tunnel saved on `host` that is not already up.
    ///
    /// Returns the routes actually started, for a toast. A tunnel that is
    /// already running — or whose local port is taken by one — is skipped in
    /// silence: reconnecting to a host you are already tunnelled into is the
    /// normal case, not an error worth interrupting the connection for.
    pub fn start_auto(&mut self, host: &Host, all_hosts: &HashMap<String, Host>) -> Vec<String> {
        let wanted: Vec<Tunnel> = auto_start_tunnels(host).into_iter().cloned().collect();
        let mut started = Vec::new();
        for t in wanted {
            if self.start(host, &t, all_hosts).is_ok() {
                started.push(tunnel_route(&t));
            }
        }
        started
    }

    /// Relaunch every queued tunnel whose backoff has elapsed. Returns the
    /// routes brought back up, for a toast.
    ///
    /// Split from [`Self::reap`] because a relaunch needs the host DB, which
    /// the reap path (called from several places) does not carry.
    pub fn restart_due(&mut self, all_hosts: &HashMap<String, Host>) -> Vec<String> {
        if self.pending_restarts.is_empty() {
            return Vec::new();
        }
        let now = Instant::now();
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_restarts)
            .into_iter()
            .partition(|p| p.due <= now);
        self.pending_restarts = waiting;

        let mut revived = Vec::new();
        for p in ready {
            let Some(host) = all_hosts.get(&p.host_name) else {
                // The host was deleted while the tunnel was down — nothing to
                // reconnect to, so stop trying.
                crate::os::notify(
                    "SSHM — tunnel not restarted",
                    &format!(
                        "{} ({} no longer exists)",
                        tunnel_route(&p.tunnel),
                        p.host_name
                    ),
                );
                continue;
            };
            match self.start(host, &p.tunnel, all_hosts) {
                Ok(()) => {
                    if let Some(t) = self.active.last_mut() {
                        t.restarts = p.attempt;
                    }
                    revived.push(tunnel_route(&p.tunnel));
                }
                Err(_) if p.attempt < MAX_RESTARTS => {
                    // Still failing (host down, port busy) — back off further.
                    self.pending_restarts.push(PendingRestart {
                        due: Instant::now() + restart_delay(p.attempt + 1),
                        attempt: p.attempt + 1,
                        ..p
                    });
                }
                Err(e) => {
                    crate::os::notify(
                        "SSHM — tunnel not restarted",
                        &format!("{} — {}", tunnel_route(&p.tunnel), e),
                    );
                }
            }
        }
        revived
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

/// The tunnels saved on `host` that are marked to come up on connect.
///
/// Split out so the selection can be checked without spawning anything: the
/// rest of `start_auto` is process handling.
pub fn auto_start_tunnels(host: &Host) -> Vec<&Tunnel> {
    host.tunnels.iter().filter(|t| t.auto_start).collect()
}

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
            let rh = if t.remote_host.is_empty() {
                "localhost"
            } else {
                &t.remote_host
            };
            format!(":{} → {}:{}", t.local_port, rh, t.remote_port)
        }
        TunnelKind::Remote => {
            let rh = if t.remote_host.is_empty() {
                "localhost"
            } else {
                &t.remote_host
            };
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
pub fn draw_tunnels_popup(f: &mut Frame, manager: &TunnelManager, selected: usize, theme: &Theme) {
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
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
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
    let rect = Rect {
        x,
        y,
        width: w,
        height: h,
    };

    let block = Block::default()
        .title(format!(" Background tunnels — {} active ", manager.len()))
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
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
        let list = List::new(lines).highlight_symbol("➜ ").highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
        f.render_stateful_widget(list, chunks[0], &mut ls);
    }

    f.render_widget(
        Paragraph::new("  ↑↓ move   d/x stop   o open url   Esc/t/q close")
            .style(Style::default().fg(theme.muted)),
        chunks[1],
    );
}

/// Whether `pid` is a live `ssh -N` started by sshm. Same guard the TUI
/// applies before signalling anything.
pub fn pid_is_ssh_tunnel(pid: u32) -> bool {
    process_is_ssh_tunnel(pid)
}

/// SIGTERM `pid`, but only when it still looks like one of our tunnels.
/// Returns whether the signal was sent.
pub fn terminate_tunnel_pid(pid: u32) -> bool {
    if !process_is_ssh_tunnel(pid) {
        return false;
    }
    // SAFETY: `kill` with SIGTERM on a PID we just verified is one of our own
    // `ssh -N` children — the same call and the same guard as `kill_all`.
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tunnel(kind: TunnelKind, local: u16) -> Tunnel {
        Tunnel {
            label: String::new(),
            kind,
            local_port: local,
            remote_port: 5432,
            remote_host: String::new(),
            auto_start: false,
            auto_restart: false,
        }
    }

    #[test]
    fn the_backoff_grows_and_then_plateaus() {
        // A host that is down for a while must not be retried once per tick,
        // but the delay has to stop growing or the last attempt never lands.
        let delays: Vec<u64> = (1..=6).map(|n| restart_delay(n).as_secs()).collect();
        assert_eq!(delays, vec![2, 5, 15, 30, 30, 30]);
        for pair in delays.windows(2) {
            assert!(pair[1] >= pair[0], "the backoff must never shrink");
        }
    }

    #[test]
    fn the_first_restart_is_not_instant() {
        // Relaunching within the same reap tick would spin on a port clash.
        assert!(restart_delay(1) >= Duration::from_secs(1));
        assert!(restart_delay(0) >= Duration::from_secs(1));
    }

    #[test]
    fn the_give_up_window_is_minutes_not_hours() {
        // How long a dropped tunnel keeps being retried before sshm stops.
        // Pinning the total documents the behaviour instead of leaving it an
        // emergent property of two constants that can drift apart.
        let total: u64 = (1..=MAX_RESTARTS).map(|n| restart_delay(n).as_secs()).sum();
        assert_eq!(total, 82, "≈1min20 of retries before giving up");
    }

    #[test]
    fn same_route_ignores_the_label() {
        // Two tunnels that forward the same thing are the same tunnel, even if
        // the user named them differently — that is what blocks a relaunch.
        let mut a = tunnel(TunnelKind::Local, 8080);
        let mut b = a.clone();
        a.label = "one".into();
        b.label = "two".into();
        assert!(same_route(&a, &b));
        b.local_port = 8081;
        assert!(!same_route(&a, &b));
    }

    #[test]
    fn same_route_distinguishes_kinds_on_the_same_port() {
        let a = tunnel(TunnelKind::Local, 8080);
        let b = tunnel(TunnelKind::Remote, 8080);
        assert!(!same_route(&a, &b));
    }

    #[test]
    fn auto_restart_does_not_affect_route_identity() {
        let a = tunnel(TunnelKind::Local, 8080);
        let mut b = a.clone();
        b.auto_restart = true;
        assert!(same_route(&a, &b), "it is a policy, not part of the route");
    }

    #[test]
    fn the_route_summary_names_what_is_forwarded() {
        assert_eq!(
            tunnel_route(&tunnel(TunnelKind::Dynamic, 1080)),
            "SOCKS5 on :1080"
        );
        assert_eq!(
            tunnel_route(&tunnel(TunnelKind::Local, 15432)),
            ":15432 → localhost:5432"
        );
        let mut t = tunnel(TunnelKind::Local, 15432);
        t.remote_host = "db.internal".into();
        assert_eq!(tunnel_route(&t), ":15432 → db.internal:5432");
    }

    #[test]
    fn uptime_gains_an_hours_field_only_when_needed() {
        assert_eq!(fmt_uptime(0), "00:00");
        assert_eq!(fmt_uptime(65), "01:05");
        assert_eq!(fmt_uptime(3600), "01:00:00");
        assert_eq!(fmt_uptime(3661), "01:01:01");
    }

    #[test]
    fn a_dead_pid_is_never_signalled() {
        // The guard against PID reuse: PID 0 and an absurd PID are not ours.
        assert!(!pid_is_ssh_tunnel(0));
        assert!(!terminate_tunnel_pid(0));
        assert!(!pid_is_ssh_tunnel(u32::MAX));
    }

    #[test]
    fn our_own_process_is_not_mistaken_for_a_tunnel() {
        // The test binary is alive but is not an `ssh -N`.
        assert!(!pid_is_ssh_tunnel(std::process::id()));
    }
}

#[cfg(test)]
mod auto_start_tests {
    use super::*;

    fn tun(label: &str, port: u16, auto: bool) -> Tunnel {
        Tunnel {
            label: label.into(),
            kind: TunnelKind::Local,
            local_port: port,
            remote_port: 5432,
            remote_host: String::new(),
            auto_start: auto,
            auto_restart: false,
        }
    }

    fn host_with(tunnels: Vec<Tunnel>) -> Host {
        Host {
            name: "db".into(),
            host: "10.0.0.9".into(),
            tunnels,
            ..Default::default()
        }
    }

    #[test]
    fn only_the_marked_tunnels_come_up() {
        let h = host_with(vec![
            tun("pg", 5432, true),
            tun("redis", 6379, false),
            tun("metrics", 9090, true),
        ]);
        let picked: Vec<&str> = auto_start_tunnels(&h)
            .iter()
            .map(|t| t.label.as_str())
            .collect();
        assert_eq!(picked, vec!["pg", "metrics"]);
    }

    #[test]
    fn a_host_with_no_marked_tunnel_starts_nothing() {
        assert!(auto_start_tunnels(&host_with(vec![tun("pg", 5432, false)])).is_empty());
        assert!(auto_start_tunnels(&host_with(vec![])).is_empty());
    }

    #[test]
    fn auto_start_and_auto_restart_are_independent() {
        // Two different questions: "bring it up when I connect" and "bring it
        // back if it drops". A tunnel can want either, both or neither.
        let mut t = tun("pg", 5432, true);
        t.auto_restart = false;
        let h = host_with(vec![t]);
        assert_eq!(auto_start_tunnels(&h).len(), 1);

        let mut t = tun("pg", 5432, false);
        t.auto_restart = true;
        let h = host_with(vec![t]);
        assert!(
            auto_start_tunnels(&h).is_empty(),
            "auto_restart alone must not start anything on connect"
        );
    }

    #[test]
    fn neither_flag_changes_what_the_tunnel_forwards() {
        // They are policy, not route: two tunnels differing only in their
        // flags are the same forward, and `start` must still refuse the
        // duplicate.
        let plain = tun("pg", 5432, false);
        let mut flagged = plain.clone();
        flagged.auto_start = true;
        flagged.auto_restart = true;
        assert!(same_route(&plain, &flagged));
        assert_eq!(
            build_tunnel_argv(&host_with(vec![]), &plain, &HashMap::new()),
            build_tunnel_argv(&host_with(vec![]), &flagged, &HashMap::new()),
            "the flags must not reach the ssh command line"
        );
    }

    #[test]
    fn a_host_saved_before_auto_start_existed_still_loads() {
        let old = r#"{"label":"pg","kind":"Local","local_port":5432,"remote_port":5432,"remote_host":""}"#;
        let t: Tunnel = serde_json::from_str(old).expect("pre-2.2 tunnel parses");
        assert!(!t.auto_start);
        assert!(!t.auto_restart);
    }
}
