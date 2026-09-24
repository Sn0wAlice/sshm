use ratatui::style::Color;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub muted: Color,
    pub error: Color,
    pub success: Color,
    /// Something that needs attention but isn't a failure — an enabled
    /// ForwardAgent, a host that just went unreachable. Before this role
    /// existed these borrowed `error`, which overstated them.
    ///
    /// Falls back to `error` when a `theme.toml` doesn't set it, so existing
    /// themes render exactly as they did.
    pub warning: Color,
    /// Box borders and separators. Falls back to `muted`.
    pub border: Color,
    /// Background of the selected row. Falls back to `accent`.
    pub selection: Color,
}

pub fn hex_to_color(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
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
            // Presets declare the six classic roles; the three added later
            // inherit the ones they used to borrow, so every preset still
            // looks exactly as its author intended.
            warning: hex_to_color(self.error).unwrap_or(Color::Rgb(220, 80, 80)),
            border: hex_to_color(self.muted).unwrap_or(Color::Rgb(100, 100, 100)),
            selection: hex_to_color(self.accent).unwrap_or(Color::Rgb(128, 128, 128)),
        }
    }
}

pub const PRESETS: &[ThemePreset] = &[
    ThemePreset {
        name: "Gruvbox",
        bg: "#282828",
        fg: "#dcdccc",
        accent: "#b5bd68",
        muted: "#969696",
        error: "#cc6666",
        success: "#b5bd68",
    },
    ThemePreset {
        name: "Dracula",
        bg: "#282a36",
        fg: "#f8f8f2",
        accent: "#bd93f9",
        muted: "#6272a4",
        error: "#ff5555",
        success: "#50fa7b",
    },
    ThemePreset {
        name: "Monokai",
        bg: "#272822",
        fg: "#f8f8f2",
        accent: "#a6e22e",
        muted: "#75715e",
        error: "#f92672",
        success: "#a6e22e",
    },
    ThemePreset {
        name: "Nord",
        bg: "#2e3440",
        fg: "#eceff4",
        accent: "#88c0d0",
        muted: "#4c566a",
        error: "#bf616a",
        success: "#a3be8c",
    },
    ThemePreset {
        name: "Solarized",
        bg: "#002b36",
        fg: "#839496",
        accent: "#268bd2",
        muted: "#586e75",
        error: "#dc322f",
        success: "#859900",
    },
    ThemePreset {
        name: "Tokyo Night",
        bg: "#1a1b26",
        fg: "#c0caf5",
        accent: "#7aa2f7",
        muted: "#565f89",
        error: "#f7768e",
        success: "#9ece6a",
    },
];

#[derive(Deserialize)]
struct Config {
    bg: Option<String>,
    fg: Option<String>,
    accent: Option<String>,
    muted: Option<String>,
    error: Option<String>,
    success: Option<String>,
    warning: Option<String>,
    border: Option<String>,
    selection: Option<String>,
    /// When true, the background is left transparent: `bg` resolves to
    /// `Color::Reset` so the terminal's own background shows through. The
    /// `bg` hex is still kept on disk so unchecking restores it.
    transparent_bg: Option<bool>,
}

/// Raw values backing the Theme tab form — the hex strings as stored on
/// disk (never `Color::Reset`) plus the transparent-background flag.
pub struct ThemeFormValues {
    pub bg: String,
    pub fg: String,
    pub accent: String,
    pub muted: String,
    pub error: String,
    pub success: String,
    pub transparent_bg: bool,
}

fn theme_path() -> PathBuf {
    crate::config::path::config_dir().join("theme.toml")
}

/// Cached [`load_from_disk`]: the draw loop calls this every frame, so only
/// re-read + re-parse `theme.toml` when its mtime/size change (a Theme tab
/// save, or an external rewrite such as a config sync).
pub fn load() -> Theme {
    use std::sync::Mutex;
    use std::time::SystemTime;
    type Key = Option<(SystemTime, u64)>;
    static CACHE: Mutex<Option<(Key, Theme)>> = Mutex::new(None);

    let key: Key = fs::metadata(theme_path())
        .ok()
        .and_then(|m| Some((m.modified().ok()?, m.len())));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, t)) = cache.as_ref() {
        if *k == key {
            return t.clone();
        }
    }
    let theme = load_from_disk();
    *cache = Some((key, theme.clone()));
    theme
}

