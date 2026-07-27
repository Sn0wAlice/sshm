//! This app's own background-tunnel children.
//!
//! Spawns `ssh -N` port-forwards (argv built by `sshm_core::tunnels`), tracks
//! the child handles, and mirrors them to `~/.config/sshm/tunnels/<pid>.json`
//! in the shared record format — so the TUI's dashboard sees this app's tunnels
//! and vice-versa. We only ever kill children we started.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};

use chrono::Utc;
use sshm_core::models::{Host, Tunnel};
use sshm_core::tunnels::{build_tunnel_argv, state_file_for, TunnelRecord};

struct Owned {
    child: Child,
    record: TunnelRecord,
}

#[derive(Default)]
pub struct GuiTunnels {
    active: Vec<Owned>,
}

impl GuiTunnels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a background tunnel for `host`/`tunnel`, record it, and persist.
    pub fn start(
        &mut self,
        host: &Host,
        tunnel: &Tunnel,
        all_hosts: &HashMap<String, Host>,
    ) -> Result<(), String> {
        self.reap();

        let argv = build_tunnel_argv(host, tunnel, all_hosts);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd.spawn().map_err(|e| e.to_string())?;

        let record = TunnelRecord {
            pid: child.id(),
            host_name: host.name.clone(),
            host_display: format!("{}@{}:{}", host.username, host.host, host.port),
            tunnel: tunnel.clone(),
            started: Utc::now().to_rfc3339(),
        };
        self.active.push(Owned { child, record });
        self.persist();
        Ok(())
    }

    /// Stop a tunnel this app owns (by pid). Errors if we didn't start it.
    pub fn stop(&mut self, pid: u32) -> Result<(), String> {
        match self.active.iter().position(|o| o.record.pid == pid) {
            Some(pos) => {
                let mut o = self.active.remove(pos);
                let _ = o.child.kill();
                let _ = o.child.wait();
                self.persist();
                Ok(())
            }
            None => Err(format!("no tunnel with pid {pid} is owned by this app")),
        }
    }

    /// Drop children that have exited on their own; persist if anything changed.
    pub fn reap(&mut self) {
        let before = self.active.len();
        self.active.retain_mut(|o| matches!(o.child.try_wait(), Ok(None)));
        if self.active.len() != before {
            self.persist();
        }
    }

    /// Kill every child and remove our state file. Call on window close.
    pub fn shutdown(&mut self) {
        for o in &mut self.active {
            let _ = o.child.kill();
            let _ = o.child.wait();
        }
        self.active.clear();
        let _ = std::fs::remove_file(state_file_for(std::process::id()));
    }

    fn persist(&self) {
        let path = state_file_for(std::process::id());
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let records: Vec<&TunnelRecord> = self.active.iter().map(|o| &o.record).collect();
        if let Ok(json) = serde_json::to_string_pretty(&records) {
            let _ = sshm_core::config::io::atomic_write(&path, json.as_bytes());
        }
    }
}
