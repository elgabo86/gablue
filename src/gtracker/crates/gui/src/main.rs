//! GTracker — GUI egui : navigation joshw.info, lecture rodio, export MP3/FLAC.

mod gtracker;
mod shell;

use shell::{LibraryTab, Workspace};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, RichText, Sense,
    Stroke, Vec2,
};
use gtracker_core::archive::Archive;
use gtracker_core::convert::{Format, Tags};
use gtracker_core::db::Db;
use gtracker_core::decode::{self, Decoded};
use gtracker_core::model::{Entry, LoopWindow, Track};
use gtracker_core::player::Player;
use gtracker_core::tempo::{self, Envelope, Tempo};
use gtracker_core::{joshw, util};

/// Thème unique de l'application : couleurs, tailles, densité. Une seule
/// source de vérité pour que la coquille, la bibliothèque et l'arrangement
/// aient la même peau (avant, chacun avait ses gris).
mod theme {
    use eframe::egui::{
        self, style::WidgetVisuals, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle,
    };

    pub const BG: Color32 = Color32::from_rgb(0x0d, 0x0f, 0x13);
    pub const PANEL: Color32 = Color32::from_rgb(0x15, 0x18, 0x1d);
    pub const CHROME: Color32 = Color32::from_rgb(0x1a, 0x1e, 0x24);
    pub const RAISED: Color32 = Color32::from_rgb(0x22, 0x27, 0x2e);
    pub const HOVER: Color32 = Color32::from_rgb(0x2b, 0x31, 0x3a);
    pub const LINE: Color32 = Color32::from_rgb(0x2d, 0x34, 0x3e);
    pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xec, 0xf1);
    pub const DIM: Color32 = Color32::from_rgb(0x9c, 0xa5, 0xb2);
    pub const FAINT: Color32 = Color32::from_rgb(0x6b, 0x75, 0x83);
    pub const ACCENT: Color32 = Color32::from_rgb(0x4d, 0x9d, 0xff);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x25, 0x43, 0x69);

    pub fn apply(ctx: &egui::Context) {
        ctx.all_styles_mut(|s| {
            let mut v = egui::Visuals::dark();
            v.panel_fill = PANEL;
            v.window_fill = CHROME;
            v.extreme_bg_color = BG;
            v.faint_bg_color = RAISED;
            v.code_bg_color = BG;
            v.hyperlink_color = ACCENT;
            v.warn_fg_color = Color32::from_rgb(0xf0, 0xb4, 0x4c);
            v.error_fg_color = Color32::from_rgb(0xf0, 0x6c, 0x6c);
            v.window_stroke = Stroke::new(1.0, LINE);
            v.window_corner_radius = CornerRadius::same(10);
            v.menu_corner_radius = CornerRadius::same(10);
            // les textes secondaires doivent rester lisibles : 0,6 par défaut
            // les rendait gris sur gris
            v.weak_text_alpha = 1.0;
            v.selection.bg_fill = ACCENT_SOFT;
            v.selection.stroke = Stroke::new(1.0, ACCENT);
            let base = |bg: Color32, fg: Color32| WidgetVisuals {
                bg_fill: bg,
                weak_bg_fill: bg,
                bg_stroke: Stroke::new(1.0, LINE),
                corner_radius: CornerRadius::same(6),
                fg_stroke: Stroke::new(1.0, fg),
                expansion: 0.0,
            };
            v.widgets.noninteractive = base(PANEL, TEXT);
            v.widgets.inactive = base(RAISED, TEXT);
            v.widgets.hovered = WidgetVisuals {
                bg_fill: HOVER,
                weak_bg_fill: HOVER,
                bg_stroke: Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
                corner_radius: CornerRadius::same(6),
                fg_stroke: Stroke::new(1.0, TEXT),
                expansion: 0.0,
            };
            v.widgets.active = WidgetVisuals {
                bg_fill: ACCENT_SOFT,
                weak_bg_fill: ACCENT_SOFT,
                bg_stroke: Stroke::new(1.0, ACCENT),
                corner_radius: CornerRadius::same(6),
                fg_stroke: Stroke::new(1.0, TEXT),
                expansion: 0.0,
            };
            v.widgets.open = v.widgets.active;
            s.visuals = v;
            s.spacing.item_spacing = egui::vec2(8.0, 7.0);
            s.spacing.button_padding = egui::vec2(10.0, 5.0);
            s.spacing.interact_size.y = 24.0;
            s.spacing.window_margin = Margin::same(10);
            s.spacing.menu_margin = Margin::same(8);
            s.text_styles.insert(
                TextStyle::Heading,
                FontId::new(18.0, FontFamily::Proportional),
            );
            s.text_styles
                .insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
            s.text_styles.insert(
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            );
            s.text_styles
                .insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
            s.text_styles.insert(
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            );
        });
    }

    /// Titre de section : petites capitales espacées, puis un trait fin.
    pub fn section(ui: &mut egui::Ui, text: &str) {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .size(9.5)
                .strong()
                .color(DIM),
        );
        ui.add_space(1.0);
        ui.separator();
        ui.add_space(2.0);
    }

    /// Ligne « libellé : valeur » de l'inspecteur (les valeurs se lisent d'un
    /// coup d'œil, les libellés s'effacent).
    pub fn field(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).size(10.5).color(FAINT));
            ui.label(
                egui::RichText::new(value)
                    .size(11.5)
                    .color(TEXT),
            );
        });
    }
}

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
            // la fenêtre s'ouvre **maximisée** (demande user 22/11) ; la taille
            // intérieure ne sert plus que de repli si le compositeur refuse
            .with_maximized(true)
            .with_icon(icon)
            // app_id Wayland = nom du `.desktop` : sans lui KWin ne rattache pas
            // la fenêtre et affiche l'icône générique (même piège que glauncher)
            .with_app_id("gtracker")
            .with_title("GTracker"),
        ..Default::default()
    };
    eframe::run_native(
        "GTracker",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

// ---------------------------------------------------------------- jobs

enum Job {
    Open(Entry),
    Play { entry: Entry, ordinal: usize },
    /// Décode et charge sans jouer : ouverture d'un fichier déposé sur la
    /// fenêtre (il attend le clic sur « écouter »).
    Load { entry: Entry, ordinal: usize },
    Convert { entry: Entry, ordinal: usize, out: PathBuf, format: Format, tags: Tags },
    /// Export d'une suite de tranches (arrangement de la découpe) en un fichier.
    ConvertSegments {
        entry: Entry,
        ordinal: usize,
        spans: Vec<(f64, f64)>,
        out: PathBuf,
        format: Format,
        tags: Tags,
        name: String,
    },
    Delete(i64),
    Update,
    /// Analyse du rythme du mode Découpe. Séparée du décodage : le réseau coûte
    /// 1 à 3 s et retardait la lecture (demande user 21/11).
    Tempo {
        track: Track,
        entry_id: i64,
        ordinal: usize,
    },
    /// Décode une source du GTracker (la piste est déjà extraite : le fichier
    /// existe, aucun réseau).
    GtrackerLoad { track: Track, make_track: bool },
    /// Rythme + tonalité d'une source GTracker, hors du chemin de lecture.
    GtrackerTempo { track: Track },
    /// Rendu et encodage du projet GTracker.
    GtrackerExport {
        project: Box<gtracker_core::project::Project>,
        audio: Vec<Option<gtracker_core::mixer::SourceAudio>>,
        format: Format,
        name: String,
    },
}

enum Msg {
    Status(String),
    Progress(f32),
    /// `(archive, pistes, compositeur, album)` : l'identifiant évite qu'une
    /// réponse tardive écrase la liste de l'archive ouverte entre-temps.
    Tracks(i64, Vec<Track>, Option<String>, Option<String>),
    /// `autoplay` = lancer la sortie son dès le chargement. `false` pour un
    /// fichier déposé : il est prêt, muet, et la lecture attend l'utilisateur.
    Playing(
        i64,
        Vec<Track>,
        usize,
        Decoded,
        Option<String>,
        Option<String>,
        bool,
    ),
    /// Durée / fenêtre de boucle mesurées pour une piste (entry, ordinal).
    Timing(i64, usize, Option<f64>, Option<(f64, f64)>),
    /// Grille du mode Découpe, calculée en tâche de fond.
    Tempo(i64, usize, Tempo),
    /// Source du GTracker décodée (piste, PCM).
    GtrackerLoaded(Track, Decoded, bool),
    /// Rythme et tonalité d'une source GTracker.
    GtrackerTempo(std::path::PathBuf, Option<Tempo>, Option<gtracker_core::key::Key>),
    /// Projet GTracker exporté.
    GtrackerExported(std::path::PathBuf),
    Converted(PathBuf),
    Deleted(i64, u64),
    ReloadDb,
    Error(String),
}

// ---------------------------------------------------------------- état


struct CachedRow {
    entry_idx: Option<usize>,
    entry_id: i64,
    bytes: u64,
    tracks: usize,
}

/// Piste de bibliothèque telle que le Player l'a chargée : de quoi la remettre
/// en service sans relire le fichier (et sans effacer la découpe).
#[derive(Clone)]
struct LibAudio {
    pcm: Arc<Vec<f32>>,
    channels: u16,
    rate: u32,
    total: f64,
    window: LoopWindow,
    /// Dernière position connue, en secondes.
    pos: f64,
}

impl LibAudio {
    /// Réinstalle la piste dans le lecteur, en pause, à sa position. Aucun
    /// fichier relu : le PCM est le même Arc que celui chargé au départ.
    fn restore(&self, player: &mut Option<Player>, looping: bool, volume: f32) {
        let Some(p) = player.as_mut() else {
            return;
        };
        p.load(
            self.pcm.clone(),
            self.channels,
            self.rate,
            self.total,
            self.window,
            looping,
            false,
        );
        p.set_volume(volume);
        if self.pos > 0.0 {
            p.seek(self.pos);
        }
    }
}

/// Comportement d'un clic sur une tuile (mode Découpe).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClickMode {
    /// Seul le **dernier** clic compte (défaut) : le segment en cours finit,
    /// puis le dernier cliqué (un clic plus récent remplace celui qui attendait).
    Last,
    /// Chaque clic s'empile : tout ce qui est cliqué sera joué, dans l'ordre.
    Queue,
}

impl ClickMode {
    fn label(self) -> &'static str {
        match self {
            ClickMode::Queue => "clic : en file",
            ClickMode::Last => "clic : dernier seul",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            ClickMode::Queue => {
                "chaque clic s'empile : tout ce que tu cliques est joué, dans l'ordre"
            }
            ClickMode::Last => {
                "seul le dernier clic compte : le segment en cours finit, puis le dernier \
                 cliqué — un clic plus récent remplace celui qui attendait"
            }
        }
    }
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
    /// Sélection de l'onglet « Téléchargés », **indépendante** du catalogue :
    /// ouvrir un jeu déjà téléchargé ne doit pas changer la ligne sélectionnée
    /// dans le catalogue, ni afficher ses pistes sous l'en-tête d'un autre jeu
    /// (vécu 21/11 : les deux listes se mélangeaient).
    cached_sel: Option<i64>,
    /// Jeu dont la liste de pistes est chargée (venu du catalogue **ou** des
    /// téléchargés). C'est lui qui fait foi pour jouer, exporter, sonder et
    /// enregistrer une boucle A/B — pas `selected`, qui n'est qu'une position
    /// de liste.
    open_entry: Option<Entry>,
    /// `true` si l'archive ouverte vient de « Téléchargés » : la playlist reste
    /// dans l'onglet qui l'a ouverte (l'autre garde la sienne).
    open_from_cached: bool,
    tracks: Vec<Track>,
    track_ordinal: Option<usize>,
    composer: Option<String>,
    album: Option<String>,

    player: Option<Player>,
    /// Piste de bibliothèque chargée, gardée pour la **réinjecter** : le Player
    /// ne sert qu'un flux à la fois et `play_source` (arrangement, écoute de
    /// tranche) oublie le PCM de la piste. Sans cette copie, revenir dans
    /// l'éditeur laissait un lecteur vide — « je peux plus jouer » (vécu 21/11).
    lib_audio: Option<LibAudio>,
    /// Une tranche est en écoute : la piste de bibliothèque ne doit pas la
    /// couper en se réinstallant.
    auditioning: bool,
    /// État du GTracker (projet, timeline, éditeur de source).
    gtracker: gtracker::Gtracker,
    looping: bool,
    volume: f32,
    ab_a: Option<f64>,
    ab_b: Option<f64>,
    ab_msg: Option<String>,
    /// Tempo du rendu courant : grille de temps pour aimanter A/B et découper
    /// des segments d'une ou plusieurs mesures. `None` = pas de rythme net.
    tempo: Option<Tempo>,
    /// Enveloppe d'attaque du rendu : recale chaque coupe sur une attaque
    /// réelle, sinon un BPM estimé 0,1 % faux décale les dernières mesures.
    envelope: Option<Envelope>,
    /// Forme d'onde du rendu (min/max par tranche) : dans le mode Découpe, elle
    /// montre où tombent les temps et les frontières de coupe — un tempo faux ne
    /// s'entend qu'à l'oreille, mais se voit d'un coup d'œil (vécu 21/11).
    peaks: Option<Arc<Vec<(f32, f32)>>>,
    /// Tempo réglé à la main (BPM absolu), ou `None` = celui de la détection.
    /// Filet de sécurité quand l'automatique tombe à côté ; « auto » l'efface
    /// (demande user 21/11).
    bpm_manual: Option<f64>,
    /// Accumulateur de molette : un cran ≈ 30 px de défilement lissé.
    bpm_wheel: f64,
    /// Départ des segments par rapport au point de boucle, en **temps** (pas
    /// seulement en mesures) : ±1 ou ±2 temps pour recaler un départ au demi-
    /// temps près, ±1 mesure pour changer le groupement (demande user 21/11).
    /// Le tempo ne bouge pas, seule l'origine de la grille glisse.
    shift_beats: i32,
    /// Analyse du rythme en cours pour (archive, piste) : évite de relancer un
    /// job à chaque image en attendant la réponse.
    tempo_req: Option<(i64, usize)>,
    /// L'analyse a échoué pour la piste courante : on ne relance pas en boucle.
    tempo_failed: bool,
    /// Le clic droit de cette trame a été utilisé par un widget (ligne de la
    /// suite) : il ne doit pas arrêter la lecture en plus.
    right_consumed: bool,
    /// L'ordre « maximisée » a-t-il déjà été envoyé ? Le drapeau du
    /// `ViewportBuilder` est ignoré par KWin quand la fenêtre est un client
    /// XWayland (constaté eframe 0.37 / 22/11) : on redemande une fois la
    /// première trame affichée.
    maximized_once: bool,
    /// Zoom de la forme d'onde, en secondes (`0` = toute la boucle) et centre de
    /// la vue : la grille est entièrement automatique, la forme d'onde sert à
    /// **vérifier** à l'œil (et non plus à la caler à la main).
    wave_zoom: f64,
    wave_center: f64,
    /// Boucle déclarée **par le fichier** (`.ast`, `.btsnd`, PSF…), telle quelle :
    /// sert à proposer « ignorer la boucle du fichier » quand le marqueur du rip
    /// n'est pas musical (vécu 21/11, Super Mario Galaxy).
    fmt_loop: Option<(f64, f64)>,
    /// L'utilisateur **active** la boucle du fichier pour cette piste
    /// (mémorisé en base). Défaut : **non** — une découpe se fait sur la piste
    /// entière, sans intro fabriquée par un marqueur de rip (demande user 21/11).
    auto_loop: bool,
    /// Bord de boucle tenu à la souris dans la forme d'onde : `true` = début,
    /// `false` = fin, `None` = rien. Sert à distinguer le glisser d'une poignée
    /// du glisser de la vue (demande user 21/11).
    loop_grab: Option<bool>,
    /// Trait de **coupe** tenu à la souris (décalage souris → trait, secondes) :
    /// glisser déplace toute la grille, pas un seul trait — les durées restent
    /// égales (demande user 21/11).
    cut_grab: Option<f64>,
    /// Origine de grille posée à la main (secondes). Quand elle existe, la
    /// découpe est **rigide** autour d'elle (`tempo::cut`) au lieu d'être
    /// réajustée sur les attaques (`cut_on`) : sinon `fit_grid` déplacerait les
    /// traits sous la souris.
    cut_anchor: Option<f64>,
    /// Origine des lignes de calage pendant le geste : les bords se collent sur
    /// les **frontières de tranche** (temps × longueur de segment), celles qui
    /// sont dessinées — sinon en mode « 4 mesures » le bord retombait sur un
    /// temps quelconque.
    loop_snap: f64,
    /// Boucle réellement jouée (format/détection) : sa fin borne la découpe et
    /// son début ancre la grille — un PSF a une intro, la boucle commence
    /// plus tard (vécu 21/11).
    loop_win: Option<LoopWindow>,
    /// La fenêtre courante est un choix **de l'utilisateur** (bouton « poser
    /// une boucle » ou bords glissés), pas la longueur du `.m3u` ni la boucle
    /// du fichier. Seul ce cas propose « retirer la boucle » : une longueur de
    /// `.m3u` n'est pas une boucle à effacer.
    loop_manual: bool,
    /// Début de la boucle = ancre de la grille de découpe (0 = début de piste).
    anchor: f64,
    /// Ne pas mettre l'intro (avant la boucle) dans la suite par défaut.
    skip_intro: bool,
    /// Aimant : A/B collent à la grille (quand un tempo existe).
    magnet: bool,
    /// Longueur d'un segment en temps (1, 2, 4, 8, 16) : 4 temps = 1 mesure 4/4.
    clip_beats: f64,
    /// Segments de la découpe automatique (tranches de la piste).
    clips: Vec<LoopWindow>,
    /// Arrangement : indices de `clips`, répétitions comprises.
    arrange: Vec<usize>,
    /// Suite en cours de lecture (indices de `clips`) et position.
    seq: Vec<usize>,
    seq_pos: usize,
    /// Premier segment de la suite en cours de lecture (bouclage) et si cette
    /// suite doit boucler : « écouter » depuis le transport boucle, un
    /// enchaînement de clics sur les tuiles joue une fois (demande user 21/11).
    seq_from: usize,
    seq_loop: bool,
    /// Dernier saut de suite vu (horloge `Player::segment_jumps`).
    seq_jump: u64,
    /// Comportement des clics sur les tuiles (file ou dernier seul).
    click_mode: ClickMode,
    /// Pads live : touches actuellement tenues (ordre d'appui), segment qui
    /// sonne, et segment programmé pour la couture suivante.
    pads_held: Vec<usize>,
    pad_cur: Option<usize>,
    pad_next: Option<usize>,

    status: String,
    busy: bool,
    progress: f32,

    /// Fil local : lecture, découpe, export, suppression.
    job_tx: Sender<Job>,
    /// Fil réseau : téléchargement d'archives et mise à jour du catalogue.
    net_tx: Sender<Job>,
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,

    out_dir: String,
    format: Format,
    last_export: Option<String>,

    styled: bool,
    focus_search: bool,
    window_title: String,

    /// Coquille unique : bibliothèque, espace de travail, inspecteur.
    shell: shell::Shell,
    cached: Vec<CachedRow>,
    pending_delete: Option<i64>,
    scroll_to_selected: bool,
    probe_cancel: Arc<AtomicBool>,
}

