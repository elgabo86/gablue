use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{bail, Context, Result};

use crate::model::Track;
use crate::util::{decoded_dir, is_vgm_file};

pub struct Decoded {
    pub pcm: Arc<Vec<f32>>,
    pub channels: u16,
    pub rate: u32,
    pub total: f64,
    /// Fenêtre de boucle issue du format ou détectée dans le rendu, en secondes.
    pub window: Option<(f64, f64)>,
    /// Nombre de tours trouvés dans le rendu (PSF sans information de boucle).
    pub window_passes: Option<u32>,
}

/// Décode une piste en WAV via `zxtune123 --wav`, avec cache disque.
pub fn decode(zxtune: &Path, track: &Track) -> Result<Decoded> {
    let cache = decoded_dir();
    std::fs::create_dir_all(&cache)?;
    // formats que zxtune ne lit pas (.txtp, .vgmstream…) : vgmstream donne en
    // plus les points de boucle exacts du format
    if crate::vgmstream::handles(&track.file) {
        if let Some(bin) = crate::vgmstream::binary() {
            return decode_vgmstream(&bin, track, &cache);
        }
    }
    // un PSF qui déclare un fondu est rendu depuis une copie `fade=0` : le
    // fondu tombe dans la durée rendue et reviendrait à chaque tour de boucle
    let no_fade = crate::psf::no_fade_copy(&track.file);
    let base = crate::util::hash_path(&track.file, track.subsong as u64);
    let key = if no_fade.is_some() {
        format!("{base}-nofade")
    } else {
        base.clone()
    };
    let wav = cache.join(format!("{key}.wav"));
    if no_fade.is_some() {
        // le rendu sans fondu est le seul que la lecture utilise : l'ancien
        // WAV avec fondu (rendu avant cette règle) n'est plus atteignable
        let plain = cache.join(format!("{base}.wav"));
        if plain.exists() {
            let _ = std::fs::remove_file(&plain);
            let _ = std::fs::remove_file(cache.join(format!("{base}.loop")));
        }
    }
    // période de boucle détectée pour ce rendu (évite 250 ms d'analyse à
    // chaque lecture) : fichier à côté du WAV, invalidé avec lui
    let side = cache.join(format!("{key}.loop"));

    if !wav.exists() {
        // zxtune n'écrit rien si l'extension de sortie n'est pas audio
        let part = cache.join(format!("{key}.part.wav"));
        let _ = std::fs::remove_file(&part);
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
                &format!("filename={}", part.display()),
                "--quiet",
                &target,
            ],
        )?;
        if let Some(copy) = &no_fade {
            let _ = std::fs::remove_file(copy);
        }
        if !part.exists() {
            bail!(
                "zxtune n'a pas produit de WAV pour {} : {}",
                track.file.display(),
                stderr.trim()
            );
        }
        std::fs::rename(&part, &wav)?;
        let _ = std::fs::remove_file(&side);
    }

    // cache plafonné : on marque la piste comme jouée à l'instant et on évince
    // les plus anciennes au-delà du plafond (RETROPLAYER_CACHE_MB)
    touch_cache(&cache, &key);

    let (pcm, channels, rate) = read_wav(&wav)?;
    let total = pcm.len() as f64 / (channels as f64 * rate as f64);
    // PSF : `length` vaut N tours entiers de la séquence (zxtune rend la boucle
    // plusieurs fois), donc boucler sur la durée du tag rejoue la même partie.
    // On retrouve la période réelle dans le rendu ; à défaut, l'appelant
    // retombe sur la durée du tag.
    let (window, window_passes) = if is_vgm_file(&track.file) {
        (vgm_window(&track.file, total).ok().flatten(), None)
    } else if crate::psf::length_fade(&track.file).is_some() {
        let found = read_loop_cache(&side).unwrap_or_else(|| {
            let found = crate::loopdetect::detect(&pcm, channels, rate, total)
                .map(|e| (e.start, e.start + e.period, e.passes));
            write_loop_cache(&side, found);
            found
        });
        match found {
            Some((start, end, passes)) => (Some((start, end)), Some(passes)),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    Ok(Decoded {
        pcm: Arc::new(pcm),
        channels,
        rate,
        total,
        window,
        window_passes,
    })
}

/// Période de boucle mémorisée pour un rendu : `None` = rien de mémorisé,
/// `Some(None)` = déjà analysé, aucune répétition trouvée.
/// Version du format de cache de boucle : une analyse produite par un ancien
/// détecteur ne doit pas être réutilisée.
const LOOP_CACHE_VERSION: u32 = 4;

fn read_loop_cache(path: &Path) -> Option<Option<(f64, f64, u32)>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut tokens = text.split_whitespace();
    if tokens.next()?.parse::<u32>().ok()? != LOOP_CACHE_VERSION {
        return None;
    }
    let nums: Vec<f64> = tokens.filter_map(|v| v.parse().ok()).collect();
    match nums.as_slice() {
        [start, period, passes] if *start >= 0.0 && *period > 1.0 && *passes >= 2.0 => {
            Some(Some((*start, start + period, *passes as u32)))
        }
        _ => Some(None),
    }
}

