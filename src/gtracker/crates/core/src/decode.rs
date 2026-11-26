use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{bail, Context, Result};

use crate::model::Track;
use crate::util::is_vgm_file;

#[derive(Clone)]
pub struct Decoded {
    pub pcm: Arc<Vec<f32>>,
    pub channels: u16,
    pub rate: u32,
    pub total: f64,
    /// Fenêtre de boucle issue du format ou détectée dans le rendu, en secondes.
    pub window: Option<(f64, f64)>,
    /// Nombre de tours trouvés dans le rendu (PSF sans information de boucle).
    pub window_passes: Option<u32>,
    /// Tempo estimé du rendu : grille de temps pour découper une boucle
    /// musicale (A/B aimanté, boucle d'une mesure). `None` = pas de rythme
    /// exploitable.
    /// Rempli **à la demande** par `analyse_tempo` (mode Découpe) : `None` au
    /// décodage, pour que la lecture ne paie pas l'analyse du rythme.
    pub tempo: Option<crate::tempo::Tempo>,
    /// Enveloppe d'attaque du rendu entier : recale chaque frontière de coupe
    /// sur une attaque réelle (corrige la dérive d'un BPM estimé au plus juste).
    pub envelope: Option<crate::tempo::Envelope>,
    /// Forme d'onde (min/max par tranche) pour l'afficher dans la vue Découpe :
    /// un tempo faux ne se voit qu'en regardant où tombent les temps (vécu 21/11).
    pub peaks: Arc<Vec<(f32, f32)>>,
}

/// Nombre de tranches de la forme d'onde affichée (~1 px par tranche sur 2000 px).
const PEAK_BUCKETS: usize = 2400;

/// Min/max par tranche, en mono. Coût négligeable devant le décodage, mémoire
/// fixe (2400 paires de f32 ≈ 19 Ko quel que soit le morceau).
fn peaks_of(pcm: &[f32], channels: u16, buckets: usize) -> Arc<Vec<(f32, f32)>> {
    let ch = channels.max(1) as usize;
    let frames = pcm.len() / ch;
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(buckets);
    if frames == 0 || buckets == 0 {
        return Arc::new(out);
    }
    for b in 0..buckets {
        let i0 = frames * b / buckets;
        let i1 = (frames * (b + 1) / buckets).max(i0 + 1).min(frames);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for f in i0..i1 {
            let v: f32 = pcm[f * ch..f * ch + ch].iter().sum::<f32>() / ch as f32;
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        out.push((lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0)));
    }
    Arc::new(out)
}

/// Décode une piste (zxtune ou vgmstream), avec cache **en mémoire**.
///
/// Rien n'est gardé sur le disque : le rendu passe par un fichier temporaire
/// dans `/dev/shm` (RAM) aussitôt relu et supprimé, et les échantillons restent
/// en mémoire pour les relectures (plafond `GTRACKER_RAM_MB`, 512 Mo par
/// défaut). Un cache disque d'un WAV par piste jouée remplissait 1,7 Go pour
/// 47 pistes (vécu 20/11) pour un gain nul : rejouer une piste redécode en
/// ~1 s, et l'analyse de boucle reste, elle, dans l'entrée mémoire.
pub fn decode(zxtune: &Path, track: &Track) -> Result<Decoded> {
    let key = crate::util::hash_path(&track.file, track.subsong as u64);
    if let Some(d) = ram_get(&key) {
        return Ok(d);
    }
    let d = decode_uncached(zxtune, track)?;
    ram_put(key, d.clone());
    Ok(d)
}

