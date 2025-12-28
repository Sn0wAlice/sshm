use crate::filter::apply_filter;
use crate::models::{tags_to_string, Database, Host};
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
        ScrollbarState, Clear,
    },
    Terminal,
};
use ratatui::text::Span;
use std::{io::stdout, time::Duration};
use crate::config::io::save_db;
use crate::tui::functions::build_rows;
use crate::tui::theme;

// Import all custom TUI functions for custom keypress
use crate::tui::char::{q};

// Row type for folders/hosts in the TUI list

pub enum Row<'a> {
    Folder(String),
    Host(&'a Host),
}

// --- Delete confirmation modal state ---
enum DeleteMode {
    None,
    Host { name: String },
    EmptyFolder { name: String },
    FolderWithHosts { name: String, host_count: usize },
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

    // Folder navigation: None = All/root
    let mut current_folder: Option<String> = None;
    let mut last_rows_len: usize = 0; // updated on each draw

    // Delete confirmation modal state
    let mut delete_mode = DeleteMode::None;
    let mut delete_button_index: usize = 0;

    enable_raw_mode().ok();
    execute!(stdout(), EnterAlternateScreen).ok();
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        // --- Draw ---
        terminal
            .draw(|f| {
                let size = f.size();
                let theme = theme::load();
                let vchunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(85),
                        Constraint::Percentage(15),
                    ])
                    .split(size);
                let hchunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(vchunks[0]);

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
                    format!("{}|", filter) // visual caret
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
                let mut rows = build_rows(db, &items, &filtered, &filter, &current_folder);

