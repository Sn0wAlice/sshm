//! Thin wrappers around Apple's `container` CLI (macOS 26+ / Apple silicon).
//!
//! Apple's `container` tool runs Linux containers in lightweight per-container
//! VMs. Its CLI is docker-shaped (`container ls`, `exec`, `logs`, `start`,
//! `stop`, `inspect`), so this module mirrors [`super::docker`] closely and
//! reuses [`ContainerInfo`] for the list snapshot.
//!
//! JSON schema (from apple/container `ContainerSnapshot`): each `container ls
//! --format json` element is `{ configuration: { id, image: { reference },
//! initProcess, mounts, publishedPorts, … }, status: "running"|"stopped"|…,
//! networks: [ { ipv4Address, … } ], startedDate }`. `id` doubles as the name.

use std::io::stdout;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{cursor::Show, execute, terminal::disable_raw_mode};

use super::models::{ContainerDetail, ContainerInfo, DetailSection, LifecycleAction};
use super::shell::SHELL_PATH;

/// True when `name` resolves on `PATH`.
fn bin_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Cached "is the `container` system service up?" check. `container system
/// status` is a round-trip to the apiserver, so cache it like the Docker
/// daemon probe.
struct SystemCache {
    last: Instant,
    value: bool,
}
static SYSTEM_CACHE: Mutex<Option<SystemCache>> = Mutex::new(None);
const SYSTEM_TTL: Duration = Duration::from_secs(5);

/// Whether the Apple `container` runtime is usable here: macOS, the CLI on
/// PATH, and the system service answering. Result cached for [`SYSTEM_TTL`].
pub fn available() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    if let Ok(mut guard) = SYSTEM_CACHE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.last.elapsed() < SYSTEM_TTL {
                return c.value;
            }
        }
        let value = bin_exists("container")
            && Command::new("container")
                .args(["system", "status"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        *guard = Some(SystemCache { last: Instant::now(), value });
        value
    } else {
        false
    }
}

/// Force-clear the availability cache so the next [`available`] call re-probes.
pub fn invalidate_cache() {
    if let Ok(mut g) = SYSTEM_CACHE.lock() {
        *g = None;
    }
}

/// `container ls -a --format json` parsed into [`ContainerInfo`]. Returns an
/// empty vec when the runtime is unavailable (never a hard error, since the
/// worker polls on a loop).
pub fn list_containers() -> Result<Vec<ContainerInfo>> {
    if !available() {
        return Ok(Vec::new());
    }
    let out = Command::new("container")
        .args(["ls", "-a", "--format", "json"])
        .stderr(Stdio::null())
        .output()
        .context("running `container ls`")?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Ok(parse_container_ls(&raw))
}

/// Pure parser for `container ls --format json`. Defensive against schema
/// drift — every field is optional and falls back to a sensible default.
pub fn parse_container_ls(raw: &str) -> Vec<ContainerInfo> {
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter().filter_map(snapshot_to_info).collect()
}

/// Map one `ContainerSnapshot` JSON object to a [`ContainerInfo`].
fn snapshot_to_info(v: &serde_json::Value) -> Option<ContainerInfo> {
    let cfg = v.get("configuration")?;
    let id = cfg.get("id").and_then(|x| x.as_str())?.to_string();
    let image = cfg
        .get("image")
        .and_then(|i| i.get("reference"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let status_raw = v.get("status").and_then(|x| x.as_str()).unwrap_or("unknown");
    let running = status_raw.eq_ignore_ascii_case("running");
    // Present the status title-cased ("Running", "Stopped") to line up with
    // the human-readable Docker status strings in the same list.
    let status = title_case(status_raw);
    Some(ContainerInfo {
        id: id.clone(),
        name: id, // Apple containers are keyed by id; the id *is* the name.
        image,
        status,
        running,
    })
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `container exec -i -t <id> /bin/sh` in the foreground.
pub fn exec_shell(id: &str) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    Command::new("container")
        .args(["exec", "-i", "-t", id, SHELL_PATH])
        .status()
}

/// `container logs [-n N] [--follow] <id>` in the foreground.
pub fn logs(id: &str, tail: u32, follow: bool) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut cmd = Command::new("container");
    cmd.arg("logs").arg("-n").arg(tail.to_string());
    if follow {
        cmd.arg("--follow");
    }
    cmd.arg(id);
    cmd.status()
}

/// `container start|stop <id>`. Apple's CLI has no `restart`, so we emulate it
/// with a stop followed by a start.
pub fn lifecycle(id: &str, action: LifecycleAction) -> Result<()> {
    match action {
        LifecycleAction::Start => run_simple(&["start", id]),
        LifecycleAction::Stop => run_simple(&["stop", id]),
        LifecycleAction::Restart => {
            // Best-effort stop (ignore "already stopped"), then start.
            let _ = run_simple(&["stop", id]);
            run_simple(&["start", id])
        }
    }
}

/// Build the rich detail view for one Apple container: `container inspect
/// <id>` parsed into sections, plus a short log tail.
pub fn inspect_detail(id: &str) -> Result<ContainerDetail> {
    let out = Command::new("container")
        .args(["inspect", id])
        .stderr(Stdio::null())
        .output()
        .context("running `container inspect`")?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("container inspect exited {}", out.status));
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let mut detail = parse_inspect(id, &raw)
        .ok_or_else(|| anyhow::anyhow!("could not parse container inspect JSON"))?;
    // Best-effort recent logs (non-follow). Ignore failures.
    if let Ok(o) = Command::new("container")
        .args(["logs", "-n", "20", id])
        .stderr(Stdio::null())
        .output()
    {
        if o.status.success() {
            detail.log_tail = String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();
        }
    }
    Ok(detail)
}

