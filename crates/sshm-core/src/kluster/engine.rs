//! The Docker-compatible container CLIs, parameterised by binary.
//!
//! Podman deliberately mirrors Docker's command line for everything sshm uses
//! — `ps`, `exec`, `logs`, `inspect`, `start`/`stop`/`restart` — so supporting
//! it is a matter of running a different binary, not writing a second backend.
//! This module holds the command construction once; [`super::docker`] and
//! [`super::podman`] are thin wrappers that pick an engine.
//!
//! The parsers are *not* here: they are pure functions on the CLI's output and
//! live in `docker`, which podman reuses. That is also where the real risk of
//! this sharing sits — the two engines' `inspect` JSON is compatible in the
//! fields sshm reads, but it is compatible by convention rather than by spec,
//! so [`super::docker::parse_inspect`] stays defensive about every field.

use std::collections::HashMap;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::models::LifecycleAction;
use super::shell::shell_path;

/// One container CLI: which binary to run, and how to point it at a remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerCli {
    /// Executable name, looked up on PATH.
    pub bin: &'static str,
    /// Environment variable that redirects the CLI to a remote daemon.
    pub host_env: &'static str,
    /// How the engine is named in the UI and in error messages.
    pub label: &'static str,
}

pub const DOCKER: ContainerCli = ContainerCli {
    bin: "docker",
    host_env: "DOCKER_HOST",
    label: "Docker",
};

pub const PODMAN: ContainerCli = ContainerCli {
    bin: "podman",
    host_env: "CONTAINER_HOST",
    label: "Podman",
};

struct DaemonCache {
    last: Instant,
    value: bool,
}

/// Keyed by binary name: each engine is probed and cached independently, so a
/// machine with both does not have one answer stand in for the other.
static DAEMON_CACHE: Mutex<Option<HashMap<&'static str, DaemonCache>>> = Mutex::new(None);
const DAEMON_TTL: Duration = Duration::from_secs(5);

impl ContainerCli {
    fn command(&self, remote: Option<&str>) -> Command {
        let mut cmd = Command::new(self.bin);
        if let Some(u) = remote {
            cmd.env(self.host_env, u);
        }
        cmd
    }

    /// True when the engine's daemon answers `<bin> info`. Cached for
    /// [`DAEMON_TTL`]: `info` is a round-trip and the worker polls on a loop.
    pub fn daemon_running(&self) -> bool {
        let Ok(mut guard) = DAEMON_CACHE.lock() else {
            return false;
        };
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(c) = map.get(self.bin) {
            if c.last.elapsed() < DAEMON_TTL {
                return c.value;
            }
        }
        let value = Command::new(self.bin)
            .arg("info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        map.insert(
            self.bin,
            DaemonCache {
                last: Instant::now(),
                value,
            },
        );
        value
    }

    /// Drop this engine's cached probe so the next call asks again.
    pub fn invalidate_cache(&self) {
        if let Ok(mut g) = DAEMON_CACHE.lock() {
            if let Some(map) = g.as_mut() {
                map.remove(self.bin);
            }
        }
    }

    /// `<bin> ps -a` in the tab-separated format
    /// [`super::docker::parse_docker_ps`] expects.
    ///
    /// `remote = None` means the local daemon, and returns an empty list when
    /// it is not running. `Some(uri)` means a remote: a failure there *is* an
    /// error, because the caller flags the host unreachable.
    pub fn list_containers_raw(&self, remote: Option<&str>) -> Result<Option<String>> {
        if remote.is_none() && !self.daemon_running() {
            return Ok(None);
        }
        let mut cmd = self.command(remote);
        cmd.args([
            "ps",
            "-a",
            "--format",
            "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.State}}",
        ])
        .stderr(Stdio::null());
        let out = cmd
            .output()
            .with_context(|| format!("running `{} ps`", self.bin))?;
        if !out.status.success() {
            return if remote.is_some() {
                Err(anyhow::anyhow!("{} ps exited {}", self.bin, out.status))
            } else {
                Ok(None)
            };
        }
        Ok(Some(String::from_utf8_lossy(&out.stdout).to_string()))
    }

    pub fn exec_shell(&self, id: &str, remote: Option<&str>) -> std::io::Result<ExitStatus> {
        crate::tty::release_terminal();
        self.command(remote)
            .args(["exec", "-it", id, &shell_path()])
            .status()
    }

