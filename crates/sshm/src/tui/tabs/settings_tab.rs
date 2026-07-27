use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use crate::config::settings::AppConfig;
use crate::tui::theme::Theme;

pub struct SettingsFormState {
    pub default_port: String,
    pub default_username: String,
    pub default_identity_file: String,
    pub export_path: String,
    pub auto_health_check: bool,
    pub pause_health_on_session: bool,
    pub health_ttl_secs: String,
    pub health_probe_timeout_ms: String,
    pub kluster_refresh_secs: String,
    pub kluster_log_tail_lines: String,
    pub notifications_enabled: bool,
    pub selected_field: usize,
    pub dirty: bool,
}

/// Index of the boolean `auto_health_check` field in the form.
const AUTO_HEALTH_FIELD: usize = 4;
const HEALTH_TTL_FIELD: usize = 5;
const HEALTH_TIMEOUT_FIELD: usize = 6;
const KLUSTER_REFRESH_FIELD: usize = 7;
const KLUSTER_TAIL_FIELD: usize = 8;
/// Index of the boolean `notifications_enabled` field.
const NOTIFY_FIELD: usize = 9;
/// Index of the boolean `pause_health_on_session` field. Numbered after the
/// others so existing text-field indices keep their meaning; placed in the
/// Health checks section via `SECTIONS`.
const PAUSE_HEALTH_FIELD: usize = 10;

/// Settings grouped into labelled sections — drives the form layout.
struct Section {
    title: &'static str,
    fields: &'static [usize],
}

const SECTIONS: &[Section] = &[
    Section { title: "Defaults for new hosts", fields: &[0, 1, 2] },
    Section { title: "Export",                 fields: &[3] },
    Section { title: "Health checks",          fields: &[AUTO_HEALTH_FIELD, PAUSE_HEALTH_FIELD, HEALTH_TTL_FIELD, HEALTH_TIMEOUT_FIELD] },
    Section { title: "Kluster",                fields: &[KLUSTER_REFRESH_FIELD, KLUSTER_TAIL_FIELD] },
    Section { title: "Notifications",          fields: &[NOTIFY_FIELD] },
];

/// Human label for a field index.
fn field_label(i: usize) -> &'static str {
    match i {
        0 => "Default Port",
        1 => "Default Username",
        2 => "Default Identity File",
        3 => "Export Path",
        AUTO_HEALTH_FIELD => "Auto Health Check",
        PAUSE_HEALTH_FIELD => "Pause During SSH Session",
        HEALTH_TTL_FIELD => "Health Refresh / Cache TTL (s)",
        HEALTH_TIMEOUT_FIELD => "Probe Connect Timeout (ms)",
        KLUSTER_REFRESH_FIELD => "Kluster Refresh Interval (s)",
        KLUSTER_TAIL_FIELD => "Kluster Log Tail (lines)",
        NOTIFY_FIELD => "Desktop notifications",
        _ => "",
    }
}

impl SettingsFormState {
    pub fn from_config(config: &AppConfig) -> Self {
        SettingsFormState {
            default_port: config.default_port.to_string(),
            default_username: config.default_username.clone(),
            default_identity_file: config.default_identity_file.clone(),
            export_path: config.export_path.clone(),
            auto_health_check: config.auto_health_check,
            pause_health_on_session: config.pause_health_on_session,
            health_ttl_secs: config.health_ttl_secs.to_string(),
            health_probe_timeout_ms: config.health_probe_timeout_ms.to_string(),
            kluster_refresh_secs: config.kluster_refresh_secs.to_string(),
            kluster_log_tail_lines: config.kluster_log_tail_lines.to_string(),
            notifications_enabled: config.notifications_enabled,
            selected_field: 0,
            dirty: false,
        }
    }

