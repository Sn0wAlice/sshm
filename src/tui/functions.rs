use std::collections::HashMap;
use crate::models::{Database, Host};
use crate::tui::app::Row;

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
            }
        }
        folders.sort();
        folders.dedup();

        // For each folder, emit folder row, then hosts if expanded
        for f_name in &folders {
            let is_collapsed = collapsed.get(f_name).copied().unwrap_or(true);
            rows.push(Row::Folder { name: f_name.clone(), collapsed: is_collapsed });
            if !is_collapsed {
                for h in items.iter().copied().filter(|h| h.folder.as_deref() == Some(f_name.as_str())) {
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
            rows.push(Row::Host(*h));
        }
    }

    rows
}
