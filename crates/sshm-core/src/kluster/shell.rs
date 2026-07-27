//! Shell command used for `exec` into containers.
//!
//! We deliberately go through `/bin/sh` directly: every Linux container
//! image we care about ships it, the bash-fallback wrapper caused too many
//! corner cases (distroless, non-interactive bash configs, weird exit
//! handling). If a user needs `bash`, they can launch it from the `sh`
//! prompt manually.
pub const SHELL_PATH: &str = "/bin/sh";
