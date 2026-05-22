//! Thin wrappers around the `docker` CLI.

use std::io::stdout;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use std::sync::Mutex;

use anyhow::{Context, Result};
use crossterm::{
    cursor::Show,
    execute,
    terminal::disable_raw_mode,
};

use super::models::{ContainerInfo, LifecycleAction};
use super::shell::SHELL_PATH;

/// Build the `ssh://[user@]host[:port]` URI passed to `DOCKER_HOST` for a
/// remote Docker daemon reached over SSH.
pub fn host_to_docker_uri(h: &crate::models::Host) -> String {
    if h.port == 22 {
        format!("ssh://{}@{}", h.username, h.host)
    } else {
        format!("ssh://{}@{}:{}", h.username, h.host, h.port)
    }
}

#[cfg(test)]
mod ssh_uri_tests {
    use super::*;
    use crate::models::Host;

    fn h(user: &str, host: &str, port: u16) -> Host {
        Host {
            name: "x".into(),
            host: host.into(),
            port,
            username: user.into(),
            identity_file: None,
            proxy_jump: None,
            tags: None,
            folder: None,
            last_connected_at: None,
            use_count: 0,
            favorite: false,
            tunnels: vec![],
            forward_agent: false,
            mosh: false,
            notes: None,
        }
    }

    #[test]
    fn omits_port_when_default() {
        assert_eq!(host_to_docker_uri(&h("alice", "1.2.3.4", 22)), "ssh://alice@1.2.3.4");
    }
    #[test]
    fn includes_port_when_custom() {
        assert_eq!(host_to_docker_uri(&h("alice", "1.2.3.4", 2222)), "ssh://alice@1.2.3.4:2222");
    }
}

/// Cached "is the daemon up?" check. `docker info` is heavy (~100ms) so we
/// don't want to call it on every refresh tick.
struct DaemonCache {
    last: Instant,
    value: bool,
}

static DAEMON_CACHE: Mutex<Option<DaemonCache>> = Mutex::new(None);
const DAEMON_TTL: Duration = Duration::from_secs(5);

/// Returns true when the local Docker daemon answers `docker info`.
/// Caches the result for [`DAEMON_TTL`] to avoid repeated probes.
pub fn daemon_running() -> bool {
    if let Ok(mut guard) = DAEMON_CACHE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.last.elapsed() < DAEMON_TTL {
                return c.value;
            }
        }
        let value = Command::new("docker")
            .arg("info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        *guard = Some(DaemonCache { last: Instant::now(), value });
        value
    } else {
        false
    }
}

/// Force-clear the daemon cache so the next `daemon_running()` call probes
/// fresh. Useful right after a UI-triggered refresh.
pub fn invalidate_daemon_cache() {
    if let Ok(mut g) = DAEMON_CACHE.lock() {
        *g = None;
    }
}

/// `docker ps -a` with a tab-separated format, parsed into [`ContainerInfo`].
///
/// `docker_host`:
/// - `None` → local daemon. Skipped entirely if [`daemon_running`] says no.
/// - `Some(uri)` → set `DOCKER_HOST=<uri>` and try the remote. Returns an
///   empty `Vec` on failure (network error, daemon down, key rejected) — we
///   don't surface those as hard errors because the worker probes on a loop.
pub fn list_containers(docker_host: Option<&str>) -> Result<Vec<ContainerInfo>> {
    if docker_host.is_none() && !daemon_running() {
        return Ok(Vec::new());
    }
    let mut cmd = Command::new("docker");
    if let Some(u) = docker_host {
        cmd.env("DOCKER_HOST", u);
    }
    cmd.args([
        "ps",
        "-a",
        "--format",
        "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.State}}",
    ])
    .stderr(Stdio::null());
    let out = cmd.output().context("running `docker ps`")?;
    if !out.status.success() {
        // For a remote, surface this as Err so callers can flag the host
        // as unreachable; for the local daemon we historically return
        // Ok(empty) which the existing flow expects.
        return if docker_host.is_some() {
            Err(anyhow::anyhow!("docker ps exited {}", out.status))
        } else {
            Ok(Vec::new())
        };
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Ok(parse_docker_ps(&raw))
}

/// Pure parser for the tab-separated `docker ps` output we ask for above.
pub fn parse_docker_ps(raw: &str) -> Vec<ContainerInfo> {
    raw.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 5 { return None; }
            let state = parts[4].trim().to_ascii_lowercase();
            Some(ContainerInfo {
                id: parts[0].trim().to_string(),
                name: parts[1].trim().to_string(),
                image: parts[2].trim().to_string(),
                status: parts[3].trim().to_string(),
                running: state == "running",
            })
        })
        .collect()
}

/// Run `docker exec -it <id> /bin/sh` in the foreground.
/// `docker_host = Some(uri)` routes via SSH (`DOCKER_HOST=<uri>`).
pub fn exec_shell(id: &str, docker_host: Option<&str>) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut cmd = Command::new("docker");
    if let Some(u) = docker_host { cmd.env("DOCKER_HOST", u); }
    cmd.args(["exec", "-it", id, SHELL_PATH]).status()
}

/// Run `docker logs [--tail N] [--follow] <id>` in the foreground.
pub fn logs(
    id: &str,
    tail: u32,
    follow: bool,
    docker_host: Option<&str>,
) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut cmd = Command::new("docker");
    if let Some(u) = docker_host { cmd.env("DOCKER_HOST", u); }
    cmd.arg("logs").arg("--tail").arg(tail.to_string());
    if follow {
        cmd.arg("--follow");
    }
    cmd.arg(id);
    cmd.status()
}

/// Run `docker start|stop|restart <id>` (optionally against a remote daemon).
/// Output is captured; on failure the daemon's stderr is surfaced. `stop` and
/// `restart` are bounded to a 5s graceful window so the UI doesn't freeze for
/// Docker's default 10s.
pub fn lifecycle(
    id: &str,
    action: LifecycleAction,
    docker_host: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new("docker");
    if let Some(u) = docker_host {
        cmd.env("DOCKER_HOST", u);
    }
    cmd.arg(action.subcommand());
    if matches!(action, LifecycleAction::Stop | LifecycleAction::Restart) {
        cmd.args(["-t", "5"]);
    }
    cmd.arg(id).stdout(Stdio::null()).stderr(Stdio::piped());
    let out = cmd.output().context("running docker lifecycle command")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(anyhow::anyhow!(
            "{}",
            if err.is_empty() { "non-zero exit".to_string() } else { err }
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let raw = "abc123\tnginx-1\tnginx:1.27\tUp 2 hours\trunning\n\
                   def456\tpg-test\tpostgres:16\tExited (0) 5 minutes ago\texited\n";
        let v = parse_docker_ps(raw);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "nginx-1");
        assert_eq!(v[0].image, "nginx:1.27");
        assert!(v[0].running);
        assert!(!v[1].running);
    }

    #[test]
    fn parse_empty() {
        assert!(parse_docker_ps("").is_empty());
        assert!(parse_docker_ps("\n\n").is_empty());
    }

    #[test]
    fn parse_skips_short_lines() {
        // Only 3 fields → skipped.
        let raw = "abc\tname\timage\n";
        assert!(parse_docker_ps(raw).is_empty());
    }
}
