use std::collections::HashMap;
use crate::models::{tags_to_string, Database};
use crate::tui::app::{HostStatus, Row};
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

#[allow(clippy::too_many_arguments)]
pub fn show_detail_box(last_rows_len: usize, selected: usize, rows: &[Row], f: &mut ratatui::Frame<'_>, hchunks: &[ratatui::layout::Rect], theme: &crate::tui::theme::Theme, db: &Database, host_status: &HashMap<String, HostStatus>) {
    if let Some(sel) = (last_rows_len > 0).then_some(selected) {
        if let Some(row) = rows.get(sel) {
            match row {
                Row::Host(h) => {
                    let status_line: String = match host_status.get(&h.name) {
                        Some(HostStatus::Reachable { latency_ms, ssh_banner }) => {
                            match ssh_banner {
                                Some(b) => format!("Status: ● reachable ({} ms) — {}", latency_ms, b),
                                None => format!("Status: ● reachable ({} ms) — no SSH banner", latency_ms),
                            }
                        }
                        Some(HostStatus::Unreachable) => "Status: ● unreachable".to_string(),
                        None => "Status: — not checked".to_string(),
                    };
                    let name_display = if h.favorite {
                        format!("★ {}", h.name)
                    } else {
                        h.name.clone()
                    };
                    let last_used = crate::history::format_last_used(h.last_connected_at.as_deref());
                    let fa_line = if h.forward_agent {
                        "ForwardAgent: ⚠ ON (-A)\n"
                    } else {
                        ""
                    };
                    let tunnels_line = if h.tunnels.is_empty() {
                        String::new()
                    } else {
                        let kinds: Vec<String> = h.tunnels.iter()
                            .map(|t| t.kind.short().to_string())
                            .collect();
                        format!("Tunnels: {} saved [{}]\n", h.tunnels.len(), kinds.join(", "))
                    };
                    let frecency = crate::history::frecency_score(
                        h.use_count,
                        h.last_connected_at.as_deref(),
                    );
                    let detail = format!(
                        "Name: {}\nUser: {}\nHost: {}\nPort: {}\nTags: {}\nIdentityFile: {}\nProxyJump: {}\nFolder: {}\n{}{}Last used: {}\nUses: {}\nFrecency: {:.2}\n{}",
                        name_display,
                        h.username,
                        h.host,
                        h.port,
                        tags_to_string(&h.tags),
                        h.identity_file.as_deref().unwrap_or_default(),
                        h.proxy_jump.as_deref().unwrap_or_default(),
                        h.folder.as_deref().unwrap_or("-"),
                        fa_line,
                        tunnels_line,
                        last_used,
                        h.use_count,
                        frecency,
                        status_line,
                    );

                    let border_color = match host_status.get(&h.name) {
                        Some(HostStatus::Reachable { .. }) => theme.success,
                        Some(HostStatus::Unreachable) => theme.error,
                        None => theme.accent,
                    };

                    let p = Paragraph::new(detail).block(
                        Block::default()
                            .title("Details")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(border_color))
                            .style(Style::default().bg(theme.bg).fg(theme.fg)),
                    );
                    f.render_widget(p, hchunks[1]);
                }
                Row::Folder { name, collapsed } => {
                    // Count direct hosts + hosts in sub-folders
                    let prefix = format!("{}/", name);
                    let count = db
                        .hosts
                        .values()
                        .filter(|h| {
                            if let Some(ref f) = h.folder {
                                f == name || f.starts_with(&prefix)
                            } else {
                                false
                            }
                        })
                        .count();

                    let sub_count = db.folders.iter()
                        .filter(|f| f.starts_with(&prefix))
                        .count();

                    let state_text = if *collapsed { "collapsed" } else { "expanded" };
                    let detail = if sub_count > 0 {
                        format!(
                            "Folder: {}\nSub-folders: {}\nHosts (total): {}\nState: {}\n\nEnter to expand/collapse.",
                            name, sub_count, count, state_text
                        )
                    } else {
                        format!(
                            "Folder: {}\nHosts inside: {}\nState: {}\n\nEnter to expand/collapse.",
                            name, count, state_text
                        )
                    };

                    let p = Paragraph::new(detail).block(
                        Block::default()
                            .title("Folder Details")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.accent))
                            .style(Style::default().bg(theme.bg).fg(theme.muted)),
                    );
                    f.render_widget(p, hchunks[1]);
                }
            }
        }
    }
}
