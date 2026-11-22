//! Moteur multitrack : plan de voix, mixage temps réel et rendu hors temps réel.
//!
//! Invariants (détail dans `AGENTS.md`) :
//! - une seule horloge, en trames de sortie ; le projet est en beats ;
//! - tout est mélangé par le **même** code en temps réel et à l'export ;
//! - aucun trou : une voix qui finit libère sa place, le mixer continue ;
//! - le plan (qui joue quoi) est reconstruit côté GUI et remplacé sous verrou
//!   court ; le chemin audio ne fait que lire.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::project::{Fill, Project, StretchMode};

/// PCM d'une source, tel que le mixer le voit.
#[derive(Clone)]
pub struct SourceAudio {
    pub pcm: Arc<Vec<f32>>,
    pub channels: u16,
    pub rate: u32,
}

impl SourceAudio {
    pub fn new(pcm: Arc<Vec<f32>>, channels: u16, rate: u32) -> Self {
        Self {
            pcm,
            channels: channels.max(1),
            rate: rate.max(1),
        }
    }
}

/// Une voix prête à jouer : tout est calculé, le mixage ne fait plus d'arithmétique
/// de tempo.
#[derive(Clone)]
pub struct Voice {
    pcm: Arc<Vec<f32>>,
    channels: usize,
    src_start: usize,
    src_end: usize,
    /// Fenêtre de sortie, en trames du projet.
    pub start: u64,
    pub end: u64,
    /// Trames de source lues par trame de sortie.
    pub ratio: f64,
    gain_l: f32,
    gain_r: f32,
    fade_in: u64,
    fade_out: u64,
    loop_src: bool,
}

#[derive(Clone, Default)]
pub struct Plan {
    pub out_rate: u32,
    /// Fin du projet en trames de sortie.
    pub frames: u64,
    /// Boucle du projet : `(début, fin)` en trames de sortie.
    pub loop_region: Option<(u64, u64)>,
    pub voices: Vec<Voice>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty() || self.frames == 0
    }

    /// Construit le plan depuis un projet et les PCM de ses sources (indexées
    /// comme `Project.sources`). Une source absente est simplement muette.
    pub fn build(project: &Project, audio: &[Option<SourceAudio>], out_rate: u32) -> Plan {
        let out_rate = out_rate.max(8000);
        let fpb = project.beat_sec() * out_rate as f64;
        let mut voices = Vec::with_capacity(project.clips.len());

        for clip in &project.clips {
            let Some(Some(src)) = audio.get(clip.source) else {
                continue;
            };
            let Some(track) = project.track(clip.track) else {
                continue;
            };
            let src_len = clip.src_end.saturating_sub(clip.src_start);
            if src_len == 0 || clip.length_beats <= 0.0 {
                continue;
            }
            let out_len = (clip.length_beats * fpb).round().max(1.0) as u64;
            let semi = project.effective_transpose(clip);
            // une transposition impose le stretch : en repitch, la durée suivrait
            let mode = if semi != 0 {
                StretchMode::Stretch
            } else {
                project.effective_stretch(clip)
            };

            // `src_start/src_end` désignent le buffer RÉELLEMENT lu : pour un
            // clip étiré, c'est le buffer étiré (index 0), pas la plage source —
            // sinon la lecture s'arrête au milieu du clip.
            let (pcm, ratio, s0, s1) = match mode {
                StretchMode::Repitch => (
                    src.pcm.clone(),
                    src_len as f64 / out_len as f64,
                    clip.src_start,
                    clip.src_end,
                ),
                StretchMode::Stretch => {
                    let pitch = 2f64.powf(semi as f64 / 12.0);
                    let frames = (out_len as f64 * pitch * src.rate as f64 / out_rate as f64)
                        .round()
                        .max(1.0) as usize;
                    let buf = crate::stretch::cached(
                        &src.pcm,
                        src.channels,
                        clip.src_start,
                        clip.src_end,
                        frames,
                    );
                    let end = buf.len() / src.channels as usize;
                    (buf, pitch * src.rate as f64 / out_rate as f64, 0, end)
                }
            };

            let start = (clip.at_beat * fpb).round().max(0.0) as u64;
            let end = start + out_len;
            let pan = track.pan.clamp(-1.0, 1.0) as f64;
            let g = project.clip_gain(clip);
            let gain_l = (g as f64 * ((1.0 - pan) * std::f64::consts::FRAC_PI_4).cos()) as f32;
            let gain_r = (g as f64 * ((1.0 + pan) * std::f64::consts::FRAC_PI_4).cos()) as f32;

            voices.push(Voice {
                pcm,
                channels: src.channels as usize,
                src_start: s0,
                src_end: s1,
                start,
                end,
                ratio,
                gain_l,
                gain_r,
                fade_in: (clip.fade_in.max(0.0) * fpb).round() as u64,
                fade_out: (clip.fade_out.max(0.0) * fpb).round() as u64,
                loop_src: clip.fill == Fill::Loop,
            });
        }

        // gain général : appliqué à chaque voix, donc identique au temps réel
        // et à l'export (une seule multiplication, pas un étage de plus)
        let master = project.master_gain.clamp(0.0, 2.0);
        if (master - 1.0).abs() > 1e-6 {
            for v in &mut voices {
                v.gain_l *= master;
                v.gain_r *= master;
            }
        }

        let frames = (project.len_beats() * fpb).round().max(1.0) as u64;
        let loop_region = project.loop_beats.and_then(|(a, b)| {
            let a = (a * fpb).round().max(0.0) as u64;
            let b = (b * fpb).round().max(0.0) as u64;
            if b > a && b <= frames {
                Some((a, b))
            } else {
                None
            }
        });

        if project.metronome {
            add_metronome(&mut voices, frames, fpb, out_rate);
        }

        Plan {
            out_rate,
            frames,
            loop_region,
            voices,
        }
    }
}

