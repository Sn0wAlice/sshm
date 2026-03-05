//! Couche configuration : chemins + I/O JSON (lecture/écriture + migrations).
pub mod path;
pub mod io;
pub mod settings;

pub use path::{config_path, ensure_config_file};
pub use io::{load_hosts, save_hosts};
