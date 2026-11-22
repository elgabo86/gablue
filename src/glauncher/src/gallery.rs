//! Galerie : captures d'écran, enregistrements et replays.
//!
//! Les fichiers viennent des deux dossiers de `recorder.conf` (les mêmes que
//! l'enregistreur, l'overlay manette et les Réglages) : `screenshot_*.png` pour
//! les captures d'écran, `Video_*.mp4` / `Replay_*.mp4` pour les vidéos.
//!
//! Les vignettes sont fabriquées **sur un fil de fond**, une par une, et mises
//! en cache dans `<cache>/gallery/` : une capture 4K pèse 5 à 15 Mo et se
//! décoderait sur le fil de l'interface (iced décode en plus sur un seul
//! thread). Une vidéo n'a pas d'image : `ffmpeg` en extrait une à 0,5 s. Le
//! cache porte le mtime dans sa clé, donc un fichier remplacé par un autre du
//! même nom est refait ; un fichier supprimé laisse juste une vignette orpheline
//! (nettoyée au prochain scan des mêmes chemins).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use image::imageops::FilterType;

use crate::recorder;

/// Largeur des vignettes (JPEG). 16:9 pour la vidéo, ratio conservé pour les
/// captures — la vue recadre en `Cover` dans une tuile de taille fixe.
pub const THUMB_WIDTH: u32 = 320;
const QUALITY: u8 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Genre {
    Image,
    Video,
}

impl Genre {
    pub fn badge(self) -> &'static str {
        match self {
            Genre::Image => "CAPTURE",
            Genre::Video => "VIDÉO",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub path: PathBuf,
    pub genre: Genre,
    pub mtime: SystemTime,
    /// Taille en octets, pour l'afficher sous la vignette.
    pub taille: u64,
}

impl Item {
    pub fn nom(&self) -> String {
        self.path
            .file_name()
            .map(|nom| nom.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Date lisible `2026-11-22 22:31`, en **heure locale** (celle que l'affiche
    /// le gestionnaire de fichiers du poste).
    pub fn date(&self) -> String {
        let secondes = self
            .mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duree| duree.as_secs())
            .unwrap_or(0);
        let (annee, mois, jour, heure, minute) = locale(secondes);
        format!("{annee:04}-{mois:02}-{jour:02} {heure:02}:{minute:02}")
    }

    pub fn poids(&self) -> String {
        if self.taille >= 1024 * 1024 {
            format!("{:.1} Mo", self.taille as f64 / (1024.0 * 1024.0))
        } else {
            format!("{} Ko", self.taille.div_ceil(1024))
        }
    }
}

/// Conversion horodatage → date **locale** par `localtime_r` (libc, déjà
/// utilisé pour `PR_SET_PDEATHSIG`) : le fuseau et l'heure d'été du poste sont
/// respectés, sans dépendance de calendrier. Repli `civil` (UTC) si `libc`
/// refuse l'instant.
fn locale(secondes: u64) -> (i64, u32, u32, u32, u32) {
    let instant = secondes as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::localtime_r(&instant, &mut tm) };
    if ok.is_null() {
        return civil(secondes);
    }
    (
        tm.tm_year as i64 + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    )
}

/// Conversion horodatage → (année, mois, jour, heure, minute), en UTC (repli).
fn civil(secondes: u64) -> (i64, u32, u32, u32, u32) {
    let jours = (secondes / 86_400) as i64;
    let reste = secondes % 86_400;
    let (heure, minute) = ((reste / 3600) as u32, ((reste % 3600) / 60) as u32);
    // algorithme civil_from_days (Howard Hinnant), sans table ni fuseau
    let z = jours + 719_468;
    let ere = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + ere * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let j = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let annee = if m <= 2 { y + 1 } else { y };
    (annee, m, j, heure, minute)
}

/// Liste des captures et vidéos des deux dossiers, la plus récente en tête.
pub fn scanne() -> Vec<Item> {
    let mut items = Vec::new();
    let shots = dossier(recorder::CLE_SHOTS, recorder::defaut_shots());
    let captures = dossier(recorder::CLE_CAPTURES, recorder::defaut_captures());
    for (dossier, prefixes, genre) in [
        (shots, &["screenshot_", "Capture"][..], Genre::Image),
        (captures, &["Video_", "Replay_"][..], Genre::Video),
    ] {
        let Ok(entrees) = std::fs::read_dir(&dossier) else {
            continue;
        };
        for entree in entrees.flatten() {
            let path = entree.path();
            let nom = entree.file_name().to_string_lossy().to_string();
            let extension = path
                .extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let bon_type = match genre {
                Genre::Image => matches!(extension.as_str(), "png" | "jpg" | "jpeg"),
                Genre::Video => matches!(extension.as_str(), "mp4" | "mkv" | "webm"),
            };
            if !bon_type || !prefixes.iter().any(|prefixe| nom.starts_with(prefixe)) {
                continue;
            }
            let Ok(meta) = entree.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            items.push(Item {
                path,
                genre,
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                taille: meta.len(),
            });
        }
    }
    items.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    items
}

/// Dossier réglé, ou défaut quand la clé est absente/vide. Le chemin est
/// développé s'il commence par `~` (un autre outil peut l'avoir écrit tel quel).
fn dossier(cle: &str, defaut: String) -> PathBuf {
    match recorder::dossier(cle) {
        Some(chemin) if chemin.starts_with("~/") => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/"));
            home.join(&chemin[2..])
        }
        Some(chemin) => PathBuf::from(chemin),
        None => PathBuf::from(defaut),
    }
}

fn cle_cache(item: &Item) -> String {
    let mut hasher = DefaultHasher::new();
    item.path.hash(&mut hasher);
    item.mtime.hash(&mut hasher);
    format!("{:016x}.jpg", hasher.finish())
}

/// Vignette de cet élément : cache d'abord, fabrication ensuite. `None` quand
/// rien ne peut être produit (fichier disparu, ffmpeg absent) — la vue dessine
/// alors une case vide, jamais une erreur.
pub fn miniature(item: &Item, cache: &Path) -> Option<Vec<u8>> {
    let chemin = cache.join(cle_cache(item));
    if let Ok(bytes) = std::fs::read(&chemin)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }
    std::fs::create_dir_all(cache).ok()?;
    match item.genre {
        Genre::Image => reduire(&item.path, &chemin),
        Genre::Video => extraire(&item.path, &chemin),
    }
    .then(|| std::fs::read(&chemin).ok())
    .flatten()
}

