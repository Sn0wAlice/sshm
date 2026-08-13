//! Every Tauri command the webview can call. Each is a thin wrapper over
//! `sshm_core`, so `host.json` / `kluster.json` / `settings.toml` stay the
//! single source of truth shared with the TUI.

use std::collections::BTreeSet;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use tauri::{AppHandle, State};

use sshm_core::config::io::{save_db, load_db};
use sshm_core::config::path::config_path;
use sshm_core::config::export::export_ssh_config;
use sshm_core::config::settings::{load_settings, try_save_settings, AppConfig};
use sshm_core::filter::apply_filter;
use sshm_core::kluster::{
    self, Cluster, ContainerInfo, IncusInstance, LifecycleAction, PodInfo,
};
use sshm_core::models::{Database, Host, Tunnel};
use sshm_core::ssh::client::build_ssh_argv;
use sshm_core::ssh::{agent, keys, known_hosts};
use sshm_core::tunnels::{read_all_records, TunnelRecord};

use crate::dto::{HostKeyInfo, HostKeyStatus, HostPing, IdentityDto, KlusterOverview};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helpers (not commands)
// ---------------------------------------------------------------------------

/// Ensure `db.folders` lists every folder referenced by a host, sorted+unique.
fn sync_folders(db: &mut Database) {
    let mut set: BTreeSet<String> = db.folders.iter().filter(|f| !f.is_empty()).cloned().collect();
    for h in db.hosts.values() {
        if let Some(f) = &h.folder {
            if !f.is_empty() {
                set.insert(f.clone());
            }
        }
    }
    db.folders = set.into_iter().collect();
}

/// Persist `db`, rebase its source signature (so the watcher doesn't flag our
/// own write as external), and re-export `~/.ssh/config` when a path is set.
fn persist(db: &mut Database) {
    save_db(db);
    db.set_source(&config_path());
    let settings = load_settings();
    if !settings.export_path.trim().is_empty() {
        let _ = export_ssh_config(db, &settings.export_path);
    }
}

fn resolve_docker_host(db: &Database, alias: Option<&str>) -> Option<String> {
    let alias = alias?;
    db.hosts.get(alias).map(kluster::docker::host_to_docker_uri)
}

// ---------------------------------------------------------------------------
// Hosts & folders
// ---------------------------------------------------------------------------

