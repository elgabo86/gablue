//! Détection de la boucle d'un rendu audio.
//!
//! Certains rips (PSF) ne donnent que la durée totale (`length`), que zxtune
//! remplit en rejouant la séquence : la durée du tag n'est donc pas la période
//! de boucle. Boucler sur `length` rejoue la même partie (vécu 20/11, Ape
//! Escape « Dark Ruins » : `length=6:02` = 2 × 3:00.73) ou coupe en plein
//! milieu (vécu 20/11, « Western Land » : `length=2:43` = intro 54 s + 1,7 × 64 s).
//!
//! Méthode : enveloppe RMS du rendu (50 ms) et, pour chaque décalage, moyenne
//! des corrélations par fenêtre sur toute la piste (robuste à une intro qu'un
//! seul tour contient). Les vrais tours se répètent à l'échantillon près → on
//! affine la période sur le signal décimé. Le point de reprise est le premier
//! endroit dont le contenu se retrouve une période plus loin : il saute le
//! silence d'entrée (vécu « Stage Select » : 218 ms de blanc à chaque tour) et
//! l'intro éventuelle.

/// Boucle trouvée dans un rendu.
pub struct Estimate {
    /// Début de la boucle (fin du silence ou de l'intro).
    pub start: f64,
    /// Période en secondes.
    pub period: f64,
    /// Nombre de tours contenus dans le rendu.
    pub passes: u32,
}

impl Estimate {
    pub fn window(&self) -> (f64, f64) {
        (self.start, self.start + self.period)
    }
}

