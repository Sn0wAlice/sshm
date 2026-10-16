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
            parts.push(format!(
                "{} updated locally",
                self.updated_locally.join(", ")
            ));
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
    super::crypt::preflight(cfg)?;
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
    text.contains("non-fast-forward") || text.contains("fetch first") || text.contains("[rejected]")
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
        // Only a missing file means "absent" (which the merge turns into a
        // deletion and publishes). Any other read error must stop the run.
        let local_disk = match std::fs::read_to_string(item.local_path()) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(e).with_context(|| format!("reading {}", item.local_path().display()))
            }
        };
        if let Some(t) = &local_disk {
            ensure_local_parses(item, t)?;
        }
        let local_repo = local_disk.as_deref().map(|t| to_repo(item, t));
        // Everything read back from the repo goes through `unseal` first: the
        // merge below reconciles entry by entry, so it has to see structure,
        // not a blob. A plaintext blob passes through untouched.
        let base = match base_rev.as_deref().and_then(|rev| git.show_file(rev, file)) {
            Some(blob) => {
                let text = super::crypt::unseal(cfg, &blob)
                    .with_context(|| format!("decrypting {file} at the merge base"))?;
                // The base ref only advances on success, so failing here would
                // abort every run forever. Fall back to a two-way merge.
                match ensure_parses(item, &text, "merge base") {
                    Ok(()) => Some(from_repo(item, text)),
                    Err(e) => {
                        eprintln!("sync: {e:#}; merging without a base");
                        None
                    }
                }
            }
            None => None,
        };
        let remote = match remote_commit
            .as_deref()
            .and_then(|rev| git.show_file(rev, file))
        {
            Some(blob) => {
                let text = super::crypt::unseal(cfg, &blob)
                    .with_context(|| format!("decrypting {file} from the remote"))?;
                match ensure_parses(item, &text, "remote") {
                    Ok(()) => Some(from_repo(item, text)),
                    // A push is how a user repairs a broken remote: treat it
                    // as absent so local state overwrites it.
                    Err(e) if direction == Direction::Push => {
                        eprintln!("sync: {e:#}; overwriting it with the local copy");
                        None
                    }
                    Err(e) => return Err(e),
                }
            }
            None => None,
        };

        let outcome = merge_item(
            item,
            base.as_deref(),
            local_repo.as_deref(),
            remote.as_deref(),
            policy,
        );
        stats.pulled += outcome.stats.pulled;
        stats.pushed += outcome.stats.pushed;
        stats.deleted += outcome.stats.deleted;
        stats.conflicts += outcome.stats.conflicts;

        pending.push(Pending {
            item,
            local_disk,
            merged: outcome.content,
        });
    }

    let mut report = SyncReport {
        stats,
        ..SyncReport::default()
    };

    // --- Apply the merged result to the local config files ---
    // Every direction writes locally, push included: the merged state is what
    // the remote ends up holding, so leaving the local copy behind would make
    // the next run mistake it for a fresh local edit and undo the difference.
    let mut writes = Vec::new();
    for p in &pending {
        let Some(merged) = &p.merged else { continue };
        let want = to_local(p.item, merged, &local_sync, p.local_disk.as_deref());
        if p.local_disk.as_deref().map(str::trim_end) == Some(want.trim_end()) {
            continue;
        }
        writes.push((p, want));
    }
    // The merge took a while (network); a TUI or CLI may have saved in the
    // meantime. Writing now would silently drop that edit, so bail and let the
    // next run (the scheduler retries) merge it properly. Checked for every
    // file before writing any, so an abort leaves nothing half-applied.
    for (p, _) in &writes {
        let now = std::fs::read_to_string(p.item.local_path()).ok();
        if now != p.local_disk {
            bail!("local {} changed during sync, retry", p.item.file_name());
        }
    }
    for (p, want) in writes {
        // Re-check right before each write to shrink the window further.
        if std::fs::read_to_string(p.item.local_path()).ok() != p.local_disk {
            bail!("local {} changed during sync, retry", p.item.file_name());
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
                // The last step before anything reaches a commit. `seal` fails
                // rather than falling back to cleartext, and the `?` here is
                // what makes that failure abort the run.
                let mut body = super::crypt::seal(cfg, text)
                    .with_context(|| format!("encrypting {}", p.item.file_name()))?;
                if !body.ends_with('\n') {
                    body.push('\n');
                }
                crate::config::io::write_private(&path, body.as_bytes())
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
// repo". settings.toml drops its `[sync]` block, host.json its history.
// -----------------------------------------------------------------------------

/// Refuse to sync a file that exists but doesn't parse. Every transform and
/// merge downstream would otherwise read it as empty, and the "merged" result
/// would delete the user's entries — locally and on every other machine.
fn ensure_parses(item: SyncItem, text: &str, side: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let ok = match item {
        SyncItem::Hosts => crate::config::io::parse_db_text(text).is_some(),
        SyncItem::Kluster => {
            serde_json::from_str::<crate::kluster::models::KlusterDb>(text).is_ok()
        }
        SyncItem::Settings => toml::from_str::<AppConfig>(text).is_ok(),
        // Merged as an opaque flat file; nothing to misread.
        SyncItem::Theme => true,
    };
    if !ok {
        bail!(
            "{} ({side}) is not valid; fix it before syncing",
            item.file_name()
        );
    }
    Ok(())
}

/// [`ensure_parses`] for this machine's own file. A blank host.json or
/// kluster.json (truncated by an editor, a full disk…) must not be published
/// as "delete everything"; a missing one never gets here.
fn ensure_local_parses(item: SyncItem, text: &str) -> Result<()> {
    if text.trim().is_empty() && matches!(item, SyncItem::Hosts | SyncItem::Kluster) {
        bail!(
            "local {} is empty; fix or remove it before syncing",
            item.file_name()
        );
    }
    ensure_parses(item, text, "local copy")
}

/// Local file → repo form.
fn to_repo(item: SyncItem, text: &str) -> String {
    match item {
        SyncItem::Settings => settings_neutralized(text),
        SyncItem::Hosts => hosts_without_history(text),
        _ => text.to_string(),
    }
}

/// A blob read back from the repo. Remotes written before history stopped
/// travelling still carry it; strip it so it can't surface as a conflict.
fn from_repo(item: SyncItem, text: String) -> String {
    match item {
        SyncItem::Hosts => hosts_without_history(&text),
        _ => text,
    }
}

/// Repo form → local file, preserving this machine's `[sync]` block and
/// connection history (`local_disk` is the file as it is on disk now).
fn to_local(item: SyncItem, text: &str, keep: &SyncConfig, local_disk: Option<&str>) -> String {
    match item {
        SyncItem::Settings => settings_with_sync(text, keep),
        SyncItem::Hosts => hosts_with_history(text, local_disk),
        _ => text.to_string(),
    }
}

/// host.json as it travels: `last_connected_at` / `use_count` describe what
/// *this* machine did, and since hosts merge as whole entries, a connection
/// would otherwise collide with a real edit made elsewhere. Unparseable text
/// passes through untouched.
fn hosts_without_history(text: &str) -> String {
    let Some(mut db) = crate::config::io::parse_db_text(text) else {
        return text.to_string();
    };
    for h in db.hosts.values_mut() {
        h.last_connected_at = None;
        h.use_count = 0;
    }
    crate::config::io::serialize_db(&db).unwrap_or_else(|_| text.to_string())
}

/// The reverse: hosts this machine already knows get their history back.
fn hosts_with_history(text: &str, local_disk: Option<&str>) -> String {
    let (Some(mut db), Some(local)) = (
        crate::config::io::parse_db_text(text),
        local_disk.and_then(crate::config::io::parse_db_text),
    ) else {
        return text.to_string();
    };
    let names: std::collections::HashSet<String> = db.hosts.keys().cloned().collect();
    for (name, h) in db.hosts.iter_mut() {
        // A rename coming from elsewhere changes the alias: fall back to the
        // one local host (whose alias is gone) with the same endpoint, if
        // unambiguous.
        let found = local.hosts.get(name).or_else(|| {
            let mut same = local.hosts.iter().filter(|(k, l)| {
                !names.contains(*k)
                    && l.host == h.host
                    && l.port == h.port
                    && l.username == h.username
            });
            match (same.next(), same.next()) {
                (Some((_, l)), None) => Some(l),
                _ => None,
            }
        });
        if let Some(l) = found {
            h.last_connected_at = l.last_connected_at.clone();
            h.use_count = l.use_count;
        }
    }
    crate::config::io::serialize_db(&db).unwrap_or_else(|_| text.to_string())
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
    // ---- encryption is wired into the gate, not just available -------------

    fn encrypting_cfg(identity: &str) -> SyncConfig {
        SyncConfig {
            enabled: true,
            repo_url: "git@example.com:me/cfg.git".into(),
            encrypt: true,
            age_identity: identity.into(),
            ..Default::default()
        }
    }

    #[test]
    fn preflight_refuses_a_run_that_could_not_encrypt() {
        // The property that matters: when the user asked for encryption and it
        // cannot be delivered, the run stops *before* anything is written or
        // pushed — never falling back to cleartext.
        if !Git::is_available() {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        for identity in ["", "/nonexistent/age.key"] {
            let err = preflight(&encrypting_cfg(identity))
                .expect_err("encryption on without a usable identity must not pass preflight");
            let text = format!("{err:#}");
            assert!(
                text.contains("age") || text.contains("identity") || text.contains("encrypt"),
                "the error should name the encryption problem, got: {text}"
            );
        }
    }

    #[test]
    fn preflight_is_unaffected_when_encryption_is_off() {
        if !Git::is_available() {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        let mut cfg = encrypting_cfg("/nonexistent/age.key");
        cfg.encrypt = false;
        assert!(preflight(&cfg).is_ok(), "{:?}", preflight(&cfg));
    }

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
        assert!(
            !out.contains("github.com:me/private"),
            "the repo URL must not travel"
        );
        assert!(
            out.contains("alice"),
            "the rest of the settings still travel"
        );
        let back: AppConfig = toml::from_str(&out).unwrap();
        assert!(!back.sync.enabled);
    }

    #[test]
    fn settings_coming_back_keep_this_machines_sync_config() {
        let mine = toml::from_str::<AppConfig>(&sample_settings())
            .unwrap()
            .sync;
        let theirs = AppConfig {
            default_username: "bob".into(),
            ..AppConfig::default()
        };
        let incoming = settings_neutralized(&toml::to_string_pretty(&theirs).unwrap());

        let merged: AppConfig = toml::from_str(&settings_with_sync(&incoming, &mine)).unwrap();
        assert_eq!(merged.default_username, "bob", "remote settings applied");
        assert_eq!(
            merged.sync.ssh_key, "~/.ssh/secret_key",
            "local sync config kept"
        );
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
        assert!(
            toml::from_str::<AppConfig>(&text).is_ok(),
            "and it must read back"
        );
    }

    #[test]
    fn the_theme_is_not_rewritten_on_the_way_back() {
        assert_eq!(
            to_local(
                SyncItem::Theme,
                "bg = \"#000\"",
                &SyncConfig::default(),
                None
            ),
            "bg = \"#000\""
        );
    }

    #[test]
    fn unparseable_non_empty_files_abort_instead_of_reading_as_empty() {
        assert!(ensure_parses(SyncItem::Hosts, "{ broken", "local copy").is_err());
        assert!(ensure_parses(SyncItem::Kluster, "nope", "remote").is_err());
        assert!(ensure_parses(SyncItem::Settings, "default_port = \"x\"", "base").is_err());
        assert!(ensure_parses(SyncItem::Hosts, "  \n", "local copy").is_ok());
        assert!(ensure_parses(SyncItem::Hosts, r#"{"hosts":{}}"#, "remote").is_ok());
    }

    #[test]
    fn a_blank_local_db_is_refused() {
        assert!(ensure_local_parses(SyncItem::Hosts, " \n").is_err());
        assert!(ensure_local_parses(SyncItem::Kluster, "").is_err());
        assert!(ensure_local_parses(SyncItem::Theme, "").is_ok());
        assert!(ensure_local_parses(SyncItem::Hosts, r#"{"hosts":{}}"#).is_ok());
    }

    #[test]
    fn history_follows_a_host_renamed_elsewhere() {
        let local =
            r#"{"hosts":{"old":{"name":"old","host":"1.2.3.4","use_count":7}},"folders":[]}"#;
        let merged = r#"{"hosts":{"new":{"name":"new","host":"1.2.3.4"}},"folders":[]}"#;
        let back = hosts_with_history(merged, Some(local));
        let db = crate::config::io::parse_db_text(&back).unwrap();
        assert_eq!(db.hosts["new"].use_count, 7);
    }

    #[test]
    fn host_history_never_travels_and_survives_the_round_trip() {
        let local = r#"{"hosts":{"prod":{"name":"prod","host":"1.2.3.4","last_connected_at":"2026-10-16T10:00:00Z","use_count":7}},"folders":[]}"#;
        let out = to_repo(SyncItem::Hosts, local);
        assert!(!out.contains("2026-10-16"), "{out}");
        assert!(!out.contains("\"use_count\": 7"), "{out}");

        // Remote changed the port; this machine's history comes back.
        let merged = out.replace("1.2.3.4", "5.6.7.8");
        let back = to_local(
            SyncItem::Hosts,
            &merged,
            &SyncConfig::default(),
            Some(local),
        );
        let db = crate::config::io::parse_db_text(&back).unwrap();
        let h = &db.hosts["prod"];
        assert_eq!(h.host, "5.6.7.8");
        assert_eq!(h.use_count, 7);
        assert_eq!(h.last_connected_at.as_deref(), Some("2026-10-16T10:00:00Z"));
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
        assert!(!is_push_race(&anyhow::anyhow!(
            "Permission denied (publickey)"
        )));
    }

    #[test]
    fn the_summary_reads_well() {
        let quiet = SyncReport::default();
        assert_eq!(quiet.summary(), "already up to date");

        let busy = SyncReport {
            stats: MergeStats {
                conflicts: 2,
                ..MergeStats::default()
            },
            updated_locally: vec!["host.json".into()],
            pushed: true,
            commit: None,
        };
        assert_eq!(
            busy.summary(),
            "host.json updated locally · pushed · 2 conflict(s) resolved"
        );
    }
}
