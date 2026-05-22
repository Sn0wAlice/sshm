//! Thin wrappers around the `incus` CLI.
//!
//! Mirrors `docker.rs` but for [Incus](https://linuxcontainers.org/incus/),
//! the LXC-based system-container/VM manager. A "remote" in Incus terms is
//! roughly equivalent to a saved cluster: `local` is implicit, anything
//! else needs a `<remote>:` prefix on instance names.

use std::io::stdout;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{cursor::Show, execute, terminal::disable_raw_mode};

use super::models::{IncusInstance, LifecycleAction};
use super::shell::SHELL_PATH;

struct AvailCache {
    last: Instant,
    value: bool,
}

static AVAIL_CACHE: Mutex<Option<AvailCache>> = Mutex::new(None);
const AVAIL_TTL: Duration = Duration::from_secs(5);

/// True when the `incus` CLI exists *and* the local daemon is reachable.
/// Cached for [`AVAIL_TTL`] to avoid repeating the daemon probe.
pub fn local_available() -> bool {
    if let Ok(mut g) = AVAIL_CACHE.lock() {
        if let Some(c) = g.as_ref() {
            if c.last.elapsed() < AVAIL_TTL {
                return c.value;
            }
        }
        let value = Command::new("incus")
            .args(["info", "--target", "none"])
            // `incus info` (no target) prints daemon info on success.
            // Using --target none is invalid syntax → fall back to plain info.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .or_else(|_| {
                Command::new("incus")
                    .arg("info")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
            })
            .unwrap_or(false);
        *g = Some(AvailCache { last: Instant::now(), value });
        value
    } else {
        false
    }
}

pub fn invalidate_cache() {
    if let Ok(mut g) = AVAIL_CACHE.lock() { *g = None; }
}

/// Discover saved Incus remotes (excluding `local`) by scraping
/// `incus remote list --format=csv`. Format: `name,url,protocol,...`.
pub fn list_remotes() -> Vec<String> {
    let out = match Command::new("incus")
        .args(["remote", "list", "--format=csv"])
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    parse_remotes_csv(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_remotes_csv(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let name = line.split(',').next()?.trim();
            // Strip trailing " (current)" annotation incus adds on the active remote.
            let name = name.split_whitespace().next().unwrap_or(name);
            if name.is_empty() || name == "local" || name == "NAME" {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

/// `incus list [<remote>:] --format=csv -c ns,t,d` parsed into [`IncusInstance`].
///
/// Columns:
/// - n: name
/// - s: status
/// - t: type (container / virtual-machine)
/// - d: description (we abuse this slot for image; not all setups populate it,
///   but `-c` doesn't expose image directly — use `--columns=ns,t,4` instead)
///
/// We use `--format=json` for richer fields and parse minimally.
pub fn list_instances(remote: Option<&str>) -> Result<Vec<IncusInstance>> {
    if !local_available() {
        return Ok(Vec::new());
    }
    let mut cmd = Command::new("incus");
    cmd.arg("list");
    if let Some(r) = remote {
        cmd.arg(format!("{}:", r));
    }
    cmd.args(["--format=csv", "-c", "ns,t,b"]);
    cmd.stderr(Stdio::null());
    let out = cmd.output().context("running `incus list`")?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_list_csv(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse the CSV emitted by `incus list -c ns,t,b`.
/// Columns: name, status, type, base-image (often blank).
pub fn parse_list_csv(raw: &str) -> Vec<IncusInstance> {
    raw.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 3 { return None; }
            let name = parts[0].trim().to_string();
            if name.is_empty() || name.eq_ignore_ascii_case("name") {
                return None;
            }
            let status = parts[1].trim().to_string();
            let kind = parts[2].trim().to_string();
            let image = parts.get(3).map(|s| s.trim().to_string()).unwrap_or_default();
            let running = status.eq_ignore_ascii_case("running");
            Some(IncusInstance { name, kind, status, image, running })
        })
        .collect()
}

fn qualified(name: &str, remote: Option<&str>) -> String {
    match remote {
        Some(r) => format!("{}:{}", r, name),
        None => name.to_string(),
    }
}

/// `incus exec [<remote>:]<name> -- /bin/sh`.
pub fn exec_shell(name: &str, remote: Option<&str>) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    Command::new("incus")
        .arg("exec")
        .arg(qualified(name, remote))
        .args(["--", SHELL_PATH])
        .status()
}

/// Approximate `docker logs -f` for Incus instances by streaming
/// `journalctl -fn N` from inside the instance. Only works on systemd-based
/// images; on others the call exits non-zero and the caller surfaces a toast.
pub fn logs(
    name: &str,
    remote: Option<&str>,
    tail: u32,
    follow: bool,
) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut journal_args = vec!["-n".to_string(), tail.to_string()];
    if follow {
        journal_args.push("-f".to_string());
    }
    let inner = format!("journalctl {}", journal_args.join(" "));
    Command::new("incus")
        .arg("exec")
        .arg(qualified(name, remote))
        .args(["--", "sh", "-c", &inner])
        .status()
}

/// Run `incus start|stop|restart [<remote>:]<name>`. Output is captured; on
/// failure the daemon's stderr is surfaced. `stop` and `restart` get a 5s
/// graceful timeout.
pub fn lifecycle(
    name: &str,
    remote: Option<&str>,
    action: LifecycleAction,
) -> Result<()> {
    let mut cmd = Command::new("incus");
    cmd.arg(action.subcommand()).arg(qualified(name, remote));
    if matches!(action, LifecycleAction::Stop | LifecycleAction::Restart) {
        cmd.arg("--timeout=5");
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());
    let out = cmd.output().context("running incus lifecycle command")?;
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
    fn parse_remotes_skips_header_local_and_empty() {
        let raw = "\
NAME,URL,PROTOCOL,AUTH TYPE,PUBLIC,GLOBAL
local (current),unix://,incus,,no,no
homelab,https://10.0.0.1:8443,incus,tls,no,no
public-images,https://images.linuxcontainers.org,simplestreams,none,yes,no
";
        let v = parse_remotes_csv(raw);
        assert_eq!(v, vec!["homelab".to_string(), "public-images".to_string()]);
    }

    #[test]
    fn parse_list_basic() {
        let raw = "\
nginx,RUNNING,container,images:debian/12
db,STOPPED,container,
vm-1,RUNNING,virtual-machine,images:ubuntu/24.04
";
        let v = parse_list_csv(raw);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].name, "nginx");
        assert_eq!(v[0].kind, "container");
        assert!(v[0].running);
        assert!(!v[1].running);
        assert_eq!(v[2].kind, "virtual-machine");
        assert_eq!(v[2].image, "images:ubuntu/24.04");
    }

    #[test]
    fn parse_list_empty_and_header() {
        assert!(parse_list_csv("").is_empty());
        assert!(parse_list_csv("NAME,STATUS,TYPE,BASE\n").is_empty());
    }

    #[test]
    fn qualified_uses_remote_prefix_when_set() {
        assert_eq!(qualified("nginx", None), "nginx");
        assert_eq!(qualified("nginx", Some("homelab")), "homelab:nginx");
    }
}
