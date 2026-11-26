//! Tempo (BPM) et grille de temps d'un rendu audio.
//!
//! Sert à découper une boucle **musicale** (un temps, une mesure) au lieu de
//! couper au pixel près de la souris : sans grille, une boucle « mini partie »
//! tombe au milieu d'une attaque et la couture s'entend, même avec le fondu
//! croisé de 2 ms.
//!
//! Méthode, volontairement bornée (≤ 60 s analysées, mono décimé à ~11 kHz) :
//! enveloppe d'attaque (diff demi-redressée du log de l'énergie par trame),
//! autocorrélation pour le tempo, puis recherche de phase sur la grille. Le
//! tempo des bandes-son de jeu est tenu par le tracker (aucune dérive) : une
//! phase ancrée au début reste juste jusqu'à la fin du morceau, et le coût
//! reste de quelques millisecondes, une fois par décodage (l'analyse vit dans
//! l'entrée du cache mémoire, comme la détection de boucle).

/// Tempo estimé et grille de temps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    /// Durée d'un temps (noire) en secondes.
    pub beat: f64,
    /// Position d'un temps de la grille. La détection la donne dans
    /// `0 ≤ phase < beat` ; un décalage manuel peut la faire sortir de cet
    /// intervalle, ce qui est **voulu** : un décalage d'un temps ne change pas
    /// la grille de temps mais change le départ des mesures (`cut` prend
    /// `phase mod longueur de tranche`).
    pub phase: f64,
    /// Corrélation de l'enveloppe d'attaque à la période retenue (0…1) : sous
    /// ~0,15 il n'y a pas de rythme exploitable (nappe, bruit, morceau lent).
    pub confidence: f64,
}

impl Tempo {
    /// Mesure 4/4 : le découpage propose des multiples de 4 temps. Les bandes-son
    /// de jeu qui sont en 3/4 restent utilisables (on boucle alors 4 temps, soit
    /// 1 mesure + 1 temps) : mieux vaut une grille de temps juste qu'un métre
    /// deviné faux.
    pub const BEATS_PER_BAR: f64 = 4.0;

    /// Position du temps le plus proche de `t`.
    pub fn snap(&self, t: f64) -> f64 {
        if self.beat <= 0.0 {
            return t.max(0.0);
        }
        let k = ((t - self.phase) / self.beat).round();
        (self.phase + k * self.beat).max(0.0)
    }

    /// Durée de `bars` mesures (4/4) en secondes.
    pub fn bars(&self, bars: f64) -> f64 {
        bars * Self::BEATS_PER_BAR * self.beat
    }

    /// Même grille, décalée de `secs` (négatif accepté). Sert au réglage
    /// manuel : sur un morceau dont la première attaque tombe entre deux
    /// temps, la grille détectée est juste mais pas au bon endroit.
    ///
    /// La phase n'est **pas** ramenée dans `[0, beat)` : un décalage d'un
    /// temps doit décaler le départ des mesures (sinon `+ temps` ne ferait
    /// rien du tout sur une découpe d'une mesure).
    pub fn shifted(&self, secs: f64) -> Tempo {
        let mut t = *self;
        if self.beat > 0.0 && secs.is_finite() {
            t.phase = self.phase + secs;
        }
        t
    }

    /// Même musique, tempo corrigé d'un facteur (`0.5` = moitié, `2` =
    /// double). Sert quand l'autocorrélation a choisi l'octave voisine.
    pub fn scaled(&self, factor: f64) -> Tempo {
        let mut t = *self;
        if factor > 0.0 && factor.is_finite() {
            t.bpm = self.bpm * factor;
            t.beat = self.beat / factor;
            if t.beat > 0.0 {
                t.phase = self.phase.rem_euclid(t.beat);
            }
        }
        t
    }

    /// Tempo recalé pour qu'un nombre **entier** de temps tombe pile sur
    /// `span` (la fin de boucle). Un BPM estimé juste à 0,2 % près dérive d'un
    /// demi-temps sur 40 s : la dernière coupe tombe alors au milieu d'une
    /// attaque et « il manque un mini bout » (vécu 21/11, demandé par
    /// l'utilisateur : « c'est pas le bpm qui est faux ? »). On ne bouge que si
    /// le BPM corrigé reste à moins de 3 % de l'estimé (sinon c'est `span` qui
    /// n'est pas un multiple de temps, et on ne force pas).
    pub fn across(&self, span: f64) -> Tempo {
        if !(span > 0.0) || self.bpm <= 0.0 || self.beat <= 0.0 {
            return *self;
        }
        let beats = (span / self.beat).round();
        if beats < 4.0 {
            return *self;
        }
        let bpm = 60.0 * beats / span;
        if (bpm / self.bpm - 1.0).abs() > 0.03 {
            return *self;
        }
        self.with_bpm(bpm)
    }

    /// Grille de secours, quand ni le réseau ni l'enveloppe ne donnent de
    /// pouls : un temps par demi-seconde (120 BPM), ancré au début. Elle ne
    /// prétend rien de musical, elle **permet de découper** — et la forme
    /// d'onde montre tout de suite qu'elle ne colle pas aux attaques.
    /// `confidence` vaut 0 pour que l'interface le dise.
    pub fn fallback() -> Tempo {
        Tempo {
            bpm: 120.0,
            beat: 0.5,
            phase: 0.0,
            confidence: 0.0,
        }
    }

    /// Tempo corrigé pour viser `bpm` (réglage fin au BPM près).
    pub fn with_bpm(&self, bpm: f64) -> Tempo {
        if bpm > 20.0 {
            self.scaled(bpm / self.bpm)
        } else {
            *self
        }
    }
}

/// Enveloppe d'attaque d'un rendu, sur toute sa durée : c'est elle qui permet
/// de recaler les frontières de coupe sur les attaques réelles (voir `cut_on`).
#[derive(Clone)]
pub struct Envelope {
    /// Flux d'attaque par trame (variation positive du log de l'énergie).
    pub flux: Vec<f64>,
    /// Durée d'une trame, en secondes.
    pub hop: f64,
}

/// Enveloppe d'attaque : mono décimé vers ~11 kHz, trames de 20 ms espacées de
/// 5 ms. Une trame longue décale l'attaque de ~20 ms, ce qui déplace tout le
/// morceau une fois la grille posée (vécu 21/11 : phase 26 ms trop tôt).
fn attack_flux(pcm: &[f32], channels: u16, rate: u32) -> Option<(Vec<f64>, f64)> {
    let ch = channels.max(1) as usize;
    if rate == 0 || pcm.len() < ch * rate as usize {
        return None;
    }
    let dec = ((rate as usize) / 11025).max(1);
    let mono: Vec<f32> = pcm
        .chunks_exact(dec * ch)
        .map(|b| b.iter().sum::<f32>() / b.len() as f32)
        .collect();
    let mr = rate as f64 / dec as f64;
    if mono.len() < (4.0 * mr) as usize {
        return None;
    }
    let frame = ((0.020 * mr) as usize).max(32);
    let hop = ((0.005 * mr) as usize).max(4);
    let hop_s = hop as f64 / mr;
    let mut flux = Vec::with_capacity(mono.len() / hop + 1);
    let mut prev = 0.0f64;
    let mut i = 0usize;
    while i + frame <= mono.len() {
        let e: f64 = mono[i..i + frame]
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            / frame as f64;
        let level = (e + 1e-9).ln();
        // le log-diff est déjà centré : retirer en plus une moyenne glissante
        // écrasait les rythmes rapides (un temps toutes les 0,4 s tombe dans la
        // fenêtre de 0,5 s, vécu 21/11 : thème de SMB non détecté)
        flux.push((level - prev).max(0.0));
        prev = level;
        i += hop;
    }
    if flux.len() < 64 {
        return None;
    }
    Some((flux, hop_s))
}

/// Enveloppe du rendu entier, pour recaler les coupes sur les attaques.
pub fn envelope(pcm: &[f32], channels: u16, rate: u32) -> Option<Envelope> {
    let (flux, hop) = attack_flux(pcm, channels, rate)?;
    Some(Envelope { flux, hop })
}

