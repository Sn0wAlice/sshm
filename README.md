# sshm – SSH Host Manager

**sshm** est un outil en ligne de commande écrit en Rust pour gérer facilement une liste d’hôtes SSH stockée dans un fichier JSON local. Il permet de lister, créer, modifier, supprimer et se connecter à des hôtes SSH depuis une interface interactive en terminal grâce à la bibliothèque [`inquire`](https://github.com/mikaelmello/inquire).

## 📦 Installation

### Prérequis

- [Rust](https://www.rust-lang.org/tools/install) installé (via `rustup`)
- `ssh` disponible dans votre terminal

### Compilation

```bash
git clone https://github.com/tonrepo/sshm.git
cd sshm
cargo build --release
```

Le binaire sera disponible dans ./target/release/sshm.

Pour l’utiliser globalement :

```bash
cp ./target/release/sshm /usr/local/bin/
```


## Fichier de configuration

Le fichier est automatiquement créé à l’emplacement suivant si absent :

```
$HOME/.config/sshm/host.json
```

Il contient un dictionnaire JSON des hôtes SSH avec la structure suivante :
```json
{
  "my-server": {
    "name": "my-server",
    "ip": "192.168.1.10",
    "port": 22,
    "username": "alice"
  }
}
```

🧰 Commandes disponibles
```
sshm list
```
Affiche tous les hôtes enregistrés.
```
sshm create
```
Ajoute un nouvel hôte interactif.
```
sshm edit
```
Édite un hôte existant via sélection interactive.
```
sshm delete
```
Supprime un hôte de la configuration.
```
sshm connect [nom]
sshm c [nom]
```
Se connecte à un hôte. Si plusieurs hôtes correspondent au nom, une sélection interactive est proposée. Si aucun nom n’est fourni, tous les hôtes sont proposés.

## Exemple

```bash
$ sshm create
Name: dev-server
IP: 10.0.0.5
Port: 22
Username: ubuntu

$ sshm list
dev-server => ubuntu@10.0.0.5:22

$ sshm c dev
# ssh vers ubuntu@10.0.0.5 -p 22
```

## 🛠️ Dépendances principales
- inquire – Interface interactive CLI
- serde + serde_json – Lecture/écriture JSON
- dirs – Gestion du chemin de configuration utilisateur