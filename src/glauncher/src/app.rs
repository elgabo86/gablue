//! État, messages et interface du launcher.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use iced::advanced::text::Wrapping;
use iced::futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use iced::futures::sink::SinkExt;
use iced::futures::{Stream, StreamExt};
use iced::keyboard::key::Named;
use iced::widget::operation;
use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset};
use iced::widget::{
    Column, Id, Row, column, container, image, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Fill, Font, Length, Padding, Point,
    Rectangle, Shadow, Size, Subscription, Task, Theme, Vector, border, keyboard, window,
};

use crate::assets;
use crate::config::{Config, ViewMode};
use crate::copy;
use crate::covers;
use crate::cursor;
use crate::dialog;
use crate::favorites;
use crate::recent;
use crate::folders;
use crate::gallery;
use crate::gamepad::{PadButton, PadEvent, PadKind};
use crate::index::{Index, Refresh};
use crate::launch;
use crate::library::{self, ArtworkCache, Game, Kind};
use crate::marquee;
use crate::mode::Mode;
use crate::music;
use crate::pergame;
use crate::play;
use crate::playtime;
use crate::pose::Pose;
use crate::recorder;
use crate::sparkles;
use crate::spinner;
use crate::state::{self, Scope, SortBy};
use crate::status::{self, Reseau};
use crate::thumbs;
use crate::updates;

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

const GAP: f32 = 20.0;
/// Rayon des coins d'une vignette (partagé avec le canvas, qui redessine le
/// cadre de sélection grossi).
const CARD_RADIUS: f32 = 14.0;
/// Rayon des coins de la cover elle-même : juste cassé, pour que le visuel
/// ressemble à une boîte posée et non à un rectangle brut (demandé le 20/11).
const ART_RADIUS: f32 = 5.0;
/// Marge verticale autour de la liste (la verticale de `PAGE_PADDING_X`).
/// Les deux servent aussi à situer la vignette sélectionnée, d'où le partage
/// avec `selection_center`.
pub(crate) const PAGE_PADDING_Y: f32 = 18.0;
/// Bande du pied de page : le canvas des particules n'est pas découpé par la
/// zone défilante, donc il découpe lui-même à cette bande.
pub(crate) const FOOTER_BAND: f32 = 56.0;
/// Colonnes du menu de choix d'image.
const PICKER_COLUMNS: usize = 5;

/// Identifiant du défilement du menu de choix d'image : la navigation à la
/// manette change la sélection sans que le `scrollable` suive, donc les
/// dernières lignes restaient sous le panneau (signalé le 23/11).
const PICKER_SCROLL: &str = "glauncher-picker";
const PADDING: f32 = 12.0;
/// Marge horizontale du launcher (padding de la vue), à gauche. À droite on
/// ajoute `SCROLLBAR_GUTTER` : la barre de défilement d'iced est une
/// surimpression, elle ne réserve aucune place — sans cette marge, la colonne
/// de droite passait dessous (constaté 20/11).
const PAGE_PADDING_X: f32 = 26.0;
const SCROLLBAR_GUTTER: f32 = 16.0;
const PAGE_PADDING_X_RIGHT: f32 = PAGE_PADDING_X + SCROLLBAR_GUTTER;
/// Marge entre le bord de la case (la place réservée dans la grille) et la
/// boîte visible : la vignette rétrécit d'autant et l'air entre les boîtes
/// augmente, sans toucher au nombre de colonnes (demandé, 20/11).
const CARD_INSET: f32 = 9.0;
/// Ratio hauteur/largeur d'une cover (600×900) : les vignettes ont la forme des
/// vraies covers, sample compris.
const ART_RATIO: f32 = 1.5;
/// Hauteur de la ligne de titre sous une vignette.
const CARD_CAPTION: f32 = 24.0;
/// Écart entre la cover et son titre.
const TITLE_GAP: f32 = 6.0;

/// Hauteur, sous le bas de la cover, du **milieu** de la ligne de titre : c'est
/// le point fixe de la respiration (le texte ne bouge pas d'un pixel, la
/// vignette grossit autour de lui).
fn caption_middle() -> f32 {
    TITLE_GAP + (CARD_CAPTION - TITLE_GAP) * 0.5
}

/// En mosaïque, les vignettes sont plus petites que `cell_size` (densité façon
/// Switch Online) ; le zoom agit sur cette taille comme sur la grille.
const MOSAIC_SCALE: f32 = 0.55;
/// Fenêtre de double appui sur « retour » pour quitter le launcher.
const QUIT_WINDOW: Duration = Duration::from_millis(1500);
/// Zoom des vignettes : pas, bornes (px de largeur).
const ZOOM_STEP: f32 = 40.0;
/// Durée par défaut de la glissade d'un changement de taille (`+`/`−`, L2/R2) :
/// les vignettes partent de leur ancienne case et s'installent dans la nouvelle.
/// `GLAUNCHER_ZOOM_MS` règle la durée sans recompiler (40 à 2000 ms), pour juger
/// le mouvement sur place — même esprit que `GLAUNCHER_FX_MS`.
const ZOOM_SECONDS: f32 = 0.16;

/// Durée réellement utilisée (variable d'environnement ou valeur par défaut).
fn zoom_seconds() -> f32 {
    static VALUE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("GLAUNCHER_ZOOM_MS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(|ms| (ms.clamp(40, 2000) as f32) / 1000.0)
            .unwrap_or(ZOOM_SECONDS)
    })
}
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
/// Roue de chargement : taille (px) et vitesse de rotation (rad/s).
const SPIN_SIZE: f32 = 22.0;

/// Hauteur de la bande de progression des copies, tout en haut de la fenêtre.
/// Présente même au repos (transparente) : la forme de la pile racine ne doit
/// pas changer d'un rendu à l'autre.
const BANDE_COPIE: f32 = 20.0;
const SPIN_SPEED: f32 = 6.0;
/// Durée du fondu de la page : montée vers le fond, bascule, descente. Deux
/// rampes, parce que sauter au noir d'un coup puis en ressortir se lit comme un
/// éclair (jugé « trop agressif » le 20/11) — le fondu enchaîné par le fond,
/// lui, ne fait aucun saut : l'ancien écran s'efface, le nouveau émerge.
const PAGE_FADE_RISE: f32 = 0.12;
const PAGE_FADE_FALL: f32 = 0.20;
/// Bascule de LISTE (vue, tri, filtre d'année) : le même fondu enchaîné, plus
/// court et sans aller au noir complet — on ne change pas d'écran, on
/// réordonne le même. Le contenu change au sommet, quand on ne le voit plus.
const SWAP_RISE: f32 = 0.12;
const SWAP_FALL: f32 = 0.18;
const SWAP_PEAK: f32 = 0.92;
/// Apparition d'un visuel de la page (héros, logo, cover) arrivé après coup.
const PAGE_ASSET_SECONDS: f32 = 0.30;
/// Plongée au lancement d'un jeu : la page grossit de 6 % pendant que le voile
/// monte, comme si on entrait dans l'écran. Un **agrandissement** (et non un
/// recul) parce qu'un contenu qui rétrécit laissait voir son bord au milieu du
/// noir (jugé moche le 20/11) — agrandir ne découvre jamais rien.
const PAGE_DIVE: f32 = 0.06;
/// Retour d'un jeu : chaque vignette revient de son côté de l'écran
/// (`RETURN_SPREAD` = part de la largeur parcourue) en dézoomant
/// (`RETURN_ZOOM` de marge) et avec un temps de retard qui dépend de sa
/// distance au centre (`RETURN_STAGGER`). `RETURN_END` laisse les vignettes se
/// poser AVANT la fin du fondu : sans ça, l'ultime image où la carte cesse
/// d'être transformée sautait de quelques pixels.
const RETURN_SPREAD: f32 = 0.55;
const RETURN_ZOOM: f32 = 0.10;
const RETURN_STAGGER: f32 = 0.45;
const RETURN_END: f32 = 0.15;
/// Constante de temps du défilement animé (secondes).
const SCROLL_TAU: f32 = 0.10;

/// Sonde de fluidité (`GLAUNCHER_ANIM_DEBUG=1`) : `tick` journalise alors
/// l'intervalle réel entre deux images pendant un fondu (16,7 ms = 60 Hz).
fn anim_debug() -> bool {
    static VALUE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| std::env::var_os("GLAUNCHER_ANIM_DEBUG").is_some())
}

/// Vignette de remplacement, à la taille et à la forme d'une vraie cover.
const COVER_SAMPLE: &[u8] = include_bytes!("../assets/cover-sample.png");
/// Étoile des favoris : un PNG embarqué plutôt que le caractère `★`, qui
/// tombait sur un glyphe de repli minuscule (illisible sur une vignette).
const STAR: &[u8] = include_bytes!("../assets/star.png");

/// Police de l'aide (pied de page) : Lato, embarquée dans le binaire. La
/// sans-serif système change d'un poste à l'autre ; celle-ci est plus ronde et
/// se lit mieux en petit sur fond sombre.
pub(crate) const HELP_FONT: Font = Font::with_name("Lato");

// --- glyphes ----------------------------------------------------------------

const XBOX_CONFIRM: &[u8] = include_bytes!("../assets/glyphs/xbox/confirm.png");
const XBOX_BACK: &[u8] = include_bytes!("../assets/glyphs/xbox/back.png");
const XBOX_VIEW: &[u8] = include_bytes!("../assets/glyphs/xbox/view.png");
const XBOX_MENU: &[u8] = include_bytes!("../assets/glyphs/xbox/menu.png");
const XBOX_START: &[u8] = include_bytes!("../assets/glyphs/xbox/start.png");
const XBOX_ZOOM_IN: &[u8] = include_bytes!("../assets/glyphs/xbox/zoom-in.png");
const XBOX_ZOOM_OUT: &[u8] = include_bytes!("../assets/glyphs/xbox/zoom-out.png");
const PS_CONFIRM: &[u8] = include_bytes!("../assets/glyphs/playstation/confirm.png");
const PS_BACK: &[u8] = include_bytes!("../assets/glyphs/playstation/back.png");
const PS_VIEW: &[u8] = include_bytes!("../assets/glyphs/playstation/view.png");
const PS_MENU: &[u8] = include_bytes!("../assets/glyphs/playstation/menu.png");
const PS_START: &[u8] = include_bytes!("../assets/glyphs/playstation/start.png");
const PS_ZOOM_IN: &[u8] = include_bytes!("../assets/glyphs/playstation/zoom-in.png");
const PS_ZOOM_OUT: &[u8] = include_bytes!("../assets/glyphs/playstation/zoom-out.png");

/// Glyphes de boutons, dans le style de la manette utilisée.
///
/// `view` porte le bouton du **changement de type de liste** : depuis le
/// 22/11 c'est **Start** (Triangle et Start ont été échangés, Start est
/// explicite ; `sort` reste Triangle). `menu` = X/Carré (retirer, effacer,
/// tout ce qui est armé).
struct Glyphs {
    confirm: image::Handle,
    back: image::Handle,
    view: image::Handle,
    sort: image::Handle,
    menu: image::Handle,
    zoom_in: image::Handle,
    zoom_out: image::Handle,
}

impl Glyphs {
    fn for_kind(kind: PadKind) -> Self {
        let (confirm, back, view, sort, menu, zoom_in, zoom_out) = match kind {
            PadKind::Playstation => (
                PS_CONFIRM, PS_BACK, PS_START, PS_VIEW, PS_MENU, PS_ZOOM_IN, PS_ZOOM_OUT,
            ),
            _ => (
                XBOX_CONFIRM,
                XBOX_BACK,
                XBOX_START,
                XBOX_VIEW,
                XBOX_MENU,
                XBOX_ZOOM_IN,
                XBOX_ZOOM_OUT,
            ),
        };
        Self {
            confirm: image::Handle::from_bytes(confirm),
            back: image::Handle::from_bytes(back),
            view: image::Handle::from_bytes(view),
            sort: image::Handle::from_bytes(sort),
            menu: image::Handle::from_bytes(menu),
            zoom_in: image::Handle::from_bytes(zoom_in),
            zoom_out: image::Handle::from_bytes(zoom_out),
        }
    }
}

// --- bus d'évènements (manettes, fin de jeu) --------------------------------

static BUS: OnceLock<UnboundedSender<Message>> = OnceLock::new();
static BUS_RX: Mutex<Option<UnboundedReceiver<Message>>> = Mutex::new(None);

pub(crate) fn post(message: Message) {
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
        covers::CoverEvent::Choice {
            slug,
            kind,
            url,
            thumb,
        } => Message::Choice {
            slug,
            kind,
            url,
            thumb,
        },
        covers::CoverEvent::Icon { slug, bytes } => Message::Icon { slug, bytes },
        covers::CoverEvent::Games {
            slug,
            query,
            result,
        } => Message::IdentifyResults {
            slug,
            query,
            result,
        },
        covers::CoverEvent::Year { slug, key, year } => Message::Year { slug, key, year },
        covers::CoverEvent::Identified { slug, identity } => Message::Identified { slug, identity },
        covers::CoverEvent::Absent { slug, requete } => Message::Absent { slug, requete },
    }
}

/// Lance le jeu et attend sa fin, puis renvoie le résultat par le bus.
fn spawn_launch(name: String, command: launch::Launch, cache: PathBuf, slug: String, key: String) {
    let _ = std::thread::Builder::new()
        .name(format!("glauncher-run:{name}"))
        .spawn(move || {
            let result = launch::run(&command, &cache, &slug, &key, &name);
            post(Message::Launched { name, result });
        });
}

/// Réindexe en arrière-plan (scan complet ou rafale de chemins) et poste le
/// résultat : le thread UI ne touche jamais le disque ni SQLite.
/// Compte les captures et vidéos **par jeu** (nom écrit dans le sidecar au
/// lancement) sur un fil de fond : ouvrir une page ne doit pas parcourir les
/// dossiers de sortie ni ouvrir les PNG sans sidecar.
fn spawn_comptes_galerie() {
    let _ = std::thread::Builder::new()
        .name("glauncher-galerie".into())
        .spawn(|| {
            let mut comptes: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for item in gallery::scanne() {
                if let Some(titre) = item.titre {
                    *comptes.entry(titre).or_default() += 1;
                }
            }
            post(Message::GalerieComptes(comptes));
        });
}

fn spawn_refresh(
    config: Config,
    index: Arc<Mutex<Option<Index>>>,
    changed: Vec<PathBuf>,
    decouvrir: bool,
) {
    let _ = std::thread::Builder::new()
        .name("glauncher-index".into())
        .spawn(move || {
            let mut config = config;
            // Disques externes : la découverte (bornée, mémorisée) vit ici, dans
            // le fil d'index. Un disque jamais vu coûte un parcours borné ; un
            // disque déjà vu ne coûte que des `stat` sur les racines mémorisées.
            let racines = if config.scan_disks {
                crate::disks::racines(&config.cache_dir(), crate::disks::PROFONDEUR)
            } else {
                Vec::new()
            };
            if racines != config.extra_dirs {
                config.extra_dirs = racines.clone();
                post(Message::Discovered(racines));
            }
            let started = std::time::Instant::now();
            let result = (|| -> Result<Refresh, String> {
                let mut guard = index.lock().map_err(|_| "index verrouillé".to_string())?;
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
            eprintln!("glauncher : index en {} ms", started.elapsed().as_millis());
            post(Message::Refreshed(result));

            // Phase 2, seulement au branchement (jamais à chaque rafale) : un
            // dossier de jeux créé depuis le dernier branchement n'est pas dans
            // la mémoire des racines. Le parcours borné tourne APRÈS le
            // rafraîchissement, donc les jeux des dossiers connus sont déjà à
            // l'écran quand il commence.
            if !decouvrir || !config.scan_disks {
                return;
            }
            let ajouts = crate::disks::nouveaux(&config.extra_dirs, crate::disks::PROFONDEUR);
            if ajouts.is_empty() {
                return;
            }
            let mut racines = config.extra_dirs.clone();
            racines.extend(ajouts.iter().cloned());
            racines.sort();
            crate::disks::memoriser(&config.cache_dir(), &racines);
            post(Message::Discovered(racines));
            post(Message::Changed(ajouts));
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
    /// Clic ou molette : la barre de défilement revient (voir `barre`).
    Souris,
    CardClicked(usize),
    /// Visuel SteamGridDB (grid, héros, logo, icône) ; `bytes: None` = introuvable.
    Art {
        slug: String,
        kind: covers::ArtKind,
        bytes: Option<Vec<u8>>,
    },
    /// Icône extraite du paquet (`.icon.png` dans le squashfs).
    Icon {
        slug: String,
        bytes: Vec<u8>,
    },
    /// Miniature 320 px d'une cover (worker de miniatures).
    Thumb {
        slug: String,
        bytes: Vec<u8>,
    },
    /// Icône cherchée à côté du paquet (worker d'assets, disque externe).
    LocalIcon {
        slug: String,
        icon: Option<image::Handle>,
    },
    ArtFailed {
        slug: String,
        kind: covers::ArtKind,
        error: String,
    },
    /// Menu « choisir une image » pour un type de visuel.
    OpenPicker(covers::ArtKind),
    ClosePicker,
    PickerNav(Nav),
    /// Clic sur une vignette : elle est **choisie** (le menu se referme tout de
    /// suite, le téléchargement finit en fond), là où le clavier/manette
    /// sélectionnent puis valident. Un simple clic ne laisse donc pas le menu
    /// ouvert sur une case seulement mise en évidence.
    PickerChoose(usize),
    PickerPick,
    /// Choisit une image perso (fichier local) : ouvre le sélecteur système
    /// dans un fil à part, la réponse arrive en `FileChosen`.
    PickerFile,
    FileChosen {
        slug: String,
        kind: covers::ArtKind,
        path: Option<PathBuf>,
    },
    /// « Ce jeu a déjà des visuels : les remplacer ? » (posée après une
    /// identification) : droite/gauche puis valider, « Non » sélectionné.
    ReplaceNav(i32),
    ReplaceChoose(usize),
    ReplacePick,
    /// Le scan des disques externes a été basculé dans les Réglages.
    SettingsDisques,
    /// « Copier le jeu… » : ouvre le choix de la destination.
    CopyOpen,
    CopyNav(i32),
    CopyChoose(usize),
    CopyPick,
    /// « ce fichier existe en plus ancien : le mettre à jour ? »
    CopyReplaceNav(i32),
    CopyReplaceChoose(usize),
    CopyReplacePick,
    /// Avancement de la copie en cours (octets écrits pour ce jeu).
    CopyStep { fait: u64 },
    CopyDone { size: u64 },
    CopyFailed { nom: String, erreur: String },
    /// Racines de jeux trouvées sur les disques montés (ou liste vide).
    Discovered(Vec<PathBuf>),
    /// L'ensemble des disques montés a changé (branchement, retrait).
    Disks,
    /// Le lien réseau a changé (Wi-Fi ↔ filaire ↔ hors ligne).
    Reseau(Reseau),
    /// « Oublier les jeux disparus » : ouvre le dialogue de confirmation.
    OublierOpen,
    OublierNav(i32),
    OublierChoose(usize),
    OublierPick,
    Choices {
        slug: String,
        kind: covers::ArtKind,
        choices: Vec<covers::Choice>,
    },
    /// Vignette d'une proposition, par son `url` (l'ordre des propositions ne
    /// bouge pas, mais l'URL est le seul identifiant stable).
    Choice {
        slug: String,
        kind: covers::ArtKind,
        url: String,
        thumb: Vec<u8>,
    },
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
    /// Identifie le jeu chez SteamGridDB (menu d'images) : nom recherché,
    /// puis choix dans la liste des jeux proposés.
    IdentifyOpen,
    IdentifyInput(String),
    IdentifySubmit,
    IdentifyCancel,
    /// Jeux proposés par l'autocomplete (ou l'erreur réseau).
    IdentifyResults {
        slug: String,
        query: String,
        result: Result<Vec<covers::FoundGame>, String>,
    },
    IdentifyNav(i32),
    /// Choisit le jeu n de la liste (clic).
    IdentifyChoose(usize),
    IdentifyPick,
    IdentifyClose,
    /// Menu « Options » de la page (visuels, identification, date), ouvert par
    /// la pastille de la page : aucun message pour l'ouvrir à distance.
    ArtMenuNav(i32),
    /// Choisit l'entrée n du menu (clic).
    ArtMenuChoose(usize),
    ArtMenuPick,
    ArtMenuClose,
    /// Prompt « date de sortie » : saisie manuelle de l'année.
    DateOpen,
    DateInput(String),
    DateSubmit,
    DateCancel,
    /// Demande la date à SteamGridDB (jeu identifié en priorité).
    DateRefresh,
    /// Taille des vignettes : +1 plus grandes, -1 plus petites.
    Zoom(i32),
    /// Fenêtre active (focus) : sans focus, ni clavier ni manette n'agissent.
    Active(bool),
    /// Position réelle du défilement (molette, glisser, `scroll_to`) et
    /// maximum que le `scrollable` d'iced autorise pour le contenu courant
    /// (`content - viewport`) : viser au-delà ne déplace rien de plus.
    Scrolled {
        offset: f32,
        max: f32,
    },
    Play(usize),
    /// Chemins modifiés signalés par le veilleur.
    Changed(Vec<PathBuf>),
    /// Liste à jour après réindexation (fond).
    Refreshed(Result<Refresh, String>),
    Launched {
        name: String,
        result: Result<i32, String>,
    },
    Tick,
    /// Une image de la poussière d'étoiles (minuteur de `sparkles`).
    Sparkle,
    /// Progression d'une mise à jour : étape du protocole `::gablue:` ou fin
    /// de la commande (voir `src/updates.rs`).
    Update(updates::Event),
    /// Lance la mise à jour de l'image (clavier `M` pour l'instant).
    UpdateApply,
    /// Page Réglages : ouverture, fermeture, navigation.
    SettingsOpen,
    SettingsClose,
    SettingsNav(i32),
    /// Croix : ajouter un dossier (ou entrer dans un dossier existant).
    SettingsAct,
    /// Gauche/droite : cycle la ligne « écran du jeu ».
    SettingsChoix(i32),
    /// Carré : armer le retrait du dossier sélectionné.
    SettingsArm,
    /// Carré sur un dossier de sortie : retire la clé de `recorder.conf`
    /// (retour au défaut du démon et de `screenshot.sh`).
    SettingsDefault(&'static str),
    /// Navigateur de dossiers : navigation, choix, remontée.
    BrowserNav(i32),
    BrowserAct,
    BrowserUp,
    /// La liste des dossiers a changé : écriture de `config.toml`, rescan, et
    /// ce libellé dans le pied de page.
    SettingsChanged(String),
    /// La musique de fond a changé (activée/coupée, volume) : appliqué à
    /// chaud puis écrit dans `config.toml`.
    SettingsMusic,
    /// Galerie : ouvrir (avec le nom du jeu pour n'afficher QUE ses captures et
    /// vidéos ; `None` = tout), fermer, naviguer, ouvrir l'élément sélectionné.
    GalleryOpen(Option<String>),
    GalleryClose,
    GalleryNav(i32),
    GalleryAct,
    /// Clic sur une vignette : sélectionne, ou ouvre si déjà sélectionnée.
    GalleryChoose(usize),
    /// Carré : armer l'effacement (Croix confirme, Rond annule).
    GalleryArm,
    GalleryUnarm,
    /// Triangle (ou Tab) : cycle tout → captures → vidéos.
    GalleryFilter,
    /// Carré dans l'aperçu : copie du screenshot avec le logo du jeu.
    ApercuCopy,
    /// Combien de captures/vidéos par jeu (lues en fond, pour savoir si la
    /// page d'un jeu propose le bouton « Galerie »).
    GalerieComptes(std::collections::HashMap<String, usize>),
    /// Alias visuel : choisir un autre jeu de la bibliothèque comme source.
    AliasOpen,
    AliasNav(i32),
    AliasChoose(usize),
    AliasPick,
    AliasClose,
    /// Une vignette de galerie est prête (ou absente).
    GalleryThumb {
        path: PathBuf,
        bytes: Option<Vec<u8>>,
    },
    /// Horloge de la galerie : fin de lecture vidéo, fondu de l'aperçu.
    GalleryTick,
    /// La vidéo est terminée (ou tuée après le délai de grâce) : retour.
    GalleryVideoEnd,
    /// Aperçu plein écran d'une capture : précédent / suivant / fermer.
    ApercuNav(i32),
    ApercuClose,
    /// Un téléchargement commence (le worker de visuels réveille l'interface,
    /// sinon la roue de chargement n'apparaîtrait qu'une fois la requête finie).
    Activity,
    /// Aucune fiche SteamGridDB : la marque datée est retenue et écrite dans
    /// `assets/absents.txt` (l'utilisateur peut la revérifier depuis Options).
    Absent { slug: String, requete: String },
    /// Année de sortie apprise par le worker pour ce jeu (`None` : SGDB n'en
    /// donne pas, c'est noté pour ne plus redemander). `key` = sous quel nom
    /// elle est rangée dans `assets/years.txt` (le jeu SteamGridDB, partagé).
    Year {
        slug: String,
        key: String,
        year: Option<u32>,
    },
    /// Le jeu SteamGridDB d'un paquet a été appris par le worker (et écrit dans
    /// `assets/identities.txt`) : c'est lui qui nomme les visuels partagés.
    Identified {
        slug: String,
        identity: covers::Identity,
    },
    ToggleView,
    /// Retour (clic droit, comme Échap/B) : referme menu, page, prompt.
    Back,
    /// Menu « type de liste » (triangle / Y) : ouvrir, naviguer, choisir.
    ViewMenu,
    ViewMenuNav(i32),
    ViewMenuChoose(usize),
    /// Menu « tri » (carré / X, `T`) : A→Z, récents, favoris.
    SortMenu,
    SortMenuNav(i32),
    SortMenuChoose(usize),
    /// Menu « année » (filtre), ouvert depuis la ligne « Année » du tri.
    YearMenuNav(i32),
    YearMenuChoose(usize),
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
    /// Vignettes prêtes à afficher, dans le même ordre que `choices` ;
    /// `None` = pas encore téléchargée (la case reste vide). Construites UNE
    /// fois : `image::Handle::from_bytes` dans la vue créait un handle neuf à
    /// chaque image, donc iced_wgpu redécodait les 24 PNG à chaque rendu —
    /// images qui clignotent et menu poussif (constaté 18/11).
    thumbs: Vec<Option<image::Handle>>,
}

/// Menu d'identification : les jeux proposés par l'autocomplete de
/// SteamGridDB pour un nom donné. Choisir une ligne écrit l'identification
/// dans `assets/identities.txt` (partagée par Syncthing) et redemande les
/// quatre visuels du jeu choisi, en fond.
/// Menu « reprendre les visuels d'un autre jeu » : choix figé à l'ouverture,
/// pour que la liste ne bouge pas pendant la navigation.
#[derive(Debug, Clone)]
struct AliasMenu {
    /// Jeu cible (le paquet ouvert).
    slug: String,
    /// `(nom affiché, slug source, vignette)` des jeux qui ont déjà des visuels.
    choix: Vec<(String, String, Option<image::Handle>)>,
    index: usize,
}

struct IdentifyMenu {
    slug: String,
    /// Nom envoyé à SteamGridDB (modifiable, souvent l'édition en trop).
    query: String,
    index: usize,
    loading: bool,
    error: Option<String>,
    games: Vec<covers::FoundGame>,
    /// Identification actuelle du paquet (coche la ligne correspondante).
    current: Option<u64>,
}

/// Question posée après une identification quand le paquet a **déjà** des
/// visuels : les remplacer par ceux du jeu choisi, ou les garder ?
///
/// Le jeu et le nom SteamGridDB déjà retenus sont conservés ici, parce qu'ils
/// ne servent qu'à lancer le téléchargement si l'utilisateur dit oui.
struct Replace {
    slug: String,
    /// Clé des visuels de l'ANCIEN jeu (celle qu'on oublie si on remplace).
    old_key: String,
    game: covers::FoundGame,
    /// 0 = « Non, garder » (choix par défaut), 1 = « Oui, remplacer ».
    index: usize,
}

/// Destinations proposées pour copier un paquet : le dossier de
/// téléchargements de l'utilisateur, puis chaque dossier de jeux choisi, avec
/// la place libre de chacun (mesurée à l'ouverture, `statvfs`).
struct CopyMenu {
    dests: Vec<CopyDest>,
    index: usize,
    /// Taille du paquet à copier (affichée dans le titre).
    size: u64,
}

struct CopyDest {
    label: String,
    dir: PathBuf,
    libre: Option<u64>,
}

/// Question posée quand la destination contient déjà le même fichier, **plus
/// ancien** que la source : « le mettre à jour ? ». Rien n'est écrasé sans un
/// accord explicite, et « Annuler » est le choix par défaut.
struct CopyReplace {
    job: copy::Job,
    index: usize,
}

/// File de copies en cours : `total` et `fait` sont en octets et couvrent
/// **toutes** les copies demandées, d'où une seule barre qui avance d'un jeu au
/// suivant sans repartir de zéro.
#[derive(Default)]
struct Copies {
    total: u64,
    /// Octets des copies déjà terminées.
    fait: u64,
    /// Octets écrits pour la copie en cours.
    courant: u64,
    /// Jeu en cours (nom affiché) ; `None` entre deux copies.
    nom: String,
    /// Destination de la copie en cours, pour refuser un doublon.
    en_cours: Option<PathBuf>,
    attente: std::collections::VecDeque<copy::Job>,
    nombre: usize,
}

/// Dialogue « Oublier les jeux disparus » : nombre de fiches concernées et
/// bouton choisi (0 = annuler, 1 = oublier).
struct Oublier {
    count: usize,
    index: usize,
}

/// Géométrie de la grille (ou de la mosaïque) à un instant donné. Capturée
/// avant un changement de taille, elle sert à faire **glisser** chaque vignette
/// de son ancienne case vers la nouvelle au lieu de la faire sauter.
#[derive(Debug, Clone, Copy)]
struct Layout {
    cell: f32,
    art: f32,
    columns: usize,
    slot_height: f32,
    gap: f32,
    inset: f32,
    scroll: f32,
}

/// Changement de taille des vignettes (`+`/`−`, L2/R2) : la nouvelle
/// disposition est posée tout de suite, et chaque vignette part de sa case
/// précédente (position ET taille) pour s'y installer en `ZOOM_SECONDS`
/// (`App::zoom_pose`). Un second cran pendant l'animation repart de la
/// position visuelle courante, donc appuyer sur R2 ne fait pas de sauts.
#[derive(Debug, Clone, Copy)]
struct Zoom {
    old: Layout,
    elapsed: f32,
}

/// Selon le cas : « Jouer », « Favori », « Options » (les quatre visuels,
/// l'identification du jeu et sa date sont réunis derrière ce seul bouton :
/// « Images » ne décrivait plus ce qu'on y trouve) et, **seulement si le jeu a
/// des captures ou des vidéos**, « Galerie » — qui n'affiche que les siennes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageAction {
    Play,
    Favorite,
    Options,
    Gallery,
}

/// Actions présentes pour **tout** jeu ; « Galerie » s'ajoute au bout selon ce
/// que les dossiers de sortie contiennent pour lui (`page_actions`).
const PAGE_ACTIONS: [PageAction; 3] = [PageAction::Play, PageAction::Favorite, PageAction::Options];

impl PageAction {
    fn label(self) -> &'static str {
        match self {
            PageAction::Play => "Jouer",
            PageAction::Favorite => "Favori",
            PageAction::Options => "Options",
            PageAction::Gallery => "Galerie",
        }
    }
}

/// Page d'un jeu (ouverte en validant une vignette).
struct Page {
    index: usize,
    action: usize,
}

/// Basculement de page à faire au sommet du fondu (le fond est alors opaque,
/// donc le changement d'écran ne se voit pas).
#[derive(Clone, Copy)]
enum PageStep {
    Open(usize),
    Close,
}

/// Bascule de liste à faire au sommet de `swap_fade` (vue, tri, année) : le
/// contenu change quand le fond le cache, donc aucun saut visible.
#[derive(Clone, Copy)]
enum Swap {
    View(ViewMode),
    Sort(SortBy),
    Year(Option<u32>),
    /// Résultat d'une réindexation : la liste change (disque branché, paquet
    /// ajouté ou retiré). Rien à décider, juste à poser au sommet du fondu.
    Reload,
}

/// Ordre du menu « Options » de la page : les quatre types de visuels,
/// l'identification, la date de sortie, puis les **options de lancement**
/// propres à ce jeu et à ce poste (`pergame`).
///
/// Le mode manette Xbox ne concerne que les `.wgp` : `gwine --xbox` filtre les
/// manettes d'un jeu Windows, `lgplaunch` n'a pas d'équivalent — la ligne
/// n'apparaît donc que pour un paquet Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtMenuEntry {
    Kind(covers::ArtKind),
    Identify,
    /// Reprendre les visuels d'un autre jeu (alias visuel) : la réponse pour
    /// un jeu absent de SteamGridDB, un fangame qui reprend l'art de sa série.
    Alias,
    Date,
    /// Copier le paquet vers le dossier de téléchargements ou un dossier de
    /// jeux (l'original n'est jamais touché).
    Copier,
    Gamescope,
    Resolution,
    Xbox,
}

fn art_menu_entries(kind: Kind) -> Vec<ArtMenuEntry> {
    let mut entries = vec![
        ArtMenuEntry::Kind(covers::ArtKind::Grid),
        ArtMenuEntry::Kind(covers::ArtKind::Hero),
        ArtMenuEntry::Kind(covers::ArtKind::Logo),
        ArtMenuEntry::Kind(covers::ArtKind::Icon),
        ArtMenuEntry::Identify,
        ArtMenuEntry::Alias,
        ArtMenuEntry::Date,
        ArtMenuEntry::Copier,
        ArtMenuEntry::Gamescope,
        ArtMenuEntry::Resolution,
    ];
    if kind == Kind::Wgp {
        entries.push(ArtMenuEntry::Xbox);
    }
    entries
}

/// Une ligne du menu « Tri » : les ordres, le filtre d'année, la portée de la
/// bibliothèque, et l'effacement de l'historique quand il n'est pas vide.
/// Construire la liste des lignes (au lieu de calculer les indices) évite les
/// décalages quand une ligne conditionnelle apparaît.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMenuEntry {
    Sort(SortBy),
    Year,
    Scope,
    Clear,
}

fn sort_menu_entries(historique_vide: bool) -> Vec<SortMenuEntry> {
    let mut entries: Vec<SortMenuEntry> = SortBy::ALL
        .iter()
        .map(|&sort| SortMenuEntry::Sort(sort))
        .collect();
    entries.push(SortMenuEntry::Year);
    entries.push(SortMenuEntry::Scope);
    if !historique_vide {
        entries.push(SortMenuEntry::Clear);
    }
    entries
}

/// Les vues, dans l'ordre du menu « type de liste ». Les favoris ne sont pas
/// une vue mais un tri (`state::SortBy::Favorites`).
const VIEWS: [ViewMode; 3] = [ViewMode::Grid, ViewMode::List, ViewMode::Mosaic];

/// Fenêtre glissante d'une liste : `(première ligne dessinée, fin exclue)`.
/// Le curseur reste au centre quand il le peut, sinon on colle au bord — sans
/// zone défilante, l'état de défilement ne peut pas être perdu.
fn fenetre(selected: usize, total: usize, visibles: usize) -> (usize, usize) {
    let visibles = visibles.max(1);
    if total <= visibles {
        return (0, total);
    }
    let top = selected.saturating_sub(visibles / 2).min(total - visibles);
    (top, top + visibles)
}

/// Nom affiché d'une vue (menu « type de liste »).
fn view_label(view: ViewMode) -> &'static str {
    match view {
        ViewMode::Grid => "Grille",
        ViewMode::List => "Liste",
        ViewMode::Mosaic => "Mosaïque",
    }
}

/// Légende d'en-tête de la galerie : filtre courant et volumétrie réelle
/// (affichés / préparés), pour ne pas confondre « 3 captures » et « 1200
/// vidéos » quand un filtre est actif.
fn galerie_legende(galerie: &Galerie) -> String {
    // volumétrie du jeu quand la galerie est filtrée : « 3 captures au total »
    // doit compter les captures DU JEU, pas celles du poste
    let perimetre: Vec<&gallery::Item> = galerie
        .tous
        .iter()
        .filter(|item| match (&galerie.jeu, item.titre.as_deref()) {
            (None, _) => true,
            (Some(nom), Some(titre)) => titre == nom,
            (Some(_), None) => false,
        })
        .collect();
    let images = perimetre
        .iter()
        .filter(|item| item.genre == gallery::Genre::Image)
        .count();
    let videos = perimetre.len().saturating_sub(images);
    let filtrees = match galerie.filtre {
        None => String::new(),
        Some(gallery::Genre::Image) => format!(" · {images} capture(s) au total"),
        Some(gallery::Genre::Video) => format!(" · {videos} vidéo(s) au total"),
    };
    let restant = if galerie.restant > 0 {
        format!(" · {} vignette(s) en préparation", galerie.restant)
    } else {
        String::new()
    };
    format!(
        "{} affichée(s) · {} chargée(s){filtrees}{restant}",
        galerie.items.len(),
        galerie.thumbs.len()
    )
}

/// Position d'une vue dans `VIEWS`.
fn view_index(view: ViewMode) -> usize {
    VIEWS.iter().position(|&v| v == view).unwrap_or(0)
}

/// Position dans le menu « année » : 0 = toutes les années, puis la liste
/// (déjà triée, la plus récente d'abord).
fn year_index(filter: Option<u32>, years: Vec<u32>) -> usize {
    filter
        .and_then(|year| years.iter().position(|&known| known == year))
        .map_or(0, |position| position + 1)
}

/// Écran du jeu, appliqué au démarrage de la session console : `plasma` suit
/// la disposition enregistrée par Plasma, `clone` réplique l'écran principal
/// sur les autres, `primary` éteint les autres. Le format et l'application
/// vivent dans `/usr/libexec/gablue-display` (une seule vérité).
const ECRANS: [(&str, &str); 3] = [
    ("plasma", "Comme Plasma"),
    ("clone", "Cloner"),
    ("primary", "Principal seul"),
];

/// Helper d'écran ; surchargeable pour le banc (absent de l'arbre de travail).
fn display_helper() -> PathBuf {
    std::env::var_os("GLAUNCHER_DISPLAY_HELPER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/libexec/gablue-display"))
}

/// Préférence enregistrée → index dans `ECRANS` (défaut : Plasma).
fn ecran_lu() -> usize {
    let sortie = std::process::Command::new(display_helper())
        .arg("status")
        .output();
    let cle = sortie
        .ok()
        .map(|sortie| String::from_utf8_lossy(&sortie.stdout).trim().to_string())
        .unwrap_or_default();
    ECRANS.iter().position(|(connue, _)| *connue == cle).unwrap_or(0)
}

/// Écrit la préférence (Helper : `gablue-display set <mode>`).
fn ecran_ecrit(index: usize) -> Result<(), String> {
    let (cle, _) = ECRANS[index.min(ECRANS.len() - 1)];
    match std::process::Command::new(display_helper())
        .args(["set", cle])
        .status()
    {
        Ok(statut) if statut.success() => Ok(()),
        Ok(statut) => Err(format!("code {}", statut.code().unwrap_or(-1))),
        Err(err) => Err(err.to_string()),
    }
}

/// Programme qui ouvre le clavier virtuel de l'overlay manette. Surchargeable
/// pour le banc (`GLAUNCHER_KBD_CMD`).
fn clavier_helper() -> String {
    std::env::var("GLAUNCHER_KBD_CMD").unwrap_or_else(|_| "gpad-clavier".to_string())
}

/// Galerie : captures d'écran, enregistrements et replays des deux dossiers de
/// `recorder.conf`, la plus récente en tête. Les vignettes arrivent du worker
/// de fond (`gallery::spawn`) et restent en mémoire pour la session.
struct Galerie {
    /// Tout ce que contiennent les dossiers (le filtre ne le relit pas).
    tous: Vec<gallery::Item>,
    /// Éléments affichés : `tous` filtré par `filtre`.
    items: Vec<gallery::Item>,
    /// `None` = tout, sinon captures ou vidéos (Triangle fait défiler).
    filtre: Option<gallery::Genre>,
    /// Jeu dont on ne montre que les captures et vidéos (bouton « Galerie » de
    /// sa page) ; `None` = la galerie complète. Comparé au **nom affiché**
    /// écrit dans le sidecar `<fichier>.jeu` au lancement, qui est le nom du
    /// paquet (`row.name`, la tige du `.wgp`/`.lgp`) — donc juste aussi pour
    /// un jeu non identifié sur SteamGridDB, dont c'est le seul nom.
    jeu: Option<String>,
    selected: usize,
    /// Effacement armé (index) : Croix efface, Rond ou un déplacement désarme.
    arme: Option<usize>,
    thumbs: std::collections::HashMap<PathBuf, Option<image::Handle>>,
    /// Chemins dont la vignette a déjà été demandée (le fil de vignettes ne
    /// traite que la fenêtre visible : une galerie de 2000 vidéos ne lance
    /// pas 2000 `ffmpeg`).
    prepared: HashSet<PathBuf>,
    /// Vignettes encore attendues (aucune roue pour ça : le compteur suffit).
    restant: usize,
}

/// Capture d'écran ouverte en plein écran (animation d'entrée).
struct Apercu {
    index: usize,
    /// Entrée de l'image : 1 → 0 en ~0,16 s (60 Hz). Elle n'est pas pilotée
    /// par le minuteur de la galerie (8 Hz) : à ce rythme il fallait une
    /// seconde entière pour la voir — « trop long » (constaté le 23/11).
    entree: f32,
}

/// Page « Réglages » du launcher (hors jeu) : la liste des dossiers de jeux.
///
/// Le retrait est **armé** avant d'agir : retirer `~/Games` de la liste n'est
/// pas réversible (même si le disque n'est jamais touché). Confirmation à la
/// **Croix** (bouton bas), annulation au **Rond** (bouton droite) — la
/// convention de tout le launcher.
struct Settings {
    /// Copie éditable de `config.game_dirs` ; écrite sur disque à chaque
    /// changement, en ne touchant qu'à la clé `game_dirs`.
    dossiers: Vec<String>,
    /// Ligne sélectionnée : 0 = « Ajouter un dossier », puis les dossiers.
    selected: usize,
    /// Index (dans `dossiers`) dont le retrait attend confirmation.
    confirm: Option<usize>,
    /// Navigateur de dossiers ouvert par-dessus la liste.
    browser: Option<Browser>,
    /// Dossier des enregistrements et des replays (`None` = défaut du démon).
    captures: Option<String>,
    /// Dossier des captures d'écran (`None` = défaut de `screenshot.sh`).
    shots: Option<String>,
    /// Musique de fond jouée (`music` de `config.toml`), réglable ici depuis
    /// le 22/11 : le launcher n'écrit que `music` et `music_volume`, jamais le
    /// reste du fichier.
    musique: bool,
    /// Volume de la musique en pourcentage (`music_volume`), par pas de 5.
    volume: u8,
    /// Écran du jeu (index dans `ECRANS`).
    ecran: usize,
    /// Scanne les disques externes montés (`scan_disks`).
    disques: bool,
}

/// Ce que le navigateur de dossiers choisit : un dossier de jeux (ajout à la
/// liste) ou un dossier de sortie (une seule valeur, écrite dans
/// `recorder.conf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choix {
    Jeux,
    Captures,
    Shots,
}

/// Navigateur de dossiers à la manette : ligne 0 = « choisir ce dossier »,
/// puis « .. » et les sous-dossiers. Aucune boîte de dialogue système, donc
/// utilisable au pad comme au clavier.
struct Browser {
    chemin: PathBuf,
    entrees: Vec<folders::Dossier>,
    selected: usize,
    /// Ce que la Croix fait en validant ce dossier.
    choix: Choix,
}

impl Settings {
    fn nouveau(config: &Config) -> Self {
        Self {
            dossiers: config.game_dirs.clone(),
            selected: 0,
            confirm: None,
            browser: None,
            captures: recorder::dossier(recorder::CLE_CAPTURES),
            shots: recorder::dossier(recorder::CLE_SHOTS),
            musique: config.music,
            volume: config.music_volume.min(100),
            ecran: ecran_lu(),
            disques: config.scan_disks,
        }
    }

    /// Ligne du dossier des enregistrements (après « ajouter » et les jeux).
    fn ligne_captures(&self) -> usize {
        self.dossiers.len() + 1
    }

    /// Ligne du dossier des captures d'écran.
    fn ligne_shots(&self) -> usize {
        self.dossiers.len() + 2
    }

    /// Ligne de la musique de fond (jouée / coupée).
    fn ligne_musique(&self) -> usize {
        self.dossiers.len() + 3
    }

    /// Ligne du volume de la musique.
    fn ligne_volume(&self) -> usize {
        self.dossiers.len() + 4
    }

    /// Ligne de la préférence d'écran (après « ajouter » et les dossiers).
    fn ligne_ecran(&self) -> usize {
        self.dossiers.len() + 5
    }

    /// Ligne « Oublier les jeux disparus », dernière de la page (entretien).
    fn ligne_oublier(&self) -> usize {
        self.dossiers.len() + 6
    }

    /// Nombre de lignes de la liste (« ajouter », les dossiers, les deux
    /// dossiers de sortie, la musique et son volume, l'écran, l'entretien).
    /// Ligne « Scanner les disques externes » (entretien).
    fn ligne_disques(&self) -> usize {
        self.dossiers.len() + 7
    }

    fn lignes(&self) -> usize {
        self.dossiers.len() + 8
    }

    /// Clé de `recorder.conf` d'une ligne de sortie, si c'en est une.
    fn cle_sortie(&self, ligne: usize) -> Option<&'static str> {
        if ligne == self.ligne_captures() {
            Some(recorder::CLE_CAPTURES)
        } else if ligne == self.ligne_shots() {
            Some(recorder::CLE_SHOTS)
        } else {
            None
        }
    }
}

pub struct App {
    config: Config,
    /// Emplacement de `config.toml`, réécrit quand les dossiers changent.
    config_path: PathBuf,
    /// Page Réglages ouverte (aucune en jeu : le launcher est alors caché).
    settings: Option<Settings>,
    /// Poignée de la surveillance des dossiers (liste modifiable à chaud).
    watch: Option<crate::watch::Veille>,
    games: Vec<Game>,
    selected: usize,
    /// Nom du jeu en cours d'exécution (fenêtre noire) ; `None` = launcher.
    running: Option<String>,
    /// Slug du jeu lancé, pour le temps de jeu (rempli au lancement effectif).
    pending_slug: Option<String>,
    /// Nom partagé de ses visuels ([`covers::key`]), publié dans `running.txt`
    /// pour l'overlay manette (logo et icône).
    pending_key: Option<String>,
    /// Changement de taille des vignettes en cours (`+`/`−`, L2/R2) : les
    /// vignettes glissent de leur ancienne case vers la nouvelle.
    zoom: Option<Zoom>,
    /// Format du jeu en cours de lancement (fichier du temps de jeu).
    pending_kind: Option<library::Kind>,
    /// Départ du chrono du jeu en cours (slug, format, instant).
    play_started: Option<(String, library::Kind, std::time::Instant)>,
    /// Temps de jeu cumulé, par slug.
    playtime: std::collections::HashMap<String, u64>,
    /// Fichiers du temps de jeu, dans les dossiers de sauvegardes.
    playtime_paths: playtime::Paths,
    /// Années de sortie connues, par slug (`None` = regardé, SGDB n'en donne
    /// pas). Chargées depuis `assets/years.txt`, le fichier partagé.
    years: BTreeMap<String, Option<u32>>,
    /// Ordre de la bibliothèque (A→Z par défaut).
    sort: SortBy,
    /// Menu « type de liste » (triangle / Y) : index choisi.
    view_menu: Option<usize>,
    /// Menu « tri » (carré / X, `T`) : index choisi dans `SortBy::ALL`, plus
    /// une dernière ligne « Année » qui ouvre `year_menu`.
    sort_menu: Option<usize>,
    /// Menu « année » (filtre sur la date de sortie) : index choisi, 0 = toutes.
    year_menu: Option<usize>,
    /// Filtre d'année actif (None = toutes les années).
    year_filter: Option<u32>,
    status: Option<(String, bool)>,
    /// Mise à jour en cours (ou la dernière) : sa barre et son libellé vivent
    /// dans le pied de page.
    update: Option<updates::Update>,
    notice: Option<String>,
    pad: Option<PadKind>,
    /// Toutes les manettes connectées : en débrancher une ne doit pas effacer
    /// l'indicateur des autres.
    pads: Vec<(PadKind, String)>,
    /// Dernier appui sur « retour » : deux appuis rapprochés quittent.
    back_press: Option<std::time::Instant>,
    /// L'invite « appuie encore sur retour » est affichée.
    quit_hint: bool,
    /// Fondu d'apparition des menus et de la galerie (1 = noir, 0 = fini) :
    /// un voile noir AU-DESSUS de la couche `menus`, qui se dissipe en 0,18 s
    /// à 60 Hz. iced 0.14 n'a pas d'opacité de sous-arbre : c'est le seul
    /// moyen d'un vrai fondu sans toucher chaque couleur de panneau.
    overlay_fade: f32,
    /// Signature des couches visibles au message précédent : tout changement
    /// (ouverture, remplacement, fermeture) relance `overlay_fade`.
    overlay_vu: u64,
    glyphs: Glyphs,
    window: Option<window::Id>,
    fullscreen: bool,
    /// Session console Gablue (KWin nu) ou bureau : en console, le launcher
    /// ne s'auto-ferme pas, la sortie passe par le menu système de `gpad`.
    mode: Mode,
    /// Visuels mémorisés par slug (pas de relecture à chaque réindexation).
    artwork: ArtworkCache,
    /// Index SQLite des paquets, partagé avec les threads de rafraîchissement.
    index: Arc<Mutex<Option<Index>>>,
    /// Un rafraîchissement tourne déjà ; les chemins suivants sont mis en attente.
    refreshing: bool,
    pending: Vec<PathBuf>,
    /// Une réindexation en attente doit aussi relancer la découverte des
    /// disques (branchement survenu pendant un scan).
    pending_disques: bool,
    /// Résultat mis de côté le temps du fondu (`Swap::Reload`).
    pending_reload: Option<Refresh>,
    /// Choix de la destination d'une copie (page d'un jeu).
    copy_menu: Option<CopyMenu>,
    /// « le fichier existe en plus ancien : le mettre à jour ? »
    copy_replace: Option<CopyReplace>,
    /// Copies en cours (file + progression cumulée) : la barre du haut.
    copies: Option<Copies>,
    /// Portée de la bibliothèque : tous, dossiers choisis, disques scannés.
    scope: Scope,
    /// État du lien réseau, affiché en icône (coin haut gauche).
    reseau: Reseau,
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
    /// Fondu propre à la page d'un jeu : elle apparaît depuis le noir et y
    /// retourne quand on la ferme (1 = noir). Séparé de `fade` pour ne pas
    /// interférer avec le fondu du lancement.
    page_fade: f32,
    page_fade_target: f32,
    /// La page attend son basculement : le noir doit être complet avant qu'elle
    /// entre ou sorte de l'arbre (`PageStep::Open` / `PageStep::Close`).
    pending_page: Option<PageStep>,
    /// Bascule de liste en cours (vue, tri, filtre) : opacité du fond et cible.
    swap_fade: f32,
    swap_fade_target: f32,
    pending_swap: Option<Swap>,
    /// Progression de la bande héros de la page (0 = rien, 1 = posée) : quand
    /// le héros arrive après l'ouverture, la bande grandit et l'image apparaît
    /// en fondu au lieu de faire sauter le bloc du bas.
    page_hero: f32,
    /// Même chose pour le logo et la cover, qui n'ont pas de place à réserver :
    /// seule leur opacité monte (`page_view`).
    page_logo: f32,
    page_cover: f32,
    /// Retour de partie en cours : le voile du lancement s'éclaircit et chaque
    /// vignette revient de son côté de l'écran en dézoomant (`comeback`).
    returning: bool,
    /// Lancement à effectuer dès que l'écran est noir (nom, commande, début du fondu).
    to_launch: Option<(String, launch::Launch, std::time::Instant)>,
    /// Musique de fond (`music = false` dans la config, ou `mpv` absent : aucune).
    music: Option<music::Music>,
    /// Galerie des captures et vidéos (ouvre le menu des vues si `None`).
    galerie: Option<Galerie>,
    /// Combien de captures/vidéos par jeu, compté **en fond** : la page d'un
    /// jeu n'a pas à scanner les dossiers de sortie (des milliers de fichiers)
    /// pour savoir s'il faut y poser le bouton « Galerie ».
    galerie_comptes: std::collections::HashMap<String, usize>,
    /// Dernier comptage demandé (`None` : jamais) — un scan à la demande, pas
    /// à chaque page ouverte.
    galerie_comptes_at: Option<std::time::Instant>,
    /// Capture ouverte en plein écran (par-dessus la galerie).
    apercu: Option<Apercu>,
    /// Lecture vidéo en cours : `mpv` plein écran, piloté au pad.
    lecteur: Option<play::Lecteur>,
    /// Vidéo à lancer dès que le voile est noir (même mécanisme que le jeu).
    a_lancer_video: Option<(PathBuf, String, std::time::Instant)>,
    /// Voile noir de l'ouverture d'un film **sans** l'hyperespace des
    /// étoiles : il couvre l'animation d'ouverture de la fenêtre de mpv
    /// (qui « venait de bas à droite ») puis s'efface en ~0,18 s.
    video_veil: f32,
    video_veil_target: f32,
    /// Instant où le voile peut se lever (mpv a eu le temps de s'ouvrir).
    video_lift_at: Option<std::time::Instant>,
    /// Défilement animé vers la sélection.
    scroll_id: Id,
    scroll: f32,
    scroll_target: f32,
    /// Maximum de défilement du contenu courant, tel que le `scrollable` d'iced
    /// le rapporte (`content - viewport`). `0` = pas encore connu : sans lui,
    /// `scroll_drawn` ne borne rien. Voir `scroll_drawn` pour la raison d'être.
    scroll_max: f32,
    /// Menu « choisir une image » pour le jeu sélectionné.
    picker: Option<Picker>,
    /// Image choisie dont le téléchargement est en cours : le menu se referme
    /// tout de suite (il ne doit pas rester ouvert pendant le réseau).
    picked: Option<(String, covers::ArtKind)>,
    /// Prompt « nom de recherche » de l'identification (texte en cours de
    /// frappe).
    identify: Option<String>,
    /// Menu d'identification : jeux proposés par SteamGridDB.
    identify_menu: Option<IdentifyMenu>,
    /// Menu « reprendre les visuels d'un autre jeu » (alias visuel).
    alias_menu: Option<AliasMenu>,
    /// Jeu identifié qui a **déjà** des visuels : question « les remplacer ? »
    /// avant de les effacer (par défaut sur « Non »).
    replace: Option<Replace>,
    /// Dialogue « Oublier les jeux disparus ».
    oublier: Option<Oublier>,
    /// Identifications retenues, par jeu (`assets/identities.txt`, partagé
    /// par Syncthing) : id SteamGridDB exact, pour que ce jeu ne retombe pas
    /// sur le premier résultat de l'autocomplete.
    identities: BTreeMap<String, covers::Identity>,
    /// Jeux marqués « introuvable sur SteamGridDB », datés (`assets/absents.txt`,
    /// partagé). Une marque fraîche évite une recherche à chaque démarrage ;
    /// elle expire (30 jours) et se lève dès qu'une identification réussit.
    absents: BTreeMap<String, covers::Absent>,
    /// Prompt « date de sortie » (année tapée à la main).
    date: Option<String>,
    /// L'utilisateur a demandé la date à SteamGridDB pour ce slug : le
    /// résultat est annoncé dans le pied de page.
    asked_year: Option<String>,
    /// Identifiant du champ de saisie, pour lui donner le focus.
    input_id: Id,
    /// Page du jeu (héros, logo, actions) ; `None` = liste des jeux.
    page: Option<Page>,
    /// Menu « Options » de la page : index dans `ART_MENU`.
    art_menu: Option<usize>,
    /// Slugs marqués ★ (fichier `favorites.txt`).
    favorites: HashSet<String>,
    /// jeux lancés, du plus récent au plus ancien (ordre « Derniers lancés »)
    recent: Vec<(String, u64)>,
    /// recherche par nom : texte saisi, et champ ouvert au clavier
    search: String,
    search_open: bool,
    /// taille des lignes de la vue LISTE (0 compact, 1 normal, 2 grand)
    list_size: u8,
    /// Barre de défilement visible : seulement quand on se sert de la souris
    /// ou du clavier. À la manette, on ne peut pas la saisir et elle ne dit
    /// rien de plus que la position dans la liste — c'est du bruit.
    barre: bool,
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
    /// Rotation de la roue de chargement (radians).
    spin: f32,
    /// Titre sélectionné qui défile : temps écoulé depuis la sélection (s) et
    /// slug du jeu concerné, pour repartir de zéro sur un autre titre.
    title_elapsed: f32,
    title_game: Option<String>,
    /// Poussière d'étoiles du fond (particules scintillantes, `sparkles = true`).
    sparkles: sparkles::Sparkles,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn boot(
        config: Config,
        config_path: PathBuf,
        notice: Option<String>,
        autoplay: Option<usize>,
        open_picker: bool,
        mode: Mode,
    ) -> (Self, Task<Message>) {
        // ancienne disposition (covers/, icons/ à la racine) → assets/
        config.migrate_cache();
        // visuels déjà là, renommés d'après le JEU SteamGridDB (et non d'après
        // le paquet) : deux postes dont les `.wgp`/`.lgp` portent des noms
        // différents partagent alors les mêmes fichiers par Syncthing
        let renommes = covers::migrate_names(
            &config.assets_dir(),
            &config.cache_dir().join("marks"),
        );
        if renommes > 0 {
            eprintln!("glauncher : {renommes} visuel(s) renommé(s) d'après le jeu");
        }
        // identifications partagées (même dossier Syncthing que les visuels) :
        // elles nomment les fichiers, donc lues AVANT la bibliothèque
        let identities = covers::load_identities(&covers::identities_path(&config.assets_dir()));
        let absents = covers::load_absents(&covers::absents_path(&config.assets_dir()));
        let (bus, receiver) = unbounded();
        let _ = BUS.set(bus);
        if let Ok(mut slot) = BUS_RX.lock() {
            *slot = Some(receiver);
        }
        crate::gamepad::spawn(|event| post(Message::Pad(event)));

        // surveillance des seuls dossiers de jeux (jamais le cache : boucle)
        let veille =
            crate::watch::spawn(config.scan_dirs(), |paths| post(Message::Changed(paths)));
        // Un disque branché n'ajoute pas de dossier à la liste surveillée tant
        // que la découverte n'a pas tourné : un petit guetteur signale les
        // changements de l'ensemble des disques montés (branchement, retrait).
        crate::disks::surveiller(|| post(Message::Disks));
        // réseau : relève de /sys toutes les 3 s, message au changement
        status::surveiller(|etat| post(Message::Reseau(etat)));

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
        // l'interface est réveillée au début de chaque téléchargement (roue de
        // chargement) — sans ça elle ne redessinerait qu'à la fin de la requête
        covers::wake_with(|| post(Message::Activity));
        let assets = assets::spawn(
            config.icon_dirs(),
            config.pack_icon_dir(),
            |event| match event {
                assets::Event::Icon { slug, icon } => post(Message::LocalIcon { slug, icon }),
            },
        );

        // index persistant : la liste s'affiche sans relire les 540 dossiers
        let index = Arc::new(Mutex::new(Index::open(&config.index_path()).ok()));
        let mut artwork = ArtworkCache::new();
        let rows = index
            .lock()
            .ok()
            .and_then(|db| db.as_ref().and_then(|db| db.all().ok()))
            .unwrap_or_default();
        let all_games = library::from_rows(&config, &rows, &mut artwork, &[], &identities);
        let favorites = favorites::load(&config.favorites_path());
        let recent = recent::load(&recent::path(&config.cache_dir()));
        // temps de jeu : avec les sauvegardes (lgp-saves / LocalSavesWGP).
        // L'ancien fichier du cache est rangé ici, une fois la liste connue,
        // pour que le format de chaque jeu soit connu.
        let playtime_paths = playtime::Paths {
            lgp: config.playtime_path(library::Kind::Lgp.as_str()),
            wgp: config.playtime_path(library::Kind::Wgp.as_str()),
            legacy: config.legacy_playtime_path(),
        };
        let playtime_games: Vec<(String, &'static str)> = all_games
            .iter()
            .map(|game| (game.slug.clone(), game.kind.as_str()))
            .collect();
        playtime::migrate(&playtime_paths, &playtime_games);
        let playtime = playtime::load(&playtime_paths);
        let fullscreen = config.fullscreen;
        // dernier type de vue et dernier tri choisis : ils survivent à la
        // fermeture (le fichier d'état est dans le cache, jamais dans
        // `config.toml` qui appartient à gablue-manager)
        let saved = state::load(&config.state_path());
        let mut config = config;
        if let Some(view) = saved.view {
            config.view = view;
        }
        // capturé AVANT que `config` n'entre dans `App` (le Task part après)
        let scan_disks = config.scan_disks;
        let mut app = Self {
            config,
            config_path,
            settings: None,
            watch: Some(veille),
            games: Vec::new(),
            selected: 0,
            running: None,
            status: None,
            update: None,
            notice,
            pad: None,
            pads: Vec::new(),
            back_press: None,
            quit_hint: false,
            overlay_fade: 0.0,
            overlay_vu: 0,
            glyphs: Glyphs::for_kind(PadKind::Xbox),
            window: None,
            fullscreen,
            mode,
            artwork,
            index,
            refreshing: false,
            pending: Vec::new(),
            pending_disques: false,
            pending_reload: None,
            copy_menu: None,
            copy_replace: None,
            copies: None,
            scope: saved.scope,
            reseau: status::reseau(),
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
            page_fade: 0.0,
            page_fade_target: 0.0,
            pending_page: None,
            swap_fade: 0.0,
            swap_fade_target: 0.0,
            pending_swap: None,
            page_hero: 0.0,
            page_logo: 0.0,
            page_cover: 0.0,
            returning: false,
            to_launch: None,
            music: None,
            galerie: None,
            galerie_comptes: std::collections::HashMap::new(),
            galerie_comptes_at: None,
            apercu: None,
            lecteur: None,
            a_lancer_video: None,
            video_veil: 0.0,
            video_veil_target: 0.0,
            video_lift_at: None,
            pending_slug: None,
            pending_key: None,
            zoom: None,
            pending_kind: None,
            play_started: None,
            playtime,
            playtime_paths,
            years: BTreeMap::new(),
            sort: saved.sort,
            list_size: saved.list_size.min(2),
            view_menu: None,
            sort_menu: None,
            year_menu: None,
            year_filter: saved.year,
            scroll_id: Id::unique(),
            scroll: 0.0,
            scroll_target: 0.0,
            scroll_max: 0.0,
            last_tick: std::time::Instant::now(),
            spin: 0.0,
            title_elapsed: 0.0,
            title_game: None,
            picker: None,
            picked: None,
            identify: None,
            identify_menu: None,
            alias_menu: None,
            replace: None,
            oublier: None,
            identities,
            absents,
            date: None,
            asked_year: None,
            input_id: Id::unique(),
            page: None,
            art_menu: None,
            favorites,
            recent,
            search: String::new(),
            search_open: false,
            barre: false,
            all_games,
            active: true,
            input: Input::Keyboard,
            sample: image::Handle::from_bytes(COVER_SAMPLE),
            star: image::Handle::from_bytes(STAR),
            sparkles: sparkles::Sparkles::new(),
        };
        // années déjà connues (fichier partagé dans `assets/`)
        app.years = covers::load_years(&covers::years_path(&app.config.assets_dir()));
        app.apply_filter();
        // captures et vidéos par jeu : comptées en fond dès le départ, pour que
        // la première page ouverte sache déjà si elle a un bouton « Galerie »
        app.rafraichir_comptes_galerie(true);
        // musique de fond : lecture par `mpv` dans un fil à part, l'UI
        // n'attend rien (et sans `mpv` le launcher reste simplement muet)
        // volume 0 = muet : inutile de lancer `mpv` pour rien
        if app.config.music && app.config.music_volume > 0 {
            app.music = music::Music::start(&app.config.cache_dir(), app.config.music_volume);
        }
        // poussière d'étoiles : le minuteur n'existe pas si c'est coupé, donc
        // aucune image n'est demandée au moteur pour rien
        if app.config.sparkles {
            sparkles::spawn_ticker();
        }
        // pointeur : la relève existe toujours (fil endormi), elle ne réveille
        // l'UI que quand `subscription` l'arme
        cursor::spawn_ticker();
        // galerie : minuteur de lecture vidéo et de fondu d'aperçu (dort au
        // repos, la galerie fermée)
        gallery::spawn_ticker();
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
        // crochet de banc : ouvre la page Réglages (puis le navigateur) au
        // démarrage, pour la photographier sans clavier ni manette
        match std::env::var("GLAUNCHER_SETTINGS_DEMO").as_deref() {
            Ok("1") => tasks.push(Task::done(Message::SettingsOpen)),
            Ok("browser") => {
                tasks.push(Task::done(Message::SettingsOpen));
                // ligne 0 = « ajouter » : ouvre le navigateur sur $HOME
                tasks.push(Task::done(Message::SettingsAct));
            }
            // ligne « Son » : musique et son volume (banc, bac à sable)
            Ok("son") => {
                tasks.push(Task::done(Message::SettingsOpen));
                tasks.push(Task::done(Message::SettingsNav(3)));
            }
            _ => {}
        }
        // crochet de banc : ouvre la galerie au démarrage, pour la
        // photographier sans manette (dossiers lus dans recorder.conf)
        if std::env::var_os("GLAUNCHER_GALLERY_DEMO").is_some() {
            tasks.push(Task::done(Message::GalleryOpen(None)));
            // valeurs `apercu` / `video` : ouvre aussi l'élément du genre
            if matches!(
                std::env::var("GLAUNCHER_GALLERY_DEMO").as_deref(),
                Ok("apercu") | Ok("video")
            ) {
                tasks.push(Task::done(Message::GalleryAct));
            }
        }
        // crochet de banc : page d'un jeu (index) et, avec
        // `GLAUNCHER_OPTIONS_DEMO`, son menu « Options » (réglages de
        // lancement : gamescope, résolution, mode manette Xbox)
        if let Ok(index) = std::env::var("GLAUNCHER_PAGE_DEMO") {
            let index = index.parse::<usize>().unwrap_or(0);
            if std::env::var_os("GLAUNCHER_OPTIONS_DEMO").is_some() {
                // `OpenPage` remet `art_menu` à zéro : on pose directement la
                // bascule différée, puis le menu (la page s'installe au
                // sommet du fondu, sans passer par un message qui l'efface)
                app.pending_page = Some(PageStep::Open(index));
                app.page_fade_target = 1.0;
                app.art_menu = Some(0);
            } else {
                tasks.push(Task::done(Message::OpenPage(index)));
            }
        }
        if std::env::var_os("GLAUNCHER_UPDATE_DEMO").is_some() {
            // crochet de banc : lance la mise à jour au démarrage (avec un
            // `GLAUNCHER_UPDATE_CMD` factice) pour regarder la barre sans
            // toucher au clavier ni au système
            tasks.push(Task::done(Message::UpdateApply));
        }

        // crochet de banc : choix de la destination d'une copie (avec
        // `GLAUNCHER_PAGE_DEMO` pour avoir une page ouverte), et `=go` pour
        // lancer la copie vers la première destination (téléchargements)
        match std::env::var("GLAUNCHER_COPY_DEMO").as_deref() {
            // `=go` : une copie vers la première destination (téléchargements)
            Ok("go") => {
                tasks.push(Task::done(Message::CopyOpen));
                tasks.push(Task::done(Message::CopyChoose(0)));
            }
            // `=file` : deux copies d'affilée (deux jeux différents), pour voir
            // la progression cumulée de la file
            Ok("file") => {
                tasks.push(Task::done(Message::CopyOpen));
                tasks.push(Task::done(Message::CopyChoose(0)));
                tasks.push(Task::done(Message::CardClicked(1)));
                tasks.push(Task::done(Message::CopyOpen));
                tasks.push(Task::done(Message::CopyChoose(0)));
            }
            Ok(_) => tasks.push(Task::done(Message::CopyOpen)),
            Err(_) => {}
        }

        // Disques externes : la découverte tourne tout de suite (le veilleur
        // inotify, lui, met 5 s à se poser). Opt-in : sans `scan_disks`, aucun
        // accès aux disques avant la première image.
        if scan_disks {
            tasks.push(Task::done(Message::Disks));
        }

        crate::stamp("préparation");
        (app, Task::batch(tasks))
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Poussière d'étoiles : elle ne tourne que si elle est réellement
        // visible. C'est le SEUL poste de dépense de l'animation (une image
        // à 20 Hz), donc on le coupe dès que l'écran est occupé autrement :
        // fenêtre sans focus, menu, prompt, écran noir. La page d'un jeu GARDE
        // les étoiles (demandé le 20/11) : elles y sont dessinées au-dessus du
        // contenu, comme sur la liste.
        // Le fondu du lancement n'a pas besoin de ce minuteur : `window::frames`
        // reconstruit déjà la vue à 60 Hz pour le fondu, et l'« hyperespace »
        // n'est piloté que par `fade` (les particules ne dérivent pas pendant
        // ces 0,9 s) — d'où `to_launch` dans la condition.
        let sparkles_running = self.config.sparkles
            && self.active
            && self.fade < 0.999
            && self.view_menu.is_none()
            && self.sort_menu.is_none()
            && self.year_menu.is_none()
            && self.art_menu.is_none()
            && self.picker.is_none()
            && self.identify.is_none()
            && self.identify_menu.is_none()
            && self.replace.is_none()
            && self.date.is_none()
            && self.to_launch.is_none()
            // respiration : ce sont les images de la fenêtre qui la cadence
            // (60 Hz, `breath_pulses` plus bas) ; le minuteur à 20 Hz
            // s'arrête, sinon deux horloges font avancer le même temps
            && !self.breath_pulses();
        sparkles::set_running(sparkles_running);
        let animating = self.animating();
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
        // Pointeur au repos : iced ne recalcule l'interaction (`Hidden`) que
        // pendant une passe de mise à jour, jamais tout seul — sans relève, le
        // pointeur ne se cacherait qu'au prochain évènement (voir `cursor`).
        // La relève ne demande aucune image : elle ne sert qu'à donner l'heure,
        // donc elle n'est armée que si rien d'autre ne redessine déjà.
        cursor::set_ticking(self.active && !animating && !sparkles_running);
        if animating {
            subscriptions.push(window::frames().map(|_| Message::Tick));
        }
        // Galerie : une lecture vidéo tourne (il faut voir quand mpv sort).
        // Minuteur à 8 Hz, comme la poussière et le pointeur : au repos, le
        // fil dort. L'entrée de l'aperçu, elle, est à 60 Hz (`animating`).
        gallery::set_ticking(self.lecteur.is_some());
        Subscription::batch(subscriptions)
    }

    /// Une animation tourne-t-elle ? (sinon aucun abonnement aux images)
    fn animating(&self) -> bool {
        (self.fade - self.fade_target).abs() > 0.001
            || (self.page_fade - self.page_fade_target).abs() > 0.001
            || (self.swap_fade - self.swap_fade_target).abs() > 0.001
            || (self.scroll - self.scroll_target).abs() > 0.5
            || self.page_assets_animating()
            || self.to_launch.is_some()
            || self.a_lancer_video.is_some()
            || self.video_lift_at.is_some()
            || (self.video_veil - self.video_veil_target).abs() > 0.001
            || self.apercu.as_ref().is_some_and(|apercu| apercu.entree > 0.0)
            || self.title_scrolls()
            || self.loading()
            || self.scanning()
            // respiration de la vignette sélectionnée : continue, donc à la
            // cadence de la fenêtre (voir `subscription`)
            || self.breath_pulses()
            // changement de taille : chaque vignette glisse vers sa place
            || self.zoom.is_some()
            // aperçu d'une capture : entrée courte (pilotée par `tick`)
            || self.apercu.as_ref().is_some_and(|apercu| apercu.entree > 0.0)
            || self.a_lancer_video.is_some()
            // fondu d'apparition des menus / galerie
            || self.overlay_fade > 0.001
    }

    /// Téléchargements en cours (ou tout juste finis) : la roue de chargement
    /// est visible et doit tourner. Lue aussi par `animating`, sinon plus
    /// aucune image ne serait demandée au moteur et la roue resterait figée.
    fn loading(&self) -> bool {
        covers::busy()
    }

    /// Réindexation en cours (scan des dossiers ou d'un disque branché) : la
    /// seconde roue, libellée « Scan », tourne à côté de celle des visuels.
    fn scanning(&self) -> bool {
        self.refreshing
    }

    /// Le titre de la vignette sélectionnée défile-t-il ? (nom plus long que ce
    /// qui tient dans la vignette). Sert à demander des images au moteur — au
    /// repos, aucun défilement, donc aucune dépense.
    fn title_scrolls(&self) -> bool {
        if !self.active
            || self.page.is_some()
            || !matches!(self.config.view, ViewMode::Grid)
            || self.view_menu.is_some()
            || self.sort_menu.is_some()
            || self.year_menu.is_some()
            || self.art_menu.is_some()
            || self.picker.is_some()
            || self.identify.is_some()
            || self.identify_menu.is_some()
            || self.date.is_some()
        {
            return false;
        }
        let Some(game) = self.games.get(self.selected) else {
            return false;
        };
        game.name.chars().count() > self.title_budget()
    }

    /// Nombre de caractères qui tiennent dans une vignette. Volontairement
    /// **pessimiste** (0,62 em par caractère, marges déduites) : mieux vaut
    /// animer un titre qui tenait (le `Marquee` le centre alors, sans
    /// défilement) que laisser figé un titre qui déborde en pixels.
    fn title_budget(&self) -> usize {
        let (cell, _, _) = self.metrics();
        let size = if cell >= 220.0 { 17.0 } else { 15.5 };
        let available = (cell - 2.0 * self.cell_inset() - 2.0 * PADDING).max(40.0);
        (available / (size * 0.62)) as usize
    }

    /// Le jeu de la page ouverte, s'il y en a une.
    /// Jeu dont on parle : celui de la page ouverte, sinon celui qui est
    /// sélectionné dans la liste (le crochet de banc `GLAUNCHER_COPY_DEMO`
    /// travaille sans page).
    fn jeu_courant(&self) -> Option<&Game> {
        self.page_game().or_else(|| self.games.get(self.selected))
    }

    fn page_game(&self) -> Option<&Game> {
        self.page
            .as_ref()
            .and_then(|page| self.games.get(page.index))
    }

    /// Jeu concerné par les menus : celui de la page ouverte, celui du menu
    /// d'images, sinon la sélection de la liste.
    fn current_slug(&self) -> Option<String> {
        self.page_game()
            .map(|game| game.slug.clone())
            .or_else(|| self.picker.as_ref().map(|picker| picker.slug.clone()))
            .or_else(|| self.games.get(self.selected).map(|game| game.slug.clone()))
    }

    /// Nombre de colonnes de la galerie, calculé sur la largeur de la fenêtre
    /// (mêmes marges que la page, tuiles de 240 px).
    fn gallery_columns(&self) -> usize {
        // taille FIXE (demandé le 23/11 : le zoom était une mauvaise idée)
        galerie_geometrie(self.viewport.0).0
    }

    /// Applique le filtre courant en gardant la sélection sur le même
    /// fichier quand il reste visible.
    /// Demande le comptage des captures/vidéos par jeu, au plus toutes les
    /// [`COMPTES_GALERIE_TTL`] secondes (ou tout de suite avec `force`).
    fn rafraichir_comptes_galerie(&mut self, force: bool) {
        const COMPTES_GALERIE_TTL: Duration = Duration::from_secs(20);
        let recent = self
            .galerie_comptes_at
            .is_some_and(|instant| instant.elapsed() < COMPTES_GALERIE_TTL);
        if recent && !force {
            return;
        }
        // daté tout de suite : une rafale d'ouvertures de page ne lance pas
        // un scan par page
        self.galerie_comptes_at = Some(std::time::Instant::now());
        spawn_comptes_galerie();
    }

    fn gallery_apply_filter(&mut self) {
        let Some(galerie) = self.galerie.as_mut() else {
            return;
        };
        let courant = galerie
            .items
            .get(galerie.selected)
            .map(|item| item.path.clone());
        let filtre = galerie.filtre;
        let jeu = galerie.jeu.clone();
        galerie.items = galerie
            .tous
            .iter()
            .filter(|item| filtre.is_none_or(|genre| item.genre == genre))
            .filter(|item| match (&jeu, item.titre.as_deref()) {
                (None, _) => true,
                (Some(nom), Some(titre)) => titre == nom,
                (Some(_), None) => false,
            })
            .cloned()
            .collect();
        galerie.selected = courant
            .and_then(|path| galerie.items.iter().position(|item| item.path == path))
            .unwrap_or(0)
            .min(galerie.items.len().saturating_sub(1));
        galerie.arme = None;
    }

    /// Demande les vignettes de la fenêtre visible (plus un rang de marge) :
    /// une galerie de milliers de vidéos ne lance pas mille `ffmpeg`.
    fn gallery_prepare(&mut self) {
        let cache = self.config.cache_dir().join("gallery");
        let colonnes = self.gallery_columns().max(1);
        let pas = galerie_geometrie(self.viewport.0).3;
        let lignes = (((self.viewport.1 - GALERIE_CHROME) / pas).floor() as usize).max(1);
        let Some(galerie) = self.galerie.as_mut() else {
            return;
        };
        let total_lignes = galerie.items.len().div_ceil(colonnes).max(1);
        let (haut, bas) = fenetre(galerie.selected / colonnes, total_lignes, lignes);
        let debut = haut.saturating_sub(1) * colonnes;
        let fin = ((bas + 1) * colonnes).min(galerie.items.len());
        let lot: Vec<gallery::Item> = galerie.items[debut.min(fin)..fin]
            .iter()
            .filter(|item| !galerie.prepared.contains(&item.path))
            .cloned()
            .collect();
        for item in &lot {
            galerie.prepared.insert(item.path.clone());
        }
        galerie.restant += lot.len();
        if lot.is_empty() {
            return;
        }
        gallery::spawn(lot, cache, |event| match event {
            gallery::Event::Thumb { path, bytes } => {
                post(Message::GalleryThumb { path, bytes });
            }
            gallery::Event::Done => {}
        });
    }

    /// Efface l'élément `index` de la galerie. Le fichier disparaît du disque :
    /// l'action n'est atteignable qu'après un appui sur Carré puis la Croix,
    /// et un déplacement désarme (on ne confirme que ce qu'on voit).
    /// Retire l'élément de la galerie en le **mettant à la corbeille** (jamais
    /// d'effacement définitif). Le sidecar `.jeu` suit, sinon il étiquetterait
    /// une future capture du même nom.
    fn gallery_delete(&mut self, index: usize) {
        let Some(galerie) = self.galerie.as_mut() else {
            return;
        };
        let Some(item) = galerie.items.get(index).cloned() else {
            return;
        };
        let sidecar = PathBuf::from(format!("{}.jeu", item.path.display()));
        match gallery::mettre_a_la_corbeille(&item.path) {
            Ok(corbeille) => {
                if sidecar.is_file() {
                    let _ = gallery::mettre_a_la_corbeille(&sidecar);
                }
                galerie.items.remove(index);
                galerie.tous.retain(|autre| autre.path != item.path);
                galerie.prepared.remove(&item.path);
                galerie.thumbs.remove(&item.path);
                galerie.selected = galerie.selected.min(galerie.items.len().saturating_sub(1));
                self.status = Some((
                    format!("{} dans la corbeille", item.nom()),
                    false,
                ));
                println!(
                    "GALERIE corbeille {} -> {}",
                    item.path.display(),
                    corbeille.display()
                );
            }
            Err(err) => {
                self.status = Some((format!("corbeille impossible ({err})"), true));
            }
        }
    }

    /// Session console : ouvre le clavier virtuel de l'overlay, qui frappe par
    /// uinput dans la fenêtre active (le prompt garde le focus clavier). Au
    /// bureau, il y a un vrai clavier : rien à faire.
    fn clavier_console(&mut self) {
        if !self.mode.is_console() {
            return;
        }
        let helper = clavier_helper();
        match std::process::Command::new(&helper)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => println!("glauncher : clavier virtuel ouvert ({helper})"),
            Err(err) => {
                self.status = Some((format!("clavier virtuel indisponible ({err})"), true))
            }
        }
    }

    /// Nom et id SteamGridDB d'un jeu : l'identification partagée fait foi,
    /// sinon le nom du paquet (comportement historique).
    fn who(&self, slug: &str, name: &str) -> (String, Option<u64>) {
        match self.identities.get(slug) {
            Some(identity) if !identity.name.is_empty() => {
                (identity.name.clone(), Some(identity.id))
            }
            Some(identity) => (name.to_string(), Some(identity.id)),
            None => (name.to_string(), None),
        }
    }

    /// Nom des fichiers de ce jeu dans `assets/` partagé ([`covers::key`]) :
    /// le jeu SteamGridDB quand il est identifié, sinon le paquet.
    fn key_of(&self, slug: &str) -> String {
        covers::key_of(&self.identities, slug)
    }

    /// Retient l'identification choisie dans la liste, écrit le fichier
    /// partagé (`assets/identities.txt`), puis met les visuels à jour — en
    /// demandant d'abord quand le paquet en a déjà (voir [`Replace`]).
    fn apply_identity(&mut self, slug: &str, game: covers::FoundGame) {
        // le jeu existe (on vient de le choisir) : la marque « introuvable »
        // est fausse
        self.oublier_absent(slug);
        let same = self
            .identities
            .get(slug)
            .is_some_and(|identity| identity.id == game.id);
        // les visuels de l'ANCIEN jeu (le plus souvent ceux du paquet) : c'est
        // leur clé qu'il faudra oublier
        let old_key = self.key_of(slug);
        let mut identities = self.identities.clone();
        identities.insert(
            slug.to_string(),
            covers::Identity {
                id: game.id,
                name: game.name.clone(),
            },
        );
        covers::save_identities(
            &covers::identities_path(&self.config.assets_dir()),
            &identities,
        );
        self.identities = identities;
        // le jeu identifié apporte sa date (ou « pas de date »), rangée sous
        // la clé du jeu
        let key = self.key_of(slug);
        self.remember_year(&key, game.year);

        // les menus se referment : on revient à la page (ou à la liste)
        self.identify = None;
        self.identify_menu = None;
        self.picker = None;
        self.art_menu = None;

        // Même jeu que l'identification courante : les visuels en cache sont
        // déjà les bons, et peuvent avoir été choisis à la main — on ne les
        // efface pas (un choix par erreur ne doit pas détruire le travail).
        if same {
            self.status = Some((format!("déjà identifié : {}", game.name), false));
            self.resume_covers();
            return;
        }

        // Le paquet a DÉJÀ des visuels (SteamGridDB ou choisis à la main) :
        // on demande avant de les remplacer. Les effacer d'office jetait le
        // travail de l'utilisateur (demandé le 20/11), d'où « Non » par défaut.
        if covers::has_art(&self.config.assets_dir(), self.identities.get(slug), slug) {
            self.pause_covers();
            self.replace = Some(Replace {
                slug: slug.to_string(),
                old_key,
                game,
                index: 0,
            });
            return;
        }

        self.replace_art(slug, &game, &old_key);
    }

    /// Efface les visuels de l'ancien jeu et redemande les quatre types du jeu
    /// identifié, en fond. Appelé quand le paquet n'avait aucune image, ou
    /// après un « Oui, remplacer ».
    fn replace_art(&mut self, slug: &str, game: &covers::FoundGame, old_key: &str) {
        // Les visuels en cache sont ceux de l'ANCIEN jeu : les garder ferait
        // garder la cover, le logo et la date de « Hoobadventure » sur
        // « Marsupilami 2 ». Le fichier partagé et les marqueurs de la machine
        // sont retirés, puis tout est redemandé.
        covers::forget(
            &self.config.assets_dir(),
            &self.config.cache_dir().join("marks"),
            slug,
            old_key,
        );
        self.forget_local_art(slug);
        self.picked = None;
        if let Some(index) = self.games.iter().position(|game| game.slug == slug) {
            // même sélection que la page : les quatre types, dans l'urgence.
            // Les apparitions repartent de zéro : le nouveau héros grandit sa
            // bande au lieu de la faire sauter d'un coup.
            self.reset_page_assets(index);
            self.request_page_assets(index);
        } else {
            self.send_cover_query(covers::Query::Auto {
                slug: slug.to_string(),
                name: game.name.clone(),
                id: Some(game.id),
                key: Some(self.key_of(slug)),
                kinds: covers::ArtKind::ALL.to_vec(),
            });
        }
        self.status = Some((format!("identifié : {}", game.name), false));
        self.resume_covers();
    }

    /// Lève la marque « introuvable » d'un jeu et la retire du fichier partagé.
    /// Appelée dès qu'une identification, une recherche fructueuse ou une image
    /// choisie à la main rend la marque fausse : une marque ne doit jamais
    /// survivre à ce qui la contredit.
    fn oublier_absent(&mut self, slug: &str) {
        if self.absents.remove(slug).is_some() {
            covers::clear_absent(&covers::absents_path(&self.config.assets_dir()), slug);
        }
    }

    /// Ouvre « Quel jeu est … ? » et lance la recherche SteamGridDB : point
    /// d'entrée commun du prompt (Croix sur « Identifier ») et de
    /// « Revérifier » sur un jeu marqué introuvable.
    fn recherche_identite(&mut self, slug: String, query: String) {
        let current = self.identities.get(&slug).map(|identity| identity.id);
        self.identify_menu = Some(IdentifyMenu {
            slug: slug.clone(),
            query: query.clone(),
            index: 0,
            loading: true,
            error: None,
            games: Vec::new(),
            current,
        });
        // aucune cover de fond pendant qu'on regarde la liste
        self.pause_covers();
        eprintln!("glauncher : identification, recherche « {query} » pour {slug}");
        self.send_cover_query(covers::Query::Search { slug, name: query });
    }

    /// Relit les visuels d'un jeu depuis `assets/` (après un alias visuel) :
    /// l'interface n'a pas de téléchargement à attendre, les fichiers sont là.
    fn recharger_visuels(&mut self, slug: &str) {
        let assets = self.config.assets_dir();
        let identity = self.identities.get(slug).cloned();
        let lire = |kind: covers::ArtKind| {
            covers::cached_for(&assets.join(kind.dir()), identity.as_ref(), slug)
                .and_then(|path| std::fs::read(path).ok())
                .map(image::Handle::from_bytes)
        };
        let cover = lire(covers::ArtKind::Grid);
        let hero = lire(covers::ArtKind::Hero);
        let logo = lire(covers::ArtKind::Logo);
        let icon = lire(covers::ArtKind::Icon);
        for game in self.games.iter_mut().chain(self.all_games.iter_mut()) {
            if game.slug != slug {
                continue;
            }
            game.cover = cover.clone();
            game.hero = hero.clone();
            game.logo = logo.clone();
            game.thumb = None;
            if icon.is_some() {
                game.icon = icon.clone();
            }
        }
        if let Some(art) = self.artwork.get_mut(slug) {
            art.cover = cover;
            art.hero = hero;
            art.logo = logo;
            art.thumb = None;
            if icon.is_some() {
                art.icon = icon;
            }
        }
    }

    /// Efface les visuels en mémoire d'un jeu (l'icône extraite du paquet,
    /// elle, est correcte par définition : on la relit du cache).
    fn forget_local_art(&mut self, slug: &str) {
        self.art_requested.retain(|(known, _)| known != slug);
        self.cover_failed.remove(slug);
        self.thumb_requested.remove(slug);
        self.icon_requests.remove(slug);
        let icon = std::fs::read(self.config.pack_icon_dir().join(format!("{slug}.png")))
            .ok()
            .map(image::Handle::from_bytes);
        for game in self.games.iter_mut().chain(self.all_games.iter_mut()) {
            if game.slug != slug {
                continue;
            }
            game.cover = None;
            game.hero = None;
            game.logo = None;
            game.thumb = None;
            game.icon = icon.clone();
        }
        if let Some(art) = self.artwork.get_mut(slug) {
            art.cover = None;
            art.hero = None;
            art.logo = None;
            art.thumb = None;
            art.icon = icon;
        }
    }

    /// Recale les trois apparitions sur ce que le jeu a déjà : présent = déjà
    /// là (pas d'animation), absent = 0 et ça montera quand l'image arrivera.
    fn reset_page_assets(&mut self, index: usize) {
        let game = self.games.get(index);
        let present = |asset: Option<&image::Handle>| if asset.is_some() { 1.0 } else { 0.0 };
        self.page_hero = present(game.and_then(|game| game.hero.as_ref()));
        self.page_logo = present(game.and_then(|game| game.logo.as_ref()));
        self.page_cover = present(game.and_then(|game| game.cover.as_ref()));
    }

    /// Un visuel de la page est-il en train d'apparaître ? (sans ça, aucune
    /// image n'est demandée au moteur et le fondu ne progresserait pas)
    fn page_assets_animating(&self) -> bool {
        let Some(game) = self.page_game() else {
            return false;
        };
        (self.page_hero < 1.0 && game.hero.is_some())
            || (self.page_logo < 1.0 && game.logo.is_some())
            || (self.page_cover < 1.0 && game.cover.is_some())
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    /// Signature des couches qui s'ouvrent en fondu (menus, prompts, galerie).
    /// Un bit par couche : deux menus qui se remplacent changent la signature
    /// et relancent le fondu.
    fn overlay_signature(&self) -> u64 {
        let mut sig = 0u64;
        let mut bit = |valeur: bool| {
            sig = (sig << 1) | u64::from(valeur);
        };
        bit(self.settings.is_some());
        bit(self.view_menu.is_some());
        bit(self.sort_menu.is_some());
        bit(self.year_menu.is_some());
        bit(self.art_menu.is_some());
        bit(self.picker.is_some());
        bit(self.identify.is_some());
        bit(self.identify_menu.is_some());
        bit(self.alias_menu.is_some());
        bit(self.replace.is_some());
        bit(self.date.is_some());
        // la galerie entre en fondu ; l'aperçu d'une photo et le panneau
        // d'effacement ont leur propre transition et ne doivent PAS faire
        // clignoter tout l'écran en noir (23/11)
        bit(self.galerie.is_some());
        sig
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Le fondu est armé APRÈS le traitement : la vue qui suit cette mise à
        // jour voit déjà le voile, sinon le menu apparaissait une image en
        // plein puis passait au noir. Les `return` internes de `update_inner`
        // ramènent leur Task ici, donc aucun chemin d'ouverture n'est oublié.
        let task = self.update_inner(message);
        let signature = self.overlay_signature();
        if signature != self.overlay_vu && signature != 0 {
            // un menu (ou la galerie) vient de s'ouvrir, ou un menu en
            // remplace un autre : voile entier, `tick` le dissipe
            self.overlay_fade = 1.0;
        }
        self.overlay_vu = signature;
        task
    }

    fn update_inner(&mut self, message: Message) -> Task<Message> {
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
            Message::Souris => {
                self.barre = true;
                Task::none()
            }
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
            Message::Year { slug, key, year } => {
                self.remember_year(&key, year);
                // demandé depuis le prompt « date » : on annonce le résultat
                if self.asked_year.as_deref() == Some(slug.as_str()) {
                    self.asked_year = None;
                    self.status = Some(match year {
                        Some(year) => (format!("date de sortie : {year}"), false),
                        None => ("SteamGridDB ne donne pas de date".to_string(), true),
                    });
                }
                Task::none()
            }
            Message::Identified { slug, identity } => {
                // le worker a résolu le jeu et l'a écrit dans
                // `assets/identities.txt` : sa table locale suit, c'est cette
                // identification qui nomme les visuels à partir de maintenant
                eprintln!(
                    "glauncher : {} identifié « {} » (id {})",
                    slug, identity.name, identity.id
                );
                self.identities.insert(slug.clone(), identity);
                self.oublier_absent(&slug);
                Task::none()
            }
            Message::Absent { slug, requete } => {
                let marque = covers::marquer_absent(
                    &covers::absents_path(&self.config.assets_dir()),
                    &slug,
                    &requete,
                );
                eprintln!(
                    "glauncher : {} introuvable sur SteamGridDB (vérifié le {})",
                    slug, marque.verifie
                );
                self.absents.insert(slug, marque);
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

                // nom et id réellement envoyés à SteamGridDB : l'identification
                // du jeu fait foi, sinon le nom du paquet
                let (query, id) = self.who(&slug, &name);
                let thumbs = choices
                    .iter()
                    .map(|choice| {
                        (!choice.thumb.is_empty())
                            .then(|| image::Handle::from_bytes(choice.thumb.clone()))
                    })
                    .collect();
                self.picker = Some(Picker {
                    slug: slug.clone(),
                    name: name.clone(),
                    query: query.clone(),
                    kind,
                    index: 0,
                    loading: true,
                    error: None,
                    choices,
                    thumbs,
                });
                // le menu d'image a la priorité : plus de visuels de fond
                self.pause_covers();
                self.send_cover_query(covers::Query::Choices {
                    slug,
                    name: query,
                    id,
                    kind,
                });
                Task::none()
            }
            Message::IdentifyOpen => {
                // préremplit avec l'identification courante (ou le nom exact
                // du jeu identifié), sinon le nom du paquet
                let Some(slug) = self.current_slug() else {
                    return Task::none();
                };
                // préremplit avec la recherche en cours (correction du nom),
                // sinon l'identification courante, sinon le nom du paquet
                let current = self
                    .identify_menu
                    .as_ref()
                    .filter(|menu| menu.slug == slug)
                    .map(|menu| menu.query.clone())
                    .or_else(|| {
                        self.identities
                            .get(&slug)
                            .map(|identity| identity.name.clone())
                            .filter(|name| !name.is_empty())
                    });
                let name = current.unwrap_or_else(|| {
                    self.games
                        .get(self.selected)
                        .map(|game| game.name.clone())
                        .unwrap_or_default()
                });
                self.identify = Some(name);
                // le menu des visuels laisse la place au prompt
                self.art_menu = None;
                // session console : aucun clavier physique, le prompt ouvre
                // celui de l'overlay (frappe uinput dans la fenêtre active)
                self.clavier_console();
                // présélection : taper remplace directement le nom proposé
                Task::batch([
                    operation::focus(self.input_id.clone()),
                    operation::select_all(self.input_id.clone()),
                ])
            }
            Message::IdentifyInput(text) => {
                if self.identify.is_some() {
                    self.identify = Some(text);
                }
                Task::none()
            }
            Message::IdentifyCancel => {
                self.identify = None;
                Task::none()
            }
            Message::IdentifySubmit => {
                let Some(query) = self.identify.take() else {
                    return Task::none();
                };
                let query = query.trim().to_string();
                // un nom vide ne donnerait rien : on ne relance pas
                if query.is_empty() {
                    return Task::none();
                }
                let Some(slug) = self.current_slug() else {
                    return Task::none();
                };
                self.recherche_identite(slug, query);
                Task::none()
            }
            Message::IdentifyResults {
                slug,
                query,
                result,
            } => {
                let vide = matches!(&result, Ok(games) if games.is_empty());
                let trouve = matches!(&result, Ok(games) if !games.is_empty());
                if let Some(menu) = self.identify_menu.as_mut()
                    && menu.slug == slug
                    && menu.query == query
                {
                    menu.loading = false;
                    match result {
                        Ok(games) => {
                            // le jeu déjà identifié est sélectionné d'office
                            menu.index = menu
                                .current
                                .and_then(|id| games.iter().position(|game| game.id == id))
                                .unwrap_or(0);
                            menu.games = games;
                            menu.error = None;
                            eprintln!(
                                "glauncher : {} jeu(x) proposé(s) pour « {query} »",
                                menu.games.len()
                            );
                        }
                        Err(error) => menu.error = Some(error),
                    }
                }
                // la marque suit le dernier essai : rien trouvé = datée,
                // trouvé = levée
                if vide {
                    let marque = covers::marquer_absent(
                        &covers::absents_path(&self.config.assets_dir()),
                        &slug,
                        &query,
                    );
                    self.absents.insert(slug, marque);
                } else if trouve {
                    self.oublier_absent(&slug);
                }
                Task::none()
            }
            Message::IdentifyNav(delta) => {
                if let Some(menu) = self.identify_menu.as_mut()
                    && !menu.games.is_empty()
                {
                    let last = menu.games.len() - 1;
                    menu.index = (menu.index as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::IdentifyChoose(index) => {
                if let Some(menu) = self.identify_menu.as_mut() {
                    menu.index = index.min(menu.games.len().saturating_sub(1));
                }
                Task::done(Message::IdentifyPick)
            }
            Message::IdentifyPick => {
                let Some(menu) = self.identify_menu.as_ref() else {
                    return Task::none();
                };
                // erreur réseau ou liste vide : A relance la recherche
                if let Some(error) = menu.error.clone() {
                    let (slug, query) = (menu.slug.clone(), menu.query.clone());
                    if let Some(menu) = self.identify_menu.as_mut() {
                        menu.error = Some(error);
                        menu.loading = true;
                    }
                    self.send_cover_query(covers::Query::Search { slug, name: query });
                    return Task::none();
                }
                let Some(game) = menu.games.get(menu.index).cloned() else {
                    return Task::none();
                };
                let slug = menu.slug.clone();
                self.apply_identity(&slug, game);
                Task::none()
            }
            Message::IdentifyClose => {
                self.identify = None;
                self.identify_menu = None;
                Task::none()
            }
            Message::AliasOpen => {
                let Some(slug) = self.current_slug() else {
                    return Task::none();
                };
                let assets = self.config.assets_dir();
                let target_key = self.key_of(&slug);
                // ne proposer que les jeux qui ont DÉJÀ un visuel sur le
                // disque : reprendre le néant n'aide personne. Un autre paquet
                // du MÊME jeu partage déjà les fichiers (même clé) : inutile.
                let choix: Vec<(String, String, Option<image::Handle>)> = self
                    .all_games
                    .iter()
                    .filter(|game| game.slug != slug && self.key_of(&game.slug) != target_key)
                    .filter(|game| {
                        covers::has_art(&assets, self.identities.get(&game.slug), &game.slug)
                    })
                    .map(|game| {
                        (
                            game.name.clone(),
                            game.slug.clone(),
                            game.cover.clone().or_else(|| game.thumb.clone()),
                        )
                    })
                    .collect();
                if choix.is_empty() {
                    self.status = Some((
                        "aucun autre jeu n'a de visuels — choisis une image pour un jeu (Carré sur sa page) avant de la reprendre"
                            .to_string(),
                        true,
                    ));
                    return Task::none();
                }
                self.pause_covers();
                self.alias_menu = Some(AliasMenu {
                    slug,
                    choix,
                    index: 0,
                });
                Task::none()
            }
            Message::AliasNav(delta) => {
                if let Some(menu) = self.alias_menu.as_mut()
                    && !menu.choix.is_empty()
                {
                    let last = menu.choix.len() - 1;
                    menu.index = (menu.index as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::AliasChoose(index) => {
                if let Some(menu) = self.alias_menu.as_mut() {
                    menu.index = index.min(menu.choix.len().saturating_sub(1));
                }
                Task::done(Message::AliasPick)
            }
            Message::AliasPick => {
                let Some(menu) = self.alias_menu.clone() else {
                    return Task::none();
                };
                let Some((nom, source, _)) = menu.choix.get(menu.index).cloned() else {
                    return Task::none();
                };
                let assets = self.config.assets_dir();
                let marks = self.config.cache_dir().join("marks");
                let source_key = self.key_of(&source);
                let target_key = self.key_of(&menu.slug);
                if source_key == target_key {
                    self.alias_menu = None;
                    self.resume_covers();
                    self.status = Some((
                        "même jeu SteamGridDB : les visuels sont déjà partagés".to_string(),
                        false,
                    ));
                    return Task::none();
                }
                let copies =
                    covers::alias_visuels(&assets, &marks, &source_key, &target_key, &menu.slug);
                self.alias_menu = None;
                if copies == 0 {
                    self.status = Some((
                        format!("aucun visuel à reprendre de « {nom} »"),
                        true,
                    ));
                    self.resume_covers();
                    return Task::none();
                }
                // l'alias vaut identification visuelle : plus de marque
                // « introuvable », et les fichiers choisis ne seront pas
                // remplacés par une recherche
                self.oublier_absent(&menu.slug);
                self.forget_local_art(&menu.slug);
                self.recharger_visuels(&menu.slug);
                if let Some(index) = self.games.iter().position(|game| game.slug == menu.slug) {
                    self.reset_page_assets(index);
                }
                self.request_thumb(&menu.slug);
                self.status = Some((
                    format!("{copies} visuel(s) repris de « {nom} »"),
                    false,
                ));
                self.resume_covers();
                Task::none()
            }
            Message::AliasClose => {
                self.alias_menu = None;
                self.resume_covers();
                Task::none()
            }
            Message::DateOpen => {
                let Some(slug) = self.current_slug() else {
                    return Task::none();
                };
                // préremplit avec l'année connue : corriger une date fausse
                // n'oblige pas à tout retaper
                self.date = Some(
                    self.year_of(&slug)
                        .map(|year| year.to_string())
                        .unwrap_or_default(),
                );
                self.art_menu = None;
                self.clavier_console();
                Task::batch([
                    operation::focus(self.input_id.clone()),
                    operation::select_all(self.input_id.clone()),
                ])
            }
            Message::DateInput(text) => {
                if self.date.is_some() {
                    self.date = Some(text);
                }
                Task::none()
            }
            Message::DateCancel => {
                self.date = None;
                Task::none()
            }
            Message::DateSubmit => {
                let Some(value) = self.date.take() else {
                    return Task::none();
                };
                let Some(slug) = self.current_slug() else {
                    return Task::none();
                };
                let key = self.key_of(&slug);
                let value = value.trim().to_string();
                // vide = on ne sait pas / on ne veut pas de date pour ce jeu
                if value.is_empty() {
                    self.remember_year(&key, None);
                    self.status = Some(("date de sortie effacée".to_string(), false));
                    return Task::none();
                }
                match value.parse::<u32>() {
                    Ok(year) if (1950..=2100).contains(&year) => {
                        self.remember_year(&key, Some(year));
                        self.status = Some((format!("date de sortie : {year}"), false));
                    }
                    _ => {
                        self.status = Some(("année invalide (1950 à 2100)".to_string(), true));
                        // on garde le prompt ouvert pour corriger
                        self.date = Some(value);
                    }
                }
                Task::none()
            }
            Message::DateRefresh => {
                self.date = None;
                let Some(slug) = self.current_slug() else {
                    return Task::none();
                };
                let Some(name) = self
                    .page_game()
                    .or_else(|| self.games.get(self.selected))
                    .map(|game| game.name.clone())
                else {
                    return Task::none();
                };
                let (name, id) = self.who(&slug, &name);
                let key = id.map(|_| self.key_of(&slug));
                self.asked_year = Some(slug.clone());
                self.status = Some(("recherche de la date…".to_string(), false));
                self.send_cover_query(covers::Query::Year {
                    slug,
                    name,
                    id,
                    key,
                });
                Task::none()
            }
            Message::GalleryOpen(jeu) => {
                let tous = gallery::scanne();
                self.galerie = Some(Galerie {
                    items: Vec::new(),
                    tous,
                    filtre: None,
                    jeu,
                    selected: 0,
                    arme: None,
                    thumbs: std::collections::HashMap::new(),
                    prepared: HashSet::new(),
                    restant: 0,
                });
                // le filtre de jeu (bouton « Galerie » d'une page) s'applique
                // dès l'ouverture : la grille ne montre jamais, même un
                // instant, les captures des autres jeux
                self.gallery_apply_filter();
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                // banc : `apercu`/`video` sélectionne le premier élément du
                // genre, pour photographier le lecteur sans manette
                if let Ok(quoi) = std::env::var("GLAUNCHER_GALLERY_DEMO") {
                    let genre = match quoi.as_str() {
                        "apercu" => Some(gallery::Genre::Image),
                        "video" => Some(gallery::Genre::Video),
                        _ => None,
                    };
                    if let Some(genre) = genre
                        && let Some(index) = galerie
                            .items
                            .iter()
                            .position(|item| item.genre == genre)
                    {
                        galerie.selected = index;
                    }
                }
                // la couche menus est reconstruite à part : rien à préserver
                self.view_menu = None;
                self.status = Some((format!("galerie : {} élément(s)", galerie.items.len()), false));
                if galerie.items.is_empty() {
                    self.status = Some((
                        "galerie vide : rien dans les dossiers de sortie".to_string(),
                        false,
                    ));
                }
                // les vignettes déjà en cache arrivent tout de suite (lecture
                // de fichiers), les autres suivent par fenêtre visible
                self.gallery_prepare();
                Task::none()
            }
            Message::GalerieComptes(comptes) => {
                self.galerie_comptes = comptes;
                Task::none()
            }
            Message::GalleryClose => {
                // une capture ou une vidéo a pu passer à la corbeille : le
                // bouton « Galerie » de la page doit suivre
                self.rafraichir_comptes_galerie(true);
                self.galerie = None;
                self.apercu = None;
                self.video_veil = 0.0;
                self.video_veil_target = 0.0;
                self.video_lift_at = None;
                Task::none()
            }
            Message::GalleryNav(delta) => {
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                let total = galerie.items.len();
                if total == 0 {
                    return Task::none();
                }
                let cible = galerie.selected as i64 + delta as i64;
                galerie.selected = cible.clamp(0, total as i64 - 1) as usize;
                galerie.arme = None;
                self.gallery_prepare();
                Task::none()
            }
            Message::GalleryAct => {
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                // effacement armé : Croix le confirme
                if let Some(index) = galerie.arme.take() {
                    self.gallery_delete(index);
                    return Task::none();
                }
                let Some(item) = galerie.items.get(galerie.selected).cloned() else {
                    return Task::none();
                };
                let index = galerie.selected;
                match item.genre {
                    gallery::Genre::Image => {
                        // l'image est là TOUT DE SUITE ; seule une petite
                        // mise à l'échelle (60 Hz) accompagne l'ouverture
                        self.apercu = Some(Apercu {
                            index,
                            entree: 1.0,
                        });
                        Task::none()
                    }
                    gallery::Genre::Video => {
                        // Voile noir **sans** l'« hyperespace » des étoiles :
                        // le fondu du lancement de jeu faisait filer la
                        // poussière pendant ~1 s avant d'ouvrir le film
                        // (jugé trop long, 23/11). Le film est lancé tout de
                        // suite, le voile ne se lève que quand mpv a eu le
                        // temps de s'ouvrir en plein écran.
                        self.video_veil = 1.0;
                        self.video_veil_target = 1.0;
                        self.a_lancer_video =
                            Some((item.path.clone(), item.nom(), std::time::Instant::now()));
                        Task::none()
                    }
                }
            }
            Message::GalleryChoose(index) => {
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                if index >= galerie.items.len() {
                    return Task::none();
                }
                if index == galerie.selected {
                    return Task::done(Message::GalleryAct);
                }
                galerie.selected = index;
                galerie.arme = None;
                Task::none()
            }
            Message::GalleryArm => {
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                if galerie.items.is_empty() {
                    return Task::none();
                }
                galerie.arme = Some(galerie.selected);
                let nom = galerie
                    .items
                    .get(galerie.selected)
                    .map(|item| item.nom())
                    .unwrap_or_default();
                self.status = Some((
                    format!("mettre {nom} à la corbeille ? Croix = oui · Rond = annuler"),
                    true,
                ));
                Task::none()
            }
            Message::GalleryUnarm => {
                if let Some(galerie) = self.galerie.as_mut() {
                    galerie.arme = None;
                }
                self.status = None;
                Task::none()
            }
            Message::GalleryFilter => {
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                galerie.filtre = match galerie.filtre {
                    None => Some(gallery::Genre::Image),
                    Some(gallery::Genre::Image) => Some(gallery::Genre::Video),
                    Some(gallery::Genre::Video) => None,
                };
                self.gallery_apply_filter();
                self.gallery_prepare();
                if let Some(galerie) = self.galerie.as_ref() {
                    self.status = Some((galerie_legende(galerie), false));
                }
                Task::none()
            }
            Message::GalleryThumb { path, bytes } => {
                let Some(galerie) = self.galerie.as_mut() else {
                    return Task::none();
                };
                galerie.restant = galerie.restant.saturating_sub(1);
                galerie
                    .thumbs
                    .insert(path, bytes.map(image::Handle::from_bytes));
                Task::none()
            }
            Message::GalleryTick => {
                // fin de la vidéo : on rend la main au launcher, puis on
                // efface le voile (la fenêtre de mpv a disparu)
                if let Some(lecteur) = self.lecteur.as_mut()
                    && (lecteur.fini() || lecteur.en_retard())
                {
                    return Task::done(Message::GalleryVideoEnd);
                }
                Task::none()
            }
            Message::GalleryVideoEnd => {
                if let Some(mut lecteur) = self.lecteur.take() {
                    lecteur.tuer();
                }
                self.fade_target = 0.0;
                self.video_veil = 0.0;
                self.video_veil_target = 0.0;
                self.video_lift_at = None;
                self.status = None;
                match self.window {
                    Some(id) => window::gain_focus(id).map(|()| Message::Nothing),
                    None => Task::none(),
                }
            }
            Message::ApercuNav(delta) => {
                let Some(apercu) = self.apercu.as_mut() else {
                    return Task::none();
                };
                let Some(galerie) = self.galerie.as_ref() else {
                    return Task::none();
                };
                // seules les captures d'écran se suivent (une vidéo se lance)
                let images: Vec<usize> = galerie
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.genre == gallery::Genre::Image)
                    .map(|(index, _)| index)
                    .collect();
                let Some(position) = images.iter().position(|index| *index == apercu.index) else {
                    return Task::none();
                };
                let cible = (position as i64 + delta as i64).clamp(0, images.len() as i64 - 1);
                apercu.index = images[cible as usize];
                // l'image suivante arrive tout de suite, avec sa propre
                // petite entrée (on ne refait pas le fondu complet)
                apercu.entree = 1.0;
                Task::none()
            }
            Message::ApercuClose => {
                self.apercu = None;
                Task::none()
            }
            Message::ApercuCopy => {
                let Some(item) = self
                    .apercu
                    .as_ref()
                    .and_then(|apercu| {
                        self.galerie
                            .as_ref()
                            .and_then(|galerie| galerie.items.get(apercu.index))
                    })
                    .cloned()
                else {
                    return Task::none();
                };
                match gallery::copie_avec_logo(&item) {
                    Ok(path) => {
                        self.status = Some((
                            format!("copie avec logo : {}", path.display()),
                            false,
                        ));
                        println!("GALERIE copie {}", path.display());
                    }
                    Err(erreur) => self.status = Some((erreur, true)),
                }
                Task::none()
            }
            Message::ClosePicker => {
                let was_open = self.picker.take().is_some();
                self.identify = None;
                self.identify_menu = None;
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
                    // La grille défile avec la sélection : `RelativeOffset` ne
                    // demande aucune mesure (iced borne de lui-même), la
                    // dernière rangée arrive donc en bas du panneau au lieu
                    // de rester dessous.
                    let lignes = picker.choices.len().div_ceil(columns).max(1);
                    let ligne = picker.index / columns;
                    let y = if lignes > 1 {
                        ligne as f32 / (lignes - 1) as f32
                    } else {
                        0.0
                    };
                    return operation::snap_to(
                        Id::new(PICKER_SCROLL),
                        RelativeOffset {
                            x: None,
                            y: Some(y),
                        },
                    );
                }
                Task::none()
            }
            Message::PickerChoose(index) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.index = index;
                }
                Task::done(Message::PickerPick)
            }
            Message::PickerPick => {
                let Some(picker) = self.picker.as_ref() else {
                    return Task::none();
                };
                // en cas d'échec, A relance la recherche d'images
                if let Some(error) = picker.error.clone() {
                    let (slug, name, kind) =
                        (picker.slug.clone(), picker.name.clone(), picker.kind);
                    // le nom envoyé est celui qui a servi pour ce menu : le nom
                    // du jeu identifié s'il y en a un
                    let (name, id) = self.who(&slug, &name);
                    if let Some(picker) = self.picker.as_mut() {
                        picker.error = Some(error);
                        picker.loading = true;
                    }
                    self.send_cover_query(covers::Query::Choices {
                        slug,
                        name,
                        id,
                        kind,
                    });
                    return Task::none();
                }
                let Some(choice) = picker.choices.get(picker.index) else {
                    return Task::none();
                };
                let (slug, kind) = (picker.slug.clone(), picker.kind);
                let (url, ext) = (choice.url.clone(), choice.ext.clone());
                let key = self.key_of(&slug);
                self.send_cover_query(covers::Query::Pick {
                    slug: slug.clone(),
                    key,
                    kind,
                    url,
                    ext,
                });
                // Le menu se referme IMMÉDIATEMENT : rester ouvert pendant le
                // téléchargement donnait l'impression que la validation
                // n'était pas prise en compte (constaté 18/11). `picked` garde
                // la trace du choix pour la suite (remplacement de l'icône).
                self.picker = None;
                self.oublier_absent(&slug);
                self.picked = Some((slug, kind));
                self.status = Some(("application de l'image…".to_string(), false));
                self.resume_covers();
                Task::none()
            }
            Message::PickerFile => {
                // session console : le sélecteur de fichiers du bureau
                // (kdialog/zenity) n'existe pas, et une image perso se choisit
                // hors jeu (Plasma). Aucune entrée ne mène ici, c'est une
                // ceinture en plus — le clavier virtuel pourrait taper « f ».
                if self.mode.is_console() {
                    return Task::none();
                }
                let Some(picker) = self.picker.as_ref() else {
                    return Task::none();
                };
                let (slug, kind) = (picker.slug.clone(), picker.kind);
                let depart = crate::config::home_dir();
                let titre = format!("Choisir {}", kind.label());
                // dialogue système dans un VRAI fil (il reste ouvert tant que
                // l'utilisateur navigue) ; le résultat revient par un canal,
                // donc aucun fil de l'UI n'est occupé
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                let lance = std::thread::Builder::new()
                    .name("glauncher-dialogue".into())
                    .spawn(move || {
                        let _ = tx.send(dialog::choisir_image(&depart, &titre));
                    });
                if lance.is_err() {
                    self.status = Some(("dialogue d'image indisponible".to_string(), true));
                    return Task::none();
                }
                Task::perform(async move { rx.await.ok().flatten() }, move |path| {
                    Message::FileChosen {
                        slug: slug.clone(),
                        kind,
                        path,
                    }
                })
            }
            Message::FileChosen { slug, kind, path } => {
                // annulé : le menu d'images reste ouvert, rien ne change
                let Some(path) = path else {
                    return Task::none();
                };
                eprintln!(
                    "glauncher : image perso pour {slug} ({}) : {}",
                    kind.label(),
                    path.display()
                );
                // même sortie qu'un choix SteamGridDB : le menu se referme, le
                // fichier est adapté puis écrit par le worker
                self.picker = None;
                self.oublier_absent(&slug);
                self.picked = Some((slug.clone(), kind));
                self.status = Some(("application de l'image…".to_string(), false));
                self.resume_covers();
                let key = self.key_of(&slug);
                self.send_cover_query(covers::Query::UseFile {
                    slug,
                    key,
                    kind,
                    path,
                });
                Task::none()
            }
            Message::ReplaceNav(delta) => {
                if let Some(replace) = self.replace.as_mut() {
                    replace.index = (replace.index as i32 + delta).clamp(0, 1) as usize;
                }
                Task::none()
            }
            Message::ReplaceChoose(index) => {
                if let Some(replace) = self.replace.as_mut() {
                    replace.index = index.min(1);
                }
                Task::done(Message::ReplacePick)
            }
            Message::ReplacePick => {
                let Some(replace) = self.replace.take() else {
                    return Task::none();
                };
                if replace.index == 0 {
                    // « Non » (défaut) : les visuels déjà là restent, y compris
                    // ceux choisis à la main — les écraser sans demander est
                    // exactement ce qu'on veut éviter
                    self.status = Some((
                        format!("visuels conservés pour {}", replace.game.name),
                        false,
                    ));
                    self.resume_covers();
                    return Task::none();
                }
                self.replace_art(&replace.slug, &replace.game, &replace.old_key);
                Task::none()
            }
            Message::OublierOpen => {
                let count = self.slugs_disparus().len();
                if count == 0 {
                    self.status = Some(("Aucun jeu à oublier".to_string(), false));
                    return Task::none();
                }
                self.oublier = Some(Oublier { count, index: 0 });
                Task::none()
            }
            Message::OublierNav(delta) => {
                if let Some(oublier) = self.oublier.as_mut() {
                    oublier.index = (oublier.index as i32 + delta).clamp(0, 1) as usize;
                }
                Task::none()
            }
            Message::OublierChoose(index) => {
                if let Some(oublier) = self.oublier.as_mut() {
                    oublier.index = index.min(1);
                }
                Task::done(Message::OublierPick)
            }
            Message::CopyOpen => {
                let Some(game) = self.jeu_courant() else {
                    return Task::none();
                };
                let source = game.path.clone();
                let size = std::fs::metadata(&source).map(|meta| meta.len()).unwrap_or(0);
                let mut dests = vec![CopyDest {
                    label: "Téléchargements".to_string(),
                    dir: copy::telechargements(),
                    libre: None,
                }];
                for dir in self.config.game_dirs() {
                    dests.push(CopyDest {
                        label: folders::affiche(&dir),
                        dir,
                        libre: None,
                    });
                }
                // place libre lue une fois, à l'ouverture : un `statvfs` par
                // destination, jamais par image
                for dest in dests.iter_mut() {
                    dest.libre = copy::libre(&dest.dir);
                }
                self.art_menu = None;
                self.copy_menu = Some(CopyMenu {
                    dests,
                    index: 0,
                    size,
                });
                Task::none()
            }
            Message::CopyNav(delta) => {
                if let Some(menu) = self.copy_menu.as_mut() {
                    let last = menu.dests.len().saturating_sub(1) as i32;
                    menu.index = (menu.index as i32 + delta).clamp(0, last) as usize;
                }
                Task::none()
            }
            // un clic vaut la décision tout de suite (même règle que le menu
            // d'images) : pas de message intermédiaire, donc pas de risque
            // qu'un autre menu soit pris pour celui qu'on vient de choisir
            Message::CopyChoose(index) => {
                if let Some(menu) = self.copy_menu.as_mut() {
                    menu.index = index.min(menu.dests.len().saturating_sub(1));
                }
                self.copy_pick()
            }
            Message::CopyPick => self.copy_pick(),
            Message::CopyReplaceNav(delta) => {
                if let Some(replace) = self.copy_replace.as_mut() {
                    replace.index = (replace.index as i32 + delta).clamp(0, 1) as usize;
                }
                Task::none()
            }
            Message::CopyReplaceChoose(index) => {
                if let Some(replace) = self.copy_replace.as_mut() {
                    replace.index = index.min(1);
                }
                Task::done(Message::CopyReplacePick)
            }
            Message::CopyReplacePick => {
                let Some(replace) = self.copy_replace.take() else {
                    return Task::none();
                };
                if replace.index == 0 {
                    return Task::none();
                }
                self.enqueue_copy(replace.job);
                Task::none()
            }
            Message::CopyStep { fait, .. } => {
                if let Some(copies) = self.copies.as_mut() {
                    copies.courant = fait;
                }
                Task::none()
            }
            Message::CopyDone { size, .. } => {
                let Some(copies) = self.copies.as_mut() else {
                    return Task::none();
                };
                copies.fait += size;
                copies.courant = 0;
                copies.en_cours = None;
                if copies.attente.is_empty() {
                    let total = copies.total;
                    let nombre = copies.nombre;
                    self.copies = None;
                    self.status = Some((
                        format!(
                            "{} copié{} · {}",
                            plural(nombre),
                            if nombre > 1 { "s" } else { "" },
                            copy::poids(total)
                        ),
                        false,
                    ));
                    return Task::none();
                }
                self.demarrer_copie();
                Task::none()
            }
            Message::CopyFailed { nom, erreur } => {
                // une copie cassée arrête la file : mieux vaut le dire que
                // remplir un disque plein en silence
                eprintln!("glauncher : copie de {nom} impossible : {erreur}");
                self.copies = None;
                self.status = Some((format!("copie de {nom} impossible : {erreur}"), true));
                Task::none()
            }
            Message::OublierPick => {
                let Some(oublier) = self.oublier.take() else {
                    return Task::none();
                };
                if oublier.index == 0 {
                    return Task::none();
                }
                let oublies = self.oublier_disparus();
                self.status = Some((
                    format!(
                        "{} oublié{} de la liste (images conservées)",
                        plural(oublies),
                        if oublies > 1 { "s" } else { "" }
                    ),
                    false,
                ));
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
                    picker.choices = keep.into_iter().chain(choices).collect();
                    // une case par proposition, vide : les vignettes arrivent
                    // ensuite, une par message
                    picker.thumbs.resize(picker.choices.len(), None);
                    for (index, choice) in picker.choices.iter().enumerate() {
                        if picker.thumbs[index].is_none() && !choice.thumb.is_empty() {
                            picker.thumbs[index] =
                                Some(image::Handle::from_bytes(choice.thumb.clone()));
                        }
                    }
                    picker.loading = false;
                    picker.index = 0;
                }
                Task::none()
            }
            Message::Choice {
                slug,
                kind,
                url,
                thumb,
            } => {
                if let Some(picker) = self.picker.as_mut()
                    && picker.slug == slug
                    && picker.kind == kind
                    && let Some(index) = picker.choices.iter().position(|choice| choice.url == url)
                    && let Some(slot) = picker.thumbs.get_mut(index)
                {
                    *slot = Some(image::Handle::from_bytes(thumb));
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
                if let Some((slug, kind, started)) = self.play_started.take() {
                    let seconds = started.elapsed().as_secs();
                    if seconds >= 5 {
                        playtime::add(
                            &self.playtime_paths,
                            &mut self.playtime,
                            &slug,
                            kind.as_str(),
                            seconds,
                        );
                    }
                }
                self.fade_target = 0.0;
                // les vignettes reviennent des bords pendant que le voile
                // s'éclaircit
                self.returning = true;
                // retour au launcher : la musique revient en fondu — sauf si
                // elle a été coupée dans les Réglages entre-temps
                if self.config.music
                    && self.config.music_volume > 0
                    && let Some(music) = &self.music
                {
                    music.resume();
                }
                // on revient dans la liste des jeux, pas dans la page du jeu
                self.page = None;
                // aucune bascule de page en attente : c'est le voile du
                // lancement (`fade`) qui fait le fondu de retour
                self.pending_page = None;
                self.page_fade = 0.0;
                self.page_fade_target = 0.0;
                self.page_hero = 0.0;
                self.page_logo = 0.0;
                self.page_cover = 0.0;
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
                // le bouton « Galerie » dépend de ce que contiennent les
                // dossiers de sortie : recompter au besoin (une capture prise
                // depuis le dernier comptage doit apparaître)
                self.rafraichir_comptes_galerie(false);
                self.selected = index;
                // La bascule se fait au sommet du fondu (voir `tick`) : la page
                // n'entre pas dans l'arbre tout de suite, sinon le changement
                // d'écran se verrait à travers un fond à peine opaque.
                self.pending_page = Some(PageStep::Open(index));
                self.page_fade_target = 1.0;
                self.art_menu = None;
                self.update_scroll_target();
                // le héros de la page est prioritaire sur les visuels de fond
                // et la file de fond s'arrête : ce qu'on regarde est la page,
                // pas la liste qu'elle recouvre (`resume_covers` à la fermeture).
                // La pause libère d'abord les demandes abandonnées, puis la
                // page réclame ce qui lui manque (y compris ce que la file de
                // fond venait de lâcher).
                self.pause_covers();
                self.request_page_assets(index);
                // année de sortie : une simple requête d'autocomplete, et
                // seulement si on ne la connaît pas — des visuels déjà en cache
                // ne l'apprennent pas. Un jeu identifié demande SA date.
                if let Some(game) = self.games.get(index) {
                    let (slug, name) = (game.slug.clone(), game.name.clone());
                    let (name, id) = self.who(&slug, &name);
                    let key = id.map(|_| self.key_of(&slug));
                    if self.year_of(&slug).is_none()
                        && !self.years.contains_key(&self.key_of(&slug))
                    {
                        self.send_cover_query(covers::Query::Year {
                            slug,
                            name,
                            id,
                            key,
                        });
                    }
                }
                Task::none()
            }
            Message::ClosePage => {
                // même chose dans l'autre sens : la page sort au sommet du
                // fondu, on ne la retire pas à l'aveugle
                if self.page.is_some() {
                    self.pending_page = Some(PageStep::Close);
                    self.page_fade_target = 1.0;
                }
                self.art_menu = None;
                Task::none()
            }
            Message::PageNav(delta) => {
                // borne calculée comme dans `page_view` : « Galerie » n'existe
                // que si le jeu a des captures (sinon Reste-Suivante s'y
                // arrêtait sans rien afficher)
                let last = self.page_actions().len().saturating_sub(1);
                if let Some(page) = self.page.as_mut() {
                    let next = page.action as i32 + delta;
                    page.action = next.clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::PageChoose(index) => {
                let last = self.page_actions().len().saturating_sub(1);
                if let Some(page) = self.page.as_mut() {
                    page.action = index.min(last);
                }
                self.page_act()
            }
            Message::PageAct => self.page_act(),
            Message::ArtMenuNav(delta) => {
                let last = self.art_entries().len().saturating_sub(1);
                if let Some(choice) = self.art_menu.as_mut() {
                    *choice = (*choice as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::ArtMenuChoose(index) => {
                self.art_menu = Some(index.min(self.art_entries().len().saturating_sub(1)));
                Task::done(Message::ArtMenuPick)
            }
            Message::ArtMenuPick => {
                let Some(choice) = self.art_menu else {
                    return Task::none();
                };
                let Some(entry) = self.art_entries().get(choice).copied() else {
                    return Task::none();
                };
                match entry {
                    ArtMenuEntry::Kind(kind) => {
                        self.art_menu = None;
                        Task::done(Message::OpenPicker(kind))
                    }
                    ArtMenuEntry::Identify => {
                        self.art_menu = None;
                        let Some(slug) = self.current_slug() else {
                            return Task::none();
                        };
                        // jeu marqué introuvable : Croix = revérifier tout de
                        // suite, sans attendre l'expiration (30 jours)
                        if let Some(absent) = self.absents.get(&slug).cloned() {
                            self.oublier_absent(&slug);
                            covers::oublier_introuvables(
                                &self.config.cache_dir().join("marks"),
                                &slug,
                            );
                            self.forget_local_art(&slug);
                            self.recherche_identite(slug, absent.requete);
                            return Task::none();
                        }
                        Task::done(Message::IdentifyOpen)
                    }
                    ArtMenuEntry::Alias => {
                        self.art_menu = None;
                        Task::done(Message::AliasOpen)
                    }
                    ArtMenuEntry::Copier => {
                        self.art_menu = None;
                        Task::done(Message::CopyOpen)
                    }
                    ArtMenuEntry::Date => {
                        self.art_menu = None;
                        Task::done(Message::DateOpen)
                    }
                    // les réglages de lancement restent un **interrupteur** :
                    // on ne referme pas le menu, l'état se voit changer sous
                    // les yeux (comme un switch d'overlay)
                    ArtMenuEntry::Gamescope => {
                        self.bascule_option(|options| options.gamescope = !options.gamescope);
                        Task::none()
                    }
                    ArtMenuEntry::Xbox => {
                        self.bascule_option(|options| options.xbox = !options.xbox);
                        Task::none()
                    }
                    ArtMenuEntry::Resolution => {
                        self.bascule_option(|options| options.resolution_suivante());
                        Task::none()
                    }
                }
            }
            Message::ArtMenuClose => {
                self.art_menu = None;
                Task::none()
            }
            Message::Scrolled { offset, max } => {
                // le maximum du contenu que iced vient de dessiner, pour
                // `scroll_drawn`
                self.scroll_max = max;
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
                // la géométrie AVANT le cran : les vignettes partiront de là
                // (un cran pendant l'animation repart de la position visuelle)
                let avant = self.layout_blend();
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
                    // liste : ses lignes ont leurs trois crans à elles (L2/R2,
                    // +/−). La largeur de colonne ne les touchait pas, donc la
                    // liste n'avait aucun réglage de taille (signalé 23/11/2026).
                    ViewMode::List => {
                        self.list_size = (self.list_size as i32 + step).clamp(0, 2) as u8;
                    }
                    ViewMode::Grid => {
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
                self.remember();
                // glissade vers la nouvelle disposition (aucune en liste, où le
                // zoom ne change pas la taille des vignettes)
                if matches!(self.config.view, ViewMode::Grid | ViewMode::Mosaic) {
                    self.zoom = Some(Zoom {
                        old: avant,
                        elapsed: 0.0,
                    });
                }
                self.request_visible_covers();
                self.status = None;
                Task::none()
            }
            Message::ToggleView => {
                // bascule de liste : le changement se fait au sommet du fondu.
                // La vue courante ne change qu'au sommet, donc un appui répété
                // doit repartir de la bascule DÉJÀ en attente (sinon deux V
                // rapprochés ne feraient qu'une seule bascule).
                let next = match self.pending_swap {
                    Some(Swap::View(view)) => view.flipped(),
                    _ => self.config.view.flipped(),
                };
                self.begin_swap(Swap::View(next));
                Task::none()
            }
            Message::Changed(paths) => {
                self.refresh(paths);
                Task::none()
            }
            Message::SettingsDisques => {
                let actif = self.settings.as_ref().is_some_and(|s| s.disques);
                match crate::config::save_scan_disks(&self.config_path, actif) {
                    Ok(()) => {
                        self.config.scan_disks = actif;
                        // L'index NE DOIT contenir que ce que l'option autorise :
                        // décochée, seuls les dossiers choisis comptent, les
                        // jeux des disques externes quittent la liste et leurs
                        // racines ne sont plus surveillées.
                        self.config.extra_dirs.clear();
                        // la portée « disques scannés » n'a plus de sens
                        if self.scope == Scope::Disques {
                            self.scope = Scope::Tous;
                            self.remember();
                        }
                        if let Some(veille) = self.watch.as_ref() {
                            veille.definir(self.config.scan_dirs());
                        }
                        self.refresh_disques(Vec::new());
                    }
                    Err(err) => {
                        self.status = Some((format!("écriture impossible ({err})"), true))
                    }
                }
                Task::none()
            }
            Message::Discovered(dirs) => {
                if dirs != self.config.extra_dirs {
                    self.config.extra_dirs = dirs;
                    if let Some(veille) = self.watch.as_ref() {
                        veille.definir(self.config.scan_dirs());
                    }
                }
                Task::none()
            }
            Message::Reseau(etat) => {
                self.reseau = etat;
                Task::none()
            }
            Message::Disks => {
                // seuls les montages de /run/media et /media changent : un scan
                // complet par snapshot ne relit que les dossiers dont la mtime
                // a bougé, c'est le prix pour voir arriver un disque inconnu
                if self.config.scan_disks {
                    self.refresh_disques(Vec::new());
                }
                Task::none()
            }
            Message::Refreshed(result) => {
                self.refreshing = false;
                match result {
                    Ok(refresh) => {
                        // La liste change (paquet ajouté, disque branché ou
                        // retiré) : le contenu se réordonne au sommet du même
                        // voile que les bascules de vue, jamais sous les yeux.
                        // Un fondu déjà en cours : on applique sans en empiler
                        // un second.
                        if !self.meme_liste(&refresh) && self.pending_swap.is_none() {
                            self.pending_reload = Some(refresh);
                            self.begin_swap(Swap::Reload);
                        } else {
                            self.apply(&refresh);
                        }
                    }
                    Err(error) => self.status = Some((error, true)),
                }
                if !self.pending.is_empty() || self.pending_disques {
                    let paths = std::mem::take(&mut self.pending);
                    let decouvrir = std::mem::take(&mut self.pending_disques);
                    self.refresh_avec(paths, decouvrir);
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
                    // les vues, puis « Réglages… » et « Galerie… »
                    let last = VIEWS.len() + 1;
                    *choice = (*choice as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::ViewMenuChoose(index) => {
                self.view_menu = None;
                // avant-dernière ligne : les réglages (dossiers de jeux, à la manette)
                if index == VIEWS.len() {
                    return Task::done(Message::SettingsOpen);
                }
                // dernière ligne : la galerie des captures et vidéos
                if index == VIEWS.len() + 1 {
                    return Task::done(Message::GalleryOpen(None));
                }
                let Some(&view) = VIEWS.get(index) else {
                    return Task::none();
                };
                self.begin_swap(Swap::View(view));
                Task::none()
            }
            Message::SortMenu => {
                if self.running.is_some() {
                    return Task::none();
                }
                self.sort_menu = Some(self.sort.index());
                Task::none()
            }
            Message::SortMenuNav(delta) => {
                let entries = self.sort_menu_entries();
                if let Some(choice) = self.sort_menu.as_mut() {
                    // les lignes réelles (`sort_menu_entries`), pas un calcul
                    let last = entries.len().saturating_sub(1) as i32;
                    *choice = (*choice as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::SortMenuChoose(index) => {
                let Some(entry) = self.sort_menu_entries().get(index).copied() else {
                    self.sort_menu = None;
                    return Task::none();
                };
                match entry {
                    // « Effacer les derniers lancés » : vide l'historique, pas
                    // les temps de jeu
                    SortMenuEntry::Clear => {
                        self.sort_menu = None;
                        self.effacer_derniers();
                        self.status = Some(("Derniers lancés effacés".to_string(), false));
                    }
                    // le filtre d'année ouvre son propre menu
                    SortMenuEntry::Year => {
                        self.sort_menu = None;
                        self.year_menu = Some(year_index(self.year_filter, self.known_years()));
                    }
                    // la portée cycle sur place : le menu reste ouvert pour
                    // qu'on voie la valeur changer, et la liste suit tout de
                    // suite (sans annoncer la ligne vide, la pastille de tri
                    // dit déjà la portée)
                    SortMenuEntry::Scope => {
                        self.scope = self.scope.next();
                        self.remember();
                        self.apply_filter();
                        self.selected = self.selected.min(self.games.len().saturating_sub(1));
                        self.update_scroll_target();
                        self.request_visible_covers();
                    }
                    SortMenuEntry::Sort(sort) => {
                        self.sort_menu = None;
                        self.begin_swap(Swap::Sort(sort));
                    }
                }
                Task::none()
            }
            Message::YearMenuNav(delta) => {
                let last = self.known_years().len();
                if let Some(choice) = self.year_menu.as_mut() {
                    *choice = (*choice as i32 + delta).clamp(0, last as i32) as usize;
                }
                Task::none()
            }
            Message::YearMenuChoose(index) => {
                self.year_menu = None;
                let year = if index == 0 {
                    None
                } else {
                    self.known_years().get(index - 1).copied()
                };
                self.begin_swap(Swap::Year(year));
                Task::none()
            }
            Message::Back => {
                // même priorité qu'Échap : le plus profond d'abord
                if self.date.is_some() {
                    self.date = None;
                    return Task::none();
                }
                if self.identify.is_some() {
                    self.identify = None;
                    return Task::none();
                }
                if self.identify_menu.is_some() {
                    return Task::done(Message::IdentifyClose);
                }
                if self.picker.is_some() {
                    return Task::done(Message::ClosePicker);
                }
                if self.replace.is_some() {
                    return Task::done(Message::ReplaceChoose(0));
                }
                if self.art_menu.is_some() {
                    return Task::done(Message::ArtMenuClose);
                }
                if self.view_menu.is_some() {
                    self.view_menu = None;
                    return Task::none();
                }
                if self.year_menu.is_some() {
                    self.year_menu = None;
                    return Task::none();
                }
                if self.sort_menu.is_some() {
                    self.sort_menu = None;
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
                // Les décorations ne se touchent JAMAIS ici : sous Wayland
                // (xdg-decoration) elles sont négociées à la création de la
                // fenêtre, `toggle_decorations` n'a aucun effet ensuite, et
                // sortir du plein écran laissait une fenêtre sans barre de
                // titre (signalé puis confirmé le 20/11). La fenêtre est donc
                // toujours créée décorée — KWin n'affiche pas de barre en
                // plein écran — donc paramètre fixe dans `main.rs`.
                match self.window {
                    Some(id) => window::set_mode(id, mode).map(|()| Message::Nothing),
                    None => Task::none(),
                }
            }
            // une image de la poussière d'étoiles : le temps seul avance, les
            // positions se recalculent dans `draw` (aucun état à balayer)
            Message::Sparkle => {
                self.sparkles.advance();
                Task::none()
            }
            // réveil du worker de téléchargements : ce message n'a d'autre but
            // que de faire reconstruire la vue et démarrer les images (la roue
            // de chargement apparaît ainsi au DÉBUT de la requête)
            Message::Activity => Task::none(),
            // mise à jour de l'image : une seule à la fois, et jamais deux
            // commandes concurrentes sur le même déploiement
            Message::UpdateApply => {
                let deja = self
                    .update
                    .as_ref()
                    .is_some_and(|update| !update.done && update.error.is_none());
                if deja {
                    return Task::none();
                }
                self.update = Some(updates::Update::default());
                updates::spawn(updates::default_command(), |event| {
                    post(Message::Update(event));
                });
                Task::none()
            }
            Message::SettingsOpen => {
                // jamais pendant une partie : le launcher est caché, et la
                // manette pilote le jeu
                if self.running.is_some() {
                    return Task::none();
                }
                self.settings = Some(Settings::nouveau(&self.config));
                Task::none()
            }
            Message::SettingsClose => {
                self.settings = None;
                Task::none()
            }
            Message::SettingsNav(delta) => {
                if let Some(settings) = self.settings.as_mut()
                    && settings.browser.is_none()
                {
                    let last = settings.lignes().saturating_sub(1) as i32;
                    settings.selected =
                        (settings.selected as i32 + delta).clamp(0, last) as usize;
                    // changer de ligne désarme : on ne confirme que ce qu'on voit
                    settings.confirm = None;
                }
                Task::none()
            }
            Message::SettingsArm => {
                // dossier de sortie : Carré remet le défaut (la clé disparaît,
                // le démon et le script reprennent leur chemin habituel)
                let sortie = self.settings.as_ref().and_then(|settings| {
                    settings
                        .browser
                        .is_none()
                        .then(|| settings.cle_sortie(settings.selected))
                        .flatten()
                });
                if let Some(cle) = sortie {
                    return Task::done(Message::SettingsDefault(cle));
                }
                if let Some(settings) = self.settings.as_mut()
                    && settings.browser.is_none()
                    // seules les lignes de dossier se retirent : ni « ajouter »,
                    // ni les dossiers de sortie, ni la préférence d'écran
                    && settings.selected > 0
                    && settings.selected <= settings.dossiers.len()
                {
                    settings.confirm = Some(settings.selected - 1);
                }
                Task::none()
            }
            Message::SettingsDefault(cle) => {
                let Some(settings) = self.settings.as_mut() else {
                    return Task::none();
                };
                match recorder::definir(cle, None) {
                    Ok(()) => {
                        if cle == recorder::CLE_CAPTURES {
                            settings.captures = None;
                        } else {
                            settings.shots = None;
                        }
                        let quoi = if cle == recorder::CLE_CAPTURES {
                            "enregistrements"
                        } else {
                            "captures d'écran"
                        };
                        self.status = Some((format!("{quoi} : dossier par défaut"), false));
                    }
                    Err(err) => {
                        self.status = Some((format!("écriture impossible ({err})"), true));
                    }
                }
                Task::none()
            }
            Message::SettingsAct => {
                let Some(settings) = self.settings.as_mut() else {
                    return Task::none();
                };
                // confirmation armée : Croix retire vraiment (le disque n'est
                // jamais touché, seule la liste change)
                if let Some(index) = settings.confirm.take() {
                    let Some(retire) = folders::retirer(&mut settings.dossiers, index) else {
                        return Task::none();
                    };
                    settings.selected = settings.selected.min(settings.lignes() - 1);
                    return Task::done(Message::SettingsChanged(format!(
                        "{} retiré de la liste",
                        retire
                    )));
                }
                // ligne écran : elle ne réagit qu'à gauche/droite
                if settings.selected == settings.ligne_ecran() {
                    return Task::none();
                }
                // entretien : bascule le scan des disques externes
                if settings.selected == settings.ligne_disques() {
                    settings.disques = !settings.disques;
                    return Task::done(Message::SettingsDisques);
                }
                // entretien : ouvre le dialogue de confirmation
                if settings.selected == settings.ligne_oublier() {
                    return Task::done(Message::OublierOpen);
                }
                // ligne musique : Croix coupe / relance, tout de suite
                if settings.selected == settings.ligne_musique() {
                    settings.musique = !settings.musique;
                    return Task::done(Message::SettingsMusic);
                }
                // dossier de sortie : naviguer à partir du dossier courant
                if let Some(cle) = settings.cle_sortie(settings.selected) {
                    let courant = if cle == recorder::CLE_CAPTURES {
                        settings.captures.clone()
                    } else {
                        settings.shots.clone()
                    };
                    let defaut = if cle == recorder::CLE_CAPTURES {
                        recorder::defaut_captures()
                    } else {
                        recorder::defaut_shots()
                    };
                    let depart = courant
                        .map(PathBuf::from)
                        .filter(|chemin| chemin.is_dir())
                        .or_else(|| {
                            let defaut = PathBuf::from(&defaut);
                            defaut.is_dir().then_some(defaut)
                        })
                        .unwrap_or_else(|| {
                            std::env::var_os("HOME")
                                .map(PathBuf::from)
                                .unwrap_or_else(|| PathBuf::from("/"))
                        });
                    let choix = if cle == recorder::CLE_CAPTURES {
                        Choix::Captures
                    } else {
                        Choix::Shots
                    };
                    self.ouvrir_navigateur(depart, choix);
                    return Task::none();
                }
                let cible = if settings.selected == 0 {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("/"))
                } else {
                    let Some(dossier) = settings.dossiers.get(settings.selected - 1) else {
                        return Task::none();
                    };
                    folders::resolu(Path::new(dossier))
                };
                self.ouvrir_navigateur(cible, Choix::Jeux);
                Task::none()
            }
            Message::SettingsChanged(message) => self.persist_dirs(message),
            Message::SettingsMusic => self.persist_musique(),
            Message::SettingsChoix(delta) => {
                // volume de la musique : gauche/droite par pas de 5 %
                let volume = {
                    let Some(settings) = self.settings.as_mut() else {
                        return Task::none();
                    };
                    if settings.browser.is_some() || settings.selected != settings.ligne_volume() {
                        None
                    } else {
                        let pas = 5 * delta.signum();
                        settings.volume = (settings.volume as i32 + pas).clamp(0, 100) as u8;
                        settings.confirm = None;
                        Some(settings.volume)
                    }
                };
                if volume.is_some() {
                    return Task::done(Message::SettingsMusic);
                }
                let choix = {
                    let Some(settings) = self.settings.as_mut() else {
                        return Task::none();
                    };
                    if settings.browser.is_some() || settings.selected != settings.ligne_ecran() {
                        return Task::none();
                    }
                    let nombre = ECRANS.len() as i32;
                    settings.ecran = (settings.ecran as i32 + delta).rem_euclid(nombre) as usize;
                    settings.confirm = None;
                    settings.ecran
                };
                let (_, libelle) = ECRANS[choix];
                self.status = Some(match ecran_ecrit(choix) {
                    Ok(()) => (format!("écran du jeu : {libelle}"), false),
                    Err(err) => (format!("écran : écriture impossible ({err})"), true),
                });
                Task::none()
            }
            Message::BrowserNav(delta) => {
                if let Some(browser) = self.settings.as_mut().and_then(|s| s.browser.as_mut()) {
                    let last = browser.entrees.len() as i32; // ligne 0 = choisir
                    browser.selected = (browser.selected as i32 + delta).clamp(0, last) as usize;
                }
                Task::none()
            }
            Message::BrowserAct => {
                let Some((choisir, chemin)) = self.browser_cible() else {
                    return Task::none();
                };
                if !choisir {
                    self.naviguer_vers(chemin, None);
                    return Task::none();
                }
                let Some(settings) = self.settings.as_mut() else {
                    return Task::none();
                };
                let choix = settings
                    .browser
                    .as_ref()
                    .map(|browser| browser.choix)
                    .unwrap_or(Choix::Jeux);
                if choix != Choix::Jeux {
                    let (cle, quoi, ligne) = if choix == Choix::Captures {
                        (
                            recorder::CLE_CAPTURES,
                            "enregistrements",
                            settings.ligne_captures(),
                        )
                    } else {
                        (recorder::CLE_SHOTS, "captures d'écran", settings.ligne_shots())
                    };
                    let valeur = chemin.display().to_string();
                    return match recorder::definir(cle, Some(&valeur)) {
                        Ok(()) => {
                            if choix == Choix::Captures {
                                settings.captures = Some(valeur.clone());
                            } else {
                                settings.shots = Some(valeur.clone());
                            }
                            settings.browser = None;
                            settings.selected = ligne;
                            self.status =
                                Some((format!("{quoi} : {}", folders::affiche(&chemin)), false));
                            Task::none()
                        }
                        Err(err) => {
                            self.status = Some((format!("écriture impossible ({err})"), true));
                            Task::none()
                        }
                    };
                }
                match folders::ajouter(&mut settings.dossiers, &chemin) {
                    Ok(()) => {
                        settings.browser = None;
                        settings.selected = settings.lignes() - 1;
                        let libelle = folders::affiche(&chemin);
                        Task::done(Message::SettingsChanged(format!("{libelle} ajouté")))
                    }
                    Err(err) => {
                        self.status = Some((err, true));
                        Task::none()
                    }
                }
            }
            Message::BrowserUp => {
                let Some(chemin) = self
                    .settings
                    .as_ref()
                    .and_then(|s| s.browser.as_ref())
                    .map(|b| b.chemin.clone())
                else {
                    return Task::none();
                };
                match chemin.parent() {
                    Some(parent) => {
                        self.naviguer_vers(parent.to_path_buf(), Some(&chemin));
                        Task::none()
                    }
                    // racine : Rond referme le navigateur, on ne reste pas coincé
                    None => {
                        if let Some(settings) = self.settings.as_mut() {
                            settings.browser = None;
                        }
                        Task::none()
                    }
                }
            }
            Message::Update(event) => {
                let Some(update) = self.update.as_mut() else {
                    return Task::none();
                };
                match event {
                    updates::Event::Line(step) => update.apply(step),
                    updates::Event::Detail(line) => {
                        if !line.trim().is_empty() {
                            update.detail = line;
                        }
                    }
                    updates::Event::Exited(code) => update.finish(code),
                    updates::Event::Failed(err) => {
                        update.error = Some(err.clone());
                        update.label = format!("mise à jour impossible : {err}");
                    }
                }
                Task::none()
            }
            Message::Quit => iced::exit(),            Message::Nothing => Task::none(),
        }
    }

    // --- actions ------------------------------------------------------------

    fn navigate(&mut self, nav: Nav) -> Task<Message> {
        if self.games.is_empty() {
            return Task::none();
        }
        let step = match self.config.view {
            ViewMode::Grid | ViewMode::Mosaic => self.metrics().2,
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
        self.noter_lancement(&game.slug);
        self.running = Some(game.name.clone());
        self.pending_slug = Some(game.slug.clone());
        self.pending_key = Some(self.key_of(&game.slug));
        self.pending_kind = Some(game.kind);
        self.status = None;
        // le jeu démarre quand le fondu au noir est terminé (voir tick)
        self.to_launch = Some((game.name.clone(), command, std::time::Instant::now()));
        self.fade_target = 1.0;
        // la musique s'efface pendant ce même voile noir : le jeu n'a pas de
        // musique par-dessus son propre son
        if let Some(music) = &self.music {
            music.fade_out();
        }
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
        let ecoule = now.duration_since(self.last_tick).as_secs_f32();
        let dt = ecoule.clamp(0.001, 0.05);
        self.last_tick = now;

        // Sonde de fluidité (`GLAUNCHER_ANIM_DEBUG=1`) : intervalle RÉEL entre
        // deux images pendant un fondu (16,7 ms = 60 Hz ; au-delà de ~20 ms
        // l'œil voit une saccade). Sert à trancher entre « c'est l'animation
        // qui rame » et « c'est le compositeur ».
        if anim_debug() && (self.fade < 0.999 || (self.fade - self.fade_target).abs() > 0.001) {
            eprintln!(
                "glauncher : image {} ms (fade {:.3} → {:.1}, page {:.2})",
                (ecoule * 1000.0).round(),
                self.fade,
                self.fade_target,
                self.page_fade
            );
        }

        // Poussière et respiration : le temps avance du pas RÉEL quand la
        // fenêtre fournit les images (`window::frames`, 60 Hz). Quand le
        // minuteur à 20 Hz tourne, c'est `Message::Sparkle` qui l'avance (pas
        // fixe) : jamais les deux, sinon le temps courrait 1,3× trop vite.
        if !sparkles::running() {
            self.sparkles.advance_by(dt);
        }

        // Changement de taille : la glissade vers la nouvelle disposition
        if let Some(zoom) = self.zoom.as_mut() {
            zoom.elapsed += dt;
        }
        if self.zoom.is_some_and(|zoom| zoom.elapsed >= zoom_seconds()) {
            self.zoom = None;
        }

        // roue de chargement : elle tourne tant qu'un téléchargement court ou
        // vient de finir
        if self.loading() || self.scanning() {
            self.spin = (self.spin + dt * SPIN_SPEED) % std::f32::consts::TAU;
        }

        // titre trop long : son horloge repart de zéro quand la sélection
        // change, pour qu'on relise le nom depuis le début
        if self.title_scrolls() {
            let slug = self.games.get(self.selected).map(|game| game.slug.clone());
            if slug != self.title_game {
                self.title_game = slug;
                self.title_elapsed = 0.0;
            }
            self.title_elapsed += dt;
        }

        // l'invite « appuie encore sur retour » expire avec la fenêtre
        if self.quit_hint && self.back_press.is_none_or(|at| at.elapsed() >= QUIT_WINDOW) {
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
        // voile entièrement retiré : les covers ont fini de se reposer
        if self.returning && self.fade <= 0.001 {
            self.returning = false;
        }

        // Fondu enchaîné de la page : on monte vers le fond, on bascule au
        // sommet (le fond est alors opaque, le changement d'écran ne se voit
        // pas), puis on redescend sur le nouveau contenu. Aucun saut —
        // contrairement à un passage au noir instantané, jugé trop agressif.
        if (self.page_fade - self.page_fade_target).abs() > 0.001 {
            let rising = self.page_fade < self.page_fade_target;
            let step = dt
                / if rising {
                    PAGE_FADE_RISE
                } else {
                    PAGE_FADE_FALL
                };
            self.page_fade = if rising {
                (self.page_fade + step).min(self.page_fade_target)
            } else {
                (self.page_fade - step).max(self.page_fade_target)
            };
        }
        if self.page_fade >= 0.999
            && let Some(step) = self.pending_page.take()
        {
            match step {
                PageStep::Open(index) => {
                    self.page = Some(Page { index, action: 0 });
                    self.reset_page_assets(index);
                }
                PageStep::Close => {
                    self.page = None;
                    self.art_menu = None;
                    self.page_hero = 0.0;
                    self.page_logo = 0.0;
                    self.page_cover = 0.0;
                    // de retour sur la liste : les covers de fond reprennent
                    self.resume_covers();
                }
            }
            // on ressort du fond sur le nouvel écran
            self.page_fade_target = 0.0;
        }

        // Visuels arrivés après l'ouverture de leur page : la bande du héros
        // grandit, le logo et la cover montent en opacité.
        let assets = self.page_game().map(|game| {
            (
                game.hero.is_some(),
                game.logo.is_some(),
                game.cover.is_some(),
            )
        });
        if let Some((hero, logo, cover)) = assets {
            advance_asset(&mut self.page_hero, hero, dt);
            advance_asset(&mut self.page_logo, logo, dt);
            advance_asset(&mut self.page_cover, cover, dt);
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
            let key = self.pending_key.take().unwrap_or_else(|| slug.clone());
            let kind = self.pending_kind.take();
            self.play_started = if slug.is_empty() {
                None
            } else {
                Some((
                    slug.clone(),
                    kind.unwrap_or(library::Kind::Wgp),
                    std::time::Instant::now(),
                ))
            };
            spawn_launch(name, command, self.config.cache_dir(), slug, key);
        }

        // lecture d'un film : lancement IMMÉDIAT (plus de montée en fondu de
        // ~1 s avec la poussière qui file). Le voile noir est déjà posé ; il
        // ne se lèvera qu'après l'ouverture de la fenêtre de mpv, sinon on
        // voyait son animation « venir de bas à droite » (constaté le 23/11).
        if let Some((fichier, nom, started)) = self.a_lancer_video.take() {
            match play::Lecteur::lance(&fichier) {
                Some(lecteur) => {
                    eprintln!(
                        "glauncher : lecture de {nom} demandée en {} ms",
                        started.elapsed().as_millis()
                    );
                    self.lecteur = Some(lecteur);
                    self.video_lift_at = Some(now + Duration::from_millis(400));
                    self.status = Some((format!("lecture : {nom}"), false));
                }
                None => {
                    self.video_veil_target = 0.0;
                    self.status = Some(("lecture impossible (mpv absent ?)".to_string(), true));
                }
            }
        }
        // la fenêtre de mpv a eu le temps de s'ouvrir : le voile se lève
        if let Some(at) = self.video_lift_at
            && now >= at
        {
            self.video_lift_at = None;
            self.video_veil_target = 0.0;
        }
        if (self.video_veil - self.video_veil_target).abs() > 0.001 {
            let pas = dt / 0.18;
            self.video_veil = if self.video_veil < self.video_veil_target {
                (self.video_veil + pas).min(self.video_veil_target)
            } else {
                (self.video_veil - pas).max(self.video_veil_target)
            };
        }
        // entrée de l'aperçu d'une capture : 0,16 s, à 60 Hz
        if let Some(apercu) = self.apercu.as_mut()
            && apercu.entree > 0.0
        {
            apercu.entree = (apercu.entree - dt / 0.16).max(0.0);
        }
        // menus et entrée en galerie : voile noir qui se dissipe (0,18 s)
        if self.overlay_fade > 0.0 {
            self.overlay_fade = (self.overlay_fade - dt / FONDU_MENUS).max(0.0);
        }

        // Bascule de liste : montée vers le fond, échange au sommet, descente.
        if (self.swap_fade - self.swap_fade_target).abs() > 0.001 {
            let rising = self.swap_fade < self.swap_fade_target;
            let step = dt / if rising { SWAP_RISE } else { SWAP_FALL };
            self.swap_fade = if rising {
                (self.swap_fade + step).min(self.swap_fade_target)
            } else {
                (self.swap_fade - step).max(self.swap_fade_target)
            };
        }
        if self.swap_fade >= SWAP_PEAK - 0.001
            && let Some(swap) = self.pending_swap.take()
        {
            tasks.push(self.apply_swap(swap));
            self.swap_fade_target = 0.0;
        }

        // iced borne le défilement au maximum du contenu : viser au-delà ne
        // dessine rien de plus. Sans ce bornage, la dernière rangée laissait
        // `scroll`/`scroll_target` en avance de ~une demi-rangée sur le dessin.
        if self.scroll_max > 0.0 {
            self.scroll_target = self.scroll_target.min(self.scroll_max);
            self.scroll = self.scroll.min(self.scroll_max);
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

    /// Défilement tel que le `scrollable` va le **dessiner** (voir
    /// [`scroll_drawn`]) : c'est lui qui positionne vignettes, cadre d'accent
    /// et particules.
    fn scroll_drawn(&self) -> f32 {
        scroll_drawn(self.scroll, self.scroll_max)
    }

    /// Rectangles de la case `index` (cadre compris) et de son image, en
    /// pixels de la fenêtre. Repose sur la même géométrie que
    /// `update_scroll_target` (marges, colonnes, pas de rangée, défilement).
    /// Grille et favoris seulement.
    fn slot_rect(&self, index: usize) -> Option<(Rectangle, Rectangle)> {
        if !matches!(self.config.view, ViewMode::Grid) {
            return None;
        }
        self.rects_in(index, &self.layout_now())
    }

    /// Géométrie de la disposition courante (grille ou mosaïque).
    fn layout_now(&self) -> Layout {
        let (cell, art, columns) = self.metrics();
        Layout {
            cell,
            art,
            columns,
            slot_height: self.slot_height(),
            gap: self.gap(),
            inset: self.cell_inset(),
            scroll: self.scroll_drawn(),
        }
    }

    /// Géométrie **visuelle** d'une vignette : si un changement de taille est en
    /// cours, `old` et `now` sont mélangés au prorata de l'animation. Un second
    /// cran capturé ici repart donc exactement d'où les vignettes sont, et non
    /// de la disposition précédente déjà quittée.
    fn layout_blend(&self) -> Layout {
        let now = self.layout_now();
        let Some(zoom) = self.zoom.as_ref() else {
            return now;
        };
        let e = ease_reveal((zoom.elapsed / zoom_seconds()).clamp(0.0, 1.0));
        let old = zoom.old;
        Layout {
            cell: old.cell + (now.cell - old.cell) * e,
            art: old.art + (now.art - old.art) * e,
            // les colonnes ne se mélangent pas : c'est la disposition NOUVELLE
            // qui place les vignettes, l'animation rattrape leur ancienne case
            columns: now.columns,
            slot_height: old.slot_height + (now.slot_height - old.slot_height) * e,
            gap: now.gap,
            inset: now.inset,
            scroll: now.scroll,
        }
    }

    /// Position de la vignette `index` dans une géométrie donnée : la case (fond
    /// de sélection, titre) et la cover, en coordonnées écran.
    fn rects_in(&self, index: usize, layout: &Layout) -> Option<(Rectangle, Rectangle)> {
        self.games.get(index)?;
        let columns = layout.columns.max(1);
        let column = (index % columns) as f32;
        let row = (index / columns) as f32;
        let mosaic = matches!(self.config.view, ViewMode::Mosaic);
        let pad = if mosaic { 0.0 } else { PADDING };
        let height = layout.art * ART_RATIO + 2.0 * pad + if mosaic { 0.0 } else { CARD_CAPTION };
        let card = Rectangle::new(
            Point::new(
                PAGE_PADDING_X + column * (layout.cell + layout.gap) + layout.inset,
                PAGE_PADDING_Y + row * (height + 2.0 * layout.inset + layout.gap) + layout.inset
                    - layout.scroll,
            ),
            Size::new(layout.cell - 2.0 * layout.inset, height),
        );
        let art = Rectangle::new(
            Point::new(card.x + pad, card.y + pad),
            Size::new(layout.art, layout.art * ART_RATIO),
        );
        Some((card, art))
    }

    /// Animation du changement de taille : échelle et décalage à appliquer à la
    /// vignette `index` pour qu'elle parte de son ancienne case et arrive à la
    /// nouvelle. `None` au repos (aucune transformation, aucun coût).
    /// `pivot` = point fixe de la transformation, en coordonnées locales à la
    /// vignette (son coin haut-gauche pour origine) : le même que celui des
    /// `Pose` de la carte, sinon la cover arriverait à côté de sa case.
    fn zoom_pose(&self, index: usize, pivot: Vector) -> Option<(f32, Vector)> {
        let zoom = self.zoom.as_ref()?;
        if !matches!(self.config.view, ViewMode::Grid | ViewMode::Mosaic) {
            return None;
        }
        let (_, old_art) = self.rects_in(index, &zoom.old)?;
        let (card, art) = self.slot_rect_for(index)?;
        if old_art.width <= 0.0 || art.width <= 0.0 {
            return None;
        }
        let e = ease_reveal((zoom.elapsed / zoom_seconds()).clamp(0.0, 1.0));
        Some(zoom_transform(&old_art, &card, &art, card.position() + pivot, e))
    }

    /// Case d'une vignette dans la disposition courante, mosaïque comprise.
    fn slot_rect_for(&self, index: usize) -> Option<(Rectangle, Rectangle)> {
        self.rects_in(index, &self.layout_now())
    }

    /// Le canvas des particules dessine lui-même le cadre de sélection — il
    /// doit grossir avec la vignette — donc la carte laisse sa bordure
    /// transparente : deux cadres se superposeraient pendant la respiration.
    fn canvas_frame(&self) -> bool {
        self.config.sparkles && matches!(self.config.view, ViewMode::Grid)
    }

    /// Facteur de respiration de la vignette sélectionnée (`1.0` au repos).
    /// Calculé une seule fois par image et partagé par la carte (`Pose`) et par
    /// le cadre du canvas, qui doivent grossir **exactement** ensemble.
    fn breath_factor(&self) -> f32 {
        sparkles::breath_factor(self.sparkles.time(), self.breath_pulses())
    }

    /// Distance verticale entre le centre d'une vignette et celui de son titre,
    /// c'est-à-dire le point autour duquel la respiration grossit : le titre
    /// reste immobile (aucune re-rasterisation, aucun saut d'un pixel), la
    /// vignette monte au-dessus de son nom.
    fn caption_pivot(&self) -> f32 {
        let (_, art_width, _) = self.metrics();
        let art_height = art_width * ART_RATIO;
        PADDING + art_height + caption_middle() - self.row_height() * 0.5
    }

    /// La vignette sélectionnée respire-t-elle ? Sert au facteur d'échelle
    /// **et** à l'abonnement aux images : la respiration est continue, donc
    /// cadencée par la fenêtre (60 Hz) et non par le minuteur des particules
    /// (20 Hz), qui la faisait avancer par paliers de 50 ms — « pas assez
    /// fluide » (retour du 22/11).
    fn breath_pulses(&self) -> bool {
        if !self.canvas_frame() || self.returning {
            return false;
        }
        let Some((_, art)) = self.slot_rect(self.selected) else {
            return false;
        };
        // une respiration qui mord le pied de page reste moche : la vignette
        // se pose alors sans bouger. Sous le voile du lancement, elle s'arrête
        // aussi (la cover ne « flotte » pas dans un écran noir).
        art.y >= PAGE_PADDING_Y - 0.5
            && art.y + art.height <= self.viewport.1 - FOOTER_BAND
            && self.fade <= sparkles::VEIL_CLEAR
    }

    /// La vignette sélectionnée confiée au canvas : il ne redessine que son
    /// cadre d'accent, à l'échelle de la carte.
    fn breathing(&self) -> Option<sparkles::Breath> {
        // Pendant le retour d'un jeu, les vignettes glissent depuis les bords
        // (`comeback`) : le canvas ne peut pas les suivre, il s'efface pour
        // laisser la carte peindre la sienne, sinon la sélection resterait
        // figée pendant que les autres arrivent.
        if !self.canvas_frame() || self.returning {
            return None;
        }
        self.games.get(self.selected)?;
        let (card, art) = self.slot_rect(self.selected)?;
        // pivot de la carte (centre du titre), en coordonnées locales à la case
        let pivot = Vector::new(card.width * 0.5, card.height * 0.5 + self.caption_pivot());
        // la glissade du changement de taille s'applique au cadre comme à la
        // carte, sinon il resterait à la nouvelle place pendant que l'image
        // arrive de l'ancienne
        let (zoom_scale, zoom_offset) =
            self.zoom_pose(self.selected, pivot).unwrap_or((1.0, Vector::ZERO));
        let breath = self.breath_factor();
        Some(sparkles::Breath {
            pivot: card.position() + pivot,
            art,
            radius: CARD_RADIUS,
            frame_color: ACCENT,
            frame_width: FOCUS_BORDER,
            factor: breath * zoom_scale,
            offset: zoom_offset,
        })
    }

    /// Retour de jeu : décalage horizontal et échelle de la vignette `index`,
    /// pour qu'elle revienne de son côté de l'écran en dézoomant. Les vignettes
    /// du bord partent de plus loin et se posent plus tard que celles du
    /// centre. `None` quand il n'y a rien à animer (hors retour, autre vue).
    fn comeback(&self, index: usize) -> Option<(f32, f32)> {
        if !self.returning || self.fade <= 0.001 || !matches!(self.config.view, ViewMode::Grid) {
            return None;
        }
        let (cell, _, columns) = self.metrics();
        let columns = columns.max(1);
        let column = (index % columns) as f32;
        let center = (columns as f32 - 1.0) * 0.5;
        // 0 au centre, 1 sur les colonnes du bord
        let away = (column - center).abs() / center.max(0.5);
        let dir = if column < center { -1.0 } else { 1.0 };
        let delay = RETURN_STAGGER * (1.0 - away);
        let span = (1.0 - RETURN_END - delay).max(0.01);
        let progress = ((self.fade - RETURN_END - delay) / span).clamp(0.0, 1.0);
        let remaining = ease_reveal(progress);
        let travel = (self.viewport.0 * RETURN_SPREAD + cell) * (0.55 + 0.45 * away);
        Some((1.0 + RETURN_ZOOM * remaining, dir * travel * remaining))
    }

    /// Position de défilement qui garde la sélection à l'écran (centrée).
    fn update_scroll_target(&mut self) {
        let columns = self.metrics().2;
        let (row, row_height) = match self.config.view {
            ViewMode::Grid | ViewMode::Mosaic => {
                (self.selected / columns, self.slot_height() + self.gap())
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
    /// Slugs qui traînent dans les fiches (favoris, historique, temps de jeu,
    /// années, identifications, réglages de lancement) alors que le jeu n'est
    /// plus dans la liste : ce que « Oublier les jeux disparus » efface.
    fn slugs_disparus(&self) -> HashSet<String> {
        let connus: HashSet<String> = self
            .all_games
            .iter()
            .map(|game| game.slug.clone())
            .collect();
        let mut tous: HashSet<String> = HashSet::new();
        tous.extend(self.favorites.iter().cloned());
        tous.extend(self.recent.iter().map(|(slug, _)| slug.clone()));
        tous.extend(self.playtime.keys().cloned());
        tous.extend(self.years.keys().cloned());
        tous.extend(self.identities.keys().cloned());
        // réglages de lancement : fichier relu (petit, une ligne par jeu réglé)
        if let Ok(texte) = std::fs::read_to_string(pergame::chemin()) {
            tous.extend(pergame::parse(&texte).into_keys());
        }
        tous.retain(|slug| !connus.contains(slug));
        tous
    }

    /// Efface les fiches des jeux absents — favoris, historique, temps de jeu,
    /// années, identifications, réglages, lignes d'index — et **garde les
    /// images** (`assets/`, cache des vignettes) : un jeu rebranché retrouve
    /// ses visuels, mais repart sans favori ni temps de jeu.
    /// Renvoie le nombre de jeux oubliés.
    fn oublier_disparus(&mut self) -> usize {
        let disparus = self.slugs_disparus();
        if disparus.is_empty() {
            return 0;
        }
        let garde: HashSet<String> = self
            .all_games
            .iter()
            .map(|game| game.slug.clone())
            .collect();

        self.favorites.retain(|slug| garde.contains(slug));
        let _ = favorites::save(&self.config.favorites_path(), &self.favorites);

        let recent_path = recent::path(&self.config.cache_dir());
        recent::retain(&recent_path, &garde);
        self.recent = recent::load(&recent_path);

        playtime::retain(&self.playtime_paths, &garde);
        self.playtime = playtime::load(&self.playtime_paths);

        let assets = self.config.assets_dir();
        covers::oublier_slugs(&covers::years_path(&assets), &garde);
        self.years.retain(|slug, _| garde.contains(slug));
        covers::oublier_slugs(&covers::identities_path(&assets), &garde);
        self.identities.retain(|slug, _| garde.contains(slug));

        pergame::oublier(&garde);

        // index : les paquets dont le fichier n'existe plus (disque absent)
        if let Ok(mut slot) = self.index.lock()
            && let Some(index) = slot.as_mut()
        {
            let _ = index.purge_missing();
        }

        self.apply_filter();
        self.selected = self.selected.min(self.games.len().saturating_sub(1));
        self.update_scroll_target();
        self.request_visible_covers();
        disparus.len()
    }

    /// Destination choisie : on regarde ce qu'il y a sur place avant de
    /// promettre quoi que ce soit. Rien n'est écrasé sans confirmation, et un
    /// fichier déjà à jour ne déclenche aucune copie.
    fn copy_pick(&mut self) -> Task<Message> {
        let Some(menu) = self.copy_menu.take() else {
            return Task::none();
        };
        let Some(dest) = menu.dests.get(menu.index) else {
            return Task::none();
        };
        let Some(game) = self.jeu_courant() else {
            return Task::none();
        };
        let source = game.path.clone();
        match copy::prepare(&source, &dest.dir) {
            Err(err) => self.status = Some((err, true)),
            Ok(copy::Prepare::Existe(path)) => {
                eprintln!("glauncher : copie inutile, déjà présent : {}", path.display());
                self.status = Some((
                    format!("déjà présent dans {} — rien à copier", folders::affiche(&path)),
                    false,
                ));
            }
            Ok(copy::Prepare::Copier(job)) => {
                if self.place_pour(&dest.dir, job.size) {
                    self.enqueue_copy(job);
                }
            }
            Ok(copy::Prepare::Remplacer(job)) => {
                if !self.place_pour(&dest.dir, job.size) {
                    return Task::none();
                }
                self.copy_replace = Some(CopyReplace { job, index: 0 });
            }
        }
        Task::none()
    }

    /// Y a-t-il la place pour `size` octets dans `dir`, une fois les copies déjà
    /// en file comptées ? La réponse est dite à l'écran quand elle est non.
    fn place_pour(&mut self, dir: &Path, size: u64) -> bool {
        // ce qui reste à écrire pour les copies en attente compte : deux jeux
        // lancés d'affilée ne doivent pas dépasser ensemble l'espace libre
        let reserve = self
            .copies
            .as_ref()
            .map(|copies| {
                copies
                    .total
                    .saturating_sub(copies.fait + copies.courant)
            })
            .unwrap_or(0);
        let Some(libre) = copy::libre(dir) else {
            // place illisible : on ne bloque pas, l'échec arrivera au pire
            return true;
        };
        if libre >= size + reserve {
            return true;
        }
        self.status = Some((
            format!(
                "place insuffisante dans {} : {} libres, {} nécessaires",
                folders::affiche(dir),
                copy::poids(libre.saturating_sub(reserve)),
                copy::poids(size)
            ),
            true,
        ));
        false
    }

    /// Met une copie dans la file (une seule barre pour toutes les copies
    /// demandées), et démarre la première.
    fn enqueue_copy(&mut self, job: copy::Job) {
        // même destination déjà en cours ou en attente : une seule copie
        // (deux appuis rapides sur Croix écriraient le même fichier en même
        // temps, chacun dans son `.part`)
        if let Some(copies) = self.copies.as_ref()
            && (copies.en_cours.as_ref() == Some(&job.dest)
                || copies.attente.iter().any(|autre| autre.dest == job.dest))
        {
            self.status = Some((
                format!("{} est déjà en cours de copie", job.nom),
                false,
            ));
            return;
        }
        let size = job.size;
        let nom = job.nom.clone();
        let copies = self.copies.get_or_insert_with(Copies::default);
        copies.total += size;
        copies.nombre += 1;
        let demarre = !copies.attente.is_empty() || !copies.nom.is_empty();
        copies.attente.push_back(job);
        if !demarre {
            self.demarrer_copie();
        } else {
            self.status = Some((format!("{nom} en file d'attente"), false));
        }
    }

    fn demarrer_copie(&mut self) {
        let Some(copies) = self.copies.as_mut() else {
            return;
        };
        let Some(job) = copies.attente.pop_front() else {
            return;
        };
        copies.courant = 0;
        copies.nom = job.nom.clone();
        copies.en_cours = Some(job.dest.clone());
        eprintln!(
            "glauncher : copie de {} vers {}",
            job.nom,
            job.dest.display()
        );
        self.status = Some((format!("copie de {}…", job.nom), false));
        copy::spawn(job, 0, |event| {
            post(match event {
                copy::Event::Progress { fait, .. } => Message::CopyStep { fait },
                copy::Event::Done { size, .. } => Message::CopyDone { size },
                copy::Event::Failed { nom, erreur } => Message::CopyFailed { nom, erreur },
            })
        });
    }

    fn refresh(&mut self, changed: Vec<PathBuf>) {
        self.refresh_avec(changed, false);
    }

    /// Réindexation qui relance aussi la découverte des disques montés
    /// (branchement, retrait, option activée) — jamais à chaque rafale inotify.
    fn refresh_disques(&mut self, changed: Vec<PathBuf>) {
        self.refresh_avec(changed, true);
    }

    fn refresh_avec(&mut self, changed: Vec<PathBuf>, decouvrir: bool) {
        if self.refreshing {
            self.pending.extend(changed);
            self.pending_disques |= decouvrir;
            return;
        }
        self.refreshing = true;
        let decouvrir = decouvrir || std::mem::take(&mut self.pending_disques);
        spawn_refresh(
            self.config.clone(),
            self.index.clone(),
            changed,
            decouvrir,
        );
    }

    /// Remplace la liste par le résultat d'une réindexation.
    /// La réindexation donne-t-elle la même liste que celle à l'écran ?
    /// Comparaison sur les chemins : c'est le seul identifiant stable d'un
    /// paquet (un renommage change le slug, la taille et la date).
    fn meme_liste(&self, refresh: &Refresh) -> bool {
        if refresh.rows.len() != self.all_games.len() {
            return false;
        }
        let mut ecran: Vec<&Path> = self
            .all_games
            .iter()
            .map(|game| game.path.as_path())
            .collect();
        let mut disque: Vec<&Path> = refresh.rows.iter().map(|row| row.path.as_path()).collect();
        ecran.sort_unstable();
        disque.sort_unstable();
        ecran == disque
    }

    fn apply(&mut self, refresh: &Refresh) {
        let previous = self.games.get(self.selected).map(|game| game.path.clone());
        self.all_games = library::from_rows(
            &self.config,
            &refresh.rows,
            &mut self.artwork,
            &refresh.changed,
            &self.identities,
        );
        self.games = self.all_games.clone();
        self.apply_filter();
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

    /// (Re)monte le navigateur sur `chemin`, ligne 0 sélectionnée.
    fn ouvrir_navigateur(&mut self, chemin: PathBuf, choix: Choix) {
        let entrees = folders::lister(&chemin);
        if let Some(settings) = self.settings.as_mut() {
            settings.browser = Some(Browser {
                chemin,
                entrees,
                selected: 0,
                choix,
            });
            settings.confirm = None;
        }
    }

    /// Entre dans un dossier (le navigateur reste ouvert). `depuis` : dossier
    /// quitté — au retour on se pose dessus, comme un gestionnaire de fichiers.
    fn naviguer_vers(&mut self, chemin: PathBuf, depuis: Option<&Path>) {
        let entrees = folders::lister(&chemin);
        let selection = depuis
            .and_then(|ancien| folders::index_retour(&entrees, ancien))
            .map_or(0, |index| index + 1); // ligne 0 = « choisir ce dossier »
        if let Some(browser) = self.settings.as_mut().and_then(|s| s.browser.as_mut()) {
            browser.chemin = chemin;
            browser.entrees = entrees;
            browser.selected = selection;
        }
    }

    /// Ce que ferait la Croix dans le navigateur : `(choisir le dossier
    /// courant, chemin visé)` ; `None` = ligne vide, rien à faire.
    fn browser_cible(&self) -> Option<(bool, PathBuf)> {
        let browser = self.settings.as_ref()?.browser.as_ref()?;
        if browser.selected == 0 {
            return Some((true, browser.chemin.clone()));
        }
        browser
            .entrees
            .get(browser.selected - 1)
            .map(|entree| (false, entree.chemin.clone()))
    }

    /// Écrit `game_dirs` dans `config.toml` (seule cette clé change), met la
    /// surveillance à jour et relance un scan complet : un dossier retiré doit
    /// faire disparaître ses jeux de la liste.
    /// Applique **à chaud** la musique de fond puis l'écrit dans `config.toml`
    /// (seules `music` et `music_volume` changent). Couper envoie `Stop` — le
    /// fil garde sa position, donc relancer reprend la piste où elle était.
    fn persist_musique(&mut self) -> Task<Message> {
        let Some(settings) = self.settings.as_ref() else {
            return Task::none();
        };
        let (musique, volume) = (settings.musique, settings.volume);
        self.config.music = musique;
        self.config.music_volume = volume;
        if !musique || volume == 0 {
            if let Some(music) = &self.music {
                music.fade_out();
            }
        } else if let Some(music) = &self.music {
            // volume à chaud : `mpv` reste, seule la rampe suit
            music.volume(volume);
            music.resume();
        } else {
            self.music = music::Music::start(&self.config.cache_dir(), volume);
        }
        // aucun message d'état : le réglage se voit sur la ligne (interrupteur,
        // barre) — « musique de fond : 60 % » dans le pied de page était du
        // bruit (demandé le 22/11)
        if let Err(err) = crate::config::save_music(&self.config_path, musique, volume) {
            self.status = Some((format!("écriture impossible ({err})"), true));
        }
        Task::none()
    }

    fn persist_dirs(&mut self, message: String) -> Task<Message> {
        let dossiers = self
            .settings
            .as_ref()
            .map(|settings| settings.dossiers.clone())
            .unwrap_or_default();
        match crate::config::save_game_dirs(&self.config_path, &dossiers) {
            Ok(()) => {
                self.config.game_dirs = dossiers;
                if let Some(veille) = self.watch.as_ref() {
                    veille.definir(self.config.scan_dirs());
                }
                self.status = Some((message, false));
                self.refresh(self.config.scan_dirs());
            }
            Err(err) => self.status = Some((format!("écriture impossible : {err}"), true)),
        }
        Task::none()
    }

    /// Envoie une action de l'utilisateur (menu image) : elle passe devant.
    fn send_cover_query(&self, query: covers::Query) {
        if let Some(queue) = self.covers.as_ref() {
            queue.urgent(query);
        }
    }

    /// La page est refermée (ou le menu d'image) : les covers de fond
    /// reprennent. Rien à faire tant qu'un des deux reste ouvert : la file de
    /// fond doit rester muette, sinon la roue de chargement tournerait pour des
    /// covers qu'on ne regarde pas (demandé le 20/11).
    /// Suspend les visuels de fond (page ou menu d'image ouvert) et libère les
    /// demandes abandonnées par la file vidée : sans ça, ces jeux restaient
    /// marqués « demandés » pour toute la session et ne recevaient plus jamais
    /// leurs visuels (constaté 18/11).
    fn pause_covers(&mut self) {
        if let Some(queue) = self.covers.as_ref() {
            queue.pause(true);
        }
        for cle in std::mem::take(&mut self.visible_covers) {
            self.art_requested.remove(&cle);
        }
    }

    fn resume_covers(&mut self) {
        if self.page.is_some() || self.picker.is_some() {
            return;
        }
        if let Some(queue) = self.covers.clone() {
            // `pause(true)` vide la file de fond : ses demandes redeviennent
            // demandables, sinon ces jeux restaient marqués « demandés » pour
            // toute la session et ne recevaient plus jamais leurs visuels.
            for cle in std::mem::take(&mut self.visible_covers) {
                self.art_requested.remove(&cle);
            }
            queue.pause(false);
        }
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
            ViewMode::Grid | ViewMode::Mosaic => self.slot_height() + self.gap(),
            ViewMode::List => self.list_row_height() + 8.0,
        }
    }

    /// Nombre de jeux d'une page (L1/R1, Page↑/↓).
    fn page_size(&self) -> usize {
        match self.config.view {
            ViewMode::Grid | ViewMode::Mosaic => self.rows_visible() * self.metrics().2,
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
        let first = (self.scroll_drawn() / self.row_span()).floor().max(0.0) as usize;
        let rows = self.rows_visible();
        match self.config.view {
            ViewMode::Grid | ViewMode::Mosaic => {
                let columns = self.metrics().2;
                visible_items(self.scroll_drawn(), self.row_span(), rows, columns, len)
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
    fn missing_assets(&mut self, index: usize, urgent: bool, tout: bool) -> Option<covers::Query> {
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
        // l'identification du jeu fait foi : c'est SON id qui part chez
        // SteamGridDB, sinon le premier résultat de la recherche par nom — et
        // c'est elle qui donne le nom partagé des fichiers (`key`)
        let (name, id) = self.who(&game.slug, &game.name);
        let key = self.key_of(&game.slug);
        // marque fraîche et aucune identification : inutile de redemander à
        // SteamGridDB à chaque démarrage. « Revérifier » (Options) la lève.
        let absent = id.is_none()
            && self
                .absents
                .get(&game.slug)
                .is_some_and(covers::absent_frais);
        // visuels repris d'un autre jeu : ne pas résoudre le nom chez SGDB,
        // la recherche automatique renommerait les fichiers copiés
        let alias = covers::est_alias(&self.config.cache_dir().join("marks"), &game.slug);
        for &kind in demande {
            if absent || alias {
                continue;
            }
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
            // une demande déjà en vol (ou faite pour un autre affichage) n'est
            // pas renvoyée : la fermeture d'un menu ou d'une page libère les
            // demandes abandonnées (`pause_covers`/`resume_covers`)
            if present
                || self.art_requested.contains(&(game.slug.clone(), kind))
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
            name,
            id,
            // clé connue seulement si le jeu est identifié : sinon c'est le
            // worker qui résout le jeu (et retient l'identification)
            key: id.map(|_| key),
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
        // les miniatures sont locales (nommées d'après le paquet) mais leur
        // source est le fichier partagé, nommé d'après le jeu
        let source = covers::cached_for(
            &self.config.cover_cache_dir(),
            self.identities.get(slug),
            slug,
        );
        let Some(source) = source else {
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
                let source =
                    covers::cached_for(&cover_dir, self.identities.get(&game.slug), &game.slug)?;
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

    /// Année de sortie du jeu, si on la connaît (la table est chargée une
    /// fois au démarrage ; `None` = inconnue ou « SGDB n'en donne pas »).
    fn year_of(&self, slug: &str) -> Option<u32> {
        self.years.get(&self.key_of(slug)).copied().flatten()
    }

    /// Retient l'année apprise sous la **clé du jeu** et l'écrit dans
    /// `assets/years.txt` (fusionné avec ce qui est déjà sur le disque :
    /// `--update` peut écrire en même temps).
    fn remember_year(&mut self, key: &str, year: Option<u32>) {
        if self.years.get(key) == Some(&year) {
            return;
        }
        self.years.insert(key.to_string(), year);
        covers::save_years(&covers::years_path(&self.config.assets_dir()), &self.years);
    }

    /// Exécute l'action choisie sur la page (jouer, étoile, menu des visuels).
    fn page_act(&mut self) -> Task<Message> {
        let Some(page) = self.page.as_ref() else {
            return Task::none();
        };
        let index = page.index;
        let Some(action) = self.page_actions().get(page.action).copied() else {
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
            PageAction::Options => {
                self.art_menu = Some(0);
                Task::none()
            }
            PageAction::Gallery => {
                let nom = self.games.get(index).map(|game| game.name.clone());
                Task::done(Message::GalleryOpen(nom))
            }
        }
    }

    /// Actions de la page du jeu courant : les trois de base, plus « Galerie »
    /// quand les dossiers de sortie contiennent au moins une capture ou une
    /// vidéo **de ce jeu** (comptées en fond par `spawn_comptes_galerie`).
    fn page_actions(&self) -> Vec<PageAction> {
        let mut actions = PAGE_ACTIONS.to_vec();
        let jeu = self.page.as_ref().and_then(|page| self.games.get(page.index));
        if let Some(game) = jeu
            && self.galerie_comptes.get(&game.name).copied().unwrap_or(0) > 0
        {
            actions.push(PageAction::Gallery);
        }
        actions
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
        if self.sort == SortBy::Favorites {
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
                year: self.year_filter,
                scope: self.scope,
                list_size: self.list_size,
            },
        );
    }

    /// Ordre de la bibliothèque : A→Z (défaut), paquets récents d'abord, ou
    /// liste des favoris (alphabétique, retour 18/11).
    fn sort_games(&mut self) {
        match self.sort {
            SortBy::Name | SortBy::Favorites => self
                .games
                .sort_by_cached_key(|game| game.name.to_lowercase()),
            SortBy::Modified => self.games.sort_by_key(|game| std::cmp::Reverse(game.mtime)),
            // derniers lancés d'abord, puis le reste A→Z : un jeu jamais lancé
            // n'a pas de rang, il ne doit pas disparaître de la liste
            SortBy::Recent => {
                let rang: std::collections::HashMap<String, usize> = self
                    .recent
                    .iter()
                    .enumerate()
                    .map(|(index, (slug, _))| (slug.clone(), index))
                    .collect();
                self.games.sort_by_cached_key(|game| {
                    (
                        rang.get(&game.slug).copied().unwrap_or(usize::MAX),
                        game.name.to_lowercase(),
                    )
                });
            }
        }
    }

    /// Note le jeu comme lancé (ordre « Derniers lancés ») et relit la liste.
    fn noter_lancement(&mut self, slug: &str) {
        let path = recent::path(&self.config.cache_dir());
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duree| duree.as_secs())
            .unwrap_or(0);
        if recent::record(&path, slug, epoch) {
            self.recent = recent::load(&path);
        }
    }

    /// Vide l'historique des derniers lancés (le fichier, pas les temps de jeu).
    fn effacer_derniers(&mut self) {
        let path = recent::path(&self.config.cache_dir());
        if recent::clear(&path) {
            self.recent.clear();
            self.apply_filter();
            self.selected = 0;
            self.scroll = 0.0;
            self.scroll_target = 0.0;
        }
    }

    /// Années présentes dans la bibliothèque, les plus récentes d'abord.
    /// Seuls les jeux dont l'année est **connue** comptent.
    fn known_years(&self) -> Vec<u32> {
        let mut years: Vec<u32> = self
            .all_games
            .iter()
            .filter_map(|game| self.year_of(&game.slug))
            .collect();
        years.sort_unstable();
        years.dedup();
        years.reverse();
        years
    }

    /// Nombre de jeux de la bibliothèque sortis cette année-là.
    fn year_count(&self, year: u32) -> usize {
        self.all_games
            .iter()
            .filter(|game| self.year_of(&game.slug) == Some(year))
            .count()
    }

    /// Prépare une bascule de liste : le changement sera appliqué au sommet du
    /// fondu (`tick`). Rien ne bouge tout de suite.
    fn begin_swap(&mut self, swap: Swap) {
        self.pending_swap = Some(swap);
        self.swap_fade_target = SWAP_PEAK;
    }

    /// Applique la bascule demandée, une fois le fond atteint.
    fn apply_swap(&mut self, swap: Swap) -> Task<Message> {
        match swap {
            // une vue ne filtre pas : la vignette courante est conservée, on
            // la recadre simplement dans la nouvelle disposition
            Swap::View(view) => {
                self.config.view = view;
                self.remember();
                self.apply_filter();
                self.scroll = 0.0;
                self.update_scroll_target();
                self.request_visible_covers();
                return Task::none();
            }
            // tri et filtre d'année remettent en DÉBUT de liste (demandé le
            // 20/11) : seul moyen de voir le résultat depuis le haut
            Swap::Sort(sort) => {
                self.sort = sort;
                self.remember();
                self.apply_filter();
            }
            Swap::Year(year) => {
                self.year_filter = year;
                self.remember();
                self.apply_filter();
            }
            // réindexation : la sélection et le défilement ne bougent pas, on
            // ne fait que remplacer le contenu
            Swap::Reload => {
                // aucun message d'état : la liste et la pastille de total
                // disent déjà ce qui a changé (un « 4 jeux » en double au
                // démarrage était du bruit)
                if let Some(refresh) = self.pending_reload.take() {
                    self.apply(&refresh);
                }
                return Task::none();
            }
        }
        self.selected = 0;
        self.scroll = 0.0;
        self.scroll_target = 0.0;
        self.request_visible_covers();
        operation::scroll_to(self.scroll_id.clone(), AbsoluteOffset { x: 0.0, y: 0.0 })
    }

    fn apply_filter(&mut self) {
        let current = self.games.get(self.selected).map(|game| game.slug.clone());
        let favorites = self.sort == SortBy::Favorites;
        let year = self.year_filter;
        // recherche : sous-chaîne, minuscules et sans accents (« zelda »
        // trouve « The Legend of Zelda », « epee » trouve « Épée »)
        let requete = normalise(&self.search);
        // portée : d'où vient le paquet. Les dossiers choisis priment sur les
        // disques (un dossier de `config.toml` posé sur un disque externe est
        // « choisi », pas « trouvé »).
        let scope = self.scope;
        let choisis = self.config.game_dirs();
        let disques = self.config.extra_dirs.clone();
        self.games = self
            .all_games
            .iter()
            .filter(|game| requete.is_empty() || normalise(&game.name).contains(&requete))
            .filter(|game| match scope {
                Scope::Tous => true,
                Scope::Dossiers => choisis.iter().any(|dir| game.path.starts_with(dir)),
                Scope::Disques => {
                    disques.iter().any(|dir| game.path.starts_with(dir))
                        && !choisis.iter().any(|dir| game.path.starts_with(dir))
                }
            })
            .filter(|game| !favorites || self.favorites.contains(&game.slug))
            // filtre d'année : seuls les jeux dont la date est connue peuvent
            // y répondre (elle s'apprend page par page)
            .filter(|game| year.is_none_or(|year| self.year_of(&game.slug) == Some(year)))
            .cloned()
            .collect();
        self.sort_games();
        self.selected = current
            .and_then(|slug| self.games.iter().position(|game| game.slug == slug))
            .unwrap_or(0)
            .min(self.games.len().saturating_sub(1));
        self.update_scroll_target();
    }

    fn on_key(&mut self, event: keyboard::Event) -> Task<Message> {
        // Wayland route déjà le clavier vers la fenêtre active ; la garde
        // couvre les événements en retard et le cas « jeu lancé ».
        if !self.active || self.running.is_some() {
            return Task::none();
        }
        // Une touche MAINTENUE ne doit pas rejouer son action : le noyau répète
        // `KeyPressed` (auto-repeat X11/Wayland), et un `T` appuyé un peu trop
        // longtemps basculait le tri deux fois (constaté 20/11). La navigation
        // n'a jamais été répétée au clavier, donc ignorer les répétitions ne
        // change rien d'autre.
        if matches!(event, keyboard::Event::KeyPressed { repeat: true, .. }) {
            return Task::none();
        }
        // une touche pressée : les aides repassent en langage clavier, menus
        // compris (le pied de page du menu d'image suit aussi)
        if matches!(event, keyboard::Event::KeyPressed { .. }) {
            self.input = Input::Keyboard;
            self.barre = true;
        }

        // aperçu d'une capture : plein écran, seules les images se suivent
        if self.apercu.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::ApercuClose),
                keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::ApercuNav(-1)),
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::ApercuNav(1)),
                keyboard::Key::Character(ch) if ch.as_ref() == "c" => {
                    Task::done(Message::ApercuCopy)
                }
                _ => Task::none(),
            };
        }
        // galerie : navigation dans la grille, ouverture, effacement armé
        if self.galerie.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            let colonnes = self.gallery_columns() as i32;
            let arme = self.galerie.as_ref().is_some_and(|galerie| galerie.arme.is_some());
            if arme {
                return match key {
                    keyboard::Key::Named(Named::Escape) => Task::done(Message::GalleryUnarm),
                    keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                        Task::done(Message::GalleryAct)
                    }
                    _ => Task::none(),
                };
            }
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::GalleryClose),
                keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::GalleryNav(-1)),
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::GalleryNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::GalleryNav(-colonnes)),
                keyboard::Key::Named(Named::ArrowDown) => {
                    Task::done(Message::GalleryNav(colonnes))
                }
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::GalleryAct)
                }
                keyboard::Key::Named(Named::Delete) => Task::done(Message::GalleryArm),
                keyboard::Key::Named(Named::Tab) => Task::done(Message::GalleryFilter),
                _ => Task::none(),
            };
        }
        // prompt « nom de recherche » (identification) : le champ de saisie
        // prend les touches, on n'intercepte que l'annulation (Entrée passe par
        // `on_submit`)
        if self.identify.is_some() {
            return match event {
                keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                } => Task::done(Message::IdentifyCancel),
                _ => Task::none(),
            };
        }
        // prompt « date de sortie »
        if self.date.is_some() {
            return match event {
                keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(Named::Escape),
                    ..
                } => Task::done(Message::DateCancel),
                _ => Task::none(),
            };
        }
        // liste des jeux sources pour un alias visuel
        if self.alias_menu.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::AliasClose),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::AliasPick)
                }
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::AliasNav(-1)),
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::AliasNav(1)),
                _ => Task::none(),
            };
        }
        // liste des jeux proposés (identification)
        if self.identify_menu.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::IdentifyClose),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::IdentifyPick)
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::IdentifyNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::IdentifyNav(-1)),
                // « r » : corriger le nom recherché
                keyboard::Key::Character(ch) if ch.as_ref() == "r" => {
                    Task::done(Message::IdentifyOpen)
                }
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
                keyboard::Key::Named(Named::ArrowRight) => {
                    Task::done(Message::PickerNav(Nav::Next))
                }
                keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::PickerNav(Nav::Prev)),
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::PickerNav(Nav::Down)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::PickerNav(Nav::Up)),
                keyboard::Key::Named(Named::PageDown) => {
                    Task::done(Message::PickerNav(Nav::PageDown))
                }
                keyboard::Key::Named(Named::PageUp) => Task::done(Message::PickerNav(Nav::PageUp)),
                // « f » : une image perso (fichier local) au lieu de la liste
                keyboard::Key::Character(ch) if ch.as_ref() == "f" => {
                    Task::done(Message::PickerFile)
                }
                _ => Task::none(),
            };
        }
        // « oublier les jeux disparus ? » : « Annuler » sélectionné, Entrée
        // annule ; Échap aussi
        if self.oublier.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::OublierChoose(0)),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::OublierPick)
                }
                keyboard::Key::Named(Named::ArrowLeft) | keyboard::Key::Named(Named::Tab) => {
                    Task::done(Message::OublierNav(-1))
                }
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::OublierNav(1)),
                _ => Task::none(),
            };
        }

        // « remplacer les visuels ? » : « Non » est sélectionné, donc Entrée
        // garde tout ; Échap fait la même chose
        if self.replace.is_some() {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return Task::none();
            };
            return match key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::ReplaceChoose(0)),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::ReplacePick)
                }
                keyboard::Key::Named(Named::ArrowLeft) | keyboard::Key::Named(Named::Tab) => {
                    Task::done(Message::ReplaceNav(-1))
                }
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::ReplaceNav(1)),
                _ => Task::none(),
            };
        }
        let keyboard::Event::KeyPressed { key, .. } = event else {
            return Task::none();
        };

        // recherche ouverte : le champ capte le clavier (aucun raccourci ne
        // doit se déclencher pendant qu'on tape un titre)
        if self.search_open {
            return match &key {
                keyboard::Key::Named(Named::Escape) => {
                    self.search.clear();
                    self.search_open = false;
                    self.apply_filter();
                    Task::none()
                }
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::OpenPage(self.selected))
                }
                keyboard::Key::Named(Named::Backspace) => {
                    self.search.pop();
                    self.apply_filter();
                    Task::none()
                }
                keyboard::Key::Named(Named::ArrowRight) => self.navigate(Nav::Next),
                keyboard::Key::Named(Named::ArrowLeft) => self.navigate(Nav::Prev),
                keyboard::Key::Named(Named::ArrowDown) => self.navigate(Nav::Down),
                keyboard::Key::Named(Named::ArrowUp) => self.navigate(Nav::Up),
                keyboard::Key::Character(ch) => {
                    self.search.push_str(ch);
                    self.apply_filter();
                    Task::none()
                }
                _ => Task::none(),
            };
        }

        // page RÉGLAGES : elle capte tout tant qu'elle est ouverte
        if let Some(settings) = self.settings.as_ref() {
            if settings.browser.is_some() {
                return match &key {
                    // Rond/Échap remonte d'un niveau (le navigateur se referme
                    // à la racine, sinon on resterait coincé)
                    keyboard::Key::Named(Named::Escape) => Task::done(Message::BrowserUp),
                    keyboard::Key::Named(Named::Backspace)
                    | keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::BrowserUp),
                    keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                        Task::done(Message::BrowserAct)
                    }
                    keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::BrowserNav(1)),
                    keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::BrowserNav(-1)),
                    _ => Task::none(),
                };
            }
            return match &key {
                // une confirmation armée se désarme avant de fermer
                keyboard::Key::Named(Named::Escape) => Task::done(if settings.confirm.is_some() {
                    Message::SettingsNav(0)
                } else {
                    Message::SettingsClose
                }),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::SettingsAct)
                }
                keyboard::Key::Named(Named::Delete) => Task::done(Message::SettingsArm),
                keyboard::Key::Named(Named::ArrowRight) => Task::done(Message::SettingsChoix(1)),
                keyboard::Key::Named(Named::ArrowLeft) => Task::done(Message::SettingsChoix(-1)),
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::SettingsNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::SettingsNav(-1)),
                _ => Task::none(),
            };
        }

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

        // menu « tri » (T) : haut/bas puis Entrée, et T le referme
        if let Some(choice) = self.sort_menu {
            return match &key {
                keyboard::Key::Named(Named::Escape) => {
                    self.sort_menu = None;
                    Task::none()
                }
                keyboard::Key::Character(touche) if touche.as_str() == "t" => {
                    self.sort_menu = None;
                    Task::none()
                }
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::SortMenuChoose(choice))
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::SortMenuNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::SortMenuNav(-1)),
                _ => Task::none(),
            };
        }

        // menu « année » (filtre) : haut/bas puis Entrée
        if let Some(choice) = self.year_menu {
            return match &key {
                keyboard::Key::Named(Named::Escape) => {
                    self.year_menu = None;
                    Task::none()
                }
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::YearMenuChoose(choice))
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::YearMenuNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::YearMenuNav(-1)),
                _ => Task::none(),
            };
        }

        // menu « Options » de la page : haut/bas puis Entrée
        if self.art_menu.is_some() {
            return match &key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::ArtMenuClose),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::ArtMenuPick)
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::ArtMenuNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::ArtMenuNav(-1)),
                _ => Task::none(),
            };
        }

        // « copier vers… » : haut/bas puis Entrée
        if self.copy_menu.is_some() {
            return match &key {
                keyboard::Key::Named(Named::Escape) => {
                    self.copy_menu = None;
                    Task::none()
                }
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::CopyPick)
                }
                keyboard::Key::Named(Named::ArrowDown) => Task::done(Message::CopyNav(1)),
                keyboard::Key::Named(Named::ArrowUp) => Task::done(Message::CopyNav(-1)),
                _ => Task::none(),
            };
        }

        // « mettre à jour la copie ? » : « Annuler » sélectionné, Entrée annule
        if self.copy_replace.is_some() {
            return match &key {
                keyboard::Key::Named(Named::Escape) => Task::done(Message::CopyReplaceChoose(0)),
                keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
                    Task::done(Message::CopyReplacePick)
                }
                keyboard::Key::Named(Named::ArrowLeft) | keyboard::Key::Named(Named::Tab) => {
                    Task::done(Message::CopyReplaceNav(-1))
                }
                keyboard::Key::Named(Named::ArrowRight) => {
                    Task::done(Message::CopyReplaceNav(1))
                }
                _ => Task::none(),
            };
        }

        // page d'un jeu : gauche/droite choisissent l'action, Entrée valide.
        // Aucun raccourci pour les visuels, l'identification ou la date : tout
        // passe par le bouton « Options ▸ » de la page (demandé le 20/11).
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
                    // « c » : copier le paquet (téléchargements ou dossier de jeux)
                    "c" => Task::done(Message::CopyOpen),
                    // « g » : les captures et vidéos de CE jeu (bouton Galerie)
                    "g" if self.page_actions().contains(&PageAction::Gallery) => {
                        let nom = self
                            .page
                            .as_ref()
                            .and_then(|page| self.games.get(page.index))
                            .map(|game| game.name.clone());
                        Task::done(Message::GalleryOpen(nom))
                    }
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
                Named::Escape => {
                    return if self.mode.is_console() {
                        Task::none()
                    } else {
                        Task::done(Message::Quit)
                    };
                }
                Named::F11 => return Task::done(Message::ToggleFullscreen),
                Named::F2 => return Task::done(Message::SettingsOpen),
                Named::Tab => return Task::done(Message::ToggleView),
                _ => {}
            },
            keyboard::Key::Character(ch) => match ch.as_ref() {
                "d" | "l" => return self.navigate(Nav::Next),
                "a" | "h" => return self.navigate(Nav::Prev),
                "s" | "j" => return self.navigate(Nav::Down),
                "w" | "k" => return self.navigate(Nav::Up),
                "v" => return Task::done(Message::ToggleView),
                // recherche : « / » ouvre le champ (le clavier physique est
                // branché en session console — K400 —, la manette ne tape pas)
                "/" => {
                    self.search.clear();
                    self.search_open = true;
                    self.apply_filter();
                    return Task::none();
                }
                "t" => return Task::done(Message::SortMenu),
                "+" | "=" | "＋" => return Task::done(Message::Zoom(1)),
                "-" | "_" | "−" | "–" | "‐" => return Task::done(Message::Zoom(-1)),
                "f" => return Task::done(Message::ToggleFullscreen),
                "m" => return Task::done(Message::UpdateApply),
                "q" if self.mode.is_console() => return Task::none(),
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
                // la manette reprend la main : la barre de défilement
                // s'efface (elle ne se saisit pas à la manette)
                self.barre = false;
                // Lecture vidéo : mpv a le focus, donc la garde de focus
                // ci-dessous ne doit pas s'appliquer — la manette est lue en
                // direct sur evdev et c'est elle qui pilote le film.
                if self.lecteur.is_some() {
                    let Some(lecteur) = self.lecteur.as_mut() else {
                        return Task::none();
                    };
                    match button {
                        PadButton::Confirm => lecteur.pause(),
                        PadButton::Back => lecteur.quitter(),
                        PadButton::Left => lecteur.seek(-play::PAS),
                        PadButton::Right => lecteur.seek(play::PAS),
                        _ => {}
                    }
                    return Task::none();
                }
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
                if self.apercu.is_some() {
                    return match button {
                        PadButton::Back => Task::done(Message::ApercuClose),
                        PadButton::Left => Task::done(Message::ApercuNav(-1)),
                        PadButton::Right => Task::done(Message::ApercuNav(1)),
                        PadButton::Menu => Task::done(Message::ApercuCopy),
                        _ => Task::none(),
                    };
                }
                if self.galerie.is_some() {
                    let colonnes = self.gallery_columns() as i32;
                    let arme = self
                        .galerie
                        .as_ref()
                        .is_some_and(|galerie| galerie.arme.is_some());
                    // panneau de confirmation ouvert : Croix/Rond, rien d'autre
                    if arme {
                        return match button {
                            PadButton::Confirm => Task::done(Message::GalleryAct),
                            PadButton::Back => Task::done(Message::GalleryUnarm),
                            _ => Task::none(),
                        };
                    }
                    return match button {
                        PadButton::Confirm => Task::done(Message::GalleryAct),
                        PadButton::Back => Task::done(Message::GalleryClose),
                        PadButton::Menu => Task::done(Message::GalleryArm),
                        // triangle / Y : filtre tout → captures → vidéos
                        PadButton::North => Task::done(Message::GalleryFilter),
                        PadButton::Left => Task::done(Message::GalleryNav(-1)),
                        PadButton::Right => Task::done(Message::GalleryNav(1)),
                        PadButton::Up => Task::done(Message::GalleryNav(-colonnes)),
                        PadButton::Down => Task::done(Message::GalleryNav(colonnes)),
                        _ => Task::none(),
                    };
                }
                if self.identify.is_some() || self.date.is_some() {
                    // le prompt demande un clavier : la manette ne peut
                    // qu'annuler (clavier virtuel plus tard)
                    return match button {
                        PadButton::Back => Task::done(if self.date.is_some() {
                            Message::DateCancel
                        } else {
                            Message::IdentifyCancel
                        }),
                        _ => Task::none(),
                    };
                }
                if self.alias_menu.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::AliasPick),
                        PadButton::Back => Task::done(Message::AliasClose),
                        PadButton::Up => Task::done(Message::AliasNav(-1)),
                        PadButton::Down => Task::done(Message::AliasNav(1)),
                        _ => Task::none(),
                    };
                }
                if self.identify_menu.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::IdentifyPick),
                        PadButton::Back => Task::done(Message::IdentifyClose),
                        PadButton::Up => Task::done(Message::IdentifyNav(-1)),
                        PadButton::Down => Task::done(Message::IdentifyNav(1)),
                        _ => Task::none(),
                    };
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
                // page RÉGLAGES : Croix valide, Rond annule ou remonte
                if let Some(settings) = self.settings.as_ref() {
                    let navigateur = settings.browser.is_some();
                    let arme = settings.confirm.is_some();
                    return match button {
                        PadButton::Confirm => Task::done(if navigateur {
                            Message::BrowserAct
                        } else {
                            Message::SettingsAct
                        }),
                        PadButton::Back => Task::done(if navigateur {
                            Message::BrowserUp
                        } else if arme {
                            Message::SettingsNav(0)
                        } else {
                            Message::SettingsClose
                        }),
                        PadButton::Up => Task::done(if navigateur {
                            Message::BrowserNav(-1)
                        } else {
                            Message::SettingsNav(-1)
                        }),
                        PadButton::Down => Task::done(if navigateur {
                            Message::BrowserNav(1)
                        } else {
                            Message::SettingsNav(1)
                        }),
                        PadButton::Left => Task::done(if navigateur {
                            Message::BrowserNav(-1)
                        } else {
                            Message::SettingsChoix(-1)
                        }),
                        PadButton::Right => Task::done(if navigateur {
                            Message::BrowserNav(1)
                        } else {
                            Message::SettingsChoix(1)
                        }),
                        // carré : armé le retrait (jamais direct)
                        PadButton::Menu => Task::done(Message::SettingsArm),
                        _ => Task::none(),
                    };
                }
                // menu « type de liste »
                if let Some(choice) = self.view_menu {
                    return match button {
                        PadButton::Confirm => Task::done(Message::ViewMenuChoose(choice)),
                        // le bouton qui l'a ouvert le referme : appuyer deux
                        // fois sur Start ne doit pas laisser un menu ouvert
                        PadButton::View | PadButton::North | PadButton::Back => {
                            self.view_menu = None;
                            Task::none()
                        }
                        PadButton::Up => Task::done(Message::ViewMenuNav(-1)),
                        PadButton::Down => Task::done(Message::ViewMenuNav(1)),
                        _ => Task::none(),
                    };
                }
                // menu « tri » (carré / X)
                if let Some(choice) = self.sort_menu {
                    return match button {
                        PadButton::Confirm => Task::done(Message::SortMenuChoose(choice)),
                        // même geste pour fermer que pour ouvrir
                        PadButton::Menu | PadButton::North | PadButton::Back => {
                            self.sort_menu = None;
                            Task::none()
                        }
                        PadButton::Up => Task::done(Message::SortMenuNav(-1)),
                        PadButton::Down => Task::done(Message::SortMenuNav(1)),
                        _ => Task::none(),
                    };
                }
                // menu « année » (filtre)
                if let Some(choice) = self.year_menu {
                    return match button {
                        PadButton::Confirm => Task::done(Message::YearMenuChoose(choice)),
                        PadButton::Menu | PadButton::North | PadButton::Back => {
                            self.year_menu = None;
                            Task::none()
                        }
                        PadButton::Up => Task::done(Message::YearMenuNav(-1)),
                        PadButton::Down => Task::done(Message::YearMenuNav(1)),
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
                // « remplacer les visuels ? » : A valide le choix courant
                // (« Non » à l'ouverture), B garde les visuels
                if self.replace.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::ReplacePick),
                        PadButton::Back => Task::done(Message::ReplaceChoose(0)),
                        PadButton::Left => Task::done(Message::ReplaceNav(-1)),
                        PadButton::Right => Task::done(Message::ReplaceNav(1)),
                        _ => Task::none(),
                    };
                }
                // page d'un jeu : croix gauche/droite entre les actions ; les
                // visuels et l'identification passent par « Options ▸ »
                if self.page.is_some() {
                    return match button {
                        PadButton::Confirm => Task::done(Message::PageAct),
                        PadButton::Back => Task::done(Message::ClosePage),
                        PadButton::Left => Task::done(Message::PageNav(-1)),
                        PadButton::Right => Task::done(Message::PageNav(1)),
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
                    // start : le MENU des types de liste (le cycle rapide
                    // reste au clavier, V/Tab)
                    PadButton::View => Task::done(Message::ViewMenu),
                    // triangle / Y et carré / X : le MENU du tri (A→Z,
                    // récents, favoris). Les deux boutons sont distincts
                    // depuis le 23/11, mais gardent ce raccourci.
                    PadButton::North | PadButton::Menu => Task::done(Message::SortMenu),
                    PadButton::Confirm => Task::done(Message::OpenPage(self.selected)),
                    // « retour » ne quitte pas du premier coup : deux appuis
                    // rapprochés (1,5 s) sont nécessaires, pour ne pas
                    // s'éjecter sur une pression malheureuse en jouant.
                    // En session console il ne quitte RIEN : le launcher est
                    // la session, la sortie passe par le menu système de gpad
                    // (Home maintenu), qui propose aussi Éteindre/Redémarrer.
                    PadButton::Back => {
                        if self.mode.is_console() {
                            Task::none()
                        } else if self.back_press.is_some_and(|at| at.elapsed() < QUIT_WINDOW) {
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
        // Le nombre de couches est CONSTANT (liste, poussière, page, voile,
        // menus) : iced apparie les états des enfants d'un `stack` par
        // position, donc ajouter une couche au milieu faisait perdre à la
        // grille son état de défilement — le retour depuis une page ramenait
        // tout en haut (mesuré 18/11 : offset 0 au moment même de l'ouverture
        // de la page).
        let mut view: Element<'_, Message> = self.launcher_view();

        // Poussière d'étoiles : couche transparente AU-DESSUS de la liste (elle
        // flotte donc devant les vignettes). Quand la page d'un jeu est
        // ouverte, elle est opaque et recouvre cette couche : c'est la copie
        // `page_dust` ci-dessous qui prend le relais, au-dessus de la page.
        // Une seule des deux est dessinée à la fois.
        let dust: Element<'_, Message> = if self.config.sparkles && self.page.is_none() {
            self.sparkles
                .view(self.scroll_drawn(), self.fade, self.breathing())
        } else {
            container(Row::new()).width(Fill).height(Fill).into()
        };
        view = stack![view, dust].into();

        let page_layer: Element<'_, Message> = if self.page.is_some() {
            // la page doit intercepter clic et molette, sinon ils atteignent
            // les vignettes de la grille restée dessous
            let page = mouse_area(self.page_view())
                .on_press(Message::Nothing)
                .on_scroll(|_| Message::Nothing);
            // Lancement d'un jeu : la page plonge vers l'écran (agrandissement)
            // pendant que le voile monte. `Pose` applique une vraie
            // transformation au rendu (l'échelle reste à 1 au repos, donc rien
            // ne change hors du fondu) et l'effet ne touche pas à la mise en
            // page.
            if self.fade > 0.001 {
                // échelle CONTINUE : quantifiée par pas de 0,5 %, la plongée
                // sautait visiblement (jugé saccadé le 20/11) — le texte est
                // ré-rasterisé à chaque nouvelle taille, mais c'est le prix
                // d'un mouvement continu.
                Pose::new(page).scale(1.0 + PAGE_DIVE * self.fade).into()
            } else {
                page.into()
            }
        } else {
            container(Row::new()).width(Fill).height(Fill).into()
        };
        view = stack![view, page_layer].into();

        // Poussière AU-DESSUS de la page d'un jeu (demandé le 20/11) : la page
        // est opaque et recouvre la couche `dust`, donc elle a sa propre copie
        // des mêmes particules. C'est aussi cette couche qui porte
        // l'« hyperespace » du lancement (particules filant vers les bords) :
        // on lance un jeu depuis sa page, et sans elle l'effet ne se voyait que
        // depuis la grille — donc jamais (constaté 20/11).
        let page_dust: Element<'_, Message> = if self.config.sparkles && self.page.is_some() {
            self.sparkles.view(self.scroll_drawn(), self.fade, None)
        } else {
            container(Row::new()).width(Fill).height(Fill).into()
        };
        view = stack![view, page_dust].into();

        // Coin bas droit (réseau, manettes, roues) : AVANT le voile, sinon les
        // icônes restaient en pleine couleur pendant que tout l'écran virait au
        // noir au lancement d'un jeu (constaté le 24/11).
        view = stack![view, self.coin_bas_view()].into();

        // Le voile noir du fondu existe MÊME à opacité nulle, et il passe APRÈS
        // la page : lancer un jeu depuis une page doit virer au noir comme
        // depuis la grille, et l'arbre doit garder la même forme d'un rendu à
        // l'autre (un changement de forme recrée la zone défilante, donc la
        // position). Il porte trois fondus : celui du lancement (`fade`),
        // celui de la page (`page_fade`, ouverture et fermeture) et celui
        // d'une bascule de liste (`swap_fade`) — jamais ensemble, donc le plus
        // opaque des trois suffit.
        let veil_opacity = self
            .fade
            .max(self.page_fade)
            .max(self.swap_fade)
            .max(self.video_veil);
        let veil = container(Row::new())
            .width(Fill)
            .height(Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(Color {
                    a: veil_opacity,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            });
        view = stack![view, veil].into();

        // menus : une seule couche, construite à part (aucun état à préserver).
        // La couche de BASE doit remplir la fenêtre : `Stack` prend sa taille
        // sur son premier enfant, une couche vide réduisait les menus à zéro
        // (constaté 18/11 : menus invisibles).
        let mut menus: Element<'_, Message> = container(Row::new()).width(Fill).height(Fill).into();
        if self.settings.is_some() {
            menus = stack![menus, self.settings_view()].into();
        }
        if self.view_menu.is_some() {
            menus = stack![menus, self.view_menu_view()].into();
        }
        if self.sort_menu.is_some() {
            menus = stack![menus, self.sort_menu_view()].into();
        }
        if self.year_menu.is_some() {
            menus = stack![menus, self.year_menu_view()].into();
        }
        if self.art_menu.is_some() {
            menus = stack![menus, self.art_menu_view()].into();
        }
        if self.picker.is_some() {
            menus = stack![menus, self.picker_view()].into();
        }
        if self.identify_menu.is_some() {
            menus = stack![menus, self.identify_menu_view()].into();
        }
        if self.alias_menu.is_some() {
            menus = stack![menus, self.alias_menu_view()].into();
        }
        if self.identify.is_some() {
            menus = stack![menus, self.identify_view()].into();
        }
        if self.replace.is_some() {
            menus = stack![menus, self.replace_view()].into();
        }
        if self.date.is_some() {
            menus = stack![menus, self.date_view()].into();
        }
        if self.oublier.is_some() {
            menus = stack![menus, self.oublier_view()].into();
        }
        if self.copy_menu.is_some() {
            menus = stack![menus, self.copy_menu_view()].into();
        }
        if self.copy_replace.is_some() {
            menus = stack![menus, self.copy_replace_view()].into();
        }
        if self.galerie.is_some() {
            menus = stack![menus, self.gallery_view()].into();
        }
        if self.galerie.as_ref().is_some_and(|galerie| galerie.arme.is_some()) {
            menus = stack![menus, self.gallery_confirm_view()].into();
        }
        if self.apercu.is_some() {
            menus = stack![menus, self.apercu_view()].into();
        }
        // Fondu d'apparition : un voile noir AU-DESSUS des menus, dissipé par
        // `tick`. Il existe MÊME à opacité nulle (comme le voile de lancement) :
        // l'arbre garde la même forme d'un rendu à l'autre.
        let entree = container(Row::new())
            .width(Fill)
            .height(Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(Color {
                    a: self.overlay_fade,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            });
        view = stack![view, menus, entree].into();

        // Roue de chargement : emplacement TOUJOURS présent (jamais un enfant
        // conditionnel de la pile racine : un changement de forme recrée le
        // sous-arbre, donc la zone défilante, et la liste remonte — constaté
        // 20/11). Le canvas ne dessine rien hors téléchargement.
        view = stack![view, self.copy_overlay()].into();

        // clic droit = retour, partout : les couches du dessus n'ont pas de
        // gestion du bouton droit, l'évènement remonte donc jusqu'ici.
        // L'enveloppe `cursor::Idle` est AU-DESSUS de tout : elle cache le
        // pointeur immobile quelle que soit la couche affichée, et n'a aucune
        // influence sur la mise en page.
        let view = mouse_area(view).on_right_press(Message::Back);
        // Un vrai geste de souris (mouvement, clic, molette) rallume la barre
        // de défilement. `muet` quand elle est déjà là : sinon chaque
        // mouvement du pointeur reconstruirait la vue pour rien.
        cursor::Idle::new(view)
            .on_geste(Message::Souris)
            .muet(self.barre)
            .into()
    }

    /// Barre de progression des copies, tout en haut de la fenêtre (les
    /// indicateurs du coin restent dessinés par-dessus). Le pourcentage est
    /// celui de la **file entière** : trois jeux demandés d'affilée donnent une
    /// seule barre qui va de 0 à 100 %, pas trois barres successives.
    fn copy_overlay(&self) -> Element<'_, Message> {
        let Some(copies) = self.copies.as_ref().filter(|copies| copies.total > 0) else {
            // forme d'arbre constante : la bande existe toujours, transparente
            return container(Row::new())
                .width(Fill)
                .height(Fill)
                .align_y(Alignment::Start)
                .into();
        };
        let fait = copies.fait + copies.courant;
        let percent = (fait as f32 / copies.total as f32 * 100.0).clamp(0.0, 100.0);
        let libelle = format!(
            "copie de {} · {:.0} % · {} sur {}",
            short(&copies.nom, 28),
            percent,
            copy::poids(fait),
            copy::poids(copies.total)
        );
        let rempli = (percent * 100.0).round() as u16;
        let segment = |portion: u16, teinte: Color| {
            container(Row::new())
                .width(Length::FillPortion(portion.max(1)))
                .height(Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(teinte)),
                    border: Border {
                        radius: 2.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                })
        };
        let track = row![
            segment(rempli, ACCENT),
            segment(100u16.saturating_sub(rempli), CELL_BG),
        ];
        let bande = container(
            row![
                text(libelle).size(12).font(HELP_FONT).color(TEXT),
                container(track).width(Fill).height(Length::Fixed(3.0)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .height(Length::Fixed(BANDE_COPIE))
        .padding([0.0, 12.0])
        .align_y(Alignment::Center)
        .style(|_| container::Style {
            background: Some(Background::Color(Color {
                a: 0.62,
                ..Color::BLACK
            })),
            ..container::Style::default()
        });
        // enveloppe pleine page alignée en haut : un enfant de hauteur fixe
        // posé directement dans une `stack` n'est pas collé au bord
        container(bande)
            .width(Fill)
            .height(Fill)
            .align_y(Alignment::Start)
            .into()
    }

    /// Choix de la destination d'une copie : le dossier de téléchargements puis
    /// les dossiers de jeux, avec la place libre de chacun. La place manquante
    /// se voit sur la ligne (rouge), pas seulement au moment de valider.
    fn copy_menu_view(&self) -> Element<'_, Message> {
        let Some(menu) = &self.copy_menu else {
            return Row::new().into();
        };
        let nom = self
            .jeu_courant()
            .map(|game| game.name.clone())
            .unwrap_or_default();
        let mut rows = Column::new().spacing(6);
        for (index, dest) in menu.dests.iter().enumerate() {
            let selected = index == menu.index;
            let assez = dest.libre.map(|libre| libre >= menu.size).unwrap_or(true);
            let detail = match dest.libre {
                Some(libre) if assez => format!("{} libres", copy::poids(libre)),
                Some(libre) => format!("{} libres · insuffisant", copy::poids(libre)),
                None => "place inconnue".to_string(),
            };
            let couleur = if selected { Color::BLACK } else { TEXT };
            let ligne = container(
                row![
                    text(dest.label.clone()).size(17).width(Fill).color(couleur),
                    text(detail).size(13).color(if selected {
                        Color::BLACK
                    } else if assez {
                        MUTED
                    } else {
                        DANGER
                    }),
                ]
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([9.0, 14.0])
            .style(move |_| action_style(selected));
            rows = rows.push(mouse_area(ligne).on_press(Message::CopyChoose(index)));
        }

        let glyphes: Vec<(&image::Handle, &'static str)> = vec![
            (&self.glyphs.confirm, "Copier"),
            (&self.glyphs.back, "Annuler"),
        ];
        let panel = container(
            column![
                text(format!("Copier « {} » vers…", short(&nom, 34)))
                    .size(18)
                    .color(TEXT),
                text(format!(
                    "{} · l'original reste en place",
                    copy::poids(menu.size)
                ))
                .size(12)
                .color(MUTED),
                rows,
                self.aide_menu(&glyphes, "Croix = copier · Rond = annuler", false),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(560.0))
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

    /// Confirmation de mise à jour d'une copie existante mais plus ancienne.
    fn copy_replace_view(&self) -> Element<'_, Message> {
        let Some(replace) = &self.copy_replace else {
            return Row::new().into();
        };
        let mut actions = Row::new().spacing(12).align_y(Alignment::Center);
        for (index, label) in ["Annuler", "Mettre à jour"].into_iter().enumerate() {
            let selected = index == replace.index;
            let bouton = container(
                text(label)
                    .size(16)
                    .color(if selected { Color::BLACK } else { TEXT }),
            )
            .padding([9.0, 18.0])
            .style(move |_| action_style(selected));
            actions = actions.push(mouse_area(bouton).on_press(Message::CopyReplaceChoose(index)));
        }
        let glyphes: Vec<(&image::Handle, &'static str)> = vec![
            (&self.glyphs.confirm, "Valider"),
            (&self.glyphs.back, "Annuler"),
        ];
        let panel = container(
            column![
                text("Une copie plus ancienne est déjà là").size(18).color(TEXT),
                text(format!(
                    "{} existe dans {} — la version d'origine est plus récente.                      La remplacer par la nouvelle ?",
                    copy::poids(replace.job.size),
                    folders::affiche(&replace.job.dest)
                ))
                .size(13)
                .color(MUTED),
                actions,
                self.aide_menu(&glyphes, "Croix = valider · Rond = annuler", false),
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(600.0))
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

    /// État du poste, coin **haut gauche** : le réseau puis chaque manette
    /// connectée (une icône par manette, teintée par famille — quatre au plus,
    /// un compteur au-delà). La couche existe toujours (forme d'arbre
    /// constante) ; sans manette ni réseau elle ne dessine qu'une icône
    /// « hors ligne ».
    fn etat_view(&self) -> Element<'_, Message> {
        let mut ligne = Row::new().spacing(10).align_y(Alignment::Center);
        // réseau : en accent quand le lien est actif, sourdine sinon
        let couleur = match self.reseau {
            Reseau::Wifi | Reseau::Filaire => ACCENT,
            Reseau::HorsLigne => MUTED,
        };
        ligne = ligne.push(status::vue_reseau(self.reseau, couleur));
        let mut montrees = 0;
        for (kind, _) in self.pads.iter() {
            if montrees == 4 {
                break;
            }
            ligne = ligne.push(status::vue_manette(*kind));
            montrees += 1;
        }
        if self.pads.len() > montrees {
            ligne = ligne.push(
                text(format!("+{}", self.pads.len() - montrees))
                    .size(12)
                    .color(MUTED),
            );
        }
        ligne.into()
    }

    /// Coin bas droit de la fenêtre : état du poste (réseau, manettes) puis les
    /// deux roues (Scan à droite, Visuels à gauche d'elle). Le pied de page
    /// laisse cette zone libre — le tri et le compte sont partis à gauche.
    fn coin_bas_view(&self) -> Element<'_, Message> {
        container(
            row![
                self.etat_view(),
                self.indicateur(self.spin + 1.7, "Scan", self.scanning()),
                self.indicateur(self.spin, "Visuels", self.loading()),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .padding(Padding {
            top: 0.0,
            right: 14.0,
            bottom: 12.0,
            left: 0.0,
        })
        .into()
    }

    /// Indicateur d'activité du coin haut droit : la roue et son libellé à
    /// gauche. Éteint, il ne dessine rien (la place reste réservée : la forme
    /// de l'arbre ne doit pas changer d'un rendu à l'autre).
    fn indicateur(&self, angle: f32, libelle: &'static str, actif: bool) -> Element<'_, Message> {
        row![
            spinner::view::<Message>(angle, SPIN_SIZE, actif),
            text(if actif { libelle } else { "" }).size(12).color(MUTED),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    /// Liste des jeux (grille, liste ou mosaïque) avec en-tête et pied.
    fn launcher_view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = if self.games.is_empty() {
            if self.sort == SortBy::Favorites {
                self.favorites_empty()
            } else if self.sort == SortBy::Recent && self.recent.is_empty() {
                self.recent_empty()
            } else if let Some(year) = self.year_filter {
                self.year_empty(year)
            } else {
                self.empty_state()
            }
        } else {
            match self.config.view {
                ViewMode::Grid | ViewMode::Mosaic => scrollable(self.grid())
                    .id(self.scroll_id.clone())
                    .style(barre_style(self.barre))
                    .height(Fill)
                    .on_scroll(|viewport| Message::Scrolled {
                        offset: viewport.absolute_offset().y,
                        max: (viewport.content_bounds().height - viewport.bounds().height).max(0.0),
                    })
                    .into(),
                ViewMode::List => row![
                    scrollable(self.list())
                        .id(self.scroll_id.clone())
                        .style(barre_style(self.barre))
                        .height(Fill)
                        .width(Length::FillPortion(3))
                        .on_scroll(|viewport| Message::Scrolled {
                            offset: viewport.absolute_offset().y,
                            max: (viewport.content_bounds().height - viewport.bounds().height)
                                .max(0.0),
                        }),
                    container(self.preview())
                        .width(Length::FillPortion(2))
                        .height(Fill),
                ]
                .spacing(22)
                .into(),
            }
        };
        let footer = self.footer();
        // champ de recherche : visible tant qu'il est ouvert ou qu'un filtre
        // est actif, pour qu'on ne cherche pas pourquoi la liste est courte
        let mut pile = Column::new().spacing(0);
        if self.search_open || !self.search.is_empty() {
            let vide = self.search.is_empty();
            let ligne = text(if vide {
                "Rechercher : tape le début d'un nom…".to_string()
            } else {
                format!("Rechercher : {}", self.search)
            })
            .size(16)
            .color(if vide { MUTED } else { TEXT });
            let champ = container(ligne)
                .width(Fill)
                .padding([6.0, 10.0])
                .style(|_| container::Style {
                    background: Some(Background::Color(CELL_BG)),
                    border: Border {
                        color: if self.search_open { ACCENT } else { CELL_BORDER },
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..container::Style::default()
                });
            pile = pile.push(
                container(champ)
                    .width(Fill)
                    .center_x(Fill)
                    .padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 10.0,
                        left: 0.0,
                    }),
            );
        }
        pile = pile.push(content).push(footer);

        container(pile)
            .width(Fill)
            .height(Fill)
            // marge de droite plus large : la barre de défilement d'iced est
            // dessinée par-dessus le contenu, elle ne réserve aucune place
            .padding(Padding {
                top: PAGE_PADDING_Y,
                right: PAGE_PADDING_X_RIGHT,
                bottom: PAGE_PADDING_Y,
                left: PAGE_PADDING_X,
            })
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
        // Bande héros : elle grandit de 0 à sa hauteur quand l'image arrive
        // après l'ouverture (`page_hero`), et l'image apparaît en fondu —
        // décélération quadratique, pour que ça se pose au lieu de claquer.
        // La place réservée suit la même progression, donc la moitié basse se
        // comprime et glisse vers le bas au lieu de sauter d'un coup.
        let reveal = 1.0 - (1.0 - self.page_hero).powi(2);
        // Le bloc occupe franchement la place sous la bande (0,78 de la
        // hauteur restante) : à 0,68 il restait centré avec une grande bande
        // vide sous la ligne d'aide, qui paraissait remontée (signalé le
        // 23/11). Le plafond évite qu'il touche le bas sur un écran très haut.
        let dispo = (self.viewport.1 - band_h * reveal - 12.0).max(300.0);
        let cover_h = (dispo * 0.78).clamp(300.0, 540.0);
        let cover_w = cover_h * 205.0 / 308.0;
        // tout le bloc suit la cover : textes, pastilles et espacements
        let ech = (cover_h / 308.0).clamp(1.0, 1.7);
        // Pas de héros : aucune bande n'est réservée, et le bloc du bas se
        // centre sur toute la page — une bande grise vide en haut avec le
        // contenu collé en dessous faisait pauvre (retour 18/11).
        let band: Option<Element<'_, Message>> = game.hero.as_ref().map(|handle| {
            container(
                image(handle.clone())
                    .width(Length::Fixed(width))
                    .height(Length::Fixed(band_h))
                    .content_fit(ContentFit::Cover)
                    .opacity(reveal),
            )
            .width(Length::Fixed(width))
            .height(Length::Fixed(band_h * reveal))
            .clip(true)
            .into()
        });

        // Hauteur BORNÉE : le logo suivait `ech` sans plafond et, sur un grand
        // écran, poussait les pastilles et la ligne d'aide hors du bas de page
        // (constaté le 23/11). 150 px suffisent à le lire, quel que soit `ech`.
        let logo_h = (118.0 * ech).min(150.0);
        let logo_w = logo_h * 450.0 / 155.0;
        let logo: Element<'_, Message> = match &game.logo {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(logo_w))
                .height(Length::Fixed(logo_h))
                .content_fit(ContentFit::Contain)
                // logo arrivé après l'ouverture : il monte en fondu
                .opacity(ease_reveal(self.page_logo))
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
                // cover arrivée après l'ouverture : elle monte en fondu
                .opacity(ease_reveal(self.page_cover))
                .into(),
            None => sample_art(
                &self.sample,
                &game.name,
                game.icon.as_ref(),
                cover_w,
                cover_h,
            ),
        };

        // année de sortie (SteamGridDB) quand on la connaît, puis le temps de
        // jeu : les deux se lisent sur la même ligne, discrète
        let played = playtime::format(self.playtime.get(&game.slug).copied());
        let mut details = column![
            logo,
            text(short(&game.name, 60)).size(25.0 * ech).color(TEXT),
        ]
        .spacing(10.0 * ech)
        .align_x(Alignment::Center);
        // année de sortie juste sous le titre (c'est une information du jeu) ;
        // le temps de jeu reste en dessous, c'est une information du joueur
        if let Some(year) = self.year_of(&game.slug) {
            details = details.push(
                text(format!("Sorti en {year}"))
                    .size(14.0 * ech)
                    .color(MUTED),
            );
        }
        details = details.push(
            text(format!("Temps de jeu : {played}"))
                .size(14.0 * ech)
                .color(MUTED),
        );

        let favorite = self.favorites.contains(&game.slug);
        let mut actions = Row::new().spacing(14.0 * ech).align_y(Alignment::Center);
        for (index, action) in self.page_actions().iter().enumerate() {
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
                PageAction::Options => text("Options ▸")
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
                hint(self.glyphs.confirm.clone(), "Valider"),
                hint(self.glyphs.back.clone(), "Retour"),
            ]
            .spacing(18.0 * ech)
            .align_y(Alignment::Center)
            .into()
        } else {
            let mut aide = row![
                key_hint(&["Entrée"], "valider"),
                key_hint(&["Échap"], "retour"),
                key_hint(&["J"], "jouer"),
                key_hint(&["F"], "favori"),
            ];
            if self.page_actions().contains(&PageAction::Gallery) {
                aide = aide.push(key_hint(&["G"], "galerie"));
            }
            aide.spacing(18.0 * ech)
                .align_y(Alignment::Center)
                .into()
        };

        // Le bloc (cover, textes, pastilles) se centre dans la place laissée
        // libre **au-dessus** de l'aide de commande. Posé en bas (23/11), il
        // descendait trop : avec une bande héros, tout l'espace gagné partait
        // sous lui. L'aide, elle, reste la dernière ligne, collée au bas de la
        // page (demandé le 23/11).
        let milieu = container(
            row![
                cover,
                column![details, actions]
                    .spacing(20.0 * ech)
                    .align_x(Alignment::Center)
            ]
            .spacing(30.0 * ech)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill);
        let pied = container(help)
            .width(Fill)
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 8.0,
                left: 0.0,
            })
            .center_x(Fill);
        let body = column![milieu, pied]
            .width(Fill)
            .height(Fill)
            .padding(Padding {
                top: 20.0 * ech,
                right: 20.0 * ech,
                bottom: 0.0,
                left: 20.0 * ech,
            });

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

    /// Faut-il afficher les glyphes de manette ? Seulement si une manette est
    /// branchée ET que c'est elle qui a servi en dernier : reprendre le clavier
    /// fait réapparaître les touches dans le pied de page et les menus.
    fn pad_hints(&self) -> bool {
        self.pad.is_some()
            && (self.input == Input::Pad
                // session console : aucun clavier physique. Un mouvement de
                // souris (gpad-mouse, pointeur du jeu) basculait l'état en
                // « clavier » et les pages de jeu affichaient des aides en
                // texte, donc sans glyphes (signalé le 23/11).
                || self.mode.is_console())
    }

    /// Entrées du menu « Options », selon le format du paquet ouvert (le mode
    /// manette Xbox ne s'adresse qu'aux `.wgp`).
    /// Lignes du menu « Tri », dans l'ordre affiché.
    fn sort_menu_entries(&self) -> Vec<SortMenuEntry> {
        sort_menu_entries(self.recent.is_empty())
    }

    fn art_entries(&self) -> Vec<ArtMenuEntry> {
        let kind = self.art_kind();
        art_menu_entries(kind)
    }

    fn art_kind(&self) -> Kind {
        self.page
            .as_ref()
            .and_then(|page| self.games.get(page.index))
            .map(|game| game.kind)
            .unwrap_or(Kind::Lgp)
    }

    /// Réglages de lancement du jeu ouvert (défauts si aucun).
    fn art_options(&self) -> pergame::Options {
        self.page
            .as_ref()
            .and_then(|page| self.games.get(page.index))
            .map(|game| pergame::lire(&game.slug))
            .unwrap_or_default()
    }

    /// Applique une modification aux options du jeu ouvert et l'écrit tout de
    /// suite (fichier local, jamais dans `assets/` synchronisé).
    fn bascule_option<F: FnOnce(&mut pergame::Options)>(&mut self, modifier: F) {
        let Some(game) = self
            .page
            .as_ref()
            .and_then(|page| self.games.get(page.index))
        else {
            return;
        };
        let mut options = pergame::lire(&game.slug);
        modifier(&mut options);
        if let Err(err) = pergame::ecrire(&game.slug, options) {
            self.status = Some((format!("réglages du jeu non enregistrés : {err}"), false));
        }
    }

    /// Aide d'un menu : **glyphes de manette** quand c'est la manette qui
    /// pilote, texte quand c'est le clavier. Les options parlaient jusqu'ici
    /// uniquement en toutes lettres (« Croix = … ») alors que le pied de page
    /// de la liste affichait déjà les glyphes : l'écosystème doit se
    /// ressembler d'un écran à l'autre.
    fn aide_menu<'a>(
        &'a self,
        items: &[(&'a image::Handle, &'static str)],
        texte: &str,
        danger: bool,
    ) -> Element<'a, Message> {
        if !self.pad_hints() || items.is_empty() {
            return text(texte.to_string())
                .size(13)
                .color(if danger { DANGER } else { MUTED })
                .into();
        }
        let mut ligne = Row::new().spacing(16).align_y(Alignment::Center);
        for (glyphe, label) in items {
            ligne = ligne.push(hint((*glyphe).clone(), label));
        }
        ligne.into()
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

    /// Prompt d'identification : le nom à chercher chez SteamGridDB, puis la
    /// liste des jeux proposés (`identify_menu_view`).
    fn identify_view(&self) -> Element<'_, Message> {
        let Some(value) = self.identify.as_ref() else {
            return Row::new().into();
        };

        let panel = container(
            column![
                text("Identifier le jeu").size(18).color(TEXT),
                text(
                    "Cherche le nom exact du jeu chez SteamGridDB, puis choisis-le \
                     dans la liste. Le premier résultat peut être un autre épisode : \
                     c'est ce choix qui fixe les visuels et la date, et il est \
                     partagé avec les autres postes."
                )
                .size(14)
                .color(MUTED),
                text_input("Nom du jeu sur SteamGridDB", value)
                    .id(self.input_id.clone())
                    .on_input(Message::IdentifyInput)
                    .on_submit(Message::IdentifySubmit)
                    .padding(10)
                    .size(18)
                    .style(input_style),
                row![
                    key_hint(&["Entrée"], "chercher"),
                    key_hint(&["Échap"], "annuler"),
                ]
                .spacing(18)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(600.0))
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

    /// Confirmation « Oublier les jeux disparus ? » : « Annuler » par défaut,
    /// même raison que pour les visuels — rien ne part sur un appui malheureux.
    fn oublier_view(&self) -> Element<'_, Message> {
        let Some(oublier) = &self.oublier else {
            return Row::new().into();
        };

        let mut actions = Row::new().spacing(12).align_y(Alignment::Center);
        for (index, label) in ["Annuler", "Oublier"].into_iter().enumerate() {
            let selected = index == oublier.index;
            let bouton = container(
                text(label)
                    .size(16)
                    .color(if selected { Color::BLACK } else { TEXT }),
            )
            .padding([9.0, 18.0])
            .style(move |_| action_style(selected));
            actions = actions.push(mouse_area(bouton).on_press(Message::OublierChoose(index)));
        }

        let help: Element<'_, Message> = if self.pad_hints() {
            row![
                hint(self.glyphs.confirm.clone(), "Valider"),
                hint(self.glyphs.back.clone(), "Annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                key_hint(&["←", "→"], "choisir"),
                key_hint(&["Entrée"], "valider"),
                key_hint(&["Échap"], "annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        };

        let panel = container(
            column![
                text("Oublier les jeux disparus ?").size(18).color(TEXT),
                text(format!(
                    "{} ne sont plus dans la liste (disque débranché ou dossier retiré). \
                     Leurs favoris, historique, temps de jeu et réglages seront effacés. \
                     Les images, elles, sont conservées : un jeu rebranché retrouve ses visuels.",
                    plural(oublier.count)
                ))
                .size(14)
                .color(MUTED),
                actions,
                help,
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(640.0))
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

    /// Question posée après une identification : « ce paquet a déjà des
    /// visuels, on les remplace ? ». « Non » est sélectionné par défaut : un
    /// appui malheureux ne détruit pas des images choisies à la main.
    fn replace_view(&self) -> Element<'_, Message> {
        let Some(replace) = &self.replace else {
            return Row::new().into();
        };
        // nom du paquet (l'ancien libellé), pour que la question soit lisible
        let ancien = self
            .games
            .iter()
            .find(|game| game.slug == replace.slug)
            .map(|game| game.name.clone())
            .unwrap_or_else(|| replace.slug.clone());

        let mut actions = Row::new().spacing(12).align_y(Alignment::Center);
        for (index, label) in ["Non, garder", "Oui, remplacer"].into_iter().enumerate() {
            let selected = index == replace.index;
            let bouton =
                container(
                    text(label)
                        .size(16)
                        .color(if selected { Color::BLACK } else { TEXT }),
                )
                .padding([9.0, 18.0])
                .style(move |_| action_style(selected));
            actions = actions.push(mouse_area(bouton).on_press(Message::ReplaceChoose(index)));
        }

        let help: Element<'_, Message> = if self.pad_hints() {
            row![
                hint(self.glyphs.confirm.clone(), "Valider"),
                hint(self.glyphs.back.clone(), "Garder"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                key_hint(&["←", "→"], "choisir"),
                key_hint(&["Entrée"], "valider"),
                key_hint(&["Échap"], "garder"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        };

        let panel = container(
            column![
                text("Remplacer les visuels ?").size(18).color(TEXT),
                text(format!(
                    "« {ancien} » a déjà des visuels. Télécharger ceux de « {} » \
                     à la place ? Les images actuelles seront effacées.",
                    replace.game.name
                ))
                .size(14)
                .color(MUTED),
                actions,
                help,
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(640.0))
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

    /// Liste des jeux proposés par l'autocomplete : une ligne par jeu, avec
    /// son année, le jeu déjà identifié coché. Entrée (ou un clic) retient le
    /// choix et lance le téléchargement des visuels en fond.
    fn identify_menu_view(&self) -> Element<'_, Message> {
        let Some(menu) = &self.identify_menu else {
            return Row::new().into();
        };

        let body: Element<'_, Message> = if menu.loading {
            text("recherche sur SteamGridDB…")
                .size(14)
                .color(MUTED)
                .into()
        } else if let Some(error) = &menu.error {
            text(error.clone()).size(14).color(DANGER).into()
        } else if menu.games.is_empty() {
            text(
                "aucun jeu trouvé — corrige le nom (R) : le jeu n'est peut-être \
                 tout simplement pas dans SteamGridDB",
            )
            .size(14)
            .color(MUTED)
            .into()
        } else {
            let mut rows = Column::new().spacing(6);
            for (index, game) in menu.games.iter().enumerate() {
                let selected = index == menu.index;
                let current = Some(game.id) == menu.current;
                let label = match game.year {
                    Some(year) => format!("{}  ·  {year}", game.name),
                    None => game.name.clone(),
                };
                let line = container(
                    row![
                        text(label).size(17).width(Fill).color(if selected {
                            Color::BLACK
                        } else {
                            TEXT
                        }),
                        text(if current { "✓" } else { "" })
                            .size(14)
                            .color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([9.0, 14.0])
                .style(move |_| action_style(selected));

                rows = rows.push(mouse_area(line).on_press(Message::IdentifyChoose(index)));
            }
            scrollable(rows)
                .style(barre_style(self.barre))
                .height(Length::Fixed(320.0))
                .into()
        };

        let help: Element<'_, Message> = if self.pad_hints() {
            row![
                hint(self.glyphs.confirm.clone(), "Identifier"),
                hint(self.glyphs.back.clone(), "Annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                key_hint(&["Entrée"], "identifier ce jeu"),
                key_hint(&["R"], "corriger le nom"),
                key_hint(&["Échap"], "annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        };

        let panel = container(
            column![
                text(format!("Quel jeu est « {} » ?", menu.query))
                    .size(18)
                    .color(TEXT),
                body,
                help,
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(700.0))
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

    /// Prompt « date de sortie » : l'année tapée à la main (jeu absent de
    /// SteamGridDB), ou la demande à SteamGridDB.
    fn date_view(&self) -> Element<'_, Message> {
        let Some(value) = self.date.as_ref() else {
            return Row::new().into();
        };

        let refresh = mouse_area(
            container(text("Demander à SteamGridDB").size(14).color(TEXT))
                .padding([5.0, 12.0])
                .style(|_| action_style(false)),
        )
        .on_press(Message::DateRefresh);

        let panel = container(
            column![
                text("Date de sortie").size(18).color(TEXT),
                text(
                    "Tape l'année (1950 à 2100), ou laisse vide pour effacer la \
                     date. « Demander à SteamGridDB » la relit depuis le jeu \
                     identifié ou le nom du paquet."
                )
                .size(14)
                .color(MUTED),
                text_input("1998", value)
                    .id(self.input_id.clone())
                    .on_input(Message::DateInput)
                    .on_submit(Message::DateSubmit)
                    .padding(10)
                    .size(18)
                    .style(input_style),
                row![
                    container(
                        row![
                            key_hint(&["Entrée"], "enregistrer"),
                            key_hint(&["Échap"], "annuler"),
                        ]
                        .spacing(18)
                        .align_y(Alignment::Center),
                    )
                    .width(Fill),
                    refresh,
                ]
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(560.0))
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

    /// Menu « Options » de la page (visuels, identification, date).
    /// Menu « type de liste » (triangle / Y) : les quatre vues, la courante ✓.
    /// Lignes visibles dans les listes des réglages : la fenêtre glisse avec
    /// le curseur (aucune zone défilante à état, donc rien à reconstruire).
    fn lignes_visibles(&self) -> usize {
        (((self.viewport.1 - 300.0) / 52.0).floor() as usize).max(4)
    }

    /// Page Réglages : trois sections (dossiers de jeux, sortie, affichage).
    /// Les en-têtes ne sont pas sélectionnables : le curseur reste sur les
    /// lignes qui font quelque chose.
    fn settings_view(&self) -> Element<'_, Message> {
        let Some(settings) = self.settings.as_ref() else {
            return Row::new().into();
        };
        if let Some(browser) = settings.browser.as_ref() {
            return self.browser_view(settings, browser);
        }

        // deux lignes de moins que la liste nue : les trois en-têtes occupent
        // la place, et la fenêtre doit garder le curseur visible
        let visibles = self.lignes_visibles().saturating_sub(2).max(3);
        let (top, fin) = fenetre(settings.selected, settings.lignes(), visibles);
        let mut rows = Column::new().spacing(2);
        let mut section = "";
        for index in top..fin {
            let selected = index == settings.selected;
            // quelle section : jeux, sortie, son, affichage
            let courante = if index <= settings.dossiers.len() {
                "Dossiers de jeux"
            } else if index <= settings.ligne_shots() {
                "Sortie"
            } else if index <= settings.ligne_volume() {
                "Son"
            } else if index <= settings.ligne_ecran() {
                "Affichage"
            } else {
                "Entretien"
            };
            if courante != section {
                section = courante;
                rows = rows.push(
                    container(text(section).size(12).color(ACCENT))
                        .width(Fill)
                        .padding(Padding {
                            top: if index == top { 0.0 } else { 14.0 },
                            right: 14.0,
                            bottom: 4.0,
                            left: 14.0,
                        }),
                );
            }

            // dossiers de sortie : chemin courant (ou « défaut ») à droite
            if let Some(cle) = settings.cle_sortie(index) {
                let (label, courant, defaut) = if cle == recorder::CLE_CAPTURES {
                    (
                        "Enregistrements et replays",
                        settings.captures.clone(),
                        recorder::defaut_captures(),
                    )
                } else {
                    (
                        "Captures d'écran",
                        settings.shots.clone(),
                        recorder::defaut_shots(),
                    )
                };
                let (chemin, est_defaut) = recorder::affichage(courant, defaut);
                let affiche = folders::affiche(std::path::Path::new(&chemin));
                let valeur = if est_defaut {
                    format!("défaut · {affiche}")
                } else {
                    affiche
                };
                let ligne = container(
                    row![
                        text(label)
                            .size(17)
                            .width(Fill)
                            .color(if selected { Color::BLACK } else { TEXT }),
                        text(valeur).size(14).color(if selected {
                            Color::BLACK
                        } else if est_defaut {
                            MUTED
                        } else {
                            ACCENT
                        }),
                        text(" ▸")
                            .size(14)
                            .color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([11.0, 14.0])
                .style(move |_| action_style(selected));
                rows = rows.push(mouse_area(ligne).on_press(if selected {
                    Message::SettingsAct
                } else {
                    Message::Nothing
                }));
                continue;
            }

            // musique de fond : Croix coupe / relance (appliqué tout de suite)
            if index == settings.ligne_musique() {
                let ligne = container(
                    row![
                        text("Musique de fond")
                            .size(17)
                            .width(Fill)
                            .color(if selected { Color::BLACK } else { TEXT }),
                        text(if settings.musique { "activée" } else { "coupée" })
                            .size(14)
                            .color(if selected {
                                Color::BLACK
                            } else if settings.musique {
                                ACCENT
                            } else {
                                MUTED
                            }),
                        interrupteur(settings.musique, selected),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([11.0, 14.0])
                .style(move |_| action_style(selected));
                rows = rows.push(mouse_area(ligne).on_press(if selected {
                    Message::SettingsAct
                } else {
                    Message::Nothing
                }));
                continue;
            }

            // volume de la musique : gauche/droite par pas de 5 %
            if index == settings.ligne_volume() {
                let ligne = container(
                    row![
                        text("Volume de la musique")
                            .size(17)
                            .width(Fill)
                            .color(if selected { Color::BLACK } else { TEXT }),
                        barre(settings.volume, 170.0, selected),
                        // « ◀ ▶ » : la barre seule ne dit pas qu'elle se
                        // règle au pad (il n'y a pas de glyphe de croix
                        // directionnelle dans les visuels embarqués)
                        text(format!("◀ {} % ▶", settings.volume))
                            .size(15)
                            .width(Length::Fixed(86.0))
                            .color(if selected {
                                Color::BLACK
                            } else if settings.musique {
                                TEXT
                            } else {
                                MUTED
                            }),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([11.0, 14.0])
                .style(move |_| action_style(selected));
                rows = rows.push(ligne);
                continue;
            }

            // dernière ligne : l'écran du jeu (gauche/droite), pas un dossier
            if index == settings.ligne_ecran() {
                let (_, libelle) = ECRANS[settings.ecran];
                let valeur = format!("◀ {libelle} ▶");
                let ligne = container(
                    row![
                        text("Écran du jeu")
                            .size(17)
                            .width(Fill)
                            .color(if selected { Color::BLACK } else { TEXT }),
                        text(valeur).size(15).color(if selected {
                            Color::BLACK
                        } else {
                            MUTED
                        }),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([11.0, 14.0])
                .style(move |_| action_style(selected));
                rows = rows.push(ligne);
                continue;
            }

            // entretien : scanner les disques externes branchés
            if index == settings.ligne_disques() {
                let ligne = container(
                    row![
                        text("Scanner les disques externes")
                            .size(17)
                            .width(Fill)
                            .color(if selected { Color::BLACK } else { TEXT }),
                        text(if settings.disques { "activé" } else { "coupé" })
                            .size(14)
                            .color(if selected {
                                Color::BLACK
                            } else if settings.disques {
                                ACCENT
                            } else {
                                MUTED
                            }),
                        interrupteur(settings.disques, selected),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([11.0, 14.0])
                .style(move |_| action_style(selected));
                rows = rows.push(mouse_area(ligne).on_press(if selected {
                    Message::SettingsAct
                } else {
                    Message::Nothing
                }));
                continue;
            }

            // entretien : oublier les fiches des jeux qui ne sont plus là
            if index == settings.ligne_oublier() {
                let ligne = container(
                    row![
                        text("Oublier les jeux disparus")
                            .size(17)
                            .width(Fill)
                            .color(if selected { Color::BLACK } else { TEXT }),
                        text(" ▸")
                            .size(14)
                            .color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding([11.0, 14.0])
                .style(move |_| action_style(selected));
                rows = rows.push(
                    mouse_area(ligne)
                        .on_press(if selected { Message::OublierOpen } else { Message::Nothing }),
                );
                continue;
            }

            let (label, detail, danger) = if index == 0 {
                ("+ Ajouter un dossier…".to_string(), String::new(), false)
            } else {
                let dossier = &settings.dossiers[index - 1];
                let arme = settings.confirm == Some(index - 1);
                (
                    folders::affiche(Path::new(dossier)),
                    if arme { "retirer ?".to_string() } else { String::new() },
                    arme,
                )
            };
            let couleur = if danger {
                DANGER
            } else if selected {
                Color::BLACK
            } else if index == 0 {
                ACCENT
            } else {
                TEXT
            };
            let ligne = container(
                row![
                    text(label).size(17).width(Fill).color(couleur),
                    text(detail)
                        .size(14)
                        .color(if selected { Color::BLACK } else { DANGER }),
                ]
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([11.0, 14.0])
            .style(move |_| action_style(selected));
            let cible = if index == 0 {
                Message::SettingsAct
            } else {
                Message::SettingsArm
            };
            // clic : même chose que la Croix sur cette ligne
            rows = rows.push(mouse_area(ligne).on_press(if index == settings.selected {
                cible
            } else {
                Message::Nothing
            }));
        }

        let aide = if let Some(index) = settings.confirm {
            format!(
                "Retirer {} de la liste ? Le dossier et son contenu ne sont pas touchés. Croix = retirer · Rond = annuler",
                settings
                    .dossiers
                    .get(index)
                    .map(|dossier| folders::affiche(Path::new(dossier)))
                    .unwrap_or_default()
            )
        } else if let Some(cle) = settings.cle_sortie(settings.selected) {
            let (quoi, defaut) = if cle == recorder::CLE_CAPTURES {
                ("enregistrements et replays", recorder::defaut_captures())
            } else {
                ("captures d'écran", recorder::defaut_shots())
            };
            format!(
                "Croix = choisir le dossier des {quoi} · Carré = revenir au défaut ({defaut})"
            )
        } else if settings.selected == settings.ligne_musique() {
            if settings.musique {
                "Croix = couper la musique de fond du lanceur. Elle ne joue pas pendant une partie, et se tait dès qu'une autre application joue du son.".to_string()
            } else {
                "Croix = relancer la musique de fond (elle reprend là où elle s'était arrêtée).".to_string()
            }
        } else if settings.selected == settings.ligne_volume() {
            "Gauche/droite = volume de la musique, par pas de 5 % — 0 % la rend silencieuse sans la couper.".to_string()
        } else if settings.selected == settings.ligne_ecran() {
            "Gauche/droite = écran du jeu. « Comme Plasma » suit la disposition du bureau ; « Cloner » réplique l'écran principal sur les autres ; « Principal seul » éteint les autres — à la prochaine session de jeu.".to_string()
        } else if settings.selected == settings.ligne_disques() {
            "Croix = activer ou couper le scan des disques externes branchés. Activé, un disque monté apporte ses jeux à la bibliothèque et son retrait les retire. La découverte est mémorisée : un disque déjà vu est reconnu instantanément.".to_string()
        } else if settings.selected == settings.ligne_oublier() {
            "Oublie les jeux qui ne sont plus dans la liste (disque débranché, dossier retiré) : favoris, historique, temps de jeu, réglages de lancement. Les images sont conservées — un jeu rebranché retrouve ses visuels.".to_string()
        } else if settings.dossiers.is_empty() {
            "Aucun dossier de jeux : la bibliothèque est vide. Croix = ajouter un dossier.".to_string()
        } else if settings.selected == 0 {
            "Croix = parcourir et ajouter un dossier de jeux (.wgp / .lgp).".to_string()
        } else {
            "Croix = ouvrir le dossier dans le navigateur · Carré = retirer de la liste · Rond = fermer".to_string()
        };

        // glyphes de l'aide, selon la ligne courante (le texte reste pour le
        // clavier : `aide_menu` choisit)
        let glyphes: Vec<(&image::Handle, &'static str)> = if settings.confirm.is_some() {
            vec![
                (&self.glyphs.confirm, "Retirer"),
                (&self.glyphs.back, "Annuler"),
            ]
        } else if settings.cle_sortie(settings.selected).is_some() {
            vec![
                (&self.glyphs.confirm, "Choisir"),
                (&self.glyphs.menu, "Défaut"),
                (&self.glyphs.back, "Fermer"),
            ]
        } else if settings.selected == settings.ligne_musique() {
            vec![
                (
                    &self.glyphs.confirm,
                    if settings.musique { "Couper" } else { "Relancer" },
                ),
                (&self.glyphs.back, "Fermer"),
            ]
        } else if settings.selected == 0 {
            vec![
                (&self.glyphs.confirm, "Ajouter"),
                (&self.glyphs.back, "Fermer"),
            ]
        } else if settings.selected == settings.ligne_disques() {
            vec![
                (
                    &self.glyphs.confirm,
                    if settings.disques { "Couper" } else { "Activer" },
                ),
                (&self.glyphs.back, "Fermer"),
            ]
        } else if settings.selected == settings.ligne_oublier() {
            vec![(&self.glyphs.confirm, "Oublier"), (&self.glyphs.back, "Fermer")]
        } else if settings.selected == settings.ligne_volume()
            || settings.selected == settings.ligne_ecran()
            || settings.selected < settings.lignes()
        {
            vec![(&self.glyphs.back, "Fermer")]
        } else {
            vec![(&self.glyphs.back, "Fermer")]
        };

        let panel = container(
            column![
                row![
                    text("Réglages").size(28).color(TEXT).width(Fill),
                    text(format!("{} / {}", settings.selected + 1, settings.lignes()))
                        .size(13)
                        .color(MUTED),
                ]
                .align_y(Alignment::Center),
                text("Réglages du lanceur — hors jeu. Pendant une partie, c'est le menu de la manette (Home).")
                    .size(13)
                    .color(MUTED),
                rows,
                self.aide_menu(&glyphes, &aide, settings.confirm.is_some()),
            ]
            .spacing(10),
        )
        .padding(24)
        .width(Length::Fixed(780.0))
        .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.9,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    /// Navigateur de dossiers : chemin courant, « choisir ce dossier », puis
    /// « .. » et les sous-dossiers.
    /// Navigateur de dossiers : même langage que la page Réglages (titre,
    /// compteur, lignes en pastilles, aide en glyphes). Le chemin courant est
    /// écrit en **fil d'Ariane**, et « Choisir ce dossier » est la première
    /// ligne, détachée : c'est l'action pour laquelle on est venu.
    fn browser_view(&self, _settings: &Settings, browser: &Browser) -> Element<'_, Message> {
        let visibles = self.lignes_visibles().saturating_sub(2).max(3);
        let total = browser.entrees.len() + 1;
        let (top, fin) = fenetre(browser.selected, total, visibles);
        let mut rows = Column::new().spacing(2);

        for index in top..fin {
            let selected = index == browser.selected;
            // ligne 0 : l'action, en accent ; ensuite « .. » puis les dossiers
            let (prefixe, label, couleur) = if index == 0 {
                ("✓", "Choisir ce dossier".to_string(), ACCENT)
            } else {
                let entree = &browser.entrees[index - 1];
                if entree.remonter {
                    ("↰", "Remonter d'un niveau".to_string(), MUTED)
                } else {
                    ("▸", entree.nom.clone(), TEXT)
                }
            };
            let ligne = container(
                row![
                    text(prefixe)
                        .size(15)
                        .width(Length::Fixed(18.0))
                        .color(if selected { Color::BLACK } else { couleur }),
                    text(label)
                        .size(17)
                        .width(Fill)
                        .color(if selected { Color::BLACK } else { couleur }),
                ]
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([11.0, 14.0])
            .style(move |_| action_style(selected));
            rows = rows.push(mouse_area(ligne).on_press(if selected {
                Message::BrowserAct
            } else {
                Message::Nothing
            }));
        }

        // dossier vide : on le dit, plutôt que de laisser un panneau nu
        if browser.entrees.iter().all(|entree| entree.remonter) {
            rows = rows.push(
                container(text("Aucun sous-dossier ici").size(14).color(MUTED))
                    .width(Fill)
                    .padding([10.0, 14.0]),
            );
        }

        let quoi = match browser.choix {
            Choix::Jeux => "Dossier de jeux à ajouter à la bibliothèque",
            Choix::Captures => "Dossier des enregistrements et replays",
            Choix::Shots => "Dossier des captures d'écran",
        };

        // fil d'Ariane : les trois derniers niveaux, le courant en accent
        let complet = folders::affiche(&browser.chemin);
        let niveaux: Vec<&str> = complet
            .split('/')
            .filter(|partie| !partie.is_empty())
            .collect();
        let premiers = niveaux.len().saturating_sub(3);
        let mut fil = Row::new().spacing(6).align_y(Alignment::Center);
        if premiers > 0 {
            fil = fil.push(text("… /").size(15).color(MUTED));
        }
        for (rang, niveau) in niveaux.iter().enumerate().skip(premiers) {
            if rang > premiers {
                fil = fil.push(text("/").size(15).color(MUTED));
            }
            let dernier = rang + 1 == niveaux.len();
            fil = fil.push(
                text((*niveau).to_string())
                    .size(15)
                    .color(if dernier { ACCENT } else { MUTED }),
            );
        }

        let glyphes: Vec<(&image::Handle, &'static str)> = vec![
            (&self.glyphs.confirm, "Entrer / choisir"),
            (&self.glyphs.back, "Remonter"),
        ];
        let aide = "Croix = entrer dans le dossier (ou le choisir, première ligne) · Rond = remonter d'un niveau, et refermer à la racine";

        let panel = container(
            column![
                row![
                    text("Choisir un dossier").size(26).color(TEXT).width(Fill),
                    text(format!(
                        "{} / {}",
                        browser.selected.saturating_add(1),
                        total
                    ))
                    .size(13)
                    .color(MUTED),
                ]
                .align_y(Alignment::Center),
                text(quoi).size(13).color(MUTED),
                container(fil).padding([8.0, 12.0]).style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.45, ..BG })),
                    border: Border {
                        radius: 10.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
                rows,
                self.aide_menu(&glyphes, aide, false),
            ]
            .spacing(10),
        )
        .padding(24)
        .width(Length::Fixed(780.0))
        .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.82,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    fn gallery_view(&self) -> Element<'_, Message> {
        let Some(galerie) = self.galerie.as_ref() else {
            return Row::new().into();
        };
        let (colonnes, tuile_w, tuile_h, pas) = galerie_geometrie(self.viewport.0);
        let colonnes = colonnes.max(1);
        // rangées qui tiennent VRAIMENT : la hauteur restante, pas une
        // constante pessimiste (4 rangées en 2K au lieu de 5)
        let lignes_vues = (((self.viewport.1 - GALERIE_CHROME) / pas).floor() as usize).max(1);
        let total_lignes = galerie.items.len().div_ceil(colonnes).max(1);
        let (haut, bas) = fenetre(galerie.selected / colonnes, total_lignes, lignes_vues);
        let mut grille = Column::new().spacing(10);
        for ligne in haut..bas {
            let mut rangee = Row::new().spacing(12);
            for colonne in 0..colonnes {
                let index = ligne * colonnes + colonne;
                let Some(item) = galerie.items.get(index) else {
                    break;
                };
                let selectionne = index == galerie.selected;
                let arme = galerie.arme == Some(index);
                let vignette: Element<'_, Message> =
                    match galerie.thumbs.get(&item.path).and_then(|handle| handle.clone()) {
                        Some(handle) => image(handle)
                            .width(Length::Fixed(tuile_w))
                            .height(Length::Fixed(tuile_h))
                            .content_fit(ContentFit::Cover)
                            .into(),
                        // vignette en cours de fabrication : case vide, jamais
                        // un trou qui fait sauter la grille
                        None => container(
                            text(if item.genre == gallery::Genre::Video {
                                "…"
                            } else {
                                ""
                            })
                            .size(18)
                            .color(MUTED),
                        )
                        .width(Length::Fixed(tuile_w))
                        .height(Length::Fixed(tuile_h))
                        .center_x(Length::Fixed(tuile_w))
                        .center_y(Length::Fixed(tuile_h))
                        .style(|_| container::Style {
                            background: Some(Background::Color(Color {
                                a: 0.35,
                                ..Color::BLACK
                            })),
                            border: Border {
                                color: Color { a: 0.25, ..TEXT },
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..container::Style::default()
                        })
                        .into(),
                    };
                // Deux lignes, pas plus : le **nom du jeu** quand la capture
                // le connaît (métadonnée écrite au moment de la prise), puis
                // `genre · date · poids`. Le nom du FICHIER
                // (`Video_20261120_101500.mp4`) ne dit rien à personne et
                // n'apparaît jamais. Capture d'avant la métadonnée : la
                // première ligne reprend le badge et le poids, comme avant.
                let date = item.date_lisible();
                let (premiere, seconde, taille, teinte) = match item.titre.as_deref() {
                    Some(titre) => (
                        short(titre, 30),
                        format!("{} · {date} · {}", item.genre.badge(), item.poids()),
                        14,
                        if selectionne { ACCENT } else { TEXT },
                    ),
                    None => (
                        format!("{} · {}", item.genre.badge(), item.poids()),
                        date,
                        12,
                        if selectionne { ACCENT } else { MUTED },
                    ),
                };
                let legende = column![
                    text(premiere).size(taille).color(teinte),
                    text(seconde).size(12).color(MUTED),
                ]
                .spacing(2);
                // la confirmation est un vrai panneau : la tuile armée se
                // contente d'un liseré rouge, sans texte qui pousse la grille
                let tuile = container(column![vignette, legende].spacing(6))
                    .padding(6)
                    .style(move |_| {
                        let mut style = card_style(selectionne, false);
                        if arme {
                            style.border.color = DANGER;
                            style.border.width = 2.0;
                        }
                        style
                    });
                let cliquable: Element<'_, Message> =
                    mouse_area(tuile).on_press(Message::GalleryChoose(index)).into();
                rangee = rangee.push(cliquable);
            }
            grille = grille.push(rangee);
        }

        let (aide, glyphes): (String, Vec<(&image::Handle, &'static str)>) =
            if galerie.items.is_empty() {
                (
                    match galerie.jeu.as_deref() {
                        Some(nom) => format!("Aucune capture ni vidéo pour {}.", short(nom, 40)),
                        None => "Aucune capture ni vidéo dans les dossiers de sortie (Réglages pour les changer)."
                            .to_string(),
                    },
                    vec![(&self.glyphs.back, "Fermer")],
                )
            } else {
                (
                    "Croix = ouvrir · Carré = corbeille · Triangle = captur./vidéos · Rond = fermer"
                        .to_string(),
                    vec![
                        (&self.glyphs.confirm, "Ouvrir"),
                        (&self.glyphs.menu, "Corbeille"),
                        (&self.glyphs.sort, "Filtrer"),
                        (&self.glyphs.back, "Fermer"),
                    ],
                )
            };

        let filtre = match galerie.filtre {
            None => "tout",
            Some(gallery::Genre::Image) => "captures",
            Some(gallery::Genre::Video) => "vidéos",
        };
        let panel = container(
            column![
                row![
                    text("Galerie").size(28).color(TEXT),
                    match galerie.jeu.as_deref() {
                        Some(nom) => text(short(nom, 40)).size(17).color(TEXT),
                        None => text(""),
                    },
                    text(filtre).size(17).color(ACCENT),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                text(galerie_legende(galerie)).size(13).color(MUTED),
                grille,
                text(format!(
                    "{}/{} · {}",
                    galerie.selected.saturating_add(1),
                    galerie.items.len(),
                    if galerie.restant > 0 {
                        format!("{} vignette(s) en préparation", galerie.restant)
                    } else {
                        "vignettes prêtes".to_string()
                    }
                ))
                .size(12)
                .color(MUTED),
                self.aide_menu(&glyphes, &aide, false),
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Fill)
        .height(Fill);

        // Fond opaque (la galerie EST l'écran : la liste des jeux ne doit pas
        // se lire derrière) PUIS la poussière d'étoiles par-dessus, comme sur
        // la liste : la galerie ne perd pas le fond du launcher.
        let fond = container(Row::new())
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::BLACK)),
                ..container::Style::default()
            });
        let poussiere: Element<'_, Message> = if self.config.sparkles {
            self.sparkles.view(self.scroll_drawn(), self.fade, None)
        } else {
            container(Row::new()).width(Fill).height(Fill).into()
        };
        stack![fond, panel, poussiere].into()
    }

    /// Confirmation d'effacement : un vrai panneau au centre, pas un texte
    /// sous la tuile (demandé le 23/11). Croix efface, Rond annule — les deux
    /// seuls boutons actifs tant qu'il est ouvert.
    fn gallery_confirm_view(&self) -> Element<'_, Message> {
        let Some(galerie) = self.galerie.as_ref() else {
            return Row::new().into();
        };
        let Some(item) = galerie.arme.and_then(|index| galerie.items.get(index)) else {
            return Row::new().into();
        };
        let quoi = if item.genre == gallery::Genre::Video {
            "CETTE VIDÉO"
        } else {
            "CETTE CAPTURE"
        };
        let mut texte = column![text(format!("METTRE {quoi} À LA CORBEILLE ?"))
            .size(20)
            .color(TEXT)]
        .spacing(10);
        if let Some(titre) = item.titre.as_deref() {
            texte = texte.push(text(short(titre, 44)).size(17).color(ACCENT));
        }
        texte = texte.push(
            text(format!("{} · {}", item.genre.badge(), item.poids()))
                .size(14)
                .color(MUTED),
        );
        texte = texte.push(text(item.date_lisible()).size(14).color(MUTED));
        texte = texte.push(
            text("Le fichier part dans la corbeille : récupérable depuis Dolphin.")
                .size(13)
                .color(MUTED),
        );
        texte = texte.push(
            self.aide_menu(
                &[
                    (&self.glyphs.confirm, "Corbeille"),
                    (&self.glyphs.back, "Annuler"),
                ],
                "Croix = corbeille · Rond = annuler",
                true,
            ),
        );

        let panel = container(texte)
            .padding(26)
            .width(Length::Fixed(520.0))
            .style(|_| picker_panel_style());

        container(panel)
            .width(Fill)
            .height(Fill)
            .padding(24)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.82,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    /// Capture d'écran en plein écran : fondu et léger agrandissement à
    /// l'ouverture (aucune animation ensuite — une image ne bouge pas).
    fn apercu_view(&self) -> Element<'_, Message> {
        let Some(apercu) = self.apercu.as_ref() else {
            return Row::new().into();
        };
        let Some(galerie) = self.galerie.as_ref() else {
            return Row::new().into();
        };
        let Some(item) = galerie.items.get(apercu.index) else {
            return Row::new().into();
        };
        // l'image PLEINE (pas la vignette) : c'est une capture, on la regarde
        // l'image arrive pleine, elle ne « monte » pas en fondu : une entrée
        // de 0,16 s (légère mise à l'échelle) suffit à faire une transition
        let image: Element<'_, Message> = image(image::Handle::from_path(&item.path))
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Contain)
            .opacity(1.0 - 0.30 * apercu.entree)
            .into();
        let image: Element<'_, Message> = Pose::new(image)
            .scale(1.0 + 0.03 * apercu.entree)
            .into();
        let mut glyphes: Vec<(&image::Handle, &'static str)> =
            vec![(&self.glyphs.back, "Retour")];
        if item.logo.is_some() {
            glyphes.insert(0, (&self.glyphs.menu, "Copie avec le logo"));
        }
        let aide = if item.logo.is_some() {
            "Gauche/droite = précédente / suivante · Carré = copie avec le logo · Rond = retour"
        } else {
            "Gauche/droite = précédente / suivante · Rond = retour · (copie avec logo : capture faite en jeu uniquement)"
        };
        let pied = container(self.aide_menu(&glyphes, aide, false))
            .width(Fill)
            .padding(12);

        let fond = container(Row::new())
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::BLACK)),
                ..container::Style::default()
            });

        stack![fond, image, container(pied).width(Fill).height(Fill).align_y(Alignment::End)].into()
    }

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

        // dernière ligne : la page RÉGLAGES (dossiers de jeux). Le launcher
        // n'a pas de bouton libre sur la manette : elle vit donc dans ce menu,
        // ouvert au triangle — et `F2` au clavier.
        let reglages_selected = choice == VIEWS.len();
        let ligne = container(
            row![
                text("Réglages…")
                    .size(18)
                    .width(Fill)
                    .color(if reglages_selected { Color::BLACK } else { TEXT }),
                text("▸")
                    .size(14)
                    .color(if reglages_selected { Color::BLACK } else { MUTED }),
            ]
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([9.0, 14.0])
        .style(move |_| action_style(reglages_selected));
        rows = rows.push(
            mouse_area(ligne).on_press(Message::ViewMenuChoose(VIEWS.len())),
        );

        // puis la galerie : captures d'écran, enregistrements, replays
        let galerie_selected = choice == VIEWS.len() + 1;
        let ligne = container(
            row![
                text("Galerie…")
                    .size(18)
                    .width(Fill)
                    .color(if galerie_selected { Color::BLACK } else { TEXT }),
                text("▸")
                    .size(14)
                    .color(if galerie_selected { Color::BLACK } else { MUTED }),
            ]
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([9.0, 14.0])
        .style(move |_| action_style(galerie_selected));
        rows = rows.push(
            mouse_area(ligne).on_press(Message::ViewMenuChoose(VIEWS.len() + 1)),
        );

        let panel =
            container(column![text("Type de liste").size(18).color(TEXT), rows].spacing(12))
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

    /// Menu « tri » (carré / X, `T`) : A→Z, récents, favoris — la même
    /// présentation que le menu des vues, le tri courant coché.
    fn sort_menu_view(&self) -> Element<'_, Message> {
        let Some(choice) = self.sort_menu else {
            return Row::new().into();
        };
        let entries = self.sort_menu_entries();

        let mut rows = Column::new().spacing(6);
        for (index, entry) in entries.iter().enumerate() {
            let selected = index == choice;
            let couleur = if selected { Color::BLACK } else { TEXT };
            let ligne = match entry {
                SortMenuEntry::Sort(sort) => {
                    let current = *sort == self.sort;
                    container(
                        row![
                            text(sort.label()).size(18).width(Fill).color(couleur),
                            text(if current { "✓" } else { "" })
                                .size(14)
                                .color(if selected { Color::BLACK } else { MUTED }),
                        ]
                        .align_y(Alignment::Center),
                    )
                }
                // filtre d'année : ouvre son propre menu
                SortMenuEntry::Year => container(
                    row![
                        text("Année").size(18).width(Fill).color(couleur),
                        text(match self.year_filter {
                            Some(year) => format!("{year}  ▸"),
                            None => "toutes  ▸".to_string(),
                        })
                        .size(14)
                        .color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                ),
                // portée : cycle sur place (tous → dossiers → disques)
                SortMenuEntry::Scope => container(
                    row![
                        text("Bibliothèque").size(18).width(Fill).color(couleur),
                        text(self.scope.label()).size(14).color(if selected {
                            Color::BLACK
                        } else if self.scope == Scope::Tous {
                            MUTED
                        } else {
                            ACCENT
                        }),
                    ]
                    .align_y(Alignment::Center),
                ),
                // effacement de l'historique : jamais proposé sur du vide
                SortMenuEntry::Clear => container(
                    text("Effacer les derniers lancés")
                        .size(18)
                        .color(if selected { Color::BLACK } else { MUTED }),
                ),
            };
            let ligne = ligne
                .width(Fill)
                .padding([9.0, 14.0])
                .style(move |_| action_style(selected));
            rows = rows.push(mouse_area(ligne).on_press(Message::SortMenuChoose(index)));
        }

        let panel = container(column![text("Tri").size(18).color(TEXT), rows].spacing(12))
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

    /// Menu « Année » : filtre la bibliothèque sur la date de sortie. Les
    /// années proposées sont celles **connues** (elles s'apprennent à
    /// l'ouverture de la page d'un jeu), avec le nombre de jeux concernés.
    fn year_menu_view(&self) -> Element<'_, Message> {
        let Some(choice) = self.year_menu else {
            return Row::new().into();
        };
        let years = self.known_years();

        let mut rows = Column::new().spacing(6);
        // « toutes » d'abord
        let all_selected = choice == 0;
        let line = container(
            row![
                text("Toutes les années")
                    .size(18)
                    .width(Fill)
                    .color(if all_selected { Color::BLACK } else { TEXT }),
                text(if self.year_filter.is_none() {
                    "✓"
                } else {
                    ""
                })
                .size(14)
                .color(if all_selected { Color::BLACK } else { MUTED }),
            ]
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([9.0, 14.0])
        .style(move |_| action_style(all_selected));
        rows = rows.push(mouse_area(line).on_press(Message::YearMenuChoose(0)));

        for (position, year) in years.iter().enumerate() {
            let selected = choice == position + 1;
            let current = self.year_filter == Some(*year);
            let line = container(
                row![
                    text(format!("{year}"))
                        .size(18)
                        .width(Fill)
                        .color(if selected { Color::BLACK } else { TEXT }),
                    text(format!(
                        "{}{}",
                        self.year_count(*year),
                        if current { " ✓" } else { "" }
                    ))
                    .size(14)
                    .color(if selected { Color::BLACK } else { MUTED }),
                ]
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([9.0, 14.0])
            .style(move |_| action_style(selected));

            rows = rows.push(mouse_area(line).on_press(Message::YearMenuChoose(position + 1)));
        }

        let panel = container(
            column![
                text("Année de sortie").size(18).color(TEXT),
                rows,
                text("Les années s'apprennent en ouvrant la page d'un jeu.")
                    .size(12)
                    .color(MUTED),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(320.0))
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
        let identity = self.identities.get(&game.slug);
        let absent = self.absents.get(&game.slug).filter(|absent| covers::absent_frais(absent));
        let year = self.year_of(&game.slug);
        let options = self.art_options();
        let entries = art_menu_entries(game.kind);

        let mut rows = Column::new().spacing(6);
        for (index, entry) in entries.iter().enumerate() {
            let selected = index == choice;
            let couleur = if selected { Color::BLACK } else { TEXT };
            // les trois options de lancement se dessinent comme dans
            // l'overlay : interrupteur pour un état, valeur pour un cycle
            let ligne = match entry {
                ArtMenuEntry::Gamescope | ArtMenuEntry::Xbox => {
                    let (label, actif) = if *entry == ArtMenuEntry::Gamescope {
                        ("Lancer dans gamescope", options.gamescope)
                    } else {
                        ("Manettes en mode Xbox", options.xbox)
                    };
                    container(
                        row![
                            text(label).size(18).width(Fill).color(couleur),
                            interrupteur(actif, selected),
                        ]
                        .align_y(Alignment::Center),
                    )
                }
                ArtMenuEntry::Resolution => container(
                    row![
                        text("Résolution forcée").size(18).width(Fill).color(couleur),
                        text("◀ ").size(14).color(if selected {
                            Color::BLACK
                        } else {
                            MUTED
                        }),
                        text(options.resolution_label())
                            .size(15)
                            .color(if selected {
                                Color::BLACK
                            } else if options.resolution.is_some() {
                                ACCENT
                            } else {
                                MUTED
                            }),
                        text(" ▶").size(14).color(if selected {
                            Color::BLACK
                        } else {
                            MUTED
                        }),
                    ]
                    .align_y(Alignment::Center),
                ),
                ArtMenuEntry::Kind(kind) => {
                    let slot = covers::ArtKind::ALL
                        .iter()
                        .position(|known| known == kind)
                        .unwrap_or(0);
                    let mark = if marks[slot] { "✓" } else { "…" };
                    container(
                        row![
                            text(kind.label()).size(18).width(Fill).color(couleur),
                            text(mark).size(14).color(if selected {
                                Color::BLACK
                            } else {
                                MUTED
                            }),
                        ]
                        .align_y(Alignment::Center),
                    )
                }
                ArtMenuEntry::Identify => container(
                    row![
                        // la marque est datée : le libellé dit l'état réel, et
                        // Croix refait l'essai immédiatement
                        text(if identity.is_none() && absent.is_some() {
                            "Revérifier sur SteamGridDB"
                        } else {
                            "Identifier le jeu…"
                        })
                        .size(18)
                        .width(Fill)
                        .color(couleur),
                        text(match (identity.is_some(), absent.is_some()) {
                            (true, _) => "✓",
                            (false, true) => "introuvable",
                            _ => "",
                        })
                        .size(14)
                        .color(if selected {
                            Color::BLACK
                        } else if absent.is_some() {
                            DANGER
                        } else {
                            MUTED
                        }),
                    ]
                    .align_y(Alignment::Center),
                ),
                ArtMenuEntry::Alias => container(
                    row![
                        text("Reprendre les visuels d'un jeu…")
                            .size(18)
                            .width(Fill)
                            .color(couleur),
                        text(if game.cover.is_some() { "✓" } else { "" })
                            .size(14)
                            .color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                ),
                ArtMenuEntry::Copier => container(
                    row![
                        text("Copier le jeu…").size(18).width(Fill).color(couleur),
                        text("▸").size(14).color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                ),
                ArtMenuEntry::Date => container(
                    row![
                        text("Date de sortie…").size(18).width(Fill).color(couleur),
                        text(year.map(|year| year.to_string()).unwrap_or_default())
                            .size(14)
                            .color(if selected { Color::BLACK } else { MUTED }),
                    ]
                    .align_y(Alignment::Center),
                ),
            };
            let ligne = ligne
                .width(Fill)
                .padding([9.0, 14.0])
                .style(move |_| action_style(selected));

            rows = rows.push(mouse_area(ligne).on_press(Message::ArtMenuChoose(index)));
        }

        // ce que SteamGridDB croit que ce jeu est : la cause des mauvaises
        // covers, icônes et dates (constaté 20/11 : « Marsupilami 2 » tombait
        // sur « Hoobadventure »)
        let believed = match identity {
            Some(identity) if !identity.name.is_empty() => {
                format!("Identifié : {}", short(&identity.name, 40))
            }
            _ => match absent {
                Some(absent) => format!(
                    "Introuvable sur SteamGridDB · vérifié le {}",
                    covers::date_verifie(absent)
                ),
                None => "Identification : automatique (premier résultat)".to_string(),
            },
        };

        let glyphes: Vec<(&image::Handle, &'static str)> = vec![
            (&self.glyphs.confirm, "Valider"),
            (&self.glyphs.back, "Fermer"),
        ];
        let aide = if identity.is_none() && absent.is_some() {
            "Croix = revérifier maintenant · Rond = fermer · les réglages de lancement sont propres à ce poste"
        } else {
            "Croix = choisir ou basculer · Rond = fermer · les réglages de lancement sont propres à ce poste"
        };

        let panel = container(
            column![
                text(format!("Options de « {} »", short(&game.name, 38)))
                    .size(18)
                    .color(TEXT),
                text(believed).size(12).color(MUTED),
                rows,
                self.aide_menu(&glyphes, aide, false),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(380.0))
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

    /// Menu « reprendre les visuels d'un autre jeu » : liste des jeux qui ont
    /// déjà des images, avec leur cover pour les reconnaître d'un coup d'œil.
    fn alias_menu_view(&self) -> Element<'_, Message> {
        let Some(menu) = self.alias_menu.as_ref() else {
            return Row::new().into();
        };
        let cible = self
            .all_games
            .iter()
            .find(|game| game.slug == menu.slug)
            .map(|game| game.name.clone())
            .unwrap_or_else(|| menu.slug.clone());

        let mut rows = Column::new().spacing(6);
        for (index, (nom, _slug, vignette)) in menu.choix.iter().enumerate() {
            let selected = index == menu.index;
            let apercu: Element<'_, Message> = match vignette {
                Some(handle) => image(handle.clone())
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(56.0))
                    .content_fit(ContentFit::Cover)
                    .into(),
                None => container(Row::new())
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(56.0))
                    .into(),
            };
            let line = container(
                row![
                    apercu,
                    text(short(nom, 58))
                        .size(17)
                        .width(Fill)
                        .color(if selected { Color::BLACK } else { TEXT }),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .width(Fill)
            .padding([7.0, 12.0])
            .style(move |_| action_style(selected));
            rows = rows.push(mouse_area(line).on_press(Message::AliasChoose(index)));
        }

        let help: Element<'_, Message> = if self.pad_hints() {
            row![
                hint(self.glyphs.confirm.clone(), "Reprendre ces visuels"),
                hint(self.glyphs.back.clone(), "Annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                key_hint(&["Entrée"], "reprendre ces visuels"),
                key_hint(&["Échap"], "annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        };

        let panel = container(
            column![
                text(format!("Visuels de quel jeu pour « {} » ?", short(&cible, 34)))
                    .size(18)
                    .color(TEXT),
                text("Le jeu garde son nom et ses options ; seuls ses visuels changent.")
                    .size(13)
                    .color(MUTED),
                scrollable(rows)
                    .style(barre_style(self.barre))
                    .height(Length::Fixed(340.0)),
                help,
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Length::Fixed(660.0))
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
                let thumb: Element<'_, Message> = match picker
                    .thumbs
                    .get(index)
                    .and_then(|vignette| vignette.as_ref())
                {
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
                    .style(move |_| card_style(selected, false));

                current = current.push(mouse_area(cell).on_press(Message::PickerChoose(index)));
                in_row += 1;
                if in_row == PICKER_COLUMNS {
                    rows = rows.push(std::mem::replace(&mut current, Row::new().spacing(12)));
                    in_row = 0;
                }
            }
            if in_row > 0 {
                rows = rows.push(current);
            }

            scrollable(rows)
                .id(Id::new(PICKER_SCROLL))
                .style(barre_style(self.barre))
                .height(Fill)
                .into()
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
        } else if self.mode.is_console() {
            // session console : aucun sélecteur de fichiers (pas de kdialog
            // ni de zenity), donc aucune touche à proposer pour ça
            row![
                key_hint(&["Entrée"], "utiliser cette image"),
                key_hint(&["Échap"], "annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                key_hint(&["Entrée"], "utiliser cette image"),
                key_hint(&["F"], "choisir un fichier"),
                key_hint(&["Échap"], "annuler"),
            ]
            .spacing(18)
            .align_y(Alignment::Center)
            .into()
        };

        // le jeu peut être mal reconnu côté SteamGridDB : on l'identifie (nom
        // recherché, puis choix dans la liste), ce qui vaut pour tous les types.
        // « Choisir un fichier… » prend une image perso à la place (adaptée au
        // ratio du type par le worker) : rien n'oblige à passer par SGDB.
        let mut barre = Row::new()
            .push(
                text(format!("Recherche : « {} »", picker.query))
                    .size(14)
                    .color(MUTED)
                    .width(Fill),
            )
            .push(
                mouse_area(
                    container(text("Identifier le jeu").size(14).color(TEXT))
                        .padding([5.0, 12.0])
                        .style(|_| action_style(false)),
                )
                .on_press(Message::IdentifyOpen),
            )
            .spacing(10)
            .align_y(Alignment::Center);
        // session console : pas de sélecteur de fichiers, donc pas de bouton
        // « Choisir un fichier… » (une image perso se choisit sous Plasma)
        if !self.mode.is_console() {
            barre = barre.push(
                mouse_area(
                    container(text("Choisir un fichier…").size(14).color(TEXT))
                        .padding([5.0, 12.0])
                        .style(|_| action_style(false)),
                )
                .on_press(Message::PickerFile),
            );
        }

        let panel = container(
            column![
                text(format!("{} de « {} »", picker.kind.label(), picker.name))
                    .size(18)
                    .color(TEXT),
                body,
                barre,
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
                // la boîte visible est plus petite que sa case : la marge
                // autour est transparente, donc l'air entre les vignettes
                // augmente sans changer le nombre de colonnes
                _ => container(self.card(game, index))
                    .padding(self.cell_inset())
                    .into(),
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
        let (cell, _, columns) = grid_metrics(self.viewport.0, min_cell, max_columns, self.gap());
        // en mosaïque la cover occupe TOUTE la vignette (aucune marge interne)
        let art = match self.config.view {
            ViewMode::Mosaic => cell,
            _ => cell - 2.0 * (PADDING + CARD_INSET),
        };
        (cell, art, columns)
    }

    /// Marge entre la case et la boîte visible (nulle en mosaïque, les covers
    /// se touchent).
    fn cell_inset(&self) -> f32 {
        match self.config.view {
            ViewMode::Mosaic => 0.0,
            _ => CARD_INSET,
        }
    }

    /// Pas vertical d'une rangée *sans* l'espacement : la boîte visible plus
    /// ses marges. C'est ce pas (et non la hauteur de la boîte) qui règle le
    /// défilement et le repère de la vignette sélectionnée.
    fn slot_height(&self) -> f32 {
        self.row_height() + 2.0 * self.cell_inset()
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
        }
    }

    /// Vignette des lignes de liste, proportionnelle au zoom de la grille.
    fn list_thumb(&self) -> f32 {
        match self.list_size {
            0 => 38.0,
            2 => 78.0,
            _ => 56.0,
        }
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

        // La carte peint TOUT son visuel (cover, tranche, étoile) : c'est elle
        // que la respiration grossit, d'un bloc, via `Pose` — le canvas ne
        // redessine que le cadre d'accent, par-dessus. Une vignette sans cover
        // (sample + icône) ne savait pas être rejouée par le canvas, d'où
        // l'ancienne répartition ; elle n'a plus lieu d'être.
        let art: Element<'_, Message> = match self.cover_handle(game, art_width) {
            Some(handle) => image(handle)
                .width(Length::Fixed(art_width))
                .height(Length::Fixed(art_height))
                .content_fit(ContentFit::Contain)
                // coins cassés : une boîte posée, pas un rectangle brut
                .border_radius(ART_RADIUS)
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
        // Tranche du boîtier : bande sombre au bord gauche de la cover et arête
        // claire, superposées (aucune mise en page refaite). Elle suit les
        // coins arrondis de la cover à gauche, sinon son angle carré dépasserait
        // du visuel. Hauteurs FIXES : un `Fill` dans un `stack` prendrait la
        // place offerte et la rangée dépasserait `row_height`.
        let spine_width = sparkles::spine_width(art_width);
        let spine_edge = container(Row::new())
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(art_height))
            .style(|_| container::Style {
                background: Some(Background::Color(sparkles::SPINE_EDGE)),
                ..container::Style::default()
            });
        let spine = container(spine_edge)
            .width(Length::Fixed(spine_width))
            .height(Length::Fixed(art_height))
            .align_x(Alignment::End)
            .style(|_| container::Style {
                background: Some(Background::Color(sparkles::SPINE_FILL)),
                border: Border {
                    radius: border::Radius {
                        top_left: ART_RADIUS,
                        bottom_left: ART_RADIUS,
                        ..border::Radius::default()
                    },
                    ..Border::default()
                },
                ..container::Style::default()
            });
        let art: Element<'_, Message> = stack![art, spine].into();

        // Titre : la sélection fait défiler son nom s'il ne tient pas (lecture
        // complète, sans élargir la vignette ni animer toute la grille) ; les
        // autres restent tronqués avec une ellipse.
        let title_size = if cell >= 220.0 { 17.0 } else { 15.5 };
        let title: Element<'_, Message> = if selected {
            marquee::Marquee::new(
                text(game.name.as_str())
                    .size(title_size)
                    .color(TEXT)
                    .wrapping(Wrapping::None)
                    .width(Length::Shrink),
            )
            .elapsed(self.title_elapsed)
            .into()
        } else {
            text(short(&game.name, self.title_budget()))
                .size(title_size)
                .color(MUTED)
                .into()
        };
        // Respiration : la vignette grossit, le TITRE NON. iced re-rasterise un
        // texte à chaque changement de taille (`text.size × scale_factor`) :
        // une lettre de 17 px qui suit l'échelle par pas de 1 px se met à
        // scintiller et à sauter alors que la cover glisse en sub-pixel
        // (mesuré 22/11 : lignes du texte qui apparaissent/disparaissent).
        // Le titre est donc posé HORS de toute transformation : seuls le fond
        // de la vignette et son visuel sont grossis, autour du centre du titre
        // (`Pose::pivot`), qui devient le point fixe de l'animation — la
        // vignette monte au-dessus de son nom, le nom ne bouge pas.
        let breath = if selected { self.breath_factor() } else { 1.0 };
        let largeur = cell - 2.0 * self.cell_inset();
        let hauteur = self.row_height();
        let pivot = Vector::new(largeur * 0.5, PADDING + art_height + caption_middle());
        let (zoom_scale, zoom_offset) = self
            .zoom_pose(index, pivot)
            .unwrap_or((1.0, Vector::ZERO));
        let scale = breath * zoom_scale;

        // Le canvas dessine le cadre de la vignette sélectionnée (il doit
        // grossir avec elle) : la carte garde l'épaisseur pour la mise en page
        // mais la peint en transparent.
        let canvas_frame = self.canvas_frame() && selected && !self.returning;
        let visuel = Pose::new(art)
            // même point de l'écran que le fond, exprimé dans SES coordonnées
            .pivot(Point::new(art_width * 0.5, art_height + caption_middle()))
            .scale(scale)
            .offset(zoom_offset);
        let contenu = container(
            column![visuel, title]
                .spacing(TITLE_GAP)
                .align_x(Alignment::Center),
        )
        .width(Length::Fixed(largeur))
        // hauteur IMPOSÉE par le calcul de défilement (`row_height`) : sans
        // ça la hauteur réelle (celle du texte de titre) divergeait de
        // quelques pixels et le cadre du canvas dérivait de rangée en rangée
        .height(Length::Fixed(hauteur))
        .padding(PADDING);
        let fond = Pose::new(
            container(Row::new())
                .width(Length::Fixed(largeur))
                .height(Length::Fixed(hauteur))
                .style(move |_| card_style(selected, canvas_frame)),
        )
        .pivot(Point::new(pivot.x, pivot.y))
        .scale(scale)
        .offset(zoom_offset);

        let card: Element<'_, Message> = mouse_area(stack![fond, contenu])
            .on_press(Message::CardClicked(index))
            .into();
        // Retour de jeu : la vignette revient de son côté de l'écran en
        // dézoomant (`Pose` applique une vraie transformation au rendu — titre
        // compris — sans toucher à la mise en page). Au repos, aucune
        // transformation : le widget se dessine normalement.
        if let Some((scale, dx)) = self.comeback(index) {
            return Pose::new(card)
                .scale(scale)
                .offset(Vector::new(dx, 0.0))
                .into();
        }
        card
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
        let card: Element<'_, Message> = mouse_area(tile)
            .on_press(Message::CardClicked(index))
            .into();
        // changement de taille : la tuile glisse et grandit comme en grille
        // (mosaïque : pas de titre, le pivot reste le centre de la tuile)
        let pivot = Vector::new(cell * 0.5, cell * ART_RATIO * 0.5);
        if let Some((scale, offset)) = self.zoom_pose(index, pivot) {
            return Pose::new(card).scale(scale).offset(offset).into();
        }
        card
    }

    /// Disposition en lignes : vignette à gauche, nom à droite.
    fn list(&self) -> Column<'_, Message> {
        let mut rows = Column::new().spacing(8);
        let hauteur = self.list_thumb();
        // couverture au format portrait des pochettes (205×308)
        let largeur = hauteur * 205.0 / 308.0;

        for (index, game) in self.games.iter().enumerate() {
            let selected = index == self.selected;
            let titre = if selected { TEXT } else { Color::from_rgb(0.80, 0.83, 0.89) };

            // art : la pochette d'abord (c'est elle qui identifie un jeu),
            // l'icône du paquet ensuite, une vignette générée en dernier
            let art: Element<'_, Message> = match self.cover_handle(game, largeur) {
                Some(handle) => image(handle)
                    .width(Length::Fixed(largeur))
                    .height(Length::Fixed(hauteur))
                    .content_fit(ContentFit::Cover)
                    .into(),
                None => sample_art(&self.sample, &game.name, game.icon.as_ref(), largeur, hauteur),
            };

            // méta à DROITE, sur une colonne de largeur fixe : les années et
            // les temps de jeu s'alignent d'une ligne à l'autre au lieu de
            // flotter après le titre
            let mut meta: Vec<String> = Vec::new();
            if let Some(year) = self.year_of(&game.slug) {
                meta.push(year.to_string());
            }
            let secondes = self.playtime.get(&game.slug).copied().unwrap_or(0);
            if secondes >= 60 {
                meta.push(playtime::format(Some(secondes)));
            }
            let taille_titre = match self.list_size {
                0 => 15.0,
                2 => 21.0,
                _ => 18.0,
            };
            let titre_texte = text(short(&game.name, 72))
                .size(taille_titre)
                .color(titre)
                .width(Length::Fill);
            let meta_texte: Element<'_, Message> = if meta.is_empty() {
                Row::new().into()
            } else {
                text(meta.join("  ·  "))
                    .size(13)
                    .color(MUTED)
                    .width(Length::Fixed(150.0))
                    .align_x(Alignment::End)
                    .into()
            };

            let star: Element<'_, Message> = if self.favorites.contains(&game.slug) {
                self.star_icon(16.0, 1.0)
            } else {
                Row::new().into()
            };

            // filet d'accent à gauche de la ligne sélectionnée : le fond teinté
            // seul se perdait dans une longue liste (retour 18/11)
            let filet = container(Row::new())
                .width(Length::Fixed(3.0))
                .height(Length::Fixed(hauteur.max(40.0)))
                .style(move |_| container::Style {
                    background: Some(Background::Color(if selected {
                        ACCENT
                    } else {
                        Color::TRANSPARENT
                    })),
                    ..container::Style::default()
                });

            let body = row![filet, art, titre_texte, meta_texte, star,]
            .spacing(14)
            .align_y(Alignment::Center)
            .padding(Padding {
                top: 8.0,
                right: 16.0,
                bottom: 8.0,
                left: 9.0,
            });

            let line = container(body)
                .width(Fill)
                .style(move |_| list_style(selected));

            rows = rows.push(mouse_area(line).on_press(Message::CardClicked(index)));
        }

        rows
    }

    /// Panneau de droite de la vue liste : la pochette du jeu sélectionné,
    /// BORNÉE en hauteur, puis son nom et son état.
    ///
    /// La cover remplissait toute la place (`Fill`) : à 1440p elle devenait
    /// énorme et écrasait la liste (signalé le 23/11/2026). Elle est plafonnée
    /// et centrée ; le chemin du paquet, qui ne servait qu'au développement,
    /// a disparu.
    fn preview(&self) -> Element<'_, Message> {
        let Some(game) = self.games.get(self.selected) else {
            return Row::new().into();
        };

        // bannière héros en haut du panneau quand elle existe : elle occupe
        // toute la largeur de la carte et la remplit, au lieu d'une pochette
        // perdue au milieu du vide. Repli : la pochette, plafonnée.
        let picture: Element<'_, Message> = match &game.hero {
            Some(handle) => image(handle.clone())
                .width(Fill)
                .height(Length::Fixed((self.viewport.1 * 0.28).clamp(140.0, 300.0)))
                .content_fit(ContentFit::Cover)
                .into(),
            None => {
                let hauteur = (self.viewport.1 * 0.46).clamp(180.0, 460.0);
                let largeur = hauteur * 205.0 / 308.0;
                match game.cover.clone().or_else(|| game.icon.clone()) {
                    Some(handle) => image(handle)
                        .width(Length::Fixed(largeur))
                        .height(Length::Fixed(hauteur))
                        .content_fit(ContentFit::Contain)
                        .into(),
                    None => sample_art(
                        &self.sample,
                        &game.name,
                        game.icon.as_ref(),
                        largeur,
                        hauteur,
                    ),
                }
            }
        };

        let mut meta: Vec<String> = Vec::new();
        if let Some(year) = self.year_of(&game.slug) {
            meta.push(format!("Sorti en {year}"));
        }
        let joue = playtime::format(self.playtime.get(&game.slug).copied());
        meta.push(format!("Temps de jeu : {joue}"));
        let favori = if self.favorites.contains(&game.slug) {
            "  ·  ★"
        } else {
            ""
        };

        let caption = column![
            text(short(&game.name, 46)).size(21).color(TEXT),
            text(format!("{}{favori}", meta.join("  ·  ")))
                .size(13)
                .color(MUTED),
        ]
        .spacing(5)
        .align_x(Alignment::Center);

        // la carte prend SA hauteur (bannière + textes) et se centre dans la
        // colonne : une carte pleine hauteur laissait un grand vide sous le
        // contenu (signalé 23/11/2026)
        let carte = container(
            column![picture, caption]
                .spacing(16)
                .align_x(Alignment::Center),
        )
        .width(Fill)
        .padding(18)
        .style(|_| container::Style {
            background: Some(Background::Color(CELL_BG)),
            border: Border {
                color: CELL_BORDER,
                width: 1.0,
                radius: 18.0.into(),
            },
            ..container::Style::default()
        });

        container(carte)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    }

    /// Vue « favoris » sans aucun favori : on explique comment en poser un.
    /// Aucun jeu pour l'année filtrée : le dire, sinon l'écran « aucun jeu »
    /// laisse croire que la bibliothèque est vide.
    fn year_empty(&self, year: u32) -> Element<'_, Message> {
        container(
            column![
                text(format!("Aucun jeu sorti en {year}"))
                    .size(22)
                    .color(TEXT),
                text("Le menu Tri (T) → Année remet toutes les années.")
                    .size(14)
                    .color(MUTED),
                text("L'année d'un jeu s'apprend en ouvrant sa page.")
                    .size(14)
                    .color(MUTED),
            ]
            .spacing(8),
        )
        .width(Fill)
        .height(Fill)
        .padding(24)
        .center_x(Fill)
        .center_y(Fill)
        .into()
    }

    fn recent_empty(&self) -> Element<'_, Message> {
        container(
            column![
                text("Aucun jeu lancé").size(22).color(TEXT),
                text("L'ordre « Derniers lancés » se remplit au premier jeu ouvert.")
                    .size(14)
                    .color(MUTED),
                text("Tri (menu T ou Carré/X) pour revenir à A→Z.")
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

    fn favorites_empty(&self) -> Element<'_, Message> {
        container(
            column![
                text("Aucun favori").size(22).color(TEXT),
                text("Ouvre un jeu (Entrée ou A), puis appuie sur ☆ Favori.")
                    .size(14)
                    .color(MUTED),
                text("Le tri Favoris (menu T, ou Carré/X) ne montre qu'eux.")
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
        // En session console, aucune aide ne propose de quitter : le launcher
        // n'a pas de sortie propre (c'est le menu système de gpad qui la
        // porte). Afficher « Quitter » y serait un mensonge.
        let console = self.mode.is_console();
        let hints: Element<'_, Message> = match (self.pad_hints(), console) {
            (true, false) => row![
                hint(self.glyphs.confirm.clone(), "Ouvrir"),
                hint(self.glyphs.back.clone(), "Quitter (2×)"),
                hint(self.glyphs.view.clone(), self.next_view_label()),
                hint(self.glyphs.sort.clone(), "Tri"),
                hint(self.glyphs.zoom_out.clone(), "Taille −"),
                hint(self.glyphs.zoom_in.clone(), "Taille +"),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
            // session console : la sortie n'existe pas (menu système de gpad),
            // donc aucune aide ne la propose
            (true, true) => row![
                hint(self.glyphs.confirm.clone(), "Ouvrir"),
                hint(self.glyphs.view.clone(), self.next_view_label()),
                hint(self.glyphs.sort.clone(), "Tri"),
                hint(self.glyphs.zoom_out.clone(), "Taille −"),
                hint(self.glyphs.zoom_in.clone(), "Taille +"),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
            (false, false) => row![
                key_hint(&["←↑→↓"], "naviguer"),
                key_hint(&["Entrée"], "ouvrir"),
                key_hint(&["/"], "rechercher"),
                key_hint(&["V"], "vue"),
                key_hint(&["−", "+"], "taille"),
                key_hint(&["T"], "tri"),
                key_hint(&["F"], "plein écran"),
                key_hint(&["Échap"], "quitter"),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
            (false, true) => row![
                key_hint(&["←↑→↓"], "naviguer"),
                key_hint(&["Entrée"], "ouvrir"),
                key_hint(&["/"], "rechercher"),
                key_hint(&["V"], "vue"),
                key_hint(&["−", "+"], "taille"),
                key_hint(&["T"], "tri"),
                key_hint(&["F"], "plein écran"),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
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
        } else if let Some(notice) = &self.notice {
            text(notice.clone()).size(14).color(ACCENT).into()
        } else {
            Row::new().into()
        };

        // L'ordre de tri reste visible sans ouvrir de menu, et il doit se voir :
        // une pastille discrète plutôt qu'une ligne grise noyée dans le pied de
        // page (demandé le 20/11). Pas de compte de jeux ni de progression des
        // covers : le pied de page ne doit porter aucune statistique.
        // le tri ET le filtre d'année se lisent dans la même pastille
        let mut tri = match self.year_filter {
            Some(year) => format!("{} · {year}", self.sort.label()),
            None => self.sort.label().to_string(),
        };
        // portée : « A→Z · disques » — la pastille suffit, aucun message
        if self.scope != Scope::Tous {
            tri.push_str(" · ");
            tri.push_str(self.scope.short());
        }
        let sort: Element<'_, Message> = container(
            row![
                text("TRI").size(11).font(HELP_FONT).color(MUTED),
                text(tri).size(15).font(HELP_FONT).color(TEXT),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 5.0,
            right: 12.0,
            bottom: 5.0,
            left: 12.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(CELL_BG)),
            border: Border {
                color: ACCENT,
                width: 1.0,
                radius: 11.0.into(),
            },
            ..container::Style::default()
        })
        .into();

        // Total de la bibliothèque, tout à droite : une pastille discrète, la
        // seule statistique du pied de page (demandée le 20/11). En favoris,
        // c'est le nombre de favoris — c'est la liste affichée. Le nom du jeu
        // n'est plus répété ici (la vignette sélectionnée le porte, et il
        // encombrait le pied de page — demandé le 20/11).
        let total: Element<'_, Message> = container(
            text(match self.sort {
                SortBy::Favorites if self.games.len() > 1 => {
                    format!("{} favoris", self.games.len())
                }
                SortBy::Favorites => format!("{} favori", self.games.len()),
                _ => plural(self.games.len()),
            })
            .size(13)
            .font(HELP_FONT)
            .color(MUTED),
        )
        .padding(Padding {
            top: 4.0,
            right: 11.0,
            bottom: 4.0,
            left: 11.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(CELL_BG)),
            border: Border {
                color: CELL_BORDER,
                width: 1.0,
                radius: 9.0.into(),
            },
            ..container::Style::default()
        })
        .into();

        // Aide des touches au CENTRE, tri à gauche, message et total à droite :
        // deux couches, parce qu'une `Row` ne sait pas centrer dans la fenêtre
        // quand les blocs des côtés n'ont pas la même largeur. Un `Stack` prend
        // sa hauteur sur son PREMIER enfant : c'est la pastille de tri (plus
        // haute que le texte d'aide au clavier) qui donne la hauteur, sinon la
        // pastille dépassait sous sa bordure (constaté 20/11). Les deux couches
        // sont centrées verticalement.
        let hints_centered = container(hints)
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .center_y(Fill);
        // gauche : tri puis compte (le compte demande le 24/11 juste après le
        // tri, les deux se lisent d'un coup) ; puis le message d'état, et la
        // place libre à droite pour les icônes d'état et les roues
        let sides = container(
            row![sort, total, status, Row::new().width(Fill)]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .width(Fill);

        container(column![self.update_bar(), stack![sides, hints_centered]].spacing(6))
            .padding(Padding {
                top: 14.0,
                right: 4.0,
                bottom: 0.0,
                left: 4.0,
            })
            .into()
    }

    /// Barre de progression de la mise à jour, au-dessus de l'aide, avec son
    /// libellé **à côté d'elle** (et non dans le message d'état : un libellé
    /// long y passait sous l'aide centrée — constaté au banc le 22/11).
    ///
    /// Hauteur **fixe même au repos** : le pied de page ne doit pas sauter
    /// quand elle apparaît, et le remplissage ne change qu'un `FillPortion`,
    /// sur message, jamais sur une horloge (aucune mise en page par image).
    fn update_bar(&self) -> Element<'_, Message> {
        let (percent, color) = match &self.update {
            // échec : barre pleine en rouge, le libellé dit pourquoi (le code
            // de sortie seul n'aide personne)
            Some(update) if update.error.is_some() => (100.0, DANGER),
            Some(update) => (update.percent.unwrap_or(35.0), ACCENT),
            None => (0.0, Color::TRANSPARENT),
        };
        let filled = (percent.clamp(0.0, 100.0) * 100.0).round() as u16;
        let (left_color, right_color) = if filled == 0 {
            (Color::TRANSPARENT, Color::TRANSPARENT)
        } else {
            (color, CELL_BG)
        };
        let segment = |portion: u16, tint: Color| {
            container(Row::new())
                .width(Length::FillPortion(portion.max(1)))
                .height(Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(tint)),
                    border: Border {
                        radius: 2.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                })
        };
        // piste + remplissage : deux portions d'une même rangée, donc le
        // pourcentage est exact quelle que soit la largeur de la fenêtre
        let track = row![
            segment(filled, left_color),
            segment(100u16.saturating_sub(filled), right_color),
        ];

        let (label, label_color) = match &self.update {
            Some(update) if update.error.is_some() => (update.label.clone(), DANGER),
            Some(update) if update.done => (update.label.clone(), ACCENT),
            Some(update) => (update.label.clone(), MUTED),
            None => (String::new(), MUTED),
        };

        container(
            row![
                container(track).width(Fill).height(Length::Fixed(3.0)),
                text(label).size(12).font(HELP_FONT).color(label_color),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .height(Length::Fixed(16.0))
        .into()
    }
}

// --- petits helpers ---------------------------------------------------------

/// Défilement réellement dessiné : iced borne l'offset demandé au maximum du
/// contenu (`content - viewport`), donc viser au-delà ne déplace rien de plus.
/// La cible de l'animation (`scroll`) et le centrage de la dernière rangée
/// dépassent ce maximum : placer les vignettes ou le cadre d'accent depuis elle
/// les décalait d'une demi-rangée (constaté 20/11 : au retour d'un jeu, le cadre
/// de sélection tombait entre deux jeux).
fn scroll_drawn(scroll: f32, max: f32) -> f32 {
    if max > 0.0 { scroll.min(max) } else { scroll }
}

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

/// Avance l'apparition d'un visuel de la page (héros, logo, cover) quand il est
/// enfin là. Absent : la progression ne bouge pas (elle vaut 0 et repartira de
/// là quand l'image arrivera).
fn advance_asset(progress: &mut f32, present: bool, dt: f32) {
    if present && *progress < 1.0 {
        *progress = (*progress + dt / PAGE_ASSET_SECONDS).min(1.0);
    }
}

/// Décélération de l'apparition : vive au début, elle se pose à la fin (une
/// rampe linéaire donne l'impression que l'image « claque »).
fn ease_reveal(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(2)
}

/// Largeur minimale de vignette qui produit **exactement** `columns` colonnes
/// dans une grille étalée sur toute la largeur : c'est l'inverse de
/// `grid_metrics`, nécessaire au zoom arrière (`cell_size` ne peut pas être
/// laissé sur la largeur affichée, sinon le nombre de colonnes plafonne).
fn cell_for_columns(window_width: f32, columns: usize, gap: f32) -> u32 {
    let columns = columns.max(1) as f32;
    let usable = usable_width(window_width).max(CELL_MIN);
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

/// Échelle et décalage d'une vignette pendant la glissade d'un changement de
/// taille : `e` va de 0 (elle est encore à sa case d'avant) à 1 (elle a rejoint
/// la nouvelle). L'échelle part du rapport des **covers** (c'est l'image qu'on
/// regarde) et le décalage est calculé sur le centre de la cover, pas sur le
/// pivot : à `e = 0`, la cover dessinée avec cette pose occupe EXACTEMENT son
/// ancien rectangle (la case suit à quelques pixels près).
///
/// `pivot` (écran) est le point fixe des `Pose` de la carte. Passer le centre de
/// la case pour un pivot au centre de la case reproduit le calcul d'origine.
fn zoom_transform(
    old_art: &Rectangle,
    _card: &Rectangle,
    art: &Rectangle,
    pivot: Point,
    e: f32,
) -> (f32, Vector) {
    let start = old_art.width / art.width;
    let scale = start + (1.0 - start) * e;
    // la cover n'est pas au pivot (marge interne, ligne de titre) : son centre
    // est décalé de `shift`, que la mise à l'échelle déplace aussi
    let shift = art.center() - pivot;
    let offset = (old_art.center() - pivot - shift * scale) * (1.0 - e);
    (scale, offset)
}

fn grid_metrics(
    window_width: f32,
    min_cell: f32,
    max_columns: usize,
    gap: f32,
) -> (f32, f32, usize) {
    let min_cell = min_cell.max(80.0);
    let max_columns = max_columns.max(1) as f32;
    let usable = usable_width(window_width).max(min_cell);

    // combien de vignettes tiennent à la largeur minimale…
    let fitted = ((usable + gap) / (min_cell + gap)).floor().max(1.0);
    // … sans dépasser `columns`, puis on étale pour remplir la largeur
    let columns = fitted.min(max_columns).max(1.0);
    let cell = ((usable - (columns - 1.0) * gap) / columns)
        .floor()
        .max(min_cell);
    (cell, cell - 2.0 * PADDING, columns as usize)
}

/// Largeur utile de la grille : la fenêtre moins les deux marges (celle de
/// droite réserve la place de la barre de défilement).
fn usable_width(window_width: f32) -> f32 {
    (window_width - PAGE_PADDING_X - PAGE_PADDING_X_RIGHT).max(1.0)
}

/// Interrupteur façon overlay (`gpad`) : pilule arrondie, bouton à droite
/// quand c'est activé. Le launcher et l'overlay doivent parler la même langue
/// visuelle — c'est un écosystème, pas deux applications.
///
/// `sur_ligne` : la ligne porte le fond d'accent (sélection) ; la pilule
/// s'inverse alors (piste sombre, bouton clair) pour rester lisible.
fn interrupteur(actif: bool, sur_ligne: bool) -> Element<'static, Message> {
    let piste = if actif {
        if sur_ligne {
            Color { a: 0.55, ..Color::BLACK }
        } else {
            ACCENT
        }
    } else if sur_ligne {
        Color { a: 0.30, ..Color::BLACK }
    } else {
        Color { a: 0.22, ..TEXT }
    };
    let bouton = if actif {
        if sur_ligne { Color::WHITE } else { Color::WHITE }
    } else {
        MUTED
    };
    let bouton = container(Row::new())
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(bouton)),
            border: Border {
                radius: 9.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    container(
        container(bouton)
            .width(Fill)
            .height(Fill)
            .padding(5)
            .align_x(if actif {
                Alignment::End
            } else {
                Alignment::Start
            }),
    )
    .width(Length::Fixed(50.0))
    .height(Length::Fixed(28.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(piste)),
        border: Border {
            radius: 14.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// Barre de réglage (0..=100) : piste arrondie, remplissage d'accent — le
/// même dessin que les curseurs de l'overlay, sans interaction à la souris
/// (tout se règle au pad ou au clavier).
fn barre(pourcent: u8, largeur: f32, sur_ligne: bool) -> Element<'static, Message> {
    let part = (pourcent.min(100) as f32) / 100.0;
    let plein = if sur_ligne { Color::BLACK } else { ACCENT };
    let vide = if sur_ligne {
        Color { a: 0.30, ..Color::BLACK }
    } else {
        Color { a: 0.16, ..TEXT }
    };
    let avant = container(Row::new())
        .width(Length::Fixed((largeur * part).max(4.0)))
        .height(Length::Fixed(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(plein)),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    let apres = container(Row::new())
        .width(Length::Fixed(largeur * (1.0 - part)))
        .height(Length::Fixed(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(vide)),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    row![avant, apres].spacing(0).into()
}

/// Géométrie de la galerie pour une largeur d'écran : `(colonnes, largeur de
/// tuile, hauteur, pas d'une rangée)`.
///
/// La grille doit **remplir** la largeur : sur un écran 2K elle s'arrêtait
/// « à la zone 1080 » (6 colonnes de 240 px figées, ~1500 px au milieu de
/// 2560). Le nombre de colonnes suit donc la largeur, pour une tuile visée de
/// ~300 px, et la tuile est étalée sur la place restante.
///
/// `pas` est le pas RÉEL d'une rangée : vignette + 6 px d'interligne avec sa
/// légende (trois lignes) + 12 px de marge de tuile + 10 px entre deux
/// rangées. Il valait `tuile_h + 86` (5 px de trop), ce qui faisait perdre
/// une rangée entière en 2K : la galerie s'arrêtait à 4 rangées alors que la
/// 5e tenait (constaté le 23/11).
fn galerie_geometrie(largeur: f32) -> (usize, f32, f32, f32) {
    const GOUTTIERE: f32 = 12.0;
    let dispo = (largeur - 48.0).max(240.0);
    // taille FIXE : une tuile visée de 300 px, sans cran de zoom (le zoom de
    // la galerie a été retiré le 23/11, jugé inutile sur une grille plein écran)
    let visee = 300.0;
    let colonnes = ((dispo / visee).floor() as usize).clamp(2, 10);
    let tuile_w = ((dispo - GOUTTIERE * (colonnes as f32 - 1.0)) / colonnes as f32)
        .floor()
        .max(120.0);
    let tuile_h = (tuile_w * 9.0 / 16.0).floor().max(68.0);
    (colonnes, tuile_w, tuile_h, tuile_h + 77.0)
}

/// Hauteur prise au-dessus et au-dessous de la grille de la galerie : padding
/// (2 × 20), titre (28), sous-titre (13), compteur (12), aide (13) et leurs
/// quatre interlignes de 12. Mesuré sur une capture 2560×1440 le 23/11 : 173.
const GALERIE_CHROME: f32 = 173.0;
/// Durée du fondu d'apparition des menus et de la galerie (secondes).
const FONDU_MENUS: f32 = 0.18;

/// Pastille de touche, l'équivalent clavier de [`hint`] : un capuchon arrondi
/// portant le nom de la touche, puis l'action. Les aides au clavier étaient de
/// longues phrases en texte nu, à côté des glyphes de manette — même langage
/// visuel pour les deux périphériques (demandé le 23/11).
fn key_hint(keys: &'static [&'static str], label: &'static str) -> Element<'static, Message> {
    let mut caps = Row::new().spacing(4).align_y(Alignment::Center);
    for key in keys {
        caps = caps.push(
            container(text(*key).size(12).font(HELP_FONT).color(TEXT))
                .padding(Padding {
                    top: 2.0,
                    right: 7.0,
                    bottom: 3.0,
                    left: 7.0,
                })
                .style(|_| container::Style {
                    background: Some(Background::Color(CELL_BG)),
                    border: Border {
                        color: CELL_BORDER,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..container::Style::default()
                }),
        );
    }

    row![
        caps,
        text(majuscule(label)).size(15).font(HELP_FONT).color(MUTED)
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// « naviguer » → « Naviguer » : l'aide au clavier prend la même casse que les
/// glyphes de manette (« Ouvrir », « Tri »), sinon les deux aides se lisent
/// comme deux applications (signalé le 24/11).
fn majuscule(texte: &str) -> String {
    let mut chars = texte.chars();
    match chars.next() {
        Some(premier) => premier.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn hint(glyph: image::Handle, label: &'static str) -> Element<'static, Message> {
    row![
        image(glyph)
            .width(Length::Fixed(26.0))
            .height(Length::Fixed(26.0)),
        text(label).size(15).font(HELP_FONT).color(MUTED),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
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
            .color(Color {
                a: 0.55,
                ..Color::WHITE
            })
            .into(),
    };

    let size = (width / 13.0).clamp(11.0, 22.0);
    // largeur approximative d'un caractère pour cette taille de police
    let max = ((width / (size * 0.58)) as usize).max(6);
    let label = text(short(name, max))
        .size(size)
        .color(Color {
            a: 0.85,
            ..Color::WHITE
        })
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

/// Vignette de grille : fond teinté, ombre portée (la cover se décolle du fond
/// comme un boîtier posé) et cadre d'accent. Quand `canvas_frame` est vrai,
/// c'est le canvas des particules qui peint le cadre (il doit grossir avec la
/// vignette) : la carte garde l'épaisseur pour la mise en page, en transparent.
fn card_style(selected: bool, canvas_frame: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if selected {
            CELL_BG_SELECTED
        } else {
            Color::TRANSPARENT
        })),
        // le fond teinté seul ne suffisait pas : sur 666 vignettes on perdait
        // de vue laquelle était sélectionnée (retour 18/11)
        border: Border {
            color: if selected && !canvas_frame {
                ACCENT
            } else {
                Color::TRANSPARENT
            },
            width: if selected { FOCUS_BORDER } else { 0.0 },
            radius: CARD_RADIUS.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, if selected { 0.55 } else { 0.38 }),
            offset: Vector::new(0.0, if selected { 8.0 } else { 6.0 }),
            blur_radius: if selected { 20.0 } else { 14.0 },
        },
        ..container::Style::default()
    }
}

/// Champ de saisie des prompts (identification, date) : mêmes couleurs que le
/// panneau, bordure d'accent quand il a le focus.
fn input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
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
/// Style de la barre de défilement : celle d'iced quand la souris ou le
/// clavier pilote, parfaitement invisible quand c'est la manette (rail et
/// curseur transparents). Les marges sont conservées : la faire disparaître ne
/// doit pas décaler le contenu.
fn barre_style(
    visible: bool,
) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut style = scrollable::default(theme, status);
        if !visible {
            let invisible = scrollable::Rail {
                background: None,
                border: Border::default(),
                scroller: scrollable::Scroller {
                    background: Color::TRANSPARENT.into(),
                    border: Border::default(),
                },
            };
            style.vertical_rail = invisible;
            style.horizontal_rail = invisible;
        }
        style
    }
}

fn list_style(selected: bool) -> container::Style {
    // Toutes les lignes ont un fond (très discret) et un liseré : sans ça la
    // liste flottait dans le vide noir, et la sélection ne se voyait que par
    // la teinte. La ligne choisie prend le fond clair + liseré d'accent.
    container::Style {
        background: Some(Background::Color(if selected {
            CELL_BG_SELECTED
        } else {
            Color::from_rgba(0.07, 0.08, 0.11, 0.55)
        })),
        border: Border {
            color: if selected {
                ACCENT
            } else {
                Color::from_rgba(0.14, 0.16, 0.21, 0.85)
            },
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

/// Texte de comparaison de la recherche : minuscules, sans accents ni
/// ligatures. « Épée » et « epee » doivent trouver le même jeu, et un titre
/// français tapé sans accent ne doit pas demander de le taper avec.
fn normalise(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    for c in texte.to_lowercase().chars() {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => sortie.push('a'),
            'ç' => sortie.push('c'),
            'è' | 'é' | 'ê' | 'ë' => sortie.push('e'),
            'ì' | 'í' | 'î' | 'ï' => sortie.push('i'),
            'ñ' => sortie.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => sortie.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => sortie.push('u'),
            'ý' | 'ÿ' => sortie.push('y'),
            'œ' => sortie.push_str("oe"),
            'æ' => sortie.push_str("ae"),
            _ => sortie.push(c),
        }
    }
    sortie
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

#[cfg(test)]
mod recherche_tests {
    use super::normalise;

    #[test]
    fn recherche_insensible_aux_accents_et_la_casse() {
        assert_eq!(normalise("Épée de Légende"), "epee de legende");
        assert!(normalise("The Legend of Zelda").contains(&normalise("ZELDA")));
        // ligatures : « cœur » se tape « coeur »
        assert!(normalise("Cœur").contains(&normalise("coeur")));
        assert!(normalise("Æther").contains(&normalise("aether")));
    }
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
    fn le_dernier_ecran_ne_decale_pas_le_cadre() {
        // le centrage de la dernière rangée vise au-delà du maximum du
        // contenu : iced dessine le maximum, donc le placement doit suivre le
        // maximum, sinon cadre et vignettes se décalent (constaté 20/11 :
        // « au retour d'un jeu la sélection est entre deux jeux »)
        assert_eq!(scroll_drawn(5_686.0, 5_334.0), 5_334.0);
        // au milieu de la liste, rien n'est borné
        assert_eq!(scroll_drawn(1_200.0, 5_334.0), 1_200.0);
        // maximum encore inconnu (première image) : on garde la position
        assert_eq!(scroll_drawn(120.0, 0.0), 120.0);
    }

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
        assert!(
            rendered <= 6 && rendered >= 5,
            "6 demandées, {rendered} rendues"
        );
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
    fn la_glissade_de_taille_part_de_l_ancienne_case() {
        // 6 → 5 colonnes sur 1920 : la cover passe de 218 à 250 px de large
        let old_art = Rectangle::new(Point::new(100.0, 60.0), Size::new(218.0, 327.0));
        let card = Rectangle::new(Point::new(400.0, 300.0), Size::new(292.0, 480.0));
        let art = Rectangle::new(Point::new(412.0, 312.0), Size::new(250.0, 375.0));
        // pivot de la respiration : centre du titre, sous la cover
        let pivot = Point::new(card.center().x, art.y + art.height + 18.0);

        // e = 0 : la cover est encore exactement à son ancienne place et taille
        let (scale, offset) = zoom_transform(&old_art, &card, &art, pivot, 0.0);
        assert!((scale - 218.0 / 250.0).abs() < 1e-4, "échelle de départ {scale}");
        let centre = pivot + offset + (art.center() - pivot) * scale;
        assert!((centre.x - old_art.center().x).abs() < 0.01, "x {centre:?}");
        assert!((centre.y - old_art.center().y).abs() < 0.01, "y {centre:?}");
        assert!((art.width * scale - old_art.width).abs() < 0.01);
        assert!((art.height * scale - old_art.height).abs() < 0.01);

        // e = 1 : aucune transformation, la cover est à sa nouvelle place
        let (scale, offset) = zoom_transform(&old_art, &card, &art, pivot, 1.0);
        assert!((scale - 1.0).abs() < 1e-6);
        assert_eq!(offset, Vector::ZERO);

        // entre les deux : l'écart se referme, sans dépassement
        let mut precedent = zoom_transform(&old_art, &card, &art, pivot, 0.0);
        for step in 1..=40 {
            let e = step as f32 / 40.0;
            let (scale, offset) = zoom_transform(&old_art, &card, &art, pivot, e);
            assert!(scale >= 218.0 / 250.0 - 1e-4 && scale <= 1.0 + 1e-4, "{e} : {scale}");
            assert!(offset.x.abs() <= precedent.1.x.abs() + 1e-3, "{e} : {offset:?}");
            assert!(offset.y.abs() <= precedent.1.y.abs() + 1e-3, "{e} : {offset:?}");
            precedent = (scale, offset);
        }
    }

    #[test]
    fn galerie_remplit_la_largeur() {
        // 1080p : 6 colonnes qui occupent toute la largeur utile
        let (colonnes, w, h, pas) = galerie_geometrie(1920.0);
        assert_eq!(colonnes, 6);
        let occupe = w * colonnes as f32 + 12.0 * (colonnes as f32 - 1.0);
        assert!((occupe - (1920.0 - 48.0)).abs() <= 3.0, "occupe {occupe}");
        assert!(h < w && pas > h, "16:9 et la légende sous la vignette");
        // 2K : plus de colonnes, et la grille occupe encore toute la largeur
        let (colonnes, w, _, _) = galerie_geometrie(2560.0);
        assert_eq!(colonnes, 8);
        let occupe = w * colonnes as f32 + 12.0 * (colonnes as f32 - 1.0);
        // arrondi vers le bas de la largeur de tuile : quelques pixels de
        // reste, jamais une bande vide
        assert!((occupe - (2560.0 - 48.0)).abs() <= 16.0, "occupe {occupe}");
        // petite fenêtre : deux colonnes au moins, jamais zéro
        assert_eq!(galerie_geometrie(400.0).0, 2);
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
        assert!(filled <= usable_width(2560.0));
        assert!(filled >= usable_width(2560.0) - 5.0, "aucun vide à droite");

        // fenêtre étroite : moins de colonnes, jamais de débordement
        let (_, _, columns) = grid_metrics(700.0, 220.0, 5, GAP);
        assert_eq!(columns, 2);
        let (cell, _, _) = grid_metrics(700.0, 220.0, 5, GAP);
        assert!(cell * 2.0 + GAP <= usable_width(700.0));
    }

    #[test]
    fn mosaique_sans_espace() {
        // espacement nul : les colonnes collées remplissent exactement la largeur
        let (cell, _, columns) = grid_metrics(1280.0, 220.0, 8, 0.0);
        assert_eq!(columns, 5);
        assert!((cell * columns as f32 - usable_width(1280.0)).abs() <= 5.0);
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

    /// Fenêtre glissante des listes de réglages : tout tient → tout est
    /// dessiné ; sinon le curseur reste visible, au centre quand il peut.
    #[test]
    fn fenetre_glissante() {
        assert_eq!(fenetre(0, 3, 10), (0, 3));
        assert_eq!(fenetre(2, 10, 5), (0, 5), "début de liste");
        assert_eq!(fenetre(5, 10, 5), (3, 8), "curseur au centre");
        assert_eq!(fenetre(9, 10, 5), (5, 10), "fin de liste");
        // garde-fous : jamais de fenêtre vide ni hors bornes
        assert_eq!(fenetre(0, 0, 5), (0, 0));
        assert_eq!(fenetre(4, 10, 0), (4, 5));
    }
}
