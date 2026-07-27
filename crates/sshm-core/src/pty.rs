//! A minimal, frontend-agnostic pseudo-terminal session.
//!
//! Wraps `portable-pty` so a frontend that embeds a terminal (the desktop GUI)
//! can spawn the system `ssh` (or any argv) inside a real PTY, stream its
//! output, feed it input, resize it, and kill it — without knowing anything
//! about how the bytes are transported to the UI. The TUI never uses this (it
//! *is* the terminal), so the whole module — and `portable-pty` — sits behind
//! the off-by-default `pty` feature.

use std::io::{Read, Write};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

/// One live PTY child (e.g. an `ssh` process) plus its master side.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
}

impl PtySession {
    /// Spawn `argv` (argv[0] is the program) inside a fresh PTY of `cols`×`rows`.
    /// The child's cwd is the user's home directory.
    pub fn spawn(argv: &[String], cols: u16, rows: u16) -> Result<Self> {
        let program = argv.first().context("empty argv")?;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(&argv[1..]);
        if let Some(home) = dirs::home_dir() {
            cmd.cwd(home);
        }

        let child = pair.slave.spawn_command(cmd).context("spawn in pty")?;
        // Drop the slave handle so the child owns the only slave fd; when it
        // exits, the master read side sees EOF.
        drop(pair.slave);

        let writer = pair.master.take_writer().context("take pty writer")?;
        Ok(Self { master: pair.master, child, writer })
    }

    /// An independent reader for the child's output. Spawn a thread that reads
    /// from this and forwards bytes to the UI; it ends (EOF) when the session is
    /// killed or the child exits.
    pub fn output_reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader().context("clone pty reader")
    }

    /// Write keystrokes/input to the child.
    pub fn write_input(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    /// Resize the PTY (call when the terminal widget resizes).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize pty")
    }

    /// Kill the child process (SIGKILL). Idempotent-ish: a dead child errors
    /// harmlessly.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
