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

    /// Date sans l'année quand c'est l'année courante (« 26 nov. · 23:31 ») :
    /// sur une tuile de galerie, trois caractères gagnés valent mieux qu'un
    /// « 2026 » que tout le monde connaît. Année différente = date complète.
    pub fn date_courte(&self, maintenant: SystemTime) -> String {
        let secondes = |instant: SystemTime| {
            instant
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duree| duree.as_secs())
                .unwrap_or(0)
        };
        let (annee, mois, jour, heure, minute) = locale(secondes(self.mtime));
        if annee == locale(secondes(maintenant)).0 {
            format!("{jour} {} · {heure:02}:{minute:02}", mois_abrege(mois))
        } else {
            self.date_lisible()
        }
    }

    pub fn poids(&self) -> String {
        if self.taille >= 1024 * 1024 {
            format!("{:.1} Mo", self.taille as f64 / (1024.0 * 1024.0))
        } else {
            format!("{} Ko", self.taille.div_ceil(1024))
        }
    }
}

/// Section datée de chaque élément (intertitres de la grille). Calculée en une
/// passe au chargement de la galerie : la vue ne rappelle pas `localtime_r`
/// pour chaque tuile à chaque image.
pub fn sections(items: &[Item], maintenant: SystemTime) -> Vec<&'static str> {
    let aujourdhui = jour_local(maintenant);
    items
        .iter()
        .map(|item| section_jours(jour_local(item.mtime), aujourdhui))
        .collect()
}

/// « Aujourd'hui / Hier / Cette semaine / Ce mois-ci / Plus ancien » sur des
/// jours ordinaux **locaux** (un changement d'heure ne décale pas la veille).
fn section_jours(jour: i64, aujourdhui: i64) -> &'static str {
    // horloge en avance (capture « du futur ») : aujourd'hui, pas « plus
    // ancien » — la soustraction saturée donnait un écart négatif
    if aujourdhui <= jour {
        return "Aujourd'hui";
    }
    match aujourdhui - jour {
        1 => "Hier",
        2..=6 => "Cette semaine",
        7..=29 => "Ce mois-ci",
        _ => "Plus ancien",
    }
}

/// Jour ordinal local d'un instant (jours depuis l'époque Unix).
fn jour_local(instant: SystemTime) -> i64 {
    let secondes = instant
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duree| duree.as_secs())
        .unwrap_or(0);
    let (annee, mois, jour, _, _) = locale(secondes);
    jours_civils(annee, mois, jour)
}

/// `days_from_civil` (Howard Hinnant) : date civile → jour ordinal.
fn jours_civils(annee: i64, mois: u32, jour: u32) -> i64 {
    let y = if mois <= 2 { annee - 1 } else { annee };
    let ere = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - ere * 400;
    let mp = (i64::from(mois) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(jour) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    ere * 146_097 + doe - 719_468
}

/// Durée d'une vidéo, en secondes : cache `<hash>.dur` d'abord, sinon demandée
/// à `ffmpeg` — en-tête seulement (`-i` sans sortie, aucun décodage), donc
/// quelques millisecondes. Seules les vidéos visibles passent par là (le fil de
/// vignettes), et le résultat est retenu pour les sessions suivantes.
pub fn duree(item: &Item, cache: &Path) -> Option<f64> {
    if item.genre != Genre::Video {
        return None;
    }
    let chemin = cache.join(format!(
        "{}.dur",
        cle_cache(item).trim_end_matches(".jpg")
    ));
    if let Ok(texte) = std::fs::read_to_string(&chemin)
        && let Ok(secondes) = texte.trim().parse::<f64>()
        && secondes.is_finite()
    {
        return Some(secondes);
    }
    let sortie = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-i"])
        .arg(&item.path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;
    let texte = String::from_utf8_lossy(&sortie.stderr);
    let secondes = texte.lines().find_map(parse_duree)?;
    std::fs::create_dir_all(cache).ok()?;
    let _ = std::fs::write(&chemin, format!("{secondes:.3}"));
    Some(secondes)
}

/// `Duration: 00:01:12.50, start: …` → secondes. `N/A` (flux sans durée) ne
/// donne rien : la pastille reste absente plutôt que fausse.
fn parse_duree(ligne: &str) -> Option<f64> {
    let reste = ligne.split("Duration:").nth(1)?.trim();
    let duree = reste.split(',').next()?.trim();
    let mut parties = duree.split(':');
    let heures: f64 = parties.next()?.parse().ok()?;
    let minutes: f64 = parties.next()?.parse().ok()?;
    let secondes: f64 = parties.next()?.parse().ok()?;
    Some(heures * 3600.0 + minutes * 60.0 + secondes)
}

/// Durée courte d'une pastille : `0:20`, `1:02:03`.
pub fn duree_texte(secondes: f64) -> String {
    let total = secondes.max(0.0) as u64;
    let (heures, minutes, secondes) = (total / 3600, (total % 3600) / 60, total % 60);
    if heures > 0 {
        format!("{heures}:{minutes:02}:{secondes:02}")
    } else {
        format!("{minutes}:{secondes:02}")
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
        return metadonnees_image(path);
    }
    (None, None)
}

/// Nom et logo lus **dans l'image elle-même** (demandé le 27/11 : aucun fichier
/// à côté d'une capture) : iTXt `Title`/`Logo` d'un PNG, commentaire
/// `Title=`/`Logo=` d'un JPEG. Le format est reconnu aux **octets magiques**,
/// pas à l'extension — `gpad-shot` écrit du JPEG par défaut dans un fichier
/// nommé `.png`, et une capture JPEG n'a pas de morceau de texte, d'où le
/// commentaire.
fn metadonnees_image(path: &Path) -> (Option<String>, Option<PathBuf>) {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return (None, None);
    };
    let mut sigle = [0u8; 8];
    if f.read_exact(&mut sigle).is_err() {
        return (None, None);
    }
    if sigle == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return metadonnees_png(&mut f);
    }
    if sigle[0] == 0xff && sigle[1] == 0xd8 {
        // retour au début : le balayage des marqueurs repart de SOI
        return metadonnees_jpeg(path);
    }
    (None, None)
}

