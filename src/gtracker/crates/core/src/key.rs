//! Tonalité d'une tranche : chroma + profils de Krumhansl-Schmuckler.
//!
//! Sert à la case « même gamme » du GTracker : on transpose un clip **en bloc**
//! vers la tonalité de la piste maîtresse. On ne replace jamais chaque note dans
//! la gamme (ce serait de la réharmonisation, impossible sans séparation de
//! sources) — la limite est assumée dans `crates/core/AGENTS.md`.
//!
//! L'analyse porte sur une **plage** (le clip), pas sur la source entière : une
//! intro de 4 mesures n'est pas dans la tonalité du refrain.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Classe de hauteur de la tonique, 0 = do.
    pub tonic: u8,
    pub mode: Mode,
    /// Écart de corrélation avec la deuxième meilleure tonalité (0…1).
    /// Sous ~0,08 c'est une hypothèse : l'interface doit le dire.
    pub confidence: f32,
}

/// Profils de Krumhansl & Kessler (1982), dans l'ordre des classes de hauteur.
const MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

const NOMS: [&str; 12] = [
    "do", "do#", "ré", "mib", "mi", "fa", "fa#", "sol", "lab", "la", "sib", "si",
];

impl Key {
    pub fn new(tonic: u8, mode: Mode, confidence: f32) -> Self {
        Self { tonic: tonic % 12, mode, confidence: confidence.clamp(0.0, 1.0) }
    }

    pub fn name(&self) -> String {
        format!(
            "{} {}",
            NOMS[self.tonic as usize % 12],
            match self.mode {
                Mode::Major => "majeur",
                Mode::Minor => "mineur",
            }
        )
    }

    /// Centre du **mode majeur relatif** : la mineur naturelle et do majeur ont
    /// exactement les mêmes notes. C'est cette « gamme » qu'on aligne, pas
    /// l'étiquette de tonique (transposer la mineur en majeur est impossible :
    /// une transposition conserve le mode).
    pub fn scale_center(&self) -> u8 {
        (self.tonic + if self.mode == Mode::Minor { 3 } else { 0 }) % 12
    }

    /// Demi-tons à appliquer pour que les **notes** du clip tombent dans la même
    /// gamme que `target` (plus court chemin, `-6..=5`).
    pub fn transpose_for_scale(&self, target: &Key) -> i32 {
        signed_step(self.scale_center(), target.scale_center())
    }

    /// Demi-tons pour aligner la **tonique** (autre choix possible : on garde la
    /// couleur du morceau, on ne cherche que le même centre).
    pub fn transpose_for_tonic(&self, target: &Key) -> i32 {
        signed_step(self.tonic, target.tonic)
    }

    /// Hauteur induite par un changement de vitesse (`ratio` de lecture), en
    /// demi-tons. Un repitch de 1,5× monte de 7 demi-tons.
    pub fn induced_semitones(ratio: f64) -> f64 {
        if ratio > 0.0 && ratio.is_finite() {
            12.0 * ratio.log2()
        } else {
            0.0
        }
    }
}

