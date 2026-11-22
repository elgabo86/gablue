//! Grille rythmique **automatique** par réseau de neurones : Beat This!
//! (ISMIR 2024, CPJKU Linz), via le portage Rust `beat-this` (backend `rten`,
//! Rust pur : aucune bibliothèque système, aucun interpréteur Python).
//!
//! Pourquoi un réseau en plus de `tempo::detect` : ce dernier n'extrait qu'**une
//! période et une phase** d'une enveloppe d'attaque, et sur un rendu PSF
//! réverbéré plusieurs tempos sont également « plausibles » (Dark Ruins :
//! 113, 170, 176 BPM — vécu 21/11). Le réseau donne des **positions de temps**
//! réelles, ce qui permet de mesurer sa propre cohérence : si ses temps ne
//! s'alignent pas sur une droite à moins de [`MAX_RESIDUAL`], il n'y a pas de
//! grille à en tirer et on retombe sur le détecteur d'enveloppe.
//!
//! Mesures de référence (21/11) : synthétique 120 BPM → 120,00 (résidu 2 ms) ;
//! chiptune synthétique 170 BPM avec réverbération → 170,00 (7 ms) ; Specter's
//! Factory → 175,99 (7 ms, d'accord avec l'enveloppe à 176) ; Dark Ruins →
//! rejeté (résidu 50 ms), Super Mario Bros → rejeté (640 ms).
//!
//! Les deux fichiers de modèle (~83 Mo) sont téléchargés **une fois** dans
//! `~/.cache/gtracker/models/` (empreinte SHA-256 vérifiée, écriture
//! atomique). Sans réseau, la fonction renvoie `None` : l'application reste
//! utilisable et retombe sur le détecteur d'enveloppe.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use beat_this::{BeatThis, RtenRuntime};

use crate::tempo::Tempo;

/// Secondes analysées au plus. Au-delà, le coût croît sans gagner en justesse
/// (le réseau travaille par fenêtres de 30 s) et l'ajustement de droite est déjà
/// stable sur deux minutes.
const MAX_ANALYSIS_SECS: f64 = 150.0;

/// Résidu médian maximal (secondes) pour faire confiance aux temps du réseau.
/// Sur un morceau à tempo stable, les temps tombent à ~10 ms d'une droite (la
/// trame du mél est à 20 ms) ; au-delà de 45 ms, il n'y a pas de grille à
/// extraire (voir les mesures en tête de module).
const MAX_RESIDUAL: f64 = 0.045;

/// Période acceptée (0,20 s = 300 BPM … 1,2 s = 50 BPM).
const MIN_PERIOD: f64 = 0.20;
const MAX_PERIOD: f64 = 1.20;

const MEL_FILE: &str = "mel_spectrogram.onnx";
const MEL_URL: &str =
    "https://raw.githubusercontent.com/danigb/beat-this-rs/main/models/mel_spectrogram.onnx";
const MEL_BYTES: u64 = 270_742;
const MEL_SHA: &str = "fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9";

const BEAT_FILE: &str = "beat_this.onnx";
const BEAT_URL: &str =
    "https://github.com/danigb/beat-this-rs/releases/download/model-large/beat_this.onnx";
const BEAT_BYTES: u64 = 83_162_650;
const BEAT_SHA: &str = "5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02";

/// Grille détectée par le réseau, ou `None` s'il n'y a rien de fiable (pas de
/// réseau, modèle absent, temps incohérents). L'appelant retombe alors sur
/// `tempo::detect`.
pub fn detect(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    total: f64,
    window: Option<(f64, f64)>,
) -> Option<Tempo> {
    if total < 8.0 || rate == 0 {
        return None;
    }
    let (mel, model) = match models() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("grille automatique indisponible : {e:#} — repli sur l'enveloppe");
            return None;
        }
    };
    // la boucle bat la mesure : on analyse depuis son début (une intro peut
    // être libre ou très lente et fausser l'ajustement), sur deux minutes max
    let start = window.map(|(a, _)| a).unwrap_or(0.0).max(0.0);
    let len = (total - start).min(MAX_ANALYSIS_SECS);
    if len < 8.0 {
        return None;
    }
    let mono = mono_slice(pcm, channels, rate, start, len);
    let mut tracker = BeatThis::new(&RtenRuntime, &mel, &model).ok()?;
    let found = tracker.analyze_audio(&mono, rate).ok()?;
    let beats: Vec<f64> = found.beats.iter().map(|b| *b as f64 + start).collect();
    let grid = fit(&beats)?;
    if !consistent(&beats) {
        eprintln!(
            "grille automatique rejetée : la période ne tient pas sur les deux moitiés du morceau — repli sur l'enveloppe"
        );
        return None;
    }
    if grid.residual > MAX_RESIDUAL {
        eprintln!(
            "grille automatique rejetée : résidu {:.0} ms > {:.0} ms ({} temps) — repli sur l'enveloppe",
            grid.residual * 1000.0,
            MAX_RESIDUAL * 1000.0,
            beats.len()
        );
        return None;
    }
    if std::env::var("RP_GRID_DEBUG").is_ok() {
        eprintln!(
            "grille automatique : {:.2} BPM (réseau, {} temps, résidu {:.0} ms)",
            60.0 / grid.period,
            beats.len(),
            grid.residual * 1000.0
        );
    }
    Some(Tempo {
        bpm: 60.0 / grid.period,
        beat: grid.period,
        phase: grid.phase.rem_euclid(grid.period),
        confidence: (1.0 - grid.residual / MAX_RESIDUAL).clamp(0.0, 1.0),
    })
}