/// Attaque la plus nette à ±`win` secondes de `t`. `None` = rien de franc dans
/// la fenêtre (note tenue, silence) : la grille reste la référence.
fn nearest_attack(env: &Envelope, t: f64, win: f64) -> Option<f64> {
    let n = env.flux.len();
    if n < 3 || env.hop <= 0.0 || t <= 0.0 {
        return None;
    }
    let i0 = ((t - win) / env.hop).max(0.0) as usize;
    let i1 = (((t + win) / env.hop) as usize + 1).min(n);
    if i1 <= i0 + 2 {
        return None;
    }
    let (mut vmax, mut jmax, mut sum) = (0.0f64, i0, 0.0f64);
    for j in i0..i1 {
        let v = env.flux[j];
        sum += v;
        if v > vmax {
            vmax = v;
            jmax = j;
        }
    }
    let mean = sum / (i1 - i0) as f64;
    // une attaque franche seulement : sur du bruit, la moyenne est du même
    // ordre que le max et la grille reste plus fiable
    if vmax <= 1e-9 || vmax < mean * 1.8 {
        return None;
    }
    Some(jmax as f64 * env.hop)
}

/// Découpe `[0, whole]` en tranches de `beats` temps, **recalées sur les
/// attaques réelles**.
///
/// La grille donne la structure, l'enveloppe corrige la dérive : un BPM estimé
/// 0,1 % faux décale la fin d'un morceau de 3 min de ~0,4 s, et les dernières
/// coupes tombent alors au milieu des attaques (vécu 21/11 : « petit à petit
/// ça se décale »).
///
/// Le recalage est **séquentiel** : chaque frontière est cherchée autour de la
/// précédente *réellement trouvée* plus une tranche, jamais autour de la grille
/// théorique — sinon la dérive accumulée sort de la fenêtre de recherche et les
/// dernières tranches restent fausses.
pub fn cut_on(
    whole: f64,
    tempo: &Tempo,
    beats: f64,
    env: Option<&Envelope>,
) -> Vec<crate::model::LoopWindow> {
    let grid = cut(whole, tempo, beats);
    let Some(env) = env else {
        return grid;
    };
    if grid.len() < 2 || tempo.beat <= 0.0 || !whole.is_finite() {
        return grid;
    }
    let len0 = tempo.beat * beats.max(1.0);
    if len0 <= 0.05 {
        return grid;
    }
    // Grille ajustée **globalement** (une ancre, une période) puis découpage
    // strictement périodique : toutes les tranches durent la même chose.
    let (anchor, len) = fit_grid(env, tempo, beats, whole);
    if std::env::var("RP_CUT_DEBUG").is_ok() {
        eprintln!(
            "grille ajustée : ancre {:.3} (théorique {:.3}) · période {:.4} (théorique {:.4})",
            anchor,
            tempo.phase,
            len,
            len0
        );
    }
    // Bornes **strictement périodiques** : toutes les tranches durent `len`,
    // seule la queue (fin de boucle) est à part. Une vraie intro (ancre ≥ 35 %
    // d'une tranche) reste une tranche ; une tête courte est **jetée** —
    // silencieuse ou non. La fondre dans la première tranche la rendait plus
    // longue que les autres (GoldenEye « Bunker 1 » : 4,084 s contre 4,004,
    // demande user 21/11 « ils doivent être tous du même temps, bordel »).
    // Elle est de toute façon hors mesure (moins de 35 % d'une tranche) et
    // `loop_win.start`/l'ancre suivent le premier temps, pas 0.
    let out = cut_from(whole, anchor, len);
    if out.is_empty() {
        return grid;
    }
    out
}

/// Découpe **strictement périodique** à partir d'une grille déjà ajustée :
/// toutes les tranches durent `len`, l'intro (≥ 35 % d'une tranche) reste une
/// tranche à part et seule la dernière peut être plus courte.
///
/// Extraite de `cut_on` pour que l'arrangement et la forme d'onde découpent
/// avec **exactement** la même grille que l'éditeur. Sans ça, l'arrangement
/// recalculait `beat × pas` avec le BPM détecté (faux de 0,1 % sur un morceau
/// N64) : les tuiles dérivaient de quelques ms par tranche, soit 300 ms au bout
/// de la piste — « les séquences se décalent de plus en plus loin du départ »
/// (vécu 26/11).
pub fn cut_from(whole: f64, anchor: f64, len: f64) -> Vec<crate::model::LoopWindow> {
    let mut out: Vec<crate::model::LoopWindow> = Vec::new();
    if len <= 0.05 || whole <= 0.5 || !whole.is_finite() || !len.is_finite() {
        return out;
    }
    let anchor = if anchor.is_finite() { anchor } else { 0.0 };
    let mut t;
    if anchor > 0.02 && anchor >= len * 0.35 {
        out.push(crate::model::LoopWindow {
            start: 0.0,
            end: anchor,
        });
        t = anchor;
    } else if anchor > 0.02 {
        // Tête courte : elle est **absorbée** par la première tranche (demande
        // user 26/11). Avant, elle était jetée pour que toutes les tranches
        // durent pareil (demande 21/11), mais cette tête est une **anacrouse**
        // — de la musique — et la jeter faisait commencer le montage quelques
        // dizaines de ms après le début de la piste (« il manque un mini bout
        // au début de chaque musique »). Seule la première tranche est un peu
        // plus longue, et la grille reste sur les attaques.
        let end = (anchor + len).min(whole);
        out.push(crate::model::LoopWindow { start: 0.0, end });
        t = end;
    } else {
        // ancre négative : la grille commence **avant** 0, on avance d'une
        // tranche pour que la tête reste une tranche (même règle que `cut`).
        // Sans ça, `t = 0` perdait le demi-temps qui sépare la grille du corps
        // du morceau (vécu 26/11).
        t = anchor;
        if t < 0.0 && len > 0.0 {
            t -= (t / len).floor() * len;
        }
        t = t.max(0.0);
        if t > 0.02 {
            // la tête (avant la première ligne de grille) est de la musique :
            // elle reste une tranche, éventuellement plus courte
            out.push(crate::model::LoopWindow { start: 0.0, end: t });
        }
    }
    while t + len <= whole - 0.05 {
        out.push(crate::model::LoopWindow {
            start: t,
            end: t + len,
        });
        t += len;
    }
    if whole - t > 0.15 {
        if whole - t < len * 0.35 {
            match out.last_mut() {
                Some(last) => last.end = whole,
                None => out.push(crate::model::LoopWindow { start: t, end: whole }),
            }
        } else {
            out.push(crate::model::LoopWindow {
                start: t,
                end: whole,
            });
        }
    }
    out
}

/// Grille **réellement** utilisée par `cut_on` : ancre ajustée et période de
/// tranche (ancre, période). `None` = pas d'enveloppe, pas de grille.
///
/// Publique pour que tout ce qui *dessine* ou *découpe* une source (tuiles de
/// l'arrangement, repères de la forme d'onde, aimant) utilise la même période
/// que la découpe. Utiliser `tempo.beat` à la place fait dériver les tuiles de
/// 0,1 % par tranche (vécu 26/11 : « les séquences se décalent de plus en plus,
/// plus elles sont loin du départ du son »).
pub fn fitted(
    env: Option<&Envelope>,
    tempo: &Tempo,
    beats: f64,
    whole: f64,
) -> Option<(f64, f64)> {
    let env = env?;
    let len0 = tempo.beat * beats.max(1.0);
    if len0 <= 0.05 || whole <= 0.5 || !whole.is_finite() || !tempo.beat.is_finite() {
        return None;
    }
    Some(fit_grid(env, tempo, beats, whole))
}

/// Flux maximal dans ±`win` autour de `t` (0 hors du signal).
fn peak_flux(env: &Envelope, t: f64, win: f64) -> f64 {
    let n = env.flux.len();
    if n < 3 || env.hop <= 0.0 || t <= 0.0 {
        return 0.0;
    }
    let i0 = ((t - win) / env.hop).max(0.0) as usize;
    let i1 = (((t + win) / env.hop) as usize + 1).min(n);
    let mut vmax = 0.0f64;
    for j in i0..i1 {
        if env.flux[j] > vmax {
            vmax = env.flux[j];
        }
    }
    vmax
}

/// Force d'une attaque **franche** autour de `t` (0 si le maximum est noyé
/// dans la moyenne : sur du bruit, aucune attaque ne se détache).
fn attack_strength(env: &Envelope, t: f64, win: f64) -> f64 {
    let n = env.flux.len();
    if n < 3 || env.hop <= 0.0 || t <= 0.0 {
        return 0.0;
    }
    let i0 = ((t - win) / env.hop).max(0.0) as usize;
    let i1 = (((t + win) / env.hop) as usize + 1).min(n);
    if i1 <= i0 + 2 {
        return 0.0;
    }
    let (mut vmax, mut sum) = (0.0f64, 0.0f64);
    for j in i0..i1 {
        let v = env.flux[j];
        sum += v;
        if v > vmax {
            vmax = v;
        }
    }
    let mean = sum / (i1 - i0) as f64;
    if vmax <= 1e-9 || vmax < mean * 1.8 {
        0.0
    } else {
        vmax
    }
}

