//! Three-way merge of the synced config files.
//!
//! Every sync run has three versions of each file in hand: the **base** (what
//! this machine last agreed on with the remote), the **local** copy, and the
//! **remote** one. With those, the common cases resolve without ever bothering
//! the user:
//!
//! * only one side moved → take that side (a deletion counts as a move);
//! * both sides moved *identically* → nothing to do;
//! * both sides moved differently → merge entry-by-entry for the databases
//!   (`host.json`, `kluster.json`), and fall back to
//!   [`ConflictPolicy`](crate::config::settings::ConflictPolicy) for the two
//!   flat files (`settings.toml`, `theme.toml`) and for individual entries
//!   that genuinely collide.
//!
//! Entry-level merging is what makes "laptop added a host, desktop added
//! another" a non-event instead of a conflict.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::settings::{ConflictPolicy, SyncItem};
use crate::kluster::models::{Cluster, DockerRemote, KlusterDb};
use crate::models::{Database, Host};

/// What a merge did, for the one-line summary a sync prints.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MergeStats {
    /// Entries taken from (or added by) the remote.
    pub pulled: usize,
    /// Entries this machine contributes.
    pub pushed: usize,
    /// Entries removed on one side and untouched on the other.
    pub deleted: usize,
    /// Entries changed differently on both sides; resolved by policy.
    pub conflicts: usize,
}

impl MergeStats {
    pub fn is_quiet(&self) -> bool {
        self.pulled == 0 && self.pushed == 0 && self.deleted == 0 && self.conflicts == 0
    }

    fn merge_in(&mut self, other: MergeStats) {
        self.pulled += other.pulled;
        self.pushed += other.pushed;
        self.deleted += other.deleted;
        self.conflicts += other.conflicts;
    }
}

/// Merge result for one file. `content: None` means the file should not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub content: Option<String>,
    pub stats: MergeStats,
}

/// Compare while ignoring trailing whitespace, so a stray newline picked up by
/// an editor never registers as a change.
fn norm(v: Option<&str>) -> Option<&str> {
    v.map(|s| s.trim_end())
}

fn same(a: Option<&str>, b: Option<&str>) -> bool {
    norm(a) == norm(b)
}

