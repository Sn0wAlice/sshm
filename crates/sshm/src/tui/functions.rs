use crate::models::{Database, Host};
use crate::tui::app::Row;
use std::collections::{HashMap, HashSet};

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
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(seg);
        out.push(acc.clone());
    }
    out
}

fn collect_all_folders(db: &Database) -> Vec<String> {
    // Dedupe host folders first so `folder_chain` runs once per distinct folder,
    // not once per host.
    let leaves: HashSet<&str> = db
        .hosts
        .values()
        .filter_map(|h| h.folder.as_deref())
        .collect();
    let mut set: HashSet<String> = db.folders.iter().cloned().collect();
    for f in leaves {
        set.extend(folder_chain(f));
    }
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort();
    v
}

/// Push rows for the sub-tree rooted at `parent_path` (or top-level if `None`).
/// `children` maps a parent to its sub-folders (sorted); `hosts_by_folder` maps a
/// folder to its hosts (in `items` order). Both are built once per `build_rows`.
fn push_folder_subtree<'a>(
    rows: &mut Vec<Row<'a>>,
    parent_path: Option<&str>,
    children: &HashMap<Option<&str>, Vec<&str>>,
    hosts_by_folder: &HashMap<&str, Vec<&'a Host>>,
    collapsed: &HashMap<String, bool>,
) {
    let Some(kids) = children.get(&parent_path) else {
        return;
    };
    for &child in kids {
        let is_collapsed = collapsed.get(child).copied().unwrap_or(true);
        rows.push(Row::Folder {
            name: child.to_string(),
            collapsed: is_collapsed,
        });

        if !is_collapsed {
            push_folder_subtree(rows, Some(child), children, hosts_by_folder, collapsed);
            if let Some(hs) = hosts_by_folder.get(child) {
                rows.extend(hs.iter().copied().map(Row::Host));
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
        let mut children: HashMap<Option<&str>, Vec<&str>> = HashMap::new();
        for f in &folders {
            children.entry(folder_parent(f)).or_default().push(f);
        }
        let mut hosts_by_folder: HashMap<&str, Vec<&'a Host>> = HashMap::new();
        for h in items.iter().copied() {
            if let Some(f) = h.folder.as_deref() {
                hosts_by_folder.entry(f).or_default().push(h);
            }
        }

        // Top-level folders + their sub-trees
        push_folder_subtree(&mut rows, None, &children, &hosts_by_folder, collapsed);

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

    let mut buckets: std::collections::BTreeMap<&str, Vec<&Host>> = Default::default();
    for h in items.iter().copied() {
        match &h.tags {
            Some(tags) if !tags.is_empty() => {
                for t in tags {
                    buckets.entry(t.as_str()).or_default().push(h);
                }
            }
            _ => {
                buckets.entry("(no tag)").or_default().push(h);
            }
        }
    }

    let mut rows: Vec<Row<'a>> = Vec::new();
    for (tag, hosts) in buckets {
        // Use a `tag:` prefix in the collapse map so it can't collide with a
        // user-defined folder of the same name.
        let key = format!("tag:{}", tag);
        let is_collapsed = collapsed.get(&key).copied().unwrap_or(true);
        rows.push(Row::Folder {
            name: key.clone(),
            collapsed: is_collapsed,
        });
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

    fn host(name: &str, folder: Option<&str>) -> Host {
        Host {
            name: name.to_string(),
            folder: folder.map(str::to_string),
            ..Default::default()
        }
    }

    fn render(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Folder { name, collapsed } => format!("F {} {}", name, collapsed),
                Row::Host(h) => format!("H {}", h.name),
            })
            .collect()
    }

    #[test]
    fn build_rows_folder_tree_order() {
        let mut db = Database::default();
        db.folders = vec!["b".into(), "a".into(), "c/d".into()];
        for h in [
            host("h1", Some("a/x")),
            host("h2", Some("a")),
            host("h3", None),
            host("h4", Some("b")),
            host("h5", Some("a")),
            host("h6", Some("a/x/y")),
        ] {
            db.hosts.insert(h.name.clone(), h);
        }
        let order = ["h5", "h1", "h3", "h6", "h2", "h4"];
        let items: Vec<&Host> = order.iter().map(|n| &db.hosts[*n]).collect();
        let collapsed: HashMap<String, bool> = [("a", false), ("a/x", false), ("b", true)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let rows = build_rows(&db, &items, &[], "", &collapsed);
        assert_eq!(
            render(&rows),
            [
                "F a false",
                "F a/x false",
                "F a/x/y true",
                "H h1",
                "H h5",
                "H h2",
                "F b true",
                // "c/d" has no recorded parent "c", so (as before) it is not shown.
                "H h3",
            ]
        );
    }

    #[test]
    fn build_rows_by_tag_groups_sorted() {
        let mut a = host("a", None);
        a.tags = Some(vec!["web".into(), "prod".into()]);
        let b = host("b", None);
        let items = vec![&a, &b];
        let collapsed: HashMap<String, bool> =
            [("tag:prod".to_string(), false)].into_iter().collect();
        let rows = build_rows_by_tag(&items, &[], "", &collapsed);
        assert_eq!(
            render(&rows),
            [
                "F tag:(no tag) true",
                "F tag:prod false",
                "H a",
                "F tag:web true"
            ]
        );
    }
}
