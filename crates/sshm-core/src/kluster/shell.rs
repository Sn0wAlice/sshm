//! Shell command used for `exec` into containers.
//!
//! We deliberately go through `/bin/sh` directly rather than probing for a
//! nicer shell: every Linux container image we care about ships it, and the
//! bash-fallback wrapper we used to have caused too many corner cases
//! (distroless, non-interactive bash configs, weird exit handling). If a user
//! needs `bash`, they can launch it from the `sh` prompt manually.
//!
//! What the fixed path could not answer is the image that has no `/bin/sh` at
//! all — a distroless or busybox-at-another-path build. So the default stays
//! `/bin/sh` and stays un-probed, but it is now *overridable*: set
//! `kluster_shell` in `settings.toml` and every exec uses that instead. One
//! declared value, no runtime guessing.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Bound for a background CLI query (list / inspect / probe). The kluster
/// worker polls on a loop; a remote daemon behind a dead SSH link or a stuck
/// local socket must not wedge it forever.
pub(crate) const QUERY_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound for start/stop/restart, which legitimately take a few seconds.
pub(crate) const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(30);

/// `cmd.output()` with a deadline: the child is killed once `timeout` passes
/// and a `TimedOut` error is returned. stdin is closed (nothing here is
/// interactive) and both output streams are captured — drained on their own
/// threads so a chatty child can't fill a pipe and stall before the deadline.
///
/// On unix the child leads its own process group and the whole group is
/// killed, so helpers it spawned (docker's `ssh://` transport, a kubectl
/// credential plugin) die with it. The deadline also covers the readers: a
/// helper that escaped the group and still holds a pipe can't block the
/// caller — its reader thread is detached instead of joined.
///
/// ponytail: polls every 50ms.
pub(crate) fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    }
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let timed_out = || {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timed out after {}s", timeout.as_secs()),
        )
    };
    let kill = |child: &mut std::process::Child| {
        #[cfg(unix)]
        // SAFETY: plain syscall; the negative pid targets the group the child
        // leads (pgid == its pid via `process_group(0)`).
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    };
    let deadline = Instant::now() + timeout;
    let mut status = None;
    while status.is_none() || !(out.is_finished() && err.is_finished()) {
        if status.is_none() {
            status = child.try_wait()?;
        }
        if Instant::now() >= deadline {
            // Exited or not, something still holds the pipes: kill the group
            // and give the readers a moment to see EOF, else detach them.
            kill(&mut child);
            let grace = Instant::now() + Duration::from_millis(500);
            while !(out.is_finished() && err.is_finished()) && Instant::now() < grace {
                std::thread::sleep(Duration::from_millis(20));
            }
            if status.is_none() || !(out.is_finished() && err.is_finished()) {
                return Err(timed_out());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(Output {
        status: status.expect("loop exits only with a status"),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// The shell used when nothing overrides it.
pub const DEFAULT_SHELL_PATH: &str = "/bin/sh";

/// Process-wide override, mirrored from `AppConfig.kluster_shell`.
static OVERRIDE: RwLock<String> = RwLock::new(String::new());
/// Cheap "is there an override at all?" gate, so the common path never takes
/// the lock — `exec_shell` sits on the hot path of every container open.
static HAS_OVERRIDE: AtomicBool = AtomicBool::new(false);

/// Set (or clear, with an empty string) the shell used for every `exec`.
/// Called on startup and whenever the setting changes.
pub fn set_shell_path(path: &str) {
    let path = path.trim();
    if let Ok(mut guard) = OVERRIDE.write() {
        guard.clear();
        guard.push_str(path);
        HAS_OVERRIDE.store(!path.is_empty(), Ordering::Relaxed);
    }
}

/// The shell to exec into a container with.
pub fn shell_path() -> String {
    if HAS_OVERRIDE.load(Ordering::Relaxed) {
        if let Ok(guard) = OVERRIDE.read() {
            if !guard.is_empty() {
                return guard.clone();
            }
        }
    }
    DEFAULT_SHELL_PATH.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    // These share one process-wide static, so they run under a mutex rather
    // than racing each other across the test threads.
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn output_with_timeout_captures_and_kills() {
        let o = output_with_timeout(
            Command::new("sh").args(["-c", "echo out; echo err >&2"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(o.status.success());
        assert_eq!(o.stdout, b"out\n");
        assert_eq!(o.stderr, b"err\n");

        let start = Instant::now();
        let e = output_with_timeout(
            Command::new("sh").args(["-c", "exec sleep 5"]),
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_grandchild_holding_the_pipe_cannot_outlive_the_deadline() {
        // The direct child exits at once, but its background grandchild keeps
        // stdout open — the old code then blocked joining the reader.
        let start = Instant::now();
        let r = output_with_timeout(
            Command::new("sh").args(["-c", "sleep 5 & echo hi"]),
            Duration::from_millis(300),
        );
        assert!(start.elapsed() < Duration::from_secs(3), "{r:?}");
    }

    #[test]
    fn default_is_bin_sh() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set_shell_path("");
        assert_eq!(shell_path(), "/bin/sh");
    }

    #[test]
    fn an_override_replaces_the_default() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set_shell_path("/busybox/sh");
        assert_eq!(shell_path(), "/busybox/sh");
        set_shell_path("");
    }

    #[test]
    fn a_blank_override_falls_back_rather_than_execing_nothing() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set_shell_path("/bin/bash");
        set_shell_path("   ");
        assert_eq!(shell_path(), "/bin/sh");
    }

    #[test]
    fn an_override_is_trimmed() {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set_shell_path("  /bin/ash  ");
        assert_eq!(shell_path(), "/bin/ash");
        set_shell_path("");
    }
}