/// List hosts (optionally fuzzy/prefix-filtered by core's matcher), sorted by
/// name. Reloads from disk first so TUI edits show up live.
#[tauri::command]
#[specta::specta]
pub fn list_hosts(state: State<AppState>, filter: Option<String>) -> Vec<Host> {
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    let mut refs: Vec<&Host> = db.hosts.values().collect();
    refs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    match filter {
        Some(f) if !f.trim().is_empty() => apply_filter(&f, &refs).into_iter().cloned().collect(),
        _ => refs.into_iter().cloned().collect(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn list_folders(state: State<AppState>) -> Vec<String> {
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    let mut set: BTreeSet<String> = db.folders.iter().filter(|f| !f.is_empty()).cloned().collect();
    for h in db.hosts.values() {
        if let Some(f) = &h.folder {
            if !f.is_empty() {
                set.insert(f.clone());
            }
        }
    }
    set.into_iter().collect()
}

#[tauri::command]
#[specta::specta]
pub fn get_host(state: State<AppState>, name: String) -> Option<Host> {
    let db = state.db.lock().unwrap();
    db.hosts.get(&name).cloned()
}

/// Create or update a host. When `original_name` differs from `host.name` the
/// old key is removed (a rename). Writes `host.json` and re-exports.
#[tauri::command]
#[specta::specta]
pub fn save_host(
    state: State<AppState>,
    host: Host,
    original_name: Option<String>,
) -> Result<(), String> {
    if host.name.trim().is_empty() {
        return Err("host name cannot be empty".into());
    }
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    if let Some(orig) = original_name {
        if orig != host.name {
            db.hosts.remove(&orig);
        }
    }
    db.hosts.insert(host.name.clone(), host);
    sync_folders(&mut db);
    persist(&mut db);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn delete_host(state: State<AppState>, name: String) -> Result<(), String> {
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    if db.hosts.remove(&name).is_none() {
        return Err(format!("host '{name}' not found"));
    }
    sync_folders(&mut db);
    persist(&mut db);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn clone_host(state: State<AppState>, name: String, new_name: String) -> Result<Host, String> {
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    if db.hosts.contains_key(&new_name) {
        return Err(format!("a host named '{new_name}' already exists"));
    }
    let mut clone = db
        .hosts
        .get(&name)
        .cloned()
        .ok_or_else(|| format!("host '{name}' not found"))?;
    clone.name = new_name.clone();
    clone.last_connected_at = None;
    clone.use_count = 0;
    db.hosts.insert(new_name, clone.clone());
    persist(&mut db);
    Ok(clone)
}

#[tauri::command]
#[specta::specta]
pub fn create_folder(state: State<AppState>, name: String) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("folder name cannot be empty".into());
    }
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    if !db.folders.contains(&name) {
        db.folders.push(name);
    }
    sync_folders(&mut db);
    persist(&mut db);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn rename_folder(
    state: State<AppState>,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    if new_name.trim().is_empty() {
        return Err("folder name cannot be empty".into());
    }
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    for h in db.hosts.values_mut() {
        if h.folder.as_deref() == Some(old_name.as_str()) {
            h.folder = Some(new_name.clone());
        }
    }
    db.folders.retain(|f| f != &old_name);
    sync_folders(&mut db);
    persist(&mut db);
    Ok(())
}

/// Delete a folder. When `delete_hosts` is true its hosts go too; otherwise
/// they're moved to the top level (folder cleared).
#[tauri::command]
#[specta::specta]
pub fn delete_folder(
    state: State<AppState>,
    name: String,
    delete_hosts: bool,
) -> Result<(), String> {
    let mut db = state.db.lock().unwrap();
    let _ = db.reload_if_changed();
    if delete_hosts {
        db.hosts.retain(|_, h| h.folder.as_deref() != Some(name.as_str()));
    } else {
        for h in db.hosts.values_mut() {
            if h.folder.as_deref() == Some(name.as_str()) {
                h.folder = None;
            }
        }
    }
    db.folders.retain(|f| f != &name);
    sync_folders(&mut db);
    persist(&mut db);
    Ok(())
}

// ---------------------------------------------------------------------------
// Connect (external terminal)
// ---------------------------------------------------------------------------

/// Open an ssh session to `name` in a new external terminal window, honoring
/// `external_terminal` from settings. Never embeds ssh in the webview.
#[tauri::command]
#[specta::specta]
pub fn connect_host(state: State<AppState>, name: String) -> Result<(), String> {
    let (argv, term) = {
        let db = state.db.lock().unwrap();
        let host = db
            .hosts
            .get(&name)
            .ok_or_else(|| format!("host '{name}' not found"))?;
        let argv = build_ssh_argv(host, &db.hosts);
        (argv, load_settings().external_terminal)
    };
    sshm_core::os::open_in_terminal(&argv, &term)
}

/// Probe every host's reachability (concurrent TCP connect to `host:port`).
/// Hosts behind a ProxyJump are skipped (reported as `None`) since their
/// address usually isn't directly reachable.
#[tauri::command]
#[specta::specta]
pub fn ping_hosts(state: State<AppState>) -> Vec<HostPing> {
    let targets: Vec<(String, String, u16, bool)> = {
        let db = state.db.lock().unwrap();
        db.hosts
            .values()
            .map(|h| {
                let via_proxy = h.proxy_jump.as_deref().is_some_and(|s| !s.trim().is_empty());
                (h.name.clone(), h.host.clone(), h.port, via_proxy)
            })
            .collect()
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let n = targets.len();
    for (name, host, port, via_proxy) in targets {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let latency_ms = if via_proxy {
                None
            } else {
                tcp_ping(&host, port, Duration::from_millis(1200))
            };
            let _ = tx.send(HostPing { name, latency_ms });
        });
    }
    drop(tx);

    let mut out = Vec::with_capacity(n);
    while let Ok(p) = rx.recv() {
        out.push(p);
    }
    out
}

fn tcp_ping(host: &str, port: u16, timeout: Duration) -> Option<u32> {
    let addr = format!("{host}:{port}");
    let sa = addr.to_socket_addrs().ok()?.next()?;
    let start = Instant::now();
    TcpStream::connect_timeout(&sa, timeout).ok()?;
    Some(start.elapsed().as_millis() as u32)
}

// ---------------------------------------------------------------------------
// Host keys (known_hosts / trust-on-first-use)
// ---------------------------------------------------------------------------

/// Resolve a saved host's `(hostname, port)` for a `known_hosts` lookup.
fn host_endpoint(state: &AppState, name: &str) -> Result<(String, u16), String> {
    let db = state.db.lock().unwrap();
    db.hosts
        .get(name)
        .map(|h| (h.host.clone(), h.port))
        .ok_or_else(|| format!("host '{name}' not found"))
}

/// Inspect a host's SSH key: the fingerprint pinned in `~/.ssh/known_hosts`
/// alongside the one the server presents right now (`ssh-keyscan`), plus a
/// verdict (unpinned / match / changed / unreachable). Mirrors the TUI's `F`
/// inspector. The live scan is a network round-trip (~5s worst case).
#[tauri::command]
#[specta::specta]
pub fn host_key_info(state: State<AppState>, name: String) -> Result<HostKeyInfo, String> {
    let (host, port) = host_endpoint(&state, &name)?;
    let pinned = known_hosts::pinned_fingerprints(&host, port);
    let live = known_hosts::scan_fingerprints(&host, port);

    // Compared on fingerprint alone, ignoring which algorithm matched — same
    // rule the TUI verdict uses.
    let matches = !pinned.is_empty()
        && live.iter().any(|l| pinned.iter().any(|p| p.fingerprint == l.fingerprint));
    let status = match (pinned.is_empty(), live.is_empty()) {
        (true, false) => HostKeyStatus::Unpinned,
        (false, true) => HostKeyStatus::Unreachable,
        _ if matches => HostKeyStatus::Match,
        (false, false) => HostKeyStatus::Changed,
        (true, true) => HostKeyStatus::Unknown,
    };

    Ok(HostKeyInfo {
        host,
        port,
        pinned: pinned.into_iter().map(Into::into).collect(),
        live: live.into_iter().map(Into::into).collect(),
        status,
    })
}

/// Trust-on-first-use: fetch the host's live key and pin it in `known_hosts`.
/// The caller is expected to have shown the fingerprint and gotten consent.
#[tauri::command]
#[specta::specta]
pub fn pin_host_key(state: State<AppState>, name: String) -> Result<(), String> {
    let (host, port) = host_endpoint(&state, &name)?;
    known_hosts::pin_host(&host, port).map_err(|e| e.to_string())
}

/// Forget a host's pinned key (`ssh-keygen -R`, including the `[host]:port`
/// form for non-default ports).
#[tauri::command]
#[specta::specta]
pub fn forget_host_key(state: State<AppState>, name: String) -> Result<(), String> {
    let (host, port) = host_endpoint(&state, &name)?;
    known_hosts::remove_known_host_port(&host, port).map_err(|e| e.to_string())
}

/// Recover from a changed host key: forget the stale entry, then pin the key the
/// server presents now. Fails (leaving nothing pinned) if the host is
/// unreachable, so the user can retry rather than end up trusting nothing.
#[tauri::command]
#[specta::specta]
pub fn replace_host_key(state: State<AppState>, name: String) -> Result<(), String> {
    let (host, port) = host_endpoint(&state, &name)?;
    known_hosts::remove_known_host_port(&host, port).map_err(|e| e.to_string())?;
    known_hosts::pin_host(&host, port).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_settings() -> AppConfig {
    load_settings()
}

#[tauri::command]
#[specta::specta]
pub fn save_settings(config: AppConfig) -> Result<(), String> {
    sshm_core::os::set_notifications_enabled(config.notifications_enabled);
    sshm_core::os::set_notification_icon(&config.notification_icon);
    try_save_settings(&config).map_err(|e| format!("{e:#}"))
}

// ---------------------------------------------------------------------------
// Identities (~/.ssh)
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn list_identities() -> Vec<IdentityDto> {
    keys::scan_ssh_dir().into_iter().map(IdentityDto::from).collect()
}

#[tauri::command]
#[specta::specta]
pub fn generate_identity(
    key_type: String,
    name: String,
    comment: String,
    passphrase: String,
) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("cannot locate home directory")?;
    let path = home.join(".ssh").join(&name);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    keys::generate_key(&key_type, &path, &comment, &passphrase).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn agent_add_identity(private_path: String) -> Result<(), String> {
    agent::agent_add(std::path::Path::new(&private_path)).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn agent_remove_identity(private_path: String) -> Result<(), String> {
    agent::agent_remove(std::path::Path::new(&private_path)).map_err(|e| e.to_string())
}

/// Push a public key to a host's `authorized_keys`. `pub_path` overrides the
/// key; otherwise the host's identity `.pub` (or a default) is used.
#[tauri::command]
#[specta::specta]
pub fn push_pubkey(
    state: State<AppState>,
    host_name: String,
    pub_path: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    let host = db
        .hosts
        .get(&host_name)
        .ok_or_else(|| format!("host '{host_name}' not found"))?;

    let pubkey = if let Some(p) = pub_path.filter(|s| !s.trim().is_empty()) {
        std::path::PathBuf::from(shellexpand_tilde(&p))
    } else if let Some(id) = &host.identity_file {
        keys::pub_from_identity(id)
            .or_else(keys::default_pubkey_path)
            .ok_or("no public key found for this host; pass one explicitly")?
    } else {
        keys::default_pubkey_path().ok_or("no default public key (~/.ssh/id_*.pub) found")?
    };

    keys::install_pubkey_on_host(host, &pubkey).map_err(|e| e.to_string())
}

fn shellexpand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    p.to_string()
}

// ---------------------------------------------------------------------------
// Kluster (docker / k8s / incus)
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn kluster_overview() -> KlusterOverview {
    let (db, _) = kluster::db::load_or_bootstrap();
    KlusterOverview {
        clusters: db.clusters,
        incus_remotes: db.incus_remotes,
        docker_remotes: db.docker_remotes,
        docker_local_available: kluster::docker::daemon_running(),
        incus_local_available: kluster::incus::local_available(),
        kube_available: kluster::kube::cli_available(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn kluster_docker_containers(
    state: State<AppState>,
    host_alias: Option<String>,
) -> Result<Vec<ContainerInfo>, String> {
    let uri = {
        let db = state.db.lock().unwrap();
        resolve_docker_host(&db, host_alias.as_deref())
    };
    kluster::docker::list_containers(uri.as_deref()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
#[specta::specta]
pub fn kluster_pods(cluster: Cluster) -> Result<Vec<PodInfo>, String> {
    kluster::kube::list_pods(&cluster).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
#[specta::specta]
pub fn kluster_incus(remote: Option<String>) -> Result<Vec<IncusInstance>, String> {
    kluster::incus::list_instances(remote.as_deref()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
#[specta::specta]
pub fn kluster_docker_lifecycle(
    state: State<AppState>,
    id: String,
    action: LifecycleAction,
    host_alias: Option<String>,
) -> Result<(), String> {
    let uri = {
        let db = state.db.lock().unwrap();
        resolve_docker_host(&db, host_alias.as_deref())
    };
    kluster::docker::lifecycle(&id, action, uri.as_deref()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
#[specta::specta]
pub fn kluster_incus_lifecycle(
    name: String,
    action: LifecycleAction,
    remote: Option<String>,
) -> Result<(), String> {
    kluster::incus::lifecycle(&name, remote.as_deref(), action).map_err(|e| format!("{e:#}"))
}

/// Open a shell into a Docker container as an embedded terminal session.
/// Returns the session id.
#[tauri::command]
#[specta::specta]
pub fn kluster_docker_shell(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    host_alias: Option<String>,
) -> Result<String, String> {
    let uri = {
        let db = state.db.lock().unwrap();
        resolve_docker_host(&db, host_alias.as_deref())
    };
    let argv = match uri {
        Some(u) => vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("DOCKER_HOST={u} docker exec -it {id} /bin/sh"),
        ],
        None => vec!["docker".into(), "exec".into(), "-it".into(), id, "/bin/sh".into()],
    };
    crate::terminal::spawn_session(&app, &state, argv)
}

/// Tail a Docker container's logs as an embedded terminal session.
#[tauri::command]
#[specta::specta]
pub fn kluster_docker_logs(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    host_alias: Option<String>,
) -> Result<String, String> {
    let uri = {
        let db = state.db.lock().unwrap();
        resolve_docker_host(&db, host_alias.as_deref())
    };
    let tail = load_settings().kluster_log_tail_lines;
    let argv = match uri {
        Some(u) => vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("DOCKER_HOST={u} docker logs -f --tail {tail} {id}"),
        ],
        None => vec![
            "docker".into(),
            "logs".into(),
            "-f".into(),
            "--tail".into(),
            tail.to_string(),
            id,
        ],
    };
    crate::terminal::spawn_session(&app, &state, argv)
}

/// Open a shell into a k8s pod as an embedded terminal session.
#[tauri::command]
#[specta::specta]
pub fn kluster_pod_shell(
    app: AppHandle,
    state: State<AppState>,
    cluster: Cluster,
    namespace: String,
    pod: String,
) -> Result<String, String> {
    let mut argv = vec!["kubectl".to_string()];
    if let Some(kc) = &cluster.kubeconfig {
        argv.push("--kubeconfig".into());
        argv.push(kc.clone());
    }
    if let Some(ctx) = &cluster.context {
        argv.push("--context".into());
        argv.push(ctx.clone());
    }
    argv.extend([
        "exec".into(),
        "-it".into(),
        "-n".into(),
        namespace,
        pod,
        "--".into(),
        "/bin/sh".into(),
    ]);
    crate::terminal::spawn_session(&app, &state, argv)
}

/// Open a shell into an Incus instance as an embedded terminal session.
#[tauri::command]
#[specta::specta]
pub fn kluster_incus_shell(
    app: AppHandle,
    state: State<AppState>,
    name: String,
    remote: Option<String>,
) -> Result<String, String> {
    let target = match remote {
        Some(r) if !r.is_empty() => format!("{r}:{name}"),
        _ => name,
    };
    let argv = vec!["incus".into(), "exec".into(), target, "--".into(), "/bin/sh".into()];
    crate::terminal::spawn_session(&app, &state, argv)
}

// ---------------------------------------------------------------------------
// Tunnels
// ---------------------------------------------------------------------------

/// Every background tunnel across all sshm instances (fed by the shared
/// `tunnels/<pid>.json` files), plus a reap of this app's dead children.
#[tauri::command]
#[specta::specta]
pub fn list_tunnels(state: State<AppState>) -> Vec<TunnelRecord> {
    state.tunnels.lock().unwrap().reap();
    read_all_records()
}

/// Saved (persisted) tunnels defined on a host.
#[tauri::command]
#[specta::specta]
pub fn host_tunnels(state: State<AppState>, name: String) -> Vec<Tunnel> {
    let db = state.db.lock().unwrap();
    db.hosts.get(&name).map(|h| h.tunnels.clone()).unwrap_or_default()
}

/// Start the host's saved tunnel at `index` as a background `ssh -N`.
#[tauri::command]
#[specta::specta]
pub fn start_tunnel(state: State<AppState>, host_name: String, index: usize) -> Result<(), String> {
    let (host, tunnel, hosts) = {
        let db = state.db.lock().unwrap();
        let host = db
            .hosts
            .get(&host_name)
            .ok_or_else(|| format!("host '{host_name}' not found"))?;
        let tunnel = host
            .tunnels
            .get(index)
            .cloned()
            .ok_or_else(|| format!("no saved tunnel #{index} on '{host_name}'"))?;
        (host.clone(), tunnel, db.hosts.clone())
    };
    state.tunnels.lock().unwrap().start(&host, &tunnel, &hosts)
}

/// Stop a background tunnel this app started, by pid.
#[tauri::command]
#[specta::specta]
pub fn stop_tunnel(state: State<AppState>, pid: u32) -> Result<(), String> {
    state.tunnels.lock().unwrap().stop(pid)
}

/// Force a reload of the DB from disk (e.g. after a live-sync event) and return
/// the fresh host list.
#[tauri::command]
#[specta::specta]
pub fn reload_db(state: State<AppState>) -> Vec<Host> {
    let mut db = state.db.lock().unwrap();
    // Replace with a fresh load unconditionally.
    *db = load_db();
    let mut refs: Vec<&Host> = db.hosts.values().collect();
    refs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    refs.into_iter().cloned().collect()
}
