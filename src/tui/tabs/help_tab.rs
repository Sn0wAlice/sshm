use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use crate::tui::theme::Theme;

pub struct HelpTabState {
    pub scroll: u16,
}

impl HelpTabState {
    pub fn new() -> Self {
        HelpTabState { scroll: 0 }
    }
}

pub fn handle_help_event(key: KeyCode, state: &mut HelpTabState) {
    match key {
        KeyCode::Down | KeyCode::Char('j') => {
            state.scroll = state.scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.scroll = state.scroll.saturating_sub(1);
        }
        KeyCode::PageDown => {
            state.scroll = state.scroll.saturating_add(10);
        }
        KeyCode::PageUp => {
            state.scroll = state.scroll.saturating_sub(10);
        }
        KeyCode::Home => {
            state.scroll = 0;
        }
        _ => {}
    }
}

const HELP_TEXT: &str = r#"
  SSHM — SSH Host Manager
  ════════════════════════

  A terminal UI to manage, organize, and connect to your SSH hosts.

  ─── Navigation ───────────────────────────────

  ←/→            Switch between tabs (Hosts, Settings, Theme, Help)
  ↑/↓            Move selection up/down
  PageUp/PageDn  Scroll fast
  Home/End       Jump to first/last item
  q              Quit the application

  ─── Hosts Tab ────────────────────────────────

  Enter          Connect to the selected host via SSH
  /              Open the fuzzy search filter
  a              Add a new host (inherits folder context)
  e              Edit the selected host
  d              Delete the selected host or folder
  r              Rename the selected folder
  p              Port forwarding (SSH tunnel)
  i              Manage identity file for the selected host

  ─── Fuzzy Search ─────────────────────────────

  Type any text to filter hosts by name, hostname, username, or tags.
  Results are ranked by relevance (fzf-style).

  Prefix filters:
    name:xxx     Search only by host alias
    host:xxx     Search only by hostname/IP
    user:xxx     Search only by username
    tag:xxx      Search only by tags

  Esc            Clear filter and return to full list

  ─── Folders ──────────────────────────────────

  Hosts can be organized into collapsible folders.
  Folders start collapsed by default.

  Enter          Expand or collapse a folder
  a (on folder)  Add a new host inside that folder
  d (on folder)  Delete the folder (with options for hosts)
  r (on folder)  Rename the folder

  ─── Port Forwarding ─────────────────────────

  Press 'p' on a host to create an SSH tunnel.
  Enter the local port and remote port, then start.
  The tunnel runs with a live animated display.

  Example: local 8080 → remote 80
  This forwards localhost:8080 to remote-host:80.

  Press Esc, Enter, or 'q' to stop the tunnel.

  ─── Settings Tab ─────────────────────────────

  Configure default values for new hosts:
    • Default port (default: 22)
    • Default username (default: root)
    • Default identity file

  ↑/↓ or Tab     Navigate fields
  Type           Edit the selected field
  Enter          Save settings
  Esc            Reset to saved values

  ─── Theme Tab ────────────────────────────────

  Choose from preset themes or create a custom one.

  Presets        Select and press Enter to apply instantly
  Custom Colors  Enter hex values (#RRGGBB) for:
                   Background, Foreground, Accent, Muted
  [ Save Custom ]  Apply your custom colors

  ─── Tips ─────────────────────────────────────

  • The help bar at the bottom shows available keys for the current context
  • Toast notifications appear briefly after actions (save, delete, etc.)
  • Delete confirmations use a modal popup with keyboard navigation
  • All data is stored locally in ~/.config/sshm/
"#;

pub fn draw_help_tab(f: &mut Frame, area: Rect, state: &HelpTabState, theme: &Theme) {
    let block = Block::default()
        .title("Help")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line> = HELP_TEXT
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("───") || l.trim_start().starts_with("═") {
                Line::from(Span::styled(l.to_string(), Style::default().fg(theme.accent)))
            } else if l.contains("SSHM") && l.contains("SSH Host Manager") {
                Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                ))
            } else if l.trim_start().starts_with("•") {
                Line::from(Span::styled(l.to_string(), Style::default().fg(theme.fg)))
            } else {
                // Highlight key bindings (lines where first non-space word is a key)
                let trimmed = l.trim_start();
                if !trimmed.is_empty() && trimmed.contains("  ") {
                    // Split at the first double-space gap
                    let indent = l.len() - trimmed.len();
                    if let Some(gap) = trimmed.find("  ") {
                        let key_part = &trimmed[..gap];
                        let desc_part = &trimmed[gap..];
                        // Only style as key+desc if key_part looks like a shortcut
                        if key_part.len() <= 16 && !key_part.contains('.') {
                            return Line::from(vec![
                                Span::raw(" ".repeat(indent)),
                                Span::styled(
                                    key_part.to_string(),
                                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(desc_part.to_string(), Style::default().fg(theme.muted)),
                            ]);
                        }
                    }
                }
                Line::from(Span::styled(l.to_string(), Style::default().fg(theme.fg)))
            }
        })
        .collect();

    let total_lines = lines.len() as u16;
    let visible = inner.height;
    let max_scroll = total_lines.saturating_sub(visible);

    let scroll = state.scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    f.render_widget(paragraph, inner);

    // Scrollbar
    if total_lines > visible {
        let mut sb_state = ScrollbarState::new(total_lines as usize)
            .position(scroll as usize);
        let sb = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
        f.render_stateful_widget(sb, inner, &mut sb_state);
    }
}
