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
    /// Frecency = recency × frequency (Mozilla-style). Reorders so that
    /// hosts you often *and* recently use bubble to the top.
    Frecency,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Name => SortMode::Mru,
            SortMode::Mru => SortMode::MostUsed,
            SortMode::MostUsed => SortMode::Favorites,
            SortMode::Favorites => SortMode::Frecency,
            SortMode::Frecency => SortMode::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::Mru => "most recently used",
            SortMode::MostUsed => "most used",
            SortMode::Favorites => "favorites first",
            SortMode::Frecency => "frecency (recent × frequent)",
        }
    }
}

/// Compute a frecency score for a host: a higher value means "more interesting".
/// Inputs: connection count, hours since last use (None ⇒ never).
/// Heuristic: score = use_count / (1 + log10(hours_since)). Hosts never used
/// score 0 (so they sink to the bottom).
pub fn frecency_score(use_count: u32, last_connected_at: Option<&str>) -> f64 {
    if use_count == 0 { return 0.0; }
    let Some(stamp) = last_connected_at else { return 0.0; };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(stamp) else { return 0.0; };
    let hours = chrono::Utc::now()
        .signed_duration_since(parsed.with_timezone(&chrono::Utc))
        .num_minutes()
        .max(1) as f64
        / 60.0;
    let decay = 1.0 + (hours.max(1.0)).log10();
    (use_count as f64) / decay
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
        SortMode::Frecency => {
            items.sort_by(|a, b| {
                let sa = frecency_score(a.use_count, a.last_connected_at.as_deref());
                let sb = frecency_score(b.use_count, b.last_connected_at.as_deref());
                sb.partial_cmp(&sa)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::frecency_score;

    #[test]
    fn never_used_scores_zero() {
        assert_eq!(frecency_score(0, None), 0.0);
        assert_eq!(frecency_score(5, None), 0.0);
    }

    #[test]
    fn frequency_dominates_when_recency_equal() {
        let now = chrono::Utc::now().to_rfc3339();
        let a = frecency_score(10, Some(&now));
        let b = frecency_score(2, Some(&now));
        assert!(a > b);
    }

    #[test]
    fn recency_dominates_when_frequency_equal() {
        let now = chrono::Utc::now().to_rfc3339();
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let recent = frecency_score(5, Some(&now));
        let old = frecency_score(5, Some(&week_ago));
        assert!(recent > old);
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
