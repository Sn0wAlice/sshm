use std::collections::HashMap;
use std::io::stdout;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
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
use crate::models::{Host, Tunnel, TunnelKind};
use crate::ssh::proxy::resolve_proxy_jump;
use crate::tui::theme;
use crate::tui::ssh::modal::centered_rect;

// ============================================================================
// Form state
// ============================================================================

struct PortForwardForm {
    kind: TunnelKind,
    local_port: String,
    remote_host: String,
    remote_port: String,
    label: String,
    save: bool,
    selected_field: usize,
    error: Option<String>,
}

impl PortForwardForm {
    fn new() -> Self {
        Self {
            kind: TunnelKind::Local,
            local_port: String::new(),
            remote_host: String::new(),
            remote_port: String::new(),
            label: String::new(),
            save: false,
            selected_field: 0,
            error: None,
        }
    }

    fn from_existing(t: &Tunnel) -> Self {
        Self {
            kind: t.kind,
            local_port: t.local_port.to_string(),
            remote_host: t.remote_host.clone(),
            remote_port: if t.kind == TunnelKind::Dynamic { String::new() } else { t.remote_port.to_string() },
            label: t.label.clone(),
            save: true,
            selected_field: 0,
            error: None,
        }
    }

    /// Field layout depends on kind. Returns the slice of field indices currently shown.
    /// Order:
    ///   0: kind selector
    ///   1: local port
    ///   2: remote host (Local/Remote only)
    ///   3: remote port (Local/Remote only)
    ///   4: label
    ///   5: save toggle
    ///   6: start button
    fn visible_fields(&self) -> Vec<usize> {
        match self.kind {
            TunnelKind::Dynamic => vec![0, 1, 4, 5, 6],
            _ => vec![0, 1, 2, 3, 4, 5, 6],
        }
    }

    fn next_field(&mut self) {
        let visible = self.visible_fields();
        let idx = visible.iter().position(|&f| f == self.selected_field).unwrap_or(0);
        self.selected_field = visible[(idx + 1) % visible.len()];
    }

    fn prev_field(&mut self) {
        let visible = self.visible_fields();
        let idx = visible.iter().position(|&f| f == self.selected_field).unwrap_or(0);
        self.selected_field = visible[(idx + visible.len() - 1) % visible.len()];
    }

    fn cycle_kind(&mut self, forward: bool) {
        let order = [TunnelKind::Local, TunnelKind::Remote, TunnelKind::Dynamic];
        let idx = order.iter().position(|k| *k == self.kind).unwrap_or(0);
        let next = if forward {
            (idx + 1) % order.len()
        } else {
            (idx + order.len() - 1) % order.len()
        };
        self.kind = order[next];
        // If we landed on a hidden field, snap back to a visible one.
        if !self.visible_fields().contains(&self.selected_field) {
            self.selected_field = 0;
        }
    }

    fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.selected_field {
            1 => Some(&mut self.local_port),
            2 => Some(&mut self.remote_host),
            3 => Some(&mut self.remote_port),
            4 => Some(&mut self.label),
            _ => None,
        }
    }

    fn push_char(&mut self, c: char) {
        let is_port_field = matches!(self.selected_field, 1 | 3);
        if is_port_field && !c.is_ascii_digit() {
            return;
        }
        if let Some(field) = self.active_value_mut() {
            field.push(c);
        }
    }

    fn pop_char(&mut self) {
        if let Some(field) = self.active_value_mut() {
            field.pop();
        }
    }

    fn validate(&self) -> Result<Tunnel, String> {
        let lp: u16 = self.local_port.trim().parse()
            .map_err(|_| "Local port must be a number 1-65535".to_string())?;
        match self.kind {
            TunnelKind::Dynamic => Ok(Tunnel {
                label: self.label.trim().to_string(),
                kind: TunnelKind::Dynamic,
                local_port: lp,
                remote_port: 0,
                remote_host: String::new(),
            }),
            kind => {
                let rp: u16 = self.remote_port.trim().parse()
                    .map_err(|_| "Remote port must be a number 1-65535".to_string())?;
                Ok(Tunnel {
                    label: self.label.trim().to_string(),
                    kind,
                    local_port: lp,
                    remote_port: rp,
                    remote_host: self.remote_host.trim().to_string(),
                })
            }
        }
    }
}