/// Chaîne complète de détection du rythme d'un rendu déjà décodé : réseau
/// (`beats`, positions de temps réelles), puis enveloppe d'attaque, puis pouls
/// faible, puis grille régulière de secours. **Jamais** appelée au décodage :
/// le réseau coûte 1 à 3 s, la lecture ne doit pas l'attendre.
pub fn analyse_tempo(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    total: f64,
    window: Option<(f64, f64)>,
) -> crate::tempo::Tempo {
    let base = crate::beats::detect(pcm, channels, rate, total, window)
        .or_else(|| crate::tempo::detect(pcm, channels, rate, total))
        .or_else(|| crate::tempo::detect_weak(pcm, channels, rate, total))
        .unwrap_or_else(crate::tempo::Tempo::fallback);
    // Dernier recalage sur les attaques : la détection peut poser la grille un
    // demi-temps à côté (vécu 26/11 : « toutes les séquences ont un bout en trop
    // à la fin »). **Pas** quand le format déclare son point de boucle : ce
    // point est le bon ancrage par construction (PSF, VGM, genh), seule la
    // découpe le raffine de ±60 ms (`fit_grid`).
    if window.is_some() {
        return base;
    }
    match crate::tempo::envelope(pcm, channels, rate) {
        Some(env) => crate::tempo::pulse(&env, &base),
        None => base,
    }
}

/// Mémorise la grille dans l'entrée mémoire de la piste : revenir sur le mode
/// Découpe ne relance pas l'analyse (elle reste hors du cache disque, comme la
/// boucle : un tempo dépend du rendu, pas du fichier).
pub fn remember_tempo(track: &Track, tempo: crate::tempo::Tempo) {
    let key = crate::util::hash_path(&track.file, track.subsong as u64);
    if let Ok(mut list) = ram().lock() {
        if let Some(e) = list.iter_mut().find(|(k, _)| *k == key) {
            e.1.tempo = Some(tempo);
        }
    }
}

