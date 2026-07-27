use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type de tunnel SSH.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TunnelKind {
    /// `-L` Local forward: localhost:local_port -> remote:remote_port (via SSH host)
    #[default]
    Local,
    /// `-R` Remote forward: remote:remote_port -> localhost:local_port
    Remote,
    /// `-D` Dynamic SOCKS proxy on localhost:local_port
    Dynamic,
}

impl TunnelKind {
    pub fn label(&self) -> &'static str {
        match self {
            TunnelKind::Local => "Local (-L)",
            TunnelKind::Remote => "Remote (-R)",
            TunnelKind::Dynamic => "Dynamic SOCKS (-D)",
        }
    }
    pub fn short(&self) -> &'static str {
        match self {
            TunnelKind::Local => "L",
            TunnelKind::Remote => "R",
            TunnelKind::Dynamic => "D",
        }
    }
}

/// Définition d'un tunnel SSH sauvegardable.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tunnel {
    /// Libellé court (ex : "Postgres prod").
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: TunnelKind,
    /// Port côté local (Local/Dynamic) ou côté remote-bind (Remote).
    pub local_port: u16,
    /// Port distant cible (Local/Remote). Ignoré pour Dynamic.
    #[serde(default)]
    pub remote_port: u16,
    /// Hôte distant cible (Local/Remote). Vide => `localhost` côté remote.
    #[serde(default)]
    pub remote_host: String,
}

/// Représente une entrée d'hôte SSH (schéma v2).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Host {
    /// Alias (clé logique)
    pub name: String,
    /// Hostname ou IP (ex `1.2.3.4` ou `example.com`)
    pub host: String,
    /// Port SSH (par défaut 22)
    #[serde(default = "default_port")]
    pub port: u16,
    /// Nom d'utilisateur SSH
    #[serde(default = "default_username")]
    pub username: String,
    /// Chemin vers la clé privée (ex: ~/.ssh/id_ed25519)
    #[serde(default)]
    pub identity_file: Option<String>,
    /// ProxyJump éventuel. Peut être une chaîne multi-hop séparée par des virgules
    /// (ex: "bastion1,bastion2"). Chaque entrée peut être un nom d'hôte sauvegardé
    /// dans sshm — il sera alors résolu en `user@host:port` au lancement.
    #[serde(default)]
    pub proxy_jump: Option<String>,
    /// Tags pour filtrage/organisation
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Folder logique (ex : "Production", "Staging", etc.)
    /// Peut être None (pas de dossier)
    #[serde(default)]
    pub folder: Option<String>,
    /// Timestamp RFC3339 UTC de la dernière connexion réussie.
    #[serde(default)]
    pub last_connected_at: Option<String>,
    /// Nombre total de connexions depuis ce gestionnaire.
    #[serde(default)]
    pub use_count: u32,
    /// Marqueur "favori" (affiché en tête de liste via tri dédié).
    #[serde(default)]
    pub favorite: bool,
    /// Tunnels persistants associés à cet hôte.
    #[serde(default)]
    pub tunnels: Vec<Tunnel>,
    /// Forward le ssh-agent local (`-A`). Implication de sécurité : permet à un
    /// utilisateur root du host distant d'utiliser tes clés. À n'activer que sur
    /// des hôtes de confiance (typiquement bastions). Désactivé par défaut.
    #[serde(default)]
    pub forward_agent: bool,
    /// Connecte via `mosh` plutôt que `ssh`. mosh doit être installé localement
    /// et sur l'hôte distant ; le port / l'identité / le ProxyJump sont passés
    /// à mosh via `--ssh`. Désactivé par défaut.
    #[serde(default)]
    pub mosh: bool,
    /// Note libre de l'utilisateur sur cet hôte (affichée dans le panneau de
    /// détail). `None` quand aucune note n'a été saisie.
    #[serde(default)]
    pub notes: Option<String>,
    /// Commande à exécuter automatiquement à la connexion (mappée sur l'option
    /// ssh `RemoteCommand`, avec `-t` pour forcer un TTY). Par défaut, la
    /// commande est lancée puis on enchaîne sur un shell interactif de login
    /// (`; exec $SHELL -l` ajouté automatiquement) — sinon ssh fermerait la
    /// session dès la fin de la commande. Si la commande contient `exec `,
    /// elle est utilisée telle quelle (l'utilisateur gère le shell lui-même).
    /// `None` = shell de login normal. Ignoré en mode mosh.
    #[serde(default)]
    pub remote_command: Option<String>,
}

