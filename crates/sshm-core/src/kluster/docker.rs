//! Thin wrappers around the `docker` CLI.

use std::process::ExitStatus;

use anyhow::Result;

use super::engine;
use super::models::{ContainerDetail, ContainerInfo, DetailSection, LifecycleAction};

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
            ..Default::default()
        }
    }

    #[test]
    fn omits_port_when_default() {
        assert_eq!(
            host_to_docker_uri(&h("alice", "1.2.3.4", 22)),
            "ssh://alice@1.2.3.4"
        );
    }
    #[test]
    fn includes_port_when_custom() {
        assert_eq!(
            host_to_docker_uri(&h("alice", "1.2.3.4", 2222)),
            "ssh://alice@1.2.3.4:2222"
        );
    }
}

/// Cached "is the daemon up?" check. `docker info` is heavy (~100ms) so we
/// don't want to call it on every refresh tick.
/// Returns true when the local Docker daemon answers `docker info`.
/// Caches the result for a few seconds to avoid repeated probes.
pub fn daemon_running() -> bool {
    engine::DOCKER.daemon_running()
}

/// Force-clear the daemon cache so the next `daemon_running()` call probes
/// fresh. Useful right after a UI-triggered refresh.
pub fn invalidate_daemon_cache() {
    engine::DOCKER.invalidate_cache();
}

/// `docker ps -a` with a tab-separated format, parsed into [`ContainerInfo`].
///
/// `docker_host`:
/// - `None` → local daemon. Skipped entirely if [`daemon_running`] says no.
/// - `Some(uri)` → set `DOCKER_HOST=<uri>` and try the remote. Failure there
///   is an `Err` so callers can flag the host unreachable; a local daemon that
///   is simply down is `Ok(empty)`, which the existing flow expects.
pub fn list_containers(docker_host: Option<&str>) -> Result<Vec<ContainerInfo>> {
    Ok(engine::DOCKER
        .list_containers_raw(docker_host)?
        .map(|raw| parse_docker_ps(&raw))
        .unwrap_or_default())
}

/// Pure parser for the tab-separated `docker ps` output we ask for above.
pub fn parse_docker_ps(raw: &str) -> Vec<ContainerInfo> {
    raw.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 5 {
                return None;
            }
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
    engine::DOCKER.exec_shell(id, docker_host)
}

/// Run `docker logs [--tail N] [--follow] <id>` in the foreground.
pub fn logs(
    id: &str,
    tail: u32,
    follow: bool,
    docker_host: Option<&str>,
) -> std::io::Result<ExitStatus> {
    engine::DOCKER.logs(id, tail, follow, docker_host)
}

/// Run `docker start|stop|restart <id>` (optionally against a remote daemon).
pub fn lifecycle(id: &str, action: LifecycleAction, docker_host: Option<&str>) -> Result<()> {
    engine::DOCKER.lifecycle(id, action, docker_host)
}

/// Build the rich detail view for one Docker container from `docker inspect`,
/// plus a short log tail. `docker_host = Some(uri)` routes over SSH.
pub fn inspect_detail(id: &str, docker_host: Option<&str>) -> Result<ContainerDetail> {
    detail_from(engine::DOCKER, id, docker_host)
}

/// Shared by Docker and Podman: inspect, parse, then attach a log tail.
pub(crate) fn detail_from(
    cli: engine::ContainerCli,
    id: &str,
    remote: Option<&str>,
) -> Result<ContainerDetail> {
    let raw = cli.inspect_raw(id, remote)?;
    let mut detail = parse_inspect(id, &raw)
        .ok_or_else(|| anyhow::anyhow!("could not parse `{} inspect` JSON", cli.bin))?;
    detail.log_tail = cli.log_tail(id, 20, remote);
    Ok(detail)
}

