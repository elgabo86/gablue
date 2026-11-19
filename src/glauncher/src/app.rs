//! État, messages et interface du launcher.

use std::collections::{HashMap, HashSet};
use std::time::Duration;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use iced::futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use iced::futures::sink::SinkExt;
use iced::futures::{Stream, StreamExt};
use iced::keyboard::key::Named;
use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{
    Column, Id, Row, column, container, image, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Fill, Length, Padding, Subscription,
    Task, Theme, keyboard, window,
};

use crate::config::{Config, ViewMode};
use crate::assets;
use crate::covers;
use crate::favorites;
use crate::gamepad::{PadButton, PadEvent, PadKind};
use crate::index::{Index, Refresh};
use crate::launch;
use crate::library::{self, ArtworkCache, Game};
use crate::playtime;
use crate::state::{self, SortBy};
use crate::thumbs;

// --- palette -----------------------------------------------------------------

const BG: Color = Color::from_rgb(0.035, 0.04, 0.06);
const TEXT: Color = Color::from_rgb(0.88, 0.90, 0.95);
const MUTED: Color = Color::from_rgb(0.48, 0.52, 0.60);
const ACCENT: Color = Color::from_rgb(0.31, 0.60, 1.0);
const DANGER: Color = Color::from_rgb(1.0, 0.42, 0.42);
const CELL_BG: Color = Color::from_rgb(0.07, 0.08, 0.11);
const CELL_BG_SELECTED: Color = Color::from_rgb(0.16, 0.22, 0.36);
/// Cadre de sélection des vignettes et des lignes de liste.
const FOCUS_BORDER: f32 = 4.0;
/// Types de visuels demandés pour la vue courante (grille, liste, mosaïque) :
/// la cover et l'icône suffisent, le héros et le logo ne servent qu'à la page.
const DEUX_TYPES: [covers::ArtKind; 2] = [covers::ArtKind::Grid, covers::ArtKind::Icon];
const CELL_BORDER: Color = Color::from_rgb(0.14, 0.16, 0.21);

const GAP: f32 = 14.0;
/// Colonnes du menu de choix d'image.
const PICKER_COLUMNS: usize = 5;
const PADDING: f32 = 12.0;
/// Marge horizontale du launcher (padding `[18, 26]` de la vue).
const PAGE_PADDING_X: f32 = 26.0;
/// Ratio hauteur/largeur d'une cover (600×900) : les vignettes ont la forme des
/// vraies covers, sample compris.
const ART_RATIO: f32 = 1.5;
/// Hauteur de la ligne de titre sous une vignette.
const CARD_CAPTION: f32 = 24.0;
/// En mosaïque, les vignettes sont plus petites que `cell_size` (densité façon
/// Switch Online) ; le zoom agit sur cette taille comme sur la grille.
const MOSAIC_SCALE: f32 = 0.55;
/// Fenêtre de double appui sur « retour » pour quitter le launcher.
const QUIT_WINDOW: Duration = Duration::from_millis(1500);
/// Zoom des vignettes : pas, bornes (px de largeur).
const ZOOM_STEP: f32 = 40.0;
const CELL_MIN: f32 = 120.0;
const CELL_MAX: f32 = 520.0;
/// Bornes du nombre de colonnes atteignables au zoom (grille qui remplit la
/// largeur : le zoom y agit sur les colonnes, pas sur la largeur minimale).
/// Épaisseur du cadre de sélection en mosaïque.
const SELECT_BORDER: f32 = 6.0;
const COLUMNS_MIN: usize = 3;
const COLUMNS_MAX: usize = 12;
/// Durée du fondu au noir (secondes) — « doucement ».
const FADE_SECONDS: f32 = 0.9;
/// Constante de temps du défilement animé (secondes).
const SCROLL_TAU: f32 = 0.10;

/// Vignette de remplacement, à la taille et à la forme d'une vraie cover.
const COVER_SAMPLE: &[u8] = include_bytes!("../assets/cover-sample.png");
/// Étoile des favoris : un PNG embarqué plutôt que le caractère `★`, qui
/// tombait sur un glyphe de repli minuscule (illisible sur une vignette).
const STAR: &[u8] = include_bytes!("../assets/star.png");

// --- glyphes ----------------------------------------------------------------

const XBOX_CONFIRM: &[u8] = include_bytes!("../assets/glyphs/xbox/confirm.png");
const XBOX_BACK: &[u8] = include_bytes!("../assets/glyphs/xbox/back.png");
const XBOX_VIEW: &[u8] = include_bytes!("../assets/glyphs/xbox/view.png");
const XBOX_ZOOM_IN: &[u8] = include_bytes!("../assets/glyphs/xbox/zoom-in.png");
const XBOX_ZOOM_OUT: &[u8] = include_bytes!("../assets/glyphs/xbox/zoom-out.png");
const PS_CONFIRM: &[u8] = include_bytes!("../assets/glyphs/playstation/confirm.png");
const PS_BACK: &[u8] = include_bytes!("../assets/glyphs/playstation/back.png");
const PS_VIEW: &[u8] = include_bytes!("../assets/glyphs/playstation/view.png");
const PS_ZOOM_IN: &[u8] = include_bytes!("../assets/glyphs/playstation/zoom-in.png");
const PS_ZOOM_OUT: &[u8] = include_bytes!("../assets/glyphs/playstation/zoom-out.png");

/// Glyphes de boutons du jeu affiché dans le pied de page.
struct Glyphs {
    confirm: image::Handle,
    back: image::Handle,
    view: image::Handle,
    zoom_in: image::Handle,
    zoom_out: image::Handle,
}

impl Glyphs {
    fn for_kind(kind: PadKind) -> Self {
        let (confirm, back, view, zoom_in, zoom_out) = match kind {
            PadKind::Playstation => (PS_CONFIRM, PS_BACK, PS_VIEW, PS_ZOOM_IN, PS_ZOOM_OUT),
            _ => (XBOX_CONFIRM, XBOX_BACK, XBOX_VIEW, XBOX_ZOOM_IN, XBOX_ZOOM_OUT),
        };
        Self {
            confirm: image::Handle::from_bytes(confirm),
            back: image::Handle::from_bytes(back),
            view: image::Handle::from_bytes(view),
            zoom_in: image::Handle::from_bytes(zoom_in),
            zoom_out: image::Handle::from_bytes(zoom_out),
        }
    }
}

// --- bus d'évènements (manettes, fin de jeu) --------------------------------

static BUS: OnceLock<UnboundedSender<Message>> = OnceLock::new();
static BUS_RX: Mutex<Option<UnboundedReceiver<Message>>> = Mutex::new(None);

fn post(message: Message) {
    if let Some(bus) = BUS.get() {
        let _ = bus.unbounded_send(message);
    }
}

/// Traduction d'un évènement du worker de covers en message d'application.
fn cover_message(event: covers::CoverEvent) -> Message {
    match event {
        covers::CoverEvent::Found { slug, kind, bytes } => Message::Art {
            slug,
            kind,
            bytes: Some(bytes),
        },
        covers::CoverEvent::Missing { slug, kind } => Message::Art {
            slug,
            kind,
            bytes: None,
        },
        covers::CoverEvent::Failed { slug, kind, error } => {
            Message::ArtFailed { slug, kind, error }
        }
        covers::CoverEvent::Choices {
            slug,
            kind,
            choices,
        } => Message::Choices {
            slug,
            kind,
            choices,
        },
        covers::CoverEvent::Icon { slug, bytes } => Message::Icon { slug, bytes },
    }
}

/// Lance le jeu et attend sa fin, puis renvoie le résultat par le bus.
fn spawn_launch(name: String, command: launch::Launch, cache: PathBuf, slug: String) {
    let _ = std::thread::Builder::new()
        .name(format!("glauncher-run:{name}"))
        .spawn(move || {
            let result = launch::run(&command, &cache, &slug, &name);
            post(Message::Launched { name, result });
        });
}

/// Réindexe en arrière-plan (scan complet ou rafale de chemins) et poste le
/// résultat : le thread UI ne touche jamais le disque ni SQLite.
fn spawn_refresh(config: Config, index: Arc<Mutex<Option<Index>>>, changed: Vec<PathBuf>) {
    let _ = std::thread::Builder::new()
        .name("glauncher-index".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            let result = (|| -> Result<Refresh, String> {
                let mut guard = index
                    .lock()
                    .map_err(|_| "index verrouillé".to_string())?;
                let Some(index) = guard.as_mut() else {
                    return Err("index indisponible".into());
                };
                let rows = if changed.is_empty() {
                    let snapshot = index.snapshot()?;
                    let walk = library::walk_packs(&config, &snapshot);
                    let rows = walk.rows.clone();
                    index.replace_all(&walk)?;
                    rows
                } else {
                    library::apply_changes(&config, index, &changed)?
                };
                Ok(Refresh { rows, changed })
            })();
            eprintln!(
                "glauncher : index en {} ms",
                started.elapsed().as_millis()
            );
            post(Message::Refreshed(result));
        });
}

fn bus_stream() -> impl Stream<Item = Message> {
    iced::stream::channel(64, async |mut output| {
        let receiver = BUS_RX.lock().ok().and_then(|mut slot| slot.take());
        let Some(mut receiver) = receiver else {
            return;
        };
        while let Some(message) = receiver.next().await {
            if output.send(message).await.is_err() {
                return;
            }
        }
    })
}

// --- état -------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum Nav {
    Prev,
    Next,
    Up,
    Down,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone)]
pub enum Message {
    Window(window::Id),
    Resized(iced::Size),
    Key(keyboard::Event),
    Pad(PadEvent),
    CardClicked(usize),
    /// Visuel SteamGridDB (grid, héros, logo, icône) ; `bytes: None` = introuvable.
    Art { slug: String, kind: covers::ArtKind, bytes: Option<Vec<u8>> },
    /// Icône extraite du paquet (`.icon.png` dans le squashfs).
    Icon { slug: String, bytes: Vec<u8> },
    /// Miniature 320 px d'une cover (worker de miniatures).
    Thumb { slug: String, bytes: Vec<u8> },
    /// Icône cherchée à côté du paquet (worker d'assets, disque externe).
    LocalIcon { slug: String, icon: Option<image::Handle> },
    ArtFailed { slug: String, kind: covers::ArtKind, error: String },
    /// Menu « choisir une image » pour un type de visuel.
    OpenPicker(covers::ArtKind),
    ClosePicker,
    PickerNav(Nav),
    PickerSelect(usize),
    PickerPick,
    Choices { slug: String, kind: covers::ArtKind, choices: Vec<covers::Choice> },
    /// Page d'un jeu : héros, logo et actions (jouer, changer les visuels).
    OpenPage(usize),
    ClosePage,
    /// Déplace l'action sélectionnée de la page (-1 / +1).
    PageNav(i32),
    /// Exécute l'action sélectionnée sur la page.
    PageAct,
    /// Sélectionne puis exécute l'action n (clic sur une pastille).
    PageChoose(usize),
    /// Ajoute ou retire le jeu des favoris (étoile de la page).
    /// Modifie le nom envoyé à SteamGridDB (jeu mal reconnu).
    RenameOpen,
    RenameInput(String),
    RenameSubmit,
    RenameCancel,
    /// Menu « quel visuel changer » de la page (Grid / Héros / Logo / Icône).
    ArtMenu,
    ArtMenuNav(i32),
    /// Choisit le visuel n du menu (clic).
    ArtMenuChoose(usize),
    ArtMenuPick,
    ArtMenuClose,
    /// Taille des vignettes : +1 plus grandes, -1 plus petites.
    Zoom(i32),
    /// Fenêtre active (focus) : sans focus, ni clavier ni manette n'agissent.
    Active(bool),
    /// Position réelle du défilement (molette, glisser, `scroll_to`).
    Scrolled(f32),
    Play(usize),
    /// Chemins modifiés signalés par le veilleur.
    Changed(Vec<PathBuf>),
    /// Liste à jour après réindexation (fond).
    Refreshed(Result<Refresh, String>),
    Launched { name: String, result: Result<i32, String> },
    Tick,
    ToggleView,
    /// Retour (clic droit, comme Échap/B) : referme menu, page, prompt.
    Back,
    /// Menu « type de liste » (triangle / Y) : ouvrir, naviguer, choisir.
    ViewMenu,
    ViewMenuNav(i32),
    ViewMenuChoose(usize),
    /// Carré / X : A→Z ou date de modification (jamais dans les favoris).
    ToggleSort,
    ToggleFullscreen,
    Quit,
    Nothing,
}

/// Dernier périphérique utilisé : les aides à l'écran parlent sa langue
/// (glyphes de manette ou touches du clavier), sans qu'il faille débrancher
/// quoi que ce soit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    Keyboard,
    Pad,
}

/// Menu de choix d'image (propositions SteamGridDB) pour un type donné.
struct Picker {
    slug: String,
    /// Nom du jeu (affiché).
    name: String,
    /// Nom réellement envoyé à SteamGridDB : le jeu peut y être mal nommé, et
    /// c'est souvent LA raison pour laquelle la recherche ne donne rien.
    query: String,
    kind: covers::ArtKind,
    index: usize,
    loading: bool,
    error: Option<String>,
    choices: Vec<covers::Choice>,
    /// Vignettes prêtes à afficher, dans le même ordre que `choices`.
    /// Construites UNE fois : `image::Handle::from_bytes` dans la vue créait un
    /// handle neuf à chaque image, donc iced_wgpu redécodait les 24 PNG à
    /// chaque rendu — images qui clignotent et menu poussif (constaté 18/11).
    thumbs: Vec<image::Handle>,
}

/// Actions de la page d'un jeu. Les quatre visuels sont réunis derrière un
/// seul bouton « Images » : la page garde trois pastilles au lieu de cinq.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageAction {
    Play,
    Favorite,
    Images,
}

const PAGE_ACTIONS: [PageAction; 3] = [PageAction::Play, PageAction::Favorite, PageAction::Images];

impl PageAction {
    fn label(self) -> &'static str {
        match self {
            PageAction::Play => "Jouer",
            PageAction::Favorite => "Favori",
            PageAction::Images => "Images",
        }
    }
}

/// Page d'un jeu (ouverte en validant une vignette).
struct Page {
    index: usize,
    action: usize,
}

/// Ordre du menu « Images » : le même que les types de visuels.
const ART_MENU: [covers::ArtKind; 4] = covers::ArtKind::ALL;

/// Les quatre vues, dans l'ordre du menu « type de liste ».
const VIEWS: [ViewMode; 4] = [
    ViewMode::Grid,
    ViewMode::List,
    ViewMode::Mosaic,
    ViewMode::Favorites,
];

/// Nom affiché d'une vue (menu « type de liste »).
fn view_label(view: ViewMode) -> &'static str {
    match view {
        ViewMode::Grid => "Grille",
        ViewMode::List => "Liste",
        ViewMode::Mosaic => "Mosaïque",
        ViewMode::Favorites => "Favoris",
    }
}

