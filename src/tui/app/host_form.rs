//! Host create/edit modal form: rendering, validation, event loop.

use std::io::stdout;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

use crate::config::settings::load_settings;
use crate::models::{Database, Host};
use crate::tui::ssh::host_form_state::HostFormState;
use crate::tui::ssh::modal::centered_rect;
use crate::tui::theme;

use super::save_and_export;

pub fn draw_host_form(f: &mut Frame, state: &HostFormState) {
    let size = f.area();
    let area = centered_rect(70, 80, size);
    let theme = theme::load();
    let bg = theme.bg;
    let fg = theme.fg;
    let accent = theme.accent;

    let block = Block::default()
        .title(Span::styled(
            if state.is_edit { "Edit host" } else { "Create host" },
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .style(Style::default().bg(bg).fg(fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(1), // name
                Constraint::Length(1), // host
                Constraint::Length(1), // port
                Constraint::Length(1), // username
                Constraint::Length(1), // identity
                Constraint::Length(1), // proxyjump
                Constraint::Length(1), // tags
                Constraint::Length(1), // folder
                Constraint::Length(1), // forward agent
                Constraint::Length(1), // actions
            ]
            .as_ref(),
        )
        .split(inner);

    let mk_line = |label: &str, value: &str, selected: bool| {
        let value_span = if selected {
            Span::styled(
                format!("[{}]", value),
                Style::default().bg(accent).fg(bg).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(format!("[{}]", value))
        };
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{label}: "), Style::default().add_modifier(Modifier::BOLD)),
            value_span,
        ]))
    };

    f.render_widget(mk_line("Name", &state.name, state.selected_field == 0), chunks[0]);
    f.render_widget(mk_line("Host/IP", &state.host, state.selected_field == 1), chunks[1]);
    f.render_widget(mk_line("Port", &state.port, state.selected_field == 2), chunks[2]);
    f.render_widget(mk_line("Username", &state.username, state.selected_field == 3), chunks[3]);
    f.render_widget(mk_line("Identity file", &state.identity_file, state.selected_field == 4), chunks[4]);
    f.render_widget(mk_line("ProxyJump", &state.proxy_jump, state.selected_field == 5), chunks[5]);
    f.render_widget(mk_line("Tags", &state.tags, state.selected_field == 6), chunks[6]);
    f.render_widget(mk_line("Folder", &state.folder, state.selected_field == 7), chunks[7]);

    // Forward-agent toggle row
    let fa_selected = state.selected_field == 8;
    let fa_value = if state.forward_agent { "[x]" } else { "[ ]" };
    let fa_label = "ForwardAgent (-A)";
    let fa_marker_style = if fa_selected {
        Style::default().bg(accent).fg(bg).add_modifier(Modifier::BOLD)
    } else if state.forward_agent {
        Style::default().fg(theme.error).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let warning_style = if state.forward_agent {
        Style::default().fg(theme.error)
    } else {
        Style::default().fg(theme.muted)
    };
    let fa_warning = if state.forward_agent {
        "  ⚠ shares your local agent with this host"
    } else {
        "  Space to toggle (off by default)"
    };
    let fa_para = Paragraph::new(Line::from(vec![
        Span::styled(format!("{}: ", fa_label), Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(fa_value, fa_marker_style),
        Span::styled(fa_warning.to_string(), warning_style),
    ]));
    f.render_widget(fa_para, chunks[8]);

    let save_selected = state.selected_field == HostFormState::fields_count();
    let save_style = if save_selected {
        Style::default().bg(accent).fg(bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(accent)
    };

    let actions = Paragraph::new(Line::from(vec![
        Span::styled("[ Save ]", save_style),
        Span::raw("  "),
        Span::styled("[ Esc = Cancel ]", Style::default().fg(theme.muted)),
    ]));

    f.render_widget(actions, chunks[9]);

    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(2),
        width: inner.width,
        height: 2,
    };
    if let Some(ref err) = state.error {
        let err_para = Paragraph::new(Line::from(vec![
            Span::styled("✗ ", Style::default().fg(theme.error).add_modifier(Modifier::BOLD)),
            Span::styled(err.clone(), Style::default().fg(theme.error).add_modifier(Modifier::BOLD)),
        ]));
        f.render_widget(err_para, footer_area);
    } else {
        let pj_hint = if state.selected_field == 5 {
            "ProxyJump: comma-separated multi-hop, e.g. \"bastion1,bastion2\". Each entry can be a saved host name (auto-resolved) or user@host[:port]."
        } else if state.selected_field == 8 {
            "ForwardAgent (-A): forwards your local ssh-agent to this host. Only enable on hosts you fully trust — root there can use your keys."
        } else {
            "Tab/Shift+Tab or ↑/↓ to move • Type to edit • Enter to save when [ Save ] is selected • Esc to cancel"
        };
        let help = Paragraph::new(Line::from(vec![Span::raw(pj_hint)]))
            .style(Style::default().fg(theme.muted));
        f.render_widget(help, footer_area);
    }
}

pub fn apply_host_form(db: &mut Database, state: &HostFormState) -> Result<(), String> {
    let name = state.name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty".into());
    }
    // Reject names that contain non-printable / control characters — those
    // sneak in via terminal smart-text features and produce confusing aliases.
    if let Some((idx, c)) = name.char_indices().find(|(_, c)| c.is_control()) {
        return Err(format!(
            "Name contains a control character at position {} (U+{:04X}). Disable smart-text in your terminal or retype.",
            idx, c as u32
        ));
    }

    let host = state.host.trim();
    if host.is_empty() {
        return Err("Host cannot be empty".into());
    }

    let port: u16 = state
        .port
        .trim()
        .parse()
        .map_err(|_| format!("Port '{}' is not a valid number 1-65535", state.port.trim()))?;

    if let Some(orig) = &state.original_name {
        if name != orig && db.hosts.contains_key(name) {
            return Err(format!("Host alias '{}' already exists", name));
        }
    } else if db.hosts.contains_key(name) {
        return Err(format!("Host alias '{}' already exists", name));
    }

    let username = state.username.trim();
    let username = if username.is_empty() { "root" } else { username }.to_string();

    let identity_file = if state.identity_file.trim().is_empty() {
        None
    } else {
        Some(state.identity_file.trim().to_string())
    };
    let proxy_jump = if state.proxy_jump.trim().is_empty() {
        None
    } else {
        Some(state.proxy_jump.trim().to_string())
    };
    let folder = if state.folder.trim().is_empty() {
        None
    } else {
        Some(state.folder.trim().to_string())
    };

    let tags = {
        let v = state.tags.trim();
        if v.is_empty() {
            None
        } else {
            let v: Vec<String> = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if v.is_empty() { None } else { Some(v) }
        }
    };

    if state.is_edit {
        if let Some(orig_name) = &state.original_name {
            let (last_connected_at, use_count, favorite, tunnels) = db
                .hosts
                .get(orig_name)
                .map(|h| (h.last_connected_at.clone(), h.use_count, h.favorite, h.tunnels.clone()))
                .unwrap_or((None, 0, false, vec![]));
            db.hosts.remove(orig_name);
            let new_host = Host {
                name: name.to_string(),
                host: host.to_string(),
                port,
                username,
                identity_file,
                proxy_jump,
                folder,
                tags,
                last_connected_at,
                use_count,
                favorite,
                tunnels,
                forward_agent: state.forward_agent,
            };
            db.hosts.insert(new_host.name.clone(), new_host);
        }
    } else {
        let host_obj = Host {
            name: name.to_string(),
            host: host.to_string(),
            port,
            username,
            identity_file,
            proxy_jump,
            folder,
            tags,
            last_connected_at: None,
            use_count: 0,
            favorite: false,
            tunnels: vec![],
            forward_agent: state.forward_agent,
        };
        db.hosts.insert(name.to_string(), host_obj);
    }

    let cfg = load_settings();
    save_and_export(db, &cfg);
    Ok(())
}

pub fn run_host_form(db: &mut Database, mut state: HostFormState) {
    let mut stdout = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        let _ = terminal.draw(|f| draw_host_form(f, &state));

        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Esc => break,
                        KeyCode::Tab | KeyCode::Down => state.next_field(),
                        KeyCode::BackTab | KeyCode::Up => state.prev_field(),
                        KeyCode::Enter => {
                            if state.selected_field == HostFormState::fields_count() {
                                match apply_host_form(db, &state) {
                                    Ok(()) => break,
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
        }
    }

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
}