/// Clic de métronome : une voix ordinaire dont le PCM est un bip court, donc
/// le même chemin de mixage que la musique (aucun code audio en plus).
fn add_metronome(voices: &mut Vec<Voice>, frames: u64, fpb: f64, out_rate: u32) {
    let click_len = (out_rate / 50).max(8) as usize; // 20 ms
    let mut pcm: Vec<f32> = Vec::with_capacity(click_len * 2);
    for i in 0..click_len {
        let t = i as f32 / out_rate as f32;
        let env = (1.0 - i as f32 / click_len as f32).powf(2.0);
        let v = 0.32 * env * (std::f32::consts::TAU * 1500.0 * t).sin();
        pcm.push(v);
        pcm.push(v);
    }
    let accent = {
        let mut a: Vec<f32> = Vec::with_capacity(click_len * 2);
        for i in 0..click_len {
            let t = i as f32 / out_rate as f32;
            let env = (1.0 - i as f32 / click_len as f32).powf(2.0);
            a.push(0.45 * env * (std::f32::consts::TAU * 2200.0 * t).sin());
            a.push(0.45 * env * (std::f32::consts::TAU * 2200.0 * t).sin());
        }
        a
    };
    let normal = Arc::new(pcm);
    let strong = Arc::new(accent);
    let mut beat = 0u64;
    while (beat as f64) * fpb < frames as f64 {
        let start = (beat as f64 * fpb).round() as u64;
        let accent = beat.is_multiple_of(4);
        voices.push(Voice {
            pcm: if accent {
                strong.clone()
            } else {
                normal.clone()
            },
            channels: 2,
            src_start: 0,
            src_end: click_len,
            start,
            end: start + click_len as u64,
            ratio: 1.0,
            gain_l: 0.6,
            gain_r: 0.6,
            fade_in: 0,
            fade_out: 0,
            loop_src: false,
        });
        beat += 1;
    }
}

