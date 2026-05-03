//! Thin wrappers around the `kubectl` CLI.

use std::io::stdout;
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result};
use crossterm::{cursor::Show, execute, terminal::disable_raw_mode};

use super::models::{Cluster, PodInfo};
use super::shell::SHELL_PATH;

/// Returns true if `kubectl` is on PATH.
pub fn cli_available() -> bool {
    which("kubectl")
}

fn which(bin: &str) -> bool {
    Command::new(bin)
        .arg("version")
        .arg("--client=true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Build a `kubectl` command pre-loaded with the saved cluster's
/// `--kubeconfig` and `--context` flags. Args are appended by the caller.
pub fn base_cmd(cluster: &Cluster) -> Command {
    let mut cmd = Command::new("kubectl");
    if let Some(ref kc) = cluster.kubeconfig {
        let expanded = shellexpand::tilde(kc);
        cmd.arg("--kubeconfig").arg(expanded.as_ref());
    }
    if let Some(ref ctx) = cluster.context {
        cmd.arg("--context").arg(ctx);
    }
    cmd
}

/// `kubectl get pods --all-namespaces` parsed into [`PodInfo`].
pub fn list_pods(cluster: &Cluster) -> Result<Vec<PodInfo>> {
    if !cli_available() {
        return Ok(Vec::new());
    }
    let mut cmd = base_cmd(cluster);
    // jsonpath: namespace,name,phase,each container's name (space-joined)
    cmd.args([
        "get", "pods",
        "--all-namespaces",
        "-o", "jsonpath={range .items[*]}{.metadata.namespace}{\"\\t\"}{.metadata.name}{\"\\t\"}{.status.phase}{\"\\t\"}{range .spec.containers[*]}{.name}{\" \"}{end}{\"\\n\"}{end}",
    ]);
    cmd.stderr(Stdio::null());
    let out = cmd.output().context("running `kubectl get pods`")?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Ok(parse_pods_jsonpath(&raw))
}

/// Pure parser for the tab-separated jsonpath output emitted by [`list_pods`].
pub fn parse_pods_jsonpath(raw: &str) -> Vec<PodInfo> {
    raw.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 4 { return None; }
            let containers: Vec<String> = parts[3]
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            Some(PodInfo {
                namespace: parts[0].trim().to_string(),
                name: parts[1].trim().to_string(),
                phase: parts[2].trim().to_string(),
                containers,
            })
        })
        .filter(|p| !p.name.is_empty())
        .collect()
}

/// `kubectl exec -it -n NS POD [-c CONTAINER] -- /bin/sh`.
pub fn exec_shell(
    cluster: &Cluster,
    namespace: &str,
    pod: &str,
    container: Option<&str>,
) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut cmd = base_cmd(cluster);
    cmd.args(["exec", "-it", "-n", namespace, pod]);
    if let Some(c) = container {
        cmd.args(["-c", c]);
    }
    cmd.args(["--", SHELL_PATH]);
    cmd.status()
}

/// `kubectl delete pod -n NS POD [--wait=false]`.
///
/// Used to clean up terminated (Succeeded / Failed) pods left behind by Jobs
/// or CronJobs. We pass `--wait=false` so the command returns immediately
/// once the deletion has been requested — the caller refreshes the list.
pub fn delete_pod(
    cluster: &Cluster,
    namespace: &str,
    pod: &str,
) -> std::io::Result<std::process::Output> {
    let mut cmd = base_cmd(cluster);
    cmd.args(["delete", "pod", "-n", namespace, pod, "--wait=false"]);
    cmd.output()
}

/// `kubectl logs -n NS POD [-c CONTAINER] [--tail N] [-f]`.
pub fn logs(
    cluster: &Cluster,
    namespace: &str,
    pod: &str,
    container: Option<&str>,
    tail: u32,
    follow: bool,
) -> std::io::Result<ExitStatus> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), Show);
    let mut cmd = base_cmd(cluster);
    cmd.args(["logs", "-n", namespace, pod, "--tail", &tail.to_string()]);
    if let Some(c) = container {
        cmd.args(["-c", c]);
    }
    if follow {
        cmd.arg("-f");
    }
    cmd.status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kluster::models::ClusterKind;

    #[test]
    fn parse_two_pods() {
        let raw = "default\tnginx-abc\tRunning\tnginx \n\
                   kube-system\tcoredns-xyz\tRunning\tcoredns \n";
        let pods = parse_pods_jsonpath(raw);
        assert_eq!(pods.len(), 2);
        assert_eq!(pods[0].namespace, "default");
        assert_eq!(pods[0].name, "nginx-abc");
        assert_eq!(pods[0].containers, vec!["nginx".to_string()]);
        assert_eq!(pods[1].namespace, "kube-system");
    }

    #[test]
    fn parse_multi_container() {
        let raw = "default\tapp-1\tRunning\tapi sidecar metrics \n";
        let pods = parse_pods_jsonpath(raw);
        assert_eq!(pods[0].containers, vec!["api", "sidecar", "metrics"]);
    }

    #[test]
    fn base_cmd_skips_unset_flags() {
        let cluster = Cluster {
            name: "test".into(),
            kind: ClusterKind::K8s,
            kubeconfig: None,
            context: None,
            namespace_default: None,
        };
        let cmd = base_cmd(&cluster);
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert!(args.is_empty(), "expected no args when both unset, got {args:?}");
    }

    #[test]
    fn base_cmd_includes_set_flags() {
        let cluster = Cluster {
            name: "test".into(),
            kind: ClusterKind::K3s,
            kubeconfig: Some("/tmp/kc".into()),
            context: Some("homelab".into()),
            namespace_default: None,
        };
        let cmd = base_cmd(&cluster);
        let args: Vec<String> = cmd.get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--kubeconfig", "/tmp/kc", "--context", "homelab"]);
    }
}
