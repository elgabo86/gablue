//! Formes d'onde : pyramide min/max multi-résolution.
//!
//! `Decoded.peaks` (2400 tranches fixes) suffit à dessiner un morceau entier,
//! mais pas un clip de 3 s : au zoom on n'y trouve que quelques tranches, et
//! l'enveloppe devient un escalier. Une pyramide à trois niveaux donne la même
//! information à toute échelle, pour une mémoire fixe (~1 Mo par source de 3 min).

use std::sync::Arc;

/// Tranches (en trames) des niveaux, du plus fin au plus grossier.
const LEVELS: [usize; 3] = [64, 512, 4096];

/// Forme d'onde d'une source : nombre de trames et min/max par tranche.
#[derive(Clone, Debug, Default)]
pub struct Wave {
    pub frames: usize,
    pub rate: u32,
    pub channels: u16,
    /// `(trames par tranche, tranches)` — niveau 0 le plus fin.
    pub levels: Vec<(usize, Arc<Vec<(f32, f32)>>)>,
}

impl Wave {
    /// Construit la pyramide depuis un PCM entrelacé. Mono moyen (les canaux
    /// sont additionnés) : une forme d'onde de clip n'a pas de sens par canal
    /// dans une lane, et le coût mémoire triple sinon.
    pub fn build(pcm: &[f32], channels: u16, rate: u32) -> Self {
        let ch = channels.max(1) as usize;
        let frames = pcm.len() / ch;
        if frames == 0 {
            return Self { frames: 0, rate, channels, levels: Vec::new() };
        }
        let mut levels: Vec<(usize, Arc<Vec<(f32, f32)>>)> = Vec::new();
        let mut base: Vec<(f32, f32)> = Vec::with_capacity(frames / LEVELS[0] + 1);
        let mut i = 0usize;
        while i < frames {
            let end = (i + LEVELS[0]).min(frames);
            let (mut lo, mut hi) = (0.0f32, 0.0f32);
            for f in i..end {
                let v: f32 = pcm[f * ch..f * ch + ch].iter().sum::<f32>() / ch as f32;
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
            base.push((lo, hi));
            i = end;
        }
        levels.push((LEVELS[0], Arc::new(base)));
        // niveaux grossiers : agrégation du précédent, donc O(n) en tout
        for k in 1..LEVELS.len() {
            let prev = &levels[k - 1].1;
            let factor = LEVELS[k] / LEVELS[k - 1];
            let mut out = Vec::with_capacity(prev.len() / factor + 1);
            for chunk in prev.chunks(factor) {
                let mut lo = 0.0f32;
                let mut hi = 0.0f32;
                for (a, b) in chunk {
                    lo = lo.min(*a);
                    hi = hi.max(*b);
                }
                out.push((lo, hi));
            }
            levels.push((LEVELS[k], Arc::new(out)));
        }
        Self { frames, rate, channels, levels }
    }

    pub fn is_empty(&self) -> bool {
        self.frames == 0 || self.levels.is_empty()
    }

    /// Enveloppe d'une plage `[start, end)` (trames) en `cols` colonnes.
    /// Les colonnes hors plage valent zéro : l'appelant peut dessiner sans
    /// vérifier les bornes.
    pub fn range(&self, start: usize, end: usize, cols: usize) -> Vec<(f32, f32)> {
        let cols = cols.max(1);
        let end = end.min(self.frames);
        let start = start.min(end);
        let mut out = vec![(0.0f32, 0.0f32); cols];
        let span = end - start;
        if span == 0 || self.levels.is_empty() {
            return out;
        }
        let per_col = span as f64 / cols as f64;
        // niveau le plus grossier encore assez fin pour une colonne
        let mut chosen = &self.levels[0];
        for lv in &self.levels {
            if lv.0 as f64 <= per_col.max(1.0) {
                chosen = lv;
            } else {
                break;
            }
        }
        let (fps, buckets) = (&chosen.0, &chosen.1);
        for (c, slot) in out.iter_mut().enumerate() {
            let a = start + (span * c / cols);
            let b = start + (span * (c + 1) / cols);
            if b <= a {
                continue;
            }
            let i0 = a / fps;
            let i1 = ((b + fps - 1) / fps).max(i0 + 1).min(buckets.len());
            let mut lo = 0.0f32;
            let mut hi = 0.0f32;
            for (x, y) in &buckets[i0.min(buckets.len())..i1] {
                lo = lo.min(*x);
                hi = hi.max(*y);
            }
            *slot = (lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0));
        }
        out
    }

    /// Min/max d'une plage (pour un calcul de crête/attaque).
    pub fn peak(&self, start: usize, end: usize) -> (f32, f32) {
        let mut r = self.range(start, end, 1);
        r.pop().unwrap_or((0.0, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave() -> Wave {
        // 3 s à 1000 Hz mono : alternance +0,5 / -0,5 toutes les 100 trames
        let pcm: Vec<f32> = (0..3000)
            .map(|i| if (i / 100) % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        Wave::build(&pcm, 1, 1000)
    }

    #[test]
    fn pyramide_a_trois_niveaux() {
        let w = wave();
        assert_eq!(w.frames, 3000);
        assert_eq!(w.levels.len(), 3);
        assert_eq!(w.levels[0].0, 64);
        // le niveau 0 couvre bien toutes les trames
        assert_eq!(w.levels[0].1.len(), 3000usize.div_ceil(64));
    }

    #[test]
    fn enveloppe_par_colonne() {
        let w = wave();
        let env = w.range(0, 3000, 10);
        assert_eq!(env.len(), 10);
        // chaque colonne de 300 trames traverse les deux polarités
        assert!(env.iter().all(|(lo, hi)| *lo < -0.4 && *hi > 0.4));
        // une plage vide rend des zéros, pas une panique
        assert_eq!(w.range(3000, 3000, 4), vec![(0.0, 0.0); 4]);
    }

    #[test]
    fn plage_hors_bornes_bornee() {
        let w = wave();
        let env = w.range(2900, 99_999, 8);
        assert_eq!(env.len(), 8);
        assert!(env.iter().all(|(lo, hi)| *lo >= -1.0 && *hi <= 1.0));
        assert_eq!(w.peak(0, 50), (0.0, 0.5));
    }
}