impl App {
    fn new() -> Self {
        let initial_query = std::env::args().skip(1).find(|a| !a.starts_with('-')).unwrap_or_default();
        let (job_tx, net_tx, msg_tx, msg_rx) = spawn_workers();
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
        // dossier d'export : le dossier Musique **XDG** (pas un nom deviné —
        // c'est `~/Musique`, sans « s », sur ce poste, vécu 21/11)
        let out_dir = util::music_dir().join("GTracker").display().to_string();
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
            cached_sel: None,
            open_entry: None,
            open_from_cached: false,
            tracks: Vec::new(),
            track_ordinal: None,
            composer: None,
            album: None,
            player: Player::new().ok(),
            lib_audio: None,
            auditioning: false,
            gtracker: gtracker::Gtracker::new(),
            // la musique de jeu boucle : c'est la préférence de l'utilisateur,
            // conservée d'une piste à l'autre (le bouton la change)
            looping: true,
            volume: 1.0,
            ab_a: None,
            ab_b: None,
            ab_msg: None,
            tempo: None,
            bpm_manual: None,
            bpm_wheel: 0.0,
            shift_beats: 0,
            tempo_req: None,
            tempo_failed: false,
            right_consumed: false,
            maximized_once: false,
            envelope: None,
            peaks: None,
            wave_zoom: 0.0,
            wave_center: 0.0,
            fmt_loop: None,
            auto_loop: false,
            loop_grab: None,
            cut_grab: None,
            cut_anchor: None,
            loop_snap: 0.0,
            loop_manual: false,
            loop_win: None,
            anchor: 0.0,
            skip_intro: true,
            magnet: true,
            // une mesure par défaut (4/4) — le passage à deux mesures a été
            // annulé (demande user 22/11)
            clip_beats: 4.0,
            clips: Vec::new(),
            arrange: Vec::new(),
            seq: Vec::new(),
            seq_pos: 0,
            seq_from: 0,
            seq_loop: false,
            seq_jump: 0,
            click_mode: ClickMode::Last,
            pads_held: Vec::new(),
            pad_cur: None,
            pad_next: None,
            status,
            busy: false,
            progress: 0.0,
            job_tx,
            net_tx,
            msg_tx,
            msg_rx,
            out_dir,
            format: Format::Mp3Vbr,
            last_export: None,
            styled: false,
            focus_search: false,
            window_title: String::new(),
            shell: shell::Shell::default(),
            cached: Vec::new(),
            pending_delete: None,
            scroll_to_selected: false,
            probe_cancel: Arc::new(AtomicBool::new(false)),
        };
        app.refresh_cached();
        // tri initial : sans cet appel la liste garde l'ordre de la base
        // (`ORDER BY site, folder, file_name`), donc les premiers sites
        // alphabétiques d'abord (NDS avant NSF…) au lieu de A→Z (vécu 21/11)
        app.refilter();
        app
    }

    /// Envoie un travail au **bon** fil : le téléchargement et la mise à jour
    /// du catalogue ont le leur, tout le reste doit rester réactif pendant
    /// qu'une archive se télécharge (demande user 21/11 : « pourquoi je ne peux
    /// pas écouter ce que j'ai déjà pendant un dl »).
    fn submit(&self, job: Job) {
        let cached = match &job {
            Job::Open(e) | Job::Play { entry: e, .. } => self.is_cached(e.id).is_some(),
            _ => false,
        };
        let tx = if goes_to_net(&job, cached) {
            &self.net_tx
        } else {
            &self.job_tx
        };
        let _ = tx.send(job);
    }

    fn refresh_cached(&mut self) {
        let list = gtracker_core::archive::cached(&self.db).unwrap_or_default();
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
                Msg::Tracks(id, tracks, composer, album) => {
                    // l'archive qui finit de se télécharger apparaît tout de
                    // suite dans « Téléchargés »…
                    self.refresh_cached();
                    // …mais sa liste de pistes ne doit pas écraser celle qu'on
                    // est en train d'écouter
                    if self.opened().map(|e| e.id) != Some(id) {
                        continue;
                    }
                    // nouvelle archive : plus aucune piste chargée, donc plus de
                    // découpe, de tempo ni de boucle — les garder affichait la
                    // boucle du morceau précédent (vécu 21/11 : un PSX sans
                    // boucle héritait de celle du titre d'avant).
                    self.clear_cut();
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
                    if self.opened().map(|e| e.id) == Some(entry_id) {
                        if let Some(t) = self.tracks.get_mut(ordinal) {
                            if length.is_some() {
                                t.length = length;
                            }
                            t.loop_start = window.map(|(s, _)| s);
                            t.loop_end = window.map(|(_, e)| e);
                        }
                    }
                }
                Msg::Playing(id, tracks, ordinal, decoded, composer, album, autoplay) => {
                    self.refresh_cached();
                    // lecture demandée sur une autre archive (on l'a ouverte
                    // entre-temps) : réponse périmée
                    if self.opened().map(|e| e.id) != Some(id) {
                        continue;
                    }
                    self.tracks = tracks;
                    self.composer = composer;
                    self.album = album;
                    self.track_ordinal = Some(ordinal);
                    self.busy = false;
                    self.progress = 1.0;
                    self.ab_a = None;
                    self.ab_b = None;
                    self.ab_msg = None;
                    self.fmt_loop = decoded.window;
                    self.auto_loop = self
                        .opened()
                        .and_then(|e| self.db.auto_loop(e.id, ordinal).ok())
                        .unwrap_or(false);
                    // nouveau rendu : nouvelle grille de tempo, découpe neuve —
                    // on la prépare tout de suite, le mode Découpe est prêt.
                    // Les corrections manuelles repartent de l'automatique.
                    // le tempo n'est plus calculé au décodage : il arrive par
                    // `Msg::Tempo` quand le mode Découpe le demande
                    self.tempo = decoded.tempo;
                    self.tempo_req = None;
                    self.tempo_failed = false;
                    self.shift_beats = 0;
                    self.bpm_manual = None;
                    self.bpm_wheel = 0.0;
                    self.envelope = decoded.envelope;
                    self.peaks = Some(decoded.peaks.clone());
                    self.wave_zoom = 0.0;
                    self.wave_center = 0.0;
                    // une poignée tenue sur la piste précédente n'a plus de sens
                    self.loop_grab = None;
                    self.cut_grab = None;
                    self.cut_anchor = None;
                    self.loop_snap = 0.0;
                    let stored = self
                        .opened()
                        .and_then(|e| self.db.loop_window(e.id, ordinal).ok().flatten())
                        // une boucle « piste entière » n'est pas un choix A/B :
                        // si le rendu contient plusieurs tours, la période
                        // détectée dedans est plus juste
                        .filter(|w| {
                            !(decoded.window.is_some()
                                && w.start < 0.5
                                && (w.end - decoded.total).abs() <= 1.0)
                        })
                        // Une fenêtre impossible (fin au-delà du rendu) ou qui
                        // englobe la boucle détectée est périmée : elle masquait
                        // la vraie boucle (vécu 21/11 : (0, 362) en base pour un
                        // rendu PSF de 269 s, la boucle 20 → 192,6 n'était
                        // jamais jouée alors que zxtune la connaissait).
                        .filter(|w| {
                            if w.end > decoded.total + 0.5 || w.start < -0.5 {
                                return false;
                            }
                            match decoded.window {
                                Some((a, b)) => !(w.start < a - 0.5 && w.end > b + 0.5),
                                None => true,
                            }
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
                    let (w, origin, manual) = if let Some(st) = stored {
                        (st, "boucle A/B enregistrée".to_string(), true)
                    } else if let Some((a, b)) = decoded.window.filter(|_| self.auto_loop) {
                        let origin = match decoded.window_passes {
                            Some(n) => format!("boucle détectée dans le rendu ({n} tours)"),
                            None => "boucle du format".to_string(),
                        };
                        (LoopWindow { start: a, end: b }, origin, false)
                    } else if let Some((start, end)) = fmt_loop.filter(|_| self.auto_loop) {
                        let origin = if start > 0.5 {
                            "boucle du format"
                        } else {
                            "boucle du format (hors fondu)"
                        };
                        (LoopWindow { start, end }, origin.to_string(), false)
                    } else if let Some(len) = m3u_len {
                        (
                            LoopWindow {
                                start: 0.0,
                                end: len,
                            },
                            "longueur du .m3u".to_string(),
                            false,
                        )
                    } else {
                        (
                            LoopWindow {
                                start: 0.0,
                                end: decoded.total,
                            },
                            "fichier entier".to_string(),
                            false,
                        )
                    };
                    // Une boucle réelle (ni le rendu entier, ni une fenêtre
                    // vide) : sa fin borne la découpe et son début ancre la
                    // grille. Un PSF a une intro, la boucle commence plus tard.
                    self.loop_win = if w.end > w.start + 1.0
                        && w.end <= decoded.total + 0.5
                        && (w.end - w.start) < decoded.total - 1.0
                    {
                        Some(w)
                    } else {
                        None
                    };
                    // la source porte la même boucle : l'arrangement découpe
                    // exactement là où l'éditeur vient de boucler
                    if let Some(w) = self.loop_win {
                        if let Some(t) = self.tracks.get(ordinal) {
                            self.gtracker
                                .set_source_loop(&t.file, t.subsong, (w.start, w.end));
                        }
                    }
                    self.loop_manual = manual && self.loop_win.is_some();
                    self.anchor = self.loop_win.map(|l| l.start).unwrap_or(0.0);
                    self.clips = match self.grid() {
                        Some(t) => tempo::cut_on(
                            self.cut_whole(decoded.total),
                            &t,
                            self.clip_beats,
                            self.envelope.as_ref(),
                        ),
                        None => Vec::new(),
                    };
                    self.apply_skip_intro();
                    self.arrange.clear();
                    self.seq.clear();
                    self.seq_pos = 0;
                    self.seq_jump = 0;
                    self.pads_held.clear();
                    self.pad_cur = None;
                    self.pad_next = None;
                    // un seul propriétaire du son : la bibliothèque coupe le
                    // transport du GTracker (le projet et l'arrangement
                    // restent intacts)
                    self.gtracker.halt();
                    self.auditioning = false;
                    let looping = self.looping;
                    let volume = self.volume;
                    // la bibliothèque garde de quoi reprendre le son après un
                    // passage dans l'arrangement (même Arc, aucune copie PCM)
                    self.lib_audio = Some(LibAudio {
                        pcm: decoded.pcm.clone(),
                        channels: decoded.channels,
                        rate: decoded.rate,
                        total: decoded.total,
                        window: w,
                        pos: 0.0,
                    });
                    match self.player.as_mut() {
                        Some(player) => {
                            player.load(
                                decoded.pcm,
                                decoded.channels,
                                decoded.rate,
                                decoded.total,
                                w,
                                looping,
                                autoplay,
                            );
                            player.set_volume(volume);
                            self.status = if autoplay {
                                format!(
                                    "Lecture — {origin} · {} – {}",
                                    util::fmt_time_cs(w.start),
                                    util::fmt_time_cs(w.end)
                                )
                            } else {
                                format!(
                                    "Prêt à découper — {origin} · {} – {} · en pause",
                                    util::fmt_time_cs(w.start),
                                    util::fmt_time_cs(w.end)
                                )
                            };
                        }
                        None => self.status = "Sortie audio indisponible".into(),
                    }
                }
                Msg::Deleted(id, bytes) => {
                    if self.cached_sel == Some(id) {
                        self.cached_sel = None;
                    }
                    if self.opened().map(|e| e.id) == Some(id) {
                        self.tracks.clear();
                        self.track_ordinal = None;
                        self.composer = None;
                        self.album = None;
                        self.open_entry = None;
                        self.peaks = None;
                        self.tempo = None;
                        self.clips.clear();
                        self.seq.clear();
                    }
                    self.status = format!("{} libérés (archive #{id})", fmt_size(bytes));
                    self.refresh_cached();
                    self.pending_delete = None;
                }
                Msg::Tempo(id, ord, t) => {
                    if self.opened().map(|e| e.id) == Some(id) && self.track_ordinal == Some(ord) {
                        self.tempo_req = None;
                        self.shift_beats = 0;
                        self.bpm_manual = None;
                        self.tempo = Some(t);
                        // la découpe attendait cette grille : on l'applique
                        // maintenant, sans avoir retenu la lecture
                        self.recut();
                        self.status = format!("Grille : {:.1} BPM", t.bpm);
                    }
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
                Msg::GtrackerLoaded(track, decoded, make_track) => {
                    self.gtracker.add_audio(&track, decoded, make_track);
                    self.status = format!("GTracker : « {} » chargée", track.title);
                    let _ = self.job_tx.send(Job::GtrackerTempo { track });
                }
                Msg::GtrackerTempo(path, tempo, key) => {
                    match tempo {
                        Some(t) => self.gtracker.set_tempo_full(&path, t, key),
                        None => self.gtracker.set_analysis(&path, None, key),
                    }
                    self.status = match tempo {
                        Some(t) => format!("GTracker : grille {:.1} BPM", t.bpm),
                        None => "GTracker : pas de rythme net sur cette source".into(),
                    };
                }
                Msg::GtrackerExported(p) => {
                    self.status = format!("GTracker : exporté → {}", p.display());
                    self.last_export = Some(p.display().to_string());
                }
                Msg::Error(e) => {
                    self.busy = false;
                    if self.tempo_req.is_some() {
                        self.tempo_req = None;
                        self.tempo_failed = true;
                    }
                    self.status = format!("Erreur : {e}");
                }
            }
        }
    }

    /// Mesure en tâche de fond les durées que les `.m3u` ne donnent pas
    /// (PSF, XA, sets sans playlist) : une sonde zxtune par piste, résultat
    /// écrit en base pour ne pas recommencer.
    fn start_probe(&mut self) {
        let Some(entry) = self.opened().map(|e| (e.id, e.name.clone())) else {
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
        let zxtune = gtracker_core::zxtune_path();
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

    /// Jeu dont les pistes sont chargées (catalogue ou téléchargé).
    fn opened(&self) -> Option<&Entry> {
        self.open_entry.as_ref()
    }

    fn open_archive(&mut self, ei: usize) {
        let Some(entry) = self.entries.get(ei).cloned() else {
            return;
        };
        self.open_entry = Some(entry.clone());
        // vient du **catalogue** : sa playlist ne s'affiche que là (le drapeau
        // était inversé, la playlist des téléchargés restait visible ici —
        // demande user 21/11)
        self.open_from_cached = false;
        self.probe_cancel.store(true, Ordering::Relaxed);
        self.tracks.clear();
        self.track_ordinal = None;
        self.busy = true;
        self.progress = 0.0;
        self.status = "Ouverture de l'archive…".into();
        self.submit(Job::Open(entry));
    }

    /// Entrée synthétique d'un fichier audio déposé sur la fenêtre : site
    /// `fichiers`, chemin absolu, identifiant **négatif** (jamais un rowid).
    /// Rien n'est écrit dans la base : le fichier n'apparaît pas au catalogue.
    fn local_entry(path: &std::path::Path) -> Entry {
        let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let name = abs
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| abs.display().to_string());
        let folder = abs
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Entry {
            id: util::local_entry_id(&abs),
            site: gtracker_core::model::LOCAL_SITE.into(),
            folder,
            name,
            path: abs.display().to_string(),
        }
    }

    /// Ouvre un fichier audio déposé sur la fenêtre : lecture immédiate et
    /// bascule en mode Découpe, la grille arrive ensuite (`dj_waiting`).
    fn open_file(&mut self, path: std::path::PathBuf) {
        let entry = Self::local_entry(&path);
        let title = entry.name.clone();
        self.open_entry = Some(entry.clone());
        self.open_from_cached = false;
        self.probe_cancel.store(true, Ordering::Relaxed);
        self.tracks.clear();
        self.track_ordinal = None;
        self.clear_cut();
        self.busy = true;
        self.progress = 0.0;
        self.shell.set_workspace(Workspace::Edit);
        self.status = format!("Ouverture de {title}…");
        // **sans** lecture automatique : le fichier s'ouvre prêt à découper
        self.submit(Job::Load { entry: entry.clone(), ordinal: 0 });
        // un seul document : le fichier déposé est aussi une source du projet
        let track = Track {
            subsong: 0,
            title: entry.name.clone(),
            file: PathBuf::from(&entry.path),
            length: None,
            loop_start: None,
            loop_end: None,
        };
        self.submit(Job::GtrackerLoad { track, make_track: true });
    }

    /// Fichier audio déposé alors que le GTracker est affiché : il devient une
    /// **source** du projet (décodage hors ligne, puis analyse rythme/tonalité).
    fn gtracker_drop(&mut self, path: PathBuf) {
        let entry = Self::local_entry(&path);
        let track = Track {
            subsong: 0,
            title: entry.name.clone(),
            file: PathBuf::from(&entry.path),
            length: None,
            loop_start: None,
            loop_end: None,
        };
        self.status = format!("GTracker : chargement de « {} »…", track.title);
        self.submit(Job::GtrackerLoad { track, make_track: true });
    }

    /// Commandes rendues par la vue GTracker (chargement, ouverture, export).
    fn gtracker_cmd(&mut self, cmd: gtracker::Cmd) {
        match cmd {
            gtracker::Cmd::Load(track) => {
                self.status = format!("GTracker : chargement de « {} »…", track.title);
                self.submit(Job::GtrackerLoad {
                    track: *track,
                    make_track: true,
                });
                self.shell.set_workspace(Workspace::Arrange);
            }
            gtracker::Cmd::Preview {
                source,
                src_start,
                src_end,
                label,
            } => {
                // écouter une tranche prend le son, mais la piste de la
                // bibliothèque garde sa position : « Lecture » la reprend ensuite
                self.remember_library_pos();
                self.auditioning =
                    self.gtracker
                        .preview(&mut self.player, source, src_start, src_end, label);
            }
            gtracker::Cmd::Export { project, audio, format, name } => {
                self.status = "GTracker : rendu et encodage…".into();
                self.submit(Job::GtrackerExport { project, audio, format, name });
            }
            gtracker::Cmd::SourceLoop { source, start, end } => {
                let w = LoopWindow { start, end };
                // même piste que celle de l'éditeur : la boucle jouée et la
                // découpe suivent tout de suite
                let same = self
                    .track_ordinal
                    .and_then(|i| self.tracks.get(i).cloned())
                    .zip(self.gtracker.project.sources.get(source).cloned())
                    .map(|(t, s)| t.file == s.file && t.subsong == s.subsong)
                    .unwrap_or(false);
                if same {
                    self.loop_win = Some(w);
                    self.anchor = w.start;
                    self.loop_manual = true;
                    self.fmt_loop = Some((w.start, w.end));
                    self.recut();
                    if self.looping {
                        if let Some(p) = self.player.as_mut() {
                            p.set_window(w);
                        }
                    }
                }
                // base : seulement si la source vient de l'archive ouverte
                if let Some(src) = self.gtracker.project.sources.get(source).cloned() {
                    if src.entry_id >= 0 && Some(src.entry_id) == self.opened().map(|e| e.id) {
                        let _ = self.db.set_loop_window(
                            src.entry_id,
                            src.subsong.saturating_sub(1) as usize,
                            w,
                        );
                    }
                }
                self.status = format!(
                    "boucle de la source : {} → {}",
                    util::fmt_time_cs(start),
                    util::fmt_time_cs(end)
                );
            }
            gtracker::Cmd::Open(path) => match self.gtracker.open(path.clone()) {
                Ok(tracks) => {
                    self.status = format!(
                        "GTracker : projet « {} » ouvert ({} sources à recharger)",
                        self.gtracker.project_name(),
                        tracks.len()
                    );
                    for track in tracks {
                        self.submit(Job::GtrackerLoad {
                            track,
                            make_track: false,
                        });
                    }
                }
                Err(e) => {
                    self.status = format!("GTracker : ouverture impossible : {e}");
                }
            },
        }
    }

    /// Entrée du catalogue par identifiant, sinon depuis la base (archive
    /// orpheline d'un ancien catalogue).
    fn entry_by_id(&self, id: i64) -> Option<Entry> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .or_else(|| {
                self.db
                    .all_entries()
                    .ok()?
                    .into_iter()
                    .find(|e| e.id == id)
            })
    }

    /// Ouvre une archive depuis « Téléchargés ». Elle peut être absente du
    /// catalogue (archive orpheline) : on la retrouve par identifiant, jamais
    /// par l'index d'une autre liste.
    fn open_cached(&mut self, id: i64) {
        let Some(entry) = self.entry_by_id(id) else {
            self.status = format!("archive #{id} introuvable");
            return;
        };
        self.open_entry = Some(entry.clone());
        // vient de **« Téléchargés »** : sa playlist ne s'affiche que là
        self.open_from_cached = true;
        self.probe_cancel.store(true, Ordering::Relaxed);
        self.tracks.clear();
        self.track_ordinal = None;
        self.busy = true;
        self.progress = 0.0;
        self.status = "Ouverture de l'archive…".into();
        self.submit(Job::Open(entry));
    }

    /// Clic gauche sur une piste : elle se lance.
    fn play_track(&mut self, ordinal: usize) {
        self.open_track(ordinal, true);
    }

    /// Clic **droit** sur une piste : elle s'ouvre en mode **Découpe**, sans
    /// être lancée (demande user 21/11 : « au lieu de lire la chanson »).
    fn cut_track(&mut self, ordinal: usize) {
        // éditer une source, c'est l'ajouter au document (sans lui imposer une
        // piste : le double-clic sert à ça) puis montrer l'éditeur
        if let Some(t) = self.tracks.get(ordinal).cloned() {
            self.submit(Job::GtrackerLoad {
                track: t,
                make_track: false,
            });
        }
        self.switch_workspace(Workspace::Edit);
        self.open_track(ordinal, false);
    }

    fn open_track(&mut self, ordinal: usize, autoplay: bool) {
        let Some(entry) = self.opened().cloned() else {
            return;
        };
        self.busy = true;
        self.progress = 0.0;
        // la forme d'onde appartient à la piste précédente : la garder afficherait
        // l'onde d'un morceau sous la grille d'un autre pendant le décodage
        self.peaks = None;
        self.status = "Préparation…".into();
        self.submit(if autoplay {
            Job::Play { entry, ordinal }
        } else {
            Job::Load { entry, ordinal }
        });
    }

    // ------------------------------------------------------------ UI

    fn style(&mut self, ctx: &egui::Context) {
        if self.styled {
            return;
        }
        self.styled = true;
        theme::apply(ctx);
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        // le GTracker a ses propres raccourcis (espace, flèches, chiffres) :
        // les traiter ici aussi ferait deux actions pour une touche
        if self.shell.workspace == Workspace::Arrange {
            return;
        }
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        self.ensure_library_sound(false);
        let (space, left, right, l, r) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::L),
                i.key_pressed(egui::Key::R),
            )
        });
        // les pads live sont gérés à part (touches tenues + alternance)
        if let Some(player) = self.player.as_mut() {
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
                // quitter une suite et les pads : la boucle prend le relais
                self.seq.clear();
                self.pad_cur = None;
                self.pad_next = None;
                self.pads_held.clear();
                            }
            if r {
                player.seek(0.0);
            }
        }
    }

    /// Pads live : maintenir une touche boucle son segment ; maintenir deux
    /// touches (ou plus) fait alterner les segments à chaque couture, en
    /// suivant l'ordre d'appui. Tout relâchement rend la main au morceau à la
    /// fin du tour en cours (la boucle s'arrête sans clic).
    fn pads(&mut self, ctx: &egui::Context) {
        // en Arrangement, les pads de l'éditeur (encore en mémoire) ne doivent
        // pas sonner sous les doigts : une touche = un panneau focalisé
        if self.shell.workspace == Workspace::Arrange {
            return;
        }
        // frappe dans un champ de saisie : les pads ne doivent pas sonner.
        // En mode Découpe il n'y a aucun champ de texte : le focus resté dans
        // la recherche du catalogue rendait les touches mortes (vécu 21/11).
        if self.shell.workspace == Workspace::Edit && ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if self.clips.is_empty() {
            self.pads_held.clear();
            self.pad_cur = None;
            self.pad_next = None;
                        return;
        }
        let (pressed, down) = ctx.input(|i| {
            let mut pressed: Vec<usize> = Vec::new();
            if !i.modifiers.any() {
                for (n, (k, _)) in PAD_KEYS.iter().enumerate() {
                    if i.key_pressed(*k) {
                        pressed.push(n);
                    }
                }
            }
            let down: Vec<bool> = PAD_KEYS.iter().map(|(k, _)| i.key_down(*k)).collect();
            (pressed, down)
        });
        for i in &pressed {
            if *i < self.clips.len() && !self.pads_held.contains(i) {
                self.pads_held.push(*i);
            }
        }
        // clavier : un appui compte comme tenu au moins le temps d'une frame
        // (tap rapide, sinon le pad ne sonnerait pas)
        let keys: Vec<usize> = self
            .pads_held
            .iter()
            .copied()
            .filter(|i| down.get(*i).copied().unwrap_or(false) || pressed.contains(i))
            .collect();
        self.pads_held = keys;
        // clavier uniquement : la souris joue une fois et enchaîne (voir
        // `hit_tile`), les deux modes ne se mélangent pas
        let held = self.pads_held.clone();

        if held.is_empty() {
            self.stop_pads("live : pad relâché — la boucle s'arrête en fin de tour");
            return;
        }

        if self.pad_cur.is_none() {
            let first = held[0];
            self.start_pad(first);
        }

        // quel segment sonne réellement ? La source applique la bascule seule,
        // à la couture ; la GUI se contente de la suivre. On regarde la fenêtre
        // live (pas le pad programmé) pour rattraper aussi le cas où le pad
        // suivant a été relâché juste après son entrée en boucle.
        if let Some(live) = self.player.as_ref().map(|p| p.window()) {
            let sounding = self.clips.iter().position(|c| {
                (c.start - live.start).abs() < 0.02 && (c.end - live.end).abs() < 0.02
            });
            if let Some(k) = sounding {
                if self.pad_cur != Some(k) {
                    self.pad_cur = Some(k);
                    self.pad_next = None;
                }
            }
        }

        // prochain pad : le suivant dans l'ordre d'appui, en boucle. Avec un
        // seul pad tenu il n'y a rien à alterner.
        let successor = next_pad(&held, self.pad_cur);
        if successor != self.pad_next {
            match successor {
                Some(s) => {
                    if let Some(w) = self.clips.get(s).copied() {
                        if let Some(p) = self.player.as_mut() {
                            p.queue_loop(w);
                        }
                        self.pad_next = Some(s);
                        self.status = format!(
                            "live : seg {} en boucle – seg {} à la couture",
                            self.pad_cur.map(|i| i + 1).unwrap_or(0),
                            s + 1
                        );
                    }
                }
                None => {
                    if let Some(p) = self.player.as_mut() {
                        p.cancel_queued_loop();
                    }
                    self.pad_next = None;
                }
            }
        }
    }

    /// Relâchement : le tour de boucle en cours finit, puis la lecture
    /// s'arrête (elle ne fuit pas dans la suite du morceau).
    fn stop_pads(&mut self, msg: &str) {
        if self.pad_cur.is_some() || self.pad_next.is_some() {
            if let Some(p) = self.player.as_mut() {
                p.cancel_queued_loop();
                p.stop_after_window();
            }
            self.pad_cur = None;
            self.pad_next = None;
            self.status = msg.into();
        }
    }

    fn start_pad(&mut self, i: usize) {
        let Some(w) = self.clips.get(i).copied() else {
            return;
        };
        let Some(player) = self.player.as_mut() else {
            return;
        };
        player.set_window(w);
        player.set_looping(true);
        player.seek(w.start);
        player.resume();
        self.pad_cur = Some(i);
        self.pad_next = None;
        self.seq.clear();
        self.ab_a = Some(w.start);
        self.ab_b = Some(w.end);
        self.ab_msg = Some(format!(
            "live : segment {} — maintenir pour boucler, une autre touche pour alterner",
            i + 1
        ));
        self.status = format!(
            "live : segment {} — {} – {}",
            i + 1,
            util::fmt_time_cs(w.start),
            util::fmt_time_cs(w.end)
        );
    }

    /// Barre haute de la coquille : identité, espace de travail, source
    /// courante, panneaux. Il n'y a plus d'onglets plein écran.
    fn app_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            // marque : un carré plein et le nom, comme un vrai logiciel
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            ui.painter()
                .rect_filled(mark, CornerRadius::same(6), theme::ACCENT);
            ui.painter().text(
                mark.center(),
                Align2::CENTER_CENTER,
                "G",
                FontId::proportional(13.0),
                Color32::from_rgb(0x0b, 0x12, 0x1c),
            );
            ui.label(RichText::new("GTracker").size(14.5).strong());
            ui.add_space(8.0);

            // espace de travail : contrôle segmenté, pas deux mots flottants
            Frame::new()
                .fill(theme::BG)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::same(2))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.horizontal(|ui| {
                        for w in [Workspace::Arrange, Workspace::Edit] {
                            let b = egui::Button::new(RichText::new(w.label()).size(12.5))
                                .selected(self.shell.workspace == w)
                                .min_size(Vec2::new(86.0, 22.0));
                            if ui
                                .add(b)
                                .on_hover_text(match w {
                                    Workspace::Arrange => {
                                        "Le document : pistes, clips, tranches (F1)"
                                    }
                                    Workspace::Edit => {
                                        "L'éditeur de la source courante (F2)"
                                    }
                                })
                                .clicked()
                            {
                                self.switch_workspace(w);
                            }
                        }
                    });
                });
            ui.add_space(4.0);

            // le document (nom, nouveau, enregistrer, ouvrir, exporter) reste
            // accessible depuis les deux espaces de travail
            let mut doc_cmds = Vec::new();
            self.gtracker.project_menu(ui, &mut doc_cmds);
            if self.gtracker.is_dirty() {
                // point **dessiné** : les polices embarquées n'ont pas « ● »,
                // il sortait en carré jaune à côté du menu (vécu 22/11)
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
                ui.painter().circle_filled(
                    r.center(),
                    3.4,
                    ui.visuals().warn_fg_color,
                );
                resp.on_hover_text("Modifications non enregistrées");
            }
            for c in doc_cmds {
                self.gtracker_cmd(c);
            }
            ui.separator();

            // ce que l'espace de travail montre : le document (arrangement) ou
            // la source éditée — jamais un mélange des deux
            let current = self
                .track_ordinal
                .and_then(|i| self.tracks.get(i))
                .cloned();
            match self.shell.workspace {
                Workspace::Arrange => {
                    let name = self
                        .gtracker
                        .selected_source_name()
                        .unwrap_or_else(|| "Aucune source".into());
                    ui.vertical(|ui| {
                        ui.label(RichText::new(name).strong().size(12.5));
                        ui.label(
                            RichText::new(format!(
                                "{} pistes · {} clips",
                                self.gtracker.project_tracks(),
                                self.gtracker.project_clips()
                            ))
                            .size(10.5)
                            .color(theme::FAINT),
                        );
                    });
                }
                Workspace::Edit => match &current {
                    Some(t) => {
                        let archive = self
                            .opened()
                            .map(|e| e.display_name())
                            .unwrap_or_default();
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&t.title).strong().size(12.5));
                            ui.label(
                                RichText::new(format!(
                                    "{archive}{}",
                                    t.length
                                        .map(|l| format!(" · {}", util::fmt_time(l)))
                                        .unwrap_or_default()
                                ))
                                .size(10.5)
                                .color(theme::FAINT),
                            );
                        });
                    }
                    None => {
                        ui.label(
                            RichText::new("Aucune source ouverte")
                                .size(12.0)
                                .color(theme::FAINT),
                        );
                    }
                },
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // panneaux : de vrais boutons, avec une flèche qui dit le sens.
                // C'est aussi le bouton de retour quand un panneau est fermé.
                self.panel_toggle(ui, false);
                self.panel_toggle(ui, true);
                ui.add_space(4.0);
                if ui
                    .add_enabled(
                        self.gtracker.can_redo(),
                        egui::Button::new("Refaire").min_size(Vec2::new(26.0, 22.0)),
                    )
                    .on_hover_text("Refaire (ctrl+shift+Z)")
                    .clicked()
                {
                    self.gtracker.redo();
                }
                if ui
                    .add_enabled(
                        self.gtracker.can_undo(),
                        egui::Button::new("Annuler").min_size(Vec2::new(26.0, 22.0)),
                    )
                    .on_hover_text("Annuler (ctrl+Z)")
                    .clicked()
                {
                    self.gtracker.undo();
                }
                if let Some(t) = current {
                    if ui
                        .button("Ajouter au projet")
                        .on_hover_text(
                            "La source écoutée devient une piste du projet\
                             \n(un double-clic dans la playlist fait la même chose)",
                        )
                        .clicked()
                    {
                        self.gtracker_cmd(gtracker::Cmd::Load(Box::new(t)));
                    }
                }
            });
        });
        ui.add_space(4.0);
    }

    /// Bascule d'un panneau latéral, avec le sens du repli. Quand le panneau
    /// est fermé, le bouton reste visible et devient le moyen de le rouvrir
    /// (F7/F8 au clavier).
    fn panel_toggle(&mut self, ui: &mut egui::Ui, library: bool) {
        let open = if library {
            self.shell.show_library
        } else {
            self.shell.show_inspector
        };
        let label = if library { "Bibliothèque" } else { "Inspecteur" };
        let text = label.to_string();
        let key = if library { "F8" } else { "F7" };
        let hint = if open {
            format!("Replier ({key})")
        } else {
            format!("Afficher ({key})")
        };
        let b = egui::Button::new(RichText::new(text).size(12.0))
            .selected(open)
            .min_size(Vec2::new(96.0, 22.0));
        if ui
            .add(b)
            .on_hover_text(hint)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            if library {
                self.shell.toggle_library();
            } else {
                self.shell.toggle_inspector();
            }
        }
    }

    /// Rail d'un panneau fermé : une bande cliquable sur le bord, avec son nom
    /// à la verticale. Le panneau ne peut pas « disparaître » sans laisser de
    /// porte de retour au même endroit.
    fn closed_rail(&mut self, ui: &mut egui::Ui, library: bool) {
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ui.available_height()),
            Sense::click(),
        );
        let hovered = resp.hovered();
        if hovered {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::HOVER);
        }
        let color = if hovered { theme::TEXT } else { theme::DIM };
        // chevron dessiné : la police embarquée n'a pas ▸/◂
        let c = egui::pos2(rect.center().x, rect.top() + 16.0);
        let dx = if library { 1.0 } else { -1.0 };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x - 3.0 * dx, c.y - 5.0),
                egui::pos2(c.x + 3.0 * dx, c.y),
                egui::pos2(c.x - 3.0 * dx, c.y + 5.0),
            ],
            color,
            Stroke::NONE,
        ));
        let name = if library { "BIBLIOTHÈQUE" } else { "INSPECTEUR" };
        let mut y = rect.top() + 44.0;
        for ch in name.chars() {
            ui.painter().text(
                egui::pos2(rect.center().x, y),
                Align2::CENTER_CENTER,
                ch.to_string(),
                FontId::proportional(9.5),
                color,
            );
            y += 11.0;
        }
        let hint = if library {
            "Ouvrir la bibliothèque (F8)"
        } else {
            "Ouvrir l'inspecteur (F7)"
        };
        if resp
            .on_hover_text(hint)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            if library {
                self.shell.toggle_library();
            } else {
                self.shell.toggle_inspector();
            }
        }
    }

    /// La piste de bibliothèque sonne-t-elle (et non le projet ni une écoute
    /// de tranche) ? Une piste en pause ou terminée ne compte pas : c'est ce
    /// qui décide de montrer « Arrêter ».
    fn library_sound_on(&self) -> bool {
        if self.auditioning || self.gtracker.playing {
            return false;
        }
        self.player
            .as_ref()
            .is_some_and(|p| p.ready() && !p.is_paused() && !p.finished())
    }

    /// Un son quelconque est en cours (piste de bibliothèque, écoute de
    /// tranche, projet) : il y a donc quelque chose à arrêter.
    fn sound_on(&self) -> bool {
        self.auditioning || self.gtracker.playing || self.library_sound_on()
    }

    /// Arrêt global : écoute de tranche, projet et piste de bibliothèque se
    /// taisent. Pause, pas `stop_source` : le PCM reste chargé et chaque
    /// transport repart de sa position (sinon « Lecture » resterait muet).
    fn stop_sound(&mut self) {
        if let Some(p) = self.player.as_mut() {
            p.halt();
        }
        self.auditioning = false;
        self.gtracker.silence();
        self.status = "Lecture arrêtée".into();
    }

    /// Bascule d'espace de travail : elle coupe le flux de l'autre espace
    /// (projet ↔ pré-écoute) pour qu'un seul transport sonne à la fois.
    fn switch_workspace(&mut self, w: Workspace) {
        if self.shell.workspace == w {
            return;
        }
        // quitter l'arrangement rend la main à la bibliothèque : une écoute de
        // tranche en cours ne doit pas garder le son
        self.auditioning = false;
        match w {
            Workspace::Edit => {
                // le projet se tait ; la piste de la bibliothèque reprend le
                // même lecteur, à sa position (sinon « Lecture » resterait muet)
                self.gtracker.halt();
                self.ensure_library_sound(true);
            }
            Workspace::Arrange => {
                self.remember_library_pos();
                // pause, PAS stop : vider la source perdrait le PCM de la piste
                if let Some(p) = self.player.as_mut() {
                    p.halt();
                }
            }
        }
        self.shell.set_workspace(w);
    }

    /// Mémorise où en était la piste (seulement si c'est bien elle qui sonne :
    /// quand l'arrangement joue, la position appartient au projet).
    fn remember_library_pos(&mut self) {
        if self.gtracker.playing || self.auditioning {
            return;
        }
        let (Some(p), Some(lib)) = (self.player.as_ref(), self.lib_audio.as_mut()) else {
            return;
        };
        lib.pos = p.position();
    }

    /// Remet la piste de bibliothèque dans le Player si un autre flux l'a
    /// remplacée (arrangement, écoute de tranche). Sans effet sinon : on ne
    /// recharge pas à chaque image.
    fn ensure_library_sound(&mut self, force: bool) {
        if self.auditioning {
            return;
        }
        let Some(lib) = self.lib_audio.clone() else {
            return;
        };
        if !force && self.player.as_ref().is_some_and(|p| p.ready()) {
            return;
        }
        lib.restore(&mut self.player, self.looping, self.volume);
    }

    /// Bibliothèque latérale : Catalogue et Téléchargés sont deux **filtres**,
    /// pas deux écrans. Le document, le transport et l'inspecteur ne bougent pas.
    fn library_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("BIBLIOTHÈQUE")
                    .size(10.0)
                    .strong()
                    .color(theme::DIM),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let btn = ui.add_enabled(
                    !self.busy,
                    egui::Button::new(RichText::new("Actualiser").size(11.5)),
                );
                if btn
                    .on_hover_text("Réinterroge les sites joshw.info (1–2 min)")
                    .clicked()
                {
                    self.busy = true;
                    self.progress = 0.0;
                    self.submit(Job::Update);
                }
            });
        });
        ui.add_space(6.0);
        // Catalogue / Téléchargés : un filtre segmenté, pas des onglets d'écran
        Frame::new()
            .fill(theme::BG)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(2))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal(|ui| {
                    for tab in [LibraryTab::Catalogue, LibraryTab::Cached] {
                        let label = match tab {
                            LibraryTab::Catalogue => tab.label().to_string(),
                            LibraryTab::Cached => {
                                format!("{} · {}", tab.label(), self.cached.len())
                            }
                        };
                        let b = egui::Button::new(RichText::new(label).size(12.0))
                            .selected(self.shell.library == tab)
                            .min_size(Vec2::new(0.0, 22.0));
                        if ui
                            .add(b)
                            .on_hover_text(match tab {
                                LibraryTab::Catalogue => {
                                    "Les archives joshw.info, à télécharger"
                                }
                                LibraryTab::Cached => {
                                    "Les archives déjà extraites, conservées"
                                }
                            })
                            .clicked()
                        {
                            self.shell.set_library(tab);
                            if tab == LibraryTab::Cached {
                                self.refresh_cached();
                            }
                        }
                    }
                });
            });
        ui.add_space(6.0);

        match self.shell.library {
            LibraryTab::Catalogue => {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 60.0).max(120.0);
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
                        && ui.small_button("×").on_hover_text("effacer").clicked()
                    {
                        self.query.clear();
                    }
                    ui.label(
                        RichText::new(format!("{} / {}", self.filtered.len(), self.entries.len()))
                            .weak()
                            .small(),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("console")
                        .selected_text(match self.console_filter {
                            Some(c) => format!("console : {c}"),
                            None => "toutes les consoles".to_string(),
                        })
                        .width(165.0)
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
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for (s, label) in [
                                (SortBy::Name, "nom (A-Z)"),
                                (SortBy::Console, "console puis nom"),
                                (SortBy::Date, "date (récent d'abord)"),
                            ] {
                                if ui.selectable_label(self.sort == s, label).clicked() {
                                    self.sort = s;
                                }
                            }
                        });
                });
                ui.separator();
                self.entries_panel(ui);
            }
            LibraryTab::Cached => {
                let total: u64 = self.cached.iter().map(|c| c.bytes).sum();
                let tracks: usize = self.cached.iter().map(|c| c.tracks).sum();
                ui.horizontal(|ui| {
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
                });
                ui.separator();
                self.cached_panel(ui);
            }
        }
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
                     Double-clique une archive dans la bibliothèque :\n\
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
        // A → Z sur le titre, **toutes consoles mêlées** (demande user 21/11 :
        // la liste était triée par console puis par nom, ce qui éparpillait une
        // même série) ; la console ne sert qu'à départager deux titres égaux.
        rows.sort_by_cached_key(|r| cached_sort_key(&r.3, &r.2));

        let mut sel: Option<i64> = None;
        let mut delete = None;
        let mut pending = self.pending_delete;
        let count = rows.len();
        egui::ScrollArea::vertical()
            .id_salt("cached")
            .auto_shrink([false, false])
            .show_rows(ui, 40.0, count, |ui, range| {
                for i in range {
                    let (id, _idx, console, title, bytes, tracks) = &rows[i];
                    let budget = ((ui.available_width() - 120.0) / 7.1).max(14.0) as usize;
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    self.cached_sel == Some(*id),
                                    RichText::new(truncate_chars(title, budget)).size(13.0),
                                )
                                .on_hover_text(title)
                                .clicked()
                            {
                                sel = Some(*id);
                            }
                            // console/taille/pistes : une méta discrète, pas
                            // une deuxième ligne de titre (retour user 22/11)
                            ui.label(
                                RichText::new(format!(
                                    "{console} · {} · {tracks} pistes",
                                    fmt_size(*bytes)
                                ))
                                .size(10.5)
                                .color(theme::FAINT),
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
        if let Some(id) = sel {
            // on reste dans « Téléchargés » et on n'écrase pas la sélection du
            // catalogue : chaque onglet garde la sienne (demande user, 21/11)
            self.cached_sel = Some(id);
            self.open_cached(id);
        }
        if let Some(id) = delete {
            self.status = "Suppression du cache…".into();
            self.submit(Job::Delete(id));
        }
    }

    fn tracks_panel(&mut self, ui: &mut egui::Ui) {
        let weak = ui.visuals().weak_text_color();
        let strong = ui.visuals().text_color();
        let on_sel = ui.visuals().selection.stroke.color;

        ui.add_space(2.0);
        let shown = self.opened().map(|e| (e.id, e.display_name()));
        // La playlist appartient **à l'onglet qui l'a ouverte** : ouvrir un jeu
        // depuis « Téléchargés » puis revenir au catalogue n'affiche rien, et
        // inversement (demande user 21/11). Une simple sélection dans l'autre
        // onglet ne suffit pas à la faire apparaître : seul Découpe la suit
        // toujours.
        let local = self
            .opened()
            .map(|e| e.site == gtracker_core::model::LOCAL_SITE)
            .unwrap_or(false);
        let mine = shown.is_some()
            && list_belongs(
                self.shell.library == LibraryTab::Cached,
                self.shell.workspace == Workspace::Edit,
                self.open_from_cached,
                local,
            );
        if let Some((_, name)) = shown.as_ref().filter(|_| mine) {
            let ei = self.opened().and_then(|e| {
                self.entries.iter().position(|x| x.id == e.id)
            });
            match ei {
                Some(i) => {
                    ui.label(RichText::new(&self.rows[i].title).strong().size(15.5));
                    ui.label(RichText::new(name).weak().small());
                }
                None => {
                    ui.label(RichText::new(name).strong().size(15.5));
                }
            }
        } else {
            ui.label(RichText::new("Pistes").strong().size(15.5));
        }
        ui.separator();

        if !mine {
            ui.add_space(14.0);
            ui.label(
                RichText::new("Aucune archive ouverte dans cette liste.")
                    .size(11.5)
                    .color(theme::FAINT),
            );
            ui.label(
                RichText::new("Ouvrez une archive pour voir ses pistes ici.")
                    .size(10.5)
                    .color(theme::FAINT),
            );
            return;
        }

        if self.tracks.is_empty() {
            ui.add_space(16.0);
            ui.label(
                RichText::new(
                    "Double-clic sur une archive à gauche :\ntéléchargement, extraction, puis liste des pistes.\n\n\
                     Un clic sur une piste lance la lecture, un second clic l'arrête.",
                )
                .weak(),
            );
            return;
        }

        let count = self.tracks.len();
        let lib_on = self.library_sound_on();
        let mut to_play = None;
        let mut to_stop = false;
        let mut to_cut = None;
        let mut to_add = None;
        egui::ScrollArea::vertical()
            .id_salt("tracks")
            .auto_shrink([false, false])
            .show_rows(ui, 21.0, count, |ui, range| {
                let max_chars = ((ui.available_width() - 110.0) / 7.1).max(12.0) as usize;
                for i in range {
                    let t = &self.tracks[i];
                    // le marqueur suit le son, pas la simple sélection : après
                    // un arrêt, aucune ligne ne se prétend en lecture
                    let playing = self.track_ordinal == Some(i) && lib_on;
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = f32::INFINITY;
                    job.append(
                        if playing { "> " } else { "  " },
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
                    let resp = ui
                        .selectable_label(playing, job)
                        .on_hover_text(
                            "clic : écouter / arrêter · double-clic : ajouter au projet (piste prête) · clic droit : éditer la source",
                        );
                    if resp.clicked() {
                        // même piste déjà en train de sonner : le clic l'arrête
                        if self.track_ordinal == Some(i) && lib_on {
                            to_stop = true;
                        } else {
                            to_play = Some(i);
                        }
                    }
                    if resp.double_clicked() {
                        to_play = None;
                        to_add = Some(i);
                    }
                    // clic droit **consommé** ici : il n'arrête plus la musique
                    if resp.clicked_by(egui::PointerButton::Secondary) {
                        self.right_consumed = true;
                        to_cut = Some(i);
                    }
                }
            });
        if to_stop {
            self.stop_sound();
        } else if let Some(i) = to_play {
            self.play_track(i);
        }
        if let Some(i) = to_cut {
            self.cut_track(i);
        }
        if let Some(i) = to_add {
            self.add_track_to_project(i);
        }
    }

    /// Double-clic dans la playlist : la source devient une **piste du projet**
    /// (décodée, analysée), prête à recevoir des tranches dans l'arrangement.
    fn add_track_to_project(&mut self, ordinal: usize) {
        let Some(track) = self.tracks.get(ordinal).cloned() else {
            return;
        };
        self.status = format!("Ajout au projet : {}", track.title);
        self.submit(Job::GtrackerLoad {
            track,
            make_track: true,
        });
        self.switch_workspace(Workspace::Arrange);
    }

    /// Inspecteur : playlist de l'archive ouverte + détails de la sélection.
    /// Il ne remplace pas l'éditeur, il l'accompagne.
    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("INSPECTEUR")
                    .size(10.0)
                    .strong()
                    .color(theme::DIM),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // arrêt du son écouté depuis la playlist : il n'existait
                // aucune commande une fois le panneau ouvert (vécu 21/11)
                if self.sound_on()
                    && ui
                        .add(
                            egui::Button::new(
                                RichText::new("Arrêter").size(10.5).color(theme::DIM),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Couper le son (piste, écoute de tranche ou projet)")
                        .clicked()
                {
                    self.stop_sound();
                }
                // un chevron, pas une croix : un clic au hasard dans le coin ne
                // doit pas faire disparaître le panneau (vécu 21/11)
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Replier").size(10.5).color(theme::FAINT),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Replier l'inspecteur (F7)")
                    .clicked()
                {
                    self.shell.toggle_inspector();
                }
            });
        });
        ui.add_space(2.0);
        ui.separator();
        // la playlist défile : une liste coupée en bas était inutilisable
        let h = (ui.available_height() * 0.42).clamp(150.0, 360.0);
        egui::ScrollArea::vertical()
            .id_salt("insp-playlist")
            .max_height(h)
            .auto_shrink([false, false])
            .show(ui, |ui| self.tracks_panel(ui));
        ui.separator();
        self.details_panel(ui);
    }

    fn details_panel(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Détails");
        egui::ScrollArea::vertical()
            .id_salt("details")
            .auto_shrink([false, false])
            .show(ui, |ui| self.details_body(ui));
    }

    fn details_body(&mut self, ui: &mut egui::Ui) {
        // les détails suivent la **sélection de l'onglet** (un simple clic dans
        // le catalogue doit montrer l'archive sans l'ouvrir) ; la liste de pistes,
        // elle, n'apparaît que pour le jeu réellement ouvert dans cet onglet
        let entry = match self.shell.library {
            LibraryTab::Cached => self.cached_sel.and_then(|id| self.entry_by_id(id)),
            LibraryTab::Catalogue => self.selected.and_then(|i| self.entries.get(i)).cloned(),
        };
        let Some(entry) = entry else {
            ui.add_space(12.0);
            ui.label(RichText::new("Sélectionne une archive").weak().small());
            return;
        };
        {
            let ei = self.entries.iter().position(|x| x.id == entry.id);
            let (title, console, date, tags) = match ei {
                Some(i) => (
                    self.rows[i].title.clone(),
                    self.rows[i].console,
                    self.rows[i].date.clone(),
                    self.rows[i].tags.clone(),
                ),
                None => (entry.display_name(), "", None, String::new()),
            };
            let name = entry.display_name();
            let url = entry.url();
            let loc = format!(
                "{}/{}",
                entry.site.trim_start_matches("https://"),
                entry.folder
            );
            let entry_id = entry.id;
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
            // compositeur/album viennent de l'archive **ouverte** : sur une
            // simple sélection d'une autre archive, ils afficheraient les
            // métadonnées du jeu de l'autre onglet (vécu 21/11)
            let is_open = self.opened().map(|e| e.id) == Some(entry_id);
            if is_open {
                if let Some(c) = &self.composer {
                    ui.label(RichText::new(format!("- {c}")).weak().small());
                }
                if let Some(a) = &self.album {
                    ui.label(RichText::new(format!("album : {a}")).weak().small());
                }
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
                    theme::field(
                        ui,
                        "cache",
                        format!("{} · {} piste(s)", fmt_size(bytes), tracks),
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
                            self.submit(Job::Delete(entry_id));
                        } else {
                            self.pending_delete = Some(entry_id);
                        }
                    }
                }
                None => {
                    theme::field(ui, "cache", "pas encore téléchargé");
                }
            }
        }

        ui.separator();
        ui.label(RichText::new("Export").strong());
        ui.add_space(2.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.out_dir)
                .hint_text("dossier de sortie")
                .desired_width(f32::INFINITY),
        );
        // deux boutons, un par format : plus de pastille à cocher puis un
        // seul bouton dont le libellé dépendait du réglage (demande user 21/11)
        let can = !self.busy && self.track_ordinal.is_some();
        let mut export_as = None;
        ui.horizontal(|ui| {
            for f in [Format::Mp3Vbr, Format::Flac] {
                let label = match f {
                    Format::Flac => "Exporter en FLAC".to_string(),
                    _ => "Exporter en MP3".to_string(),
                };
                if ui
                    .add_enabled(
                        can,
                        egui::Button::new(RichText::new(label).size(12.0))
                            .min_size(egui::vec2(0.0, 26.0)),
                    )
                    .on_hover_text(format!(
                        "{}\nzxtune encode, les tags (titre/compositeur/album) sont écrits ensuite",
                        f.hint()
                    ))
                    .clicked()
                {
                    export_as = Some(f);
                }
            }
        });
        if let Some(f) = export_as {
            self.format = f;
            if let (Some(entry), Some(ord)) = (self.opened().cloned(), self.track_ordinal) {
                let track = self.tracks[ord].clone();
                let tags = Tags {
                    title: Some(track.title.clone()),
                    artist: self.composer.clone(),
                    album: self.album.clone(),
                };
                self.busy = true;
                self.progress = 0.0;
                self.status = "Export en cours…".into();
                self.submit(Job::Convert {
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
            ui.label(RichText::new(format!("fichier : {p}")).small().weak());
        }
    }

    /// Grille effective : celle que donne la détection automatique (réseau
    /// Beat This!, sinon enveloppe d'attaque), sans aucun réglage manuel.
    /// `None` = piste sans rythme exploitable.
    fn grid(&self) -> Option<Tempo> {
        let mut g = self.tempo?;
        // La grille passe **toujours** par le début de la boucle : un PSF
        // commence par une intro, et les mesures de la boucle ne sont pas
        // alignées sur le début du fichier (vécu 21/11). Une piste qui boucle
        // depuis 0 (NES/NSF) doit aussi être ancrée à 0, sinon la phase
        // détectée (~0,4 s) fabriquait une première tranche de 0,4 s (vécu
        // 21/11, Kirby « Green Fields »). `loop_win` absent = pas de boucle :
        // on garde la phase détectée.
        if let Some(l) = self.loop_win {
            g.phase = l.start;
        }
        // tempo réglé à la main : on ne touche qu'à la période, pas à la phase
        // (une correction de 0,5 BPM ne doit pas déplacer le départ des mesures)
        if let Some(bpm) = self.bpm_manual.filter(|b| *b > 20.0) {
            g.bpm = bpm;
            g.beat = 60.0 / bpm;
        }
        // origine posée à la souris (traits de coupe glissés) : elle remplace
        // celle de la boucle/du tempo
        if let Some(a) = self.cut_anchor {
            g.phase = a;
        }
        // décalage du départ des segments, en temps : la grille de temps est
        // intacte, seule son origine glisse (±1/±2 temps, ±1 mesure)
        if self.shift_beats != 0 && g.beat > 0.0 {
            g.phase += self.shift_beats as f64 * g.beat;
        }
        Some(g)
    }

    /// Segment sous la tête de lecture de la **piste** (lecture simple, hors
    /// montage) : c'est la tuile qui doit s'allumer pendant qu'elle sonne.
    fn clip_at_player(&self) -> Option<usize> {
        if !self.seq.is_empty() {
            return None;
        }
        let p = self.player.as_ref()?;
        if p.is_paused() || p.finished() {
            return None;
        }
        let t = p.position_abs();
        self.clips.iter().position(|w| t >= w.start && t < w.end)
    }

    /// Position rythmique de la lecture : `(bloc, temps dans le bloc, fraction
    /// du temps écoulé)`, 1 = premier temps / premier bloc. Le **bloc** est le
    /// pas de segment choisi (`clip_beats`) : 4 temps pour 1 mesure, mais 8
    /// pour 2 mesures, 2 pour 2 temps, etc. `None` si rien ne joue (pause, fin)
    /// ou si la piste n'a pas de grille. Sert au voyant de battement, à la
    /// bande surlignée de la forme d'onde et aux points de la tuile qui sonne
    /// (demande user 21/11 : « le beat visible doit s'adapter si je mets 2
    /// mesures ou 2 temps »).
    fn beat_now(&self, g: &Tempo) -> Option<(i64, usize, f64)> {
        let p = self.player.as_ref()?;
        if p.is_paused() || p.finished() || g.beat <= 0.05 {
            return None;
        }
        Some(beat_at(
            p.position_abs(),
            g.phase,
            g.beat,
            self.clip_beats.max(1.0),
        ))
    }

    /// Voyant de battement : un point par temps de la mesure, celui qui sonne
    /// allumé (et qui s'éteint doucement pendant le temps), avec le numéro de
    /// mesure. `now` = `None` hors lecture : tous les points restent éteints.
    fn beat_lights(&self, ui: &mut egui::Ui, now: Option<(i64, usize, f64)>) {
        beat_lights(ui, now, self.clip_beats.max(1.0) as usize)
    }

    /// Fin de la zone découpée : la fin de la boucle quand la piste boucle
    /// (l'intro et ce qui suit la boucle ne sont pas des segments), sinon la
    /// durée de la piste — celle de la liste quand le rendu contient plusieurs
    /// tours (NSF rendu en 3 min pour une piste de 1:55).
    /// La boucle affichée est-elle un **choix de l'utilisateur** (case « boucle
    /// auto », bouton « poser une boucle », bords glissés) ? Sinon `loop_win`
    /// n'est que la **fin de découpe** (longueur du `.m3u`, rendu entier) : on
    /// ne dessine ni poignées ni libellés « boucle »/« fin » et on ne peut pas
    /// les saisir (vécu 21/11 : « il affiche la boucle alors que je n'ai pas
    /// cliqué sur poser une boucle »).
    fn shows_loop(&self) -> bool {
        self.auto_loop || self.loop_manual
    }

    fn cut_whole(&self, total: f64) -> f64 {
        if let Some(l) = self.loop_win {
            return l.end.clamp(0.0, total.max(0.0));
        }
        whole_of(
            self.track_ordinal
                .and_then(|i| self.tracks.get(i))
                .and_then(|t| t.length),
            total,
        )
    }

    /// Le premier clip est-il l'intro (ce qui précède la boucle) ?
    fn is_intro(&self, i: usize) -> bool {
        self.anchor > 0.25
            && i == 0
            && self
                .clips
                .first()
                .map(|c| c.start < 0.02 && c.end <= self.anchor + 0.6)
                .unwrap_or(false)
    }

    /// Remet à zéro tout ce qui appartient à une **piste** (découpe, tempo,
    /// boucle, forme d'onde, arrangement) : appelé à l'ouverture d'une archive,
    /// où plus aucune piste n'est chargée. Sans ça, l'écran Découpe montrait la
    /// boucle et les tranches du morceau précédent.
    fn clear_cut(&mut self) {
        self.tempo = None;
        self.tempo_req = None;
        self.tempo_failed = false;
        self.clips.clear();
        self.loop_win = None;
        self.loop_manual = false;
        self.anchor = 0.0;
        self.fmt_loop = None;
        self.auto_loop = false;
        self.loop_grab = None;
        self.cut_grab = None;
        self.cut_anchor = None;
        self.loop_snap = 0.0;
        self.envelope = None;
        self.peaks = None;
        self.bpm_manual = None;
        self.bpm_wheel = 0.0;
        self.shift_beats = 0;
        self.wave_zoom = 0.0;
        self.wave_center = 0.0;
        self.arrange.clear();
        self.seq.clear();
        self.seq_pos = 0;
        self.seq_from = 0;
        self.seq_loop = false;
        self.seq_jump = 0;
        self.pad_cur = None;
        self.pad_next = None;
        self.pads_held.clear();
    }

    /// Boucle par défaut du bouton « poser une boucle » : **4 mesures** à
    /// partir de la mesure la plus proche de la tête de lecture (0 si rien ne
    /// joue), bornée au rendu. C'est un point de départ : les bords se glissent
    /// ensuite dans la forme d'onde, calés sur le pas de découpe.
    fn default_loop(&self) -> Option<LoopWindow> {
        let total = self.player.as_ref()?.total();
        let g = self.grid()?;
        let pos = self.player.as_ref().map(|p| p.position_abs()).unwrap_or(0.0);
        manual_loop(total, g.phase, g.bars(1.0), pos)
    }

    /// Boucle « du fichier » (marqueur du rip, ou boucle trouvée dans le rendu
    /// de plusieurs tours), **sans** tenir compte du choix de l'utilisateur :
    /// sert à proposer la case « boucle auto » et à l'appliquer.
    fn detected_loop(&self) -> Option<LoopWindow> {
        if let Some((a, b)) = self.fmt_loop {
            return Some(LoopWindow { start: a, end: b });
        }
        let t = self.track_ordinal.and_then(|i| self.tracks.get(i))?;
        let end = t.loop_end?;
        let start = t.loop_start.unwrap_or(0.0);
        (end > start + 1.0 && end > 2.0).then_some(LoopWindow { start, end })
    }

    /// « sans l'intro » (case cochée) : l'intro **sort de la découpe**, elle
    /// n'est plus une tuile du tout. Cocher/décocher recoupe (demande user
    /// 21/11) : avant, la case ne filtrait que la suite par défaut et l'écran
    /// ne bougeait pas.
    fn apply_skip_intro(&mut self) {
        if self.skip_intro && self.is_intro(0) {
            self.clips.remove(0);
        }
    }

    /// Segments à jouer/assembler par défaut : la boucle, sans l'intro sauf si
    /// l'utilisateur la demande.
    fn base_seq(&self) -> Vec<usize> {
        let from = if self.skip_intro && self.is_intro(0) { 1 } else { 0 };
        (from..self.clips.len()).collect()
    }

    /// Forme d'onde du mode Découpe : les temps de la grille (fins), les
    /// mesures (plus marquées), les frontières de coupe réelles (accent) et la
    /// tête de lecture. C'est le seul moyen de repérer d'un coup d'œil une
    /// grille à côté de la musique — les PSF réverbérés donnent tous les tempos
    /// « plausibles » (vécu 21/11 : Dark Ruins 113 au lieu de 170).
    fn wave_strip(&mut self, ui: &mut egui::Ui, tempo: &Tempo, whole: f64) {
        let Some(peaks) = self.peaks.clone().filter(|p| !p.is_empty()) else {
            return;
        };
        let total = self.player.as_ref().map(|p| p.total()).unwrap_or(0.0);
        let span = whole.clamp(0.1, total.max(0.1));
        if total <= 0.1 || span <= 0.1 {
            return;
        }
        let h = 92.0;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), h),
            egui::Sense::click_and_drag(),
        );
        let v = ui.visuals().clone();
        let accent = v.selection.bg_fill;

        // --- fenêtre visible : molette = zoom autour du pointeur
        let mut len = if self.wave_zoom >= 1.0 {
            self.wave_zoom.clamp(1.0, span)
        } else {
            span
        };
        let start_now = if len >= span - 1e-6 {
            0.0
        } else {
            (self.wave_center - len / 2.0).clamp(0.0, (span - len).max(0.0))
        };
        if resp.hovered() {
            let (wheel, pinch) = ui.ctx().input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            // La molette zoome **proportionnellement** à son delta (~6 % par
            // cran de 50 pt) : 0,8 / 1,25 par événement était bien trop rapide
            // (demande user 21/11), et un trackpad qui envoie trente petits
            // deltas zoomait trente fois. Le pincement (`zoom_delta`) est déjà
            // un facteur multiplicatif par image.
            let factor = if (pinch - 1.0).abs() > 1e-3 {
                pinch as f64
            } else {
                (-(wheel as f64) * 0.0013).exp()
            }
            .clamp(0.75, 1.33);
            if (factor - 1.0).abs() > 1e-6 {
                let px = resp
                    .hover_pos()
                    .map(|p| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64)
                    .unwrap_or(0.5);
                let anchor = start_now + px * len;
                let new_len = (len * factor).clamp(1.0, span);
                let new_start = (anchor - px * new_len).clamp(0.0, (span - new_len).max(0.0));
                self.wave_zoom = if new_len >= span - 1e-6 { 0.0 } else { new_len };
                self.wave_center = new_start + new_len / 2.0;
                len = new_len;
            }
        }

        // Vue telle que le zoom l'a laissée : elle sert à tester les poignées
        // de boucle AVANT le glisser de vue, sinon le premier pixel du geste
        // déplacerait aussi la vue.
        let view_start = |center: f64, len: f64| {
            if len >= span - 1e-6 {
                0.0
            } else {
                (center - len / 2.0).clamp(0.0, (span - len).max(0.0))
            }
        };
        let start_zoom = view_start(self.wave_center, len);
        let map_zoom = |t: f64| -> f32 {
            rect.left() + (((t - start_zoom) / len).clamp(0.0, 1.0) as f32) * rect.width()
        };

        // une « boucle » qui n'est que la fin de découpe (longueur du `.m3u`)
        // n'a pas de poignées : rien à corriger, elle n'a pas été demandée
        let loopw = self.loop_win.filter(|_| self.shows_loop());
        // --- poignées de boucle : glisser un bord corrige la détection (demande
        // user 21/11 : « bouger la boucle qu'il détecte si il se trompe »).
        // La préhension se fait **sur la poignée du haut** (24 px) : ailleurs,
        // le glisser déplace la vue (demande user 21/11 : pouvoir se promener
        // dans le zoom sans attraper un bord au passage).
        let tol = 8.0;
        let grip = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.right(), rect.top() + 24.0),
        );
        // un bord hors de la fenêtre zoomée n'a pas de poignée visible : il ne
        // doit pas voler le glisser de vue au bord de l'écran
        let in_view = |t: f64| t >= start_zoom - 1e-9 && t <= start_zoom + len + 1e-9;
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.loop_grab = match (loopw, resp.interact_pointer_pos()) {
                (Some(l), Some(p)) if grip.contains(p) => {
                    let ds = if in_view(l.start) {
                        (p.x - map_zoom(l.start)).abs()
                    } else {
                        f32::INFINITY
                    };
                    let de = if in_view(l.end) {
                        (p.x - map_zoom(l.end)).abs()
                    } else {
                        f32::INFINITY
                    };
                    if ds <= tol && ds <= de {
                        Some(true)
                    } else if de <= tol {
                        Some(false)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if self.loop_grab.is_some() {
                // lignes de calage = celles qui sont dessinées à cet instant
                self.loop_snap = self.grid().map(|g| g.phase).unwrap_or(0.0);
            }
        }
        let mut loop_release = false;
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            loop_release = self.loop_grab.take().is_some();
        }
        // --- traits de **coupe** : les glisser déplace toute la grille (les
        // durées restent égales) ; au relâchement, le trait se cale sur le pas
        // du mode (1/2 temps, 1/2/4 mesures, demande user 21/11). Hors de la
        // poignée du haut, sinon on ne pourrait plus bouger la vue.
        let t_at = |x: f32| start_zoom + ((x - rect.left()) / rect.width().max(1.0)) as f64 * len;
        let cut_hit = |p: egui::Pos2| -> Option<f64> {
            if grip.contains(p) {
                return None;
            }
            self.clips
                .iter()
                .skip(1)
                .map(|c| c.start)
                .filter(|t| in_view(*t))
                .map(|t| (t, (p.x - map_zoom(t)).abs()))
                .filter(|(_, d)| *d <= 6.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(t, _)| t)
        };
        // pas de la grille : la longueur de segment choisie
        let cut_step = self
            .grid()
            .map(|g| g.beat * self.clip_beats.max(1.0))
            .unwrap_or(0.0);
        if resp.drag_started_by(egui::PointerButton::Primary) && self.loop_grab.is_none() {
            self.cut_grab = match resp.interact_pointer_pos() {
                Some(p) => cut_hit(p).map(|t| t - t_at(p.x)),
                None => None,
            };
            if self.cut_grab.is_some() {
                // le glisser est absolu : les boutons de décalage repartent de 0
                self.shift_beats = 0;
            }
        }
        let mut cut_release = false;
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            cut_release = self.cut_grab.take().is_some();
        }
        if resp.hovered() && self.loop_grab.is_none() && self.cut_grab.is_none() {
            if let Some(p) = resp.hover_pos() {
                if cut_hit(p).is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
        }
        if self.loop_grab.is_none() && resp.hovered() {
            if let (Some(l), Some(p)) = (loopw, resp.hover_pos()) {
                if grip.contains(p) {
                    let near_start = in_view(l.start) && (p.x - map_zoom(l.start)).abs() <= tol;
                    let near_end = in_view(l.end) && (p.x - map_zoom(l.end)).abs() <= tol;
                    if near_start || near_end {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                }
            }
        }

        // --- glisser = déplacer la vue (la grille est entièrement automatique ;
        // une poignée tenue a la priorité)
        if self.loop_grab.is_none()
            && self.cut_grab.is_none()
            && resp.dragged_by(egui::PointerButton::Primary)
        {
            let dx = resp.drag_delta().x as f64 / rect.width().max(1.0) as f64 * len;
            self.wave_center =
                (self.wave_center - dx).clamp(len / 2.0, (span - len / 2.0).max(len / 2.0));
        }

        // la grille dessinée est celle que la détection a donnée (ancrée sur la
        // boucle), pas une version corrigée à la main
        let tempo = &self.grid().unwrap_or(*tempo);
        let start = view_start(self.wave_center, len);
        let view_end = (start + len).min(span);
        let map = |t: f64| -> f32 {
            rect.left() + (((t - start) / len).clamp(0.0, 1.0) as f32) * rect.width()
        };

        // --- trait de coupe tenu : toute la grille suit. L'origine est
        // **normalisée dans [0, pas)** pour ne pas fabriquer une « intro » de
        // la taille du morceau (sinon `cut` sortirait une tranche 0 → t).
        if let (Some(off), Some(p)) = (self.cut_grab, resp.interact_pointer_pos()) {
            if cut_step > 0.05 {
                let t = t_at(p.x) + off;
                self.cut_anchor = Some(t.rem_euclid(cut_step));
                self.recut_clips();
            }
        }
        // relâchement du trait de coupe : la grille **reste où on l'a lâchée**
        // (au pixel). Le calage sur le pas du mode sautait de 2 mesures et
        // annulait le geste (retour user 22/11) ; il ne se fait plus que sur
        // demande, avec **Maj**.
        if cut_release {
            if let Some(a) = self.cut_anchor {
                let shift = ui.input(|i| i.modifiers.shift);
                if let (true, Some(base)) = (shift, self.tempo.map(|t| t.phase)) {
                    if cut_step > 0.05 {
                        let snapped = (a + ((base - a) / cut_step).round() * cut_step)
                            .rem_euclid(cut_step);
                        self.cut_anchor = Some(snapped);
                    }
                }
                self.recut();
                self.status = if shift {
                    format!(
                        "grille recalée sur le {} : {} segments",
                        beats_label(self.clip_beats),
                        self.clips.len()
                    )
                } else {
                    format!("grille déplacée : {} segments", self.clips.len())
                };
            }
        }

        // --- déplacement du bord tenu : la découpe et la boucle jouée suivent
        // en direct (l'aimant colle à la grille **détectée**, alt = libre)
        if let (Some(is_start), Some(ptr)) = (self.loop_grab, resp.interact_pointer_pos()) {
            if let Some(old) = loopw {
                let mut w = old;
                let frac = ((ptr.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
                let t = start + frac * len;
                // **Fin par défaut** : le bord suit le pointeur au pixel (donc
                // à la milliseconde sur un zoom serré). Le calage sur la
                // frontière de tranche (pas de 1/2 temps à 4 mesures) reste
                // disponible en tenant **Maj** — le saut de 2 mesures imposé à
                // chaque pixel rendait le geste inutilisable (retour user 22/11).
                let step = match self.grid() {
                    Some(g) => g.beat * self.clip_beats.max(1.0),
                    None => 0.0,
                };
                let shift = ui.input(|i| i.modifiers.shift);
                let t = gtracker_core::util::snap_edge(t, self.loop_snap, step, shift);
                let (a, b) = gtracker_core::util::move_edge((w.start, w.end), is_start, t, total);
                w = LoopWindow { start: a, end: b };
                if (w.start - old.start).abs() > 1e-9 || (w.end - old.end).abs() > 1e-9 {
                    self.loop_win = Some(w);
                    self.anchor = w.start;
                    self.recut_clips();
                    if self.looping {
                        if let Some(p) = self.player.as_mut() {
                            p.set_window(w);
                        }
                    }
                }
            }
        }
        // relâchement : la correction devient la boucle de la piste, mémorisée
        if loop_release {
            if let Some(w) = self.loop_win {
                if let (Some(id), Some(ord)) = (self.opened().map(|e| e.id), self.track_ordinal) {
                    let _ = self.db.set_loop_window(id, ord, w);
                }
                if let Some(t) = self.track_ordinal.and_then(|i| self.tracks.get(i)).cloned() {
                    self.gtracker
                        .set_source_loop(&t.file, t.subsong, (w.start, w.end));
                }
                self.recut();
                self.loop_manual = true;
                self.status = format!(
                    "boucle corrigée : {} → {} · {} segments",
                    util::fmt_time_cs(w.start),
                    util::fmt_time_cs(w.end),
                    self.clips.len()
                );
            }
        }

        let p = ui.painter_at(rect);
        p.rect_filled(
            rect,
            CornerRadius::same(8),
            v.extreme_bg_color.gamma_multiply(0.45),
        );
        p.rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color),
            egui::StrokeKind::Inside,
        );

        // bloc en cours de lecture (la tranche, au pas du mode choisi) : bande
        // teintée sous l'onde, pour suivre le battement sans fixer la tête de
        // lecture. Le pas est `clip_beats`, sinon une tuile de 2 mesures serait
        // surlignée sur la moitié de sa longueur (demande user 21/11).
        if let Some((bar, _, _)) = self.beat_now(tempo) {
            let bar_len = tempo.beat * self.clip_beats.max(1.0);
            let b0 = tempo.phase + (bar - 1) as f64 * bar_len;
            let b1 = b0 + bar_len;
            let x0 = rect.left()
                + (((b0 - start) / len).clamp(0.0, 1.0) as f32) * rect.width();
            let x1 = rect.left()
                + (((b1 - start) / len).clamp(0.0, 1.0) as f32) * rect.width();
            if x1 > x0 + 0.5 {
                p.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, rect.top()),
                        egui::pos2(x1, rect.bottom()),
                    ),
                    CornerRadius::same(2),
                    accent.gamma_multiply(0.10),
                );
            }
        }

        // forme d'onde : une colonne par pixel
        let mid = rect.center().y;
        let amp = (rect.height() * 0.46).max(1.0);
        let wave = v.weak_text_color().gamma_multiply(0.95);
        let n = peaks.len() as f64;
        let x0 = rect.left() + 1.0;
        let x1 = rect.right() - 1.0;
        let wpx = (x1 - x0).max(1.0);
        let mut x = x0;
        while x <= x1 {
            let t0 = start + ((x - x0) as f64 / wpx as f64) * len;
            let t1 = start + ((((x + 1.0) - x0) as f64) / wpx as f64) * len;
            if t1 > 0.0 && t0 < total {
                let b0 = ((t0.max(0.0) / total) * n) as usize;
                let b1 = (((t1.min(total) / total) * n) as usize).max(b0 + 1).min(peaks.len());
                let (mut lo, mut hi) = (0.0f32, 0.0f32);
                for b in b0.min(peaks.len())..b1 {
                    lo = lo.min(peaks[b].0);
                    hi = hi.max(peaks[b].1);
                }
                p.line_segment(
                    [
                        egui::pos2(x, mid - hi * amp),
                        egui::pos2(x, mid - lo * amp),
                    ],
                    Stroke::new(1.0, wave),
                );
            }
            x += 1.0;
        }

        // temps (fins) et mesures (marquées) : seulement ce qui est visible
        let beat = tempo.beat;
        if beat > 0.05 {
            let k0 = ((start - tempo.phase) / beat).ceil() as i64;
            let k1 = ((view_end - tempo.phase) / beat).floor() as i64;
            let bar = Tempo::BEATS_PER_BAR as i64;
            let thin = v.weak_text_color().gamma_multiply(0.30);
            // mesures : teintées de l'accent, plus larges que les temps — la
            // grille se lit d'un coup d'œil même sur un morceau chargé
            let thick = accent.gamma_multiply(0.55);
            // un numéro de mesure toutes les `n` mesures seulement si la place
            // le permet (au zoom large les numéros se chevaucheraient)
            let px_per_bar = beat * bar as f64 / len * rect.width() as f64;
            let label_every = if px_per_bar >= 44.0 {
                1
            } else if px_per_bar * 2.0 >= 44.0 {
                2
            } else if px_per_bar * 4.0 >= 44.0 {
                4
            } else {
                0
            };
            let mut k = k0;
            while k <= k1 && k <= k0 + 4000 {
                let t = tempo.phase + k as f64 * beat;
                if t >= start && t <= view_end {
                    let is_bar = k.rem_euclid(bar) == 0;
                    p.line_segment(
                        [
                            egui::pos2(map(t), rect.top() + 2.0),
                            egui::pos2(map(t), rect.bottom() - 2.0),
                        ],
                        Stroke::new(
                            if is_bar { 1.5 } else { 1.0 },
                            if is_bar { thick } else { thin },
                        ),
                    );
                    if is_bar && label_every > 0 {
                        let n = k.div_euclid(bar) + 1;
                        if n.rem_euclid(label_every as i64) == 1 {
                            p.text(
                                egui::pos2(map(t) + 3.0, rect.top() + 1.0),
                                egui::Align2::LEFT_TOP,
                                format!("{n}"),
                                FontId::proportional(9.5),
                                v.weak_text_color().gamma_multiply(0.8),
                            );
                        }
                    }
                }
                k += 1;
            }
        }

        // frontières de coupe
        for c in self.clips.iter().skip(1) {
            if c.start <= start || c.start >= view_end {
                continue;
            }
            p.line_segment(
                [
                    egui::pos2(map(c.start), rect.top()),
                    egui::pos2(map(c.start), rect.bottom()),
                ],
                Stroke::new(1.6, accent),
            );
        }

        // boucle : début et fin, chacun avec sa poignée de correction
        if let Some(l) = loopw {
            let warm = v.warn_fg_color;
            for (is_start, t) in [(true, l.start), (false, l.end)] {
                if t < start || t > view_end {
                    continue;
                }
                let x = map(t);
                let hot = self.loop_grab == Some(is_start)
                    || (self.loop_grab.is_none()
                        && resp
                            .hover_pos()
                            .map(|p| (p.x - x).abs() <= tol)
                            .unwrap_or(false));
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(
                        if is_start { 1.5 } else { 1.2 },
                        warm.gamma_multiply(if hot { 1.0 } else { 0.7 }),
                    ),
                );
                // poignée visible en haut (la zone de préhension)
                p.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(x, rect.top() + 6.0),
                        egui::vec2(7.0, 10.0),
                    ),
                    CornerRadius::same(2),
                    warm.gamma_multiply(if hot { 1.0 } else { 0.8 }),
                );
                p.text(
                    egui::pos2(x + 5.0, rect.top() + 2.0),
                    egui::Align2::LEFT_TOP,
                    if is_start { "boucle" } else { "fin" },
                    FontId::proportional(10.0),
                    warm,
                );
            }
        }

        // tête de lecture
        if let Some(player) = self.player.as_ref() {
            let t = player.position_abs();
            if t >= start && t <= view_end {
                p.line_segment(
                    [
                        egui::pos2(map(t), rect.top()),
                        egui::pos2(map(t), rect.bottom()),
                    ],
                    Stroke::new(1.8, v.strong_text_color()),
                );
            }
        }

        // survol : curseur + temps
        if let Some(pos) = resp.hover_pos() {
            p.line_segment(
                [egui::pos2(pos.x, rect.top()), egui::pos2(pos.x, rect.bottom())],
                Stroke::new(1.0, v.weak_text_color().gamma_multiply(0.5)),
            );
            let t = start + ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * len;
            p.text(
                egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                util::fmt_time_cs(t),
                FontId::monospace(11.0),
                v.weak_text_color(),
            );
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    "molette : zoom · glisser : déplacer la vue · poignée « boucle » / « fin » \
                     (en haut) : corriger au pixel · trait de coupe : décaler la grille \
                     (Maj = caler)",
                )
                .weak()
                .small(),
            );
            if self.wave_zoom >= 1.0 {
                ui.label(
                    RichText::new(format!("zoom {:.0} s", self.wave_zoom))
                        .small()
                        .color(accent),
                );
            }
        });
    }

    /// Mode DJ : découpe rythmique au centre, pads à gauche, suite à droite.
    ///
    /// Une tuile se **tient** au clic (elle boucle comme une touche, relâcher
    /// arrête en fin de tour) et se **glisse** vers la suite pour composer.
    fn dj_panel(&mut self, ui: &mut egui::Ui) {
        let title = self
            .track_ordinal
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "aucune piste".into());
        let Some(auto) = self.tempo else {
            if self.track_ordinal.is_some() {
                self.dj_waiting(ui);
            } else {
                self.dj_empty(ui);
            }
            return;
        };
        if self.clips.is_empty() {
            self.dj_empty(ui);
            return;
        }
        let tempo = self.grid().unwrap_or(auto);
        let whole = whole_of(
            self.track_ordinal
                .and_then(|i| self.tracks.get(i))
                .and_then(|t| t.length),
            self.player.as_ref().map(|p| p.total()).unwrap_or(0.0),
        );
        // segment qui sonne : le montage d'abord, sinon la piste elle-même
        // (avant, une lecture simple n'allumait aucune tuile : on entendait
        // sans voir où on en était, retour user 21/11)
        let current = self
            .pad_cur
            .or_else(|| self.seq.get(self.seq_pos).copied())
            .or_else(|| self.clip_at_player());
        let v = ui.visuals().clone();
        let accent = v.selection.bg_fill;

        // ---------------- en-tête : piste, tempo, longueur, grille
        let mut pick: Option<f64> = None;
        let mut shift: Option<i32> = None;
        // le bouton « par défaut » efface aussi la grille posée à la souris
        let mut cut_reset = false;
        let mut bpm_set: Option<f64> = None;
        let mut bpm_reset = false;
        let mut bpm_dirty = false;
        let mut cut_dirty = false;
        // le marqueur de boucle du fichier a changé (distinct de « sans
        // l'intro », qui ne doit pas écraser une boucle A/B enregistrée)
        let mut fmt_dirty = false;
        // la correction manuelle des bords est oubliée : retour à la boucle
        // détectée (bouton « auto »)
        let mut auto_reset = false;
        // boucle posée / retirée à la main (demande user 21/11 : une piste sans
        // boucle détectée doit pouvoir en recevoir une)
        let mut loop_place = false;
        let mut loop_remove = false;
        Frame::new()
            .fill(v.faint_bg_color)
            .corner_radius(10)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&title).strong().size(19.0));
                        ui.label(
                            RichText::new(format!(
                                "{} segments · {} de piste",
                                self.clips.len(),
                                util::fmt_time(whole)
                            ))
                            .weak()
                            .small(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let manual = self.bpm_manual.is_some();
                        let badge = Frame::new()
                            .fill(accent.gamma_multiply(if manual { 0.38 } else { 0.20 }))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(10, 4))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{:.1}", tempo.bpm))
                                        .strong()
                                        .monospace()
                                        .size(16.0),
                                );
                                ui.label(RichText::new("BPM").weak().small());
                            });
                        // Le badge capte le survol et le glisser : un seul widget
                        // pour les deux gestes. Deux widgets superposés se volent
                        // le survol (vécu 21/11 : le glisser ajouté par-dessus a
                        // rendu la molette muette).
                        let badge = ui
                            .interact(
                                badge.response.rect,
                                ui.scope_id().with("bpm-gestes"),
                                egui::Sense::click_and_drag(),
                            )
                            .on_hover_text(
                                "glisser (haut/bas) : ±0,1 BPM par pixel · maj : ×0,2 · \
                                 molette : ±0,5 · alt+molette : ×2 / ÷2\n« auto » revient au \
                                 tempo détecté",
                            );
                        if badge.hovered() || badge.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                        }
                        // glisser vertical : réglage fin (pavé tactile, souris sans
                        // roue). Le pas s'applique à la valeur courante à chaque
                        // image, donc pas de saut.
                        if badge.dragged_by(egui::PointerButton::Primary) {
                            let dy = badge.drag_delta().y as f64;
                            let fine = ui.ctx().input(|i| i.modifiers.shift);
                            let b = self.bpm_manual.unwrap_or(auto.bpm);
                            let next = (b - dy * if fine { 0.02 } else { 0.1 }).clamp(20.0, 400.0);
                            if (next - b).abs() > 1e-9 {
                                self.bpm_manual = Some(next);
                                bpm_dirty = true;
                            }
                        }
                        // molette : réglage du tempo (filet de sécurité quand la
                        // détection tombe à côté)
                        if badge.hovered() {
                            let (dy, shift, alt) = ui.ctx().input(|i| {
                                (i.smooth_scroll_delta.y, i.modifiers.shift, i.modifiers.alt)
                            });
                            if dy.abs() > 0.0 {
                                self.bpm_wheel += dy as f64;
                                while self.bpm_wheel.abs() >= 30.0 {
                                    let sign = self.bpm_wheel.signum();
                                    self.bpm_wheel -= sign * 30.0;
                                    let base = self.bpm_manual.unwrap_or(auto.bpm);
                                    let next = if alt {
                                        base * if sign > 0.0 { 2.0 } else { 0.5 }
                                    } else {
                                        base + sign * if shift { 0.1 } else { 0.5 }
                                    };
                                    bpm_set = Some(next);
                                }
                            }
                        }
                        if manual
                            && ui
                                .small_button("auto")
                                .on_hover_text("revient au tempo détecté automatiquement")
                                .clicked()
                        {
                            bpm_reset = true;
                        }
                        ui.add_space(10.0);
                        self.beat_lights(ui, self.beat_now(&tempo));
                        ui.label(
                            RichText::new(format!(
                                "1 mesure = {}",
                                util::fmt_time_cs(tempo.bars(1.0))
                            ))
                            .weak()
                            .small(),
                        );
                        // grille de secours (aucun pouls trouvé) : le dire, sinon
                        // l'utilisateur croit à une détection ratée sans le savoir
                        if tempo.confidence < 0.15 {
                            ui.label(
                                RichText::new("grille régulière")
                                    .weak()
                                    .small()
                                    .color(accent),
                            )
                            .on_hover_text(
                                "aucun rythme net sur cette piste : les segments sont \
                                 réguliers (120 BPM) et non musicaux — la forme d'onde \
                                 montre l'écart avec les attaques",
                            );
                        }
                    });
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Segment").weak().small());
                    for (beats, label) in [
                        (1.0, "1 temps"),
                        (2.0, "2 temps"),
                        (4.0, "1 mesure"),
                        (8.0, "2 mesures"),
                        (16.0, "4 mesures"),
                    ] {
                        let sel = (self.clip_beats - beats).abs() < 0.01;
                        if ui
                            .add(egui::Button::selectable(sel, label))
                            .on_hover_text(format!(
                                "chaque segment = {} ({} à ce tempo)",
                                beats_label(beats),
                                util::fmt_time_cs(tempo.beat * beats)
                            ))
                            .clicked()
                        {
                            pick = Some(beats);
                        }
                    }
                });
                // Départ des segments : sa **propre ligne**. Tout sur une seule
                // rangée (5 tailles + 6 flèches + boucle) débordait sur deux
                // lignes et devenait illisible (vécu 21/11). L'état est un badge
                // **neutre** : le texte en couleur d'accent restait collé aux
                // boutons et se lisait comme un bouton sélectionné de plus.
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Départ des segments").weak().small());
                    // où commence le premier segment (et pourquoi il est là)
                    let first = self.grid().and_then(|g| {
                        let len = g.beat * self.clip_beats.max(1.0);
                        (len > 0.05).then(|| {
                            if g.phase < 0.0 {
                                g.phase.rem_euclid(len)
                            } else {
                                g.phase
                            }
                        })
                    });
                    let at = first.map(util::fmt_time_cs).unwrap_or_else(|| "—".into());
                    let (txt, tip) = if self.shift_beats != 0 {
                        (
                            shift_label(self.shift_beats),
                            format!("origine décalée à la main ; premier segment à {at}"),
                        )
                    } else if self.cut_anchor.is_some() {
                        (
                            "posé à la souris".to_string(),
                            format!("grille déplacée dans la forme d'onde ; premier segment à {at}"),
                        )
                    } else {
                        (
                            "auto".to_string(),
                            format!(
                                "départ détecté (point de boucle ou première attaque) ; \
                                 premier segment à {at}"
                            ),
                        )
                    };
                    egui::Frame::new()
                        .fill(v.widgets.noninteractive.bg_fill)
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(7, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&txt).small().strong())
                                .on_hover_text(&tip);
                        });
                });
                // Les flèches sur leur **propre rangée** : six boutons ne
                // tiennent pas toujours à côté de l'état, et la rangée passe sur
                // une deuxième ligne plutôt que d'écraser les libellés
                // (« fais deux lignes de bouton si besoin », demande user 21/11).
                ui.horizontal_wrapped(|ui| {
                    let bar = Tempo::BEATS_PER_BAR as i32;
                    for (delta, text, hint) in [
                        (-bar, "− mesure", "une mesure plus tôt"),
                        (-2, "− 2 temps", "deux temps plus tôt"),
                        (-1, "− 1 temps", "un temps plus tôt"),
                        (1, "+ 1 temps", "un temps plus tard"),
                        (2, "+ 2 temps", "deux temps plus tard"),
                        (bar, "+ mesure", "une mesure plus tard"),
                    ] {
                        // décalage **cumulé** : on peut enchaîner les clics, le
                        // badge de la ligne du dessus dit toujours où on en est
                        if ui.button(text).on_hover_text(hint).clicked() {
                            shift = Some(self.shift_beats + delta);
                        }
                    }
                    if self.shift_beats != 0 || self.cut_anchor.is_some() {
                        // le seul retour en arrière : un vrai bouton, pas un « 0 »
                        if ui
                            .button("par défaut")
                            .on_hover_text(
                                "annule le décalage et la grille posée à la souris : \
                                 le départ revient là où la détection l'a mis",
                            )
                            .clicked()
                        {
                            shift = Some(0);
                            cut_reset = true;
                        }
                    }
                });
                // Boucle : sa propre ligne aussi (case « boucle auto », poser ou
                // retirer la boucle, « sans l'intro »)
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if self.detected_loop().is_some() {
                        if ui
                            .checkbox(&mut self.auto_loop, "boucle auto")
                            .on_hover_text(
                                "décoché (défaut) : la piste est découpée entière, sans intro \
                                 — le point de boucle d'un rip n'est pas toujours musical. \
                                 Cochée : la boucle détectée (marqueur du fichier, ou boucle \
                                 trouvée dans le rendu) borne la découpe et ancre la grille. \
                                 Choix retenu pour cette piste.",
                            )
                            .changed()
                        {
                            cut_dirty = true;
                            fmt_dirty = true;
                        }
                        // poignées déplacées : une boucle « A/B » en base masque
                        // la détection, « auto » la rend à la piste
                        if self.auto_loop && self.loop_win != self.detected_loop() {
                            if ui
                                .small_button("auto")
                                .on_hover_text(
                                    "revient à la boucle détectée pour cette piste : \
                                     efface la correction manuelle des bords",
                                )
                                .clicked()
                            {
                                auto_reset = true;
                            }
                        }
                    }
                    // boucle **manuelle** : pour les pistes sans boucle détectée
                    // (ou pour en poser une autre), et pour la retirer ensuite
                    if !self.auto_loop {
                        if self.loop_manual && self.loop_win.is_some() {
                            if ui
                                .small_button("retirer la boucle")
                                .on_hover_text(
                                    "la découpe repart sur la piste entière, la piste \
                                     redevient sans boucle",
                                )
                                .clicked()
                            {
                                loop_remove = true;
                            }
                        } else if ui
                            .small_button("poser une boucle")
                            .on_hover_text(
                                "pose une boucle de 4 mesures à partir de la lecture \
                                 (à défaut, depuis le début) : ses bords se glissent \
                                 ensuite dans la forme d'onde",
                            )
                            .clicked()
                        {
                            loop_place = true;
                        }
                    }
                    if self.is_intro(0) {
                        let intro = self.clips[0].len();
                        ui.add_space(12.0);
                        ui.separator();
                        if ui
                            .checkbox(
                                &mut self.skip_intro,
                                format!("sans l'intro ({})", util::fmt_time_cs(intro)),
                            )
                            .on_hover_text(
                                "l'intro (avant la boucle) sort de la découpe : cocher \
                                 ou décocher recoupe la piste",
                            )
                            .changed()
                        {
                            // la découpe change : le drapeau passe par `dirty`
                            // (un seul `recut` en fin de panneau)
                            cut_dirty = true;
                        }
                    }
                });

            });

        let mut dirty = false;
        if let Some(b) = pick {
            if (self.clip_beats - b).abs() > 1e-9 {
                self.clip_beats = b;
                dirty = true;
            }
        }
        if cut_reset {
            self.cut_anchor = None;
            dirty = true;
        }
        if let Some(b) = shift {
            if self.shift_beats != b {
                self.shift_beats = b;
                dirty = true;
            }
        }
        if auto_reset {
            self.auto_loop = true;
            self.loop_manual = false;
            self.loop_win = self.detected_loop();
            self.anchor = self.loop_win.map(|l| l.start).unwrap_or(0.0);
            if let (Some(id), Some(ord)) = (self.opened().map(|e| e.id), self.track_ordinal) {
                let _ = self.db.clear_loop_window(id, ord);
                let _ = self.db.set_auto_loop(id, ord, true);
            }
            dirty = true;
        }
        if loop_remove {
            self.loop_win = None;
            self.loop_manual = false;
            self.anchor = 0.0;
            if let (Some(id), Some(ord)) = (self.opened().map(|e| e.id), self.track_ordinal) {
                let _ = self.db.clear_loop_window(id, ord);
            }
            dirty = true;
            self.status = "boucle retirée : découpe sur la piste entière".into();
        }
        if loop_place {
            if let Some(w) = self.default_loop() {
                self.auto_loop = false;
                self.loop_manual = true;
                self.loop_win = Some(w);
                self.anchor = w.start;
                if let (Some(id), Some(ord)) = (self.opened().map(|e| e.id), self.track_ordinal) {
                    let _ = self.db.set_loop_window(id, ord, w);
                    let _ = self.db.set_auto_loop(id, ord, false);
                }
                dirty = true;
                self.status = format!(
                    "boucle posée : {} → {} · glisse les bords pour l'ajuster",
                    util::fmt_time_cs(w.start),
                    util::fmt_time_cs(w.end)
                );
            }
        }
        if fmt_dirty {
            // la boucle du fichier (ou son absence) pilote la boucle jouée,
            // l'ancre de grille et la fin de découpe : on les remet d'aplomb
            // avant de recouper
            self.loop_win = if self.auto_loop {
                self.detected_loop()
            } else {
                None
            };
            self.loop_manual = false;
            self.anchor = self.loop_win.map(|l| l.start).unwrap_or(0.0);
            if let (Some(id), Some(ord)) = (self.opened().map(|e| e.id), self.track_ordinal) {
                let _ = self.db.set_auto_loop(id, ord, self.auto_loop);
                // activer « boucle auto » veut dire « la boucle détectée » : une
                // correction manuelle (A/B en base) ne doit pas la masquer à la
                // réouverture de la piste
                if self.auto_loop {
                    let _ = self.db.clear_loop_window(id, ord);
                }
            }
        }
        if bpm_reset {
            self.bpm_manual = None;
            self.bpm_wheel = 0.0;
            bpm_dirty = true;
        }
        if let Some(b) = bpm_set {
            let b = b.clamp(20.0, 400.0);
            if self.bpm_manual != Some(b) {
                self.bpm_manual = Some(b);
                bpm_dirty = true;
            }
        }
        if bpm_dirty || cut_dirty {
            dirty = true;
        }
        if dirty {
            self.recut();
        }

        // ---------------- forme d'onde : grille et coupes à l'œil
        self.wave_strip(ui, &tempo, whole);

        // ---------------- corps : pads | suite
        ui.add_space(8.0);
        let bar_h = 44.0;
        let body_h = (ui.available_height() - bar_h - 12.0).max(150.0);
        ui.horizontal_top(|ui| {
            let suite_w = 286.0;
            let pads_w = (ui.available_width() - suite_w - 8.0).max(240.0);
            ui.allocate_ui_with_layout(
                egui::vec2(pads_w, body_h),
                Layout::top_down(Align::Min),
                |ui| self.pad_grid(ui, current),
            );
            ui.allocate_ui_with_layout(
                egui::vec2(suite_w, body_h),
                Layout::top_down(Align::Min),
                |ui| self.suite_panel(ui, body_h),
            );
        });
        ui.add_space(6.0);
        self.dj_transport(ui);

        // Fantôme sous le pointeur : une tuile ou une pastille en cours de
        // glissement est visible, on sait toujours ce qu'on tient.
        if let Some(d) = egui::DragAndDrop::payload::<DragSeg>(ui.ctx()) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let layer =
                    egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("gtracker-dj-drag-ghost"));
                let p = ui.ctx().layer_painter(layer);
                let strong = ui.visuals().strong_text_color();
                let rect = egui::Rect::from_min_size(
                    pos + egui::vec2(14.0, 12.0),
                    egui::vec2(86.0, 26.0),
                );
                p.rect(
                    rect,
                    CornerRadius::same(7),
                    accent,
                    Stroke::new(1.0, strong.gamma_multiply(0.6)),
                    egui::StrokeKind::Inside,
                );
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("seg {:02}", d.seg + 1),
                    FontId::proportional(12.5),
                    strong,
                );
            }
        }
    }

    /// Aucune piste (ou aucun rythme exploitable) : on explique quoi faire.
    /// Arrêt net : la musique et les patterns en cours (suite de lecture et
    /// pads). La **suite composée** (`arrange`) est conservée : c'est une
    /// composition, pas une lecture — le bouton « vider » s'en charge.
    /// Appelé par le bouton « stop » du transport **et** par le clic droit
    /// n'importe où dans la fenêtre (demande user 21/11).
    fn stop_all(&mut self) {
        if let Some(p) = self.player.as_mut() {
            p.halt();
        }
        self.seq.clear();
        self.seq_pos = 0;
        self.seq_from = 0;
        self.seq_loop = false;
        self.pad_cur = None;
        self.pad_next = None;
        self.pads_held.clear();
        self.ab_a = None;
        self.ab_b = None;
        self.status = "arrêt".into();
    }

    /// Demande la grille rythmique en tâche de fond, **une seule fois** par
    /// piste et par session (le résultat est ensuite dans l'entrée mémoire de
    /// `decode`). Rien n'est calculé tant qu'on ne va pas dans le mode Découpe.
    fn request_tempo(&mut self) {
        if self.tempo_failed || self.tempo.is_some() {
            return;
        }
        let (Some(track), Some(ord)) = (
            self.track_ordinal.and_then(|i| self.tracks.get(i)).cloned(),
            self.track_ordinal,
        ) else {
            return;
        };
        let Some(entry) = self.opened().cloned() else {
            return;
        };
        if self.tempo_req == Some((entry.id, ord)) {
            return;
        }
        self.tempo_req = Some((entry.id, ord));
        self.status = "Analyse du rythme…".into();
        self.submit(Job::Tempo {
            track,
            entry_id: entry.id,
            ordinal: ord,
        });
    }

    /// Mode Découpe pendant l'analyse : la piste joue déjà, la grille arrive.
    fn dj_waiting(&mut self, ui: &mut egui::Ui) {
        self.request_tempo();
        // la roue de chargement a besoin de tourner : 30 ms suffisent (~33 img/s)
        ui.ctx().request_repaint_after(Duration::from_millis(30));
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Découpe rythmique").strong().size(20.0));
            ui.add_space(6.0);
            let msg = if self.tempo_failed {
                "L'analyse du rythme a échoué sur cette piste."
            } else {
                "Analyse du rythme en cours (une seule fois par piste)…\n\n\
                 La lecture n'attend pas : la grille et les segments arrivent \
                 dès que l'analyse est finie."
            };
            ui.label(RichText::new(msg).weak());
            if !self.tempo_failed {
                // roue de chargement : l'analyse (réseau Beat This!) prend 1 à 3 s
                ui.add_space(14.0);
                ui.add(egui::Spinner::new().size(30.0).color(ui.visuals().selection.bg_fill));
            }
        });
    }

    fn dj_empty(&mut self, ui: &mut egui::Ui) {
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Découpe rythmique").strong().size(20.0));
            ui.add_space(6.0);
            let msg = if self.track_ordinal.is_some() {
                "Découpage en préparation…"
            } else {
                "Lance une piste depuis la bibliothèque : le tempo est détecté et la piste \
                 est découpée en temps/mesures, prête à jouer en pads."
            };
            ui.label(RichText::new(msg).weak());
            ui.add_space(12.0);
            if ui.button("ouvrir la bibliothèque").clicked() {
                self.shell.set_library(LibraryTab::Catalogue);
            }
        });
    }

    /// Grille des tuiles : tenir = boucler, glisser = composer la suite.
    fn pad_grid(&mut self, ui: &mut egui::Ui, current: Option<usize>) {
        let v = ui.visuals().clone();
        let clips = self.clips.clone();
        let arrange = self.arrange.clone();
        let intros: Vec<bool> = (0..clips.len()).map(|i| self.is_intro(i)).collect();
        // segments cliqués qui attendent leur tour : sans repère, on joue à
        // l'aveugle quand on en enchaîne plusieurs (demande user 21/11)
        let pending: Vec<usize> = if self.seq.is_empty() {
            Vec::new()
        } else {
            self.seq.iter().skip(self.seq_pos + 1).copied().collect()
        };
        // battement du segment qui sonne : un point par temps **du pas choisi**
        // (4 pour 1 mesure, 8 pour 2 mesures, 2 pour 2 temps), le temps courant
        // allumé (demande user 21/11). Même pas que le voyant de l'en-tête, donc
        // les deux sont toujours d'accord et s'adaptent ensemble.
        let pulse: Option<(usize, f32)> = current
            .and_then(|_| self.grid())
            .and_then(|g| self.beat_now(&g))
            .map(|(_, beat, frac)| (beat, frac as f32));
        let mut hit: Option<usize> = None;
        Frame::new()
            .fill(v.extreme_bg_color.gamma_multiply(0.55))
            .corner_radius(10)
            .inner_margin(egui::Margin::same(10))
            .stroke(Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for m in [ClickMode::Last, ClickMode::Queue] {
                        if ui
                            .selectable_label(self.click_mode == m, m.label())
                            .on_hover_text(m.hint())
                            .clicked()
                        {
                            self.click_mode = m;
                        }
                    }
                    if !pending.is_empty() {
                        ui.label(
                            RichText::new(format!("{} en attente", pending.len()))
                                .weak()
                                .small(),
                        );
                    }
                });
                let h = ui.available_height().max(80.0);
                egui::ScrollArea::vertical()
                    .id_salt("dj-pads")
                    .max_height(h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (i, c) in clips.iter().enumerate() {
                                let rank = pending.iter().position(|p| *p == i).map(|k| k + 1);
                                let resp = pad_tile(
                                    ui,
                                    i,
                                    *c,
                                    current == Some(i),
                                    if current == Some(i) { pulse } else { None },
                                    self.clip_beats.max(1.0) as usize,
                                    arrange.contains(&i),
                                    intros.get(i).copied().unwrap_or(false),
                                    rank,
                                );
                                if resp.clicked_by(egui::PointerButton::Primary) {
                                    hit = Some(i);
                                }
                                resp.dnd_set_drag_payload(DragSeg { seg: i, from: None });
                            }
                        });
                    });
            });
        if let Some(i) = hit {
            self.hit_tile(i);
        }
    }

    /// Clic sur une tuile : le segment se joue **une fois**. Un clic pendant
    /// qu'un autre joue l'enchaîne à la couture — donc en tempo, sans trou — et
    /// la chaîne s'arrête après le dernier segment cliqué.
    fn hit_tile(&mut self, i: usize) {
        if self.clips.get(i).is_none() {
            return;
        }
        // la souris prend la main : un pad clavier en cours s'arrête net
        self.stop_keyboard();
        if self.seq.is_empty() {
            // un enchaînement de clics joue **une fois** (pads souris)
            self.start_seq(vec![i], 0, false);
            self.status = format!(
                "seg {} — une fois · clique un autre segment pour enchaîner",
                i + 1
            );
            return;
        }
        // ce qui est programmé pour la couture dépend du mode : tout s'empile,
        // ou le dernier clic remplace celui qui attendait
        let program = click_plan(self.click_mode, &mut self.seq, self.seq_pos, i);
        if let Some(seg) = program {
            if let Some(w) = self.clips.get(seg).copied() {
                // la source saute dessus à la couture, sans passer par la GUI
                if let Some(p) = self.player.as_mut() {
                    p.queue_segment(w);
                }
            }
        }
        self.status = match self.click_mode {
            ClickMode::Queue => format!("seg {} en file · {} segments", i + 1, self.seq.len()),
            ClickMode::Last => format!("seg {} — prochain à la couture", i + 1),
        };
    }

    /// Coupe un pad clavier en cours (la souris reprend la main), sans toucher
    /// à la préférence de boucle.
    fn stop_keyboard(&mut self) {
        if self.pad_cur.is_some() || self.pad_next.is_some() {
            if let Some(p) = self.player.as_mut() {
                p.cancel_queued_loop();
            }
        }
        self.pad_cur = None;
        self.pad_next = None;
        self.pads_held.clear();
    }

    /// Suite : liste des segments de l'arrangement, réordonnable au glisser.
    fn suite_panel(&mut self, ui: &mut egui::Ui, body_h: f32) {
        let v = ui.visuals().clone();
        let accent = v.selection.bg_fill;
        let mut play_at: Option<usize> = None;
        let mut remove_at: Option<usize> = None;
        let mut clear = false;
        let mut rects: Vec<(usize, egui::Rect)> = Vec::new();
        let whole_len: f64 = if self.arrange.is_empty() {
            self.base_seq()
                .iter()
                .filter_map(|i| self.clips.get(*i))
                .map(|c| c.len())
                .sum()
        } else {
            self.arrange
                .iter()
                .filter_map(|i| self.clips.get(*i))
                .map(|c| c.len())
                .sum()
        };
        let frame = Frame::new()
            .fill(v.faint_bg_color)
            .corner_radius(10)
            .inner_margin(egui::Margin::same(10))
            .stroke(Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color));
        let (zone, dropped) = ui.dnd_drop_zone::<DragSeg, _>(frame, |ui| {
            ui.set_min_height((body_h - 26.0).max(90.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Montage").strong().size(15.0));
                let n = self.arrange.len();
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        if n == 0 {
                            "toute la découpe".to_string()
                        } else {
                            format!("{n} segments")
                        },
                        util::fmt_time(whole_len)
                    ))
                    .weak()
                    .small(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if n > 0 && ui.small_button("vider").clicked() {
                        clear = true;
                    }
                });
            });
            ui.add_space(6.0);
            if self.arrange.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("glisse une tuile ici\npour composer un enchaînement")
                            .weak()
                            .small(),
                    );
                });
                return;
            }
            let h = (ui.available_height() - 4.0).max(60.0);
            egui::ScrollArea::vertical()
                .id_salt("dj-suite")
                .max_height(h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (pos, seg) in self.arrange.iter().copied().enumerate() {
                        let clip = self.clips.get(seg).copied();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 28.0),
                            Sense::click_and_drag(),
                        );
                        // Seule l'occurrence qui sonne s'illumine : deux copies du
                        // même segment ne doivent pas s'allumer ensemble (vécu
                        // 21/11). Le test de segment évite une fausse pastille
                        // quand la suite est éditée en cours de lecture.
                        let hot = !self.seq.is_empty()
                            && self.seq_pos == pos
                            && self.seq.get(pos) == Some(&seg);
                        let fill = if hot {
                            accent
                        } else if resp.hovered() {
                            v.widgets.hovered.bg_fill
                        } else {
                            v.widgets.inactive.bg_fill.gamma_multiply(0.75)
                        };
                        let p = ui.painter();
                        p.rect(
                            rect,
                            CornerRadius::same(7),
                            fill,
                            Stroke::NONE,
                            egui::StrokeKind::Inside,
                        );
                        // filet d'accent à gauche
                        p.rect(
                            egui::Rect::from_min_size(
                                rect.left_top() + egui::vec2(0.0, 6.0),
                                egui::vec2(3.0, rect.height() - 12.0),
                            ),
                            CornerRadius::same(2),
                            accent.gamma_multiply(if hot { 1.0 } else { 0.6 }),
                            Stroke::NONE,
                            egui::StrokeKind::Inside,
                        );
                        p.text(
                            rect.left_center() + egui::vec2(12.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("seg {:02}", seg + 1),
                            FontId::proportional(13.0),
                            if hot {
                                v.strong_text_color()
                            } else {
                                v.text_color()
                            },
                        );
                        let badge = clip
                            .map(|c| util::fmt_time_cs(c.len()))
                            .unwrap_or_default();
                        p.text(
                            rect.right_center() + egui::vec2(-12.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            badge,
                            FontId::monospace(11.5),
                            v.weak_text_color(),
                        );
                        rects.push((pos, rect));
                        if resp.secondary_clicked() {
                            remove_at = Some(pos);
                            // geste volontaire sur cette ligne : pas d'arrêt
                            self.right_consumed = true;
                        } else if resp.clicked() {
                            play_at = Some(pos);
                        }
                        let resp = resp.on_hover_text(
                            "clic : écouter depuis ici · clic droit : retirer · \
                             glisser : déplacer",
                        );
                        resp.dnd_set_drag_payload(DragSeg {
                            seg,
                            from: Some(pos),
                        });
                    }
                });
        });

        // trait d'insertion : on voit **où** la pastille va atterrir (vécu
        // 21/11 : sans repère on ne sait pas si on tient quelque chose)
        if egui::DragAndDrop::has_payload_of_type::<DragSeg>(ui.ctx()) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                if zone.response.rect.contains(pos) && !self.arrange.is_empty() {
                    let idx = drop_index(&rects, Some(pos), self.arrange.len());
                    let y = match rects.get(idx) {
                        Some((_, r)) => r.top(),
                        None => rects
                            .last()
                            .map(|(_, r)| r.bottom())
                            .unwrap_or(zone.response.rect.center().y),
                    };
                    let x0 = zone.response.rect.left() + 12.0;
                    let x1 = zone.response.rect.right() - 12.0;
                    ui.painter().line_segment(
                        [egui::pos2(x0, y), egui::pos2(x1, y)],
                        Stroke::new(2.0, accent),
                    );
                }
            }
        }
        if let Some(d) = dropped {
            let p = ui.ctx().input(|i| i.pointer.interact_pos());
            let idx = drop_index(&rects, p, self.arrange.len());
            match d.from {
                Some(from) if from < self.arrange.len() => {
                    let seg = self.arrange.remove(from);
                    let at = if idx > from { idx - 1 } else { idx };
                    let at = at.min(self.arrange.len());
                    self.arrange.insert(at, seg);
                }
                _ => {
                    let at = idx.min(self.arrange.len());
                    self.arrange.insert(at, d.seg);
                }
            }
        }
        if clear {
            self.arrange.clear();
        }
        if let Some(at) = remove_at {
            // clic droit : on retire cette occurrence précise (les copies du
            // même segment restent indépendantes)
            if at < self.arrange.len() {
                self.arrange.remove(at);
            }
        }
        if let Some(at) = play_at {
            let seq = self.arrange.clone();
            // depuis la suite : ça boucle, on peut l'écouter en entier
            self.start_seq(seq, at, true);
        }
    }

    /// Barre de transport du mode Découpe (en bas, pleine largeur).
    fn dj_transport(&mut self, ui: &mut egui::Ui) {
        let v = ui.visuals().clone();
        let mut play_seq = false;
        let mut export = false;
        let mut stop = false;
        let seq: Vec<usize> = if self.arrange.is_empty() {
            self.base_seq()
        } else {
            self.arrange.clone()
        };
        let dur: f64 = seq
            .iter()
            .filter_map(|i| self.clips.get(*i))
            .map(|c| c.len())
            .sum();
        let label = if self.arrange.is_empty() {
            format!("toute la découpe · {}", util::fmt_time(dur))
        } else {
            format!("montage de {} segments · {}", seq.len(), util::fmt_time(dur))
        };
        Frame::new()
            .fill(v.faint_bg_color)
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let can = !self.busy && !self.clips.is_empty();
                    ui.add_enabled_ui(can, |ui| {
                        if djb(ui, "écouter", Some(Icon::Play), true).clicked() {
                            play_seq = true;
                        }
                        if djb(ui, "stop", Some(Icon::Stop), false).clicked() {
                            stop = true;
                        }
                        // un bouton par format : la pastille « sortie » puis
                        // « exporter » demandait deux gestes pour un seul
                        // résultat (demande user 21/11)
                        for f in [Format::Mp3Vbr, Format::Flac] {
                            let name = if f == Format::Flac { "flac" } else { "mp3" };
                            if djb(ui, name, Some(Icon::Export), false)
                                .on_hover_text(format!(
                                    "{}\nenregistre le montage en un seul fichier",
                                    f.hint()
                                ))
                                .clicked()
                            {
                                self.format = f;
                                export = true;
                            }
                        }
                    });
                    ui.separator();
                    ui.label(RichText::new(label).weak().small());
                });
            });
        if stop {
            self.stop_all();
        }
        if play_seq {
            self.start_seq(seq, 0, true);
        }
        if export {
            self.export_arrangement();
        }
    }

    /// Recalcule la découpe après un changement de longueur (même grille de
    /// tempo, autre pas) : la suite et les pads repartent de zéro.
    /// Recalcule les tranches sur la grille courante **sans** toucher à
    /// l'arrangement : utilisé pendant le glisser d'une poignée de boucle, où
    /// l'écran doit suivre le geste image par image.
    fn recut_clips(&mut self) {
        let total = self.player.as_ref().map(|p| p.total()).unwrap_or(0.0);
        let whole = self.cut_whole(total);
        self.clips = self.cut_now(whole);
        self.apply_skip_intro();
    }

    /// Découpe de la grille courante. Avec une origine **posée à la souris**,
    /// elle est rigide (`tempo::cut`, pas de réajustement sur les attaques —
    /// sinon les traits bougeraient sous le curseur) et la tuile d'intro
    /// partielle est **jetée** : une grille déplacée à la main ne doit pas
    /// fabriquer une tranche de longueur différente (demande user 21/11).
    fn cut_now(&self, whole: f64) -> Vec<LoopWindow> {
        let Some(t) = self.grid() else {
            return Vec::new();
        };
        if self.cut_anchor.is_none() {
            return tempo::cut_on(whole, &t, self.clip_beats, self.envelope.as_ref());
        }
        let mut clips = tempo::cut(whole, &t, self.clip_beats);
        let len = t.beat * self.clip_beats.max(1.0);
        if clips.len() > 1
            && clips[0].start <= 1e-9
            && (clips[0].len() - len).abs() > 0.05
        {
            clips.remove(0);
        }
        clips
    }

    fn recut(&mut self) {
        let Some(t) = self.grid() else {
            self.clips.clear();
            return;
        };
        let total = self.player.as_ref().map(|p| p.total()).unwrap_or(0.0);
        let whole = self.cut_whole(total);
        self.clips = self.cut_now(whole);
        self.apply_skip_intro();
        // une suite en cours n'a plus de sens sur la nouvelle grille : le
        // segment courant finit, la suite s'arrête (pas de saut vers un ancien
        // découpage)
        if !self.seq.is_empty() {
            if let Some(p) = self.player.as_mut() {
                p.cancel_queued_segment();
            }
        }
        self.arrange.clear();
        self.seq.clear();
        self.seq_pos = 0;
        self.seq_jump = 0;
                if self.pad_cur.is_some() {
            if let Some(p) = self.player.as_mut() {
                p.halt();
            }
            self.pad_cur = None;
            self.pad_next = None;
            self.pads_held.clear();
        }
        self.status = format!(
            "Découpe : {} segments de {} — ≈ {:.0} BPM",
            self.clips.len(),
            beats_label(self.clip_beats),
            t.bpm
        );
    }

    /// Enregistre l'arrangement courant en un seul MP3/FLAC (le format est
    /// celui du sélecteur « Export » juste en dessous).
    fn export_arrangement(&mut self) {
        // suite vide = toute la découpe (le morceau rejoué par tranches)
        let indices: Vec<usize> = if self.arrange.is_empty() {
            self.base_seq()
        } else {
            self.arrange.clone()
        };
        let spans: Vec<(f64, f64)> = indices
            .iter()
            .filter_map(|i| self.clips.get(*i))
            .map(|c| (c.start, c.end))
            .collect();
        if spans.is_empty() {
            return;
        }
        let (Some(entry), Some(ord)) = (self.opened().cloned(), self.track_ordinal) else {
            return;
        };
        let Some(track) = self.tracks.get(ord).cloned() else {
            return;
        };
        let name = format!("{} (montage)", track.title);
        let tags = Tags {
            title: Some(name.clone()),
            artist: self.composer.clone(),
            album: self.album.clone(),
        };
        self.busy = true;
        self.progress = 0.0;
        self.status = "Export du montage…".into();
        self.submit(Job::ConvertSegments {
            entry,
            ordinal: ord,
            spans,
            out: PathBuf::from(&self.out_dir),
            format: self.format,
            tags,
            name,
        });
    }

    fn player_bar(&mut self, ui: &mut egui::Ui) {
        self.ensure_library_sound(false);
        let title = self
            .track_ordinal
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "—".to_string());
        // le nom affiché est celui du jeu ouvert, quel que soit l'onglet
        let archive = self
            .opened()
            .map(|e| {
                self.entries
                    .iter()
                    .position(|x| x.id == e.id)
                    .map(|i| self.rows[i].title.clone())
                    .unwrap_or_else(|| e.display_name())
            })
            .unwrap_or_default();
        let track_len = self
            .track_ordinal
            .and_then(|i| self.tracks.get(i))
            .and_then(|t| t.length);

        // lues avant l'emprunt mutable du lecteur
        let tempo = self.grid();
        // battement : la mesure et le temps de la piste **en cours**, visibles
        // dans toutes les vues (demande user 21/11)
        let beat_now = tempo.and_then(|g| self.beat_now(&g));
        let beats_n = self.clip_beats.max(1.0) as usize;
        let open_id = self.opened().map(|e| e.id);
        let Some(player) = self.player.as_mut() else {
            ui.add_space(10.0);
            ui.label(RichText::new("Périphérique audio indisponible").weak());
            return;
        };

        // Suite sans trou : la source saute elle-même au segment programmé à la
        // fin du segment courant (`queue_segment`). Le compteur `jumps` dit à la
        // GUI où elle en est ; elle programme le suivant dès qu'elle l'apprend.
        if !self.seq.is_empty() {
            let jumps = player.segment_jumps();
            if jumps != self.seq_jump {
                self.seq_jump = jumps;
                let next = seq_next(self.seq.len(), self.seq_from, self.seq_loop, self.seq_pos);
                let clip = next.and_then(|n| self.seq.get(n)).and_then(|i| self.clips.get(*i)).copied();
                match (next, clip) {
                    (Some(next), Some(w)) => {
                        self.seq_pos = next;
                        self.ab_a = Some(w.start);
                        self.ab_b = Some(w.end);
                        // on programme toujours celui d'après : c'est ce qui
                        // fait tourner la suite en boucle sans trou
                        if let Some(w2) = seq_next(
                            self.seq.len(),
                            self.seq_from,
                            self.seq_loop,
                            next,
                        )
                        .and_then(|n| self.seq.get(n))
                        .and_then(|i| self.clips.get(*i))
                        .copied()
                        {
                            player.queue_segment(w2);
                        }
                        self.status = format!(
                            "Montage {}/{} — {} – {}",
                            next + 1,
                            self.seq.len(),
                            util::fmt_time_cs(w.start),
                            util::fmt_time_cs(w.end)
                        );
                    }
                    _ => {
                        self.seq.clear();
                        self.seq_pos = 0;
                        self.seq_from = 0;
                        self.seq_loop = false;
                        self.pad_cur = None;
                        self.pad_next = None;
                        self.pads_held.clear();
                        self.status = "montage terminé".into();
                    }
                }
            } else if player.exhausted() {
                // la source s'est arrêtée sans rien avoir en file : en boucle on
                // repart du premier segment, sinon la suite est finie
                let at = if self.seq_loop && !self.seq.is_empty() {
                    Some(self.seq_from.min(self.seq.len() - 1))
                } else {
                    None
                };
                match at.and_then(|a| self.seq.get(a).and_then(|i| self.clips.get(*i)).copied().map(|w| (a, w))) {
                    Some((at, w)) => {
                        player.play_segment(w.start, w.end);
                        if let Some(w2) = seq_next(self.seq.len(), self.seq_from, self.seq_loop, at)
                            .and_then(|n| self.seq.get(n))
                            .and_then(|i| self.clips.get(*i))
                            .copied()
                        {
                            player.queue_segment(w2);
                        }
                        self.seq_pos = at;
                        self.seq_jump = player.segment_jumps();
                        self.ab_a = Some(w.start);
                        self.ab_b = Some(w.end);
                        self.status = format!(
                            "Montage (boucle) {}/{} — {} – {}",
                            at + 1,
                            self.seq.len(),
                            util::fmt_time_cs(w.start),
                            util::fmt_time_cs(w.end)
                        );
                    }
                    None => {
                        self.seq.clear();
                        self.seq_pos = 0;
                        self.seq_from = 0;
                        self.seq_loop = false;
                        self.pad_cur = None;
                        self.pad_next = None;
                        self.pads_held.clear();
                        self.status = "montage terminé".into();
                    }
                }
            }
        }

        let total = player.total();
        let pos = player.position();
        let window = player.window();
        // état réel de la source (barre de temps) et préférence utilisateur
        // (bouton) : les pads bouclent sans toucher la préférence
        let live_loop = player.looping();
        let pref_loop = self.looping;
        let paused = player.is_paused();
        let volume = player.volume();
        let in_seq = !self.seq.is_empty();
        let clip_now = self
            .seq
            .get(self.seq_pos)
            .and_then(|i| self.clips.get(*i))
            .copied();
        // région mise en évidence : le segment joué, sinon la fenêtre de boucle
        let region = clip_now.unwrap_or(window);

        // Axe de la barre de temps : le cycle de boucle quand il est plus court
        // que le rendu (PSF rendu en plusieurs tours, genh…), sinon la durée de
        // la piste (celle de la liste) — zxtune rend 3 min fixes pour un NSF de
        // 1:55. Le curseur warp au point de reprise en fin de cycle.
        let cycle = !in_seq && live_loop && window.end > window.start + 1.0 && window.end < total - 1.0;
        let axis_start = if cycle { window.start } else { 0.0 };
        let axis_end = if cycle {
            window.end
        } else if live_loop {
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
                if tempo.is_some() {
                    beat_lights(ui, beat_now, beats_n);
                    ui.add_space(8.0);
                }
                if total > 0.0 {
                    if let Some(c) = clip_now {
                        let text = format!(
                            "montage {}/{} · {} – {}",
                            self.seq_pos + 1,
                            self.seq.len(),
                            util::fmt_time_cs(c.start),
                            util::fmt_time_cs(c.end)
                        );
                        ui.label(RichText::new(text).small());
                    } else if live_loop {
                        let full = window.start < 0.5 && (window.end - whole).abs() <= 1.0;
                        let mut text = if full {
                            "boucle : piste entière".to_string()
                        } else {
                            format!(
                                "boucle {} – {} ({})",
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
                }
            });
        });

        let mut seek = None;
        if total > 0.0 {
            let shown = LoopWindow {
                start: region.start - axis_start,
                end: region.end - axis_start,
            };
            let highlight = live_loop || clip_now.is_some();
            if let Some(t) = timeline(
                ui,
                pos - axis_start,
                axis_end - axis_start,
                shown,
                highlight,
                tempo,
                axis_start,
            ) {
                seek = Some(t + axis_start);
            }
        } else {
            ui.add_space(14.0);
        }

        let mut toggle_pause = false;
        let mut toggle_loop = false;
        let mut jump = None;
        let mut vol = None;

        ui.horizontal(|ui| {
            if ui.add_enabled(total > 0.0, egui::Button::new("Début")).clicked() {
                jump = Some(0.0);
            }
            if ui.add_enabled(total > 0.0, egui::Button::new("-5 s")).clicked() {
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
                .selectable_label(pref_loop, RichText::new(if pref_loop { "boucle ON" } else { "boucle OFF" }))
                .on_hover_text("L")
                .clicked()
            {
                toggle_loop = true;
            }
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

        let mut set_a = false;
        let mut set_b = false;
        let mut apply_ab = false;
        let mut whole_loop = false;
        let mut magnet = self.magnet;
        let mut open_cut = false;
        let editable = total > 0.0;

        ui.horizontal(|ui| {
            if ui.add_enabled(editable, egui::Button::new("A = ici")).clicked() {
                set_a = true;
            }
            if ui.add_enabled(editable, egui::Button::new("B = ici")).clicked() {
                set_b = true;
            }
            if ui
                .add_enabled(editable, egui::Button::new("appliquer"))
                .on_hover_text("boucle la fenêtre A-B et la mémorise pour la piste")
                .clicked()
            {
                apply_ab = true;
            }
            if ui
                .add_enabled(editable, egui::Button::new("piste entière"))
                .on_hover_text("revient à la piste complète et efface le A/B enregistré")
                .clicked()
            {
                whole_loop = true;
            }
            ui.add(egui::Checkbox::new(&mut magnet, "aimant"))
                .on_hover_text("A/B collent au temps le plus proche");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = self.clips.len();
                let text = if n > 0 {
                    format!("Découpe · {n} segments")
                } else {
                    "Découpe".to_string()
                };
                if ui
                    .add(egui::Button::new(RichText::new(text).strong()))
                    .on_hover_text("mode DJ : pads, montage, export")
                    .clicked()
                {
                    open_cut = true;
                }
                if let Some(t) = tempo {
                    ui.label(
                        RichText::new(format!("≈ {:.0} BPM", t.bpm))
                            .small()
                            .monospace()
                            .weak(),
                    );
                }
            });
        });
        if open_cut {
            self.shell.set_workspace(Workspace::Edit);
        }
        self.magnet = magnet;

        let snap = |t: f64| match (tempo, magnet) {
            (Some(g), true) => g.snap(t),
            _ => t,
        };

        if set_a {
            self.ab_a = Some(snap(pos));
            self.ab_msg = None;
        }
        if set_b {
            self.ab_b = Some(snap(pos));
            self.ab_msg = None;
        }
        if apply_ab {
            let a = snap(self.ab_a.unwrap_or(region.start));
            let b = snap(self.ab_b.unwrap_or(region.end));
            let w = LoopWindow {
                start: a.min(b),
                end: a.max(b),
            };
            player.set_window(w);
            player.set_looping(true);
            self.looping = true;
            self.seq.clear();
            self.pad_cur = None;
            self.pad_next = None;
            self.pads_held.clear();
            if let (Some(id), Some(ord)) = (open_id, self.track_ordinal) {
                let _ = self.db.set_loop_window(id, ord, w);
            }
            self.ab_msg = Some("boucle A/B active et enregistrée pour cette piste".into());
            self.status = "Lecture — boucle A/B".into();
        }
        if whole_loop {
            // retour à la piste : le A/B enregistré est oublié, sinon il
            // reviendrait à la prochaine lecture (vécu 21/11)
            player.set_window(LoopWindow {
                start: 0.0,
                end: whole,
            });
            self.ab_a = None;
            self.ab_b = None;
            self.seq.clear();
            self.seq_pos = 0;
            self.pad_cur = None;
            self.pad_next = None;
            self.pads_held.clear();
            if let (Some(id), Some(ord)) = (open_id, self.track_ordinal) {
                let _ = self.db.clear_loop_window(id, ord);
            }
            self.ab_msg = Some("boucle : piste entière (A/B effacé)".into());
            self.status = "Lecture — piste entière".into();
        }
        if toggle_pause {
            if self.auditioning {
                // on interrompt l'écoute et la piste reprend **à sa position**,
                // pas à zéro (sinon la sortie resterait vide)
                let at = self.lib_audio.as_ref().map(|l| l.pos).unwrap_or(0.0);
                player.resume_at(at);
                self.auditioning = false;
            } else {
                player.toggle_pause();
            }
        }
        if toggle_loop {
            let now = !pref_loop;
            player.set_looping(now);
            self.looping = now;
            // quitter une suite et les pads : la boucle prend le relais
            self.seq.clear();
            self.pad_cur = None;
            self.pad_next = None;
            self.pads_held.clear();
            self.status = if now {
                "Lecture — boucle active".into()
            } else {
                "Lecture — boucle coupée".into()
            };
        }
        if let Some(t) = seek.or(jump) {
            player.seek(t);
            if seek.is_some() {
                self.seq.clear();
                self.pad_cur = None;
                self.pad_next = None;
                self.pads_held.clear();
            }
        }
        if let Some(v) = vol {
            player.set_volume(v);
            self.volume = v;
        }
        if let Some(m) = &self.ab_msg {
            ui.label(RichText::new(m).small().weak());
        }
    }

    /// Démarre `seq[at..]` : la suite s'enchaîne segment après segment.
    fn start_seq(&mut self, seq: Vec<usize>, at: usize, looped: bool) {
        let Some(w) = seq.get(at).and_then(|i| self.clips.get(*i)).copied() else {
            return;
        };
        let Some(player) = self.player.as_mut() else {
            return;
        };
        player.play_segment(w.start, w.end);
        // programme tout de suite le suivant : la source enchaîne à la couture
        // (en boucle, le « suivant » du dernier est le premier de la suite)
        if let Some(next) =
            seq_next(seq.len(), at, looped, at).and_then(|n| seq.get(n)).and_then(|i| self.clips.get(*i)).copied()
        {
            player.queue_segment(next);
        }
        self.seq_jump = player.segment_jumps();
        self.ab_a = Some(w.start);
        self.ab_b = Some(w.end);
        self.status = format!(
            "Montage {}/{} — {} – {}",
            at + 1,
            seq.len(),
            util::fmt_time_cs(w.start),
            util::fmt_time_cs(w.end)
        );
        self.seq = seq;
        self.seq_pos = at;
        self.seq_from = at;
        self.seq_loop = looped;
        self.pad_cur = None;
        self.pad_next = None;
        self.pads_held.clear();
            }

}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // ouverture **maximisée** : `with_maximized(true)` ne suffit pas sous
        // KWin/XWayland, la fenêtre reste à sa taille intérieure (vécu 22/11)
        if !self.maximized_once {
            self.maximized_once = true;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        // lu en début de trame et appliqué en fin : une ligne de la suite peut
        // consommer le clic droit (retirer l'occurrence) sans arrêter la lecture
        let right_click = ui.ctx().input(|i| i.pointer.secondary_clicked());
        self.right_consumed = false;
        // fichier audio déposé sur la fenêtre (mp3, flac, wav, ogg… ou format
        // de jeu) : ouvert en mode Découpe, prêt à être découpé (demande user
        // 21/11). Le premier fichier reconnu gagne.
        let dropped = ui
            .ctx()
            .input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.iter().find_map(|f| {
            let p = f.path();
            (util::is_playable_file(p) && p.is_file()).then(|| p.to_path_buf())
        }) {
            if self.shell.workspace == Workspace::Arrange {
                self.gtracker_drop(path);
            } else {
                self.open_file(path);
            }
        }
        self.style(ui.ctx());
        self.pump();
        if self.query != self.last_query
            || self.console_filter != self.last_console
            || self.sort != self.last_sort
        {
            self.refilter();
        }
        if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            // chercher, c'est aller dans la bibliothèque : on l'ouvre au lieu
            // de laisser un champ caché prendre le focus
            self.shell.show_library = true;
            self.shell.set_library(LibraryTab::Catalogue);
            self.focus_search = true;
        }
        // enregistrer depuis n'importe quel panneau (l'arrangement a son propre
        // raccourci, l'éditeur n'en avait pas)
        if ui
            .ctx()
            .input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
            && self.shell.workspace == Workspace::Edit
        {
            self.gtracker.save_project();
        }
        self.keyboard(ui.ctx());
        self.pads(ui.ctx());

        // raccourcis de coquille, valables dans les deux espaces
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.switch_workspace(Workspace::Arrange);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
            self.switch_workspace(Workspace::Edit);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F7)) {
            self.shell.toggle_inspector();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F8)) {
            self.shell.toggle_library();
        }

        egui::Panel::top("appbar")
            .frame(
                Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(12, 2)),
            )
            .show(ui, |ui| self.app_bar(ui));

        // une seule barre d'état : celle du document en Arrangement, celle de
        // la lecture en Éditeur — jamais deux vérités affichées
        egui::Panel::bottom("status")
            .exact_size(24.0)
            .frame(
                Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(12, 2)),
            )
            .show(ui, |ui| {
                if self.shell.workspace == Workspace::Arrange {
                    self.gtracker.status_bar(ui);
                } else {
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
                }
            });

        // **Le** transport : celui du projet en Arrangement, celui de la
        // pré-écoute en Éditeur. Un seul, toujours à la même place.
        match self.shell.workspace {
            Workspace::Arrange => {
                egui::Panel::bottom("transport_arrange")
                    .exact_size(40.0)
                    .frame(
                        Frame::new()
                            .fill(theme::CHROME)
                            .inner_margin(egui::Margin::symmetric(12, 4)),
                    )
                    .show(ui, |ui| {
                        self.gtracker.transport_bar(ui, &mut self.player);
                    });
            }
            Workspace::Edit => {
                egui::Panel::bottom("transport_edit")
                    .exact_size(140.0)
                    .frame(
                        Frame::new()
                            .fill(theme::CHROME)
                            .inner_margin(egui::Margin::symmetric(12, 6)),
                    )
                    .show(ui, |ui| self.player_bar(ui));
            }
        }

        let side_frame = || {
            Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 8))
        };
        let third = (ui.available_width() / 3.0).max(300.0);
        if self.shell.show_library {
            egui::Panel::left("library")
                .resizable(true)
                .default_size(380.0_f32.min(third))
                .min_size(300.0)
                .max_size(620.0_f32.min(third))
                .frame(side_frame())
                .show(ui, |ui| self.library_panel(ui));
        } else {
            egui::Panel::left("library_rail")
                .exact_size(26.0)
                .frame(Frame::new().fill(theme::CHROME))
                .show(ui, |ui| self.closed_rail(ui, true));
        }
        if self.shell.show_inspector {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(340.0_f32.min(third))
                .min_size(280.0)
                .max_size(520.0_f32.min(third))
                .frame(side_frame())
                .show(ui, |ui| self.inspector_panel(ui));
        } else {
            egui::Panel::right("inspector_rail")
                .exact_size(26.0)
                .frame(Frame::new().fill(theme::CHROME))
                .show(ui, |ui| self.closed_rail(ui, false));
        }

        egui::CentralPanel::default().show(ui, |ui| match self.shell.workspace {
            Workspace::Arrange => {
                if self.player.is_none() {
                    self.player = Player::new().ok();
                }
                let mut cmds = Vec::new();
                // le centre EST l'arrangement + son éditeur de source : les
                // deux panneaux restent visibles, comme dans un DAW
                egui::Panel::bottom("gt_editor")
                    .resizable(true)
                    .default_size(230.0)
                    .min_size(130.0)
                    .show(ui, |ui| self.gtracker.source_panel(ui, &mut cmds));
                self.gtracker.ui_center(ui, &mut self.player);
                for c in cmds {
                    self.gtracker_cmd(c);
                }
            }
            Workspace::Edit => self.dj_panel(ui),
        });

        let want = match (self.shell.workspace, self.track_ordinal.and_then(|i| self.tracks.get(i))) {
            (Workspace::Arrange, _) => {
                format!("GTracker — {}", self.gtracker.project_name())
            }
            (Workspace::Edit, Some(t)) => format!(
                "GTracker — {} · {}",
                self.gtracker.project_name(),
                t.title
            ),
            (Workspace::Edit, None) => {
                format!("GTracker — {}", self.gtracker.project_name())
            }
        };
        if want != self.window_title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(want.clone()));
            self.window_title = want;
        }

        // clic droit **n'importe où** : arrêt de la musique et des patterns en
        // cours (demande user 21/11)
        // clic droit dans le GTracker = menu du clip, pas l'arrêt global
        // clic droit dans l'éditeur : arrêt ; en Arrangement il appartient au
        // menu du clip et ne doit pas couper le projet
        if right_click && !self.right_consumed && self.shell.workspace == Workspace::Edit {
            self.stop_all();
        }
        // le voyant de battement a besoin de plus de finesse que la barre de
        // temps : 30 ms (~33 img/s) pendant la lecture, 60 ms au repos
        let playing = self
            .player
            .as_ref()
            .map(|p| !p.is_paused() && !p.finished())
            .unwrap_or(false);
        ui.ctx()
            .request_repaint_after(Duration::from_millis(if playing { 30 } else { 60 }));
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

