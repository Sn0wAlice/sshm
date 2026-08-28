use std::path::PathBuf;
use std::io;
use std::fs;

/// The sshm configuration directory (`~/.config/sshm`), created on demand.
pub fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });

    let dir = base.join("sshm");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("Cannot create folder {:?}: {}", dir, e);
    }
    dir
}

pub fn config_path() -> PathBuf {
    config_dir().join("host.json")
}
/// Path to the Kluster (Docker + k8s) database, sibling of `host.json`.
pub fn kluster_path() -> PathBuf {
    config_path().with_file_name("kluster.json")
}

/// Path to the theme file, sibling of `host.json`.
pub fn theme_path() -> PathBuf {
    config_path().with_file_name("theme.toml")
}

/// Working clone used by the git config sync (`~/.config/sshm/sync-repo`).
///
/// Deliberately *not* a sibling file of the DBs: it's a directory, and the
/// config watcher only ever classifies the three known file names, so nothing
/// in here can be mistaken for a config change.
pub fn sync_repo_dir() -> PathBuf {
    config_dir().join("sync-repo")
}

/// Cross-process lock guarding a sync run (see [`crate::sync::lock`]).
pub fn sync_lock_path() -> PathBuf {
    config_dir().join("sync.lock")
}

/// Shared, machine-wide sync bookkeeping (last run timestamp, last error).
pub fn sync_state_path() -> PathBuf {
    config_dir().join("sync-state.json")
}

pub fn ensure_config_file(path: &PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        fs::write(path, "{}\n")?;
    }
    Ok(())
}