/// Position d'une vue dans `VIEWS`.
fn view_index(view: ViewMode) -> usize {
    VIEWS.iter().position(|&v| v == view).unwrap_or(0)
}

pub struct App {
    config: Config,
    games: Vec<Game>,
    selected: usize,
    /// Nom du jeu en cours d'exécution (fenêtre noire) ; `None` = launcher.
    running: Option<String>,
    /// Slug du jeu lancé, pour le temps de jeu (rempli au lancement effectif).
    pending_slug: Option<String>,
    /// Départ du chrono du jeu en cours (slug, instant).
    play_started: Option<(String, std::time::Instant)>,
    /// Temps de jeu cumulé, par slug.
    playtime: std::collections::HashMap<String, u64>,
    /// Ordre de la bibliothèque (A→Z par défaut).
    sort: SortBy,
    /// Menu « type de liste » (triangle / Y) : index choisi.
    view_menu: Option<usize>,
    status: Option<(String, bool)>,
    notice: Option<String>,
    pad: Option<PadKind>,
    /// Toutes les manettes connectées : en débrancher une ne doit pas effacer
    /// l'indicateur des autres.
    pads: Vec<(PadKind, String)>,
    /// Dernier appui sur « retour » : deux appuis rapprochés quittent.
    back_press: Option<std::time::Instant>,
    /// L'invite « appuie encore sur retour » est affichée.
    quit_hint: bool,
    glyphs: Glyphs,
    window: Option<window::Id>,
    fullscreen: bool,
    /// Visuels mémorisés par slug (pas de relecture à chaque réindexation).
    artwork: ArtworkCache,
    /// Index SQLite des paquets, partagé avec les threads de rafraîchissement.
    index: Arc<Mutex<Option<Index>>>,
    /// Un rafraîchissement tourne déjà ; les chemins suivants sont mis en attente.
    refreshing: bool,
    pending: Vec<PathBuf>,
    /// Slugs dont la cover a échoué (réseau) : on ne réessaie pas en boucle.
    cover_failed: HashSet<String>,
    /// (slug, type) actuellement dans la voie de fond : évite de renvoyer sans
    /// cesse la même demande au worker, et sert à libérer celles qui sont
    /// abandonnées quand la vue change.
    visible_covers: Vec<(String, covers::ArtKind)>,
    /// Slugs dont l'icône a déjà été extraite du paquet.
    icon_requests: HashSet<String>,
    /// (slug, type) déjà demandés à SteamGridDB.
    art_requested: HashSet<(String, covers::ArtKind)>,
    /// File du worker de covers (aucune si `covers = false`).
    covers: Option<covers::Queue>,
    /// File du worker de miniatures (grille rapide).
    thumbs: Option<thumbs::Queue>,
    /// File du worker d'assets (icônes à côté des paquets, disque externe).
    assets: Option<assets::Queue>,
    /// Slugs dont le voisinage du paquet a déjà été examiné.
    icon_scanned: HashSet<String>,
    /// Slugs dont la miniature a déjà été demandée.
    thumb_requested: HashSet<String>,
    /// Taille de la fenêtre, pour ne demander que les covers visibles.
    viewport: (f32, f32),
    /// Opacité de l'écran noir (0 = launcher, 1 = noir complet).
    fade: f32,
    fade_target: f32,
    /// Lancement à effectuer dès que l'écran est noir (nom, commande, début du fondu).
    to_launch: Option<(String, launch::Launch, std::time::Instant)>,
    /// Défilement animé vers la sélection.
    scroll_id: Id,
    scroll: f32,
    scroll_target: f32,
    /// Menu « choisir une image » pour le jeu sélectionné.
    picker: Option<Picker>,
    /// Image choisie dont le téléchargement est en cours : le menu se referme
    /// tout de suite (il ne doit pas rester ouvert pendant le réseau).
    picked: Option<(String, covers::ArtKind)>,
    /// Prompt « nom de recherche » (texte en cours de frappe).
    rename: Option<String>,
    /// Noms corrigés pour la session : changer le grid d'un jeu mal reconnu ne
    /// doit pas obliger à re-corriger pour le héros ou le logo.
    search_names: HashMap<String, String>,
    /// Identifiant du champ de saisie, pour lui donner le focus.
    rename_id: Id,
    /// Page du jeu (héros, logo, actions) ; `None` = liste des jeux.
    page: Option<Page>,
    /// Menu « quel visuel changer » de la page : index dans `ART_MENU`.
    art_menu: Option<usize>,
    /// Slugs marqués ★ (fichier `favorites.txt`).
    favorites: HashSet<String>,
    /// Tous les jeux ; `games` n'en est que la vue filtrée (favoris).
    all_games: Vec<Game>,
    /// La fenêtre a le focus. La manette est lue sur `/dev/input` directement,
    /// donc sans cette garde elle piloterait le launcher pendant qu'on joue ou
    /// qu'on travaille dans une autre fenêtre.
    active: bool,
    /// Dernier périphérique utilisé (clavier ou manette).
    input: Input,
    /// Image de remplacement (sample) quand la cover n'est pas encore trouvée.
    sample: image::Handle,
    /// Étoile des favoris (décodée une seule fois).
    star: image::Handle,
    /// Horodatage de la dernière image (animations indépendantes du framerate).
    last_tick: std::time::Instant,
}

