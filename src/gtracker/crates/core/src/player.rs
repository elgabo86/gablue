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
    /// Fin d'un segment joué une fois (`0` = aucun) : la source rend `None` à
    /// cet échantillon, ce qui permet de jouer des tranches à la suite.
    stop: AtomicU64,
    /// Fenêtre de boucle suivante, appliquée **à la couture** par la source :
    /// un changement de pad en live tombe ainsi à l'échantillon près, sans
    /// attendre le prochain rafraîchissement de la GUI.
    next_start: AtomicU64,
    next_end: AtomicU64,
    /// Segment suivant d'une suite (lecture unique), **empaqueté dans un seul
    /// atomique** : `(fin << 32) | début`, `0` = rien. Deux atomiques séparés
    /// laissaient une fenêtre où la source lisait un début neuf avec une fin
    /// encore à zéro → `None` → trou audible à la couture (question user
    /// 21/11 : « aucune latence entre chaque segment ? »).
    seg_next: AtomicU64,
    /// Nombre de sauts de suite effectués (horloge de la GUI).
    jumps: AtomicU64,
    /// Nombre de tours de boucle effectués (la GUI s'en sert comme horloge).
    turns: AtomicU64,
}

struct LoopSource {
    data: Arc<Vec<f32>>,
    channels: u16,
    rate: u32,
    pos: usize,
    /// Zéros exacts en tête du rendu (voir `head_silence`), **en échantillons
    /// entrelacés** (trames × canaux) comme `pos` : enjambés dès que la lecture
    /// revient à 0, sinon ils s'entendent comme un trou à la couture « dernière
    /// tranche → première » (vécu 21/11, FF8 « Force Your Way »). Une valeur en
    /// trames donnerait une position impaire en stéréo, donc les canaux
    /// inversés (vécu 21/11 : « une fois à gauche, une fois à droite »).
    head: usize,
    shared: Arc<Shared>,
    /// Fondu croisé à la jonction de boucle (échantillons) : la période
    /// détectée peut être à quelques millisecondes près, et le rendu PSF n'est
    /// pas toujours identique d'un tour à l'autre → sans lui, la couture
    /// s'entend (vécu 20/11 : « mini coupe »). Court volontairement : au-delà
    /// de ~2 ms on mélange deux moments musicaux différents (fin de phrase +
    /// attaque), ce qui s'entend comme un flam (vécu 20/11 : « quelques ms de
    /// décalage » avec 10 ms).
    xfade: usize,
    /// Échantillons de tête du segment suivant **déjà entendus** dans le fondu
    /// de couture (voir `seam_skip` dans `next`) : la reprise les saute, sinon
    /// les 2 ms du raccord seraient rejouées (flam).
    seam_skip: usize,
}

