use crate::filter::apply_filter;
use crate::models::{Database, Host};
use crate::util::clear_console;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
    Terminal,
};
use std::collections::HashMap;
use std::io::stdout;
use std::net::TcpStream;
use std::time::Duration;
use crate::tui::ssh::toast::Toast;
use crate::config::io::save_db;
use crate::config::settings::{load_settings, save_settings, AppConfig};
use crate::tui::functions::build_rows;
use crate::tui::theme;
use crate::tui::tabs::tab_bar::draw_tab_bar;
use crate::tui::tabs::settings_tab::{self, SettingsFormState, SettingsAction};
use crate::tui::tabs::theme_tab::{self, ThemeTabState, ThemeAction};
use crate::tui::tabs::help_tab::{self, HelpTabState};

use crate::tui::ssh::folder_form_state::FolderFormState;
use crate::tui::ssh::host_form_state::HostFormState;

use crate::tui::char::q;


pub enum Row<'a> {
    Folder { name: String, collapsed: bool },
    Host(&'a Host),
}

// --- Delete confirmation modal state ---
pub enum DeleteMode {
    None,
    Host { name: String },
    EmptyFolder { name: String },
    FolderWithHosts { name: String, host_count: usize },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HostStatus {
    Reachable,
    Unreachable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Hosts,
    Settings,
    Theme,
    Help,
}

impl ActiveTab {
    pub fn next(self) -> Self {
        match self {
            Self::Hosts => Self::Settings,
            Self::Settings => Self::Theme,
            Self::Theme => Self::Help,
            Self::Help => Self::Hosts,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Hosts => Self::Help,
            Self::Settings => Self::Hosts,
            Self::Theme => Self::Settings,
            Self::Help => Self::Theme,
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Hosts => 0,
            Self::Settings => 1,
            Self::Theme => 2,
            Self::Help => 3,
        }
    }
}

fn save_and_export(db: &Database, app_config: &AppConfig) {
    save_db(db);
    if !app_config.export_path.is_empty() {
        let _ = crate::config::export::export_ssh_config(db, &app_config.export_path);
    }
}

pub fn run_tui(db: &mut Database) {
    // Source items
    let mut items: Vec<&Host> = db.hosts.values().collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));

    // Filter state
    let mut filter = String::new();
    let mut filtered: Vec<&Host> = items.clone();
    let mut input_mode: bool = false; // true while typing a filter; disable hotkeys

    // List/selection state
    let mut selected: usize = 0;
    let mut list_state = ListState::default();
    list_state.select(Some(selected));
    let mut viewport_h: usize = 10;

    // Collapsible folders: true = collapsed, false = expanded
    let mut collapsed: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    {
        let mut all_folders: Vec<String> = db.folders.clone();
        for h in db.hosts.values() {
            if let Some(ref f) = h.folder {
                if !all_folders.contains(f) {
                    all_folders.push(f.clone());
                }
                // Ensure parent folder exists for sub-folders
                if let Some(parent) = f.split('/').next() {
                    if f.contains('/') && !all_folders.contains(&parent.to_string()) {
                        all_folders.push(parent.to_string());
                    }
                }
            }
        }
        for f in all_folders {
            collapsed.insert(f, true);
        }
    }
    let mut last_rows_len: usize = 0;

    // Delete confirmation modal state
    let mut delete_mode = DeleteMode::None;
    let mut delete_button_index: usize = 0;

    // Tab state
    let mut active_tab = ActiveTab::Hosts;
    let mut app_config = load_settings();
    let mut settings_state = SettingsFormState::from_config(&app_config);
    let mut theme_state = ThemeTabState::new(&theme::load());
    let mut help_state = HelpTabState::new();

    // Toast notification
    let mut toast: Option<Toast> = None;

    // Host reachability status (name → status)
    let mut host_status: HashMap<String, HostStatus> = HashMap::new();

    enable_raw_mode().ok();
    execute!(stdout(), EnterAlternateScreen).ok();
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        // Expire toast
        if toast.as_ref().is_some_and(|t| t.is_expired()) {
            toast = None;
        }
        // --- Draw ---
        terminal
            .draw(|f| {
                let size = f.area();
                let theme = theme::load();

                // Top-level layout: tab bar + content + help
                let vchunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Min(0),
                        Constraint::Length(1),
                    ])
                    .split(size);

                // Tab bar
                draw_tab_bar(f, vchunks[0], active_tab.index(), &theme);

                match active_tab {
                    ActiveTab::Hosts => {
                        let hchunks = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                            .split(vchunks[1]);

                        // Left pane: filter bar + list
                        let left_chunks = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([Constraint::Length(3), Constraint::Min(0)])
                            .split(hchunks[0]);

                        let list_area = left_chunks[1];
                        let vh = list_area.height.saturating_sub(2) as usize;
                        viewport_h = vh.max(1);

                        // ----- Filter bar -----
                        let filter_label = if input_mode {
                            format!("{}|", filter)
                        } else if filter.is_empty() {
                            "(press '/' to start)".to_string()
                        } else {
                            filter.clone()
                        };
                        let filter_para = Paragraph::new(Line::from(vec![
                            Span::styled("Filter ", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(filter_label),
                        ]))
                        .block(
                            Block::default()
                                .title("Filter")
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(theme.accent))
                                .style(Style::default().bg(theme.bg).fg(theme.fg))
                        );
                        f.render_widget(filter_para, left_chunks[0]);

                        // ----- Build rows (folders + hosts) -----
                        let rows = build_rows(db, &items, &filtered, &filter, &collapsed);

                        last_rows_len = rows.len();
                        if last_rows_len == 0 {
                            list_state.select(None);
                        } else {
                            if selected >= last_rows_len {
                                selected = last_rows_len - 1;
                            }
                            list_state.select(Some(selected));
                        }

                        // ----- Render list -----
                        let list_items: Vec<ListItem> = crate::tui::ssh::listitems::get_item_list(&rows, &host_status, &theme);

                        let list_title = "Hosts (↑/↓ / filter)".to_string();
                        let list = List::new(list_items)
                            .block(
                                Block::default()
                                    .title(list_title)
                                    .borders(Borders::ALL)
                                    .border_style(Style::default().fg(theme.accent))
                                    .style(Style::default().bg(theme.bg).fg(theme.fg))
                            )
                            .highlight_symbol("➜ ")
                            .highlight_style(
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD)
                            );

                        f.render_stateful_widget(list, list_area, &mut list_state);

                        let mut sb_state = ScrollbarState::new(last_rows_len.max(1))
                            .position(selected.saturating_sub(viewport_h / 2));
                        let sb = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
                        f.render_stateful_widget(sb, list_area, &mut sb_state);

                        // ----- Details (Host or Folder) -----
                        crate::tui::ssh::detailbox::show_detail_box(
                            last_rows_len, selected, &rows, f, &hchunks, &theme, db, &host_status,
                        );

                        // ----- Delete confirmation modal -----
                        crate::tui::ssh::deletebox::show_delete_box(&delete_mode, delete_button_index, f, size, &theme);
                    }
                    ActiveTab::Settings => {
                        settings_tab::draw_settings_tab(f, vchunks[1], &settings_state, &theme);
                    }
                    ActiveTab::Theme => {
                        theme_tab::draw_theme_tab(f, vchunks[1], &theme_state, &theme);
                    }
                    ActiveTab::Help => {
                        help_tab::draw_help_tab(f, vchunks[1], &help_state, &theme);
                    }
                }

                // Contextual help bar (unified for all tabs)
                use crate::tui::ssh::helpbox::HelpContext;
                let help_ctx = match active_tab {
                    ActiveTab::Hosts => {
                        if !matches!(delete_mode, DeleteMode::None) {
                            HelpContext::DeleteModal
                        } else if input_mode {
                            HelpContext::FilterMode
                        } else if last_rows_len == 0 {
                            HelpContext::Empty
                        } else {
                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                            match rows.get(selected) {
                                Some(Row::Folder { .. }) => HelpContext::FolderNav,
                                Some(Row::Host(_)) => HelpContext::HostNav,
                                None => HelpContext::Empty,
                            }
                        }
                    }
                    ActiveTab::Settings => HelpContext::SettingsTab,
                    ActiveTab::Theme => HelpContext::ThemeTab,
                    ActiveTab::Help => HelpContext::HelpTab,
                };
                f.render_widget(
                    crate::tui::ssh::helpbox::get_contextual_help(help_ctx, &theme),
                    vchunks[2],
                );

                // Toast overlay (rendered last, on top of everything)
                if let Some(ref t) = toast {
                    if !t.is_expired() {
                        crate::tui::ssh::toast::render_toast(f, size, t, &theme);
                    }
                }
            })
            .ok();

        // --- Events ---
        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {

                    // --- Global: tab navigation when not editing ---
                    let tab_nav_allowed = match active_tab {
                        ActiveTab::Hosts => !input_mode && matches!(delete_mode, DeleteMode::None),
                        ActiveTab::Settings => !settings_state.is_editing_field(),
                        ActiveTab::Theme => !theme_state.is_editing_custom_field(),
                        ActiveTab::Help => true,
                    };

                    if tab_nav_allowed {
                        match k.code {
                            KeyCode::Right => { active_tab = active_tab.next(); continue; }
                            KeyCode::Left => { active_tab = active_tab.prev(); continue; }
                            KeyCode::Char('q') | KeyCode::Char('Q') => { q::press(); }
                            _ => {}
                        }
                    }

                    // --- Tab-specific event handling ---
                    match active_tab {
                        ActiveTab::Hosts => {
                    // If a delete modal is open, handle only its keys
                    if !matches!(delete_mode, DeleteMode::None) {
                        match k.code {
                            KeyCode::Left | KeyCode::Up => {
                                delete_button_index = delete_button_index.saturating_sub(1);
                            }
                            KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                                let max = match delete_mode {
                                    DeleteMode::Host { .. } | DeleteMode::EmptyFolder { .. } => 1,
                                    DeleteMode::FolderWithHosts { .. } => 2,
                                    DeleteMode::None => 0,
                                };
                                if delete_button_index >= max {
                                    delete_button_index = 0;
                                } else {
                                    delete_button_index += 1;
                                }
                            }
                            KeyCode::Esc => {
                                delete_mode = DeleteMode::None;
                                delete_button_index = 0;
                            }
                            KeyCode::Enter => {
                                match &delete_mode {
                                    DeleteMode::Host { name } => {
                                        if delete_button_index == 0 {
                                            let deleted_name = name.clone();
                                            db.hosts.remove(name);
                                            save_and_export(db, &app_config);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                            toast = Some(Toast::success(format!("Deleted: {}", deleted_name)));
                                        }
                                        delete_mode = DeleteMode::None;
                                        delete_button_index = 0;
                                    }
                                    DeleteMode::EmptyFolder { name } => {
                                        if delete_button_index == 0 {
                                            let deleted_name = name.clone();
                                            let prefix = format!("{}/", name);
                                            // Remove this folder + sub-folders from collapsed
                                            collapsed.retain(|k, _| k != name && !k.starts_with(&prefix));
                                            db.folders.retain(|f| f != name && !f.starts_with(&prefix));
                                            for h in db.hosts.values_mut() {
                                                if let Some(ref f) = h.folder {
                                                    if f == name || f.starts_with(&prefix) {
                                                        h.folder = None;
                                                    }
                                                }
                                            }
                                            save_and_export(db, &app_config);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                            toast = Some(Toast::success(format!("Deleted folder: {}", deleted_name)));
                                        }
                                        delete_mode = DeleteMode::None;
                                        delete_button_index = 0;
                                    }
                                    DeleteMode::FolderWithHosts { name, .. } => {
                                        let deleted_name = name.clone();
                                        let prefix = format!("{}/", name);
                                        match delete_button_index {
                                            0 => {
                                                // Delete folder + sub-folders + all hosts inside
                                                collapsed.retain(|k, _| k != name && !k.starts_with(&prefix));
                                                db.hosts.retain(|_, h| {
                                                    if let Some(ref f) = h.folder {
                                                        f != name && !f.starts_with(&prefix)
                                                    } else {
                                                        true
                                                    }
                                                });
                                                db.folders.retain(|f| f != name && !f.starts_with(&prefix));
                                                toast = Some(Toast::success(format!("Deleted folder & hosts: {}", deleted_name)));
                                            }
                                            1 => {
                                                // Delete folder + sub-folders, move hosts to root
                                                collapsed.retain(|k, _| k != name && !k.starts_with(&prefix));
                                                for h in db.hosts.values_mut() {
                                                    if let Some(ref f) = h.folder.clone() {
                                                        if f == name || f.starts_with(&prefix) {
                                                            h.folder = None;
                                                        }
                                                    }
                                                }
                                                db.folders.retain(|f| f != name && !f.starts_with(&prefix));
                                                toast = Some(Toast::success(format!("Deleted folder: {}", deleted_name)));
                                            }
                                            _ => { /* Cancel */ }
                                        }
                                        save_and_export(db, &app_config);
                                        items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                        delete_mode = DeleteMode::None;
                                        delete_button_index = 0;
                                    }
                                    DeleteMode::None => {}
                                }
                            }
                            _ => {}
                        }
                    } else {
                        match k.code {
                            KeyCode::Up => {
                                selected = selected.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                selected = selected.saturating_add(1);
                            }
                            KeyCode::PageDown => {
                                selected = selected.saturating_add(viewport_h);
                            }
                            KeyCode::PageUp => {
                                selected = selected.saturating_sub(viewport_h);
                            }
                            KeyCode::Home => {
                                selected = 0;
                            }
                            KeyCode::End => {
                                if last_rows_len > 0 {
                                    selected = last_rows_len - 1;
                                }
                            }

                            KeyCode::Esc => {
                                if input_mode {
                                    input_mode = false;
                                    filter.clear();
                                    filtered = apply_filter(&filter, &items);
                                    selected = 0;
                                    list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                }
                            }

                            KeyCode::Char('/') => {
                                input_mode = true;
                                filter.clear();
                                filtered = apply_filter(&filter, &items);
                                selected = 0;
                                list_state.select(if filtered.is_empty() { None } else { Some(0) });
                            }

                            KeyCode::Backspace => {
                                if input_mode {
                                    filter.pop();
                                    filtered = apply_filter(&filter, &items);
                                    selected = 0;
                                    list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                }
                            }

                            KeyCode::Enter => {
                                if input_mode {
                                    input_mode = false;
                                } else {
                                    let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                    if let Some(row) = rows.get(selected) {
                                        match row {
                                            Row::Folder { name, collapsed: is_c } => {
                                                collapsed.insert(name.clone(), !is_c);
                                            }
                                            Row::Host(h) => {
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                crate::ssh::client::launch_ssh(h, None);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                clear_console();
                                                return;
                                            }
                                        }
                                    }
                                }
                            }

                            KeyCode::Char(c) => {
                                if input_mode {
                                    filter.push(c);
                                    filtered = apply_filter(&filter, &items);
                                    selected = 0;
                                    list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                } else {
                                    match c {
                                        'q' | 'Q' => { /* handled globally above */ }
                                        'e' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let state = HostFormState::new_edit(db, &h.name);
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                run_host_form(db, state);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                items = db.hosts.values().collect();
                                                items.sort_by(|a, b| a.name.cmp(&b.name));
                                                filtered = apply_filter(&filter, &items);
                                                selected = 0;
                                                list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                                let _ = terminal.clear();
                                            }
                                        }
                                        'r' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Folder { name: folder_name, .. }) = rows.get(selected) {
                                                let folder_name = folder_name.clone();
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                run_folder_rename_form(db, &folder_name);
                                                // Rebuild collapsed map: keep states for folders that still exist
                                                let old_collapsed = collapsed.clone();
                                                collapsed.clear();
                                                for f in &db.folders {
                                                    let state = old_collapsed.get(f).copied()
                                                        .unwrap_or(true);
                                                    collapsed.insert(f.clone(), state);
                                                }
                                                save_and_export(db, &app_config);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                items = db.hosts.values().collect();
                                                items.sort_by(|a, b| a.name.cmp(&b.name));
                                                filtered = apply_filter(&filter, &items);
                                                selected = 0;
                                                list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                                let _ = terminal.clear();
                                            }
                                        }
                                        'd' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                            if let Some(row) = rows.get(selected) {
                                                match row {
                                                    Row::Host(h) => {
                                                        delete_mode = DeleteMode::Host { name: h.name.clone() };
                                                        delete_button_index = 0;
                                                    }
                                                    Row::Folder { name: folder_name, .. } => {
                                                        let prefix = format!("{}/", folder_name);
                                                        let count = db.hosts.values()
                                                            .filter(|h| {
                                                                if let Some(ref f) = h.folder {
                                                                    f == folder_name || f.starts_with(&prefix)
                                                                } else {
                                                                    false
                                                                }
                                                            })
                                                            .count();
                                                        delete_button_index = 0;
                                                        if count == 0 {
                                                            delete_mode = DeleteMode::EmptyFolder { name: folder_name.clone() };
                                                        } else {
                                                            delete_mode = DeleteMode::FolderWithHosts {
                                                                name: folder_name.clone(),
                                                                host_count: count,
                                                            };
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        'c' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let name = h.name.clone();
                                                let addr = format!("{}:{}", h.host, h.port);
                                                // TCP connect with 3-second timeout
                                                let status = match addr.parse::<std::net::SocketAddr>() {
                                                    Ok(sock) => {
                                                        match TcpStream::connect_timeout(&sock, Duration::from_secs(3)) {
                                                            Ok(_) => HostStatus::Reachable,
                                                            Err(_) => HostStatus::Unreachable,
                                                        }
                                                    }
                                                    Err(_) => {
                                                        // hostname — resolve then try
                                                        use std::net::ToSocketAddrs;
                                                        match addr.to_socket_addrs() {
                                                            Ok(mut addrs) => {
                                                                if let Some(sock) = addrs.next() {
                                                                    match TcpStream::connect_timeout(&sock, Duration::from_secs(3)) {
                                                                        Ok(_) => HostStatus::Reachable,
                                                                        Err(_) => HostStatus::Unreachable,
                                                                    }
                                                                } else {
                                                                    HostStatus::Unreachable
                                                                }
                                                            }
                                                            Err(_) => HostStatus::Unreachable,
                                                        }
                                                    }
                                                };
                                                let msg = match status {
                                                    HostStatus::Reachable => format!("{} is reachable ✓", name),
                                                    HostStatus::Unreachable => format!("{} is unreachable ✗", name),
                                                };
                                                toast = Some(match status {
                                                    HostStatus::Reachable => Toast::success(msg),
                                                    HostStatus::Unreachable => Toast::error(msg),
                                                });
                                                host_status.insert(name, status);
                                            }
                                        }
                                        'p' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                crate::tui::ssh::portforward::run_port_forward(h);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                let _ = terminal.clear();
                                            }
                                        }
                                        'i' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let name = h.name.clone();
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                crate::ssh::add_identity::cmd_add_identity(
                                                    &db.hosts,
                                                    Some(name),
                                                    &[],
                                                );
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                let _ = terminal.clear();
                                            }
                                        }
                                        'a' => {
                                            // Determine folder context from selected row
                                            let folder_ctx = {
                                                let rows = build_rows(db, &items, &filtered, &filter, &collapsed);
                                                match rows.get(selected) {
                                                    Some(Row::Folder { name, .. }) => Some(name.clone()),
                                                    Some(Row::Host(h)) => h.folder.clone(),
                                                    None => None,
                                                }
                                            };
                                            let _ = disable_raw_mode();
                                            let _ = execute!(stdout(), LeaveAlternateScreen);
                                            let state = HostFormState::new_create(folder_ctx, &app_config);
                                            run_host_form(db, state);
                                            let _ = enable_raw_mode();
                                            let _ = execute!(stdout(), EnterAlternateScreen);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                            let _ = terminal.clear();
                                        }
                                        _ => {
                                            input_mode = true;
                                            filter.clear();
                                            filter.push(c);
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() {
                                                None
                                            } else {
                                                Some(0)
                                            });
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                        } // ActiveTab::Hosts

                        ActiveTab::Settings => {
                            match k.code {
                                KeyCode::Esc => {
                                    settings_state = SettingsFormState::from_config(&app_config);
                                }
                                _ => {
                                    match settings_tab::handle_settings_event(k.code, &mut settings_state) {
                                        SettingsAction::Save => {
                                            match settings_state.default_port.trim().parse::<u16>() {
                                                Ok(port) => {
                                                    app_config.default_port = port;
                                                    app_config.default_username = settings_state.default_username.trim().to_string();
                                                    app_config.default_identity_file = settings_state.default_identity_file.trim().to_string();
                                                    app_config.export_path = settings_state.export_path.trim().to_string();
                                                    save_settings(&app_config);
                                                    settings_state.dirty = false;
                                                    // Auto-export if export_path is set
                                                    if !app_config.export_path.is_empty() {
                                                        if let Err(e) = crate::config::export::export_ssh_config(db, &app_config.export_path) {
                                                            toast = Some(Toast::error(format!("Export failed: {e}")));
                                                        } else {
                                                            toast = Some(Toast::success("Settings saved & exported!"));
                                                        }
                                                    } else {
                                                        toast = Some(Toast::success("Settings saved!"));
                                                    }
                                                }
                                                Err(_) => {
                                                    toast = Some(Toast::error("Invalid port number"));
                                                }
                                            }
                                        }
                                        SettingsAction::None => {}
                                    }
                                }
                            }
                        }

                        ActiveTab::Theme => {
                            match k.code {
                                KeyCode::Esc => {
                                    let current = theme::load();
                                    theme_state = ThemeTabState::new(&current);
                                }
                                _ => {
                                    match theme_tab::handle_theme_event(k.code, &mut theme_state) {
                                        ThemeAction::ApplyPreset(idx) => {
                                            let preset = &theme::PRESETS[idx];
                                            theme::save_theme(preset.bg, preset.fg, preset.accent, preset.muted, preset.error, preset.success);
                                            theme_state.custom_bg = preset.bg.to_string();
                                            theme_state.custom_fg = preset.fg.to_string();
                                            theme_state.custom_accent = preset.accent.to_string();
                                            theme_state.custom_muted = preset.muted.to_string();
                                            theme_state.custom_error = preset.error.to_string();
                                            theme_state.custom_success = preset.success.to_string();
                                            theme_state.dirty = false;
                                            toast = Some(Toast::success(format!("Theme: {}", preset.name)));
                                        }
                                        ThemeAction::SaveCustom => {
                                            let valid = [&theme_state.custom_bg, &theme_state.custom_fg,
                                                         &theme_state.custom_accent, &theme_state.custom_muted,
                                                         &theme_state.custom_error, &theme_state.custom_success]
                                                .iter().all(|h| theme::hex_to_color(h).is_some());
                                            if valid {
                                                theme::save_theme(
                                                    &theme_state.custom_bg, &theme_state.custom_fg,
                                                    &theme_state.custom_accent, &theme_state.custom_muted,
                                                    &theme_state.custom_error, &theme_state.custom_success,
                                                );
                                                theme_state.dirty = false;
                                                toast = Some(Toast::success("Custom theme saved!"));
                                            } else {
                                                toast = Some(Toast::error("Invalid hex color(s)"));
                                            }
                                        }
                                        ThemeAction::None => {}
                                    }
                                }
                            }
                        }

                        ActiveTab::Help => {
                            help_tab::handle_help_event(k.code, &mut help_state);
                        }
                    } // match active_tab
                }
            }
        }
    }
}


