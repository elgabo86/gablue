use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::{OutputStream, Sink, Source};

use crate::model::LoopWindow;

#[derive(Default)]
struct Shared {
    pos: AtomicU64,
    seek: AtomicI64,
    loop_start: AtomicU64,
    loop_end: AtomicU64,
    looping: AtomicBool,
}

struct LoopSource {
    data: Arc<Vec<f32>>,
    channels: u16,
    rate: u32,
    pos: usize,
    shared: Arc<Shared>,
    /// Fondu croisé à la jonction de boucle (échantillons) : la période
    /// détectée peut être à quelques millisecondes près, et le rendu PSF n'est
    /// pas toujours identique d'un tour à l'autre → sans lui, la couture
    /// s'entend (vécu 20/11 : « mini coupe »). Court volontairement : au-delà
    /// de ~2 ms on mélange deux moments musicaux différents (fin de phrase +
    /// attaque), ce qui s'entend comme un flam (vécu 20/11 : « quelques ms de
    /// décalage » avec 10 ms).
    xfade: usize,
}

impl Iterator for LoopSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let target = self.shared.seek.swap(-1, Ordering::Relaxed);
        if target >= 0 {
            self.pos = target as usize;
        }
        let looping = self.shared.looping.load(Ordering::Relaxed);
        if looping {
            let mut end = self.shared.loop_end.load(Ordering::Relaxed) as usize;
            let start = self.shared.loop_start.load(Ordering::Relaxed) as usize;
            // fin non renseignée ou hors buffer = fin du fichier : on boucle quand même
            if end == 0 || end > self.data.len() {
                end = self.data.len();
            }
            if end > start && self.pos >= end {
                self.pos = start;
            }
            // fondu croisé de fin de tour : on mélange la fin du tour avec le
            // début du suivant, puis on reprend après la partie déjà fondue
            let xfade = self
                .xfade
                .min((end.saturating_sub(start)) / 4)
                .min(end.saturating_sub(start).saturating_sub(1));
            if end > start + 2 && xfade > 0 && self.pos >= end - xfade {
                let k = self.pos - (end - xfade);
                // rampe en cosinus : pente nulle aux deux bouts, donc pas de
                // marche résiduelle à l'entrée ni à la sortie du fondu
                let g = 0.5 - 0.5 * (std::f32::consts::PI * k as f32 / xfade as f32).cos();
                let tail = self.data[self.pos];
                let head = self.data[start + k.min(end - start - 1)];
                self.pos += 1;
                if self.pos >= end {
                    self.pos = start + xfade;
                }
                self.shared.pos.store(self.pos as u64, Ordering::Relaxed);
                return Some(tail * (1.0 - g) + head * g);
            }
        }
        if self.pos >= self.data.len() {
            return None;
        }
        let v = self.data[self.pos];
        self.pos += 1;
        self.shared.pos.store(self.pos as u64, Ordering::Relaxed);
        Some(v)
    }
}

impl Source for LoopSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.data.len() as f64 / (self.rate as f64 * self.channels as f64),
        ))
    }
}

