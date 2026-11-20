//! RetroPlayer — GUI egui : navigation joshw.info, lecture rodio, export MP3/FLAC.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use eframe::egui::{
    self, Align, CornerRadius, FontFamily, FontId, Layout, RichText, Sense, Stroke, TextStyle,
};
use retroplayer_core::archive::Archive;
use retroplayer_core::convert::{Format, Tags};
use retroplayer_core::db::Db;
use retroplayer_core::decode::{self, Decoded};
use retroplayer_core::model::{Entry, LoopWindow, Track};
use retroplayer_core::player::Player;
use retroplayer_core::{joshw, util};

fn main() -> eframe::Result<()> {
    // icône de la fenêtre (barre des tâches) : pixel-art embarquée,
    // générée par assets/make-icon.py
    let icon = egui::IconData {
        rgba: include_bytes!("../../../assets/icon.rgba").to_vec(),
        width: 128,
        height: 128,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([880.0, 520.0])
            .with_icon(icon)
            // app_id Wayland = nom du `.desktop` : sans lui KWin ne rattache pas
            // la fenêtre et affiche l'icône générique (même piège que glauncher)
            .with_app_id("retroplayer")
            .with_title("RetroPlayer"),
        ..Default::default()
    };
    eframe::run_native(
        "RetroPlayer",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

// ---------------------------------------------------------------- jobs

enum Job {
    Open(Entry),
    Play { entry: Entry, ordinal: usize },
    Convert { entry: Entry, ordinal: usize, out: PathBuf, format: Format, tags: Tags },
    Delete(i64),
    Update,
}

enum Msg {
    Status(String),
    Progress(f32),
    Tracks(Vec<Track>, Option<String>, Option<String>),
    Playing(Vec<Track>, usize, Decoded, Option<String>, Option<String>),
    /// Durée / fenêtre de boucle mesurées pour une piste (entry, ordinal).
    Timing(i64, usize, Option<f64>, Option<(f64, f64)>),
    Converted(PathBuf),
    Deleted(i64, u64),
    ReloadDb,
    Error(String),
}

// ---------------------------------------------------------------- état

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Catalogue,
    Cached,
}

struct CachedRow {
    entry_idx: Option<usize>,
    entry_id: i64,
    bytes: u64,
    tracks: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Name,
    Console,
    Date,
}

impl SortBy {
    fn label(self) -> &'static str {
        match self {
            SortBy::Name => "tri : nom",
            SortBy::Console => "tri : console",
            SortBy::Date => "tri : date",
        }
    }
}

struct RowMeta {
    title: String,
    sort_key: String,
    console: &'static str,
    date: Option<String>,
    tags: String,
}

struct App {
    db: Db,
    entries: Vec<Entry>,
    rows: Vec<RowMeta>,
    consoles: Vec<&'static str>,
    filtered: Vec<usize>,

    query: String,
    last_query: String,
    console_filter: Option<&'static str>,
    sort: SortBy,
    last_console: Option<&'static str>,
    last_sort: SortBy,

    selected: Option<usize>,
    tracks: Vec<Track>,
    track_ordinal: Option<usize>,
    composer: Option<String>,
    album: Option<String>,

    player: Option<Player>,
    looping: bool,
    volume: f32,
    ab_a: Option<f64>,
    ab_b: Option<f64>,
    ab_msg: Option<String>,

    status: String,
    busy: bool,
    progress: f32,

    job_tx: Sender<Job>,
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,

    out_dir: String,
    format: Format,
    last_export: Option<String>,

    styled: bool,
    focus_search: bool,
    window_title: String,

    view: View,
    cached: Vec<CachedRow>,
    pending_delete: Option<i64>,
    scroll_to_selected: bool,
    probe_cancel: Arc<AtomicBool>,
}

impl App {
    fn new() -> Self {
        let initial_query = std::env::args().skip(1).find(|a| !a.starts_with('-')).unwrap_or_default();
        let (job_tx, msg_tx, msg_rx) = spawn_worker();
        let (db, entries, status) = match Db::open(&util::db_path()) {
            Ok(db) => {
                let entries = db.all_entries().unwrap_or_default();
                let n = entries.len();
                let status = if n == 0 {
                    "Base vide : clique sur « Mettre à jour »".to_string()
                } else {
                    format!("{n} archives en base")
                };
                (db, entries, status)
            }
            Err(e) => {
                let db = Db::open(&util::db_path()).expect("base");
                (db, Vec::new(), format!("Erreur base : {e}"))
            }
        };
        let (rows, consoles) = build_meta(&entries);
        let filtered = (0..entries.len()).collect();
        let out_dir = std::env::var("HOME")
            .map(|h| format!("{h}/Musiques/RetroPlayer"))
            .unwrap_or_else(|_| ".".into());
        let mut app = Self {
            db,
            entries,
            rows,
            consoles,
            filtered,
            query: initial_query,
            last_query: String::new(),
            console_filter: None,
            sort: SortBy::Name,
            last_console: None,
            last_sort: SortBy::Name,
            selected: None,
            tracks: Vec::new(),
            track_ordinal: None,
            composer: None,
            album: None,
            player: Player::new().ok(),
            looping: false,
            volume: 1.0,
            ab_a: None,
            ab_b: None,
            ab_msg: None,
            status,
            busy: false,
            progress: 0.0,
            job_tx,
            msg_tx,
            msg_rx,
            out_dir,
            format: Format::Mp3,
            last_export: None,
            styled: false,
            focus_search: false,
            window_title: String::new(),
            view: View::Catalogue,
            cached: Vec::new(),
            pending_delete: None,
            scroll_to_selected: false,
            probe_cancel: Arc::new(AtomicBool::new(false)),
        };
        app.refresh_cached();
        app
    }

