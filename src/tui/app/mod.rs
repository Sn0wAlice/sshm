use crate::filter::apply_filter;
use crate::history::{record_connection, sort_items, SortMode};
use crate::models::{Database, Host};
use crate::t;
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::tui::ssh::toast::Toast;
use crate::config::io::save_db;
use crate::config::settings::{load_settings, save_settings, AppConfig};
use crate::tui::functions::{rows_for, ViewMode};
use crate::tui::theme;
use crate::tui::tabs::tab_bar::draw_tab_bar;
use crate::tui::tabs::settings_tab::{self, SettingsFormState, SettingsAction};
use crate::tui::tabs::theme_tab::{self, ThemeTabState, ThemeAction};
use crate::tui::tabs::help_tab::{self, HelpTabState};
use crate::tui::tabs::identities_tab::{
    self, handle_identities_event, IdentitiesAction, IdentitiesTabState,
};
use crate::tui::tabs::kluster_tab::{
    self, handle_kluster_event, KlusterAction, KlusterTabState,
};

use crate::tui::ssh::folder_form_state::FolderFormState;
use crate::tui::ssh::host_form_state::HostFormState;

use crate::tui::char::q;

pub mod health_worker;
use health_worker::{spawn_health_worker, sync_health_targets, HealthTargets, WorkerGuard};


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

#[derive(Clone, PartialEq, Eq)]
pub enum HostStatus {
    /// TCP connect succeeded. `ssh_banner` holds the parsed remote version
    /// (e.g. `"OpenSSH_9.6"`) when the peer announced a valid `SSH-2.0-…`
    /// banner. `None` means the port is open but didn't speak SSH within
    /// the read timeout.
    Reachable { latency_ms: u32, ssh_banner: Option<String> },
    Unreachable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Hosts,
    Kluster,
    Identities,
    Settings,
    Theme,
    Help,
}

impl ActiveTab {
    pub fn next(self) -> Self {
        match self {
            Self::Hosts => Self::Kluster,
            Self::Kluster => Self::Identities,
            Self::Identities => Self::Settings,
            Self::Settings => Self::Theme,
            Self::Theme => Self::Help,
            Self::Help => Self::Hosts,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Hosts => Self::Help,
            Self::Kluster => Self::Hosts,
            Self::Identities => Self::Kluster,
            Self::Settings => Self::Identities,
            Self::Theme => Self::Settings,
            Self::Help => Self::Theme,
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Hosts => 0,
            Self::Kluster => 1,
            Self::Identities => 2,
            Self::Settings => 3,
            Self::Theme => 4,
            Self::Help => 5,
        }
    }
}

fn save_and_export(db: &Database, app_config: &AppConfig) {
    save_db(db);
    if !app_config.export_path.is_empty() {
        let _ = crate::config::export::export_ssh_config(db, &app_config.export_path);
    }
}

// Health worker → see `health_worker` submodule.
// Interactive key generation / known_hosts flows → see `key_flows` submodule.

pub mod key_flows;
use key_flows::{run_generate_key_flow, run_known_hosts_clean_flow};

pub mod fanout;

pub mod kluster_worker;
use kluster_worker::{spawn_kluster_worker, KlusterTargets, KlusterUpdate};

pub mod cluster_form;
pub mod kluster_actions;
use kluster_actions::{
    handle_kluster_open_logs, handle_kluster_open_shell,
    kluster_add_cluster_flow, kluster_add_docker_remote_flow, kluster_delete_cluster_flow,
    kluster_delete_docker_remote_flow, kluster_delete_pod_flow, kluster_edit_cluster_flow,
    sync_kluster_targets,
};