/// Qualité d'une grille de temps : fraction des temps qui tombent sur une
/// attaque franche, et force moyenne de ces attaques.
fn beat_quality(env: &Envelope, beat: f64, phase: f64, total: f64) -> (f64, f64) {
    let win = (beat * 0.12).min(0.06);
    let (mut n, mut hit, mut sum) = (0.0, 0.0, 0.0);
    let mut t = phase;
    while t < total - 0.05 {
        let v = attack_strength(env, t, win);
        if v > 0.0 {
            hit += 1.0;
            sum += v;
        }
        n += 1.0;
        t += beat;
    }
    if n <= 0.0 {
        return (0.0, 0.0);
    }
    (hit / n, sum / n)
}

/// Phase la plus juste pour une période donnée : balayage sur **±½ temps**.
///
/// La détection (réseau comme enveloppe) peut poser la grille un demi-temps à
/// côté — mesuré sur Banjo-Tooie « Cauldron Keep: Gun Chamber » : la grille du
/// réseau tombait 144 ms (0,27 temps) avant les attaques, et `fit_grid` ne
/// pouvait alors pas la corriger (son garde-fou était de ±60 ms). Résultat :
/// la frontière de chaque tranche tombait **après** l'attaque — « toutes les
/// séquences ont un bout en trop à la fin » (vécu 26/11). Le garde-fou est
/// passé à ±½ temps et l'ajustement est robuste, mais `pulse` reste le point de
/// départ : c'est lui qui donne à `fit_grid` une phase à moins d'un demi-temps,
/// et il couvre le cas où l'ajustement échoue (moins de 4 attaques suivies).
fn refine_phase(env: &Envelope, beat: f64, phase: f64, total: f64) -> f64 {
    // Fenêtre **étroite** (±8 ms) : une fenêtre large rendrait l'objectif plat
    // (le maximum d'une attaque est le même pour tout décalage qui la contient)
    // et la phase resterait jusqu'à 60 ms à côté de l'attaque — exactement le
    // défaut qu'on corrige.
    let win = 0.008;
    let mut best = (f64::NEG_INFINITY, phase);
    let mut o = -beat / 2.0;
    while o <= beat / 2.0 + 1e-9 {
        let p = phase + o;
        let mut sum = 0.0;
        let mut n = 0.0;
        let mut t = p;
        while t < total - 0.05 {
            sum += peak_flux(env, t, win);
            n += 1.0;
            t += beat;
        }
        let score = if n > 0.0 { sum / n } else { 0.0 };
        if score > best.0 {
            best = (score, p);
        }
        o += env.hop;
    }
    best.1
}

/// Ramène une phase dans la tuile en préférant l'origine **juste avant 0** :
/// une phase à 0,98 temps ferait commencer la grille un temps après le début du
/// morceau (la première tranche absorberait presque une mesure de plus) ; à
/// −0,02 temps la tête est négligeable. Les deux sont musicalement équivalentes.
fn origin_phase(phase: f64, beat: f64) -> f64 {
    let mut p = phase.rem_euclid(beat);
    if p > beat * 0.75 {
        p -= beat;
    }
    p
}

/// Recalage final du pouls : phase remise sur les attaques, et **octave** quand
/// la grille détectée ne tombe sur rien.
///
/// Mesuré sur cinq pistes (Banjo-Tooie ×3, FF8, un PSF) : les temps ajoutés par
/// un doublement sont systématiquement ~la moitié moins forts (rapport 0,44 à
/// 0,75), donc la force ne peut pas décider de l'octave — « Banjo's House
/// Blues » (84,97 BPM) et « Blue Fields » (160) seraient doublés à tort. Le seul
/// signal retenu est l'**erreur de phase** : quand la grille détectée est à plus
/// de 0,15 temps des attaques (Cauldron Keep : 0,27), c'est que la détection a
/// pris un niveau de subdivision ; on passe au double s'il tombe juste (0,97 de
/// couverture mesurée). Les autres pistes mesurées sont à moins de 0,05 temps
/// et gardent leur tempo.
///
/// **Limite assumée** : l'octave est la partie ambiguë (le repli d'enveloppe de
/// « Cauldron Keep » donne une période de 2,0866 s = 4 temps à 115,13 BPM, donc
/// le doublement fait des tuiles d'une demi-mesure). L'essai de le supprimer a
/// été **annulé** le 26/11 après un « tout est faux » à l'écoute : on revient au
/// comportement que l'utilisateur jugeait meilleur, et le BPM reste réglable au
/// badge (alt+molette = ×2/÷2, « auto » pour revenir).
pub fn pulse(env: &Envelope, tempo: &Tempo) -> Tempo {
    let total = env.flux.len() as f64 * env.hop;
    if total < 8.0 || !(tempo.beat > 0.0) || !tempo.bpm.is_finite() {
        return *tempo;
    }
    let phase = refine_phase(env, tempo.beat, tempo.phase, total);
    let mut err = (phase - tempo.phase).abs() / tempo.beat;
    err = err.min(1.0 - err);
    let mut out = Tempo {
        bpm: tempo.bpm,
        beat: tempo.beat,
        phase: origin_phase(phase, tempo.beat),
        confidence: tempo.confidence,
    };
    let bpm2 = tempo.bpm * 2.0;
    if err > 0.15 && bpm2 <= 260.0 {
        let beat2 = 60.0 / bpm2;
        let phase2 = refine_phase(env, beat2, tempo.phase, total);
        let (cov2, _) = beat_quality(env, beat2, phase2, total);
        if std::env::var("RP_GRID_DEBUG").is_ok() {
            eprintln!(
                "pouls : phase {:.4} (détectée {:.4}, erreur {:.2} temps) · octave 2× {:.2} BPM couverture {:.2} phase {:.4}",
                phase, tempo.phase, err, bpm2, cov2, phase2
            );
        }
        if cov2 >= 0.90 {
            out = Tempo {
                bpm: bpm2,
                beat: beat2,
                phase: origin_phase(phase2, beat2),
                confidence: tempo.confidence,
            };
        }
    }
    out
}