    fn refresh_cached(&mut self) {
        let list = retroplayer_core::archive::cached(&self.db).unwrap_or_default();
        let map: HashMap<i64, usize> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        self.cached = list
            .into_iter()
            .map(|c| CachedRow {
                entry_idx: map.get(&c.entry_id).copied(),
                entry_id: c.entry_id,
                bytes: c.bytes,
                tracks: c.tracks,
            })
            .collect();
    }

    fn is_cached(&self, entry_id: i64) -> Option<&CachedRow> {
        self.cached.iter().find(|c| c.entry_id == entry_id)
    }

    fn refilter(&mut self) {
        let words: Vec<String> = self
            .query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let mut idx: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| self.console_filter.map_or(true, |c| r.console == c))
            .filter(|(_, r)| words.iter().all(|w| r.sort_key.contains(w.as_str())))
            .map(|(i, _)| i)
            .collect();
        match self.sort {
            SortBy::Name => idx.sort_by(|a, b| self.rows[*a].sort_key.cmp(&self.rows[*b].sort_key)),
            SortBy::Console => idx.sort_by(|a, b| {
                let (ra, rb) = (&self.rows[*a], &self.rows[*b]);
                (ra.console, &ra.sort_key).cmp(&(rb.console, &rb.sort_key))
            }),
            SortBy::Date => idx.sort_by(|a, b| {
                let (ra, rb) = (&self.rows[*a], &self.rows[*b]);
                rb.date.cmp(&ra.date).then_with(|| ra.sort_key.cmp(&rb.sort_key))
            }),
        }
        self.filtered = idx;
        self.last_query = self.query.clone();
        self.last_console = self.console_filter;
        self.last_sort = self.sort;
    }

    fn reload(&mut self) {
        self.entries = self.db.all_entries().unwrap_or_default();
        let (rows, consoles) = build_meta(&self.entries);
        self.rows = rows;
        self.consoles = consoles;
        self.refilter();
    }

    fn pump(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                Msg::Status(s) => self.status = s,
                Msg::Progress(p) => self.progress = p,
                Msg::Tracks(tracks, composer, album) => {
                    self.refresh_cached();
                    let need = tracks
                        .iter()
                        .any(|t| t.length.is_none() || t.loop_end.is_none());
                    self.tracks = tracks;
                    self.composer = composer;
                    self.album = album;
                    self.track_ordinal = None;
                    self.busy = false;
                    self.progress = 1.0;
                    if need {
                        self.start_probe();
                    }
                }
                Msg::Timing(entry_id, ordinal, length, window) => {
                    if self.selected_entry().map(|e| e.id) == Some(entry_id) {
                        if let Some(t) = self.tracks.get_mut(ordinal) {
                            if length.is_some() {
                                t.length = length;
                            }
                            t.loop_start = window.map(|(s, _)| s);
                            t.loop_end = window.map(|(_, e)| e);
                        }
                    }
                }
                Msg::Playing(tracks, ordinal, decoded, composer, album) => {
                    self.refresh_cached();
                    self.tracks = tracks;
                    self.composer = composer;
                    self.album = album;
                    self.track_ordinal = Some(ordinal);
                    self.busy = false;
                    self.progress = 1.0;
                    self.ab_a = None;
                    self.ab_b = None;
                    self.ab_msg = None;
                    let stored = self
                        .selected
                        .and_then(|i| self.entries.get(i))
                        .and_then(|e| self.db.loop_window(e.id, ordinal).ok().flatten())
                        // une boucle « piste entière » n'est pas un choix A/B :
                        // si le rendu contient plusieurs tours, la période
                        // détectée dedans est plus juste
                        .filter(|w| {
                            !(decoded.window.is_some()
                                && w.start < 0.5
                                && (w.end - decoded.total).abs() <= 1.0)
                        });
                    // boucle par défaut : A/B enregistrée > boucle du format > longueur
                    // du .m3u (NSF/SPC/GBS rendent 3 min par défaut, la vraie piste
                    // dure ce que dit le .m3u) > fichier entier
                    let m3u_len = self
                        .tracks
                        .get(ordinal)
                        .and_then(|t| t.length)
                        .filter(|l| *l > 2.0 && *l < decoded.total - 0.5);
                    // boucle annoncée par zxtune : fin de la partie qui boucle,
                    // le fondu qui suit n'est donc jamais rejoué
                    // zxtune donne la LONGUEUR de boucle (pas son point de départ) :
                    // valable pour les formats qui bouclent depuis 0 ; pour le VGM le
                    // point de départ vient de l'en-tête, traité juste au-dessus.
                    let fmt_loop = self
                        .tracks
                        .get(ordinal)
                        .and_then(|t| {
                            let end = t.loop_end?;
                            Some((t.loop_start.unwrap_or(0.0), end))
                        })
                        .filter(|(s, e)| *e > *s + 1.0 && *e > 2.0);
                    let (w, origin) = if let Some(st) = stored {
                        (st, "boucle A/B enregistrée".to_string())
                    } else if let Some((a, b)) = decoded.window {
                        let origin = match decoded.window_passes {
                            Some(n) => format!("boucle détectée dans le rendu ({n} tours)"),
                            None => "boucle du format".to_string(),
                        };
                        (LoopWindow { start: a, end: b }, origin)
                    } else if let Some((start, end)) = fmt_loop {
                        let origin = if start > 0.5 {
                            "boucle du format"
                        } else {
                            "boucle du format (hors fondu)"
                        };
                        (LoopWindow { start, end }, origin.to_string())
                    } else if let Some(len) = m3u_len {
                        (
                            LoopWindow {
                                start: 0.0,
                                end: len,
                            },
                            "longueur du .m3u".to_string(),
                        )
                    } else {
                        (
                            LoopWindow {
                                start: 0.0,
                                end: decoded.total,
                            },
                            "fichier entier".to_string(),
                        )
                    };
                    let looping = self.looping;
                    let volume = self.volume;
                    match self.player.as_mut() {
                        Some(player) => {
                            player.load(
                                decoded.pcm,
                                decoded.channels,
                                decoded.rate,
                                decoded.total,
                                w,
                                looping,
                            );
                            player.set_volume(volume);
                            self.status = format!("Lecture — {origin}");
                        }
                        None => self.status = "Sortie audio indisponible".into(),
                    }
                }
                Msg::Deleted(id, bytes) => {
                    if self.selected_entry().map(|e| e.id) == Some(id) {
                        self.tracks.clear();
                        self.track_ordinal = None;
                        self.composer = None;
                        self.album = None;
                    }
                    self.status = format!("{} libérés (archive #{id})", fmt_size(bytes));
                    self.refresh_cached();
                    self.pending_delete = None;
                }
                Msg::Converted(p) => {
                    self.busy = false;
                    self.progress = 1.0;
                    self.last_export = Some(p.display().to_string());
                    self.status = format!("Exporté : {}", p.display());
                }
                Msg::ReloadDb => {
                    self.reload();
                    self.busy = false;
                    self.progress = 1.0;
                    self.status = format!("Base à jour : {} archives", self.entries.len());
                }
                Msg::Error(e) => {
                    self.busy = false;
                    self.status = format!("Erreur : {e}");
                }
            }
        }
    }

    /// Mesure en tâche de fond les durées que les `.m3u` ne donnent pas
    /// (PSF, XA, sets sans playlist) : une sonde zxtune par piste, résultat
    /// écrit en base pour ne pas recommencer.
    fn start_probe(&mut self) {
        let Some(entry) = self.selected_entry().map(|e| (e.id, e.name.clone())) else {
            return;
        };
        let tracks = self.tracks.clone();
        if tracks.is_empty() {
            return;
        }
        self.probe_cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.probe_cancel.clone();
        let tx = self.msg_tx.clone();
        let db_path = util::db_path();
        let zxtune = retroplayer_core::zxtune_path();
        std::thread::spawn(move || {
            let Ok(db) = Db::open(&db_path) else {
                return;
            };
            for (i, t) in tracks.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if t.length.is_some() && t.loop_end.is_some() {
                    continue;
                }
                match decode::probe(&zxtune, t) {
                    Ok(p) => {
                        let length = t.length.or(Some(p.total));
                        // (0, 0) = mesuré, aucune boucle distincte du fichier
                        let window = p.window.unwrap_or((0.0, 0.0));
                        let stored = Some(window).filter(|(_, e)| *e > 0.0);
                        let _ = db.set_track_timing(
                            entry.0,
                            i,
                            length,
                            stored.map(|(s, _)| s),
                            stored.map(|(_, e)| e),
                        );
                        let _ = tx.send(Msg::Timing(entry.0, i, length, stored));
                    }
                    // piste que zxtune ne sait pas lire : on note « mesuré »
                    // pour ne pas relancer la sonde à chaque ouverture
                    Err(_) => {
                        let _ = db.set_track_timing(
                            entry.0,
                            i,
                            None,
                            Some(0.0),
                            Some(0.0),
                        );
                    }
                }
            }
            let _ = tx.send(Msg::Status(format!(
                "durées mesurées — {}",
                entry.1.trim_end_matches(".7z")
            )));
        });
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    fn open_archive(&mut self, ei: usize) {
        let Some(entry) = self.entries.get(ei).cloned() else {
            return;
        };
        self.probe_cancel.store(true, Ordering::Relaxed);
        self.tracks.clear();
        self.track_ordinal = None;
        self.busy = true;
        self.progress = 0.0;
        self.status = "Ouverture de l'archive…".into();
        let _ = self.job_tx.send(Job::Open(entry));
    }

    fn play_track(&mut self, ordinal: usize) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        self.busy = true;
        self.progress = 0.0;
        self.status = "Préparation…".into();
        let _ = self.job_tx.send(Job::Play { entry, ordinal });
    }

    // ------------------------------------------------------------ UI

    fn style(&mut self, ctx: &egui::Context) {
        if self.styled {
            return;
        }
        self.styled = true;
        ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            // 0.6 par défaut : trop pâle pour les petites tailles, surtout en sombre
            s.visuals.weak_text_alpha = 0.78;
            s.spacing.button_padding = egui::vec2(9.0, 4.0);
            s.spacing.interact_size.y = 22.0;
            s.text_styles.insert(TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Button, FontId::new(13.5, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        });
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (space, left, right, l, r) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::L),
                i.key_pressed(egui::Key::R),
            )
        });
        let Some(player) = self.player.as_mut() else {
            return;
        };
        if space {
            player.toggle_pause();
        }
        if left {
            let t = (player.position() - 5.0).max(0.0);
            player.seek(t);
        }
        if right {
            let t = (player.position() + 5.0).min(player.total());
            player.seek(t);
        }
        if l {
            let on = !player.looping();
            player.set_looping(on);
            self.looping = on;
        }
        if r {
            player.seek(0.0);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.view == View::Catalogue, RichText::new("Catalogue").size(13.5))
                .clicked()
            {
                self.view = View::Catalogue;
            }
            if ui
                .selectable_label(
                    self.view == View::Cached,
                    RichText::new(format!("Téléchargés {}", self.cached.len())).size(13.5),
                )
                .on_hover_text("archives déjà extraites, conservées dans ~/.cache/retroplayer")
                .clicked()
            {
                self.view = View::Cached;
                self.refresh_cached();
            }
            ui.separator();
            match self.view {
                View::Catalogue => {
                    let w = (ui.available_width() - 250.0).max(160.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("rechercher un jeu, une console, une année…")
                            .desired_width(w),
                    );
                    if self.focus_search {
                        resp.request_focus();
                        self.focus_search = false;
                    }
                    if !self.query.is_empty()
                        && ui.button("✕").on_hover_text("effacer").clicked()
                    {
                        self.query.clear();
                    }
                    ui.label(
                        RichText::new(format!("{} / {}", self.filtered.len(), self.entries.len()))
                            .weak()
                            .small(),
                    );
                }
                View::Cached => {
                    ui.label(
                        RichText::new("archives extraites, gardées d'un lancement à l'autre")
                            .weak()
                            .small(),
                    );
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let btn = ui.add_enabled(!self.busy, egui::Button::new("Mettre à jour"));
                if btn.clicked() {
                    self.busy = true;
                    self.progress = 0.0;
                    let _ = self.job_tx.send(Job::Update);
                }
                btn.on_hover_text("réinterroge les 24 sites joshw.info (1–2 min)");
            });
        });

        ui.horizontal(|ui| {
            match self.view {
                View::Catalogue => {
                    egui::ComboBox::from_id_salt("console")
                        .selected_text(match self.console_filter {
                            Some(c) => format!("console : {c}"),
                            None => "toutes les consoles".to_string(),
                        })
                        .width(190.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    self.console_filter.is_none(),
                                    "toutes les consoles",
                                )
                                .clicked()
                            {
                                self.console_filter = None;
                            }
                            ui.separator();
                            for c in &self.consoles {
                                if ui
                                    .selectable_label(self.console_filter == Some(*c), *c)
                                    .clicked()
                                {
                                    self.console_filter = Some(*c);
                                }
                            }
                        });
                    egui::ComboBox::from_id_salt("tri")
                        .selected_text(self.sort.label())
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for (s, label) in [
                                (SortBy::Name, "nom (A→Z)"),
                                (SortBy::Console, "console puis nom"),
                                (SortBy::Date, "date (récent d'abord)"),
                            ] {
                                if ui.selectable_label(self.sort == s, label).clicked() {
                                    self.sort = s;
                                }
                            }
                        });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} consoles", self.consoles.len()))
                                .weak()
                                .small(),
                        );
                    });
                }
                View::Cached => {
                    let total: u64 = self.cached.iter().map(|c| c.bytes).sum();
                    let tracks: usize = self.cached.iter().map(|c| c.tracks).sum();
                    ui.label(
                        RichText::new(format!(
                            "{} archives · {} · {tracks} pistes",
                            self.cached.len(),
                            fmt_size(total)
                        ))
                        .small()
                        .weak(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("rafraîchir").clicked() {
                            self.refresh_cached();
                        }
                    });
                }
            }
        });
        ui.add_space(2.0);
    }

    fn entries_panel(&mut self, ui: &mut egui::Ui) {
        let weak = ui.visuals().weak_text_color();
        let strong = ui.visuals().text_color();
        let accent = ui.visuals().selection.bg_fill;
        let on_sel = ui.visuals().selection.stroke.color;
        let row_h = 21.0;
        let n = self.filtered.len();
        let mut sa = egui::ScrollArea::vertical()
            .id_salt("entries")
            .auto_shrink([false, false]);
        if self.scroll_to_selected {
            if let Some(ei) = self.selected {
                if let Some(pos) = self.filtered.iter().position(|i| *i == ei) {
                    let view = ui.available_height().max(60.0);
                    sa = sa.vertical_scroll_offset((pos as f32 * row_h - view * 0.35).max(0.0));
                    self.scroll_to_selected = false;
                }
            }
        }
        sa.show_rows(ui, row_h, n, |ui, range| {
                let budget = ((ui.available_width() - 76.0) / 7.1).max(12.0) as usize;
                for idx in range {
                    let ei = self.filtered[idx];
                    let entry = &self.entries[ei];
                    let row = &self.rows[ei];
                    let selected = self.selected == Some(ei);

                    // priorité au titre : les tags ne s'affichent que si le titre
                    // garde au moins une vingtaine de caractères
                    let mut tags = if row.tags.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", row.tags)
                    };
                    let mut title = truncate_chars(&row.title, budget);
                    let tlen = tags.chars().count();
                    if title.chars().count() + tlen > budget {
                        let room = budget.saturating_sub(tlen);
                        if room >= 22 {
                            title = truncate_chars(&row.title, room);
                        } else {
                            tags.clear();
                        }
                    }
                    let cached = self.is_cached(entry.id).is_some();
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = f32::INFINITY;
                    job.append(
                        if cached { "• " } else { "  " },
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(11.0, FontFamily::Proportional),
                            color: if cached { accent } else { weak },
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    job.append(
                        &format!("{:<7}", row.console),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(11.0, FontFamily::Monospace),
                            color: if selected { on_sel } else { weak },
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    job.append(
                        &title,
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(13.0, FontFamily::Proportional),
                            color: if selected { on_sel } else { strong },
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    if !tags.is_empty() {
                        job.append(
                            &tags,
                            0.0,
                            egui::TextFormat {
                                font_id: FontId::new(11.5, FontFamily::Proportional),
                                color: if selected { accent } else { weak },
                                valign: egui::Align::Center,
                                ..Default::default()
                            },
                        );
                    }

                    let resp = ui
                        .selectable_label(selected, job)
                        .on_hover_text(format!(
                            "{}\n{}{}\n{}{}",
                            row.title,
                            row.console,
                            row.date
                                .as_ref()
                                .map(|d| format!(" · {d}"))
                                .unwrap_or_default(),
                            if row.tags.is_empty() {
                                String::new()
                            } else {
                                format!("({})\n", row.tags)
                            },
                            format!("{}/{}", entry.site.trim_start_matches("https://"), entry.folder)
                        ));
                    if resp.clicked() {
                        self.selected = Some(ei);
                    }
                    if resp.double_clicked() {
                        self.selected = Some(ei);
                        self.open_archive(ei);
                    }
                }
            });
    }

    fn cached_panel(&mut self, ui: &mut egui::Ui) {
        if self.cached.is_empty() {
            ui.add_space(14.0);
            ui.label(
                RichText::new(
                    "Aucune archive téléchargée.\n\n\
                     Double-clique une archive dans Catalogue :\n\
                     elle est extraite puis conservée ici, sans retéléchargement.",
                )
                .weak(),
            );
            return;
        }

        let mut rows: Vec<(i64, Option<usize>, String, String, u64, usize)> = self
            .cached
            .iter()
            .map(|c| match c.entry_idx {
                Some(i) => (
                    c.entry_id,
                    Some(i),
                    self.rows[i].console.to_string(),
                    if self.rows[i].tags.is_empty() {
                        self.rows[i].title.clone()
                    } else {
                        format!("{} [{}]", self.rows[i].title, self.rows[i].tags)
                    },
                    c.bytes,
                    c.tracks,
                ),
                None => (
                    c.entry_id,
                    None,
                    "?".to_string(),
                    format!("archive #{}, absente du catalogue", c.entry_id),
                    c.bytes,
                    c.tracks,
                ),
            })
            .collect();
        rows.sort_by(|a, b| (&a.2, &a.3).cmp(&(&b.2, &b.3)));

        let mut open = None;
        let mut delete = None;
        let mut pending = self.pending_delete;
        let count = rows.len();
        egui::ScrollArea::vertical()
            .id_salt("cached")
            .auto_shrink([false, false])
            .show_rows(ui, 40.0, count, |ui, range| {
                for i in range {
                    let (id, idx, console, title, bytes, tracks) = &rows[i];
                    let budget = ((ui.available_width() - 120.0) / 7.1).max(14.0) as usize;
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    false,
                                    RichText::new(truncate_chars(title, budget)).size(13.0),
                                )
                                .on_hover_text(title)
                                .clicked()
                            {
                                open = *idx;
                            }
                            ui.label(
                                RichText::new(format!(
                                    "{console} · {} · {tracks} pistes",
                                    fmt_size(*bytes)
                                ))
                                .small()
                                .weak(),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let confirming = pending == Some(*id);
                            let label = if confirming { "confirmer" } else { "supprimer" };
                            if ui
                                .small_button(label)
                                .on_hover_text(
                                    "supprime l'archive extraite et ses WAV décodés\n\
                                     (elle sera retéléchargée à la prochaine ouverture)",
                                )
                                .clicked()
                            {
                                if confirming {
                                    delete = Some(*id);
                                } else {
                                    pending = Some(*id);
                                }
                            }
                        });
                    });
                }
            });
        self.pending_delete = pending;
        if let Some(i) = open {
            self.selected = Some(i);
            self.view = View::Catalogue;
            self.scroll_to_selected = true;
            self.open_archive(i);
        }
        if let Some(id) = delete {
            self.status = "Suppression du cache…".into();
            let _ = self.job_tx.send(Job::Delete(id));
        }
    }

    fn tracks_panel(&mut self, ui: &mut egui::Ui) {
        let weak = ui.visuals().weak_text_color();
        let strong = ui.visuals().text_color();
        let on_sel = ui.visuals().selection.stroke.color;

        ui.add_space(2.0);
        if let Some(ei) = self.selected {
            ui.label(RichText::new(&self.rows[ei].title).strong().size(15.5));
            ui.label(RichText::new(self.entries[ei].display_name()).weak().small());
        } else {
            ui.label(RichText::new("Pistes").strong().size(15.5));
        }
        ui.separator();

        if self.tracks.is_empty() {
            ui.add_space(16.0);
            ui.label(
                RichText::new(
                    "Double-clic sur une archive à gauche :\ntéléchargement, extraction, puis liste des pistes.\n\n\
                     Un clic sur une piste lance la lecture (zxtune décode, rodio joue).",
                )
                .weak(),
            );
            return;
        }

        let count = self.tracks.len();
        let mut to_play = None;
        egui::ScrollArea::vertical()
            .id_salt("tracks")
            .auto_shrink([false, false])
            .show_rows(ui, 21.0, count, |ui, range| {
                let max_chars = ((ui.available_width() - 110.0) / 7.1).max(12.0) as usize;
                for i in range {
                    let t = &self.tracks[i];
                    let playing = self.track_ordinal == Some(i);
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = f32::INFINITY;
                    job.append(
                        if playing { "▶ " } else { "  " },
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(11.0, FontFamily::Monospace),
                            color: on_sel,
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    job.append(
                        &format!("{:>2}. ", i + 1),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(11.0, FontFamily::Monospace),
                            color: if playing { on_sel } else { weak },
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    job.append(
                        &format!(
                            "{:>6}  ",
                            t.length.map(util::fmt_time).unwrap_or_else(|| "  —  ".into())
                        ),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(11.0, FontFamily::Monospace),
                            color: if playing { on_sel } else { weak },
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    job.append(
                        &truncate_chars(&t.title, max_chars),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(13.0, FontFamily::Proportional),
                            color: if playing { on_sel } else { strong },
                            valign: egui::Align::Center,
                            ..Default::default()
                        },
                    );
                    if ui.selectable_label(playing, job).clicked() {
                        to_play = Some(i);
                    }
                }
            });
        if let Some(i) = to_play {
            self.play_track(i);
        }
    }

    fn details_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.label(RichText::new("Détails").strong().size(15.5));
        ui.separator();

        if let Some(ei) = self.selected {
            let title = self.rows[ei].title.clone();
            let console = self.rows[ei].console;
            let date = self.rows[ei].date.clone();
            let tags = self.rows[ei].tags.clone();
            let name = self.entries[ei].display_name();
            let url = self.entries[ei].url();
            let loc = format!(
                "{}/{}",
                self.entries[ei].site.trim_start_matches("https://"),
                self.entries[ei].folder
            );
            let entry_id = self.entries[ei].id;
            let cached = self.is_cached(entry_id).map(|c| (c.bytes, c.tracks));

            ui.label(RichText::new(title).strong());
            ui.label(
                RichText::new(format!(
                    "{console}{}",
                    date.map(|d| format!(" · {d}")).unwrap_or_default()
                ))
                .weak()
                .small(),
            );
            if !tags.is_empty() {
                ui.label(RichText::new(format!("({tags})")).weak().small());
            }
            if let Some(c) = &self.composer {
                ui.label(RichText::new(format!("- {c}")).weak().small());
            }
            if let Some(a) = &self.album {
                ui.label(RichText::new(format!("album : {a}")).weak().small());
            }
            ui.add_space(4.0);
            let char_w = 6.6;
            let chars = ((ui.available_width() - 4.0) / char_w).max(16.0) as usize;
            ui.label(RichText::new(truncate_chars(&name, chars)).weak().small())
                .on_hover_text(&name);
            ui.label(RichText::new(truncate_chars(&loc, chars)).weak().small())
                .on_hover_text(&url);
            ui.add_space(4.0);

            match cached {
                Some((bytes, tracks)) => {
                    ui.label(
                        RichText::new(format!(
                            "téléchargé · {} · {tracks} pistes",
                            fmt_size(bytes)
                        ))
                        .small(),
                    );
                    let confirming = self.pending_delete == Some(entry_id);
                    let label = if confirming {
                        "confirmer la suppression"
                    } else {
                        "supprimer du cache"
                    };
                    if ui.small_button(label).clicked() {
                        if confirming {
                            self.status = "Suppression du cache…".into();
                            let _ = self.job_tx.send(Job::Delete(entry_id));
                        } else {
                            self.pending_delete = Some(entry_id);
                        }
                    }
                }
                None => {
                    ui.label(RichText::new("pas encore téléchargé").weak().small());
                }
            }
        } else {
            ui.label(RichText::new("Sélectionne une archive").weak().small());
        }

        ui.separator();
        ui.label(RichText::new("Export").strong());
        ui.add_space(2.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.out_dir)
                .hint_text("dossier de sortie")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if ui.selectable_label(self.format == Format::Mp3, "MP3").clicked() {
                self.format = Format::Mp3;
            }
            if ui.selectable_label(self.format == Format::Flac, "FLAC").clicked() {
                self.format = Format::Flac;
            }
        });
        let can = !self.busy && self.track_ordinal.is_some();
        let label = match self.format {
            Format::Mp3 => "exporter la piste en MP3",
            Format::Flac => "exporter la piste en FLAC",
        };
        if ui
            .add_enabled(can, egui::Button::new(label).min_size(egui::vec2(0.0, 26.0)))
            .on_hover_text("zxtune encode, les tags (titre/compositeur/album) sont écrits ensuite")
            .clicked()
        {
            if let (Some(entry), Some(ord)) = (self.selected_entry().cloned(), self.track_ordinal) {
                let track = self.tracks[ord].clone();
                let tags = Tags {
                    title: Some(track.title.clone()),
                    artist: self.composer.clone(),
                    album: self.album.clone(),
                };
                self.busy = true;
                self.progress = 0.0;
                self.status = "Export en cours…".into();
                let _ = self.job_tx.send(Job::Convert {
                    entry,
                    ordinal: ord,
                    out: PathBuf::from(&self.out_dir),
                    format: self.format,
                    tags,
                });
            }
        }
        if let Some(p) = &self.last_export {
            ui.add_space(2.0);
            ui.label(RichText::new(format!("↳ {p}")).small().weak());
        }
    }

    fn player_bar(&mut self, ui: &mut egui::Ui) {
        let title = self
            .track_ordinal
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "—".to_string());
        let archive = self
            .selected
            .map(|ei| self.rows[ei].title.clone())
            .unwrap_or_default();
        let track_len = self
            .track_ordinal
            .and_then(|i| self.tracks.get(i))
            .and_then(|t| t.length);

        let Some(player) = self.player.as_mut() else {
            ui.add_space(10.0);
            ui.label(RichText::new("Périphérique audio indisponible").weak());
            return;
        };
        let total = player.total();
        let pos = player.position();
        let window = player.window();
        let looping = player.looping();
        let paused = player.is_paused();
        let volume = player.volume();

        // Axe de la barre de temps : le cycle de boucle quand il est plus court
        // que le rendu (PSF rendu en plusieurs tours, genh…), sinon la durée de
        // la piste (celle de la liste) — zxtune rend 3 min fixes pour un NSF de
        // 1:55. Le curseur warp au point de reprise en fin de cycle.
        let cycle = looping && window.end > window.start + 1.0 && window.end < total - 1.0;
        let axis_start = if cycle { window.start } else { 0.0 };
        let axis_end = if cycle {
            window.end
        } else if looping {
            track_len
                .filter(|l| *l > 1.0)
                .map(|l| l.max(window.end).min(total))
                .unwrap_or(total)
        } else {
            total
        };
        // référence pour dire « piste entière » : la piste, pas le cycle
        let whole = track_len
            .filter(|l| *l > 1.0)
            .map(|l| l.min(total))
            .unwrap_or(total);

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if total > 0.0 {
                ui.label(RichText::new(title).strong().size(15.0));
                ui.label(RichText::new(archive).weak().small());
            } else {
                ui.label(RichText::new("Rien en lecture").weak());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if total > 0.0 && looping {
                    let full = window.start < 0.5 && (window.end - whole).abs() <= 1.0;
                    let mut text = if full {
                        "boucle : piste entière".to_string()
                    } else {
                        format!(
                            "boucle {} → {} ({})",
                            util::fmt_time_cs(window.start),
                            util::fmt_time_cs(window.end),
                            util::fmt_time(window.len())
                        )
                    };
                    // le rendu peut contenir plusieurs tours : on rappelle la
                    // durée totale de la piste à côté du cycle
                    if let Some(l) = track_len.filter(|l| *l > window.end + 1.0) {
                        text.push_str(&format!(" · piste {}", util::fmt_time(l)));
                    }
                    ui.label(RichText::new(text).small());
                }
            });
        });

        let mut seek = None;
        if total > 0.0 {
            let shown = LoopWindow {
                start: window.start - axis_start,
                end: window.end - axis_start,
            };
            if let Some(t) = timeline(ui, pos - axis_start, axis_end - axis_start, shown, looping) {
                seek = Some(t + axis_start);
            }
        } else {
            ui.add_space(14.0);
        }

        let mut toggle_pause = false;
        let mut toggle_loop = false;
        let mut jump = None;
        let mut set_a = false;
        let mut set_b = false;
        let mut apply_ab = false;
        let mut vol = None;

        ui.horizontal(|ui| {
            if ui.add_enabled(total > 0.0, egui::Button::new("Début")).clicked() {
                jump = Some(0.0);
            }
            if ui.add_enabled(total > 0.0, egui::Button::new("−5 s")).clicked() {
                jump = Some((pos - 5.0).max(0.0));
            }
            let play_label = if paused { "Lecture" } else { "Pause" };
            if ui
                .add_sized([86.0, 26.0], egui::Button::new(RichText::new(play_label).strong()))
                .clicked()
            {
                toggle_pause = true;
            }
            if ui.add_enabled(total > 0.0, egui::Button::new("+5 s")).clicked() {
                jump = Some((pos + 5.0).min(axis_end));
            }
            if ui
                .selectable_label(looping, RichText::new(if looping { "boucle ON" } else { "boucle OFF" }))
                .on_hover_text("L")
                .clicked()
            {
                toggle_loop = true;
            }
            ui.separator();
            if ui.add_enabled(total > 0.0, egui::Button::new("A = ici")).clicked() {
                set_a = true;
            }
            if ui.add_enabled(total > 0.0, egui::Button::new("B = ici")).clicked() {
                set_b = true;
            }
            if ui
                .add_enabled(total > 0.0, egui::Button::new("appliquer"))
                .on_hover_text("boucle la fenêtre A→B et la mémorise pour la piste")
                .clicked()
            {
                apply_ab = true;
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "A {}   B {}",
                    util::fmt_time_cs(self.ab_a.unwrap_or(window.start)),
                    util::fmt_time_cs(self.ab_b.unwrap_or(window.end))
                ))
                .small()
                .monospace(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(util::fmt_time(axis_end.max(pos))).monospace().small());
                ui.label(RichText::new("/").weak().small());
                ui.label(RichText::new(util::fmt_time(pos)).monospace().small());
                ui.add_space(10.0);
                let mut v = volume;
                if ui
                    .add_sized([100.0, 18.0], egui::Slider::new(&mut v, 0.0..=1.2).show_value(false))
                    .changed()
                {
                    vol = Some(v);
                }
                ui.label(RichText::new("vol").weak().small());
            });
        });

        if set_a {
            self.ab_a = Some(pos);
            self.ab_msg = None;
        }
        if set_b {
            self.ab_b = Some(pos);
            self.ab_msg = None;
        }
        if apply_ab {
            let a = self.ab_a.unwrap_or(window.start);
            let b = self.ab_b.unwrap_or(window.end);
            let w = LoopWindow {
                start: a.min(b),
                end: a.max(b),
            };
            player.set_window(w);
            player.set_looping(true);
            self.looping = true;
            if let (Some(ei), Some(ord)) = (self.selected, self.track_ordinal) {
                let id = self.entries[ei].id;
                let _ = self.db.set_loop_window(id, ord, w);
            }
            self.ab_msg = Some("boucle A/B active et enregistrée pour cette piste".into());
            self.status = "Lecture — boucle A/B".into();
        }
        if toggle_pause {
            player.toggle_pause();
        }
        if toggle_loop {
            let now = !looping;
            player.set_looping(now);
            self.looping = now;
            self.status = if now {
                "Lecture — boucle active".into()
            } else {
                "Lecture — boucle coupée".into()
            };
        }
        if let Some(t) = seek.or(jump) {
            player.seek(t);
        }
        if let Some(v) = vol {
            player.set_volume(v);
            self.volume = v;
        }
        if let Some(m) = &self.ab_msg {
            ui.label(RichText::new(m).small().weak());
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.style(ui.ctx());
        self.pump();
        if self.query != self.last_query
            || self.console_filter != self.last_console
            || self.sort != self.last_sort
        {
            self.refilter();
        }
        if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            self.focus_search = true;
        }
        self.keyboard(ui.ctx());

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));

        egui::Panel::bottom("status")
            .exact_size(22.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).small());
                    if self.busy {
                        ui.add(
                            egui::ProgressBar::new(self.progress)
                                .desired_width(190.0)
                                .animate(true),
                        );
                    }
                });
            });

        egui::Panel::bottom("player")
            .exact_size(104.0)
            .show(ui, |ui| self.player_bar(ui));

        egui::Panel::left("entries")
            .resizable(true)
            .default_size(430.0)
            .min_size(300.0)
            .show(ui, |ui| {
                ui.add_space(2.0);
                match self.view {
                    View::Catalogue => self.entries_panel(ui),
                    View::Cached => self.cached_panel(ui),
                }
            });

        egui::Panel::right("details")
            .resizable(true)
            .default_size(300.0)
            .min_size(240.0)
            .show(ui, |ui| self.details_panel(ui));

        egui::CentralPanel::default().show(ui, |ui| self.tracks_panel(ui));

        let want = match self.track_ordinal.and_then(|i| self.tracks.get(i)) {
            Some(t) => format!("RetroPlayer — {}", t.title),
            None => "RetroPlayer".to_string(),
        };
        if want != self.window_title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(want.clone()));
            self.window_title = want;
        }

        ui.ctx().request_repaint_after(Duration::from_millis(60));
    }
}

