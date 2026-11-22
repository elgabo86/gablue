//! GTracker : vue multitrack (transport, pistes, timeline, éditeur de source).
//!
//! Le moteur vit dans `gtracker_core` (mixer, projet, stretch, key) ; ici,
//! uniquement l'état d'interface et les gestes. Contrat :
//! `crates/gui/src/gtracker/AGENTS.md`.

mod edit;
mod wave;

pub use edit::History;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Vec2,
};
use gtracker_core::convert::Format;
use gtracker_core::decode::Decoded;
use gtracker_core::key::{self, Key};
use gtracker_core::mixer::{Plan, SourceAudio, Transport};
use gtracker_core::model::Track as AudioTrack;
use gtracker_core::peaks::Wave;
use gtracker_core::player::Player;
use gtracker_core::project::{self, project_path, Clip, Id, Project, Role, Source, StretchMode};
use gtracker_core::tempo::Tempo;

use wave::{WaveCache, WaveKey};

/// Fréquence du plan : rodio rééchantillonne vers le périphérique.
const OUT_RATE: u32 = 48_000;
/// Hauteur d'une lane et de la règle (pixels).
const LANE_H: f32 = 64.0;
const RULER_H: f32 = 24.0;
/// Largeur de la colonne des pistes.
const HEADER_W: f32 = 196.0;

const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x5b, 0x9d, 0xf5),
    Color32::from_rgb(0x6f, 0xcf, 0x97),
    Color32::from_rgb(0xf2, 0xb1, 0x4b),
    Color32::from_rgb(0xd9, 0x77, 0xb0),
    Color32::from_rgb(0x8a, 0x8f, 0xf0),
    Color32::from_rgb(0x5f, 0xc8, 0xd8),
    Color32::from_rgb(0xd8, 0x6b, 0x5f),
    Color32::from_rgb(0xb8, 0xa6, 0x6a),
];

fn tint(i: u8) -> Color32 {
    PALETTE[i as usize % PALETTE.len()]
}

/// Demande faite à l'application (fil de travail, disque).
pub enum Cmd {
    /// Décoder une piste et l'ajouter au projet (index de source à la retour).
    Load(Box<AudioTrack>),
    /// Écouter une tranche une fois, sans la poser sur la timeline.
    Preview {
        source: usize,
        src_start: usize,
        src_end: usize,
        label: usize,
    },
    /// Rendre et encoder le projet.
    Export {
        project: Box<Project>,
        audio: Vec<Option<SourceAudio>>,
        format: Format,
        name: String,
    },
    /// Ouvrir un projet du disque et recharger ses sources.
    Open(PathBuf),
    /// Boucle d'une source corrigée depuis la pile de l'arrangement : la
    /// coquille la mémorise en base et recale l'éditeur si c'est la même piste.
    SourceLoop { source: usize, start: f64, end: f64 },
}

#[derive(Clone)]
enum Drag {
    /// Corps : `at_beat` d'origine.
    Move { id: Id, from: f64, grab: f64 },
    /// Bord droit : longueur d'origine.
    Right { id: Id, len: f64 },
    /// Bord gauche : `at_beat` et longueur d'origine.
    Left { id: Id, at: f64, len: f64 },
    /// Tranche tirée de l'éditeur vers une lane.
    Slice {
        source: usize,
        src_start: usize,
        src_end: usize,
        length_beats: f64,
    },
}

/// Tuile de l'éditeur de source : rectangle affiché et plage de contenu.
/// Sert au test du glisser (le rendu, lui, passe par les réponses egui).
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Tile {
    pub rect: Rect,
    pub source: usize,
    pub src_start: usize,
    pub src_end: usize,
    pub length_beats: f64,
}

/// Nouveau décalage horizontal pour garder la tête `b` dans la fenêtre
/// `[scroll, scroll + vis]` : `None` si elle y est déjà (la vue ne bouge pas
/// tant qu'on est dedans), sinon on la replace à 35 % de la largeur — un saut
/// de page, pas un défilement continu qui scintille.
fn follow_scroll(b: f64, scroll: f64, vis: f64) -> Option<f64> {
    if b > scroll + vis * 0.85 || b < scroll + vis * 0.05 {
        let target = (b - vis * 0.35).max(0.0);
        // au tout début du projet la cible vaut déjà le décalage courant :
        // inutile de réécrire la vue à chaque image
        if (target - scroll).abs() > 1e-9 {
            return Some(target);
        }
    }
    None
}