fn write_loop_cache(path: &Path, value: Option<(f64, f64, u32)>) {
    let text = match value {
        Some((start, end, passes)) => {
            format!("{LOOP_CACHE_VERSION} {start:.6} {:.6} {passes}\n", end - start)
        }
        None => format!("{LOOP_CACHE_VERSION} 0 0 0\n"),
    };
    let _ = std::fs::write(path, text);
}


/// Plafond du cache audio, en Mo (variable `RETROPLAYER_CACHE_MB`).
const CACHE_CAP_MB: u64 = 1536;

fn cache_cap() -> u64 {
    std::env::var("RETROPLAYER_CACHE_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(CACHE_CAP_MB)
        * 1024
        * 1024
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Un WAV décodé par piste jouée : sans plafond, une collection entière de PSF
/// remplit le disque (constaté 1,7 Go pour 47 pistes, 20/11). On tient un index
/// `clé<TAB>dernière lecture` et on jette les pistes les moins récemment jouées.
fn touch_cache(cache: &Path, key: &str) {
    let index = cache.join("index");
    let mut used: Vec<(String, u64)> = std::fs::read_to_string(&index)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (k, t) = l.split_once('\t')?;
            Some((k.to_string(), t.parse().ok()?))
        })
        .collect();
    let now = now_secs();
    match used.iter_mut().find(|(k, _)| k == key) {
        Some(e) => e.1 = now,
        None => used.push((key.to_string(), now)),
    }

    // fichiers présents, taille totale et date de dernière lecture
    let mut files: Vec<(u64, PathBuf, u64)> = Vec::new(); // (dernière lecture, chemin, taille)
    let mut total = 0u64;
    if let Ok(dir) = std::fs::read_dir(cache) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("wav") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // rendu en cours d'écriture
            if stem.ends_with(".part") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            total += meta.len();
            let last = used
                .iter()
                .find(|(k, _)| k == stem)
                .map(|(_, t)| *t)
                .unwrap_or(0);
            files.push((last, path, meta.len()));
        }
    }

    let cap = cache_cap();
    if total > cap {
        files.sort_by_key(|(t, _, _)| *t);
        for (_, path, size) in files {
            if total <= cap {
                break;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem == key {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
                let _ = std::fs::remove_file(cache.join(format!("{stem}.loop")));
                used.retain(|(k, _)| k != stem);
            }
        }
    }

    // l'index ne garde que ce qui est encore en cache
    let mut live: Vec<String> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(cache) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wav") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    live.push(stem.to_string());
                }
            }
        }
    }
    used.retain(|(k, _)| live.contains(k));
    let mut text = String::new();
    for (k, t) in &used {
        text.push_str(&format!("{k}\t{t}\n"));
    }
    let _ = std::fs::write(&index, text);
}

/// Décodage par vgmstream (un tour, sans fondu) + boucle du format.
fn decode_vgmstream(bin: &Path, track: &Track, cache: &Path) -> Result<Decoded> {
    let info = crate::vgmstream::info(bin, &track.file)?;
    let key = format!(
        "{}-vgm",
        crate::util::hash_path(&track.file, track.subsong as u64)
    );
    let wav = cache.join(format!("{key}.wav"));
    if !wav.exists() {
        let part = cache.join(format!("{key}.part.wav"));
        crate::vgmstream::decode(bin, &track.file, &part)?;
        std::fs::rename(&part, &wav)?;
        let _ = std::fs::remove_file(cache.join(format!("{key}.loop")));
    }
    touch_cache(cache, &key);
    let (pcm, channels, rate) = read_wav(&wav)?;
    let total = pcm.len() as f64 / (channels as f64 * rate as f64);
    // le format ne donne pas toujours de boucle : on regarde dans le rendu
    // (un tour unique a été décodé, donc pas de détection de plusieurs tours)
    let (window, passes) = match info.window() {
        Some(w) => (Some(w), None),
        None => (None, None),
    };
    Ok(Decoded {
        pcm: Arc::new(pcm),
        channels,
        rate,
        total,
        window,
        window_passes: passes,
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
            let info = crate::vgmstream::info(&bin, &track.file)?;
            return Ok(Probe {
                total: info.total(),
                window: info.window(),
                title: None,
                author: None,
            });
        }
    }
    let dir = decoded_dir().join("probe");
    std::fs::create_dir_all(&dir)?;
    let key = crate::util::hash_path(&track.file, track.subsong as u64);
    let wav = dir.join(format!("{key}-{}.wav", std::process::id()));
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
    let _ = std::fs::remove_file(&wav);

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

/// Chemin du WAV en cache pour une piste (utile pour l'export/les tests).
pub fn cached_wav(track: &Track) -> PathBuf {
    let key = crate::util::hash_path(&track.file, track.subsong as u64);
    let nofade = decoded_dir().join(format!("{key}-nofade.wav"));
    if nofade.exists() {
        return nofade;
    }
    decoded_dir().join(format!("{key}.wav"))
}
