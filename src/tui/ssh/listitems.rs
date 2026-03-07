use std::collections::HashMap;
use crate::tui::app::{HostStatus, Row};
use crate::tui::functions::folder_depth;
use crate::tui::theme::Theme;
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::ListItem;

pub fn get_item_list<'a>(rows: &[Row], host_status: &HashMap<String, HostStatus>, theme: &Theme) -> Vec<ListItem<'a>> {
    rows.iter()
        .map(|r| match r {
            Row::Folder { name, collapsed } => {
                let icon = if *collapsed { "▸" } else { "▾" };
                let depth = folder_depth(name);
                let indent = "    ".repeat(depth);
                // Show only the leaf name for sub-folders
                let display = if let Some(pos) = name.rfind('/') {
                    &name[pos + 1..]
                } else {
                    name.as_str()
                };
                ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(
                        format!("{} {}", icon, display),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
            Row::Host(h) => {
                let depth = match &h.folder {
                    Some(f) => folder_depth(f) + 1,
                    None => 0,
                };
                let indent = "    ".repeat(depth);

                // Color based on reachability status
                let name_style = match host_status.get(&h.name) {
                    Some(HostStatus::Reachable) => {
                        Style::default().fg(theme.success).add_modifier(Modifier::BOLD)
                    }
                    Some(HostStatus::Unreachable) => {
                        Style::default().fg(theme.error).add_modifier(Modifier::BOLD)
                    }
                    None => Style::default().add_modifier(Modifier::BOLD),
                };

                let host_style = match host_status.get(&h.name) {
                    Some(HostStatus::Reachable) => Style::default().fg(theme.success),
                    Some(HostStatus::Unreachable) => Style::default().fg(theme.error),
                    None => Style::default(),
                };

                let status_icon = match host_status.get(&h.name) {
                    Some(HostStatus::Reachable) => " ●",
                    Some(HostStatus::Unreachable) => " ●",
                    None => "",
                };

                let mut spans = vec![
                    Span::raw(indent),
                    Span::styled(h.name.clone(), name_style),
                    Span::styled(format!("  {}", h.host), host_style),
                ];

                if !status_icon.is_empty() {
                    spans.push(Span::styled(
                        status_icon.to_string(),
                        match host_status.get(&h.name) {
                            Some(HostStatus::Reachable) => Style::default().fg(theme.success),
                            Some(HostStatus::Unreachable) => Style::default().fg(theme.error),
                            _ => Style::default(),
                        },
                    ));
                }

                ListItem::new(Line::from(spans))
            }
        })
        .collect()
}
