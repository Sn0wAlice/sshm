//! Key handling for the tabs whose state is self-contained.
//!
//! Split out of `run_tui`, which had every tab's event arm inline in one
//! ~1760-line function. These four own a small, explicit slice of the loop's
//! state, so they extract cleanly and can be exercised without a terminal.
//! The Hosts arm does not — it touches most of the loop — and stays put until
//! that state is bundled.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::io::stdout;

use crossterm::event::KeyCode;
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// The terminal sshm draws on. The Kluster and Identities flows suspend it to
/// hand the TTY to a child (a shell, `ssh-keygen`), so they need to clear and
/// redraw on the way back.
pub type Term = Terminal<CrosstermBackend<std::io::Stdout>>;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

use crate::t;

use crate::config::settings::{save_settings, AppConfig};
use crate::models::Database;
use crate::tui::app::key_flows::{run_generate_key_flow, run_known_hosts_clean_flow};
use crate::tui::app::kluster_actions::{
    build_kluster_detail, handle_kluster_lifecycle, handle_kluster_open_logs,
    handle_kluster_open_shell, kluster_add_cluster_flow, kluster_add_docker_remote_flow,
    kluster_delete_cluster_flow, kluster_delete_docker_remote_flow, kluster_delete_pod_flow,
    kluster_edit_cluster_flow, sync_kluster_targets,
};
use crate::tui::app::kluster_worker::KlusterTargets;
use crate::tui::ssh::toast::Toast;
use crate::tui::tabs::identities_tab::{
    handle_identities_event, IdentitiesAction, IdentitiesTabState,
};
use crate::tui::tabs::kluster_tab::{handle_kluster_event, KlusterAction, KlusterTabState};
use crate::tui::tabs::settings_tab::{self, SettingsAction, SettingsFormState};
use crate::tui::tabs::theme_tab::{self, ThemeAction, ThemeTabState};
use crate::tui::theme;

/// Settings tab. Esc discards the form back to the saved config; everything
/// else goes to the tab's own handler, and a Save writes `settings.toml`.
/// The live handles the Settings tab writes through on Save: the background
/// workers read these rather than the config, so a changed interval takes
/// effect without a restart.
pub struct LiveKnobs<'a> {
    pub sync_cfg: &'a Arc<Mutex<crate::config::settings::SyncConfig>>,
    pub health_interval_secs: &'a Arc<AtomicU64>,
    pub health_probe_ms: &'a Arc<AtomicU64>,
    pub kluster_interval_secs: &'a Arc<AtomicU64>,
}

pub fn handle_settings_tab(
    k: KeyCode,
    settings_state: &mut SettingsFormState,
    app_config: &mut AppConfig,
    db: &Database,
    knobs: &LiveKnobs<'_>,
    toast: &mut Option<Toast>,
) {
    match k {
        KeyCode::Esc => {
            *settings_state = SettingsFormState::from_config(app_config);
        }
        _ => {
            match settings_tab::handle_settings_event(k, settings_state) {
                SettingsAction::Save => {
                    match settings_state.default_port.trim().parse::<u16>() {
                        Ok(port) => {
                            app_config.default_port = port;
                            app_config.default_username =
                                settings_state.default_username.trim().to_string();
                            app_config.default_identity_file =
                                settings_state.default_identity_file.trim().to_string();
                            app_config.export_path = settings_state.export_path.trim().to_string();
                            app_config.auto_health_check = settings_state.auto_health_check;
                            app_config.pause_health_on_session =
                                settings_state.pause_health_on_session;
                            app_config.notifications_enabled = settings_state.notifications_enabled;
                            crate::os::set_notifications_enabled(app_config.notifications_enabled);
                            if let Ok(v) = settings_state.health_ttl_secs.trim().parse::<u64>() {
                                app_config.health_ttl_secs = v.max(1);
                            }
                            if let Ok(v) =
                                settings_state.health_probe_timeout_ms.trim().parse::<u64>()
                            {
                                app_config.health_probe_timeout_ms = v.max(100);
                            }
                            if let Ok(v) = settings_state.kluster_refresh_secs.trim().parse::<u64>()
                            {
                                app_config.kluster_refresh_secs = v.max(2);
                            }
                            if let Ok(v) =
                                settings_state.kluster_log_tail_lines.trim().parse::<u32>()
                            {
                                app_config.kluster_log_tail_lines = v.max(1);
                            }
                            // Config sync. An interval of 0 means "manual
                            // only"; anything else is floored by the engine.
                            app_config.sync.enabled = settings_state.sync_enabled;
                            app_config.sync.repo_url =
                                settings_state.sync_repo_url.trim().to_string();
                            app_config.sync.ssh_key =
                                settings_state.sync_ssh_key.trim().to_string();
                            app_config.sync.branch = settings_state.sync_branch.trim().to_string();
                            app_config.sync.on_start = settings_state.sync_on_start;
                            app_config.sync.on_exit = settings_state.sync_on_exit;
                            app_config.sync.encrypt = settings_state.sync_encrypt;
                            app_config.sync.age_identity =
                                settings_state.sync_age_identity.trim().to_string();
                            let minutes = settings_state
                                .sync_interval_min
                                .trim()
                                .parse::<u64>()
                                .unwrap_or(0);
                            if minutes == 0 {
                                app_config.sync.mode = crate::config::settings::SyncMode::Manual;
                            } else {
                                app_config.sync.mode = crate::config::settings::SyncMode::Interval;
                                app_config.sync.interval_secs = minutes * 60;
                            }
                            if let Ok(mut shared) = knobs.sync_cfg.lock() {
                                *shared = app_config.sync.clone();
                            }

                            // Push live values to the background workers.
                            knobs
                                .health_interval_secs
                                .store(app_config.health_ttl_secs, Ordering::Relaxed);
                            knobs
                                .health_probe_ms
                                .store(app_config.health_probe_timeout_ms, Ordering::Relaxed);
                            knobs
                                .kluster_interval_secs
                                .store(app_config.kluster_refresh_secs, Ordering::Relaxed);
                            save_settings(app_config);
                            settings_state.dirty = false;
                            // Auto-export if export_path is set
                            if !app_config.export_path.is_empty() {
                                if let Err(e) = crate::config::export::export_ssh_config(
                                    db,
                                    &app_config.export_path,
                                ) {
                                    *toast =
                                        Some(Toast::error(t!("toast.export_failed", "error" => e)));
                                } else {
                                    *toast =
                                        Some(Toast::success(t!("toast.settings_saved_exported")));
                                }
                            } else {
                                *toast = Some(Toast::success(t!("toast.settings_saved")));
                            }
                        }
                        Err(_) => {
                            *toast = Some(Toast::error(t!("toast.invalid_port")));
                        }
                    }
                }
                SettingsAction::None => {}
            }
        }
    }
}

