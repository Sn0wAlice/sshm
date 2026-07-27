//! Library root for the `sshm` terminal UI + CLI frontend.
//!
//! The engine lives in [`sshm_core`]; this crate is the ratatui/crossterm TUI,
//! the CLI dispatch, and the interactive command flows. Core modules are
//! re-exported at this crate's root so existing `crate::models`,
//! `crate::kluster`, `crate::config`, … paths keep resolving unchanged after
//! the workspace split.

// --- Engine, re-exported so `crate::<module>` keeps working across the TUI ---
pub use sshm_core::{
    config, filter, history, i18n, import, kluster, models, os, tty,
};
pub use sshm_core::{cfg_io, cfg_path};
// Macro re-export: keeps `use crate::t;` working throughout the TUI.
pub use sshm_core::t;

// --- Frontend-only modules ---
pub mod util;
pub mod ssh;
pub mod tui;
pub mod commands;

// Convenience re-exports (matching the pre-split layout).
pub use commands::{connect, crud, list, tags};