/// Bouton de transport avec l'icône **dessinée** (triangle / carré) : les
/// polices embarquées n'ont ni ▶ ni ■ (vécu 21/11 : des carrés vides).
fn transport_icon_button(ui: &mut egui::Ui, playing: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(106.0, 26.0), Sense::click());
    let v = ui.visuals().clone();
    let bg = if resp.hovered() {
        v.widgets.hovered.bg_fill
    } else if playing {
        crate::theme::ACCENT_SOFT
    } else {
        v.widgets.inactive.bg_fill
    };
    let stroke = if playing {
        Stroke::new(1.0, crate::theme::ACCENT)
    } else {
        Stroke::new(1.0, crate::theme::LINE)
    };
    ui.painter()
        .rect(rect, CornerRadius::same(6), bg, stroke, StrokeKind::Inside);
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + 18.0, rect.center().y),
        Vec2::splat(11.0),
    );
    if playing {
        ui.painter()
            .rect_filled(icon.shrink(1.0), CornerRadius::same(2), crate::theme::TEXT);
    } else {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                icon.left_top() + Vec2::new(1.5, 0.0),
                egui::pos2(icon.right() + 1.5, icon.center().y),
                icon.left_bottom() + Vec2::new(1.5, 0.0),
            ],
            crate::theme::TEXT,
            Stroke::NONE,
        ));
    }
    ui.painter().text(
        egui::pos2(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        if playing { "Arrêter" } else { "Écouter" },
        FontId::proportional(13.0),
        crate::theme::TEXT,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub struct Gtracker {
    pub project: Project,
    pub path: Option<PathBuf>,
    audio: Vec<Option<Arc<Decoded>>>,
    waves: Vec<Option<Arc<Wave>>>,
    tempos: Vec<Option<Tempo>>,
    keys: Vec<Option<Key>>,

    sel_source: usize,
    sel_track: Option<Id>,
    sel_clip: Option<Id>,

    transport: Option<Transport>,
    /// Tête de lecture quand aucun transport n'existe encore (clic sur la
    /// règle avant la première lecture) : sans ça, « écouter » repartait de 0.
    pending_seek: f64,
    pub playing: bool,
    loop_on: bool,

    zoom: f32,
    scroll: f64,
    /// La timeline recadre-t-elle sur la tête pendant la lecture ? Tombé quand on
    /// zoome ou qu'on défile soi-même, relevé à chaque « écouter ».
    follow: bool,
    snap: f64,
    clip_beats: f64,

    hist: History,
    drag: Option<Drag>,
    /// Bord de boucle tenu dans la pile de sources : (source, bord gauche).
    src_grab: Option<(usize, bool)>,
    /// Export en FLAC (sinon MP3 VBR).
    export_flac: bool,
    tex: WaveCache,
    timeline_rect: Rect,
    /// Rectangles des tuiles de l'éditeur (dernière image) : sert au test du
    /// glisser tranche → piste, sans dépendre d'une capture d'écran.
    tiles: Vec<Tile>,
    pub status: String,
    dirty: bool,
}

impl Default for Gtracker {
    fn default() -> Self {
        let mut project = Project::new("projet");
        project.tempo_bpm = 120.0;
        Self {
            project,
            path: None,
            audio: Vec::new(),
            waves: Vec::new(),
            tempos: Vec::new(),
            keys: Vec::new(),
            sel_source: 0,
            sel_track: None,
            sel_clip: None,
            transport: None,
            pending_seek: 0.0,
            playing: false,
            loop_on: false,
            zoom: 48.0,
            scroll: 0.0,
            follow: true,
            snap: 0.25,
            // une mesure : même défaut que l'éditeur (demande user 22/11)
            clip_beats: 4.0,
            hist: History::default(),
            drag: None,
            src_grab: None,
            export_flac: false,
            tex: WaveCache::default(),
            timeline_rect: Rect::NOTHING,
            tiles: Vec::new(),
            status: "Ajoute une piste (bouton en haut à gauche) pour commencer.".into(),
            dirty: false,
        }
    }
}

impl Gtracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute (ou retrouve) une source et son PCM. Rend l'index de source.
    ///
    /// `make_track` : l'utilisateur a demandé cette source explicitement (dépôt
    /// ou « ajouter au projet ») → on lui donne une piste, prête à recevoir des
    /// tranches. Un rechargement de projet ne crée rien (les pistes existent).
    pub fn add_audio(&mut self, track: &AudioTrack, decoded: Decoded, make_track: bool) -> usize {
        // la boucle mesurée (tag `.spc`, en-tête GENH/VGM, `.m3u` recalé) part
        // avec la source : l'arrangement et l'éditeur découpent au même endroit
        let window = decoded.window;
        let src = Source {
            id: self.project.next_id(),
            title: track.title.clone(),
            site: "fichiers".into(),
            entry_id: -1,
            file: track.file.clone(),
            subsong: track.subsong,
            bpm: 0.0,
            phase: 0.0,
            key: None,
            loop_start: window.map(|(a, _)| a),
            loop_end: window.map(|(_, b)| b),
        };
        let idx = self.project.add_source(src);
        while self.audio.len() <= idx {
            self.audio.push(None);
            self.waves.push(None);
            self.tempos.push(None);
            self.keys.push(None);
        }
        {
            let src = &self.project.sources[idx];
            gtracker_core::decode::pin(std::path::Path::new(&src.file), src.subsong as usize);
        }
        if self.audio[idx].is_none() {
            self.waves[idx] = Some(Arc::new(Wave::build(
                &decoded.pcm,
                decoded.channels,
                decoded.rate,
            )));
            self.audio[idx] = Some(Arc::new(decoded));
        }
        // une piste est créée si le projet n'en a aucune, ou si l'utilisateur a
        // ajouté explicitement cette source et qu'aucune piste ne la porte
        let carries = self.project.tracks.iter().any(|t| t.source == Some(idx));
        if self.project.tracks.is_empty() || (make_track && !carries) {
            let color = (self.project.tracks.len() % PALETTE.len()) as u8;
            let id = self.project.add_track(&track.title, Some(idx), color);
            self.sel_track = Some(id);
        }
        self.sel_source = idx;
        self.audio[idx]
            .as_ref()
            .map(|d| d.total)
            .unwrap_or_default();
        idx
    }

    /// Analyse du rythme et de la tonalité arrivée du fil de travail.
    pub fn set_analysis(&mut self, file: &std::path::Path, bpm: Option<f64>, key: Option<Key>) {
        for (i, s) in self.project.sources.iter_mut().enumerate() {
            if s.file == file {
                if let Some(b) = bpm {
                    s.bpm = b;
                    if i < self.tempos.len() && self.tempos[i].is_none() {
                        let phase = 0.0;
                        self.tempos[i] = Some(Tempo {
                            bpm: b,
                            beat: 60.0 / b,
                            phase,
                            confidence: 0.5,
                        });
                    }
                }
                if key.is_some() {
                    s.key = key;
                }
                if i < self.keys.len() {
                    self.keys[i] = key;
                }
            }
        }
        // seule la source de la piste **maîtresse** pose le tempo du projet ;
        // analyser une autre source ne doit pas déplacer la grille de tous
        if let Some(b) = bpm {
            if self.is_master_source(file) {
                self.project.tempo_bpm = b.clamp(20.0, 400.0);
            }
        }
    }

    fn is_master_source(&self, file: &std::path::Path) -> bool {
        self.project
            .master
            .and_then(|id| self.project.track(id))
            .and_then(|t| t.source)
            .and_then(|i| self.project.sources.get(i))
            .map(|s| s.file == file)
            .unwrap_or(false)
    }

    pub fn set_tempo_full(&mut self, file: &std::path::Path, tempo: Tempo, key: Option<Key>) {
        for (i, s) in self.project.sources.iter_mut().enumerate() {
            if s.file == file {
                s.bpm = tempo.bpm;
                s.phase = tempo.phase;
                if i < self.tempos.len() {
                    self.tempos[i] = Some(tempo);
                }
                if let Some(k) = key {
                    s.key = Some(k);
                    if i < self.keys.len() {
                        self.keys[i] = Some(k);
                    }
                }
            }
        }
        if self.is_master_source(file) {
            self.project.tempo_bpm = tempo.bpm.clamp(20.0, 400.0);
        }
    }

    fn checkpoint(&mut self, label: &str) {
        self.hist.checkpoint(label, &self.project);
        self.dirty = true;
    }

    fn beat_sec(&self) -> f64 {
        60.0 / self.project.tempo_bpm.clamp(20.0, 400.0)
    }

    fn frames_per_beat(&self) -> f64 {
        self.beat_sec() * OUT_RATE as f64
    }

    /// PCM de toutes les sources, indexés comme `Project.sources` : c'est ce
    /// que le mixer attend, pour le projet comme pour une écoute isolée.
    fn source_audio(&self) -> Vec<Option<SourceAudio>> {
        self.audio
            .iter()
            .map(|a| {
                a.as_ref()
                    .map(|d| SourceAudio::new(d.pcm.clone(), d.channels, d.rate))
            })
            .collect()
    }

    fn plan(&self) -> Plan {
        Plan::build(&self.project, &self.source_audio(), OUT_RATE)
    }

    /// Plan d'**écoute** d'une tranche seule, sans toucher au projet : un clip
    /// de la longueur réelle de la tranche, à l'instant 0, sur une piste qui
    /// porte cette source. C'est ce que joue un clic sur une tuile (Fruity
    /// Loops : on essaie une tranche avant de la poser).
    fn audition_plan(&self, source: usize, src_start: usize, src_end: usize) -> Option<Plan> {
        let rate = self
            .audio
            .get(source)
            .and_then(|a| a.as_ref())
            .map(|d| d.rate)
            .unwrap_or(44_100);
        let secs = src_end.saturating_sub(src_start) as f64 / rate as f64;
        if secs <= 0.0 {
            return None;
        }
        let mut p = self.project.clone();
        p.clips.clear();
        // l'écoute d'une tranche n'a ni métronome ni boucle de projet
        p.metronome = false;
        p.loop_beats = None;
        let track_id = p
            .tracks
            .iter()
            .find(|t| t.source == Some(source))
            .or_else(|| p.tracks.first())
            .map(|t| t.id)?;
        let beats = (secs / self.beat_sec()).max(0.01);
        p.add_clip(edit::clip_from_slice(
            track_id, source, src_start, src_end, 0.0, 0.0, beats,
        ));
        let mut plan = Plan::build(&p, &self.source_audio(), OUT_RATE);
        // `len_beats` impose un minimum de 4 temps au projet : l'écoute, elle,
        // s'arrête à la fin de la tranche (pas de silence après)
        if let Some(v) = plan.voices.first() {
            plan.frames = v.end;
        }
        (!plan.is_empty()).then_some(plan)
    }

    /// Joue la tranche une fois (clic sur une tuile de l'éditeur). Le projet se
    /// tait mais n'est pas modifié ; l'écoute passe par le même périphérique,
    /// sans consommer la piste de la bibliothèque.
    pub fn preview(
        &mut self,
        player: &mut Option<Player>,
        source: usize,
        src_start: usize,
        src_end: usize,
        label: usize,
    ) -> bool {
        let Some(plan) = self.audition_plan(source, src_start, src_end) else {
            return false;
        };
        // le projet se tait (l'écoute prend le son), mais son transport garde
        // sa position : « lecture » reprendra où on en était
        self.playing = false;
        if let Some(p) = player {
            p.play_preview(Transport::new(plan).source());
        }
        self.status = format!("écoute · tranche {label} — une fois");
        true
    }

    fn replan(&mut self) {
        if let Some(t) = &self.transport {
            t.set_plan(self.plan());
        }
    }

    // ── transport ───────────────────────────────────────────────────────────

    fn play(&mut self, player: &mut Option<Player>) {
        let plan = self.plan();
        if plan.is_empty() {
            self.status = "Rien à jouer : dépose une tranche sur une piste.".into();
            return;
        }
        // une nouvelle écoute réarme le suivi de la tête
        self.follow = true;
        // reprise : la lecture repart de la tête de lecture, sauf si elle est
        // déjà à la fin du projet (sinon « écouter » ne jouerait rien)
        let resume = self
            .transport
            .as_ref()
            .map(|t| t.position())
            .filter(|p| *p < plan.frames)
            .unwrap_or_else(|| {
                (self.pending_seek.max(0.0) * self.frames_per_beat()).round() as u64
            });
        self.pending_seek = 0.0;
        let t = Transport::new(plan);
        if resume > 0 {
            t.seek(resume);
        }
        if let Some(p) = player {
            p.play_source(t.source());
        }
        self.transport = Some(t);
        self.playing = true;
        self.status = "Lecture".into();
    }

    fn stop(&mut self, player: &mut Option<Player>) {
        if let Some(p) = player {
            p.stop_source();
        }
        // pause : la position reste où elle est (espace = lecture/pause) ;
        // seul « début » ramène à zéro
        self.playing = false;
    }

    pub fn position_beats(&self) -> f64 {
        self.transport
            .as_ref()
            .map(|t| t.position() as f64 / self.frames_per_beat())
            .unwrap_or(self.pending_seek)
    }

    pub fn seek_beats(&mut self, beat: f64) {
        let beat = beat.max(0.0);
        match &self.transport {
            Some(t) => t.seek((beat * self.frames_per_beat()).round() as u64),
            None => self.pending_seek = beat,
        }
    }

    /// Coupe la lecture du GTracker (la bibliothèque prend le son : un seul
    /// propriétaire à la fois, mais le projet reste intact).
    pub fn halt(&mut self) {
        self.playing = false;
        self.transport = None;
        self.pending_seek = 0.0;
    }

    /// Clip sous la tête de lecture **pendant la lecture** : sans lui, on
    /// entend sans voir d'où vient le son (retour user 21/11).
    pub fn sounding_clip(&self) -> Option<&Clip> {
        if !self.playing {
            return None;
        }
        let b = self.position_beats();
        self.project
            .clips
            .iter()
            .find(|c| b >= c.at_beat && b < c.at_beat + c.length_beats)
    }

    /// Coupe le projet sans toucher à la tête de lecture : la coquille s'en
    /// sert pour l'arrêt global (bouton de l'inspecteur). L'audio lui-même est
    /// mis en pause par le `Player`, ici on remet juste l'état à l'arrêt.
    pub fn silence(&mut self) {
        self.playing = false;
    }

    // ── interface ───────────────────────────────────────────────────────────

    /// Composition complète (barre d'état, transport, éditeur compact) :
    /// utilisée par les tests du module. La coquille compose les mêmes pièces
    /// dans ses propres panneaux.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        player: &mut Option<Player>,
        cmds: &mut Vec<Cmd>,
    ) {
        egui::Panel::bottom("gt_status")
            .exact_size(22.0)
            .show(ui, |ui| self.status_bar(ui));
        self.transport_bar(ui, player);
        ui.separator();
        egui::Panel::bottom("gt_source")
            .resizable(true)
            .default_size(210.0)
            .min_size(140.0)
            .show(ui, |ui| self.source_panel(ui, cmds));
        self.ui_center(ui, player);
    }

    /// Cœur de l'arrangement : rail des pistes + timeline. La coquille y ajoute
    /// son transport, sa barre d'état et son éditeur compact.
    pub fn ui_center(&mut self, ui: &mut egui::Ui, player: &mut Option<Player>) {
        self.shortcuts(ui, player);
        egui::Panel::left("gt_tracks")
            .exact_size(HEADER_W)
            .resizable(false)
            .show(ui, |ui| self.tracks_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.timeline(ui, player));

        // fin de lecture : la source audio a consommé le plan
        if self.playing {
            if let Some(t) = &self.transport {
                let plan = t.plan();
                if plan.loop_region.is_none() && t.position() >= plan.frames {
                    self.playing = false;
                    if let Some(p) = player {
                        p.stop_source();
                    }
                }
            }
        }
        if self.playing {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(60));
        }
    }

    /// Transport du projet, exposé à la coquille pour qu'il soit **le** transport
    /// en bas de la fenêtre (et non un second, caché dans le centre).
    pub fn transport_bar(&mut self, ui: &mut egui::Ui, player: &mut Option<Player>) {
        self.transport_bar_impl(ui, player)
    }

    /// Barre d'état : position musicale (une seule source de vérité, le
    /// transport), sélection, message.
    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        let beat = self.position_beats();
        let bar = (beat / 4.0).floor() + 1.0;
        let in_bar = beat - (bar - 1.0) * 4.0;
        let total = self.project.len_beats();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "mesure {bar:.0} · temps {:.2}   /   {:.1} mesures",
                    in_bar + 1.0,
                    (total / 4.0).max(1.0)
                ))
                .size(10.5)
                .monospace(),
            );
            ui.separator();
            let sel = self
                .sel_clip
                .and_then(|id| {
                    self.project.clip(id).map(|c| {
                        let name = self
                            .project
                            .sources
                            .get(c.source)
                            .map(|s| s.title.clone())
                            .unwrap_or_else(|| format!("source {}", c.source));
                        format!(
                            "{name} · {:.2} temps · {} voix",
                            c.length_beats,
                            self.project.clips.len()
                        )
                    })
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} pistes · {} clips",
                        self.project.tracks.len(),
                        self.project.clips.len()
                    )
                });
            ui.label(egui::RichText::new(sel).size(10.5));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&self.status)
                        .size(10.5)
                        .color(ui.visuals().weak_text_color()),
                );
            });
        });
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui, player: &mut Option<Player>) {
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if typing {
            return;
        }
        let (space, del, ctrl_z, ctrl_y, ctrl_d, home, cut) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.modifiers.command && i.key_pressed(egui::Key::Z),
                i.modifiers.command && i.key_pressed(egui::Key::Y),
                i.modifiers.command && i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::S) && !i.modifiers.command,
            )
        });
        if space {
            if self.playing {
                self.stop(player);
            } else {
                self.play(player);
            }
        }
        if home {
            self.stop(player);
            self.scroll = 0.0;
        }
        if ctrl_z {
            if let Some(label) = self.hist.undo(&mut self.project) {
                self.status = format!("annulé : {label}");
                self.dirty = true;
                self.replan();
            }
        }
        if ctrl_y {
            if let Some(label) = self.hist.redo(&mut self.project) {
                self.status = format!("refait : {label}");
                self.dirty = true;
                self.replan();
            }
        }
        if del {
            if let Some(id) = self.sel_clip {
                self.checkpoint("supprimer");
                self.project.remove_clip(id);
                self.sel_clip = None;
                self.replan();
            }
        }
        if ctrl_d {
            if let Some(id) = self.sel_clip {
                self.checkpoint("dupliquer");
                if let Some(new) = edit::duplicate(&mut self.project, id) {
                    self.sel_clip = Some(new);
                }
                self.replan();
            }
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save();
        }
        // mute de la piste sélectionnée
        if ui.input(|i| !i.modifiers.command && i.key_pressed(egui::Key::M)) {
            if let Some(id) = self.sel_track {
                self.checkpoint("mute");
                if let Some(t) = self.project.track_mut(id) {
                    t.mute = !t.mute;
                }
                self.replan();
            }
        }
        // 1–9 : sélection de piste
        for n in 1..=9usize {
            let key = match n {
                1 => egui::Key::Num1,
                2 => egui::Key::Num2,
                3 => egui::Key::Num3,
                4 => egui::Key::Num4,
                5 => egui::Key::Num5,
                6 => egui::Key::Num6,
                7 => egui::Key::Num7,
                8 => egui::Key::Num8,
                _ => egui::Key::Num9,
            };
            if ui.input(|i| !i.modifiers.command && i.key_pressed(key)) {
                if let Some(t) = self.project.tracks.get(n - 1) {
                    self.sel_track = Some(t.id);
                    if let Some(src) = t.source {
                        self.sel_source = src;
                    }
                }
            }
        }
        // flèches : déplacer le clip sélectionné d'un pas de grille
        // (maj = une mesure) ; haut/bas = changer de piste
        if let Some(id) = self.sel_clip {
            let (left, right, up, down, shift) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.modifiers.shift,
                )
            });
            if left || right || up || down {
                let step = if shift { 4.0 } else { self.snap };
                self.checkpoint("déplacer au clavier");
                let tracks: Vec<Id> = self.project.tracks.iter().map(|t| t.id).collect();
                if let Some(c) = self.project.clip_mut(id) {
                    if left {
                        c.at_beat = (c.at_beat - step).max(0.0);
                    }
                    if right {
                        c.at_beat += step;
                    }
                    if up || down {
                        if let Some(i) = tracks.iter().position(|t| *t == c.track) {
                            let j = if up {
                                i.saturating_sub(1)
                            } else {
                                (i + 1).min(tracks.len().saturating_sub(1))
                            };
                            c.track = tracks[j];
                            self.sel_track = Some(tracks[j]);
                        }
                    }
                }
                self.replan();
                self.dirty = true;
            }
        }
        if cut {
            if let (Some(id), Some(track)) = (self.sel_clip, self.sel_track) {
                let beat = self.position_beats();
                self.checkpoint("couper");
                if edit::split_at(&mut self.project, id, beat).is_some() {
                    self.status = "clip coupé".into();
                }
                let _ = track;
                self.replan();
            }
        }
    }

    fn transport_bar_impl(&mut self, ui: &mut egui::Ui, player: &mut Option<Player>) {
        ui.horizontal(|ui| {
            if transport_icon_button(ui, self.playing).clicked() {
                if self.playing {
                    self.stop(player);
                } else {
                    self.play(player);
                }
            }
            if ui
                .add(egui::Button::new("Début").min_size(Vec2::new(56.0, 26.0)))
                .on_hover_text("Revenir au début")
                .clicked()
            {
                self.stop(player);
                self.seek_beats(0.0);
                self.scroll = 0.0;
            }

            ui.separator();
            // BPM : glisser vertical ±0,1 BPM/px, molette ±0,5
            let mut bpm = self.project.tempo_bpm;
            let badge = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{:.1} BPM", bpm))
                        .strong()
                        .size(14.0),
                )
                .fill(ui.visuals().widgets.inactive.bg_fill),
            );
            let dy = ui.input(|i| i.pointer.delta().y);
            if badge.dragged() && dy != 0.0 {
                let step = if ui.input(|i| i.modifiers.shift) { 0.02 } else { 0.1 };
                bpm = (bpm - dy as f64 * step).clamp(20.0, 400.0);
                self.project.tempo_bpm = bpm;
                self.dirty = true;
                self.replan();
            }
            if badge.hovered() {
                let wheel = ui.input(|i| i.smooth_scroll_delta.y);
                if wheel != 0.0 {
                    let step = if ui.input(|i| i.modifiers.alt) {
                        bpm * 0.02
                    } else {
                        0.5
                    };
                    self.project.tempo_bpm =
                        (self.project.tempo_bpm + wheel as f64 * step / 50.0).clamp(20.0, 400.0);
                    self.dirty = true;
                    self.replan();
                }
                badge.clone().on_hover_text(
                    "Tempo du projet.\nGlisser vertical : ±0,1 BPM/px (maj ×0,2), molette : ±0,5.\nChoisir une autre piste maîtresse repose ce tempo.",
                );
            }

            // pas de découpe (pour les tuiles de l'éditeur)
            ui.separator();
            ui.label("segment");
            for (b, name) in [(1.0, "1 temps"), (2.0, "2 temps"), (4.0, "1 mesure"), (8.0, "2 mes.")] {
                if ui
                    .selectable_label((self.clip_beats - b).abs() < 1e-9, name)
                    .clicked()
                {
                    self.clip_beats = b;
                    self.dirty = true;
                }
            }

            ui.separator();
            let mut looped = self.loop_on;
            if ui.checkbox(&mut looped, "boucle").changed() {
                self.loop_on = looped;
                self.project.loop_beats = if looped {
                    let end = self.project.len_beats();
                    Some((0.0, (end / 4.0).ceil() * 4.0))
                } else {
                    None
                };
                self.dirty = true;
                self.replan();
            }
            let mut metro = self.project.metronome;
            if ui
                .checkbox(&mut metro, "métronome")
                .on_hover_text("clics générés par le moteur ; jamais dans l'export")
                .changed()
            {
                self.project.metronome = metro;
                self.dirty = true;
                self.replan();
            }

            ui.separator();
            let mut vol = self.project.master_gain;
            if ui
                .add(
                    egui::Slider::new(&mut vol, 0.0..=1.5)
                        .show_value(false)
                        .text("vol"),
                )
                .on_hover_text(format!("gain général : {:.0} %", vol * 100.0))
                .changed()
            {
                self.project.master_gain = vol;
                self.dirty = true;
                self.replan();
            }

            ui.separator();
            if ui.button("Nouvelle piste").clicked() {
                self.checkpoint("nouvelle piste");
                let color = (self.project.tracks.len() % PALETTE.len()) as u8;
                let name = format!("piste {}", self.project.tracks.len() + 1);
                let src = if self.project.sources.is_empty() {
                    None
                } else {
                    Some(self.sel_source)
                };
                let id = self.project.add_track(&name, src, color);
                self.sel_track = Some(id);
                self.replan();
            }
            ui.separator();

            // position musicale : le repère qu'on cherche des yeux en premier.
            // Dans une fenêtre étroite, elle passait sous les boutons.
            if ui.available_width() < 160.0 {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let beats = self.position_beats();
                let total = self.project.len_beats();
                let secs = |b: f64| b * self.beat_sec();
                ui.label(
                    egui::RichText::new(format!(
                        "{} / {}",
                        gtracker_core::util::fmt_time(secs(beats)),
                        gtracker_core::util::fmt_time(secs(total))
                    ))
                    .monospace()
                    .size(12.5),
                );
                let bar = (beats / 4.0).floor() as i64 + 1;
                let beat = (beats % 4.0).floor() as i64 + 1;
                ui.label(
                    egui::RichText::new(format!("mesure {bar} · temps {beat}"))
                        .size(10.5)
                        .color(crate::theme::FAINT),
                );
            });
        });
    }

    /// Nouveau projet vide : les sources décodées restent en mémoire, mais le
    /// projet ne les référence plus (les indices changeraient sinon).
    fn new_project(&mut self) {
        self.project = Project::new("projet");
        self.project.tempo_bpm = 120.0;
        self.audio.clear();
        self.waves.clear();
        self.tempos.clear();
        self.keys.clear();
        self.path = None;
        self.sel_clip = None;
        self.sel_track = None;
        self.sel_source = 0;
        self.transport = None;
        self.pending_seek = 0.0;
        self.playing = false;
        self.tex.clear();
        gtracker_core::decode::unpin_all();
        self.hist = History::default();
        self.loop_on = false;
        self.dirty = false;
        self.status = "nouveau projet".into();
    }

    fn save(&mut self) {
        let path = self
            .path
            .clone()
            .unwrap_or_else(|| project_path(&self.project.name));
        match self.project.save(&path) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.status = format!("projet enregistré : {}", path.display());
            }
            Err(e) => self.status = format!("échec de l'enregistrement : {e}"),
        }
    }

    fn request_export(&mut self, cmds: &mut Vec<Cmd>) {
        let audio: Vec<Option<SourceAudio>> = self
            .audio
            .iter()
            .map(|a| {
                a.as_ref()
                    .map(|d| SourceAudio::new(d.pcm.clone(), d.channels, d.rate))
            })
            .collect();
        cmds.push(Cmd::Export {
            project: Box::new(self.project.clone()),
            audio,
            format: if self.export_flac {
                Format::Flac
            } else {
                Format::Mp3Vbr
            },
            name: if self.project.name.is_empty() {
                "montage".into()
            } else {
                self.project.name.clone()
            },
        });
        self.status = "export en cours…".into();
    }

    /// Menu **document** de la coquille : nom, nouveau, enregistrer, ouvrir,
    /// exporter. Il vit dans la barre haute, pas dans le transport : changer
    /// d'espace de travail ne doit jamais cacher les actions du projet.
    pub fn project_menu(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        ui.menu_button(
            egui::RichText::new(format!("projet · {}", self.project.name)).size(12.5),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("nom");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.project.name)
                                .desired_width(180.0),
                        )
                        .changed()
                    {
                        self.dirty = true;
                    }
                });
                ui.separator();
                if ui.button("nouveau").clicked() {
                    self.new_project();
                    ui.close();
                }
                if ui.button("enregistrer  (ctrl+S)").clicked() {
                    self.save();
                    ui.close();
                }
                let projects = project::projects_in(&self.project_dir());
                if !projects.is_empty() {
                    ui.menu_button(format!("ouvrir ({})", projects.len()), |ui| {
                        for (name, path) in projects.iter().take(24) {
                            let here = self.path.as_deref() == Some(path.as_path());
                            if ui
                                .selectable_label(here, name)
                                .on_hover_text(path.display().to_string())
                                .clicked()
                            {
                                cmds.push(Cmd::Open(path.clone()));
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                let mut flac = self.export_flac;
                if ui.checkbox(&mut flac, "exporter en FLAC").changed() {
                    self.export_flac = flac;
                }
                if ui.button("exporter").clicked() {
                    self.request_export(cmds);
                    ui.close();
                }
            },
        );
    }

    /// Annuler / refaire exposés à la coquille (une seule entrée d'historique
    /// par action, comme les raccourcis).
    pub fn undo(&mut self) -> Option<String> {
        let label = self.hist.undo(&mut self.project)?;
        self.status = format!("annulé : {label}");
        self.dirty = true;
        self.replan();
        Some(label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let label = self.hist.redo(&mut self.project)?;
        self.status = format!("refait : {label}");
        self.dirty = true;
        self.replan();
        Some(label)
    }

    /// Enregistrer depuis n'importe quel panneau (ctrl+S global).
    /// Source que l'éditeur de l'arrangement montre (pour la barre haute).
    pub fn project_tracks(&self) -> usize {
        self.project.tracks.len()
    }

    pub fn project_clips(&self) -> usize {
        self.project.clips.len()
    }

    pub fn selected_source_name(&self) -> Option<String> {
        self.project
            .sources
            .get(self.sel_source)
            .map(|s| s.title.clone())
    }

    pub fn save_project(&mut self) {
        self.save();
    }

    pub fn can_undo(&self) -> bool {
        self.hist.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.hist.can_redo()
    }

    // ── colonne des pistes ──────────────────────────────────────────────────

    fn tracks_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("PISTES")
                    .size(10.0)
                    .strong()
                    .color(crate::theme::DIM),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("+ piste").size(10.5))
                            .frame(false),
                    )
                    .on_hover_text("Ajouter une piste")
                    .clicked()
                {
                    self.checkpoint("nouvelle piste");
                    let color = (self.project.tracks.len() % PALETTE.len()) as u8;
                    let name = format!("piste {}", self.project.tracks.len() + 1);
                    let src = if self.project.sources.is_empty() {
                        None
                    } else {
                        Some(self.sel_source)
                    };
                    let id = self.project.add_track(&name, src, color);
                    self.sel_track = Some(id);
                    self.replan();
                }
            });
        });
        ui.add_space(4.0);
        let ids: Vec<Id> = self.project.tracks.iter().map(|t| t.id).collect();
        if ids.is_empty() {
            ui.label(
                egui::RichText::new("Aucune piste")
                    .size(11.5)
                    .color(crate::theme::FAINT),
            );
            return;
        }
        ui.spacing_mut().slider_width = 92.0;
        for id in ids {
            let Some(t) = self.project.track(id).cloned() else {
                continue;
            };
            let selected = self.sel_track == Some(id);
            let color = tint(t.color);
            let frame = egui::Frame::new()
                .fill(if selected {
                    crate::theme::ACCENT_SOFT.gamma_multiply(0.55)
                } else {
                    crate::theme::RAISED.gamma_multiply(0.55)
                })
                .stroke(Stroke::new(
                    1.0,
                    if selected {
                        crate::theme::ACCENT
                    } else {
                        crate::theme::LINE
                    },
                ))
                .inner_margin(egui::Margin::same(7))
                .corner_radius(CornerRadius::same(8));
            frame.show(ui, |ui| {
                // ligne 1 : couleur, nom, mute/solo/fermeture
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(3.0, 16.0), Sense::hover());
                    ui.painter().rect_filled(r, CornerRadius::same(2), color);
                    let name = if t.name.is_empty() {
                        format!("#{}", t.id)
                    } else {
                        crate::truncate_chars(&t.name, 20)
                    };
                    let full = t.name.clone();
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(name).size(11.5).strong(),
                            )
                            .frame(false),
                        )
                        .on_hover_text(full)
                        .clicked()
                    {
                        self.sel_track = Some(id);
                        if let Some(s) = t.source {
                            self.sel_source = s;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("×").size(11.0))
                                    .frame(false)
                                    .small(),
                            )
                            .on_hover_text("Supprimer la piste et ses clips (annulable)")
                            .clicked()
                        {
                            self.checkpoint("supprimer la piste");
                            if self.project.remove_track(id) {
                                self.sel_track = self.project.tracks.first().map(|x| x.id);
                                if self.sel_clip.and_then(|c| self.project.clip(c)).is_none() {
                                    self.sel_clip = None;
                                }
                                self.replan();
                            }
                        }
                        for (on, label, what) in [
                            (t.solo, "S", "solo"),
                            (t.mute, "M", "mute"),
                        ] {
                            if ui
                                .selectable_label(
                                    on,
                                    egui::RichText::new(label).size(10.0).strong(),
                                )
                                .on_hover_text(what)
                                .clicked()
                            {
                                self.checkpoint(what);
                                if let Some(x) = self.project.track_mut(id) {
                                    if what == "mute" {
                                        x.mute = !x.mute;
                                    } else {
                                        x.solo = !x.solo;
                                    }
                                }
                                self.replan();
                            }
                        }
                    });
                });
                // ligne 2 : rôle, tempo, tonalité — les faits de la piste
                ui.horizontal(|ui| {
                    let master = t.role == Role::Master;
                    let badge = ui.add(
                        egui::Button::new(
                            egui::RichText::new(if master { "maître" } else { "suit" })
                                .size(9.5)
                                .color(if master {
                                    color
                                } else {
                                    crate::theme::FAINT
                                }),
                        )
                        .frame(false)
                        .small(),
                    );
                    if badge
                        .on_hover_text(
                            "Cette piste donne le tempo du projet ;\nles autres se recalent dessus.",
                        )
                        .clicked()
                        && !master
                    {
                        self.checkpoint("maîtresse");
                        self.project.set_master(id);
                        self.replan();
                    }
                    if let Some(bpm) = self.project.track_bpm(&t) {
                        ui.label(
                            egui::RichText::new(format!("{bpm:.0} BPM"))
                                .size(10.0)
                                .monospace()
                                .color(crate::theme::DIM),
                        );
                    }
                    if let Some(k) = t
                        .source
                        .and_then(|s| self.project.sources.get(s))
                        .and_then(|s| s.key)
                    {
                        ui.label(
                            egui::RichText::new(k.name())
                                .size(10.0)
                                .color(if k.confidence < 0.08 {
                                    crate::theme::FAINT
                                } else {
                                    crate::theme::DIM
                                }),
                        )
                        .on_hover_text(if k.confidence < 0.08 {
                            "Tonalité incertaine (musique modale) : « Accord » est à vérifier à l'oreille"
                        } else {
                            "Tonalité détectée sur la source"
                        });
                    }
                });
                // ligne 3 : mode de lecture, accord
                ui.horizontal(|ui| {
                    let stretch = t.stretch == StretchMode::Stretch;
                    if ui
                        .selectable_label(
                            !stretch,
                            egui::RichText::new("Repitch").size(10.0),
                        )
                        .on_hover_text("La vitesse change, la hauteur suit (tracker)")
                        .clicked()
                        && stretch
                    {
                        self.checkpoint("repitch");
                        if let Some(x) = self.project.track_mut(id) {
                            x.stretch = StretchMode::Repitch;
                        }
                        self.replan();
                    }
                    if ui
                        .selectable_label(
                            stretch,
                            egui::RichText::new("Stretch").size(10.0),
                        )
                        .on_hover_text("Durée et hauteur indépendantes, requis pour accorder")
                        .clicked()
                        && !stretch
                    {
                        self.checkpoint("stretch");
                        if let Some(x) = self.project.track_mut(id) {
                            x.stretch = StretchMode::Stretch;
                        }
                        self.replan();
                    }
                    if ui
                        .selectable_label(
                            t.key_lock,
                            egui::RichText::new("Gamme").size(10.0),
                        )
                        .on_hover_text(
                            "Même gamme : transpose les clips vers la tonalité de la piste maîtresse",
                        )
                        .clicked()
                    {
                        self.checkpoint("accord");
                        if let Some(x) = self.project.track_mut(id) {
                            x.key_lock = !x.key_lock;
                        }
                        self.replan();
                    }
                });
                // ligne 4 : gain
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Gain")
                            .size(9.5)
                            .color(crate::theme::FAINT),
                    );
                    let mut gain = t.gain;
                    if ui
                        .add(egui::Slider::new(&mut gain, 0.0..=1.5).show_value(false))
                        .on_hover_text(format!("Gain de la piste : {:.0} %", t.gain * 100.0))
                        .changed()
                    {
                        if let Some(x) = self.project.track_mut(id) {
                            x.gain = gain;
                        }
                        self.dirty = true;
                        self.replan();
                    }
                });
            });
            ui.add_space(4.0);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.hist.can_undo(),
                    egui::Button::new(egui::RichText::new("Annuler").size(10.5)).small(),
                )
                .clicked()
            {
                if let Some(l) = self.hist.undo(&mut self.project) {
                    self.status = format!("annulé : {l}");
                    self.replan();
                }
            }
            if ui
                .add_enabled(
                    self.hist.can_redo(),
                    egui::Button::new(egui::RichText::new("Refaire").size(10.5)).small(),
                )
                .clicked()
            {
                if let Some(l) = self.hist.redo(&mut self.project) {
                    self.status = format!("refait : {l}");
                    self.replan();
                }
            }
        });
    }

    // ── timeline ────────────────────────────────────────────────────────────

    fn beat_to_x(&self, rect: Rect, beat: f64) -> f32 {
        rect.left() + ((beat - self.scroll) * self.zoom as f64) as f32
    }

    fn x_to_beat(&self, rect: Rect, x: f32) -> f64 {
        self.scroll + (x - rect.left()) as f64 / self.zoom.max(1.0) as f64
    }

    fn timeline(&mut self, ui: &mut egui::Ui, player: &mut Option<Player>) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.timeline_rect = rect;
        let p = ui.painter_at(rect);
        let v = ui.visuals().clone();

        // molette = **zoom** autour du pointeur, Maj+molette = défilement. Le
        // zoom existait déjà mais seulement en ctrl+molette : personne ne le
        // trouvait (demande user 22/11 : « pouvoir zoomer dans l'arrangement »).
        if resp.hovered() {
            let (dy, dx, shift, pos) = ui.input(|i| {
                (
                    i.smooth_scroll_delta.y,
                    i.smooth_scroll_delta.x,
                    i.modifiers.shift,
                    i.pointer.hover_pos(),
                )
            });
            if dy != 0.0 || dx != 0.0 {
                let delta = if dx.abs() > dy.abs() { dx } else { dy };
                if shift {
                    self.scroll =
                        (self.scroll - delta as f64 / self.zoom.max(1.0) as f64).max(0.0);
                } else {
                    let anchor = pos
                        .map(|p| self.x_to_beat(rect, p.x))
                        .unwrap_or(self.scroll);
                    let factor = (delta as f64 * 0.0013).exp();
                    self.zoom = (self.zoom as f64 * factor).clamp(8.0, 400.0) as f32;
                    if let Some(p) = pos {
                        let px = (p.x - rect.left()) as f64;
                        self.scroll = (anchor - px / self.zoom as f64).max(0.0);
                    }
                }
                // un geste manuel reprend la main : le suivi se réarme au
                // prochain « écouter » (sinon la vue se battrait avec le doigt)
                self.follow = false;
            }
        }

        // la tête reste dans la fenêtre pendant la lecture
        if self.playing && self.follow {
            let b = self.position_beats();
            let vis = rect.width() as f64 / self.zoom.max(1.0) as f64;
            if let Some(scroll) = follow_scroll(b, self.scroll, vis) {
                self.scroll = scroll;
            }
        }

        let lanes_top = rect.top() + RULER_H;
        let tracks = self.project.tracks.clone();
        let n = tracks.len().max(1);

        // règle : une bande pleine, sinon les numéros flottent sur le vide
        let ruler = Rect::from_min_size(rect.left_top(), Vec2::new(rect.width(), RULER_H));
        p.rect_filled(ruler, CornerRadius::ZERO, crate::theme::CHROME);
        p.hline(
            rect.left()..=rect.right(),
            lanes_top - 0.5,
            Stroke::new(1.0, crate::theme::LINE),
        );

        // grille : mesures + temps
        let beat0 = self.x_to_beat(rect, rect.left()).floor() as i64;
        let beat1 = self.x_to_beat(rect, rect.right()).ceil() as i64;
        for b in beat0..=beat1 {
            let x = self.beat_to_x(rect, b as f64);
            if x < rect.left() || x > rect.right() {
                continue;
            }
            let bar = b.rem_euclid(4) == 0;
            let col = if bar {
                v.widgets.noninteractive.bg_stroke.color
            } else {
                v.faint_bg_color
            };
            p.line_segment(
                [egui::pos2(x, lanes_top), egui::pos2(x, rect.bottom())],
                Stroke::new(if bar { 1.2 } else { 0.6 }, col),
            );
            if bar {
                p.text(
                    egui::pos2(x + 4.0, rect.top() + 4.0),
                    Align2::LEFT_TOP,
                    format!("{}", b / 4 + 1),
                    FontId::monospace(10.0),
                    crate::theme::DIM,
                );
            } else {
                // temps intermédiaires : un cran court dans la règle
                p.line_segment(
                    [
                        egui::pos2(x, ruler.bottom() - 4.0),
                        egui::pos2(x, ruler.bottom()),
                    ],
                    Stroke::new(1.0, crate::theme::LINE),
                );
            }
        }

        // fond des lanes
        for i in 0..n {
            let y0 = lanes_top + i as f32 * LANE_H;
            let r =
                Rect::from_min_size(egui::pos2(rect.left(), y0), Vec2::new(rect.width(), LANE_H));
            if i % 2 == 1 {
                p.rect_filled(r, CornerRadius::ZERO, v.faint_bg_color.gamma_multiply(0.5));
            }
            // marque de piste maîtresse
            if let Some(t) = tracks.get(i) {
                if t.role == Role::Master {
                    p.rect_filled(
                        Rect::from_min_size(r.min, Vec2::new(3.0, LANE_H)),
                        CornerRadius::ZERO,
                        tint(t.color),
                    );
                }
            }
        }

        // piste vide : un état vide lisible, pas une grille muette
        if self.project.clips.is_empty() {
            let (title, hint) = if self.project.sources.len() < 2 {
                (
                    "Aucune séquence",
                    "Glissez une tranche depuis l'éditeur, en bas.\nDouble-cliquez une archive pour ajouter une autre musique.",
                )
            } else {
                (
                    "Aucune séquence",
                    "Glissez des tranches sur les pistes : elles se jouent ensemble.",
                )
            };
            let c = rect.center();
            p.text(
                egui::pos2(c.x, c.y - 12.0),
                Align2::CENTER_CENTER,
                title,
                FontId::proportional(14.0),
                crate::theme::DIM,
            );
            // le texte s'enroule : dans un panneau étroit, une ligne unique
            // était coupée aux deux bords
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = (rect.width() - 40.0).max(90.0);
            job.append(
                hint,
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(11.5),
                    color: crate::theme::FAINT,
                    ..Default::default()
                },
            );
            let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
            p.galley(
                egui::pos2(c.x - galley.size().x / 2.0, c.y + 2.0),
                galley,
                crate::theme::FAINT,
            );
        }

        // clips
        let sounding = self.sounding_clip().map(|c| c.id);
        let mut hits: Vec<(Rect, Id, bool, bool)> = Vec::new();
        for (i, track) in tracks.iter().enumerate() {
            let y0 = lanes_top + i as f32 * LANE_H + 4.0;
            for clip in self.project.clips.iter().filter(|c| c.track == track.id) {
                let x0 = self.beat_to_x(rect, clip.at_beat);
                let x1 = self.beat_to_x(rect, clip.at_beat + clip.length_beats);
                if x1 < rect.left() || x0 > rect.right() {
                    continue;
                }
                let r = Rect::from_min_max(
                    egui::pos2(x0.max(rect.left() - 20.0), y0),
                    egui::pos2(x1.min(rect.right() + 20.0), y0 + LANE_H - 8.0),
                );
                let col = tint(track.color);
                let on = sounding == Some(clip.id);
                p.rect_filled(
                    r,
                    CornerRadius::same(4),
                    col.gamma_multiply(if on { 0.55 } else { 0.28 }),
                );
                // jauge : la part déjà jouée du clip, sinon on ne sait pas où
                // on en est dans une longue séquence
                if on {
                    let frac = ((self.position_beats() - clip.at_beat) / clip.length_beats)
                        .clamp(0.0, 1.0) as f32;
                    p.rect_filled(
                        Rect::from_min_size(
                            egui::pos2(r.left(), r.bottom() - 3.0),
                            Vec2::new(r.width() * frac, 3.0),
                        ),
                        CornerRadius::ZERO,
                        crate::theme::ACCENT,
                    );
                }
                let wave = self.waves.get(clip.source).and_then(|w| w.clone());
                if let Some(wave) = wave {
                    let key = WaveKey {
                        source: clip.source,
                        start: clip.src_start,
                        end: clip.src_end,
                        tint: [col.r(), col.g(), col.b(), 255],
                    };
                    let tex = self.tex.get_or_build(ui.ctx(), key, &wave, col);
                    p.image(
                        tex.id(),
                        r.shrink2(Vec2::new(2.0, 3.0)),
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::from_rgba_unmultiplied(255, 255, 255, 210),
                    );
                }
                // zoom profond : une colonne couvre moins de 4 trames → on
                // dessine le PCM, pas une enveloppe aplatie
                let span = clip.src_end.saturating_sub(clip.src_start);
                let px_per_frame = r.width() / span.max(1) as f32;
                if px_per_frame > 0.25 && r.width() < 4000.0 {
                    if let Some(d) = self.audio.get(clip.source).and_then(|a| a.as_ref()) {
                        let ch = d.channels.max(1) as usize;
                        let n = r.width().max(1.0) as usize;
                        let mut pts = Vec::with_capacity(n);
                        for k in 0..n {
                            let f =
                                clip.src_start as f64 + (k as f64 / n as f64) * span.max(1) as f64;
                            let idx = (f as usize).min(clip.src_end.saturating_sub(1));
                            let s = d.pcm.get(idx * ch).copied().unwrap_or(0.0);
                            pts.push(egui::pos2(
                                r.left() + k as f32,
                                r.center().y - s * r.height() * 0.45,
                            ));
                        }
                        p.add(egui::Shape::line(pts, Stroke::new(1.0, col)));
                    }
                }
                // fades : la pente réelle appliquée par le mixer
                let fi = (clip.fade_in * self.zoom as f64) as f32;
                if fi > 1.0 {
                    p.line_segment(
                        [
                            egui::pos2(r.left(), r.bottom()),
                            egui::pos2(r.left() + fi, r.top()),
                        ],
                        Stroke::new(1.0, col.gamma_multiply(0.8)),
                    );
                }
                let fo = (clip.fade_out * self.zoom as f64) as f32;
                if fo > 1.0 {
                    p.line_segment(
                        [
                            egui::pos2(r.right() - fo, r.top()),
                            egui::pos2(r.right(), r.bottom()),
                        ],
                        Stroke::new(1.0, col.gamma_multiply(0.8)),
                    );
                }
                let selected = self.sel_clip == Some(clip.id);
                p.rect_stroke(
                    r,
                    CornerRadius::same(4),
                    Stroke::new(
                        if on || selected { 2.0 } else { 1.0 },
                        if on { crate::theme::ACCENT } else { col },
                    ),
                    StrokeKind::Inside,
                );
                if r.width() > 42.0 {
                    let name = self
                        .project
                        .sources
                        .get(clip.source)
                        .map(|s| s.title.clone())
                        .unwrap_or_default();
                    p.text(
                        egui::pos2(r.left() + 5.0, r.top() + 3.0),
                        Align2::LEFT_TOP,
                        name,
                        FontId::proportional(9.5),
                        v.strong_text_color(),
                    );
                }
                // poignées de rognage visibles au survol
                let edge = 5.0;
                let hover = ui.input(|i| i.pointer.hover_pos());
                if let Some(h) = hover {
                    if r.contains(h) {
                        let left = (h.x - r.left()).abs() <= edge;
                        let right = (h.x - r.right()).abs() <= edge;
                        if left || right {
                            p.rect_filled(
                                Rect::from_min_size(
                                    egui::pos2(
                                        if left { r.left() } else { r.right() - 3.0 },
                                        r.top(),
                                    ),
                                    Vec2::new(3.0, r.height()),
                                ),
                                CornerRadius::ZERO,
                                col,
                            );
                        }
                    }
                }
                hits.push((r, clip.id, true, true));
            }
        }

        // tête de lecture : trait + repère dans la règle, sinon on la perd
        if self.playing || self.transport.is_some() {
            let b = self.position_beats();
            let x = self.beat_to_x(rect, b);
            if x >= rect.left() && x <= rect.right() {
                let accent = crate::theme::ACCENT;
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(2.0, accent),
                );
                p.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(x - 5.0, rect.top()),
                        egui::pos2(x + 5.0, rect.top()),
                        egui::pos2(x, rect.top() + 8.0),
                    ],
                    accent,
                    Stroke::NONE,
                ));
            }
        }

        // clic droit : sélectionner le clip sous le curseur puis menu
        if resp.secondary_clicked() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if let Some((_, id, _, _)) = hits.iter().rev().find(|(r, _, _, _)| r.contains(pos))
                {
                    self.sel_clip = Some(*id);
                    if let Some(c) = self.project.clip(*id) {
                        self.sel_track = Some(c.track);
                    }
                } else {
                    self.sel_clip = None;
                }
            }
        }
        resp.context_menu(|ui| self.clip_menu(ui));

        // interactions
        if resp.clicked() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if pos.y < lanes_top {
                    let beat = self.x_to_beat(rect, pos.x).max(0.0);
                    self.seek_beats(edit::snap(beat, self.snap));
                    self.status = format!("tête de lecture : mesure {:.0}", beat / 4.0 + 1.0);
                } else if let Some((_, id, _, _)) =
                    hits.iter().rev().find(|(r, _, _, _)| r.contains(pos))
                {
                    // clic sur une séquence de la liste (= un clip) : elle se
                    // joue depuis son début — c'est le geste d'écoute attendu,
                    // pas seulement une sélection
                    self.sel_clip = Some(*id);
                    if let Some(c) = self.project.clip(*id).cloned() {
                        self.sel_track = Some(c.track);
                        self.seek_beats(c.at_beat);
                        if !self.playing {
                            self.play(player);
                        }
                        self.status = format!(
                            "Lecture depuis la séquence — mesure {}",
                            (c.at_beat / 4.0).floor() + 1.0
                        );
                    }
                } else {
                    self.sel_clip = None;
                }
            }
        }

        // glisser : démarrer sur un clip, suivre, relâcher
        if resp.drag_started() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if pos.y >= lanes_top {
                    if let Some((r, id, _, _)) =
                        hits.iter().rev().find(|(r, _, _, _)| r.contains(pos))
                    {
                        self.sel_clip = Some(*id);
                        if let Some(c) = self.project.clip(*id) {
                            self.sel_track = Some(c.track);
                            // le geste se décide sur le point d'APPUI : au
                            // moment où egui signale le glisser, le pointeur a
                            // déjà quitté la poignée de quelques pixels (sinon
                            // un rognage devenait un déplacement)
                            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                            let edge = 6.0;
                            let left = (origin.x - r.left()).abs() <= edge;
                            let right = (origin.x - r.right()).abs() <= edge;
                            self.drag = if left {
                                Some(Drag::Left {
                                    id: *id,
                                    at: c.at_beat,
                                    len: c.length_beats,
                                })
                            } else if right {
                                Some(Drag::Right {
                                    id: *id,
                                    len: c.length_beats,
                                })
                            } else {
                                // l'origine du glisser est le point d'APPUI, pas
                                // celui où le seuil est franchi : sinon le clip
                                // saute de la distance parcourue avant le seuil
                                let grab = ui
                                    .input(|i| i.pointer.press_origin())
                                    .map(|p| self.x_to_beat(rect, p.x))
                                    .unwrap_or_else(|| self.x_to_beat(rect, pos.x));
                                Some(Drag::Move {
                                    id: *id,
                                    from: c.at_beat,
                                    grab,
                                })
                            };
                            if !matches!(self.drag, Some(Drag::Slice { .. })) {
                                self.checkpoint("déplacer");
                            }
                        }
                    }
                } else {
                    let beat = self.x_to_beat(rect, pos.x).max(0.0);
                    self.seek_beats(edit::snap(beat, self.snap));
                }
            }
        }
        if resp.dragged() {
            if let (Some(d), Some(pos)) =
                (self.drag.clone(), ui.input(|i| i.pointer.interact_pos()))
            {
                let beat = self.x_to_beat(rect, pos.x);
                if !ui.input(|i| i.modifiers.alt) && self.snap > 0.0 {
                    let sx = self.beat_to_x(rect, edit::snap(beat, self.snap));
                    if sx >= rect.left() && sx <= rect.right() {
                        p.line_segment(
                            [egui::pos2(sx, lanes_top), egui::pos2(sx, rect.bottom())],
                            Stroke::new(1.0, v.selection.stroke.color.gamma_multiply(0.5)),
                        );
                    }
                }
                match d {
                    Drag::Move { id, from, grab } => {
                        let step = if ui.input(|i| i.modifiers.alt) {
                            0.0
                        } else {
                            self.snap
                        };
                        let delta = beat - grab;
                        // vertical : le clip suit la piste sous le curseur
                        let target = if pos.y >= lanes_top {
                            tracks
                                .get(((pos.y - lanes_top) / LANE_H) as usize)
                                .map(|t| t.id)
                        } else {
                            None
                        };
                        if let Some(c) = self.project.clip_mut(id) {
                            edit::move_clip(c, from + delta, step);
                            if let Some(t) = target {
                                c.track = t;
                            }
                        }
                        if let Some(t) = target {
                            self.sel_track = Some(t);
                        }
                    }
                    Drag::Right { id, len } => {
                        if let Some(c) = self.project.clip_mut(id) {
                            let new_len = beat - c.at_beat;
                            let _ = len;
                            edit::trim_right(c, new_len, self.snap.min(0.25));
                        }
                    }
                    Drag::Left { id, at, len } => {
                        if let Some(c) = self.project.clip_mut(id) {
                            let new_at = beat;
                            let _ = (at, len);
                            edit::trim_left(c, new_at, self.snap.min(0.25));
                        }
                    }
                    Drag::Slice { .. } => {}
                }
            }
        }
        if resp.drag_stopped() {
            let was = self.drag.take();
            if matches!(
                was,
                Some(Drag::Move { .. }) | Some(Drag::Right { .. }) | Some(Drag::Left { .. })
            ) {
                edit::drop_empty(&mut self.project);
                self.replan();
                self.dirty = true;
            }
        }

        // dépôt d'une tranche tirée de l'éditeur
        if let Some(Drag::Slice {
            source,
            src_start,
            src_end,
            length_beats,
        }) = self.drag.clone()
        {
            let ptr = ui.input(|i| i.pointer.interact_pos());
            let released = ui.input(|i| i.pointer.any_released());
            if let Some(pos) = ptr {
                if rect.contains(pos) && pos.y >= lanes_top {
                    let idx = ((pos.y - lanes_top) / LANE_H) as usize;
                    let lane = tracks.get(idx.min(tracks.len().saturating_sub(1)));
                    // fantôme sous le curseur
                    let color = lane.map(|t| tint(t.color)).unwrap_or_else(|| tint(0));
                    let x0 = pos.x - 12.0;
                    p.rect_filled(
                        Rect::from_min_size(
                            egui::pos2(x0, lanes_top + idx as f32 * LANE_H),
                            Vec2::new(60.0, LANE_H - 6.0),
                        ),
                        CornerRadius::same(4),
                        color.gamma_multiply(0.45),
                    );
                    if released {
                        let at = edit::snap(self.x_to_beat(rect, pos.x), self.snap).max(0.0);
                        self.checkpoint("déposer une tranche");
                        // plus aucune piste (la dernière a été supprimée) : la
                        // tranche déposée recrée une piste pour sa source
                        let track_id = match lane {
                            Some(t) => t.id,
                            None => {
                                let id = self.project.add_track("piste 1", Some(source), 0);
                                self.sel_track = Some(id);
                                id
                            }
                        };
                        let name = self
                            .project
                            .track(track_id)
                            .map(|t| t.name.clone())
                            .unwrap_or_default();
                        let mut clip = edit::clip_from_slice(
                            track_id,
                            source,
                            src_start,
                            src_end,
                            at,
                            self.snap,
                            length_beats,
                        );
                        clip.key = self
                            .audio
                            .get(source)
                            .and_then(|a| a.as_ref())
                            .and_then(|d| slice_key(d, src_start, src_end));
                        let id = self.project.add_clip(clip);
                        self.sel_clip = Some(id);
                        self.sel_track = Some(track_id);
                        self.drag = None;
                        self.replan();
                        self.status = format!(
                            "tranche déposée sur « {name} » à mesure {:.0}",
                            at / 4.0 + 1.0
                        );
                    }
                } else if released {
                    self.drag = None;
                }
            } else if released {
                self.drag = None;
            }
        }
    }

    /// Menu contextuel d'un clip : toutes les actions passent par un
    /// instantané d'historique (une action = un undo).
    fn clip_menu(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.sel_clip else {
            ui.label("aucun clip sélectionné");
            return;
        };
        let Some(clip) = self.project.clip(id).cloned() else {
            return;
        };
        let title = self
            .project
            .sources
            .get(clip.source)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "clip".into());
        ui.label(egui::RichText::new(title).strong());
        ui.label(
            egui::RichText::new(format!(
                "mesure {:.0} · {:.2} temps",
                clip.at_beat / 4.0 + 1.0,
                clip.length_beats
            ))
            .size(10.5)
            .color(ui.visuals().weak_text_color()),
        );
        ui.separator();
        if ui.button("dupliquer").clicked() {
            self.checkpoint("dupliquer");
            if let Some(new) = edit::duplicate(&mut self.project, id) {
                self.sel_clip = Some(new);
            }
            self.replan();
            ui.close();
        }
        if ui
            .add_enabled(
                {
                    let b = self.position_beats();
                    b > clip.at_beat && b < clip.at_beat + clip.length_beats
                },
                egui::Button::new("couper à la tête de lecture"),
            )
            .clicked()
        {
            self.checkpoint("couper");
            let beat = self.position_beats();
            if edit::split_at(&mut self.project, id, beat).is_some() {
                self.status = "clip coupé".into();
            }
            self.replan();
            ui.close();
        }
        if ui.button("supprimer").clicked() {
            self.checkpoint("supprimer");
            self.project.remove_clip(id);
            self.sel_clip = None;
            self.replan();
            ui.close();
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("transposer");
            for d in [-12, -1, 1, 12] {
                if ui.small_button(format!("{d:+}")).clicked() {
                    self.checkpoint("transposer");
                    if let Some(c) = self.project.clip_mut(id) {
                        let base = c.transpose.unwrap_or(0);
                        c.transpose = Some((base + d).clamp(-48, 48));
                    }
                    self.replan();
                    ui.close();
                }
            }
        });
        let locked = clip.key_lock.unwrap_or(false);
        if ui
            .selectable_label(locked, "accorder à la maîtresse")
            .on_hover_text("transposition en bloc vers la tonalité de la piste maîtresse (passe le clip en stretch)")
            .clicked()
        {
            self.checkpoint("même gamme");
            if let Some(c) = self.project.clip_mut(id) {
                c.key_lock = Some(!locked);
            }
            self.replan();
            ui.close();
        }
        ui.horizontal(|ui| {
            let stretch = clip
                .stretch
                .unwrap_or(self.project.effective_stretch(&clip))
                == StretchMode::Stretch;
            if ui.selectable_label(!stretch, "repitch").clicked() {
                self.checkpoint("repitch");
                if let Some(c) = self.project.clip_mut(id) {
                    c.stretch = Some(StretchMode::Repitch);
                }
                self.replan();
                ui.close();
            }
            if ui.selectable_label(stretch, "stretch").clicked() {
                self.checkpoint("stretch");
                if let Some(c) = self.project.clip_mut(id) {
                    c.stretch = Some(StretchMode::Stretch);
                }
                self.replan();
                ui.close();
            }
        });
        ui.separator();
        let mut g = clip.gain;
        if ui
            .add(egui::Slider::new(&mut g, 0.0..=1.5).text("gain du clip"))
            .changed()
        {
            if let Some(c) = self.project.clip_mut(id) {
                c.gain = g;
            }
            self.dirty = true;
            self.replan();
        }
        ui.separator();
        if ui.button("fades 1/4 temps").clicked() {
            self.checkpoint("fades");
            let q = self.snap.max(0.25);
            if let Some(c) = self.project.clip_mut(id) {
                c.fade_in = q;
                c.fade_out = q;
            }
            self.replan();
            ui.close();
        }
        if ui.button("sans fades").clicked() {
            self.checkpoint("fades");
            if let Some(c) = self.project.clip_mut(id) {
                c.fade_in = 0.0;
                c.fade_out = 0.0;
            }
            self.replan();
            ui.close();
        }
        ui.separator();
        if ui
            .button("boucler pour remplir la mesure")
            .on_hover_text("répète le contenu jusqu'à la fin de la mesure (fill=Loop)")
            .clicked()
        {
            self.checkpoint("boucler le clip");
            edit::loop_to_bar(&mut self.project, id, 4.0, self.snap);
            self.replan();
            ui.close();
        }
        if ui
            .button("ajuster à la mesure")
            .on_hover_text("étire la durée musicale jusqu'à la mesure suivante (action explicite, le contenu est réétalé)")
            .clicked()
        {
            self.checkpoint("ajuster à la mesure");
            edit::fit_bar(&mut self.project, id, 4.0, self.snap);
            self.replan();
            ui.close();
        }
    }

    // ── éditeur de source (bas) ─────────────────────────────────────────────

    /// **Toutes** les sources, l'une sous l'autre : leurs tranches restent
    /// visibles et glissables sans passer par un sélecteur (demande user 22/11 :
    /// « c'est nul de devoir choisir entre telle ou telle chanson »).
    pub fn source_panel(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        if self.project.sources.is_empty() {
            ui.label(
                "Aucune source.\nDouble-cliquez une piste dans la bibliothèque, ou déposez un fichier sur la fenêtre.",
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SOURCES")
                    .size(10.0)
                    .strong()
                    .color(crate::theme::DIM),
            );
            ui.label(
                egui::RichText::new("clic : écouter · glisser : poser sur une piste")
                    .size(10.0)
                    .color(crate::theme::FAINT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (b, name) in [
                    (8.0, "2 mes."),
                    (4.0, "1 mesure"),
                    (2.0, "2 temps"),
                    (1.0, "1 temps"),
                ] {
                    if ui
                        .selectable_label((self.clip_beats - b).abs() < 1e-9, name)
                        .clicked()
                    {
                        self.clip_beats = b;
                        self.dirty = true;
                    }
                }
                ui.label(
                    egui::RichText::new("segment")
                        .size(10.0)
                        .color(crate::theme::FAINT),
                );
            });
        });
        ui.add_space(4.0);
        self.tiles.clear();
        let ids: Vec<usize> = (0..self.project.sources.len()).collect();
        let h = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("gt_sources")
            .max_height(h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for idx in ids {
                    self.source_row(ui, idx, cmds);
                }
            });
    }

    /// Une source dans la pile : son nom, son tempo, ses tranches. Le clic sur
    /// le nom la désigne pour l'inspecteur ; les tranches se glissent comme
    /// avant.
    fn source_row(&mut self, ui: &mut egui::Ui, idx: usize, cmds: &mut Vec<Cmd>) {
        let Some(src) = self.project.sources.get(idx).cloned() else {
            return;
        };
        let selected = self.sel_source == idx;
        let tempo = self.tempos.get(idx).and_then(|t| *t);
        let n_slices = self.slices_for(idx).len();
        let rate = self
            .audio
            .get(idx)
            .and_then(|a| a.as_ref())
            .map(|d| d.rate)
            .unwrap_or(44_100);
        let beat_src = tempo
            .map(|t| t.beat)
            .filter(|b| *b > 0.0)
            .unwrap_or_else(|| self.beat_sec());
        egui::Frame::new()
            .fill(if selected {
                crate::theme::RAISED.gamma_multiply(0.6)
            } else {
                Color32::TRANSPARENT
            })
            .inner_margin(egui::Margin::symmetric(6, 4))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let short = crate::truncate_chars(&src.title, 44);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(short)
                                    .size(11.5)
                                    .strong()
                                    .color(if selected {
                                        crate::theme::TEXT
                                    } else {
                                        crate::theme::DIM
                                    }),
                            )
                            .frame(false),
                        )
                        .on_hover_text(&src.title)
                        .clicked()
                    {
                        self.sel_source = idx;
                    }
                    if let Some(t) = tempo {
                        ui.label(
                            egui::RichText::new(format!("{:.1} BPM", t.bpm))
                                .size(10.0)
                                .monospace()
                                .color(crate::theme::FAINT),
                        );
                    }
                    if let Some(k) = src.key {
                        ui.label(
                            egui::RichText::new(k.name())
                                .size(10.0)
                                .color(crate::theme::FAINT),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{n_slices} tranches"))
                                .size(10.0)
                                .color(crate::theme::FAINT),
                        );
                    });
                });
                // bande d'onde : les deux bords de boucle se tirent ici même
                self.source_wave(ui, idx, cmds);
                let slices = self.slices_for(idx);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    for (i, (a, b)) in slices.iter().enumerate() {
                        let frames = |s: f64| (s * rate as f64).round() as usize;
                        let len_beats = ((b - a) / beat_src).max(0.25);
                        let label = format!("{}\n{:.2} t", i + 1, len_beats);
                        let (r, resp) = ui
                            .allocate_exact_size(Vec2::new(52.0, 46.0), Sense::click_and_drag());
                        let col = tint((i % 8) as u8);
                        // la tranche que le projet joue en ce moment
                        let air = self.sounding_clip().is_some_and(|c| {
                            c.source == idx
                                && c.src_start == frames(*a)
                                && c.src_end == frames(*b)
                        });
                        p_tile(ui, r, &label, col, resp.hovered() || resp.dragged(), air);
                        let _ = resp.clone().on_hover_text(format!(
                            "tranche {} · {:.2} temps\nclic : écouter une fois · glisser : poser sur une piste",
                            i + 1,
                            len_beats
                        ));
                        self.tiles.push(Tile {
                            rect: r,
                            source: idx,
                            src_start: frames(*a),
                            src_end: frames(*b),
                            length_beats: len_beats,
                        });
                        if resp.drag_started() {
                            self.drag = Some(Drag::Slice {
                                source: idx,
                                src_start: frames(*a),
                                src_end: frames(*b),
                                length_beats: len_beats,
                            });
                        }
                        if resp.clicked() {
                            // écoute rapide : la tranche se joue une fois, sans
                            // être posée sur la timeline (clic ≠ dépôt)
                            self.drag = None;
                            cmds.push(Cmd::Preview {
                                source: idx,
                                src_start: frames(*a),
                                src_end: frames(*b),
                                label: i + 1,
                            });
                        }
                    }
                });
            });
        ui.add_space(2.0);
    }

    /// Bande d'onde compacte de la source, avec sa **boucle** : les deux bords
    /// se tirent au pixel (Maj = calage sur le pas), exactement comme la
    /// poignée du mode Découpe — la correction n'oblige plus à changer
    /// d'espace (demande user 22/11 : « pourquoi le refaire dans l'éditeur ? »).
    fn source_wave(&mut self, ui: &mut egui::Ui, idx: usize, cmds: &mut Vec<Cmd>) {
        let Some(wave) = self.waves.get(idx).and_then(|w| w.clone()) else {
            return;
        };
        let total = self
            .audio
            .get(idx)
            .and_then(|a| a.as_ref())
            .map(|d| d.total)
            .unwrap_or(0.0);
        if total <= 0.1 {
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 30.0),
            Sense::click_and_drag(),
        );
        let p = ui.painter_at(rect);
        let col = tint((idx % 8) as u8);
        p.rect_filled(rect, CornerRadius::same(4), crate::theme::CHROME);
        let key = WaveKey {
            source: idx,
            start: 0,
            end: wave.frames,
            tint: [col.r(), col.g(), col.b(), 255],
        };
        let tex = self.tex.get_or_build(ui.ctx(), key, &wave, col);
        p.image(
            tex.id(),
            rect.shrink2(Vec2::new(1.0, 3.0)),
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::from_rgba_unmultiplied(255, 255, 255, 170),
        );
        let src = self.project.sources.get(idx).cloned();
        let (a, b) = match &src {
            Some(s) => (
                s.loop_start.unwrap_or(0.0).clamp(0.0, total),
                s.loop_end.unwrap_or(total).clamp(0.0, total),
            ),
            None => (0.0, total),
        };
        let b = b.max(a + 0.1).min(total);
        let map = |t: f64| rect.left() + (t / total).clamp(0.0, 1.0) as f32 * rect.width();
        let (xa, xb) = (map(a), map(b));
        // hors boucle : assombri — l'intro et le fondu ne font pas partie du
        // morceau, on voit d'un coup d'œil ce qui sera découpé
        let dark = Color32::from_black_alpha(150);
        if xa > rect.left() + 1.0 {
            p.rect_filled(
                Rect::from_min_max(rect.left_top(), egui::pos2(xa, rect.bottom())),
                CornerRadius::ZERO,
                dark,
            );
        }
        if xb < rect.right() - 1.0 {
            p.rect_filled(
                Rect::from_min_max(egui::pos2(xb, rect.top()), rect.right_bottom()),
                CornerRadius::ZERO,
                dark,
            );
        }
        p.rect_stroke(
            Rect::from_min_max(egui::pos2(xa, rect.top()), egui::pos2(xb, rect.bottom())),
            CornerRadius::ZERO,
            Stroke::new(1.0, crate::theme::ACCENT),
            StrokeKind::Inside,
        );
        for x in [xa, xb] {
            p.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(2.0, crate::theme::ACCENT),
            );
        }

        // prise du bord : ±7 px, comme la poignée de l'éditeur
        let tol = 7.0;
        if resp.drag_started() {
            self.src_grab = resp.interact_pointer_pos().and_then(|q| {
                let ds = (q.x - xa).abs();
                let de = (q.x - xb).abs();
                if ds <= tol && ds <= de {
                    Some((idx, true))
                } else if de <= tol {
                    Some((idx, false))
                } else {
                    None
                }
            });
        }
        if resp.hovered() && self.src_grab.is_none() {
            if let Some(q) = resp.hover_pos() {
                if (q.x - xa).abs() <= tol || (q.x - xb).abs() <= tol {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
        }
        if let (Some((g, left)), Some(q)) = (self.src_grab, resp.interact_pointer_pos()) {
            if g == idx {
                let t = ((q.x - rect.left()) / rect.width().max(1.0)) as f64 * total;
                let step = self
                    .tempos
                    .get(idx)
                    .and_then(|x| *x)
                    .map(|t| t.beat * self.clip_beats.max(1.0))
                    .unwrap_or(0.0);
                let t = gtracker_core::util::snap_edge(
                    t,
                    0.0,
                    step,
                    ui.input(|i| i.modifiers.shift),
                );
                let (na, nb) = gtracker_core::util::move_edge((a, b), left, t, total);
                if let Some(s) = self.project.sources.get_mut(idx) {
                    s.loop_start = Some(na);
                    s.loop_end = Some(nb);
                }
                self.dirty = true;
            }
        }
        if resp.drag_stopped() {
            if let Some((g, _)) = self.src_grab.take() {
                if let Some(s) = self.project.sources.get(g) {
                    if let (Some(a), Some(b)) = (s.loop_start, s.loop_end) {
                        self.status = format!(
                            "boucle de la source : {} → {}",
                            gtracker_core::util::fmt_time_cs(a),
                            gtracker_core::util::fmt_time_cs(b)
                        );
                        cmds.push(Cmd::SourceLoop {
                            source: g,
                            start: a,
                            end: b,
                        });
                    }
                }
            }
        }
    }

    fn slices_for(&self, idx: usize) -> Vec<(f64, f64)> {
        let total = self
            .audio
            .get(idx)
            .and_then(|a| a.as_ref())
            .map(|d| d.total)
            .unwrap_or(0.0);
        if total <= 0.0 {
            return Vec::new();
        }
        // **découpe depuis la boucle** : une source qui déclare son début de
        // boucle (genh, VGM à intro) ne fait pas de tranche dans l'intro ;
        // sinon on part de 0 comme avant.
        let (from, to) = self
            .project
            .sources
            .get(idx)
            .map(|s| {
                let a = s.loop_start.unwrap_or(0.0).clamp(0.0, total);
                let b = s
                    .loop_end
                    .map(|e| e.min(total))
                    .filter(|e| *e > a + 0.2)
                    .unwrap_or(total);
                (a, b)
            })
            .unwrap_or((0.0, total));
        let len = self
            .tempos
            .get(idx)
            .and_then(|t| *t)
            .map(|t| t.beat * self.clip_beats)
            .filter(|l| *l > 0.0)
            .unwrap_or((to - from).max(0.1) / 8.0);
        let mut out = Vec::new();
        let mut t = from;
        while t < to - 1e-3 {
            let end = (t + len).min(to);
            if end - t > 0.02 {
                out.push((t, end));
            }
            t = end;
            if out.len() > 512 {
                break;
            }
        }
        out
    }

    /// La boucle corrigée dans l'éditeur vaut pour la **source** : la pile de
    /// l'arrangement découpe au même endroit, sans second réglage.
    pub fn set_source_loop(&mut self, file: &std::path::Path, subsong: u32, w: (f64, f64)) {
        if let Some(s) = self
            .project
            .sources
            .iter_mut()
            .find(|s| s.file == file && s.subsong == subsong)
        {
            s.loop_start = Some(w.0);
            s.loop_end = Some(w.1);
        }
    }

    /// Charge un projet depuis le disque (les sources doivent être ré-analysées).
    pub fn open(&mut self, path: PathBuf) -> anyhow::Result<Vec<AudioTrack>> {
        let p = Project::load(&path)?;
        // les épingles de l'ancien projet tombent : un projet fermé ne doit pas
        // retenir son audio en RAM toute la session
        gtracker_core::decode::unpin_all();
        self.project = p;
        self.path = Some(path);
        self.audio.clear();
        self.waves.clear();
        self.tempos.clear();
        self.keys.clear();
        self.sel_clip = None;
        self.sel_track = self.project.tracks.first().map(|t| t.id);
        self.transport = None;
        self.playing = false;
        self.tex.clear();
        self.hist = History::default();
        self.dirty = false;
        let tracks: Vec<AudioTrack> = self
            .project
            .sources
            .iter()
            .map(|s| AudioTrack {
                subsong: s.subsong,
                title: s.title.clone(),
                file: s.file.clone(),
                length: None,
                loop_start: None,
                loop_end: None,
            })
            .collect();
        Ok(tracks)
    }

    pub fn project_name(&self) -> &str {
        &self.project.name
    }

    pub fn project_dir(&self) -> PathBuf {
        project::projects_dir()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// Tonalité **de la tranche** déposée, pas de la source entière : c'est le
/// matériau réellement joué par le clip.
fn slice_key(d: &Decoded, start: usize, end: usize) -> Option<Key> {
    let ch = d.channels.max(1) as usize;
    let total = d.pcm.len() / ch;
    if end <= start || end > total || end - start < d.rate as usize / 8 {
        return None;
    }
    key::analyse(&d.pcm, d.channels, d.rate, start, end)
}

fn p_tile(ui: &egui::Ui, r: Rect, label: &str, col: Color32, hot: bool, air: bool) {
    let p = ui.painter();
    let fill = if air {
        crate::theme::ACCENT_SOFT
    } else if hot {
        crate::theme::HOVER
    } else {
        crate::theme::RAISED
    };
    let edge = if air || hot {
        crate::theme::ACCENT
    } else {
        crate::theme::LINE
    };
    p.rect_filled(r, CornerRadius::same(6), fill);
    p.rect_stroke(
        r,
        CornerRadius::same(6),
        Stroke::new(if air { 1.8 } else { 1.0 }, edge),
        StrokeKind::Inside,
    );
    // l'identité de la tranche tient dans un liseré : huit couleurs pleines
    // faisaient une mosaïque de jouets (retour user 21/11)
    let strip = Rect::from_min_max(
        egui::pos2(r.left() + 1.0, r.bottom() - 3.0),
        egui::pos2(r.right() - 1.0, r.bottom() - 1.0),
    );
    p.rect_filled(strip, CornerRadius::same(1), col.gamma_multiply(0.85));
    p.text(
        egui::pos2(r.center().x, r.center().y - 2.0),
        Align2::CENTER_CENTER,
        label,
        FontId::monospace(9.0),
        if hot || air {
            crate::theme::TEXT
        } else {
            crate::theme::DIM
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, Event, Key, Modifiers, RawInput};

    /// Source synthétique de 1 s : un la 440 Hz, assez d'attaques pour que la
    /// forme d'onde ne soit pas plate.
    fn decoded() -> Decoded {
        let rate = 44_100u32;
        let frames = rate as usize;
        let mut pcm = Vec::with_capacity(frames);
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            pcm.push(0.6 * (std::f32::consts::TAU * 440.0 * t).sin());
        }
        let peaks = std::sync::Arc::new(vec![(-0.6, 0.6); 2400]);
        Decoded {
            pcm: std::sync::Arc::new(pcm),
            channels: 1,
            rate,
            total: 1.0,
            window: None,
            window_passes: None,
            tempo: None,
            envelope: None,
            peaks,
        }
    }

    fn track() -> AudioTrack {
        AudioTrack {
            subsong: 0,
            title: "la 440".into(),
            file: "/tmp/la440.wav".into(),
            length: None,
            loop_start: None,
            loop_end: None,
        }
    }

    fn setup() -> Gtracker {
        let mut g = Gtracker::new();
        let idx = g.add_audio(&track(), decoded(), true);
        g.project.tempo_bpm = 120.0;
        g.project.sources[idx].bpm = 120.0;
        // grille de la source : 120 BPM (comme après l'analyse réelle)
        g.tempos[idx] = Some(Tempo {
            bpm: 120.0,
            beat: 0.5,
            phase: 0.0,
            confidence: 0.9,
        });
        // 1 s à 120 BPM = 2 temps
        let clip = edit::clip_from_slice(g.project.tracks[0].id, idx, 0, 44_100, 4.0, 0.25, 2.0);
        let id = g.project.add_clip(clip);
        g.sel_clip = Some(id);
        g
    }

    fn frame(ctx: &egui::Context, g: &mut Gtracker, events: Vec<Event>) -> Vec<Cmd> {
        // horloge monotone par fil de test : egui calcule ses glissers dessus
        thread_local! {
            static CLOCK: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
        }
        let t = CLOCK.with(|c| {
            let v = c.get() + 0.016;
            c.set(v);
            v
        });
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
            time: Some(t),
            ..Default::default()
        };
        // `i.modifiers` suit `Event::ModifiersChanged`, pas les modificateurs
        // portés par un `Event::Key` : sans cet événement, un ctrl+z simulé
        // n'a pas de ctrl au niveau de l'état d'entrée.
        if let Some(mods) = events.iter().find_map(|e| match e {
            Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        }) {
            input.events.push(Event::ModifiersChanged(mods));
        }
        input.events.extend(events);
        let mut player: Option<Player> = None;
        let mut cmds = Vec::new();
        let mut out = ctx.run_ui(input, |ui| {
            g.ui(ui, &mut player, &mut cmds);
        });
        out.textures_delta.clear();
        // une deuxième passe : egui résout les tailles déclarées (panneaux
        // imbriqués) et les widgets reçoivent leurs rectangles réels
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
            ..Default::default()
        };
        input.time = Some(t + 0.016);
        let mut cmds2 = Vec::new();
        let mut out = ctx.run_ui(input, |ui| {
            g.ui(ui, &mut player, &mut cmds2);
        });
        out.textures_delta.clear();
        cmds.append(&mut cmds2);
        cmds
    }

    /// Une source ajoutée explicitement devient une piste du document ;
    /// un simple rechargement/écoute n'en crée pas.
    #[test]
    fn ajout_explicite_cree_une_piste_une_seule_fois() {
        let mut g = setup();
        assert_eq!(g.project.tracks.len(), 1);
        let mut other = track();
        other.title = "autre".into();
        other.file = "/tmp/autre.wav".into();
        let b = g.add_audio(&other, decoded(), true);
        assert_eq!(g.project.tracks.len(), 2, "une piste pour la source ajoutée");
        let t = g.sel_track.expect("piste sélectionnée");
        assert_eq!(g.project.track(t).unwrap().source, Some(b));
        assert_eq!(g.sel_source, b);
        // redemander la même source ne duplique ni source ni piste
        let again = g.add_audio(&other, decoded(), true);
        assert_eq!(again, b);
        assert_eq!(g.project.tracks.len(), 2);
        // sans demande explicite : pas de piste en plus
        let mut third = track();
        third.title = "trois".into();
        third.file = "/tmp/trois.wav".into();
        let c = g.add_audio(&third, decoded(), false);
        assert_eq!(g.project.tracks.len(), 2);
        assert_eq!(g.project.sources[c].title, "trois");
        // et la piste créée porte bien une tranche quand on en dépose une
        let tid = g.project.tracks[1].id;
        g.project.add_clip(edit::clip_from_slice(tid, b, 0, 1000, 0.0, 0.25, 1.0));
        let plan = g.plan();
        assert!(plan.voices.iter().any(|v| v.start == 0));
    }

    /// La coquille peut changer d'espace et de filtre : le projet (document,
    /// clips, sélection, historique) ne bouge pas d'un iota.
    #[test]
    fn changer_de_panneau_ne_touche_pas_au_document() {
        use crate::shell::{LibraryTab, Workspace};
        let mut g = setup();
        let clips = g.project.clips.len();
        let at = g.project.clips[0].at_beat;
        let name = g.project.name.clone();
        let mut s = crate::shell::Shell::default();
        for w in [Workspace::Edit, Workspace::Arrange, Workspace::Edit] {
            s.set_workspace(w);
            assert_eq!(g.project.clips.len(), clips);
            assert_eq!(g.project.clips[0].at_beat, at);
            assert_eq!(g.project.name, name);
            assert_eq!(g.sel_clip, Some(g.project.clips[0].id));
        }
        for tab in [LibraryTab::Cached, LibraryTab::Catalogue] {
            s.set_library(tab);
            assert_eq!(g.project.clips.len(), clips);
            assert_eq!(g.project.name, name);
        }
        // l'historique reste celui du document, pas de la coquille
        g.checkpoint("test");
        g.project.clips.clear();
        assert!(g.undo().is_some());
        assert_eq!(g.project.clips.len(), clips);
    }

    #[test]
    fn seule_la_maitresse_pose_le_tempo() {
        let mut g = Gtracker::new();
        let a = g.add_audio(&track(), decoded(), true);
        let mut other = track();
        other.title = "autre".into();
        other.file = "/tmp/autre.wav".into();
        let b = g.add_audio(&other, decoded(), false);
        // la piste maîtresse est la première créée (source a)
        assert_eq!(g.project.tracks[0].source, Some(a));
        g.set_analysis(std::path::Path::new("/tmp/autre.wav"), Some(90.0), None);
        assert_eq!(
            g.project.tempo_bpm, 120.0,
            "une piste qui suit ne pose rien"
        );
        g.set_analysis(std::path::Path::new("/tmp/la440.wav"), Some(140.0), None);
        assert_eq!(g.project.tempo_bpm, 140.0, "la maîtresse pose son BPM");
        // et la source b garde son propre tempo
        assert_eq!(g.project.sources[b].bpm, 90.0);
    }

    /// La composition exacte de la coquille (transport en bas, éditeur compact,
    /// centre) doit se rendre sans écran et sans panique. C'est ce chemin que
    /// l'application utilise ; `ui()` n'est plus que la variante de test.
    /// Une tranche s'écoute seule, sans toucher au projet.
    #[test]
    fn une_tranche_s_ecoute_seule() {
        let g = setup();
        let plan = g.audition_plan(0, 11_025, 22_050).expect("plan d'écoute");
        assert_eq!(plan.voices.len(), 1, "une seule tranche dans l'écoute");
        assert_eq!(plan.out_rate, OUT_RATE);
        // 0,25 s à 48 kHz : la tranche garde sa durée réelle (aucun étirement)
        assert!(
            (plan.frames as i64 - 12_000).abs() <= 2,
            "durée du plan : {} trames",
            plan.frames
        );
        // un clic sur la tuile émet bien la commande d'écoute
        let ctx = egui::Context::default();
        let mut g = setup();
        // des tranches courtes : l'écoute doit viser **la** tuile cliquée
        g.clip_beats = 0.5;
        let _ = frame(&ctx, &mut g, Vec::new());
        assert!(g.tiles.len() > 1, "plusieurs tranches affichées");
        let want = g.tiles[1].src_start;
        let c = g.tiles[1].rect.center();
        frame(
            &ctx,
            &mut g,
            vec![
                Event::PointerMoved(c),
                Event::PointerButton {
                    pos: c,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        let cmds = frame(
            &ctx,
            &mut g,
            vec![Event::PointerButton {
                pos: c,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                Cmd::Preview {
                    label: 2,
                    src_start,
                    ..
                } if *src_start == want
            )),
            "la tuile 2 doit demander son écoute, avec sa propre tranche"
        );
    }

    /// Deux musiques sur deux pistes = deux voix qui démarrent ensemble, et le
    /// rendu est exactement leur somme (c'est « les jouer ensemble »).
    #[test]
    fn deux_musiques_jouent_ensemble() {
        use gtracker_core::mixer::render;
        let mut g = setup();
        let mut other = track();
        other.title = "autre".into();
        other.file = "/tmp/autre.wav".into();
        let rate = 44_100u32;
        let n = rate as usize;
        let pcm: Vec<f32> = (0..n)
            .map(|i| 0.5 * (std::f32::consts::TAU * 660.0 * i as f32 / rate as f32).sin())
            .collect();
        let mut d = decoded();
        d.pcm = std::sync::Arc::new(pcm);
        let b = g.add_audio(&other, d, true);
        let tb = g.project.tracks[1].id;
        g.project
            .add_clip(edit::clip_from_slice(tb, b, 0, 44_100, 4.0, 0.25, 2.0));
        let plan = g.plan();
        assert_eq!(plan.voices.len(), 2, "les deux sources sont mixées");
        let start = plan.voices[0].start;
        assert_eq!(plan.voices[0].start, plan.voices[1].start);
        assert_eq!(plan.voices[0].end, plan.voices[1].end);
        assert_eq!(start, 4 * 24_000, "même départ musical");

        let both = render(&plan, plan.frames as usize);
        let solo = |source: usize| {
            let mut p = g.project.clone();
            p.clips.retain(|c| c.source == source);
            let plan = Plan::build(&p, &g.source_audio(), OUT_RATE);
            render(&plan, plan.frames as usize)
        };
        let a = solo(0);
        let bb = solo(b);
        let k = both.len().min(a.len()).min(bb.len());
        assert!(k > 1000);
        let err = (0..k)
            .map(|i| (both[i] - (a[i] + bb[i])).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-5, "le mixage doit être la somme exacte (écart {err})");
    }

    /// Clic sur une séquence de la timeline : elle se joue depuis son début.
    #[test]
    fn clic_sur_une_sequence_la_joue() {
        let ctx = egui::Context::default();
        let mut g = setup();
        let _ = frame(&ctx, &mut g, Vec::new());
        let rect = g.timeline_rect;
        let clip = g.project.clips[0].clone();
        let at = egui::pos2(
            g.beat_to_x(rect, clip.at_beat + 0.25),
            rect.top() + RULER_H + LANE_H * 0.5,
        );
        frame(
            &ctx,
            &mut g,
            vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut g,
            vec![Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(g.playing, "le clic lance la lecture");
        assert!(
            (g.position_beats() - clip.at_beat).abs() < 0.01,
            "la lecture part du début de la séquence : {}",
            g.position_beats()
        );
        assert_eq!(g.sel_clip, Some(clip.id), "la séquence est aussi sélectionnée");
    }

    #[test]
    fn la_coquille_compose_transport_editeur_et_centre() {
        let ctx = egui::Context::default();
        let mut g = setup();
        let mut player: Option<Player> = None;
        let mut cmds = Vec::new();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::Panel::bottom("status")
                .exact_size(22.0)
                .show(ui, |ui| g.status_bar(ui));
            egui::Panel::bottom("transport")
                .exact_size(34.0)
                .show(ui, |ui| g.transport_bar(ui, &mut player));
            egui::Panel::bottom("editor")
                .resizable(true)
                .default_size(230.0)
                .show(ui, |ui| g.source_panel(ui, &mut cmds));
            g.ui_center(ui, &mut player);
        });
        out.textures_delta.clear();
        assert!(g.timeline_rect.width() > 100.0, "timeline mesurée");
        // l'éditeur compact et la timeline coexistent dans la même coquille
        assert!(!g.tiles.is_empty(), "tuiles de l'éditeur présentes");
    }

    #[test]
    fn interface_ne_relance_pas_les_textures() {
        let ctx = egui::Context::default();
        let mut g = setup();
        frame(&ctx, &mut g, Vec::new());
        let built = g.tex.built;
        assert!(built >= 1, "une forme d'onde doit être construite");
        frame(&ctx, &mut g, Vec::new());
        frame(&ctx, &mut g, Vec::new());
        assert_eq!(
            g.tex.built, built,
            "les textures ne doivent pas être refaites à chaque image"
        );
    }

    /// Glisser le corps d'un clip à la souris : le vrai chemin de l'interface
    /// (hit-test, seuil de glisser, aimant, relâchement), pas seulement
    /// `edit::move_clip`.
    #[test]
    fn glisser_un_clip_a_la_souris() {
        let ctx = egui::Context::default();
        let mut g = setup();
        frame(&ctx, &mut g, Vec::new());
        let rect = g.timeline_rect;
        assert!(rect.width() > 100.0, "timeline mesurée");
        let zoom = g.zoom as f64;
        let at = g.project.clips[0].at_beat;
        let x = rect.left() + ((at + 1.0 - g.scroll) * zoom) as f32;
        let y = rect.top() + RULER_H + LANE_H * 0.5;
        let mods = Modifiers::default();
        // appui sur le corps du clip
        frame(
            &ctx,
            &mut g,
            vec![
                Event::PointerMoved(pos2(x, y)),
                Event::PointerButton {
                    pos: pos2(x, y),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: mods,
                },
            ],
        );
        // glisser de deux temps vers la droite, en deux pas (seuil egui)
        let x2 = x + (2.0 * zoom) as f32;
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(x + 12.0, y))]);
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(x2, y))]);
        frame(
            &ctx,
            &mut g,
            vec![Event::PointerButton {
                pos: pos2(x2, y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: mods,
            }],
        );
        let moved = g.project.clips[0].at_beat;
        assert!(
            (moved - (at + 2.0)).abs() < 0.01,
            "clip déplacé de 2 temps : {at} → {moved}"
        );
        assert_eq!(
            g.project.clips[0].length_beats, 2.0,
            "la longueur ne change pas au déplacement"
        );
        // et l'annulation ramène la position d'origine
        let undo = Event::Key {
            key: Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        };
        frame(&ctx, &mut g, vec![undo]);
        assert_eq!(g.project.clips[0].at_beat, at);
    }

    /// Rogner par le bord droit : le contenu suit la longueur musicale.
    #[test]
    fn rogner_par_le_bord_droit() {
        let ctx = egui::Context::default();
        let mut g = setup();
        frame(&ctx, &mut g, Vec::new());
        let rect = g.timeline_rect;
        let zoom = g.zoom as f64;
        let at = g.project.clips[0].at_beat;
        let x_end = rect.left() + ((at + 2.0 - g.scroll) * zoom) as f32;
        let y = rect.top() + RULER_H + LANE_H * 0.5;
        let mods = Modifiers::default();
        let px = x_end - 2.0; // dans la poignée de 6 px
        frame(
            &ctx,
            &mut g,
            vec![
                Event::PointerMoved(pos2(px, y)),
                Event::PointerButton {
                    pos: pos2(px, y),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: mods,
                },
            ],
        );
        let target = rect.left() + ((at + 1.0 - g.scroll) * zoom) as f32;
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(px - 12.0, y))]);
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(target, y))]);
        frame(
            &ctx,
            &mut g,
            vec![Event::PointerButton {
                pos: pos2(target, y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: mods,
            }],
        );
        let c = &g.project.clips[0];
        assert!(
            (c.length_beats - 1.0).abs() < 0.01,
            "longueur après rognage : {}",
            c.length_beats
        );
        assert_eq!(c.src_end - c.src_start, 22_050, "moitié du contenu");
    }

    /// Ouvrir un projet enregistré : les index de source restent stables, les
    /// clips continuent de pointer sur la bonne source après rechargement.
    #[test]
    fn ouvrir_un_projet_recharge_les_bonnes_sources() {
        let mut g = Gtracker::new();
        let a = g.add_audio(&track(), decoded(), true);
        let mut other = track();
        other.title = "autre".into();
        other.file = "/tmp/autre.wav".into();
        let b = g.add_audio(&other, decoded(), false);
        let tid = g.project.tracks[0].id;
        g.project
            .add_clip(edit::clip_from_slice(tid, b, 0, 22_050, 0.0, 0.25, 1.0));
        let dir = std::env::temp_dir().join(format!("gtrack-open-{}", std::process::id()));
        let path = dir.join("ouvert.gtrack");
        g.project.save(&path).expect("save");
        let mut reopened = Gtracker::new();
        let tracks = reopened.open(path.clone()).expect("open");
        assert_eq!(tracks.len(), 2);
        assert_eq!(reopened.project.clips[0].source, b, "index conservé");
        assert_eq!(reopened.project.sources[b].title, "autre");
        // rechargement des PCM dans l'ordre des sources : même index
        for (i, t) in tracks.into_iter().enumerate() {
            let idx = reopened.add_audio(&t, decoded(), false);
            assert_eq!(idx, i, "source {} rechargée au même index", i);
        }
        // la source a reste la première, b la seconde
        assert_eq!(reopened.project.sources[a].title, "la 440");
        assert!(reopened.audio[b].is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Glisser une tuile de l'éditeur vers une lane : le geste central du
    /// GTracker (créer un clip depuis une tranche), sans changer de vue.
    #[test]
    fn glisser_une_tuile_vers_une_piste() {
        let ctx = egui::Context::default();
        let mut g = setup();
        frame(&ctx, &mut g, Vec::new());
        assert!(!g.tiles.is_empty(), "l'éditeur affiche des tuiles");
        let tile = g.tiles[0];
        assert_eq!(tile.source, 0);
        assert!(tile.length_beats > 0.0);
        let rect = g.timeline_rect;
        let lane_y = rect.top() + RULER_H + LANE_H * 0.5;
        let target_beat = 8.0;
        let x = rect.left() + ((target_beat - g.scroll) * g.zoom as f64) as f32;
        let mods = Modifiers::default();
        let c = tile.rect.center();
        frame(
            &ctx,
            &mut g,
            vec![
                Event::PointerMoved(c),
                Event::PointerButton {
                    pos: c,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: mods,
                },
            ],
        );
        frame(
            &ctx,
            &mut g,
            vec![Event::PointerMoved(pos2(c.x + 12.0, c.y))],
        );
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(x, lane_y))]);
        frame(
            &ctx,
            &mut g,
            vec![Event::PointerButton {
                pos: pos2(x, lane_y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: mods,
            }],
        );
        assert_eq!(g.project.clips.len(), 2, "un clip déposé");
        let dropped = g
            .project
            .clips
            .iter()
            .find(|c| (c.at_beat - target_beat).abs() < 0.01)
            .expect("clip à la position déposée");
        assert_eq!(dropped.src_start, tile.src_start);
        assert_eq!(dropped.src_end, tile.src_end);
        assert!(
            (dropped.length_beats - tile.length_beats).abs() < 0.01,
            "la tuile porte sa longueur musicale"
        );
        assert!(dropped.key.is_some(), "tonalité analysée à la création");
        assert_eq!(dropped.track, g.project.tracks[0].id);
    }

    /// Glisser un clip verticalement change de piste (le temps ne bouge pas).
    #[test]
    fn glisser_un_clip_vers_une_autre_piste() {
        let ctx = egui::Context::default();
        let mut g = setup();
        let t2 = g.project.add_track("piste 2", Some(0), 1);
        frame(&ctx, &mut g, Vec::new());
        let rect = g.timeline_rect;
        let at = g.project.clips[0].at_beat;
        let zoom = g.zoom as f64;
        let x = rect.left() + ((at + 1.0 - g.scroll) * zoom) as f32;
        let y1 = rect.top() + RULER_H + LANE_H * 0.5;
        let y2 = rect.top() + RULER_H + LANE_H * 1.5;
        let mods = Modifiers::default();
        frame(
            &ctx,
            &mut g,
            vec![
                Event::PointerMoved(pos2(x, y1)),
                Event::PointerButton {
                    pos: pos2(x, y1),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: mods,
                },
            ],
        );
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(x, y1 + 12.0))]);
        frame(&ctx, &mut g, vec![Event::PointerMoved(pos2(x, y2))]);
        frame(
            &ctx,
            &mut g,
            vec![Event::PointerButton {
                pos: pos2(x, y2),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: mods,
            }],
        );
        let c = &g.project.clips[0];
        assert_eq!(c.track, t2, "clip descendu d'une piste");
        assert_eq!(c.at_beat, at, "le temps ne bouge pas");
    }

    /// Clic sur la règle avant la première lecture : « écouter » doit partir
    /// de là, pas de zéro.
    #[test]
    fn tete_de_lecture_avant_la_premiere_lecture() {
        let mut g = setup();
        g.seek_beats(8.0);
        assert_eq!(g.position_beats(), 8.0);
        let mut player: Option<Player> = None;
        g.play(&mut player);
        assert!(g.playing);
        let t = g.transport.as_ref().expect("transport");
        assert_eq!(
            t.position() as f64 / g.frames_per_beat(),
            8.0,
            "la lecture part de la tête posée"
        );
        // la bibliothèque prend le son : le GTracker s'arrête proprement
        g.halt();
        assert!(!g.playing && g.transport.is_none());
        assert_eq!(g.position_beats(), 0.0);
    }

    /// Suivi de la tête : la vue ne bouge pas tant que la tête est dedans, puis
    /// la replace à 35 % (gauche comme droite, boucle comprise).
    #[test]
    fn suivi_de_la_tete() {
        // dedans : rien à faire
        assert_eq!(follow_scroll(10.0, 0.0, 32.0), None);
        // sortie par la droite : saut de page
        assert_eq!(follow_scroll(30.0, 0.0, 32.0), Some(30.0 - 32.0 * 0.35));
        // sortie par la gauche (retour de boucle)
        assert_eq!(follow_scroll(1.0, 40.0, 32.0), Some(0.0));
        // début du projet : jamais de décalage négatif
        assert_eq!(follow_scroll(0.0, 0.0, 32.0), None);
    }

    /// La découpe d'une source part de **son** début de boucle : une intro
    /// (genh/VGM) ne fabrique pas de tranche, et la dernière s'arrête à la fin
    /// de boucle, fondu exclu.
    #[test]
    fn la_decoupe_part_du_debut_de_boucle() {
        let mut g = setup();
        g.project.sources[0].loop_start = Some(0.4);
        g.project.sources[0].loop_end = Some(1.0);
        let s = g.slices_for(0);
        assert!(!s.is_empty());
        assert!((s[0].0 - 0.4).abs() < 1e-6, "première tranche : {s:?}");
        assert!(
            (s.last().unwrap().1 - 1.0).abs() < 1e-6,
            "dernière tranche : {s:?}"
        );
        // sans boucle, on part de 0 comme avant
        g.project.sources[0].loop_start = None;
        g.project.sources[0].loop_end = None;
        assert_eq!(g.slices_for(0)[0].0, 0.0);
    }

    /// Les tranches de **toutes** les sources sont visibles en même temps, une
    /// source par rangée : plus besoin de choisir un morceau pour en prendre
    /// des tranches (demande user 22/11).
    #[test]
    fn les_sources_sont_empilees() {
        let ctx = egui::Context::default();
        let mut g = setup();
        let mut second = track();
        second.title = "seconde".into();
        // fichier distinct : `add_source` dédoublonne sur (chemin, sous-piste)
        second.file = "/tmp/seconde.wav".into();
        g.add_audio(&second, decoded(), false);
        frame(&ctx, &mut g, Vec::new());
        frame(&ctx, &mut g, Vec::new());
        let mut seen: Vec<usize> = g.tiles.iter().map(|t| t.source).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen, vec![0, 1], "chaque source a sa rangée de tranches");
        let top = |src: usize| {
            g.tiles
                .iter()
                .filter(|t| t.source == src)
                .map(|t| t.rect.top())
                .fold(f32::INFINITY, f32::min)
        };
        assert!(
            top(1) > top(0),
            "la seconde source est dessinée sous la première"
        );
    }

    /// Le clip sous la tête de lecture est désigné pendant la lecture (et
    /// seulement pendant) : c'est lui que la timeline allume.
    #[test]
    fn le_clip_qui_sonne_est_designe() {
        let mut g = setup();
        let want = g.project.clips[0].id;
        assert!(g.sounding_clip().is_none(), "à l'arrêt, rien ne sonne");
        let mut player: Option<Player> = None;
        g.seek_beats(4.5);
        g.play(&mut player);
        assert!(g.playing);
        assert_eq!(g.sounding_clip().map(|c| c.id), Some(want));
        // hors du clip (le projet commence à la mesure 2), rien ne sonne
        g.seek_beats(0.0);
        assert!(g.sounding_clip().is_none());
        g.silence();
        assert!(g.sounding_clip().is_none(), "arrêté, plus rien ne sonne");
    }

    /// « Arrêter » depuis l'inspecteur coupe le projet **sans** perdre la tête
    /// de lecture : `halt` la remet à zéro, `silence` non (vécu 21/11).
    #[test]
    fn arret_doux_garde_la_tete_de_lecture() {
        let mut g = setup();
        g.seek_beats(8.0);
        let mut player: Option<Player> = None;
        g.play(&mut player);
        assert!(g.playing);
        g.silence();
        assert!(!g.playing);
        assert!(g.transport.is_some(), "le transport reste en place");
        assert_eq!(g.position_beats(), 8.0, "la tête ne bouge pas");
    }

    #[test]
    fn supprimer_puis_annuler_au_clavier() {
        let ctx = egui::Context::default();
        let mut g = setup();
        frame(&ctx, &mut g, Vec::new());
        assert_eq!(g.project.clips.len(), 1);
        let del = Event::Key {
            key: Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        };
        frame(&ctx, &mut g, vec![del]);
        assert!(
            g.project.clips.is_empty(),
            "suppr retire le clip sélectionné"
        );
        let undo = Event::Key {
            key: Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        };
        frame(&ctx, &mut g, vec![undo]);
        assert_eq!(g.project.clips.len(), 1, "ctrl+z remet le clip");
    }

    #[test]
    fn tranche_porte_sa_propre_tonalite() {
        let d = decoded();
        let k = slice_key(&d, 0, 44_100).expect("tonalité de la tranche");
        assert_eq!(k.tonic, 9, "un la reste la (classe 9)");
        // trop court : pas d'analyse hasardeuse
        assert!(slice_key(&d, 0, 100).is_none());
        assert!(slice_key(&d, 44_000, 50_000).is_none(), "hors du buffer");
    }

    #[test]
    fn edition_change_le_plan_joue() {
        let mut g = setup();
        let before = g.plan().frames;
        g.checkpoint("déplacer");
        let id = g.project.clips[0].id;
        edit::move_clip(g.project.clip_mut(id).unwrap(), 8.0, 0.25);
        let after = g.plan().frames;
        assert_eq!(after - before, (4.0 * g.frames_per_beat()).round() as u64);
    }

    #[test]
    fn tranche_deposee_porte_sa_duree_musicale() {
        let mut g = setup();
        let track_id = g.project.tracks[0].id;
        g.checkpoint("déposer");
        let clip = edit::clip_from_slice(track_id, 0, 0, 22_050, 2.0, 0.25, 1.0);
        let id = g.project.add_clip(clip);
        assert_eq!(g.project.clip(id).unwrap().length_beats, 1.0);
        // le plan suit : 1 temps de contenu à 120 BPM = 0,5 s
        let plan = g.plan();
        let fpb = g.frames_per_beat();
        let v = plan
            .voices
            .iter()
            .find(|v| v.start == (2.0 * fpb).round() as u64)
            .expect("voix de la tranche déposée");
        let _ = id;
        assert!((v.end - v.start) as f64 - fpb < 2.0);
    }
}