// ---------------------------------------------------------------- helpers

fn build_meta(entries: &[Entry]) -> (Vec<RowMeta>, Vec<&'static str>) {
    let mut rows = Vec::with_capacity(entries.len());
    let mut consoles: Vec<&'static str> = Vec::new();
    for e in entries {
        let c = util::clean_name(&e.name);
        let console = joshw::short_name(&e.site);
        if !consoles.contains(&console) {
            consoles.push(console);
        }
        // le nom brut reste indexé : on peut chercher « (1985 » ou « [NES] »
        let sort_key = format!(
            "{} {} {} {}",
            c.title,
            console,
            c.tags.join(" "),
            e.name
        )
        .to_lowercase();
        rows.push(RowMeta {
            title: c.title,
            sort_key,
            console,
            date: c.date,
            tags: c.tags.join(", "),
        });
    }
    consoles.sort_unstable();
    (rows, consoles)
}

fn fmt_size(bytes: u64) -> String {
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < 3 {
        v /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} o"),
        1 => format!("{v:.0} Ko"),
        2 => format!("{v:.1} Mo"),
        _ => format!("{v:.2} Go"),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Barre de temps cliquable ; la région de boucle est teintée.
fn timeline(
    ui: &mut egui::Ui,
    pos: f64,
    total: f64,
    window: LoopWindow,
    looping: bool,
) -> Option<f64> {
    let width = ui.available_width().max(80.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 14.0), Sense::click_and_drag());
    let painter = ui.painter();
    let bg = ui.visuals().extreme_bg_color;
    let loop_col = ui.visuals().selection.bg_fill.gamma_multiply(0.45);
    let cursor = ui.visuals().strong_text_color();
    painter.rect_filled(rect, CornerRadius::same(4), bg);
    if total > 0.0 {
        if looping && window.end > window.start {
            let x0 = rect.left() + (window.start / total).clamp(0.0, 1.0) as f32 * rect.width();
            let x1 = rect.left() + (window.end / total).clamp(0.0, 1.0) as f32 * rect.width();
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
                CornerRadius::same(4),
                loop_col,
            );
            painter.line_segment(
                [egui::pos2(x1, rect.top()), egui::pos2(x1, rect.bottom())],
                Stroke::new(1.0, loop_col),
            );
        }
        let x = rect.left() + (pos / total).clamp(0.0, 1.0) as f32 * rect.width();
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x - 1.0, rect.top()),
                egui::pos2(x + 1.0, rect.bottom()),
            ),
            CornerRadius::same(1),
            cursor,
        );
    }
    if resp.clicked() || resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let frac = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            return Some(frac as f64 * total);
        }
    }
    None
}

