//! Partages `gablue-share` d'autres postes : choisir un partage, le monter en
//! lecture seule par `sshfs`, puis lancer ou copier ses jeux comme ceux d'un
//! disque externe.
//!
//! **Pourquoi `sshfs` et pas le HTTP du partage** (mesuré le 28/11/2026 sur le
//! même pack OoT, même Wi-Fi) : le débit est identique (94 Mo/s dans les deux
//! cas) mais le HTTP paie un aller-retour par bloc — 15,7 ms par Mio contre
//! 8,1 ms en SSH, où toutes les requêtes passent multiplexées sur une seule
//! connexion. Et surtout `sshfs` **ne ment pas** quand le lien gèle : sans
//! `ServerAliveInterval`, la connexion TCP retransmet au lieu de mourir, donc
//! la lecture bloque puis rend les bonnes données (vérifié : 12 s de coupure →
//! reprise à 14 s, SHA-256 identique ; sshfs avec keepalive rendait 0 octet =
//! fin de fichier prématurée, ce qui casse un jeu).
//!
//! **Le partage fournit le catalogue, SSH le système de fichiers.** Un partage
//! `gablue-share` ne publie pas son chemin (l'identifiant `s…` en est dérivé,
//! exprès). On lit donc `~/.config/gablue/share.json` du poste par SSH : on y
//! trouve l'identifiant, le nom et le chemin de chaque partage — l'identifiant
//! étant `s` + 12 hex de `sha256(chemin)`, la correspondance est vérifiable et
//! sans ambiguïté.
//!
//! Conséquence assumée : **lancer** un jeu exige un accès SSH au poste (les
//! machines Gablue du parc). Sans SSH, le partage ne peut pas être monté.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Découverte mDNS : mêmes noms de service que le démon (`announce.rs`).
const SERVICE: &str = "_gablueshare._tcp";

/// Au-delà, on n'interroge plus de postes : un réseau d'amis ne doit pas
/// transformer l'ouverture du menu en tournée de sondes.
const POSTES_MAX: usize = 8;

/// Un partage vu chez un poste, avec de quoi le monter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partage {
    /// Nom du poste tel qu'annoncé (« gen1 »).
    pub poste: String,
    /// Adresse à joindre (`192.168.1.110`, à défaut une adresse tailnet).
    pub hote: String,
    /// Identifiant `gablue-share` (`s1855bc46af4d`).
    pub id: String,
    /// Nom affiché du partage (« lgp »).
    pub nom: String,
    /// Chemin absolu chez le poste (lu dans son `share.json`).
    pub chemin: PathBuf,
}

impl Partage {
    /// Libellé du menu : « gen1 · lgp ».
    pub fn libelle(&self) -> String {
        format!("{} · {}", self.poste, self.nom)
    }

    /// Point de montage, sous `$XDG_RUNTIME_DIR` : il disparaît à la
    /// déconnexion, donc aucun montage orphelin ne survit à la session.
    ///
    /// Le nom du poste peut contenir espaces ou accents : on ne garde que
    /// `[a-z0-9-_]`, le reste devient `-` (un point de montage avec une espace
    /// compliquerait `fusermount` et les `starts_with` du filtre).
    pub fn point_de_montage(&self) -> PathBuf {
        let base = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("/tmp/glauncher-{}", unsafe {
                libc::getuid()
            })));
        base.join("glauncher")
            .join("partages")
            .join(format!(
                "{}-{}",
                slug(&self.poste),
                slug(&self.id)
            ))
    }
}

