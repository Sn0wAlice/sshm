use crate::tui::app::{HostStatus, Row};
use crate::tui::functions::folder_depth;
use crate::tui::theme::Theme;
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::ListItem;
use std::collections::{HashMap, HashSet};

pub fn get_item_list<'a>(
    rows: &[Row],
    host_status: &HashMap<String, HostStatus>,
    selection: &HashSet<String>,
    theme: &Theme,
) -> Vec<ListItem<'a>> {
    rows.iter()
        .map(|r| match r {
            Row::Folder { name, collapsed } => {
                let icon = if *collapsed { "▸" } else { "▾" };
                let (display, is_tag) = if let Some(stripped) = name.strip_prefix("tag:") {
                    (stripped, true)
                } else if let Some(pos) = name.rfind('/') {
                    (&name[pos + 1..], false)
                } else {
                    (name.as_str(), false)
                };
                let depth = if is_tag { 0 } else { folder_depth(name) };
                let indent = "    ".repeat(depth);
                let glyph = if is_tag { "#" } else { icon };
                let style = if is_tag {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };
                ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(format!("{} {}", glyph, display), style),
                ]))
            }
            Row::Host(h) => {
                let depth = match &h.folder {
                    Some(f) => folder_depth(f) + 1,
                    None => 0,
                };
                let indent = "    ".repeat(depth);

                // Color based on reachability status (one lookup per row).
                let status = host_status.get(&h.name);
                let host_style = match status {
                    Some(HostStatus::Reachable { .. }) => Style::default().fg(theme.success),
                    Some(HostStatus::Unreachable) => Style::default().fg(theme.error),
                    None => Style::default(),
                };
                let name_style = host_style.add_modifier(Modifier::BOLD);

                let status_suffix: String = match status {
                    Some(HostStatus::Reachable {
                        latency_ms,
                        ssh_banner,
                    }) => {
                        let banner_mark = match ssh_banner {
                            Some(_) => " ssh",
                            None => " ?",
                        };
                        format!(" ● {}ms{}", latency_ms, banner_mark)
                    }
                    Some(HostStatus::Unreachable) => " ●".to_string(),
                    None => String::new(),
                };

                let mut spans = vec![Span::raw(indent)];
                if !selection.is_empty() {
                    let (mark, mark_style) = if selection.contains(&h.name) {
                        (
                            "[x] ",
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        ("[ ] ", Style::default().fg(theme.muted))
                    };
                    spans.push(Span::styled(mark, mark_style));
                }
                if h.favorite {
                    spans.push(Span::styled(
                        "★ ",
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                spans.push(Span::styled(h.name.clone(), name_style));
                spans.push(Span::styled(format!("  {}", h.host), host_style));

                if h.forward_agent {
                    spans.push(Span::styled(
                        "  -A",
                        Style::default()
                            .fg(theme.error)
                            .add_modifier(Modifier::BOLD),
                    ));
                }

                if !status_suffix.is_empty() {
                    spans.push(Span::styled(status_suffix, host_style));
                }

                ListItem::new(Line::from(spans))
            }
        })
        .collect()
}
