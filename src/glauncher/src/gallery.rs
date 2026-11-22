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
    /// Nom du jeu au moment de la capture (`None` : capture d'avant cette
    /// métadonnée, ou faite hors jeu). Lu dans le sidecar `<fichier>.jeu` ou,
    /// à défaut, dans le `iTXt Title` du PNG.
    pub titre: Option<String>,
    /// Logo du jeu (chemin d'un PNG transparent), pour la copie partageable.
    pub logo: Option<PathBuf>,
}

impl Item {
    pub fn nom(&self) -> String {
        self.path
            .file_name()
            .map(|nom| nom.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Date **lisible** : c'est le seul libellé d'une tuile (le nom du fichier
    /// ne dit rien à personne). `22 nov. 2026 · 22:31`.
    pub fn date_lisible(&self) -> String {
        let secondes = self
            .mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duree| duree.as_secs())
            .unwrap_or(0);
        let (annee, mois, jour, heure, minute) = locale(secondes);
        format!(
            "{jour} {} {annee} · {heure:02}:{minute:02}",
            mois_abrege(mois)
        )
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

/// Mois abrégé en français, tel qu'on l'écrit dans une date courte.
fn mois_abrege(mois: u32) -> &'static str {
    const MOIS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
        "nov.", "déc.",
    ];
    MOIS.get(mois.saturating_sub(1) as usize).copied().unwrap_or("")
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
            let (titre, logo) = legende_capture(&path, genre);
            items.push(Item {
                path,
                genre,
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                taille: meta.len(),
                titre,
                logo,
            });
        }
    }
    items.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    items
}

/// Nom du jeu et logo d'une capture : le **sidecar** `<fichier>.jeu` d'abord
/// (écrit par `screenshot.sh` / `recorder.sh` / `replay.sh`), puis, pour une
/// image plus ancienne, la métadonnée `iTXt`/`tEXt` `Title` du PNG. Aucune
/// lecture du média au-delà des quelques octets qui précèdent les données.
fn legende_capture(path: &Path, genre: Genre) -> (Option<String>, Option<PathBuf>) {
    let sidecar = PathBuf::from(format!("{}.jeu", path.display()));
    if let Ok(texte) = std::fs::read_to_string(&sidecar) {
        let champ = |cle: &str| {
            texte
                .lines()
                .find_map(|ligne| ligne.strip_prefix(&format!("{cle}=")))
                .map(|valeur| valeur.trim().to_string())
                .filter(|valeur| !valeur.is_empty())
        };
        let logo = champ("logo").map(PathBuf::from);
        return (champ("nom"), logo);
    }
    if genre == Genre::Image {
        return (titre_png(path), None);
    }
    (None, None)
}

/// `Title` d'un PNG, sans décoder l'image : on lit les morceaux jusqu'à
/// `IDAT` (les métadonnées sont avant, toujours chez nous). Un fichier
/// quelconque n'est pas une erreur, juste « pas de titre ».
fn titre_png(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut sigle = [0u8; 8];
    f.read_exact(&mut sigle).ok()?;
    if sigle != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return None;
    }
    let mut entete = [0u8; 8];
    for _ in 0..32 {
        f.read_exact(&mut entete).ok()?;
        let len = u32::from_be_bytes([entete[0], entete[1], entete[2], entete[3]]) as usize;
        let type_ = &entete[4..8];
        if type_ == b"IDAT" || type_ == b"IEND" {
            return None;
        }
        if len > 8192 {
            // morceau inattendu : on ne saute pas 4 Mo pour rien
            return None;
        }
        let mut data = vec![0u8; len];
        f.read_exact(&mut data).ok()?;
        let mut crc = [0u8; 4];
        f.read_exact(&mut crc).ok()?;
        if type_ == b"iTXt" {
            // clé\0 flag\0 méthode\0 langue\0 traduit\0 texte
            let champs: Vec<&[u8]> = data.splitn(6, |octet| *octet == 0).collect();
            if champs.len() == 6 && champs[0] == b"Title" && champs[1] == [0] {
                return Some(String::from_utf8_lossy(champs[5]).to_string());
            }
        } else if type_ == b"tEXt" {
            let mut parts = data.splitn(2, |octet| *octet == 0);
            if let (Some(cle), Some(valeur)) = (parts.next(), parts.next())
                && cle == b"Title"
            {
                return Some(String::from_utf8_lossy(valeur).to_string());
            }
        }
    }
    None
}

