use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use crate::config::settings::AppConfig;
use crate::tui::theme::Theme;

pub struct SettingsFormState {
    pub default_port: String,
    pub default_username: String,
    pub default_identity_file: String,
    pub export_path: String,
    pub selected_field: usize,
    pub dirty: bool,
}

impl SettingsFormState {
    pub fn from_config(config: &AppConfig) -> Self {
        SettingsFormState {
            default_port: config.default_port.to_string(),
            default_username: config.default_username.clone(),
            default_identity_file: config.default_identity_file.clone(),
            export_path: config.export_path.clone(),
            selected_field: 0,
            dirty: false,
        }
    }

    pub fn fields_count() -> usize { 4 }

    pub fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % (Self::fields_count() + 1);
    }

    pub fn prev_field(&mut self) {
        if self.selected_field == 0 {
            self.selected_field = Self::fields_count();
        } else {
            self.selected_field -= 1;
        }
    }

    pub fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.selected_field {
            0 => Some(&mut self.default_port),
            1 => Some(&mut self.default_username),
            2 => Some(&mut self.default_identity_file),
            3 => Some(&mut self.export_path),
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(field) = self.active_value_mut() {
            field.push(c);
            self.dirty = true;
        }
    }

    pub fn pop_char(&mut self) {
        if let Some(field) = self.active_value_mut() {
            field.pop();
            self.dirty = true;
        }
    }

    pub fn is_editing_field(&self) -> bool {
        self.dirty && self.selected_field < Self::fields_count()
    }
}

pub enum SettingsAction {
    None,
    Save,
}

pub fn handle_settings_event(key: KeyCode, state: &mut SettingsFormState) -> SettingsAction {
    match key {
        KeyCode::Tab | KeyCode::Down => { state.next_field(); SettingsAction::None }
        KeyCode::BackTab | KeyCode::Up => { state.prev_field(); SettingsAction::None }
        KeyCode::Enter => {
            if state.selected_field == SettingsFormState::fields_count() {
                SettingsAction::Save
            } else {
                state.next_field();
                SettingsAction::None
            }
        }
        KeyCode::Char(c) => { state.push_char(c); SettingsAction::None }
        KeyCode::Backspace => { state.pop_char(); SettingsAction::None }
        _ => SettingsAction::None,
    }
}

pub fn draw_settings_tab(f: &mut Frame, area: Rect, state: &SettingsFormState, theme: &Theme) {
    let block = Block::default()
        .title("Settings")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let labels = ["Default Port", "Default Username", "Default Identity File", "Export Path"];
    let values = [&state.default_port, &state.default_username, &state.default_identity_file, &state.export_path];

    let mut constraints: Vec<Constraint> = Vec::new();
    for _ in 0..SettingsFormState::fields_count() {
        constraints.push(Constraint::Length(2));
    }
    constraints.push(Constraint::Length(2)); // save button
    constraints.push(Constraint::Min(0));    // spacer

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
        .split(inner);

    for (i, (label, value)) in labels.iter().zip(values.iter()).enumerate() {
        let is_selected = state.selected_field == i;
        let cursor = if is_selected { "|" } else { "" };
        let style = if is_selected {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };

        let text = format!("  {}: {}{}", label, value, cursor);
        let p = Paragraph::new(text).style(style);
        f.render_widget(p, chunks[i]);
    }

    // Save button
    let save_idx = SettingsFormState::fields_count();
    let save_style = if state.selected_field == save_idx {
        Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    let save = Paragraph::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("[ Save ]", save_style),
        Span::raw("  "),
        Span::styled("[ Esc = Reset ]", Style::default().fg(theme.muted)),
    ]));
    f.render_widget(save, chunks[save_idx]);
}