/// Pure parser: `docker inspect` JSON (array) → the first element as a
/// [`ContainerDetail`]. Defensive against missing fields across engine versions.
pub fn parse_inspect(id: &str, raw: &str) -> Option<ContainerDetail> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let obj = value.as_array().and_then(|a| a.first()).unwrap_or(&value);

    let name = obj
        .get("Name")
        .and_then(|x| x.as_str())
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_else(|| id.to_string());
    let config = obj.get("Config");
    let state = obj.get("State");

    let mut overview = DetailSection::new("Overview");
    overview.push("Name", &name);
    overview.push(
        "Image",
        config
            .and_then(|c| c.get("Image"))
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );
    overview.push(
        "Status",
        state
            .and_then(|s| s.get("Status"))
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );
    if let Some(pid) = state.and_then(|s| s.get("Pid")).and_then(|x| x.as_u64()) {
        if pid != 0 {
            overview.push("PID", pid.to_string());
        }
    }
    overview.push(
        "Created",
        obj.get("Created").and_then(|x| x.as_str()).unwrap_or(""),
    );
    overview.push(
        "Started",
        state
            .and_then(|s| s.get("StartedAt"))
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );

    // Networking — top-level IP plus each named network.
    let mut net = DetailSection::new("Networking");
    let netset = obj.get("NetworkSettings");
    if let Some(ip) = netset
        .and_then(|n| n.get("IPAddress"))
        .and_then(|x| x.as_str())
    {
        net.push("IPv4", ip);
    }
    if let Some(gw) = netset
        .and_then(|n| n.get("Gateway"))
        .and_then(|x| x.as_str())
    {
        net.push("Gateway", gw);
    }
    if let Some(mac) = netset
        .and_then(|n| n.get("MacAddress"))
        .and_then(|x| x.as_str())
    {
        net.push("MAC", mac);
    }
    if let Some(networks) = netset
        .and_then(|n| n.get("Networks"))
        .and_then(|x| x.as_object())
    {
        for (nname, nval) in networks {
            if let Some(ip) = nval.get("IPAddress").and_then(|x| x.as_str()) {
                if !ip.is_empty() {
                    net.push(format!("net:{}", nname), ip);
                }
            }
        }
    }

    // Ports — NetworkSettings.Ports maps "80/tcp" → [ {HostIp, HostPort} ].
    let mut ports = DetailSection::new("Ports");
    if let Some(pmap) = netset
        .and_then(|n| n.get("Ports"))
        .and_then(|x| x.as_object())
    {
        for (cport, bindings) in pmap {
            match bindings.as_array() {
                Some(arr) if !arr.is_empty() => {
                    for b in arr {
                        let hip = b
                            .get("HostIp")
                            .and_then(|x| x.as_str())
                            .unwrap_or("0.0.0.0");
                        let hport = b.get("HostPort").and_then(|x| x.as_str()).unwrap_or("");
                        ports.push(format!("{}:{}", hip, hport), format!("→ {}", cport));
                    }
                }
                _ => ports.push(cport.clone(), "(exposed, not published)".to_string()),
            }
        }
    }

    // Mounts.
    let mut mounts = DetailSection::new("Volumes");
    if let Some(arr) = obj.get("Mounts").and_then(|x| x.as_array()) {
        for m in arr {
            let dst = m.get("Destination").and_then(|x| x.as_str()).unwrap_or("");
            let src = m.get("Source").and_then(|x| x.as_str()).unwrap_or("");
            let rw = m.get("RW").and_then(|x| x.as_bool()).unwrap_or(true);
            if !dst.is_empty() {
                let mode = if rw { "rw" } else { "ro" };
                let val = if src.is_empty() {
                    format!("(volume, {})", mode)
                } else {
                    format!("{} ({})", src, mode)
                };
                mounts.push(dst.to_string(), val);
            }
        }
    }

    // Command / entrypoint.
    let mut command = DetailSection::new("Command");
    if let Some(ep) = config
        .and_then(|c| c.get("Entrypoint"))
        .and_then(|x| x.as_array())
    {
        let joined: Vec<String> = ep
            .iter()
            .filter_map(|a| a.as_str().map(String::from))
            .collect();
        command.push("Entrypoint", joined.join(" "));
    }
    if let Some(cmd) = config.and_then(|c| c.get("Cmd")).and_then(|x| x.as_array()) {
        let joined: Vec<String> = cmd
            .iter()
            .filter_map(|a| a.as_str().map(String::from))
            .collect();
        command.push("Cmd", joined.join(" "));
    }
    command.push(
        "WorkingDir",
        config
            .and_then(|c| c.get("WorkingDir"))
            .and_then(|x| x.as_str())
            .unwrap_or(""),
    );

    let sections: Vec<DetailSection> = [overview, net, ports, mounts, command]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    Some(ContainerDetail {
        title: name,
        sections,
        log_tail: Vec::new(),
    })
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

    const INSPECT: &str = r#"[
      {
        "Name": "/web",
        "Created": "2026-01-01T00:00:00Z",
        "State": { "Status": "running", "Pid": 4321, "StartedAt": "2026-01-01T00:00:01Z" },
        "Config": { "Image": "nginx:1.27", "Cmd": ["nginx","-g","daemon off;"], "Entrypoint": ["/docker-entrypoint.sh"], "WorkingDir": "/" },
        "NetworkSettings": {
          "IPAddress": "172.17.0.2",
          "Gateway": "172.17.0.1",
          "Ports": { "80/tcp": [ { "HostIp": "0.0.0.0", "HostPort": "8080" } ] },
          "Networks": { "bridge": { "IPAddress": "172.17.0.2" } }
        },
        "Mounts": [ { "Source": "/data", "Destination": "/var/www", "RW": true } ]
      }
    ]"#;

    #[test]
    fn parse_inspect_sections() {
        let d = parse_inspect("cid", INSPECT).unwrap();
        assert_eq!(d.title, "web");
        let titles: Vec<&str> = d.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Overview", "Networking", "Ports", "Volumes", "Command"]
        );
        let overview = &d.sections[0];
        assert!(overview
            .rows
            .iter()
            .any(|(k, v)| k == "Image" && v == "nginx:1.27"));
        assert!(overview.rows.iter().any(|(k, v)| k == "PID" && v == "4321"));
        let ports = &d.sections[2];
        assert!(ports
            .rows
            .iter()
            .any(|(k, v)| k == "0.0.0.0:8080" && v == "→ 80/tcp"));
        let mounts = &d.sections[3];
        assert!(mounts
            .rows
            .iter()
            .any(|(k, v)| k == "/var/www" && v == "/data (rw)"));
    }

    #[test]
    fn parse_inspect_garbage() {
        assert!(parse_inspect("x", "nope").is_none());
    }
}
