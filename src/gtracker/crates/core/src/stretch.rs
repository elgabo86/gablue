//! Changement de durée sans changer la hauteur (WSOLA) + cache.
//!
//! Le repitch suffit au chiptune (la hauteur suit la vitesse, comme un tracker),
//! mais **accorder une tranche** demande de changer la hauteur sans casser la
//! synchro : d'où le stretch. Il est calculé **hors temps réel**, par clip, et
//! mis en cache ; la voix du mixer lit ensuite un buffer ordinaire, donc le
//! chemin audio ne connaît ni WSOLA ni allocation.
//!
//! Qualité : OLA à recouvrement avec recherche de phase (WSOLA). Ce n'est pas du
//! Rubber Band (pas de lib C++), et sur un rendu réverbéré de PSF ça s'entend.
//! C'est pour ça que le repitch reste le défaut.

use std::sync::{Arc, Mutex, OnceLock};

/// Fenêtre d'analyse (trames) et pas (50 % de recouvrement).
const WIN: usize = 1024;
const HOP: usize = WIN / 2;
/// Amplitude de la recherche de phase (trames), bornée : au-delà, on mélange
/// deux moments différents et le timbre se dédouble.
const SEARCH: isize = 96;
/// Décimation pour la corrélation : divise le coût par 16 sans changer le choix.
const DECIM: usize = 4;

/// Étire `pcm[start..end]` (trames) en `out_frames` trames, hauteur tenue.
pub fn wsola(
    pcm: &[f32],
    channels: u16,
    start: usize,
    end: usize,
    out_frames: usize,
) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let total = pcm.len() / ch;
    let start = start.min(total);
    let end = end.min(total).max(start);
    let in_frames = end - start;
    if in_frames == 0 || out_frames == 0 {
        return Vec::new();
    }
    if in_frames == out_frames {
        return pcm[start * ch..end * ch].to_vec();
    }
    // mono de la plage, pour la corrélation uniquement
    let mut mono = Vec::with_capacity(in_frames);
    for f in 0..in_frames {
        let mut v = 0.0f32;
        for c in 0..ch {
            v += pcm[(start + f) * ch + c];
        }
        mono.push(v / ch as f32);
    }

    let mut out = vec![0.0f32; out_frames * ch];
    let mut wsum = vec![0.0f32; out_frames + WIN];
    let mut acc_mono = vec![0.0f32; out_frames + WIN];
    let hann: Vec<f32> = (0..WIN)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / WIN as f32).cos())
        .collect();

    let ratio = in_frames as f64 / out_frames as f64;
    let max_start = in_frames.saturating_sub(WIN.min(in_frames)) as isize;
    let mut k = 0usize;
    loop {
        let out_pos = k * HOP;
        if out_pos >= out_frames {
            break;
        }
        let ideal = (out_pos as f64 * ratio).round() as isize;
        let mut best = 0isize;
        if k > 0 {
            let mut best_score = f32::NEG_INFINITY;
            // corrélation avec ce qui est déjà écrit (chevauchement courant)
            for d in (-SEARCH..=SEARCH).step_by(1) {
                let p = ideal + d;
                if p < 0 || p > max_start {
                    continue;
                }
                let mut score = 0.0f32;
                let mut i = 0usize;
                while i < HOP {
                    score += mono[(p as usize + i).min(in_frames - 1)]
                        * acc_mono[(out_pos + i).min(out_frames - 1)];
                    i += DECIM;
                }
                if score > best_score {
                    best_score = score;
                    best = d;
                }
            }
        }
        let p = (ideal + best).clamp(0, max_start.max(0)) as usize;
        for i in 0..WIN {
            let src = (p + i).min(in_frames - 1);
            let dst = out_pos + i;
            if dst >= out.len() / ch {
                break;
            }
            let w = hann[i];
            for c in 0..ch {
                out[dst * ch + c] += pcm[(start + src) * ch + c] * w;
            }
            acc_mono[dst] += mono[src] * w;
            wsum[dst] += w;
        }
        k += 1;
    }

    // normalisation : somme des fenêtres de Hann², sinon les bords baissent
    for f in 0..out_frames {
        let ws = wsum[f];
        if ws > 1e-6 {
            for c in 0..ch {
                out[f * ch + c] /= ws;
            }
        }
    }
    out
}

