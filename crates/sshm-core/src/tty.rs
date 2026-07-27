//! Terminal handover hook.
//!
//! Core builds and executes the foreground `ssh` / `docker` / `kubectl` /
//! `incus` commands, but restoring the terminal beforehand (leaving raw mode,
//! showing the cursor, etc.) is a frontend concern — and the tools that do it
//! (crossterm) must not leak into core's dependency tree.
//!
//! So a frontend registers a callback once at startup with
//! [`set_release_hook`]; core invokes it via [`release_terminal`] immediately
//! before handing the TTY to a child process. When no hook is registered (e.g.
//! a plain CLI invocation that was never in raw mode) [`release_terminal`] is a
//! no-op, which matches the previous inline behavior — `disable_raw_mode()` on
//! a cooked terminal did nothing either.

use std::sync::RwLock;

type ReleaseHook = Box<dyn Fn() + Send + Sync + 'static>;

static HOOK: RwLock<Option<ReleaseHook>> = RwLock::new(None);

/// Register the terminal-release callback. Called once by the frontend at
/// startup. A later call replaces any previously registered hook.
pub fn set_release_hook<F>(f: F)
where
    F: Fn() + Send + Sync + 'static,
{
    if let Ok(mut guard) = HOOK.write() {
        *guard = Some(Box::new(f));
    }
}

/// Restore the terminal via the registered hook, if any. Safe to call from any
/// frontend (or none): a no-op when no hook has been set.
pub fn release_terminal() {
    if let Ok(guard) = HOOK.read() {
        if let Some(hook) = guard.as_ref() {
            hook();
        }
    }
}
