use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Représente une entrée d’hôte SSH (schéma v2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Host {
    /// Alias (clé logique)
    pub name: String,
    /// Hostname ou IP (ex `1.2.3.4` ou `example.com`)
    pub host: String,
    /// Port SSH (par défaut 22)
    #[serde(default = "default_port")]
    pub port: u16,
    /// Nom d’utilisateur SSH
    #[serde(default = "default_username")]
    pub username: String,
    /// Chemin vers la clé privée (ex: ~/.ssh/id_ed25519)
    #[serde(default)]
    pub identity_file: Option<String>,
    /// ProxyJump éventuel (ex: "bastion.example.com:22")
    #[serde(default)]
    pub proxy_jump: Option<String>,
    /// Tags pour filtrage/organisation
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Folder logique (ex: "Production", "Staging", etc.)
    /// Peut être None (pas de dossier)
    #[serde(default)]
    pub folder: Option<String>,
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

/// Convertit `Option<Vec<String>>` en string d’affichage.
pub fn tags_to_string(tags: &Option<Vec<String>>) -> String {
    match tags {
        None => "".to_string(),
        Some(v) if v.is_empty() => "".to_string(),
        Some(v) => v.join(","),
    }
}
