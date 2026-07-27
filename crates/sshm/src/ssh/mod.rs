//! Frontend SSH facade.
//!
//! The pure command builders and key/agent/known_hosts helpers live in
//! `sshm_core::ssh`. This module re-exports them so `crate::ssh::keys`,
//! `crate::ssh::proxy`, … keep resolving, and adds the pieces that need an
//! interactive terminal prompt (`inquire`) and therefore can't live in core:
//! the connection entry point with trust-on-first-use + host-key-change
//! recovery ([`client`]), and the `add-identity` command ([`add_identity`]).

pub mod client;
pub mod add_identity;

// Pure, frontend-agnostic helpers — surfaced at their historical paths.
pub use sshm_core::ssh::{agent, keys, known_hosts, proxy};
