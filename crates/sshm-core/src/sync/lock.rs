//! Cross-process lock so that only one sshm ever syncs at a time.
//!
//! Several sshm processes can be up at once — two TUIs, the desktop GUI, a
//! `sshm sync` typed by hand, a cron entry firing on the minute — and they all
//! share `~/.config/sshm`. A sync run rewrites those files *and* pushes to a
//! remote, so two concurrent runs would race each other's working tree and
//! produce pointless conflicting commits.
//!
//! The lock is a file created with `O_CREAT|O_EXCL` (`create_new`), which the
//! kernel makes atomic: exactly one process wins the create, everybody else
//! sees `AlreadyExists` and backs off. The winner writes down who it is, and
//! [`SyncLock`] removes the file on drop (including while unwinding a panic).
//!
//! A process killed with `SIGKILL` can't clean up after itself, so a lock is
//! also considered free when the recorded pid is gone (same machine only) or
//! when it is older than [`STALE_AFTER`].

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A lock whose holder went missing is taken over after this long. Comfortably
/// above a slow `git fetch`/`push` over a bad link, low enough that a crashed
/// process doesn't wedge auto-sync for a whole session.
pub const STALE_AFTER: Duration = Duration::from_secs(5 * 60);

/// Who is holding the lock, as recorded in the lock file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    /// Hostname of the locking machine — the pid liveness check is only
    /// meaningful when it matches ours (the config dir may live on a share).
    #[serde(default)]
    pub host: String,
    /// Unix seconds at acquisition.
    #[serde(default)]
    pub acquired_at: i64,
    /// What the holder is doing, for `sshm sync status`.
    #[serde(default)]
    pub what: String,
}

impl LockInfo {
    fn new(what: &str) -> Self {
        LockInfo {
            pid: std::process::id(),
            host: hostname(),
            acquired_at: chrono::Utc::now().timestamp(),
            what: what.to_string(),
        }
    }

    /// Age in seconds (0 for a clock that went backwards).
    pub fn age_secs(&self) -> i64 {
        (chrono::Utc::now().timestamp() - self.acquired_at).max(0)
    }

    /// True when this entry can be taken over: too old, or its process is
    /// provably gone on this machine.
    fn is_stale(&self) -> bool {
        if self.age_secs() as u64 >= STALE_AFTER.as_secs() {
            return true;
        }
        // Only trust pids we can actually check: same host, and not our own
        // (a recycled pid equal to ours means the file is ours already).
        if self.host == hostname() && self.pid != std::process::id() {
            return !pid_alive(self.pid);
        }
        false
    }
}

/// Held lock. Dropping it releases (deletes the file).
#[derive(Debug)]
pub struct SyncLock {
    path: PathBuf,
    /// Set once released, so `Drop` doesn't delete a lock somebody else
    /// has since acquired.
    released: bool,
}

impl SyncLock {
    /// Try once to take the sync lock. `Ok(None)` means it is legitimately
    /// held by another live process — that's not an error, the caller simply
    /// skips this run.
    pub fn try_acquire(what: &str) -> Result<Option<SyncLock>> {
        Self::try_acquire_at(&crate::config::path::sync_lock_path(), what)
    }

    /// Same, against an explicit path. Exposed for tests.
    pub fn try_acquire_at(path: &Path, what: &str) -> Result<Option<SyncLock>> {
        // Two attempts: the second one only happens after we removed a stale
        // file, and losing that race just means somebody else got in first.
        for attempt in 0..2 {
            match create_exclusive(path, &LockInfo::new(what)) {
                Ok(()) => {
                    return Ok(Some(SyncLock { path: path.to_path_buf(), released: false }))
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt == 1 {
                        return Ok(None);
                    }
                    match read_info(path) {
                        // Unreadable: a crash leftover once it has stopped
                        // being young enough to be a write in flight.
                        None => {
                            if !unreadable_lock_is_stale(path) {
                                return Ok(None);
                            }
                            let _ = fs::remove_file(path);
                        }
                        Some(info) if info.is_stale() => {
                            let _ = fs::remove_file(path);
                        }
                        Some(_) => return Ok(None),
                    }
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("creating lock {}", path.display()))
                }
            }
        }
        Ok(None)
    }

    /// Retry `try_acquire` until it succeeds or `timeout` elapses. Used by the
    /// interactive CLI, where waiting a few seconds beats telling the user to
    /// come back later.
    pub fn acquire_waiting(what: &str, timeout: Duration) -> Result<Option<SyncLock>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(lock) = Self::try_acquire(what)? {
                return Ok(Some(lock));
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// Who holds the lock right now, if anybody. Purely informational —
    /// a `None` here is no guarantee that the next acquire will win.
    pub fn holder() -> Option<LockInfo> {
        read_info(&crate::config::path::sync_lock_path())
    }

    /// Release explicitly (also done on drop).
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if !self.released {
            self.released = true;
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// Create the lock file, failing with `AlreadyExists` if one is already there.
///
/// The contents are written to a private temp file *first* and the lock is
/// then claimed by hard-linking that file into place: `link()` fails when the
/// target exists, and by the time anybody can see the lock it is already
/// complete. Creating an empty file and filling it afterwards would leave a
/// window in which another process reads a truncated lock and mistakes it for
/// a crashed leftover — which is exactly how two processes end up syncing at
/// once.
fn create_exclusive(path: &Path, info: &LockInfo) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    let tmp = dir.join(format!(
        ".sync.lock.tmp-{}-{}",
        std::process::id(),
        LOCK_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    {
        let mut f = OpenOptions::new().write(true).create(true).truncate(true).open(&tmp)?;
        let body = serde_json::to_vec_pretty(info).unwrap_or_else(|_| b"{}".to_vec());
        f.write_all(&body)?;
        f.sync_all()?;
    }

    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    linked
}

/// Makes temp lock names unique within a process (the pid alone isn't enough:
/// several threads may race for the same lock).
static LOCK_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A lock file we cannot parse is only reclaimed once it has sat there this
/// long — long enough that it can't be a write still in flight.
const CORRUPT_GRACE: Duration = Duration::from_secs(10);

/// True when an unparseable lock file is old enough to be a crash leftover
/// rather than something being written right now.
fn unreadable_lock_is_stale(path: &Path) -> bool {
    match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(modified) => modified
            .elapsed()
            .map(|age| age >= CORRUPT_GRACE)
            .unwrap_or(false),
        // No mtime to go on: leave it alone rather than risk stealing a live lock.
        Err(_) => false,
    }
}

fn read_info(path: &Path) -> Option<LockInfo> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// True when a process with this pid exists. Signal 0 performs the permission
/// and existence checks without delivering anything.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: `kill` with signal 0 sends nothing; it only reports whether the
    // pid exists (ESRCH) or we may not signal it (EPERM — so it does exist).
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Non-unix: no cheap liveness check, so only the age rule applies.
#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    true
}

/// This machine's hostname, or `"unknown"` when it can't be read.
#[cfg(unix)]
pub fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: writing at most `buf.len()` bytes into a buffer we own.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return "unknown".to_string();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).to_string()
}