/// Clé de tri de la liste « Téléchargés » : titre en minuscules d'abord (A → Z
/// toutes consoles confondues), console en second pour départager les titres
/// identiques entre deux consoles.
fn cached_sort_key(title: &str, console: &str) -> (String, String) {
    (title.to_lowercase(), console.to_string())
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

/// Barre de temps cliquable ; la région de boucle (ou le segment joué) est
/// teintée et, quand le zoom le permet, la grille de temps est tracée.
fn timeline(
    ui: &mut egui::Ui,
    pos: f64,
    total: f64,
    window: LoopWindow,
    highlight: bool,
    tempo: Option<Tempo>,
    axis_start: f64,
) -> Option<f64> {
    let width = ui.available_width().max(80.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 14.0), Sense::click_and_drag());
    let painter = ui.painter();
    let bg = ui.visuals().extreme_bg_color;
    let loop_col = ui.visuals().selection.bg_fill.gamma_multiply(0.45);
    let cursor = ui.visuals().strong_text_color();
    painter.rect_filled(rect, CornerRadius::same(4), bg);
    if total > 0.0 {
        if highlight && window.end > window.start {
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
        // grille de temps : seulement si un trait fait au moins 6 px, sinon
        // c'est une masse grise illisible (PSF de 3 min à 140 BPM)
        if let Some(t) = tempo {
            if t.beat > 0.0 && t.beat * rect.width() as f64 / total >= 6.0 {
                let first = ((axis_start - t.phase) / t.beat).ceil() as i64;
                let mut k = first;
                loop {
                    let bt = t.phase + k as f64 * t.beat;
                    if bt > axis_start + total + 1e-9 {
                        break;
                    }
                    if bt >= axis_start - 1e-9 {
                        let x = rect.left()
                            + ((bt - axis_start) / total).clamp(0.0, 1.0) as f32 * rect.width();
                        let strong = k.rem_euclid(4) == 0;
                        let col = ui
                            .visuals()
                            .text_color()
                            .gamma_multiply(if strong { 0.24 } else { 0.10 });
                        painter.line_segment(
                            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                            Stroke::new(1.0, col),
                        );
                    }
                    k += 1;
                }
            }
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
    // bouton **primaire** seulement : un clic droit sur la barre de temps ne
    // doit ni déplacer la lecture ni couper une suite en cours (vécu 21/11)
    if resp.clicked_by(egui::PointerButton::Primary)
        || resp.dragged_by(egui::PointerButton::Primary)
    {
        if let Some(p) = resp.interact_pointer_pos() {
            let frac = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            return Some(frac as f64 * total);
        }
    }
    None
}

/// Index suivant dans une suite. En boucle, le « suivant » du dernier segment
/// est son premier (`from`) : c'est ce qui enchaîne sans trou et sans fin.
fn seq_next(len: usize, from: usize, looped: bool, i: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if i + 1 < len {
        return Some(i + 1);
    }
    if looped {
        Some(from.min(len - 1))
    } else {
        None
    }
}

/// Touches des pads live, dans l'ordre des segments. Les chiffres d'abord (le
/// pavé numérique envoie les mêmes touches), puis les rangées de lettres ; R
/// (retour au début) et L (boucle) restent au transport.
const PAD_KEYS: [(egui::Key, &str); 34] = [
    (egui::Key::Num1, "1"),
    (egui::Key::Num2, "2"),
    (egui::Key::Num3, "3"),
    (egui::Key::Num4, "4"),
    (egui::Key::Num5, "5"),
    (egui::Key::Num6, "6"),
    (egui::Key::Num7, "7"),
    (egui::Key::Num8, "8"),
    (egui::Key::Num9, "9"),
    (egui::Key::Num0, "0"),
    (egui::Key::Q, "Q"),
    (egui::Key::W, "W"),
    (egui::Key::E, "E"),
    (egui::Key::T, "T"),
    (egui::Key::Y, "Y"),
    (egui::Key::U, "U"),
    (egui::Key::I, "I"),
    (egui::Key::O, "O"),
    (egui::Key::P, "P"),
    (egui::Key::A, "A"),
    (egui::Key::S, "S"),
    (egui::Key::D, "D"),
    (egui::Key::F, "F"),
    (egui::Key::G, "G"),
    (egui::Key::H, "H"),
    (egui::Key::J, "J"),
    (egui::Key::K, "K"),
    (egui::Key::Z, "Z"),
    (egui::Key::X, "X"),
    (egui::Key::C, "C"),
    (egui::Key::V, "V"),
    (egui::Key::B, "B"),
    (egui::Key::N, "N"),
    (egui::Key::M, "M"),
];

fn pad_label(i: usize) -> &'static str {
    PAD_KEYS.get(i).map(|(_, l)| *l).unwrap_or("·")
}

/// Libellé du décalage de départ : « − 1 mesure », « + 2 temps », « − 6 temps ».
/// Court volontairement : avec un enchaînement de clics la valeur enchaînait les
/// parenthèses (« − 6 temps (1 mesure 2 temps) ») et poussait la rangée sur deux
/// lignes (vécu 21/11). Le signe porte sur le total, et le détail est dans
/// l'infobulle.
fn shift_label(beats: i32) -> String {
    let sign = if beats < 0 { "−" } else { "+" };
    let a = beats.abs();
    if a == 0 {
        return "auto".into();
    }
    if a % 4 == 0 {
        let m = a / 4;
        return format!("{sign} {m} mesure{}", if m > 1 { "s" } else { "" });
    }
    format!("{sign} {a} temps")
}

/// La playlist ouverte appartient-elle au filtre courant ? Catalogue et
/// « Téléchargés » gardent chacun la leur : ouvrir un jeu d'un onglet ne doit
/// pas le faire apparaître dans l'autre (demande user 21/11). Le mode Découpe,
/// lui, affiche toujours la sienne. Un fichier déposé (`local`) n'appartient à
/// aucune des deux listes.
fn list_belongs(is_cached_view: bool, is_dj: bool, open_from_cached: bool, local: bool) -> bool {
    is_dj || (!local && open_from_cached == is_cached_view)
}

/// Fenêtre de boucle posée par le bouton « poser une boucle » : **4 mesures**
/// à partir de la mesure la plus proche de `pos` (0 si on est avant la grille),
/// bornée à `total`. Rend `None` s'il n'y a pas la place d'une demi-mesure.
fn manual_loop(total: f64, phase: f64, bar: f64, pos: f64) -> Option<LoopWindow> {
    if bar <= 0.05 || total <= bar * 0.6 {
        return None;
    }
    let k = ((pos - phase) / bar).round();
    let start = (phase + k * bar).clamp(0.0, (total - bar).max(0.0));
    let len = (4.0 * bar).min(total - start);
    (len > bar * 0.5).then_some(LoopWindow {
        start,
        end: start + len,
    })
}

/// Position rythmique de `t` sur la grille : `(bloc, temps 0-based, fraction
/// du temps écoulé)`, le bloc valant `per_unit` temps. Bloc 1 = premier bloc
/// après `phase` ; avant la phase (intro), le bloc est 0 ou négatif, ce qui est
/// voulu : la bande surlignée de la forme d'onde retombe alors sur le bon
/// endroit. `per_unit` suit le pas choisi (`clip_beats`) : 4 pour une mesure, 8
/// pour deux mesures, 2 pour deux temps.
fn beat_at(t: f64, phase: f64, beat: f64, per_unit: f64) -> (i64, usize, f64) {
    let n = per_unit.max(1.0) as i64;
    let rel = (t - phase) / beat.max(1e-6);
    let k = rel.floor() as i64;
    (k.div_euclid(n) + 1, k.rem_euclid(n) as usize, rel.fract())
}

/// Libellé du bloc du voyant : la mesure quand le pas vaut un nombre entier de
/// mesures (« mesures 3–4 » pour 2 mesures), « segment » sinon (1 ou 2 temps).
/// Avant la grille : « intro ».
fn pulse_label(per_unit: usize, unit: i64) -> String {
    if unit <= 0 {
        return "intro".into();
    }
    if per_unit % 4 == 0 {
        let m = (per_unit / 4) as i64;
        if m <= 1 {
            format!("mesure {unit}")
        } else {
            format!("mesures {}–{}", (unit - 1) * m + 1, unit * m)
        }
    } else {
        format!("segment {unit}")
    }
}

/// Voyant de battement du **pas choisi** : un point par temps du segment (4
/// pour une mesure, 8 pour deux mesures, 2 pour deux temps), celui qui sonne
/// allumé avec un halo qui s'éteint pendant le temps. Fonction libre : la
/// barre du lecteur l'appelle pendant que `self.player` est emprunté.
fn beat_lights(ui: &mut egui::Ui, now: Option<(i64, usize, f64)>, per_unit: usize) {
    let v = ui.visuals().clone();
    let accent = v.selection.bg_fill;
    let n = per_unit.max(1);
    // un point par temps, mais 16 temps doivent rester compacts
    let step = (56.0 / n as f32).clamp(5.0, 11.0);
    let r = (step * 0.33).clamp(1.6, 3.6);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(n as f32 * step, 12.0), egui::Sense::hover());
    let p = ui.painter();
    for i in 0..n {
        let c = egui::pos2(rect.left() + i as f32 * step + r + 0.4, rect.center().y);
        if now.map(|(_, b, _)| b) == Some(i) {
            let frac = now.map(|(_, _, f)| f).unwrap_or(0.0).clamp(0.0, 1.0);
            // le halo rétrécit et pâlit pendant le temps : la pulsation se voit
            // même sans regarder fixement
            p.circle_filled(c, r, accent);
            let rest = (1.0 - frac) as f32;
            p.circle_stroke(
                c,
                r * 1.4 + rest * 2.5,
                egui::Stroke::new(1.4, accent.gamma_multiply(0.75 * rest)),
            );
        } else {
            p.circle_filled(c, r * 0.78, v.weak_text_color().gamma_multiply(0.35));
        }
    }
    let text = match now {
        Some((unit, _, _)) => pulse_label(n, unit),
        None => "battement —".to_string(),
    };
    ui.label(RichText::new(text).weak().small());
}

/// File de lecture après un clic sur une tuile. Rend le segment à programmer
/// pour la couture (`None` = rien à programmer, il est plus loin dans la file).
fn click_plan(
    mode: ClickMode,
    seq: &mut Vec<usize>,
    seq_pos: usize,
    seg: usize,
) -> Option<usize> {
    match mode {
        ClickMode::Queue => {
            seq.push(seg);
            (seq.len() == seq_pos + 2).then_some(seg)
        }
        ClickMode::Last => {
            // tout ce qui attendait est remplacé : A finit, puis C (B sauté)
            seq.truncate(seq_pos + 1);
            seq.push(seg);
            Some(seg)
        }
    }
}

/// Déplace un bord de boucle avec ses invariants : le début reste avant la fin
/// (0,2 s de marge, sinon une poignée qui croise l'autre donne une boucle
/// inversée) et tout reste dans la piste. La forme d'onde applique la même
/// règle en direct ; la fonction est là pour la tester sans GUI.
/// Pad suivant de la ronde : celui qui suit `cur` dans l'ordre d'appui, en
/// revenant au début. Avec un seul pad tenu, il n'y a rien à alterner.
fn next_pad(held: &[usize], cur: Option<usize>) -> Option<usize> {
    match cur {
        Some(c) => {
            let pos = held.iter().position(|i| *i == c);
            match (pos, held.len()) {
                (Some(k), n) if n >= 2 => Some(held[(k + 1) % n]),
                // le pad qui sonne vient d'être relâché : on passe à l'autre
                (None, 1) => Some(held[0]),
                _ => None,
            }
        }
        None => held.first().copied(),
    }
}

fn beats_label(beats: f64) -> &'static str {
    match beats.round() as i64 {
        1 => "1 temps",
        2 => "2 temps",
        4 => "1 mesure",
        8 => "2 mesures",
        16 => "4 mesures",
        _ => "un segment",
    }
}

