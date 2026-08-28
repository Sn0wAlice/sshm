//! End-to-end test of the git config sync against a real repository.
//!
//! Two "machines" (two `$HOME`s pointed at the same bare repo) take turns
//! syncing, so the whole chain is exercised for real: repo bootstrap, fetch,
//! three-way merge, local write-back, commit and push.
//!
//! Everything lives in one test function on purpose — it drives `$HOME`, which
//! is process-wide state, so it must not run alongside another test.

use std::path::{Path, PathBuf};
use std::process::Command;

use sshm_core::config::settings::{SyncConfig, SyncItem};
use sshm_core::sync::{self, SyncRun};

/// Point the process (and therefore `config_dir()`) at a fresh home directory.
fn use_home(home: &Path) {
    std::fs::create_dir_all(home).unwrap();
    std::env::set_var("HOME", home);
    // Linux/XDG; ignored by `dirs` on macOS, which follows $HOME.
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
}

fn git(args: &[&str], cwd: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("running git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn host_json(entries: &[(&str, &str)]) -> String {
    let hosts: Vec<String> = entries
        .iter()
        .map(|(n, ip)| {
            format!(r#""{n}": {{"name":"{n}","host":"{ip}","port":22,"username":"root"}}"#)
        })
        .collect();
    format!(r#"{{"hosts": {{{}}}, "folders": []}}"#, hosts.join(","))
}

fn local_hosts_path() -> PathBuf {
    SyncItem::Hosts.local_path()
}

fn host_names() -> Vec<String> {
    let text = std::fs::read_to_string(local_hosts_path()).expect("a local host.json");
    let db = sshm_core::config::io::parse_db_text(&text).expect("valid host.json");
    let mut names: Vec<String> = db.hosts.keys().cloned().collect();
    names.sort();
    names
}

fn write_hosts(entries: &[(&str, &str)]) {
    std::fs::create_dir_all(local_hosts_path().parent().unwrap()).unwrap();
    std::fs::write(local_hosts_path(), host_json(entries)).unwrap();
}

fn cfg(remote: &Path) -> SyncConfig {
    SyncConfig {
        enabled: true,
        repo_url: remote.to_string_lossy().to_string(),
        ssh_key: String::new(),
        branch: "main".into(),
        items: vec![SyncItem::Hosts],
        ..SyncConfig::default()
    }
}

fn sync(cfg: &SyncConfig) -> sync::SyncReport {
    match sync::sync_now(cfg).expect("sync must not fail") {
        SyncRun::Done(report) => report,
        other => panic!("expected a completed sync, got: {}", other.summary()),
    }
}

#[test]
fn two_machines_converge_through_the_remote() {
    if !Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        eprintln!("git not available — skipping");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let remote = tmp.path().join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&["init", "--bare", "-q", "-b", "main"], &remote);

    let home_a = tmp.path().join("machine-a");
    let home_b = tmp.path().join("machine-b");
    let cfg = cfg(&remote);

    // --- Machine A publishes its hosts to an empty remote ---
    use_home(&home_a);
    write_hosts(&[("laptop", "10.0.0.1")]);
    let first = sync(&cfg);
    assert!(first.pushed, "the first sync must publish: {}", first.summary());

    // --- Machine B starts from nothing and adopts them ---
    use_home(&home_b);
    assert!(!local_hosts_path().exists(), "machine B starts empty");
    let adopted = sync(&cfg);
    assert_eq!(host_names(), vec!["laptop"], "machine B pulled the host");
    assert!(adopted.updated_locally.contains(&"host.json".to_string()));

    // --- B adds a host of its own ---
    write_hosts(&[("laptop", "10.0.0.1"), ("desktop", "10.0.0.2")]);
    assert!(sync(&cfg).pushed, "B publishes its addition");

    // --- Meanwhile A added a different one: both must survive ---
    use_home(&home_a);
    write_hosts(&[("laptop", "10.0.0.1"), ("server", "10.0.0.3")]);
    let merged = sync(&cfg);
    assert_eq!(
        host_names(),
        vec!["desktop", "laptop", "server"],
        "concurrent additions from both machines are merged, not overwritten"
    );
    assert!(merged.pushed);

    // --- B sees the merged set on its next sync ---
    use_home(&home_b);
    sync(&cfg);
    assert_eq!(host_names(), vec!["desktop", "laptop", "server"]);

    // --- A deletion propagates instead of coming back ---
    write_hosts(&[("desktop", "10.0.0.2"), ("server", "10.0.0.3")]);
    sync(&cfg);
    use_home(&home_a);
    sync(&cfg);
    assert_eq!(host_names(), vec!["desktop", "server"], "the deleted host stays deleted");

    // --- A quiet sync is a no-op, and the lock is always released ---
    let quiet = sync(&cfg);
    assert!(!quiet.pushed, "nothing changed, nothing to push: {}", quiet.summary());
    assert_eq!(quiet.summary(), "already up to date");
    assert!(
        !sshm_core::config::path::sync_lock_path().exists(),
        "the sync lock must not outlive the run"
    );

    // --- And the shared state records the run for every other instance ---
    let state = sync::SyncState::load();
    assert!(state.last_success_at.is_some());
    assert!(state.last_error.is_none());
}
