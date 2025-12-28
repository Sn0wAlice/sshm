use ratatui::prelude::Style;
use ratatui::widgets::{Block, Borders, ListState, Paragraph};
use crate::tui::app::Row;
use crate::tui::theme::Theme;

/// Generate help box content based on the selected row type
pub fn get_help_box_content(list_state: &ListState, rows_help: &Vec<Row>, theme: &Theme) -> Paragraph<'static> {
    let help_text = if let Some(sel) = list_state.selected() {
        match rows_help.get(sel) {
            Some(Row::Host(_)) => {
                "Shortcuts:  ↑/↓ move • Enter open/connect • a add • e edit • i add identity • d delete • q quit\n\
                             Notes: '/' to start filter, Enter to finish; folders shown when filter is empty."
            }
            Some(Row::Folder(_)) => {
                "Shortcuts:  ↑/↓ move • Enter open folder • a add • r rename • q quit\n\
                             Notes: '/' to start filter, Enter to finish; folders shown when filter is empty."
            }
            None => {
                "Shortcuts:  ↑/↓ move • q quit"
            }
        }
    } else {
        "Shortcuts:  ↑/↓ move • q quit"
    };

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title("Help")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.muted))
        );
    help
}