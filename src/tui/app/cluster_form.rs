//! Modal TUI form for creating / editing a Kluster cluster entry.
//!
//! Self-contained: opens its own alternate screen + raw-mode loop, draws a
//! centered card, returns `Some(Cluster)` on save or `None` on Esc / cancel.
//! Mirrors the pattern of [`super::host_form::run_host_form`].

use std::io::stdout;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};

use crate::kluster::{Cluster, ClusterKind};
use crate::tui::ssh::modal::centered_rect;
use crate::tui::theme;

const FIELD_NAME: usize = 0;
const FIELD_KIND: usize = 1;
const FIELD_KUBECONFIG: usize = 2;
const FIELD_CONTEXT: usize = 3;
const FIELD_NAMESPACE: usize = 4;
const FIELD_SAVE: usize = 5;

pub struct ClusterFormState {
    pub name: String,
    pub kind: ClusterKind,
    pub kubeconfig: String,
    pub context: String,
    pub namespace_default: String,
    pub selected: usize,
    pub error: Option<String>,
    pub is_edit: bool,
    pub original_name: Option<String>,
}

impl ClusterFormState {
    fn new_create() -> Self {
        Self {
            name: String::new(),
            kind: ClusterKind::K8s,
            kubeconfig: String::new(),
            context: String::new(),
            namespace_default: String::new(),
            selected: 0,
            error: None,
            is_edit: false,
            original_name: None,
        }
    }

    fn new_edit(c: &Cluster) -> Self {
        Self {
            name: c.name.clone(),
            kind: c.kind,
            kubeconfig: c.kubeconfig.clone().unwrap_or_default(),
            context: c.context.clone().unwrap_or_default(),
            namespace_default: c.namespace_default.clone().unwrap_or_default(),
            selected: 0,
            error: None,
            is_edit: true,
            original_name: Some(c.name.clone()),
        }
    }

    fn next_field(&mut self) {
        self.selected = (self.selected + 1) % (FIELD_SAVE + 1);
    }
    fn prev_field(&mut self) {
        if self.selected == 0 {
            self.selected = FIELD_SAVE;
        } else {
            self.selected -= 1;
        }
    }
    fn cycle_kind(&mut self) {
        self.kind = match self.kind {
            ClusterKind::K8s => ClusterKind::K3s,
            ClusterKind::K3s => ClusterKind::K8s,
        };
    }
    fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.selected {
            FIELD_NAME => Some(&mut self.name),
            FIELD_KUBECONFIG => Some(&mut self.kubeconfig),
            FIELD_CONTEXT => Some(&mut self.context),
            FIELD_NAMESPACE => Some(&mut self.namespace_default),
            _ => None,
        }
    }
    fn push_char(&mut self, c: char) {
        if self.selected == FIELD_KIND {
            if c == ' ' { self.cycle_kind(); }
            return;
        }
        if let Some(f) = self.active_value_mut() {
            f.push(c);
            self.error = None;
        }
    }
    fn pop_char(&mut self) {
        if let Some(f) = self.active_value_mut() {
            f.pop();
            self.error = None;
        }
    }

    fn validate(&self) -> Result<Cluster, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Name cannot be empty".into());
        }
        let to_opt = |s: &str| {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        };
        Ok(Cluster {
            name: name.to_string(),
            kind: self.kind,
            kubeconfig: to_opt(&self.kubeconfig),
            context: to_opt(&self.context),
            namespace_default: to_opt(&self.namespace_default),
        })
    }
}

