//! Optional encryption of the files a sync run publishes.
//!
//! Without it, `host.json` travels to the git remote in clear: hostnames, IPs,
//! usernames, ports, key paths, tags and free-text notes — a fair map of an
//! infrastructure. A private repository is still a repository (mirrors,
//! backups, org-wide access, a compromised account), so this wraps the payload
//! before it ever reaches a commit.
//!
//! **Where it sits.** Only at the git boundary. The three-way merge in
//! [`super::merge`] reconciles hosts and clusters *entry by entry*, which means
//! it has to see structure — so a run decrypts on the way in from the repo and
//! encrypts on the way out, and everything between stays plaintext. The local
//! `host.json` on your own disk is untouched and unencrypted, exactly as
//! before; this protects what leaves the machine, not what sits on it.
//!
//! **How.** Shelling out to [`age`](https://age-encryption.org), the same way
//! sync already shells out to `git` and `ssh`. One identity file, copied to
//! each machine the way an SSH key is: its public half is the recipient, its
//! private half decrypts. No passphrase — a sync runs in the background and
//! must never block on a prompt.
//!
//! **Reading is content-sniffed, writing is configured.** A blob is decrypted
//! when it carries age's armor header, whatever the settings say. That makes
//! turning encryption on a non-event: the next push encrypts, and the history
//! written before it still reads back. Turning it off is equally undramatic.
//!
//! **There is no silent fallback.** If encryption is on and `age` is missing,
//! the identity is unreadable, or the process fails, the run aborts. Pushing
//! cleartext to a remote the user asked to be encrypted would be the one
//! failure mode worth avoiding at any cost.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::config::settings::SyncConfig;

/// First line of an ASCII-armored age file. We always armor, so the repo holds
/// text rather than binary — a diff stays reviewable as "this blob changed",
/// and git does not treat the file as binary.
const ARMOR_HEADER: &str = "-----BEGIN AGE ENCRYPTED FILE-----";

/// True when `text` looks like an armored age blob.
pub fn is_encrypted(text: &str) -> bool {
    text.trim_start().starts_with(ARMOR_HEADER)
}

/// True when the `age` binary is on PATH.
pub fn age_available() -> bool {
    Command::new("age")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Absolute path of the configured identity file, `~` expanded.
pub fn identity_path(cfg: &SyncConfig) -> Option<PathBuf> {
    let raw = cfg.age_identity.trim();
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(shellexpand::tilde(raw).to_string()))
}

/// The recipient to encrypt to, read from the identity file.
///
/// `age-keygen` writes the public key as a `# public key: age1…` comment in
/// the identity it generates, so the common case needs no extra process. When
/// that comment is absent — a hand-written or converted identity — fall back to
/// asking `age-keygen -y`.
fn recipient_for(identity: &Path) -> Result<String> {
    let text = std::fs::read_to_string(identity)
        .with_context(|| format!("reading age identity {}", identity.display()))?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("# public key:") {
            let key = rest.trim();
            if !key.is_empty() {
                return Ok(key.to_string());
            }
        }
    }
    let out = Command::new("age-keygen")
        .arg("-y")
        .arg(identity)
        .output()
        .context("running `age-keygen -y` to derive the public key")?;
    if !out.status.success() {
        bail!(
            "could not derive a public key from {} — is it an age identity?",
            identity.display()
        );
    }
    let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if key.is_empty() {
        bail!("`age-keygen -y {}` produced no key", identity.display());
    }
    Ok(key)
}