impl App {
    pub fn boot(
        config: Config,
        notice: Option<String>,
        autoplay: Option<usize>,
        open_picker: bool,
    ) -> (Self, Task<Message>) {
        // ancienne disposition (covers/, icons/ à la racine) → assets/
        config.migrate_cache();
        let (bus, receiver) = unbounded();
        let _ = BUS.set(bus);
        if let Ok(mut slot) = BUS_RX.lock() {
            *slot = Some(receiver);
        }
        crate::gamepad::spawn(|event| post(Message::Pad(event)));

        // surveillance des seuls dossiers de jeux (jamais le cache : boucle)
        crate::watch::spawn(config.game_dirs(), |paths| post(Message::Changed(paths)));

        let emit_cover = |event| post(cover_message(event));
        let covers = config.covers.then(|| {
            covers::spawn(
                config.assets_dir(),
                config.cache_dir().join("marks"),
                config.steamgriddb_key(),
                emit_cover,
            )
        });
        let thumbs = thumbs::spawn(config.thumb_dir(), |event| match event {
            thumbs::ThumbEvent::Ready { slug, bytes } => post(Message::Thumb { slug, bytes }),
        });
        let assets = assets::spawn(config.icon_dirs(), config.pack_icon_dir(), |event| {
            match event {
                assets::Event::Icon { slug, icon } => post(Message::LocalIcon { slug, icon }),
            }
        });

        // index persistant : la liste s'affiche sans relire les 540 dossiers
        let index = Arc::new(Mutex::new(
            Index::open(&config.index_path()).ok(),
        ));
        let mut artwork = ArtworkCache::new();
        let rows = index
            .lock()
            .ok()
            .and_then(|db| db.as_ref().and_then(|db| db.all().ok()))
            .unwrap_or_default();
        let all_games = library::from_rows(&config, &rows, &mut artwork, &[]);
        let favorites = favorites::load(&config.favorites_path());
        let playtime_path = config.playtime_path();
        let fullscreen = config.fullscreen;
        // dernier type de vue et dernier tri choisis : ils survivent à la
        // fermeture (le fichier d'état est dans le cache, jamais dans
        // `config.toml` qui appartient à gablue-manager)
        let saved = state::load(&config.state_path());
        let mut config = config;
        if let Some(view) = saved.view {
            config.view = view;
        }
        let mut app = Self {
            config,
            games: Vec::new(),
            selected: 0,
            running: None,
            status: None,
            notice,
            pad: None,
            pads: Vec::new(),
            back_press: None,
            quit_hint: false,
            glyphs: Glyphs::for_kind(PadKind::Xbox),
            window: None,
            fullscreen,
            artwork,
            index,
            refreshing: false,
            pending: Vec::new(),
            cover_failed: HashSet::new(),
            visible_covers: Vec::new(),
            icon_requests: HashSet::new(),
            art_requested: HashSet::new(),
            covers,
            thumbs: Some(thumbs),
            assets: Some(assets),
            icon_scanned: HashSet::new(),
            thumb_requested: HashSet::new(),
            viewport: (1280.0, 720.0),
            // on ouvre sur du noir et on laisse la fenêtre apparaître en fondu
            fade: 1.0,
            fade_target: 0.0,
            to_launch: None,
            pending_slug: None,
            play_started: None,
            playtime: playtime::load(&playtime_path),
            sort: saved.sort,
            view_menu: None,
            scroll_id: Id::unique(),
            scroll: 0.0,
            scroll_target: 0.0,
            last_tick: std::time::Instant::now(),
            picker: None,
            picked: None,
            rename: None,
            search_names: HashMap::new(),
            rename_id: Id::unique(),
            page: None,
            art_menu: None,
            favorites,
            all_games,
            active: true,
            input: Input::Keyboard,
            sample: image::Handle::from_bytes(COVER_SAMPLE),
            star: image::Handle::from_bytes(STAR),
        };
        app.apply_filter();
        app.request_visible_covers();
        app.sweep_thumbs();
        app.sweep_icons();
        // réindexation complète en tâche de fond : le disque peut avoir changé
        // pendant que le launcher était fermé
        app.refresh(Vec::new());

        let mut tasks = vec![window::latest().map(|id| match id {
            Some(id) => Message::Window(id),
            None => Message::Nothing,
        })];
        if let Some(index) = autoplay {
            // crochet de test : --play <n> enchaîne directement sur un lancement
            tasks.push(Task::done(Message::Play(index)));
        }
        if open_picker {
            // crochet de test : --picker ouvre le menu de choix d'image
            tasks.push(Task::done(Message::OpenPicker(covers::ArtKind::Grid)));
        }

        crate::stamp("préparation");
        (app, Task::batch(tasks))
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            keyboard::listen().map(Message::Key),
            window::events().map(|(_id, event)| match event {
                window::Event::Resized(size) => Message::Resized(size),
                window::Event::Focused => Message::Active(true),
                window::Event::Unfocused => Message::Active(false),
                _ => Message::Nothing,
            }),
            Subscription::run(bus_stream),
        ];
        if self.animating() {
            subscriptions.push(window::frames().map(|_| Message::Tick));
        }
        Subscription::batch(subscriptions)
    }

    /// Une animation tourne-t-elle ? (sinon aucun abonnement aux images)
    fn animating(&self) -> bool {
        (self.fade - self.fade_target).abs() > 0.001
            || (self.scroll - self.scroll_target).abs() > 0.5
            || self.to_launch.is_some()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Window(id) => {
                self.window = Some(id);
                Task::none()
            }
            Message::Active(active) => {
                self.active = active;
                if !active {
                    // l'invite « appuie encore » ne survit pas à la perte de focus
                    self.back_press = None;
                    self.quit_hint = false;
                }
                Task::none()
            }
            Message::Resized(size) => {
                self.viewport = (size.width, size.height);
                self.update_scroll_target();
                self.request_visible_covers();
                Task::none()
            }
            Message::Tick => self.tick(),
            Message::Key(event) => self.on_key(event),
            Message::Pad(event) => self.on_pad(event),
            Message::Play(index) => {
                self.selected = index.min(self.games.len().saturating_sub(1));
                self.update_scroll_target();
                self.launch()
            }
            Message::Art { slug, kind, bytes } => {
                // la demande est terminée (aboutie ou marquée introuvable) :
                // elle redevient demandable, sinon un échec réseau la bloquait
                // pour toute la session
                self.art_requested.remove(&(slug.clone(), kind));
                // un visuel choisi dans le menu remplace toujours l'existant ;
                // un visuel automatique ne remplace pas une icône de paquet
                let chosen = self
                    .picked
                    .as_ref()
                    .is_some_and(|(chosen_slug, chosen_kind)| {
                        *chosen_slug == slug && *chosen_kind == kind
                    });
                if let Some(bytes) = bytes {
                    let handle = image::Handle::from_bytes(bytes);
                    for game in self.games.iter_mut().chain(self.all_games.iter_mut()) {
                        if game.slug != slug {
                            continue;
                        }
                        match kind {
                            covers::ArtKind::Grid => game.cover = Some(handle.clone()),
                            covers::ArtKind::Hero => game.hero = Some(handle.clone()),
                            covers::ArtKind::Logo => game.logo = Some(handle.clone()),
                            covers::ArtKind::Icon => {
                                if chosen || game.icon.is_none() {
                                    game.icon = Some(handle.clone());
                                }
                            }
                        }
                    }
                    // le cache de visuels alimente les réindexations : sans
                    // cette mise à jour, un rescan oubliait ce qui vient d'être
                    // téléchargé
                    if let Some(art) = self.artwork.get_mut(&slug) {
                        match kind {
                            covers::ArtKind::Grid => art.cover = Some(handle.clone()),
                            covers::ArtKind::Hero => art.hero = Some(handle.clone()),
                            covers::ArtKind::Logo => art.logo = Some(handle.clone()),
                            covers::ArtKind::Icon => {
                                if chosen || art.icon.is_none() {
                                    art.icon = Some(handle.clone());
                                }
                            }
                        }
                    }
                    // nouvelle cover : sa miniature est à refaire
                    if kind == covers::ArtKind::Grid {
                        self.thumb_requested.remove(&slug);
                        self.request_thumb(&slug);
                    }
                }
                // une image choisie efface le message d'attente
                if chosen {
                    self.picked = None;
                    self.status = None;
                }
                Task::none()
            }
            Message::Icon { slug, bytes } => {
                let handle = image::Handle::from_bytes(bytes);
                for game in self.games.iter_mut().chain(self.all_games.iter_mut()) {
                    if game.slug == slug && game.icon.is_none() {
                        game.icon = Some(handle.clone());
                    }
                }
                if let Some(art) = self.artwork.get_mut(&slug)
                    && art.icon.is_none()
                {
                    art.icon = Some(handle);
                }
                Task::none()
            }
            Message::LocalIcon { slug, icon } => {
                self.icon_scanned.insert(slug.clone());
                // jeu à l'écran sans icône à côté du paquet : l'extraction
                // depuis le squashfs peut maintenant être tentée
                let visible = icon.is_none()
                    && self
                        .visible_range()
                        .any(|index| self.games.get(index).is_some_and(|g| g.slug == slug));
                if visible {
                    self.request_visible_covers();
                }
                if let Some(handle) = icon {
                    for game in self.games.iter_mut().chain(self.all_games.iter_mut()) {
                        if game.slug == slug && game.icon.is_none() {
                            game.icon = Some(handle.clone());
                        }
                    }
                    if let Some(art) = self.artwork.get_mut(&slug)
                        && art.icon.is_none()
                    {
                        art.icon = Some(handle);
                    }
                }
                Task::none()
            }
            Message::Thumb { slug, bytes } => {
                let handle = image::Handle::from_bytes(bytes);
                for game in self.games.iter_mut().chain(self.all_games.iter_mut()) {
                    if game.slug == slug {
                        game.thumb = Some(handle.clone());
                    }
                }
                if let Some(art) = self.artwork.get_mut(&slug) {
                    art.thumb = Some(handle);
                }
                self.thumb_requested.remove(&slug);
                Task::none()
            }
            Message::ArtFailed { slug, kind, error } => {
                eprintln!("glauncher : {} {slug} : {error}", kind.label());
                self.art_requested.remove(&(slug.clone(), kind));
                if kind == covers::ArtKind::Grid {
                    self.cover_failed.insert(slug.clone());
                }
                if self
                    .picked
                    .as_ref()
                    .is_some_and(|(s, k)| *s == slug && *k == kind)
                {
                    self.picked = None;
                    self.status = Some((format!("image non appliquée : {error}"), true));
                }
                if let Some(picker) = self.picker.as_mut()
                    && picker.slug == slug
                    && picker.kind == kind
                {
                    picker.loading = false;
                    picker.error = Some(error);
                }
                Task::none()
            }
            Message::OpenPicker(kind) => {
                if self.running.is_some() {
                    return Task::none();
                }
                let Some(game) = self.games.get(self.selected) else {
                    return Task::none();
                };
                let (slug, name) = (game.slug.clone(), game.name.clone());

                // l'icône du paquet est proposée avec les icônes SteamGridDB :
                // elle reste sélectionnable même après un choix distant
                let mut choices = Vec::new();
                if kind == covers::ArtKind::Icon {
                    let pack = self.config.pack_icon_dir().join(format!("{slug}.png"));
                    if let Ok(bytes) = std::fs::read(&pack) {
                        choices.push(covers::Choice {
                            url: format!("file://{}", pack.display()),
                            ext: "png".to_string(),
                            thumb: bytes,
                        });
                    }
                }

                let query = self
                    .search_names
                    .get(&slug)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                let thumbs = choices
                    .iter()
                    .map(|choice| image::Handle::from_bytes(choice.thumb.clone()))
                    .collect();
                self.picker = Some(Picker {
                    slug: slug.clone(),
                    name: name.clone(),
                    query,
                    kind,
                    index: 0,
                    loading: true,
                    error: None,
                    choices,
                    thumbs,
                });
                // le menu d'image a la priorité : plus de visuels de fond
                if let Some(queue) = self.covers.as_ref() {
                    queue.pause(true);
                }
                self.send_cover_query(covers::Query::Choices { slug, name, kind });
                Task::none()
            }
            Message::RenameOpen => {
                let Some(picker) = self.picker.as_ref() else {
                    return Task::none();
                };
                self.rename = Some(picker.query.clone());
                // présélection : taper remplace directement le nom proposé
                Task::batch([
                    operation::focus(self.rename_id.clone()),
                    operation::select_all(self.rename_id.clone()),
                ])
            }
            Message::RenameInput(text) => {
                if self.rename.is_some() {
                    self.rename = Some(text);
                }
                Task::none()
            }
            Message::RenameCancel => {
                self.rename = None;
                Task::none()
            }
            Message::RenameSubmit => {
                let Some(query) = self.rename.take() else {
                    return Task::none();
                };
                let query = query.trim().to_string();
                // un nom vide ne relancerait la même recherche : on garde l'ancien
                if query.is_empty() {
                    return Task::none();
                }
                let Some(picker) = self.picker.as_mut() else {
                    return Task::none();
                };
                let (slug, kind) = (picker.slug.clone(), picker.kind);
                picker.query = query.clone();
                self.search_names.insert(slug.clone(), query.clone());
                picker.choices.clear();
                picker.thumbs.clear();
                picker.error = None;
                picker.loading = true;
                picker.index = 0;
                eprintln!("glauncher : recherche « {query} » pour {slug}");
                self.send_cover_query(covers::Query::Choices {
                    slug,
                    name: query,
                    kind,
                });
                Task::none()
            }
            Message::ClosePicker => {
                let was_open = self.picker.take().is_some();
                self.rename = None;
                if was_open {
                    self.resume_covers();
                }
                Task::none()
            }
            Message::PickerNav(nav) => {
                if let Some(picker) = self.picker.as_mut()
                    && !picker.choices.is_empty()
                {
                    let last = picker.choices.len() - 1;
                    let columns = 5;
                    picker.index = match nav {
                        Nav::Next => (picker.index + 1).min(last),
                        Nav::Prev => picker.index.saturating_sub(1),
                        Nav::Down => (picker.index + columns).min(last),
                        Nav::Up => picker.index.saturating_sub(columns),
                        Nav::PageDown => (picker.index + columns * 2).min(last),
                        Nav::PageUp => picker.index.saturating_sub(columns * 2),
                    };
                }
                Task::none()
            }
            Message::PickerSelect(index) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.index = index;
                }
                Task::none()
            }
            Message::PickerPick => {
                let Some(picker) = self.picker.as_ref() else {
                    return Task::none();
                };
                // en cas d'échec, A relance la recherche d'images
                if let Some(error) = picker.error.clone() {
                    let (slug, name, kind) = (picker.slug.clone(), picker.name.clone(), picker.kind);
                    if let Some(picker) = self.picker.as_mut() {
                        picker.error = Some(error);
                        picker.loading = true;
                    }
                    self.send_cover_query(covers::Query::Choices { slug, name, kind });
                    return Task::none();
                }
                let Some(choice) = picker.choices.get(picker.index) else {
                    return Task::none();
                };
                let (slug, kind) = (picker.slug.clone(), picker.kind);
                let (url, ext) = (choice.url.clone(), choice.ext.clone());
                self.send_cover_query(covers::Query::Pick {
                    slug: slug.clone(),
                    kind,
                    url,
                    ext,
                });
                // Le menu se referme IMMÉDIATEMENT : rester ouvert pendant le
                // téléchargement donnait l'impression que la validation
                // n'était pas prise en compte (constaté 18/11). `picked` garde
                // la trace du choix pour la suite (remplacement de l'icône).
                self.picker = None;
                self.picked = Some((slug, kind));
                self.status = Some(("application de l'image…".to_string(), false));
                self.resume_covers();
                Task::none()
            }
            Message::Choices {
                slug,
                kind,
                choices,
            } => {
                if let Some(picker) = self.picker.as_mut()
                    && picker.slug == slug
                    && picker.kind == kind
                {
                    // la 1re proposition est le « top » : on garde l'éventuelle
                    // icône du paquet en tête pour l'icône
                    let keep = std::mem::take(&mut picker.choices);
                    picker.loading = false;
                    picker.choices = keep.into_iter().chain(choices).collect();
                    picker.thumbs = picker
                        .choices
                        .iter()
                        .map(|choice| image::Handle::from_bytes(choice.thumb.clone()))
                        .collect();
                    picker.index = 0;
                }
                Task::none()
            }
            Message::CardClicked(index) => {
                if self.running.is_some() {
                    return Task::none();
                }
                if index == self.selected {
                    Task::done(Message::OpenPage(index))
                } else {
                    self.selected = index;
                    self.update_scroll_target();
                    self.request_visible_covers();
                    Task::none()
                }
            }
            Message::Launched { name, result } => {
                self.running = None;
                // temps de jeu : on ignore les lancements avortés (moins de 5 s,
                // un binaire qui ne démarre pas ne compte pas comme une partie)
                if let Some((slug, started)) = self.play_started.take() {
                    let seconds = started.elapsed().as_secs();
                    if seconds >= 5 {
                        playtime::add(
                            &self.config.playtime_path(),
                            &mut self.playtime,
                            &slug,
                            seconds,
                        );
                    }
                }
                self.fade_target = 0.0;
                // on revient dans la liste des jeux, pas dans la page du jeu
                self.page = None;
                self.status = match result {
                    Ok(0) => None,
                    Ok(code) => Some((format!("{name} : code de sortie {code}"), true)),
                    Err(err) => Some((err, true)),
                };
                let focus = match self.window {
                    Some(id) => window::gain_focus(id).map(|()| Message::Nothing),
                    None => Task::none(),
                };
                if std::env::var_os("GLAUNCHER_PAD_DEBUG").is_some() {
                    eprintln!(
                        "glauncher : retour de {name} — sélection {} / défilement {:.0}",
                        self.selected, self.scroll
                    );
                }
                // la liste reste là où elle était : sélection ET défilement
                Task::batch([
                    focus,
                    operation::scroll_to(
                        self.scroll_id.clone(),
                        AbsoluteOffset {
                            x: 0.0,
                            y: self.scroll,
                        },
                    ),
                ])
            }
            Message::OpenPage(index) => {
                let Some(_) = self.games.get(index) else {
                    return Task::none();
                };
                self.selected = index;
                self.page = Some(Page { index, action: 0 });
                self.art_menu = None;
                self.update_scroll_target();
                // le héros de la page est prioritaire sur les visuels de fond
                self.request_page_assets(index);
                Task::none()
            }
            Message::ClosePage => {
                self.page = None;
                self.art_menu = None;
                Task::none()
            }
            Message::PageNav(delta) => {
                if let Some(page) = self.page.as_mut() {
                    let last = PAGE_ACTIONS.len() - 1;
                    let next = page.action as i32 + delta;
                    page.action = next.clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::PageChoose(index) => {
                if let Some(page) = self.page.as_mut() {
                    page.action = index.min(PAGE_ACTIONS.len() - 1);
                }
                self.page_act()
            }
            Message::PageAct => self.page_act(),
            Message::ArtMenu => {
                self.art_menu = Some(0);
                Task::none()
            }
            Message::ArtMenuNav(delta) => {
                if let Some(choice) = self.art_menu.as_mut() {
                    let last = ART_MENU.len() - 1;
                    *choice = (*choice as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::ArtMenuChoose(index) => {
                self.art_menu = Some(index.min(ART_MENU.len() - 1));
                Task::done(Message::ArtMenuPick)
            }
            Message::ArtMenuPick => {
                let Some(choice) = self.art_menu.take() else {
                    return Task::none();
                };
                let Some(kind) = ART_MENU.get(choice).copied() else {
                    return Task::none();
                };
                Task::done(Message::OpenPicker(kind))
            }
            Message::ArtMenuClose => {
                self.art_menu = None;
                Task::none()
            }
            Message::Scrolled(offset) => {
                // on suit le défilement réel : la position survit au retour
                // d'un jeu et sert à demander les covers visibles
                if (self.scroll - offset).abs() > 0.5 {
                    // La roulette (ou la barre) déplace la vue : sa position
                    // devient la CIBLE et l'animation s'arrête.
                    //
                    // Nos propres défilements n'entrent pas ici : `tick()` pose
                    // `self.scroll` avant de demander `scroll_to`, donc l'offset
                    // reçu vaut déjà `self.scroll`. Tout écart est donc un geste
                    // de l'utilisateur. Comparer les SENS (pour « continuer
                    // l'animation si elle va dans le même sens ») mangeait des
                    // crans de molette et donnait des remontées saccadées
                    // (constaté 18/11).
                    self.scroll_target = offset;
                    self.scroll = offset;
                    self.request_visible_covers();
                }
                Task::none()
            }
            Message::Zoom(step) => {
                match self.config.view {
                    // mosaïque : la densité est continue, `cell_size` la règle
                    ViewMode::Mosaic => {
                        let next = (self.config.cell_size as f32 + step as f32 * ZOOM_STEP)
                            .clamp(CELL_MIN, CELL_MAX);
                        self.config.cell_size = next.round() as u32;
                    }
                    // grille qui remplit la largeur : `cell_size` n'est qu'une
                    // largeur MINIMALE, donc tant que la grille tient dans
                    // `columns` colonnes le zoom ne changeait rien du tout
                    // (constaté 18/11 : +/− sans effet). Le levier réel est le
                    // nombre de colonnes : un cran = une colonne.
                    ViewMode::Grid | ViewMode::Favorites | ViewMode::List => {
                        self.config.columns = zoom_columns(self.config.columns, step);
                        // `cell_size` n'est qu'une largeur MINIMALE : la fixer
                        // sur la largeur affichée rendait `fitted` égal au
                        // nombre de colonnes courant, donc le zoom arrière ne
                        // pouvait plus JAMAIS en rajouter (constaté 18/11 :
                        // « dézoomer ne fait rien », colonnes 5 → 6 en mémoire
                        // mais 4 à l'écran). On la calcule à l'envers, pour que
                        // la grille produise exactement ce nombre de colonnes.
                        self.config.cell_size =
                            cell_for_columns(self.viewport.0, self.config.columns, self.gap());
                    }
                }
                // les colonnes et la hauteur de rangée changent avec la taille
                self.update_scroll_target();
                self.request_visible_covers();
                self.status = None;
                Task::none()
            }
            Message::ToggleView => {
                let before = self.config.view;
                self.config.view = self.config.view.flipped();
                self.remember();
                if before == ViewMode::Favorites || self.config.view == ViewMode::Favorites {
                    self.apply_filter();
                }
                self.scroll = 0.0;
                self.update_scroll_target();
                self.request_visible_covers();
                Task::none()
            }
            Message::Changed(paths) => {
                self.refresh(paths);
                Task::none()
            }
            Message::Refreshed(result) => {
                self.refreshing = false;
                match result {
                    Ok(refresh) => {
                        let before = self.games.len();
                        self.apply(&refresh);
                        if self.games.len() != before {
                            self.status = Some((plural(self.games.len()), false));
                        }
                    }
                    Err(error) => self.status = Some((error, true)),
                }
                if !self.pending.is_empty() {
                    let paths = std::mem::take(&mut self.pending);
                    self.refresh(paths);
                }
                Task::none()
            }
            Message::ViewMenu => {
                if self.running.is_some() {
                    return Task::none();
                }
                self.view_menu = Some(view_index(self.config.view));
                Task::none()
            }
            Message::ViewMenuNav(delta) => {
                if let Some(choice) = self.view_menu.as_mut() {
                    let last = VIEWS.len() - 1;
                    *choice = (*choice as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::ViewMenuChoose(index) => {
                self.view_menu = None;
                let Some(&view) = VIEWS.get(index) else {
                    return Task::none();
                };
                self.config.view = view;
                self.remember();
                self.apply_filter();
                self.scroll = 0.0;
                self.scroll_target = 0.0;
                self.visible_covers.clear();
                self.request_visible_covers();
                operation::scroll_to(
                    self.scroll_id.clone(),
                    AbsoluteOffset { x: 0.0, y: 0.0 },
                )
            }
            Message::ToggleSort => {
                if self.config.view == ViewMode::Favorites {
                    self.status = Some(("tri indisponible dans les favoris".into(), false));
                    return Task::none();
                }
                self.sort = match self.sort {
                    SortBy::Name => SortBy::Modified,
                    SortBy::Modified => SortBy::Name,
                };
                self.remember();
                self.apply_filter();
                self.request_visible_covers();
                Task::none()
            }
            Message::Back => {
                // même priorité qu'Échap : le plus profond d'abord
                if self.rename.is_some() {
                    self.rename = None;
                    return Task::none();
                }
                if self.picker.is_some() {
                    return Task::done(Message::ClosePicker);
                }
                if self.art_menu.is_some() {
                    return Task::done(Message::ArtMenuClose);
                }
                if self.view_menu.is_some() {
                    self.view_menu = None;
                    return Task::none();
                }
                if self.page.is_some() {
                    return Task::done(Message::ClosePage);
                }
                Task::none()
            }
            Message::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                let mode = if self.fullscreen {
                    window::Mode::Fullscreen
                } else {
                    window::Mode::Windowed
                };
                match self.window {
                    Some(id) => window::set_mode(id, mode).map(|()| Message::Nothing),
                    None => Task::none(),
                }
            }
            Message::Quit => iced::exit(),
            Message::Nothing => Task::none(),
        }
    }

    // --- actions ------------------------------------------------------------

    fn navigate(&mut self, nav: Nav) -> Task<Message> {
        if self.games.is_empty() {
            return Task::none();
        }
        let step = match self.config.view {
            ViewMode::Grid | ViewMode::Favorites | ViewMode::Mosaic => self.metrics().2,
            ViewMode::List => 1,
        };
        let last = self.games.len() - 1;
        // L1/R1 : une page entière (ce qui tient à l'écran d'un coup)
        let page = self.page_size().max(1);
        self.selected = match nav {
            Nav::Next => (self.selected + 1).min(last),
            Nav::Prev => self.selected.saturating_sub(1),
            Nav::Down => (self.selected + step).min(last),
            Nav::Up => self.selected.saturating_sub(step),
            Nav::PageDown => (self.selected + page).min(last),
            Nav::PageUp => self.selected.saturating_sub(page),
        };
        self.update_scroll_target();
        self.request_visible_covers();

        // Saut de page (L1/R1) : on y va d'un coup. L'animation d'un écran
        // entier n'apporte rien et, en appui répété, elle courait après chaque
        // page : la sélection sortait du champ (constaté 18/11).
        if matches!(nav, Nav::PageDown | Nav::PageUp) {
            self.scroll = self.scroll_target;
            return operation::scroll_to(
                self.scroll_id.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: self.scroll,
                },
            );
        }
        Task::none()
    }

    fn launch(&mut self) -> Task<Message> {
        if self.running.is_some() {
            return Task::none();
        }
        let Some(game) = self.games.get(self.selected).cloned() else {
            return Task::none();
        };

        let command = launch::build(&self.config, &game);
        self.running = Some(game.name.clone());
        self.pending_slug = Some(game.slug.clone());
        self.status = None;
        // le jeu démarre quand le fondu au noir est terminé (voir tick)
        self.to_launch = Some((game.name.clone(), command, std::time::Instant::now()));
        self.fade_target = 1.0;
        if std::env::var_os("GLAUNCHER_PAD_DEBUG").is_some() {
            eprintln!(
                "glauncher : départ jeu — sélection {} / défilement {:.0}",
                self.selected, self.scroll
            );
        }

        Task::none()
    }

    /// Une image d'animation : fondu au noir et défilement vers la sélection.
    fn tick(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let now = std::time::Instant::now();
        // pas de temps borné : une image lente ne fait pas sauter l'animation
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f32()
            .clamp(0.001, 0.05);
        self.last_tick = now;

        // l'invite « appuie encore sur retour » expire avec la fenêtre
        if self.quit_hint
            && self
                .back_press
                .is_none_or(|at| at.elapsed() >= QUIT_WINDOW)
        {
            self.quit_hint = false;
            self.back_press = None;
        }

        if (self.fade - self.fade_target).abs() > 0.001 {
            let step = dt / FADE_SECONDS;
            self.fade = if self.fade < self.fade_target {
                (self.fade + step).min(self.fade_target)
            } else {
                (self.fade - step).max(self.fade_target)
            };
        }

        // écran noir atteint : on lance le jeu
        if self.fade >= 0.999
            && let Some((name, command, started)) = self.to_launch.take()
        {
            eprintln!(
                "glauncher : écran noir en {} ms, lancement de {name}",
                started.elapsed().as_millis()
            );
            // le chrono du temps de jeu part au lancement EFFECTIF du jeu
            let slug = self.pending_slug.take().unwrap_or_default();
            self.play_started = if slug.is_empty() {
                None
            } else {
                Some((slug.clone(), std::time::Instant::now()))
            };
            spawn_launch(name, command, self.config.cache_dir(), slug);
        }

        if (self.scroll - self.scroll_target).abs() > 0.5 {
            self.scroll = ease_or_snap(self.scroll, self.scroll_target, self.row_span(), dt);
            tasks.push(operation::scroll_to(
                self.scroll_id.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: self.scroll,
                },
            ));
        } else if self.scroll != self.scroll_target {
            self.scroll = self.scroll_target;
            tasks.push(operation::scroll_to(
                self.scroll_id.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: self.scroll,
                },
            ));
        } else {
            return Task::batch(tasks);
        }
        // le défilement change ce qui est visible : on complète les covers
        self.request_visible_covers();

        Task::batch(tasks)
    }

    /// Position de défilement qui garde la sélection à l'écran (centrée).
    fn update_scroll_target(&mut self) {
        let columns = self.metrics().2;
        let (row, row_height) = match self.config.view {
            ViewMode::Grid | ViewMode::Favorites | ViewMode::Mosaic => {
                (self.selected / columns, self.row_height() + self.gap())
            }
            ViewMode::List => (self.selected, self.list_row_height() + 8.0),
        };
        let window = (self.viewport.1 - 150.0).max(120.0);
        let y = row as f32 * row_height;
        self.scroll_target = (y - (window - row_height) / 2.0).max(0.0);
    }

    /// Rescanne les dossiers en gardant le jeu sélectionné si possible.
    /// Un dossier ou un paquet disparu sort de la liste ; les covers restent
    /// dans le cache (voir `library::cache_cover`).
    /// Demande une réindexation (liste vide = scan complet).
    fn refresh(&mut self, changed: Vec<PathBuf>) {
        if self.refreshing {
            self.pending.extend(changed);
            return;
        }
        self.refreshing = true;
        spawn_refresh(self.config.clone(), self.index.clone(), changed);
    }

    /// Remplace la liste par le résultat d'une réindexation.
    fn apply(&mut self, refresh: &Refresh) {
        let previous = self.games.get(self.selected).map(|game| game.path.clone());
        self.all_games = library::from_rows(
            &self.config,
            &refresh.rows,
            &mut self.artwork,
            &refresh.changed,
        );
        self.games = self.all_games.clone();
        if self.config.view == ViewMode::Favorites {
            self.apply_filter();
        } else {
            self.sort_games();
        }
        self.selected = previous
            .and_then(|path| self.games.iter().position(|game| game.path == path))
            .unwrap_or_else(|| self.selected.min(self.games.len().saturating_sub(1)));
        eprintln!(
            "glauncher : bibliothèque réindexée ({}, {} changements)",
            plural(self.games.len()),
            refresh.changed.len()
        );
        self.update_scroll_target();
        self.request_visible_covers();
        self.sweep_thumbs();
        self.sweep_icons();
    }

    /// Envoie une action de l'utilisateur (menu image) : elle passe devant.
    fn send_cover_query(&self, query: covers::Query) {
        if let Some(queue) = self.covers.as_ref() {
            queue.urgent(query);
        }
    }

    /// Le menu d'image est refermé : les covers de fond reprennent.
    fn resume_covers(&mut self) {
        if let Some(queue) = self.covers.clone() {
            queue.pause(false);
        }
        self.visible_covers.clear();
        self.request_visible_covers();
    }

    /// Demande les covers des seuls jeux actuellement visibles (la navigation
    /// déplace la fenêtre, donc les demandes suivent la sélection) : inutile de
    /// solliciter SteamGridDB pour 540 jeux d'un coup.
    /// Rangées qui tiennent à l'écran. La dernière peut être partielle : elle
    /// est comptée, sinon ses covers ne sont jamais demandées.
    fn rows_visible(&self) -> usize {
        let usable = (self.viewport.1 - 150.0).max(120.0);
        let height = self.row_span();
        (usable / height).ceil().max(1.0) as usize
    }

    /// Hauteur d'une rangée, espacement compris (grille ou liste).
    fn row_span(&self) -> f32 {
        match self.config.view {
            ViewMode::Grid | ViewMode::Favorites | ViewMode::Mosaic => self.row_height() + self.gap(),
            ViewMode::List => self.list_row_height() + 8.0,
        }
    }

    /// Nombre de jeux d'une page (L1/R1, Page↑/↓).
    fn page_size(&self) -> usize {
        match self.config.view {
            ViewMode::Grid | ViewMode::Favorites | ViewMode::Mosaic => self.rows_visible() * self.metrics().2,
            ViewMode::List => self.rows_visible(),
        }
    }

    /// Plage de jeux réellement à l'écran, calculée sur la position de
    /// défilement et non sur la page de la sélection : une rangée à moitié
    /// visible (haut ou bas) doit aussi déclencher le téléchargement de sa
    /// cover. Une rangée de marge est ajoutée de chaque côté.
    fn visible_range(&self) -> std::ops::Range<usize> {
        let len = self.games.len();
        if len == 0 {
            return 0..0;
        }
        let first = (self.scroll / self.row_span()).floor().max(0.0) as usize;
        let rows = self.rows_visible();
        match self.config.view {
            ViewMode::Grid | ViewMode::Favorites | ViewMode::Mosaic => {
                let columns = self.metrics().2;
                visible_items(self.scroll, self.row_span(), rows, columns, len)
            }
            ViewMode::List => first.saturating_sub(1)..(first + rows + 1).min(len),
        }
    }

    fn request_visible_covers(&mut self) {
        // miniatures d'abord : elles ne dépendent pas du réseau
        for index in self.visible_range() {
            let missing = self.games.get(index).and_then(|game| {
                (game.thumb.is_none() && game.cover.is_some()).then(|| game.slug.clone())
            });
            if let Some(slug) = missing {
                self.request_thumb(&slug);
            }
        }

        self.request_near_pack();

        let Some(queue) = self.covers.clone() else {
            return;
        };
        let mut wanted: Vec<covers::Query> = Vec::new();
        let mut cles: Vec<(String, covers::ArtKind)> = Vec::new();

        for index in self.visible_range() {
            if let Some(query) = self.missing_assets(index, false, false) {
                if let covers::Query::Auto { slug, kinds, .. } = &query {
                    for kind in kinds {
                        cles.push((slug.clone(), *kind));
                    }
                }
                wanted.push(query);
            }
        }

        // la voie de fond est remplacée : ce qui est à l'écran passe avant la
        // file accumulée en naviguant. Les demandes ainsi ABANDONNÉES
        // redeviennent demandables : sans ça, un jeu quitté avant que le worker
        // ne le traite restait marqué « demandé » pour la session et ne
        // recevait plus jamais ses images en revenant dessus (constaté 18/11 :
        // « les images de Cat Quest ne se téléchargent pas quand je suis devant »).
        if cles != self.visible_covers {
            for cle in &self.visible_covers {
                if !cles.contains(cle) {
                    self.art_requested.remove(cle);
                }
            }
            self.visible_covers = cles;
            queue.visible(wanted);
        }
    }

    /// Ce qu'il manque à un jeu : icône du paquet (voie locale) et visuels
    /// SteamGridDB. `urgent` = demande pour la page ouverte (le héros doit
    /// arriver tout de suite) ; `tout` = la page, qui affiche les quatre types
    /// (grille, liste et mosaïque n'en montrent que deux : cover et icône).
    fn missing_assets(
        &mut self,
        index: usize,
        urgent: bool,
        tout: bool,
    ) -> Option<covers::Query> {
        let queue = self.covers.clone()?;
        let game = self.games.get(index)?;

        // l'icône du paquet est locale et rapide, mais seulement après le
        // passage du worker d'assets : sinon on lancerait `unsquashfs` sur des
        // jeux dont l'icône est posée à côté du paquet
        if game.icon.is_none()
            && self.icon_scanned.contains(&game.slug)
            && self.icon_requests.insert(game.slug.clone())
        {
            let pack = covers::Query::Icon {
                slug: game.slug.clone(),
                pack: game.path.clone(),
            };
            if urgent {
                queue.urgent(pack);
            } else {
                queue.local(pack);
            }
        }

        // La grille, la liste et la mosaïque n'affichent que la cover et
        // l'icône : demander aussi héros et logo quadruplait les appels au
        // worker throttlé, et la cover du jeu qu'on avait devant soi arrivait
        // derrière des dizaines de héros inutiles (constaté 18/11).
        let demande: &[covers::ArtKind] = if tout {
            &covers::ArtKind::ALL
        } else {
            &DEUX_TYPES
        };
        let mut kinds = Vec::new();
        for &kind in demande {
            let present = match kind {
                covers::ArtKind::Grid => game.cover.is_some(),
                covers::ArtKind::Hero => game.hero.is_some(),
                covers::ArtKind::Logo => game.logo.is_some(),
                covers::ArtKind::Icon => game.icon.is_some(),
            };
            // l'icône SteamGridDB ne se demande qu'une fois la piste locale
            // épuisée (voisinage du paquet scanné, extraction tentée)
            if kind == covers::ArtKind::Icon && !self.icon_scanned.contains(&game.slug) {
                continue;
            }
            // une demande de page ignore la file de fond : c'est elle qui
            // doit se voir tout de suite
            if present
                || (!tout && self.art_requested.contains(&(game.slug.clone(), kind)))
                || covers::recently_missed(&self.config.marks_dir(kind.dir()), &game.slug)
            {
                continue;
            }
            kinds.push(kind);
        }

        if kinds.is_empty() {
            return None;
        }
        for &kind in &kinds {
            self.art_requested.insert((game.slug.clone(), kind));
        }
        Some(covers::Query::Auto {
            slug: game.slug.clone(),
            name: game.name.clone(),
            kinds,
        })
    }

    /// Demande la miniature d'un jeu (cover présente, miniature absente).
    fn request_thumb(&mut self, slug: &str) {
        let Some(queue) = self.thumbs.clone() else {
            return;
        };
        if !self.thumb_requested.insert(slug.to_string()) {
            return;
        }
        let Some(source) = covers::cached(&self.config.cover_cache_dir(), slug) else {
            self.thumb_requested.remove(slug);
            return;
        };
        queue.now([thumbs::Job::new(slug.to_string(), source)]);
    }

    /// Icône à côté du paquet : demandée en priorité pour les jeux visibles.
    fn request_near_pack(&mut self) {
        let Some(queue) = self.assets.clone() else {
            return;
        };
        let jobs: Vec<assets::Job> = self
            .visible_range()
            .filter_map(|index| self.games.get(index))
            .filter(|game| game.icon.is_none() && !self.icon_scanned.contains(&game.slug))
            .map(|game| assets::Job::new(game.slug.clone(), game.path.clone()))
            .collect();
        queue.now(jobs);
    }

    /// Balayage du reste : les paquets d'un disque externe sont lus petit à
    /// petit, jamais pendant l'affichage de la première image.
    fn sweep_icons(&mut self) {
        let Some(queue) = self.assets.clone() else {
            return;
        };
        let jobs: Vec<assets::Job> = self
            .all_games
            .iter()
            .filter(|game| game.icon.is_none() && !self.icon_scanned.contains(&game.slug))
            .map(|game| assets::Job::new(game.slug.clone(), game.path.clone()))
            .collect();
        queue.sweep(jobs);
    }

    /// Balayage discret de toute la bibliothèque : après quelques dizaines de
    /// secondes, toutes les vignettes sont prêtes, y compris très dézoomées.
    fn sweep_thumbs(&mut self) {
        let Some(queue) = self.thumbs.clone() else {
            return;
        };
        let cover_dir = self.config.cover_cache_dir();
        let jobs: Vec<thumbs::Job> = self
            .all_games
            .iter()
            .filter(|game| game.thumb.is_none())
            .filter_map(|game| {
                let source = covers::cached(&cover_dir, &game.slug)?;
                Some(thumbs::Job::new(game.slug.clone(), source))
            })
            .collect();
        queue.sweep(jobs);
    }

    /// Ouvre la page : les visuels (dont le héros) passent devant la file.
    fn request_page_assets(&mut self, index: usize) {
        if let Some(query) = self.missing_assets(index, true, true)
            && let Some(queue) = self.covers.clone()
        {
            queue.urgent(query);
        }
    }

    /// Exécute l'action choisie sur la page (jouer, étoile, menu des visuels).
    fn page_act(&mut self) -> Task<Message> {
        let Some(page) = self.page.as_ref() else {
            return Task::none();
        };
        let index = page.index;
        let Some(action) = PAGE_ACTIONS.get(page.action).copied() else {
            return Task::none();
        };
        match action {
            PageAction::Play => {
                self.selected = index;
                self.launch()
            }
            PageAction::Favorite => {
                self.toggle_favorite(index);
                Task::none()
            }
            PageAction::Images => {
                self.art_menu = Some(0);
                Task::none()
            }
        }
    }

    /// Étoile de la page : ajoute/retire le jeu des favoris et l'écrit tout de
    /// suite (le fichier est la seule mémoire des favoris).
    fn toggle_favorite(&mut self, index: usize) {
        let Some(game) = self.games.get(index).cloned() else {
            return;
        };
        let added = favorites::toggle(&self.config.favorites_path(), &mut self.favorites, &game);
        self.status = Some((
            if added {
                format!("★ {} ajouté aux favoris", game.name)
            } else {
                format!("{} retiré des favoris", game.name)
            },
            false,
        ));
        if self.config.view == ViewMode::Favorites {
            // le jeu quitte la liste des favoris sous les yeux
            self.apply_filter();
            self.update_scroll_target();
            self.request_visible_covers();
        }
    }

    /// Reconstruit la vue affichée : tous les jeux, ou les seuls favoris.
    /// La sélection est conservée par slug (et non par position).
    /// Retient le type de vue et le tri pour la prochaine session.
    fn remember(&self) {
        state::save(
            &self.config.state_path(),
            state::State {
                view: Some(self.config.view),
                sort: self.sort,
            },
        );
    }

    /// Ordre de la bibliothèque : A→Z (défaut) ou paquets récents d'abord.
    /// Les favoris gardent l'ordre alphabétique (retour 18/11).
    fn sort_games(&mut self) {
        if self.config.view == ViewMode::Favorites {
            self.games.sort_by_cached_key(|game| game.name.to_lowercase());
            return;
        }
        match self.sort {
            SortBy::Name => self.games.sort_by_cached_key(|game| game.name.to_lowercase()),
            SortBy::Modified => self.games.sort_by_key(|game| std::cmp::Reverse(game.mtime)),
        }
    }

    fn apply_filter(&mut self) {
        let current = self.games.get(self.selected).map(|game| game.slug.clone());
        self.games = if self.config.view == ViewMode::Favorites {
            let favorites = &self.favorites;
            self.all_games
                .iter()
                .filter(|game| favorites.contains(&game.slug))
                .cloned()
                .collect()
        } else {
            self.all_games.clone()
        };
        self.sort_games();
        self.selected = current
            .and_then(|slug| self.games.iter().position(|game| game.slug == slug))
            .unwrap_or(0)
            .min(self.games.len().saturating_sub(1));
        self.update_scroll_target();
    }

    /// « covers 12/540 » tant qu'il en manque (téléchargement en cours).
    fn cover_progress(&self) -> Option<String> {
        if !self.config.covers {
            return None;
        }
        let missing = self.games.iter().filter(|game| game.cover.is_none()).count();
        if missing == 0 || missing == self.games.len() {
            return None;
        }
        Some(format!("covers {}/{}", self.games.len() - missing, self.games.len()))
    }

    /// Premier dossier configuré injoignable (disque externe débranché…).
    /// Le dossier reste dans la config : ses jeux reviennent quand il réapparaît.
    fn missing_dir(&self) -> Option<std::path::PathBuf> {
        self.config.game_dirs().into_iter().find(|dir| !dir.is_dir())
    }

    fn on_key(&mut self, event: keyboard::Event) -> Task<Message> {
        // Wayland route déjà le clavier vers la fenêtre active ; la garde
        // couvre les événements en retard et le cas « jeu lancé ».
        if !self.active || self.running.is_some() {
            return Task::none();
        }
        // une touche pressée : les aides repassent en langage clavier, menus
        // compris (le pied de page du menu d'image suit aussi)
        if matches!(event, keyboard::Event::KeyPressed { .. }) {
            self.input = Input::Keyboard;
        }

        // prompt « nom de recherche » : le champ de saisie prend les touches,
        // on n'intercepte que l'annulation (Entrée passe par `on_submit`)
        if self.rename.is_some() {
            return match event {
                keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                } => Task::done(Message::RenameCancel),
                _ => Task::none(),
            };
        }
        if self.picker.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::ClosePicker),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::PickerPick)
                }
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::PickerNav(Nav::Next)),
                keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::PickerNav(Nav::Prev)),
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::PickerNav(Nav::Down)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::PickerNav(Nav::Up)),
                keyboard::Key::Named(Named::PageDown) => Task::done(Message::PickerNav(Nav::PageDown)),
                keyboard::Key::Named(Named::PageUp) => Task::done(Message::PickerNav(Nav::PageUp)),
                _ => Task::none(),
            };
        }
        let keyboard::Event::KeyPressed { key, .. } = event else {
            return Task::none();
        };

        // menu « type de liste » (triangle) : haut/bas puis Entrée
        if let Some(choice) = self.view_menu {
            return match &key {
                keyboard::Key::Named(Named::Escape) => {
                    self.view_menu = None;
                    Task::none()
                }
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::ViewMenuChoose(choice))
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::ViewMenuNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::ViewMenuNav(-1)),
                _ => Task::none(),
            };
        }

        // menu « quel visuel changer » : haut/bas puis Entrée
        if self.art_menu.is_some() {
            return match &key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::ArtMenuClose),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::ArtMenuPick)
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::ArtMenuNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::ArtMenuNav(-1)),
                keyboard::Key::Character(ch) => match ch.as_ref() {
                    "g" => Task::done(Message::ArtMenuChoose(0)),
                    "h" => Task::done(Message::ArtMenuChoose(1)),
                    "l" => Task::done(Message::ArtMenuChoose(2)),
                    "i" => Task::done(Message::ArtMenuChoose(3)),
                    _ => Task::none(),
                },
                _ => Task::none(),
            };
        }

        // page d'un jeu : gauche/droite choisissent l'action, Entrée valide
        if self.page.is_some() {
            return match &key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::ClosePage),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::PageAct)
                }
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::PageNav(1)),
                keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::PageNav(-1)),
                keyboard::Key::Character(ch) => match ch.as_ref() {
                    "j" => Task::done(Message::PageChoose(0)),
                    "f" => Task::done(Message::PageChoose(1)),
                    "i" => Task::done(Message::PageChoose(2)),
                    // raccourcis directs vers un type de visuel
                    "g" => Task::done(Message::OpenPicker(covers::ArtKind::Grid)),
                    "h" => Task::done(Message::OpenPicker(covers::ArtKind::Hero)),
                    "l" => Task::done(Message::OpenPicker(covers::ArtKind::Logo)),
                    "c" => Task::done(Message::OpenPicker(covers::ArtKind::Icon)),
                    _ => Task::none(),
                },
                _ => Task::none(),
            };
        }

        match key {
            keyboard::Key::Named(named) => match named {
                Named::ArrowRight => return self.navigate(Nav::Next),
                Named::ArrowLeft => return self.navigate(Nav::Prev),
                Named::ArrowDown => return self.navigate(Nav::Down),
                Named::ArrowUp => return self.navigate(Nav::Up),
                Named::PageDown => return self.navigate(Nav::PageDown),
                Named::PageUp => return self.navigate(Nav::PageUp),
                Named::Enter | Named::Space => {
                    return Task::done(Message::OpenPage(self.selected));
                }
                Named::Escape => return Task::done(Message::Quit),
                Named::F11 => return Task::done(Message::ToggleFullscreen),
                Named::Tab => return Task::done(Message::ToggleView),
                _ => {}
            },
            keyboard::Key::Character(ch) => match ch.as_ref() {
                "d" | "l" => return self.navigate(Nav::Next),
                "a" | "h" => return self.navigate(Nav::Prev),
                "s" | "j" => return self.navigate(Nav::Down),
                "w" | "k" => return self.navigate(Nav::Up),
                "v" => return Task::done(Message::ToggleView),
                "t" => return Task::done(Message::ToggleSort),
                "c" => return Task::done(Message::OpenPicker(covers::ArtKind::Grid)),
                "+" | "=" | "＋" => return Task::done(Message::Zoom(1)),
                "-" | "_" | "−" | "–" | "‐" => return Task::done(Message::Zoom(-1)),
                "f" => return Task::done(Message::ToggleFullscreen),
                "q" => return Task::done(Message::Quit),
                _ => {}
            },
            _ => {}
        }

        Task::none()
    }

    fn on_pad(&mut self, event: PadEvent) -> Task<Message> {
        match event {
            PadEvent::Connected { kind, name } => {
                self.pads.retain(|(_, known)| known != &name);
                self.pads.push((kind, name));
                if let Some((kind, _)) = self.pads.first() {
                    self.pad = Some(*kind);
                    self.glyphs = Glyphs::for_kind(*kind);
                }
                // une manette qui arrive prend la main : les glyphes suivent
                self.input = Input::Pad;
                Task::none()
            }
            PadEvent::Disconnected { name } => {
                self.pads.retain(|(_, known)| known != &name);
                match self.pads.first() {
                    Some((kind, _)) => {
                        self.pad = Some(*kind);
                        self.glyphs = Glyphs::for_kind(*kind);
                    }
                    None => self.pad = None,
                }
                Task::none()
            }
            PadEvent::Button(button) => {
                // manette lue sur /dev/input : hors focus, elle ne doit rien
                // faire (sinon un appui pendant qu'on joue pilote le launcher)
                if !self.active || self.running.is_some() {
                    return Task::none();
                }
                // un appui réel bascule les aides en langage manette
                self.input = Input::Pad;
                if !matches!(button, PadButton::Back) {
                    // une autre action annule le « appuie encore »
                    self.back_press = None;
                    self.quit_hint = false;
                }
                if self.picker.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::PickerPick),
                        PadButton::Back => Task::done(Message::ClosePicker),
                        PadButton::Left => Task::done(Message::PickerNav(Nav::Prev)),
                        PadButton::Right => Task::done(Message::PickerNav(Nav::Next)),
                        PadButton::Up => Task::done(Message::PickerNav(Nav::Up)),
                        PadButton::Down => Task::done(Message::PickerNav(Nav::Down)),
                        PadButton::PageUp => Task::done(Message::PickerNav(Nav::PageUp)),
                        PadButton::PageDown => Task::done(Message::PickerNav(Nav::PageDown)),
                        _ => Task::none(),
                    };
                }
                // menu « quel visuel changer » de la page
                if let Some(choice) = self.view_menu {
                    return match button {
                        PadButton::Confirm => Task::done(Message::ViewMenuChoose(choice)),
                        PadButton::Back => {
                            self.view_menu = None;
                            Task::none()
                        }
                        PadButton::Up => Task::done(Message::ViewMenuNav(-1)),
                        PadButton::Down => Task::done(Message::ViewMenuNav(1)),
                        _ => Task::none(),
                    };
                }
                if self.art_menu.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::ArtMenuPick),
                        PadButton::Back => Task::done(Message::ArtMenuClose),
                        PadButton::Down => Task::done(Message::ArtMenuNav(1)),
                        PadButton::Up => Task::done(Message::ArtMenuNav(-1)),
                        _ => Task::none(),
                    };
                }
                // page d'un jeu : croix gauche/droite entre les actions
                if self.page.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::PageAct),
                        PadButton::Back => Task::done(Message::ClosePage),
                        PadButton::Left => Task::done(Message::PageNav(-1)),
                        PadButton::Right => Task::done(Message::PageNav(1)),
                        // Y : les quatre visuels
                        PadButton::View => Task::done(Message::ArtMenu),
                        _ => Task::none(),
                    };
                }
                match button {
                    PadButton::Up => self.navigate(Nav::Up),
                    PadButton::Down => self.navigate(Nav::Down),
                    PadButton::Left => self.navigate(Nav::Prev),
                    PadButton::Right => self.navigate(Nav::Next),
                    PadButton::PageDown => self.navigate(Nav::PageDown),
                    PadButton::PageUp => self.navigate(Nav::PageUp),
                    PadButton::ZoomIn => Task::done(Message::Zoom(1)),
                    PadButton::ZoomOut => Task::done(Message::Zoom(-1)),
                    // triangle / Y : le MENU des types de liste (le cycle
                    // rapide reste au clavier, V/Tab)
                    PadButton::View => Task::done(Message::ViewMenu),
                    // carré / X : A→Z ou récents d'abord
                    PadButton::Menu => Task::done(Message::ToggleSort),
                    PadButton::Confirm => Task::done(Message::OpenPage(self.selected)),
                    // « retour » ne quitte pas du premier coup : deux appuis
                    // rapprochés (1,5 s) sont nécessaires, pour ne pas
                    // s'éjecter sur une pression malheureuse en jouant.
                    PadButton::Back => {
                        if self
                            .back_press
                            .is_some_and(|at| at.elapsed() < QUIT_WINDOW)
                        {
                            Task::done(Message::Quit)
                        } else {
                            self.back_press = Some(std::time::Instant::now());
                            self.quit_hint = true;
                            Task::none()
                        }
                    }
                }
            }
        }
    }

    // --- vue ----------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        static FIRST: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        if FIRST.set(()).is_ok() {
            crate::stamp("premier rendu");
        }
        // La liste est TOUJOURS construite, la page d'un jeu venant par-dessus :
        // remplacer l'arbre recréait la zone défilante, donc le retour depuis
        // une page ramenait la grille tout en haut (constaté 18/11), et le
        // moindre déplacement la re-situait sur le jeu sélectionné.
        // Le nombre de couches est CONSTANT (liste, page, voile, menus) : iced
        // apparie les états des enfants d'un `stack` par position, donc ajouter
        // une couche au milieu faisait perdre à la grille son état de
        // défilement — le retour depuis une page ramenait tout en haut
        // (mesuré 18/11 : offset 0 au moment même de l'ouverture de la page).
        let mut view: Element<'_, Message> = self.launcher_view();

        let page_layer: Element<'_, Message> = if self.page.is_some() {
            // la page doit intercepter clic et molette, sinon ils atteignent
            // les vignettes de la grille restée dessous
            mouse_area(self.page_view())
                .on_press(Message::Nothing)
                .on_scroll(|_| Message::Nothing)
                .into()
        } else {
            container(Row::new()).width(Fill).height(Fill).into()
        };
        view = stack![view, page_layer].into();

        // Le voile noir du fondu existe MÊME à opacité nulle, et il passe APRÈS
        // la page : lancer un jeu depuis une page doit virer au noir comme
        // depuis la grille, et l'arbre doit garder la même forme d'un rendu à
        // l'autre (un changement de forme recrée la zone défilante, donc la
        // position).
        let veil = container(Row::new())
            .width(Fill)
            .height(Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(Color {
                    a: self.fade,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            });
        view = stack![view, veil].into();

        // menus : une seule couche, construite à part (aucun état à préserver).
        // La couche de BASE doit remplir la fenêtre : `Stack` prend sa taille
        // sur son premier enfant, une couche vide réduisait les menus à zéro
        // (constaté 18/11 : menus invisibles).
        let mut menus: Element<'_, Message> = container(Row::new())
            .width(Fill)
            .height(Fill)
            .into();
        if self.view_menu.is_some() {
            menus = stack![menus, self.view_menu_view()].into();
        }
        if self.art_menu.is_some() {
            menus = stack![menus, self.art_menu_view()].into();
        }
        if self.picker.is_some() {
            menus = stack![menus, self.picker_view()].into();
        }
        if self.rename.is_some() {
            menus = stack![menus, self.rename_view()].into();
        }
        view = stack![view, menus].into();

        // clic droit = retour, partout : les couches du dessus n'ont pas de
        // gestion du bouton droit, l'évènement remonte donc jusqu'ici
        mouse_area(view).on_right_press(Message::Back).into()
    }

    /// Liste des jeux (grille, liste ou mosaïque) avec en-tête et pied.
    fn launcher_view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = if self.games.is_empty() {
            if self.config.view == ViewMode::Favorites {
                self.favorites_empty()
            } else {
                self.empty_state()
            }
        } else {
            match self.config.view {
                ViewMode::Grid | ViewMode::Favorites | ViewMode::Mosaic => scrollable(self.grid())
                    .id(self.scroll_id.clone())
                    .height(Fill)
                    .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                    .into(),
                ViewMode::List => row![
                    scrollable(self.list())
                        .id(self.scroll_id.clone())
                        .height(Fill)
                        .width(Length::FillPortion(3))
                        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y)),
                    container(self.preview())
                        .width(Length::FillPortion(2))
                        .height(Fill),
                ]
                .spacing(22)
                .into(),
            }
        };
        let footer = self.footer();

        container(column![content, footer].spacing(0))
            .width(Fill)
            .height(Fill)
            .padding([18.0, 26.0])
            .style(|_| container::Style {
                background: Some(Background::Color(BG)),
                ..container::Style::default()
            })
            .into()
    }

    /// Page d'un jeu : bannière héros en haut, logo, grille et actions.
    fn page_view(&self) -> Element<'_, Message> {
        let Some(page) = self.page.as_ref() else {
            return Row::new().into();
        };
        let Some(game) = self.games.get(page.index) else {
            return Row::new().into();
        };

        // bannière : les héros SteamGridDB font 1920×620. Largeur EXPLICITE
        // (un `Fill` laissait l'image à sa taille intrinsèque, d'où une bande
        // noire à droite) et recadrage plutôt qu'étirement.
        let width = self.viewport.0.max(320.0);
        // Hauteur de la bande : 1920×620 recadré, sans dépasser 45 % de l'écran.
        let band_height = |width: f32| {
            (width * 620.0 / 1920.0)
                .min(self.viewport.1 * 0.45)
                .max(160.0)
        };
        // Moitié basse (cover, détails, pastilles) : elle prend la hauteur
        // RESTANTE sous la bande héros, au lieu de dimensions figées — la
        // demander plus grande ne servait à rien tant qu'elle restait calée sur
        // des pixels constants, trop petits sur un écran 1080p (retour 19/11).
        let band_h = if game.hero.is_some() {
            band_height(width)
        } else {
            0.0
        };
        let dispo = (self.viewport.1 - band_h).max(300.0);
        let cover_h = (dispo * 0.72).clamp(300.0, 520.0);
        let cover_w = cover_h * 205.0 / 308.0;
        // tout le bloc suit la cover : textes, pastilles et espacements
        let ech = (cover_h / 308.0).clamp(1.0, 1.7);
        // Pas de héros : aucune bande n'est réservée, et le bloc du bas se
        // centre sur toute la page — une bande grise vide en haut avec le
        // contenu collé en dessous faisait pauvre (retour 18/11).
        let band: Option<Element<'_, Message>> = game.hero.as_ref().map(|handle| {
            image(handle.clone())
                .width(Length::Fixed(width))
                .height(Length::Fixed(band_h))
                .content_fit(ContentFit::Cover)
                .into()
        });

        let logo: Element<'_, Message> = match &game.logo {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(450.0 * ech))
                .height(Length::Fixed(155.0 * ech))
                .content_fit(ContentFit::Contain)
                .into(),
            None => text(short(&game.name, 46))
                .size(36.0 * ech)
                .color(TEXT)
                .into(),
        };

        let cover: Element<'_, Message> = match &game.cover {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(cover_w))
                .height(Length::Fixed(cover_h))
                .content_fit(ContentFit::Contain)
                .into(),
            None => sample_art(&self.sample, &game.name, game.icon.as_ref(), cover_w, cover_h),
        };

        let details = column![
            logo,
            text(short(&game.name, 60)).size(25.0 * ech).color(TEXT),
            text(game.path.display().to_string())
                .size(14.0 * ech)
                .color(MUTED),
            text(format!(
                "Temps de jeu : {}   ·   {}",
                playtime::format(self.playtime.get(&game.slug).copied()),
                self.art_status(page.index)
            ))
            .size(14.0 * ech)
            .color(MUTED),
        ]
        .spacing(10.0 * ech)
        .align_x(Alignment::Center);

        let favorite = self.favorites.contains(&game.slug);
        let mut actions = Row::new().spacing(14.0 * ech).align_y(Alignment::Center);
        for (index, action) in PAGE_ACTIONS.iter().enumerate() {
            let selected = index == page.action;
            // l'étoile montre l'état : pleine = favori, atténuée = à marquer
            let inside: Element<'_, Message> = match action {
                PageAction::Favorite => row![
                    self.star_icon(18.0 * ech, if favorite { 1.0 } else { 0.35 }),
                    text("Favori").size(18.0 * ech).color(if selected {
                        Color::BLACK
                    } else {
                        TEXT
                    }),
                ]
                .spacing(7.0 * ech)
                .align_y(Alignment::Center)
                .into(),
                PageAction::Images => text("Images ▸")
                    .size(18.0 * ech)
                    .color(if selected { Color::BLACK } else { TEXT })
                    .into(),
                other => text(other.label())
                    .size(18.0 * ech)
                    .color(if selected { Color::BLACK } else { TEXT })
                    .into(),
            };
            let pill = container(inside)
                .padding([13.0 * ech, 24.0 * ech])
                .style(move |_| action_style(selected));

            actions = actions.push(mouse_area(pill).on_press(Message::PageChoose(index)));
        }

        // aides de la page : les MÊMES glyphes que le pied de page (la croix
        // n'a pas de glyphe dans le jeu d'images embarqué, elle reste en
        // texte)
        let help: Element<'_, Message> = if self.pad_hints() {
            row![
                text("← → action").size(14.0 * ech).color(MUTED),
                hint(self.glyphs.confirm.clone(), "Valider"),
                hint(self.glyphs.back.clone(), "Retour"),
                hint(self.glyphs.view.clone(), "Visuels"),
            ]
            .spacing(18.0 * ech)
            .align_y(Alignment::Center)
            .into()
        } else {
            text("← → action   ·   Entrée valider   ·   Échap retour   ·   I visuels   ·   F favori")
                .size(14.0 * ech)
                .color(MUTED)
                .into()
        };

        let body = container(
            column![
                row![cover, column![details, actions]
                    .spacing(20.0 * ech)
                    .align_x(Alignment::Center)]
                .spacing(30.0 * ech)
                .align_y(Alignment::Center),
                help,
            ]
            .spacing(30.0 * ech)
            .align_x(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .padding(20.0 * ech)
        .center_x(Fill)
        .center_y(Fill);

        let mut page_body = Column::new().spacing(0);
        if let Some(band) = band {
            page_body = page_body.push(band);
        }
        page_body = page_body.push(body);

        container(page_body)
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(BG)),
            ..container::Style::default()
        })
        .into()
    }

    /// « Grid ✓ · Héros … » : ce qui est déjà là pour ce jeu.
    fn art_status(&self, index: usize) -> String {
        let Some(game) = self.games.get(index) else {
            return String::new();
        };
        let mark = |present: bool| if present { "✓" } else { "…" };
        format!(
            "Grid {}   ·   Héros {}   ·   Logo {}   ·   Icône {}",
            mark(game.cover.is_some()),
            mark(game.hero.is_some()),
            mark(game.logo.is_some()),
            mark(game.icon.is_some())
        )
    }

    /// Faut-il afficher les glyphes de manette ? Seulement si une manette est
    /// branchée ET que c'est elle qui a servi en dernier : reprendre le clavier
    /// fait réapparaître les touches dans le pied de page et les menus.
    fn pad_hints(&self) -> bool {
        self.pad.is_some() && self.input == Input::Pad
    }

    /// Image à afficher pour une vignette : la miniature (décodage 4× plus
    /// rapide) tant que la largeur demandée tient dans ses 320 px, la cover
    /// pleine au-delà (aucune image n'est agrandie).
    fn cover_handle(&self, game: &Game, width: f32) -> Option<image::Handle> {
        if width <= thumbs::MAX_WIDTH as f32 {
            game.thumb.clone().or_else(|| game.cover.clone())
        } else {
            game.cover.clone()
        }
    }

    /// Étoile des favoris, à la taille voulue (`opacity` atténue l'état
    /// « pas encore favori »).
    fn star_icon(&self, size: f32, opacity: f32) -> Element<'_, Message> {
        image(self.star.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .content_fit(ContentFit::Contain)
            .opacity(opacity)
            .into()
    }

    /// Prompt « nom de recherche » : champ pré-rempli avec le nom envoyé à
    /// SteamGridDB, Entrée relance la recherche (Échap annule).
    fn rename_view(&self) -> Element<'_, Message> {
        let Some(value) = self.rename.as_ref() else {
            return Row::new().into();
        };

        let panel = container(
            column![
                text("Nom de recherche").size(18).color(TEXT),
                text(
                    "SteamGridDB ne trouve rien ? Corrige le nom du jeu : \
                     souvent une édition, une année ou une plateforme en trop."
                )
                .size(14)
                .color(MUTED),
                text_input("Nom du jeu sur SteamGridDB", value)
                    .id(self.rename_id.clone())
                    .on_input(Message::RenameInput)
                    .on_submit(Message::RenameSubmit)
                    .padding(10)
                    .size(18)
                    .style(rename_input_style),
                text("Entrée : chercher   ·   Échap : annuler")
                    .size(14)
                    .color(MUTED),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(540.0))
        .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.72,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    /// Menu « quel visuel changer » : les quatre types réunis derrière une
    /// seule pastille « Images » de la page.
    /// Menu « type de liste » (triangle / Y) : les quatre vues, la courante ✓.
    fn view_menu_view(&self) -> Element<'_, Message> {
        let Some(choice) = self.view_menu else {
            return Row::new().into();
        };

        let mut rows = Column::new().spacing(6);
        for (index, view) in VIEWS.iter().enumerate() {
            let selected = index == choice;
            let current = *view == self.config.view;
            let line = container(
                row![
                    text(view_label(*view))
                        .size(18)
                        .width(Fill)
                        .color(if selected { Color::BLACK } else { TEXT }),
                    text(if current { "✓" } else { "" })
                        .size(14)
                        .color(if selected { Color::BLACK } else { MUTED }),
                ]
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([9.0, 14.0])
            .style(move |_| action_style(selected));

            rows = rows.push(mouse_area(line).on_press(Message::ViewMenuChoose(index)));
        }

        let panel = container(
            column![text("Type de liste").size(18).color(TEXT), rows]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(300.0))
        .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.6,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    fn art_menu_view(&self) -> Element<'_, Message> {
        let Some(choice) = self.art_menu else {
            return Row::new().into();
        };
        let Some(page) = self.page.as_ref() else {
            return Row::new().into();
        };
        let Some(game) = self.games.get(page.index) else {
            return Row::new().into();
        };
        let marks = [
            game.cover.is_some(),
            game.hero.is_some(),
            game.logo.is_some(),
            game.icon.is_some(),
        ];

        let mut rows = Column::new().spacing(6);
        for (index, kind) in ART_MENU.iter().enumerate() {
            let selected = index == choice;
            let line = container(
                row![
                    text(kind.label())
                        .size(18)
                        .width(Fill)
                        .color(if selected { Color::BLACK } else { TEXT }),
                    text(if marks[index] { "✓" } else { "…" })
                        .size(14)
                        .color(if selected { Color::BLACK } else { MUTED }),
                ]
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([9.0, 14.0])
            .style(move |_| action_style(selected));

            rows = rows.push(mouse_area(line).on_press(Message::ArtMenuChoose(index)));
        }

        let panel = container(
            column![
                text(format!("Visuels de « {} »", short(&game.name, 38)))
                    .size(18)
                    .color(TEXT),
                rows,
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(360.0))
        .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.6,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    /// Menu « choisir une image » : vignettes SteamGridDB du jeu sélectionné.
    fn picker_view(&self) -> Element<'_, Message> {
        let Some(picker) = &self.picker else {
            return Row::new().into();
        };

        let body: Element<'_, Message> = if picker.loading {
            text("recherche d'images sur SteamGridDB…")
                .size(14)
                .color(MUTED)
                .into()
        } else if let Some(error) = &picker.error {
            text(error.clone()).size(14).color(DANGER).into()
        } else if picker.choices.is_empty() {
            text("aucune image proposée pour ce jeu — corrige le nom ci-dessous si besoin")
                .size(14)
                .color(MUTED)
                .into()
        } else {
            let mut rows = Column::new().spacing(12);
            let mut current = Row::new().spacing(12);
            let mut in_row = 0;

            // proportions du type : grille portrait, héros/logo larges,
            // icône carrée
            let (tw, th) = match picker.kind {
                covers::ArtKind::Grid => (148.0, 222.0),
                covers::ArtKind::Hero | covers::ArtKind::Logo => (208.0, 117.0),
                covers::ArtKind::Icon => (120.0, 120.0),
            };

            for index in 0..picker.choices.len() {
                let selected = index == picker.index;
                let thumb: Element<'_, Message> = match picker.thumbs.get(index) {
                    Some(handle) => image(handle.clone())
                        .width(Length::Fixed(tw))
                        .height(Length::Fixed(th))
                        .content_fit(ContentFit::Contain)
                        .into(),
                    // vignette pas encore prête : case vide à la bonne taille
                    None => container(Row::new())
                        .width(Length::Fixed(tw))
                        .height(Length::Fixed(th))
                        .into(),
                };
                let cell = container(thumb)
                    .padding(6)
                    .style(move |_| card_style(selected));

                current = current.push(mouse_area(cell).on_press(Message::PickerSelect(index)));
                in_row += 1;
                if in_row == PICKER_COLUMNS {
                    rows = rows.push(std::mem::replace(&mut current, Row::new().spacing(12)));
                    in_row = 0;
                }
            }
            if in_row > 0 {
                rows = rows.push(current);
            }

            scrollable(rows).height(Fill).into()
        };

        // aide adaptée : glyphes à la manette (« retour » y annule), clavier
        // sinon
        let help: Element<'_, Message> = if self.pad_hints() {
            row![
                hint(self.glyphs.confirm.clone(), "Utiliser cette image"),
                hint(self.glyphs.back.clone(), "Annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        } else {
            text("Entrée : utiliser cette image   ·   Échap : annuler")
                .size(14)
                .color(MUTED)
                .into()
        };

        // le jeu peut être mal nommé côté SteamGridDB : le nom envoyé se corrige
        let rename = row![
            text(format!("Recherche : « {} »", picker.query))
                .size(14)
                .color(MUTED),
            mouse_area(
                container(text("Changer le nom").size(14).color(TEXT))
                    .padding([5.0, 12.0])
                    .style(|_| action_style(false)),
            )
            .on_press(Message::RenameOpen),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let panel = container(
            column![
                text(format!("{} de « {} »", picker.kind.label(), picker.name))
                    .size(18)
                    .color(TEXT),
                body,
                rename,
                help,
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(860.0))
        .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.78,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }


    fn grid(&self) -> Column<'_, Message> {
        let columns = self.metrics().2;
        let gap = self.gap();
        let mut rows = Column::new().spacing(gap);
        let mut current = Row::new().spacing(gap);
        let mut in_row = 0;

        for (index, game) in self.games.iter().enumerate() {
            current = current.push(match self.config.view {
                ViewMode::Mosaic => self.mosaic_card(game, index),
                _ => self.card(game, index),
            });
            in_row += 1;
            if in_row == columns {
                rows = rows.push(std::mem::replace(&mut current, Row::new().spacing(gap)));
                in_row = 0;
            }
        }
        if in_row > 0 {
            rows = rows.push(current);
        }

        rows
    }

    /// Largeur de vignette, largeur d'image et nombre de colonnes, calculés sur
    /// la largeur RÉELLE de la fenêtre : sans ça, agrandir la fenêtre laissait
    /// une bande noire à droite de la grille.
    ///
    /// `cell_size` est la largeur minimale d'une vignette, `columns` le nombre
    /// maximal de colonnes ; les vignettes s'étalent pour remplir la largeur.
    fn metrics(&self) -> (f32, f32, usize) {
        // en mosaïque, `columns` ne s'applique pas : c'est la taille des
        // vignettes (zoom) qui règle la densité, et la grille remplit l'écran
        let max_columns = match self.config.view {
            ViewMode::Mosaic => usize::MAX / 2,
            _ => self.config.columns,
        };
        let min_cell = self.config.cell_size as f32
            * match self.config.view {
                ViewMode::Mosaic => MOSAIC_SCALE,
                _ => 1.0,
            };
        let (cell, _, columns) = grid_metrics(
            self.viewport.0,
            min_cell,
            max_columns,
            self.gap(),
        );
        // en mosaïque la cover occupe TOUTE la vignette (aucune marge interne)
        let art = match self.config.view {
            ViewMode::Mosaic => cell,
            _ => cell - 2.0 * PADDING,
        };
        (cell, art, columns)
    }

    /// Espacement entre vignettes : nul en mosaïque, les covers se touchent.
    fn gap(&self) -> f32 {
        match self.config.view {
            ViewMode::Mosaic => 0.0,
            _ => GAP,
        }
    }

    /// Nom de la disposition suivante, pour le pied de page.
    fn next_view_label(&self) -> &'static str {
        match self.config.view.flipped() {
            ViewMode::Grid => "Grille",
            ViewMode::List => "Liste",
            ViewMode::Mosaic => "Mosaïque",
            ViewMode::Favorites => "Favoris",
        }
    }

    /// Vignette des lignes de liste, proportionnelle au zoom de la grille.
    fn list_thumb(&self) -> f32 {
        (self.config.cell_size as f32 / 5.0).clamp(28.0, 96.0)
    }

    /// Hauteur d'une ligne de liste (vignette + marges).
    /// Hauteur d'une ligne de liste, **sans** l'espacement entre lignes : la
    /// ligne est une vignette carrée plus 8 px de marge haut et bas. Les
    /// appelants ajoutent l'espacement (`spacing(8)`) pour obtenir le pas.
    /// Compter deux fois l'espacement décalait le défilement de 8 px par ligne :
    /// après quelques dizaines de pages, la sélection sortait de l'écran
    /// (constaté 18/11, « je ne vois plus le focus »).
    fn list_row_height(&self) -> f32 {
        self.list_thumb() + 16.0
    }

    /// Hauteur d'une vignette de grille (image portrait + titre + marges).
    fn row_height(&self) -> f32 {
        let (_, art_width, _) = self.metrics();
        match self.config.view {
            ViewMode::Mosaic => art_width * ART_RATIO,
            _ => art_width * ART_RATIO + 2.0 * PADDING + CARD_CAPTION,
        }
    }

    fn card<'a>(&'a self, game: &'a Game, index: usize) -> Element<'a, Message> {
        let selected = index == self.selected;
        let (cell, art_width, _) = self.metrics();
        let art_height = art_width * ART_RATIO;

        let art: Element<'_, Message> = match self.cover_handle(game, art_width) {
            Some(handle) => image(handle)
                .width(Length::Fixed(art_width))
                .height(Length::Fixed(art_height))
                .content_fit(ContentFit::Contain)
                .into(),
            None => sample_art(
                &self.sample,
                &game.name,
                game.icon.as_ref(),
                art_width,
                art_height,
            ),
        };
        let art: Element<'_, Message> = if self.favorites.contains(&game.slug) {
            // étoile discrète en haut à droite de la cover
            let badge = container(self.star_icon(19.0, 1.0))
                .width(Fill)
                .height(Fill)
                .padding(6)
                .align_x(Alignment::End)
                .align_y(Alignment::Start);
            stack![art, badge].into()
        } else {
            art
        };

        let body = column![
            art,
            text(short(&game.name, (cell / 11.0) as usize))
                .size(if cell >= 220.0 { 17.0 } else { 15.5 })
                .color(if selected { TEXT } else { MUTED }),
        ]
        .spacing(6)
        .align_x(Alignment::Center);

        let cell = container(body)
            .width(Length::Fixed(cell))
            .padding(PADDING)
            .style(move |_| card_style(selected));

        mouse_area(cell).on_press(Message::CardClicked(index)).into()
    }

    /// Vignette de mosaïque : cover pleine, collée aux voisines, sans nom.
    /// Seule la sélection se distingue, par un liseré d'accent.
    fn mosaic_card<'a>(&'a self, game: &'a Game, index: usize) -> Element<'a, Message> {
        let selected = index == self.selected;
        let (cell, art_width, _) = self.metrics();
        let art_height = art_width * ART_RATIO;

        let art: Element<'_, Message> = match self.cover_handle(game, art_width) {
            Some(handle) => image(handle)
                .width(Length::Fixed(art_width))
                .height(Length::Fixed(art_height))
                .content_fit(ContentFit::Cover)
                .into(),
            None => sample_art(
                &self.sample,
                &game.name,
                game.icon.as_ref(),
                art_width,
                art_height,
            ),
        };
        let art: Element<'_, Message> = if self.favorites.contains(&game.slug) {
            let badge = container(self.star_icon(18.0, 1.0))
                .width(Fill)
                .height(Fill)
                .padding(5)
                .align_x(Alignment::End)
                .align_y(Alignment::Start);
            stack![art, badge].into()
        } else {
            art
        };

        // La sélection doit se voir sur n'importe quelle cover : un liseré de
        // 4 px se perdait dans les images colorées, et un cadre dessiné par le
        // conteneur passait DERRIÈRE la cover. Voile d'accent + cadre posés
        // au-dessus de l'image, en taille explicite (le `Fill` dans un `stack`
        // se réduisait à zéro).
        let tile: Element<'_, Message> = if selected {
            let veil = container(Row::new())
                .width(Length::Fixed(cell))
                .height(Length::Fixed(cell * ART_RATIO))
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.22, ..ACCENT })),
                    ..container::Style::default()
                });
            let frame = container(Row::new())
                .width(Length::Fixed(cell))
                .height(Length::Fixed(cell * ART_RATIO))
                .style(|_| container::Style {
                    border: Border {
                        color: ACCENT,
                        width: SELECT_BORDER,
                        radius: 0.0.into(),
                    },
                    ..container::Style::default()
                });
            stack![art, veil, frame].into()
        } else {
            art
        };

        let tile = container(tile)
            .width(Length::Fixed(cell))
            .height(Length::Fixed(cell * ART_RATIO));
        mouse_area(tile).on_press(Message::CardClicked(index)).into()
    }

    /// Disposition en lignes : vignette à gauche, nom à droite.
    fn list(&self) -> Column<'_, Message> {
        let mut rows = Column::new().spacing(8);

        for (index, game) in self.games.iter().enumerate() {
            let selected = index == self.selected;
            let thumb = self.list_thumb();

            // en liste, c'est l'ICÔNE DU PAQUET qui identifie le jeu ;
            // la cover ne sert que de repli, puis le sample
            let art: Element<'_, Message> = match game
                .icon
                .clone()
                .or_else(|| self.cover_handle(game, thumb))
            {
                Some(handle) => image(handle)
                    .width(Length::Fixed(thumb))
                    .height(Length::Fixed(thumb))
                    .content_fit(ContentFit::Contain)
                    .into(),
                None => sample_art(
                    &self.sample,
                    &game.name,
                    game.icon.as_ref(),
                    thumb,
                    thumb,
                ),
            };

            let star: Element<'_, Message> = if self.favorites.contains(&game.slug) {
                self.star_icon(17.0, 1.0)
            } else {
                Row::new().into()
            };

            let body = row![
                art,
                text(short(&game.name, 64))
                    .size(18)
                    .color(if selected { TEXT } else { MUTED }),
                star,
            ]
            .spacing(14)
            .align_y(Alignment::Center)
            .padding(Padding {
                top: 8.0,
                right: 16.0,
                bottom: 8.0,
                left: 10.0,
            });

            let line = container(body)
                .width(Fill)
                .style(move |_| list_style(selected));

            rows = rows.push(mouse_area(line).on_press(Message::CardClicked(index)));
        }

        rows
    }

    /// Grand aperçu (cover) du jeu sélectionné, à droite de la liste.
    fn preview(&self) -> Element<'_, Message> {
        let Some(game) = self.games.get(self.selected) else {
            return Row::new().into();
        };

        let art = game.cover.clone().or_else(|| game.icon.clone());
        let picture: Element<'_, Message> = match art {
            Some(handle) => image(handle)
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Contain)
                .into(),
            None => sample_cover(&self.sample, &game.name, game.icon.as_ref()),
        };

        let caption = column![
            text(short(&game.name, 42)).size(22).color(TEXT),
            text(game.path.display().to_string())
                .size(13)
                .color(MUTED),
        ]
        .spacing(4)
        .align_x(Alignment::Center);

        container(column![picture, caption].spacing(14).align_x(Alignment::Center))
            .width(Fill)
            .height(Fill)
            .padding(18)
            .style(|_| container::Style {
                background: Some(Background::Color(CELL_BG)),
                border: Border {
                    color: CELL_BORDER,
                    width: 1.0,
                    radius: 18.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    }

    /// Vue « favoris » sans aucun favori : on explique comment en poser un.
    fn favorites_empty(&self) -> Element<'_, Message> {
        container(
            column![
                text("Aucun favori").size(22).color(TEXT),
                text("Ouvre un jeu (Entrée ou A), puis appuie sur ☆ Favori.")
                    .size(14)
                    .color(MUTED),
                text("V / Y font défiler grille → liste → mosaïque → favoris.")
                    .size(14)
                    .color(MUTED),
            ]
            .spacing(8),
        )
        .width(Fill)
        .height(Fill)
        .padding(24)
        .into()
    }

    fn empty_state(&self) -> Element<'_, Message> {
        let mut dirs = column![].spacing(4);
        for dir in self.config.game_dirs() {
            let exists = dir.is_dir();
            dirs = dirs.push(
                text(format!(
                    "{} {}",
                    if exists { "•" } else { "✗" },
                    dir.display()
                ))
                .size(14)
                .color(if exists { MUTED } else { DANGER }),
            );
        }

        container(
            column![
                text("Aucun jeu trouvé").size(22).color(TEXT),
                text("Dossiers scannés (.wgp / .lgp) :").size(14).color(MUTED),
                dirs,
                text("Les dossiers sont surveillés : un paquet ajouté ou retiré apparaît seul. Changer de dossier : --dir ou config.toml.")
                    .size(14)
                    .color(MUTED),
            ]
            .spacing(8),
        )
        .width(Fill)
        .height(Fill)
        .padding(24)
        .into()
    }

    fn footer(&self) -> Element<'_, Message> {
        let hints: Element<'_, Message> = match self.pad_hints() {
            true => row![
                hint(self.glyphs.confirm.clone(), "Ouvrir"),
                hint(self.glyphs.back.clone(), "Quitter (2×)"),
                hint(self.glyphs.view.clone(), self.next_view_label()),
                hint(self.glyphs.zoom_out.clone(), "Taille −"),
                hint(self.glyphs.zoom_in.clone(), "Taille +"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into(),
            false => text("←↑→↓ naviguer   ·   Entrée ouvrir   ·   V vue   ·   C image   ·   +/− taille   ·   T tri   ·   F plein écran   ·   Échap quitter")
                .size(14)
                .color(MUTED)
                .into(),
        };

        let status: Element<'_, Message> = if self.quit_hint {
            text("appuie encore sur retour pour quitter")
                .size(14)
                .color(ACCENT)
                .into()
        } else if let Some((message, error)) = &self.status {
            text(message.clone())
                .size(14)
                .color(if *error { DANGER } else { MUTED })
                .into()
        } else if let Some(dir) = self.missing_dir() {
            text(format!(
                "dossier introuvable : {} (disque débranché ?)",
                dir.display()
            ))
            .size(14)
            .color(MUTED)
            .into()
        } else if let Some(notice) = &self.notice {
            text(notice.clone()).size(14).color(ACCENT).into()
        } else if let Some(progress) = self.cover_progress() {
            text(progress).size(14).color(MUTED).into()
        } else {
            Row::new().into()
        };

        // noms masqués en mosaïque : celui du jeu sélectionné vit dans le pied
        // de page, et il doit rester visible même quand un message (mode test,
        // couvertures en cours…) occupe la place à droite
        let selection: Element<'_, Message> = if self.config.view == ViewMode::Mosaic {
            self.games
                .get(self.selected)
                .map(|game| text(game.name.clone()).size(14).color(TEXT).into())
                .unwrap_or_else(|| Row::new().into())
        } else {
            Row::new().into()
        };

        // le titre et le compte de jeux vivent ici, avec les commandes : la
        // vue y gagne toute la hauteur de l'en-tête
        // compte + ordre courant : le tri doit se voir sans ouvrir de menu
        let count_label = match self.config.view {
            ViewMode::Favorites => format!(
                "{} favori{}",
                self.games.len(),
                if self.games.len() > 1 { "s" } else { "" }
            ),
            _ => format!(
                "{} · {}",
                plural(self.games.len()),
                self.sort.label()
            ),
        };
        let count: Element<'_, Message> = text(count_label).size(14).color(MUTED).into();

        container(
            row![hints, count, Row::new().width(Fill), selection, status]
                .spacing(16)
                .align_y(Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 4.0, bottom: 0.0, left: 4.0 })
        .into()
    }
}

