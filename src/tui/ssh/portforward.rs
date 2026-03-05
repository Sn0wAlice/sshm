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
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};
use crate::models::Host;
use crate::tui::theme;
use crate::tui::ssh::modal::centered_rect;

// ============================================================================
// Port forward form state
// ============================================================================

struct PortForwardForm {
    local_port: String,
    remote_port: String,
    selected_field: usize,
    error: Option<String>,
}

impl PortForwardForm {
    fn new() -> Self {
        PortForwardForm {
            local_port: String::new(),
            remote_port: String::new(),
            selected_field: 0,
            error: None,
        }
    }

    fn fields_count() -> usize { 2 }

    fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % (Self::fields_count() + 1);
    }

    fn prev_field(&mut self) {
        if self.selected_field == 0 {
            self.selected_field = Self::fields_count();
        } else {
            self.selected_field -= 1;
        }
    }

    fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.selected_field {
            0 => Some(&mut self.local_port),
            1 => Some(&mut self.remote_port),
            _ => None,
        }
    }

    fn push_char(&mut self, c: char) {
        if c.is_ascii_digit() {
            if let Some(field) = self.active_value_mut() {
                field.push(c);
            }
        }
    }

    fn pop_char(&mut self) {
        if let Some(field) = self.active_value_mut() {
            field.pop();
        }
    }
}

// ============================================================================
// Dynamic tunnel rendering
// ============================================================================

const SPINNER: &[&str] = &["[=   ]", "[ =  ]", "[  = ]", "[   =]", "[  = ]", "[ =  ]"];

fn build_tunnel_lines(lp: &str, rp: &str, frame_idx: usize) -> Vec<String> {
    let lp_label = format!(" :{} ", lp);
    let rp_label = format!(" :{} ", rp);
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

// ============================================================================
// Draw functions
// ============================================================================

fn draw_port_form(f: &mut Frame, state: &PortForwardForm, host: &Host) {
    let size = f.area();
    let area = centered_rect(50, 45, size);
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // description
            Constraint::Length(1), // spacer
            Constraint::Length(1), // local port
            Constraint::Length(1), // remote port
            Constraint::Length(1), // spacer
            Constraint::Length(1), // start button
            Constraint::Length(1), // spacer
            Constraint::Length(1), // error / help
            Constraint::Min(0),
        ])
        .split(inner);

    let desc = Paragraph::new(format!("  SSH tunnel to {}@{}:{}", host.username, host.host, host.port))
        .style(Style::default().fg(theme.muted));
    f.render_widget(desc, chunks[0]);

    let labels = ["Local Port", "Remote Port"];
    let values = [&state.local_port, &state.remote_port];

    for (i, (label, value)) in labels.iter().zip(values.iter()).enumerate() {
        let is_sel = state.selected_field == i;
        let cursor = if is_sel { "|" } else { "" };
        let style = if is_sel {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };
        let text = format!("  {}: {}{}", label, value, cursor);
        f.render_widget(Paragraph::new(text).style(style), chunks[2 + i]);
    }

    let is_start = state.selected_field == PortForwardForm::fields_count();
    let start_style = if is_start {
        Style::default().bg(theme.accent).fg(theme.bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    };
    f.render_widget(Paragraph::new("  [ Start Tunnel ]").style(start_style), chunks[5]);

    let help_text = if let Some(ref err) = state.error {
        Paragraph::new(format!("  {}", err)).style(Style::default().fg(theme.error))
    } else {
        Paragraph::new("  Ex: local 8080 -> remote 80  |  Esc to cancel")
            .style(Style::default().fg(theme.muted))
    };
    f.render_widget(help_text, chunks[7]);
}

