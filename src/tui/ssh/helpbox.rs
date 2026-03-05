use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::Paragraph;
use crate::tui::theme::Theme;

pub enum HelpContext {
    HostNav,
    FolderNav,
    FilterMode,
    DeleteModal,
    SettingsTab,
    ThemeTab,
    Empty,
}

pub fn get_contextual_help(ctx: HelpContext, theme: &Theme) -> Paragraph<'static> {
    let text = match ctx {
        HelpContext::HostNav => {
            "↑↓ move │ Enter connect │ / filter │ a add │ e edit │ d delete │ p forward │ i identity │ q quit"
        }
        HelpContext::FolderNav => {
            "↑↓ move │ Enter expand/collapse │ / filter │ a add │ r rename │ d delete │ q quit"
        }
        HelpContext::FilterMode => {
            "Type to filter (fuzzy) │ Esc clear │ Enter confirm"
        }
        HelpContext::DeleteModal => {
            "←→ select │ Enter confirm │ Esc cancel"
        }
        HelpContext::SettingsTab => {
            "↑↓ navigate │ Type to edit │ Enter save │ ←→ tab │ Esc reset"
        }
        HelpContext::ThemeTab => {
            "↑↓ navigate │ Enter apply/save │ ←→ tab │ Esc reset"
        }
        HelpContext::Empty => {
            "a add host │ q quit │ ←→ tab"
        }
    };

    let spans = parse_help_spans(text, theme);
    Paragraph::new(Line::from(spans))
        .style(Style::default().bg(theme.bg))
}

fn parse_help_spans(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, segment) in text.split(" │ ").enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme.muted)));
        }
        if let Some(space_idx) = segment.find(' ') {
            let key = &segment[..space_idx];
            let desc = &segment[space_idx..];
            spans.push(Span::styled(
                key.to_string(),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(desc.to_string(), Style::default().fg(theme.muted)));
        } else {
            spans.push(Span::styled(segment.to_string(), Style::default().fg(theme.accent)));
        }
    }
    spans
}