/// Réduit une chaîne à ce qui passe partout dans un nom de dossier.
fn slug(texte: &str) -> String {
    let mut out = String::with_capacity(texte.len());
    for c in texte.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Postes qui annoncent un partage : mDNS d'abord, tailnet en complément.
///
/// mDNS donne l'adresse du **réseau local** (le chemin le plus court) ; la
/// sonde `tailscale status` rattrape un poste hors du LAN ou sur un réseau qui
/// filtre le multicast. Les doublons sont écartés par nom, le LAN gagnant.
fn postes() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let nous = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|n| n.trim().to_string())
        .unwrap_or_default();

    // `avahi-browse -rp` n'exit pas : on le borne par `timeout`, puis on lit ce
    // qu'il a déjà écrit. `-r` est indispensable (sans lui, aucun ligne `=`,
    // donc aucune adresse — le piège du 15/11/2026 côté gablue-share).
    if let Ok(sortie) = Command::new("timeout")
        .args(["3", "avahi-browse", "-rp", SERVICE])
        .output()
    {
        for ligne in String::from_utf8_lossy(&sortie.stdout).lines() {
            if !ligne.starts_with('=') {
                continue;
            }
            let champs: Vec<&str> = ligne.split(';').collect();
            // =;iface;proto;nom;type;domaine;hote;adresse;port;txt
            let (Some(nom), Some(adresse)) = (champs.get(3), champs.get(7)) else {
                continue;
            };
            // une seule adresse par poste : IPv4 privée de préférence (le LAN)
            let deja = out.iter().any(|(n, _)| n == nom);
            if !deja && adresse.parse::<std::net::Ipv4Addr>().is_ok() {
                out.push((nom.to_string(), adresse.to_string()));
            }
        }
    }

    if let Ok(sortie) = Command::new("tailscale").args(["status"]).output() {
        for ligne in String::from_utf8_lossy(&sortie.stdout).lines() {
            let champs: Vec<&str> = ligne.split_whitespace().collect();
            let (Some(adresse), Some(nom)) = (champs.first(), champs.get(1)) else {
                continue;
            };
            if !adresse.starts_with("100.") || ligne.contains("offline") {
                continue;
            }
            if !out.iter().any(|(n, _)| n == nom) {
                out.push((nom.to_string(), adresse.to_string()));
            }
        }
    }

    // on ne se propose pas soi-même : les dossiers de ce poste s'ajoutent comme
    // dossiers de jeux, sans passer par le réseau
    out.retain(|(nom, _)| nom != &nous);
    out.truncate(POSTES_MAX);
    out
}

/// Catalogue d'un poste : ses partages servis, lus dans son `share.json`.
///
/// Retourne vide si le poste ne répond pas en SSH (pas de clé, service coupé)
/// ou s'il ne sert aucun partage.
fn catalogue(poste: &str, hote: &str) -> Vec<(String, String, PathBuf)> {
    let sortie = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            hote,
            "cat ~/.config/gablue/share.json",
        ])
        .output();
    let Ok(sortie) = sortie else {
        return Vec::new();
    };
    if !sortie.status.success() {
        return Vec::new();
    }
    let Ok(texte) = String::from_utf8(sortie.stdout) else {
        return Vec::new();
    };
    let Ok(valeur) = serde_json::from_str::<serde_json::Value>(&texte) else {
        return Vec::new();
    };
    // un partage arrêté (`serving: false`) n'est pas servi : inutile de le
    // proposer, le montage donnerait un dossier vide
    if valeur.get("serving").and_then(|s| s.as_bool()) == Some(false) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for partage in valeur
        .get("shares")
        .and_then(|s| s.as_array())
        .into_iter()
        .flatten()
    {
        let (Some(id), Some(chemin)) = (
            partage.get("id").and_then(|v| v.as_str()),
            partage.get("path").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let nom = partage
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(poste)
            .to_string();
        out.push((id.to_string(), nom, PathBuf::from(chemin)));
    }
    out
}

/// Tous les partages joignables maintenant. **Bloquant** (mDNS + SSH) : à
/// appeler depuis un fil de fond, jamais depuis l'interface.
pub fn decouvrir() -> Vec<Partage> {
    let mut out = Vec::new();
    for (poste, hote) in postes() {
        for (id, nom, chemin) in catalogue(&poste, &hote) {
            out.push(Partage {
                poste: poste.clone(),
                hote: hote.clone(),
                id,
                nom,
                chemin,
            });
        }
    }
    out
}