/// Met un fichier à la **corbeille XDG** (`~/.local/share/Trash`) au lieu de
/// l'effacer (demandé le 23/11) : le fichier et son `.trashinfo` sont posés
/// dans les sous-dossiers de la corbeille, récupérables depuis Dolphin.
///
/// Aucune suppression définitive : si le rangement échoue (disque plein,
/// corbeille inaccessible), l'erreur remonte et le fichier reste en place.
pub fn mettre_a_la_corbeille(path: &Path) -> Result<PathBuf, String> {
    let racine = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })
        .ok_or_else(|| "ni XDG_DATA_HOME ni HOME : corbeille introuvable".to_string())?
        .join("Trash");
    let fichiers = racine.join("files");
    let infos = racine.join("info");
    std::fs::create_dir_all(&fichiers)
        .and_then(|_| std::fs::create_dir_all(&infos))
        .map_err(|erreur| format!("corbeille indisponible : {erreur}"))?;

    let nom = path
        .file_name()
        .map(|nom| nom.to_string_lossy().to_string())
        .filter(|nom| !nom.is_empty())
        .ok_or_else(|| "fichier sans nom".to_string())?;
    let (tige, extension) = match nom.rsplit_once('.') {
        Some((tige, extension)) if !tige.is_empty() => (tige.to_string(), format!(".{extension}")),
        _ => (nom.clone(), String::new()),
    };
    // nom déjà pris dans la corbeille : suffixe `.1`, `.2`… comme le fait gio
    let mut cible = fichiers.join(&nom);
    let mut n = 1u32;
    while cible.exists() || infos.join(format!("{}.trashinfo", cible.file_name().unwrap().to_string_lossy())).exists() {
        cible = fichiers.join(format!("{tige}.{n}{extension}"));
        n += 1;
        if n > 9999 {
            return Err("trop de doublons dans la corbeille".to_string());
        }
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let info = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encoder_chemin(&absolute.to_string_lossy()),
        horodatage_maintenant()
    );
    let nom_cible = cible
        .file_name()
        .map(|nom| nom.to_string_lossy().to_string())
        .unwrap_or_default();
    let info_path = infos.join(format!("{nom_cible}.trashinfo"));
    let info_tmp = infos.join(format!("{nom_cible}.trashinfo.part"));
    std::fs::write(&info_tmp, info)
        .and_then(|_| std::fs::rename(&info_tmp, &info_path))
        .map_err(|erreur| format!("écriture du .trashinfo impossible : {erreur}"))?;
    if let Err(erreur) = std::fs::rename(&absolute, &cible) {
        let _ = std::fs::remove_file(&info_path);
        return Err(format!("déplacement vers la corbeille impossible : {erreur}"));
    }
    Ok(cible)
}

/// Échappement RFC 2396 du chemin dans `.trashinfo` (`/` reste littéral).
fn encoder_chemin(chemin: &str) -> String {
    let mut sortie = String::with_capacity(chemin.len() + 8);
    for octet in chemin.bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                sortie.push(octet as char)
            }
            _ => sortie.push_str(&format!("%{octet:02X}")),
        }
    }
    sortie
}