/// Cherche la boucle d'un rendu de `total` secondes, s'il en contient une.
/// `None` = aucune répétition nette (le morceau ne boucle pas).
pub fn detect(pcm: &[f32], channels: u16, rate: u32, total: f64) -> Option<Estimate> {
    let ch = channels.max(1) as usize;
    if total < 10.0 || pcm.len() < ch * rate as usize / 2 {
        return None;
    }

    // enveloppe RMS par tranches de 50 ms
    let hop = ((rate as f64 * 0.05) as usize).max(1) * ch;
    let env: Vec<f64> = pcm
        .chunks_exact(hop)
        .map(|blk| {
            let s: f64 = blk.iter().map(|v| (*v as f64) * (*v as f64)).sum();
            (s / blk.len() as f64).sqrt()
        })
        .collect();
    if env.len() < 64 {
        return None;
    }
    let hop_s = hop as f64 / (rate as f64 * channels.max(1) as f64);

    // 1) score de chaque décalage (pas de 50 ms : une grille plus grossière
    //    rate le pic quand la période est courte) : fraction des positions où
    //    le contenu se répète une période plus loin
    let m = (2.0 / hop_s).max(4.0) as usize;
    let posstep = if total < 60.0 {
        ((0.5 / hop_s) as usize).max(1)
    } else {
        ((1.0 / hop_s) as usize).max(1)
    };
    let mut scores: Vec<(f64, f64)> = Vec::new(); // (période, score)
    let mut best = (f64::NEG_INFINITY, 0.0);
    let mut lag = ((4.0 / hop_s) as usize).max(1);
    while lag + m < env.len() {
        let (mut good, mut n) = (0usize, 0usize);
        let mut i = 0usize;
        while i + lag + m <= env.len() {
            if corr_slice(&env[i..i + m], &env[i + lag..i + lag + m]) >= 0.85 {
                good += 1;
            }
            n += 1;
            i += posstep;
        }
        if n >= 8 {
            let score = good as f64 / n as f64;
            let period = lag as f64 * hop_s;
            scores.push((period, score));
            if score > best.0 {
                best = (score, period);
            }
        }
        lag += 1;
    }
    if best.0 < 0.20 {
        return None;
    }

    // 2) candidats à qualité comparable, du plus court au plus long (boucler
    //    sur deux tours ferait réentendre la même partie)
    // un rip PSF découpe en quelques tours ; un « passage » qui revient des
    // dizaines de fois est un motif, pas la boucle du morceau
    let plausible: Vec<(f64, f64)> = scores
        .iter()
        .copied()
        .filter(|(p, _)| total / *p <= 8.0)
        .collect();
    let top = plausible.iter().map(|c| c.1).fold(0.0f64, f64::max);
    if top < 0.20 {
        return None;
    }
    let mut cands: Vec<(f64, f64)> = plausible
        .into_iter()
        .filter(|(_, s)| *s >= top - 0.03)
        .collect();
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    // un pic d'enveloppe s'étale sur plusieurs pas : on garde le meilleur de
    // chaque groupe, du plus court décalage au plus long
    let mut picked: Vec<(f64, f64)> = Vec::new();
    for (p, s) in cands {
        match picked.last_mut() {
            Some(last) if (p - last.0).abs() <= 0.75 => {
                if s > last.1 {
                    *last = (p, s);
                }
            }
            _ => picked.push((p, s)),
        }
        if picked.len() >= 6 {
            break;
        }
    }

    // 3) affinage à l'échantillon près du plus court candidat convaincant
    let dec = ((rate as usize) / 8000).max(1);
    let mono: Vec<f32> = pcm
        .chunks_exact(dec * ch)
        .map(|blk| blk.iter().sum::<f32>() / blk.len() as f32)
        .collect();
    let mrate = rate as f64 / dec as f64;

    let mut chosen: Option<(f64, f64)> = None; // (période, score)
    for (p, _) in &picked {
        let Some((refined, score)) = refine(&mono, mrate, *p, total) else {
            continue;
        };
        if score < 0.5 {
            continue;
        }
        // à qualité comparable on garde la période la plus courte : boucler
        // sur deux tours ferait réentendre la même partie
        if chosen.as_ref().map_or(true, |c| score > c.1 + 0.02) {
            chosen = Some((refined, score));
        }
    }
    let (mut period, _) = chosen?;

    // 4) point de reprise, recalculé avec la période affinée
    let mut start = find_start(&env, hop_s, &mono, mrate, period);

    // 5) recalage local de la couture : la période globale reste à quelques ms
    //    près (un rendu PSF n'est pas reproductible d'un tour à l'autre), et
    //    c'est à la couture que ça s'entend. On compare une fenêtre courte
    //    autour du début de boucle et une période plus loin — court, sinon la
    //    réverbération noie la comparaison.
    let seam = (0.25 * mrate) as usize;
    let i = (start * mrate) as usize;
    if seam > 64 && i + seam < mono.len() {
        let j0 = ((start + period) * mrate) as usize;
        let mut best_d = 0.0;
        let mut best_c = if j0 + seam < mono.len() {
            sample_corr(&mono[i..i + seam], &mono[j0..j0 + seam])
        } else {
            0.0
        };
        let mut k = -0.030 * mrate;
        while k <= 0.030 * mrate {
            let j = (start + period) * mrate + k;
            if j < 0.0 {
                k += 1.0;
                continue;
            }
            let j = j as usize;
            if j + seam < mono.len() {
                let c = sample_corr(&mono[i..i + seam], &mono[j..j + seam]);
                if c > best_c {
                    best_c = c;
                    best_d = k / mrate;
                }
            }
            k += 1.0;
        }
        // le recadrage ne vaut que s'il améliore nettement la couture
        if best_c >= 0.5 && best_d.abs() > 1.0 / mrate {
            period += best_d;
            start = find_start(&env, hop_s, &mono, mrate, period);
        }
    }
    let passes = ((total - start) / period).round().max(0.0) as u32;
    if period < 4.0 || start + period > total + 0.05 {
        return None;
    }
    Some(Estimate {
        start,
        period,
        passes: passes.max(2),
    })
}