// --- petits helpers ---------------------------------------------------------

/// Défilement après une image d'animation : glissement doux pour un
/// déplacement d'une rangée (flèches), **saut direct** quand le but est loin
/// (L1/R1, saut de page). Sans le saut, l'animation courait après chaque appui
/// et la sélection sortait de l'écran pendant l'appui répété (constaté 18/11,
/// « si je vais vite avec R1/L1 je ne vois plus le focus »).
fn ease_or_snap(scroll: f32, target: f32, span: f32, dt: f32) -> f32 {
    if (target - scroll).abs() > 2.0 * span.max(1.0) {
        return target;
    }
    let ease = 1.0 - (-dt / SCROLL_TAU).exp();
    scroll + (target - scroll) * ease
}

/// Largeur minimale de vignette qui produit **exactement** `columns` colonnes
/// dans une grille étalée sur toute la largeur : c'est l'inverse de
/// `grid_metrics`, nécessaire au zoom arrière (`cell_size` ne peut pas être
/// laissé sur la largeur affichée, sinon le nombre de colonnes plafonne).
fn cell_for_columns(window_width: f32, columns: usize, gap: f32) -> u32 {
    let columns = columns.max(1) as f32;
    let usable = (window_width - 2.0 * PAGE_PADDING_X).max(CELL_MIN);
    let cell = ((usable - (columns - 1.0) * gap) / columns).floor();
    cell.clamp(CELL_MIN, CELL_MAX).round() as u32
}