fn spawn_worker() -> (Sender<Job>, Sender<Msg>, Receiver<Msg>) {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let keep_tx = msg_tx.clone();
    std::thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let tx = msg_tx.clone();
            let res = match job {
                Job::Open(entry) => open_archive(&tx, &entry),
                Job::Play { entry, ordinal } => play(&tx, &entry, ordinal),
                Job::Convert { entry, ordinal, out, format, tags } => {
                    convert_track(&tx, &entry, ordinal, &out, format, tags)
                }
                Job::Delete(id) => delete_cached(&tx, id),
                Job::Update => update(&tx),
            };
            if let Err(e) = res {
                let _ = msg_tx.send(Msg::Error(format!("{e:#}")));
            }
        }
    });
    (job_tx, keep_tx, msg_rx)
}

fn progress_fn(tx: &Sender<Msg>) -> impl FnMut(f32, &str) + '_ {
    move |p: f32, label: &str| {
        let _ = tx.send(Msg::Progress(p));
        let _ = tx.send(Msg::Status(format!("{label}… {:.0} %", p * 100.0)));
    }
}

fn open_archive(tx: &Sender<Msg>, entry: &Entry) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let mut cb = progress_fn(tx);
    let archive: Archive = retroplayer_core::archive::ensure(entry, &db, &mut cb)?;
    let _ = tx.send(Msg::Tracks(archive.tracks, archive.composer, archive.album));
    Ok(())
}