/// Corrélation normalisée de deux tranches.
fn corr_slice(x: &[f64], y: &[f64]) -> f64 {
    let m = x.len().min(y.len());
    if m < 4 {
        return 0.0;
    }
    let (x, y) = (&x[..m], &y[..m]);
    let mx = x.iter().sum::<f64>() / m as f64;
    let my = y.iter().sum::<f64>() / m as f64;
    let (mut num, mut ex, mut ey) = (0.0f64, 0.0f64, 0.0f64);
    for (p, q) in x.iter().zip(y.iter()) {
        let dp = p - mx;
        let dq = q - my;
        num += dp * dq;
        ex += dp * dp;
        ey += dq * dq;
    }
    if ex <= 1e-12 || ey <= 1e-12 {
        return 0.0;
    }
    num / (ex.sqrt() * ey.sqrt())
}

/// Corrélation (Pearson) de deux tranches de `mono`. Sans le retrait des
/// moyennes, une composante continue ou un bourdon domine le score et le pic
/// de période devient invisible (vécu 20/11 : score 0,43 partout, détection
/// rejetée sur « Stage Select » et « Dark Ruins »).
fn sample_corr(x: &[f32], y: &[f32]) -> f64 {
    let m = x.len().min(y.len());
    if m < 4 {
        return 0.0;
    }
    let (x, y) = (&x[..m], &y[..m]);
    let mx = x.iter().map(|v| *v as f64).sum::<f64>() / m as f64;
    let my = y.iter().map(|v| *v as f64).sum::<f64>() / m as f64;
    let (mut num, mut ex, mut ey) = (0.0f64, 0.0f64, 0.0f64);
    for (p, q) in x.iter().zip(y.iter()) {
        let dp = *p as f64 - mx;
        let dq = *q as f64 - my;
        num += dp * dq;
        ex += dp * dp;
        ey += dq * dq;
    }
    if ex <= 1e-12 || ey <= 1e-12 {
        return 0.0;
    }
    num / (ex.sqrt() * ey.sqrt())
}

/// Affine la période sur le signal décimé.
///
/// Les fenêtres sont prises dans la seconde moitié du rendu (donc dans un tour
/// répété, pas dans l'intro) et on garde la meilleure : un rendu PSF n'est pas
/// identique à l'échantillon près d'un tour à l'autre (état de réverbération),
/// et moyenner dilue justement le pic qu'on cherche (mesuré : 0,43 partout sur
/// « Stage Select », détection perdue). Le prix est qu'une coïncidence peut
/// faire gagner quelques ms — d'où un fondu croisé court à la couture.
fn refine(mono: &[f32], mrate: f64, period: f64, total: f64) -> Option<(f64, f64)> {
    let span = total - period;
    if span < 4.0 {
        return None;
    }
    let win = ((1.0 * mrate) as usize).max(64);
    let span_s = period * mrate;
    let starts: Vec<usize> = [0.5, 0.65, 0.8, 0.9]
        .iter()
        .map(|f| ((span * f) * mrate) as usize)
        .filter(|i| *i + win + (span_s + 0.3 * mrate) as usize <= mono.len())
        .collect();
    if starts.is_empty() {
        return None;
    }
    let score_at = |d: f64| -> f64 {
        let mut best = 0.0f64;
        for &i in &starts {
            let j = i as f64 + (period + d) * mrate;
            if j < 0.0 {
                continue;
            }
            let j = j as usize;
            if i + win > mono.len() || j + win > mono.len() {
                continue;
            }
            let c = sample_corr(&mono[i..i + win], &mono[j..j + win]);
            if c > best {
                best = c;
            }
        }
        best
    };
    // balayage à 2 ms puis affinage à l'échantillon
    let mut best = (f64::NEG_INFINITY, 0.0f64);
    let mut d = -0.30;
    while d <= 0.30 {
        let s = score_at(d);
        if s > best.0 {
            best = (s, d);
        }
        d += 0.002;
    }
    if !best.0.is_finite() {
        return None;
    }
    let center = best.1;
    let mut d = center - 0.003;
    while d <= center + 0.003 {
        let s = score_at(d);
        if s > best.0 {
            best = (s, d);
        }
        d += 0.5 / mrate;
    }
    Some((period + best.1, best.0))
}

