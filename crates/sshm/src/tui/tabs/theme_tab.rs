use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use crate::tui::theme::{Theme, PRESETS, hex_to_color, form_values};

/// Fields layout:
/// 0..PRESETS.len()-1  = preset items
/// PRESETS.len()       = "Custom Colors" separator (skipped on Enter)
/// PRESETS.len()+1..+6 = custom hex fields (bg, fg, accent, muted, error, success)
/// PRESETS.len()+7     = Transparent-background checkbox
/// PRESETS.len()+8     = Save Custom button
pub struct ThemeTabState {
    pub selected_field: usize,
    pub custom_bg: String,
    pub custom_fg: String,
    pub custom_accent: String,
    pub custom_muted: String,
    pub custom_error: String,
    pub custom_success: String,
    /// When true the background is left transparent (terminal-native) and the
    /// `custom_bg` hex is ignored at render time — but still kept so the user
    /// can untick the box and get their colour back.
    pub transparent_bg: bool,
    pub dirty: bool,
}

impl ThemeTabState {
    pub fn new() -> Self {
        let v = form_values();
        ThemeTabState {
            selected_field: 0,
            custom_bg: v.bg,
            custom_fg: v.fg,
            custom_accent: v.accent,
            custom_muted: v.muted,
            custom_error: v.error,
            custom_success: v.success,
            transparent_bg: v.transparent_bg,
            dirty: false,
        }
    }

    fn total_fields() -> usize {
        // presets + separator + 6 colours + transparent checkbox + save
        PRESETS.len() + 1 + 6 + 1 + 1
    }

    fn separator_index() -> usize { PRESETS.len() }
    fn custom_start() -> usize { PRESETS.len() + 1 }
    pub fn transparent_index() -> usize { PRESETS.len() + 7 }
    fn save_index() -> usize { PRESETS.len() + 8 }

    pub fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % Self::total_fields();
        if self.selected_field == Self::separator_index() {
            self.selected_field += 1;
        }
    }

    pub fn prev_field(&mut self) {
        if self.selected_field == 0 {
            self.selected_field = Self::total_fields() - 1;
        } else {
            self.selected_field -= 1;
        }
        if self.selected_field == Self::separator_index() {
            if self.selected_field == 0 {
                self.selected_field = Self::total_fields() - 1;
            } else {
                self.selected_field -= 1;
            }
        }
    }

    pub fn is_on_preset(&self) -> bool {
        self.selected_field < PRESETS.len()
    }

    pub fn is_editing_custom_field(&self) -> bool {
        let start = Self::custom_start();
        self.dirty && self.selected_field >= start && self.selected_field < start + 6
    }

    fn active_custom_mut(&mut self) -> Option<&mut String> {
        let start = Self::custom_start();
        match self.selected_field.checked_sub(start) {
            Some(0) => Some(&mut self.custom_bg),
            Some(1) => Some(&mut self.custom_fg),
            Some(2) => Some(&mut self.custom_accent),
            Some(3) => Some(&mut self.custom_muted),
            Some(4) => Some(&mut self.custom_error),
            Some(5) => Some(&mut self.custom_success),
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(field) = self.active_custom_mut() {
            field.push(c);
            self.dirty = true;
        }
    }

    pub fn pop_char(&mut self) {
        if let Some(field) = self.active_custom_mut() {
            field.pop();
            self.dirty = true;
        }
    }
}

impl Default for ThemeTabState {
    fn default() -> Self {
        Self::new()
    }
}

pub enum ThemeAction {
    None,
    ApplyPreset(usize),
    SaveCustom,
}

pub fn handle_theme_event(key: KeyCode, state: &mut ThemeTabState) -> ThemeAction {
    match key {
        KeyCode::Down | KeyCode::Tab => { state.next_field(); ThemeAction::None }
        KeyCode::Up | KeyCode::BackTab => { state.prev_field(); ThemeAction::None }
        KeyCode::Enter => {
            if state.is_on_preset() {
                ThemeAction::ApplyPreset(state.selected_field)
            } else if state.selected_field == ThemeTabState::save_index() {
                ThemeAction::SaveCustom
            } else if state.selected_field == ThemeTabState::transparent_index() {
                state.transparent_bg = !state.transparent_bg;
                state.dirty = true;
                ThemeAction::None
            } else {
                state.next_field();
                ThemeAction::None
            }
        }
        KeyCode::Char(' ') if state.selected_field == ThemeTabState::transparent_index() => {
            state.transparent_bg = !state.transparent_bg;
            state.dirty = true;
            ThemeAction::None
        }
        KeyCode::Char(c) => {
            if state.is_editing_custom_field() {
                state.push_char(c);
            }
            ThemeAction::None
        }
        KeyCode::Backspace => {
            if state.is_editing_custom_field() {
                state.pop_char();
            }
            ThemeAction::None
        }
        _ => ThemeAction::None,
    }
}