/// Dossier de rendu en RAM : zxtune et vgmstream n'écrivent que dans un
/// fichier, mais il ne doit rien rester sur le disque.
fn scratch_dir() -> PathBuf {
    let base = if Path::new("/dev/shm").is_dir() {
        PathBuf::from("/dev/shm")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("gtracker-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Supprime le WAV de rendu **et** le dossier s'il est vide : rien ne doit
/// rester dans `/dev/shm`, pas même un dossier vide par processus.
fn scratch_cleanup(wav: &Path) {
    let _ = std::fs::remove_file(wav);
    if let Some(dir) = wav.parent() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Nom de fichier de rendu unique (le worker décode une piste à la fois, mais
/// deux rendus dans le même processus ne doivent pas se marcher dessus).
fn scratch_name(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    scratch_dir().join(format!("{tag}-{n}.wav"))
}

/// Cache mémoire des pistes décodées, LRU, plafonné en octets de PCM.
const RAM_CAP_MB: usize = 1024;

static RAM: OnceLock<Mutex<Vec<(String, Decoded)>>> = OnceLock::new();
/// Sources du projet ouvert : jamais évincées (une éviction silencieuse
/// couperait la lecture en plein milieu).
static PINNED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

fn pinned() -> &'static Mutex<std::collections::HashSet<String>> {
    PINNED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Épingle une source (chemin + sous-morceau) : son PCM reste en RAM tant que
/// le projet la référence.
pub fn pin(file: &std::path::Path, subsong: usize) {
    let key = crate::util::hash_path(file, subsong as u64);
    if let Ok(mut set) = pinned().lock() {
        set.insert(key);
    }
}

/// Relâche toutes les épingles (nouveau projet, projet fermé).
pub fn unpin_all() {
    if let Ok(mut set) = pinned().lock() {
        set.clear();
    }
}

fn ram() -> &'static Mutex<Vec<(String, Decoded)>> {
    RAM.get_or_init(|| Mutex::new(Vec::new()))
}

fn ram_cap() -> usize {
    std::env::var("GTRACKER_RAM_MB")
        // ancien nom, gardé : une variable déjà exportée sur le poste ne doit
        // pas être ignorée en silence
        .or_else(|_| std::env::var("RETROPLAYER_RAM_MB"))
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(RAM_CAP_MB)
        * 1024
        * 1024
}

fn ram_get(key: &str) -> Option<Decoded> {
    let mut list = ram().lock().ok()?;
    let pos = list.iter().position(|(k, _)| k == key)?;
    let entry = list.remove(pos);
    let d = entry.1.clone();
    list.push(entry); // le plus récent en fin de liste
    Some(d)
}

fn ram_put(key: String, d: Decoded) {
    let Ok(mut list) = ram().lock() else {
        return;
    };
    list.retain(|(k, _)| k != &key);
    list.push((key, d));
    let cap = ram_cap();
    let keep = pinned().lock().map(|s| s.clone()).unwrap_or_default();
    while list.len() > 1 {
        let bytes: usize = list.iter().map(|(_, d)| d.pcm.len() * 4).sum();
        if bytes <= cap {
            break;
        }
        // évince la plus ancienne **non épinglée** ; si tout est épinglé, on
        // garde tout (mieux vaut dépasser le plafond que couper le projet)
        match list.iter().position(|(k, _)| !keep.contains(k)) {
            Some(i) => {
                list.remove(i);
            }
            None => break,
        }
    }
}

/// Vide le cache mémoire (archive supprimée : ses pistes n'existent plus).
pub fn clear_ram_cache() {
    if let Ok(mut list) = ram().lock() {
        list.clear();
    }
}

fn decode_uncached(zxtune: &Path, track: &Track) -> Result<Decoded> {
    // formats que zxtune ne lit pas (.txtp, .fsb, .bfstm…) : vgmstream donne en
    // plus les points de boucle exacts du format. S'il échoue (codec absent de
    // la compilation, conteneur exotique), on retombe sur zxtune : mieux vaut
    // une piste qui joue qu'un message d'erreur.
    if crate::vgmstream::handles(&track.file) {
        if let Some(bin) = crate::vgmstream::binary() {
            match decode_vgmstream(&bin, track) {
                Ok(d) => return Ok(d),
                Err(e) => eprintln!("vgmstream : {e:#} — repli sur zxtune"),
            }
        }
    }
    // un PSF qui déclare un fondu est rendu depuis une copie `fade=0` : le
    // fondu tombe dans la durée rendue et reviendrait à chaque tour de boucle
    let no_fade = crate::psf::no_fade_copy(&track.file);
    // zxtune n'écrit rien si l'extension de sortie n'est pas audio
    let wav = scratch_name("play");
    let _ = std::fs::remove_file(&wav);
    let source = no_fade.clone().unwrap_or_else(|| track.file.clone());
    let target = if track.subsong > 1 {
        format!("{}?#{}", source.display(), track.subsong)
    } else {
        source.display().to_string()
    };
    let stderr = run_zxtune(
        zxtune,
        &[
            "--wav",
            &format!("filename={}", wav.display()),
            "--quiet",
            &target,
        ],
    )?;
    if let Some(copy) = &no_fade {
        let _ = std::fs::remove_file(copy);
    }
    if !wav.exists() {
        bail!(
            "zxtune n'a pas produit de WAV pour {} : {}",
            track.file.display(),
            stderr.trim()
        );
    }
    let read = read_wav(&wav);
    scratch_cleanup(&wav); // le PCM est en mémoire, le fichier part
    let (mut pcm, channels, rate) = read?;
    // Fichier audio **courant** (mp3/flac/wav/ogg…) : le blanc numérique de
    // tête et de queue n'est pas de la musique et fausse tout — la grille se
    // cale dessus (temps 1 dans le blanc) et les tranches commencent muettes
    // (vécu 21/11 : `04_The Dungeons.wav` a 1,13 s de blanc, la découpe tombait
    // 1,6 s à côté). On le retire du **PCM**, pas seulement à la lecture : la
    // découpe, le tempo, la forme d'onde et l'export voient alors le morceau
    // tel qu'il commence. Réservé aux formats courants : sur un chiptune, les
    // points de boucle et la durée du `.m3u` sont absolus et ne survivraient
    // pas au décalage.
    if crate::util::is_common_audio(&track.file) {
        trim_edge_silence(&mut pcm, channels.max(1) as usize, rate);
    }
    // **Tout rendu de zxtune commence par un blanc numérique** : la montée en
    // régime de son moteur. Mesuré 26/11 : 74,94 ms à l'identique sur les USF
    // N64 (Banjo-Tooie), 28 à 39 ms sur les PSF (FF8). La lecture l'enjambait
    // déjà (`head_silence`), mais la timeline le gardait : la grille se cale
    // dessus, la première tranche démarre quelques ms après 0 et le tout début
    // de la piste n'appartient à aucune tranche — « il manque un mini bout au
    // début de chaque musique, du coup ça fausse la découpe » (vécu 26/11).
    // On le retire donc du **PCM**, comme pour un fichier audio courant ; les
    // durées et fenêtres absolues (en-tête VGM/SPC, `.m3u`) sont décalées
    // d'autant, sinon elles désigneraient un autre instant du morceau.
    let ch = channels.max(1) as usize;
    let lead = head_zeros(&pcm, ch, rate);
    let shift = lead as f64 / rate as f64;
    if lead > 0 {
        pcm.drain(..lead * ch);
    }
    let total = pcm.len() as f64 / (channels as f64 * rate as f64);
    // PSF : `length` vaut N tours entiers de la séquence (zxtune rend la boucle
    // plusieurs fois), donc boucler sur la durée du tag rejoue la même partie.
    // On retrouve la période réelle dans le rendu ; à défaut, l'appelant
    // retombe sur la durée du tag.
    let (window, window_passes) = if is_vgm_file(&track.file) {
        (
            shift_window(vgm_window(&track.file, total).ok().flatten(), shift),
            None,
        )
    } else if let Some((start, end)) =
        shift_window(crate::spc::loop_window(&track.file), shift)
            .filter(|(_, end)| *end > 2.0 && *end < total - 0.3)
    {
        // boucle **du fichier** (tag ID666), pas une détection : `passes` reste
        // vide pour que la GUI annonce « boucle du format »
        (Some((start, end)), None)
    } else if crate::psf::length_fade(&track.file).is_some() {
        let found = crate::loopdetect::detect(&pcm, channels, rate, total)
            .map(|e| (e.start, e.start + e.period, e.passes));
        match found {
            Some((start, end, passes)) => (Some((start, end)), Some(passes)),
            None => (None, None),
        }
    } else if let Some(len) = track
        .length
        .map(|l| l - shift)
        .filter(|l| *l > 4.0 && total > *l * 1.5)
    {
        // Durée annoncée par le `.m3u` (NSF/GBS/SPC/USF…) : un arrondi à la
        // seconde, presque toujours un peu court, donc on perd un bout à chaque
        // tour (vécu 21/11, Kirby NES : 38 s annoncés, 38,405 s réels). Le rendu
        // contient plusieurs tours : on recale sur l'enveloppe.
        let refined = crate::loopdetect::refine_announced(&pcm, channels, rate, total, len);
        match refined {
            Some(p) => {
                let passes = ((total / p).round() as u32).max(2);
                (Some((0.0, p)), Some(passes))
            }
            // l'arrondi peut être plus large que ±0,8 s (Kirby NES : 62 s
            // annoncés pour 59,6 s). On ne retient une détection libre que si
            // elle reste dans les 10 % de la durée annoncée et boucle depuis le
            // début — sinon c'est un motif interne, pas la boucle.
            None => match crate::loopdetect::detect(&pcm, channels, rate, total) {
                Some(e) if e.start < 1.5 && (e.period / len - 1.0).abs() < 0.10 => {
                    (Some(e.window()), Some(e.passes))
                }
                _ => (None, None),
            },
        }
    } else {
        (None, None)
    };
    // tempo : grille de temps pour découper une boucle musicale (voir tempo.rs).
    // Calculé ici pour vivre dans l'entrée du cache mémoire, comme la boucle.
    // Le tempo n'est **pas** calculé ici : le réseau coûte 1 à 3 s et
    // retardait la lecture (demande user 21/11). `analyse_tempo` le donne à la
    // demande, quand on ouvre le mode Découpe.
    let envelope = crate::tempo::envelope(&pcm, channels, rate);
    let peaks = peaks_of(&pcm, channels, PEAK_BUCKETS);
    Ok(Decoded {
        pcm: Arc::new(pcm),
        channels,
        rate,
        total,
        window,
        window_passes,
        tempo: None,
        envelope,
        peaks,
    })
}

/// Retire le silence numérique de **tête et de queue** d'un rendu audio
/// courant (voir `decode_uncached`). Seuil très bas (1e-4) : le bruit de fond
/// d'un mp3 n'est pas du blanc, seule une vraie plage muette disparaît. Cap à
/// 10 s de chaque côté, et rien si le reste ferait moins d'une seconde. Le
/// blanc est retiré du PCM : découpe, tempo, export et lecture voient le même
/// morceau, sans décalage.
fn trim_edge_silence(pcm: &mut Vec<f32>, ch: usize, rate: u32) {
    let ch = ch.max(1);
    let frames = pcm.len() / ch;
    if frames == 0 {
        return;
    }
    let eps = 1e-4f32;
    let cap = (10.0 * rate as f64) as usize;
    let loud = |f: usize| pcm[f * ch..f * ch + ch].iter().any(|v| v.abs() > eps);
    let mut head = 0;
    while head < frames.min(cap) && !loud(head) {
        head += 1;
    }
    let mut tail = frames;
    while tail > head && frames - tail < cap && !loud(tail - 1) {
        tail -= 1;
    }
    if head == 0 && tail == frames {
        return;
    }
    // moins d'une seconde de musique : on ne touche à rien
    if tail.saturating_sub(head) < rate as usize {
        return;
    }
    pcm.truncate(tail * ch);
    pcm.drain(..head * ch);
}

/// Zéros **exacts** en tête d'un rendu, en **trames**, capés à 200 ms
/// (l'artefact de démarrage de zxtune fait 28 à 75 ms ; au-delà c'est un blanc
/// voulu par le rip, on n'y touche pas).
fn head_zeros(pcm: &[f32], ch: usize, rate: u32) -> usize {
    let ch = ch.max(1);
    let cap = (rate as usize / 5).max(1);
    let frames = (pcm.len() / ch).min(cap);
    let mut n = 0;
    for f in 0..frames {
        if pcm[f * ch..f * ch + ch].iter().all(|v| *v == 0.0) {
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// Décale une fenêtre de boucle **absolue** (en-tête VGM/SPC, durée `.m3u`) du
/// blanc retiré au décodage. Sans ce décalage la boucle désignerait un autre
/// instant du morceau (voir `decode_uncached`).
fn shift_window(win: Option<(f64, f64)>, shift: f64) -> Option<(f64, f64)> {
    if shift <= 0.0 {
        return win;
    }
    win.map(|(start, end)| ((start - shift).max(0.0), (end - shift).max(0.0)))
}

/// Zéros **exacts** en tête d'un rendu, capés à 200 ms, **rendus en
/// échantillons entrelacés** (trames × canaux) : c'est l'unité de tous les
/// index de `LoopSource`/`Player`. zxtune/le SPU laisse quelques dizaines de ms
/// à zéro avant la première attaque (mesuré 36 ms sur FF8 « Force Your Way »,
/// 75 ms sur les USF N64). Ce n'est pas de la musique : le rejouer à chaque
/// retour au début fabrique un faux trou. Le blanc est **retiré du PCM** au
/// décodage pour les rendus zxtune ; il ne reste ici qu'un filet pour un PCM
/// encore précédé de zéros (vgmstream), et la timeline n'est jamais touchée par
/// cette fonction.
///
/// Renvoyer des **trames** ici a déjà coûté cher : une trame impaire en stéréo
/// donnait un index impair, donc les canaux inversés (vécu 21/11 : « une fois
/// ça joue plus à gauche, une fois plus à droite »).
pub fn head_silence(pcm: &[f32], channels: usize, rate: u32) -> usize {
    head_zeros(pcm, channels, rate) * channels.max(1)
}

/// Décodage par vgmstream (un tour, sans fondu) + boucle du format.
fn decode_vgmstream(bin: &Path, track: &Track) -> Result<Decoded> {
    let info = crate::vgmstream::info(bin, &track.file, track.subsong)?;
    let wav = scratch_name("vgm");
    crate::vgmstream::decode(bin, &track.file, &wav, track.subsong)?;
    let read = read_wav(&wav);
    scratch_cleanup(&wav);
    let (pcm, channels, rate) = read?;
    let total = pcm.len() as f64 / (channels as f64 * rate as f64);
    // le format ne donne pas toujours de boucle : on regarde dans le rendu
    // (un tour unique a été décodé, donc pas de détection de plusieurs tours)
    let (window, passes) = match info.window() {
        Some(w) => (Some(w), None),
        None => (None, None),
    };
    let envelope = crate::tempo::envelope(&pcm, channels, rate);
    let peaks = peaks_of(&pcm, channels, PEAK_BUCKETS);
    Ok(Decoded {
        pcm: Arc::new(pcm),
        channels,
        rate,
        total,
        window,
        window_passes: passes,
        tempo: None,
        envelope,
        peaks,
    })
}

/// Métadonnées qu'annonce zxtune avant de rendre l'audio.
pub struct Probe {
    /// Durée totale rendue (boucle + fondu éventuel).
    pub total: f64,
    /// Fenêtre de boucle `(début, fin)` quand le format la donne (genh, PSF avec
    /// fondu). `None` = pas de boucle distincte du fichier (NSF, SPC, XA…).
    pub window: Option<(f64, f64)>,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Interroge zxtune pour obtenir durée, point de boucle et tags **sans garder
/// le rendu** : l'en-tête sort sur stdout (ligne « Time: … Loop duration: »),
/// on coupe le processus dès qu'on l'a lue. ~0,1 s par piste au lieu d'un
/// rendu complet de 34 Mo.
pub fn probe(zxtune: &Path, track: &Track) -> Result<Probe> {
    // formats hors zxtune : vgmstream annonce durée et boucle directement
    if crate::vgmstream::handles(&track.file) {
        if let Some(bin) = crate::vgmstream::binary() {
            if let Ok(info) = crate::vgmstream::info(&bin, &track.file, track.subsong) {
                return Ok(Probe {
                    total: info.total(),
                    window: info.window(),
                    title: None,
                    author: None,
                });
            }
        }
    }
    let wav = scratch_name("probe");
    let _ = std::fs::remove_file(&wav);
    let target = if track.subsong > 1 {
        format!("{}?#{}", track.file.display(), track.subsong)
    } else {
        track.file.display().to_string()
    };
    // stdbuf force la sortie ligne à ligne : sans lui, l'en-tête reste dans
    // le tampon jusqu'à la fin du rendu
    let mut child = Command::new("stdbuf")
        .arg("-oL")
        .arg(zxtune)
        .arg("--quiet")
        .arg("--wav")
        .arg(format!("filename={}", wav.display()))
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("sonde de {}", track.file.display()))?;

    let mut probe = Probe {
        total: 0.0,
        window: None,
        title: None,
        author: None,
    };
    let mut loop_len = 0.0;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("Title:") {
                let v = v.trim();
                if !v.is_empty() {
                    probe.title = Some(v.to_string());
                }
            } else if let Some(v) = line.strip_prefix("Author:") {
                let v = v.trim();
                if !v.is_empty() {
                    probe.author = Some(v.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("Time:") {
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                probe.total = tokens
                    .first()
                    .and_then(|t| crate::util::parse_clock(t))
                    .unwrap_or(0.0);
                loop_len = tokens
                    .last()
                    .and_then(|t| crate::util::parse_clock(t))
                    .unwrap_or(probe.total);
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    scratch_cleanup(&wav);

    if probe.total <= 0.0 {
        bail!(
            "zxtune n'a pas annoncé de durée pour {}",
            track.file.display()
        );
    }
    // le VGM est traité à part (en-tête, au sample près) : ne rien stocker ici
    if is_vgm_file(&track.file) {
        return Ok(probe);
    }
    // 1) genh : départ de boucle explicite dans l'en-tête
    if let Some((start, end)) = crate::genh::loop_window(&track.file) {
        probe.window = Some((start, end));
    // 2) PSF : la piste boucle sur toute sa longueur ; zxtune n'ajoute le fondu
    //    qu'en lecture unique (le rendu de lecture, lui, est fait sans fondu)
    } else if let Some((length, _fade)) = crate::psf::length_fade(&track.file) {
        if length > 2.0 {
            probe.window = Some((0.0, length.min(probe.total.max(length))));
        }
    } else if let Some(window) = crate::spc::loop_window(&track.file)
        .filter(|(_, end)| *end > 2.0 && *end < probe.total - 0.5)
    {
        probe.window = Some(window);
    } else if loop_len > 2.0 && loop_len < probe.total - 0.5 {
        probe.window = Some((0.0, loop_len));
    }
    Ok(probe)
}

pub fn run_zxtune(zxtune: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new(zxtune)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("lancement de {}", zxtune.display()))?;
    Ok(String::from_utf8_lossy(&out.stderr).to_string())
}

fn read_wav(path: &Path) -> Result<(Vec<f32>, u16, u32)> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("lecture de {}", path.display()))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()?
        }
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
    };
    Ok((samples, spec.channels, spec.sample_rate))
}

/// Fenêtre de boucle VGM : `total / loop` échantillons à 44,1 kHz (v1.50+).
fn vgm_window(path: &Path, total: f64) -> Result<Option<(f64, f64)>> {
    let mut raw = Vec::new();
    File::open(path)?.take(4 * 1024 * 1024).read_to_end(&mut raw)?;
    if raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b {
        let mut dec = flate2::read::GzDecoder::new(File::open(path)?);
        raw.clear();
        dec.by_ref().take(4 * 1024 * 1024).read_to_end(&mut raw)?;
    }
    if raw.len() < 0x28 || &raw[0..4] != b"Vgm " {
        return Ok(None);
    }
    let u32_at = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]) as u64;
    let version = u32_at(0x08);
    let total_samples = u32_at(0x18);
    let loop_samples = u32_at(0x20);
    let rate = if version >= 0x0000_0150 {
        44100.0
    } else {
        match u32_at(0x24) {
            0 => 50.0,
            r => r as f64,
        }
    };
    if loop_samples == 0 || total_samples <= loop_samples {
        return Ok(None);
    }
    let end = (total_samples as f64 / rate).min(total);
    let start = (total_samples - loop_samples) as f64 / rate;
    Ok(Some((start, end)))
}

#[cfg(test)]
mod tests {
    /// Le cache RAM et les épingles sont globaux : tous les tests qui les
    /// touchent prennent ce verrou (sinon l'un vide le cache de l'autre).
    static CACHE_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

    use super::*;

    fn fake(samples: usize) -> Decoded {
        Decoded {
            pcm: Arc::new(vec![0.0; samples]),
            channels: 2,
            rate: 44100,
            total: 0.0,
            window: None,
            window_passes: None,
            tempo: None,
            envelope: None,
            peaks: Arc::new(Vec::new()),
        }
    }

    /// Le blanc numérique de tête d'un rendu zxtune (montée en régime du
    /// moteur : 75 ms sur les USF N64, 28 à 39 ms sur les PSF) est retiré du
    /// PCM, et les fenêtres absolues suivent le décalage. Sans ça la première
    /// tranche démarre quelques ms après 0 et le début de la piste n'appartient
    /// à aucune tranche (vécu 26/11, Banjo-Tooie et FF8 : « il manque un mini
    /// bout au début de chaque musique, du coup ça fausse la découpe »).
    #[test]
    fn blanc_numerique_de_tete_retire() {
        let rate = 1000; // 1 kHz : 1 trame = 1 ms
        let ch = 2;
        let mut pcm = vec![0.0f32; 75 * ch]; // 75 ms de zéros exacts
        for i in 0..500 {
            // départ franc mais pas à zéro : sin(0) serait pris pour du blanc
            let v = 0.3 * ((i as f32 + 1.0) * 0.02).sin();
            pcm.push(v);
            pcm.push(v);
        }
        assert_eq!(head_zeros(&pcm, ch, rate), 75);
        // `head_silence` rend des échantillons entrelacés, pas des trames
        assert_eq!(head_silence(&pcm, ch, rate), 150);
        let close = |a: (f64, f64), b: (f64, f64)| {
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
        };
        // fenêtre de format (tag ID666, en-tête VGM) : décalée d'autant
        assert!(close(
            shift_window(Some((10.0, 120.0)), 0.075).unwrap(),
            (9.925, 119.925)
        ));
        assert!(close(
            shift_window(Some((0.0, 120.0)), 0.075).unwrap(),
            (0.0, 119.925)
        ));
        assert_eq!(shift_window(None, 0.075), None);
        // un blanc de plus de 200 ms n'est pas un artefact de moteur : capé
        assert_eq!(head_zeros(&vec![0.0f32; 5000], ch, rate), 200);
    }

    /// Un blanc numérique de tête et de queue est retiré du PCM (fichier audio
    /// courant) : la grille ne doit pas se caler dessus (vécu 21/11,
    /// `04_The Dungeons.wav` : 1,13 s de blanc au début).
    #[test]
    fn blanc_de_tete_et_queue_retire() {
        let rate = 1000;
        let ch = 2;
        let mut pcm = vec![0.0f32; 2 * ch]; // 2 s de blanc en tête
        for i in 0..(3 * rate) {
            // le premier échantillon doit être franc : sin(0) = 0 serait pris
            // pour du blanc et rognerait une trame
            let v = 0.5 * ((i as f32 + 1.0) * 0.01).sin();
            pcm.push(v);
            pcm.push(v);
        }
        pcm.extend(std::iter::repeat_n(0.0f32, 4 * ch)); // 4 s de blanc en queue
        trim_edge_silence(&mut pcm, ch, rate);
        assert_eq!(pcm.len(), 3 * rate as usize * ch, "3 s de musique, rien d'autre");
        // un morceau qui commence et finit par du son n'est pas touché
        let mut pcm = vec![0.5f32; 3 * rate as usize * ch];
        let avant = pcm.len();
        trim_edge_silence(&mut pcm, ch, rate);
        assert_eq!(pcm.len(), avant);
        // un blanc suivi de moins d'une seconde : on ne touche à rien
        let mut pcm = vec![0.0f32; 2 * ch];
        pcm.extend(std::iter::repeat_n(0.5f32, rate as usize / 2));
        trim_edge_silence(&mut pcm, ch, rate);
        assert_eq!(pcm.len(), 2 * ch + rate as usize / 2);
    }

    /// Le cache mémoire doit rester borné et oublier d'abord la piste la moins
    /// récemment jouée (JPEG des 512 Mo : 4 pistes de 6 min, 20/11).
    #[test]
    fn cache_memoire_borne_et_lru() {
        // le cache et les épingles sont globaux : les tests qui les touchent
        // ne doivent pas tourner en parallèle (sinon l'un vide l'autre)
        let _g = CACHE_TEST.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("GTRACKER_RAM_MB", "1"); // 1 Mo : 250 000 f32
        ram().lock().unwrap().clear();
        unpin_all();

        ram_put("a".into(), fake(1000));
        ram_put("b".into(), fake(1000));
        assert_eq!(ram().lock().unwrap().len(), 2);

        // rejouer « a » le rend le plus récent
        assert!(ram_get("a").is_some());
        {
            let list = ram().lock().unwrap();
            assert_eq!(list.last().unwrap().0, "a");
            assert_eq!(list.first().unwrap().0, "b");
        }

        // deux grosses entrées de 800 Ko : la seconde pousse le total au-delà
        // du plafond, on évince par ordre d'ancienneté (b, puis a, puis c)
        ram_put("c".into(), fake(200_000));
        ram_put("d".into(), fake(200_000));
        assert!(ram_get("b").is_none(), "la plus ancienne doit partir d'abord");
        assert!(ram_get("a").is_none());
        assert!(ram_get("c").is_none());
        assert!(ram_get("d").is_some(), "la piste en cours ne s'évince jamais");

        clear_ram_cache();
        assert_eq!(ram().lock().unwrap().len(), 0);
    }

    #[test]
    fn source_epinglee_jamais_evincee() {
        let _g = CACHE_TEST.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("GTRACKER_RAM_MB", "1");
        ram().lock().unwrap().clear();
        unpin_all();
        ram_put("projet".into(), fake(1000));
        pinned().lock().unwrap().insert("projet".into());
        ram_put("autre".into(), fake(200_000));
        ram_put("encore".into(), fake(200_000));
        assert!(ram_get("projet").is_some(), "la source du projet reste");
        assert!(ram_get("autre").is_none(), "la plus ancienne part d'abord");
        assert!(ram_get("encore").is_some(), "la dernière arrive reste");
        unpin_all();
        ram_put("encore2".into(), fake(200_000));
        ram_put("encore3".into(), fake(200_000));
        assert!(ram_get("projet").is_none(), "sans épingle, l'LRU reprend");
        clear_ram_cache();
    }
}



