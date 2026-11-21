//! Réglages de capture de l'enregistreur maison (`gpad-rec`, `src/gablue-rec`).
//!
//! `~/.config/gablue/recorder.conf` est la **seule** source de vérité :
//! l'overlay l'écrit, le démon le relit (`reload` sur sa socket). Le fichier
//! est réécrit clé par clé : une clé inconnue (ajoutée par une version future)
//! n'est jamais perdue.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// (libellé affiché, valeur écrite dans le fichier)
pub const CODECS: [(&str, &str); 2] = [("H.264", "h264"), ("H.265", "h265")];
/// Vitesse d'encodeur bornée : H.264 par défaut, c'est ce qui s'ouvre partout.
pub const QUALITES: [(&str, &str); 5] = [
    ("ECO", "1"),
    ("NORMALE", "2"),
    ("HAUTE", "3"),
    ("TRES HAUTE", "4"),
    ("MAX", "5"),
];
pub const RESOS: [(&str, &str); 4] = [
    ("720P", "1280x720"),
    ("1080P", "1920x1080"),
    ("1440P", "2560x1440"),
    ("SOURCE", "source"),
];
/// Cadence d'émission ; `AUTO` suit les flips (VFR réel) et c'est le défaut :
/// chaque image affichée est capturée, aucune n'est sautée ni dupliquée. Les
/// valeurs fixes bornent la cadence (fichiers plus légers).
pub const FPS: [(&str, &str); 4] = [("AUTO", "flips"), ("60", "60"), ("120", "120"), ("144", "144")];
pub const CURSEURS: [(&str, &str); 2] = [("MASQUE", "masque"), ("AFFICHE", "affiche")];

fn defauts() -> Vec<(String, String)> {
    [
        ("codec", "h264"),
        ("qualite", "3"),
        ("resolution", "source"),
        ("fps", "flips"),
        ("curseur", "masque"),
        ("replay_sec", "30"),
        ("auto_replay", "0"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn chemin() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/gablue/recorder.conf"))
}

pub fn socket() -> Option<PathBuf> {
    let rt = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(PathBuf::from(rt).join("gablue/gpad-rec.sock"))
}

/// Demande au démon de relire son fichier. Silencieux : au démarrage, il ne
/// tourne pas encore, ce n'est pas une erreur.
pub fn reload() {
    let Some(p) = socket() else { return };
    let Ok(mut s) = UnixStream::connect(p) else { return };
    let _ = s.write_all(b"{\"cmd\":\"reload\"}\n");
}

pub struct Conf {
    lignes: Vec<(String, String)>,
}

impl Conf {
    pub fn lire() -> Conf {
        let mut lignes = defauts();
        if let Some(p) = chemin() {
            if let Ok(txt) = fs::read_to_string(p) {
                for l in txt.lines() {
                    let l = l.trim();
                    if l.is_empty() || l.starts_with('#') {
                        continue;
                    }
                    let Some((k, v)) = l.split_once('=') else { continue };
                    let (k, v) = (k.trim().to_string(), v.trim().to_string());
                    match lignes.iter_mut().find(|(cle, _)| *cle == k) {
                        Some(e) => e.1 = v,
                        None => lignes.push((k, v)),
                    }
                }
            }
        }
        Conf { lignes }
    }

    pub fn get(&self, cle: &str) -> Option<&str> {
        self.lignes
            .iter()
            .find(|(k, _)| k == cle)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, cle: &str, val: &str) {
        match self.lignes.iter_mut().find(|(k, _)| k == cle) {
            Some(e) => e.1 = val.to_string(),
            None => self.lignes.push((cle.to_string(), val.to_string())),
        }
    }

    /// Écrit le fichier (`.tmp` + `rename`) puis réveille le démon.
    pub fn ecrire(&self) {
        let Some(p) = chemin() else { return };
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let mut txt = String::from("# réglages de gpad-rec (écrit par gpad)\n");
        for (k, v) in &self.lignes {
            txt.push_str(k);
            txt.push('=');
            txt.push_str(v);
            txt.push('\n');
        }
        let tmp = p.with_extension("tmp");
        if fs::write(&tmp, txt).is_ok() && fs::rename(&tmp, &p).is_err() {
            let _ = fs::remove_file(&tmp);
        }
        reload();
    }
}

/// Index de la valeur courante dans une table (0 si inconnue).
pub fn idx(conf: &Conf, cle: &str, table: &[(&str, &str)]) -> usize {
    match conf.get(cle) {
        Some(v) => table.iter().position(|(_, val)| *val == v).unwrap_or(0),
        None => 0,
    }
}

/// Libellé d'une valeur courante.
pub fn libelle(
    conf: &Conf,
    cle: &str,
    table: &'static [(&'static str, &'static str)],
) -> &'static str {
    table[idx(conf, cle, table)].0
}

/// État du démon, tel que sa socket le publie.
#[derive(Debug, Clone, Default)]
pub struct Etat {
    pub recording: bool,
    pub replay: bool,
    pub replay_sec: i32,
    pub file: String,
    /// secondes d'enregistrement en cours
    pub t: f64,
    /// réglage écrit mais pas encore appliqué (un mp4 est ouvert)
    pub pending: bool,
}

/// Envoie une commande (une ligne JSON) et rend la réponse. `None` si le démon
/// ne tourne pas — ce n'est pas une erreur : il démarre avec l'enregistrement.
pub fn cmd(requete: &str) -> Option<String> {
    let mut s = UnixStream::connect(socket()?).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(requete.as_bytes()).ok()?;
    s.write_all(b"\n").ok()?;
    let mut buf = String::new();
    BufReader::new(s).read_line(&mut buf).ok()?;
    Some(buf)
}

/// Valeur brute d'un champ JSON plat (`"clé":valeur`), sans les guillemets.
fn champ<'a>(js: &'a str, cle: &str) -> Option<&'a str> {
    let motif = format!("\"{cle}\":");
    let i = js.find(&motif)? + motif.len();
    let reste = &js[i..];
    if let Some(r) = reste.strip_prefix('"') {
        return r.find('"').map(|j| &r[..j]);
    }
    let fin = reste.find([',', '}']).unwrap_or(reste.len());
    Some(reste[..fin].trim())
}

pub fn etat() -> Option<Etat> {
    let js = cmd("{\"cmd\":\"status\"}")?;
    if champ(&js, "ok") != Some("true") {
        return None;
    }
    Some(Etat {
        recording: champ(&js, "recording") == Some("true"),
        replay: champ(&js, "replay") == Some("true"),
        replay_sec: champ(&js, "replay_sec").and_then(|v| v.parse().ok()).unwrap_or(0),
        file: champ(&js, "file").unwrap_or("").to_string(),
        t: champ(&js, "t").and_then(|v| v.parse().ok()).unwrap_or(0.0),
        pending: champ(&js, "pending") == Some("true"),
    })
}
