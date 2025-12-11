use std::path::PathBuf;
use std::io;
use std::fs;

pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });

    let dir = base.join("sshm");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("Cannot create folder {:?}: {}", dir, e);
    }

    dir.join("host.json")
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