// ===== Folder rename form TUI =====

fn draw_folder_form(f: &mut Frame, state: &FolderFormState) {
    let size = f.area();
    let area = centered_rect(50, 40, size);
    let theme = theme::load();
    let bg = theme.bg;
    let fg = theme.fg;
    let accent = theme.accent;

    let block = Block::default()
        .title(
            Span::styled(
                "Rename folder",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .style(Style::default().bg(bg).fg(fg));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)].as_ref())
        .split(inner);

    let name_selected = state.selected_field == 0;
    let name_span = if name_selected {
        Span::styled(
            format!("[{}]", state.name),
            Style::default().bg(accent).fg(bg).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw(format!("[{}]", state.name))
    };

    let name_line = Paragraph::new(Line::from(vec![
        Span::styled("Folder: ", Style::default().add_modifier(Modifier::BOLD)),
        name_span,
    ]));
    f.render_widget(name_line, chunks[0]);

    let save_selected = state.selected_field == FolderFormState::fields_count();
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
    f.render_widget(actions, chunks[1]);

    let error_text = if let Some(err) = &state.error {
        err.as_str()
    } else {
        "Tab/Shift+Tab or ↑/↓ to move • Type to edit • Enter to save"
    };

    let error_para = Paragraph::new(error_text).style(Style::default().fg(if state.error.is_some() { theme.error } else { theme.muted }));
    f.render_widget(error_para, chunks[2]);
}

