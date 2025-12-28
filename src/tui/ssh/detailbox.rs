use crate::models::{tags_to_string, Database};
use crate::tui::app::Row;
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn show_detail_box(last_rows_len: usize, selected: usize, rows: &Vec<Row>, f: &mut ratatui::Frame<'_>, hchunks: &[ratatui::layout::Rect], theme: &crate::tui::theme::Theme, db: &Database) {
    if let Some(sel) = (last_rows_len > 0).then(|| selected) {
        if let Some(row) = rows.get(sel) {
            match row {
                Row::Host(h) => {
                    let detail = format!(
                                    "Name: {}\nUser: {}\nHost: {}\nPort: {}\nTags: {}\nIdentityFile: {}\nProxyJump: {}\nFolder: {}\n\nPress 'f' for SFTP services",
                                    h.name,
                                    h.username,
                                    h.host,
                                    h.port,
                                    tags_to_string(&h.tags),
                                    h.identity_file.clone().unwrap_or_default(),
                                    h.proxy_jump.clone().unwrap_or_default(),
                                    h.folder.clone().unwrap_or_else(|| "-".to_string())
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
                Row::Folder(folder) => {
                    let count = db
                        .hosts
                        .values()
                        .filter(|h| h.folder.as_deref() == Some(folder.as_str()))
                        .count();

                    let detail = if folder == "All" {
                        format!(
                                        "Folder: All\nHosts: {}\n\nSelect a folder item or press Enter to open.",
                                        db.hosts.len()
                                    )
                    } else {
                        format!(
                            "Folder: {}\nHosts inside: {}\n\nPress Enter to view its hosts.",
                            folder, count
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
