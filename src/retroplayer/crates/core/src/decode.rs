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
}

/// Décode une piste (zxtune ou vgmstream), avec cache **en mémoire**.
///
/// Rien n'est gardé sur le disque : le rendu passe par un fichier temporaire
/// dans `/dev/shm` (RAM) aussitôt relu et supprimé, et les échantillons restent
/// en mémoire pour les relectures (plafond `RETROPLAYER_RAM_MB`, 512 Mo par
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

/// Dossier de rendu en RAM : zxtune et vgmstream n'écrivent que dans un
/// fichier, mais il ne doit rien rester sur le disque.
fn scratch_dir() -> PathBuf {
    let base = if Path::new("/dev/shm").is_dir() {
        PathBuf::from("/dev/shm")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("retroplayer-{}", std::process::id()));
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
const RAM_CAP_MB: usize = 512;

static RAM: OnceLock<Mutex<Vec<(String, Decoded)>>> = OnceLock::new();

fn ram() -> &'static Mutex<Vec<(String, Decoded)>> {
    RAM.get_or_init(|| Mutex::new(Vec::new()))
}

fn ram_cap() -> usize {
    std::env::var("RETROPLAYER_RAM_MB")
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
    while list.len() > 1 {
        let bytes: usize = list.iter().map(|(_, d)| d.pcm.len() * 4).sum();
        if bytes <= cap {
            break;
        }
        list.remove(0);
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
    let (pcm, channels, rate) = read?;
    let total = pcm.len() as f64 / (channels as f64 * rate as f64);
    // PSF : `length` vaut N tours entiers de la séquence (zxtune rend la boucle
    // plusieurs fois), donc boucler sur la durée du tag rejoue la même partie.
    // On retrouve la période réelle dans le rendu ; à défaut, l'appelant
    // retombe sur la durée du tag.
    let (window, window_passes) = if is_vgm_file(&track.file) {
        (vgm_window(&track.file, total).ok().flatten(), None)
    } else if crate::psf::length_fade(&track.file).is_some() {
        let found = crate::loopdetect::detect(&pcm, channels, rate, total)
            .map(|e| (e.start, e.start + e.period, e.passes));
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
    use super::*;

    fn fake(samples: usize) -> Decoded {
        Decoded {
            pcm: Arc::new(vec![0.0; samples]),
            channels: 2,
            rate: 44100,
            total: 0.0,
            window: None,
            window_passes: None,
        }
    }

    /// Le cache mémoire doit rester borné et oublier d'abord la piste la moins
    /// récemment jouée (JPEG des 512 Mo : 4 pistes de 6 min, 20/11).
    #[test]
    fn cache_memoire_borne_et_lru() {
        std::env::set_var("RETROPLAYER_RAM_MB", "1"); // 1 Mo : 250 000 f32
        ram().lock().unwrap().clear();

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
}
