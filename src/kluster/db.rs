//! Persistence layer for saved clusters (`kluster.json`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::models::{Cluster, ClusterKind, KlusterDb};
use crate::config::path::kluster_path;

/// Load the cluster DB from disk. If the file is missing, bootstrap one
/// from the user's `~/.kube/config` (and `$KUBECONFIG` if set).
///
/// Returns `(db, imported_count)` where `imported_count` is the number of
/// contexts pulled from kubeconfig on first run (0 when the file already
/// existed). The caller can surface that count via a toast.
pub fn load_or_bootstrap() -> (KlusterDb, usize) {
    let path = kluster_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(db) = serde_json::from_str::<KlusterDb>(&content) {
                return (db, 0);
            }
        }
        // File exists but unreadable / invalid: don't clobber, return empty.
        return (KlusterDb::default(), 0);
    }

    let mut db = KlusterDb::default();
    // Bootstrap Incus remotes (best-effort — empty if `incus` is not on PATH).
    db.incus_remotes = super::incus::list_remotes();
    for cfg_path in kubeconfig_paths() {
        if let Ok(text) = fs::read_to_string(&cfg_path) {
            for ctx_name in parse_kube_contexts(&text) {
                if !db.clusters.iter().any(|c| c.name == ctx_name) {
                    db.clusters.push(Cluster {
                        kind: ClusterKind::from_context_name(&ctx_name),
                        name: ctx_name.clone(),
                        kubeconfig: None,
                        context: Some(ctx_name),
                        namespace_default: None,
                    });
                }
            }
        }
    }
    let imported = db.clusters.len();
    let _ = save(&db);
    (db, imported)
}

/// Persist the DB atomically. Uses the same temp-then-rename pattern as
/// the host DB.
pub fn save(db: &KlusterDb) -> Result<()> {
    let path = kluster_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating kluster dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(db).context("serializing kluster db")?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)
        .with_context(|| format!("writing temp kluster file {}", tmp.display()))?;
    let _ = fs::remove_file(&path);
    if let Err(e) = fs::rename(&tmp, &path) {
        fs::write(&path, &json)
            .with_context(|| format!("rename failed ({e}); fallback-write {}", path.display()))?;
    }
    Ok(())
}

/// Candidate kubeconfig paths in priority order. Always returns at least
/// `~/.kube/config`; `$KUBECONFIG` may add several extras (colon-separated).
pub fn kubeconfig_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(env) = std::env::var("KUBECONFIG") {
        for p in env.split(':').filter(|s| !s.is_empty()) {
            out.push(PathBuf::from(p));
        }
    }
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".kube/config"));
    }
    out
}

/// Pure parse: extract the list of `contexts[].name` values from a
/// kubeconfig YAML text. Returns an empty `Vec` for malformed input.
///
/// We deliberately don't depend on a full YAML parser — kubeconfig
/// `contexts` is shallow, and the format is stable. We do a line-oriented
/// scan looking for the `contexts:` block and `name:` keys at the right
/// indentation. This handles the common (and overwhelming majority of)
/// real-world kubeconfigs.
pub fn parse_kube_contexts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_contexts = false;
    let mut block_indent: Option<usize> = None;

    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.starts_with("contexts:") {
            in_contexts = true;
            block_indent = None;
            continue;
        }
        // A new top-level YAML key (`clusters:`, `users:`, …) ends the block.
        // List items (`- ...`) remain inside the block even at column 0 in
        // typical kubeconfigs (block sequence indented relative to the parent).
        if in_contexts
            && indent == 0
            && trimmed.ends_with(':')
            && !trimmed.starts_with('-')
        {
            in_contexts = false;
            continue;
        }
        if !in_contexts { continue; }
        // Track the indentation of the first child entry; once we drop
        // back below it, the block is over.
        if let Some(bi) = block_indent {
            if !trimmed.is_empty() && indent < bi {
                in_contexts = false;
                continue;
            }
        } else if !trimmed.is_empty() {
            block_indent = Some(indent);
        }

        // The shape we care about:
        //   - context:
        //       cluster: foo
        //       user: bar
        //     name: my-context
        // Detect `name:` *outside* the inner `context:` mapping.
        if let Some(rest) = trimmed.strip_prefix("name:") {
            // `context:` children are deeper-indented; only top-of-entry
            // `name:` lines should be picked up. A simple heuristic: skip
            // lines that look like `name: <value>` whose immediate previous
            // significant line is `cluster:` / `user:` (those belong to
            // `cluster` / `user` sections, not contexts). Easier: just
            // accept all `name:` matches inside the contexts block — even
            // if a few cluster/user names slip in, we de-dupe later. But
            // since `clusters:` / `users:` sit *outside* the contexts
            // block, this is fine in practice.
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                out.push(value.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_two_contexts() {
        let yaml = "\
apiVersion: v1
kind: Config
clusters:
- cluster:
    server: https://1.2.3.4
  name: prod
contexts:
- context:
    cluster: prod
    user: alice
  name: prod-ctx
- context:
    cluster: dev
    user: alice
  name: dev-ctx
current-context: prod-ctx
users:
- name: alice
";
        let ctxs = parse_kube_contexts(yaml);
        assert_eq!(ctxs, vec!["dev-ctx".to_string(), "prod-ctx".to_string()]);
    }

    #[test]
    fn parse_empty_returns_empty() {
        assert!(parse_kube_contexts("").is_empty());
        assert!(parse_kube_contexts("apiVersion: v1\nkind: Config\n").is_empty());
    }

    #[test]
    fn parse_single_context_with_quotes() {
        let yaml = "\
contexts:
- context:
    cluster: c1
    user: u1
  name: \"my-quoted-ctx\"
";
        assert_eq!(parse_kube_contexts(yaml), vec!["my-quoted-ctx".to_string()]);
    }

    #[test]
    fn cluster_kind_detection() {
        assert_eq!(ClusterKind::from_context_name("homelab-k3s"), ClusterKind::K3s);
        assert_eq!(ClusterKind::from_context_name("prod-eks"), ClusterKind::K8s);
        assert_eq!(ClusterKind::from_context_name("K3S-cluster"), ClusterKind::K3s);
    }
}