// ============================================================================
// Saved-tunnels picker state
// ============================================================================

enum PickerOutcome {
    Run(Tunnel),
    Edit(usize),
    New,
    Cancel,
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
                "  Enter: run  |  e: edit  |  d: delete  |  n: new tunnel  |  Esc: cancel"
            ).style(Style::default().fg(theme.muted));
            f.render_widget(help, chunks[1]);
        });

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press { continue; }
                let sel = state.selected().unwrap_or(0).min(tunnels.len().saturating_sub(1));
                match k.code {
                    KeyCode::Esc => return PickerOutcome::Cancel,
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('a') => return PickerOutcome::New,
                    KeyCode::Char('e') | KeyCode::Char('E') => return PickerOutcome::Edit(sel),
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        if sel < tunnels.len() {
                            tunnels.remove(sel);
                            let new_sel = sel.min(tunnels.len().saturating_sub(1));
                            state.select(if tunnels.is_empty() { None } else { Some(new_sel) });
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(t) = tunnels.get(sel) {
                            return PickerOutcome::Run(t.clone());
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
                            return PickerOutcome::Run(t.clone());
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
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
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
    let desc = Paragraph::new(format!("  SSH tunnel via {}@{}:{}", host.username, host.host, host.port))
        .style(Style::default().fg(theme.muted));
    f.render_widget(desc, chunks[idx]); idx += 1;
    idx += 1; // spacer

    // Kind row
    let kind_sel = state.selected_field == 0;
    let kind_style = if kind_sel {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    let kind_line = format!("  Type: < {} >  (← → to switch)", state.kind.label());
    f.render_widget(Paragraph::new(kind_line).style(kind_style), chunks[idx]); idx += 1;

    // local port
    let lp_sel = state.selected_field == 1;
    let cursor = if lp_sel { "|" } else { "" };
    let lp_label = match state.kind {
        TunnelKind::Dynamic => "SOCKS Port",
        TunnelKind::Local => "Local Port",
        TunnelKind::Remote => "Remote Bind Port",
    };
    let lp_text = format!("  {}: {}{}", lp_label, state.local_port, cursor);
    let lp_style = if lp_sel { Style::default().fg(theme.accent) } else { Style::default().fg(theme.fg) };
    f.render_widget(Paragraph::new(lp_text).style(lp_style), chunks[idx]); idx += 1;

    if !dyn_mode {
        let rh_sel = state.selected_field == 2;
        let rh_text = format!("  Remote Host: {}{}",
            if state.remote_host.is_empty() { "localhost" } else { state.remote_host.as_str() },
            if rh_sel { "|" } else { "" }
        );
        let rh_style = if rh_sel { Style::default().fg(theme.accent) }
            else if state.remote_host.is_empty() { Style::default().fg(theme.muted) }
            else { Style::default().fg(theme.fg) };
        f.render_widget(Paragraph::new(rh_text).style(rh_style), chunks[idx]); idx += 1;

        let rp_sel = state.selected_field == 3;
        let rp_text = format!("  Remote Port: {}{}", state.remote_port, if rp_sel { "|" } else { "" });
        let rp_style = if rp_sel { Style::default().fg(theme.accent) } else { Style::default().fg(theme.fg) };
        f.render_widget(Paragraph::new(rp_text).style(rp_style), chunks[idx]); idx += 1;
    }

    // label
    let lab_sel = state.selected_field == 4;
    let lab_text = format!("  Label (optional): {}{}", state.label, if lab_sel { "|" } else { "" });
    let lab_style = if lab_sel { Style::default().fg(theme.accent) } else { Style::default().fg(theme.fg) };
    f.render_widget(Paragraph::new(lab_text).style(lab_style), chunks[idx]); idx += 1;

    // save toggle
    let save_sel = state.selected_field == 5;
    let save_mark = if state.save { "[x]" } else { "[ ]" };
    let save_text = format!("  {} Save this tunnel on host (Space to toggle)", save_mark);
    let save_style = if save_sel { Style::default().fg(theme.accent).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme.fg) };
    f.render_widget(Paragraph::new(save_text).style(save_style), chunks[idx]); idx += 1;

    idx += 1; // spacer

    // start
    let start_sel = state.selected_field == 6;
    let start_style = if start_sel {
        Style::default().bg(theme.accent).fg(theme.bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    f.render_widget(Paragraph::new("  [ Start Tunnel ]").style(start_style), chunks[idx]); idx += 1;

    idx += 1; // spacer

    // help / error
    let help_para = if let Some(ref err) = state.error {
        Paragraph::new(format!("  {}", err)).style(Style::default().fg(theme.error))
    } else {
        let hint = match state.kind {
            TunnelKind::Local => "  -L: open localhost:LP forwarded to RH:RP via the SSH host.",
            TunnelKind::Remote => "  -R: open <bind>:LP on the SSH host forwarded to RH:RP locally.",
            TunnelKind::Dynamic => "  -D: open a SOCKS5 proxy on localhost:LP — point your apps at it.",
        };
        Paragraph::new(format!("{}\n  Tab/↑↓ navigate  |  Esc cancel", hint))
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
        .map(|i| if (i + pipe_len - pos) % pipe_len < 3 { '▓' } else { '░' })
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

    f.render_widget(
        Block::default().style(Style::default().bg(theme.bg)),
        size,
    );

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
        spinner_a, tunnel.kind.short(), spinner_b
    );
    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            title_str,
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ]).alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    let forwarding = match tunnel.kind {
        TunnelKind::Dynamic => format!("SOCKS5 on localhost:{}", tunnel.local_port),
        TunnelKind::Local => {
            let rh = if tunnel.remote_host.is_empty() { "localhost" } else { tunnel.remote_host.as_str() };
            format!("localhost:{} -> {}:{}", tunnel.local_port, rh, tunnel.remote_port)
        }
        TunnelKind::Remote => {
            let rh = if tunnel.remote_host.is_empty() { "localhost" } else { tunnel.remote_host.as_str() };
            format!("remote:{} -> {}:{}", tunnel.local_port, rh, tunnel.remote_port)
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
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
    ])).alignment(Alignment::Center);
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
    f.render_widget(Paragraph::new(tunnel_art).alignment(Alignment::Center), chunks[3]);

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
            Style::default().fg(theme.success).add_modifier(Modifier::BOLD),
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
        Style::default().bg(theme.error).fg(theme.bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.error)
    };
    f.render_widget(
        Paragraph::new(Span::styled("  [ Exit Tunnel ]", exit_style)),
        chunks[8],
    );
}

