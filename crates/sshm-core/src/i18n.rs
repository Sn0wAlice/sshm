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
                    let next = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
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
    for &(code, _) in LOCALES {
        if code == lang_code {
            return code;
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
        let s = render(
            "Hello {name}, you have {n} messages",
            &[("name", "Alice"), ("n", "3")],
        );
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

    // ---- bundle integrity -------------------------------------------------
    //
    // `en` is the source of truth. These walk the bundled TOML directly rather
    // than going through `bundles()`, which caches one locale per process.

    fn bundle(code: &str) -> HashMap<String, String> {
        let raw = LOCALES
            .iter()
            .find(|(c, _)| *c == code)
            .expect("locale exists")
            .1;
        let parsed = parse_toml_strings(raw);
        assert!(
            !parsed.is_empty(),
            "{code}.toml parsed to nothing — syntax error?"
        );
        parsed
    }

    #[test]
    fn every_locale_parses() {
        for (code, _) in LOCALES {
            bundle(code);
        }
    }

    #[test]
    fn no_locale_is_missing_a_key() {
        let en = bundle("en");
        for (code, _) in LOCALES.iter().filter(|(c, _)| *c != "en") {
            let other = bundle(code);
            let mut missing: Vec<&String> = en.keys().filter(|k| !other.contains_key(*k)).collect();
            missing.sort();
            assert!(missing.is_empty(), "{code}.toml is missing: {missing:?}");
        }
    }

    #[test]
    fn no_locale_has_a_key_english_does_not() {
        // A stray key is a typo or a leftover: it can never be reached, since
        // lookups are driven by what the code asks for.
        let en = bundle("en");
        for (code, _) in LOCALES.iter().filter(|(c, _)| *c != "en") {
            let other = bundle(code);
            let mut extra: Vec<&String> = other.keys().filter(|k| !en.contains_key(*k)).collect();
            extra.sort();
            assert!(
                extra.is_empty(),
                "{code}.toml has keys en.toml does not: {extra:?}"
            );
        }
    }

    #[test]
    fn placeholders_match_across_locales() {
        // A translation that drops `{n}` silently loses the number; one that
        // invents `{count}` renders the literal braces to the user.
        fn placeholders(s: &str) -> Vec<String> {
            let mut out = Vec::new();
            let mut rest = s;
            while let Some(i) = rest.find('{') {
                let Some(j) = rest[i..].find('}') else { break };
                out.push(rest[i..=i + j].to_string());
                rest = &rest[i + j + 1..];
            }
            out.sort();
            out
        }
        let en = bundle("en");
        for (code, _) in LOCALES.iter().filter(|(c, _)| *c != "en") {
            let other = bundle(code);
            for (key, en_val) in &en {
                let Some(tr) = other.get(key) else { continue };
                assert_eq!(
                    placeholders(en_val),
                    placeholders(tr),
                    "{code}.toml `{key}` has different placeholders"
                );
            }
        }
    }

    #[test]
    fn help_strings_keep_their_segment_count() {
        // The footer truncates on ` │ ` boundaries and the popup splits on
        // them, so a translation that loses a separator loses a shortcut.
        let en = bundle("en");
        for (code, _) in LOCALES.iter().filter(|(c, _)| *c != "en") {
            let other = bundle(code);
            for (key, en_val) in en
                .iter()
                .filter(|(k, _)| k.starts_with("help.") && !k.starts_with("help.title."))
            {
                let Some(tr) = other.get(key) else { continue };
                assert_eq!(
                    en_val.split(" │ ").count(),
                    tr.split(" │ ").count(),
                    "{code}.toml `{key}` has a different number of shortcuts"
                );
            }
        }
    }

    #[test]
    fn no_translation_is_left_empty() {
        for (code, _) in LOCALES {
            for (key, val) in bundle(code) {
                assert!(!val.trim().is_empty(), "{code}.toml `{key}` is empty");
            }
        }
    }

    #[test]
    fn form_labels_fit_a_narrow_terminal() {
        // Form rows render inside a modal sized to 70% of the terminal. On an
        // 80-column terminal that is 56 columns, ~50 once borders and margins
        // are taken — and a longer string is silently clipped, not wrapped.
        // A French label overflowing here is exactly how this limit was found.
        const BUDGET: usize = 50;
        for (code, _) in LOCALES {
            for (key, val) in bundle(code) {
                if !key.starts_with("form.") {
                    continue;
                }
                let width = val.chars().count();
                assert!(
                    width <= BUDGET,
                    "{code}.toml `{key}` is {width} chars, over the {BUDGET}-column form budget: {val:?}"
                );
            }
        }
    }
}
