//! Couche configuration : chemins + I/O JSON (lecture/écriture + migrations).
pub mod export;
pub mod io;
pub mod path;
pub mod settings;

pub use path::{config_path, ensure_config_file};