/// Three-way merge of one synced file.
pub fn merge_item(
    item: SyncItem,
    base: Option<&str>,
    local: Option<&str>,
    remote: Option<&str>,
    policy: ConflictPolicy,
) -> Outcome {
    // Fast paths, valid for every file type.
    if same(local, remote) {
        return Outcome { content: local.map(str::to_string), stats: MergeStats::default() };
    }
    if same(base, local) {
        // Only the remote moved: take it wholesale (including a deletion).
        let stats = MergeStats {
            pulled: 1,
            deleted: usize::from(remote.is_none()),
            ..MergeStats::default()
        };
        return Outcome { content: remote.map(str::to_string), stats };
    }
    if same(base, remote) {
        // Only we moved.
        let stats = MergeStats { pushed: 1, ..MergeStats::default() };
        return Outcome { content: local.map(str::to_string), stats };
    }

    // Both sides moved, differently.
    match item {
        SyncItem::Hosts => merge_hosts(base, local, remote, policy),
        SyncItem::Kluster => merge_kluster(base, local, remote, policy),
        // Flat files have no entries to reconcile: one side has to win.
        SyncItem::Settings | SyncItem::Theme => {
            let winner = match policy {
                ConflictPolicy::PreferLocal => local,
                ConflictPolicy::PreferRemote => remote,
            };
            Outcome {
                content: winner.map(str::to_string),
                stats: MergeStats { conflicts: 1, ..MergeStats::default() },
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Generic keyed merge
// -----------------------------------------------------------------------------

/// Three-way merge of a keyed collection. Each key is decided on its own, so
/// two machines adding different entries both keep theirs.
fn merge_maps<T: Clone + PartialEq>(
    base: &BTreeMap<String, T>,
    local: &BTreeMap<String, T>,
    remote: &BTreeMap<String, T>,
    policy: ConflictPolicy,
) -> (BTreeMap<String, T>, MergeStats) {
    let mut out: BTreeMap<String, T> = BTreeMap::new();
    let mut stats = MergeStats::default();

    let keys: BTreeSet<&String> =
        base.keys().chain(local.keys()).chain(remote.keys()).collect();

    for key in keys {
        let b = base.get(key);
        let l = local.get(key);
        let r = remote.get(key);

        let chosen = if l == r {
            l
        } else if b == l {
            // Remote added, changed or removed it; we didn't touch it.
            if r.is_none() { stats.deleted += 1 } else { stats.pulled += 1 }
            r
        } else if b == r {
            // Our side moved; remote stood still.
            if l.is_none() { stats.deleted += 1 } else { stats.pushed += 1 }
            l
        } else {
            // Genuinely divergent: both sides changed this entry.
            stats.conflicts += 1;
            match policy {
                ConflictPolicy::PreferLocal => l.or(r),
                ConflictPolicy::PreferRemote => r.or(l),
            }
        };

        if let Some(v) = chosen {
            out.insert(key.clone(), v.clone());
        }
    }

    (out, stats)
}

/// Three-way merge of a set of plain strings. A value survives when both sides
/// still have it, or when one side has just added it; a removal on one side
/// with no change on the other removes it.
fn merge_sets(
    base: &BTreeSet<String>,
    local: &BTreeSet<String>,
    remote: &BTreeSet<String>,
) -> BTreeSet<String> {
    local
        .union(remote)
        .filter(|v| {
            let in_local = local.contains(*v);
            let in_remote = remote.contains(*v);
            let in_base = base.contains(*v);
            (in_local && in_remote) || !in_base
        })
        .cloned()
        .collect()
}

// -----------------------------------------------------------------------------
// host.json
// -----------------------------------------------------------------------------

fn parse_hosts(text: Option<&str>) -> Database {
    text.and_then(crate::config::io::parse_db_text).unwrap_or_default()
}

fn host_map(db: &Database) -> BTreeMap<String, Host> {
    db.hosts.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn merge_hosts(
    base: Option<&str>,
    local: Option<&str>,
    remote: Option<&str>,
    policy: ConflictPolicy,
) -> Outcome {
    let (b, l, r) = (parse_hosts(base), parse_hosts(local), parse_hosts(remote));

    let (hosts, stats) = merge_maps(&host_map(&b), &host_map(&l), &host_map(&r), policy);

    let mut folders = merge_sets(
        &b.folders.iter().cloned().collect(),
        &l.folders.iter().cloned().collect(),
        &r.folders.iter().cloned().collect(),
    );
    // A surviving host must keep a folder to live in, even if the other side
    // deleted that folder in the same window.
    for h in hosts.values() {
        if let Some(f) = &h.folder {
            folders.insert(f.clone());
        }
    }

    let merged = Database {
        hosts: hosts.into_iter().collect(),
        folders: folders.into_iter().collect(),
        ..Default::default()
    };

    match crate::config::io::serialize_db(&merged) {
        Ok(text) => Outcome { content: Some(text), stats },
        // Unreachable in practice; degrade to the policy winner rather than
        // dropping somebody's hosts on the floor.
        Err(_) => Outcome {
            content: match policy {
                ConflictPolicy::PreferLocal => local.map(str::to_string),
                ConflictPolicy::PreferRemote => remote.map(str::to_string),
            },
            stats: MergeStats { conflicts: 1, ..MergeStats::default() },
        },
    }
}

// -----------------------------------------------------------------------------
// kluster.json
// -----------------------------------------------------------------------------

fn parse_kluster(text: Option<&str>) -> KlusterDb {
    text.and_then(|t| serde_json::from_str::<KlusterDb>(t).ok())
        .unwrap_or_default()
}

fn cluster_map(db: &KlusterDb) -> BTreeMap<String, Cluster> {
    db.clusters.iter().map(|c| (c.name.clone(), c.clone())).collect()
}

fn docker_map(db: &KlusterDb) -> BTreeMap<String, DockerRemote> {
    db.docker_remotes.iter().map(|d| (d.host_alias.clone(), d.clone())).collect()
}

fn merge_kluster(
    base: Option<&str>,
    local: Option<&str>,
    remote: Option<&str>,
    policy: ConflictPolicy,
) -> Outcome {
    let (b, l, r) = (parse_kluster(base), parse_kluster(local), parse_kluster(remote));
    let mut stats = MergeStats::default();

    let (clusters, s1) = merge_maps(&cluster_map(&b), &cluster_map(&l), &cluster_map(&r), policy);
    stats.merge_in(s1);
    let (dockers, s2) = merge_maps(&docker_map(&b), &docker_map(&l), &docker_map(&r), policy);
    stats.merge_in(s2);

    let incus_remotes = merge_sets(
        &b.incus_remotes.iter().cloned().collect(),
        &l.incus_remotes.iter().cloned().collect(),
        &r.incus_remotes.iter().cloned().collect(),
    );

    let merged = KlusterDb {
        clusters: clusters.into_values().collect(),
        incus_remotes: incus_remotes.into_iter().collect(),
        docker_remotes: dockers.into_values().collect(),
    };

    match serde_json::to_string_pretty(&merged) {
        Ok(text) => Outcome { content: Some(text), stats },
        Err(_) => Outcome {
            content: match policy {
                ConflictPolicy::PreferLocal => local.map(str::to_string),
                ConflictPolicy::PreferRemote => remote.map(str::to_string),
            },
            stats: MergeStats { conflicts: 1, ..MergeStats::default() },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_json(entries: &[(&str, &str)]) -> String {
        let hosts: Vec<String> = entries
            .iter()
            .map(|(name, ip)| {
                format!(
                    r#""{name}": {{"name":"{name}","host":"{ip}","port":22,"username":"root"}}"#
                )
            })
            .collect();
        format!(r#"{{"hosts": {{{}}}, "folders": []}}"#, hosts.join(","))
    }

    fn names(text: &str) -> Vec<String> {
        let db = crate::config::io::parse_db_text(text).unwrap();
        let mut n: Vec<String> = db.hosts.keys().cloned().collect();
        n.sort();
        n
    }

    fn ip_of(text: &str, name: &str) -> String {
        crate::config::io::parse_db_text(text).unwrap().hosts[name].host.clone()
    }

    #[test]
    fn identical_sides_are_a_no_op() {
        let a = host_json(&[("a", "1.1.1.1")]);
        let out = merge_item(SyncItem::Hosts, Some(&a), Some(&a), Some(&a), ConflictPolicy::PreferLocal);
        assert!(out.stats.is_quiet());
        assert_eq!(out.content.as_deref(), Some(a.as_str()));
    }

    #[test]
    fn a_remote_only_change_is_pulled() {
        let base = host_json(&[("a", "1.1.1.1")]);
        let remote = host_json(&[("a", "1.1.1.1"), ("b", "2.2.2.2")]);
        let out = merge_item(
            SyncItem::Hosts, Some(&base), Some(&base), Some(&remote), ConflictPolicy::PreferLocal,
        );
        assert_eq!(out.content.as_deref(), Some(remote.as_str()));
        assert_eq!(out.stats.pulled, 1);
    }

    #[test]
    fn a_local_only_change_is_kept() {
        let base = host_json(&[("a", "1.1.1.1")]);
        let local = host_json(&[("a", "1.1.1.1"), ("b", "2.2.2.2")]);
        let out = merge_item(
            SyncItem::Hosts, Some(&base), Some(&local), Some(&base), ConflictPolicy::PreferLocal,
        );
        assert_eq!(out.content.as_deref(), Some(local.as_str()));
        assert_eq!(out.stats.pushed, 1);
    }

    #[test]
    fn both_sides_adding_different_hosts_keeps_both() {
        let base = host_json(&[("a", "1.1.1.1")]);
        let local = host_json(&[("a", "1.1.1.1"), ("laptop", "10.0.0.1")]);
        let remote = host_json(&[("a", "1.1.1.1"), ("desktop", "10.0.0.2")]);

        let out = merge_item(
            SyncItem::Hosts, Some(&base), Some(&local), Some(&remote), ConflictPolicy::PreferLocal,
        );
        let text = out.content.expect("merged content");
        assert_eq!(names(&text), vec!["a", "desktop", "laptop"]);
        assert_eq!(out.stats.conflicts, 0, "distinct additions are not a conflict");
        assert_eq!(out.stats.pulled, 1);
        assert_eq!(out.stats.pushed, 1);
    }

    #[test]
    fn a_deletion_on_one_side_wins_over_an_untouched_entry() {
        let base = host_json(&[("a", "1.1.1.1"), ("gone", "9.9.9.9")]);
        let local = host_json(&[("a", "1.1.1.1"), ("gone", "9.9.9.9"), ("new", "3.3.3.3")]);
        let remote = host_json(&[("a", "1.1.1.1")]); // deleted `gone`

        let out = merge_item(
            SyncItem::Hosts, Some(&base), Some(&local), Some(&remote), ConflictPolicy::PreferLocal,
        );
        let text = out.content.unwrap();
        assert_eq!(names(&text), vec!["a", "new"]);
        assert_eq!(out.stats.deleted, 1);
    }

    #[test]
    fn the_same_host_edited_on_both_sides_follows_the_policy() {
        let base = host_json(&[("a", "1.1.1.1")]);
        let local = host_json(&[("a", "10.0.0.1")]);
        let remote = host_json(&[("a", "20.0.0.1")]);

        let keep_local = merge_item(
            SyncItem::Hosts, Some(&base), Some(&local), Some(&remote), ConflictPolicy::PreferLocal,
        );
        assert_eq!(ip_of(&keep_local.content.unwrap(), "a"), "10.0.0.1");
        assert_eq!(keep_local.stats.conflicts, 1);

        let keep_remote = merge_item(
            SyncItem::Hosts, Some(&base), Some(&local), Some(&remote), ConflictPolicy::PreferRemote,
        );
        assert_eq!(ip_of(&keep_remote.content.unwrap(), "a"), "20.0.0.1");
        assert_eq!(keep_remote.stats.conflicts, 1);
    }

    #[test]
    fn a_first_push_has_no_base_and_no_remote() {
        let local = host_json(&[("a", "1.1.1.1")]);
        let out = merge_item(SyncItem::Hosts, None, Some(&local), None, ConflictPolicy::PreferLocal);
        assert_eq!(out.content.as_deref(), Some(local.as_str()));
        assert_eq!(out.stats.pushed, 1, "the whole file is ours to contribute");
        assert_eq!(out.stats.conflicts, 0);
    }

    #[test]
    fn a_fresh_machine_adopts_the_remote() {
        let remote = host_json(&[("a", "1.1.1.1")]);
        let out = merge_item(SyncItem::Hosts, None, None, Some(&remote), ConflictPolicy::PreferLocal);
        assert_eq!(out.content.as_deref(), Some(remote.as_str()));
        assert_eq!(out.stats.pulled, 1);
    }

    #[test]
    fn two_machines_starting_from_nothing_keep_both_host_sets() {
        // No common ancestor at all: both sides created host.json independently.
        let local = host_json(&[("laptop", "10.0.0.1")]);
        let remote = host_json(&[("desktop", "10.0.0.2")]);
        let out = merge_item(SyncItem::Hosts, None, Some(&local), Some(&remote), ConflictPolicy::PreferLocal);
        assert_eq!(names(&out.content.unwrap()), vec!["desktop", "laptop"]);
    }

    #[test]
    fn folders_survive_for_hosts_that_survive() {
        let base = r#"{"hosts":{}, "folders":["Prod"]}"#.to_string();
        let local = r#"{"hosts":{"a":{"name":"a","host":"1.1.1.1","folder":"Prod"}}, "folders":["Prod"]}"#.to_string();
        // Remote dropped the (then empty) folder.
        let remote = r#"{"hosts":{}, "folders":[]}"#.to_string();

        let out = merge_item(
            SyncItem::Hosts, Some(&base), Some(&local), Some(&remote), ConflictPolicy::PreferLocal,
        );
        let db = crate::config::io::parse_db_text(&out.content.unwrap()).unwrap();
        assert!(db.hosts.contains_key("a"));
        assert!(db.folders.contains(&"Prod".to_string()), "the surviving host keeps its folder");
    }

    #[test]
    fn trailing_newlines_are_not_a_change() {
        let a = host_json(&[("a", "1.1.1.1")]);
        let b = format!("{a}\n\n");
        let out = merge_item(SyncItem::Hosts, Some(&a), Some(&b), Some(&a), ConflictPolicy::PreferLocal);
        assert!(out.stats.is_quiet());
    }

    #[test]
    fn flat_files_fall_back_to_the_policy() {
        let out = merge_item(
            SyncItem::Theme, Some("base"), Some("mine"), Some("theirs"), ConflictPolicy::PreferLocal,
        );
        assert_eq!(out.content.as_deref(), Some("mine"));
        assert_eq!(out.stats.conflicts, 1);

        let out = merge_item(
            SyncItem::Theme, Some("base"), Some("mine"), Some("theirs"), ConflictPolicy::PreferRemote,
        );
        assert_eq!(out.content.as_deref(), Some("theirs"));
    }

    #[test]
    fn clusters_from_both_machines_are_kept() {
        let base = r#"{"clusters":[],"incus_remotes":[],"docker_remotes":[]}"#;
        let local = r#"{"clusters":[{"name":"k3s-home","kind":"K3s"}],"incus_remotes":["lab"],"docker_remotes":[]}"#;
        let remote = r#"{"clusters":[{"name":"prod","kind":"K8s"}],"incus_remotes":[],"docker_remotes":[{"host_alias":"bastion"}]}"#;

        let out = merge_item(
            SyncItem::Kluster, Some(base), Some(local), Some(remote), ConflictPolicy::PreferLocal,
        );
        let db: KlusterDb = serde_json::from_str(&out.content.unwrap()).unwrap();
        let mut names: Vec<&str> = db.clusters.iter().map(|c| c.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["k3s-home", "prod"]);
        assert_eq!(db.incus_remotes, vec!["lab".to_string()]);
        assert_eq!(db.docker_remotes.len(), 1);
    }
}