/// Mixe la fenêtre `[start, start + out.len()/2)` dans `out` (stéréo entrelacée,
/// **ajout**, pas remplacement).
pub fn mix_at(plan: &Plan, start: u64, out: &mut [f32]) {
    let frames = out.len() / 2;
    for v in &plan.voices {
        let a = start.max(v.start);
        let b = (start + frames as u64).min(v.end);
        if b <= a {
            continue;
        }
        let src_len = (v.src_end - v.src_start) as f64;
        let ch = v.channels;
        for t in a..b {
            let rel = (t - v.start) as f64;
            let mut pos = v.src_start as f64 + rel * v.ratio;
            if pos >= v.src_end as f64 {
                if v.loop_src && src_len > 0.0 {
                    pos = v.src_start as f64 + (pos - v.src_start as f64) % src_len;
                } else {
                    continue;
                }
            }
            let i0 = pos.floor() as usize;
            let frac = (pos - i0 as f64) as f32;
            let i1 = (i0 + 1).min(v.src_end.saturating_sub(1));
            let (mut l0, mut r0) = (0.0f32, 0.0f32);
            let (mut l1, mut r1) = (0.0f32, 0.0f32);
            let last = v.pcm.len() / ch;
            if i0 < last {
                l0 = v.pcm[i0 * ch];
                r0 = if ch > 1 { v.pcm[i0 * ch + 1] } else { l0 };
            }
            if i1 < last {
                l1 = v.pcm[i1 * ch];
                r1 = if ch > 1 { v.pcm[i1 * ch + 1] } else { l1 };
            }
            let dl = l0 + (l1 - l0) * frac;
            let dr = r0 + (r1 - r0) * frac;

            let mut env = 1.0f32;
            if v.fade_in > 0 && t - v.start < v.fade_in {
                env = env.min((t - v.start) as f32 / v.fade_in as f32);
            }
            if v.fade_out > 0 && v.end - t < v.fade_out {
                env = env.min((v.end - t) as f32 / v.fade_out as f32);
            }
            let k = ((t - start) * 2) as usize;
            out[k] += dl * v.gain_l * env;
            out[k + 1] += dr * v.gain_r * env;
        }
    }
}

/// Mixe au plus `out.len()/2` trames depuis `pos`, en enroulant sur la boucle du
/// projet. Rend `(trames écrites, position suivante)`.
fn mix_step(plan: &Plan, mut pos: u64, out: &mut [f32]) -> (usize, u64) {
    let want = out.len() / 2;
    let (limit_start, limit_end) = match plan.loop_region {
        Some((a, b)) => (a, b.min(plan.frames)),
        None => (0, plan.frames),
    };
    let mut written = 0usize;
    while written < want {
        if pos >= limit_end {
            if plan.loop_region.is_some() && limit_end > limit_start {
                pos = limit_start;
            } else {
                break;
            }
        }
        let room = (limit_end - pos) as usize;
        if room == 0 {
            break;
        }
        let n = (want - written).min(room);
        mix_at(plan, pos, &mut out[written * 2..(written + n) * 2]);
        pos += n as u64;
        written += n;
    }
    (written, pos)
}

/// Rendu **hors temps réel** : le même mixage, poussé d'un bloc. C'est ce qui
/// rend l'export testable sans écouter (et sans interface).
pub fn render(plan: &Plan, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    let mut pos = 0u64;
    let mut done = 0usize;
    while done < frames {
        let n = (frames - done).min(2048);
        let (w, np) = mix_step(plan, pos, &mut out[done * 2..(done + n) * 2]);
        if w == 0 {
            break;
        }
        pos = np;
        done += w;
    }
    out
}

// ── transport temps réel ────────────────────────────────────────────────────

pub struct Shared {
    pos: AtomicU64,
    seek: AtomicI64,
    plan: Mutex<Arc<Plan>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            pos: AtomicU64::new(0),
            seek: AtomicI64::new(-1),
            plan: Mutex::new(Arc::new(Plan::default())),
        }
    }
}

/// Poignée de transport : la GUI la garde, la source audio en reçoit une copie.
#[derive(Clone)]
pub struct Transport {
    shared: Arc<Shared>,
}

impl Transport {
    pub fn new(plan: Plan) -> Self {
        let rate = plan.out_rate;
        let mut plan = plan;
        if rate == 0 {
            plan.out_rate = 48_000;
        }
        Self {
            shared: Arc::new(Shared {
                plan: Mutex::new(Arc::new(plan)),
                ..Default::default()
            }),
        }
    }

    pub fn set_plan(&self, plan: Plan) {
        *self.shared.plan.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(plan);
    }