#[derive(Clone, Copy)]
struct DragSeg {
    /// Segment glissé (indice dans `clips`).
    seg: usize,
    /// Indice dans la suite quand la pastille vient de la suite (réordonner).
    from: Option<usize>,
}

#[derive(Clone, Copy)]
enum Icon {
    Play,
    Stop,
    Export,
    Plus,
}

/// Bouton du mode Découpe : rectangle arrondi peint à la main, icône dessinée
/// au trait. Aucun glyphe de police (▶, ✂, ⤓…) : les polices par défaut d'egui
/// ne les contiennent pas toutes et le bouton devient un carré vide (vécu
/// 21/11 : « les images des boutons ne sont pas visibles »).
fn djb(ui: &mut egui::Ui, label: &str, icon: Option<Icon>, primary: bool) -> egui::Response {
    let v = ui.visuals().clone();
    let enabled = ui.is_enabled();
    let font = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), v.text_color());
    let icon_w = if icon.is_some() { 18.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + icon_w + 20.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, stroke, ink) = if !enabled {
        (
            v.widgets.noninteractive.bg_fill,
            Stroke::NONE,
            v.weak_text_color().gamma_multiply(0.6),
        )
    } else if primary {
        (
            v.selection.bg_fill,
            Stroke::NONE,
            v.strong_text_color(),
        )
    } else if resp.hovered() {
        (
            v.widgets.hovered.bg_fill,
            v.widgets.hovered.bg_stroke,
            v.strong_text_color(),
        )
    } else {
        (v.widgets.inactive.bg_fill, Stroke::NONE, v.text_color())
    };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(8), fill, stroke, egui::StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    if let Some(k) = icon {
        paint_icon(p, egui::pos2(x + 6.0, rect.center().y), 6.0, k, ink);
        x += icon_w;
    }
    p.text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, label, font, ink);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Icônes au trait (triangle, carré, flèche vers le bas, croix) : dessinées,