/// Colonnes après un cran de zoom : `step` positif = vignettes plus grandes,
/// donc une colonne de moins. C'est le vrai levier dans une grille qui remplit
/// la largeur (`cell_size` n'y est qu'une largeur minimale).
fn zoom_columns(columns: usize, step: i32) -> usize {
    (columns as i32 - step).clamp(COLUMNS_MIN as i32, COLUMNS_MAX as i32) as usize
}

/// Largeur de vignette, largeur d'image et nombre de colonnes pour une largeur
/// de fenêtre donnée (`cell_size` = largeur minimale, `columns` = maximum).
///
/// La grille occupe toute la largeur utile : c'est ce qui évite la bande noire
/// à droite quand on agrandit la fenêtre.
/// Plage de vignettes à charger pour une grille : les rangées visibles, une
/// rangée de marge de chaque côté (une rangée partielle compte).
fn visible_items(
    scroll: f32,
    row_height: f32,
    rows: usize,
    columns: usize,
    len: usize,
) -> std::ops::Range<usize> {
    let columns = columns.max(1);
    let first = (scroll / row_height.max(1.0)).floor().max(0.0) as usize;
    let start = (first.saturating_sub(1) * columns).min(len);
    let end = ((first + rows.max(1) + 1) * columns).min(len);
    start..end.max(start)
}

