use std::collections::HashMap;
use crate::models::{Database, Host};
use crate::tui::app::Row;

/// Returns the nesting depth of a folder (0 = top-level, 1 = sub-folder).
pub fn folder_depth(name: &str) -> usize {
    name.matches('/').count().min(1)
}

/// Returns the parent portion of a sub-folder, e.g. "Prod/Web" → Some("Prod").
pub fn folder_parent(name: &str) -> Option<&str> {
    name.split('/').next().filter(|_| name.contains('/'))
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
        // Gather all folders (declared + inferred from hosts)
        let mut folders: Vec<String> = db.folders.clone();
        for h in db.hosts.values() {
            if let Some(ref folder) = h.folder {
                if !folders.iter().any(|f| f == folder) {
                    folders.push(folder.clone());
                }
                // Ensure parent folder exists for sub-folders
                if let Some(parent) = folder.split('/').next() {
                    if folder.contains('/') && !folders.iter().any(|f| f == parent) {
                        folders.push(parent.to_string());
                    }
                }
            }
        }
        folders.sort();
        folders.dedup();

        // Separate top-level and sub-folders
        let top_folders: Vec<&String> = folders.iter().filter(|f| !f.contains('/')).collect();
        let sub_folders: Vec<&String> = folders.iter().filter(|f| f.contains('/')).collect();

        for tf in &top_folders {
            let tf_name = tf.as_str();
            let tf_collapsed = collapsed.get(tf_name).copied().unwrap_or(true);
            rows.push(Row::Folder { name: tf_name.to_string(), collapsed: tf_collapsed });

            if !tf_collapsed {
                // Child sub-folders under this parent
                for sf in &sub_folders {
                    if folder_parent(sf) == Some(tf_name) {
                        let sf_collapsed = collapsed.get(sf.as_str()).copied().unwrap_or(true);
                        rows.push(Row::Folder { name: sf.to_string(), collapsed: sf_collapsed });

                        if !sf_collapsed {
                            for h in items.iter().copied().filter(|h| h.folder.as_deref() == Some(sf.as_str())) {
                                rows.push(Row::Host(h));
                            }
                        }
                    }
                }

                // Hosts directly in the top-level folder
                for h in items.iter().copied().filter(|h| h.folder.as_deref() == Some(tf_name)) {
                    rows.push(Row::Host(h));
                }
            }
        }

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