fn load_from_disk() -> Theme {
    let path = theme_path();

    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<Config>(&content) {
            let fallback = get_global_theme();
            return Theme {
                // Transparent background => Color::Reset, which makes ratatui
                // leave the terminal's native background untouched.
                bg: if cfg.transparent_bg.unwrap_or(false) {
                    Color::Reset
                } else {
                    cfg.bg
                        .as_ref()
                        .and_then(|v| hex_to_color(v))
                        .unwrap_or(fallback.bg)
                },
                fg: cfg
                    .fg
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.fg),
                accent: cfg
                    .accent
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.accent),
                muted: cfg
                    .muted
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.muted),
                error: cfg
                    .error
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.error),
                success: cfg
                    .success
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .unwrap_or(fallback.success),
                // Unset in an older theme.toml → inherit the role these used
                // to borrow, so nothing changes for an existing theme.
                warning: cfg
                    .warning
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .or_else(|| cfg.error.as_ref().and_then(|v| hex_to_color(v)))
                    .unwrap_or(fallback.warning),
                border: cfg
                    .border
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .or_else(|| cfg.muted.as_ref().and_then(|v| hex_to_color(v)))
                    .unwrap_or(fallback.border),
                selection: cfg
                    .selection
                    .as_ref()
                    .and_then(|v| hex_to_color(v))
                    .or_else(|| cfg.accent.as_ref().and_then(|v| hex_to_color(v)))
                    .unwrap_or(fallback.selection),
            };
        }
    }

    get_global_theme()
}

#[allow(clippy::too_many_arguments)]
pub fn save_theme(
    bg: &str,
    fg: &str,
    accent: &str,
    muted: &str,
    error: &str,
    success: &str,
    transparent_bg: bool,
) {
    let path = theme_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // The Theme tab edits six roles; `warning`, `border` and `selection` are
    // `theme.toml`-only. Carry whatever is on disk through, or saving from the
    // tab would silently drop a hand-written override.
    let existing: Option<Config> = fs::read_to_string(&path)
        .ok()
        .and_then(|c| toml::from_str(&c).ok());
    let mut content = format!(
        "bg = \"{}\"\nfg = \"{}\"\naccent = \"{}\"\nmuted = \"{}\"\nerror = \"{}\"\nsuccess = \"{}\"\ntransparent_bg = {}\n",
        bg, fg, accent, muted, error, success, transparent_bg
    );
    if let Some(prev) = existing {
        for (key, value) in [
            ("warning", prev.warning),
            ("border", prev.border),
            ("selection", prev.selection),
        ] {
            if let Some(v) = value {
                content.push_str(&format!("{key} = \"{v}\"\n"));
            }
        }
    }

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
        warning: Color::Rgb(222, 165, 74),
        border: Color::Rgb(150, 150, 150),
        selection: Color::Rgb(181, 189, 104),
    }
}

