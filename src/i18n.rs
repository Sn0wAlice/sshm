//! Lightweight runtime localization.
//!
//! Locales are bundled at compile time as TOML strings and parsed lazily on
//! first call. The active locale is picked once per process from the
//! `SSHM_LANG` environment variable (then `LANG`/`LC_ALL`), falling back to
//! English for any unknown code.
//!
//! Usage:
//!
//! ```ignore
//! use crate::t;
//! let msg = t!("toast.settings_saved");
//! let n = 3;
//! let msg = t!("toast.deleted_n", "n" => n);
//! ```
//!
//! Missing keys return the key itself prefixed with `??:` so they are easy
//! to spot in the UI rather than crashing.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Supported locale codes. `en` is the source of truth — every key must
/// exist in the English bundle.
const LOCALES: &[(&str, &str)] = &[
    ("en", include_str!("locales/en.toml")),
    ("fr", include_str!("locales/fr.toml")),
];

/// Internal: parsed bundle for the active locale, plus an `en` fallback.
struct Bundles {
    active: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

static BUNDLES: OnceLock<Bundles> = OnceLock::new();

fn parse_toml_strings(raw: &str) -> HashMap<String, String> {
    // Flatten `[section]` headers into `section.key = value` entries.
    let mut out = HashMap::new();
    let value: toml::Value = match toml::from_str(raw) {
        Ok(v) => v,
        Err(_) => return out,
    };
    fn walk(prefix: &str, value: &toml::Value, out: &mut HashMap<String, String>) {
        match value {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    let next = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    walk(&next, v, out);
                }
            }
            toml::Value::String(s) => {
                out.insert(prefix.to_string(), s.clone());
            }
            _ => {}
        }
    }
    walk("", &value, &mut out);
    out
}

fn detect_locale() -> &'static str {
    let raw = std::env::var("SSHM_LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    let lang_code = raw.split(['_', '.']).next().unwrap_or("");
    for (code, _) in LOCALES {
        if *code == lang_code {
            return *code;
        }
    }
    "en"
}

fn bundles() -> &'static Bundles {
    BUNDLES.get_or_init(|| {
        let active_code = detect_locale();
        let active = LOCALES
            .iter()
            .find(|(c, _)| *c == active_code)
            .map(|(_, raw)| parse_toml_strings(raw))
            .unwrap_or_default();
        let fallback = LOCALES
            .iter()
            .find(|(c, _)| *c == "en")
            .map(|(_, raw)| parse_toml_strings(raw))
            .unwrap_or_default();
        Bundles { active, fallback }
    })
}

/// Look up a translation key. Falls back to English, then to `??:key`.
pub fn lookup(key: &str) -> String {
    let b = bundles();
    if let Some(s) = b.active.get(key) {
        return s.clone();
    }
    if let Some(s) = b.fallback.get(key) {
        return s.clone();
    }
    format!("??:{}", key)
}

/// Substitute `{name}` placeholders in `template` using `(name, value)` pairs.
/// Unknown placeholders are left intact so they're easy to spot.
pub fn render(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in args {
        let needle = format!("{{{}}}", k);
        out = out.replace(&needle, v);
    }
    out
}

/// `t!("toast.saved")` → `lookup("toast.saved")`
/// `t!("toast.deleted_n", "n" => 3)` → templated lookup with substitution.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::lookup($key)
    };
    ($key:expr, $( $name:literal => $value:expr ),+ $(,)?) => {{
        let template = $crate::i18n::lookup($key);
        let bound: Vec<(&str, String)> = vec![ $(($name, format!("{}", $value))),+ ];
        let refs: Vec<(&str, &str)> = bound.iter().map(|(k, v)| (*k, v.as_str())).collect();
        $crate::i18n::render(&template, &refs)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flattens_sections() {
        let raw = "[toast]\nsaved = \"OK\"\n[other]\nx = \"y\"\n";
        let m = parse_toml_strings(raw);
        assert_eq!(m.get("toast.saved").unwrap(), "OK");
        assert_eq!(m.get("other.x").unwrap(), "y");
    }

    #[test]
    fn render_substitutes_placeholders() {
        let s = render("Hello {name}, you have {n} messages", &[("name", "Alice"), ("n", "3")]);
        assert_eq!(s, "Hello Alice, you have 3 messages");
    }

    #[test]
    fn render_leaves_unknown_placeholders() {
        let s = render("a = {b}", &[]);
        assert_eq!(s, "a = {b}");
    }

    #[test]
    fn lookup_falls_back_to_marker_for_unknown_key() {
        assert!(lookup("doesnotexist.foo").starts_with("??:"));
    }
}
