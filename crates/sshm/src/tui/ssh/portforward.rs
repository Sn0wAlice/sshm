use crate::models::{Host, Tunnel, TunnelKind};
use crate::tui::ssh::modal::centered_rect;
use crate::tui::ssh::portforward_state::{field, PortForwardForm};
use crate::tui::theme;
use crate::tunnels::build_tunnel_argv;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::collections::HashMap;
use std::io::stdout;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// ============================================================================
// Saved-tunnels picker state
// ============================================================================

enum PickerOutcome {
    /// Start the tunnel in the background and return to the TUI immediately.
    RunBackground(Tunnel),
    /// Run the tunnel on the blocking animated screen (watch mode).
    RunForeground(Tunnel),
    Edit(usize),
    New,
    Cancel,
}

/// Outcome of [`run_port_forward`].
pub struct PortForwardResult {
    /// Updated tunnel list to persist on the host, when it changed.
    pub updated_tunnels: Option<Vec<Tunnel>>,
    /// A tunnel the user asked to start in the background.
    pub start_background: Option<Tunnel>,
}

fn run_tunnel_picker<B: Backend>(
    terminal: &mut Terminal<B>,
    host: &Host,
    tunnels: &mut Vec<Tunnel>,
) -> PickerOutcome {
    let mut state = ListState::default();
    state.select(Some(0));

    loop {
        if tunnels.is_empty() {
            return PickerOutcome::New;
        }

        let _ = terminal.draw(|f| {
            let size = f.area();
            let area = centered_rect(60, 60, size);
            let theme = theme::load();

            f.render_widget(Clear, area);
            let block = Block::default()
                .title(Span::styled(
                    format!(" Saved tunnels - {} ", host.name),
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
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(inner);

            let items: Vec<ListItem> = tunnels.iter().enumerate().map(|(i, t)| {
                let label = if t.label.is_empty() { "(unnamed)".to_string() } else { t.label.clone() };
                let target = match t.kind {
                    TunnelKind::Dynamic => format!("SOCKS on :{}", t.local_port),
                    _ => {
                        let rh = if t.remote_host.is_empty() { "localhost" } else { t.remote_host.as_str() };
                        format!(":{} <-> {}:{}", t.local_port, rh, t.remote_port)
                    }
                };
                ListItem::new(format!(" [{}] {:<22} {:<8} {}",
                    i + 1,
                    label,
                    t.kind.short(),
                    target,
                ))
            }).collect();

            let list = List::new(items)
                .highlight_style(Style::default().bg(theme.accent).fg(theme.bg).add_modifier(Modifier::BOLD))
                .highlight_symbol("> ");
            f.render_stateful_widget(list, chunks[0], &mut state.clone());

            let help = Paragraph::new(
                "  Enter: start (background)  |  f: foreground  |  e: edit  |  d: delete  |  n: new  |  Esc: cancel"
            ).style(Style::default().fg(theme.muted));
            f.render_widget(help, chunks[1]);
        });

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                let sel = state
                    .selected()
                    .unwrap_or(0)
                    .min(tunnels.len().saturating_sub(1));
                match k.code {
                    KeyCode::Esc => return PickerOutcome::Cancel,
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('a') => {
                        return PickerOutcome::New
                    }
                    KeyCode::Char('e') | KeyCode::Char('E') => return PickerOutcome::Edit(sel),
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        if sel < tunnels.len() {
                            tunnels.remove(sel);
                            let new_sel = sel.min(tunnels.len().saturating_sub(1));
                            state.select(if tunnels.is_empty() {
                                None
                            } else {
                                Some(new_sel)
                            });
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(t) = tunnels.get(sel) {
                            return PickerOutcome::RunBackground(t.clone());
                        }
                    }
                    KeyCode::Char('f') | KeyCode::Char('F') => {
                        if let Some(t) = tunnels.get(sel) {
                            return PickerOutcome::RunForeground(t.clone());
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let i = (sel + 1) % tunnels.len();
                        state.select(Some(i));
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        let i = (sel + tunnels.len() - 1) % tunnels.len();
                        state.select(Some(i));
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                        let i = (c as usize) - ('1' as usize);
                        if let Some(t) = tunnels.get(i) {
                            return PickerOutcome::RunBackground(t.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// ============================================================================
// Form draw
// ============================================================================

fn draw_port_form(f: &mut Frame, state: &PortForwardForm, host: &Host) {
    let size = f.area();
    let area = centered_rect(60, 70, size);
    let theme = theme::load();

    let block = Block::default()
        .title(Span::styled(
            format!(" Port Forward - {} ", host.name),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut constraints = vec![
        Constraint::Length(1), // description
        Constraint::Length(1), // spacer
        Constraint::Length(1), // kind row
    ];
    let dyn_mode = state.kind == TunnelKind::Dynamic;
    constraints.push(Constraint::Length(1)); // local port
    if !dyn_mode {
        constraints.push(Constraint::Length(1)); // remote host
        constraints.push(Constraint::Length(1)); // remote port
    }
    constraints.push(Constraint::Length(1)); // label
    constraints.push(Constraint::Length(1)); // save toggle
    constraints.push(Constraint::Length(1)); // auto-start toggle
    constraints.push(Constraint::Length(1)); // auto-restart toggle
    constraints.push(Constraint::Length(1)); // spacer
    constraints.push(Constraint::Length(1)); // start
    constraints.push(Constraint::Length(1)); // spacer
    constraints.push(Constraint::Length(2)); // help / error
    constraints.push(Constraint::Min(0));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
        .split(inner);

    let mut idx = 0;
    let desc = Paragraph::new(format!(
        "  SSH tunnel via {}@{}:{}",
        host.username, host.host, host.port
    ))
    .style(Style::default().fg(theme.muted));
    f.render_widget(desc, chunks[idx]);
    idx += 1;
    idx += 1; // spacer

    // Kind row
    let kind_sel = state.selected_field == field::KIND;
    let kind_style = if kind_sel {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let kind_line = format!("  Type: < {} >  (← → to switch)", state.kind.label());
    f.render_widget(Paragraph::new(kind_line).style(kind_style), chunks[idx]);
    idx += 1;

    // local port
    let lp_sel = state.selected_field == field::LOCAL_PORT;
    let cursor = if lp_sel { "|" } else { "" };
    let lp_label = match state.kind {
        TunnelKind::Dynamic => crate::t!("form.tunnel.socks_port"),
        TunnelKind::Local => crate::t!("form.tunnel.local_port"),
        TunnelKind::Remote => crate::t!("form.tunnel.remote_bind_port"),
    };
    let lp_text = format!("  {}: {}{}", lp_label, state.local_port, cursor);
    let lp_style = if lp_sel {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.fg)
    };
    f.render_widget(Paragraph::new(lp_text).style(lp_style), chunks[idx]);
    idx += 1;

    if !dyn_mode {
        let rh_sel = state.selected_field == field::REMOTE_HOST;
        let rh_text = format!(
            "  {}: {}{}",
            crate::t!("form.tunnel.remote_host"),
            if state.remote_host.is_empty() {
                "localhost"
            } else {
                state.remote_host.as_str()
            },
            if rh_sel { "|" } else { "" }
        );
        let rh_style = if rh_sel {
            Style::default().fg(theme.accent)
        } else if state.remote_host.is_empty() {
            Style::default().fg(theme.muted)
        } else {
            Style::default().fg(theme.fg)
        };
        f.render_widget(Paragraph::new(rh_text).style(rh_style), chunks[idx]);
        idx += 1;

        let rp_sel = state.selected_field == field::REMOTE_PORT;
        let rp_text = format!(
            "  {}: {}{}",
            crate::t!("form.tunnel.remote_port"),
            state.remote_port,
            if rp_sel { "|" } else { "" }
        );
        let rp_style = if rp_sel {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };
        f.render_widget(Paragraph::new(rp_text).style(rp_style), chunks[idx]);
        idx += 1;
    }

    // label
    let lab_sel = state.selected_field == field::LABEL;
    let lab_text = format!(
        "  {}: {}{}",
        crate::t!("form.tunnel.label"),
        state.label,
        if lab_sel { "|" } else { "" }
    );
    let lab_style = if lab_sel {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.fg)
    };
    f.render_widget(Paragraph::new(lab_text).style(lab_style), chunks[idx]);
    idx += 1;

    // save toggle
    let save_sel = state.selected_field == field::SAVE;
    let save_mark = if state.save { "[x]" } else { "[ ]" };
    let save_text = format!("  {} {}", save_mark, crate::t!("form.tunnel.save"));
    let save_style = if save_sel {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    f.render_widget(Paragraph::new(save_text).style(save_style), chunks[idx]);
    idx += 1;

    // auto-start toggle
    let as_sel = state.selected_field == field::AUTO_START;
    let as_mark = if state.auto_start { "[x]" } else { "[ ]" };
    let as_text = format!("  {} {}", as_mark, crate::t!("form.tunnel.auto_start"));
    let as_style = if as_sel {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else if state.auto_start {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.fg)
    };
    f.render_widget(Paragraph::new(as_text).style(as_style), chunks[idx]);
    idx += 1;

    // auto-restart toggle
    let ar_sel = state.selected_field == field::AUTO_RESTART;
    let ar_mark = if state.auto_restart { "[x]" } else { "[ ]" };
    let ar_text = format!("  {} {}", ar_mark, crate::t!("form.tunnel.auto_restart"));
    let ar_style = if ar_sel {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else if state.auto_restart {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.fg)
    };
    f.render_widget(Paragraph::new(ar_text).style(ar_style), chunks[idx]);
    idx += 1;

    idx += 1; // spacer

    // start
    let start_sel = state.selected_field == field::START;
    let start_style = if start_sel {
        Style::default()
            .bg(theme.accent)
            .fg(theme.bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    f.render_widget(
        Paragraph::new(format!("  [ {} ]", crate::t!("form.tunnel.start"))).style(start_style),
        chunks[idx],
    );
    idx += 1;

    idx += 1; // spacer

    // help / error
    let help_para = if let Some(ref err) = state.error {
        Paragraph::new(format!("  {}", err)).style(Style::default().fg(theme.error))
    } else {
        let hint = match state.kind {
            TunnelKind::Local => "  -L: open localhost:LP forwarded to RH:RP via the SSH host.",
            TunnelKind::Remote => {
                "  -R: open <bind>:LP on the SSH host forwarded to RH:RP locally."
            }
            TunnelKind::Dynamic => {
                "  -D: open a SOCKS5 proxy on localhost:LP — point your apps at it."
            }
        };
        Paragraph::new(format!("{}\n  {}", hint, crate::t!("form.tunnel.footer")))
            .style(Style::default().fg(theme.muted))
    };
    f.render_widget(help_para, chunks[idx]);
}

// ============================================================================
// Animated tunnel screen
// ============================================================================

const SPINNER: &[&str] = &["[=   ]", "[ =  ]", "[  = ]", "[   =]", "[  = ]", "[ =  ]"];

fn build_tunnel_lines(left: &str, right: &str, frame_idx: usize) -> Vec<String> {
    let lp_label = format!(" {} ", left);
    let rp_label = format!(" {} ", right);
    let box_w = lp_label.len().max(rp_label.len());
    let lp_padded = format!("{:^bw$}", lp_label, bw = box_w);
    let rp_padded = format!("{:^bw$}", rp_label, bw = box_w);

    let pipe_len: usize = 12;
    let pos = frame_idx % pipe_len;
    let pipe: String = (0..pipe_len)
        .map(|i| {
            if (i + pipe_len - pos) % pipe_len < 3 {
                '▓'
            } else {
                '░'
            }
        })
        .collect();

    let box_h = "─".repeat(box_w);
    let pipe_h = "═".repeat(pipe_len + 2);
    let conn = "───>";
    let conn_sp = "    ";

    let row_top = format!("┌{}┐{}╔{}╗{}┌{}┐", box_h, conn_sp, pipe_h, conn_sp, box_h);
    let row_mid = format!("│{}│{}║ {} ║{}│{}│", lp_padded, conn, pipe, conn, rp_padded);
    let row_bot = format!("└{}┘{}╚{}╝{}└{}┘", box_h, conn_sp, pipe_h, conn_sp, box_h);

    let inner_w = row_top.chars().count();

    let center_in_frame = |s: &str| -> String {
        let slen = s.chars().count();
        let l = inner_w.saturating_sub(slen) / 2;
        let r = inner_w.saturating_sub(slen).saturating_sub(l);
        format!("║ {}{}{} ║", " ".repeat(l), s, " ".repeat(r))
    };

    let border = "═".repeat(inner_w + 2);

    vec![
        format!("╔{}╗", border),
        center_in_frame("LOCAL           TUNNEL           REMOTE"),
        center_in_frame(""),
        center_in_frame(&row_top),
        center_in_frame(&row_mid),
        center_in_frame(&row_bot),
        center_in_frame(""),
        center_in_frame(">>> SSH TUNNEL >>>"),
        format!("╚{}╝", border),
    ]
}

fn build_packet_line(width: usize, frame_idx: usize) -> String {
    let pkt = "~={>=>";
    let gap = 5;
    let shift = frame_idx % (pkt.len() + gap);
    let mut s = String::new();
    let mut pos = shift;
    while pos < width.saturating_sub(pkt.len()) {
        while s.len() < pos {
            s.push(' ');
        }
        s.push_str(pkt);
        pos += pkt.len() + gap;
    }
    s
}

fn draw_tunnel_screen(
    f: &mut Frame,
    host: &Host,
    tunnel: &Tunnel,
    frame_idx: usize,
    elapsed: Duration,
    exit_selected: bool,
) {
    let size = f.area();
    let theme = theme::load();

    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(size);

    let spinner_a = SPINNER[frame_idx % SPINNER.len()];
    let spinner_b = SPINNER[(frame_idx + 3) % SPINNER.len()];
    let title_str = format!(
        "{} {} TUNNEL ACTIVE {}",
        spinner_a,
        tunnel.kind.short(),
        spinner_b
    );
    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            title_str,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ])
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    let forwarding = match tunnel.kind {
        TunnelKind::Dynamic => format!("SOCKS5 on localhost:{}", tunnel.local_port),
        TunnelKind::Local => {
            let rh = if tunnel.remote_host.is_empty() {
                "localhost"
            } else {
                tunnel.remote_host.as_str()
            };
            format!(
                "localhost:{} -> {}:{}",
                tunnel.local_port, rh, tunnel.remote_port
            )
        }
        TunnelKind::Remote => {
            let rh = if tunnel.remote_host.is_empty() {
                "localhost"
            } else {
                tunnel.remote_host.as_str()
            };
            format!(
                "remote:{} -> {}:{}",
                tunnel.local_port, rh, tunnel.remote_port
            )
        }
    };

    let info = Paragraph::new(Line::from(vec![
        Span::styled("Host: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("{}@{}:{}", host.username, host.host, host.port),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  Forwarding: ", Style::default().fg(theme.muted)),
        Span::styled(
            forwarding,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(info, chunks[1]);

    let (left, right) = match tunnel.kind {
        TunnelKind::Dynamic => (format!(":{}", tunnel.local_port), "SOCKS".to_string()),
        TunnelKind::Local => (
            format!(":{}", tunnel.local_port),
            format!(":{}", tunnel.remote_port),
        ),
        TunnelKind::Remote => (
            format!(":{}", tunnel.remote_port),
            format!(":{}", tunnel.local_port),
        ),
    };
    let art_lines = build_tunnel_lines(&left, &right, frame_idx);
    let tunnel_art: Vec<Line> = art_lines
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(theme.accent))))
        .collect();
    f.render_widget(
        Paragraph::new(tunnel_art).alignment(Alignment::Center),
        chunks[3],
    );

    let pkt = build_packet_line(chunks[4].width as usize, frame_idx);
    let pkt_lines = vec![
        Line::from(Span::styled(pkt, Style::default().fg(theme.success))),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(pkt_lines), chunks[4]);

    let dots = ".".repeat((frame_idx % 4) + 1);
    let status = Paragraph::new(Line::from(vec![
        Span::styled("  Status: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("Tunnel active{:<4}", dots),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    f.render_widget(status, chunks[5]);

    let secs = elapsed.as_secs();
    let mins = secs / 60;
    let hrs = mins / 60;
    let time_str = if hrs > 0 {
        format!("{:02}:{:02}:{:02}", hrs, mins % 60, secs % 60)
    } else {
        format!("{:02}:{:02}", mins, secs % 60)
    };
    let timer = Paragraph::new(Line::from(vec![
        Span::styled("  Uptime: ", Style::default().fg(theme.muted)),
        Span::styled(time_str, Style::default().fg(theme.fg)),
    ]));
    f.render_widget(timer, chunks[6]);

    let exit_style = if exit_selected {
        Style::default()
            .bg(theme.error)
            .fg(theme.bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.error)
    };
    f.render_widget(
        Paragraph::new(Span::styled("  [ Exit Tunnel ]", exit_style)),
        chunks[8],
    );
}

// ============================================================================
// Public entry point
// ============================================================================

/// Run the port-forward TUI for a host: pick / create / edit saved tunnels.
///
/// Returns a [`PortForwardResult`] — `updated_tunnels` is set when the saved
/// list changed and should be persisted; `start_background` is set when the
/// user asked to start a tunnel (the caller spawns it via the `TunnelManager`).
///
/// `all_hosts` is used to resolve multi-hop ProxyJump entries by saved-host name.
pub fn run_port_forward(host: &Host, all_hosts: &HashMap<String, Host>) -> PortForwardResult {
    let mut stdout_handle = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout_handle, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut tunnels = host.tunnels.clone();
    let original = tunnels.clone();
    let mut edit_index: Option<usize> = None;

    // Tear the modal down and build the result. `$start` is the tunnel (if
    // any) the caller should spawn in the background.
    macro_rules! finish {
        ($start:expr) => {{
            let _ = disable_raw_mode();
            let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
            return PortForwardResult {
                updated_tunnels: if tunnels != original {
                    Some(tunnels)
                } else {
                    None
                },
                start_background: $start,
            };
        }};
    }

    // --- Phase 0: optional saved tunnels picker ---
    let mut form = if !tunnels.is_empty() {
        match run_tunnel_picker(&mut terminal, host, &mut tunnels) {
            PickerOutcome::Cancel => finish!(None),
            PickerOutcome::New => PortForwardForm::new(),
            PickerOutcome::Edit(i) => {
                edit_index = Some(i);
                PortForwardForm::from_existing(&tunnels[i])
            }
            PickerOutcome::RunBackground(t) => finish!(Some(t)),
            PickerOutcome::RunForeground(t) => {
                run_tunnel_loop(&mut terminal, host, &t, all_hosts);
                finish!(None);
            }
        }
    } else {
        PortForwardForm::new()
    };

    // --- Phase 1: form ---
    let tunnel_def = loop {
        let _ = terminal.draw(|f| draw_port_form(f, &form, host));

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                match k.code {
                    KeyCode::Esc => finish!(None),
                    KeyCode::Tab | KeyCode::Down => form.next_field(),
                    KeyCode::BackTab | KeyCode::Up => form.prev_field(),
                    KeyCode::Left => {
                        if form.selected_field == field::KIND {
                            form.cycle_kind(false);
                        }
                    }
                    KeyCode::Right => {
                        if form.selected_field == field::KIND {
                            form.cycle_kind(true);
                        }
                    }
                    // Space means "flip what's under the cursor": a toggle row
                    // flips, the kind selector advances. On a text row it is a
                    // plain character and falls through to `Char(c)` below.
                    KeyCode::Char(' ') if form.space_is_a_control() => {
                        if !form.toggle_selected() {
                            form.cycle_kind(true);
                        }
                    }
                    KeyCode::Enter => {
                        if form.selected_field == field::START {
                            match form.validate() {
                                Ok(t) => {
                                    if form.save {
                                        match edit_index {
                                            Some(i) if i < tunnels.len() => tunnels[i] = t.clone(),
                                            _ => tunnels.push(t.clone()),
                                        }
                                    }
                                    break t;
                                }
                                Err(e) => {
                                    form.error = Some(e);
                                    continue;
                                }
                            }
                        } else {
                            form.next_field();
                        }
                    }
                    KeyCode::Char(c) => {
                        form.push_char(c);
                        form.error = None;
                    }
                    KeyCode::Backspace => {
                        form.pop_char();
                        form.error = None;
                    }
                    _ => {}
                }
            }
        }
    };

    // The form's [ Start Tunnel ] button starts it in the background.
    finish!(Some(tunnel_def));
}

fn run_tunnel_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    host: &Host,
    tunnel: &Tunnel,
    all_hosts: &HashMap<String, Host>,
) {
    // --- Phase 2: spawn SSH ---
    // Same builder as the background path and the interactive connection.
    let argv = build_tunnel_argv(host, tunnel, all_hosts);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child: Option<Child> = match cmd.spawn() {
        Ok(c) => Some(c),
        Err(e) => {
            let _ = terminal.draw(|f| {
                let theme = theme::load();
                let area = centered_rect(50, 20, f.area());
                f.render_widget(Clear, area);
                let block = Block::default()
                    .title(format!(" {} ", crate::t!("dialog.error")))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.error))
                    .style(Style::default().bg(theme.bg).fg(theme.fg));
                let inner = block.inner(area);
                f.render_widget(block, area);
                f.render_widget(
                    Paragraph::new(format!("Failed to start tunnel: {}\n\nPress any key...", e)),
                    inner,
                );
            });
            loop {
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(Event::Key(_)) = event::read() {
                        break;
                    }
                }
            }
            return;
        }
    };

    // --- Phase 3: animated screen ---
    let start = Instant::now();
    let mut frame_idx: usize = 0;
    let mut last_frame = Instant::now();

    loop {
        if let Some(ref mut c) = child {
            if let Ok(Some(_)) = c.try_wait() {
                child = None;
            }
        }
        if last_frame.elapsed() >= Duration::from_millis(125) {
            frame_idx += 1;
            last_frame = Instant::now();
        }
        let elapsed = start.elapsed();
        let is_alive = child.is_some();

        let _ = terminal.draw(|f| {
            if is_alive {
                draw_tunnel_screen(f, host, tunnel, frame_idx, elapsed, true);
            } else {
                let size = f.area();
                let theme = theme::load();
                f.render_widget(Block::default().style(Style::default().bg(theme.bg)), size);
                let area = centered_rect(50, 30, size);
                f.render_widget(Clear, area);
                let block = Block::default()
                    .title(format!(" {} ", crate::t!("dialog.tunnel_closed.title")))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.error))
                    .style(Style::default().bg(theme.bg).fg(theme.fg));
                let inner = block.inner(area);
                f.render_widget(block, area);
                f.render_widget(
                    Paragraph::new(crate::t!("dialog.tunnel_closed.body")),
                    inner,
                );
            }
        });

        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                if !is_alive {
                    break;
                }
                match k.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('Q') => {
                        if let Some(ref mut c) = child {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    /// Render the form and return the visible text, one string per line.
    ///
    /// The point is less the text than the fact that it renders at all: the
    /// layout builds its constraint list conditionally and walks it with a
    /// running index, so a row added on one side and not the other indexes
    /// past the end and panics. That is exactly the mistake the auto-restart
    /// row could have introduced.
    fn render(state: &PortForwardForm) -> Vec<String> {
        let host = Host {
            name: "web".into(),
            host: "10.0.0.5".into(),
            ..Default::default()
        };
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|f| draw_port_form(f, state, &host))
            .expect("the form must render");
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn rendered_text(state: &PortForwardForm) -> String {
        render(state).join("\n")
    }

    #[test]
    fn a_local_forward_renders_every_row() {
        // Asserted through the translation keys, not English literals: the
        // active locale comes from the environment, so a literal here would
        // make the suite fail on a machine with `LANG=fr`.
        let text = rendered_text(&PortForwardForm::new());
        for key in [
            "form.tunnel.local_port",
            "form.tunnel.remote_host",
            "form.tunnel.remote_port",
            "form.tunnel.label",
            "form.tunnel.start",
        ] {
            let expected = crate::t!(key);
            assert!(text.contains(&expected), "missing {expected:?} in:\n{text}");
        }
    }

    #[test]
    fn a_dynamic_forward_renders_without_the_remote_rows() {
        let mut s = PortForwardForm::new();
        s.kind = TunnelKind::Dynamic;
        let text = rendered_text(&s);
        assert!(
            !text.contains(&crate::t!("form.tunnel.remote_host")),
            "SOCKS has no target host:\n{text}"
        );
        assert!(
            text.contains(&crate::t!("form.tunnel.socks_port")),
            "the port row renames itself for SOCKS:\n{text}"
        );
        assert!(text.contains(&crate::t!("form.tunnel.start")));
    }

    #[test]
    fn every_cursor_position_renders_in_both_layouts() {
        // Walks the whole field list for each kind; an off-by-one in the
        // render's index bookkeeping shows up as a panic here.
        for kind in [TunnelKind::Local, TunnelKind::Remote, TunnelKind::Dynamic] {
            let mut s = PortForwardForm::new();
            s.kind = kind;
            for f in s.visible_fields() {
                s.selected_field = f;
                let _ = render(&s);
            }
        }
    }

    #[test]
    fn the_auto_start_row_shows_its_state() {
        let mut s = PortForwardForm::new();
        let label = crate::t!("form.tunnel.auto_start");
        assert!(rendered_text(&s).contains(&format!("[ ] {label}")));
        s.auto_start = true;
        assert!(rendered_text(&s).contains(&format!("[x] {label}")));
    }

    #[test]
    fn the_auto_restart_row_shows_its_state() {
        let mut s = PortForwardForm::new();
        let label = crate::t!("form.tunnel.auto_restart");
        assert!(rendered_text(&s).contains(&format!("[ ] {label}")));
        s.auto_restart = true;
        assert!(rendered_text(&s).contains(&format!("[x] {label}")));
    }

    #[test]
    fn a_validation_error_is_shown_to_the_user() {
        let mut s = PortForwardForm::new();
        s.error = Some(crate::t!("error.local_port"));
        assert!(rendered_text(&s).contains(&crate::t!("error.local_port")));
    }
}