    pub fn logs(
        &self,
        id: &str,
        tail: u32,
        follow: bool,
        remote: Option<&str>,
    ) -> std::io::Result<ExitStatus> {
        crate::tty::release_terminal();
        let mut cmd = self.command(remote);
        cmd.arg("logs").arg("--tail").arg(tail.to_string());
        if follow {
            cmd.arg("--follow");
        }
        cmd.arg(id);
        cmd.status()
    }

    /// `<bin> start|stop|restart <id>`, with stop/restart bounded to a 5s
    /// graceful window so the UI does not freeze for the engine's default 10s.
    pub fn lifecycle(&self, id: &str, action: LifecycleAction, remote: Option<&str>) -> Result<()> {
        let mut cmd = self.command(remote);
        cmd.arg(action.subcommand());
        if matches!(action, LifecycleAction::Stop | LifecycleAction::Restart) {
            cmd.args(["-t", "5"]);
        }
        cmd.arg(id).stdout(Stdio::null()).stderr(Stdio::piped());
        let out = cmd
            .output()
            .with_context(|| format!("running {} lifecycle command", self.bin))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(anyhow::anyhow!(
                "{}",
                if err.is_empty() {
                    "non-zero exit".to_string()
                } else {
                    err
                }
            ));
        }
        Ok(())
    }

    /// Raw `<bin> inspect <id>` JSON.
    pub fn inspect_raw(&self, id: &str, remote: Option<&str>) -> Result<String> {
        let out = self
            .command(remote)
            .args(["inspect", id])
            .stderr(Stdio::null())
            .output()
            .with_context(|| format!("running `{} inspect`", self.bin))?;
        if !out.status.success() {
            return Err(anyhow::anyhow!(
                "{} inspect exited {}",
                self.bin,
                out.status
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Best-effort recent log lines, merged from stdout and stderr — the
    /// engine sends container stdout on ours and stderr on ours, and a tail
    /// that showed only one half would be misleading.
    pub fn log_tail(&self, id: &str, lines: u32, remote: Option<&str>) -> Vec<String> {
        let Ok(o) = self
            .command(remote)
            .args(["logs", "--tail", &lines.to_string(), id])
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
        else {
            return Vec::new();
        };
        let mut out: Vec<String> = String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(String::from)
            .collect();
        out.extend(String::from_utf8_lossy(&o.stderr).lines().map(String::from));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_engines_are_distinct_in_every_field() {
        assert_ne!(DOCKER.bin, PODMAN.bin);
        assert_ne!(
            DOCKER.host_env, PODMAN.host_env,
            "a shared env var would cross the wires"
        );
        assert_ne!(DOCKER.label, PODMAN.label);
    }

    #[test]
    fn podman_uses_its_own_remote_variable() {
        // `DOCKER_HOST` would be wrong: podman reads `CONTAINER_HOST`, and
        // pointing podman at a Docker socket is not the same thing.
        assert_eq!(PODMAN.host_env, "CONTAINER_HOST");
        assert_eq!(DOCKER.host_env, "DOCKER_HOST");
    }

    #[test]
    fn an_absent_engine_reports_as_not_running() {
        let missing = ContainerCli {
            bin: "definitely-not-a-container-engine",
            host_env: "NOPE",
            label: "None",
        };
        assert!(!missing.daemon_running());
        // And listing against it is an empty result, not an error: the worker
        // polls on a loop and a missing engine is not news.
        assert!(missing.list_containers_raw(None).unwrap().is_none());
    }

    #[test]
    fn each_engine_caches_separately() {
        // One shared cache entry would let docker's answer stand in for
        // podman's on a machine that has only one of them.
        let a = ContainerCli {
            bin: "engine-test-a",
            host_env: "A",
            label: "A",
        };
        let b = ContainerCli {
            bin: "engine-test-b",
            host_env: "B",
            label: "B",
        };
        a.daemon_running();
        b.daemon_running();
        if let Ok(g) = DAEMON_CACHE.lock() {
            let map = g.as_ref().expect("cache initialised");
            assert!(map.contains_key("engine-test-a"));
            assert!(map.contains_key("engine-test-b"));
        }
        a.invalidate_cache();
        if let Ok(g) = DAEMON_CACHE.lock() {
            let map = g.as_ref().unwrap();
            assert!(
                !map.contains_key("engine-test-a"),
                "invalidating one must not keep it"
            );
            assert!(
                map.contains_key("engine-test-b"),
                "…and must not drop the other"
            );
        }
        b.invalidate_cache();
    }
}