/// Pas signé le plus court de `from` à `to`, dans `-6..=5`.
pub fn signed_step(from: u8, to: u8) -> i32 {
    let d = (to as i32 - from as i32).rem_euclid(12);
    if d > 6 {
        d - 12
    } else {
        d
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Chroma d'une plage PCM (trames), normalisée (somme = 1). `None` si la plage
/// est trop courte (< 0,3 s) ou silencieuse.
pub fn chroma(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    start_frame: usize,
    end_frame: usize,
) -> Option<[f32; 12]> {
    let ch = channels.max(1) as usize;
    let total = pcm.len() / ch;
    let start = start_frame.min(total);
    let end = end_frame.min(total).max(start);
    let win = 4096usize;
    let hop = 2048usize;
    if end - start < (rate as usize / 3).max(win) {
        return None;
    }
    let mut chroma = [0.0f64; 12];
    let mut energy = 0.0f64;
    let mut re = vec![0.0f32; win];
    let mut im = vec![0.0f32; win];
    let hann: Vec<f32> = (0..win)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / win as f32).cos())
        .collect();

    let mut pos = start;
    while pos + win <= end {
        for k in 0..win {
            let mut v = 0.0f32;
            for c in 0..ch {
                v += pcm[(pos + k) * ch + c];
            }
            re[k] = (v / ch as f32) * hann[k];
            im[k] = 0.0;
        }
        crate::fft::fft(&mut re, &mut im);
        for bin in 1..win / 2 {
            let f = bin as f64 * rate as f64 / win as f64;
            if !(45.0..=5000.0).contains(&f) {
                continue;
            }
            let mag = ((re[bin] * re[bin] + im[bin] * im[bin]) as f64).sqrt();
            if mag <= 0.0 {
                continue;
            }
            // midi → classe de hauteur (do = 0)
            let midi = 69.0 + 12.0 * (f / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            // pondération douce : les harmoniques aiguës ne doivent pas écraser
            // le fondamental, mais un accompagnement aigu compte aussi
            let w = mag / f.sqrt();
            chroma[pc] += w;
            energy += w;
        }
        pos += hop;
    }
    if energy < 1e-6 {
        return None;
    }
    let max = chroma.iter().cloned().fold(0.0f64, f64::max);
    if max <= 0.0 {
        return None;
    }
    let mut out = [0.0f32; 12];
    for i in 0..12 {
        out[i] = (chroma[i] / energy) as f32;
    }
    Some(out)
}

fn pearson(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let n = 12.0f32;
    let ma = a.iter().sum::<f32>() / n;
    let mb = b.iter().sum::<f32>() / n;
    let mut num = 0.0f32;
    let (mut da, mut db) = (0.0f32, 0.0f32);
    for i in 0..12 {
        let x = a[i] - ma;
        let y = b[i] - mb;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    num / (da.sqrt() * db.sqrt())
}

/// Tonalité la plus probable d'un chroma, avec l'écart à la deuxième.
pub fn best_key(chroma: &[f32; 12]) -> Key {
    let mut best = (f32::NEG_INFINITY, 0u8, Mode::Major);
    let mut second = f32::NEG_INFINITY;
    for tonic in 0..12u8 {
        for mode in [Mode::Major, Mode::Minor] {
            let profile = match mode {
                Mode::Major => &MAJOR,
                Mode::Minor => &MINOR,
            };
            let mut rotated = [0.0f32; 12];
            for i in 0..12 {
                rotated[i] = profile[(i + 12 - tonic as usize) % 12];
            }
            let score = pearson(chroma, &rotated);
            if score > best.0 {
                second = best.0;
                best = (score, tonic, mode);
            } else if score > second {
                second = score;
            }
        }
    }
    let conf = (best.0 - second.max(-1.0)).clamp(0.0, 1.0);
    Key::new(best.1, best.2, conf)
}

/// Analyse complète d'une plage. `None` = plage trop courte ou muette.
pub fn analyse(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    start_frame: usize,
    end_frame: usize,
) -> Option<Key> {
    chroma(pcm, channels, rate, start_frame, end_frame).map(|c| best_key(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tonique_et_mode_sur_profil_ideal() {
        // profil majeur de do, puis mineur de la : la corrélation doit trancher
        let mut c = [0.0f32; 12];
        for i in 0..12 {
            c[i] = MAJOR[i];
        }
        assert_eq!(best_key(&c).tonic, 0);
        assert_eq!(best_key(&c).mode, Mode::Major);

        let mut a = [0.0f32; 12];
        for i in 0..12 {
            a[(i + 9) % 12] = MINOR[i];
        }
        let k = best_key(&a);
        assert_eq!((k.tonic, k.mode), (9, Mode::Minor));
    }

    #[test]
    fn sinus_la_donne_la_classe_la_et_silence_rend_none() {
        let rate = 8000u32;
        let pcm: Vec<f32> = (0..rate as usize * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let ch = chroma(&pcm, 1, rate, 0, pcm.len()).expect("chroma");
        let peak = (0..12).max_by(|a, b| ch[*a].partial_cmp(&ch[*b]).unwrap()).unwrap();
        assert_eq!(peak, 9, "la = classe 9 (do = 0)");
        // silence : aucune énergie exploitable
        let silence = vec![0.0f32; rate as usize];
        assert!(analyse(&silence, 1, rate, 0, silence.len()).is_none());
    }

    #[test]
    fn gamme_alignee_par_le_centre_du_mode() {
        let la_mineur = Key::new(9, Mode::Minor, 1.0);
        let do_majeur = Key::new(0, Mode::Major, 1.0);
        // la mineur naturelle et do majeur ont les mêmes notes : zéro transposition
        assert_eq!(la_mineur.transpose_for_scale(&do_majeur), 0);
        // ré majeur : deux demi-tons au-dessus (si mineur = même gamme que ré majeur)
        assert_eq!(la_mineur.transpose_for_scale(&Key::new(2, Mode::Major, 1.0)), 2);
        // plus court chemin, jamais au-delà de ±6
        assert_eq!(do_majeur.transpose_for_scale(&Key::new(11, Mode::Major, 1.0)), -1);
        assert!(do_majeur.transpose_for_scale(&Key::new(5, Mode::Major, 1.0)).abs() <= 6);
        assert!((Key::induced_semitones(1.5) - 7.019_550).abs() < 1e-4);
    }
}
