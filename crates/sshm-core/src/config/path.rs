use std::fs;
use std::io;
use std::path::PathBuf;

/// The sshm configuration directory (`~/.config/sshm`), created on demand.
pub fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });

    let dir = base.join("sshm");
    if !dir.is_dir() {
        if let Err(e) = create_private_dir(&dir) {
            eprintln!("Cannot create folder {:?}: {}", dir, e);
        }
    } else {
        tighten_dir_mode(&dir);
    }
    dir
}

/// sshm's own dir holds hosts, settings and the sync clone: if an older
/// version (or a umask) left it group/other accessible, pull it back to 0o700.
#[cfg(unix)]
fn tighten_dir_mode(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(dir) {
        if meta.permissions().mode() & 0o077 != 0 {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
}

#[cfg(not(unix))]
fn tighten_dir_mode(_dir: &std::path::Path) {}

/// `create_dir_all`, with the leaf owner-only (0o700) on Unix when we create
/// it. An existing directory's mode is left alone.
fn create_private_dir(dir: &std::path::Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
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

pub fn ensure_config_file(path: &std::path::Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        super::io::write_private(path, b"{}\n")?;
    }
    Ok(())
}
