use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use crate::tui::theme::Theme;

pub struct ModalButton {
    pub label: String,
    pub is_selected: bool,
}

pub struct ModalConfig {
    pub title: String,
    pub body_lines: Vec<String>,
    pub buttons: Vec<ModalButton>,
    pub width_percent: u16,
    pub height_percent: u16,
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1]);

    horizontal[1]
}

pub fn render_modal(
    f: &mut ratatui::Frame,
    size: Rect,
    config: &ModalConfig,
    theme: &Theme,
) {
    let area = centered_rect(config.width_percent, config.height_percent, size);

    // Shadow effect: dark rect offset by 1,1
    let shadow_area = Rect {
        x: (area.x + 1).min(size.width.saturating_sub(1)),
        y: (area.y + 1).min(size.height.saturating_sub(1)),
        width: area.width.min(size.width.saturating_sub(area.x + 1)),
        height: area.height.min(size.height.saturating_sub(area.y + 1)),
    };
    f.render_widget(
        Block::default().style(Style::default().bg(ratatui::prelude::Color::Rgb(20, 20, 20))),
        shadow_area,
    );

    // Clear main area and draw bordered block
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", config.title),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Body text
    let body: Vec<Line> = config.body_lines.iter()
        .map(|l| Line::from(l.clone()))
        .collect();
    let msg = Paragraph::new(body).alignment(Alignment::Center);
    f.render_widget(msg, inner);

    // Buttons at bottom
    let buttons_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(2),
        width: inner.width,
        height: 1,
    };

    let mut button_spans: Vec<Span> = Vec::new();
    for (i, btn) in config.buttons.iter().enumerate() {
        if i > 0 { button_spans.push(Span::raw("   ")); }
        let span = if btn.is_selected {
            Span::styled(
                format!("[ {} ]", btn.label),
                Style::default().bg(theme.accent).fg(theme.bg).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!("[ {} ]", btn.label), Style::default().fg(theme.accent))
        };
        button_spans.push(span);
    }

    let buttons = Paragraph::new(Line::from(button_spans)).alignment(Alignment::Center);
    f.render_widget(buttons, buttons_area);
}
