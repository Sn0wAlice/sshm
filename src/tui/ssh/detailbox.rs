use crate::models::{tags_to_string, Database};
use crate::tui::app::Row;
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn show_detail_box(last_rows_len: usize, selected: usize, rows: &[Row], f: &mut ratatui::Frame<'_>, hchunks: &[ratatui::layout::Rect], theme: &crate::tui::theme::Theme, db: &Database) {
    if let Some(sel) = (last_rows_len > 0).then_some(selected) {
        if let Some(row) = rows.get(sel) {
            match row {
                Row::Host(h) => {
                    let detail = format!(
                        "Name: {}\nUser: {}\nHost: {}\nPort: {}\nTags: {}\nIdentityFile: {}\nProxyJump: {}\nFolder: {}",
                        h.name,
                        h.username,
                        h.host,
                        h.port,
                        tags_to_string(&h.tags),
                        h.identity_file.as_deref().unwrap_or_default(),
                        h.proxy_jump.as_deref().unwrap_or_default(),
                        h.folder.as_deref().unwrap_or("-")
                    );
                    let p = Paragraph::new(detail).block(
                        Block::default()
                            .title("Details")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.accent))
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