/// Pure parser: `container inspect` JSON (array of `ContainerSnapshot`) → the
/// first snapshot as a [`ContainerDetail`]. Defensive against missing fields.
pub fn parse_inspect(id: &str, raw: &str) -> Option<ContainerDetail> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let snap = value.as_array().and_then(|a| a.first()).unwrap_or(&value);
    let cfg = snap.get("configuration")?;

    let mut overview = DetailSection::new("Overview");
    overview.push("Name", cfg.get("id").and_then(|x| x.as_str()).unwrap_or(id));
    overview.push(
        "Image",
        cfg.get("image").and_then(|i| i.get("reference")).and_then(|x| x.as_str()).unwrap_or(""),
    );
    overview.push("Status", title_case(snap.get("status").and_then(|x| x.as_str()).unwrap_or("unknown")));
    if let Some(res) = cfg.get("resources") {
        if let Some(cpus) = res.get("cpus").and_then(|x| x.as_u64()) {
            overview.push("CPUs", cpus.to_string());
        }
        if let Some(mem) = res.get("memoryInBytes").and_then(|x| x.as_u64()) {
            overview.push("Memory", human_bytes(mem));
        }
    }
    overview.push("OS/Arch", platform_str(cfg.get("platform")));
    overview.push("Created", cfg.get("creationDate").and_then(|x| x.as_str()).unwrap_or(""));
    overview.push("Started", snap.get("startedDate").and_then(|x| x.as_str()).unwrap_or(""));

    // Networking — the snapshot's top-level `networks` array of Attachments.
    let mut net = DetailSection::new("Networking");
    if let Some(arr) = snap.get("networks").and_then(|x| x.as_array()) {
        for (i, a) in arr.iter().enumerate() {
            let prefix = if arr.len() > 1 { format!("[{}] ", i) } else { String::new() };
            if let Some(ip) = a.get("ipv4Address").and_then(|x| x.as_str()) {
                net.push(format!("{}IPv4", prefix), ip);
            }
            if let Some(gw) = a.get("ipv4Gateway").and_then(|x| x.as_str()) {
                net.push(format!("{}Gateway", prefix), gw);
            }
            if let Some(host) = a.get("hostname").and_then(|x| x.as_str()) {
                net.push(format!("{}Hostname", prefix), host);
            }
            if let Some(mac) = a.get("macAddress").and_then(|x| x.as_str()) {
                net.push(format!("{}MAC", prefix), mac);
            }
        }
    }

    // Published ports.
    let mut ports = DetailSection::new("Ports");
    if let Some(arr) = cfg.get("publishedPorts").and_then(|x| x.as_array()) {
        for p in arr {
            let host_addr = p.get("hostAddress").and_then(|x| x.as_str()).unwrap_or("0.0.0.0");
            let host_port = p.get("hostPort").and_then(|x| x.as_u64()).unwrap_or(0);
            let ctr_port = p.get("containerPort").and_then(|x| x.as_u64()).unwrap_or(0);
            let proto = p.get("proto").and_then(|x| x.as_str()).unwrap_or("tcp");
            ports.push(
                format!("{}:{}", host_addr, host_port),
                format!("→ {}/{}", ctr_port, proto),
            );
        }
    }

    // Mounts / volumes.
    let mut mounts = DetailSection::new("Volumes");
    if let Some(arr) = cfg.get("mounts").and_then(|x| x.as_array()) {
        for m in arr {
            let src = m.get("source").and_then(|x| x.as_str()).unwrap_or("");
            let dst = m.get("destination").and_then(|x| x.as_str()).unwrap_or("");
            if !dst.is_empty() {
                mounts.push(dst.to_string(), if src.is_empty() { "(anonymous)".into() } else { src.to_string() });
            }
        }
    }

    // Command / entrypoint.
    let mut command = DetailSection::new("Command");
    if let Some(proc_) = cfg.get("initProcess") {
        if let Some(exe) = proc_.get("executable").and_then(|x| x.as_str()) {
            command.push("Executable", exe);
        }
        if let Some(args) = proc_.get("arguments").and_then(|x| x.as_array()) {
            let joined: Vec<String> = args.iter().filter_map(|a| a.as_str().map(String::from)).collect();
            command.push("Arguments", joined.join(" "));
        }
        if let Some(wd) = proc_.get("workingDirectory").and_then(|x| x.as_str()) {
            command.push("WorkingDir", wd);
        }
    }

    let sections: Vec<DetailSection> = [overview, net, ports, mounts, command]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let title = cfg.get("id").and_then(|x| x.as_str()).unwrap_or(id).to_string();
    Some(ContainerDetail { title, sections, log_tail: Vec::new() })
}

