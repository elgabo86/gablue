//! Configuration du démon : `~/.config/gablue/share.json`.
//!
//! Le fichier est écrit par la GUI, le démon ne fait que le lire (aucune
//! écriture, aucun état). Un partage dont le dossier a disparu (disque non
//! monté) est ignoré avec un message, jamais fatal : le reste continue de
//! servir.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Port par défaut, fixe : la sonde Tailscale des pairs doit le connaître.
pub const DEFAULT_PORT: u16 = 47823;

/// Mot de passe d'un partage, écrit par la GUI et **jamais** dérivé ici.
///
/// Le démon ne connaît que l'empreinte finale : la GUI dérive
/// `pbkdf2_hmac(sha256, mot de passe, sel, itérations)` et le client recalcule
/// la même valeur à partir du sel publié. Vérifier coûte donc une comparaison
/// de chaînes, sans dépendance cryptographique côté serveur — le seul composant
/// exposé au réseau n'a aucune clé à manipuler.
#[derive(Clone, Debug)]
pub struct Password {
    /// Empreinte attendue, en hexadécimal minuscule. Jamais publiée.
    pub hash: String,
    /// Sel de la dérivation, publié dans `/api/info` (il n'est pas secret).
    pub salt: String,
    /// Nombre d'itérations de la dérivation, publié lui aussi.
    pub rounds: u32,
    /// Jeton aléatoire régénéré à chaque changement de mot de passe : le client
    /// le compare à celui qu'il a mémorisé pour savoir qu'il doit redemander.
    pub revision: String,
}

impl Password {
    /// L'empreinte fournie ouvre-t-elle ce partage ?
    ///
    /// Comparaison à temps constant sur les octets, et sur la longueur : une
    /// réponse plus rapide sur le premier caractère faux dirait combien de
    /// caractères sont justes.
    pub fn accepts(&self, provided: Option<&str>) -> bool {
        let candidate = match provided {
            Some(value) => value.trim(),
            None => return false,
        };
        let expected = self.hash.as_bytes();
        let given = candidate.as_bytes();
        // `usize` : un `u8` tronquerait la différence de longueur (un en-tête de
        // 320 octets retomberait sur 0) et la comparaison ne porterait plus que
        // sur le préfixe.
        let mut difference = expected.len() ^ given.len();
        for (index, byte) in expected.iter().enumerate() {
            difference |= usize::from(byte ^ given.get(index).copied().unwrap_or(0));
        }
        difference == 0 && !candidate.is_empty()
    }
}

fn is_hex(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Lit un bloc `password` de `share.json`. Illisible = verrouillé quand même
/// (refus fermé) : un mot de passe mal écrit ne doit pas ouvrir le partage.
fn parse_password(value: &Value) -> Option<Password> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(|raw| raw.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let hash = text("hash");
    let salt = text("salt");
    if !is_hex(&hash) || !is_hex(&salt) {
        let revision = if is_hex(&hash) && hash.len() >= 12 {
            hash[..12].to_string()
        } else {
            "illisible".to_string()
        };
        eprintln!("partage : mot de passe illisible, il reste verrouillé");
        return Some(Password {
            hash: "!".to_string(), // aucune empreinte valide ne peut l'égaler
            salt,
            rounds: 1,
            revision,
        });
    }
    let revision = {
        let declared = text("revision");
        if is_hex(&declared) {
            declared
        } else {
            // Fichier d'une version antérieure : l'empreinte change avec le mot
            // de passe, donc elle fait un jeton de version honnête.
            hash[..12].to_string()
        }
    };
    let rounds = value
        .get("rounds")
        .and_then(Value::as_u64)
        .unwrap_or(200_000)
        .clamp(1, 5_000_000) as u32;
    Some(Password {
        hash,
        salt,
        rounds,
        revision,
    })
}

#[derive(Clone, Debug)]
pub struct Share {
    pub id: String,
    pub name: String,
    /// Chemin tel que déclaré dans la configuration.
    pub path: PathBuf,
    /// Chemin canonique : borne de tous les accès (anti-symlink sortant).
    pub root: PathBuf,
    /// Le dossier est réellement là maintenant (disque externe monté).
    pub available: bool,
    /// Montrer les fichiers et dossiers dont le nom commence par un point.
    /// Faux (défaut) = ils n'apparaissent ni au listing ni à la recherche, mais
    /// un dossier téléchargé les emporte quand même (l'archive est complète).
    pub hidden: bool,
    /// Exposer les sous-dossiers. Faux = seuls les fichiers de la racine du
    /// partage sont listés, servis et trouvables ; les sous-dossiers restent
    /// téléchargeables via l'archive du partage entier.
    pub recursive: bool,
    /// Mot de passe exigé pour ce partage (aucun = ouvert à qui l'adresse).
    pub password: Option<Password>,
}

impl Share {
    pub fn locked(&self) -> bool {
        self.password.is_some()
    }

    /// Le partage répond-il à cette empreinte ? (ouvert = toujours oui)
    pub fn accepts(&self, provided: Option<&str>) -> bool {
        match &self.password {
            Some(password) => password.accepts(provided),
            None => true,
        }
    }
}

/// Qui peut atteindre ce poste. Choix de la machine, pas du partage.
///
/// - `Both` (défaut) : réseau local **et** tailnet ;
/// - `Lan` : réseau local seulement, le tailnet est refusé ;
/// - `Tailscale` : tailnet seulement, aucune annonce mDNS (elle ne parle que
///   sur le LAN) et les adresses du réseau local sont refusées.
///
/// C'est le démon qui applique la règle, à chaque requête : une GUI ne peut pas
/// la contourner, et un poste qui change d'avis est suivi sans redémarrage
/// (le fichier de configuration est relu à chaud).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Both,
    Lan,
    Tailscale,
}