fn apply_folder_form(db: &mut Database, state: &mut FolderFormState) -> Result<(), String> {
    let new_name = state.name.trim();
    if new_name.is_empty() {
        return Err("Folder name cannot be empty".into());
    }

    if new_name == state.original_name {
        return Ok(());
    }

    if db.folders.iter().any(|f| f == new_name) {
        return Err(format!("Folder '{}' already exists", new_name));
    }

    let original = state.original_name.clone();
    let new_str = new_name.to_string();
    let old_prefix = format!("{}/", original);

    for f in db.folders.iter_mut() {
        if f == &original {
            *f = new_str.clone();
        } else if f.starts_with(&old_prefix) {
            // Update sub-folder paths: "OldParent/Child" → "NewParent/Child"
            *f = format!("{}/{}", new_str, &f[old_prefix.len()..]);
        }
    }
    for h in db.hosts.values_mut() {
        if let Some(ref f) = h.folder.clone() {
            if f == &original {
                h.folder = Some(new_str.clone());
            } else if f.starts_with(&old_prefix) {
                h.folder = Some(format!("{}/{}", new_str, &f[old_prefix.len()..]));
            }
        }
    }

    let cfg = load_settings();
    save_and_export(db, &cfg);
    Ok(())
}

fn run_folder_rename_form(db: &mut Database, folder_name: &str) {
    let mut state = FolderFormState::new_rename(folder_name);

    let mut stdout = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        let _ = terminal.draw(|f| draw_folder_form(f, &state));

        if event::poll(Duration::from_millis(120)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Esc => break,
                        KeyCode::Tab | KeyCode::Down => state.next_field(),
                        KeyCode::BackTab | KeyCode::Up => state.prev_field(),
                        KeyCode::Enter => {
                            if state.selected_field == FolderFormState::fields_count() {
                                match apply_folder_form(db, &mut state) {
                                    Ok(_) => break,
                                    Err(e) => state.error = Some(e),
                                }
                            } else {
                                state.next_field();
                            }
                        }
                        KeyCode::Char(c) => {
                            state.push_char(c);
                            state.error = None;
                        }
                        KeyCode::Backspace => {
                            state.pop_char();
                            state.error = None;
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


// ===== Host update form TUI =====

pub use crate::tui::ssh::modal::centered_rect;

fn draw_host_form(
    f: &mut Frame,
    state: &HostFormState,
) {
    let size = f.area();
    let area = centered_rect(70, 80, size);
    let theme = theme::load();
    let bg = theme.bg;
    let fg = theme.fg;
    let accent = theme.accent;

    let block = Block::default()
        .title(
            Span::styled(
                if state.is_edit { "Edit host" } else { "Create host" },
                Style::default()
                    .fg(accent)
                    .add_modifier(Modifier::BOLD),
            ),
        )
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
                Constraint::Length(1), // actions
            ]
            .as_ref(),
        )
        .split(inner);

    let mk_line = |label: &str, value: &str, selected: bool| {
        let value_span = if selected {
            Span::styled(
                format!("[{}]", value),
                Style::default()
                    .bg(accent)
                    .fg(bg)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(format!("[{}]", value))
        };
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{label}: "),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            value_span,
        ]))
    };

    f.render_widget(
        mk_line("Name", &state.name, state.selected_field == 0),
        chunks[0],
    );
    f.render_widget(
        mk_line("Host/IP", &state.host, state.selected_field == 1),
        chunks[1],
    );
    f.render_widget(
        mk_line("Port", &state.port, state.selected_field == 2),
        chunks[2],
    );
    f.render_widget(
        mk_line("Username", &state.username, state.selected_field == 3),
        chunks[3],
    );
    f.render_widget(
        mk_line(
            "Identity file",
            &state.identity_file,
            state.selected_field == 4,
        ),
        chunks[4],
    );
    f.render_widget(
        mk_line("ProxyJump", &state.proxy_jump, state.selected_field == 5),
        chunks[5],
    );
    f.render_widget(
        mk_line("Tags", &state.tags, state.selected_field == 6),
        chunks[6],
    );
    f.render_widget(
        mk_line("Folder", &state.folder, state.selected_field == 7),
        chunks[7],
    );

    let save_selected = state.selected_field == HostFormState::fields_count();
    let save_style = if save_selected {
        Style::default()
            .bg(accent)
            .fg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(accent)
    };

    let actions = Paragraph::new(Line::from(vec![
        Span::styled("[ Save ]", save_style),
        Span::raw("  "),
        Span::styled(
            "[ Esc = Cancel ]",
            Style::default().fg(theme.muted),
        ),
    ]));

    f.render_widget(actions, chunks[8]);

    let help = Paragraph::new(Line::from(vec![Span::raw("Tab/Shift+Tab or ↑/↓ to move • Type to edit • Enter to save when [ Save ] is selected • Esc to cancel")]))
        .style(Style::default().fg(theme.muted));
    let help_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(2),
        width: inner.width,
        height: 2,
    };
    f.render_widget(help, help_area);
}

