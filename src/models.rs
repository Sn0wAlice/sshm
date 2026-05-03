use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type de tunnel SSH.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelKind {
    /// `-L` Local forward: localhost:local_port -> remote:remote_port (via SSH host)
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

impl Default for TunnelKind {
    fn default() -> Self { TunnelKind::Local }
}

/// Définition d'un tunnel SSH sauvegardable.
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
}

/// Base de données de l'application (hosts + dossiers)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Database {
    #[serde(default)]
    pub hosts: HashMap<String, Host>,
    /// Liste des dossiers existants (pas de sous-dossiers)
    #[serde(default)]
    pub folders: Vec<String>,
}

fn default_port() -> u16 { 22 }
fn default_username() -> String { "root".to_string() }

/// Convertit `Option<Vec<String>>` en string d'affichage.
pub fn tags_to_string(tags: &Option<Vec<String>>) -> String {
    tags.as_ref()
        .filter(|v| !v.is_empty())
        .map_or_else(String::new, |v| v.join(","))
}
