//! Thin best-effort wrappers around the host OS (Linux + macOS only):
//! desktop notifications, opening URLs in the browser, and launching a
//! command in a new terminal window. Every function silently no-ops or
//! returns an error string on failure — nothing here is load-bearing.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Master switch for [`notify`], mirrored from `AppConfig.notifications_enabled`.
static NOTIFY_ENABLED: AtomicBool = AtomicBool::new(true);

/// Optional custom notification icon path (mirrored from `AppConfig`).
static NOTIFY_ICON: Mutex<String> = Mutex::new(String::new());

/// Enable or disable desktop notifications process-wide. Call on startup and
/// whenever the setting changes.
pub fn set_notifications_enabled(on: bool) {
    NOTIFY_ENABLED.store(on, Ordering::Relaxed);
}

/// Set the custom notification icon path (tilde-expanded). Empty = OS default.
pub fn set_notification_icon(path: &str) {
    if let Ok(mut g) = NOTIFY_ICON.lock() {
        *g = shellexpand::tilde(path.trim()).to_string();
    }
}

fn notify_icon() -> String {
    NOTIFY_ICON.lock().map(|g| g.clone()).unwrap_or_default()
}

/// Quote a string as an AppleScript string literal.
fn applescript_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

/// POSIX-shell-quote a single argument.
fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "-_./@:=".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

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

/// Fire a fire-and-forget desktop notification. macOS uses `osascript`,
/// Linux `notify-send`; a missing tool just means no notification.
pub fn notify(title: &str, body: &str) {
    if !NOTIFY_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    notify_unconditional(title, body);
}

/// Fire a one-off confirmation notification, *bypassing* the enabled gate —
/// used as immediate "it works" feedback when the user switches notifications
/// on in Settings (before the setting is even saved).
pub fn notify_test() {
    let who = std::env::var("USER").unwrap_or_else(|_| "sshm".to_string());
    notify_unconditional("SSHM", &format!("{} says hello from the other side", who));
}

/// Emit a desktop notification regardless of the enabled gate.
///
/// Custom icon: on Linux `notify-send -i` takes any icon path/name. On macOS
/// `osascript`'s `display notification` *cannot* set an icon (it's always the
/// caller's — i.e. osascript's) — so a custom icon there needs
/// `terminal-notifier` installed; we use it automatically when present.
fn notify_unconditional(title: &str, body: &str) {
    let icon = notify_icon();
    let mut cmd = if cfg!(target_os = "macos") {
        if !icon.is_empty() && bin_exists("terminal-notifier") {
            let mut c = Command::new("terminal-notifier");
            c.arg("-title")
                .arg(title)
                .arg("-message")
                .arg(body)
                .arg("-appIcon")
                .arg(&icon);
            c
        } else {
            let script = format!(
                "display notification {} with title {}",
                applescript_quote(body),
                applescript_quote(title),
            );
            let mut c = Command::new("osascript");
            c.arg("-e").arg(script);
            c
        }
    } else {
        let mut c = Command::new("notify-send");
        if !icon.is_empty() {
            c.arg("-i").arg(&icon);
        }
        c.arg(title).arg(body);
        c
    };
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Copy `text` to the system clipboard. macOS uses `pbcopy`; Linux tries
/// `wl-copy` (Wayland), then `xclip`, then `xsel` (X11). Returns an error
/// string when no clipboard tool is available or the write fails.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;

    // (binary, args) candidates, in priority order per platform.
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };

    let mut last_err = "no clipboard tool found (install xclip, xsel or wl-clipboard)".to_string();
    for (bin, args) in candidates {
        if !bin_exists(bin) {
            continue;
        }
        match Command::new(bin)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(e) = stdin.write_all(text.as_bytes()) {
                        last_err = format!("{bin}: {e}");
                        continue;
                    }
                }
                // Drop stdin (via wait) so the tool sees EOF and exits.
                return match child.wait() {
                    Ok(status) if status.success() => Ok(()),
                    Ok(status) => Err(format!("{bin} exited with {status}")),
                    Err(e) => Err(format!("{bin}: {e}")),
                };
            }
            Err(e) => last_err = format!("{bin}: {e}"),
        }
    }
    Err(last_err)
}

/// Open `url` (or a path) with the system handler — `open` on macOS,
/// `xdg-open` on Linux.
pub fn open_url(url: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    Command::new(opener)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{opener}: {e}"))
}

/// Auto-detect a terminal emulator, returned as a command prefix (the target
/// command's argv is appended to it). `None` when nothing known is found.
fn detect_terminal() -> Option<Vec<String>> {
    // An explicit $TERMINAL wins.
    if let Ok(t) = std::env::var("TERMINAL") {
        let t = t.trim();
        if !t.is_empty() && bin_exists(t) {
            return Some(vec![t.to_string(), "-e".to_string()]);
        }
    }
    // Known emulators (Linux + macOS), "prefix then argv" invocation style.
    let candidates: &[(&str, &[&str])] = &[
        ("wezterm", &["start", "--"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xterm", &["-e"]),
    ];
    for (bin, args) in candidates {
        if bin_exists(bin) {
            let mut v = vec![bin.to_string()];
            v.extend(args.iter().map(|s| s.to_string()));
            return Some(v);
        }
    }
    None
}

/// macOS fallback: ask Terminal.app to run the command in a new window.
fn macos_terminal(argv: &[String]) -> Result<(), String> {
    let cmd_str = argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
    let script = format!(
        "tell application \"Terminal\" to do script {}",
        applescript_quote(&cmd_str),
    );
    Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("osascript: {e}"))
}

/// Launch `argv` in a new terminal window.
///
/// `terminal_override` is the user's `external_terminal` setting: when set,
/// it's split on whitespace and used as the command prefix. When empty, a
/// terminal is auto-detected (with an osascript/Terminal.app fallback on macOS).
pub fn open_in_terminal(argv: &[String], terminal_override: &str) -> Result<(), String> {
    if argv.is_empty() {
        return Err("nothing to run".to_string());
    }

    let prefix: Vec<String> = if !terminal_override.trim().is_empty() {
        terminal_override.split_whitespace().map(String::from).collect()
    } else {
        match detect_terminal() {
            Some(p) => p,
            None => {
                if cfg!(target_os = "macos") {
                    return macos_terminal(argv);
                }
                return Err(
                    "no terminal emulator found — set external_terminal in settings.toml"
                        .to_string(),
                );
            }
        }
    };

    Command::new(&prefix[0])
        .args(&prefix[1..])
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", prefix[0]))
}