/// Une paire `clé\0valeur` (tEXt).
fn cle_valeur(data: &[u8]) -> Option<(String, String)> {
    let fin = data.iter().position(|octet| *octet == 0)?;
    Some((
        String::from_utf8_lossy(&data[..fin]).to_string(),
        String::from_utf8_lossy(&data[fin + 1..]).to_string(),
    ))
}

/// iTXt : `clé\0 compression méthode langue\0 traduit\0 texte` (les deux
/// derniers champs vont jusqu'au NUL, le texte occupe le reste). Le drapeau de
/// compression est un **octet**, pas un champ : une première version le lisait
/// par `splitn`, ce qui ne tombait jamais juste et rendait le titre illisible.
fn itxt(data: &[u8]) -> Option<(String, String)> {
    let fin = data.iter().position(|octet| *octet == 0)?;
    let cle = String::from_utf8_lossy(&data[..fin]).to_string();
    let reste = data.get(fin + 1..)?;
    let (&compression, apres) = reste.split_first()?;
    let (_methode, apres) = apres.split_first()?;
    let fin = apres.iter().position(|octet| *octet == 0)?;
    let apres = apres.get(fin + 1..)?;
    let fin = apres.iter().position(|octet| *octet == 0)?;
    if compression != 0 {
        return None; // texte compressé : on ne décompresse pas pour un titre
    }
    Some((
        cle,
        String::from_utf8_lossy(apres.get(fin + 1..)?).to_string(),
    ))
}

/// Morceaux d'un PNG jusqu'aux données : `iTXt` et `tEXt` portent le nom et le
/// logo. Un fichier quelconque n'est pas une erreur, juste « pas de titre ».
fn metadonnees_png(f: &mut std::fs::File) -> (Option<String>, Option<PathBuf>) {
    use std::io::Read;
    let (mut titre, mut logo) = (None, None);
    let mut entete = [0u8; 8];
    for _ in 0..32 {
        if f.read_exact(&mut entete).is_err() {
            break;
        }
        let len = u32::from_be_bytes([entete[0], entete[1], entete[2], entete[3]]) as usize;
        let type_ = &entete[4..8];
        if type_ == b"IDAT" || type_ == b"IEND" {
            break;
        }
        if len > 8192 {
            // morceau inattendu : on ne saute pas 4 Mo pour rien
            break;
        }
        let mut data = vec![0u8; len];
        if f.read_exact(&mut data).is_err() {
            break;
        }
        let mut crc = [0u8; 4];
        if f.read_exact(&mut crc).is_err() {
            break;
        }
        let champ = if type_ == b"iTXt" {
            itxt(&data)
        } else if type_ == b"tEXt" {
            cle_valeur(&data)
        } else {
            None
        };
        for (cle, valeur) in champ.into_iter().collect::<Vec<_>>() {
            let valeur = valeur.trim().to_string();
            if valeur.is_empty() {
                continue;
            }
            match cle.as_str() {
                "Title" => titre = Some(valeur),
                "Logo" => logo = Some(PathBuf::from(valeur)),
                _ => {}
            }
        }
    }
    (titre, logo)
}

