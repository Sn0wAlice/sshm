use crate::config::settings::AppConfig;
use crate::tui::theme::Theme;
use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

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
    pub sync_enabled: bool,
    pub sync_repo_url: String,
    pub sync_ssh_key: String,
    pub sync_branch: String,
    /// Minutes between automatic syncs; `0` means manual only.
    pub sync_interval_min: String,
    pub sync_on_start: bool,
    pub sync_on_exit: bool,
    pub sync_encrypt: bool,
    pub sync_age_identity: String,
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
/// Git config-sync fields. Everything else about sync (which files travel,
/// the conflict policy) lives in `settings.toml` and `sshm sync setup`, which
/// can offer real multi-select prompts.
const SYNC_ENABLED_FIELD: usize = 11;
const SYNC_REPO_FIELD: usize = 12;
const SYNC_KEY_FIELD: usize = 13;
const SYNC_BRANCH_FIELD: usize = 14;
const SYNC_INTERVAL_FIELD: usize = 15;
const SYNC_ON_START_FIELD: usize = 16;
const SYNC_ON_EXIT_FIELD: usize = 17;
/// Encrypt the synced payload with `age`. Only affects what leaves the
/// machine — the local files stay plain.
const SYNC_ENCRYPT_FIELD: usize = 18;
/// Path to the age identity backing [`SYNC_ENCRYPT_FIELD`].
const SYNC_AGE_IDENTITY_FIELD: usize = 19;

/// Settings grouped into labelled sections — drives the form layout.
struct Section {
    title: &'static str,
    fields: &'static [usize],
}

const SECTIONS: &[Section] = &[
    Section {
        title: "Defaults for new hosts",
        fields: &[0, 1, 2],
    },
    Section {
        title: "Export",
        fields: &[3],
    },
    Section {
        title: "Health checks",
        fields: &[
            AUTO_HEALTH_FIELD,
            PAUSE_HEALTH_FIELD,
            HEALTH_TTL_FIELD,
            HEALTH_TIMEOUT_FIELD,
        ],
    },
    Section {
        title: "Kluster",
        fields: &[KLUSTER_REFRESH_FIELD, KLUSTER_TAIL_FIELD],
    },
    Section {
        title: "Notifications",
        fields: &[NOTIFY_FIELD],
    },
    Section {
        title: "Config sync (git over SSH)",
        fields: &[
            SYNC_ENABLED_FIELD,
            SYNC_REPO_FIELD,
            SYNC_KEY_FIELD,
            SYNC_BRANCH_FIELD,
            SYNC_INTERVAL_FIELD,
            SYNC_ON_START_FIELD,
            SYNC_ON_EXIT_FIELD,
            SYNC_ENCRYPT_FIELD,
            SYNC_AGE_IDENTITY_FIELD,
        ],
    },
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
        SYNC_ENABLED_FIELD => "Sync config with a git repo",
        SYNC_REPO_FIELD => "Repository SSH URL",
        SYNC_KEY_FIELD => "SSH key",
        SYNC_BRANCH_FIELD => "Branch",
        SYNC_INTERVAL_FIELD => "Auto-sync every (min, 0 = manual)",
        SYNC_ON_START_FIELD => "Sync on start",
        SYNC_ON_EXIT_FIELD => "Sync on exit",
        SYNC_ENCRYPT_FIELD => "Encrypt what is pushed (age)",
        SYNC_AGE_IDENTITY_FIELD => "age identity file",
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
            sync_enabled: config.sync.enabled,
            sync_repo_url: config.sync.repo_url.clone(),
            sync_ssh_key: config.sync.ssh_key.clone(),
            sync_branch: config.sync.effective_branch(),
            sync_interval_min: match config.sync.effective_interval() {
                Some(secs) => (secs / 60).to_string(),
                None => "0".to_string(),
            },
            sync_on_start: config.sync.on_start,
            sync_on_exit: config.sync.on_exit,
            sync_encrypt: config.sync.encrypt,
            sync_age_identity: config.sync.age_identity.clone(),
            selected_field: 0,
            dirty: false,
        }
    }

    pub fn fields_count() -> usize {
        20
    }

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
            SYNC_REPO_FIELD => Some(&mut self.sync_repo_url),
            SYNC_KEY_FIELD => Some(&mut self.sync_ssh_key),
            SYNC_BRANCH_FIELD => Some(&mut self.sync_branch),
            SYNC_INTERVAL_FIELD => Some(&mut self.sync_interval_min),
            SYNC_AGE_IDENTITY_FIELD => Some(&mut self.sync_age_identity),
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        let numeric_only = matches!(
            self.selected_field,
            0 | HEALTH_TTL_FIELD
                | HEALTH_TIMEOUT_FIELD
                | KLUSTER_REFRESH_FIELD
                | KLUSTER_TAIL_FIELD
                | SYNC_INTERVAL_FIELD
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
            SYNC_ENABLED_FIELD => {
                self.sync_enabled = !self.sync_enabled;
                self.dirty = true;
                true
            }
            SYNC_ON_START_FIELD => {
                self.sync_on_start = !self.sync_on_start;
                self.dirty = true;
                true
            }
            SYNC_ON_EXIT_FIELD => {
                self.sync_on_exit = !self.sync_on_exit;
                self.dirty = true;
                true
            }
            SYNC_ENCRYPT_FIELD => {
                self.sync_encrypt = !self.sync_encrypt;
                self.dirty = true;
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
        KeyCode::Tab | KeyCode::Down => {
            state.next_field();
            SettingsAction::None
        }
        KeyCode::BackTab | KeyCode::Up => {
            state.prev_field();
            SettingsAction::None
        }
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
        KeyCode::Char(c) => {
            state.push_char(c);
            SettingsAction::None
        }
        KeyCode::Backspace => {
            state.pop_char();
            SettingsAction::None
        }
        _ => SettingsAction::None,
    }
}