impl Iterator for LoopSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let target = self.shared.seek.swap(-1, Ordering::Relaxed);
        if target >= 0 {
            self.pos = target as usize;
        }
        // retour dans le blanc de tête (début de piste, ou mélange qui y
        // retombe) : on le saute, il ne fait pas partie de la musique
        if self.pos < self.head {
            self.pos = self.head;
        }
        let looping = self.shared.looping.load(Ordering::Relaxed);
        if !looping {
            let stop = self.shared.stop.load(Ordering::Relaxed) as usize;
            if stop > 0 && self.pos >= stop {
                // segment suivant d'une suite : on enchaîne dans la source,
                // sinon un trou de 60 ms (une frame de GUI) s'entendrait
                // consommation et remise à zéro en **une** opération (swap) :
                // la GUI peut programmer le suivant juste après sans risquer
                // d'être écrasée par la remise à zéro
                let queued = self.shared.seg_next.swap(0, Ordering::Relaxed);
                let ns = (queued & 0xffff_ffff) as usize;
                let ne = (queued >> 32) as usize;
                if ne > ns && ne <= self.data.len() {
                    self.shared.stop.store(ne as u64, Ordering::Relaxed);
                    self.shared.jumps.fetch_add(1, Ordering::Relaxed);
                    // reprise après ce que le fondu de couture a déjà fait
                    // entendre du début du segment suivant
                    let debut = self.audible_start(ns, ne) + self.seam_skip;
                    self.pos = debut.min(ne.saturating_sub(1));
                    self.seam_skip = 0;
                } else {
                    return None;
                }
            } else if let Some(v) = self.seam(stop) {
                return Some(v);
            }
        }
        if looping {
            let mut end = self.shared.loop_end.load(Ordering::Relaxed) as usize;
            let start = self.shared.loop_start.load(Ordering::Relaxed) as usize;
            // fin non renseignée ou hors buffer = fin du fichier : on boucle quand même
            if end == 0 || end > self.data.len() {
                end = self.data.len();
            }
            if end > start && self.pos >= end {
                self.pos = self.wrap_to();
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
                let debut = self.audible_start(start, end);
                let first = self.data[(debut + k).min(self.data.len() - 1)];
                self.pos += 1;
                if self.pos >= end {
                    // bascule de pad à la couture : on démarre le segment
                    // suivant à son début, sans rejouer le fondu
                    self.pos = if self.swap_pending() {
                        self.wrap_to()
                    } else {
                        self.shared.turns.fetch_add(1, Ordering::Relaxed);
                        (debut + xfade).min(end)
                    };
                }
                self.shared.pos.store(self.pos as u64, Ordering::Relaxed);
                return Some(tail * (1.0 - g) + first * g);
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

impl LoopSource {
    /// Premier échantillon **audible** quand on démarre à `start` : le début du
    /// rendu peut être un blanc numérique (SPU/driver), qu'on ne rejoue pas.
    /// `end` borne le saut : une fenêtre plus courte que le blanc (cas absurde
    /// mais possible : boucle de quelques échantillons) ne doit pas renvoyer une
    /// position hors fenêtre, sinon la source boucle sur elle-même.
    fn audible_start(&self, start: usize, end: usize) -> usize {
        if start != 0 {
            return start;
        }
        let mut p = self.head.min(self.data.len());
        if end > 0 {
            p = p.min(end.saturating_sub(1));
        }
        p
    }

    /// Fondu croisé de **couture de suite** : sur les `xfade` derniers
    /// échantillons du segment courant, mélange sa fin avec le début du segment
    /// suivant programmé, puis la reprise saute ce qui vient d'être entendu.
    ///
    /// Sans lui, deux tranches contiguës se raccordent sur une marche : la
    /// dernière tranche s'arrête au milieu de la musique et l'attaque suivante
    /// démarre net — « léger grésillement saccadé mal découpé » à chaque
    /// enchaînement (vécu 26/11). Même rampe en cosinus et même durée que la
    /// couture de boucle (2 ms) : au-delà, on mélange deux moments différents.
    ///
    /// Rend `None` quand il n'y a rien à faire (pas de suivant en file, ou pas
    /// encore dans la fenêtre) : la source rend alors l'échantillon courant.
    fn seam(&mut self, stop: usize) -> Option<f32> {
        if stop == 0 || self.pos >= self.data.len() {
            return None;
        }
        // rampe **par trame** : un nombre impair d'échantillons en stéréo
        // donnerait un gain différent à gauche et à droite, et la reprise
        // tomberait sur le canal de l'autre côté (vécu 21/11 sur le blanc de
        // tête : « une fois à gauche, une fois à droite »)
        let ch = (self.channels.max(1)) as usize;
        let mut xf = (self.xfade.min(stop / 2) / ch) * ch;
        if xf <= ch || self.pos + xf < stop {
            return None;
        }
        let queued = self.shared.seg_next.load(Ordering::Relaxed);
        let ns = (queued & 0xffff_ffff) as usize;
        let ne = (queued >> 32) as usize;
        if ne <= ns || ne > self.data.len() {
            return None;
        }
        // jamais plus de la moitié d'un segment : un raccord ne doit pas
        // devenir une transition
        xf = xf.min(((ne - ns) / 2 / ch) * ch);
        if xf <= ch {
            return None;
        }
        let k = self.pos - (stop - xf);
        let g = 0.5 - 0.5 * (std::f32::consts::PI * k as f32 / xf as f32).cos();
        let tail = self.data[self.pos];
        let debut = self.audible_start(ns, ne);
        let first = self.data[(debut + k).min(self.data.len() - 1)];
        self.seam_skip = (k + 1).min(ne.saturating_sub(debut));
        self.pos += 1;
        self.shared.pos.store(self.pos as u64, Ordering::Relaxed);
        Some(tail * (1.0 - g) + first * g)
    }
}

/// Fenêtre suivante programmée pendant la boucle courante (pads live).
impl LoopSource {
    fn swap_pending(&self) -> bool {
        let ns = self.shared.next_start.load(Ordering::Relaxed);
        let ne = self.shared.next_end.load(Ordering::Relaxed);
        ne > ns
    }

    /// Position de reprise à la couture : la fenêtre suivante si elle est
    /// programmée, sinon le début de la fenêtre courante. Le changement se fait
    /// ici, dans la source, donc à l'échantillon près.
    fn wrap_to(&mut self) -> usize {
        let ns = self.shared.next_start.load(Ordering::Relaxed);
        let ne = self.shared.next_end.load(Ordering::Relaxed);
        self.shared.turns.fetch_add(1, Ordering::Relaxed);
        if ne > ns && ne as usize <= self.data.len() {
            self.shared.loop_start.store(ns, Ordering::Relaxed);
            self.shared.loop_end.store(ne, Ordering::Relaxed);
            self.shared.next_start.store(0, Ordering::Relaxed);
            self.shared.next_end.store(0, Ordering::Relaxed);
            // une bascule de pad vers la première tranche saute aussi le blanc
            let debut = self.audible_start(ns as usize, ne as usize);
            self.shared.pos.store(debut as u64, Ordering::Relaxed);
            debut
        } else {
            let start = self.audible_start(
                self.shared.loop_start.load(Ordering::Relaxed) as usize,
                self.shared.loop_end.load(Ordering::Relaxed) as usize,
            );
            self.shared.pos.store(start as u64, Ordering::Relaxed);
            start
        }
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
            looping: false,
            volume: 1.0,
            paused: false,
            pcm: None,
        })
    }

    /// Charge un rendu. `autoplay = false` le laisse **muet** (en pause, tête
    /// à 0) : un fichier déposé sur la fenêtre s'ouvre prêt à découper, sans
    /// jouer tout seul (demande user 21/11).
    pub fn load(
        &mut self,
        pcm: Arc<Vec<f32>>,
        channels: u16,
        rate: u32,
        total: f64,
        window: LoopWindow,
        looping: bool,
        autoplay: bool,
    ) {
        self.sink.stop();
        self.channels = channels;
        self.rate = rate;
        self.total = total;
        self.looping = looping;
        self.paused = false;

        let to_samples = |s: f64| {
            let frames = (s * rate as f64).round() as u64;
            frames * channels as u64
        };
        self.shared.pos.store(0, Ordering::Relaxed);
        self.shared.seek.store(-1, Ordering::Relaxed);
        self.shared.stop.store(0, Ordering::Relaxed);
        self.shared.next_start.store(0, Ordering::Relaxed);
        self.shared.next_end.store(0, Ordering::Relaxed);
        self.cancel_queued_segment();
        self.shared.jumps.store(0, Ordering::Relaxed);
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
        if autoplay {
            self.sink.play();
        } else {
            self.sink.pause();
            self.paused = true;
        }
    }

    /// (Ré)injecte la source audio à partir de `secs`. Nécessaire après la fin
    /// du morceau : rodio a consommé la source, la boucle ne peut plus s'y
    /// activer (vécu 20/11 : « boucle ON » sans effet une fois la piste finie).
    fn queue_from(&self, secs: f64) {
        let Some(pcm) = &self.pcm else {
            return;
        };
        // une source réinjectée ne doit pas hériter d'une bascule de pad périmée
        self.cancel_queued_loop();
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
            head: crate::decode::head_silence(pcm, self.channels as usize, self.rate),
            shared: self.shared.clone(),
            xfade: ((0.002 * fps).round() as usize / self.channels as usize)
                * self.channels as usize,
            seam_skip: 0,
        });
    }

    /// Joue `[start, end]` une seule fois puis s'arrête : c'est le mode
    /// « segment » de la découpe rythmique, l'appelant enchaîne sur le suivant
    /// quand `exhausted()` devient vrai.
    pub fn play_segment(&mut self, start: f64, end: f64) {
        let to_samples = |s: f64| {
            let frames = (s * self.rate as f64).round() as u64;
            frames * self.channels as u64
        };
        let start = start.clamp(0.0, self.total.max(0.0));
        let end = end.clamp(start, self.total.max(0.0));
        self.looping = false;
        self.paused = false;
        self.shared.looping.store(false, Ordering::Relaxed);
        self.shared.stop.store(to_samples(end), Ordering::Relaxed);
        self.cancel_queued_loop();
        self.cancel_queued_segment();
        // on repart d'une file propre : une source déjà consommée ne peut pas
        // recevoir le nouveau point de départ
        self.sink.stop();
        self.queue_from(start);
        self.sink.play();
    }

    /// Programme la fenêtre de boucle suivante, appliquée par la source **à la
    /// couture** du tour en cours (bascule de pad à l'échantillon près).
    pub fn queue_loop(&mut self, w: LoopWindow) {
        let to_samples = |s: f64| {
            let frames = (s * self.rate as f64).round() as u64;
            frames * self.channels as u64
        };
        let start = w.start.clamp(0.0, self.total.max(0.0));
        let end = w.end.clamp(start, self.total.max(0.0));
        let start = to_samples(start);
        let end = to_samples(end);
        if end > start {
            self.shared.next_start.store(start, Ordering::Relaxed);
            self.shared.next_end.store(end, Ordering::Relaxed);
        }
    }

    pub fn cancel_queued_loop(&self) {
        self.shared.next_start.store(0, Ordering::Relaxed);
        self.shared.next_end.store(0, Ordering::Relaxed);
    }

    /// Programme le segment suivant d'une suite : la source y saute à la fin du
    /// segment courant (`play_segment`), donc sans le trou d'une frame de GUI.
    pub fn queue_segment(&mut self, w: LoopWindow) {
        let to_samples = |s: f64| {
            let frames = (s * self.rate as f64).round() as u64;
            frames * self.channels as u64
        };
        let start = w.start.clamp(0.0, self.total.max(0.0));
        let end = w.end.clamp(start, self.total.max(0.0));
        let start = to_samples(start);
        let end = to_samples(end);
        if end > start {
            self.shared
                .seg_next
                .store((end << 32) | start, Ordering::Relaxed);
        }
    }

    pub fn cancel_queued_segment(&self) {
        self.shared.seg_next.store(0, Ordering::Relaxed);
    }

    /// Nombre de sauts de suite déjà effectués (horloge de la GUI).
    pub fn segment_jumps(&self) -> u64 {
        self.shared.jumps.load(Ordering::Relaxed)
    }

    /// Nombre de tours de boucle déjà effectués (horloge de la GUI).
    pub fn loop_turns(&self) -> u64 {
        self.shared.turns.load(Ordering::Relaxed)
    }

    /// Plus aucune source en file (le morceau est arrivé au bout).
    pub fn exhausted(&self) -> bool {
        self.sink.empty() && !self.paused
    }

    /// Fenêtre de boucle **live** : lue dans les atomiques que la source audio
    /// réécrit quand elle applique une bascule de pad à la couture (la GUI doit
    /// voir le segment réellement en train de sonner, pas le dernier réglé).
    pub fn window(&self) -> LoopWindow {
        let fps = (self.rate as f64 * self.channels as f64).max(1.0);
        LoopWindow {
            start: self.shared.loop_start.load(Ordering::Relaxed) as f64 / fps,
            end: self.shared.loop_end.load(Ordering::Relaxed) as f64 / fps,
        }
    }

    /// Relance la sortie si elle est en pause ou terminée : un pad doit sonner
    /// même quand rien ne joue (demande user, 21/11).
    pub fn resume(&mut self) {
        if self.sink.empty() {
            self.shared.stop.store(0, Ordering::Relaxed);
            self.cancel_queued_segment();
            self.queue_from(self.window().start);
        }
        self.sink.play();
        self.paused = false;
    }

    /// Relâchement d'un pad : le tour de boucle en cours finit, puis la lecture
    /// s'arrête (elle ne fuit pas dans la suite du morceau).
    pub fn stop_after_window(&mut self) {
        let to_samples = |s: f64| {
            let frames = (s * self.rate as f64).round() as u64;
            frames * self.channels as u64
        };
        let end = self.window().end;
        self.looping = false;
        self.shared.looping.store(false, Ordering::Relaxed);
        self.shared.stop.store(to_samples(end), Ordering::Relaxed);
        self.cancel_queued_loop();
    }

    /// Coupe nette : plus rien ne sonne, sans finir le tour en cours. Sert aux
    /// gestes annulés (glisser une tuile au lieu de la tenir) et aux
    /// changements de découpe, où finir la boucle ferait traîner 8 s de
    /// musique sur une nouvelle grille.
    pub fn halt(&mut self) {
        self.sink.pause();
        self.paused = true;
        self.cancel_queued_loop();
        self.cancel_queued_segment();
        self.shared.looping.store(false, Ordering::Relaxed);
        self.shared.stop.store(0, Ordering::Relaxed);
    }

    /// Change la fenêtre de boucle à chaud (éditeur A/B), sans relancer la lecture.
    pub fn set_window(&mut self, w: LoopWindow) {
        let to_samples = |s: f64| {
            let frames = (s * self.rate as f64).round() as u64;
            frames * self.channels as u64
        };
        let start = w.start.clamp(0.0, self.total.max(0.0));
        let end = w.end.clamp(start, self.total.max(0.0));
        // un changement manuel remplace une bascule de pad programmée
        self.cancel_queued_loop();
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

    /// Joue une source arbitraire (le mixer du GTracker) : coupe la piste
    /// courante. Un seul propriétaire du son, donc jamais deux flux mélangés
    /// par surprise.
    pub fn play_source<S>(&mut self, source: S)
    where
        S: rodio::Source<Item = f32> + Send + 'static,
    {
        self.sink.stop();
        self.pcm = None;
        self.looping = false;
        self.paused = false;
        self.shared.looping.store(false, Ordering::Relaxed);
        self.sink.append(source);
        self.sink.play();
    }

    /// Coupe la source courante (mixer du GTracker) sans toucher à la
    /// préférence de boucle de la bibliothèque.
    /// Écoute d'un flux court (une tranche du projet) **sans oublier la piste**
    /// chargée : `pcm` reste, donc la bibliothèque peut reprendre ensuite à sa
    /// position (`resume_at`). Contrairement à `play_source`, qui sert le
    /// mixeur d'un projet et consomme la piste.
    pub fn play_preview<S>(&mut self, source: S)
    where
        S: rodio::Source<Item = f32> + Send + 'static,
    {
        self.sink.stop();
        self.looping = false;
        self.paused = false;
        self.shared.looping.store(false, Ordering::Relaxed);
        self.sink.append(source);
        self.sink.play();
    }

    /// Reprend la piste à `secs` en repartant d'une source neuve : après une
    /// écoute de tranche, la sortie est vide et `toggle_pause` repartirait de 0.
    pub fn resume_at(&mut self, secs: f64) {
        if self.pcm.is_none() {
            return;
        }
        self.sink.stop();
        self.shared.stop.store(0, Ordering::Relaxed);
        self.cancel_queued_loop();
        self.cancel_queued_segment();
        self.queue_from(secs.max(0.0));
        self.sink.play();
        self.paused = false;
    }

    pub fn stop_source(&mut self) {
        self.sink.stop();
        self.pcm = None;
        self.paused = true;
    }

    pub fn total(&self) -> f64 {
        self.total
    }

    /// Vrai quand une piste (bibliothèque) est chargée : `play_source` sert un
    /// autre flux et l'oublie, donc l'appelant doit pouvoir savoir s'il faut la
    /// réinjecter avant de relancer la lecture.
    pub fn ready(&self) -> bool {
        self.pcm.is_some()
    }

    /// Position absolue dans le fichier (non repliée).
    pub fn position_abs(&self) -> f64 {
        self.shared.pos.load(Ordering::Relaxed) as f64 / (self.rate as f64 * self.channels as f64)
    }

    /// Position affichée : repliée dans la fenêtre de boucle.
    pub fn position(&self) -> f64 {
        let abs = self.position_abs();
        let w = self.window();
        if self.looping && w.end > w.start && abs >= w.end {
            let len = w.len();
            w.start + (abs - w.start) % len
        } else {
            abs
        }
    }

    pub fn seek(&self, secs: f64) {
        let secs = secs.clamp(0.0, self.total.max(0.0));
        // une navigation manuelle sort du mode segment et annule une bascule
        // de pad programmée
        self.shared.stop.store(0, Ordering::Relaxed);
        self.cancel_queued_loop();
        self.cancel_queued_segment();
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
            // relancer après la fin du morceau (on quitte un éventuel segment)
            self.shared.stop.store(0, Ordering::Relaxed);
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
        // bascule de mode : un segment en cours ne doit pas couper la boucle
        self.shared.stop.store(0, Ordering::Relaxed);
        if !on {
            self.cancel_queued_loop();
            self.cancel_queued_segment();
        }
        self.shared.looping.store(
            on && self.window().end > self.window().start,
            Ordering::Relaxed,
        );
        // piste déjà finie : la boucle n'aurait plus rien à enrouler
        if on && self.sink.empty() {
            self.queue_from(self.window().start);
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

    /// L'écoute d'une tranche ne doit pas oublier la piste chargée : c'est ce
    /// qui laissait le lecteur muet au retour dans l'éditeur (vécu 21/11).
    #[test]
    fn une_ecoute_ne_perd_pas_la_piste() {
        let Ok(mut p) = Player::new() else {
            eprintln!("aucune sortie audio : test non applicable");
            return;
        };
        let data = Arc::new(vec![0.0f32; 44_100 * 2]);
        p.load(
            data,
            2,
            44_100,
            1.0,
            LoopWindow {
                start: 0.0,
                end: 1.0,
            },
            false,
            false,
        );
        assert!(p.ready(), "piste chargée");
        // le mixeur d'une écoute prend la sortie…
        p.play_preview(rodio::buffer::SamplesBuffer::new(
            2,
            44_100,
            vec![0.0f32; 512],
        ));
        assert!(p.ready(), "play_preview ne consomme pas la piste");
        // …et la piste reprend ensuite, sans être rechargée du disque
        p.resume_at(0.5);
        assert!(p.ready() && !p.is_paused(), "la piste repart après l'écoute");
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
            head: 0,
            shared: shared(0, 500),
            xfade: 10,
            seam_skip: 0,
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
        assert!(p < 500, "position hors fenêtre : {p}");
    }

    /// Aucun trou, aucun échantillon perdu ni rejoué à la couture de deux
    /// segments : la source saute dans le suivant **dans le même appel** de
    /// `next()`, elle ne rend jamais `None` (question user 21/11 : « aucune
    /// latence entre chaque segment ? »).
    #[test]
    fn couture_de_segments_sans_trou() {
        let data: Arc<Vec<f32>> = Arc::new((0..10_000).map(|i| i as f32).collect());
        let sh = Arc::new(Shared::default());
        // segment courant : [0, 1000) ; suivant programmé : [5000, 6000)
        sh.stop.store(1000, Ordering::Relaxed);
        sh.seg_next.store((6000u64 << 32) | 5000, Ordering::Relaxed);
        let mut src = LoopSource {
            data,
            channels: 1,
            rate: 44100,
            pos: 0,
            head: 0,
            shared: sh.clone(),
            xfade: 0,
            seam_skip: 0,
        };
        let got: Vec<f32> = (0..2000)
            .map(|_| src.next().expect("la couture ne doit pas rendre None"))
            .collect();
        let attendu: Vec<f32> = (0..1000).chain(5000..6000).map(|i| i as f32).collect();
        assert_eq!(got, attendu, "échantillons contigus, sans trou ni doublon");
        assert_eq!(sh.jumps.load(Ordering::Relaxed), 1);
        assert_eq!(sh.stop.load(Ordering::Relaxed), 6000);
        // plus rien en file : la source s'arrête proprement, à la fin du segment
        assert!(src.next().is_none());
    }

    /// Bascule de pad programmée : elle tombe à la couture, dans la source, et
    /// pas au prochain rafraîchissement de la GUI.
    #[test]
    fn bascule_de_pad_a_la_couture() {
        let data: Arc<Vec<f32>> = Arc::new((0..2000).map(|i| i as f32).collect());
        let sh = shared(0, 500);
        sh.next_start.store(700, Ordering::Relaxed);
        sh.next_end.store(900, Ordering::Relaxed);
        let mut src = LoopSource {
            data,
            channels: 1,
            rate: 1000,
            pos: 0,
            head: 0,
            shared: sh.clone(),
            xfade: 10,
            seam_skip: 0,
        };
        // plus de trois tours de 500 échantillons : la bascule doit avoir eu lieu
        for _ in 0..1500 {
            assert!(src.next().is_some());
        }
        let p = sh.pos.load(Ordering::Relaxed) as usize;
        assert!((700..900).contains(&p), "position {p} hors du pad suivant");
        assert_eq!(sh.loop_start.load(Ordering::Relaxed), 700);
        assert_eq!(sh.loop_end.load(Ordering::Relaxed), 900);
        assert_eq!(sh.next_end.load(Ordering::Relaxed), 0, "bascule consommée");
        assert!(sh.turns.load(Ordering::Relaxed) >= 2);
    }

    /// Un rendu commence souvent par quelques dizaines de ms de zéros (SPU) :
    /// y revenir ne doit pas les rejouer, sinon la couture « dernière tranche →
    /// première » s'entend comme un trou (vécu 21/11, FF8 « Force Your Way » :
    /// 36 ms de zéros exacts en tête du WAV rendu par zxtune).
    #[test]
    fn retour_au_debut_ne_rejoue_pas_le_blanc() {
        let data: Arc<Vec<f32>> = Arc::new(
            (0..1000)
                .map(|i| if i < 40 { 0.0 } else { i as f32 })
                .collect(),
        );
        assert_eq!(
            crate::decode::head_silence(&data, 1, 1000),
            40,
            "40 zéros de tête"
        );
        // 1) démarrage à 0 puis saut de suite vers le début
        let sh = Arc::new(Shared::default());
        let head = 40; // mono : échantillons = trames
        sh.stop.store(300, Ordering::Relaxed);
        sh.seg_next.store(700u64 << 32, Ordering::Relaxed); // [0, 700)
        let mut src = LoopSource {
            data: data.clone(),
            channels: 1,
            rate: 1000,
            pos: 0,
            head,
            shared: sh.clone(),
            xfade: 0,
            seam_skip: 0,
        };
        let vals: Vec<f32> = (&mut src).take(400).collect();
        assert_eq!(vals[0], 40.0, "démarrage sur la première attaque");
        let i = vals
            .windows(2)
            .position(|w| w[1] < w[0])
            .map(|k| k + 1)
            .expect("saut de suite");
        assert!(i > 1, "le saut vers le début a eu lieu");
        assert_eq!(vals[i], 40.0, "saut vers le début sans blanc");
        assert!(vals.iter().all(|v| *v > 0.0), "aucun zéro rejoué");
        // 2) bouclage depuis 0 avec fondu croisé : le fondu mêle le vrai début
        let sh = shared(0, 300);
        let mut src = LoopSource {
            data,
            channels: 1,
            rate: 1000,
            pos: 0,
            head,
            shared: sh.clone(),
            xfade: 10,
            seam_skip: 0,
        };
        let vals: Vec<f32> = (&mut src).take(700).collect();
        assert!(vals.iter().all(|v| *v > 0.0), "aucun zéro dans la boucle");
        assert!(sh.turns.load(Ordering::Relaxed) >= 2);
    }

    /// Le saut du blanc de tête doit rester **aligné sur les trames** : un
    /// nombre impair de trames de zéros donnerait une position impaire en
    /// stéréo, donc les canaux inversés — « une fois plus à gauche, une fois
    /// plus à droite » (vécu 21/11).
    #[test]
    fn le_blanc_de_tete_ne_decale_pas_les_canaux() {
        // 3 trames de zéros (impair) puis L = +1, R = -1
        let mut data = vec![0.0f32; 6];
        for _ in 0..10 {
            data.push(1.0);
            data.push(-1.0);
        }
        let data = Arc::new(data);
        let samples = crate::decode::head_silence(&data, 2, 1000);
        assert_eq!(samples, 6, "3 trames de blanc × 2 canaux");
        let sh = Arc::new(Shared::default());
        let mut src = LoopSource {
            data,
            channels: 2,
            rate: 1000,
            pos: 0,
            head: samples, // échantillons entrelacés
            shared: sh,
            xfade: 0,
            seam_skip: 0,
        };
        let vals: Vec<f32> = (&mut src).take(4).collect();
        assert_eq!(vals, [1.0, -1.0, 1.0, -1.0], "canaux alignés après le saut");
    }

    /// Fenêtre de boucle plus courte que le blanc de tête : on ne doit pas
    /// sortir de la fenêtre (sinon la source se replierait sur elle-même).
    #[test]
    fn fenetre_plus_courte_que_le_blanc() {
        let data: Arc<Vec<f32>> = Arc::new(
            (0..200)
                .map(|i| if i < 40 { 0.0 } else { i as f32 })
                .collect(),
        );
        let sh = shared(0, 20);
        let mut src = LoopSource {
            data,
            channels: 1,
            rate: 1000,
            pos: 0,
            head: 40,
            shared: sh.clone(),
            xfade: 10,
            seam_skip: 0,
        };
        let vals: Vec<f32> = (&mut src).take(50).collect();
        assert_eq!(vals.len(), 50, "la source ne doit pas s'arrêter");
        let p = sh.pos.load(Ordering::Relaxed);
        assert!(p <= 20, "position hors fenêtre : {p}");
    }

    /// Le raccord de deux tranches est fondu sur 2 ms : une marche d'échantillon
    /// (fin de tranche + attaque suivante) s'entend comme un grésillement à
    /// chaque enchaînement (vécu 26/11). Le fondu ne doit ni perdre ni rejouer
    /// d'échantillon du segment suivant.
    #[test]
    fn couture_de_segments_fondue_sans_clic() {
        // sinus de période 64 : aux indices 1000 et 5000 les phases diffèrent,
        // donc la couture nue ferait une marche d'environ 1,3
        let data: Arc<Vec<f32>> = Arc::new(
            (0..10_000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 64.0).sin())
                .collect(),
        );
        let sh = Arc::new(Shared::default());
        sh.stop.store(1000, Ordering::Relaxed);
        sh.seg_next.store((6000u64 << 32) | 5000, Ordering::Relaxed);
        let mut src = LoopSource {
            data: data.clone(),
            channels: 1,
            rate: 1000,
            pos: 0,
            head: 0,
            shared: sh.clone(),
            xfade: 16,
            seam_skip: 0,
        };
        // le fondu **remplace** les 16 derniers échantillons de la tranche par
        // le mélange (il n'ajoute rien) : la suite perd 16 échantillons, soit
        // 2 ms par couture — inaudible, et aucun échantillon du segment suivant
        // n'est perdu ni rejoué
        let got: Vec<f32> = (0..1984).map(|_| src.next().expect("pas de trou")).collect();
        assert_eq!(got.len(), 1984);
        let max_jump = got
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_jump < 0.35, "saut de couture {max_jump} (clic)");
        // le segment suivant est rejoué en entier, sans doublon : ses 16
        // premiers échantillons passent sous le fondu, puis on reprend à 5016
        assert!((got[1000] - data[5016]).abs() < 1e-6, "reprise après le fondu");
        assert!((got[1983] - data[5999]).abs() < 1e-6, "fin du segment suivant");
        assert_eq!(sh.jumps.load(Ordering::Relaxed), 1);
        // sans suivant programmé, aucun fondu ne retarde la fin
        let sh2 = Arc::new(Shared::default());
        sh2.stop.store(300, Ordering::Relaxed);
        let mut seul = LoopSource {
            data: data.clone(),
            channels: 1,
            rate: 1000,
            pos: 0,
            head: 0,
            shared: sh2,
            xfade: 16,
            seam_skip: 0,
        };
        let n = (&mut seul).count();
        assert_eq!(n, 300, "la source s'arrête au stop, sans fondu");
    }

    /// Suite : la source saute au segment programmé sans rendre `None`, donc
    /// aucun trou audible entre deux tranches (demande user, 21/11).
    #[test]
    fn suite_sans_trou() {
        let data: Arc<Vec<f32>> = Arc::new((0..1000).map(|i| i as f32).collect());
        let sh = Arc::new(Shared::default());
        sh.stop.store(300, Ordering::Relaxed);
        sh.seg_next.store((700u64 << 32) | 500, Ordering::Relaxed);
        let mut src = LoopSource {
            data,
            channels: 1,
            rate: 1000,
            pos: 0,
            head: 0,
            shared: sh.clone(),
            xfade: 0,
            seam_skip: 0,
        };
        let vals: Vec<f32> = (&mut src).take(500).collect();
        assert_eq!(
            vals.len(),
            500,
            "la source ne doit pas s'arrêter au segment"
        );
        assert_eq!(vals[299], 299.0);
        assert_eq!(vals[300], 500.0, "saut direct au segment suivant");
        assert_eq!(vals[499], 699.0);
        assert_eq!(sh.jumps.load(Ordering::Relaxed), 1);
        assert_eq!(sh.seg_next.load(Ordering::Relaxed), 0, "queue consommée");
        assert_eq!(sh.stop.load(Ordering::Relaxed), 700);
        // plus rien de programmé : la source s'arrête à la fin du segment
        assert!((&mut src).next().is_none());
    }
}