/// Une période n'est retenue que si elle tient sur **les deux moitiés** du
/// morceau : une « moyenne » de temps qui flottent traverse n'importe quel
/// résidu (vécu 21/11 : sur Specter's Factory la période vaut 0,3409 s sur les
/// deux premières minutes et 0,3405 s sur six, soit un demi-temps de dérive).
fn consistent(beats: &[f64]) -> bool {
    if beats.len() < 40 {
        return true;
    }
    let mid = beats.len() / 2;
    match (fit(&beats[..mid]), fit(&beats[mid..])) {
        (Some(a), Some(b)) => (a.period / b.period - 1.0).abs() <= 0.005,
        _ => true,
    }
}

struct Grid {
    period: f64,
    phase: f64,
    /// Résidu médian des temps par rapport à la droite (secondes).
    residual: f64,
}

/// Ajuste une période et une phase sur des temps détectés, en tolérant les
/// temps sautés (le réseau en oublie parfois un) : chaque intervalle est
/// arrondi à un nombre entier de périodes, puis on refait une régression.
fn fit(beats: &[f64]) -> Option<Grid> {
    if beats.len() < 16 {
        return None;
    }
    let span = beats[beats.len() - 1] - beats[0];
    if span < 8.0 {
        return None;
    }
    let mut diffs: Vec<f64> = beats
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > 0.0)
        .collect();
    if diffs.len() < 15 {
        return None;
    }
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // quartile bas : le plus petit intervalle courant, insensible aux temps sautés
    let mut period = diffs[diffs.len() / 4];
    if !(MIN_PERIOD..=MAX_PERIOD).contains(&period) {
        return None;
    }
    let mut idx = vec![0.0f64; beats.len()];
    let (mut phase, mut p) = (beats[0], period);
    for _ in 0..8 {
        let mut k = 0.0;
        for i in 1..beats.len() {
            k += ((beats[i] - beats[i - 1]) / p).round().max(1.0);
            idx[i] = k;
        }
        // moindres carrés : t = p·k + phase
        let n = beats.len() as f64;
        let sx: f64 = idx.iter().sum();
        let sy: f64 = beats.iter().sum();
        let sxx: f64 = idx.iter().map(|x| x * x).sum();
        let sxy: f64 = idx.iter().zip(beats).map(|(x, y)| x * y).sum();
        let den = n * sxx - sx * sx;
        if den.abs() < 1e-9 {
            return None;
        }
        p = (n * sxy - sx * sy) / den;
        if !(p > 0.0 && p.is_finite()) {
            return None;
        }
        period = p;
        phase = (sy - p * sx) / n;
    }
    let mut res: Vec<f64> = idx
        .iter()
        .zip(beats)
        .map(|(k, b)| (b - (period * k + phase)).abs())
        .collect();
    res.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(Grid {
        period,
        phase,
        residual: res[res.len() / 2],
    })
}

/// Mono, sur `[start, start + len)` — le réseau veut un seul canal au taux
/// d'origine (il rééchantillonne lui-même à 22,05 kHz).
fn mono_slice(pcm: &[f32], channels: u16, rate: u32, start: f64, len: f64) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let frames = pcm.len() / ch;
    let i0 = ((start * rate as f64) as usize).min(frames);
    let n = ((len * rate as f64) as usize).min(frames - i0);
    let mut out = Vec::with_capacity(n);
    for f in i0..i0 + n {
        let s: f32 = pcm[f * ch..f * ch + ch].iter().sum::<f32>() / ch as f32;
        out.push(s);
    }
    out
}

// ------------------------------------------------------------------ modèles

