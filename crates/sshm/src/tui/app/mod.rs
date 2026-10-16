use crate::config::io::save_db;
use crate::config::settings::{load_settings, AppConfig};
use crate::filter::apply_filter;
use crate::history::{record_connection, sort_items, SortMode};
use crate::models::{Database, Host};
use crate::t;
use crate::tui::functions::{rows_for, ViewMode};
use crate::tui::ssh::toast::Toast;
use crate::tui::tabs::help_tab::{self, HelpTabState};
use crate::tui::tabs::identities_tab::{self, IdentitiesTabState};
use crate::tui::tabs::kluster_tab::{self, KlusterTabState};
use crate::tui::tabs::settings_tab::{self, SettingsFormState};
use crate::tui::tabs::tab_bar::draw_tab_bar;
use crate::tui::tabs::theme_tab::{self, ThemeTabState};
use crate::tui::theme;
use crate::util::clear_console;
use crate::watch::{ConfigWatcher, DbChanged};
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

use crate::tui::ssh::folder_form_state::FolderFormState;
use crate::tui::ssh::host_form_state::HostFormState;

use crate::tui::char::q;

pub mod health_worker;
use health_worker::{spawn_health_worker, sync_health_targets, HealthTargets, WorkerGuard};

pub mod sync_worker;
use sync_worker::{spawn_sync_worker, SharedSyncConfig, SyncMsg};

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
    Reachable {
        latency_ms: u32,
        ssh_banner: Option<String>,
    },
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

/// Re-read host.json before a mutation. `false` (with an error toast) when it
/// can't be read — the caller must then skip its change *and* its save, or it
/// would overwrite the file with a stale copy.
fn reload_or_toast(db: &mut Database, toast: &mut Option<Toast>) -> bool {
    match db.reload_if_changed() {
        Ok(_) => true,
        Err(e) => {
            *toast = Some(Toast::error(format!(
                "Not saved — could not re-read hosts: {e:#}"
            )));
            false
        }
    }
}

/// Name of the host on row `selected`, if that row is a host.
fn selected_host_name(rows: &[Row], selected: usize) -> Option<String> {
    match rows.get(selected) {
        Some(Row::Host(h)) => Some(h.name.clone()),
        _ => None,
    }
}

/// Row index of host `name`, so a selection survives rows being rebuilt.
fn host_row_index(rows: &[Row], name: &str) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, Row::Host(h) if h.name == name))
}

/// Leave the TUI, connect to `host` in the foreground, and come back.
///
/// The host's auto-start tunnels come up only once the destination is valid
/// and its key is trusted (their `BatchMode` ssh can't answer a TOFU prompt).
/// `Err` when ssh was never launched — the caller toasts it and must not bump
/// the connection history.
fn run_ssh_session(
    host: &Host,
    all_hosts: &HashMap<String, Host>,
    tunnels: &mut TunnelManager,
) -> Result<(), String> {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
    let res = crate::ssh::client::launch_ssh_with_recovery(host, all_hosts, None, || {
        let auto = tunnels.start_auto(host, all_hosts);
        if !auto.is_empty() {
            println!(
                "{}",
                t!("toast.auto_tunnels_started", "routes" => auto.join(", "))
            );
        }
    });
    let _ = enable_raw_mode();
    let _ = execute!(stdout(), EnterAlternateScreen);
    clear_console();
    res
}

// Health worker → see `health_worker` submodule.
// Interactive key generation / known_hosts flows → see `key_flows` submodule.

pub mod key_flows;
use key_flows::{run_host_fingerprint_flow, FingerprintOutcome};

pub mod fanout;

pub mod tab_events;
pub mod tunnels;
use tunnels::TunnelManager;

pub mod kluster_worker;
use kluster_worker::{spawn_kluster_worker, KlusterTargets, KlusterUpdate};

pub mod cluster_form;
pub mod kluster_actions;
use kluster_actions::sync_kluster_targets;

/// Restore the terminal and stop background tunnels if the UI thread panics,
/// then hand over to the previous hook so the message still prints — on the
/// normal screen, where it can be read. Installed once per process.
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Worker-thread panics don't take the UI down; leave it alone.
            if std::thread::current().name() == Some("main") {
                let _ = disable_raw_mode();
                let _ = execute!(stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
                tunnels::kill_all();
            }
            previous(info);
        }));
    });
}

/// Toast text for a manual (`c`) reachability check.
fn check_toast(name: &str, status: &HostStatus) -> Toast {
    match status {
        HostStatus::Reachable {
            latency_ms,
            ssh_banner: Some(b),
        } => Toast::success(format!("{} reachable ✓ ({} ms, {})", name, latency_ms, b)),
        HostStatus::Reachable { latency_ms, .. } => Toast::success(format!(
            "{} reachable ✓ ({} ms, no SSH banner)",
            name, latency_ms
        )),
        HostStatus::Unreachable => Toast::error(format!("{} is unreachable ✗", name)),
    }
}

