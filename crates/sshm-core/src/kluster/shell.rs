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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

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