/// Point de reprise. Deux cas : un silence d'entrée que zxtune ne rejoue pas,
/// ou une intro franche suivie d'une partie franchement répétée. En l'absence
/// de transition nette on ne coupe rien : mieux vaut rejouer un peu d'intro que
/// décaler une boucle au hasard (vécu 20/11, « Dark Ruins » : rendu PSF qui
/// varie à l'échantillon près, aucune frontière fiable avant 2,7 s).
fn find_start(env: &[f64], hop_s: f64, mono: &[f32], mrate: f64, period: f64) -> f64 {
    let silence = leading_silence(env, hop_s, mono, mrate);

    let win = (2.0 / hop_s).max(4.0) as usize;
    let lag = (period / hop_s).round() as usize;
    let step = ((0.25 / hop_s) as usize).max(1);
    if lag == 0 || lag + win >= env.len() {
        return silence;
    }
    // corrélation par position sur la piste
    let mut curve: Vec<f64> = Vec::new();
    let mut i = 0usize;
    while i + lag + win <= env.len() {
        curve.push(corr_slice(&env[i..i + win], &env[i + lag..i + lag + win]));
        i += step;
    }
    // une intro de 8 s au moins, puis 8 s de contenu franchement répété
    let need = ((8.0 / hop_s) as usize / step).max(1);
    if curve.len() < 2 * need {
        return silence;
    }
    for k in need..curve.len().saturating_sub(need) {
        let after = curve[k..k + need].iter().sum::<f64>() / need as f64;
        if after < 0.85 {
            continue;
        }
        let before = &curve[k - need..k];
        let mean = before.iter().sum::<f64>() / need as f64;
        if mean < 0.60 {
            // la moyenne décale le début : on avance jusqu'au premier point
            // franchement répété
            let mut kk = k;
            while kk + 1 < curve.len() && curve[kk] < 0.85 {
                kk += 1;
            }
            return silence.max((kk * step) as f64 * hop_s);
        }
    }
    silence
}

