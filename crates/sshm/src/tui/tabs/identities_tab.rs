//! Identities tab — manage local SSH keys and the running ssh-agent.
//!
//! v1 scope:
//! - list private keys found under `~/.ssh` (with fingerprint / type / comment
//!   / "is in agent" flag)
//! - fuzzy-filter that list with `/`
//! - generate new keys via `ssh-keygen`
//! - push the selected public key to a managed host via `ssh-copy-id`
//! - add / remove the selected key to/from ssh-agent
//! - clean stale `known_hosts` entries via `ssh-keygen -R`
//!
//! NOTE: password-manager integration (1Password / Bitwarden / pass) for
//! passphrases is deliberately out of scope for this version — each provider
//! has its own auth flow and deserves its own feature ticket.

use crossterm::event::KeyCode;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::ssh::keys::{scan_ssh_dir, KeyEntry};
use crate::tui::theme::Theme;

/// The text a key is fuzzy-matched against: file name + type + comment.
fn key_haystack(k: &KeyEntry) -> String {
    let file = k.private.file_name().and_then(|n| n.to_str()).unwrap_or("");
    format!("{} {} {}", file, k.key_type, k.comment)
}

pub struct IdentitiesTabState {
    pub keys: Vec<KeyEntry>,
    /// Indices into `keys` currently shown, after the fuzzy filter.
    pub visible: Vec<usize>,
    /// Cursor position — indexes `visible`, not `keys`.
    pub selected: usize,
    /// Fuzzy filter query. Empty = every key is visible.
    pub filter: String,
    /// True while the user is typing into `filter` (entered with `/`).
    pub input_mode: bool,
}

impl Default for IdentitiesTabState {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentitiesTabState {
    pub fn new() -> Self {
        let keys = scan_ssh_dir();
        let visible = (0..keys.len()).collect();
        IdentitiesTabState {
            keys,
            visible,
            selected: 0,
            filter: String::new(),
            input_mode: false,
        }
    }

    /// Rescan `~/.ssh`, keeping the current filter applied.
    pub fn refresh(&mut self) {
        self.keys = scan_ssh_dir();
        self.rebuild_visible();
    }

    /// Recompute `visible` from `keys` + `filter`, and clamp the cursor.
    fn rebuild_visible(&mut self) {
        if self.filter.is_empty() {
            self.visible = (0..self.keys.len()).collect();
        } else {
            let matcher = SkimMatcherV2::default().smart_case();
            self.visible = self
                .keys
                .iter()
                .enumerate()
                .filter(|(_, k)| matcher.fuzzy_match(&key_haystack(k), &self.filter).is_some())
                .map(|(i, _)| i)
                .collect();
        }
        if self.selected >= self.visible.len() {
            self.selected = self.visible.len().saturating_sub(1);
        }
    }

    pub fn selected_key(&self) -> Option<&KeyEntry> {
        self.visible.get(self.selected).and_then(|&i| self.keys.get(i))
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.visible.len() {
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
    // While typing a filter, keystrokes edit the query; arrows still navigate.
    if state.input_mode {
        match key {
            KeyCode::Esc => {
                state.input_mode = false;
                state.filter.clear();
                state.selected = 0;
                state.rebuild_visible();
            }
            KeyCode::Enter => state.input_mode = false,
            KeyCode::Backspace => {
                state.filter.pop();
                state.selected = 0;
                state.rebuild_visible();
            }
            KeyCode::Up => state.move_up(),
            KeyCode::Down => state.move_down(),
            KeyCode::Char(c) => {
                state.filter.push(c);
                state.selected = 0;
                state.rebuild_visible();
            }
            _ => {}
        }
        return IdentitiesAction::None;
    }

    match key {
        KeyCode::Char('/') => {
            state.input_mode = true;
            state.filter.clear();
            state.selected = 0;
            state.rebuild_visible();
            IdentitiesAction::None
        }
        KeyCode::Esc if !state.filter.is_empty() => {
            state.filter.clear();
            state.selected = 0;
            state.rebuild_visible();
            IdentitiesAction::None
        }
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

    // ----- Left: keys list (filtered) -----
    let items: Vec<ListItem> = state
        .visible
        .iter()
        .map(|&i| {
            let k = &state.keys[i];
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
            let marker_style = if k.in_agent {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.muted)
            };
            let mut spans = vec![
                Span::styled(format!("{}  ", agent_marker), marker_style),
            ];
            if k.is_hardware {
                spans.push(Span::styled(
                    "[HW] ".to_string(),
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                ));
            }
            spans.push(Span::styled(
                format!("{:<20} {:<8} {}", file_name, k.key_type, bits),
                Style::default().fg(theme.fg),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut ls = ListState::default();
    if !state.visible.is_empty() {
        ls.select(Some(state.selected));
    }

    let title = if state.input_mode {
        format!("SSH Keys — filter: {}▏", state.filter)
    } else if !state.filter.is_empty() {
        format!("SSH Keys — filter: {} ({} match)", state.filter, state.visible.len())
    } else {
        "SSH Keys (~/.ssh)".to_string()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
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
             Type:        {}{}{}\n\
             Comment:     {}\n\
             Fingerprint: {}\n\
             In agent:    {}\n\
             Public key:  {}",
            k.private.display(),
            k.key_type,
            k.bits.map(|b| format!(" {} bits", b)).unwrap_or_default(),
            if k.is_hardware { "  [HW-backed]" } else { "" },
            if k.comment.is_empty() { "(none)" } else { &k.comment },
            k.fingerprint,
            if k.in_agent { "yes ●" } else { "no" },
            k.public.display(),
        )
    } else if state.keys.is_empty() {
        "No keys found in ~/.ssh.\n\nPress 'g' to generate a new key.".to_string()
    } else {
        format!("No key matches \"{}\".\n\nEsc to clear the filter.", state.filter)
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