fn platform_str(v: Option<&serde_json::Value>) -> String {
    let v = match v { Some(v) => v, None => return String::new() };
    let os = v.get("os").and_then(|x| x.as_str()).unwrap_or("");
    let arch = v.get("architecture").and_then(|x| x.as_str()).unwrap_or("");
    match (os.is_empty(), arch.is_empty()) {
        (false, false) => format!("{}/{}", os, arch),
        (false, true) => os.to_string(),
        (true, false) => arch.to_string(),
        _ => String::new(),
    }
}

/// Human-readable byte size (MiB/GiB), matching how container runtimes report.
fn human_bytes(n: u64) -> String {
    const KI: u64 = 1024;
    const MI: u64 = KI * 1024;
    const GI: u64 = MI * 1024;
    if n >= GI {
        format!("{:.1} GiB", n as f64 / GI as f64)
    } else if n >= MI {
        format!("{:.0} MiB", n as f64 / MI as f64)
    } else {
        format!("{} B", n)
    }
}

fn run_simple(args: &[&str]) -> Result<()> {
    let out = Command::new("container")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("running container command")?;
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

    const SAMPLE: &str = r#"[
      {
        "configuration": {
          "id": "web",
          "image": { "reference": "docker.io/library/nginx:1.27" },
          "initProcess": { "executable": "/docker-entrypoint.sh", "arguments": ["nginx","-g","daemon off;"] }
        },
        "status": "running",
        "networks": [ { "ipv4Address": "192.168.64.3/24" } ]
      },
      {
        "configuration": {
          "id": "db",
          "image": { "reference": "postgres:16" }
        },
        "status": "stopped"
      }
    ]"#;

    #[test]
    fn parse_two_containers() {
        let v = parse_container_ls(SAMPLE);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "web");
        assert_eq!(v[0].name, "web");
        assert_eq!(v[0].image, "docker.io/library/nginx:1.27");
        assert_eq!(v[0].status, "Running");
        assert!(v[0].running);
        assert_eq!(v[1].name, "db");
        assert!(!v[1].running);
    }

    #[test]
    fn parse_empty_array() {
        assert!(parse_container_ls("[]").is_empty());
    }

    #[test]
    fn parse_garbage_is_empty() {
        assert!(parse_container_ls("not json").is_empty());
        assert!(parse_container_ls("{}").is_empty());
    }

    #[test]
    fn parse_skips_entries_without_id() {
        let raw = r#"[ { "configuration": { "image": { "reference": "x" } }, "status": "running" } ]"#;
        assert!(parse_container_ls(raw).is_empty());
    }

    const INSPECT: &str = r#"[
      {
        "configuration": {
          "id": "web",
          "image": { "reference": "nginx:1.27" },
          "resources": { "cpus": 4, "memoryInBytes": 1073741824 },
          "platform": { "os": "linux", "architecture": "arm64" },
          "publishedPorts": [ { "hostAddress": "0.0.0.0", "hostPort": 8080, "containerPort": 80, "proto": "tcp" } ],
          "mounts": [ { "source": "/data", "destination": "/var/www" } ],
          "initProcess": { "executable": "/docker-entrypoint.sh", "arguments": ["nginx","-g","daemon off;"], "workingDirectory": "/" }
        },
        "status": "running",
        "networks": [ { "ipv4Address": "192.168.64.3/24", "ipv4Gateway": "192.168.64.1", "hostname": "web" } ]
      }
    ]"#;

    #[test]
    fn parse_inspect_sections() {
        let d = parse_inspect("web", INSPECT).unwrap();
        assert_eq!(d.title, "web");
        let titles: Vec<&str> = d.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Overview", "Networking", "Ports", "Volumes", "Command"]);
        let overview = &d.sections[0];
        assert!(overview.rows.iter().any(|(k, v)| k == "Image" && v == "nginx:1.27"));
        assert!(overview.rows.iter().any(|(k, v)| k == "Memory" && v == "1.0 GiB"));
        assert!(overview.rows.iter().any(|(k, v)| k == "OS/Arch" && v == "linux/arm64"));
        let net = &d.sections[1];
        assert!(net.rows.iter().any(|(k, v)| k == "IPv4" && v == "192.168.64.3/24"));
    }

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(1073741824), "1.0 GiB");
        assert_eq!(human_bytes(536870912), "512 MiB");
        assert_eq!(human_bytes(512), "512 B");
    }
}