/// Marqueurs d'un JPEG jusqu'au début du scan : le commentaire `COM` porte
/// `Title=`/`Logo=` (une ligne par clé). Les captures d'avant le 27/11 n'y
/// mettaient que le titre nu : il est reconnu comme tel.
fn metadonnees_jpeg(path: &Path) -> (Option<String>, Option<PathBuf>) {
    use std::io::{BufReader, Read};
    let (mut titre, mut logo) = (None, None);
    let Ok(f) = std::fs::File::open(path) else {
        return (None, None);
    };
    let mut lecteur = BufReader::new(f);
    let mut octets = [0u8; 2];
    if lecteur.read_exact(&mut octets).is_err() || octets != [0xff, 0xd8] {
        return (None, None);
    }
    loop {
        // remplissage 0xFF éventuel, puis le marqueur
        let mut marqueur = [0u8; 1];
        if lecteur.read_exact(&mut marqueur).is_err() {
            break;
        }
        if marqueur[0] != 0xff {
            break; // flux incohérent : on s'arrête là
        }
        let mut code = [0u8; 1];
        while lecteur.read_exact(&mut code).is_ok() && code[0] == 0xff {}
        let code = code[0];
        // marqueurs sans longueur : SOI, EOI, RSTn, TEM
        if code == 0xd8 || code == 0xd9 || code == 0x01 || (0xd0..=0xd7).contains(&code) {
            if code == 0xd9 {
                break;
            }
            continue;
        }
        let mut longueur = [0u8; 2];
        if lecteur.read_exact(&mut longueur).is_err() {
            break;
        }
        let longueur = u16::from_be_bytes(longueur) as usize;
        if longueur < 2 {
            break;
        }
        let mut contenu = vec![0u8; longueur - 2];
        if lecteur.read_exact(&mut contenu).is_err() {
            break;
        }
        if code == 0xfe {
            // commentaire : `Clé=valeur` par ligne, ou le titre nu (ancien)
            let texte = String::from_utf8_lossy(&contenu).to_string();
            let mut structure = false;
            for ligne in texte.lines() {
                let ligne = ligne.trim();
                if let Some(valeur) = ligne.strip_prefix("Title=") {
                    structure = true;
                    if !valeur.trim().is_empty() {
                        titre = Some(valeur.trim().to_string());
                    }
                } else if let Some(valeur) = ligne.strip_prefix("Logo=") {
                    structure = true;
                    if !valeur.trim().is_empty() {
                        logo = Some(PathBuf::from(valeur.trim()));
                    }
                }
            }
            if !structure && titre.is_none() {
                let texte = texte.trim();
                if !texte.is_empty() {
                    titre = Some(texte.to_string());
                }
            }
        }
        // SOS : plus aucune métadonnée après, le flux image commence
        if code == 0xda {
            break;
        }
    }
    (titre, logo)
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
    /// Durée d'une vidéo (lue/cachée à part, pour la pastille de la tuile).
    Duree { path: PathBuf, secondes: Option<f64> },
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
                // la durée ne concerne que les vidéos : même fil, même lot de
                // tuiles visibles, donc pas de `ffmpeg -i` sur toute la galerie
                if item.genre == Genre::Video {
                    let secondes = duree(&item, &cache);
                    emit(Event::Duree {
                        path: item.path.clone(),
                        secondes,
                    });
                }
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

    #[test]
    fn sections_datees() {
        assert_eq!(section_jours(100, 100), "Aujourd'hui");
        assert_eq!(section_jours(100, 101), "Hier");
        assert_eq!(section_jours(100, 104), "Cette semaine");
        assert_eq!(section_jours(100, 120), "Ce mois-ci");
        assert_eq!(section_jours(100, 200), "Plus ancien");
        // horloge en avance : une capture « du futur » reste aujourd'hui
        assert_eq!(section_jours(101, 100), "Aujourd'hui");
    }

    #[test]
    fn jours_civils_connus() {
        // 1970-01-01 = jour 0 ; 2000-02-29 = 11016
        assert_eq!(jours_civils(1970, 1, 1), 0);
        assert_eq!(jours_civils(2000, 2, 29), 11_016);
        assert_eq!(jours_civils(2026, 11, 22), 20_779);
    }

    #[test]
    fn duree_lue_dans_l_entete_ffmpeg() {
        assert_eq!(
            parse_duree("  Duration: 00:01:12.50, start: 0.000000, bitrate: 1 kb/s"),
            Some(72.5)
        );
        assert_eq!(parse_duree("  Duration: 00:00:20.00, start: 0"), Some(20.0));
        // flux sans durée : pas de pastille plutôt qu'une valeur fausse
        assert_eq!(parse_duree("  Duration: N/A, start: 0"), None);
        assert_eq!(parse_duree("Stream #0:0: Video"), None);
    }

    /// Bac à sable d'un test : un dossier par nom, vidé avant usage.
    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("galerie-test-{}-{nom}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// PNG minimal : signature, IHDR, les iTXt demandés, puis IDAT/IEND (le
    /// lecteur s'arrête là, les CRC ne sont donc jamais relus).
    fn png_avec(paires: &[(&str, &str)]) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut morceau = |type_: &[u8; 4], data: &[u8]| {
            v.extend_from_slice(&(data.len() as u32).to_be_bytes());
            v.extend_from_slice(type_);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0, 0, 0, 0]);
        };
        morceau(b"IHDR", &[0; 13]);
        for (cle, texte) in paires {
            let mut data = cle.as_bytes().to_vec();
            data.push(0); // fin de clé
            data.extend_from_slice(&[0, 0, 0, 0]); // non compressé, langue, traduit
            data.extend_from_slice(texte.as_bytes());
            morceau(b"iTXt", &data);
        }
        morceau(b"IDAT", &[0; 4]);
        morceau(b"IEND", &[]);
        v
    }

    /// JPEG minimal : SOI, un commentaire, puis SOS/EOI.
    fn jpeg_avec_commentaire(texte: &str) -> Vec<u8> {
        let mut v = vec![0xff, 0xd8, 0xff, 0xfe];
        v.extend_from_slice(&((texte.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(texte.as_bytes());
        v.extend_from_slice(&[0xff, 0xda, 0x00, 0x02, 0xff, 0xd9]);
        v
    }

    #[test]
    fn metadonnees_png_titre_et_logo() {
        let dir = bac("meta-png");
        let f = dir.join("screenshot_20261127_120000.png");
        std::fs::write(
            &f,
            png_avec(&[("Title", "Astro Bot"), ("Logo", "/tmp/logo.png")]),
        )
        .unwrap();
        let (titre, logo) = metadonnees_image(&f);
        assert_eq!(titre.as_deref(), Some("Astro Bot"));
        assert_eq!(logo, Some(PathBuf::from("/tmp/logo.png")));
    }

    #[test]
    fn metadonnees_png_sans_logo() {
        let dir = bac("meta-png-seul");
        let f = dir.join("screenshot_20261127_120000.png");
        std::fs::write(&f, png_avec(&[("Title", "Hades II")])).unwrap();
        let (titre, logo) = metadonnees_image(&f);
        assert_eq!(titre.as_deref(), Some("Hades II"));
        assert!(logo.is_none(), "pas de logo dans l'image");
    }

    #[test]
    fn metadonnees_jpeg_commentaire_structure() {
        let dir = bac("meta-jpeg");
        // `gpad-shot` écrit du JPEG par défaut, dans un fichier nommé `.png` :
        // c'est le contenu qui décide, pas l'extension
        let f = dir.join("screenshot_20261127_120000.png");
        std::fs::write(
            &f,
            jpeg_avec_commentaire("Title=Astro Bot\nLogo=/tmp/logo.png"),
        )
        .unwrap();
        let (titre, logo) = metadonnees_image(&f);
        assert_eq!(titre.as_deref(), Some("Astro Bot"));
        assert_eq!(logo, Some(PathBuf::from("/tmp/logo.png")));
    }

    #[test]
    fn metadonnees_jpeg_ancien_titre_nu() {
        // captures d'avant le 27/11 : le commentaire ne portait que le titre
        let dir = bac("meta-jpeg-vieux");
        let f = dir.join("screenshot_20261120_101500.jpg");
        std::fs::write(&f, jpeg_avec_commentaire("Elden Ring")).unwrap();
        let (titre, logo) = metadonnees_image(&f);
        assert_eq!(titre.as_deref(), Some("Elden Ring"));
        assert!(logo.is_none());
    }

    #[test]
    fn metadonnees_fichier_illisible() {
        let dir = bac("meta-vide");
        let f = dir.join("screenshot_20261127_120000.png");
        std::fs::write(&f, b"pas une image").unwrap();
        assert_eq!(metadonnees_image(&f), (None, None));
        assert_eq!(
            metadonnees_image(&dir.join("absent.png")),
            (None, None),
            "fichier absent : aucune erreur"
        );
    }

    #[test]
    fn duree_courte() {
        assert_eq!(duree_texte(0.0), "0:00");
        assert_eq!(duree_texte(20.4), "0:20");
        assert_eq!(duree_texte(72.0), "1:12");
        assert_eq!(duree_texte(3723.0), "1:02:03");
    }
}
