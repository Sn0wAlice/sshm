//! SSH command construction and key/agent/known_hosts helpers.
//!
//! Everything here is frontend-agnostic: it builds argv vectors and spawns
//! foreground `ssh` processes (via the TTY-release hook), and manipulates the
//! user's keys, ssh-agent and `known_hosts`. The interactive pieces that used
//! to live alongside this (the trust-on-first-use prompt, the host-key-change
//! recovery flow, and the `add-identity` command) moved to the frontend crate
//! because they drive an interactive terminal prompt (`inquire`).
pub mod client;
pub mod keys;
pub mod agent;
pub mod known_hosts;
pub mod proxy;