/// Theme tab. Esc reloads the theme from disk; a preset or a custom save
/// writes `theme.toml` and reports through a toast.
pub fn handle_theme_tab(k: KeyCode, theme_state: &mut ThemeTabState, toast: &mut Option<Toast>) {
    match k {
        KeyCode::Esc => {
            *theme_state = ThemeTabState::new();
        }
        _ => {
            match theme_tab::handle_theme_event(k, theme_state) {
                ThemeAction::ApplyPreset(idx) => {
                    let preset = &theme::PRESETS[idx];
                    // A preset defines a solid background, so it
                    // clears any transparency override.
                    theme::save_theme(
                        preset.bg,
                        preset.fg,
                        preset.accent,
                        preset.muted,
                        preset.error,
                        preset.success,
                        false,
                    );
                    theme_state.custom_bg = preset.bg.to_string();
                    theme_state.custom_fg = preset.fg.to_string();
                    theme_state.custom_accent = preset.accent.to_string();
                    theme_state.custom_muted = preset.muted.to_string();
                    theme_state.custom_error = preset.error.to_string();
                    theme_state.custom_success = preset.success.to_string();
                    theme_state.transparent_bg = false;
                    *toast = Some(Toast::success(format!("Theme: {}", preset.name)));
                }
                ThemeAction::SaveCustom => {
                    let valid = [
                        &theme_state.custom_bg,
                        &theme_state.custom_fg,
                        &theme_state.custom_accent,
                        &theme_state.custom_muted,
                        &theme_state.custom_error,
                        &theme_state.custom_success,
                    ]
                    .iter()
                    .all(|h| theme::hex_to_color(h).is_some());
                    if valid {
                        theme::save_theme(
                            &theme_state.custom_bg,
                            &theme_state.custom_fg,
                            &theme_state.custom_accent,
                            &theme_state.custom_muted,
                            &theme_state.custom_error,
                            &theme_state.custom_success,
                            theme_state.transparent_bg,
                        );
                        *toast = Some(Toast::success("Custom theme saved!"));
                    } else {
                        *toast = Some(Toast::error("Invalid hex color(s)"));
                    }
                }
                ThemeAction::None => {}
            }
        }
    }
}