/// `DeletionDate` au format `AAAA-MM-JJThh:mm:ss` (heure **locale**).
fn horodatage_maintenant() -> String {
    let secondes = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duree| duree.as_secs())
        .unwrap_or(0);
    let instant = secondes as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&instant, &mut tm) }.is_null() {
        let (annee, mois, jour, heure, minute) = civil(secondes);
        return format!(
            "{annee:04}-{mois:02}-{jour:02}T{heure:02}:{minute:02}:{:02}",
            secondes % 60
        );
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// Copie partageable d'une capture : le screenshot avec le **logo du jeu**
/// incrusté en bas à droite (18 % de la largeur, marge de 2,5 %). L'original
/// n'est jamais modifié. Le sidecar `.jeu` est recopié pour que la copie garde
/// son nom de jeu dans la galerie ; elle s'appelle `<original>-logo.png`.
pub fn copie_avec_logo(item: &Item) -> Result<PathBuf, String> {
    if item.genre != Genre::Image {
        return Err("la copie avec logo ne concerne que les captures".to_string());
    }
    let logo = item
        .logo
        .as_ref()
        .filter(|chemin| chemin.is_file())
        .ok_or_else(|| {
            "aucun logo connu pour cette capture (prise hors jeu, ou visuel disparu)"
                .to_string()
        })?;
    let mut fond = image::open(&item.path)
        .map_err(|erreur| format!("capture illisible : {erreur}"))?
        .to_rgba8();
    let marque = image::open(logo)
        .map_err(|erreur| format!("logo illisible : {erreur}"))?
        .to_rgba8();
    let (largeur, hauteur) = fond.dimensions();
    if largeur == 0 || hauteur == 0 {
        return Err("capture vide".to_string());
    }
    // le logo reste un sceau discret : 18 % de la largeur, et jamais plus de
    // 30 % de la hauteur pour un logo très vertical
    let visee = (largeur as f32 * 0.18).round().max(48.0) as u32;
    let ratio = marque.height() as f32 / marque.width().max(1) as f32;
    let mut lw = visee.min(largeur).max(1);
    let mut lh = (lw as f32 * ratio).round().max(1.0) as u32;
    if lh as f32 > hauteur as f32 * 0.30 {
        lh = (hauteur as f32 * 0.30).round().max(1.0) as u32;
        lw = (lh as f32 / ratio.max(0.01)).round().max(1.0) as u32;
    }
    let marque = image::imageops::resize(&marque, lw, lh, FilterType::Lanczos3);
    let marge = ((largeur as f32 * 0.025).round() as u32).max(12);
    let x = largeur.saturating_sub(lw + marge);
    let y = hauteur.saturating_sub(lh + marge);
    image::imageops::overlay(&mut fond, &marque, x as i64, y as i64);

    let tige = item
        .path
        .file_stem()
        .map(|tige| tige.to_string_lossy().to_string())
        .unwrap_or_else(|| "capture".to_string());
    let sortie = item.path.with_file_name(format!("{tige}-logo.png"));
    // écriture atomique : une copie interrompue ne laisse pas un PNG tronqué
    let temporaire = item.path.with_file_name(format!("{tige}-logo.png.part"));
    fond.save_with_format(&temporaire, image::ImageFormat::Png)
        .map_err(|erreur| format!("écriture impossible : {erreur}"))?;
    std::fs::rename(&temporaire, &sortie)
        .map_err(|erreur| format!("rangement impossible : {erreur}"))?;
    let sidecar = PathBuf::from(format!("{}.jeu", item.path.display()));
    if sidecar.is_file() {
        let _ = std::fs::copy(&sidecar, format!("{}.jeu", sortie.display()));
    }
    Ok(sortie)
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
    fn date_lisible_francaise() {
        let item = Item {
            path: PathBuf::from("/tmp/screenshot_1.png"),
            genre: Genre::Image,
            mtime: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_795_386_660),
            taille: 1,
            titre: None,
            logo: None,
        };
        let date = item.date_lisible();
        // « 22 nov. 2026 · HH:MM » — l'heure dépend du fuseau du poste
        assert!(date.starts_with("22 nov. 2026 · "), "{date}");
        assert_eq!(date.len(), "22 nov. 2026 · 00:00".len());
    }

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
            titre: None,
            logo: None,
        };
        assert_eq!(item.genre.badge(), "VIDÉO");
        assert_eq!(item.poids(), "3.0 Mo");
        assert_eq!(item.nom(), "Video_20261122_223100.mp4");
    }

    #[test]
    fn chemin_corbeille_echappe() {
        // le .trashinfo est un fichier URL-encodé ; les espaces et accents
        // doivent être échappés, les `/` rester littéraux
        assert_eq!(
            encoder_chemin("/home/gab/Images/Screenshots/a b é.png"),
            "/home/gab/Images/Screenshots/a%20b%20%C3%A9.png"
        );
    }

    #[test]
    fn cle_cache_suit_le_mtime() {
        let mut item = Item {
            path: PathBuf::from("/tmp/a.png"),
            genre: Genre::Image,
            mtime: SystemTime::UNIX_EPOCH,
            taille: 1,
            titre: None,
            logo: None,
        };
        let avant = cle_cache(&item);
        item.mtime = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);
        assert_ne!(avant, cle_cache(&item), "fichier remplacé = vignette refaite");
    }
}
