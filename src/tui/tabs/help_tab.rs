use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use crate::tui::theme::Theme;

#[derive(Default)]
pub struct HelpTabState {
    pub scroll: u16,
}

impl HelpTabState {
    pub fn new() -> Self {
        Self::default()
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
  c              Check host reachability (TCP ping)
  i              Manage identity file for the selected host

  Examples:
    • Select "web-prod" → Enter    → opens ssh root@10.0.1.5
    • Select "web-prod" → e        → edit name, host, port, user, tags…
    • Select a folder   → a        → new host is created inside that folder
    • Select "old-box"  → d        → confirmation modal, then deleted

  ─── Host Status Check ──────────────────────────

  Press 'c' on any host to perform a TCP connection check.
  The host text turns green (reachable) or red (unreachable).
  A colored dot (●) appears next to checked hosts.
  The Details panel border also changes color.

  Examples:
    • Select "web-prod" → c → turns green if port 22 responds
    • Select "old-box"  → c → turns red if host is down or filtered
    • Check several hosts in a row to get a quick status overview

  ─── Fuzzy Search ─────────────────────────────

  Type any text to filter hosts by name, hostname, username, or tags.
  Results are ranked by relevance (fzf-style).

  Prefix filters:
    name:xxx     Search only by host alias
    host:xxx     Search only by hostname/IP
    user:xxx     Search only by username
    tag:xxx      Search only by tags

  Esc            Clear filter and return to full list

  Examples:
    • /prod             → matches "web-prod", "db-prod", "prod-api"
    • /10.0             → matches any host with IP starting with 10.0
    • /host:192.168     → only matches hostnames containing "192.168"
    • /tag:docker       → only matches hosts tagged "docker"
    • /user:deploy      → only matches hosts with username "deploy"
    • /name:api         → only matches host aliases containing "api"
    • /web              → fuzzy: matches "web-prod", "website", "aweb"

  ─── Folders ──────────────────────────────────

  Hosts can be organized into collapsible folders.
  Folders support up to 2 levels of nesting using "/" notation.
  Folders start collapsed by default.

  Enter          Expand or collapse a folder
  a (on folder)  Add a new host inside that folder
  d (on folder)  Delete the folder (with options for hosts)
  r (on folder)  Rename the folder

  Examples:
    Folder structure with 2 levels:

    ▸ Production           ← top-level folder (collapsed)
    ▾ Staging              ← top-level folder (expanded)
        ▸ Staging/Web      ← sub-folder (collapsed)
        ▾ Staging/DB       ← sub-folder (expanded)
            db-staging-1   ← host inside Staging/DB
            db-staging-2
        api-staging        ← host directly in Staging

    • Set folder to "Production"     → host goes in Production
    • Set folder to "Production/Web" → host goes in sub-folder
    • Rename "Staging" to "QA"       → all sub-folders update too
    • Delete "Production"            → removes all sub-folders inside

  ─── Port Forwarding ─────────────────────────

  Press 'p' on a host to create an SSH tunnel.
  Enter the local port and remote port, then start.
  The tunnel runs with a live animated display.

  Press Esc, Enter, or 'q' to stop the tunnel.

  Examples:
    • local 8080 → remote 80     Access remote HTTP on localhost:8080
    • local 5432 → remote 5432   Tunnel to a remote PostgreSQL
    • local 3306 → remote 3306   Tunnel to a remote MySQL
    • local 6379 → remote 6379   Tunnel to a remote Redis
    • local 9090 → remote 443    Access remote HTTPS on localhost:9090

  ─── Settings Tab ─────────────────────────────

  Configure default values for new hosts:
    • Default port (default: 22)
    • Default username (default: root)
    • Default identity file
    • Export path

  ↑/↓ or Tab     Navigate fields
  Type           Edit the selected field
  Enter          Save settings
  Esc            Reset to saved values

  Export path:
    Automatically exports all hosts in ~/.ssh/config format on save.
    Leave empty to disable. Supports ~ expansion.

  Examples:
    • Export path: ~/.ssh/config           → overwrites your SSH config
    • Export path: ~/my-ssh-config         → safe separate file
    • Export path: /tmp/ssh_hosts          → temp export for testing
    • Empty                                → auto-export disabled

  ─── Theme Tab ────────────────────────────────

  Choose from preset themes or create a custom one.

  Presets        Select and press Enter to apply instantly
  Custom Colors  Enter hex values (#RRGGBB) for:
                   Background, Foreground, Accent, Muted, Error, Success
  [ Save Custom ]  Apply your custom colors

  Examples:
    • Background: #1a1b26     Dark background (Tokyo Night style)
    • Foreground: #c0caf5     Light text
    • Accent:     #7aa2f7     Blue highlights
    • Muted:      #565f89     Dimmed hints
    • Error:      #f7768e     Red for errors and unreachable hosts
    • Success:    #9ece6a     Green for success and reachable hosts

  ─── CLI Quick Reference ────────────────────────

  sshm                              Launch the TUI (default)
  sshm list                         List all hosts
  sshm list --filter "prod"         List hosts matching "prod"
  sshm connect myserver             SSH into "myserver"
  sshm c myserver                   Short alias for connect
  sshm c myserver -L 8080:localhost:80
                                    Connect with local port forward
  sshm c myserver -i ~/.ssh/id_rsa  Connect with specific key
  sshm c myserver -J jumphost       Connect via jump host
  sshm create                       Add a new host interactively
  sshm edit                         Edit a host interactively
  sshm delete                       Delete a host interactively
  sshm tag add myserver web,prod    Add tags "web" and "prod"
  sshm tag del myserver old         Remove tag "old"
  sshm export ~/ssh-backup          Export hosts as SSH config
  sshm export                       Export using configured path
  sshm load_local_conf              Import hosts from ~/.ssh/config
  sshm add-identity myserver        Push your pubkey to a host
  sshm add-identity myserver --pub ~/.ssh/id_ed25519.pub
                                    Push a specific public key
  sshm help                         Show CLI help

  ─── Tips ─────────────────────────────────────

  • The help bar at the bottom shows available keys for the current context
  • Toast notifications appear briefly after actions (save, delete, etc.)
  • Delete confirmations use a modal popup with keyboard navigation
  • All data is stored locally in ~/.config/sshm/
  • You can import your existing ~/.ssh/config with: sshm load_local_conf
  • Tags let you group hosts logically (e.g. "docker", "prod", "gpu")
  • Use 'c' on multiple hosts to quickly audit which ones are alive

  ─── Thanks ─────────────────────────────────────
  All the crazy people who force me to update this shit:
  - @N1-gHT
  - @Batleforc
  - myself of course




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
