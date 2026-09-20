//! `sshm tunnel` — inspect and stop background tunnels from the command line.
//!
//! Tunnels are started by the TUI (`p` on a host) and recorded in
//! `~/.config/sshm/tunnels/<pid>.json`, one file per running sshm. That format
//! is the shared contract [`sshm_core::tunnels`] defines, so this command reads
//! every instance's file rather than talking to a particular sshm.
//!
//! Deliberately read-and-stop only: starting a tunnel from here would leave an
//! `ssh -N` owned by a process that exits immediately, and the TUI's
//! orphan-recovery would kill it on next launch. Starting stays in the TUI,
//! where something is alive to own the child.

use sshm_core::tunnels::{read_all_records, TunnelRecord};

use crate::models::TunnelKind;

pub fn dispatch(args: &[String]) {
    match args.get(2).map(String::as_str) {
        None | Some("list") | Some("ls") => list(),
        Some("stop") => match args.get(3) {
            Some(pid) => stop(pid),
            None => {
                eprintln!("Usage: sshm tunnel stop <pid>   (see `sshm tunnel list`)");
            }
        },
        Some(other) => {
            eprintln!("Unknown subcommand: tunnel {other}");
            usage();
        }
    }
}

pub fn usage() {
    println!("Usage:");
    println!("  sshm tunnel [list]          # running background tunnels, every instance");
    println!("  sshm tunnel stop <pid>      # terminate one tunnel by its ssh PID");
}

/// One-line route summary, matching the TUI dashboard's wording.
fn route(r: &TunnelRecord) -> String {
    let t = &r.tunnel;
    match t.kind {
        TunnelKind::Dynamic => format!("SOCKS5 on :{}", t.local_port),
        TunnelKind::Local => {
            let rh = if t.remote_host.is_empty() { "localhost" } else { &t.remote_host };
            format!(":{} -> {}:{}", t.local_port, rh, t.remote_port)
        }
        TunnelKind::Remote => {
            let rh = if t.remote_host.is_empty() { "localhost" } else { &t.remote_host };
            format!("remote :{} -> {}:{}", t.local_port, rh, t.remote_port)
        }
    }
}

/// Records whose `ssh -N` is still alive. A record can outlive its process
/// when the owning sshm was killed hard, so liveness is checked rather than
/// trusted — the same `ps` guard the TUI uses before signalling a PID.
fn live_records() -> Vec<TunnelRecord> {
    read_all_records()
        .into_iter()
        .filter(|r| crate::tui::app::tunnels::pid_is_ssh_tunnel(r.pid))
        .collect()
}

fn list() {
    let mut records = live_records();
    if records.is_empty() {
        println!("No background tunnels running.");
        return;
    }
    records.sort_by(|a, b| a.host_name.cmp(&b.host_name).then(a.pid.cmp(&b.pid)));

    println!("{:<8}  {:<18}  {:<28}  LABEL", "PID", "HOST", "ROUTE");
    for r in &records {
        let label = if r.tunnel.label.trim().is_empty() { "-" } else { r.tunnel.label.trim() };
        println!("{:<8}  {:<18}  {:<28}  {}", r.pid, r.host_name, route(r), label);
    }
}

fn stop(pid_arg: &str) {
    let Ok(pid) = pid_arg.trim().parse::<u32>() else {
        eprintln!("'{pid_arg}' is not a PID. See `sshm tunnel list`.");
        return;
    };
    let Some(record) = live_records().into_iter().find(|r| r.pid == pid) else {
        eprintln!("No running sshm tunnel with PID {pid}. See `sshm tunnel list`.");
        return;
    };
    if crate::tui::app::tunnels::terminate_tunnel_pid(pid) {
        println!("Stopped {} ({})", route(&record), record.host_name);
        println!("Note: the sshm instance that owns it will drop it from its list on the next tick.");
    } else {
        eprintln!("Could not signal PID {pid}.");
    }
}