/// Chemins des deux modèles, téléchargés et vérifiés au premier usage.
fn models() -> Result<(PathBuf, PathBuf)> {
    let dir = crate::util::cache_root().join("models");
    let mel = fetch(&dir, MEL_FILE, MEL_URL, MEL_BYTES, MEL_SHA)?;
    let beat = fetch(&dir, BEAT_FILE, BEAT_URL, BEAT_BYTES, BEAT_SHA)?;
    Ok((mel, beat))
}

fn fetch(dir: &Path, name: &str, url: &str, bytes: u64, sha: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    let path = dir.join(name);
    if let Ok(md) = std::fs::metadata(&path) {
        if md.len() == bytes && sha256(&path).map(|h| h == sha).unwrap_or(false) {
            return Ok(path);
        }
    }
    let part = dir.join(format!("{name}.part"));
    let _ = std::fs::remove_file(&part);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(120))
        .user_agent("gtracker/0.1")
        .build();
    let resp = agent
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part)
        .with_context(|| format!("écriture de {}", part.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        got += n as u64;
    }
    file.flush()?;
    drop(file);
    if got != bytes {
        let _ = std::fs::remove_file(&part);
        bail!("{name} : {got} octets reçus, {bytes} attendus");
    }
    let h = sha256(&part)?;
    if h != sha {
        let _ = std::fs::remove_file(&part);
        bail!("{name} : empreinte {h} ≠ {sha}");
    }
    // renommage atomique : un arrêt en cours de téléchargement ne laisse jamais
    // un modèle tronqué à la place du bon
    std::fs::rename(&part, &path)?;
    Ok(path)
}

fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grille parfaite à 170 BPM avec quelques temps sautés : période et phase
    /// exactes, résidu quasi nul.
    #[test]
    fn fit_grid_stable() {
        let period = 60.0 / 170.0;
        let beats: Vec<f64> = (0..200)
            .filter(|k| k % 17 != 5)
            .map(|k| 0.37 + k as f64 * period)
            .collect();
        let g = fit(&beats).expect("grille");
        assert!((g.period - period).abs() < 1e-4, "période {}", g.period);
        assert!(g.residual < 0.002, "résidu {}", g.residual);
    }

    /// Temps sans pouls (jitter aléatoire de ±80 ms autour d'une pseudo-grille) :
    /// le résidu médian dépasse le seuil, donc aucune grille n'est retenue.
    #[test]
    fn fit_grid_bruit_rejete() {
        // suite déterministe, sans dépendance : LCG à graine fixe
        let mut x: u64 = 12345;
        let beats: Vec<f64> = (0..200)
            .map(|k| {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u = (x >> 33) as f64 / (1u64 << 31) as f64; // [0, 1)
                let jitter = (u - 0.5) * 0.30; // ±150 ms
                k as f64 * 0.31 + jitter
            })
            .collect();
        match fit(&beats) {
            Some(g) => assert!(g.residual > MAX_RESIDUAL, "résidu {}", g.residual),
            None => {}
        }
    }

    /// Grille stable : cohérente entre ses deux moitiés.
    #[test]
    fn coherent_si_periode_tient() {
        let period = 60.0 / 170.0;
        let beats: Vec<f64> = (0..120).map(|k| 0.37 + k as f64 * period).collect();
        assert!(consistent(&beats));
    }

    /// Tempo qui dérive de 2 % en cours de morceau : ce n'est pas une grille,
    /// et c'est justement le cas des rendus PSF réverbérés (vécu 21/11).
    #[test]
    fn incoherent_si_periode_derive() {
        let mut t = 0.0;
        let mut beats = Vec::new();
        for k in 0..120 {
            let period = 60.0 / (170.0 + 0.02 * 170.0 * k as f64 / 120.0);
            t += period;
            beats.push(t);
        }
        assert!(!consistent(&beats));
    }

    /// Trop peu de temps : pas de grille (le réseau peut n'en donner que trois).
    #[test]
    fn fit_trop_peu_de_temps() {
        assert!(fit(&[0.5, 1.0, 1.5]).is_none());
    }

    /// Mono sur une tranche : un morceau stéréo donne la moyenne des canaux.
    #[test]
    fn mono_slice_moyenne() {
        let pcm = vec![1.0, -1.0, 0.5, 0.5, 0.2, 0.4];
        // trames 1 et 2 : (0,5 + 0,5)/2 = 0,5 puis (0,2 + 0,4)/2 = 0,3
        let m = mono_slice(&pcm, 2, 1, 1.0, 2.0);
        assert_eq!(m, vec![0.5, 0.3]);
    }
}