/// donc toujours visibles quelle que soit la police.
fn paint_icon(p: &egui::Painter, c: egui::Pos2, s: f32, icon: Icon, color: egui::Color32) {
    match icon {
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - s * 0.5, c.y - s),
                    egui::pos2(c.x - s * 0.5, c.y + s),
                    egui::pos2(c.x + s, c.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            p.rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(1.35 * s, 1.35 * s)),
                CornerRadius::same(1),
                color,
            );
        }
        Icon::Export => {
            let st = Stroke::new(1.6, color);
            p.line_segment([egui::pos2(c.x, c.y - s * 0.9), egui::pos2(c.x, c.y + s * 0.4)], st);
            p.line_segment(
                [
                    egui::pos2(c.x - s * 0.55, c.y - s * 0.1),
                    egui::pos2(c.x, c.y + s * 0.45),
                ],
                st,
            );
            p.line_segment(
                [
                    egui::pos2(c.x + s * 0.55, c.y - s * 0.1),
                    egui::pos2(c.x, c.y + s * 0.45),
                ],
                st,
            );
            p.line_segment(
                [
                    egui::pos2(c.x - s * 0.8, c.y + s * 0.85),
                    egui::pos2(c.x + s * 0.8, c.y + s * 0.85),
                ],
                st,
            );
        }
        Icon::Plus => {
            let st = Stroke::new(1.5, color);
            p.line_segment(
                [egui::pos2(c.x - s * 0.6, c.y), egui::pos2(c.x + s * 0.6, c.y)],
                st,
            );
            p.line_segment(
                [egui::pos2(c.x, c.y - s * 0.6), egui::pos2(c.x, c.y + s * 0.6)],
                st,
            );
        }
    }
}