fn draw(f: &mut Frame, state: &ClusterFormState) {
    let theme = theme::load();
    let size = f.area();
    let area = centered_rect(60, 60, size);

    f.render_widget(Clear, area);
    let block = Block::default()
        .title(Span::styled(
            if state.is_edit { " Edit cluster " } else { " Add cluster " },
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // name
            Constraint::Length(1), // kind
            Constraint::Length(1), // kubeconfig
            Constraint::Length(1), // context
            Constraint::Length(1), // namespace_default
            Constraint::Length(1), // spacer
            Constraint::Length(1), // save
            Constraint::Length(1), // spacer
            Constraint::Min(1),    // help / error
        ])
        .split(inner);

    let mk = |label: &str, value: &str, sel: bool| {
        let val_span = if sel {
            Span::styled(
                format!("[{}]", value),
                Style::default().bg(theme.accent).fg(theme.bg).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(format!("[{}]", value))
        };
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{label}: "), Style::default().add_modifier(Modifier::BOLD)),
            val_span,
        ]))
    };

    f.render_widget(mk("Name", &state.name, state.selected == FIELD_NAME), chunks[FIELD_NAME]);

    // Kind row — toggle, not text
    let kind_sel = state.selected == FIELD_KIND;
    let kind_style = if kind_sel {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let kind_line = format!(
        "Kind: < {} >  (←→/Space to switch)",
        state.kind.label()
    );
    f.render_widget(Paragraph::new(kind_line).style(kind_style), chunks[FIELD_KIND]);

    f.render_widget(mk("Kubeconfig (empty = default)", &state.kubeconfig, state.selected == FIELD_KUBECONFIG), chunks[FIELD_KUBECONFIG]);
    f.render_widget(mk("Context (empty = current)",     &state.context,    state.selected == FIELD_CONTEXT),    chunks[FIELD_CONTEXT]);
    f.render_widget(mk("Default namespace (optional)",  &state.namespace_default, state.selected == FIELD_NAMESPACE), chunks[FIELD_NAMESPACE]);

    // Save button
    let save_sel = state.selected == FIELD_SAVE;
    let save_style = if save_sel {
        Style::default().bg(theme.accent).fg(theme.bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    let save = Paragraph::new(Line::from(vec![
        Span::styled("[ Save ]", save_style),
        Span::raw("  "),
        Span::styled("[ Esc = Cancel ]", Style::default().fg(theme.muted)),
    ]));
    f.render_widget(save, chunks[FIELD_SAVE + 1]); // chunks[6] (after spacer)

    // Help / error
    let help = if let Some(ref e) = state.error {
        Paragraph::new(format!("  {}", e)).style(Style::default().fg(theme.error))
    } else {
        Paragraph::new("Tab/↑↓ navigate • Type to edit • Enter to save when [ Save ] is selected • Esc to cancel")
            .style(Style::default().fg(theme.muted))
    };
    f.render_widget(help, chunks[8]);
}

/// Run the form. Returns `Some(Cluster)` on save, `None` on cancel.
pub fn run_cluster_form(initial: Option<&Cluster>) -> Option<Cluster> {
    let mut state = match initial {
        Some(c) => ClusterFormState::new_edit(c),
        None => ClusterFormState::new_create(),
    };

    let mut stdout_h = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout_h, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout_h);
    let mut terminal = Terminal::new(backend).ok()?;

    let result = loop {
        let _ = terminal.draw(|f| draw(f, &state));

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press { continue; }
                match k.code {
                    KeyCode::Esc => break None,
                    KeyCode::Tab | KeyCode::Down => state.next_field(),
                    KeyCode::BackTab | KeyCode::Up => state.prev_field(),
                    KeyCode::Left | KeyCode::Right if state.selected == FIELD_KIND => {
                        state.cycle_kind();
                    }
                    KeyCode::Enter => {
                        if state.selected == FIELD_SAVE {
                            match state.validate() {
                                Ok(c) => break Some(c),
                                Err(e) => state.error = Some(e),
                            }
                        } else {
                            state.next_field();
                        }
                    }
                    KeyCode::Char(c) => state.push_char(c),
                    KeyCode::Backspace => state.pop_char(),
                    _ => {}
                }
            }
        }
    };

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    result
}

/// Confirmation modal for `delete cluster`. Dedicated mini-loop so the
/// caller doesn't need to weave delete-state through the whole tab.
pub fn run_cluster_delete_confirm(cluster_name: &str) -> bool {
    use crate::tui::ssh::modal::{render_modal, ModalButton, ModalConfig};

    let mut stdout_h = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout_h, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout_h);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(_) => return false,
    };

    let mut button_idx: usize = 1; // default Cancel

    let confirmed = loop {
        let _ = terminal.draw(|f| {
            let theme = theme::load();
            let size = f.area();
            let cfg = ModalConfig {
                title: "Delete cluster".into(),
                body_lines: vec![
                    format!("Remove \"{}\" from sshm?", cluster_name),
                    String::new(),
                    "The remote cluster itself is unaffected — only the saved entry is deleted.".into(),
                ],
                buttons: vec![
                    ModalButton { label: "Delete".into(), is_selected: button_idx == 0 },
                    ModalButton { label: "Cancel".into(), is_selected: button_idx == 1 },
                ],
                width_percent: 60,
                height_percent: 30,
            };
            render_modal(f, size, &cfg, &theme);
        });

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press { continue; }
                match k.code {
                    KeyCode::Esc => break false,
                    KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                        button_idx ^= 1;
                    }
                    KeyCode::Enter => break button_idx == 0,
                    KeyCode::Char('y') | KeyCode::Char('Y') => break true,
                    KeyCode::Char('n') | KeyCode::Char('N') => break false,
                    _ => {}
                }
            }
        }
    };

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    confirmed
}