// ============================================================================
// SSH command builder
// ============================================================================

fn build_forward_arg(t: &Tunnel) -> Vec<String> {
    match t.kind {
        TunnelKind::Local => {
            let rh = if t.remote_host.is_empty() { "localhost".to_string() } else { t.remote_host.clone() };
            vec!["-L".into(), format!("{}:{}:{}", t.local_port, rh, t.remote_port)]
        }
        TunnelKind::Remote => {
            let rh = if t.remote_host.is_empty() { "localhost".to_string() } else { t.remote_host.clone() };
            vec!["-R".into(), format!("{}:{}:{}", t.local_port, rh, t.remote_port)]
        }
        TunnelKind::Dynamic => {
            vec!["-D".into(), t.local_port.to_string()]
        }
    }
}

// ============================================================================
// Public entry point
// ============================================================================

/// Run the port-forward TUI for a host.
///
/// Returns `Some(updated_tunnels)` when the host's tunnel list was modified
/// (saved/edited/deleted) and should be persisted by the caller; otherwise `None`.
///
/// `all_hosts` is used to resolve multi-hop ProxyJump entries by saved-host name.
pub fn run_port_forward(
    host: &Host,
    all_hosts: &HashMap<String, Host>,
) -> Option<Vec<Tunnel>> {
    let mut stdout_handle = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout_handle, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut tunnels = host.tunnels.clone();
    let original = tunnels.clone();
    let mut tunnels_dirty = false;
    let mut edit_index: Option<usize> = None;

    // --- Phase 0: optional saved tunnels picker ---
    let mut form = if !tunnels.is_empty() {
        match run_tunnel_picker(&mut terminal, host, &mut tunnels) {
            PickerOutcome::Cancel => {
                let _ = disable_raw_mode();
                let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                if tunnels != original { return Some(tunnels); }
                return None;
            }
            PickerOutcome::New => PortForwardForm::new(),
            PickerOutcome::Edit(i) => {
                edit_index = Some(i);
                PortForwardForm::from_existing(&tunnels[i])
            }
            PickerOutcome::Run(t) => {
                if tunnels != original { tunnels_dirty = true; }
                run_tunnel_loop(&mut terminal, host, &t, all_hosts);
                let _ = disable_raw_mode();
                let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                if tunnels_dirty { return Some(tunnels); }
                return None;
            }
        }
    } else {
        PortForwardForm::new()
    };

    if tunnels != original { tunnels_dirty = true; }

    // --- Phase 1: form ---
    let tunnel_def = loop {
        let _ = terminal.draw(|f| draw_port_form(f, &form, host));

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press { continue; }
                match k.code {
                    KeyCode::Esc => {
                        let _ = disable_raw_mode();
                        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                        if tunnels_dirty { return Some(tunnels); }
                        return None;
                    }
                    KeyCode::Tab | KeyCode::Down => form.next_field(),
                    KeyCode::BackTab | KeyCode::Up => form.prev_field(),
                    KeyCode::Left => {
                        if form.selected_field == 0 { form.cycle_kind(false); }
                    }
                    KeyCode::Right => {
                        if form.selected_field == 0 { form.cycle_kind(true); }
                    }
                    KeyCode::Char(' ') if form.selected_field == 5 => {
                        form.save = !form.save;
                    }
                    KeyCode::Char(' ') if form.selected_field == 0 => {
                        form.cycle_kind(true);
                    }
                    KeyCode::Enter => {
                        if form.selected_field == 6 {
                            match form.validate() {
                                Ok(t) => {
                                    if form.save {
                                        match edit_index {
                                            Some(i) if i < tunnels.len() => tunnels[i] = t.clone(),
                                            _ => tunnels.push(t.clone()),
                                        }
                                        tunnels_dirty = true;
                                    }
                                    break t;
                                }
                                Err(e) => { form.error = Some(e); continue; }
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

    run_tunnel_loop(&mut terminal, host, &tunnel_def, all_hosts);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);

    if tunnels_dirty { Some(tunnels) } else { None }
}

fn run_tunnel_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    host: &Host,
    tunnel: &Tunnel,
    all_hosts: &HashMap<String, Host>,
) {
    // --- Phase 2: spawn SSH ---
    let mut cmd = Command::new("ssh");
    cmd.arg("-N");
    for a in build_forward_arg(tunnel) { cmd.arg(a); }
    cmd.arg(format!("{}@{}", host.username, host.host))
        .arg("-p").arg(host.port.to_string());

    if let Some(ref id) = host.identity_file {
        if !id.is_empty() { cmd.arg("-i").arg(id); }
    }
    if let Some(ref j) = host.proxy_jump {
        if let Some(resolved) = resolve_proxy_jump(j, all_hosts) {
            cmd.arg("-J").arg(resolved);
        }
    }

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
                    .title(" Error ")
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
                    if let Ok(Event::Key(_)) = event::read() { break; }
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
            if let Ok(Some(_)) = c.try_wait() { child = None; }
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
                    .title(" Tunnel Closed ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.error))
                    .style(Style::default().bg(theme.bg).fg(theme.fg));
                let inner = block.inner(area);
                f.render_widget(block, area);
                f.render_widget(
                    Paragraph::new("SSH tunnel process exited.\n\nPress any key to return..."),
                    inner,
                );
            }
        });

        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind != KeyEventKind::Press { continue; }
                if !is_alive { break; }
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
