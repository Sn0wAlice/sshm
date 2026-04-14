use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use crate::tui::theme::Theme;

pub fn draw_tab_bar(f: &mut Frame, area: Rect, active_index: usize, theme: &Theme) {
    let titles = ["Hosts", "Identities", "Settings", "Theme", "Help"];
    let divider = " │ ";

    // Build the tab label portion: " Hosts │ Settings │ Theme "
    let mut tab_spans: Vec<Span> = Vec::new();
    tab_spans.push(Span::raw(" "));
    for (i, title) in titles.iter().enumerate() {
        if i > 0 {
            tab_spans.push(Span::styled(divider, Style::default().fg(theme.muted)));
        }
        let style = if i == active_index {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        tab_spans.push(Span::styled(*title, style));
    }
    tab_spans.push(Span::raw(" "));

    // Calculate the text width of the tab portion
    let tab_width: usize = tab_spans.iter().map(|s| s.width()).sum();
    let total_width = area.width as usize;

    // Fill remaining space with ─
    let left_pad = total_width.saturating_sub(tab_width) / 2;
    let right_pad = total_width.saturating_sub(tab_width).saturating_sub(left_pad);

    let mut spans: Vec<Span> = Vec::new();
    if left_pad > 0 {
        spans.push(Span::styled("─".repeat(left_pad), Style::default().fg(theme.muted)));
    }
    spans.extend(tab_spans);
    if right_pad > 0 {
        spans.push(Span::styled("─".repeat(right_pad), Style::default().fg(theme.muted)));
    }

    let line = Line::from(spans);
    let p = Paragraph::new(line).style(Style::default().bg(theme.bg));
    f.render_widget(p, area);
}