/// Read the raw values that back the Theme tab form: the six hex strings as
/// stored in `theme.toml` (falling back to the default theme when a key is
/// missing or invalid) plus the `transparent_bg` flag. Unlike [`load`], the
/// `bg` hex is returned verbatim even when transparency is on, so the form
/// can restore it when the user unchecks the box.
pub fn form_values() -> ThemeFormValues {
    let fallback = get_global_theme();
    let mut v = ThemeFormValues {
        bg: color_to_hex(fallback.bg),
        fg: color_to_hex(fallback.fg),
        accent: color_to_hex(fallback.accent),
        muted: color_to_hex(fallback.muted),
        error: color_to_hex(fallback.error),
        success: color_to_hex(fallback.success),
        transparent_bg: false,
    };
    if let Ok(content) = fs::read_to_string(theme_path()) {
        if let Ok(cfg) = toml::from_str::<Config>(&content) {
            let take = |s: Option<String>| s.filter(|h| hex_to_color(h).is_some());
            if let Some(s) = take(cfg.bg) {
                v.bg = s;
            }
            if let Some(s) = take(cfg.fg) {
                v.fg = s;
            }
            if let Some(s) = take(cfg.accent) {
                v.accent = s;
            }
            if let Some(s) = take(cfg.muted) {
                v.muted = s;
            }
            if let Some(s) = take(cfg.error) {
                v.error = s;
            }
            if let Some(s) = take(cfg.success) {
                v.success = s;
            }
            v.transparent_bg = cfg.transparent_bg.unwrap_or(false);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_src: &str) -> Config {
        toml::from_str(toml_src).expect("theme parses")
    }

    /// Resolve a parsed config the way `load()` does, without touching disk.
    fn resolve(cfg: &Config) -> Theme {
        let fallback = get_global_theme();
        Theme {
            bg: cfg
                .bg
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .unwrap_or(fallback.bg),
            fg: cfg
                .fg
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .unwrap_or(fallback.fg),
            accent: cfg
                .accent
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .unwrap_or(fallback.accent),
            muted: cfg
                .muted
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .unwrap_or(fallback.muted),
            error: cfg
                .error
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .unwrap_or(fallback.error),
            success: cfg
                .success
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .unwrap_or(fallback.success),
            warning: cfg
                .warning
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .or_else(|| cfg.error.as_ref().and_then(|v| hex_to_color(v)))
                .unwrap_or(fallback.warning),
            border: cfg
                .border
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .or_else(|| cfg.muted.as_ref().and_then(|v| hex_to_color(v)))
                .unwrap_or(fallback.border),
            selection: cfg
                .selection
                .as_ref()
                .and_then(|v| hex_to_color(v))
                .or_else(|| cfg.accent.as_ref().and_then(|v| hex_to_color(v)))
                .unwrap_or(fallback.selection),
        }
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex_to_color("#b5bd68"), Some(Color::Rgb(0xb5, 0xbd, 0x68)));
        assert_eq!(hex_to_color("b5bd68"), Some(Color::Rgb(0xb5, 0xbd, 0x68)));
        assert_eq!(color_to_hex(Color::Rgb(0xb5, 0xbd, 0x68)), "#b5bd68");
    }

    #[test]
    fn a_malformed_hex_is_rejected_rather_than_guessed() {
        for bad in ["#12345", "#1234567", "nothex", "", "#zzzzzz"] {
            assert_eq!(hex_to_color(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn a_theme_written_before_the_new_roles_looks_identical() {
        // The whole point of the fallbacks: an existing theme.toml must render
        // exactly as it did, with warning/border/selection inheriting the roles
        // they used to borrow.
        let cfg = parse(
            r##"
            bg = "#282828"
            fg = "#dcdccc"
            accent = "#b5bd68"
            muted = "#969696"
            error = "#cc6666"
            success = "#b5bd68"
            "##,
        );
        let t = resolve(&cfg);
        assert_eq!(t.warning, t.error, "warning used to borrow error");
        assert_eq!(t.border, t.muted, "borders used to be drawn in muted");
        assert_eq!(t.selection, t.accent, "the selected row used accent");
    }

    #[test]
    fn the_new_roles_win_when_declared() {
        let cfg = parse(
            r##"
            error = "#cc6666"
            muted = "#969696"
            accent = "#b5bd68"
            warning = "#dea54a"
            border = "#404040"
            selection = "#5f87af"
            "##,
        );
        let t = resolve(&cfg);
        assert_eq!(t.warning, Color::Rgb(0xde, 0xa5, 0x4a));
        assert_eq!(t.border, Color::Rgb(0x40, 0x40, 0x40));
        assert_eq!(t.selection, Color::Rgb(0x5f, 0x87, 0xaf));
        assert_ne!(t.warning, t.error, "a declared warning must not fall back");
    }

    #[test]
    fn an_empty_theme_falls_back_to_the_built_in() {
        let t = resolve(&parse(""));
        let g = get_global_theme();
        assert_eq!(t.bg, g.bg);
        assert_eq!(t.warning, g.warning);
        assert_eq!(t.border, g.border);
        assert_eq!(t.selection, g.selection);
    }

    #[test]
    fn a_garbage_value_falls_back_instead_of_breaking_the_theme() {
        let cfg = parse(r##"accent = "not-a-color""##);
        let t = resolve(&cfg);
        assert_eq!(t.accent, get_global_theme().accent);
        assert_eq!(
            t.selection,
            get_global_theme().selection,
            "and so does what derives from it"
        );
    }

    #[test]
    fn every_preset_resolves_all_nine_roles() {
        for p in PRESETS {
            let t = p.to_theme();
            assert_eq!(t.warning, t.error, "{}: warning inherits error", p.name);
            assert_eq!(t.border, t.muted, "{}: border inherits muted", p.name);
            assert_eq!(
                t.selection, t.accent,
                "{}: selection inherits accent",
                p.name
            );
        }
    }

    #[test]
    fn the_built_in_warning_is_not_just_the_error_colour() {
        // Otherwise the new role buys nothing out of the box.
        let g = get_global_theme();
        assert_ne!(g.warning, g.error);
    }
}