    pub fn fields_count() -> usize { 11 }

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
        match self.selected_field {
            AUTO_HEALTH_FIELD => {
                self.auto_health_check = !self.auto_health_check;
                self.dirty = true;
                true
            }
            PAUSE_HEALTH_FIELD => {
                self.pause_health_on_session = !self.pause_health_on_session;
                self.dirty = true;
                true
            }
            NOTIFY_FIELD => {
                self.notifications_enabled = !self.notifications_enabled;
                self.dirty = true;
                // Switching on → fire an immediate test notification so the
                // user sees it works (bypasses the not-yet-saved gate).
                if self.notifications_enabled {
                    crate::os::notify_test();
                }
                true
            }
            _ => false,
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
            } else if state.toggle_bool() {
                // Landed on a boolean field — Enter flips it.
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

/// Current string value of a text field index.
fn settings_text_value(state: &SettingsFormState, i: usize) -> String {
    match i {
        0 => state.default_port.clone(),
        1 => state.default_username.clone(),
        2 => state.default_identity_file.clone(),
        3 => state.export_path.clone(),
        HEALTH_TTL_FIELD => state.health_ttl_secs.clone(),
        HEALTH_TIMEOUT_FIELD => state.health_probe_timeout_ms.clone(),
        KLUSTER_REFRESH_FIELD => state.kluster_refresh_secs.clone(),
        KLUSTER_TAIL_FIELD => state.kluster_log_tail_lines.clone(),
        _ => String::new(),
    }
}

/// Render one settings field (toggle or text) as a styled line.
fn field_line(state: &SettingsFormState, i: usize, theme: &Theme) -> Line<'static> {
    let is_sel = state.selected_field == i;
    let label_style = if is_sel {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let label_span = Span::styled(format!("   {:<32}", field_label(i)), label_style);

    if i == AUTO_HEALTH_FIELD || i == PAUSE_HEALTH_FIELD || i == NOTIFY_FIELD {
        let on = match i {
            AUTO_HEALTH_FIELD => state.auto_health_check,
            PAUSE_HEALTH_FIELD => state.pause_health_on_session,
            _ => state.notifications_enabled,
        };
        let val = if on { "[x] on" } else { "[ ] off" };
        let val_style = if on {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(theme.muted)
        };
        let hint = if is_sel { "   Space / ←→ to toggle" } else { "" };
        Line::from(vec![
            label_span,
            Span::styled(val.to_string(), val_style),
            Span::styled(hint.to_string(), Style::default().fg(theme.muted)),
        ])
    } else {
        let cursor = if is_sel { "|" } else { "" };
        let val_style = if is_sel {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };
        Line::from(vec![
            label_span,
            Span::styled(format!("{}{}", settings_text_value(state, i), cursor), val_style),
        ])
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

    // Build every visual line of the form. Headers, fields, blank lines
    // between sections, then a blank + the Save button — one flat list so it
    // can be rendered as a single scrollable Paragraph.
    let sel = state.selected_field;
    let save_idx = SettingsFormState::fields_count();
    let mut lines: Vec<Line> = Vec::new();
    // Line index of the row holding the current cursor (field or Save).
    let mut selected_line = 0usize;

    for (si, sec) in SECTIONS.iter().enumerate() {
        if si > 0 {
            lines.push(Line::from(""));
        }
        // Section header — only the title text is underlined, not the marker.
        lines.push(Line::from(vec![
            Span::styled(
                " ▸ ",
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                sec.title.to_string(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]));
        for &fi in sec.fields {
            if fi == sel {
                selected_line = lines.len();
            }
            lines.push(field_line(state, fi, theme));
        }
    }

    // Blank spacer + Save button.
    lines.push(Line::from(""));
    if sel == save_idx {
        selected_line = lines.len();
    }
    let save_style = if sel == save_idx {
        Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("[ Save ]", save_style),
        Span::raw("  "),
        Span::styled("[ Esc = Reset ]", Style::default().fg(theme.muted)),
    ]));

    // Content area (1-cell inset; leaves the right column for the scrollbar).
    let content = Rect {
        x: inner.x + 1,
        y: inner.y + 1,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(2),
    };
    let visible = content.height as usize;
    let total = lines.len();
    let max_scroll = total.saturating_sub(visible);
    // Scroll just enough to keep the selected row on screen (Save included).
    let scroll = if selected_line < visible {
        0
    } else {
        (selected_line + 1).saturating_sub(visible)
    }
    .min(max_scroll);

    f.render_widget(
        Paragraph::new(lines).scroll((scroll as u16, 0)),
        content,
    );

    // Scrollbar when the form is taller than the viewport.
    if total > visible {
        let mut sb_state = ScrollbarState::new(total).position(scroll);
        f.render_stateful_widget(
            Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
            inner,
            &mut sb_state,
        );
    }
}