// ── cache ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    ptr: usize,
    start: usize,
    end: usize,
    out_frames: usize,
    channels: u16,
}

struct Entry {
    key: CacheKey,
    pcm: Arc<Vec<f32>>,
    bytes: usize,
}

fn cache() -> &'static Mutex<Vec<Entry>> {
    static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn cap_bytes() -> usize {
    std::env::var("GTRACKER_STRETCH_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(256)
        * 1024
        * 1024
}

fn total_bytes(list: &[Entry]) -> usize {
    list.iter().map(|e| e.bytes).sum()
}

/// Buffer étiré, mémorisé. La clé comprend l'adresse de l'`Arc` source : le PCM
/// d'une piste décodée est stable tant qu'il vit dans le cache de décodage.
pub fn cached(
    pcm: &Arc<Vec<f32>>,
    channels: u16,
    start: usize,
    end: usize,
    out_frames: usize,
) -> Arc<Vec<f32>> {
    let key = CacheKey {
        ptr: Arc::as_ptr(pcm) as usize,
        start,
        end,
        out_frames,
        channels,
    };
    {
        let mut list = cache().lock().unwrap_or_else(|e| e.into_inner());
        // remonte l'entrée utilisée en tête (LRU)
        if let Some(i) = list.iter().position(|e| e.key == key) {
            let e = list.remove(i);
            let out = e.pcm.clone();
            list.insert(0, e);
            return out;
        }
    }
    let out = Arc::new(wsola(pcm, channels, start, end, out_frames));
    let bytes = out.len() * 4;
    let mut list = cache().lock().unwrap_or_else(|e| e.into_inner());
    list.insert(0, Entry { key, pcm: out.clone(), bytes });
    while total_bytes(&list) > cap_bytes() && list.len() > 1 {
        list.pop();
    }
    out
}

pub fn clear_cache() {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f32, hz: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    fn crossings(pcm: &[f32]) -> usize {
        pcm.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn duree_exacte_et_hauteur_tenue() {
        let rate = 44100u32;
        let src = sine(rate, 0.5, 440.0);
        let out = wsola(&src, 1, 0, src.len(), src.len() * 2);
        assert_eq!(out.len(), src.len() * 2);
        assert!(out.iter().all(|v| v.is_finite()));
        // hauteur : même nombre de passages par zéro par seconde (à 5 % près)
        let f_in = crossings(&src) as f64 / 0.5 / 2.0;
        let f_out = crossings(&out) as f64 / 1.0 / 2.0;
        assert!((f_in - f_out).abs() / f_in < 0.05, "f_in {f_in} f_out {f_out}");
        // l'amplitude ne s'écroule pas (normalisation des fenêtres)
        let peak = out.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak > 0.8, "crête {peak}");
        // pas de clic : la dérivée maximale reste proche de celle d'un sinus
        let max_d = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(max_d < 0.2, "saut {max_d}");
    }

    #[test]
    fn stéréo_garde_les_canaux() {
        let rate = 8000u32;
        let mut pcm = Vec::new();
        for i in 0..4000 {
            let v = (i as f32 * 0.01).sin();
            pcm.push(v);
            pcm.push(-v);
        }
        let out = wsola(&pcm, 2, 0, 4000, 6000);
        assert_eq!(out.len(), 12_000);
        assert!(out.chunks(2).all(|c| (c[0] + c[1]).abs() < 1e-3));
        let _ = rate;
    }

    #[test]
    fn cache_rend_le_meme_buffer() {
        let pcm = Arc::new(sine(8000, 0.2, 220.0));
        let a = cached(&pcm, 1, 0, pcm.len(), pcm.len() * 2);
        let b = cached(&pcm, 1, 0, pcm.len(), pcm.len() * 2);
        assert!(Arc::ptr_eq(&a, &b));
        clear_cache();
    }
}