/// Identities tab. Esc leaves filter mode; everything else is key generation,
/// pushing a public key, ssh-agent add/remove and known-hosts cleanup.
pub fn handle_identities_tab(
    k: KeyCode,
    identities_state: &mut IdentitiesTabState,
    db: &Database,
    terminal: &mut Term,
    toast: &mut Option<Toast>,
) {
    match handle_identities_event(k, identities_state) {
        IdentitiesAction::None => {}
        IdentitiesAction::Refresh => {
            identities_state.refresh();
            *toast = Some(Toast::success(t!("toast.keys_refreshed")));
        }
        IdentitiesAction::Generate => {
            let _ = disable_raw_mode();
            let _ = execute!(stdout(), LeaveAlternateScreen);
            match run_generate_key_flow() {
                Ok(Some(path)) => {
                    identities_state.refresh();
                    *toast = Some(Toast::success(t!(
                        "toast.generated_key",
                        "path" => path.display()
                    )));
                }
                Ok(None) => {}
                Err(e) => {
                    *toast = Some(Toast::error(t!(
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
                    &["--pub".to_string(), pub_path.display().to_string()],
                );
                let _ = enable_raw_mode();
                let _ = execute!(stdout(), EnterAlternateScreen);
                let _ = terminal.clear();
            } else {
                *toast = Some(Toast::error(t!("toast.no_key_selected")));
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
                        *toast = Some(Toast::success(t!("toast.agent_added")));
                    }
                    Err(e) => {
                        *toast = Some(Toast::error(t!(
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
                        *toast = Some(Toast::success(t!("toast.agent_removed")));
                    }
                    Err(e) => {
                        *toast = Some(Toast::error(t!(
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
                    *toast = Some(Toast::success(t!(
                        "toast.known_hosts_removed",
                        "host" => host
                    )));
                }
                Ok(None) => {}
                Err(e) => {
                    *toast = Some(Toast::error(t!(
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

/// Kluster tab. Delegates the key to the tab's own state machine, then acts
/// on the [`KlusterAction`] it returns — shells, logs, lifecycle, CRUD.
/// The Kluster tab's own slice of the loop state: what the discovery worker
/// polls, how to wake it, and the detail overlay the `i` key opens.
pub struct KlusterCtx<'a> {
    pub targets: &'a KlusterTargets,
    pub poke: &'a Arc<AtomicBool>,
    pub detail: &'a mut Option<crate::kluster::ContainerDetail>,
    pub detail_scroll: &'a mut usize,
}

pub fn handle_kluster_tab(
    k: KeyCode,
    kluster_state: &mut KlusterTabState,
    ctx: &mut KlusterCtx<'_>,
    // Read-only: the Kluster tab never edits the host DB, it only resolves
    // Docker remotes against it.
    db: &Database,
    app_config: &AppConfig,
    terminal: &mut Term,
    toast: &mut Option<Toast>,
) {
    match handle_kluster_event(k, kluster_state) {
        KlusterAction::None => {}
        KlusterAction::Refresh => {
            ctx.poke.store(true, Ordering::Relaxed);
        }
        KlusterAction::OpenShell => {
            handle_kluster_open_shell(kluster_state, terminal, toast);
        }
        KlusterAction::Lifecycle(act) => {
            handle_kluster_lifecycle(kluster_state, act, toast);
            ctx.poke.store(true, Ordering::Relaxed);
        }
        KlusterAction::OpenLogsFollow => {
            handle_kluster_open_logs(
                kluster_state,
                app_config.kluster_log_tail_lines,
                true,
                terminal,
                toast,
            );
        }
        KlusterAction::OpenDetail => {
            *ctx.detail = build_kluster_detail(kluster_state, toast);
            *ctx.detail_scroll = 0;
        }
        KlusterAction::AddCluster => {
            if let Err(e) = kluster_add_cluster_flow(kluster_state, terminal) {
                *toast = Some(Toast::error(format!("{e:#}")));
            } else {
                sync_kluster_targets(ctx.targets, kluster_state, &db.hosts);
                ctx.poke.store(true, Ordering::Relaxed);
            }
        }
        KlusterAction::EditCluster => {
            if let Err(e) = kluster_edit_cluster_flow(kluster_state, terminal) {
                *toast = Some(Toast::error(format!("{e:#}")));
            } else {
                sync_kluster_targets(ctx.targets, kluster_state, &db.hosts);
                ctx.poke.store(true, Ordering::Relaxed);
            }
        }
        KlusterAction::DeleteCluster => {
            if let Err(e) = kluster_delete_cluster_flow(kluster_state, terminal) {
                *toast = Some(Toast::error(format!("{e:#}")));
            } else {
                sync_kluster_targets(ctx.targets, kluster_state, &db.hosts);
            }
        }
        KlusterAction::DeletePod => match kluster_delete_pod_flow(kluster_state, terminal) {
            Ok(Some(name)) => {
                *toast = Some(Toast::success(format!("Deleted pod {}", name)));
                ctx.poke.store(true, Ordering::Relaxed);
            }
            Ok(None) => {}
            Err(e) => {
                *toast = Some(Toast::error(format!("{e:#}")));
            }
        },
        KlusterAction::AddDockerRemote => {
            match kluster_add_docker_remote_flow(kluster_state, db, terminal) {
                Ok(Some(alias)) => {
                    *toast = Some(Toast::success(format!("Added Docker remote: {}", alias)));
                    sync_kluster_targets(ctx.targets, kluster_state, &db.hosts);
                    ctx.poke.store(true, Ordering::Relaxed);
                }
                Ok(None) => {}
                Err(e) => {
                    *toast = Some(Toast::error(format!("{e:#}")));
                }
            }
        }
        KlusterAction::DeleteDockerRemote => {
            match kluster_delete_docker_remote_flow(kluster_state, terminal) {
                Ok(Some(alias)) => {
                    *toast = Some(Toast::success(format!("Removed Docker remote: {}", alias)));
                    sync_kluster_targets(ctx.targets, kluster_state, &db.hosts);
                }
                Ok(None) => {}
                Err(e) => {
                    *toast = Some(Toast::error(format!("{e:#}")));
                }
            }
        }
    }
}