/// True for the on/off fields (rendered as a toggle rather than a text input).
fn is_toggle(i: usize) -> bool {
    matches!(
        i,
        AUTO_HEALTH_FIELD
            | PAUSE_HEALTH_FIELD
            | NOTIFY_FIELD
            | SYNC_ENABLED_FIELD
            | SYNC_ON_START_FIELD
            | SYNC_ON_EXIT_FIELD
            | SYNC_ENCRYPT_FIELD
    )
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
        SYNC_REPO_FIELD => state.sync_repo_url.clone(),
        SYNC_KEY_FIELD => state.sync_ssh_key.clone(),
        SYNC_BRANCH_FIELD => state.sync_branch.clone(),
        SYNC_INTERVAL_FIELD => state.sync_interval_min.clone(),
        SYNC_AGE_IDENTITY_FIELD => state.sync_age_identity.clone(),
        _ => String::new(),
    }
}

/// Render one settings field (toggle or text) as a styled line.
fn field_line(state: &SettingsFormState, i: usize, theme: &Theme) -> Line<'static> {
    let is_sel = state.selected_field == i;
    let label_style = if is_sel {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let label_span = Span::styled(format!("   {:<32}", field_label(i)), label_style);

    if is_toggle(i) {
        let on = match i {
            AUTO_HEALTH_FIELD => state.auto_health_check,
            PAUSE_HEALTH_FIELD => state.pause_health_on_session,
            SYNC_ENABLED_FIELD => state.sync_enabled,
            SYNC_ON_START_FIELD => state.sync_on_start,
            SYNC_ON_EXIT_FIELD => state.sync_on_exit,
            SYNC_ENCRYPT_FIELD => state.sync_encrypt,
            _ => state.notifications_enabled,
        };
        let val = if on { "[x] on" } else { "[ ] off" };
        let val_style = if on {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(theme.muted)
        };
        let hint = if is_sel {
            "   Space / ←→ to toggle"
        } else {
            ""
        };
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
            Span::styled(
                format!("{}{}", settings_text_value(state, i), cursor),
                val_style,
            ),
        ])
    }
}

/// An extra line rendered under a field, for values whose literal text is not
/// the whole story.
///
/// The age identity is the case that needs it: the field holds whatever you
/// typed — usually with a `~` — while what sshm actually opens is the expanded
/// path. Those differ on macOS, where sshm's own config lives under
/// `~/Library/Application Support/sshm` rather than `~/.config/sshm`, so a
/// plausible-looking `~/.config/sshm/sync-age.key` can point somewhere sshm
/// never looks. Showing the resolved path — and whether a file is actually
/// there — turns that from a silent misconfiguration into something visible.
fn field_note(state: &SettingsFormState, i: usize, theme: &Theme) -> Option<Line<'static>> {
    if i != SYNC_AGE_IDENTITY_FIELD {
        return None;
    }
    let raw = state.sync_age_identity.trim();
    let (text, color) = if raw.is_empty() {
        if state.sync_encrypt {
            (
                "no identity set — encryption will refuse to run".to_string(),
                theme.warning,
            )
        } else {
            (
                format!(
                    "default if left empty: {}",
                    default_identity_path().display()
                ),
                theme.muted,
            )
        }
    } else {
        let resolved = std::path::PathBuf::from(shellexpand::tilde(raw).to_string());
        if resolved.exists() {
            (format!("→ {}", resolved.display()), theme.success)
        } else {
            (
                format!("→ {} (missing)", resolved.display()),
                if state.sync_encrypt {
                    theme.warning
                } else {
                    theme.muted
                },
            )
        }
    };
    Some(Line::from(Span::styled(
        format!("   {:<32}{}", "", text),
        Style::default().fg(color),
    )))
}