pub fn run_tui(db: &mut Database) {
    // Source items
    let mut sort_mode: SortMode = SortMode::Name;
    let mut view_mode: ViewMode = ViewMode::Folders;
    let mut items: Vec<&Host> = db.hosts.values().collect();
    sort_items(&mut items, sort_mode);

    // Multi-select state (set of host names currently selected for bulk actions).
    let mut selection: std::collections::HashSet<String> = std::collections::HashSet::new();

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
        let mut all_folders: std::collections::HashSet<String> =
            db.folders.iter().cloned().collect();
        for h in db.hosts.values() {
            if let Some(ref f) = h.folder {
                // Backfill every prefix (a/b/c → a, a/b, a/b/c) so all
                // ancestors of nested folders are collapsible too.
                let mut acc = String::new();
                for seg in f.split('/').filter(|s| !s.is_empty()) {
                    if !acc.is_empty() { acc.push('/'); }
                    acc.push_str(seg);
                    all_folders.insert(acc.clone());
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
    let mut identities_state = IdentitiesTabState::new();

    // Toast notification
    let mut toast: Option<Toast> = None;

    // Host reachability status (name → status)
    let mut host_status: HashMap<String, HostStatus> = HashMap::new();

    // --- Background health worker ---
    // Shared list of (name, host, port) targets; updated whenever the
    // host list changes so the worker always probes the current set.
    let health_targets: HealthTargets = Arc::new(Mutex::new(Vec::new()));
    sync_health_targets(&health_targets, db);
    let (health_tx, health_rx) = mpsc::channel::<(String, HostStatus)>();
    let health_stop = Arc::new(AtomicBool::new(false));
    let health_enabled = Arc::new(AtomicBool::new(app_config.auto_health_check));
    let health_interval_secs = Arc::new(AtomicU64::new(app_config.health_ttl_secs.max(1)));
    let health_probe_ms = Arc::new(AtomicU64::new(app_config.health_probe_timeout_ms.max(100)));
    let _health_guard = WorkerGuard(Arc::clone(&health_stop));
    spawn_health_worker(
        Arc::clone(&health_targets),
        Arc::clone(&health_stop),
        Arc::clone(&health_enabled),
        health_tx,
        Arc::clone(&health_interval_secs),
        Arc::clone(&health_probe_ms),
    );

    // --- Kluster tab state + background discovery ---
    let mut kluster_state = KlusterTabState::new();
    if kluster_state.bootstrap_imported > 0 {
        toast = Some(Toast::success(t!(
            "kluster.cluster_imported_n",
            "n" => kluster_state.bootstrap_imported
        )));
    }
    let kluster_targets: KlusterTargets = Arc::new(Mutex::new(
        kluster_worker::WorkerTargets::default(),
    ));
    sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
    let (kluster_tx, kluster_rx) = mpsc::channel::<KlusterUpdate>();
    let kluster_poke = Arc::new(AtomicBool::new(true)); // first refresh ASAP
    let kluster_interval_secs =
        Arc::new(AtomicU64::new(app_config.kluster_refresh_secs.max(2)));
    spawn_kluster_worker(
        Arc::clone(&kluster_targets),
        Arc::clone(&health_stop), // share the stop flag — same lifetime
        Arc::clone(&kluster_poke),
        kluster_tx,
        Arc::clone(&kluster_interval_secs),
    );

    enable_raw_mode().ok();
    execute!(stdout(), EnterAlternateScreen).ok();
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        // Expire toast
        if toast.as_ref().is_some_and(|t| t.is_expired()) {
            toast = None;
        }

        // Sync the worker's enabled flag with the current config and clear
        // any stale health data when the feature is turned off.
        let auto_enabled = app_config.auto_health_check;
        if health_enabled.load(Ordering::Relaxed) != auto_enabled {
            health_enabled.store(auto_enabled, Ordering::Relaxed);
            if !auto_enabled {
                host_status.clear();
            }
        }
        if auto_enabled {
            // Drain pending health-check results from the background worker.
            while let Ok((name, status)) = health_rx.try_recv() {
                host_status.insert(name, status);
            }
            // Keep the worker's target list in sync with the current DB.
            sync_health_targets(&health_targets, db);
        } else {
            // Discard any results produced before the user disabled the feature.
            while health_rx.try_recv().is_ok() {}
        }

        // Drain pending kluster discovery results.
        let mut kluster_dirty = false;
        while let Ok(update) = kluster_rx.try_recv() {
            match update {
                KlusterUpdate::Docker { available, containers } => {
                    kluster_state.docker_available = available;
                    kluster_state.docker_containers = containers;
                    kluster_dirty = true;
                }
                KlusterUpdate::Cluster { cluster_name, pods } => {
                    if let Some(idx) = kluster_state
                        .db
                        .clusters
                        .iter()
                        .position(|c| c.name == cluster_name)
                    {
                        if idx < kluster_state.cluster_pods.len() {
                            kluster_state.cluster_pods[idx] = Some(pods);
                            kluster_dirty = true;
                        }
                    }
                }
                KlusterUpdate::IncusLocal { available, instances } => {
                    kluster_state.incus_local_available = available;
                    kluster_state.incus_local_instances = instances;
                    kluster_dirty = true;
                }
                KlusterUpdate::IncusRemote { remote, instances } => {
                    kluster_state.incus_remote_instances.insert(remote, instances);
                    kluster_dirty = true;
                }
                KlusterUpdate::DockerRemote { host_alias, containers, reachable } => {
                    kluster_state.docker_remote_containers.insert(host_alias.clone(), containers);
                    kluster_state.docker_remote_reachable.insert(host_alias, reachable);
                    kluster_dirty = true;
                }
            }
        }
        if kluster_dirty {
            kluster_state.bootstrapped = true;
            kluster_state.rebuild_rows();
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
                        let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);

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
                        let list_items: Vec<ListItem> = crate::tui::ssh::listitems::get_item_list(&rows, &host_status, &selection, &theme);

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
                    ActiveTab::Kluster => {
                        kluster_tab::draw_kluster_tab(f, vchunks[1], &kluster_state, &theme);
                    }
                    ActiveTab::Identities => {
                        identities_tab::draw_identities_tab(f, vchunks[1], &identities_state, &theme);
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
                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                            match rows.get(selected) {
                                Some(Row::Folder { .. }) => HelpContext::FolderNav,
                                Some(Row::Host(_)) => HelpContext::HostNav,
                                None => HelpContext::Empty,
                            }
                        }
                    }
                    ActiveTab::Kluster if kluster_state.input_mode => HelpContext::FilterMode,
                    ActiveTab::Kluster => {
                        use crate::tui::tabs::kluster_tab::{KlusterRow, KlusterTarget};
                        match kluster_state.flat_rows.get(kluster_state.selected) {
                            Some(KlusterRow::ClusterHeader { .. }) => HelpContext::KlusterHeaderCluster,
                            Some(KlusterRow::DockerHeader { .. })
                            | Some(KlusterRow::IncusLocalHeader { .. })
                            | Some(KlusterRow::IncusRemoteHeader { .. }) => HelpContext::KlusterHeaderRuntime,
                            Some(KlusterRow::DockerRemoteHeader { .. }) => HelpContext::KlusterHeaderDockerRemote,
                            Some(KlusterRow::ClusterPod { .. }) => {
                                let terminal = match kluster_state.current_target() {
                                    Some(KlusterTarget::Pod { pod, .. }) => {
                                        pod.phase.eq_ignore_ascii_case("Succeeded")
                                            || pod.phase.eq_ignore_ascii_case("Failed")
                                    }
                                    _ => false,
                                };
                                if terminal { HelpContext::KlusterTerminalPod } else { HelpContext::KlusterItem }
                            }
                            Some(KlusterRow::DockerContainer(_))
                            | Some(KlusterRow::DockerRemoteContainer { .. })
                            | Some(KlusterRow::IncusLocalInstance(_))
                            | Some(KlusterRow::IncusRemoteInstance { .. }) => HelpContext::KlusterItem,
                            None => HelpContext::Empty,
                        }
                    }
                    ActiveTab::Identities => HelpContext::IdentitiesTab,
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
                        ActiveTab::Kluster => !kluster_state.input_mode,
                        ActiveTab::Identities => true,
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
                                            sort_items(&mut items, sort_mode);
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
                                            sort_items(&mut items, sort_mode);
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
                                            sort_items(&mut items, sort_mode);
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
                                    let mut launched_host: Option<String> = None;
                                    {
                                        let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                        if let Some(row) = rows.get(selected) {
                                            match row {
                                                Row::Folder { name, collapsed: is_c } => {
                                                    collapsed.insert(name.clone(), !is_c);
                                                }
                                                Row::Host(h) => {
                                                    let host_clone = (*h).clone();
                                                    let _ = disable_raw_mode();
                                                    let _ = execute!(stdout(), LeaveAlternateScreen);
                                                    crate::ssh::client::launch_ssh(&host_clone, &db.hosts, None);
                                                    let _ = enable_raw_mode();
                                                    let _ = execute!(stdout(), EnterAlternateScreen);
                                                    clear_console();
                                                    launched_host = Some(host_clone.name.clone());
                                                }
                                            }
                                        }
                                    }
                                    if let Some(name) = launched_host {
                                        // Drop borrows into db before mutating.
                                        filtered.clear();
                                        items.clear();
                                        if let Some(h) = db.hosts.get_mut(&name) {
                                            record_connection(h);
                                        }
                                        save_db(db);
                                        return;
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
                                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let state = HostFormState::new_edit(db, &h.name);
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                run_host_form(db, state);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                items = db.hosts.values().collect();
                                                sort_items(&mut items, sort_mode);
                                                filtered = apply_filter(&filter, &items);
                                                selected = 0;
                                                list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                                let _ = terminal.clear();
                                            }
                                        }
                                        'r' => {
                                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
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
                                                sort_items(&mut items, sort_mode);
                                                filtered = apply_filter(&filter, &items);
                                                selected = 0;
                                                list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                                let _ = terminal.clear();
                                            }
                                        }
                                        'd' => {
                                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
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
                                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                            if let Some(Row::Host(h)) = rows.get(selected) {
                                                let name = h.name.clone();
                                                let status = crate::tui::health::probe_host(
                                                    &h.host,
                                                    h.port,
                                                    Duration::from_millis(app_config.health_probe_timeout_ms.max(100)),
                                                );
                                                let msg = match &status {
                                                    HostStatus::Reachable { latency_ms, ssh_banner } => {
                                                        match ssh_banner {
                                                            Some(b) => format!("{} reachable ✓ ({} ms, {})", name, latency_ms, b),
                                                            None => format!("{} reachable ✓ ({} ms, no SSH banner)", name, latency_ms),
                                                        }
                                                    }
                                                    HostStatus::Unreachable => format!("{} is unreachable ✗", name),
                                                };
                                                toast = Some(match &status {
                                                    HostStatus::Reachable { .. } => Toast::success(msg),
                                                    HostStatus::Unreachable => Toast::error(msg),
                                                });
                                                host_status.insert(name, status);
                                            }
                                        }
                                        'p' => {
                                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                            let host_clone = if let Some(Row::Host(h)) = rows.get(selected) {
                                                Some((*h).clone())
                                            } else { None };
                                            drop(rows);
                                            if let Some(host_clone) = host_clone {
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                let updated = crate::tui::ssh::portforward::run_port_forward(
                                                    &host_clone,
                                                    &db.hosts,
                                                );
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                let _ = terminal.clear();
                                                if let Some(new_tunnels) = updated {
                                                    if let Some(host) = db.hosts.get_mut(&host_clone.name) {
                                                        host.tunnels = new_tunnels;
                                                    }
                                                    save_db(db);
                                                    items = db.hosts.values().collect();
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                }
                                            }
                                        }
                                        'i' => {
                                            let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
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
                                        's' => {
                                            sort_mode = sort_mode.next();
                                            sort_items(&mut items, sort_mode);
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                            toast = Some(Toast::success(t!(
                                                "toast.sort_changed",
                                                "label" => sort_mode.label()
                                            )));
                                        }
                                        'g' => {
                                            view_mode = view_mode.toggle();
                                            // Reset selection on view switch — what's "row N" changed.
                                            selected = 0;
                                            list_state.select(Some(0));
                                            toast = Some(Toast::success(t!(
                                                "toast.view_changed",
                                                "label" => view_mode.label()
                                            )));
                                        }
                                        'f' => {
                                            let target: Option<String> = {
                                                let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                                rows.get(selected).and_then(|r| match r {
                                                    Row::Host(h) => Some(h.name.clone()),
                                                    _ => None,
                                                })
                                            };
                                            if let Some(name) = target {
                                                filtered.clear();
                                                items.clear();
                                                let mut new_state = false;
                                                if let Some(h) = db.hosts.get_mut(&name) {
                                                    h.favorite = !h.favorite;
                                                    new_state = h.favorite;
                                                }
                                                save_db(db);
                                                items = db.hosts.values().collect();
                                                sort_items(&mut items, sort_mode);
                                                filtered = apply_filter(&filter, &items);
                                                toast = Some(Toast::success(format!(
                                                    "{} {}",
                                                    name,
                                                    if new_state { "★ favorited" } else { "unfavorited" }
                                                )));
                                            }
                                        }
                                        'a' => {
                                            // Determine folder context from selected row
                                            let folder_ctx = {
                                                let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
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
                                            sort_items(&mut items, sort_mode);
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                            let _ = terminal.clear();
                                        }
                                        'y' => {
                                            // Clone the selected host: full copy under a
                                            // unique `<name>-copy` alias (history reset),
                                            // then drop into the edit form to tweak it.
                                            let src_name: Option<String> = {
                                                let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                                rows.get(selected).and_then(|r| match r {
                                                    Row::Host(h) => Some(h.name.clone()),
                                                    _ => None,
                                                })
                                            };
                                            if let Some(src) = src_name {
                                                let mut clone_name = format!("{}-copy", src);
                                                let mut n = 2;
                                                while db.hosts.contains_key(&clone_name) {
                                                    clone_name = format!("{}-copy-{}", src, n);
                                                    n += 1;
                                                }
                                                if let Some(mut clone) = db.hosts.get(&src).cloned() {
                                                    clone.name = clone_name.clone();
                                                    clone.last_connected_at = None;
                                                    clone.use_count = 0;
                                                    clone.favorite = false;
                                                    db.hosts.insert(clone_name.clone(), clone);
                                                    save_and_export(db, &app_config);
                                                    let state = HostFormState::new_edit(db, &clone_name);
                                                    let _ = disable_raw_mode();
                                                    let _ = execute!(stdout(), LeaveAlternateScreen);
                                                    run_host_form(db, state);
                                                    let _ = enable_raw_mode();
                                                    let _ = execute!(stdout(), EnterAlternateScreen);
                                                    items = db.hosts.values().collect();
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                    selected = 0;
                                                    list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                                    let _ = terminal.clear();
                                                    toast = Some(Toast::success(format!("Cloned {} → {}", src, clone_name)));
                                                }
                                            }
                                        }
                                        ' ' => {
                                            // Toggle selection of the host on the current row.
                                            let target: Option<String> = {
                                                let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                                rows.get(selected).and_then(|r| match r {
                                                    Row::Host(h) => Some(h.name.clone()),
                                                    _ => None,
                                                })
                                            };
                                            if let Some(name) = target {
                                                if !selection.remove(&name) {
                                                    selection.insert(name);
                                                }
                                            }
                                        }
                                        'C' => {
                                            // Clear current bulk selection.
                                            if !selection.is_empty() {
                                                let n = selection.len();
                                                selection.clear();
                                                toast = Some(Toast::success(t!("toast.selection_cleared", "n" => n)));
                                            }
                                        }
                                        'D' => {
                                            if selection.is_empty() {
                                                toast = Some(Toast::error(t!("toast.nothing_selected")));
                                            } else {
                                                let names: Vec<String> = selection.iter().cloned().collect();
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                println!();
                                                let confirmed = inquire::Confirm::new(&format!(
                                                    "Delete {} host(s)? This cannot be undone.",
                                                    names.len()
                                                ))
                                                    .with_default(false)
                                                    .prompt()
                                                    .unwrap_or(false);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                let _ = terminal.clear();
                                                if confirmed {
                                                    filtered.clear();
                                                    items.clear();
                                                    for n in &names {
                                                        db.hosts.remove(n);
                                                    }
                                                    save_db(db);
                                                    items = db.hosts.values().collect();
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                    selection.clear();
                                                    selected = 0;
                                                    list_state.select(if filtered.is_empty() { None } else { Some(0) });
                                                    toast = Some(Toast::success(t!("toast.deleted_n_hosts", "n" => names.len())));
                                                }
                                            }
                                        }
                                        'T' => {
                                            if selection.is_empty() {
                                                toast = Some(Toast::error(t!("toast.nothing_selected")));
                                            } else {
                                                let names: Vec<String> = selection.iter().cloned().collect();
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                println!();
                                                let entry = inquire::Text::new(
                                                    &format!("Add tags to {} host(s) (comma-separated):", names.len())
                                                ).prompt().ok();
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                let _ = terminal.clear();
                                                if let Some(raw) = entry {
                                                    let new_tags: Vec<String> = raw
                                                        .split(',')
                                                        .map(|s| s.trim().to_string())
                                                        .filter(|s| !s.is_empty())
                                                        .collect();
                                                    if !new_tags.is_empty() {
                                                        filtered.clear();
                                                        items.clear();
                                                        for name in &names {
                                                            if let Some(h) = db.hosts.get_mut(name) {
                                                                let mut existing = h.tags.clone().unwrap_or_default();
                                                                for t in &new_tags {
                                                                    if !existing.iter().any(|e| e == t) {
                                                                        existing.push(t.clone());
                                                                    }
                                                                }
                                                                h.tags = if existing.is_empty() { None } else { Some(existing) };
                                                            }
                                                        }
                                                        save_db(db);
                                                        items = db.hosts.values().collect();
                                                        sort_items(&mut items, sort_mode);
                                                        filtered = apply_filter(&filter, &items);
                                                        toast = Some(Toast::success(t!(
                                                            "toast.tagged_n_hosts",
                                                            "n" => names.len(),
                                                            "tags" => new_tags.join(",")
                                                        )));
                                                    }
                                                }
                                            }
                                        }
                                        'X' => {
                                            // Fan-out: run one command across every bulk-selected host.
                                            if selection.is_empty() {
                                                toast = Some(Toast::error(t!("toast.nothing_selected")));
                                            } else {
                                                let mut names: Vec<String> = selection.iter().cloned().collect();
                                                names.sort();
                                                let _ = disable_raw_mode();
                                                let _ = execute!(stdout(), LeaveAlternateScreen);
                                                let result = fanout::run_fanout(&db.hosts, &names);
                                                let _ = enable_raw_mode();
                                                let _ = execute!(stdout(), EnterAlternateScreen);
                                                let _ = terminal.clear();
                                                if let Some((ok, failed)) = result {
                                                    toast = Some(Toast::success(format!(
                                                        "Fan-out done — {} ok, {} failed",
                                                        ok, failed
                                                    )));
                                                }
                                            }
                                        }
                                        '1'..='9' => {
                                            // Quick-connect to the Nth host in the *currently visible* row list.
                                            let n = (c as usize) - ('1' as usize);
                                            let host_name: Option<String> = {
                                                let rows = rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);
                                                rows.iter()
                                                    .filter_map(|r| if let Row::Host(h) = r { Some(h.name.clone()) } else { None })
                                                    .nth(n)
                                            };
                                            if let Some(name) = host_name {
                                                let host_clone = db.hosts.get(&name).cloned();
                                                if let Some(host_clone) = host_clone {
                                                    let _ = disable_raw_mode();
                                                    let _ = execute!(stdout(), LeaveAlternateScreen);
                                                    crate::ssh::client::launch_ssh(&host_clone, &db.hosts, None);
                                                    let _ = enable_raw_mode();
                                                    let _ = execute!(stdout(), EnterAlternateScreen);
                                                    clear_console();
                                                    filtered.clear();
                                                    items.clear();
                                                    if let Some(h) = db.hosts.get_mut(&host_clone.name) {
                                                        record_connection(h);
                                                    }
                                                    save_db(db);
                                                    return;
                                                }
                                            } else {
                                                toast = Some(Toast::error(t!("toast.quick_connect_oob", "n" => n + 1)));
                                            }
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

                        ActiveTab::Kluster => {
                            match handle_kluster_event(k.code, &mut kluster_state) {
                                KlusterAction::None => {}
                                KlusterAction::Refresh => {
                                    kluster_poke.store(true, Ordering::Relaxed);
                                }
                                KlusterAction::OpenShell => {
                                    handle_kluster_open_shell(
                                        &mut kluster_state,
                                        &mut terminal,
                                        &mut toast,
                                    );
                                }
                                KlusterAction::OpenLogsFollow => {
                                    handle_kluster_open_logs(
                                        &mut kluster_state,
                                        app_config.kluster_log_tail_lines,
                                        true,
                                        &mut terminal,
                                        &mut toast,
                                    );
                                }
                                KlusterAction::AddCluster => {
                                    if let Err(e) = kluster_add_cluster_flow(
                                        &mut kluster_state,
                                        &mut terminal,
                                    ) {
                                        toast = Some(Toast::error(format!("{e:#}")));
                                    } else {
                                        sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
                                        kluster_poke.store(true, Ordering::Relaxed);
                                    }
                                }
                                KlusterAction::EditCluster => {
                                    if let Err(e) = kluster_edit_cluster_flow(
                                        &mut kluster_state,
                                        &mut terminal,
                                    ) {
                                        toast = Some(Toast::error(format!("{e:#}")));
                                    } else {
                                        sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
                                        kluster_poke.store(true, Ordering::Relaxed);
                                    }
                                }
                                KlusterAction::DeleteCluster => {
                                    if let Err(e) = kluster_delete_cluster_flow(
                                        &mut kluster_state,
                                        &mut terminal,
                                    ) {
                                        toast = Some(Toast::error(format!("{e:#}")));
                                    } else {
                                        sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
                                    }
                                }
                                KlusterAction::DeletePod => {
                                    match kluster_delete_pod_flow(
                                        &mut kluster_state,
                                        &mut terminal,
                                    ) {
                                        Ok(Some(name)) => {
                                            toast = Some(Toast::success(format!("Deleted pod {}", name)));
                                            kluster_poke.store(true, Ordering::Relaxed);
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            toast = Some(Toast::error(format!("{e:#}")));
                                        }
                                    }
                                }
                                KlusterAction::AddDockerRemote => {
                                    match kluster_add_docker_remote_flow(
                                        &mut kluster_state,
                                        db,
                                        &mut terminal,
                                    ) {
                                        Ok(Some(alias)) => {
                                            toast = Some(Toast::success(format!("Added Docker remote: {}", alias)));
                                            sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
                                            kluster_poke.store(true, Ordering::Relaxed);
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            toast = Some(Toast::error(format!("{e:#}")));
                                        }
                                    }
                                }
                                KlusterAction::DeleteDockerRemote => {
                                    match kluster_delete_docker_remote_flow(
                                        &mut kluster_state,
                                        &mut terminal,
                                    ) {
                                        Ok(Some(alias)) => {
                                            toast = Some(Toast::success(format!("Removed Docker remote: {}", alias)));
                                            sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            toast = Some(Toast::error(format!("{e:#}")));
                                        }
                                    }
                                }
                            }
                        }

                        ActiveTab::Identities => {
                            match handle_identities_event(k.code, &mut identities_state) {
                                IdentitiesAction::None => {}
                                IdentitiesAction::Refresh => {
                                    identities_state.refresh();
                                    toast = Some(Toast::success(t!("toast.keys_refreshed")));
                                }
                                IdentitiesAction::Generate => {
                                    let _ = disable_raw_mode();
                                    let _ = execute!(stdout(), LeaveAlternateScreen);
                                    match run_generate_key_flow() {
                                        Ok(Some(path)) => {
                                            identities_state.refresh();
                                            toast = Some(Toast::success(t!(
                                                "toast.generated_key",
                                                "path" => path.display()
                                            )));
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            toast = Some(Toast::error(t!(
                                                "toast.generate_failed",
                                                "error" => e
                                            )));
                                        }
                                    }
                                    let _ = enable_raw_mode();
                                    let _ = execute!(stdout(), EnterAlternateScreen);
                                    let _ = terminal.clear();
                                }
                                IdentitiesAction::Push => {
                                    if let Some(k) = identities_state.selected_key() {
                                        let pub_path = k.public.clone();
                                        let _ = disable_raw_mode();
                                        let _ = execute!(stdout(), LeaveAlternateScreen);
                                        crate::ssh::add_identity::cmd_add_identity(
                                            &db.hosts,
                                            None,
                                            &[
                                                "--pub".to_string(),
                                                pub_path.display().to_string(),
                                            ],
                                        );
                                        let _ = enable_raw_mode();
                                        let _ = execute!(stdout(), EnterAlternateScreen);
                                        let _ = terminal.clear();
                                    } else {
                                        toast = Some(Toast::error(t!("toast.no_key_selected")));
                                    }
                                }
                                IdentitiesAction::AgentAdd => {
                                    if let Some(k) = identities_state.selected_key() {
                                        let path = k.private.clone();
                                        let _ = disable_raw_mode();
                                        let _ = execute!(stdout(), LeaveAlternateScreen);
                                        let res = crate::ssh::agent::agent_add(&path);
                                        let _ = enable_raw_mode();
                                        let _ = execute!(stdout(), EnterAlternateScreen);
                                        let _ = terminal.clear();
                                        match res {
                                            Ok(()) => {
                                                identities_state.refresh();
                                                toast = Some(Toast::success(t!("toast.agent_added")));
                                            }
                                            Err(e) => {
                                                toast = Some(Toast::error(t!(
                                                    "toast.agent_add_failed",
                                                    "error" => e
                                                )));
                                            }
                                        }
                                    }
                                }
                                IdentitiesAction::AgentRemove => {
                                    if let Some(k) = identities_state.selected_key() {
                                        let path = k.private.clone();
                                        match crate::ssh::agent::agent_remove(&path) {
                                            Ok(()) => {
                                                identities_state.refresh();
                                                toast = Some(Toast::success(t!("toast.agent_removed")));
                                            }
                                            Err(e) => {
                                                toast = Some(Toast::error(t!(
                                                    "toast.agent_remove_failed",
                                                    "error" => e
                                                )));
                                            }
                                        }
                                    }
                                }
                                IdentitiesAction::KnownHostsClean => {
                                    let _ = disable_raw_mode();
                                    let _ = execute!(stdout(), LeaveAlternateScreen);
                                    match run_known_hosts_clean_flow() {
                                        Ok(Some(host)) => {
                                            toast = Some(Toast::success(t!(
                                                "toast.known_hosts_removed",
                                                "host" => host
                                            )));
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            toast = Some(Toast::error(t!(
                                                "toast.known_hosts_clean_failed",
                                                "error" => e
                                            )));
                                        }
                                    }
                                    let _ = enable_raw_mode();
                                    let _ = execute!(stdout(), EnterAlternateScreen);
                                    let _ = terminal.clear();
                                }
                            }
                        }

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
                                                    app_config.auto_health_check = settings_state.auto_health_check;
                                                    if let Ok(v) = settings_state.health_ttl_secs.trim().parse::<u64>() {
                                                        app_config.health_ttl_secs = v.max(1);
                                                    }
                                                    if let Ok(v) = settings_state.health_probe_timeout_ms.trim().parse::<u64>() {
                                                        app_config.health_probe_timeout_ms = v.max(100);
                                                    }
                                                    if let Ok(v) = settings_state.kluster_refresh_secs.trim().parse::<u64>() {
                                                        app_config.kluster_refresh_secs = v.max(2);
                                                    }
                                                    if let Ok(v) = settings_state.kluster_log_tail_lines.trim().parse::<u32>() {
                                                        app_config.kluster_log_tail_lines = v.max(1);
                                                    }
                                                    // Push live values to the background workers.
                                                    health_interval_secs.store(app_config.health_ttl_secs, Ordering::Relaxed);
                                                    health_probe_ms.store(app_config.health_probe_timeout_ms, Ordering::Relaxed);
                                                    kluster_interval_secs.store(app_config.kluster_refresh_secs, Ordering::Relaxed);
                                                    save_settings(&app_config);
                                                    settings_state.dirty = false;
                                                    // Auto-export if export_path is set
                                                    if !app_config.export_path.is_empty() {
                                                        if let Err(e) = crate::config::export::export_ssh_config(db, &app_config.export_path) {
                                                            toast = Some(Toast::error(t!("toast.export_failed", "error" => e)));
                                                        } else {
                                                            toast = Some(Toast::success(t!("toast.settings_saved_exported")));
                                                        }
                                                    } else {
                                                        toast = Some(Toast::success(t!("toast.settings_saved")));
                                                    }
                                                }
                                                Err(_) => {
                                                    toast = Some(Toast::error(t!("toast.invalid_port")));
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


// ===== Host update form TUI → see `host_form` submodule. =====

pub mod host_form;
use crate::tui::ssh::modal::centered_rect;
use host_form::run_host_form;