/// Silence d'entrée à sauter : renvoie 0 s'il n'y en a pas.
fn leading_silence(env: &[f64], hop_s: f64, mono: &[f32], mrate: f64) -> f64 {
    let peak = env.iter().cloned().fold(0.0f64, f64::max);
    if peak <= 0.0 {
        return 0.0;
    }
    let thr = peak * 0.02;
    let Some(idx) = env.iter().position(|v| *v >= thr) else {
        return 0.0;
    };
    let rough = idx as f64 * hop_s;
    // un silence de plus de 5 s n'est pas un délai de séquence : on n'y touche pas
    if rough < 0.04 || rough > 5.0 {
        return 0.0;
    }
    // affinage à 1 ms sur le signal décimé (fenêtre RMS de 10 ms)
    let win = ((0.01 * mrate) as usize).max(16);
    let from = (((rough - 0.10).max(0.0)) * mrate) as usize;
    let to = (((rough + 0.10) * mrate) as usize).min(mono.len().saturating_sub(win));
    let mut i = from;
    while i < to {
        let seg = &mono[i..i + win];
        let rms = (seg.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / seg.len() as f64)
            .sqrt();
        if rms >= thr {
            // on garde l'attaque : 5 ms avant le premier dépassement
            return ((i as f64 / mrate) - 0.005).max(0.0);
        }
        i += 1;
    }
    rough
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 0.8
            })
            .collect()
    }

    /// Bloc d'un tour : bruit avec une enveloppe percussive aléatoire (du bruit
    /// plat aurait une courbe RMS sans information ; une enveloppe régulière
    /// ferait ressembler deux blocs différents).
    fn body(period: f64, rate: u32, seed: u64, hit_rate: f64) -> Vec<f32> {
        let p = (period * rate as f64).round() as usize;
        let raw = noise(p, seed);
        // un vrai signal n'est pas du bruit blanc : il reste corrélé sur
        // quelques millisecondes (sinon l'affinage à 2 ms ne peut pas le suivre)
        let k = (rate as usize / 200).max(2);
        let mut base = vec![0.0f32; p];
        let mut acc = 0.0f32;
        for i in 0..p {
            acc += raw[i];
            if i >= k {
                acc -= raw[i - k];
            }
            base[i] = acc / k as f32;
        }
        let hits = noise((period * hit_rate) as usize + 1, seed ^ 0x7777);
        let mut shape = vec![0.15f32; p];
        let len = (0.18 * rate as f64) as usize;
        for h in hits {
            let pos = (((h + 0.5) * p as f32) as usize).min(p - 1);
            let amp = 0.3 + (h.abs() * 1.4);
            for i in 0..len.min(p) {
                let idx = (pos + i) % p;
                let v = amp * (1.0 - i as f32 / len as f32).powi(2);
                if v > shape[idx] {
                    shape[idx] = v;
                }
            }
        }
        base.iter()
            .zip(shape)
            .map(|(v, s)| v * s)
            .collect()
    }

    /// Intro unique + tours identiques, dernier tour coupé comme le fait
    /// `length` (cas « Western Land »).
    fn render(intro: f64, period: f64, passes: f64, rate: u32) -> (Vec<f32>, f64) {
        let p = (period * rate as f64).round() as usize;
        // l'intro est d'un autre caractère (frappes rares) : rien ne doit
        // permettre de la confondre avec le tour qui boucle
        let mut pcm = if intro > 0.0 {
            body(intro, rate, 0x1234, 0.7)
        } else {
            Vec::new()
        };
        let block = body(period, rate, 0x5eed, 4.0);
        let total_passes = passes as usize;
        for k in 0..total_passes {
            let gain = 1.0 / (1.0 + 0.05 * k as f32);
            let hiss = noise(p, 0xabc + k as u64);
            pcm.extend(
                block
                    .iter()
                    .zip(hiss)
                    .map(|(v, h)| (v * gain) + h * 0.01),
            );
        }
        // queue partielle du tour suivant
        let frac = passes - passes.floor();
        let cut = (p as f64 * frac) as usize;
        if cut > 0 {
            let hiss = noise(cut, 0xdead);
            pcm.extend(
                block[..cut]
                    .iter()
                    .zip(hiss)
                    .map(|(v, h)| (v * 0.6) + h * 0.01),
            );
        }
        let total = pcm.len() as f64 / rate as f64;
        (pcm, total)
    }

    #[test]
    fn deux_tours() {
        let (pcm, total) = render(0.0, 12.0, 2.0, 22050);
        let e = detect(&pcm, 1, 22050, total).expect("période trouvée");
        assert!((e.period - 12.0).abs() < 0.05, "période {}", e.period);
        assert!(e.start < 0.05, "début {}", e.start);
    }

    #[test]
    fn quatre_tours() {
        let (pcm, total) = render(0.0, 9.0, 4.0, 22050);
        let e = detect(&pcm, 1, 22050, total).expect("période trouvée");
        assert!((e.period - 9.0).abs() < 0.05, "période {}", e.period);
        assert_eq!(e.passes, 4);
    }

    #[test]
    fn silence_d_entree() {
        let rate = 22050;
        let (mut pcm, total) = render(0.0, 8.0, 2.0, rate);
        let lead = (0.25 * rate as f64) as usize;
        let mut out = vec![0.0f32; lead];
        out.extend_from_slice(&pcm);
        pcm = out;
        let e = detect(&pcm, 1, rate, total + 0.25).expect("période trouvée");
        assert!((e.start - 0.25).abs() < 0.03, "début {}", e.start);
        assert!((e.period - 8.0).abs() < 0.05, "période {}", e.period);
    }

    /// Intro non silencieuse de 14 s, puis 2 tours et demi de 8 s
    /// (`total` n'est pas un multiple de la période).
    #[test]
    fn intro_et_tours_partiels() {
        let (pcm, total) = render(14.0, 8.0, 2.5, 22050);
        let e = detect(&pcm, 1, 22050, total).expect("période trouvée");
        assert!((e.period - 8.0).abs() < 0.05, "période {}", e.period);
        // la fenêtre d'enveloppe fait 2 s : la frontière n'est pas au dixième
        assert!((e.start - 14.0).abs() < 0.9, "début {}", e.start);
    }

    #[test]
    fn sans_repetition() {
        let rate = 22050;
        let pcm = noise(rate as usize * 30, 7);
        assert!(detect(&pcm, 1, rate, 30.0).is_none());
    }
}


