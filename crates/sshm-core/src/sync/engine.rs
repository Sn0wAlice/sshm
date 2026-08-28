//! The sync run itself: fetch, three-way merge, write back, commit, push.
//!
//! The working copy in `~/.config/sshm/sync-repo` is scratch space, never the
//! source of truth — the real files stay where they always were. A run reads
//! the local copies into memory, reconciles them against the remote, writes the
//! result to both places, and commits. That means a corrupted or deleted
//! working copy costs nothing: the next run rebuilds it.
//!
//! Git's own merge machinery is never invoked. We always fast-forward the
//! working copy to the remote tip and commit our merged result on top, so a
//! sync can't leave conflict markers or a half-merged index behind.

use anyhow::{bail, Context, Result};

use crate::config::io::atomic_write;
use crate::config::settings::{AppConfig, ConflictPolicy, SyncConfig, SyncItem};

use super::git::Git;
use super::merge::{merge_item, MergeStats};

/// Ref holding the last state this machine agreed on with the remote — the
/// base of the three-way merge.
const BASE_REF: &str = "refs/sshm/base";

/// How many times a push rejected by a concurrent writer is retried (from a
/// fresh fetch each time) before giving up.
const PUSH_ATTEMPTS: usize = 3;

/// Which way a run is allowed to move data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Merge both ways and publish the result. The normal `sshm sync`.
    #[default]
    Both,
    /// Apply the remote locally, publish nothing. Collisions resolve to the
    /// remote version.
    Pull,
    /// Publish local state. Collisions resolve to this machine's version.
    Push,
}

/// What one sync run did.
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub stats: MergeStats,
    /// Local files rewritten by this run.
    pub updated_locally: Vec<String>,
    /// Commit created and pushed, if any.
    pub commit: Option<String>,
    /// True when a commit was pushed to the remote.
    pub pushed: bool,
}

impl SyncReport {
    /// One-line summary for a toast, `sshm sync` output, or the state file.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.updated_locally.is_empty() {
            parts.push(format!("{} updated locally", self.updated_locally.join(", ")));
        }
        if self.pushed {
            parts.push("pushed".to_string());
        }
        if self.stats.conflicts > 0 {
            parts.push(format!("{} conflict(s) resolved", self.stats.conflicts));
        }
        if parts.is_empty() {
            "already up to date".to_string()
        } else {
            parts.join(" · ")
        }
    }
}

/// Fail early on the mistakes that would otherwise surface as a cryptic git
/// error in a background thread.
pub fn preflight(cfg: &SyncConfig) -> Result<()> {
    if !cfg.is_configured() {
        bail!("no sync repository configured — run `sshm sync setup`");
    }
    if !Git::is_available() {
        bail!("`git` was not found on PATH — config sync needs it");
    }
    let url = cfg.repo_url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        bail!(
            "sync is SSH-key based: {url} is an HTTPS remote. \
             Use the SSH form, e.g. git@github.com:you/sshm-config.git"
        );
    }
    if let Some(key) = cfg.expanded_key() {
        if !std::path::Path::new(&key).exists() {
            bail!("ssh key {key} does not exist");
        }
    }
    Ok(())
}