    pub fn plan(&self) -> Arc<Plan> {
        self.shared
            .plan
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn position(&self) -> u64 {
        self.shared.pos.load(Ordering::Relaxed)
    }

    pub fn seek(&self, frame: u64) {
        // la position est publiée tout de suite : la GUI ne doit pas afficher
        // l'ancienne tête pendant un bloc (le mixer applique `seek` au prochain
        // remplissage, avec la même valeur)
        self.shared.pos.store(frame, Ordering::Relaxed);
        self.shared.seek.store(frame as i64, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.seek(0);
    }

    /// Source à donner à rodio. Une nouvelle lecture = un nouveau `Mixer`.
    pub fn source(&self) -> Mixer {
        let rate = self.plan().out_rate;
        Mixer {
            shared: self.shared.clone(),
            rate,
            // vide : le premier `next()` déclenche un remplissage (avec un
            // buffer de zéros, on rendait 512 trames de silence au démarrage)
            buf: Vec::new(),
            idx: 0,
            ended: false,
        }
    }
}

const BLOCK: usize = 512;

pub struct Mixer {
    shared: Arc<Shared>,
    rate: u32,
    buf: Vec<f32>,
    idx: usize,
    ended: bool,
}

impl Mixer {
    fn fill(&mut self) -> bool {
        let plan = self
            .shared
            .plan
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut pos = self.shared.pos.load(Ordering::Relaxed);
        let target = self.shared.seek.swap(-1, Ordering::Relaxed);
        if target >= 0 {
            pos = target as u64;
        }
        if pos >= plan.frames && plan.loop_region.is_none() {
            self.shared
                .pos
                .store(pos.min(plan.frames), Ordering::Relaxed);
            return false;
        }
        self.buf.resize(BLOCK * 2, 0.0);
        for v in self.buf.iter_mut() {
            *v = 0.0;
        }
        let (w, next) = mix_step(&plan, pos, &mut self.buf);
        self.shared.pos.store(next, Ordering::Relaxed);
        self.idx = 0;
        if w == 0 {
            return false;
        }
        // une fin de bloc incomplète (fin de projet) : les échantillons
        // restants sont déjà à zéro, c'est le silence de fin, pas un trou
        if w < BLOCK {
            self.buf.truncate(w * 2);
        } else {
            self.buf.resize(BLOCK * 2, 0.0);
        }
        true
    }
}

impl Iterator for Mixer {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.ended {
            return None;
        }
        if self.idx >= self.buf.len() {
            if !self.fill() {
                self.ended = true;
                return None;
            }
        }
        let v = self.buf[self.idx];
        self.idx += 1;
        Some(v)
    }
}

impl rodio::Source for Mixer {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        let plan = self
            .shared
            .plan
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if plan.loop_region.is_some() {
            None
        } else {
            Some(Duration::from_secs_f64(
                plan.frames as f64 / self.rate.max(1) as f64,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, Fill, Project, Source};

    fn ramp(n: usize) -> Arc<Vec<f32>> {
        Arc::new((0..n).map(|i| (i as f32 / n as f32) * 0.5).collect())
    }

    fn project() -> Project {
        let mut p = Project::new("mix");
        p.tempo_bpm = 120.0;
        let idx = p.add_source(Source {
            id: 1,
            title: "a".into(),
            site: "fichiers".into(),
            entry_id: -1,
            file: "/tmp/a.wav".into(),
            subsong: 1,
            loop_start: None,
            loop_end: None,
            bpm: 120.0,
            phase: 0.0,
            key: None,
        });
        let t = p.add_track("p1", Some(idx), 1);
        p.add_clip(Clip {
            id: 0,
            track: t,
            source: idx,
            src_start: 0,
            src_end: 480,
            at_beat: 0.0,
            length_beats: 1.0,
            gain: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            transpose: None,
            key_lock: None,
            key: None,
            stretch: None,
            fill: Fill::Trim,
        });
        p
    }

    #[test]
    fn plan_place_le_clip_sur_la_grille() {
        let p = project();
        let audio = vec![Some(SourceAudio::new(ramp(480), 1, 48_000))];
        let plan = Plan::build(&p, &audio, 48_000);
        assert_eq!(plan.out_rate, 48_000);
        // 1 temps à 120 BPM = 0,5 s = 24 000 trames
        assert_eq!(plan.frames, 96_000, "4 temps minimum");
        assert_eq!(plan.voices.len(), 1);
        assert_eq!(plan.voices[0].start, 0);
        assert_eq!(plan.voices[0].end, 24_000);
        // 480 trames de source pour 24 000 de sortie → lecture très lente
        assert!((plan.voices[0].ratio - 480.0 / 24_000.0).abs() < 1e-9);
    }

    #[test]
    fn deux_voies_se_somment() {
        let mut p = project();
        let src = p.sources[0].clone();
        p.add_source(src);
        let c = p.clips[0].clone();
        let mut c2 = c.clone();
        c2.source = 1;
        p.add_clip(c2);
        let audio = vec![
            Some(SourceAudio::new(ramp(480), 1, 48_000)),
            Some(SourceAudio::new(ramp(480), 1, 48_000)),
        ];
        let plan = Plan::build(&p, &audio, 48_000);
        assert_eq!(plan.voices.len(), 2);
        let one = Plan::build(&project(), &audio[..1], 48_000);
        let a = render(&one, 1000);
        let b = render(&plan, 1000);
        // gain centre = 0,707 par voie : deux voies identiques = une fois la somme
        for i in 0..1000 {
            assert!((b[i * 2] - 2.0 * a[i * 2]).abs() < 1e-3);
        }
    }

    #[test]
    fn rendu_egal_au_temps_reel() {
        let p = project();
        let audio = vec![Some(SourceAudio::new(ramp(48_000), 2, 48_000))];
        let plan = Plan::build(&p, &audio, 48_000);
        let offline = render(&plan, 30_000);
        let t = Transport::new(plan.clone());
        let mut mixer = t.source();
        let mut live = Vec::with_capacity(60_000);
        for _ in 0..60_000 {
            live.push(mixer.next().unwrap_or(0.0));
        }
        let diffs = live.iter().zip(&offline).filter(|(a, b)| a != b).count();
        if diffs > 0 {
            let (i, (a, b)) = live
                .iter()
                .zip(&offline)
                .enumerate()
                .find(|(_, (a, b))| a != b)
                .unwrap();
            panic!("écart à l'échantillon {i} : {a} vs {b} ({diffs} écarts)");
        }
    }

    #[test]
    fn boucle_de_projet_et_fin() {
        let mut p = project();
        p.loop_beats = Some((0.0, 1.0));
        let audio = vec![Some(SourceAudio::new(ramp(480), 1, 48_000))];
        let plan = Plan::build(&p, &audio, 48_000);
        let out = render(&plan, 48_000);
        // la boucle de 1 temps (24 000 trames) se répète deux fois, à l'identique
        assert!((out[0] - out[24_000 * 2]).abs() < 1e-6);
        let t = Transport::new(plan);
        let mut m = t.source();
        // une boucle infinie ne rend jamais None
        assert!((0..200_000).all(|_| m.next().is_some()));
    }

    #[test]
    fn fade_et_gain_par_piste() {
        let mut p = project();
        p.clips[0].length_beats = 2.0;
        p.clips[0].fade_in = 0.25;
        p.clips[0].fade_out = 0.25;
        let audio = vec![Some(SourceAudio::new(
            Arc::new(vec![1.0f32; 480]),
            1,
            48_000,
        ))];
        let plan = Plan::build(&p, &audio, 48_000);
        let out = render(&plan, 48_000);
        assert!(out[0].abs() < 1e-6, "début du fade-in");
        assert!(out[48_000 * 2 - 2].abs() < 1e-3, "fin du fade-out");
        // au milieu, plus aucun fade : gain constant
        let mid = out[(24_000 * 2) as usize];
        assert!((mid - 0.707_106_77).abs() < 1e-3);
    }

    #[test]
    fn metronome_marque_les_temps_sans_polluer_le_reste() {
        let mut p = Project::new("click");
        p.tempo_bpm = 120.0; // 0,5 s par temps à 48 kHz = 24 000 trames
        p.metronome = true;
        let plan = Plan::build(&p, &[], 48_000);
        let out = render(&plan, 28_000);
        let peak =
            |a: usize, b: usize| out[a * 2..b * 2].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak(0, 900) > 0.1, "clic sur le temps 1");
        assert!(peak(10_000, 20_000) < 1e-4, "silence entre deux temps");
        assert!(peak(24_000, 24_900) > 0.1, "clic sur le temps 2");
        // sans métronome, la même plage est muette
        p.metronome = false;
        let plan = Plan::build(&p, &[], 48_000);
        let out = render(&plan, 28_000);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn gain_general_agit_sur_les_deux_sorties() {
        let p = project();
        let audio = vec![Some(SourceAudio::new(ramp(480), 1, 48_000))];
        let full = render(&Plan::build(&p, &audio, 48_000), 24_000);
        let mut half = p.clone();
        half.master_gain = 0.5;
        let quiet = render(&Plan::build(&half, &audio, 48_000), 24_000);
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak(&quiet) - peak(&full) * 0.5).abs() < 1e-5);
        assert!(quiet.iter().all(|v| v.is_finite()));
    }
}