fn grid_metrics(
    window_width: f32,
    min_cell: f32,
    max_columns: usize,
    gap: f32,
) -> (f32, f32, usize) {
    let min_cell = min_cell.max(80.0);
    let max_columns = max_columns.max(1) as f32;
    let usable = (window_width - 2.0 * PAGE_PADDING_X).max(min_cell);

    // combien de vignettes tiennent à la largeur minimale…
    let fitted = ((usable + gap) / (min_cell + gap)).floor().max(1.0);
    // … sans dépasser `columns`, puis on étale pour remplir la largeur
    let columns = fitted.min(max_columns).max(1.0);
    let cell = ((usable - (columns - 1.0) * gap) / columns)
        .floor()
        .max(min_cell);
    (cell, cell - 2.0 * PADDING, columns as usize)
}

fn hint(glyph: image::Handle, label: &'static str) -> Element<'static, Message> {
    row![
        image(glyph).width(Length::Fixed(26.0)).height(Length::Fixed(26.0)),
        text(label).size(14).color(MUTED),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// Sample occupant tout l'espace disponible (aperçu de la vue liste), avec
/// l'initiale du jeu par-dessus pour ne pas confondre deux jeux sans cover.
fn sample_cover<'a>(
    sample: &image::Handle,
    name: &str,
    icon: Option<&image::Handle>,
) -> Element<'a, Message> {
    sample_layer(sample, name, icon, 420.0)
}

/// Sample + icône du paquet (petite, ratio conservé) + NOM DU JEU : sans
/// cover, on doit au moins reconnaître le jeu et lire son nom.
fn sample_layer<'a>(
    sample: &image::Handle,
    name: &str,
    icon: Option<&image::Handle>,
    width: f32,
) -> Element<'a, Message> {
    let picture = image(sample.clone())
        .width(Fill)
        .height(Fill)
        .content_fit(ContentFit::Contain);

    // l'icône du .wgp/.lgp remplace l'initiale quand elle existe : petite et
    // jamais étirée (`Contain`)
    let emblem: Element<'a, Message> = match icon {
        Some(handle) => image(handle.clone())
            .width(Length::Fixed(width * 0.42))
            .height(Length::Fixed(width * 0.42))
            .content_fit(ContentFit::Contain)
            .into(),
        None => text(initial(name))
            .size((width * 0.30).min(96.0))
            .color(Color { a: 0.55, ..Color::WHITE })
            .into(),
    };

    let size = (width / 13.0).clamp(11.0, 22.0);
    // largeur approximative d'un caractère pour cette taille de police
    let max = ((width / (size * 0.58)) as usize).max(6);
    let label = text(short(name, max))
        .size(size)
        .color(Color { a: 0.85, ..Color::WHITE })
        .align_x(Alignment::Center);

    let overlay = container(column![emblem, label].spacing(8).align_x(Alignment::Center))
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    stack![picture, overlay].into()
}