/// Recale une période **annoncée par le format** (durée du `.m3u`) sur la
/// répétition réelle du rendu.
///
/// Les durées des `.m3u` de joshw sont des secondes entières : elles tombent
/// souvent un peu court, et chaque tour perd alors un bout de musique (vécu
/// 21/11, Kirby's Adventure NES : « Vegetable Valley » annoncé 38 s, boucle
/// réelle 38,405 s → il manquait un temps à chaque tour). Le rendu de zxtune
/// contient plusieurs tours, donc l'enveloppe RMS se répète exactement même
/// quand le signal, lui, ne se répète pas à l'échantillon près (le driver émulé
/// n'est pas reproductible à l'échantillon). On garde la **médiane** des
/// corrélations sur toutes les positions : un motif interne ne tient pas
/// partout, la vraie boucle si.
pub fn refine_announced(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    total: f64,
    announced: f64,
) -> Option<f64> {
    let ch = channels.max(1) as usize;
    if announced < 4.0 || total < announced * 1.5 {
        return None;
    }
    // enveloppe sur 5 ms : une résolution de 20 ms laissait 85 ms d'écart sur
    // la période retenue (mesuré Kirby NES), alors que le seuil de coupure d'un
    // temps s'entend
    let hop = ((rate as f64 * 0.005) as usize).max(1);
    let env: Vec<f64> = pcm
        .chunks_exact(hop * ch)
        .map(|blk| {
            let s: f64 = blk.iter().map(|v| (*v as f64) * (*v as f64)).sum();
            (s / blk.len() as f64).sqrt()
        })
        .collect();
    let hop_s = hop as f64 / rate as f64;
    let win = (6.0 / hop_s) as usize;
    if env.len() < win * 3 {
        return None;
    }
    let step = (win / 2).max(1);
    // ±0,8 s : au-delà, ce n'est plus un arrondi mais une autre boucle
    let lag_min = (((announced - 0.8) / hop_s).round() as usize).max(1);
    let lag_max = ((announced + 0.8) / hop_s).round() as usize;
    let mut best = (f64::NEG_INFINITY, 0.0f64);
    for lag in lag_min..=lag_max {
        let mut vals: Vec<f64> = Vec::new();
        let mut i = 0usize;
        while i + lag + win <= env.len() {
            vals.push(corr_slice(&env[i..i + win], &env[i + lag..i + lag + win]));
            i += step;
        }
        if vals.len() < 6 {
            continue;
        }
        vals.sort_by(|a, b| a.total_cmp(b));
        let median = vals[vals.len() / 2];
        if median > best.0 {
            best = (median, lag as f64 * hop_s);
        }
    }
    // sous ce seuil, le rendu ne se répète pas nettement à cette période
    if best.0 < 0.90 {
        return None;
    }
    Some(best.1)
}