#[cfg(not(unix))]
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_refused_while_the_first_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");

        let first = SyncLock::try_acquire_at(&path, "test").unwrap();
        assert!(first.is_some(), "first acquire must win");
        assert!(
            SyncLock::try_acquire_at(&path, "test").unwrap().is_none(),
            "a second acquire must be refused while the lock is held"
        );

        drop(first);
        assert!(
            SyncLock::try_acquire_at(&path, "test").unwrap().is_some(),
            "the lock must be free again once released"
        );
    }

    #[test]
    fn drop_removes_the_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        {
            let _held = SyncLock::try_acquire_at(&path, "test").unwrap().unwrap();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[test]
    fn a_dead_pid_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        // pid 0 is never a live process for our purposes.
        let ghost = LockInfo {
            pid: 0,
            host: hostname(),
            acquired_at: chrono::Utc::now().timestamp(),
            what: "ghost".into(),
        };
        create_exclusive(&path, &ghost).unwrap();

        let taken = SyncLock::try_acquire_at(&path, "test").unwrap();
        assert!(taken.is_some(), "a lock held by a dead pid must be taken over");
    }

    #[test]
    fn an_old_lock_from_another_machine_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        let old = LockInfo {
            pid: 1,
            host: "some-other-box".into(),
            acquired_at: chrono::Utc::now().timestamp() - (STALE_AFTER.as_secs() as i64) - 1,
            what: "old".into(),
        };
        create_exclusive(&path, &old).unwrap();
        assert!(SyncLock::try_acquire_at(&path, "test").unwrap().is_some());
    }

    #[test]
    fn a_fresh_lock_from_another_machine_is_respected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        let fresh = LockInfo {
            pid: 1,
            host: "some-other-box".into(),
            acquired_at: chrono::Utc::now().timestamp(),
            what: "busy".into(),
        };
        create_exclusive(&path, &fresh).unwrap();
        assert!(SyncLock::try_acquire_at(&path, "test").unwrap().is_none());
    }

    #[test]
    fn a_lock_file_being_written_right_now_is_left_alone() {
        // An unparseable lock that was just touched may be a write in flight;
        // stealing it is how two processes end up syncing at once.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        fs::write(&path, "{ half written").unwrap();
        assert!(SyncLock::try_acquire_at(&path, "test").unwrap().is_none());
    }

    /// Push a file's mtime `secs` into the past.
    #[cfg(unix)]
    fn backdate(path: &Path, secs: i64) {
        let when = chrono::Utc::now().timestamp() - secs;
        let tv = libc::timeval { tv_sec: when as libc::time_t, tv_usec: 0 };
        let times = [tv, tv];
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: both pointers are valid for the duration of the call.
        assert_eq!(unsafe { libc::utimes(c.as_ptr(), times.as_ptr()) }, 0);
    }

    #[test]
    #[cfg(unix)]
    fn an_old_corrupt_lock_file_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        fs::write(&path, "{ half written").unwrap();
        backdate(&path, CORRUPT_GRACE.as_secs() as i64 + 1);
        assert!(SyncLock::try_acquire_at(&path, "test").unwrap().is_some());
    }

    #[test]
    fn creating_a_lock_never_exposes_a_partial_file() {
        // The file is linked into place complete: any reader that can see it
        // can parse it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        let _held = SyncLock::try_acquire_at(&path, "test").unwrap().unwrap();
        let info = read_info(&path).expect("a readable lock file");
        assert_eq!(info.pid, std::process::id());
        assert_eq!(info.what, "test");
    }

    #[test]
    fn only_one_of_many_threads_gets_the_lock() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("sync.lock"));
        // Every winner parks its guard here, so nothing is ever released
        // mid-test — the count is decided by the lock, not by timing.
        let held: Arc<Mutex<Vec<SyncLock>>> = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();

        for _ in 0..16 {
            let path = Arc::clone(&path);
            let held = Arc::clone(&held);
            handles.push(std::thread::spawn(move || {
                if let Ok(Some(lock)) = SyncLock::try_acquire_at(&path, "race") {
                    held.lock().unwrap().push(lock);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(held.lock().unwrap().len(), 1, "exactly one thread may hold the lock");
    }
}