/// Where sshm suggests putting the age identity: inside its own config
/// directory, whatever that resolves to on this platform.
pub fn default_identity_path() -> std::path::PathBuf {
    crate::config::path::config_dir().join("sync-age.key")
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
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
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
            if let Some(note) = field_note(state, fi, theme) {
                lines.push(note);
            }
        }
    }

    // Blank spacer + Save button.
    lines.push(Line::from(""));
    if sel == save_idx {
        selected_line = lines.len();
    }
    let save_style = if sel == save_idx {
        Style::default()
            .fg(theme.bg)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD)
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

    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), content);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SettingsFormState {
        SettingsFormState::from_config(&AppConfig::default())
    }

    #[test]
    fn the_suggested_identity_lives_in_sshms_own_config_dir() {
        // Not a hard-coded `~/.config/sshm`: that is right on Linux and wrong
        // on macOS, and a key written to the wrong directory is a sync that
        // refuses to run for a reason nobody can see.
        let p = default_identity_path();
        assert!(p.is_absolute(), "{}", p.display());
        assert_eq!(p.file_name().unwrap(), "sync-age.key");
        assert_eq!(p.parent().unwrap(), crate::config::path::config_dir());
    }

    #[test]
    fn the_identity_row_shows_where_the_path_actually_lands() {
        let mut s = state();
        s.sync_age_identity = "~/some/where/id.key".into();
        let note = field_note(
            &s,
            SYNC_AGE_IDENTITY_FIELD,
            &crate::tui::theme::get_global_theme(),
        )
        .expect("the identity row carries a note");
        let text: String = note.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(!text.contains('~'), "the tilde must be resolved: {text}");
        assert!(text.contains("some/where/id.key"), "{text}");
    }

    #[test]
    fn an_empty_identity_points_at_the_default() {
        let mut s = state();
        s.sync_age_identity = String::new();
        s.sync_encrypt = false;
        let note = field_note(
            &s,
            SYNC_AGE_IDENTITY_FIELD,
            &crate::tui::theme::get_global_theme(),
        )
        .expect("note");
        let text: String = note.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(
            text.contains(&default_identity_path().display().to_string()),
            "{text}"
        );
    }

    #[test]
    fn encryption_without_an_identity_is_called_out() {
        let mut s = state();
        s.sync_age_identity = String::new();
        s.sync_encrypt = true;
        let note = field_note(
            &s,
            SYNC_AGE_IDENTITY_FIELD,
            &crate::tui::theme::get_global_theme(),
        )
        .expect("note");
        let text: String = note.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(text.contains("refuse"), "{text}");
    }

    #[test]
    fn a_missing_file_is_flagged() {
        let mut s = state();
        s.sync_age_identity = "/nonexistent/age.key".into();
        let note = field_note(
            &s,
            SYNC_AGE_IDENTITY_FIELD,
            &crate::tui::theme::get_global_theme(),
        )
        .expect("note");
        let text: String = note.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(text.contains("missing"), "{text}");
    }

    #[test]
    fn only_the_identity_row_carries_a_note() {
        let s = state();
        let theme = crate::tui::theme::get_global_theme();
        for i in 0..SettingsFormState::fields_count() {
            if i == SYNC_AGE_IDENTITY_FIELD {
                continue;
            }
            assert!(field_note(&s, i, &theme).is_none(), "field {i} grew a note");
        }
    }

    #[test]
    fn the_encrypt_row_is_a_toggle_and_the_identity_row_is_not() {
        assert!(is_toggle(SYNC_ENCRYPT_FIELD));
        assert!(!is_toggle(SYNC_AGE_IDENTITY_FIELD));
        let mut s = state();
        s.selected_field = SYNC_AGE_IDENTITY_FIELD;
        assert!(
            s.active_value_mut().is_some(),
            "the identity row is editable"
        );
    }

    #[test]
    fn both_sync_encryption_rows_are_reachable() {
        for f in SECTIONS.iter().flat_map(|s| s.fields) {
            if *f == SYNC_ENCRYPT_FIELD {
                assert!(SECTIONS
                    .iter()
                    .any(|s| s.fields.contains(&SYNC_AGE_IDENTITY_FIELD)));
                return;
            }
        }
        panic!("the encrypt row is not in any section, so it can never be selected");
    }
}