/// Lecteur unique partagé par le TUI et la GUI : rodio + boucle sur fenêtre.
pub struct Player {
    _stream: OutputStream,
    sink: Sink,
    shared: Arc<Shared>,
    channels: u16,
    rate: u32,
    total: f64,
    window: LoopWindow,
    looping: bool,
    volume: f32,
    paused: bool,
    /// Échantillons de la piste courante : permet de relancer une source
    /// (rodio jette la précédente quand elle atteint la fin).
    pcm: Option<Arc<Vec<f32>>>,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (stream, handle) = OutputStream::try_default().context("sortie audio indisponible")?;
        let sink = Sink::try_new(&handle)?;
        sink.set_volume(1.0);
        Ok(Self {
            _stream: stream,
            sink,
            shared: Arc::new(Shared::default()),
            channels: 2,
            rate: 44100,
            total: 0.0,
            window: LoopWindow {
                start: 0.0,
                end: 0.0,
            },
            looping: false,
            volume: 1.0,
            paused: false,
            pcm: None,
        })
    }

    pub fn load(
        &mut self,
        pcm: Arc<Vec<f32>>,
        channels: u16,
        rate: u32,
        total: f64,
        window: LoopWindow,
        looping: bool,
    ) {
        self.sink.stop();
        self.channels = channels;
        self.rate = rate;
        self.total = total;
        self.window = window;
        self.looping = looping;
        self.paused = false;

        let to_samples = |s: f64| {
            let frames = (s * rate as f64).round() as u64;
            frames * channels as u64
        };
        self.shared.pos.store(0, Ordering::Relaxed);
        self.shared.seek.store(-1, Ordering::Relaxed);
        self.shared
            .loop_start
            .store(to_samples(window.start), Ordering::Relaxed);
        self.shared
            .loop_end
            .store(to_samples(window.end), Ordering::Relaxed);
        self.shared
            .looping
            .store(looping && window.end > window.start, Ordering::Relaxed);

        self.pcm = Some(pcm);
        self.queue_from(0.0);
        self.sink.play();
    }

    /// (Ré)injecte la source audio à partir de `secs`. Nécessaire après la fin
    /// du morceau : rodio a consommé la source, la boucle ne peut plus s'y
    /// activer (vécu 20/11 : « boucle ON » sans effet une fois la piste finie).
    fn queue_from(&self, secs: f64) {
        let Some(pcm) = &self.pcm else {
            return;
        };
        let fps = self.rate as f64 * self.channels as f64;
        self.shared
            .pos
            .store((secs.max(0.0) * fps).round() as u64, Ordering::Relaxed);
        self.shared.seek.store(-1, Ordering::Relaxed);
        self.sink.append(LoopSource {
            data: pcm.clone(),
            channels: self.channels,
            rate: self.rate,
            pos: (secs.max(0.0) * fps).round() as usize,
            shared: self.shared.clone(),
            xfade: ((0.002 * fps).round() as usize / self.channels as usize)
                * self.channels as usize,
        });
    }

    /// Plus aucune source en file (le morceau est arrivé au bout).
    pub fn exhausted(&self) -> bool {
        self.sink.empty() && !self.paused
    }

    pub fn window(&self) -> LoopWindow {
        self.window
    }

    /// Change la fenêtre de boucle à chaud (éditeur A/B), sans relancer la lecture.
    pub fn set_window(&mut self, w: LoopWindow) {
        let to_samples = |s: f64| {
            let frames = (s * self.rate as f64).round() as u64;
            frames * self.channels as u64
        };
        let start = w.start.clamp(0.0, self.total.max(0.0));
        let end = w.end.clamp(start, self.total.max(0.0));
        self.window = LoopWindow { start, end };
        self.shared
            .loop_start
            .store(to_samples(start), Ordering::Relaxed);
        self.shared
            .loop_end
            .store(to_samples(end), Ordering::Relaxed);
        self.shared
            .looping
            .store(self.looping && end > start, Ordering::Relaxed);
    }

    pub fn total(&self) -> f64 {
        self.total
    }

    /// Position absolue dans le fichier (non repliée).
    pub fn position_abs(&self) -> f64 {
        self.shared.pos.load(Ordering::Relaxed) as f64 / (self.rate as f64 * self.channels as f64)
    }

    /// Position affichée : repliée dans la fenêtre de boucle.
    pub fn position(&self) -> f64 {
        let abs = self.position_abs();
        if self.looping && self.window.end > self.window.start && abs >= self.window.end {
            let len = self.window.len();
            self.window.start + (abs - self.window.start) % len
        } else {
            abs
        }
    }

    pub fn seek(&self, secs: f64) {
        let secs = secs.clamp(0.0, self.total.max(0.0));
        if self.sink.empty() {
            // piste terminée : on relance la source à l'endroit demandé
            self.queue_from(secs);
            self.sink.play();
            return;
        }
        let idx = (secs * self.rate as f64 * self.channels as f64).round() as i64;
        self.shared.seek.store(idx, Ordering::Relaxed);
    }

    pub fn toggle_pause(&mut self) -> bool {
        if !self.paused && self.sink.empty() {
            // relancer après la fin du morceau
            self.queue_from(0.0);
            self.sink.play();
            self.paused = false;
            return false;
        }
        self.paused = !self.paused;
        if self.paused {
            self.sink.pause();
        } else {
            self.sink.play();
        }
        self.paused
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_looping(&mut self, on: bool) {
        self.looping = on;
        self.shared
            .looping
            .store(on && self.window.end > self.window.start, Ordering::Relaxed);
        // piste déjà finie : la boucle n'aurait plus rien à enrouler
        if on && self.sink.empty() {
            self.queue_from(self.window.start);
            self.sink.play();
            self.paused = false;
        }
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.5);
        self.sink.set_volume(self.volume);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Vrai quand une lecture non bouclée est arrivée au bout.
    pub fn finished(&self) -> bool {
        !self.looping && self.total > 0.0 && self.position_abs() >= self.total - 0.05
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(start: u64, end: u64) -> Arc<Shared> {
        let sh = Arc::new(Shared::default());
        sh.loop_start.store(start, Ordering::Relaxed);
        sh.loop_end.store(end, Ordering::Relaxed);
        sh.looping.store(true, Ordering::Relaxed);
        sh
    }

    #[test]
    fn boucle_infinie_et_fondu_croise() {
        // sinus dont la période (512) ne divise pas la fenêtre (500) : sans
        // fondu croisé la couture fait un saut
        let data: Arc<Vec<f32>> = Arc::new(
            (0..2000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 512.0).sin())
                .collect(),
        );
        let mut src = LoopSource {
            data,
            channels: 1,
            rate: 1000,
            pos: 0,
            shared: shared(0, 500),
            xfade: 10,
        };
        let mut prev = 0.0f32;
        let mut max_jump = 0.0f32;
        let mut n = 0;
        for v in src.by_ref().take(1500) {
            max_jump = max_jump.max((v - prev).abs());
            prev = v;
            n += 1;
        }
        assert_eq!(n, 1500, "la boucle ne doit jamais s'arrêter");
        // un saut de couture non fondu vaudrait ~0.4 ; le fondu le réduit
        assert!(max_jump < 0.2, "saut maximal {max_jump}");
        let p = src.shared.pos.load(Ordering::Relaxed);
        assert!(p >= 0 && p < 500, "position hors fenêtre : {p}");
    }
}