fn play(tx: &Sender<Msg>, entry: &Entry, ordinal: usize) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let mut cb = progress_fn(tx);
    let archive = retroplayer_core::archive::ensure(entry, &db, &mut cb)?;
    let track = archive
        .tracks
        .get(ordinal)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("piste {ordinal} absente"))?;
    let _ = tx.send(Msg::Status(format!("Décodage : {}", track.title)));
    let decoded = decode::decode(&retroplayer_core::zxtune_path(), &track)?;
    let _ = tx.send(Msg::Playing(
        archive.tracks,
        ordinal,
        decoded,
        archive.composer,
        archive.album,
    ));
    Ok(())
}

fn convert_track(
    tx: &Sender<Msg>,
    entry: &Entry,
    ordinal: usize,
    out: &std::path::Path,
    format: Format,
    tags: Tags,
) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let mut cb = progress_fn(tx);
    let archive = retroplayer_core::archive::ensure(entry, &db, &mut cb)?;
    let track = archive
        .tracks
        .get(ordinal)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("piste {ordinal} absente"))?;
    let _ = tx.send(Msg::Status(format!(
        "Export {} : {}",
        format.ext().to_uppercase(),
        track.title
    )));
    let path = retroplayer_core::convert::export(
        &retroplayer_core::zxtune_path(),
        &track,
        out,
        format,
        &tags,
    )?;
    let _ = tx.send(Msg::Converted(path));
    Ok(())
}

fn delete_cached(tx: &Sender<Msg>, id: i64) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let _ = tx.send(Msg::Status("Suppression du cache…".into()));
    let bytes = retroplayer_core::archive::delete_cached(id, &db)?;
    let _ = tx.send(Msg::Deleted(id, bytes));
    Ok(())
}

fn update(tx: &Sender<Msg>) -> Result<()> {
    let mut db = Db::open(&util::db_path())?;
    let tx2 = tx.clone();
    let cb = std::sync::Arc::new(move |done: usize, total: usize| {
        let p = done as f32 / total.max(1) as f32;
        let _ = tx2.send(Msg::Progress(p));
        let _ = tx2.send(Msg::Status(format!("Mise à jour joshw.info… {done}/{total}")));
    });
    let added = joshw::update(&mut db, cb)?;
    let _ = tx.send(Msg::Status(format!("{added} nouvelles archives")));
    let _ = tx.send(Msg::ReloadDb);
    Ok(())
}