/// Lightweight signature (mtime + length) of the file backing a [`Database`],
/// used to cheaply detect that it changed underneath us. In-memory only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSig {
    pub mtime: std::time::SystemTime,
    pub len: u64,
}

/// Base de données de l'application (hosts + dossiers)
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Database {
    #[serde(default)]
    pub hosts: HashMap<String, Host>,
    /// Liste des dossiers existants (pas de sous-dossiers)
    #[serde(default)]
    pub folders: Vec<String>,
    /// Signature of the file this Database was last loaded from. In-memory only
    /// (`#[serde(skip)]`, so the on-disk JSON is unchanged); lets
    /// [`Database::reload_if_changed`] short-circuit when nothing changed.
    #[serde(skip)]
    #[cfg_attr(feature = "specta", specta(skip))]
    pub(crate) source_sig: Option<SourceSig>,
}

impl Database {
    /// Stat `path` and return its current signature, or `None` if it can't be
    /// stat-ed (missing / permission denied).
    pub(crate) fn sig_of(path: &std::path::Path) -> Option<SourceSig> {
        let meta = std::fs::metadata(path).ok()?;
        Some(SourceSig { mtime: meta.modified().ok()?, len: meta.len() })
    }

    /// Record `path`'s current signature as this Database's baseline. Called
    /// right after a load so later [`reload_if_changed`](Self::reload_if_changed)
    /// calls have something to compare against.
    pub fn set_source(&mut self, path: &std::path::Path) {
        self.source_sig = Self::sig_of(path);
    }

    /// Reload the host DB from disk **iff** the file changed since the last
    /// load, replacing `hosts`/`folders` in place.
    ///
    /// Returns `Ok(true)` only when the on-disk *content* actually differs from
    /// what's held in memory — so a frontend writing its own changes (or any
    /// no-op touch) does not report a spurious reload. `Ok(false)` means
    /// nothing user-visible changed. Cheap on the common path: an unchanged
    /// mtime+length short-circuits with a single `stat`, no read or parse.
    ///
    /// Detection is mtime+length based (last-writer-wins); a rare same-mtime,
    /// same-length, different-content write can slip past the fast path, which
    /// the filesystem watcher covers.
    pub fn reload_if_changed(&mut self) -> anyhow::Result<bool> {
        let path = crate::config::path::config_path();
        self.reload_if_changed_from(&path)
    }

    /// Path-injectable core of [`reload_if_changed`](Self::reload_if_changed),
    /// so the reload behaviour can be exercised against a temp file in tests
    /// rather than the user's real `host.json`.
    pub fn reload_if_changed_from(&mut self, path: &std::path::Path) -> anyhow::Result<bool> {
        let current = Self::sig_of(path);
        if current == self.source_sig {
            return Ok(false);
        }
        // The signature moved — rebase it, then decide whether anything the user
        // would see actually changed.
        self.source_sig = current;
        let text = std::fs::read_to_string(path).unwrap_or_default();

        // Compare the file against our own canonical serialization. A frontend's
        // own save writes exactly this, so this swallows self-writes (and any
        // byte-identical external write) without a spurious "hosts reloaded".
        // It also sidesteps in-memory folder ordering: `serialize_db` sorts and
        // dedups folders, matching what lands on disk.
        if let Ok(canonical) = crate::config::io::serialize_db(self) {
            if text.trim() == canonical.trim() {
                return Ok(false);
            }
        }

        let fresh = crate::config::io::parse_db_text(&text).unwrap_or_default();
        self.hosts = fresh.hosts;
        self.folders = fresh.folders;
        Ok(true)
    }
}

fn default_port() -> u16 { 22 }
fn default_username() -> String { "root".to_string() }

/// Convertit `Option<Vec<String>>` en string d'affichage.
pub fn tags_to_string(tags: &Option<Vec<String>>) -> String {
    tags.as_ref()
        .filter(|v| !v.is_empty())
        .map_or_else(String::new, |v| v.join(","))
}
