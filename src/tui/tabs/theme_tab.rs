use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use crate::tui::theme::{Theme, PRESETS, hex_to_color, color_to_hex};

/// Fields layout:
/// 0..PRESETS.len()-1  = preset items
/// PRESETS.len()       = "Custom Colors" separator (skipped on Enter)
/// PRESETS.len()+1..+4 = custom hex fields (bg, fg, accent, muted)
/// PRESETS.len()+5     = Save Custom button
pub struct ThemeTabState {
    pub selected_field: usize,
    pub custom_bg: String,
    pub custom_fg: String,
    pub custom_accent: String,
    pub custom_muted: String,
    pub status_message: Option<String>,
    pub dirty: bool,
}

impl ThemeTabState {
    pub fn new(current_theme: &Theme) -> Self {
        ThemeTabState {
            selected_field: 0,
            custom_bg: color_to_hex(current_theme.bg),
            custom_fg: color_to_hex(current_theme.fg),
            custom_accent: color_to_hex(current_theme.accent),
            custom_muted: color_to_hex(current_theme.muted),
            status_message: None,
            dirty: false,
        }
    }

    fn total_fields() -> usize {
        // presets + separator + 4 custom fields + save button
        PRESETS.len() + 1 + 4 + 1
    }

    fn separator_index() -> usize { PRESETS.len() }
    fn custom_start() -> usize { PRESETS.len() + 1 }
    fn save_index() -> usize { PRESETS.len() + 5 }

    pub fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % Self::total_fields();
        // Skip separator
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
        // Skip separator
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
        self.dirty && self.selected_field >= start && self.selected_field < start + 4
    }

    fn active_custom_mut(&mut self) -> Option<&mut String> {
        let start = Self::custom_start();
        match self.selected_field.checked_sub(start) {
            Some(0) => Some(&mut self.custom_bg),
            Some(1) => Some(&mut self.custom_fg),
            Some(2) => Some(&mut self.custom_accent),
            Some(3) => Some(&mut self.custom_muted),
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

pub enum ThemeAction {
    None,
    ApplyPreset(usize),
    SaveCustom,
}

pub fn handle_theme_event(key: KeyCode, state: &mut ThemeTabState) -> ThemeAction {
    state.status_message = None;
    match key {
        KeyCode::Down | KeyCode::Tab => { state.next_field(); ThemeAction::None }
        KeyCode::Up | KeyCode::BackTab => { state.prev_field(); ThemeAction::None }
        KeyCode::Enter => {
            if state.is_on_preset() {
                ThemeAction::ApplyPreset(state.selected_field)
            } else if state.selected_field == ThemeTabState::save_index() {
                ThemeAction::SaveCustom
            } else {
                state.next_field();
                ThemeAction::None
            }
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

    // Build layout: 1 line per preset, 1 separator, 1 line per custom field, 1 save, 1 status, rest spacer
    let mut constraints: Vec<Constraint> = Vec::new();
    // Presets header
    constraints.push(Constraint::Length(1));
    // Each preset
    for _ in PRESETS {
        constraints.push(Constraint::Length(1));
    }
    // Separator
    constraints.push(Constraint::Length(2));
    // Custom fields (4)
    for _ in 0..4 {
        constraints.push(Constraint::Length(1));
    }
    // Save button
    constraints.push(Constraint::Length(2));
    // Status
    constraints.push(Constraint::Length(1));
    // Spacer
    constraints.push(Constraint::Min(0));

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
    let custom_labels = ["Background", "Foreground", "Accent", "Muted"];
    let custom_values = [&state.custom_bg, &state.custom_fg, &state.custom_accent, &state.custom_muted];
    let custom_chunk_start = sep_chunk_idx + 1;

    for (i, (label, value)) in custom_labels.iter().zip(custom_values.iter()).enumerate() {
        let field_idx = ThemeTabState::custom_start() + i;
        let is_sel = state.selected_field == field_idx;
        let cursor = if is_sel { "|" } else { "" };

        let color_preview = hex_to_color(value);
        let preview_span = if let Some(c) = color_preview {
            Span::styled("  ", Style::default().bg(c))
        } else {
            Span::styled("??", Style::default().fg(Color::Red))
        };

        let label_style = if is_sel {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };

        let line = Line::from(vec![
            Span::styled(format!("  {:<14}: {}{} ", label, value, cursor), label_style),
            preview_span,
        ]);

        f.render_widget(Paragraph::new(line), chunks[custom_chunk_start + i]);
    }

    // --- Save button ---
    let save_chunk = custom_chunk_start + 4;
    let is_save = state.selected_field == ThemeTabState::save_index();
    let save_style = if is_save {
        Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    let save = Paragraph::new("\n  [ Save Custom ]").style(save_style);
    f.render_widget(save, chunks[save_chunk]);

    // --- Status ---
    if let Some(ref msg) = state.status_message {
        let status = Paragraph::new(format!("  {}", msg))
            .style(Style::default().fg(theme.accent));
        f.render_widget(status, chunks[save_chunk + 1]);
    }
}