                // Clamp selection to available rows
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
                let list_items: Vec<ListItem> = rows
                    .iter()
                    .map(|r| match r {
                        Row::Folder(name) => {
                            let label = if name == "All" {
                                "➤ All".to_string()
                            } else {
                                format!("➤ {}", name)
                            };
                            ListItem::new(Line::from(vec![Span::raw(label)]))
                        }
                        Row::Host(h) => ListItem::new(Line::from(vec![
                            Span::styled(
                                (*h).name.clone(),
                                Style::default().add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(format!("  {}", h.host)),
                        ])),
                    })
                    .collect();

                // Dynamic list title based on current folder
                let list_title = if let Some(folder) = &current_folder {
                    format!("Folder: {} (↑/↓ / filter)", folder)
                } else {
                    "Hosts (↑/↓ / filter)".to_string()
                };
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

                // Scrollbar uses total rows
                let mut sb_state = ScrollbarState::new(last_rows_len.max(1))
                    .position(selected.saturating_sub(viewport_h / 2));
                let sb = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
                f.render_stateful_widget(sb, list_area, &mut sb_state);

                // ----- Details (Host or Folder) -----
                if let Some(sel) = (last_rows_len > 0).then(|| selected) {
                    if let Some(row) = rows.get(sel) {
                        match row {
                            Row::Host(h) => {
                                let detail = format!(
                                    "Name: {}\nUser: {}\nHost: {}\nPort: {}\nTags: {}\nIdentityFile: {}\nProxyJump: {}\nFolder: {}\n\nPress 'f' for SFTP services",
                                    h.name,
                                    h.username,
                                    h.host,
                                    h.port,
                                    tags_to_string(&h.tags),
                                    h.identity_file.clone().unwrap_or_default(),
                                    h.proxy_jump.clone().unwrap_or_default(),
                                    h.folder.clone().unwrap_or_else(|| "-".to_string())
                                );
                                let p = Paragraph::new(detail)
                                    .block(
                                        Block::default()
                                            .title("Details")
                                            .borders(Borders::ALL)
                                            .border_style(Style::default().fg(theme.accent))
                                            .style(Style::default().bg(theme.bg).fg(theme.fg))
                                    );
                                f.render_widget(p, hchunks[1]);
                            }
                            Row::Folder(folder) => {
                                let count = db.hosts.values()
                                    .filter(|h| h.folder.as_deref() == Some(folder.as_str()))
                                    .count();

                                let detail = if folder == "All" {
                                    format!(
                                        "Folder: All\nHosts: {}\n\nSelect a folder item or press Enter to open.",
                                        db.hosts.len()
                                    )
                                } else {
                                    format!(
                                        "Folder: {}\nHosts inside: {}\n\nPress Enter to view its hosts.",
                                        folder,
                                        count
                                    )
                                };

                                let p = Paragraph::new(detail)
                                    .block(
                                        Block::default()
                                            .title("Folder Details")
                                            .borders(Borders::ALL)
                                            .border_style(Style::default().fg(theme.accent))
                                            .style(Style::default().bg(theme.bg).fg(theme.muted))
                                    );
                                f.render_widget(p, hchunks[1]);
                            }
                        }
                    }
                }

                // ----- Help -----
                let rows_help = build_rows(db, &items, &filtered, &filter, &current_folder);
                let help_text = if let Some(sel) = list_state.selected() {
                    match rows_help.get(sel) {
                        Some(Row::Host(_)) => {
                            "Shortcuts:  ↑/↓ move • Enter open/connect • a add • e edit • r rename • i add identity • d delete • q quit\n\
                             Notes: '/' to start filter, Enter to finish; folders shown when filter is empty."
                        }
                        Some(Row::Folder(_)) => {
                            "Shortcuts:  ↑/↓ move • Enter open folder • a add • r rename • q quit\n\
                             Notes: '/' to start filter, Enter to finish; folders shown when filter is empty."
                        }
                        None => {
                            "Shortcuts:  ↑/↓ move • q quit"
                        }
                    }
                } else {
                    "Shortcuts:  ↑/↓ move • q quit"
                };

                let help = Paragraph::new(help_text)
                    .block(
                        Block::default()
                            .title("Help")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.accent))
                            .style(Style::default().bg(theme.bg).fg(theme.muted))
                    );
                f.render_widget(help, vchunks[1]);

                // ----- Delete confirmation modal -----
                match &delete_mode {
                    DeleteMode::None => {}
                    DeleteMode::Host { name } => {
                        let area = centered_rect(60, 30, size);
                        let block = Block::default()
                            .title(Span::styled(
                                "Confirm delete",
                                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.accent))
                            .style(Style::default().bg(theme.bg).fg(theme.fg));
                        let inner = block.inner(area);
                        f.render_widget(Clear, area);
                        f.render_widget(block, area);

                        let lines = vec![
                            Line::from(format!("Delete host \"{}\" ?", name)),
                            Line::from(""),
                            Line::from("This action cannot be undone."),
                        ];
                        let msg = Paragraph::new(lines)
                            .alignment(Alignment::Center);
                        f.render_widget(msg, inner);

                        let buttons_area = Rect {
                            x: inner.x,
                            y: inner.y + inner.height.saturating_sub(3),
                            width: inner.width,
                            height: 3,
                        };

                        let delete_selected = delete_button_index == 0;
                        let cancel_selected = delete_button_index == 1;

                        let delete_span = if delete_selected {
                            Span::styled(
                                "[ Delete ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::styled("[ Delete ]", Style::default().fg(theme.accent))
                        };

                        let cancel_span = if cancel_selected {
                            Span::styled(
                                "[ Cancel ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("[ Cancel ]")
                        };

                        let buttons = Paragraph::new(Line::from(vec![
                            delete_span,
                            Span::raw("   "),
                            cancel_span,
                        ])).alignment(Alignment::Center);
                        f.render_widget(buttons, buttons_area);
                    }
                    DeleteMode::EmptyFolder { name } => {
                        let area = centered_rect(60, 30, size);
                        let block = Block::default()
                            .title(Span::styled(
                                "Confirm delete folder",
                                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.accent))
                            .style(Style::default().bg(theme.bg).fg(theme.fg));
                        let inner = block.inner(area);
                        f.render_widget(Clear, area);
                        f.render_widget(block, area);

                        let lines = vec![
                            Line::from(format!("Delete empty folder \"{}\" ?", name)),
                            Line::from(""),
                            Line::from("This will remove the folder only."),
                        ];
                        let msg = Paragraph::new(lines)
                            .alignment(Alignment::Center);
                        f.render_widget(msg, inner);

                        let buttons_area = Rect {
                            x: inner.x,
                            y: inner.y + inner.height.saturating_sub(3),
                            width: inner.width,
                            height: 3,
                        };

                        let delete_selected = delete_button_index == 0;
                        let cancel_selected = delete_button_index == 1;

                        let delete_span = if delete_selected {
                            Span::styled(
                                "[ Delete ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::styled("[ Delete ]", Style::default().fg(theme.accent))
                        };

                        let cancel_span = if cancel_selected {
                            Span::styled(
                                "[ Cancel ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("[ Cancel ]")
                        };

                        let buttons = Paragraph::new(Line::from(vec![
                            delete_span,
                            Span::raw("   "),
                            cancel_span,
                        ])).alignment(Alignment::Center);
                        f.render_widget(buttons, buttons_area);
                    }
                    DeleteMode::FolderWithHosts { name, host_count } => {
                        let area = centered_rect(70, 35, size);
                        let block = Block::default()
                            .title(Span::styled(
                                "Confirm delete folder & hosts",
                                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.accent))
                            .style(Style::default().bg(theme.bg).fg(theme.fg));
                        let inner = block.inner(area);
                        f.render_widget(Clear, area);
                        f.render_widget(block, area);

                        let lines = vec![
                            Line::from(format!("Folder \"{}\" contains {} hosts.", name, host_count)),
                            Line::from(""),
                            Line::from("What do you want to do?"),
                        ];
                        let msg = Paragraph::new(lines)
                            .alignment(Alignment::Center);
                        f.render_widget(msg, inner);

                        let buttons_area = Rect {
                            x: inner.x,
                            y: inner.y + inner.height.saturating_sub(3),
                            width: inner.width,
                            height: 3,
                        };

                        let delete_all_sel = delete_button_index == 0;
                        let keep_hosts_sel = delete_button_index == 1;
                        let cancel_sel = delete_button_index == 2;

                        let delete_all_span = if delete_all_sel {
                            Span::styled(
                                "[ Delete all ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::styled("[ Delete all ]", Style::default().fg(theme.accent))
                        };

                        let keep_hosts_span = if keep_hosts_sel {
                            Span::styled(
                                "[ Keep hosts ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("[ Keep hosts ]")
                        };

                        let cancel_span = if cancel_sel {
                            Span::styled(
                                "[ Cancel ]",
                                Style::default()
                                    .bg(theme.accent)
                                    .fg(theme.bg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("[ Cancel ]")
                        };

                        let buttons = Paragraph::new(Line::from(vec![
                            delete_all_span,
                            Span::raw("   "),
                            keep_hosts_span,
                            Span::raw("   "),
                            cancel_span,
                        ])).alignment(Alignment::Center);
                        f.render_widget(buttons, buttons_area);
                    }
                }
            })
            .ok();

        // --- Events ---
        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    // If a delete modal is open, handle only its keys
                    if !matches!(delete_mode, DeleteMode::None) {
                        match k.code {
                            KeyCode::Left | KeyCode::Up => {
                                if delete_button_index > 0 {
                                    delete_button_index -= 1;
                                }
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
                                            db.hosts.remove(name);
                                            save_db(db);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                        }
                                        delete_mode = DeleteMode::None;
                                        delete_button_index = 0;
                                    }
                                    DeleteMode::EmptyFolder { name } => {
                                        if delete_button_index == 0 {
                                            db.folders.retain(|f| f != name);
                                            // Safety: also detach any hosts that might still point to it
                                            for h in db.hosts.values_mut() {
                                                if h.folder.as_deref() == Some(name.as_str()) {
                                                    h.folder = None;
                                                }
                                            }
                                            save_db(db);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                        }
                                        delete_mode = DeleteMode::None;
                                        delete_button_index = 0;
                                    }
                                    DeleteMode::FolderWithHosts { name, .. } => {
                                        match delete_button_index {
                                            0 => {
                                                // Delete folder and all its hosts
                                                db.hosts.retain(|_, h| h.folder.as_deref() != Some(name.as_str()));
                                                db.folders.retain(|f| f != name);
                                            }
                                            1 => {
                                                // Keep hosts: move them to root
                                                for h in db.hosts.values_mut() {
                                                    if h.folder.as_deref() == Some(name.as_str()) {
                                                        h.folder = None;
                                                    }
                                                }
                                                db.folders.retain(|f| f != name);
                                            }
                                            _ => { /* Cancel */ }
                                        }
                                        save_db(db);
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
                                input_mode = true; // explicit filter mode
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
                                    // Finish filtering
                                    input_mode = false;
                                } else {
                                    // Rebuild rows to know what is selected
                                    let mut rows_hosts: Vec<Option<&Host>> = Vec::new();
                                    if filter.is_empty() {
                                        // Folder union
                                        let mut folders: Vec<String> = db.folders.clone();
                                        for h in db.hosts.values() {
                                            if let Some(ref folder) = h.folder {
                                                if !folders.iter().any(|f| f == folder) {
                                                    folders.push(folder.clone());
                                                }
                                            }
                                        }
                                        folders.sort();
                                        folders.dedup();
                                        match &current_folder {
                                            None => {
                                                // At root: rows = folders (None), then hosts without folder (Some)
                                                for _ in &folders {
                                                    rows_hosts.push(None);
                                                }
                                                for h in
                                                    items.iter().copied().filter(|h| h.folder.is_none())
                                                {
                                                    rows_hosts.push(Some(h));
                                                }
                                            }
                                            Some(fold) => {
                                                // In folder: first row is "..", then hosts in folder
                                                rows_hosts.push(None); // ".."
                                                for h in items.iter().copied().filter(|h| {
                                                    h.folder.as_deref() == Some(fold.as_str())
                                                }) {
                                                    rows_hosts.push(Some(h));
                                                }
                                            }
                                        }
                                        // Act based on selection
                                        if let Some(row) = rows_hosts.get(selected).cloned() {
                                            match row {
                                                None => {
                                                    // A non-host row was selected (folder/nav)
                                                    match &current_folder {
                                                        // At root: selecting a folder opens it
                                                        None => {
                                                            if let Some(folder_name) =
                                                                folders.get(selected)
                                                            {
                                                                current_folder =
                                                                    Some(folder_name.clone());
                                                                selected = 0;
                                                                list_state.select(Some(0));
                                                            }
                                                        }
                                                        // Inside a folder: 0 = breadcrumb (noop), 1 = ".." (go parent)
                                                        Some(_) => {
                                                            if selected == 0 {
                                                                current_folder = None; // go parent
                                                                selected = 0;
                                                                list_state.select(Some(0));
                                                            }
                                                            // selected == 0 -> breadcrumb: do nothing
                                                        }
                                                    }
                                                }
                                                Some(h) => {
                                                    // Connect to host
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
                                    } else {
                                        for h in &filtered {
                                            rows_hosts.push(Some(h));
                                        }
                                        if let Some(Some(h)) = rows_hosts.get(selected) {
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

                            KeyCode::Char(c) => {
                                if input_mode {
                                    // Append to filter; disable hotkeys
                                    filter.push(c);
                                    filtered = apply_filter(&filter, &items);
                                    selected = 0;
                                    list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                } else {
                                    match c {
                                        'q' | 'Q' => {
                                            q::press();
                                        }
                                        'f' => {
                                            // Launch SFTP UI for selected host
                                            let rows = build_rows(db, &items, &filtered, &filter, &current_folder);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let username = h.username.clone();
                                                let host_addr = h.host.clone();
                                                let port = h.port;
                                                let identity = h.identity_file.clone();

                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);

                                                let _ = crate::tui::app_sftp::run_sftp_ui(
                                                    &username,
                                                    &host_addr,
                                                    port,
                                                    identity.as_deref(),
                                                );

                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);

                                                // Refresh state after returning from SFTP
                                                run_tui(&mut db.clone());
                                            }
                                        }
                                        'e' => {
                                            // Edit currently selected host (if a host is selected) using TUI form
                                            let rows = build_rows(db, &items, &filtered, &filter, &current_folder);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let name = h.name.clone();
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                run_host_edit_form(db, &name);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                // refresh lists
                                                items = db.hosts.values().collect();
                                                items.sort_by(|a, b| a.name.cmp(&b.name));
                                                filtered = apply_filter(&filter, &items);
                                                list_state.select(if filtered.is_empty() {
                                                    None
                                                } else {
                                                    Some(0)
                                                });

                                                // reload ui
                                                run_tui(&mut db.clone());
                                            }
                                        }
                                        'r' => {
                                            let rows = build_rows(db, &items, &filtered, &filter, &current_folder);
                                            if let Some(row) = rows.get(selected) {
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);

                                                match row {
                                                    Row::Host(h) => {
                                                        let name = h.name.clone();
                                                        crate::commands::crud::rename_host(&mut db.hosts, &name);
                                                    }
                                                    Row::Folder(_) => {
                                                        crate::commands::crud::rename_folder(db);
                                                    }
                                                }

                                                save_db(db);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);

                                                items = db.hosts.values().collect();
                                                items.sort_by(|a, b| a.name.cmp(&b.name));
                                                filtered = apply_filter(&filter, &items);
                                                list_state.select(if filtered.is_empty() {
                                                    None
                                                } else {
                                                    Some(0)
                                                });

                                                run_tui(&mut db.clone());
                                            }
                                        }
                                        'd' => {
                                            // Open delete modal based on current selection (host or folder)
                                            let rows = build_rows(db, &items, &filtered, &filter, &current_folder);
                                            if let Some(row) = rows.get(selected) {
                                                match row {
                                                    Row::Host(h) => {
                                                        delete_mode = DeleteMode::Host { name: h.name.clone() };
                                                        delete_button_index = 0;
                                                    }
                                                    Row::Folder(folder_name) => {
                                                        if folder_name == "All" {
                                                            // Don't allow deleting "All"
                                                        } else {
                                                            let count = db.hosts.values()
                                                                .filter(|h| h.folder.as_deref() == Some(folder_name.as_str()))
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
                                        }
                                        'i' => {
                                            // Add identity to selected host, if any
                                            let rows = build_rows(db, &items, &filtered, &filter, &current_folder);
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

                                                // reload ui
                                                run_tui(&mut db.clone());
                                            }
                                        }
                                        'a' => {
                                            // Create Host or Folder; host goes into current folder
                                            let _ = disable_raw_mode();
                                            let _ = execute!(stdout(), LeaveAlternateScreen);
                                            run_host_create_form(db, current_folder.clone());
                                            let _ = enable_raw_mode();
                                            let _ = execute!(stdout(), EnterAlternateScreen);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            list_state.select(if filtered.is_empty() {
                                                None
                                            } else {
                                                Some(0)
                                            });
                                            // reload ui
                                            run_tui(&mut db.clone());
                                        }
                                        _ => {
                                            // Start implicit filter mode with this first char
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
                }
            }
        }
    }
}

// ===== Host form TUI (Zenburn-style) =====

struct HostFormState {
    name: String,
    host: String,
    port: String,
    username: String,
    identity_file: String,
    proxy_jump: String,
    tags: String,
    folder: String,
    selected_field: usize,
    is_edit: bool,
    original_name: Option<String>,
}

impl HostFormState {
    fn new_create(current_folder: Option<String>) -> Self {
        HostFormState {
            name: String::new(),
            host: String::new(),
            port: "22".to_string(),
            username: "root".to_string(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            tags: String::new(),
            folder: current_folder.unwrap_or_default(),
            selected_field: 0,
            is_edit: false,
            original_name: None,
        }
    }

    fn new_edit(db: &Database, name: &str) -> Self {
        if let Some(h) = db.hosts.get(name) {
            HostFormState {
                name: h.name.clone(),
                host: h.host.clone(),
                port: h.port.to_string(),
                username: h.username.clone(),
                identity_file: h.identity_file.clone().unwrap_or_default(),
                proxy_jump: h.proxy_jump.clone().unwrap_or_default(),
                tags: tags_to_string(&h.tags),
                folder: h.folder.clone().unwrap_or_default(),
                selected_field: 0,
                is_edit: true,
                original_name: Some(h.name.clone()),
            }
        } else {
            HostFormState::new_create(None)
        }
    }

    fn fields_count() -> usize {
        8 // name, host, port, username, identity_file, proxy_jump, tags, folder
    }

    fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % (Self::fields_count() + 1); // +1 for Save
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
            0 => Some(&mut self.name),
            1 => Some(&mut self.host),
            2 => Some(&mut self.port),
            3 => Some(&mut self.username),
            4 => Some(&mut self.identity_file),
            5 => Some(&mut self.proxy_jump),
            6 => Some(&mut self.tags),
            7 => Some(&mut self.folder),
            _ => None,
        }
    }

    fn push_char(&mut self, c: char) {
        if let Some(field) = self.active_value_mut() {
            field.push(c);
        }
    }

    fn pop_char(&mut self) {
        if let Some(field) = self.active_value_mut() {
            field.pop();
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1]);

    horizontal[1]
}

fn draw_host_form(
    f: &mut Frame,
    db: &Database,
    state: &HostFormState,
) {
    let size = f.size();
    let area = centered_rect(70, 80, size);
    let theme = theme::get_global_theme();
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
            Style::default().fg(Color::Rgb(150, 150, 150)),
        ),
    ]));

    f.render_widget(actions, chunks[8]);

    let help = Paragraph::new(Line::from(vec![Span::raw("Tab/Shift+Tab or ↑/↓ to move • Type to edit • Enter to save when [ Save ] is selected • Esc to cancel")]))
        .style(Style::default().fg(Color::Rgb(150, 150, 150)));
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
            if let Some(existing) = db.hosts.remove(orig_name) {
                let mut new_host = Host {
                    name: name.to_string(),
                    host: host.to_string(),
                    port,
                    username,
                    identity_file,
                    proxy_jump,
                    folder,
                    tags,
                };
                // Preserve other fields if needed (here we overwrite fully)
                db.hosts.insert(new_host.name.clone(), new_host);
            }
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

    save_db(db);
    Ok(())
}

fn run_host_create_form(db: &mut Database, current_folder: Option<String>) {
    let mut state = HostFormState::new_create(current_folder);

    let mut stdout = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        let _ = terminal.draw(|f| {
            draw_host_form(f, db, &state);
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

fn run_host_edit_form(db: &mut Database, name: &str) {
    let mut state = HostFormState::new_edit(db, name);

    let mut stdout = stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout, EnterAlternateScreen);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        let _ = terminal.draw(|f| {
            draw_host_form(f, db, &state);
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