/// Run `age` with `input` on stdin and return its stdout.
fn run_age(args: &[&str], input: &str, what: &str) -> Result<String> {
    let mut child = Command::new("age")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running `age` to {what}"))?;
    child
        .stdin
        .take()
        .context("age stdin")?
        .write_all(input.as_bytes())
        .with_context(|| format!("feeding `age` while trying to {what}"))?;
    let out = child
        .wait_with_output()
        .with_context(|| format!("waiting for `age` to {what}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("age failed to {what}: {err}");
    }
    String::from_utf8(out.stdout).with_context(|| format!("`age` output while trying to {what}"))
}

/// Fail early on a misconfiguration that would otherwise surface mid-run, when
/// half the work is done. Called from the sync preflight.
pub fn preflight(cfg: &SyncConfig) -> Result<()> {
    if !cfg.encrypt {
        return Ok(());
    }
    let Some(identity) = identity_path(cfg) else {
        bail!("sync encryption is on but no age identity is set — run `sshm sync setup`");
    };
    if !identity.exists() {
        bail!(
            "age identity {} does not exist — generate one with `age-keygen -o {}`",
            identity.display(),
            identity.display()
        );
    }
    if !age_available() {
        bail!("sync encryption is on but `age` was not found on PATH");
    }
    recipient_for(&identity)?;
    Ok(())
}

/// Plaintext → what gets committed. Identity when encryption is off.
pub fn seal(cfg: &SyncConfig, plaintext: &str) -> Result<String> {
    if !cfg.encrypt {
        return Ok(plaintext.to_string());
    }
    let identity =
        identity_path(cfg).context("sync encryption is on but no age identity is set")?;
    let recipient = recipient_for(&identity)?;
    run_age(
        &["--encrypt", "--armor", "--recipient", &recipient],
        plaintext,
        "encrypt the sync payload",
    )
}

/// What was committed → plaintext. A blob without age's armor header is
/// already plaintext and is returned untouched, so a repo written before
/// encryption was turned on still reads.
pub fn unseal(cfg: &SyncConfig, blob: &str) -> Result<String> {
    if !is_encrypted(blob) {
        return Ok(blob.to_string());
    }
    let Some(identity) = identity_path(cfg) else {
        bail!(
            "this sync repository is encrypted but no age identity is set — \
             run `sshm sync setup` and point it at the identity used elsewhere"
        );
    };
    if !identity.exists() {
        bail!(
            "this sync repository is encrypted but the age identity {} does not exist",
            identity.display()
        );
    }
    let identity = identity.to_string_lossy().to_string();
    run_age(
        &["--decrypt", "--identity", &identity],
        blob,
        "decrypt the sync payload",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(encrypt: bool, identity: &str) -> SyncConfig {
        SyncConfig {
            encrypt,
            age_identity: identity.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn armor_is_what_marks_a_blob_encrypted() {
        assert!(is_encrypted("-----BEGIN AGE ENCRYPTED FILE-----\nabc\n"));
        assert!(is_encrypted("\n  -----BEGIN AGE ENCRYPTED FILE-----\n"));
        assert!(!is_encrypted("{\"hosts\":{}}"));
        assert!(!is_encrypted(""));
        // A JSON document that merely mentions the header is not a blob.
        assert!(!is_encrypted(
            "{\"note\":\"-----BEGIN AGE ENCRYPTED FILE-----\"}"
        ));
    }

    #[test]
    fn sealing_is_a_no_op_when_encryption_is_off() {
        let c = cfg(false, "");
        assert_eq!(seal(&c, "{\"hosts\":{}}").unwrap(), "{\"hosts\":{}}");
    }

    #[test]
    fn plaintext_reads_back_whatever_the_settings_say() {
        // Turning encryption on must not make existing history unreadable.
        for c in [cfg(false, ""), cfg(true, "/nonexistent")] {
            assert_eq!(unseal(&c, "{\"hosts\":{}}").unwrap(), "{\"hosts\":{}}");
        }
    }

    #[test]
    fn sealing_without_an_identity_fails_rather_than_writing_cleartext() {
        // The one outcome worth ruling out: encryption on, something missing,
        // and the run quietly pushes the payload in clear anyway.
        let err = seal(&cfg(true, ""), "secret").unwrap_err();
        assert!(
            format!("{err:#}").contains("identity"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn sealing_with_a_missing_identity_file_fails() {
        let err = seal(&cfg(true, "/nonexistent/age.key"), "secret").unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.contains("nonexistent") || text.contains("reading age identity"),
            "{text}"
        );
    }

    #[test]
    fn decrypting_an_encrypted_repo_without_an_identity_says_so() {
        let err = unseal(&cfg(false, ""), "-----BEGIN AGE ENCRYPTED FILE-----\nx\n").unwrap_err();
        assert!(format!("{err:#}").contains("encrypted"), "{err:#}");
    }

    #[test]
    fn preflight_passes_when_encryption_is_off() {
        assert!(preflight(&cfg(false, "")).is_ok());
        // Even with a bogus identity: it is not consulted.
        assert!(preflight(&cfg(false, "/nonexistent")).is_ok());
    }

    #[test]
    fn preflight_rejects_encryption_without_an_identity() {
        assert!(preflight(&cfg(true, "")).is_err());
        assert!(preflight(&cfg(true, "/nonexistent/age.key")).is_err());
    }

    #[test]
    fn the_recipient_comes_from_the_identity_comment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id.key");
        std::fs::write(
            &path,
            "# created: 2026-09-20\n# public key: age1qqqqqqqqqq\nAGE-SECRET-KEY-1XXXX\n",
        )
        .unwrap();
        assert_eq!(recipient_for(&path).unwrap(), "age1qqqqqqqqqq");
    }

    #[test]
    fn a_tilde_in_the_identity_path_is_expanded() {
        let c = cfg(true, "~/.config/sshm/sync.key");
        let p = identity_path(&c).unwrap();
        assert!(!p.to_string_lossy().starts_with('~'), "{}", p.display());
        assert!(p.is_absolute(), "{}", p.display());
    }

    #[test]
    fn no_identity_configured_reads_as_none() {
        assert!(identity_path(&cfg(true, "")).is_none());
        assert!(identity_path(&cfg(true, "   ")).is_none());
    }

    // ---- round trip against a real `age` -----------------------------------
    //
    // Self-skipping rather than `#[ignore]`d: on a machine (or a CI runner)
    // with `age` installed this runs as part of the normal suite, which is the
    // only way a shell-out gets genuinely exercised.

    fn with_identity<T>(f: impl FnOnce(&SyncConfig) -> T) -> Option<T> {
        if !age_available() {
            eprintln!("skipping: `age` is not on PATH");
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let id = dir.path().join("sync-age.key");
        let out = Command::new("age-keygen")
            .arg("-o")
            .arg(&id)
            .output()
            .expect("age-keygen runs when age is installed");
        assert!(out.status.success(), "age-keygen failed");
        let c = SyncConfig {
            encrypt: true,
            age_identity: id.to_string_lossy().to_string(),
            ..Default::default()
        };
        Some(f(&c))
    }

    #[test]
    fn a_payload_survives_a_round_trip() {
        with_identity(|c| {
            let plain = r#"{"hosts":{"web":{"name":"web","host":"10.0.0.5"}}}"#;
            let blob = seal(c, plain).expect("seal");
            assert!(is_encrypted(&blob), "the blob must be armored:\n{blob}");
            assert!(
                !blob.contains("10.0.0.5"),
                "the address leaked into the blob"
            );
            assert_eq!(unseal(c, &blob).expect("unseal"), plain);
        });
    }

    #[test]
    fn a_blob_is_not_readable_with_another_identity() {
        with_identity(|a| {
            let blob = seal(a, "secret payload").expect("seal");
            with_identity(|b| {
                assert!(
                    unseal(b, &blob).is_err(),
                    "a different identity must not decrypt this"
                );
            });
        });
    }

    #[test]
    fn preflight_accepts_a_real_identity() {
        with_identity(|c| assert!(preflight(c).is_ok(), "{:?}", preflight(c)));
    }

    #[test]
    fn multi_line_content_round_trips_intact() {
        // theme.toml and settings.toml are multi-line; armoring must not eat
        // or add a newline, or every sync would see a spurious diff.
        with_identity(|c| {
            let plain = "bg = \"#282828\"\nfg = \"#dcdccc\"\n";
            let blob = seal(c, plain).expect("seal");
            assert_eq!(unseal(c, &blob).expect("unseal"), plain);
        });
    }

    #[test]
    fn an_empty_payload_round_trips() {
        with_identity(|c| {
            let blob = seal(c, "").expect("seal");
            assert_eq!(unseal(c, &blob).expect("unseal"), "");
        });
    }
}
