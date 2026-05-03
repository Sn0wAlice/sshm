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
    pub auto_health_check: bool,
    pub health_ttl_secs: String,
    pub health_probe_timeout_ms: String,
    pub kluster_refresh_secs: String,
    pub kluster_log_tail_lines: String,
    pub selected_field: usize,
    pub dirty: bool,
}

/// Index of the boolean `auto_health_check` field in the form.
const AUTO_HEALTH_FIELD: usize = 4;
const HEALTH_TTL_FIELD: usize = 5;
const HEALTH_TIMEOUT_FIELD: usize = 6;
const KLUSTER_REFRESH_FIELD: usize = 7;
const KLUSTER_TAIL_FIELD: usize = 8;

impl SettingsFormState {
    pub fn from_config(config: &AppConfig) -> Self {
        SettingsFormState {
            default_port: config.default_port.to_string(),
            default_username: config.default_username.clone(),
            default_identity_file: config.default_identity_file.clone(),
            export_path: config.export_path.clone(),
            auto_health_check: config.auto_health_check,
            health_ttl_secs: config.health_ttl_secs.to_string(),
            health_probe_timeout_ms: config.health_probe_timeout_ms.to_string(),
            kluster_refresh_secs: config.kluster_refresh_secs.to_string(),
            kluster_log_tail_lines: config.kluster_log_tail_lines.to_string(),
            selected_field: 0,
            dirty: false,
        }
    }

    pub fn fields_count() -> usize { 9 }

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
            HEALTH_TTL_FIELD => Some(&mut self.health_ttl_secs),
            HEALTH_TIMEOUT_FIELD => Some(&mut self.health_probe_timeout_ms),
            KLUSTER_REFRESH_FIELD => Some(&mut self.kluster_refresh_secs),
            KLUSTER_TAIL_FIELD => Some(&mut self.kluster_log_tail_lines),
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        let numeric_only = matches!(
            self.selected_field,
            0 | HEALTH_TTL_FIELD | HEALTH_TIMEOUT_FIELD | KLUSTER_REFRESH_FIELD | KLUSTER_TAIL_FIELD
        );
        if numeric_only && !c.is_ascii_digit() {
            return;
        }
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

    pub fn toggle_bool(&mut self) -> bool {
        if self.selected_field == AUTO_HEALTH_FIELD {
            self.auto_health_check = !self.auto_health_check;
            self.dirty = true;
            true
        } else {
            false
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
            } else if state.selected_field == AUTO_HEALTH_FIELD {
                state.toggle_bool();
                SettingsAction::None
            } else {
                state.next_field();
                SettingsAction::None
            }
        }
        KeyCode::Left | KeyCode::Right => {
            state.toggle_bool();
            SettingsAction::None
        }
        KeyCode::Char(' ') => {
            if state.toggle_bool() {
                SettingsAction::None
            } else {
                state.push_char(' ');
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

    let labels = [
        "Default Port",
        "Default Username",
        "Default Identity File",
        "Export Path",
    ];
    let values = [
        &state.default_port,
        &state.default_username,
        &state.default_identity_file,
        &state.export_path,
    ];

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

    // Boolean field: Auto health check
    {
        let is_selected = state.selected_field == AUTO_HEALTH_FIELD;
        let style = if is_selected {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };
        let value = if state.auto_health_check { "[x] on" } else { "[ ] off" };
        let hint = if is_selected { "  (Space/←/→/Enter to toggle)" } else { "" };
        let text = format!("  Auto Health Check: {}{}", value, hint);
        let p = Paragraph::new(text).style(style);
        f.render_widget(p, chunks[AUTO_HEALTH_FIELD]);
    }

    // Numeric: TTL + probe timeout + kluster
    for (idx, label, val_str) in [
        (HEALTH_TTL_FIELD,       "Health Refresh / Cache TTL (s)", &state.health_ttl_secs),
        (HEALTH_TIMEOUT_FIELD,   "Probe Connect Timeout (ms)",     &state.health_probe_timeout_ms),
        (KLUSTER_REFRESH_FIELD,  "Kluster Refresh Interval (s)",   &state.kluster_refresh_secs),
        (KLUSTER_TAIL_FIELD,     "Kluster Log Tail (lines)",       &state.kluster_log_tail_lines),
    ] {
        let is_selected = state.selected_field == idx;
        let cursor = if is_selected { "|" } else { "" };
        let style = if is_selected {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };
        let text = format!("  {}: {}{}", label, val_str, cursor);
        f.render_widget(Paragraph::new(text).style(style), chunks[idx]);
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
