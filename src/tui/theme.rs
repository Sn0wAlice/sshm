use std::fs;
use std::path::PathBuf;
use serde::Deserialize;
use ratatui::style::Color;

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub muted: Color,
    pub error: Color,
    pub success: Color,
}

pub fn hex_to_color(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 { return None; }
    if let Ok(rgb) = u32::from_str_radix(hex, 16) {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        return Some(Color::Rgb(r, g, b));
    }
    None
}

pub fn color_to_hex(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        _ => "#000000".to_string(),
    }
}

pub struct ThemePreset {
    pub name: &'static str,
    pub bg: &'static str,
    pub fg: &'static str,
    pub accent: &'static str,
    pub muted: &'static str,
    pub error: &'static str,
    pub success: &'static str,
}

impl ThemePreset {
    pub fn to_theme(&self) -> Theme {
        Theme {
            bg: hex_to_color(self.bg).unwrap_or(Color::Rgb(0, 0, 0)),
            fg: hex_to_color(self.fg).unwrap_or(Color::Rgb(255, 255, 255)),
            accent: hex_to_color(self.accent).unwrap_or(Color::Rgb(128, 128, 128)),
            muted: hex_to_color(self.muted).unwrap_or(Color::Rgb(100, 100, 100)),
            error: hex_to_color(self.error).unwrap_or(Color::Rgb(220, 80, 80)),
            success: hex_to_color(self.success).unwrap_or(Color::Rgb(100, 200, 100)),
        }
    }
}

pub const PRESETS: &[ThemePreset] = &[
    ThemePreset { name: "Gruvbox",      bg: "#282828", fg: "#dcdccc", accent: "#b5bd68", muted: "#969696", error: "#cc6666", success: "#b5bd68" },
    ThemePreset { name: "Dracula",      bg: "#282a36", fg: "#f8f8f2", accent: "#bd93f9", muted: "#6272a4", error: "#ff5555", success: "#50fa7b" },
    ThemePreset { name: "Monokai",      bg: "#272822", fg: "#f8f8f2", accent: "#a6e22e", muted: "#75715e", error: "#f92672", success: "#a6e22e" },
    ThemePreset { name: "Nord",         bg: "#2e3440", fg: "#eceff4", accent: "#88c0d0", muted: "#4c566a", error: "#bf616a", success: "#a3be8c" },
    ThemePreset { name: "Solarized",    bg: "#002b36", fg: "#839496", accent: "#268bd2", muted: "#586e75", error: "#dc322f", success: "#859900" },
    ThemePreset { name: "Tokyo Night",  bg: "#1a1b26", fg: "#c0caf5", accent: "#7aa2f7", muted: "#565f89", error: "#f7768e", success: "#9ece6a" },
];

#[derive(Deserialize)]
struct Config {
    bg: Option<String>,
    fg: Option<String>,
    accent: Option<String>,
    muted: Option<String>,
    error: Option<String>,
    success: Option<String>,
}

fn theme_path() -> PathBuf {
    let mut config_path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_path.push("sshm/theme.toml");
    config_path
}

pub fn load() -> Theme {
    let path = theme_path();

    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<Config>(&content) {
            let fallback = get_global_theme();
            return Theme {
                bg: cfg.bg.as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.bg),
                fg: cfg.fg.as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.fg),
                accent: cfg.accent.as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.accent),
                muted: cfg.muted.as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.muted),
                error: cfg.error.as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.error),
                success: cfg.success.as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.success),
            };
        }
    }

    get_global_theme()
}

pub fn save_theme(bg: &str, fg: &str, accent: &str, muted: &str, error: &str, success: &str) {
    let path = theme_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let content = format!(
        "bg = \"{}\"\nfg = \"{}\"\naccent = \"{}\"\nmuted = \"{}\"\nerror = \"{}\"\nsuccess = \"{}\"\n",
        bg, fg, accent, muted, error, success
    );

    let tmp = path.with_extension("toml.tmp");
    if let Err(e) = fs::write(&tmp, &content) {
        eprintln!("Failed to write temp theme file: {e}");
        return;
    }
    let _ = fs::remove_file(&path);
    if let Err(e) = fs::rename(&tmp, &path) {
        eprintln!("Failed to move theme into place: {e}");
        let _ = fs::write(&path, &content);
    }
}

pub fn get_global_theme() -> Theme {
    Theme {
        bg: Color::Rgb(40, 40, 40),
        fg: Color::Rgb(220, 220, 204),
        accent: Color::Rgb(181, 189, 104),
        muted: Color::Rgb(150, 150, 150),
        error: Color::Rgb(204, 102, 102),
        success: Color::Rgb(181, 189, 104),
    }
}
