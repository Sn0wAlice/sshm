//! A very small `git` wrapper: just the handful of plumbing calls the sync
//! engine needs, run against a private clone in `~/.config/sshm/sync-repo`.
//!
//! We shell out to the user's own `git` rather than linking a git library so
//! that their `~/.ssh/config`, agent, signing setup and proxy settings all keep
//! working. Authentication is pinned to the configured key through
//! `GIT_SSH_COMMAND`, and every prompt is disabled: a sync that would block
//! asking for a passphrase must fail fast instead of hanging a background
//! worker forever.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};

use crate::config::settings::SyncConfig;

/// A git working copy plus the environment used to talk to its remote.
pub struct Git {
    dir: PathBuf,
    ssh_command: String,
}

/// Quote one argument for the `GIT_SSH_COMMAND` string, which git hands to a
/// shell. Paths with spaces are common enough on macOS to matter.
fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "-_./@:=~".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Build the `ssh` invocation git will use.
pub fn ssh_command_for(cfg: &SyncConfig) -> String {
    let mut cmd = String::from("ssh");
    if let Some(key) = cfg.expanded_key() {
        // `IdentitiesOnly` stops ssh-agent from offering a different key first
        // and burning the server's auth attempts before ours is tried.
        cmd.push_str(&format!(" -i {} -o IdentitiesOnly=yes", shell_quote(&key)));
    }
    let host_keys = if cfg.strict_host_key_checking { "yes" } else { "accept-new" };
    cmd.push_str(&format!(" -o StrictHostKeyChecking={host_keys}"));
    // No password/passphrase prompts, and don't sit on a dead network.
    cmd.push_str(" -o BatchMode=yes -o ConnectTimeout=10");
    cmd
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>, cfg: &SyncConfig) -> Self {
        Git { dir: dir.into(), ssh_command: ssh_command_for(cfg) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// True when a usable `git` is on PATH.
    pub fn is_available() -> bool {
        Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Run a git command in the working copy, returning trimmed stdout.
    /// A non-zero exit becomes an error carrying git's own stderr.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let out = self.raw(args)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let detail = if stderr.trim().is_empty() { stdout } else { stderr };
            bail!("git {}: {}", args.join(" "), detail.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Run a git command, tolerating failure — the caller inspects the status.
    pub fn raw(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            // Identity for the commits we create. Set per-invocation so we
            // never depend on (or touch) the user's global git config.
            .args(["-c", "user.name=sshm", "-c", "user.email=sshm@localhost"])
            .args(args)
            .env("GIT_SSH_COMMAND", &self.ssh_command)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .env("SSH_ASKPASS", "")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running `git {}`", args.join(" ")))
    }

    /// True when `dir` already holds a git repository.
    pub fn is_repo(&self) -> bool {
        self.dir.join(".git").exists()
    }

    /// Create the working copy if needed and point `origin` at `url`.
    ///
    /// We `init` + `remote add` rather than `clone` so that an empty, freshly
    /// created remote (the common case — the user just made the repo) works
    /// exactly like a populated one.
    pub fn ensure_repo(&self, url: &str, branch: &str) -> Result<()> {
        if url.trim().is_empty() {
            return Err(anyhow!("no sync repository configured"));
        }
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;

        if !self.is_repo() {
            // `init -b` needs git >= 2.28; fall back to pointing HEAD at the
            // branch by hand on older ones.
            if self.run(&["init", "-q", "-b", branch]).is_err() {
                self.run(&["init", "-q"]).context("initializing the sync working copy")?;
                let head = format!("refs/heads/{branch}");
                self.run(&["symbolic-ref", "HEAD", &head])?;
            }
        }

        // Point origin at the configured URL (idempotent, and picks up a URL
        // the user changed in Settings since the last run).
        let has_origin = self
            .raw(&["remote", "get-url", "origin"])
            .map(|o| o.status.success())
            .unwrap_or(false);
        if has_origin {
            self.run(&["remote", "set-url", "origin", url])?;
        } else {
            self.run(&["remote", "add", "origin", url])?;
        }
        Ok(())
    }

    /// Fetch `branch` from origin. Returns the fetched commit, or `None` when
    /// the branch doesn't exist upstream yet (a brand-new empty repo).
    pub fn fetch(&self, branch: &str) -> Result<Option<String>> {
        let refspec = format!("refs/heads/{branch}");
        let out = self.raw(&["fetch", "--quiet", "origin", &refspec])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // An empty remote (or one without our branch) is a normal first
            // run, not a failure — everything else is a real error.
            if stderr.contains("couldn't find remote ref")
                || stderr.contains("Couldn't find remote ref")
            {
                return Ok(None);
            }
            bail!("git fetch: {}", stderr.trim());
        }
        Ok(self.rev_parse("FETCH_HEAD"))
    }

    /// Resolve a revision to a commit id, or `None` when it doesn't exist.
    pub fn rev_parse(&self, rev: &str) -> Option<String> {
        let out = self.raw(&["rev-parse", "--verify", "--quiet", rev]).ok()?;
        if !out.status.success() {
            return None;
        }
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if id.is_empty() { None } else { Some(id) }
    }

    /// Contents of `file` at `rev`, or `None` when either is absent.
    pub fn show_file(&self, rev: &str, file: &str) -> Option<String> {
        let spec = format!("{rev}:{file}");
        let out = self.raw(&["show", &spec]).ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Move the working copy onto `rev`, discarding anything local. Safe
    /// because the engine keeps every local edit in memory and rewrites it
    /// straight after — the working copy is scratch space, never the truth.
    pub fn reset_hard(&self, rev: &str) -> Result<()> {
        self.run(&["reset", "--hard", "--quiet", rev]).map(|_| ())
    }

    /// Move a ref to a commit. Used to remember the last state this machine
    /// agreed on with the remote (`refs/sshm/base`), which is the ancestor a
    /// three-way merge needs. A ref — rather than a sha in a state file —
    /// keeps the commit alive through `git gc`.
    pub fn update_ref(&self, name: &str, rev: &str) -> Result<()> {
        self.run(&["update-ref", name, rev]).map(|_| ())
    }

    /// Stage everything and commit. Returns `false` when the tree was clean
    /// (nothing to commit — a pure no-op sync).
    pub fn commit_all(&self, message: &str) -> Result<bool> {
        self.run(&["add", "-A"])?;
        let staged = self.raw(&["diff", "--cached", "--quiet"])?;
        // `diff --quiet` exits 1 exactly when there is something staged.
        if staged.status.success() {
            return Ok(false);
        }
        self.run(&["commit", "--quiet", "-m", message])?;
        Ok(true)
    }

    /// Push the current commit to `branch` on origin.
    ///
    /// Never forced: if somebody pushed in between, the rejection bubbles up
    /// and the engine retries the whole fetch/merge cycle.
    pub fn push(&self, branch: &str) -> Result<()> {
        let refspec = format!("HEAD:refs/heads/{branch}");
        self.run(&["push", "--quiet", "origin", &refspec]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(key: &str, strict: bool) -> SyncConfig {
        SyncConfig {
            ssh_key: key.to_string(),
            strict_host_key_checking: strict,
            ..SyncConfig::default()
        }
    }

    #[test]
    fn ssh_command_pins_the_configured_key() {
        let c = ssh_command_for(&cfg("/home/me/.ssh/id_ed25519", false));
        assert!(c.contains("-i /home/me/.ssh/id_ed25519"));
        assert!(c.contains("IdentitiesOnly=yes"));
        assert!(c.contains("StrictHostKeyChecking=accept-new"));
        assert!(c.contains("BatchMode=yes"));
    }

    #[test]
    fn ssh_command_quotes_paths_with_spaces() {
        let c = ssh_command_for(&cfg("/Users/me/My Keys/id_ed25519", false));
        assert!(c.contains("-i '/Users/me/My Keys/id_ed25519'"), "got: {c}");
    }

    #[test]
    fn ssh_command_without_a_key_offers_no_identity_flag() {
        let c = ssh_command_for(&cfg("  ", false));
        assert!(!c.contains(" -i "));
        assert!(c.contains("BatchMode=yes"));
    }

    #[test]
    fn strict_host_key_checking_is_opt_in() {
        assert!(ssh_command_for(&cfg("", true)).contains("StrictHostKeyChecking=yes"));
    }

    #[test]
    fn quoting_leaves_plain_paths_alone() {
        assert_eq!(shell_quote("~/.ssh/id_ed25519"), "~/.ssh/id_ed25519");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