/// Ajuste la grille sur les attaques réelles, en **global** : une ancre et une
/// période pour tout le morceau, ajustées aux moindres carrés sur les attaques
/// fiables. Le recollage frontière par frontière (avant le 21/11) suivait mieux
/// les micro-écarts mais donnait des tranches de durées différentes — 2,76 /
/// 2,74 / 2,68 s sur FF8 « Force Your Way » — alors qu'une mesure doit durer
/// une mesure. La période ajustée absorbe aussi l'erreur de BPM (0,1 % faux =
/// 0,4 s de dérive sur 3 min), ce qui était la raison du recollage séquentiel.
fn fit_grid(env: &Envelope, tempo: &Tempo, beats: f64, whole: f64) -> (f64, f64) {
    let len0 = tempo.beat * beats.max(1.0);
    let win = tempo.beat * 0.25;
    // Phase **négative** (bouton « − mesure ») : l'origine utile est la
    // première ligne ≥ 0, comme dans `cut`. Sans ça, la fenêtre de l'ancre
    // était inversée (`clamp(0,0, phase + awin)` avec phase négative) et le
    // programme **plantait** au clic sur « − mesure » (vécu 21/11 :
    // « si j'appuie sur mesure − ça quitte le programme »).
    let mut phase = tempo.phase;
    if phase < 0.0 {
        phase -= (phase / len0).floor() * len0;
    }
    // On suit les attaques **de proche en proche** (comme le faisait le
    // recollage séquentiel) : c'est ce qui permet de retrouver le vrai appui
    // même quand le BPM détecté dérive — une recherche autour de la grille
    // théorique sort de la fenêtre au bout de quelques mesures.
    let mut pts: Vec<(f64, f64)> = Vec::new(); // (k, temps de l'attaque)
    let mut cur = match nearest_attack(env, phase.max(0.001), win) {
        Some(a) if (a - phase).abs() <= len0 * 0.1 => a,
        _ => phase,
    };
    pts.push((0.0, cur));
    let mut k = 1usize;
    loop {
        let pred = cur + len0;
        if pred > whole - 0.05 {
            break;
        }
        let Some(next) = nearest_attack(env, pred, win) else {
            break;
        };
        // une attaque à plus d'un dixième de tranche n'est pas le temps suivant
        if (next - pred).abs() > len0 * 0.1 || next <= cur + len0 * 0.5 {
            break;
        }
        pts.push((k as f64, next));
        cur = next;
        k += 1;
    }
    if std::env::var("RP_CUT_DEBUG").is_ok() {
        eprintln!(
            "points (k, attaque - grille théorique, ms) : {:?} … {:?}",
            pts.iter()
                .take(3)
                .map(|(k, a)| (*k, ((a - (tempo.phase + k * len0)) * 1000.0).round()))
                .collect::<Vec<_>>(),
            pts.iter()
                .rev()
                .take(3)
                .map(|(k, a)| (*k, ((a - (tempo.phase + k * len0)) * 1000.0).round()))
                .collect::<Vec<_>>()
        );
    }
    if pts.len() < 4 {
        return (phase, len0);
    }
    // moindres carrés sur les attaques suivies, puis refus des quelques points
    // aberrants (résidu > 3 écarts médians) et second ajustement
    let line = match fit_line(&pts) {
        Some(l) => l,
        None => return (phase, len0),
    };
    let resid: Vec<f64> = pts
        .iter()
        .map(|(k, a)| (a - (line.0 + k * line.1)).abs())
        .collect();
    let mut sorted = resid.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = sorted[sorted.len() / 2];
    let tol = (mad * 3.0).max(0.015);
    let kept: Vec<(f64, f64)> = pts
        .iter()
        .zip(resid.iter())
        .filter(|(_, r)| **r <= tol)
        .map(|(p, _)| *p)
        .collect();
    let (anchor, len) = if kept.len() >= 4 {
        fit_line(&kept).unwrap_or(line)
    } else {
        line
    };
    // garde-fous : ±2 % sur la période (au-delà, l'appariement est douteux) et
    // ±60 ms sur l'ancre : au-delà, c'est l'appariement des premières attaques
    // qui est douteux, pas la grille (vécu 21/11, SPC « Lockjaw's Saga » :
    // +191 ms, soit une fausse intro de 0,93 s). Un essai d'élargir à ±½ temps
    // (26/11) a été **annulé** : l'ancre partait sur un autre appui et tout le
    // morceau sonnait faux — « tout est faux maintenant » (retour user 26/11).
    if !len.is_finite() || !anchor.is_finite() || (len / len0 - 1.0).abs() > 0.02 {
        return (phase, len0);
    }
    let awin = win.min(0.06);
    let lo = (phase - awin).max(0.0);
    let hi = (phase + awin).max(lo);
    let anchor = anchor.clamp(lo, hi);
    if std::env::var("RP_CUT_DEBUG").is_ok() {
        eprintln!(
            "fit_grid : ancre {:.4} (phase {:.4}) · période {:.4} (théorique {:.4}) · gardés {}/{}",
            anchor, phase, len, len0, kept.len(), pts.len()
        );
    }
    (anchor, len)
}

/// Moindres carrés `attaque = ancre + k × période`, l'ancre étant exprimée en
/// absolu (départ de grille, pas un écart à la phase).
/// Ajuste une droite sur `(k, attaque)` : moindres carrés, puis refus des
/// quelques points aberrants (fait par l'appelant).
fn fit_line(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = pts.len() as f64;
    if n < 2.0 {
        return None;
    }
    let sk: f64 = pts.iter().map(|(k, _)| k).sum();
    let sa: f64 = pts.iter().map(|(_, a)| a).sum();
    let skk: f64 = pts.iter().map(|(k, _)| k * k).sum();
    let ska: f64 = pts.iter().map(|(k, a)| k * a).sum();
    let den = n * skk - sk * sk;
    if den.abs() < 1e-9 {
        return None;
    }
    let len = (n * ska - sk * sa) / den;
    let anchor = (sa - len * sk) / n;
    Some((anchor, len))
}

pub fn cut(whole: f64, tempo: &Tempo, beats: f64) -> Vec<crate::model::LoopWindow> {
    use crate::model::LoopWindow;
    let mut out: Vec<LoopWindow> = Vec::new();
    if whole <= 0.5 || tempo.beat <= 0.0 || !whole.is_finite() {
        return out;
    }
    let len = tempo.beat * beats.max(1.0);
    if len <= 0.05 {
        return out;
    }
    // Origine de la grille : la phase, qui porte déjà le point de boucle du
    // format (`App::grid`) et le décalage manuel. Pas de modulo : une origine
    // ancrée à 20 s doit rester à 20 s, pas retomber dans `[0, tranche)`.
    // Négative (décalage manuel vers la gauche), on avance d'une tranche.
    let mut t = tempo.phase;
    if t < 0.0 {
        t -= (t / len).floor() * len;
    }
    t = t.clamp(0.0, whole);
    // ce qui précède est une intro : une seule tranche, jamais perdue — mais
    // seulement si elle vaut la peine (> 35 % d'une tranche) : sinon c'est un
    // résidu de phase et on démarre à 0 (vécu 21/11, Kirby « Green Fields »).
    if t > (len * 0.35).max(0.25) {
        out.push(LoopWindow { start: 0.0, end: t });
    } else {
        t = 0.0;
    }
    while t + len <= whole - 0.05 {
        out.push(LoopWindow {
            start: t,
            end: t + len,
        });
        t += len;
    }
    // Une queue trop courte n'est pas une tranche : elle est fondue dans la
    // précédente (vécu 21/11, Kirby « Green Fields » : 16 mesures justes puis
    // un miette de 0,3 s en fin de boucle, qui n'a aucun sens à jouer).
    if whole - t > 0.15 {
        if whole - t < len * 0.35 {
            match out.last_mut() {
                Some(last) => last.end = whole,
                None => out.push(LoopWindow {
                    start: t,
                    end: whole,
                }),
            }
        } else {
            out.push(LoopWindow {
                start: t,
                end: whole,
            });
        }
    }
    // une grille très lente peut ne rien produire sur une piste courte
    if out.is_empty() {
        out.push(LoopWindow {
            start: 0.0,
            end: whole,
        });
    }
    out
}

/// Estime le tempo d'un rendu de `total` secondes. `None` = aucun rythme
/// exploitable (mieux vaut ne pas aimanter que de découper au hasard).
pub fn detect(pcm: &[f32], channels: u16, rate: u32, total: f64) -> Option<Tempo> {
    detect_inner(pcm, channels, rate, total, true)
}

/// Même détection **sans la porte de rejet** : renvoie la meilleure grille
/// trouvée même si le pouls est peu marqué. Sert de repli avant
/// [`Tempo::fallback`] : mieux vaut une grille imparfaite, que l'on peut
/// vérifier à l'œil sur la forme d'onde, qu'un mode Découpe fermé (vécu 21/11 :
/// 41 pistes sur 109 de Donkey Kong Country 2 n'avaient aucun tempo et
/// étaient donc impossibles à découper).
pub fn detect_weak(pcm: &[f32], channels: u16, rate: u32, total: f64) -> Option<Tempo> {
    detect_inner(pcm, channels, rate, total, false)
}

