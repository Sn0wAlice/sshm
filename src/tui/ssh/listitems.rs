use crate::tui::app::Row;
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::ListItem;

pub fn get_item_list<'a>(rows: &[Row]) -> Vec<ListItem<'a>> {
    rows.iter()
        .map(|r| match r {
            Row::Folder { name, collapsed } => {
                let icon = if *collapsed { "▸" } else { "▾" };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} {}", icon, name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
            Row::Host(h) => {
                let indent = if h.folder.is_some() { "    " } else { "" };
                ListItem::new(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled(
                        h.name.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("  {}", h.host)),
                ]))
            }
        })
        .collect()
}