pub fn draw_theme_tab(f: &mut Frame, area: Rect, state: &ThemeTabState, theme: &Theme) {
    let block = Block::default()
        .title("Theme")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut constraints: Vec<Constraint> = Vec::new();
    constraints.push(Constraint::Length(1)); // Presets header
    for _ in PRESETS {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(2)); // Separator
    for _ in 0..6 {
        constraints.push(Constraint::Length(1)); // Custom fields
    }
    constraints.push(Constraint::Length(1)); // Transparent checkbox
    constraints.push(Constraint::Length(2)); // Save button
    constraints.push(Constraint::Min(0));    // Spacer

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
        .split(inner);

    // --- Presets header ---
    let header = Paragraph::new("  Presets (Enter to apply)")
        .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
    f.render_widget(header, chunks[0]);

    // --- Preset items ---
    for (i, preset) in PRESETS.iter().enumerate() {
        let is_sel = state.selected_field == i;
        let marker = if is_sel { "> " } else { "  " };
        let preview_theme = preset.to_theme();

        let line = Line::from(vec![
            Span::styled(
                format!("{}  {:<14}", marker, preset.name),
                if is_sel {
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg)
                },
            ),
            Span::styled("  ", Style::default().bg(preview_theme.bg)),
            Span::styled("  ", Style::default().bg(preview_theme.accent)),
            Span::styled("  ", Style::default().bg(preview_theme.fg)),
            Span::styled("  ", Style::default().bg(preview_theme.muted)),
        ]);

        f.render_widget(Paragraph::new(line), chunks[1 + i]);
    }

    // --- Custom separator ---
    let sep_chunk_idx = 1 + PRESETS.len();
    let sep = Paragraph::new("\n  Custom Colors")
        .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
    f.render_widget(sep, chunks[sep_chunk_idx]);

    // --- Custom fields ---
    let custom_labels = ["Background", "Foreground", "Accent", "Muted", "Error", "Success"];
    let custom_values = [&state.custom_bg, &state.custom_fg, &state.custom_accent, &state.custom_muted, &state.custom_error, &state.custom_success];
    let custom_chunk_start = sep_chunk_idx + 1;

    for (i, (label, value)) in custom_labels.iter().zip(custom_values.iter()).enumerate() {
        let field_idx = ThemeTabState::custom_start() + i;
        let is_sel = state.selected_field == field_idx;
        let cursor = if is_sel { "|" } else { "" };

        // The Background hex is inert while transparency is on — show it dimmed.
        let bg_overridden = i == 0 && state.transparent_bg;

        let color_preview = hex_to_color(value);
        let preview_span = if bg_overridden {
            Span::styled("  ", Style::default())
        } else if let Some(c) = color_preview {
            Span::styled("  ", Style::default().bg(c))
        } else {
            Span::styled("??", Style::default().fg(Color::Red))
        };

        let label_style = if bg_overridden {
            Style::default().fg(theme.muted).add_modifier(Modifier::DIM)
        } else if is_sel {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };

        let suffix = if bg_overridden { "  (overridden — transparent)" } else { "" };
        let line = Line::from(vec![
            Span::styled(format!("  {:<14}: {}{} ", label, value, cursor), label_style),
            preview_span,
            Span::styled(suffix, Style::default().fg(theme.muted)),
        ]);

        f.render_widget(Paragraph::new(line), chunks[custom_chunk_start + i]);
    }

    // --- Transparent-background checkbox ---
    let trans_chunk = custom_chunk_start + 6;
    let trans_sel = state.selected_field == ThemeTabState::transparent_index();
    let checkbox = if state.transparent_bg { "[x]" } else { "[ ]" };
    let trans_label_style = if trans_sel {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let trans_line = Line::from(vec![
        Span::styled(format!("  {} Transparent background", checkbox), trans_label_style),
        Span::styled(
            "  (use the terminal's own background — Space to toggle)",
            Style::default().fg(theme.muted),
        ),
    ]);
    f.render_widget(Paragraph::new(trans_line), chunks[trans_chunk]);

    // --- Save button ---
    let save_chunk = custom_chunk_start + 7;
    let is_save = state.selected_field == ThemeTabState::save_index();
    let save_style = if is_save {
        Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    // Style only the button span — not the whole row — so the highlight
    // hugs the text instead of filling the line.
    let save = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("[ Save Custom ]", save_style),
        ]),
    ]);
    f.render_widget(save, chunks[save_chunk]);
}
