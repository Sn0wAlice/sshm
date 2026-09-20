//! Frontend-agnostic background-tunnel primitives.
//!
//! This is the shared substrate under both frontends' tunnel features:
//!
//! - argv construction for an `ssh -N` port-forward ([`build_tunnel_argv`]);
//! - the on-disk record format ([`TunnelRecord`]) each sshm instance mirrors to
//!   `~/.config/sshm/tunnels/<pid>.json`, and [`read_all_records`] to read every
//!   instance's file for a cross-instance dashboard.
//!
//! Spawning/killing the child process stays frontend-side (each frontend owns
//! its own children); core only builds the command and defines the format.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::models::{Host, Tunnel, TunnelKind};

/// The `ssh` port-forward flag pair for one tunnel: `-L p:h:p`, `-R p:h:p`, or
/// `-D p`.
pub fn build_forward_arg(t: &Tunnel) -> Vec<String> {
    match t.kind {
        TunnelKind::Local => {
            let rh = if t.remote_host.is_empty() {
                "localhost".to_string()
            } else {
                t.remote_host.clone()
            };
            vec!["-L".into(), format!("{}:{}:{}", t.local_port, rh, t.remote_port)]
        }
        TunnelKind::Remote => {
            let rh = if t.remote_host.is_empty() {
                "localhost".to_string()
            } else {
                t.remote_host.clone()
            };
            vec!["-R".into(), format!("{}:{}:{}", t.local_port, rh, t.remote_port)]
        }
        TunnelKind::Dynamic => vec!["-D".into(), t.local_port.to_string()],
    }
}

/// Full argv for a background tunnel:
/// `ssh -N <forward> user@host -p port [-i id] [-J jump] [-A]`. `argv[0]` is the
/// program (`ssh`). `all_hosts` resolves a named multi-hop ProxyJump chain, the
/// same way [`crate::ssh::client::build_ssh_argv`] does.
pub fn build_tunnel_argv(
    host: &Host,
    tunnel: &Tunnel,
    all_hosts: &HashMap<String, Host>,
) -> Vec<String> {
    let mut argv = vec!["ssh".to_string(), "-N".to_string()];
    argv.extend(build_forward_arg(tunnel));
    argv.push(format!("{}@{}", host.username, host.host));
    argv.extend(crate::ssh::client::build_ssh_opts(host, all_hosts));
    argv
}

/// `~/.config/sshm/tunnels/`.
pub fn tunnels_dir() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("sshm");
    p.push("tunnels");
    p
}

/// State file owned by the sshm process with PID `sshm_pid`.
pub fn state_file_for(sshm_pid: u32) -> PathBuf {
    tunnels_dir().join(format!("{sshm_pid}.json"))
}

/// One running background tunnel, as persisted to a per-instance state file.
///
/// The field names here are the shared on-disk contract every frontend reads
/// and writes, so a dashboard in one instance can list another's tunnels.
/// `started` is an RFC3339 timestamp (the TUI serializes a `DateTime<Utc>` to
/// the exact same string, so the two interoperate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelRecord {
    /// PID of the `ssh -N` process.
    pub pid: u32,
    pub host_name: String,
    /// `user@host:port`, for display.
    pub host_display: String,
    pub tunnel: Tunnel,
    /// RFC3339 start timestamp.
    pub started: String,
}

/// Read every instance's state file and return all recorded tunnels. A missing
/// directory yields an empty list; unparseable files are skipped.
pub fn read_all_records() -> Vec<TunnelRecord> {
    let dir = tunnels_dir();
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(list) = serde_json::from_str::<Vec<TunnelRecord>>(&text) {
                    out.extend(list);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        Host {
            name: "web".into(),
            host: "10.0.0.5".into(),
            port: 2222,
            username: "root".into(),
            ..Default::default()
        }
    }

    #[test]
    fn local_forward_argv() {
        let t = Tunnel {
            label: "pg".into(),
            kind: TunnelKind::Local,
            local_port: 5432,
            remote_port: 5432,
            remote_host: String::new(),
            ..Default::default()
        };
        let argv = build_tunnel_argv(&host(), &t, &HashMap::new());
        assert_eq!(
            argv,
            vec!["ssh", "-N", "-L", "5432:localhost:5432", "root@10.0.0.5", "-p", "2222"]
        );
    }

    #[test]
    fn dynamic_forward_argv() {
        let t = Tunnel {
            label: "socks".into(),
            kind: TunnelKind::Dynamic,
            local_port: 1080,
            remote_port: 0,
            remote_host: String::new(),
            ..Default::default()
        };
        let argv = build_tunnel_argv(&host(), &t, &HashMap::new());
        assert!(argv.contains(&"-D".to_string()));
        assert!(argv.contains(&"1080".to_string()));
    }
}

