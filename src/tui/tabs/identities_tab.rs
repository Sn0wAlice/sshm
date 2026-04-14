//! Identities tab — manage local SSH keys and the running ssh-agent.
//!
//! v1 scope:
//! - list private keys found under `~/.ssh` (with fingerprint / type / comment
//!   / "is in agent" flag)
//! - generate new keys via `ssh-keygen`
//! - push the selected public key to a managed host via `ssh-copy-id`
//! - add / remove the selected key to/from ssh-agent
//! - clean stale `known_hosts` entries via `ssh-keygen -R`
//!
//! NOTE: password-manager integration (1Password / Bitwarden / pass) for
//! passphrases is deliberately out of scope for this version — each provider
//! has its own auth flow and deserves its own feature ticket.

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::ssh::keys::{scan_ssh_dir, KeyEntry};
use crate::tui::theme::Theme;

pub struct IdentitiesTabState {
    pub keys: Vec<KeyEntry>,
    pub selected: usize,
}

impl Default for IdentitiesTabState {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentitiesTabState {
    pub fn new() -> Self {
        let keys = scan_ssh_dir();
        IdentitiesTabState { keys, selected: 0 }
    }

    pub fn refresh(&mut self) {
        self.keys = scan_ssh_dir();
        if self.keys.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.keys.len() {
            self.selected = self.keys.len() - 1;
        }
    }

    pub fn selected_key(&self) -> Option<&KeyEntry> {
        self.keys.get(self.selected)
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.keys.len() {
            self.selected += 1;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

pub enum IdentitiesAction {
    None,
    Refresh,
    Generate,
    Push,
    AgentAdd,
    AgentRemove,
    KnownHostsClean,
}

pub fn handle_identities_event(
    key: KeyCode,
    state: &mut IdentitiesTabState,
) -> IdentitiesAction {
    match key {
        KeyCode::Up => {
            state.move_up();
            IdentitiesAction::None
        }
        KeyCode::Down => {
            state.move_down();
            IdentitiesAction::None
        }
        KeyCode::Char('r') => IdentitiesAction::Refresh,
        KeyCode::Char('g') => IdentitiesAction::Generate,
        KeyCode::Char('p') => IdentitiesAction::Push,
        KeyCode::Char('a') => IdentitiesAction::AgentAdd,
        KeyCode::Char('x') => IdentitiesAction::AgentRemove,
        KeyCode::Char('K') => IdentitiesAction::KnownHostsClean,
        _ => IdentitiesAction::None,
    }
}

pub fn draw_identities_tab(
    f: &mut Frame,
    area: Rect,
    state: &IdentitiesTabState,
    theme: &Theme,
) {
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // ----- Left: keys list -----
    let items: Vec<ListItem> = state
        .keys
        .iter()
        .map(|k| {
            let agent_marker = if k.in_agent { "●" } else { "∘" };
            let file_name = k
                .private
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?");
            let bits = k
                .bits
                .map(|b| format!("{}b", b))
                .unwrap_or_else(|| "--".to_string());
            let label = format!("{}  {:<20} {:<8} {}", agent_marker, file_name, k.key_type, bits);
            let marker_style = if k.in_agent {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.muted)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  ", agent_marker), marker_style),
                Span::styled(
                    label[(agent_marker.chars().count() + 2)..].to_string(),
                    Style::default().fg(theme.fg),
                ),
            ]))
        })
        .collect();

    let mut ls = ListState::default();
    if !state.keys.is_empty() {
        ls.select(Some(state.selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("SSH Keys (~/.ssh)")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg)),
        )
        .highlight_symbol("➜ ")
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, hchunks[0], &mut ls);

    // ----- Right: details -----
    let detail_text: String = if let Some(k) = state.selected_key() {
        format!(
            "File:        {}\n\
             Type:        {}{}\n\
             Comment:     {}\n\
             Fingerprint: {}\n\
             In agent:    {}\n\
             Public key:  {}",
            k.private.display(),
            k.key_type,
            k.bits.map(|b| format!(" {} bits", b)).unwrap_or_default(),
            if k.comment.is_empty() { "(none)" } else { &k.comment },
            k.fingerprint,
            if k.in_agent { "yes ●" } else { "no" },
            k.public.display(),
        )
    } else {
        "No keys found in ~/.ssh.\n\nPress 'g' to generate a new key.".to_string()
    };

    let detail = Paragraph::new(detail_text).block(
        Block::default()
            .title("Details")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().bg(theme.bg).fg(theme.fg)),
    );
    f.render_widget(detail, hchunks[1]);
}