fn draw_tunnel_screen(
    f: &mut Frame,
    host: &Host,
    local_port: &str,
    remote_port: &str,
    frame_idx: usize,
    elapsed: Duration,
    exit_selected: bool,
) {
    let size = f.area();
    let theme = theme::load();

    // Full screen background
    f.render_widget(
        Block::default().style(Style::default().bg(theme.bg)),
        size,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),  // title
            Constraint::Length(1),  // connection info
            Constraint::Length(1),  // spacer
            Constraint::Length(9),  // ASCII art (dynamic)
            Constraint::Length(2),  // packet animation
            Constraint::Length(1),  // status line
            Constraint::Length(1),  // timer
            Constraint::Length(1),  // spacer
            Constraint::Length(1),  // exit button
            Constraint::Min(0),
        ])
        .split(size);

    // Title (centered by ratatui)
    let spinner_a = SPINNER[frame_idx % SPINNER.len()];
    let spinner_b = SPINNER[(frame_idx + 3) % SPINNER.len()];
    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("{} TUNNEL ACTIVE {}", spinner_a, spinner_b),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ]).alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    // Connection info (centered by ratatui)
    let info = Paragraph::new(Line::from(vec![
        Span::styled("Host: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("{}@{}:{}", host.username, host.host, host.port),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  Forwarding: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("localhost:{} -> remote:{}", local_port, remote_port),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
    ])).alignment(Alignment::Center);
    f.render_widget(info, chunks[1]);

    // Dynamic ASCII tunnel art (centered by ratatui)
    let art_lines = build_tunnel_lines(local_port, remote_port, frame_idx);
    let tunnel: Vec<Line> = art_lines
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(theme.accent))))
        .collect();
    f.render_widget(Paragraph::new(tunnel).alignment(Alignment::Center), chunks[3]);

    // Packet animation
    let pkt = build_packet_line(chunks[4].width as usize, frame_idx);
    let pkt_lines = vec![
        Line::from(Span::styled(pkt, Style::default().fg(theme.success))),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(pkt_lines), chunks[4]);

    // Status
    let dots = ".".repeat((frame_idx % 4) + 1);
    let status = Paragraph::new(Line::from(vec![
        Span::styled("  Status: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("Tunnel active{:<4}", dots),
            Style::default().fg(theme.success).add_modifier(Modifier::BOLD),
        ),
    ]));
    f.render_widget(status, chunks[5]);

    // Timer
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

    // Exit button
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
// Public entry point
// ============================================================================

pub fn run_port_forward(host: &Host) {
    // --- Phase 1: Port input form ---
    let mut form = PortForwardForm::new();

    let mut stdout_handle = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout_handle, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend).unwrap();

    let (local_port, remote_port) = loop {
        let _ = terminal.draw(|f| draw_port_form(f, &form, host));

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Esc => {
                            let _ = disable_raw_mode();
                            let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                            return;
                        }
                        KeyCode::Tab | KeyCode::Down => form.next_field(),
                        KeyCode::BackTab | KeyCode::Up => form.prev_field(),
                        KeyCode::Enter => {
                            if form.selected_field == PortForwardForm::fields_count() {
                                // Validate
                                let lp = form.local_port.trim().to_string();
                                let rp = form.remote_port.trim().to_string();
                                if lp.is_empty() || rp.is_empty() {
                                    form.error = Some("Both ports are required".into());
                                    continue;
                                }
                                match (lp.parse::<u16>(), rp.parse::<u16>()) {
                                    (Ok(_), Ok(_)) => break (lp, rp),
                                    _ => {
                                        form.error = Some("Invalid port number (1-65535)".into());
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
        }
    };

    // --- Phase 2: Launch SSH tunnel process ---
    let mut cmd = Command::new("ssh");
    cmd.arg("-N") // no remote command
        .arg("-L")
        .arg(format!("{}:localhost:{}", local_port, remote_port))
        .arg(format!("{}@{}", host.username, host.host))
        .arg("-p")
        .arg(host.port.to_string());

    if let Some(ref id) = host.identity_file {
        cmd.arg("-i").arg(id);
    }
    if let Some(ref j) = host.proxy_jump {
        cmd.arg("-J").arg(j);
    }

    // Detach stdin/stdout so it runs in background
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child: Option<Child> = match cmd.spawn() {
        Ok(c) => Some(c),
        Err(e) => {
            // Show error briefly then return
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
                    if let Ok(Event::Key(_)) = event::read() {
                        break;
                    }
                }
            }
            let _ = disable_raw_mode();
            let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
            return;
        }
    };

    // --- Phase 3: Animated tunnel screen ---
    let start = Instant::now();
    let mut frame_idx: usize = 0;
    let mut last_frame = Instant::now();

    loop {
        // Check if SSH process died
        if let Some(ref mut c) = child {
            if let Ok(Some(_status)) = c.try_wait() {
                child = None;
            }
        }

        // Animate at ~8 FPS
        if last_frame.elapsed() >= Duration::from_millis(125) {
            frame_idx += 1;
            last_frame = Instant::now();
        }

        let elapsed = start.elapsed();
        let is_alive = child.is_some();

        let _ = terminal.draw(|f| {
            if is_alive {
                draw_tunnel_screen(f, host, &local_port, &remote_port, frame_idx, elapsed, true);
            } else {
                // Tunnel died - show dead state
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
                if k.kind == KeyEventKind::Press {
                    if !is_alive {
                        // Any key to exit
                        break;
                    }
                    match k.code {
                        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('Q') => {
                            // Kill the tunnel
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

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
}
