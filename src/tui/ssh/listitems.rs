use crate::tui::app::Row;
use crate::tui::functions::folder_depth;
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::ListItem;

pub fn get_item_list<'a>(rows: &[Row]) -> Vec<ListItem<'a>> {
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
                ListItem::new(Line::from(vec![
                    Span::raw(indent),
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