/// Capture d'écran : décodage complet puis réduction (JPEG qualité 80).
fn reduire(source: &Path, cible: &Path) -> bool {
    let Ok(image) = image::open(source) else {
        return false;
    };
    let largeur = image.width();
    let hauteur = image.height();
    let (l, h) = if largeur <= THUMB_WIDTH {
        (largeur, hauteur)
    } else {
        let ratio = hauteur as f32 / largeur as f32;
        (THUMB_WIDTH, (THUMB_WIDTH as f32 * ratio).round() as u32)
    };
    let reduite = image
        .resize_exact(l.max(1), h.max(1), FilterType::Triangle)
        .to_rgb8();
    let mut sortie = match std::fs::File::create(cible) {
        Ok(fichier) => fichier,
        Err(_) => return false,
    };
    let encodeur = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut sortie, QUALITY);
    reduite.write_with_encoder(encodeur).is_ok()
}

/// Vidéo : une image à 0,5 s, réduite à 320 px de large. `ffmpeg` est déjà là
/// (le son de capture de `screenshot.sh` passe par `ffplay`).
fn extraire(source: &Path, cible: &Path) -> bool {
    let statut = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-ss", "0.5", "-i"])
        .arg(source)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale={THUMB_WIDTH}:-2"),
            "-q:v",
            "4",
        ])
        .arg(cible)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    statut.is_ok_and(|statut| statut.success()) && cible.is_file()
}

/// L'horloge de la galerie tourne-t-elle ? Elle ne réveille l'UI que pendant
/// une lecture vidéo (pour voir `mpv` sortir) ou le fondu d'un aperçu.
static TICKING: AtomicBool = AtomicBool::new(false);

pub fn set_ticking(on: bool) {
    TICKING.store(on, Ordering::Relaxed);
}

/// Fil endormi qui envoie `GalleryTick` quand la galerie en a besoin. Même
/// principe que les minuteurs de la poussière et du pointeur : aucun coût au
/// repos, et un message par 125 ms (la vidéo occupe l'écran, rien à dessiner).
pub fn spawn_ticker() {
    let _ = std::thread::Builder::new()
        .name("glauncher-galerie-tick".into())
        .spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(125));
                if TICKING.load(Ordering::Relaxed) {
                    crate::app::post(crate::app::Message::GalleryTick);
                }
            }
        });
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Vignette prête (ou définitivement absente) pour ce chemin.
    Thumb { path: PathBuf, bytes: Option<Vec<u8>> },
    /// Toutes les vignettes demandées ont été traitées.
    Done,
}

/// Fabrique les vignettes manquantes sur un fil, une par une. Les appelants
/// traduisent `Event` en message ; ce module n'a aucune dépendance à l'UI.
pub fn spawn<F>(items: Vec<Item>, cache: PathBuf, emit: F)
where
    F: Fn(Event) + Send + Sync + 'static,
{
    let emit: Arc<F> = Arc::new(emit);
    let _ = std::thread::Builder::new()
        .name("glauncher-galerie".into())
        .spawn(move || {
            for item in items {
                let bytes = miniature(&item, &cache);
                emit(Event::Thumb {
                    path: item.path.clone(),
                    bytes,
                });
            }
            emit(Event::Done);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_locale_du_poste() {
        // le fuseau du banc peut être UTC ou Europe/Zurich : on vérifie la
        // cohérence (année 2026, jour 22) plutôt que l'heure exacte
        let (annee, mois, jour, _, _) = locale(1_795_386_660);
        assert_eq!((annee, mois, jour), (2026, 11, 22));
    }

    #[test]
    fn date_civile_connue() {
        // 2026-11-22 22:31 UTC
        assert_eq!(civil(1_795_386_660), (2026, 11, 22, 22, 31));
        // 1970-01-01 00:00
        assert_eq!(civil(0), (1970, 1, 1, 0, 0));
        // 2000-02-29 (année bissextile séculaire)
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0));
    }

    #[test]
    fn badges_et_poids() {
        let item = Item {
            path: PathBuf::from("/tmp/Video_20261122_223100.mp4"),
            genre: Genre::Video,
            mtime: SystemTime::UNIX_EPOCH,
            taille: 3 * 1024 * 1024,
        };
        assert_eq!(item.genre.badge(), "VIDÉO");
        assert_eq!(item.poids(), "3.0 Mo");
        assert_eq!(item.nom(), "Video_20261122_223100.mp4");
    }

    #[test]
    fn cle_cache_suit_le_mtime() {
        let mut item = Item {
            path: PathBuf::from("/tmp/a.png"),
            genre: Genre::Image,
            mtime: SystemTime::UNIX_EPOCH,
            taille: 1,
        };
        let avant = cle_cache(&item);
        item.mtime = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);
        assert_ne!(avant, cle_cache(&item), "fichier remplacé = vignette refaite");
    }
}
