use std::collections::{HashMap, HashSet};
use crate::models::{Database, Host};
use crate::tui::app::Row;

/// Top-level grouping for the host list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Group hosts by user-defined folders (default).
    Folders,
    /// Group hosts by tag — each tag becomes a virtual folder. Hosts with
    /// multiple tags appear once per tag.
    Tags,
}

impl ViewMode {
    pub fn toggle(self) -> Self {
        match self {
            ViewMode::Folders => ViewMode::Tags,
            ViewMode::Tags => ViewMode::Folders,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Folders => "folders",
            ViewMode::Tags => "tags",
        }
    }
}

/// Dispatch build_rows depending on the current [`ViewMode`].
pub fn rows_for<'a>(
    view: ViewMode,
    db: &'a Database,
    items: &'a [&'a Host],
    filtered: &'a [&'a Host],
    filter: &str,
    collapsed: &HashMap<String, bool>,
) -> Vec<Row<'a>> {
    match view {
        ViewMode::Folders => build_rows(db, items, filtered, filter, collapsed),
        ViewMode::Tags => build_rows_by_tag(items, filtered, filter, collapsed),
    }
}

/// Returns the nesting depth of a folder (0 = top-level, 1 = first sub-folder, …).
/// Folders use `/` as the path separator.
pub fn folder_depth(name: &str) -> usize {
    name.matches('/').count()
}

/// Returns the parent path of a folder, e.g. `"Prod/EU/Web"` → `Some("Prod/EU")`.
/// Returns `None` for top-level folders.
pub fn folder_parent(name: &str) -> Option<&str> {
    name.rfind('/').map(|i| &name[..i])
}

/// Walk up the path from `name` and return every prefix segment so that
/// `"a/b/c"` yields `["a", "a/b", "a/b/c"]`. Used to backfill missing
/// intermediate folders when only leaves are recorded on hosts.
fn folder_chain(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut acc = String::new();
    for seg in name.split('/').filter(|s| !s.is_empty()) {
        if !acc.is_empty() { acc.push('/'); }
        acc.push_str(seg);
        out.push(acc.clone());
    }
    out
}

fn collect_all_folders(db: &Database) -> Vec<String> {
    let mut set: HashSet<String> = db.folders.iter().cloned().collect();
    for h in db.hosts.values() {
        if let Some(ref f) = h.folder {
            for p in folder_chain(f) {
                set.insert(p);
            }
        }
    }
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort();
    v
}

/// Push rows for the sub-tree rooted at `parent_path` (or top-level if `None`).
fn push_folder_subtree<'a>(
    rows: &mut Vec<Row<'a>>,
    parent_path: Option<&str>,
    folders: &[String],
    items: &[&'a Host],
    collapsed: &HashMap<String, bool>,
) {
    let children: Vec<&String> = folders
        .iter()
        .filter(|f| folder_parent(f).as_deref() == parent_path)
        .collect();

    for child in children {
        let is_collapsed = collapsed.get(child.as_str()).copied().unwrap_or(true);
        rows.push(Row::Folder { name: child.clone(), collapsed: is_collapsed });

        if !is_collapsed {
            push_folder_subtree(rows, Some(child.as_str()), folders, items, collapsed);

            for h in items.iter().copied().filter(|h| h.folder.as_deref() == Some(child.as_str())) {
                rows.push(Row::Host(h));
            }
        }
    }
}

pub fn build_rows<'a>(
    db: &'a Database,
    items: &'a [&'a Host],
    filtered: &'a [&'a Host],
    filter: &str,
    collapsed: &HashMap<String, bool>,
) -> Vec<Row<'a>> {
    let mut rows: Vec<Row<'a>> = Vec::new();

    if filter.is_empty() {
        let folders = collect_all_folders(db);

        // Top-level folders + their sub-trees
        push_folder_subtree(&mut rows, None, &folders, items, collapsed);

        // Unfiled hosts (no folder)
        for h in items.iter().copied().filter(|h| h.folder.is_none()) {
            rows.push(Row::Host(h));
        }
    } else {
        // Filtered view: flat host list, no folders
        for h in filtered {
            rows.push(Row::Host(h));
        }
    }

    rows
}

/// Build rows in "group-by-tag" mode: virtual folders represent tags. A host
/// with multiple tags appears once per tag. Untagged hosts go under a synthetic
/// `"(no tag)"` group.
pub fn build_rows_by_tag<'a>(
    items: &'a [&'a Host],
    filtered: &'a [&'a Host],
    filter: &str,
    collapsed: &HashMap<String, bool>,
) -> Vec<Row<'a>> {
    if !filter.is_empty() {
        return filtered.iter().copied().map(Row::Host).collect();
    }

    let mut buckets: std::collections::BTreeMap<String, Vec<&Host>> = Default::default();
    for h in items.iter().copied() {
        match &h.tags {
            Some(tags) if !tags.is_empty() => {
                for t in tags {
                    buckets.entry(t.clone()).or_default().push(h);
                }
            }
            _ => {
                buckets.entry("(no tag)".to_string()).or_default().push(h);
            }
        }
    }

    let mut rows: Vec<Row<'a>> = Vec::new();
    for (tag, hosts) in buckets {
        // Use a `tag:` prefix in the collapse map so it can't collide with a
        // user-defined folder of the same name.
        let key = format!("tag:{}", tag);
        let is_collapsed = collapsed.get(&key).copied().unwrap_or(true);
        rows.push(Row::Folder { name: key.clone(), collapsed: is_collapsed });
        if !is_collapsed {
            for h in hosts {
                rows.push(Row::Host(h));
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_depth_counts_slashes() {
        assert_eq!(folder_depth("a"), 0);
        assert_eq!(folder_depth("a/b"), 1);
        assert_eq!(folder_depth("a/b/c"), 2);
    }

    #[test]
    fn folder_parent_strips_leaf() {
        assert_eq!(folder_parent("a"), None);
        assert_eq!(folder_parent("a/b"), Some("a"));
        assert_eq!(folder_parent("a/b/c"), Some("a/b"));
    }

    #[test]
    fn folder_chain_yields_prefixes() {
        assert_eq!(folder_chain("a/b/c"), vec!["a", "a/b", "a/b/c"]);
        assert_eq!(folder_chain("solo"), vec!["solo"]);
    }
}
