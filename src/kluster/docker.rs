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

use super::models::ContainerInfo;
use super::shell::SHELL_PATH;

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
/// Returns an empty `Vec` when the daemon is unreachable so the UI can
/// distinguish "no containers" from "no daemon" via [`daemon_running`].
pub fn list_containers() -> Result<Vec<ContainerInfo>> {
    if !daemon_running() {
        return Ok(Vec::new());
    }
    let out = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--format",
            "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.State}}",
        ])
        .stderr(Stdio::null())
        .output()
        .context("running `docker ps`")?;
    if !out.status.success() {
        return Ok(Vec::new());
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
/// The caller is expected to have already left the alternate screen; this
/// function inherits stdin/stdout/stderr so the user lands directly in the
/// container.
pub fn exec_shell(id: &str) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    Command::new("docker")
        .args(["exec", "-it", id, SHELL_PATH])
        .status()
}

/// Run `docker logs [--tail N] [--follow] <id>` in the foreground.
pub fn logs(id: &str, tail: u32, follow: bool) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut cmd = Command::new("docker");
    cmd.arg("logs").arg("--tail").arg(tail.to_string());
    if follow {
        cmd.arg("--follow");
    }
    cmd.arg(id);
    cmd.status()
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