/// Tuile d'un segment : touche de pad, numéro, durée. Tenir la tuile = boucler
/// le segment ; la glisser = composer la suite.
fn pad_tile(
    ui: &mut egui::Ui,
    i: usize,
    clip: LoopWindow,
    current: bool,
    // temps courant du segment (`0..n` + fraction du temps) : dessiné en haut
    // de la tuile quand elle sonne, `None` sinon
    pulse: Option<(usize, f32)>,
    // nombre de temps par segment (le pas du mode) : c'est le nombre de points
    beats: usize,
    in_suite: bool,
    intro: bool,
    pending: Option<usize>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(88.0, 46.0), Sense::click_and_drag());
    let v = ui.visuals().clone();
    let accent = v.selection.bg_fill;
    let fill = if current {
        accent
    } else if pending.is_some() {
        // en file : teinte d'avertissement, bien distincte du « déjà dans la
        // suite » (accent pâle) et du segment qui sonne (accent plein)
        v.warn_fg_color.gamma_multiply(0.30)
    } else if resp.hovered() {
        v.widgets.hovered.bg_fill
    } else if in_suite {
        accent.gamma_multiply(0.26)
    } else if intro {
        v.extreme_bg_color.gamma_multiply(0.75)
    } else {
        v.widgets.inactive.bg_fill
    };
    let stroke = if current {
        Stroke::new(1.6, v.selection.stroke.color)
    } else if pending.is_some() {
        Stroke::new(1.2, v.warn_fg_color)
    } else if in_suite {
        Stroke::new(1.0, accent.gamma_multiply(0.9))
    } else {
        Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color)
    };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(8), fill, stroke, egui::StrokeKind::Inside);
    let ink = if current {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    // touche de clavier, en haut à gauche
    let key = pad_label(i);
    if key != "·" {
        let kr = egui::Rect::from_min_size(rect.left_top() + egui::vec2(6.0, 5.0), egui::vec2(14.0, 13.0));
        p.rect(
            kr,
            CornerRadius::same(3),
            v.extreme_bg_color.gamma_multiply(0.85),
            Stroke::NONE,
            egui::StrokeKind::Inside,
        );
        p.text(
            kr.center(),
            egui::Align2::CENTER_CENTER,
            key,
            FontId::monospace(9.5),
            v.weak_text_color(),
        );
    }
    // battement : un point par temps, le temps courant allumé, au-dessus du
    // numéro du segment qui sonne (entre la touche et le « + »)
    if let Some((beat, frac)) = pulse {
        let n = beats.max(1);
        // 16 points doivent tenir dans les 88 px de la tuile : l'écart et le
        // rayon se resserrent quand le segment s'allonge
        let step = (72.0 / n as f32).clamp(3.6, 7.0);
        let r = (step * 0.37).clamp(1.4, 2.6);
        let x0 = rect.center().x - (n as f32 - 1.0) * step / 2.0;
        for k in 0..n {
            let c = egui::pos2(x0 + k as f32 * step, rect.top() + 6.5);
            if k == beat {
                let rest = (1.0 - frac).clamp(0.0, 1.0);
                p.circle_filled(c, r, v.strong_text_color());
                p.circle_stroke(
                    c,
                    r * 1.3 + rest * 1.8,
                    Stroke::new(
                        1.0,
                        v.strong_text_color().gamma_multiply(0.35 + 0.45 * rest),
                    ),
                );
            } else {
                p.circle_filled(c, r * 0.73, v.strong_text_color().gamma_multiply(0.35));
            }
        }
    }
    // « + » discret, toujours visible : c'est l'affordance d'ajout
    let over_plus = resp.hovered()
        && ui
            .ctx()
            .pointer_interact_pos()
            .map(|q| q.x > rect.right() - 22.0 && q.y < rect.top() + 22.0)
            .unwrap_or(false);
    paint_icon(
        p,
        egui::pos2(rect.right() - 11.0, rect.top() + 11.0),
        5.0,
        Icon::Plus,
        if over_plus {
            v.strong_text_color()
        } else {
            v.weak_text_color().gamma_multiply(0.75)
        },
    );
    p.text(
        rect.center() + egui::vec2(0.0, 1.0),
        egui::Align2::CENTER_CENTER,
        if intro {
            "intro".to_string()
        } else {
            format!("{:02}", i + 1)
        },
        FontId::proportional(if intro { 12.0 } else { 15.0 }),
        if intro { v.weak_text_color() } else { ink },
    );
    // rang dans la file d'attente (1 = joue juste après le segment en cours)
    if let Some(rank) = pending {
        p.text(
            rect.left_bottom() + egui::vec2(7.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            format!("+{rank}"),
            FontId::monospace(9.5),
            v.warn_fg_color,
        );
    }
    p.text(
        rect.center_bottom() - egui::vec2(0.0, 5.0),
        egui::Align2::CENTER_BOTTOM,
        util::fmt_time_cs(clip.len()),
        FontId::monospace(9.5),
        if current {
            v.strong_text_color()
        } else {
            v.weak_text_color()
        },
    );
    resp.on_hover_text(if intro {
        format!(
            "intro · {} – {} (avant la boucle)\nclic : jouer une fois · glisser : ajouter au montage",
            util::fmt_time_cs(clip.start),
            util::fmt_time_cs(clip.end)
        )
    } else {
        format!(
            "segment {} · {} – {}\nclic : jouer une fois · glisser : ajouter au montage · touche {} : boucler",
            i + 1,
            util::fmt_time_cs(clip.start),
            util::fmt_time_cs(clip.end),
            key
        )
    })
}

/// Où insérer dans la suite : au-dessus de la pastille sous le pointeur, ou
/// après elle si le pointeur est dans sa moitié basse.
fn drop_index(chips: &[(usize, egui::Rect)], p: Option<egui::Pos2>, len: usize) -> usize {
    let Some(p) = p else {
        return len;
    };
    for (pos, rect) in chips {
        if p.y <= rect.top() {
            return *pos;
        }
        if p.y <= rect.bottom() {
            return if p.y < rect.center().y {
                *pos
            } else {
                *pos + 1
            };
        }
    }
    len
}

/// Durée de référence de la piste : celle de la liste quand zxtune rend
/// plusieurs tours (NSF 1:55 rendu en 3 min), sinon le rendu entier.
fn whole_of(track_len: Option<f64>, total: f64) -> f64 {
    track_len
        .filter(|l| *l > 1.0)
        .map(|l| l.min(total))
        .unwrap_or(total)
}

/// Un fil de travail sur son propre canal de travaux.
fn spawn_worker(msg_tx: Sender<Msg>) -> Sender<Job> {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            let tx = msg_tx.clone();
            let res = match job {
                Job::Open(entry) => open_archive(&tx, &entry),
                Job::Play { entry, ordinal } => play(&tx, &entry, ordinal, true),
                Job::Load { entry, ordinal } => play(&tx, &entry, ordinal, false),
                Job::Convert { entry, ordinal, out, format, tags } => {
                    convert_track(&tx, &entry, ordinal, &out, format, tags)
                }
                Job::ConvertSegments { entry, ordinal, spans, out, format, tags, name } => {
                    convert_segments(&tx, &entry, ordinal, &spans, &out, format, tags, &name)
                }
                Job::Delete(id) => delete_cached(&tx, id),
                Job::Tempo { track, entry_id, ordinal } => {
                    analyse_tempo(&tx, &track, entry_id, ordinal)
                }
                Job::GtrackerLoad { track, make_track } => {
                    gtracker_load(&tx, &track, make_track)
                }
                Job::GtrackerTempo { track } => gtracker_tempo(&tx, &track),
                Job::GtrackerExport { project, audio, format, name } => {
                    gtracker_export(&tx, &project, &audio, format, &name)
                }
                Job::Update => update(&tx),
            };
            if let Err(e) = res {
                let _ = msg_tx.send(Msg::Error(format!("{e:#}")));
            }
        }
    });
    job_tx
}

