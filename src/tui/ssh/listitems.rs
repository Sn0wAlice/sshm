use crate::tui::app::Row;
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::ListItem;

pub fn get_item_list<'a>(rows: &[Row]) -> Vec<ListItem<'a>> {
    rows.iter()
        .map(|r| match r {
            Row::Folder(name) => {
                ListItem::new(Line::from(vec![Span::raw(format!("➤ {}", name))]))
            }
            Row::Host(h) => ListItem::new(Line::from(vec![
                Span::styled(
                    h.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  {}", h.host)),
            ])),
        })
        .collect()
}
