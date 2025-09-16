use std::path::PathBuf;
use std::io;

pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });
    base.join("sshm/host.json")
}

pub fn ensure_config_file(path: &PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::write(path, "{}\n")?;
    }
    Ok(())
}
