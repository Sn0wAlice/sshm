use std::fs;
use std::path::PathBuf;
use serde::Deserialize;
use toml;
use ratatui::style::Color;

pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub muted: Color,
}

fn hex_to_color(hex: &str) -> Option<Color> {
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

#[derive(Deserialize)]
struct Config {
    bg: Option<String>,
    fg: Option<String>,
    accent: Option<String>,
    muted: Option<String>,
}

pub fn load() -> Theme {
    let mut config_path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_path.push("sshm/theme.toml");

    if let Ok(content) = fs::read_to_string(&config_path) {
        if let Ok(cfg) = toml::from_str::<Config>(&content) {
            let fallback = zenburn();
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
            };
        }
    }

    zenburn()
}

pub fn zenburn() -> Theme {
    Theme {
        bg: Color::Rgb(40, 40, 40),
        fg: Color::Rgb(220, 220, 204),
        accent: Color::Rgb(181, 189, 104),
        muted: Color::Rgb(150, 150, 150),
    }
}