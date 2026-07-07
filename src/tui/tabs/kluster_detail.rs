//! Rich detail overlay for the Kluster tab — a scrollable, sectioned inspect
//! view (Overview / Networking / Ports / Volumes / Command + a log tail),
//! rendered on top of the container list. Data comes from
//! [`crate::tui::app::kluster_actions::build_kluster_detail`].

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::kluster::ContainerDetail;
use crate::tui::ssh::modal::centered_rect;
use crate::tui::theme::Theme;

/// Blank lines appended after the content so scrolling to the bottom makes it
/// obvious you've reached the end of the popup.
const TRAILING_BLANKS: usize = 3;

/// Flatten a [`ContainerDetail`] into styled lines: section headers, aligned
/// `label → value` rows, then a Logs section with the captured tail.
fn detail_lines(detail: &ContainerDetail, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    // Widest label across all sections, so values line up in one column.
    let label_w = detail
        .sections
        .iter()
        .flat_map(|s| s.rows.iter())
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0)
        .min(22);

    for (i, sec) in detail.sections.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            format!("▸ {}", sec.title),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )));
        for (k, v) in &sec.rows {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<width$}  ", k, width = label_w),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(v.clone(), Style::default().fg(theme.fg)),
            ]));
        }
    }

    if !detail.log_tail.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("▸ Logs (last {})", detail.log_tail.len()),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )));
        for l in &detail.log_tail {
            lines.push(Line::from(Span::styled(
                format!("  {}", l),
                Style::default().fg(theme.muted),
            )));
        }
    }
    for _ in 0..TRAILING_BLANKS {
        lines.push(Line::from(""));
    }
    lines
}

/// Total number of rendered lines — used by the caller to clamp scrolling.
/// Computed structurally so it needs no [`Theme`] (styling doesn't change the
/// count), keeping it callable from the event loop where no theme is in scope.
pub fn detail_line_count(detail: &ContainerDetail) -> usize {
    let mut n = 0usize;
    for (i, sec) in detail.sections.iter().enumerate() {
        if i > 0 {
            n += 1; // blank spacer between sections
        }
        n += 1 + sec.rows.len(); // header + rows
    }
    if !detail.log_tail.is_empty() {
        n += 2 + detail.log_tail.len(); // blank + header + lines
    }
    n + TRAILING_BLANKS
}

pub fn draw_kluster_detail(
    f: &mut Frame,
    detail: &ContainerDetail,
    scroll: &mut usize,
    theme: &Theme,
) {
    let area = centered_rect(72, 82, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", detail.title),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " ↑↓/jk scroll · Esc close ",
            Style::default().fg(theme.muted),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = detail_lines(detail, theme);
    let total = lines.len();
    let visible = inner.height as usize;
    let max_scroll = total.saturating_sub(visible);
    // Clamp the *caller's* scroll to what's actually reachable, so the event
    // loop can increment freely without accumulating an invisible offset past
    // the last page (which would make Up appear to do nothing for a while).
    *scroll = (*scroll).min(max_scroll);
    let scroll = *scroll;

    f.render_widget(
        Paragraph::new(lines).scroll((scroll as u16, 0)),
        inner,
    );

    if total > visible {
        let mut sb = ScrollbarState::new(total).position(scroll);
        f.render_stateful_widget(
            Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb,
        );
    }
}
