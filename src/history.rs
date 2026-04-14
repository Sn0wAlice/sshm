//! Connection history + sort modes.
//!
//! Provides:
//! - [`record_connection`] to bump `use_count` and stamp `last_connected_at`
//!   on a host after a successful launch of the `ssh` command.
//! - [`SortMode`] + [`sort_items`] to re-order the TUI host list by name,
//!   most-recently-used, most-used, or favorites-first.
//! - [`format_last_used`] to render `last_connected_at` as a compact
//!   human-readable relative string for the detail box.

use crate::models::Host;

/// Update a host's history after a (successful) connection attempt.
pub fn record_connection(host: &mut Host) {
    host.last_connected_at = Some(chrono::Utc::now().to_rfc3339());
    host.use_count = host.use_count.saturating_add(1);
}

/// TUI sort mode for the host list. Cycled by the `s` hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Mru,
    MostUsed,
    Favorites,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Name => SortMode::Mru,
            SortMode::Mru => SortMode::MostUsed,
            SortMode::MostUsed => SortMode::Favorites,
            SortMode::Favorites => SortMode::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::Mru => "most recently used",
            SortMode::MostUsed => "most used",
            SortMode::Favorites => "favorites first",
        }
    }
}

/// Re-sort `items` in place according to the chosen [`SortMode`].
///
/// For MRU/MostUsed, hosts with no history fall to the bottom and are
/// alphabetized among themselves for a stable display.
pub fn sort_items(items: &mut [&Host], mode: SortMode) {
    match mode {
        SortMode::Name => {
            items.sort_by(|a, b| a.name.cmp(&b.name));
        }
        SortMode::Mru => {
            items.sort_by(|a, b| {
                // Descending on last_connected_at, None last, tie-break on name.
                match (&a.last_connected_at, &b.last_connected_at) {
                    (Some(x), Some(y)) => y.cmp(x).then_with(|| a.name.cmp(&b.name)),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.name.cmp(&b.name),
                }
            });
        }
        SortMode::MostUsed => {
            items.sort_by(|a, b| {
                b.use_count
                    .cmp(&a.use_count)
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
        SortMode::Favorites => {
            items.sort_by(|a, b| {
                // Favorites first, then by name.
                b.favorite
                    .cmp(&a.favorite)
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
    }
}

/// Human-friendly relative formatting of `last_connected_at`.
/// Returns `"never"` if the field is missing or unparsable.
pub fn format_last_used(stamp: Option<&str>) -> String {
    let Some(s) = stamp else { return "never".to_string() };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(s) else {
        return "never".to_string();
    };
    let now = chrono::Utc::now();
    let delta = now.signed_duration_since(parsed.with_timezone(&chrono::Utc));
    let secs = delta.num_seconds();
    let rel = if secs < 5 {
        "just now".to_string()
    } else if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 86_400 * 30 {
        format!("{}d ago", secs / 86_400)
    } else {
        format!("{}mo ago", secs / (86_400 * 30))
    };
    let absolute = parsed
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%d %H:%M UTC");
    format!("{} ({})", rel, absolute)
}