/// Run one sync. The caller is responsible for holding the sync lock — use
/// [`super::sync_now`] or [`super::sync_if_due`] rather than calling this
/// directly.
pub fn run(cfg: &SyncConfig, direction: Direction) -> Result<SyncReport> {
    preflight(cfg)?;

    let branch = cfg.effective_branch();
    let items = cfg.effective_items();
    let policy = match direction {
        Direction::Both => cfg.conflict,
        Direction::Pull => ConflictPolicy::PreferRemote,
        Direction::Push => ConflictPolicy::PreferLocal,
    };

    let repo_dir = crate::config::path::sync_repo_dir();
    let git = Git::new(&repo_dir, cfg);
    git.ensure_repo(cfg.repo_url.trim(), &branch)?;

    let mut last_err = None;
    for attempt in 0..PUSH_ATTEMPTS {
        match attempt_sync(&git, cfg, &branch, &items, policy, direction) {
            Ok(report) => return Ok(report),
            Err(e) => {
                // A rejected push means somebody else pushed between our fetch
                // and our push: start over from their new tip. Anything else
                // is a real failure.
                if !is_push_race(&e) || attempt + 1 == PUSH_ATTEMPTS {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("sync failed")))
}

/// True when the error is git refusing a non-fast-forward push.
fn is_push_race(e: &anyhow::Error) -> bool {
    let text = format!("{e:#}");
    text.contains("non-fast-forward")
        || text.contains("fetch first")
        || text.contains("[rejected]")
}

fn attempt_sync(
    git: &Git,
    cfg: &SyncConfig,
    branch: &str,
    items: &[SyncItem],
    policy: ConflictPolicy,
    direction: Direction,
) -> Result<SyncReport> {
    let remote_commit = git.fetch(branch)?;
    let base_rev = git.rev_parse(BASE_REF);

    // Snapshot everything *before* touching the working copy: the reset below
    // wipes it, and the base can only be read from git.
    struct Pending {
        item: SyncItem,
        /// The local file exactly as it is on disk right now.
        local_disk: Option<String>,
        merged: Option<String>,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut stats = MergeStats::default();

    // The machine-local `[sync]` block, re-injected into any settings.toml we
    // write back. It never travels: a synced `[sync]` could point every other
    // machine at the wrong key, or switch sync off everywhere at once.
    let local_sync = cfg.clone();

    for &item in items {
        let file = item.file_name();
        let local_disk = std::fs::read_to_string(item.local_path()).ok();
        let local_repo = local_disk.as_deref().map(|t| to_repo(item, t));
        let base = base_rev.as_deref().and_then(|rev| git.show_file(rev, file));
        let remote = remote_commit.as_deref().and_then(|rev| git.show_file(rev, file));

        let outcome = merge_item(item, base.as_deref(), local_repo.as_deref(), remote.as_deref(), policy);
        stats.pulled += outcome.stats.pulled;
        stats.pushed += outcome.stats.pushed;
        stats.deleted += outcome.stats.deleted;
        stats.conflicts += outcome.stats.conflicts;

        pending.push(Pending { item, local_disk, merged: outcome.content });
    }

    let mut report = SyncReport { stats, ..SyncReport::default() };

    // --- Apply the merged result to the local config files ---
    // Every direction writes locally, push included: the merged state is what
    // the remote ends up holding, so leaving the local copy behind would make
    // the next run mistake it for a fresh local edit and undo the difference.
    for p in &pending {
        let Some(merged) = &p.merged else { continue };
        let want = to_local(p.item, merged, &local_sync);
        if p.local_disk.as_deref().map(str::trim_end) == Some(want.trim_end()) {
            continue;
        }
        atomic_write(&p.item.local_path(), want.as_bytes())
            .with_context(|| format!("writing {}", p.item.local_path().display()))?;
        report.updated_locally.push(p.item.file_name().to_string());
    }

    // A pull never publishes; it just remembers what it merged against.
    if direction == Direction::Pull {
        if let Some(rc) = &remote_commit {
            git.update_ref(BASE_REF, rc)?;
        }
        return Ok(report);
    }

    // --- Rebuild the working copy on top of the remote tip and publish ---
    if let Some(rc) = &remote_commit {
        git.reset_hard(rc)?;
    }
    for p in &pending {
        let path = git.dir().join(p.item.file_name());
        match &p.merged {
            Some(text) => {
                let mut body = text.clone();
                if !body.ends_with('\n') {
                    body.push('\n');
                }
                std::fs::write(&path, body)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let message = format!("sshm: sync from {}", super::lock::hostname());
    if git.commit_all(&message)? {
        git.push(branch)?;
        report.pushed = true;
        report.commit = git.rev_parse("HEAD");
    }

    // Whatever the remote now holds is our new merge base.
    if let Some(head) = git.rev_parse("HEAD") {
        git.update_ref(BASE_REF, &head)?;
    } else if let Some(rc) = &remote_commit {
        git.update_ref(BASE_REF, rc)?;
    }

    Ok(report)
}

// -----------------------------------------------------------------------------
// Per-item transforms between "the file on this machine" and "the file in the
// repo". Only settings.toml needs one.
// -----------------------------------------------------------------------------

/// Local file → repo form.
fn to_repo(item: SyncItem, text: &str) -> String {
    match item {
        SyncItem::Settings => settings_neutralized(text),
        _ => text.to_string(),
    }
}

/// Repo form → local file, preserving this machine's `[sync]` block.
fn to_local(item: SyncItem, text: &str, keep: &SyncConfig) -> String {
    match item {
        SyncItem::Settings => settings_with_sync(text, keep),
        _ => text.to_string(),
    }
}

/// settings.toml as it travels: parsed, re-serialized (so formatting can't
/// cause spurious diffs) with a default `[sync]` block, since sync config is
/// per-machine — it holds a key path and a repo URL, and syncing `enabled`
/// could switch every other machine off at once.
fn settings_neutralized(text: &str) -> String {
    let mut cfg: AppConfig = toml::from_str(text).unwrap_or_default();
    cfg.sync = SyncConfig::default();
    toml::to_string_pretty(&cfg).unwrap_or_else(|_| text.to_string())
}

/// The reverse: put this machine's `[sync]` block back in.
fn settings_with_sync(text: &str, keep: &SyncConfig) -> String {
    let mut cfg: AppConfig = toml::from_str(text).unwrap_or_default();
    cfg.sync = keep.clone();
    toml::to_string_pretty(&cfg).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::SyncMode;

    fn sample_settings() -> String {
        let cfg = AppConfig {
            default_username: "alice".into(),
            sync: SyncConfig {
                enabled: true,
                repo_url: "git@github.com:me/private.git".into(),
                ssh_key: "~/.ssh/secret_key".into(),
                mode: SyncMode::Interval,
                ..SyncConfig::default()
            },
            ..AppConfig::default()
        };
        toml::to_string_pretty(&cfg).unwrap()
    }

    #[test]
    fn settings_leaving_this_machine_carry_no_sync_config() {
        let out = settings_neutralized(&sample_settings());
        assert!(!out.contains("secret_key"), "the key path must not travel");
        assert!(!out.contains("github.com:me/private"), "the repo URL must not travel");
        assert!(out.contains("alice"), "the rest of the settings still travel");
        let back: AppConfig = toml::from_str(&out).unwrap();
        assert!(!back.sync.enabled);
    }

    #[test]
    fn settings_coming_back_keep_this_machines_sync_config() {
        let mine = toml::from_str::<AppConfig>(&sample_settings()).unwrap().sync;
        let theirs = AppConfig { default_username: "bob".into(), ..AppConfig::default() };
        let incoming = settings_neutralized(&toml::to_string_pretty(&theirs).unwrap());

        let merged: AppConfig = toml::from_str(&settings_with_sync(&incoming, &mine)).unwrap();
        assert_eq!(merged.default_username, "bob", "remote settings applied");
        assert_eq!(merged.sync.ssh_key, "~/.ssh/secret_key", "local sync config kept");
        assert!(merged.sync.enabled);
    }

    #[test]
    fn neutralizing_is_stable() {
        // Two runs on the same input must produce identical text, or every
        // sync would commit a no-op diff.
        let once = settings_neutralized(&sample_settings());
        assert_eq!(settings_neutralized(&once), once);
    }

    #[test]
    fn the_sync_table_stays_last_in_the_serialized_settings() {
        // TOML cannot express a plain value that comes after a table, so
        // serializing fails outright if a new setting is ever declared after
        // `sync` in AppConfig. This test is that tripwire.
        let text = toml::to_string_pretty(&AppConfig::default())
            .expect("`sync` must be the last field of AppConfig");
        let sync_at = text.find("[sync]").expect("a [sync] table");
        let scalar_at = text.find("notification_icon").expect("a scalar setting");
        assert!(scalar_at < sync_at, "scalars must precede the [sync] table");
        assert!(toml::from_str::<AppConfig>(&text).is_ok(), "and it must read back");
    }

    #[test]
    fn only_settings_gets_rewritten_on_the_way_out() {
        let raw = r#"{"hosts":{}}"#;
        assert_eq!(to_repo(SyncItem::Hosts, raw), raw);
        assert_eq!(to_local(SyncItem::Theme, "bg = \"#000\"", &SyncConfig::default()), "bg = \"#000\"");
    }

    #[test]
    fn preflight_rejects_an_https_remote() {
        let cfg = SyncConfig {
            repo_url: "https://github.com/me/conf.git".into(),
            ..SyncConfig::default()
        };
        let err = preflight(&cfg).unwrap_err().to_string();
        assert!(err.contains("SSH"), "got: {err}");
    }

    #[test]
    fn preflight_rejects_an_unconfigured_remote() {
        let err = preflight(&SyncConfig::default()).unwrap_err().to_string();
        assert!(err.contains("sshm sync setup"), "got: {err}");
    }

    #[test]
    fn preflight_rejects_a_missing_key() {
        let cfg = SyncConfig {
            repo_url: "git@github.com:me/conf.git".into(),
            ssh_key: "/definitely/not/here/id_ed25519".into(),
            ..SyncConfig::default()
        };
        let err = preflight(&cfg).unwrap_err().to_string();
        assert!(err.contains("does not exist"), "got: {err}");
    }

    #[test]
    fn a_rejected_push_is_recognized_as_a_race() {
        let e = anyhow::anyhow!("git push: ! [rejected] main -> main (non-fast-forward)");
        assert!(is_push_race(&e));
        assert!(!is_push_race(&anyhow::anyhow!("Permission denied (publickey)")));
    }

    #[test]
    fn the_summary_reads_well() {
        let quiet = SyncReport::default();
        assert_eq!(quiet.summary(), "already up to date");

        let busy = SyncReport {
            stats: MergeStats { conflicts: 2, ..MergeStats::default() },
            updated_locally: vec!["host.json".into()],
            pushed: true,
            commit: None,
        };
        assert_eq!(busy.summary(), "host.json updated locally · pushed · 2 conflict(s) resolved");
    }
}
