use std::time::Instant;
use ratatui::layout::Rect;
use ratatui::prelude::{Color, Line, Modifier, Span, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use crate::tui::theme::Theme;

#[derive(Clone, Copy, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
}

pub struct Toast {
    pub message: String,
    pub created: Instant,
    pub kind: ToastKind,
}

impl Toast {
    pub fn success(message: impl Into<String>) -> Self {
        Toast { message: message.into(), created: Instant::now(), kind: ToastKind::Success }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Toast { message: message.into(), created: Instant::now(), kind: ToastKind::Error }
    }

    pub fn is_expired(&self) -> bool {
        self.created.elapsed().as_secs() >= 3
    }
}

pub fn render_toast(f: &mut ratatui::Frame, screen: Rect, toast: &Toast, theme: &Theme) {
    let msg_width = (toast.message.len() as u16 + 6).min(50).max(12);
    let toast_height: u16 = 3;

    let x = screen.width.saturating_sub(msg_width + 1);
    let y = screen.height.saturating_sub(toast_height + 2);

    let area = Rect { x, y, width: msg_width, height: toast_height };

    let border_color = match toast.kind {
        ToastKind::Success => Color::Rgb(100, 200, 100),
        ToastKind::Error => Color::Rgb(220, 80, 80),
    };

    let icon = match toast.kind {
        ToastKind::Success => "✓ ",
        ToastKind::Error => "✗ ",
    };

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.bg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = Paragraph::new(Line::from(vec![
        Span::styled(icon.to_string(), Style::default().fg(border_color).add_modifier(Modifier::BOLD)),
        Span::styled(toast.message.clone(), Style::default().fg(theme.fg)),
    ]));
    f.render_widget(text, inner);
}