/// Deux fils : **local** (lecture, découpe, export) et **réseau**
/// (téléchargement, mise à jour du catalogue). Un seul fil faisait attendre
/// l'écoute d'une archive déjà présente derrière un téléchargement de plusieurs
/// minutes (demande user 21/11).
fn spawn_workers() -> (Sender<Job>, Sender<Job>, Sender<Msg>, Receiver<Msg>) {
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let local = spawn_worker(msg_tx.clone());
    let net = spawn_worker(msg_tx.clone());
    (local, net, msg_tx, msg_rx)
}

/// Ce travail passe-t-il par le fil réseau ? Seuls le téléchargement d'une
/// archive absente et la mise à jour du catalogue prennent des minutes.
/// `cached` = l'archive est déjà sur le disque (aucun réseau).
fn goes_to_net(job: &Job, cached: bool) -> bool {
    match job {
        Job::Update => true,
        // un fichier déposé à la main est sur le disque par définition
        Job::Open(e) | Job::Play { entry: e, .. } | Job::Load { entry: e, .. } => {
            e.site != gtracker_core::model::LOCAL_SITE && !cached
        }
        _ => false,
    }
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
    let archive: Archive = gtracker_core::archive::ensure(entry, &db, &mut cb)?;
    let _ = tx.send(Msg::Tracks(
        entry.id,
        archive.tracks,
        archive.composer,
        archive.album,
    ));
    Ok(())
}

