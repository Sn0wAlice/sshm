use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_port")]
    pub default_port: u16,
    #[serde(default = "default_username")]
    pub default_username: String,
    #[serde(default)]
    pub default_identity_file: String,
}

fn default_port() -> u16 { 22 }
fn default_username() -> String { "root".to_string() }

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            default_port: 22,
            default_username: "root".to_string(),
            default_identity_file: String::new(),
        }
    }
}

pub fn settings_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });
    base.join("sshm").join("settings.toml")
}

pub fn load_settings() -> AppConfig {
    let path = settings_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<AppConfig>(&content) {
            return cfg;
        }
    }
    AppConfig::default()
}

pub fn save_settings(config: &AppConfig) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let toml_str = match toml::to_string_pretty(config) {
        Ok(s) => s,
        Err(e) => { eprintln!("Failed to serialize settings: {e}"); return; }
    };

    let tmp = path.with_extension("toml.tmp");
    if let Err(e) = fs::write(&tmp, &toml_str) {
        eprintln!("Failed to write temp settings file: {e}");
        return;
    }
    let _ = fs::remove_file(&path);
    if let Err(e) = fs::rename(&tmp, &path) {
        eprintln!("Failed to move settings into place: {e}");
        let _ = fs::write(&path, &toml_str);
    }
}
