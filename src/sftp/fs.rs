use std::{fs, io};
use std::path::{Path, PathBuf};
use crate::tui::app_sftp::FileEntry;
pub fn read_local_dir(path: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let file_name = entry
            .file_name()
            .to_string_lossy()
            .to_string();
        let is_dir = meta.is_dir();
        entries.push(FileEntry {
            name: file_name,
            is_dir,
        });
    }
    // Sort directories first, then files, both alphabetically
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    Ok(entries)
}


pub fn unique_local_path(dir: &Path, file_name: &str) -> PathBuf {
    // Split into base and suffix (keep multi-part extensions like .tar.gz as suffix)
    let (base, suffix) = if let Some(pos) = file_name.find('.') {
        let (b, s) = file_name.split_at(pos);
        (b.to_string(), s.to_string())
    } else {
        (file_name.to_string(), String::new())
    };

    let mut candidate = dir.join(format!("{}{}", base, suffix));
    if !candidate.exists() {
        return candidate;
    }

    let mut n = 1;
    loop {
        let name = format!("{} ({}){}", base, n, suffix);
        candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}