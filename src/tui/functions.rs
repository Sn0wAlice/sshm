use crate::models::{Database, Host};
use crate::tui::app::Row;
pub fn build_rows<'a>(
    db: &'a Database,
    items: &'a Vec<&'a Host>,
    filtered: &'a Vec<&'a Host>,
    filter: &str,
    current_folder: &Option<String>,
) -> Vec<Row<'a>> {
    let mut rows: Vec<Row<'a>> = Vec::new();

    if filter.is_empty() {
        // Union of declared folders and folders inferred from hosts
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

        match current_folder {
            None => {
                // At root: show folders + hosts without folder
                for f_name in &folders {
                    rows.push(Row::Folder(f_name.clone()));
                }
                for h in items.iter().copied().filter(|h| h.folder.is_none()) {
                    rows.push(Row::Host(h));
                }
            }
            Some(fold) => {
                // Inside a folder: show breadcrumb + hosts
                //rows.push(Row::Folder(format!("<{}>", fold))); // breadcrumb
                rows.push(Row::Folder("..".to_string()));      // go parent
                for h in items
                    .iter()
                    .copied()
                    .filter(|h| h.folder.as_deref() == Some(fold.as_str()))
                {
                    rows.push(Row::Host(h));
                }
            }
        }
    } else {
        // Filtered view ignores folders
        for h in filtered {
            rows.push(Row::Host(*h));
        }
    }

    rows
}