pub fn run_tui(db: &mut Database, tunnels: &mut TunnelManager) {
    install_panic_hook();
    // Concurrent-safe live reload: another sshm instance (or a text editor)
    // may rewrite `host.json` under us. Sync once on entry — this
    // catches any change that happened while we were away in an ssh session —
    // then watch the config dir so we can hot-reload the host list in place.
    let _ = db.reload_if_changed();
    let watcher = ConfigWatcher::start().ok();
    // Set once a watched change is pending but a reload is being deferred (an
    // overlay modal is open); cleared when we actually reload.
    let mut pending_reload = false;
    // Same, for settings.toml (rewritten by a background config sync).
    let mut pending_settings_reload = false;

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
                    if !acc.is_empty() {
                        acc.push('/');
                    }
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

    // Help popup overlay: when true, `h` has opened the full-shortcut popup
    // and every keystroke is captured until it's dismissed.
    let mut help_popup = false;

    // Kluster rich-detail overlay (opened with `i` on a container/pod row).
    let mut kluster_detail: Option<crate::kluster::ContainerDetail> = None;
    let mut kluster_detail_scroll: usize = 0;

    // Background-tunnels dashboard overlay (opened with `t`).
    let mut tunnels_popup = false;
    let mut tunnels_popup_sel: usize = 0;

    // Tab state
    let mut active_tab = ActiveTab::Hosts;
    // True once the Kluster tab has been opened this session — gates the
    // one-time start of its background discovery worker.
    let mut kluster_started = false;
    let mut app_config = load_settings();
    crate::os::set_notifications_enabled(app_config.notifications_enabled);
    crate::os::set_notification_icon(&app_config.notification_icon);
    crate::kluster::shell::set_shell_path(&app_config.kluster_shell);
    let mut settings_state = SettingsFormState::from_config(&app_config);
    let mut theme_state = ThemeTabState::new();
    let mut help_state = HelpTabState::new();
    // Scanned on first visit to the tab, not before the first frame.
    let mut identities_state = IdentitiesTabState::empty();
    let mut identities_loaded = false;

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
    // Manual `c` checks probe off the UI thread and report through the same
    // channel; `manual_check` names the hosts whose result gets a toast.
    let check_tx = health_tx.clone();
    let mut manual_check: std::collections::HashSet<String> = Default::default();
    // Set wherever the DB is reloaded or mutated; the health worker's target
    // list is rebuilt only then, not every tick or keystroke.
    let mut health_targets_stale = false;
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

    // Surface (once) any background tunnels cleaned up after a previous crash.
    if tunnels.recovered_orphans > 0 {
        toast = Some(Toast::success(format!(
            "Cleaned {} orphaned tunnel(s) from a previous session",
            tunnels.recovered_orphans
        )));
        tunnels.recovered_orphans = 0;
    }
    let kluster_targets: KlusterTargets =
        Arc::new(Mutex::new(kluster_worker::WorkerTargets::default()));
    sync_kluster_targets(&kluster_targets, &mut kluster_state, &db.hosts);
    let (kluster_tx, kluster_rx) = mpsc::channel::<KlusterUpdate>();
    let kluster_poke = Arc::new(AtomicBool::new(true)); // first refresh ASAP
                                                        // The Kluster discovery worker (docker/kubectl/incus/apple polling) stays
                                                        // dormant until the user opens the Kluster tab at least once this session —
                                                        // a pure-SSH user never pays for container/cluster probing. Flipped true by
                                                        // the gate at the top of the event loop.
    let kluster_enabled = Arc::new(AtomicBool::new(false));
    let kluster_interval_secs = Arc::new(AtomicU64::new(app_config.kluster_refresh_secs.max(2)));
    spawn_kluster_worker(
        Arc::clone(&kluster_targets),
        Arc::clone(&health_stop), // share the stop flag — same lifetime
        Arc::clone(&kluster_enabled),
        Arc::clone(&kluster_poke),
        kluster_tx,
        Arc::clone(&kluster_interval_secs),
    );

    // --- Background config sync (git) ---
    // The schedule and the one-syncer-at-a-time rule live in `sshm_core::sync`
    // and are shared through the config dir, so several open sshm instances
    // still add up to a single sync per interval.
    let sync_cfg: SharedSyncConfig = Arc::new(Mutex::new(app_config.sync.clone()));
    let (sync_tx, sync_rx) = mpsc::channel::<SyncMsg>();
    // "Sync on start" is just a forced first run — once per process, not once
    // per `run_tui` (main re-enters it after every ssh session).
    static SYNC_ON_START_FIRED: AtomicBool = AtomicBool::new(false);
    let sync_on_start =
        app_config.sync.on_start && !SYNC_ON_START_FIRED.swap(true, Ordering::Relaxed);
    let sync_poke = Arc::new(AtomicBool::new(sync_on_start));
    spawn_sync_worker(
        Arc::clone(&sync_cfg),
        Arc::clone(&health_stop), // share the stop flag — same lifetime
        Arc::clone(&sync_poke),
        sync_tx,
    );

    enable_raw_mode().ok();
    execute!(stdout(), EnterAlternateScreen).ok();
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).unwrap();

    // Redraw only when something happened (input, a worker message, a reload,
    // a toast expiring) — rebuilding every row of a large host list 6×/s for
    // an idle screen is pure waste. The heartbeat keeps time-derived text
    // (relative "last used", tunnel uptimes) from going stale.
    const HEARTBEAT: Duration = Duration::from_secs(1);
    let mut needs_redraw = true;
    let mut last_draw = std::time::Instant::now();

    loop {
        // Expire toast
        if toast.as_ref().is_some_and(|t| t.is_expired()) {
            toast = None;
            needs_redraw = true;
        }

        // --- Live reload: pick up external changes to host.json ---
        // Drain the (debounced) watcher every tick so its channel never grows,
        // capturing any host change into `pending_reload`.
        if let Some(w) = &watcher {
            for change in w.poll() {
                match change {
                    DbChanged::Hosts => pending_reload = true,
                    // Config sync can rewrite settings.toml under us; without
                    // this the next Save would push our stale copy back out.
                    DbChanged::Settings => pending_settings_reload = true,
                    DbChanged::Kluster => {}
                }
            }
        }

        // Adopt externally-changed settings, unless the user is in the middle
        // of editing them — their unsaved edits win until they save or Esc.
        if pending_settings_reload && !settings_state.dirty {
            pending_settings_reload = false;
            needs_redraw = true;
            app_config = load_settings();
            settings_state = SettingsFormState::from_config(&app_config);
            crate::os::set_notifications_enabled(app_config.notifications_enabled);
            crate::os::set_notification_icon(&app_config.notification_icon);
            crate::kluster::shell::set_shell_path(&app_config.kluster_shell);
            health_interval_secs.store(app_config.health_ttl_secs.max(1), Ordering::Relaxed);
            health_probe_ms.store(
                app_config.health_probe_timeout_ms.max(100),
                Ordering::Relaxed,
            );
            kluster_interval_secs.store(app_config.kluster_refresh_secs.max(2), Ordering::Relaxed);
            if let Ok(mut shared) = sync_cfg.lock() {
                *shared = app_config.sync.clone();
            }
        }
        // Defer the actual reload while an overlay modal is open (delete confirm,
        // help popup, tunnels dashboard, kluster detail); the flag holds until it
        // closes. Editor forms run their own blocking loop, so they can never be
        // open at this point. Reloading rebuilds the `db`-borrowing `items`
        // in place, exactly like the CRUD paths below.
        let overlay_open = !matches!(delete_mode, DeleteMode::None)
            || help_popup
            || tunnels_popup
            || kluster_detail.is_some();
        if pending_reload && !overlay_open {
            pending_reload = false;
            needs_redraw = true;
            health_targets_stale = true;
            // Follow the selected host by name: rows shift when hosts are
            // added or removed elsewhere, and Enter / `d` must not land on a
            // different host than the one the user is looking at.
            let keep = selected_host_name(
                &rows_for(view_mode, db, &items, &filtered, &filter, &collapsed),
                selected,
            );
            let changed = match db.reload_if_changed() {
                Ok(c) => c,
                Err(e) => {
                    toast = Some(Toast::error(format!("Could not reload hosts: {e:#}")));
                    false
                }
            };
            items = db.hosts.values().collect();
            sort_items(&mut items, sort_mode);
            filtered = apply_filter(&filter, &items);
            // Otherwise the draw clamps `selected` to the new row count.
            if let Some(i) = keep.and_then(|n| {
                host_row_index(
                    &rows_for(view_mode, db, &items, &filtered, &filter, &collapsed),
                    &n,
                )
            }) {
                selected = i;
            }
            if changed {
                toast = Some(Toast::success(t!("toast.hosts_reloaded")));
            }
        }

        // Same idea for the Identities tab's `~/.ssh` scan.
        if !identities_loaded && matches!(active_tab, ActiveTab::Identities) {
            identities_loaded = true;
            identities_state.refresh();
        }

        // Lazily start the Kluster discovery worker the first time the user
        // lands on the Kluster tab. Until then it never polls docker/kubectl/
        // incus/apple — so a session that only ever connects over SSH pays
        // nothing for container/cluster discovery. `poke` forces an immediate
        // first pass instead of waiting a full interval.
        if !kluster_started && matches!(active_tab, ActiveTab::Kluster) {
            kluster_started = true;
            kluster_enabled.store(true, Ordering::Relaxed);
            kluster_poke.store(true, Ordering::Relaxed);
        }

        // Drop background tunnels whose ssh process has exited on its own,
        // then bring back the ones marked auto-restart whose backoff elapsed.
        if tunnels.reap() {
            needs_redraw = true;
        }
        let revived = tunnels.restart_due(&db.hosts);
        if !revived.is_empty() {
            needs_redraw = true;
            toast = Some(Toast::success(format!(
                "Tunnel restarted: {}",
                revived.join(", ")
            )));
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
        // Drain health-check results. With auto-check off, only the answer to
        // a manual `c` check is kept; anything else predates the switch-off.
        while let Ok((name, status)) = health_rx.try_recv() {
            if manual_check.remove(&name) {
                toast = Some(check_toast(&name, &status));
            } else if !auto_enabled {
                continue;
            }
            needs_redraw = true;
            // Desktop-notify on a reachable<->unreachable transition. The
            // first probe (no prior entry) is silent — only real changes.
            if let Some(prev) = host_status.get(&name).filter(|_| auto_enabled) {
                let was = matches!(prev, HostStatus::Reachable { .. });
                let now = matches!(status, HostStatus::Reachable { .. });
                if was && !now {
                    crate::os::notify("SSHM — host unreachable", &name);
                } else if !was && now {
                    crate::os::notify("SSHM — host back online", &name);
                }
            }
            host_status.insert(name, status);
        }
        // Keep the worker's target list in sync with the current DB.
        if auto_enabled && health_targets_stale {
            health_targets_stale = false;
            sync_health_targets(&health_targets, db);
        }

        // Surface the background config sync. The rewritten files are picked
        // up by the config watcher like any other external change, so there is
        // nothing to reload here — only something to say.
        while let Ok(msg) = sync_rx.try_recv() {
            needs_redraw = true;
            toast = Some(match msg {
                SyncMsg::Changed(text) | SyncMsg::Quiet(text) => {
                    Toast::success(t!("toast.sync_done", "summary" => text))
                }
                SyncMsg::Failed(err) => Toast::error(t!("toast.sync_failed", "error" => err)),
            });
        }

        // Drain pending kluster discovery results.
        let mut kluster_dirty = false;
        while let Ok(update) = kluster_rx.try_recv() {
            match update {
                KlusterUpdate::Apple {
                    available,
                    containers,
                } => {
                    kluster_state.apple_available = available;
                    kluster_state.apple_containers = containers;
                    kluster_dirty = true;
                }
                KlusterUpdate::Podman {
                    available,
                    containers,
                } => {
                    kluster_state.podman_available = available;
                    kluster_state.podman_containers = containers;
                    kluster_dirty = true;
                }
                KlusterUpdate::Docker {
                    available,
                    containers,
                } => {
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
                KlusterUpdate::IncusLocal {
                    available,
                    instances,
                } => {
                    kluster_state.incus_local_available = available;
                    kluster_state.incus_local_instances = instances;
                    kluster_dirty = true;
                }
                KlusterUpdate::IncusRemote { remote, instances } => {
                    kluster_state
                        .incus_remote_instances
                        .insert(remote, instances);
                    kluster_dirty = true;
                }
                KlusterUpdate::DockerRemote {
                    host_alias,
                    containers,
                    reachable,
                } => {
                    kluster_state
                        .docker_remote_containers
                        .insert(host_alias.clone(), containers);
                    kluster_state
                        .docker_remote_reachable
                        .insert(host_alias, reachable);
                    kluster_dirty = true;
                }
            }
        }
        if kluster_dirty {
            needs_redraw = true;
            kluster_state.bootstrapped = true;
            kluster_state.rebuild_rows();
        }
        // --- Draw ---
        if needs_redraw || last_draw.elapsed() >= HEARTBEAT {
            needs_redraw = false;
            last_draw = std::time::Instant::now();
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
                    // Hosts tab: whether the selected row is a folder (Some(true)) or
                    // a host (Some(false)) — captured from the rows built for the
                    // list so the help bar doesn't rebuild them.
                    let mut selected_is_folder: Option<bool> = None;

                    match active_tab {
                        ActiveTab::Hosts => {
                            let hchunks = Layout::default()
                                .direction(Direction::Horizontal)
                                .constraints([
                                    Constraint::Percentage(65),
                                    Constraint::Percentage(35),
                                ])
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
                                Span::styled(
                                    "Filter ",
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                                Span::raw(filter_label),
                            ]))
                            .block(
                                Block::default()
                                    .title("Filter")
                                    .borders(Borders::ALL)
                                    .border_style(Style::default().fg(theme.accent))
                                    .style(Style::default().bg(theme.bg).fg(theme.fg)),
                            );
                            f.render_widget(filter_para, left_chunks[0]);

                            // ----- Build rows (folders + hosts) -----
                            let rows =
                                rows_for(view_mode, db, &items, &filtered, &filter, &collapsed);

                            last_rows_len = rows.len();
                            if last_rows_len == 0 {
                                list_state.select(None);
                            } else {
                                if selected >= last_rows_len {
                                    selected = last_rows_len - 1;
                                }
                                list_state.select(Some(selected));
                            }
                            selected_is_folder =
                                rows.get(selected).map(|r| matches!(r, Row::Folder { .. }));

                            // ----- Render list -----
                            // Only the visible window becomes ListItems (a
                            // large host list would otherwise be styled in
                            // full every frame). The offset follows the same
                            // keep-selection-visible rule as `List` itself.
                            let mut offset = list_state
                                .offset()
                                .min(last_rows_len.saturating_sub(viewport_h));
                            if selected < offset {
                                offset = selected;
                            } else if selected >= offset + viewport_h {
                                offset = selected + 1 - viewport_h;
                            }
                            let end = (offset + viewport_h).min(last_rows_len);
                            let list_items: Vec<ListItem> =
                                crate::tui::ssh::listitems::get_item_list(
                                    &rows[offset.min(end)..end],
                                    &host_status,
                                    &selection,
                                    &theme,
                                );

                            let list_title = "Hosts (↑/↓ / filter)".to_string();
                            let list = List::new(list_items)
                                .block(
                                    Block::default()
                                        .title(list_title)
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

                            let mut window_state = ListState::default()
                                .with_selected(list_state.selected().map(|s| s - offset));
                            f.render_stateful_widget(list, list_area, &mut window_state);
                            *list_state.offset_mut() = offset;

                            let mut sb_state =
                                ScrollbarState::new(last_rows_len.max(1)).position(selected);
                            let sb = Scrollbar::default()
                                .orientation(ScrollbarOrientation::VerticalRight);
                            f.render_stateful_widget(sb, list_area, &mut sb_state);

                            // ----- Details (Host or Folder) -----
                            crate::tui::ssh::detailbox::show_detail_box(
                                last_rows_len,
                                selected,
                                &rows,
                                f,
                                &hchunks,
                                &theme,
                                db,
                                &host_status,
                            );

                            // ----- Delete confirmation modal -----
                            crate::tui::ssh::deletebox::show_delete_box(
                                &delete_mode,
                                delete_button_index,
                                f,
                                size,
                                &theme,
                            );
                        }
                        ActiveTab::Kluster => {
                            kluster_tab::draw_kluster_tab(f, vchunks[1], &kluster_state, &theme);
                        }
                        ActiveTab::Identities => {
                            identities_tab::draw_identities_tab(
                                f,
                                vchunks[1],
                                &identities_state,
                                &theme,
                            );
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
                                match selected_is_folder {
                                    Some(true) => HelpContext::FolderNav,
                                    Some(false) => HelpContext::HostNav,
                                    None => HelpContext::Empty,
                                }
                            }
                        }
                        ActiveTab::Kluster if kluster_state.input_mode => HelpContext::FilterMode,
                        ActiveTab::Kluster => {
                            use crate::tui::tabs::kluster_tab::{KlusterRow, KlusterTarget};
                            match kluster_state.flat_rows.get(kluster_state.selected) {
                                Some(KlusterRow::ClusterHeader { .. }) => {
                                    HelpContext::KlusterHeaderCluster
                                }
                                Some(KlusterRow::DockerHeader { .. })
                                | Some(KlusterRow::AppleHeader { .. })
                                | Some(KlusterRow::PodmanHeader { .. })
                                | Some(KlusterRow::IncusLocalHeader { .. })
                                | Some(KlusterRow::IncusRemoteHeader { .. }) => {
                                    HelpContext::KlusterHeaderRuntime
                                }
                                Some(KlusterRow::DockerRemoteHeader { .. }) => {
                                    HelpContext::KlusterHeaderDockerRemote
                                }
                                Some(KlusterRow::ClusterPod { .. }) => {
                                    let terminal = match kluster_state.current_target() {
                                        Some(KlusterTarget::Pod { pod, .. }) => {
                                            pod.phase.eq_ignore_ascii_case("Succeeded")
                                                || pod.phase.eq_ignore_ascii_case("Failed")
                                        }
                                        _ => false,
                                    };
                                    if terminal {
                                        HelpContext::KlusterTerminalPod
                                    } else {
                                        HelpContext::KlusterItem
                                    }
                                }
                                Some(KlusterRow::DockerContainer(_))
                                | Some(KlusterRow::AppleContainer(_))
                                | Some(KlusterRow::PodmanContainer(_))
                                | Some(KlusterRow::DockerRemoteContainer { .. })
                                | Some(KlusterRow::IncusLocalInstance(_))
                                | Some(KlusterRow::IncusRemoteInstance { .. }) => {
                                    HelpContext::KlusterItem
                                }
                                None => HelpContext::Empty,
                            }
                        }
                        ActiveTab::Identities if identities_state.input_mode => {
                            HelpContext::FilterMode
                        }
                        ActiveTab::Identities => HelpContext::IdentitiesTab,
                        ActiveTab::Settings => HelpContext::SettingsTab,
                        ActiveTab::Theme => HelpContext::ThemeTab,
                        ActiveTab::Help => HelpContext::HelpTab,
                    };
                    f.render_widget(
                        crate::tui::ssh::helpbox::get_contextual_help(
                            help_ctx,
                            &theme,
                            vchunks[2].width,
                        ),
                        vchunks[2],
                    );

                    // Toast overlay (rendered last, on top of everything)
                    if let Some(ref t) = toast {
                        if !t.is_expired() {
                            crate::tui::ssh::toast::render_toast(f, size, t, &theme);
                        }
                    }

                    // Kluster detail overlay — rich inspect view for one item.
                    if let Some(ref detail) = kluster_detail {
                        crate::tui::tabs::kluster_detail::draw_kluster_detail(
                            f,
                            detail,
                            &mut kluster_detail_scroll,
                            &theme,
                        );
                    }

                    // Help popup overlay — full shortcut list, above everything.
                    if help_popup {
                        crate::tui::ssh::helpbox::draw_help_popup(f, help_ctx, &theme);
                    }

                    // Background-tunnels dashboard overlay.
                    if tunnels_popup {
                        crate::tui::app::tunnels::draw_tunnels_popup(
                            f,
                            tunnels,
                            tunnels_popup_sel,
                            &theme,
                        );
                    }
                })
                .ok();
        }

        // --- Events ---
        if event::poll(Duration::from_millis(150)).unwrap_or(false) {
            // Any event (key, resize, …) may change what's on screen.
            needs_redraw = true;
            if let Ok(Event::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press {
                    // --- Kluster detail: modal, scroll + close ---
                    if let Some(ref detail) = kluster_detail {
                        let total = crate::tui::tabs::kluster_detail::detail_line_count(detail);
                        let page = 10;
                        match k.code {
                            KeyCode::Esc | KeyCode::Char('i') | KeyCode::Char('q') => {
                                kluster_detail = None;
                                kluster_detail_scroll = 0;
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                kluster_detail_scroll =
                                    (kluster_detail_scroll + 1).min(total.saturating_sub(1));
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                kluster_detail_scroll = kluster_detail_scroll.saturating_sub(1);
                            }
                            KeyCode::PageDown => {
                                kluster_detail_scroll =
                                    (kluster_detail_scroll + page).min(total.saturating_sub(1));
                            }
                            KeyCode::PageUp => {
                                kluster_detail_scroll = kluster_detail_scroll.saturating_sub(page);
                            }
                            KeyCode::Home => kluster_detail_scroll = 0,
                            KeyCode::End => kluster_detail_scroll = total.saturating_sub(1),
                            _ => {}
                        }
                        continue;
                    }

                    // --- Help popup: modal, swallows every keystroke ---
                    if help_popup {
                        if matches!(k.code, KeyCode::Esc | KeyCode::Char('h')) {
                            help_popup = false;
                        }
                        continue;
                    }

                    // --- Tunnels dashboard: modal, navigate + stop tunnels ---
                    if tunnels_popup {
                        tunnels.reap();
                        match k.code {
                            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => {
                                tunnels_popup = false;
                            }
                            KeyCode::Up => {
                                tunnels_popup_sel = tunnels_popup_sel.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                if tunnels_popup_sel + 1 < tunnels.len() {
                                    tunnels_popup_sel += 1;
                                }
                            }
                            KeyCode::Char('d') | KeyCode::Char('x') => {
                                if tunnels_popup_sel < tunnels.len() {
                                    tunnels.stop(tunnels_popup_sel);
                                    if tunnels_popup_sel >= tunnels.len() {
                                        tunnels_popup_sel = tunnels.len().saturating_sub(1);
                                    }
                                }
                            }
                            KeyCode::Char('o') => {
                                // Open a local (-L) tunnel's localhost URL in the browser.
                                match tunnels.active.get(tunnels_popup_sel) {
                                    Some(at)
                                        if at.tunnel.kind == crate::models::TunnelKind::Local =>
                                    {
                                        let url =
                                            format!("http://localhost:{}", at.tunnel.local_port);
                                        match crate::os::open_url(&url) {
                                            Ok(()) => {
                                                toast =
                                                    Some(Toast::success(format!("Opened {url}")))
                                            }
                                            Err(e) => toast = Some(Toast::error(e)),
                                        }
                                    }
                                    Some(_) => {
                                        toast = Some(Toast::error(
                                            "Open in browser only works for local (-L) tunnels"
                                                .to_string(),
                                        ))
                                    }
                                    None => {}
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // --- Global: tab navigation when not editing ---
                    let tab_nav_allowed = match active_tab {
                        ActiveTab::Hosts => !input_mode && matches!(delete_mode, DeleteMode::None),
                        ActiveTab::Kluster => !kluster_state.input_mode,
                        ActiveTab::Identities => !identities_state.input_mode,
                        ActiveTab::Settings => !settings_state.is_editing_field(),
                        ActiveTab::Theme => !theme_state.is_editing_custom_field(),
                        ActiveTab::Help => true,
                    };

                    // Settings text fields have no cursor, so ←→ mean nothing
                    // there: keep them switching tabs, as the help bar says.
                    // Letter shortcuts stay blocked while a field takes input.
                    if matches!(active_tab, ActiveTab::Settings) && !tab_nav_allowed {
                        match k.code {
                            KeyCode::Right => {
                                active_tab = active_tab.next();
                                continue;
                            }
                            KeyCode::Left => {
                                active_tab = active_tab.prev();
                                continue;
                            }
                            _ => {}
                        }
                    }

                    if tab_nav_allowed {
                        match k.code {
                            KeyCode::Right => {
                                active_tab = active_tab.next();
                                continue;
                            }
                            KeyCode::Left => {
                                active_tab = active_tab.prev();
                                continue;
                            }
                            KeyCode::Char('h') => {
                                help_popup = true;
                                continue;
                            }
                            KeyCode::Char('t') => {
                                tunnels_popup = true;
                                tunnels_popup_sel = 0;
                                continue;
                            }
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                q::press();
                            }
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
                                            DeleteMode::Host { .. }
                                            | DeleteMode::EmptyFolder { .. } => 1,
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
                                                    if reload_or_toast(db, &mut toast) {
                                                        db.hosts.remove(name);
                                                        save_and_export(db, &app_config);
                                                        toast = Some(Toast::success(format!(
                                                            "Deleted: {}",
                                                            deleted_name
                                                        )));
                                                    }
                                                    items = db.hosts.values().collect();
                                                    health_targets_stale = true;
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                    selected = 0;
                                                    list_state.select(if filtered.is_empty() {
                                                        None
                                                    } else {
                                                        Some(0)
                                                    });
                                                }
                                                delete_mode = DeleteMode::None;
                                                delete_button_index = 0;
                                            }
                                            DeleteMode::EmptyFolder { name } => {
                                                if delete_button_index == 0 {
                                                    let deleted_name = name.clone();
                                                    let reloaded = reload_or_toast(db, &mut toast);
                                                    let prefix = format!("{}/", name);
                                                    if reloaded {
                                                        // Remove this folder + sub-folders from collapsed
                                                        collapsed.retain(|k, _| {
                                                            k != name && !k.starts_with(&prefix)
                                                        });
                                                        db.folders.retain(|f| {
                                                            f != name && !f.starts_with(&prefix)
                                                        });
                                                        for h in db.hosts.values_mut() {
                                                            if let Some(ref f) = h.folder {
                                                                if f == name
                                                                    || f.starts_with(&prefix)
                                                                {
                                                                    h.folder = None;
                                                                }
                                                            }
                                                        }
                                                        save_and_export(db, &app_config);
                                                    }
                                                    items = db.hosts.values().collect();
                                                    health_targets_stale = true;
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                    selected = 0;
                                                    list_state.select(if filtered.is_empty() {
                                                        None
                                                    } else {
                                                        Some(0)
                                                    });
                                                    if reloaded {
                                                        toast = Some(Toast::success(format!(
                                                            "Deleted folder: {}",
                                                            deleted_name
                                                        )));
                                                    }
                                                }
                                                delete_mode = DeleteMode::None;
                                                delete_button_index = 0;
                                            }
                                            DeleteMode::FolderWithHosts { name, .. } => {
                                                let deleted_name = name.clone();
                                                let prefix = format!("{}/", name);
                                                // Cancel (2) touches nothing; a failed
                                                // reload skips the change and the save.
                                                let apply = delete_button_index < 2
                                                    && reload_or_toast(db, &mut toast);
                                                match if apply { delete_button_index } else { 2 } {
                                                    0 => {
                                                        // Delete folder + sub-folders + all hosts inside
                                                        collapsed.retain(|k, _| {
                                                            k != name && !k.starts_with(&prefix)
                                                        });
                                                        db.hosts.retain(|_, h| {
                                                            if let Some(ref f) = h.folder {
                                                                f != name && !f.starts_with(&prefix)
                                                            } else {
                                                                true
                                                            }
                                                        });
                                                        db.folders.retain(|f| {
                                                            f != name && !f.starts_with(&prefix)
                                                        });
                                                        toast = Some(Toast::success(format!(
                                                            "Deleted folder & hosts: {}",
                                                            deleted_name
                                                        )));
                                                    }
                                                    1 => {
                                                        // Delete folder + sub-folders, move hosts to root
                                                        collapsed.retain(|k, _| {
                                                            k != name && !k.starts_with(&prefix)
                                                        });
                                                        for h in db.hosts.values_mut() {
                                                            if let Some(ref f) = h.folder.clone() {
                                                                if f == name
                                                                    || f.starts_with(&prefix)
                                                                {
                                                                    h.folder = None;
                                                                }
                                                            }
                                                        }
                                                        db.folders.retain(|f| {
                                                            f != name && !f.starts_with(&prefix)
                                                        });
                                                        toast = Some(Toast::success(format!(
                                                            "Deleted folder: {}",
                                                            deleted_name
                                                        )));
                                                    }
                                                    _ => { /* Cancel */ }
                                                }
                                                if apply {
                                                    save_and_export(db, &app_config);
                                                }
                                                items = db.hosts.values().collect();
                                                health_targets_stale = true;
                                                sort_items(&mut items, sort_mode);
                                                filtered = apply_filter(&filter, &items);
                                                selected = 0;
                                                list_state.select(if filtered.is_empty() {
                                                    None
                                                } else {
                                                    Some(0)
                                                });
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
                                            list_state.select(if filtered.is_empty() {
                                                None
                                            } else {
                                                Some(0)
                                            });
                                        }
                                    }

                                    KeyCode::Char('/') => {
                                        input_mode = true;
                                        filter.clear();
                                        filtered = apply_filter(&filter, &items);
                                        selected = 0;
                                        list_state.select(if filtered.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    }

                                    KeyCode::Backspace => {
                                        if input_mode {
                                            filter.pop();
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() {
                                                None
                                            } else {
                                                Some(0)
                                            });
                                        }
                                    }

                                    KeyCode::Enter => {
                                        if input_mode {
                                            input_mode = false;
                                        } else {
                                            let mut launched_host: Option<String> = None;
                                            {
                                                let rows = rows_for(
                                                    view_mode, db, &items, &filtered, &filter,
                                                    &collapsed,
                                                );
                                                if let Some(row) = rows.get(selected) {
                                                    match row {
                                                        Row::Folder {
                                                            name,
                                                            collapsed: is_c,
                                                        } => {
                                                            collapsed.insert(name.clone(), !is_c);
                                                        }
                                                        Row::Host(h) => {
                                                            let host_clone = (*h).clone();
                                                            // Silence the background workers (host health +
                                                            // Kluster docker/kubectl/incus polling) for the
                                                            // duration of the foreground session; they resume
                                                            // when `n` re-enters after we return.
                                                            if app_config.pause_health_on_session {
                                                                health_enabled.store(
                                                                    false,
                                                                    Ordering::Relaxed,
                                                                );
                                                                kluster_enabled.store(
                                                                    false,
                                                                    Ordering::Relaxed,
                                                                );
                                                            }
                                                            match run_ssh_session(
                                                                &host_clone,
                                                                &db.hosts,
                                                                tunnels,
                                                            ) {
                                                                Ok(()) => {
                                                                    launched_host = Some(
                                                                        host_clone.name.clone(),
                                                                    )
                                                                }
                                                                Err(e) => {
                                                                    kluster_enabled.store(
                                                                        kluster_started,
                                                                        Ordering::Relaxed,
                                                                    );
                                                                    toast = Some(Toast::error(e));
                                                                    let _ = terminal.clear();
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(name) = launched_host {
                                                // Drop borrows into db before mutating.
                                                filtered.clear();
                                                items.clear();
                                                if reload_or_toast(db, &mut toast) {
                                                    if let Some(h) = db.hosts.get_mut(&name) {
                                                        record_connection(h);
                                                    }
                                                    save_db(db);
                                                    return;
                                                }
                                                // History not saved: stay, and say why.
                                                kluster_enabled
                                                    .store(kluster_started, Ordering::Relaxed);
                                                items = db.hosts.values().collect();
                                                health_targets_stale = true;
                                                sort_items(&mut items, sort_mode);
                                                filtered = apply_filter(&filter, &items);
                                            }
                                        }
                                    }

                                    KeyCode::Char(c) => {
                                        if input_mode {
                                            filter.push(c);
                                            filtered = apply_filter(&filter, &items);
                                            selected = 0;
                                            list_state.select(if filtered.is_empty() {
                                                None
                                            } else {
                                                Some(0)
                                            });
                                        } else {
                                            match c {
                                                'q' | 'Q' => { /* handled globally above */ }
                                                'e' => {
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    if let Some(Row::Host(h)) = rows.get(selected) {
                                                        let name = h.name.clone();
                                                        if !reload_or_toast(db, &mut toast) {
                                                            // toast says why
                                                        } else if !db.hosts.contains_key(&name) {
                                                            // Deleted elsewhere: an edit form
                                                            // for it would open blank.
                                                            toast = Some(Toast::error(format!(
                                                                "{name} no longer exists"
                                                            )));
                                                        } else {
                                                            let state =
                                                                HostFormState::new_edit(db, &name);
                                                            let _ = disable_raw_mode();
                                                            let _ = execute!(
                                                                stdout(),
                                                                LeaveAlternateScreen
                                                            );
                                                            run_host_form(db, state);
                                                            let _ = enable_raw_mode();
                                                            let _ = execute!(
                                                                stdout(),
                                                                EnterAlternateScreen
                                                            );
                                                        }
                                                        items = db.hosts.values().collect();
                                                        health_targets_stale = true;
                                                        sort_items(&mut items, sort_mode);
                                                        filtered = apply_filter(&filter, &items);
                                                        selected = 0;
                                                        list_state.select(if filtered.is_empty() {
                                                            None
                                                        } else {
                                                            Some(0)
                                                        });
                                                        let _ = terminal.clear();
                                                    }
                                                }
                                                'F' => {
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    if let Some(Row::Host(h)) = rows.get(selected) {
                                                        let (host, port) = (h.host.clone(), h.port);
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        match run_host_fingerprint_flow(&host, port)
                                                        {
                                                            Ok(FingerprintOutcome::Pinned) => {
                                                                toast = Some(Toast::success(t!(
                                                                    "toast.fingerprint_pinned",
                                                                    "host" => host.clone()
                                                                )));
                                                            }
                                                            Ok(FingerprintOutcome::Forgotten) => {
                                                                toast = Some(Toast::success(t!(
                                                                    "toast.known_hosts_removed",
                                                                    "host" => host.clone()
                                                                )));
                                                            }
                                                            Ok(FingerprintOutcome::Nothing) => {}
                                                            Err(e) => {
                                                                toast = Some(Toast::error(t!(
                                                                    "toast.fingerprint_failed",
                                                                    "error" => e
                                                                )));
                                                            }
                                                        }
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
                                                        let _ = terminal.clear();
                                                    }
                                                }
                                                'r' => {
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    if let Some(Row::Folder {
                                                        name: folder_name,
                                                        ..
                                                    }) = rows.get(selected)
                                                    {
                                                        let folder_name = folder_name.clone();
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        run_folder_rename_form(db, &folder_name);
                                                        // Rebuild collapsed map: keep states for folders that still exist
                                                        let old_collapsed = collapsed.clone();
                                                        collapsed.clear();
                                                        for f in &db.folders {
                                                            let state = old_collapsed
                                                                .get(f)
                                                                .copied()
                                                                .unwrap_or(true);
                                                            collapsed.insert(f.clone(), state);
                                                        }
                                                        // apply_folder_form already saved (and a
                                                        // cancel must not save at all).
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
                                                        items = db.hosts.values().collect();
                                                        health_targets_stale = true;
                                                        sort_items(&mut items, sort_mode);
                                                        filtered = apply_filter(&filter, &items);
                                                        selected = 0;
                                                        list_state.select(if filtered.is_empty() {
                                                            None
                                                        } else {
                                                            Some(0)
                                                        });
                                                        let _ = terminal.clear();
                                                    }
                                                }
                                                'd' => {
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    if let Some(row) = rows.get(selected) {
                                                        match row {
                                                            Row::Host(h) => {
                                                                delete_mode = DeleteMode::Host {
                                                                    name: h.name.clone(),
                                                                };
                                                                delete_button_index = 0;
                                                            }
                                                            Row::Folder {
                                                                name: folder_name,
                                                                ..
                                                            } => {
                                                                let prefix =
                                                                    format!("{}/", folder_name);
                                                                let count = db
                                                                    .hosts
                                                                    .values()
                                                                    .filter(|h| {
                                                                        if let Some(ref f) =
                                                                            h.folder
                                                                        {
                                                                            f == folder_name
                                                                                || f.starts_with(
                                                                                    &prefix,
                                                                                )
                                                                        } else {
                                                                            false
                                                                        }
                                                                    })
                                                                    .count();
                                                                delete_button_index = 0;
                                                                if count == 0 {
                                                                    delete_mode =
                                                                        DeleteMode::EmptyFolder {
                                                                            name: folder_name
                                                                                .clone(),
                                                                        };
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
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    if let Some(Row::Host(h)) = rows.get(selected) {
                                                        // Probe off the UI thread; the result
                                                        // comes back through the health channel.
                                                        let (name, host, port) = (
                                                            h.name.clone(),
                                                            h.host.clone(),
                                                            h.port,
                                                        );
                                                        let timeout = Duration::from_millis(
                                                            app_config
                                                                .health_probe_timeout_ms
                                                                .max(100),
                                                        );
                                                        let tx = check_tx.clone();
                                                        let reply = name.clone();
                                                        std::thread::spawn(move || {
                                                            let status =
                                                                crate::tui::health::probe_host(
                                                                    &host, port, timeout,
                                                                );
                                                            let _ = tx.send((reply, status));
                                                        });
                                                        toast = Some(Toast::success(format!(
                                                            "Checking {}…",
                                                            name
                                                        )));
                                                        manual_check.insert(name);
                                                    }
                                                }
                                                'p' => {
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    let host_clone = if let Some(Row::Host(h)) =
                                                        rows.get(selected)
                                                    {
                                                        Some((*h).clone())
                                                    } else {
                                                        None
                                                    };
                                                    drop(rows);
                                                    if let Some(host_clone) = host_clone {
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        let result = crate::tui::ssh::portforward::run_port_forward(
                                                    &host_clone,
                                                    &db.hosts,
                                                );
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
                                                        let _ = terminal.clear();
                                                        if let Some(new_tunnels) =
                                                            result.updated_tunnels
                                                        {
                                                            if reload_or_toast(db, &mut toast) {
                                                                if let Some(host) = db
                                                                    .hosts
                                                                    .get_mut(&host_clone.name)
                                                                {
                                                                    host.tunnels = new_tunnels;
                                                                }
                                                                save_db(db);
                                                            }
                                                            items = db.hosts.values().collect();
                                                            health_targets_stale = true;
                                                            sort_items(&mut items, sort_mode);
                                                            filtered =
                                                                apply_filter(&filter, &items);
                                                        }
                                                        if let Some(t) = result.start_background {
                                                            match tunnels.start(&host_clone, &t, &db.hosts) {
                                                        Ok(()) => toast = Some(Toast::success(
                                                            format!("Tunnel started in background ({})", host_clone.name)
                                                        )),
                                                        Err(e) => toast = Some(Toast::error(
                                                            format!("Tunnel failed to start: {e}")
                                                        )),
                                                    }
                                                        }
                                                    }
                                                }
                                                'o' => {
                                                    // Open the SSH session in a new terminal window.
                                                    let host_clone = {
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
                                                        if let Some(Row::Host(h)) =
                                                            rows.get(selected)
                                                        {
                                                            Some((*h).clone())
                                                        } else {
                                                            None
                                                        }
                                                    };
                                                    if let Some(host_clone) = host_clone {
                                                        let argv =
                                                            crate::ssh::client::build_ssh_argv(
                                                                &host_clone,
                                                                &db.hosts,
                                                            );
                                                        match crate::os::open_in_terminal(
                                                            &argv,
                                                            &app_config.external_terminal,
                                                        ) {
                                                            Ok(()) => {
                                                                toast = Some(Toast::success(t!(
                                                                    "toast.opened_in_terminal",
                                                                    "name" => host_clone.name
                                                                )));
                                                            }
                                                            Err(e) => {
                                                                toast = Some(Toast::error(t!(
                                                                    "toast.terminal_failed",
                                                                    "error" => e
                                                                )));
                                                            }
                                                        }
                                                    }
                                                }
                                                'i' => {
                                                    let rows = rows_for(
                                                        view_mode, db, &items, &filtered, &filter,
                                                        &collapsed,
                                                    );
                                                    if let Some(Row::Host(h)) = rows.get(selected) {
                                                        let name = h.name.clone();
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        crate::ssh::add_identity::cmd_add_identity(
                                                            &db.hosts,
                                                            Some(name),
                                                            &[],
                                                        );
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
                                                        let _ = terminal.clear();
                                                    }
                                                }
                                                's' => {
                                                    sort_mode = sort_mode.next();
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                    selected = 0;
                                                    list_state.select(if filtered.is_empty() {
                                                        None
                                                    } else {
                                                        Some(0)
                                                    });
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
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
                                                        rows.get(selected).and_then(|r| match r {
                                                            Row::Host(h) => Some(h.name.clone()),
                                                            _ => None,
                                                        })
                                                    };
                                                    if let Some(name) = target {
                                                        filtered.clear();
                                                        items.clear();
                                                        if reload_or_toast(db, &mut toast) {
                                                            match db.hosts.get_mut(&name) {
                                                                Some(h) => {
                                                                    h.favorite = !h.favorite;
                                                                    let fav = h.favorite;
                                                                    save_db(db);
                                                                    toast = Some(Toast::success(
                                                                        format!(
                                                                            "{} {}",
                                                                            name,
                                                                            if fav {
                                                                                "★ favorited"
                                                                            } else {
                                                                                "unfavorited"
                                                                            }
                                                                        ),
                                                                    ));
                                                                }
                                                                None => {
                                                                    toast = Some(Toast::error(
                                                                        format!(
                                                                            "{name} no longer exists"
                                                                        ),
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                        items = db.hosts.values().collect();
                                                        health_targets_stale = true;
                                                        sort_items(&mut items, sort_mode);
                                                        filtered = apply_filter(&filter, &items);
                                                        // A favorite can re-sort; keep the
                                                        // cursor on the host just toggled.
                                                        if let Some(i) = host_row_index(
                                                            &rows_for(
                                                                view_mode, db, &items, &filtered,
                                                                &filter, &collapsed,
                                                            ),
                                                            &name,
                                                        ) {
                                                            selected = i;
                                                        }
                                                    }
                                                }
                                                'a' => {
                                                    // Determine folder context from selected row
                                                    let folder_ctx = {
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
                                                        match rows.get(selected) {
                                                            Some(Row::Folder { name, .. }) => {
                                                                Some(name.clone())
                                                            }
                                                            Some(Row::Host(h)) => h.folder.clone(),
                                                            None => None,
                                                        }
                                                    };
                                                    let _ = disable_raw_mode();
                                                    let _ =
                                                        execute!(stdout(), LeaveAlternateScreen);
                                                    // apply_host_form reloads before saving.
                                                    let state = HostFormState::new_create(
                                                        folder_ctx,
                                                        &app_config,
                                                    );
                                                    run_host_form(db, state);
                                                    let _ = enable_raw_mode();
                                                    let _ =
                                                        execute!(stdout(), EnterAlternateScreen);
                                                    items = db.hosts.values().collect();
                                                    health_targets_stale = true;
                                                    sort_items(&mut items, sort_mode);
                                                    filtered = apply_filter(&filter, &items);
                                                    selected = 0;
                                                    list_state.select(if filtered.is_empty() {
                                                        None
                                                    } else {
                                                        Some(0)
                                                    });
                                                    let _ = terminal.clear();
                                                }
                                                'y' => {
                                                    // Clone the selected host: full copy under a
                                                    // unique `<name>-copy` alias (history reset),
                                                    // then drop into the edit form to tweak it.
                                                    let src_name: Option<String> = {
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
                                                        rows.get(selected).and_then(|r| match r {
                                                            Row::Host(h) => Some(h.name.clone()),
                                                            _ => None,
                                                        })
                                                    };
                                                    if let Some(src) = src_name {
                                                        if reload_or_toast(db, &mut toast) {
                                                            let mut clone_name =
                                                                format!("{}-copy", src);
                                                            let mut n = 2;
                                                            while db.hosts.contains_key(&clone_name)
                                                            {
                                                                clone_name =
                                                                    format!("{}-copy-{}", src, n);
                                                                n += 1;
                                                            }
                                                            if let Some(mut clone) =
                                                                db.hosts.get(&src).cloned()
                                                            {
                                                                clone.name = clone_name.clone();
                                                                clone.last_connected_at = None;
                                                                clone.use_count = 0;
                                                                clone.favorite = false;
                                                                db.hosts.insert(
                                                                    clone_name.clone(),
                                                                    clone,
                                                                );
                                                                save_and_export(db, &app_config);
                                                                let state = HostFormState::new_edit(
                                                                    db,
                                                                    &clone_name,
                                                                );
                                                                let _ = disable_raw_mode();
                                                                let _ = execute!(
                                                                    stdout(),
                                                                    LeaveAlternateScreen
                                                                );
                                                                run_host_form(db, state);
                                                                let _ = enable_raw_mode();
                                                                let _ = execute!(
                                                                    stdout(),
                                                                    EnterAlternateScreen
                                                                );
                                                                let _ = terminal.clear();
                                                                toast =
                                                                    Some(Toast::success(format!(
                                                                        "Cloned {} → {}",
                                                                        src, clone_name
                                                                    )));
                                                            }
                                                        }
                                                        // Rebuilt either way: the reload above
                                                        // may have replaced the host map.
                                                        items = db.hosts.values().collect();
                                                        health_targets_stale = true;
                                                        sort_items(&mut items, sort_mode);
                                                        filtered = apply_filter(&filter, &items);
                                                        selected = 0;
                                                        list_state.select(if filtered.is_empty() {
                                                            None
                                                        } else {
                                                            Some(0)
                                                        });
                                                    }
                                                }
                                                'Y' => {
                                                    // Copy the selected host's connection string
                                                    // (`user@host`) to the system clipboard.
                                                    let target: Option<(String, String)> = {
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
                                                        rows.get(selected).and_then(|r| match r {
                                                            Row::Host(h) => Some((
                                                                h.name.clone(),
                                                                format!(
                                                                    "{}@{}",
                                                                    h.username, h.host
                                                                ),
                                                            )),
                                                            _ => None,
                                                        })
                                                    };
                                                    if let Some((hname, conn)) = target {
                                                        toast = Some(
                                                            match crate::os::copy_to_clipboard(
                                                                &conn,
                                                            ) {
                                                                Ok(()) => Toast::success(format!(
                                                                    "Copied {} ({})",
                                                                    conn, hname
                                                                )),
                                                                Err(e) => Toast::error(format!(
                                                                    "Clipboard: {e}"
                                                                )),
                                                            },
                                                        );
                                                    }
                                                }
                                                ' ' => {
                                                    // Toggle selection of the host on the current row.
                                                    let target: Option<String> = {
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
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
                                                        toast = Some(Toast::success(
                                                            t!("toast.selection_cleared", "n" => n),
                                                        ));
                                                    }
                                                }
                                                'D' => {
                                                    if selection.is_empty() {
                                                        toast = Some(Toast::error(t!(
                                                            "toast.nothing_selected"
                                                        )));
                                                    } else {
                                                        let names: Vec<String> =
                                                            selection.iter().cloned().collect();
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        println!();
                                                        let confirmed =
                                                            inquire::Confirm::new(&format!(
                                                    "Delete {} host(s)? This cannot be undone.",
                                                    names.len()
                                                ))
                                                            .with_default(false)
                                                            .prompt()
                                                            .unwrap_or(false);
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
                                                        let _ = terminal.clear();
                                                        if confirmed {
                                                            filtered.clear();
                                                            items.clear();
                                                            let ok =
                                                                reload_or_toast(db, &mut toast);
                                                            if ok {
                                                                for n in &names {
                                                                    db.hosts.remove(n);
                                                                }
                                                                save_db(db);
                                                                selection.clear();
                                                                toast = Some(Toast::success(
                                                                    t!("toast.deleted_n_hosts", "n" => names.len()),
                                                                ));
                                                            }
                                                            items = db.hosts.values().collect();
                                                            health_targets_stale = true;
                                                            sort_items(&mut items, sort_mode);
                                                            filtered =
                                                                apply_filter(&filter, &items);
                                                            selected = 0;
                                                            list_state.select(
                                                                if filtered.is_empty() {
                                                                    None
                                                                } else {
                                                                    Some(0)
                                                                },
                                                            );
                                                        }
                                                    }
                                                }
                                                'T' => {
                                                    if selection.is_empty() {
                                                        toast = Some(Toast::error(t!(
                                                            "toast.nothing_selected"
                                                        )));
                                                    } else {
                                                        let names: Vec<String> =
                                                            selection.iter().cloned().collect();
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        println!();
                                                        let entry = inquire::Text::new(
                                                    &format!("Add tags to {} host(s) (comma-separated):", names.len())
                                                ).prompt().ok();
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
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
                                                                let ok =
                                                                    reload_or_toast(db, &mut toast);
                                                                if ok {
                                                                    for name in &names {
                                                                        if let Some(h) =
                                                                            db.hosts.get_mut(name)
                                                                        {
                                                                            let mut existing = h
                                                                                .tags
                                                                                .clone()
                                                                                .unwrap_or_default(
                                                                                );
                                                                            for t in &new_tags {
                                                                                if !existing
                                                                                    .iter()
                                                                                    .any(|e| e == t)
                                                                                {
                                                                                    existing.push(
                                                                                        t.clone(),
                                                                                    );
                                                                                }
                                                                            }
                                                                            h.tags = if existing
                                                                                .is_empty()
                                                                            {
                                                                                None
                                                                            } else {
                                                                                Some(existing)
                                                                            };
                                                                        }
                                                                    }
                                                                    save_db(db);
                                                                }
                                                                items = db.hosts.values().collect();
                                                                health_targets_stale = true;
                                                                sort_items(&mut items, sort_mode);
                                                                filtered =
                                                                    apply_filter(&filter, &items);
                                                                if ok {
                                                                    toast = Some(Toast::success(
                                                                        t!(
                                                                            "toast.tagged_n_hosts",
                                                                            "n" => names.len(),
                                                                            "tags" => new_tags.join(",")
                                                                        ),
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                'X' => {
                                                    // Fan-out: run one command across every bulk-selected host.
                                                    if selection.is_empty() {
                                                        toast = Some(Toast::error(t!(
                                                            "toast.nothing_selected"
                                                        )));
                                                    } else {
                                                        let mut names: Vec<String> =
                                                            selection.iter().cloned().collect();
                                                        names.sort();
                                                        let _ = disable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            LeaveAlternateScreen
                                                        );
                                                        let result =
                                                            fanout::run_fanout(&db.hosts, &names);
                                                        let _ = enable_raw_mode();
                                                        let _ = execute!(
                                                            stdout(),
                                                            EnterAlternateScreen
                                                        );
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
                                                        let rows = rows_for(
                                                            view_mode, db, &items, &filtered,
                                                            &filter, &collapsed,
                                                        );
                                                        rows.iter()
                                                            .filter_map(|r| {
                                                                if let Row::Host(h) = r {
                                                                    Some(h.name.clone())
                                                                } else {
                                                                    None
                                                                }
                                                            })
                                                            .nth(n)
                                                    };
                                                    if let Some(name) = host_name {
                                                        let host_clone =
                                                            db.hosts.get(&name).cloned();
                                                        if let Some(host_clone) = host_clone {
                                                            if app_config.pause_health_on_session {
                                                                health_enabled.store(
                                                                    false,
                                                                    Ordering::Relaxed,
                                                                );
                                                                kluster_enabled.store(
                                                                    false,
                                                                    Ordering::Relaxed,
                                                                );
                                                            }
                                                            let launched = run_ssh_session(
                                                                &host_clone,
                                                                &db.hosts,
                                                                tunnels,
                                                            );
                                                            filtered.clear();
                                                            items.clear();
                                                            match launched {
                                                                Err(e) => {
                                                                    toast = Some(Toast::error(e));
                                                                    let _ = terminal.clear();
                                                                }
                                                                Ok(())
                                                                    if reload_or_toast(
                                                                        db, &mut toast,
                                                                    ) =>
                                                                {
                                                                    if let Some(h) = db
                                                                        .hosts
                                                                        .get_mut(&host_clone.name)
                                                                    {
                                                                        record_connection(h);
                                                                    }
                                                                    save_db(db);
                                                                    return;
                                                                }
                                                                Ok(()) => {}
                                                            }
                                                            // Not leaving: resume the workers
                                                            // and rebuild the dropped lists.
                                                            kluster_enabled.store(
                                                                kluster_started,
                                                                Ordering::Relaxed,
                                                            );
                                                            items = db.hosts.values().collect();
                                                            health_targets_stale = true;
                                                            sort_items(&mut items, sort_mode);
                                                            filtered =
                                                                apply_filter(&filter, &items);
                                                        }
                                                    } else {
                                                        toast = Some(Toast::error(
                                                            t!("toast.quick_connect_oob", "n" => n + 1),
                                                        ));
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
                            tab_events::handle_kluster_tab(
                                k.code,
                                &mut kluster_state,
                                &mut tab_events::KlusterCtx {
                                    targets: &kluster_targets,
                                    poke: &kluster_poke,
                                    detail: &mut kluster_detail,
                                    detail_scroll: &mut kluster_detail_scroll,
                                },
                                db,
                                &app_config,
                                &mut terminal,
                                &mut toast,
                            );
                        }

                        ActiveTab::Identities => {
                            tab_events::handle_identities_tab(
                                k.code,
                                &mut identities_state,
                                db,
                                &mut terminal,
                                &mut toast,
                            );
                        }

                        ActiveTab::Settings => {
                            tab_events::handle_settings_tab(
                                k.code,
                                &mut settings_state,
                                &mut app_config,
                                db,
                                &tab_events::LiveKnobs {
                                    sync_cfg: &sync_cfg,
                                    health_interval_secs: &health_interval_secs,
                                    health_probe_ms: &health_probe_ms,
                                    kluster_interval_secs: &kluster_interval_secs,
                                },
                                &mut toast,
                            );
                        }

                        ActiveTab::Theme => {
                            tab_events::handle_theme_tab(k.code, &mut theme_state, &mut toast);
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
        .title(Span::styled(
            "Rename folder",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
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
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(inner);

    let name_selected = state.selected_field == 0;
    let name_span = if name_selected {
        Span::styled(
            format!("[{}]", state.name),
            Style::default()
                .bg(accent)
                .fg(bg)
                .add_modifier(Modifier::BOLD),
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
        Span::styled("[ Esc = Cancel ]", Style::default().fg(theme.muted)),
    ]));
    f.render_widget(actions, chunks[1]);

    let error_text = if let Some(err) = &state.error {
        err.as_str()
    } else {
        "Tab/Shift+Tab or ↑/↓ to move • Type to edit • Enter to save"
    };

    let error_para =
        Paragraph::new(error_text).style(Style::default().fg(if state.error.is_some() {
            theme.error
        } else {
            theme.muted
        }));
    f.render_widget(error_para, chunks[2]);
}

fn apply_folder_form(db: &mut Database, state: &mut FolderFormState) -> Result<(), String> {
    db.reload_if_changed()
        .map_err(|e| format!("Not saved — could not re-read hosts: {e:#}"))?;
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