#[cfg(test)]
mod ssh_option_tests {
    use super::*;

    #[test]
    fn a_background_tunnel_inherits_per_host_ssh_options() {
        // Regression guard for the duplication this replaced: a tunnel used to
        // build its own flags, so anything added to `build_ssh_opts` silently
        // skipped it.
        let host = Host {
            name: "db".into(),
            host: "10.0.0.9".into(),
            port: 2222,
            identity_file: Some("/k/id".into()),
            forward_agent: true,
            ssh_options: vec!["ServerAliveInterval=30".into()],
            ..Default::default()
        };
        let t = Tunnel {
            label: "pg".into(),
            kind: TunnelKind::Local,
            local_port: 5432,
            remote_port: 5432,
            remote_host: String::new(),
            ..Default::default()
        };
        let argv = build_tunnel_argv(&host, &t, &HashMap::new());
        assert_eq!(argv[0], "ssh");
        assert_eq!(argv[1], "-N");
        for expected in ["-p", "2222", "-i", "/k/id", "-A", "-o", "ServerAliveInterval=30"] {
            assert!(argv.iter().any(|a| a == expected), "missing {expected} in {argv:?}");
        }
    }

    #[test]
    fn the_forward_flag_still_precedes_the_target() {
        let host = Host { name: "db".into(), host: "h".into(), ..Default::default() };
        let t = Tunnel {
            label: String::new(),
            kind: TunnelKind::Dynamic,
            local_port: 1080,
            remote_port: 0,
            remote_host: String::new(),
            ..Default::default()
        };
        let argv = build_tunnel_argv(&host, &t, &HashMap::new());
        let fwd = argv.iter().position(|a| a == "-D").unwrap();
        let target = argv.iter().position(|a| a == "root@h").unwrap();
        assert!(fwd < target, "{argv:?}");
    }
}

#[cfg(test)]
mod record_tests {
    use super::*;

    #[test]
    fn a_record_written_before_auto_restart_existed_still_parses() {
        // The on-disk format is a contract between instances: an sshm that has
        // not been restarted yet is still writing the old shape.
        let old = r#"[{"pid":1234,"host_name":"web","host_display":"root@10.0.0.5:22",
            "tunnel":{"label":"pg","kind":"Local","local_port":15432,"remote_port":5432,"remote_host":""},
            "started":"2026-09-20T10:00:00Z"}]"#;
        let list: Vec<TunnelRecord> = serde_json::from_str(old).expect("pre-2.2 record parses");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].pid, 1234);
        assert!(!list[0].tunnel.auto_restart, "defaults to off");
    }

    #[test]
    fn a_record_round_trips_through_json() {
        let r = TunnelRecord {
            pid: 42,
            host_name: "web".into(),
            host_display: "root@10.0.0.5:22".into(),
            tunnel: Tunnel {
                label: "pg".into(),
                kind: TunnelKind::Local,
                local_port: 15432,
                remote_port: 5432,
                remote_host: "db".into(),
                auto_restart: true,
            },
            started: "2026-09-20T10:00:00Z".into(),
        };
        let json = serde_json::to_string(&vec![r]).unwrap();
        let back: Vec<TunnelRecord> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0].pid, 42);
        assert!(back[0].tunnel.auto_restart);
        assert_eq!(back[0].tunnel.remote_host, "db");
    }

    #[test]
    fn auto_restart_reaches_the_argv_path_unchanged() {
        // It is a frontend policy: nothing about it belongs on the ssh command.
        let h = Host { name: "web".into(), host: "h".into(), ..Default::default() };
        let mut t = Tunnel {
            label: String::new(),
            kind: TunnelKind::Local,
            local_port: 8080,
            remote_port: 80,
            remote_host: String::new(),
            auto_restart: false,
        };
        let without = build_tunnel_argv(&h, &t, &HashMap::new());
        t.auto_restart = true;
        assert_eq!(build_tunnel_argv(&h, &t, &HashMap::new()), without);
    }
}