fn apply_host_form(db: &mut Database, state: &HostFormState) -> Result<(), String> {
    let name = state.name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty".into());
    }

    let host = state.host.trim();
    if host.is_empty() {
        return Err("Host cannot be empty".into());
    }

    let port: u16 = state
        .port
        .trim()
        .parse()
        .map_err(|_| "Port must be a number".to_string())?;

    // Validate alias uniqueness for create or rename
    if let Some(orig) = &state.original_name {
        if name != orig && db.hosts.contains_key(name) {
            return Err(format!("Host alias '{}' already exists", name));
        }
    } else if db.hosts.contains_key(name) {
        return Err(format!("Host alias '{}' already exists", name));
    }

    let username = state.username.trim();
    let username = if username.is_empty() {
        "root"
    } else {
        username
    }
    .to_string();

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
            let v = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        }
    };

    if state.is_edit {
        if let Some(orig_name) = &state.original_name {
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
        };
        db.hosts.insert(name.to_string(), host_obj);
    }

    let cfg = load_settings();
    save_and_export(db, &cfg);
    Ok(())
}

fn run_host_form(db: &mut Database, mut state: HostFormState) {
    let mut stdout = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        let _ = terminal.draw(|f| {
            draw_host_form(f, &state);
        });

        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Esc => break,
                        KeyCode::Tab | KeyCode::Down => state.next_field(),
                        KeyCode::BackTab | KeyCode::Up => state.prev_field(),
                        KeyCode::Enter => {
                            if state.selected_field == HostFormState::fields_count() {
                                if apply_host_form(db, &state).is_ok() {
                                    break;
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