/// Monte le partage en lecture seule, et rend son point de montage.
///
/// **Bloquant** (une seconde au plus) : à appeler depuis un fil de fond.
/// Idempotent : un partage déjà monté est réutilisé tel quel.
pub fn monter(partage: &Partage) -> Result<PathBuf, String> {
    let point = partage.point_de_montage();
    if est_monte(&point) {
        return Ok(point);
    }
    std::fs::create_dir_all(&point)
        .map_err(|err| format!("dossier de montage impossible : {err}"))?;
    let source = format!("{}:{}", partage.hote, partage.chemin.display());
    // Pas de `ServerAliveInterval` : c'est lui qui déclarait la connexion morte
    // après 4 s de gel, puis rendait 0 octet sur les fichiers déjà ouverts.
    // `kernel_cache` garde les données lues en mémoire récupérable (relire une
    // zone déjà vue ne repasse ni par le réseau ni par un cache disque).
    let sortie = Command::new("sshfs")
        .arg("-o")
        .arg("ro,reconnect,kernel_cache")
        .arg(&source)
        .arg(&point)
        .output()
        .map_err(|err| format!("sshfs introuvable : {err}"))?;
    if !sortie.status.success() || !est_monte(&point) {
        let detail = String::from_utf8_lossy(&sortie.stderr);
        let detail = detail.lines().next().unwrap_or("").trim().to_string();
        return Err(if detail.is_empty() {
            format!("montage de {} impossible", partage.libelle())
        } else {
            detail
        });
    }
    Ok(point)
}

/// Démonte un partage, sans jamais échouer bruyamment (il peut déjà être
/// tombé avec la connexion).
pub fn demonter(point: &Path) {
    let _ = Command::new("fusermount3")
        .args(["-u", "-z"])
        .arg(point)
        .output();
    // `fusermount3` manque sur les systèmes plus anciens ; `fusermount` fait le
    // même travail.
    if est_monte(point) {
        let _ = Command::new("fusermount")
            .args(["-u", "-z"])
            .arg(point)
            .output();
    }
}

/// Le point de montage porte-t-il vraiment un système de fichiers ?
///
/// Lecture de `/proc/self/mounts` (aucune commande externe) ; les espaces y
/// sont échappés en `\040`, d'où le dé-échappement avant comparaison.
pub fn est_monte(point: &Path) -> bool {
    let Ok(texte) = std::fs::read_to_string("/proc/self/mounts") else {
        return false;
    };
    let cible = point.to_string_lossy().to_string();
    texte.lines().any(|ligne| {
        ligne
            .split_whitespace()
            .nth(1)
            .map(|chemin| chemin.replace("\\040", " ") == cible)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_de_montage_neutre_les_caracteres_genants() {
        let partage = Partage {
            poste: "Gen 1".into(),
            hote: "192.168.1.110".into(),
            id: "s1855bc46af4d".into(),
            nom: "lgp".into(),
            chemin: PathBuf::from("/var/home/gab/Games/lgp"),
        };
        let point = partage.point_de_montage();
        let nom = point.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(nom, "gen-1-s1855bc46af4d");
        assert!(point.to_string_lossy().ends_with("glauncher/partages/gen-1-s1855bc46af4d"));
    }

    /// Montage réel d'un partage du réseau : exige un poste `gablue-share`
    /// joignable en SSH. Ignoré par défaut, `cargo test -- --ignored`.
    #[test]
    #[ignore = "réseau : exige un poste gablue-share joignable en SSH"]
    fn monte_et_demonte_un_partage_reel() {
        let partages = decouvrir();
        let Some(partage) = partages.first().cloned() else {
            eprintln!("aucun partage joignable — test ignoré");
            return;
        };
        let point = monter(&partage).expect("montage du partage");
        assert!(est_monte(&point), "le point de montage doit être monté");
        let entrees = std::fs::read_dir(&point).expect("lecture du partage").count();
        assert!(entrees > 0, "un partage vide signalerait un mauvais chemin");
        eprintln!("{} monté sur {} ({entrees} entrées)", partage.libelle(), point.display());
        demonter(&point);
        assert!(!est_monte(&point), "le démontage doit être effectif");
    }

    #[test]
    fn libelle_reunit_poste_et_partage() {
        let partage = Partage {
            poste: "gen1".into(),
            hote: "192.168.1.110".into(),
            id: "sf67861d9462a".into(),
            nom: "wgp".into(),
            chemin: PathBuf::from("/var/home/gab/Games/wgp"),
        };
        assert_eq!(partage.libelle(), "gen1 · wgp");
    }
}
