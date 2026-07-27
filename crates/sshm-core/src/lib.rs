//! `sshm-core` — the frontend-agnostic engine behind sshm.
//!
//! This crate holds everything that has no rendering or event-loop concern:
//! the data model, config/database IO, ssh & kluster command construction,
//! the fuzzy/prefix filter, connection history (frecency), and the i18n string
//! catalog. Both the terminal UI (`sshm`) and future frontends depend on it and
//! share the exact same on-disk database.
//!
//! Hard rule: no terminal-UI dependency (ratatui / crossterm / inquire) may
//! appear in this crate's dependency tree. When a frontend needs the terminal
//! restored before a child process takes over the TTY, it registers a callback
//! via [`tty::set_release_hook`]; core calls it through [`tty::release_terminal`].
pub mod models;
pub mod os;
pub mod tty;

pub mod config;
pub mod ssh;
pub mod import;
pub mod filter;
pub mod history;
pub mod i18n;
pub mod kluster;

// Convenience re-exports
pub use config::{io as cfg_io, path as cfg_path};
