use crate::tui::app::Row;
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::ListItem;

pub fn get_item_list<'a>(rows: &Vec<Row>) -> Vec<ListItem<'a>> {
    let list_items: Vec<ListItem> = rows
        .iter()
        .map(|r| match r {
            Row::Folder(name) => {
                let label = if name == "All" {
                    "➤ All".to_string()
                } else {
                    format!("➤ {}", name)
                };
                ListItem::new(Line::from(vec![Span::raw(label)]))
            }
            Row::Host(h) => ListItem::new(Line::from(vec![
                Span::styled(
                    (*h).name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  {}", h.host)),
            ])),
        })
        .collect();
    list_items
}