/// Sample à la taille exacte d'une cover (vignette de grille, ligne de liste).
/// En dessous de 90 px, l'initiale n'est pas lisible : on garde l'image seule.
fn sample_art<'a>(
    sample: &image::Handle,
    name: &str,
    icon: Option<&image::Handle>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    if width < 90.0 {
        // vignette minuscule (ligne de liste) : l'icône seule suffit, le nom
        // est écrit à côté
        return match icon {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(width))
                .height(Length::Fixed(height))
                .content_fit(ContentFit::Contain)
                .into(),
            None => image(sample.clone())
                .width(Length::Fixed(width))
                .height(Length::Fixed(height))
                .content_fit(ContentFit::Contain)
                .into(),
        };
    }
    container(sample_layer(sample, name, icon, width))
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

/// Initiale affichée sur le sample.
fn initial(name: &str) -> String {
    name.chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string()
}

/// Vignette de grille : aucun cadre. La sélection se voit au fond teinté.
fn card_style(selected: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if selected {
            CELL_BG_SELECTED
        } else {
            Color::TRANSPARENT
        })),
        // le fond teinté seul ne suffisait pas : sur 666 vignettes on perdait
        // de vue laquelle était sélectionnée (retour 18/11)
        border: Border {
            color: if selected { ACCENT } else { Color::TRANSPARENT },
            width: if selected { FOCUS_BORDER } else { 0.0 },
            radius: 14.0.into(),
        },
        ..container::Style::default()
    }
}

