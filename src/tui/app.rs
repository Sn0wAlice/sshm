
use crate::filter::apply_filter;
use crate::models::{tags_to_string, Database, Host};
use crate::util::clear_console;
use crossterm::{
    cursor::Show,
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
use std::{io::stdout, process, time::Duration};

use crate::config::io::save_db;

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

    enable_raw_mode().ok();
    execute!(stdout(), EnterAlternateScreen).ok();
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        // --- Draw ---
        terminal
            .draw(|f| {
                let size = f.size();
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
                .block(Block::default().title("Filter").borders(Borders::ALL));
                f.render_widget(filter_para, left_chunks[0]);

                // ----- Build rows (folders + hosts) -----
                enum Row<'a> {
                    Folder(String),
                    Host(&'a Host),
                }
                let mut rows: Vec<Row> = Vec::new();

                if filter.is_empty() {
                    // Union of declared folders and folders inferred from hosts
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

                    // All + each folder
                    rows.push(Row::Folder("All".to_string()));
                    for f_name in &folders {
                        rows.push(Row::Folder(f_name.clone()));
                    }

                    // Hosts for the current folder
                    let host_iter: Vec<&Host> = match &current_folder {
                        None => items.iter().copied().collect(),
                        Some(fold) => items
                            .iter()
                            .copied()
                            .filter(|h| h.folder.as_deref() == Some(fold.as_str()))
                            .collect(),
                    };
                    for h in host_iter {
                        rows.push(Row::Host(h));
                    }
                } else {
                    // Filtered view ignores folders
                    for h in &filtered {
                        rows.push(Row::Host(h));
                    }
                }

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
                                "📁 All".to_string()
                            } else {
                                format!("📁 {}", name)
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

                let list = List::new(list_items)
                    .block(Block::default().title("Hosts (↑/↓ / filter)").borders(Borders::ALL))
                    .highlight_symbol("➜ ")
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

                f.render_stateful_widget(list, list_area, &mut list_state);

                // Scrollbar uses total rows
                let mut sb_state = ScrollbarState::new(last_rows_len.max(1))
                    .position(selected.saturating_sub(viewport_h / 2));
                let sb = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
                f.render_stateful_widget(sb, list_area, &mut sb_state);

                // ----- Details (only for Host rows) -----
                if let Some(sel) = (last_rows_len > 0).then(|| selected) {
                    // Determine selected row again
                    let mut idx = sel;
                    let selected_row = rows.get_mut(idx);
                    if let Some(Row::Host(h)) = selected_row {
                        let detail = format!(
                            "Name: {}\nUser: {}\nHost: {}\nPort: {}\nTags: {}\nIdentityFile: {}\nProxyJump: {}\nFolder: {}",
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
                            .block(Block::default().title("Details").borders(Borders::ALL));
                        f.render_widget(p, hchunks[1]);
                    }
                }

                // ----- Help -----
                let help = Paragraph::new(
                    "Shortcuts:  ↑/↓ move • Enter open/connect • a add • e edit • r rename • i add identity • d delete • q quit\n\
                     Notes: '/' to start filter, Enter to finish; folders shown when filter is empty."
                )
                .block(Block::default().title("Help").borders(Borders::ALL));
                f.render_widget(help, vchunks[1]);
            })
            .ok();

        // --- Events ---
        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
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
                                    // rows: All + folders => None, then hosts => Some
                                    rows_hosts.push(None); // All
                                    for _ in &folders {
                                        rows_hosts.push(None);
                                    }
                                    // hosts according to current folder
                                    let host_iter: Vec<&Host> = match &current_folder {
                                        None => items.iter().copied().collect(),
                                        Some(f) => items
                                            .iter()
                                            .copied()
                                            .filter(|h| h.folder.as_deref() == Some(f.as_str()))
                                            .collect(),
                                    };
                                    for h in host_iter {
                                        rows_hosts.push(Some(h));
                                    }
                                    // Act based on selection
                                    if let Some(row) = rows_hosts.get(selected).cloned() {
                                        match row {
                                            None => {
                                                // Navigating folders
                                                if selected == 0 {
                                                    current_folder = None; // All
                                                } else {
                                                    let idx = selected - 1; // 0 = All
                                                    if let Some(folder_name) = folders.get(idx) {
                                                        current_folder = Some(folder_name.clone());
                                                    }
                                                }
                                                selected = 0;
                                                list_state.select(Some(0));
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
                                        let _ = disable_raw_mode();
                                        let _ = execute!(stdout(), LeaveAlternateScreen);
                                        let _ = disable_raw_mode();
                                        let _ = execute!(stdout(), Show);
                                        process::exit(0);
                                    }
                                    'e' => {
                                        // Edit currently selected host (if a host is selected)
                                        // Rebuild quick view of current selection
                                        let mut current_host: Option<String> = None;
                                        if filter.is_empty() {
                                            // Determine if selection is a host row
                                            let mut folders: Vec<String> = db.folders.clone();
                                            for h in db.hosts.values() {
                                                if let Some(ref folder) = h.folder {
                                                    if !folders.iter().any(|f| f == folder) {
                                                        folders.push(folder.clone());
                                                    }
                                                }
                                            }
                                            folders.sort(); folders.dedup();
                                            let hosts_start = 1 + folders.len();
                                            if selected >= hosts_start {
                                                let idx = selected - hosts_start;
                                                let host_iter: Vec<&Host> = match &current_folder {
                                                    None => items.iter().copied().collect(),
                                                    Some(f) => items.iter().copied().filter(|h| h.folder.as_deref() == Some(f.as_str())).collect(),
                                                };
                                                if let Some(h) = host_iter.get(idx) { current_host = Some(h.name.clone()); }
                                            }
                                        } else {
                                            if let Some(h) = filtered.get(selected) { current_host = Some(h.name.clone()); }
                                        }

                                        if let Some(name) = current_host {
                                            let _ = disable_raw_mode();
                                            let _ = execute!(stdout(), LeaveAlternateScreen);
                                            crate::commands::crud::edit_host_by_name(&mut db.hosts, &name);
                                            save_db(db);
                                            let _ = enable_raw_mode();
                                            let _ = execute!(stdout(), EnterAlternateScreen);
                                            // refresh lists
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                        }
                                    }
                                    'r' => {
                                        // Rename selected host, if any
                                        let mut current_host: Option<String> = None;
                                        if filter.is_empty() {
                                            let mut folders: Vec<String> = db.folders.clone();
                                            for h in db.hosts.values() {
                                                if let Some(ref folder) = h.folder {
                                                    if !folders.iter().any(|f| f == folder) {
                                                        folders.push(folder.clone());
                                                    }
                                                }
                                            }
                                            folders.sort(); folders.dedup();
                                            let hosts_start = 1 + folders.len();
                                            if selected >= hosts_start {
                                                let idx = selected - hosts_start;
                                                let host_iter: Vec<&Host> = match &current_folder {
                                                    None => items.iter().copied().collect(),
                                                    Some(f) => items.iter().copied().filter(|h| h.folder.as_deref() == Some(f.as_str())).collect(),
                                                };
                                                if let Some(h) = host_iter.get(idx) { current_host = Some(h.name.clone()); }
                                            }
                                        } else if let Some(h) = filtered.get(selected) { current_host = Some(h.name.clone()); }

                                        if let Some(name) = current_host {
                                            let _ = disable_raw_mode();
                                            let _ = execute!(stdout(), LeaveAlternateScreen);
                                            crate::commands::crud::rename_host(&mut db.hosts, &name);
                                            save_db(db);
                                            let _ = enable_raw_mode();
                                            let _ = execute!(stdout(), EnterAlternateScreen);
                                            items = db.hosts.values().collect();
                                            items.sort_by(|a, b| a.name.cmp(&b.name));
                                            filtered = apply_filter(&filter, &items);
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                        }
                                    }
                                    'd' => {
                                        // Open delete menu (Host or Folder) as requested
                                        let _ = disable_raw_mode();
                                        let _ = execute!(stdout(), LeaveAlternateScreen);
                                        crate::commands::crud::delete(db);
                                        let _ = enable_raw_mode();
                                        let _ = execute!(stdout(), EnterAlternateScreen);
                                        items = db.hosts.values().collect();
                                        items.sort_by(|a, b| a.name.cmp(&b.name));
                                        filtered = apply_filter(&filter, &items);
                                        list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                    }
                                    'i' => {
                                        // Add identity to selected host, if any
                                        let mut current_host: Option<String> = None;
                                        if filter.is_empty() {
                                            let mut folders: Vec<String> = db.folders.clone();
                                            for h in db.hosts.values() {
                                                if let Some(ref folder) = h.folder {
                                                    if !folders.iter().any(|f| f == folder) {
                                                        folders.push(folder.clone());
                                                    }
                                                }
                                            }
                                            folders.sort(); folders.dedup();
                                            let hosts_start = 1 + folders.len();
                                            if selected >= hosts_start {
                                                let idx = selected - hosts_start;
                                                let host_iter: Vec<&Host> = match &current_folder {
                                                    None => items.iter().copied().collect(),
                                                    Some(f) => items.iter().copied().filter(|h| h.folder.as_deref() == Some(f.as_str())).collect(),
                                                };
                                                if let Some(h) = host_iter.get(idx) { current_host = Some(h.name.clone()); }
                                            }
                                        } else if let Some(h) = filtered.get(selected) { current_host = Some(h.name.clone()); }

                                        if let Some(name) = current_host {
                                            let _ = disable_raw_mode();
                                            let _ = execute!(stdout(), LeaveAlternateScreen);
                                            crate::ssh::add_identity::cmd_add_identity(&db.hosts, Some(name), &[]);
                                            let _ = enable_raw_mode();
                                            let _ = execute!(stdout(), EnterAlternateScreen);
                                        }
                                    }
                                    'a' => {
                                        // Create Host or Folder; host goes into current folder
                                        let _ = disable_raw_mode();
                                        let _ = execute!(stdout(), LeaveAlternateScreen);
                                        crate::commands::crud::create(db, current_folder.clone());
                                        let _ = enable_raw_mode();
                                        let _ = execute!(stdout(), EnterAlternateScreen);
                                        items = db.hosts.values().collect();
                                        items.sort_by(|a, b| a.name.cmp(&b.name));
                                        filtered = apply_filter(&filter, &items);
                                        list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                    }
                                    _ => {
                                        // Start implicit filter mode with this first char
                                        input_mode = true;
                                        filter.clear();
                                        filter.push(c);
                                        filtered = apply_filter(&filter, &items);
                                        selected = 0;
                                        list_state.select(if filtered.is_empty() { None } else { Some(0) });
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