/// Grille du mode Découpe, hors du chemin de lecture.
///
/// Le rendu est déjà en mémoire (le cache RAM de `decode`), donc l'appel ne
/// redécode rien ; l'analyse elle-même (réseau) tourne dans le fil de travail.
fn analyse_tempo(tx: &Sender<Msg>, track: &Track, entry_id: i64, ordinal: usize) -> Result<()> {
    let _ = tx.send(Msg::Status("Analyse du rythme…".into()));
    let decoded = decode::decode(&gtracker_core::zxtune_path(), track)?;
    let tempo = decode::analyse_tempo(
        &decoded.pcm,
        decoded.channels,
        decoded.rate,
        decoded.total,
        decoded.window,
    );
    decode::remember_tempo(track, tempo);
    let _ = tx.send(Msg::Tempo(entry_id, ordinal, tempo));
    Ok(())
}

/// Décode une source du GTracker : fichier local ou piste extraite, même
/// chemin que la bibliothèque (cache RAM partagé).
fn gtracker_load(tx: &Sender<Msg>, track: &Track, make_track: bool) -> Result<()> {
    let _ = tx.send(Msg::Status(format!(
        "GTracker : décodage de « {} »…",
        track.title
    )));
    let decoded = decode::decode(&gtracker_core::zxtune_path(), track)?;
    let _ = tx.send(Msg::GtrackerLoaded(track.clone(), decoded, make_track));
    Ok(())
}

/// Rythme et tonalité d'une source, **hors** du décodage : l'analyse prend
/// 1 à 3 s et la source doit être utilisable tout de suite.
fn gtracker_tempo(tx: &Sender<Msg>, track: &Track) -> Result<()> {
    let _ = tx.send(Msg::Status("GTracker : rythme et tonalité…".into()));
    let decoded = decode::decode(&gtracker_core::zxtune_path(), track)?;
    let tempo = decode::analyse_tempo(
        &decoded.pcm,
        decoded.channels,
        decoded.rate,
        decoded.total,
        decoded.window,
    );
    decode::remember_tempo(track, tempo);
    let ch = decoded.channels.max(1) as usize;
    let frames = decoded.pcm.len() / ch;
    let key = gtracker_core::key::analyse(&decoded.pcm, decoded.channels, decoded.rate, 0, frames);
    let _ = tx.send(Msg::GtrackerTempo(
        PathBuf::from(&track.file),
        Some(tempo),
        key,
    ));
    Ok(())
}

/// Rend le projet GTracker et l'encode (MP3/FLAC) dans `~/Musique/GTracker`.
fn gtracker_export(
    tx: &Sender<Msg>,
    project: &gtracker_core::project::Project,
    audio: &[Option<gtracker_core::mixer::SourceAudio>],
    format: Format,
    name: &str,
) -> Result<()> {
    const RATE: u32 = 48_000;
    let _ = tx.send(Msg::Status("GTracker : mixage du projet…".into()));
    // l'export est le projet, pas la séance d'écoute : ni clics, ni boucle
    let mut clean = project.clone();
    clean.metronome = false;
    clean.loop_beats = None;
    let plan = gtracker_core::mixer::Plan::build(&clean, audio, RATE);
    if plan.is_empty() {
        anyhow::bail!("projet vide : rien à exporter");
    }
    let frames = plan.frames.max(1) as usize;
    let pcm = gtracker_core::mixer::render(&plan, frames);
    let _ = tx.send(Msg::Status("GTracker : encodage…".into()));
    let tags = Tags {
        title: Some(name.to_string()),
        artist: Some("GTracker".into()),
        album: Some(project.name.clone()),
    };
    let out_dir = util::music_dir().join("GTracker");
    let path = gtracker_core::convert::export_pcm(&pcm, 2, RATE, &out_dir, format, &tags, name)?;
    let _ = tx.send(Msg::GtrackerExported(path));
    Ok(())
}

fn play(tx: &Sender<Msg>, entry: &Entry, ordinal: usize, autoplay: bool) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let mut cb = progress_fn(tx);
    let archive = gtracker_core::archive::ensure(entry, &db, &mut cb)?;
    let track = archive
        .tracks
        .get(ordinal)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("piste {ordinal} absente"))?;
    let _ = tx.send(Msg::Status(format!("Décodage : {}", track.title)));
    let decoded = decode::decode(&gtracker_core::zxtune_path(), &track)?;
    let _ = tx.send(Msg::Playing(
        entry.id,
        archive.tracks,
        ordinal,
        decoded,
        archive.composer,
        archive.album,
        autoplay,
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
    let archive = gtracker_core::archive::ensure(entry, &db, &mut cb)?;
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
    let path = gtracker_core::convert::export(
        &gtracker_core::zxtune_path(),
        &track,
        out,
        format,
        &tags,
    )?;
    let _ = tx.send(Msg::Converted(path));
    Ok(())
}

fn convert_segments(
    tx: &Sender<Msg>,
    entry: &Entry,
    ordinal: usize,
    spans: &[(f64, f64)],
    out: &std::path::Path,
    format: Format,
    tags: Tags,
    name: &str,
) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let mut cb = progress_fn(tx);
    let archive = gtracker_core::archive::ensure(entry, &db, &mut cb)?;
    let track = archive
        .tracks
        .get(ordinal)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("piste {ordinal} absente"))?;
    let _ = tx.send(Msg::Status(format!(
        "Export {} : {} segments",
        format.ext().to_uppercase(),
        spans.len()
    )));
    let path = gtracker_core::convert::export_segments(
        &gtracker_core::zxtune_path(),
        &track,
        spans,
        out,
        format,
        &tags,
        name,
    )?;
    let _ = tx.send(Msg::Converted(path));
    Ok(())
}

fn delete_cached(tx: &Sender<Msg>, id: i64) -> Result<()> {
    let db = Db::open(&util::db_path())?;
    let _ = tx.send(Msg::Status("Suppression du cache…".into()));
    let bytes = gtracker_core::archive::delete_cached(id, &db)?;
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
    let report = joshw::update(&mut db, cb)?;
    for url in &report.failed {
        eprintln!("catalogue : dossier non récupéré {url}");
    }
    let tail = if report.failed.is_empty() {
        String::new()
    } else {
        format!(
            " · {} dossiers non récupérés, relancer la mise à jour",
            report.failed.len()
        )
    };
    let _ = tx.send(Msg::Status(format!(
        "{} nouvelles archives{tail}",
        report.inserted
    )));
    let _ = tx.send(Msg::ReloadDb);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ce que le Player a besoin de retrouver quand la bibliothèque reprend le
    /// son : après le mixeur du projet (`play_source` oublie le PCM), la piste
    /// doit se réinstaller sans relire le fichier — sinon « je peux plus jouer »
    /// au retour dans l'éditeur (vécu 21/11).
    #[test]
    fn une_piste_revient_apres_le_projet() {
        let Ok(player) = Player::new() else {
            eprintln!("aucune sortie audio : test non applicable");
            return;
        };
        let mut player = Some(player);
        let lib = LibAudio {
            pcm: Arc::new(vec![0.0f32; 44_100 * 2]),
            channels: 2,
            rate: 44_100,
            total: 1.0,
            window: LoopWindow {
                start: 0.0,
                end: 1.0,
            },
            pos: 0.5,
        };
        lib.restore(&mut player, false, 1.0);
        assert!(player.as_ref().unwrap().ready(), "piste chargée");
        // le projet prend la sortie : la piste est oubliée par le Player
        player.as_mut().unwrap().play_source(rodio::buffer::SamplesBuffer::new(
            2,
            44_100,
            vec![0.0f32; 512],
        ));
        assert!(!player.as_ref().unwrap().ready(), "le mixeur a pris la sortie");
        // retour dans l'éditeur : la piste revient, en pause, prête à jouer
        lib.restore(&mut player, false, 1.0);
        let p = player.as_ref().unwrap();
        assert!(p.ready(), "la piste est réinstallée");
        assert!(p.is_paused(), "elle attend « Lecture »");
        // et « Lecture » a bien une source à jouer : une sortie vide serait le
        // bug « je peux plus jouer »
        player.as_mut().unwrap().toggle_pause();
        let p = player.as_ref().unwrap();
        assert!(!p.is_paused());
        assert!(!p.exhausted(), "la sortie contient la piste, pas du vide");
    }

    #[test]
    fn etiquette_du_decalage() {
        assert_eq!(shift_label(0), "auto");
        assert_eq!(shift_label(4), "+ 1 mesure");
        assert_eq!(shift_label(-8), "− 2 mesures");
        assert_eq!(shift_label(2), "+ 2 temps");
        assert_eq!(shift_label(-1), "− 1 temps");
        // court, sans parenthèse : un enchaînement de clics ne doit pas
        // allonger la rangée jusqu'au retour à la ligne (vécu 21/11)
        assert_eq!(shift_label(-6), "− 6 temps");
        assert_eq!(shift_label(23), "+ 23 temps");
    }

    /// Catalogue et téléchargés gardent chacun leur playlist : ouvrir dans l'un
    /// ne doit rien afficher dans l'autre, et un fichier déposé n'apparaît que
    /// dans Découpe.
    #[test]
    fn playlist_du_bon_onglet() {
        // ouvert depuis le catalogue
        assert!(list_belongs(false, false, false, false));
        assert!(!list_belongs(true, false, false, false), "pas dans Téléchargés");
        // ouvert depuis « Téléchargés »
        assert!(list_belongs(true, false, true, false));
        assert!(!list_belongs(false, false, true, false), "pas dans Catalogue");
        // fichier déposé : seulement en Découpe
        assert!(list_belongs(false, true, false, true));
        assert!(!list_belongs(false, false, false, true));
        assert!(!list_belongs(true, false, false, true));
    }

    /// Bouton « poser une boucle » : 4 mesures calées sur la grille, bornées au
    /// rendu, et rien si la piste est trop courte pour une demi-mesure.
    #[test]
    fn boucle_manuelle_quatre_mesures() {
        // 120 BPM (mesure de 2 s), phase 0,6 s, lecture à 10,1 s : la mesure
        // la plus proche commence à 10,6 s
        let w = manual_loop(60.0, 0.6, 2.0, 10.1).unwrap();
        assert!((w.start - 10.6).abs() < 1e-9, "départ : {}", w.start);
        assert!((w.end - w.start - 8.0).abs() < 1e-9, "4 mesures de 2 s");
        // près de la fin : la fenêtre est bornée au rendu
        let w = manual_loop(12.0, 0.0, 2.0, 11.0).unwrap();
        assert!((w.start - 10.0).abs() < 1e-9);
        assert!((w.end - 12.0).abs() < 1e-9);
        // avant la grille (intro) : départ ramené à 0
        let w = manual_loop(60.0, 5.0, 2.0, 0.0).unwrap();
        assert!((w.start - 0.0).abs() < 1e-9, "départ : {}", w.start);
        // piste plus courte qu'une mesure : pas de boucle posable
        assert!(manual_loop(1.0, 0.0, 2.0, 0.0).is_none());
    }

    /// Le voyant de battement suit la mesure et le temps de la grille : 120 BPM
    /// (0,5 s), phase 0,5 s → mesure 1 temps 1 pile à la phase, mesure 2 au
    /// temps 4 (2,5 s), et la fraction avance dans le temps.
    #[test]
    fn battement_mesure_et_temps() {
        let m = Tempo::BEATS_PER_BAR;
        let (bar, beat, frac) = beat_at(0.5, 0.5, 0.5, m);
        assert_eq!((bar, beat), (1, 0));
        assert!(frac.abs() < 1e-9);
        let (bar, beat, frac) = beat_at(0.75, 0.5, 0.5, m);
        assert_eq!((bar, beat), (1, 0));
        assert!((frac - 0.5).abs() < 1e-9, "mi-temps : {frac}");
        let (bar, beat, _) = beat_at(0.5 + 1.5, 0.5, 0.5, m);
        assert_eq!((bar, beat), (1, 3), "quatrième temps de la mesure 1");
        let (bar, beat, frac) = beat_at(0.5 + 2.0, 0.5, 0.5, m);
        assert_eq!((bar, beat), (2, 0), "mesure suivante");
        assert!(frac.abs() < 1e-9);
        // avant la phase (intro) : mesure 0, la bande reste cohérente
        let (bar, beat, _) = beat_at(0.25, 0.5, 0.5, m);
        assert_eq!((bar, beat), (0, 3));
        assert_eq!(beat_at(0.0, 0.0, 0.5, m), (1, 0, 0.0));
    }

    /// Le voyant suit le **pas du mode**, pas la mesure : 2 temps → 2 points,
    /// 2 mesures → 8 points, et le libellé dit le bon bloc (demande user 21/11).
    #[test]
    fn battement_suit_le_pas_du_mode() {
        // 2 temps par segment à 120 BPM, phase 0,5 s
        let (unit, b, _) = beat_at(0.5, 0.5, 0.5, 2.0);
        assert_eq!((unit, b), (1, 0));
        let (unit, b, _) = beat_at(1.0, 0.5, 0.5, 2.0);
        assert_eq!((unit, b), (1, 1), "deuxième temps du segment");
        let (unit, b, _) = beat_at(1.5, 0.5, 0.5, 2.0);
        assert_eq!((unit, b), (2, 0), "segment suivant, pas mesure suivante");
        // 2 mesures = 8 temps par segment : le bloc ne change qu'au 9e temps
        let (unit, b, _) = beat_at(0.5 + 7.5 * 0.5, 0.5, 0.5, 8.0);
        assert_eq!((unit, b), (1, 7));
        let (unit, b, _) = beat_at(0.5 + 8.0 * 0.5, 0.5, 0.5, 8.0);
        assert_eq!((unit, b), (2, 0));
        // libellés
        assert_eq!(pulse_label(4, 5), "mesure 5");
        assert_eq!(pulse_label(8, 1), "mesures 1–2");
        assert_eq!(pulse_label(8, 3), "mesures 5–6");
        assert_eq!(pulse_label(16, 2), "mesures 5–8");
        assert_eq!(pulse_label(2, 7), "segment 7");
        assert_eq!(pulse_label(1, 3), "segment 3");
        assert_eq!(pulse_label(4, 0), "intro");
    }

    /// Deux modes de clic : en file (tout est joué) ou dernier seul (un clic
    /// récent remplace celui qui attendait à la couture).
    #[test]
    fn clics_en_file_ou_dernier_seul() {
        // en file : le premier clic devient le suivant, les autres s'empilent
        let mut seq = vec![2];
        assert_eq!(click_plan(ClickMode::Queue, &mut seq, 0, 5), Some(5));
        assert_eq!(seq, [2, 5]);
        assert_eq!(click_plan(ClickMode::Queue, &mut seq, 0, 7), None);
        assert_eq!(seq, [2, 5, 7]);
        // dernier seul : B (5) attend, C (7) le remplace → A puis C
        let mut seq = vec![2, 5];
        assert_eq!(click_plan(ClickMode::Last, &mut seq, 0, 7), Some(7));
        assert_eq!(seq, [2, 7], "B est sauté");
        // et un clic de plus remplace encore le précédent
        assert_eq!(click_plan(ClickMode::Last, &mut seq, 0, 9), Some(9));
        assert_eq!(seq, [2, 9]);
        // le segment en cours n'est jamais retiré de la file
        let mut seq = vec![2, 5, 7];
        assert_eq!(click_plan(ClickMode::Last, &mut seq, 1, 9), Some(9));
        assert_eq!(seq, [2, 5, 9]);
    }

    /// Un téléchargement ne doit pas bloquer l'écoute : il part sur le fil
    /// réseau, le reste sur le fil local (demande user 21/11 : « pourquoi je ne
    /// peux pas écouter ce que j'ai déjà pendant un dl »).
    #[test]
    fn fil_reseau_seulement_pour_le_telechargement() {
        let e = |id: i64| Entry {
            id,
            site: "snes".into(),
            folder: "a".into(),
            name: "nom".into(),
            path: "p.7z".into(),
        };
        assert!(goes_to_net(&Job::Update, false));
        assert!(
            goes_to_net(&Job::Open(e(1)), false),
            "archive absente = téléchargement"
        );
        assert!(!goes_to_net(&Job::Open(e(1)), true), "archive locale = local");
        assert!(goes_to_net(
            &Job::Play {
                entry: e(1),
                ordinal: 0
            },
            false
        ));
        assert!(!goes_to_net(
            &Job::Play {
                entry: e(1),
                ordinal: 0
            },
            true
        ));
        let track = Track {
            subsong: 1,
            title: "t".into(),
            file: std::path::PathBuf::from("/x"),
            length: None,
            loop_start: None,
            loop_end: None,
        };
        let tempo = Job::Tempo {
            track,
            entry_id: 1,
            ordinal: 0,
        };
        assert!(!goes_to_net(&tempo, false), "la découpe reste locale");
        assert!(!goes_to_net(&Job::Delete(1), true));
        // fichier déposé à la main : sur le disque, donc jamais le fil réseau
        let local = Entry {
            id: -7,
            site: gtracker_core::model::LOCAL_SITE.into(),
            folder: "/home/gab".into(),
            name: "Morceau".into(),
            path: "/home/gab/Morceau.mp3".into(),
        };
        assert!(!goes_to_net(&Job::Open(local.clone()), false));
        assert!(!goes_to_net(
            &Job::Play {
                entry: local,
                ordinal: 0
            },
            false
        ));
    }

    /// Liste « Téléchargés » : A → Z sur le titre, toutes consoles mêlées.
    #[test]
    fn telecharges_tries_a_z_toutes_consoles() {
        let mut v = vec![
            ("Zelda", "NES"),
            ("Chrono Trigger", "SNES"),
            ("Actraiser", "SNES"),
        ];
        v.sort_by_cached_key(|(t, c)| cached_sort_key(t, c));
        assert_eq!(
            v.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            ["Actraiser", "Chrono Trigger", "Zelda"]
        );
        // titres identiques : la console départage, sans mélanger le reste
        let mut v = vec![("Same", "SNES"), ("Same", "NES")];
        v.sort_by_cached_key(|(t, c)| cached_sort_key(t, c));
        assert_eq!(v[0].1, "NES");
    }

    /// Correction manuelle de la boucle détectée : impossible de croiser les
    /// bords ou de sortir de la piste.
    #[test]
    fn bord_de_boucle_borne() {
        let w = LoopWindow {
            start: 10.0,
            end: 40.0,
        };
        let edge = |start: bool, t: f64| {
            let (a, b) = gtracker_core::util::move_edge((w.start, w.end), start, t, 100.0);
            LoopWindow { start: a, end: b }
        };
        // le début ne dépasse pas la fin (0,2 s de marge)
        let m = edge(true, 60.0);
        assert!((m.start - 39.8).abs() < 1e-9, "{m:?}");
        assert!((m.end - 40.0).abs() < 1e-9, "{m:?}");
        // et ne sort pas de la piste, même à gauche
        let m = edge(true, -5.0);
        assert!((m.start - 0.0).abs() < 1e-9, "{m:?}");
        // la fin ne passe pas avant le début
        let m = edge(false, 2.0);
        assert!((m.end - 10.2).abs() < 1e-9, "{m:?}");
        // ni au-delà de la piste
        let m = edge(false, 500.0);
        assert!((m.end - 100.0).abs() < 1e-9, "{m:?}");
        // déplacement normal : le bord visé bouge, l'autre ne bouge pas
        let m = edge(true, 12.5);
        assert_eq!((m.start, m.end), (12.5, 40.0));
    }

    /// Régression (vécu 21/11) : le glisser du badge BPM avait été posé dans un
    /// second `ui.interact` **par-dessus** le badge. egui ne donne le survol
    /// qu'au widget du dessus : `badge.hovered()` devenait faux et la molette
    /// cessait de régler le tempo. Un seul widget doit porter les deux gestes.
    #[test]
    fn un_seul_widget_par_geste() {
        use eframe::egui;
        let ctx = egui::Context::default();
        let mut vu = (false, false);
        for _ in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            };
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(30.0, 20.0)));
            let mut out = ctx.run_ui(input, |ui| {
                let dessous = ui.allocate_response(egui::vec2(60.0, 40.0), egui::Sense::hover());
                let dessus = ui.interact(
                    dessous.rect,
                    ui.scope_id().with("gestes"),
                    egui::Sense::click_and_drag(),
                );
                vu = (dessous.hovered(), dessus.hovered());
            });
            // pas de moteur de rendu en test : sans ça epaint panique
            out.textures_delta.clear();
        }
        assert_eq!(vu, (false, true), "le widget du dessus capte seul le survol");
    }

    /// L'alternance des pads est la règle demandée : maintenir deux touches
    /// fait tourner les segments dans l'ordre d'appui, indéfiniment.
    #[test]
    fn alternance_des_pads() {
        // deux pads tenus : 2 → 5 → 2 → 5…
        assert_eq!(next_pad(&[2, 5], Some(2)), Some(5));
        assert_eq!(next_pad(&[2, 5], Some(5)), Some(2));
        // trois pads : ronde complète
        assert_eq!(next_pad(&[1, 4, 7], Some(1)), Some(4));
        assert_eq!(next_pad(&[1, 4, 7], Some(4)), Some(7));
        assert_eq!(next_pad(&[1, 4, 7], Some(7)), Some(1));
        // un seul pad tenu : il boucle seul
        assert_eq!(next_pad(&[3], Some(3)), None);
        // le pad qui sonne a été relâché : on passe à l'autre
        assert_eq!(next_pad(&[9], Some(3)), Some(9));
        // rien de tenu
        assert_eq!(next_pad(&[], Some(3)), None);
        assert_eq!(next_pad(&[], None), None);
    }

    /// Suite : les répétitions consécutives sont groupées, et une pastille se
    /// dépose avant ou après celle qui est sous le pointeur.
    #[test]
    fn depot_dans_la_suite() {
        let a = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 28.0));
        let b = egui::Rect::from_min_size(egui::pos2(0.0, 28.0), egui::vec2(100.0, 28.0));
        let chips = vec![(0usize, a), (1usize, b)];
        // moitié haute d'une pastille = avant elle, moitié basse = après
        assert_eq!(drop_index(&chips, Some(egui::pos2(50.0, 5.0)), 2), 0);
        assert_eq!(drop_index(&chips, Some(egui::pos2(50.0, 22.0)), 2), 1);
        assert_eq!(drop_index(&chips, Some(egui::pos2(50.0, 35.0)), 2), 1);
        // sous la dernière pastille = fin
        assert_eq!(drop_index(&chips, Some(egui::pos2(50.0, 55.0)), 2), 2);
        assert_eq!(drop_index(&chips, None, 2), 2);
    }

    /// Une mesure 4/4 = 4 temps : la découpe doit suivre la longueur choisie.
    #[test]
    fn longueurs_de_decoupe() {
        let t = Tempo {
            bpm: 120.0,
            beat: 0.5,
            phase: 0.0,
            confidence: 1.0,
        };
        // 8 s à 120 BPM : 16 temps, 4 mesures de 4 temps
        assert_eq!(tempo::cut(8.0, &t, 1.0).len(), 16);
        assert_eq!(tempo::cut(8.0, &t, 4.0).len(), 4);
        assert_eq!(tempo::cut(8.0, &t, 16.0).len(), 1);
    }
}
