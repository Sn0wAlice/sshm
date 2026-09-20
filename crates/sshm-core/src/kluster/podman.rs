//! Podman containers, local daemon.
//!
//! Podman mirrors Docker's command line for everything sshm uses, so this is
//! a set of wrappers over [`engine::PODMAN`] — the command construction lives
//! in [`super::engine`] and the output parsers in [`super::docker`], which
//! podman reuses verbatim rather than getting a second copy.
//!
//! **Local only, for now.** Docker remotes work because `DOCKER_HOST=ssh://…`
//! is all a remote daemon needs; podman's equivalent is `CONTAINER_HOST` plus
//! a `podman system connection` set up on the machine, which is a different
//! flow rather than the same one with another variable. The engine knows the
//! right variable, so adding remotes later is wiring, not redesign.
//!
//! **Rootless is the normal case.** Podman usually runs without a daemon at
//! all, per user; `podman info` still answers, which is what availability is
//! probed with, so nothing special is needed here.

use anyhow::Result;
use std::process::ExitStatus;

use super::docker::parse_docker_ps;
use super::engine;
use super::models::{ContainerDetail, ContainerInfo, LifecycleAction};

/// True when `podman info` answers. Cached briefly, independently of Docker's
/// probe — a machine can have one, both or neither.
pub fn available() -> bool {
    engine::PODMAN.daemon_running()
}

/// Drop the cached availability so the next call probes fresh.
pub fn invalidate_cache() {
    engine::PODMAN.invalidate_cache();
}

/// `podman ps -a`, parsed into [`ContainerInfo`]. Empty when podman is not
/// there — never a hard error, since the discovery worker polls on a loop.
pub fn list_containers() -> Result<Vec<ContainerInfo>> {
    Ok(engine::PODMAN
        .list_containers_raw(None)?
        .map(|raw| parse_docker_ps(&raw))
        .unwrap_or_default())
}

/// `podman exec -it <id> <shell>` in the foreground.
pub fn exec_shell(id: &str) -> std::io::Result<ExitStatus> {
    engine::PODMAN.exec_shell(id, None)
}

/// `podman logs [--tail N] [--follow] <id>` in the foreground.
pub fn logs(id: &str, tail: u32, follow: bool) -> std::io::Result<ExitStatus> {
    engine::PODMAN.logs(id, tail, follow, None)
}

/// `podman start|stop|restart <id>`.
pub fn lifecycle(id: &str, action: LifecycleAction) -> Result<()> {
    engine::PODMAN.lifecycle(id, action, None)
}

/// The rich detail view, from `podman inspect` plus a short log tail.
pub fn inspect_detail(id: &str) -> Result<ContainerDetail> {
    super::docker::detail_from(engine::PODMAN, id, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_podman_lists_nothing_rather_than_failing() {
        // On a machine without podman — which is most of them — the Kluster
        // tab must simply not show the section, not surface an error.
        if available() {
            eprintln!("skipping: podman is installed here");
            return;
        }
        assert_eq!(list_containers().unwrap(), Vec::new());
    }

    #[test]
    fn podman_shares_dockers_ps_parser() {
        // The compatibility this backend rests on: identical `--format`
        // output, so one parser serves both.
        let raw = "abc123\tweb\tnginx:latest\tUp 2 minutes\trunning\n";
        let parsed = parse_docker_ps(raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "web");
        assert!(parsed[0].running);
    }
}