fn detect_inner(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    total: f64,
    strict: bool,
) -> Option<Tempo> {
    if rate == 0 || total < 6.0 {
        return None;
    }
    // enveloppe d'attaque sur tout le rendu, puis 60 s analysées : au-delà on
    // paierait pour rien (un tempo de tracker ne dérive pas). C'est l'enveloppe
    // complète qui sert ensuite à recaler les coupes (voir `cut_on`).
    let (all, hop_s) = attack_flux(pcm, channels, rate)?;
    let n = ((60.0 / hop_s) as usize).min(all.len());
    if n < 64 {
        return None;
    }
    let flux = &all[..n];
    let span = n as f64 * hop_s;
    let energy: f64 = flux.iter().map(|v| v * v).sum::<f64>();
    if energy <= 1e-12 {
        return None;
    }

    // autocorrélation normalisée (r(0) = 1)
    let fr = 1.0 / hop_s;
    let max_lag = ((2.2 * fr) as usize).min(n - 1);
    let min_lag = ((0.25 * fr) as usize).max(1);
    if max_lag <= min_lag + 4 {
        return None;
    }
    let ac0 = energy / n as f64;
    let mut ac = vec![0.0f64; max_lag + 1];
    for lag in min_lag..=max_lag {
        let mut s = 0.0f64;
        for k in 0..n - lag {
            s += flux[k] * flux[k + lag];
        }
        ac[lag] = s / (n - lag) as f64 / ac0;
    }
    let ac_at = |lag: f64| -> f64 {
        if lag < min_lag as f64 || lag >= max_lag as f64 {
            return 0.0;
        }
        let i = lag.floor() as usize;
        let f = lag - i as f64;
        ac[i] * (1.0 - f) + ac[i + 1] * f
    };

    // 1) tempo grossier : peigne des harmoniques 1…4 avec un faible a priori
    //    (un morceau est plus souvent entre 90 et 160 qu'à 60 ou 200 BPM)
    let mut best = (f64::NEG_INFINITY, 120.0f64);
    // candidats du peigne, gardés pour ne pas rejeter une piste dont le
    // meilleur *score de peigne* n'est pas le temps le mieux marqué
    let mut cands: Vec<(f64, f64)> = Vec::new();
    let mut bpm = 60.0f64;
    while bpm <= 200.0 {
        let beat = 60.0 / bpm;
        let mut sum = 0.0;
        let mut weights = 0.0;
        for (k, w) in [(1.0, 1.0), (2.0, 0.6), (3.0, 0.35), (4.0, 0.2)] {
            let lag = k * beat * fr;
            if lag < max_lag as f64 {
                sum += w * ac_at(lag);
                weights += w;
            }
        }
        let score = if weights > 0.0 { sum / weights } else { 0.0 };
        let prior = (-0.5 * ((bpm / 120.0).log2() / 1.0).powi(2)).exp();
        let score = score * prior;
        if score > best.0 {
            best = (score, bpm);
        }
        cands.push((score, bpm));
        bpm += 0.5;
    }

    // Le peigne des harmoniques favorise les sous-multiples (additionner des
    // harmoniques sur un temps long est flatteur) : dans la bande des bons
    // candidats, on retient celui dont le temps est le mieux marqué, et à
    // qualité comparable le plus rapide.
    let top = cands.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
    let band: Vec<(f64, f64, f64)> = cands
        .iter()
        .copied()
        .filter(|(s, _)| *s >= top * 0.85)
        .map(|(s, b)| (s, b, ac_at(60.0 / b * fr)))
        .collect();
    let ac_max = band.iter().map(|c| c.2).fold(0.0f64, f64::max);
    if let Some((s, b, _)) = band
        .iter()
        .copied()
        .filter(|(_, _, ac)| *ac >= ac_max * 0.90)
        .max_by(|a, b| a.1.total_cmp(&b.1))
    {
        best = (s, b);
    }

    // 2) période affinée : interpolation parabolique du pic d'autocorrélation
    //    (la grille de trames ferait ~2 % d'erreur à 120 BPM, soit un temps
    //    décalé de 10 ms sur une mesure)
    let lag = 60.0 / best.1 * fr;
    let mut beat = 60.0 / best.1;
    let i = lag.round() as usize;
    if i >= min_lag + 1 && i + 1 <= max_lag {
        let (a, b, c) = (ac[i - 1], ac[i], ac[i + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-12 {
            let delta = 0.5 * (a - c) / denom;
            if delta.abs() < 1.0 {
                let refined = (i as f64 + delta) / fr;
                // un décalage de plus d'un demi-temps trahirait un saut d'octave
                if (refined - beat).abs() < beat * 0.5 {
                    beat = refined;
                }
            }
        }
    }

    // 3) phase : on pose la grille au plus près des attaques, puis on resserre
    //    le tempo de ±1 % (le pic parabolique reste approximatif quand la
    //    musique est légèrement balancée)
    let score_at = |beat: f64, phase: f64| -> f64 {
        if beat <= 0.0 {
            return 0.0;
        }
        let (mut sum, mut count) = (0.0f64, 0usize);
        let mut t = phase;
        while t < span - 0.05 {
            let idx = t / hop_s;
            let k = idx as usize;
            if k + 1 >= n {
                break;
            }
            let f = idx - k as f64;
            sum += flux[k] * (1.0 - f) + flux[k + 1] * f;
            count += 1;
            t += beat;
        }
        if count >= 4 {
            sum / count as f64
        } else {
            0.0
        }
    };
    let best_phase = |beat: f64| -> f64 {
        let step = beat / 200.0;
        let mut best = (f64::NEG_INFINITY, 0.0f64);
        let mut p = 0.0;
        while p < beat {
            let s = score_at(beat, p);
            if s > best.0 {
                best = (s, p);
            }
            p += step;
        }
        best.1
    };
    // 2bis) **le tempo doit poser les temps sur les attaques.** Le peigne et
    // l'autocorrélation départagent mal les parentés métriques (2/3, 3/2, 2) :
    // ils donnaient 113 BPM sur Ape Escape « Dark Ruins » alors que les accents
    // sont à 0,35 s (170 BPM) — la découpe tombait donc au milieu des phrases
    // (vécu 21/11 : « la coupe n'est pas au bon endroit »). On mesure, pour
    // chaque candidat du peigne, la fraction de ses temps qui tombent à ±30 ms
    // d'une attaque franche, et on garde le meilleur. C'est la seule mesure qui
    // regarde le morceau au lieu du modèle.
    let attack_at = |t: f64, w: f64| -> Option<f64> {
        let i0 = (((t - w).max(0.0)) / hop_s) as usize;
        let i1 = ((((t + w) / hop_s) as usize) + 1).min(n);
        if i1 <= i0 + 2 {
            return None;
        }
        let (mut vmax, mut jmax, mut sum) = (0.0f64, i0, 0.0f64);
        for j in i0..i1 {
            sum += flux[j];
            if flux[j] > vmax {
                vmax = flux[j];
                jmax = j;
            }
        }
        let mean = sum / (i1 - i0) as f64;
        if vmax <= 1e-9 || vmax < mean * 1.8 {
            return None;
        }
        Some(jmax as f64 * hop_s)
    };
    let regularity = |bt: f64, p: f64| -> f64 {
        let (mut good, mut cnt) = (0usize, 0usize);
        let mut t = p;
        while t < span - 0.05 {
            if let Some(a) = attack_at(t, bt * 0.25) {
                cnt += 1;
                if (a - t).abs() <= 0.030 {
                    good += 1;
                }
            }
            t += bt;
        }
        if cnt < 8 {
            0.0
        } else {
            good as f64 / cnt as f64
        }
    };
    // balayage complet : restreindre au peigne écarterait justement les temps
    // rapides, qu'il sous-note systématiquement (mesuré : 176 BPM à 0,10 de
    // peigne contre 0,21 pour 70 BPM, alors que 176 pose 59 % des temps sur une
    // attaque et 70 seulement 15 %).
    let mut best_reg = (f64::NEG_INFINITY, beat);
    let mut c = 60.0;
    while c <= 200.0 {
        let bt = 60.0 / c;
        let p = best_phase(bt);
        let r = regularity(bt, p);
        // à régularité comparable, on garde le temps le plus long (phrases plus
        // larges, moins de risque d'un demi-temps)
        if r > best_reg.0 + 1e-9 || (r > best_reg.0 - 0.02 && bt > 60.0 / best_reg.1) {
            best_reg = (r, c);
        }
        if std::env::var("RP_TEMPO_DEBUG").is_ok() && r > 0.30 {
            eprintln!("  candidat {c:6.1} BPM · régularité {r:.2}");
        }
        c += 1.0;
    }
    // Une autocorrélation franche (≥ 0,35) est plus fiable que le balayage :
    // sur une piste à tempo net (Kirby NES 0,75, Wario GB 0,75) le pouls tombe
    // tout seul, et le balayage y préférait un temps voisin (188 au lieu de 200).
    // Le balayage ne sert que là où le pic d'enveloppe est noyé (rendu PSF
    // réverbéré : 0,11–0,28), c'est-à-dire Dark Ruins et Specter's Factory.
    // Une autocorrélation nette (≥ 0,30) est plus fiable que le balayage : sur
    // une piste à pulse franc (Kirby NES 0,75, Wario GB 0,75, SMB 0,32) le
    // balayage préférait un temps voisin. Il ne sert que là où le pic
    // d'enveloppe est noyé (PSF réverbérés : 0,11–0,28) — Dark Ruins et
    // Specter's Factory, justement.
    let ac_sel = ac_at(beat * fr).clamp(0.0, 1.0);
    if ac_sel < 0.30 && best_reg.0.is_finite() && best_reg.0 >= 0.40 {
        beat = 60.0 / best_reg.1;
    }
    let confidence = ac_at(beat * fr).clamp(0.0, 1.0);
    let reg_chosen = best_reg.0.max(0.0);
    if std::env::var("RP_TEMPO_DEBUG").is_ok() {
        eprintln!(
            "tempo: {:.1} BPM · beat {beat:.4} · confiance {confidence:.3} · régularité {reg_chosen:.2}",
            60.0 / beat
        );
    }
    // une autocorrélation faible passe si les temps tombent vraiment sur les
    // attaques (rendu réverbéré : le pic d'enveloppe est noyé)
    if strict && confidence < 0.15 && reg_chosen < 0.40 {
        return None;
    }

    let mut phase = best_phase(beat);
    let center = beat;
    let mut best_beat = (score_at(center, phase), center);
    let mut d = -0.01;
    while d <= 0.01 {
        let cand = center * (1.0 + d);
        let s = score_at(cand, phase);
        if s > best_beat.0 {
            best_beat = (s, cand);
        }
        d += 0.001;
    }
    beat = best_beat.1;
    phase = best_phase(beat);

    // 4) recollage et régression : chaque temps est ramené sur le maximum
    //    d'attaque voisin, puis une droite `t = a + b·k` donne période et phase
    //    exactes. C'est ce qui évite une dérive de quelques ms par temps sur
    //    une piste de 3 min (vécu 21/11 : phase 24 ms trop tôt sur 20 s).
    let mean_flux = flux.iter().sum::<f64>() / n as f64;
    let peak_flux = flux.iter().cloned().fold(0.0f64, f64::max);
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let max_k = ((span - phase) / beat).floor().max(0.0) as i64;
    for k in 0..=max_k {
        let pred = phase + k as f64 * beat;
        let i0 = (((pred - 0.2 * beat) / hop_s).max(0.0) as usize).min(n);
        let i1 = ((((pred + 0.2 * beat) / hop_s) as usize) + 1).min(n);
        let (mut best_v, mut best_t) = (0.0, pred);
        for j in i0..i1 {
            if flux[j] > best_v {
                best_v = flux[j];
                best_t = j as f64 * hop_s;
            }
        }
        // une attaque nette seulement : le bruit ne doit pas fabriquer une
        // grille (vécu 21/11 : bruit blanc « détecté » à 120 BPM)
        if best_v >= 0.25 * peak_flux && best_v >= 2.0 * mean_flux {
            pts.push((k as f64, best_t));
        }
    }
    if pts.len() >= 8 {
        let m = pts.len() as f64;
        let mk = pts.iter().map(|p| p.0).sum::<f64>() / m;
        let mt = pts.iter().map(|p| p.1).sum::<f64>() / m;
        let (mut num, mut den) = (0.0, 0.0);
        for (k, t) in &pts {
            num += (k - mk) * (t - mt);
            den += (k - mk) * (k - mk);
        }
        if den > 1e-9 {
            let b = num / den;
            // un écart de plus de 2 % trahirait un saut d'octave ou un point
            // aberrant : on garde la période de l'autocorrélation
            if b > 0.0 && (b - beat).abs() < beat * 0.02 {
                beat = b;
                phase = (mt - mk * b).rem_euclid(b);
            }
        }
    }

    // 5) clarté du pouls : les temps doivent tomber sur des attaques franches,
    //    pas sur le bruit moyen de l'enveloppe
    let (mut hits, mut count) = (0.0f64, 0usize);
    let mut t = phase;
    while t < span - 0.05 {
        let idx = t / hop_s;
        let k = idx as usize;
        if k + 1 >= n {
            break;
        }
        let f = idx - k as f64;
        hits += flux[k] * (1.0 - f) + flux[k + 1] * f;
        count += 1;
        t += beat;
    }
    if count < 4 || mean_flux <= 1e-12 {
        return None;
    }
    let pulse = (hits / count as f64) / mean_flux;
    if strict && pulse < 1.6 {
        return None;
    }

    if !beat.is_finite() || beat <= 0.05 || phase >= beat {
        return None;
    }
    Some(Tempo {
        bpm: 60.0 / beat,
        beat,
        phase,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempo_150() -> Tempo {
        Tempo {
            bpm: 150.0,
            beat: 0.4,
            phase: 0.12,
            confidence: 1.0,
        }
    }

    #[test]
    fn bpm_recale_sur_la_fin_de_boucle() {
        // 38,318 s pour un temps de 0,4 s = 95,8 temps : on recale à 96 temps,
        // la dernière coupe tombe pile sur la couture (Kirby NES, vécu 21/11)
        let t = tempo_150().across(38.318);
        assert!((t.beat * 96.0 - 38.318).abs() < 1e-6, "beat {}", t.beat);
        assert!((t.bpm - 150.32).abs() < 0.5, "bpm {}", t.bpm);
        // la phase absolue ne bouge pas : la grille reste ancrée au début
        assert!((t.phase - 0.12).abs() < 1e-9);
    }

    #[test]
    fn bpm_non_force_quand_la_duree_n_est_pas_un_multiple() {
        // 4,2 s pour 10 temps demanderait 142,9 BPM (−4,8 %) : hors tolérance,
        // c'est la durée qui n'est pas un multiple de temps, pas le BPM qui est faux
        let t = tempo_150();
        let f = t.across(4.2);
        assert!((f.bpm - t.bpm).abs() < 1e-9);
    }

    /// Enveloppe jouet : un pic net toutes les `step` secondes.
    fn pulse_env(total: f64, step: f64, phase: f64) -> Envelope {
        let hop = 0.005;
        let n = (total / hop) as usize;
        let mut flux = vec![0.0f64; n];
        let mut t = phase;
        while t < total {
            let i = (t / hop).round() as usize;
            if i > 1 && i + 1 < n {
                flux[i] = 1.0;
            }
            t += step;
        }
        Envelope { flux, hop }
    }

    /// **La grille ajustée ne dérive pas** : un BPM détecté 0,2 % faux décale
    /// les dernières tranches de plusieurs centaines de ms si la découpe suit
    /// `beat`. `cut_on` recolle aux attaques, et `fitted`/`cut_from` (utilisés
    /// par l'arrangement et les repères) doivent rendre **exactement** la même
    /// grille — sinon les tuiles et les traits dérivent chacun de leur côté
    /// (vécu 26/11 : « les séquences se décalent de plus en plus, plus elles
    /// sont loin du départ du son »).
    #[test]
    fn grille_ajustee_ne_derive_pas_en_fin_de_piste() {
        let beat = 0.5; // vraie période d'un temps : 120 BPM
        let total = 60.0;
        let env = pulse_env(total, beat, 0.0);
        // BPM annoncé 0,2 % trop lent (le cas mesuré : 115,13 pour 115,37)
        let t = fixed(60.0 / (beat * 1.002), 0.0);
        // sans ajustement, la dernière tranche serait 240 ms à côté
        let brut = cut(total, &t, 4.0);
        let fin_brute = brut.last().map(|c| c.start).unwrap_or(0.0);
        assert!(
            (fin_brute - (fin_brute / 2.0).round() * 2.0).abs() > 0.05,
            "la grille théorique devait dériver, fin {fin_brute}"
        );
        let clips = cut_on(total, &t, 4.0, Some(&env));
        assert!(clips.len() > 20, "{} tranches", clips.len());
        let len = clips[1].len();
        assert!((len - 4.0 * beat).abs() < 0.006, "période de tranche {len}");
        // la dernière tranche colle encore à une attaque (une trame de 5 ms)
        let last = clips.last().unwrap();
        let k = (last.start / beat).round();
        assert!(
            (last.start - k * beat).abs() < 0.010,
            "fin de piste décalée de {:.0} ms",
            (last.start - k * beat) * 1000.0
        );
        // `fitted` + `cut_from` = exactement la découpe de `cut_on`
        let (anchor, period) = fitted(Some(&env), &t, 4.0, total).expect("grille ajustée");
        let again = cut_from(total, anchor, period);
        assert_eq!(again.len(), clips.len());
        for (a, b) in again.iter().zip(clips.iter()) {
            assert!(
                (a.start - b.start).abs() < 1e-9 && (a.end - b.end).abs() < 1e-9,
                "{a:?} ≠ {b:?}"
            );
        }
        // sans enveloppe, pas d'ajustement : la grille théorique reste seule
        assert!(fitted(None, &t, 4.0, total).is_none());
    }

    /// La phase détectée peut tomber près d'un demi-temps à côté : `pulse` la
    /// remet sur les attaques, et double l'octave quand la grille détectée ne
    /// tombe sur **rien** alors que le double tombe juste (vécu 26/11,
    /// « Cauldron Keep » : frontières après chaque attaque → « un bout en trop
    /// à la fin », et 115 BPM pour un pouls à 230).
    #[test]
    fn pouls_recale_sur_les_attaques() {
        let rate = 22050;
        let total = 30.0;
        let true_beat = 0.2608; // 230 BPM de pouls réel
        let pcm = click_track(60.0 / true_beat, total, 0.146, rate);
        let env = envelope(&pcm, 1, rate).expect("enveloppe");
        // détection : la moitié du pouls (115 BPM), et posée 144 ms à côté
        let detected = fixed(60.0 / (2.0 * true_beat), 0.0017);
        let got = pulse(&env, &detected);
        assert!(
            (got.bpm - 60.0 / true_beat).abs() < 3.0,
            "octave non corrigée : {} BPM",
            got.bpm
        );
        let mut t = got.phase;
        while t < total - 0.2 {
            // la grille de 115 BPM doit tomber sur un clic sur deux
            let k = ((t - 0.146) / true_beat).round();
            // l'enveloppe d'attaque avance de ~20 ms sur le clic (trame de
            // 20 ms, saut de 5 ms) : la grille tombe juste **avant** l'attaque,
            // ce qui est le bon côté pour couper
            assert!(
                (t - (0.146 + k * true_beat)).abs() < 0.030,
                "temps {t:.3} hors attaque"
            );
            t += got.beat;
        }
        // une grille déjà juste n'est pas déplacée
        let pcm2 = click_track(120.0, total, 0.0, rate);
        let env2 = envelope(&pcm2, 1, rate).expect("enveloppe");
        let same = pulse(&env2, &fixed(120.0, 0.0));
        assert!((same.bpm - 120.0).abs() < 0.5, "BPM {}", same.bpm);
        // la grille reste sur le clic, ~20 ms avant (avance de l'enveloppe)
        assert!(same.phase.abs() < 0.03, "phase {}", same.phase);
    }

    fn noise(n: usize, seed: u64) -> Vec<f32> {        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 0.05
            })
            .collect()
    }

    /// Clics percussifs (décroissance exponentielle) sur un fond de bruit, avec
    /// une phase décalée : c'est exactement ce que la grille doit retrouver.
    fn click_track(bpm: f64, total: f64, phase: f64, rate: u32) -> Vec<f32> {
        let n = (total * rate as f64) as usize;
        let mut pcm = noise(n, 0x1234);
        let beat = 60.0 / bpm;
        let click = ((0.03 * rate as f64) as usize).max(1);
        let mut t = phase;
        while t < total - 0.05 {
            let pos = (t * rate as f64) as usize;
            for i in 0..click {
                if pos + i >= n {
                    break;
                }
                let g = (1.0 - i as f32 / click as f32).powi(3) * 0.8;
                pcm[pos + i] += g * (i as f32 * 0.9).sin();
            }
            t += beat;
        }
        pcm
    }

    #[test]
    fn tempo_et_phase() {
        let rate = 22050;
        let total = 20.0;
        let pcm = click_track(140.0, total, 0.037, rate);
        let t = detect(&pcm, 1, rate, total).expect("tempo trouvé");
        assert!((t.bpm - 140.0).abs() < 2.0, "BPM {}", t.bpm);
        assert!((t.phase - 0.037).abs() < 0.02, "phase {}", t.phase);
        // aimant : le temps le plus proche d'un clic retombe sur le clic
        let click = 0.037 + 7.0 * 60.0 / 140.0;
        assert!((t.snap(click + 0.012) - click).abs() < 0.02);
    }

    #[test]
    fn mesure_de_quatre_temps() {
        let rate = 22050;
        let total = 20.0;
        let pcm = click_track(100.0, total, 0.0, rate);
        let t = detect(&pcm, 1, rate, total).expect("tempo trouvé");
        assert!((t.bars(1.0) - 2.4).abs() < 0.05, "mesure {}", t.bars(1.0));
    }

    #[test]
    fn sans_rythme() {
        // bruit plat : aucune attaque périodique
        let rate = 22050;
        let total = 20.0;
        let pcm = noise((total * rate as f64) as usize, 77);
        let got = detect(&pcm, 1, rate, total);
        if let Some(t) = got {
            println!("FAUX POSITIF : {:.1} BPM, confiance {:.3}", t.bpm, t.confidence);
        }
        assert!(got.is_none());
    }

    fn fixed(bpm: f64, phase: f64) -> Tempo {
        let beat = 60.0 / bpm;
        Tempo {
            bpm,
            beat,
            phase,
            confidence: 1.0,
        }
    }

    #[test]
    fn decoupe_en_mesures() {
        // 120 BPM : une mesure 4/4 = 2 s ; 8 s = 4 mesures pile
        let t = fixed(120.0, 0.0);
        let c = cut(8.0, &t, 4.0);
        assert_eq!(c.len(), 4);
        assert!((c[0].start, c[0].end) == (0.0, 2.0));
        assert!((c[3].start, c[3].end) == (6.0, 8.0));
        // deux mesures : deux segments
        assert_eq!(cut(8.0, &t, 8.0).len(), 2);
        // un temps : huit segments d'un demi-seconde
        let c = cut(8.0, &t, 1.0);
        assert_eq!(c.len(), 16);
        assert!((c[0].start, c[0].end) == (0.0, 0.5));
    }

    #[test]
    fn decoupe_garde_intro_et_queue() {
        // boucle qui commence à 2 s (une mesure d'intro à 120 BPM), piste de 5 s
        let t = fixed(120.0, 2.0);
        let c = cut(5.0, &t, 4.0);
        assert_eq!(c.len(), 3, "{c:?}");
        assert!((c[0].start, c[0].end) == (0.0, 2.0));
        assert!((c[1].start, c[1].end) == (2.0, 4.0));
        assert!((c[2].start, c[2].end) == (4.0, 5.0));
        // les segments se touchent : la suite reconstruit la piste
        for w in c.windows(2) {
            assert!((w[0].end - w[1].start).abs() < 1e-9);
        }
    }

    /// Un BPM estimé 0,4 % faux décale les dernières coupes de plusieurs
    /// dixièmes de seconde ; `cut_on` recolle chaque frontière sur les attaques
    /// réelles (vécu 21/11 : « petit à petit ça se décale »).
    #[test]
    fn coupe_tete_et_queue_courtes() {
        let t = Tempo {
            bpm: 60.0,
            beat: 1.0,
            phase: 0.0,
            confidence: 1.0,
        };
        // 8,3 s de boucle, tranches de 4 temps : la queue de 0,3 s rejoint la
        // dernière tranche au lieu de faire une tuile de 0,3 s
        let v = cut(8.3, &t, 4.0);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!((v[1].start - 4.0).abs() < 1e-9);
        assert!((v[1].end - 8.3).abs() < 1e-9);
        // une phase de 0,3 s n'est pas une intro : la grille démarre à 0
        let t = Tempo {
            bpm: 60.0,
            beat: 1.0,
            phase: 0.3,
            confidence: 1.0,
        };
        let v = cut(8.0, &t, 4.0);
        assert!((v[0].start - 0.0).abs() < 1e-9, "{v:?}");
        assert!((v[0].end - 4.0).abs() < 1e-9, "{v:?}");
        // une vraie intro (boucle à 20 s) reste une tranche à part
        let t = Tempo {
            bpm: 60.0,
            beat: 1.0,
            phase: 20.0,
            confidence: 1.0,
        };
        let v = cut(32.0, &t, 4.0);
        assert!((v[0].start - 0.0).abs() < 1e-9);
        assert!((v[0].end - 20.0).abs() < 1e-9);
        assert_eq!(v.len(), 4, "{v:?}");
    }

    #[test]
    fn coupe_recollee_sur_les_attaques() {
        let rate = 22050;
        let total = 60.0;
        let phase = 0.30;
        let pcm = click_track(120.0, total, phase, rate);
        let env = envelope(&pcm, 1, rate).expect("enveloppe");
        let bad = Tempo {
            bpm: 120.0,
            beat: 0.5 * 1.004,
            phase,
            confidence: 1.0,
        };
        let plain = cut(total, &bad, 4.0);
        let fixed = cut_on(total, &bad, 4.0, Some(&env));
        assert!(
            (plain.len() as i64 - fixed.len() as i64).abs() <= 1,
            "même découpage à une tranche près ({} vs {})",
            plain.len(),
            fixed.len()
        );
        // la grille rigide a dérivé, la grille ajustée non (comparaison au même
        // instant, pas au même index : l'intro décale les indices)
        let mid = total / 2.0;
        let near = |v: &[crate::model::LoopWindow]| {
            v.iter()
                .map(|w| w.start)
                .filter(|s| *s > mid)
                .fold(f64::INFINITY, f64::min)
        };
        let drift = (near(&plain) - near(&fixed)).abs();
        assert!(drift > 0.04, "dérive attendue sur la grille rigide : {drift}");
        for w in fixed.iter().skip(1) {
            let k = ((w.start - phase) / 0.5).round();
            let expect = phase + k * 0.5;
            assert!(
                (w.start - expect).abs() < 0.04,
                "frontière {} hors attaque {}",
                w.start,
                expect
            );
        }
        // sans enveloppe, on garde la grille telle quelle
        assert_eq!(cut_on(total, &bad, 4.0, None).len(), plain.len());
        // une tête courte (< 35 % d'une tranche) est **absorbée** par la
        // première tranche (demande user 26/11 : le début de la piste ne doit
        // pas manquer) : elle commence à 0 et dure une tranche **plus la tête**
        assert!(
            fixed[0].start.abs() < 1e-9,
            "première tranche à {} au lieu de 0",
            fixed[0].start
        );
        assert!(
            (fixed[0].len() - (2.0 + phase)).abs() < 0.02,
            "première tranche de {} s au lieu de {}",
            fixed[0].len(),
            2.0 + phase
        );
    }

    /// Décalage manuel vers la gauche (bouton « − mesure ») : la phase devient
    /// négative. La fenêtre de l'ancre se retournait (`clamp(0, 0, phase + 60 ms)`
    /// avec phase négative) et Rust **paniquait** : « min > max… ça quitte le
    /// programme » (vécu 21/11). La phase est désormais ramenée sur la première
    /// ligne ≥ 0, comme dans `cut`.
    #[test]
    fn decalage_negatif_ne_plante_pas() {
        let rate = 22050;
        let total = 30.0;
        let n = (total * rate as f64) as usize;
        let mut pcm = vec![0.0f32; n];
        let mut t = 0.0;
        while t < total - 0.05 {
            let pos = (t * rate as f64) as usize;
            let click = ((0.03 * rate as f64) as usize).max(1);
            for i in 0..click {
                if pos + i < n {
                    let g = (1.0 - i as f32 / click as f32).powi(3) * 0.8;
                    pcm[pos + i] += g * (i as f32 * 0.9).sin();
                }
            }
            t += 0.5;
        }
        let env = envelope(&pcm, 1, rate).expect("enveloppe");
        let mut g = fixed(120.0, 0.0);
        g.phase -= Tempo::BEATS_PER_BAR * g.beat; // « − mesure »
        assert!(g.phase < 0.0);
        let clips = cut_on(total, &g, 4.0, Some(&env));
        assert!(clips.len() > 4, "{} tranches", clips.len());
        // les mesures pleines gardent leur durée
        for c in clips.iter().skip(1) {
            assert!(
                (c.len() - 2.0).abs() < 0.05,
                "tranche de {} s",
                c.len()
            );
        }
        // et le décalage à droite reste sain
        let mut g = fixed(120.0, 0.0);
        g.phase += Tempo::BEATS_PER_BAR * g.beat;
        assert!(cut_on(total, &g, 4.0, Some(&env)).len() > 4);
    }

    /// Toutes les tranches d'une même piste durent **la même chose** : la grille
    /// est une mesure, pas une suite de frontières recollées une à une (vécu
    /// 21/11 : 2,76 / 2,74 / 2,68 s sur FF8 « Force Your Way »).
    #[test]
    fn tranches_de_meme_duree() {
        let rate = 22050;
        let total = 60.0;
        let phase = 0.30;
        // attaques décalées de ±8 ms : le recollage frontière par frontière
        // donnait des tranches inégales, une période unique les garde égales
        let n = (total * rate as f64) as usize;
        let mut pcm = vec![0.0f32; n];
        let mut t = phase;
        let mut k = 0;
        while t < total - 0.05 {
            let jitter = if k % 2 == 0 { 0.008 } else { -0.008 };
            let pos = ((t + jitter) * rate as f64) as usize;
            let click = ((0.03 * rate as f64) as usize).max(1);
            for i in 0..click {
                if pos + i < n {
                    let g = (1.0 - i as f32 / click as f32).powi(3) * 0.8;
                    pcm[pos + i] += g * (i as f32 * 0.9).sin();
                }
            }
            t += 0.5;
            k += 1;
        }
        let env = envelope(&pcm, 1, rate).expect("enveloppe");
        let t0 = fixed(120.0, phase);
        let clips = cut_on(total, &t0, 4.0, Some(&env));
        assert!(clips.len() > 8, "{} tranches", clips.len());
        // La première tranche absorbe la tête (elle dure une tranche **plus**
        // la phase, demande user 26/11) et la dernière suit la fin de boucle :
        // toutes les tranches du milieu doivent durer exactement une mesure.
        assert!(
            (clips[0].len() - (2.0 + phase)).abs() < 0.02,
            "première tranche de {} s au lieu de {}",
            clips[0].len(),
            2.0 + phase
        );
        let inner: Vec<f64> = clips
            .iter()
            .skip(1)
            .take(clips.len().saturating_sub(2))
            .map(|c| c.len())
            .collect();
        let mini = inner.iter().cloned().fold(f64::MAX, f64::min);
        let maxi = inner.iter().cloned().fold(0.0f64, f64::max);
        assert!(
            maxi - mini < 0.001,
            "durées inégales : {mini:.3} à {maxi:.3}"
        );
        assert!((mini - 2.0).abs() < 0.01, "tranche d'une mesure : {mini:.3}");
    }

    /// Une piste qui commence par une intro (cas des PSF) : la grille est
    /// ancrée sur le début de la boucle, l'intro devient **une seule tranche**
    /// à part, et la découpe s'arrête à la fin de la boucle.
    #[test]
    fn decoupe_ancree_sur_la_boucle() {
        let rate = 22050;
        let pcm = click_track(120.0, 60.0, 0.0, rate);
        let env = envelope(&pcm, 1, rate).unwrap();
        // ancre à 20 s (début de boucle), fin de boucle à 52 s
        let t = fixed(120.0, 20.0);
        let c = cut_on(52.0, &t, 4.0, Some(&env));
        assert!(c.len() >= 3);
        assert_eq!(c[0].start, 0.0, "l'intro reste une tranche");
        assert!((c[0].end - 20.0).abs() < 0.05, "intro jusqu'à la boucle : {}", c[0].end);
        assert!((c[1].start - c[0].end).abs() < 1e-9, "aucun trou");
        assert!((c[1].end - c[1].start - 2.0).abs() < 0.05, "mesure de 2 s à 120 BPM");
        // la dernière tranche s'arrête à la fin de la boucle, pas après
        assert!((c[c.len() - 1].end - 52.0).abs() < 1e-9);
        // décalage manuel : la grille glisse à partir de l'ancre
        let c2 = cut_on(52.0, &fixed(120.0, 20.5), 4.0, None);
        assert!((c2[0].end - 20.5).abs() < 1e-9);
    }

    /// Décalage manuel : la grille glisse d'un temps (ou d'une fraction) et
    /// n'est plus ancrée au hasard sur le morceau.
    #[test]
    fn decalage_de_grille() {
        let t = fixed(120.0, 0.0);
        // un décalage d'une mesure et demie déplace le départ des tranches
        let c = cut(8.0, &t.shifted(2.5), 4.0);
        assert!((c[0].end - 2.5).abs() < 1e-9, "le décalage suit : {}", c[0].end);
        // un décalage plus court qu'un tiers de tranche est absorbé dans la
        // première tranche (pas de tuile de 0,3 s)
        let c = cut(8.0, &t.shifted(0.3), 4.0);
        assert!(c[0].start <= 1e-9 && (c[0].end - 2.0).abs() < 1e-9, "tête fondue : {c:?}");
        // découpe d'un temps : invariant (la grille de temps ne bouge pas)
        assert_eq!(
            cut(8.0, &t.shifted(0.5), 1.0)
                .iter()
                .map(|w| (w.start, w.end))
                .collect::<Vec<_>>(),
            cut(8.0, &t, 1.0)
                .iter()
                .map(|w| (w.start, w.end))
                .collect::<Vec<_>>()
        );
        // découpe d'une mesure : un temps de décalage change le départ des mesures
        let b = cut(8.0, &t.shifted(2.5), 4.0);
        assert!((b[1].start - 2.5).abs() < 1e-9, "{}", b[1].start);
        // octave : un tempo doublé coupe deux fois plus court
        let d = t.with_bpm(240.0);
        assert!((d.beat - 0.25).abs() < 1e-9);
        assert_eq!(cut(8.0, &d, 4.0).len(), 8);
    }
}