/// Champ de saisie du prompt « nom de recherche » : mêmes couleurs que le
/// panneau, bordure d'accent quand il a le focus.
fn rename_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border = match status {
        text_input::Status::Focused { .. } => ACCENT,
        _ => CELL_BORDER,
    };
    text_input::Style {
        background: Background::Color(CELL_BG),
        border: Border {
            color: border,
            width: 1.0,
            radius: 10.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: ACCENT,
    }
}

/// Pastille d'action de la page d'un jeu.
fn action_style(selected: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if selected { ACCENT } else { CELL_BG })),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

fn picker_panel_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.06, 0.07, 0.10))),
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: 18.0.into(),
        },
        ..container::Style::default()
    }
}

/// Ligne de liste : aucun cadre, fond teinté seulement pour la sélection.
fn list_style(selected: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if selected {
            CELL_BG_SELECTED
        } else {
            Color::TRANSPARENT
        })),
        // le fond teinté seul ne suffisait pas : sur 666 vignettes on perdait
        // de vue laquelle était sélectionnée (retour 18/11)
        border: Border {
            color: if selected { ACCENT } else { Color::TRANSPARENT },
            width: if selected { FOCUS_BORDER } else { 0.0 },
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Nom court pour l'affichage (les noms de paquets peuvent être longs).
fn short(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_string();
    }
    let mut out: String = name.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn plural(count: usize) -> String {
    if count > 1 {
        format!("{count} jeux")
    } else {
        format!("{count} jeu")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rangée_partielle_comprise_dans_la_plage() {
        // 3 rangées pleines + la 4e partielle doivent être demandées
        assert_eq!(visible_items(0.0, 300.0, 3, 5, 100), 0..20);
        // au défilement de la 3e rangée, la marge du haut est incluse
        assert_eq!(visible_items(900.0, 300.0, 3, 5, 100), 10..35);
        // dernière rangée de la liste
        assert_eq!(visible_items(5_700.0, 300.0, 3, 5, 100), 90..100);
        // défilement au-delà de la fin : plage vide, pas de débordement
        assert_eq!(visible_items(30_000.0, 300.0, 3, 5, 100), 100..100);
    }

    #[test]
    fn saut_de_page_va_directement_au_but() {
        // une rangée à parcourir : glissement doux
        let doux = ease_or_snap(0.0, 68.0, 68.0, 0.016);
        assert!(doux > 0.0 && doux < 68.0, "un pas de rangée s'anime");
        // plusieurs écrans : on colle au but, sinon la sélection sort du champ
        assert_eq!(ease_or_snap(0.0, 5_000.0, 68.0, 0.016), 5_000.0);
    }

    #[test]
    fn zoom_arriere_rajoute_des_colonnes_a_l_ecran() {
        // le zoom arrière doit se voir : la largeur minimale calculée pour N
        // colonnes doit en produire N, et non plafonner au compte courant
        for columns in COLUMNS_MIN..=COLUMNS_MAX {
            let cell = cell_for_columns(2560.0, columns, GAP) as f32;
            let (_, _, rendered) = grid_metrics(2560.0, cell, columns, GAP);
            assert_eq!(rendered, columns, "{columns} colonnes demandées");
        }
        // fenêtre étroite : bornée par la largeur, jamais plus que demandé
        let cell = cell_for_columns(1280.0, 6, GAP) as f32;
        let (_, _, rendered) = grid_metrics(1280.0, cell, 6, GAP);
        assert!(rendered <= 6 && rendered >= 5, "6 demandées, {rendered} rendues");
    }

    #[test]
    fn zoom_change_les_colonnes() {
        // grille qui remplit la largeur : le cran change une colonne, pas la
        // largeur minimale (sinon `columns` plafonnait et le zoom ne faisait rien)
        assert_eq!(zoom_columns(5, 1), 4, "zoom + : vignettes plus grandes");
        assert_eq!(zoom_columns(5, -1), 6, "zoom − : vignettes plus petites");
        assert_eq!(zoom_columns(COLUMNS_MIN, 1), COLUMNS_MIN, "borne basse");
        assert_eq!(zoom_columns(COLUMNS_MAX, -1), COLUMNS_MAX, "borne haute");
    }

    #[test]
    fn grille_remplit_la_fenetre() {
        // 1280 px : 5 vignettes de ~234 px, toute la largeur est occupée
        let (cell, art, columns) = grid_metrics(1280.0, 220.0, 5, GAP);
        assert_eq!(columns, 5);
        assert!(cell >= 220.0);
        assert!((art - (cell - 2.0 * PADDING)).abs() < 0.01);

        // agrandir étale les vignettes, sans bande noire à droite
        let (large, _, columns) = grid_metrics(2560.0, 220.0, 5, GAP);
        assert_eq!(columns, 5);
        assert!(large > cell, "les vignettes grandissent avec la fenêtre");
        let filled = large * 5.0 + GAP * 4.0;
        assert!(filled <= 2560.0 - 2.0 * PAGE_PADDING_X);
        assert!(filled >= 2560.0 - 2.0 * PAGE_PADDING_X - 5.0, "aucun vide à droite");

        // fenêtre étroite : moins de colonnes, jamais de débordement
        let (_, _, columns) = grid_metrics(700.0, 220.0, 5, GAP);
        assert_eq!(columns, 2);
        let (cell, _, _) = grid_metrics(700.0, 220.0, 5, GAP);
        assert!(cell * 2.0 + GAP <= 700.0 - 2.0 * PAGE_PADDING_X);
    }

    #[test]
    fn mosaique_sans_espace() {
        // espacement nul : les colonnes collées remplissent exactement la largeur
        let (cell, _, columns) = grid_metrics(1280.0, 220.0, 8, 0.0);
        assert_eq!(columns, 5);
        assert!((cell * columns as f32 - (1280.0 - 2.0 * PAGE_PADDING_X)).abs() <= 5.0);
    }

    #[test]
    fn nom_court_tronque() {
        assert_eq!(short("Court", 10), "Court");
        assert_eq!(short("Un nom vraiment très long", 10), "Un nom vr…");
    }

    #[test]
    fn pluriel() {
        assert_eq!(plural(0), "0 jeu");
        assert_eq!(plural(1), "1 jeu");
        assert_eq!(plural(3), "3 jeux");
    }
}