impl Visibility {
    pub fn parse(raw: &str) -> Visibility {
        match raw.trim().to_ascii_lowercase().as_str() {
            "lan" | "local" => Visibility::Lan,
            "tailscale" | "tailnet" => Visibility::Tailscale,
            _ => Visibility::Both,
        }
    }

    /// Annonce-t-on sur le réseau local ? mDNS ne parle que sur le LAN : en
    /// mode tailnet, publier une annonce serait au mieux inutile, au pire une
    /// invitation que le filtre d'adresses refuserait.
    pub fn announces(self) -> bool {
        matches!(self, Visibility::Both | Visibility::Lan)
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub peer_id: String,
    pub name: String,
    pub port: u16,
    pub visibility: Visibility,
    /// Le serveur doit-il servir ? `false` = partage arrêté depuis la GUI, le
    /// démon sort (et ne démarre pas au lancement suivant) tant que le fichier
    /// ne dit pas l'inverse. La GUI et le service permanent lisent la même
    /// vérité : un partage arrêté ne revient pas parce qu'on rouvre la fenêtre.
    pub serving: bool,
    pub shares: Vec<Share>,
}

impl Config {
    pub fn share(&self, id: &str) -> Option<&Share> {
        self.shares.iter().find(|s| s.id == id)
    }

    /// Partages réellement servis : un disque débranché n'est ni annoncé, ni
    /// listé dans `/api/info`, ni servi — il réapparaît seul au remontage.
    pub fn available(&self) -> impl Iterator<Item = &Share> {
        self.shares.iter().filter(|share| share.available)
    }
}

/// Racines où un dossier peut être un point de montage de disque externe.
const MOUNT_ROOTS: [&str; 3] = ["/mnt", "/media", "/run/media"];

/// Le partage est-il utilisable à cet instant ?
///
/// Un disque externe débranché ne répond plus ; sous udisks le dossier de
/// montage (`/run/media/<user>/<label>`) disparaît avec lui, donc `is_dir`
/// suffit. Reste le point de montage manuel (`/mnt/sauvegarde` dans fstab) :
/// le dossier reste là, vide, sur le système de fichiers racine. On le reconnaît
/// à son périphérique, identique à celui de son parent.
pub fn is_available(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let meta = match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => meta,
        _ => return false,
    };
    if MOUNT_ROOTS.iter().any(|root| path.starts_with(root)) {
        if let Some(parent) = path.parent().and_then(|parent| fs::metadata(parent).ok()) {
            if parent.dev() == meta.dev() {
                return false; // point de montage vide : rien n'est monté dessus
            }
        }
    }
    true
}

/// Chemin de la configuration, aligné sur le reste de Gablue.
pub fn default_path() -> PathBuf {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        }
    };
    base.join("gablue").join("share.json")
}

/// Charge et valide la configuration. Erreur = message lisible par l'humain.
pub fn load(path: &Path) -> Result<Config, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("configuration illisible ({}) : {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("configuration invalide ({}) : {e}", path.display()))?;

    let peer_id = doc
        .get("peer_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(machine_id);
    let name = doc
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(hostname);
    let port = doc
        .get("port")
        .and_then(Value::as_u64)
        .filter(|p| (1..=65535).contains(p))
        .unwrap_or(DEFAULT_PORT as u64) as u16;

    let visibility = Visibility::parse(
        doc.get("visibility")
            .and_then(Value::as_str)
            .unwrap_or("both"),
    );
    let serving = doc
        .get("serving")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let mut shares: Vec<Share> = Vec::new();
    for (index, item) in doc
        .get("shares")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let declared = item.get("path").and_then(Value::as_str).unwrap_or("").trim();
        if declared.is_empty() {
            continue;
        }
        let path = PathBuf::from(declared);
        // Un disque débranché n'est pas une erreur : le partage reste déclaré,
        // simplement indisponible, et revient dès que le chemin réapparaît.
        let (root, available) = if path.is_dir() {
            let root = path.canonicalize().unwrap_or_else(|_| path.clone());
            (root, is_available(&path))
        } else if !path.exists() {
            eprintln!("partage en attente (dossier absent) : {declared}");
            (path.clone(), false)
        } else {
            eprintln!("partage ignoré (pas un dossier) : {declared}");
            continue;
        };
        let mut id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("s{}", index + 1));
        if shares.iter().any(|s| s.id == id) {
            id = format!("{id}-{}", index + 1);
        }
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                root.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| id.clone())
            });
        shares.push(Share {
            id,
            name,
            path,
            root,
            available,
            hidden: item
                .get("hidden")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            recursive: item
                .get("recursive")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            password: item.get("password").and_then(parse_password),
        });
    }

    Ok(Config {
        peer_id,
        name,
        port,
        visibility,
        serving,
        shares,
    })
}

/// Identifiant stable du poste : `/etc/machine-id`, repli horodatage.
fn machine_id() -> String {
    if let Ok(raw) = fs::read_to_string("/etc/machine-id") {
        let id = raw.trim();
        if !id.is_empty() {
            return id.to_string();
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{now:x}")
}

fn hostname() -> String {
    if let Ok(raw) = fs::read_to_string("/etc/hostname") {
        let name = raw.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "gablue".to_string())
}
