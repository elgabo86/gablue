//! Poussière d'étoiles posée sur l'interface, façon fond de PS3 : quelques
//! dizaines de particules qui dérivent lentement et scintillent, plus la
//! vignette sélectionnée qui respire — **cadre compris**, comme un boîtier
//! qu'on approche.
//!
//! **Coût** — c'est la contrainte, donc tout est fait pour qu'il soit nul au
//! repos et faible en marche :
//! - un seul `canvas` transparent, redessiné **20 fois par seconde** (50 ms) ;
//! - les particules sont des **fonctions pures du temps** : aucune position
//!   n'est stockée ni mise à jour, chaque image recalcule x/y/alpha à partir de
//!   la graine de la particule et de `time` (quelques dizaines de µs) ;
//! - le minuteur (`RUNNING`) est **coupé** dès que la scène ne peut pas être
//!   vue : fenêtre sans focus, page de jeu ouverte, menu, voile de lancement.
//!   Aucune image n'est alors demandée au moteur, donc zéro dépense ;
//! - le nombre de particules suit la surface (≈ 118 en 1920×1080, plafond 130)
//!   et les tailles suivent la hauteur d'écran ;
//! - `GLAUNCHER_FX_DEBUG=1` affiche dans le journal le coût moyen d'une image.
//!
//! Le sprite (`assets/sparkle.png`, 128×128, généré : halo + cœur + quatre
//! branches) est embarqué et teinté uniquement par son opacité — pas de
//! dégradé à tesseller, pas de flou à calculer.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use iced::widget::canvas::{Canvas, Frame, Geometry, Image, Path, Program, Stroke};
use iced::widget::image::Handle;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use crate::app::{FOOTER_BAND, Message, PAGE_PADDING_Y};

const SPRITE: &[u8] = include_bytes!("../assets/sparkle.png");

/// Intervalle du minuteur : 50 ms, soit 20 images par seconde. Assez pour une
/// dérive lente et un scintillement doux, trois fois moins cher que 60 Hz —
/// car **chaque message provoque une reconstruction et une image complètes**
/// dans iced (il n'y a pas de redessin partiel). `GLAUNCHER_FX_MS` change
/// l'intervalle sans recompiler (16 à 200 ms) pour juger du coût sur place.
pub const FRAME_MS: u64 = 50;

/// Intervalle réellement utilisé (variable d'environnement ou valeur par défaut).
fn interval_ms() -> u64 {
    static VALUE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("GLAUNCHER_FX_MS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(|ms| ms.clamp(16, 200))
            .unwrap_or(FRAME_MS)
    })
}

/// Le minuteur doit-il envoyer des images ? Piloté par `App::subscription`.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Mesure du coût de dessin (`GLAUNCHER_FX_DEBUG=1`).
static DRAWN: AtomicU64 = AtomicU64::new(0);
static NANOS: AtomicU64 = AtomicU64::new(0);

/// Demande (ou non) une image à chaque tour du minuteur.
pub fn set_running(running: bool) {
    RUNNING.store(running, Ordering::Relaxed);
}

/// Le minuteur envoie-t-il les images ? Sinon c'est la fenêtre
/// (`window::frames`, 60 Hz) qui cadence le temps de l'animation : `App::tick`
/// avance alors la poussière et la respiration du pas réel écoulé, et le
/// minuteur ne compte pas une seconde fois (voir `Sparkles::advance_by`).
pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Démarre le minuteur des particules : un fil endormi qui ne réveille l'UI
/// que lorsque `set_running(true)` est en place.
pub fn spawn_ticker() {
    let _ = std::thread::Builder::new()
        .name("glauncher-fx".into())
        .spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(interval_ms()));
                if RUNNING.load(Ordering::Relaxed) {
                    crate::app::post(Message::Sparkle);
                }
            }
        });
}

/// État de l'animation : le sprite décodé une fois, et le temps écoulé.
pub struct Sparkles {
    sprite: Handle,
    time: f32,
}

/// Sélection qui respire : le fond de la vignette et son visuel grossissent par
/// la carte elle-même (`Pose`), et le canvas redessine le cadre d'accent autour
/// de la cover, à la même échelle et autour du même point fixe — le centre du
/// titre, qui ne bouge pas (voir `App::card`).
#[derive(Clone)]
pub struct Breath {
    /// Point fixe de l'échelle, dans la fenêtre : centre du titre.
    pub pivot: Point,
    /// Rectangle de l'image dans la fenêtre.
    pub art: Rectangle,
    /// Rayon des coins de la vignette.
    pub radius: f32,
    /// Couleur et épaisseur du cadre de sélection.
    pub frame_color: Color,
    pub frame_width: f32,
    /// Facteur d'échelle courant (1.0 = repos). Il est calculé une seule fois
    /// par image, dans `App::breathing`, pour que la carte et le cadre du
    /// canvas bougent EXACTEMENT ensemble.
    pub factor: f32,
    /// Décalage écran de la vignette (glissade d'un changement de taille) :
    /// la carte est dessinée à cette position par une `Pose`, le cadre doit la
    /// suivre au pixel près.
    pub offset: Vector,
}

/// Respiration de la vignette sélectionnée : période (s) et amplitude (5 %).
/// Doublée le 22/11 (« pas assez fort ») et rendue continue à 60 Hz (voir
/// `advance_by` et `App::subscription` : c'est l'image de la fenêtre qui
/// cadence la respiration, le minuteur de particules à 20 Hz la faisait
/// avancer par paliers visibles).
const BREATH_PERIOD: f32 = 4.0;
const BREATH_AMPLITUDE: f32 = 0.05;
/// Rayon du cadre d'accent, plus court que celui de la carte : le rectangle
/// qu'il entoure est plus petit (la cover seule).
const FRAME_RADIUS_RATIO: f32 = 0.72;

/// Tranche du boîtier : bande sombre au bord gauche de la cover (le boîtier vu
/// de face mais légèrement de côté), avec un trait clair sur son arête.
const SPINE_RATIO: f32 = 0.045;
const SPINE_MIN: f32 = 3.0;
const SPINE_MAX: f32 = 11.0;
/// Fond de la tranche et arête claire. La vignette (`App::card`) et le canvas
/// doivent utiliser EXACTEMENT les mêmes valeurs, sinon la tranche saute quand
/// la respiration redessine la cover : d'où ces constantes partagées.
pub const SPINE_FILL: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.30);
pub const SPINE_EDGE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.09);

/// Largeur de la tranche pour une cover de `art_width` pixels de large.
pub fn spine_width(art_width: f32) -> f32 {
    (art_width * SPINE_RATIO).clamp(SPINE_MIN, SPINE_MAX)
}

impl Sparkles {
    pub fn new() -> Self {
        Self {
            sprite: Handle::from_bytes(SPRITE),
            time: 0.0,
        }
    }

    /// Temps écoulé depuis le lancement (s) : la carte s'en sert pour respirer
    /// au même rythme que le canvas, sans attendre une image de particules.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// Avance le temps d'une image du minuteur (pas fixe, 20 Hz).
    pub fn advance(&mut self) {
        self.advance_by(interval_ms() as f32 / 1000.0);
    }

    /// Avance le temps du pas RÉEL écoulé (`App::tick`), pour que la
    /// respiration suive la cadence de la fenêtre (60 Hz) au lieu de sauter
    /// toutes les 50 ms. Le temps n'avance que pendant les images : une pause
    /// (menu, page, fenêtre sans focus) fige la phase et la respiration reprend
    /// où elle s'était arrêtée, sans saut d'échelle.
    pub fn advance_by(&mut self, dt: f32) {
        self.time += dt.clamp(0.0, 0.25);
        // bornée : après une heure la précision de f32 sur des sinus se dégrade
        if self.time > 3600.0 {
            self.time -= 3600.0;
        }
    }

    /// Couche transparente à poser au-dessus de l'interface. `fade` est
    /// l'opacité du voile noir du lancement : les particules foncent vers les
    /// bords (« hyperespace ») et s'éteignent sous le voile, `scroll` fait
    /// glisser la poussière plus lentement que la grille (parallaxe).
    /// `breath` est la vignette sélectionnée, qui respire (cadre compris).
    pub fn view(
        &self,
        scroll: f32,
        fade: f32,
        breath: Option<Breath>,
    ) -> Element<'_, Message> {
        Canvas::new(Field {
            sprite: self.sprite.clone(),
            time: self.time,
            scroll,
            fade,
            breath,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

/// Ce que le `canvas` dessine pour une image donnée.
struct Field {
    sprite: Handle,
    time: f32,
    scroll: f32,
    fade: f32,
    breath: Option<Breath>,
}

impl Program<Message> for Field {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let started = std::time::Instant::now();
        // Une seule géométrie : la respiration est peinte par la carte
        // elle-même (`Pose`), le canvas ne porte que le cadre d'accent et les
        // particules. Dans une géométrie, iced dessine **tous les tracés avant
        // toutes les images** : le cadre (tracé) passe donc sous les particules
        // (images), ce qui est l'ordre voulu.
        let mut frame = Frame::new(renderer, bounds.size());
        if bounds.width <= 1.0 || bounds.height <= 1.0 {
            return vec![frame.into_geometry()];
        }

        let width = bounds.width;
        let height = bounds.height;
        let scale = (height / 1080.0).clamp(0.62, 1.7);

        // Vignette sélectionnée : son facteur de respiration est déjà calculé
        // par la carte (`App::breathing`) ; le canvas ne fait que redessiner
        // le cadre d'accent autour de la cover, à la même échelle. Le cadre est
        // dessiné même sous le voile du lancement (sinon la sélection
        // disparaîtrait pendant les 0,9 s du fondu) : c'est la respiration
        // seule qui s'arrête.
        if let Some(breath) = &self.breath {
            draw_breath(&mut frame, breath, bounds);
        }

        let count = ((width * height) / (1920.0 * 1080.0) * 132.0).clamp(40.0, 150.0);
        // Voile du lancement : les particules foncent vers les bords et
        // s'étirent en traînée. C'est le voile noir, AU-DESSUS de cette
        // couche, qui les éteint — inutile de baisser leur opacité en plus.
        let warp = self.fade.clamp(0.0, 1.0);
        let (center_x, center_y) = (width * 0.5, height * 0.5);

        for i in 0..count as u32 {
            let fleck = fleck(i, self.time, width, height, self.scroll, scale);
            if fleck.alpha < 0.006 {
                continue;
            }
            let (mut x, mut y) = (fleck.x, fleck.y);
            let mut direction = (0.0, 0.0);
            let mut copies = 1;
            if warp > 0.02 {
                // poussée radiale depuis le centre : accélère avec le fondu
                let (dx, dy) = (x - center_x, y - center_y);
                let length = (dx * dx + dy * dy).sqrt().max(1.0);
                direction = (dx / length, dy / length);
                let push = warp * warp * width.max(height) * (0.40 + 0.40 * hash(i, 31));
                x += direction.0 * push;
                y += direction.1 * push;
                copies = 3;
            }
            for copy in 0..copies {
                let trail = copy as f32;
                // le voile noir passe AU-DESSUS : sans ce coup de pouce, la
                // poussée ne se voit que pendant les 300 premières ms
                let boost = 1.0 + 2.8 * warp;
                let alpha = (fleck.alpha
                    * boost
                    * match copy {
                        0 => 1.0,
                        1 => 0.45,
                        _ => 0.18,
                    })
                .min(1.0);
                if alpha < 0.006 {
                    continue;
                }
                let size = fleck.size * (1.0 - 0.12 * trail);
                let offset = warp * warp * 95.0 * scale * trail;
                draw_sprite(
                    &mut frame,
                    &self.sprite,
                    x - direction.0 * offset,
                    y - direction.1 * offset,
                    size,
                    alpha,
                    fleck.rot,
                );
            }
        }

        if std::env::var_os("GLAUNCHER_FX_DEBUG").is_some() {
            report(started.elapsed().as_nanos() as u64);
        }

        vec![frame.into_geometry()]
    }
}

/// Facteur de respiration : `1.0` au repos, jusqu'à `1 + BREATH_AMPLITUDE` au
/// sommet, sur `BREATH_PERIOD` secondes — un aller-retour lent et continu
/// (départ et arrivée à `1.0`, donc aucun saut en reprenant la vignette).
/// `App` s'en sert pour la carte (`Pose`) ET pour le cadre du canvas, avec le
/// même temps : les deux bougent donc exactement ensemble.
pub fn breath_factor(time: f32, pulse: bool) -> f32 {
    if !pulse {
        return 1.0;
    }
    let phase = time / BREATH_PERIOD * std::f32::consts::TAU;
    1.0 + BREATH_AMPLITUDE * (0.5 - 0.5 * phase.cos())
}

/// Dessine le **cadre d'accent** autour de la cover, grossi de `factor` autour
/// du même point fixe que la carte (le centre du titre) — exactement la
/// transformation qu'elle applique (`Pose`), donc le cadre reste collé à l'image
/// qui respire.
///
/// Seul le cadre vient du canvas : le fond de la vignette, la cover, la tranche
/// et l'étoile sont grossis par la carte, ce qui laisse le cadre au-dessus de
/// l'image (une géométrie dessinée après l'arbre). Le cadre entoure la cover,
/// pas le titre (demandé le 20/11).
fn draw_breath(shapes: &mut Frame, breath: &Breath, bounds: Rectangle) {
    // Le canvas couvre toute la fenêtre alors que la liste est découpée par sa
    // zone défilante : on découpe nous-mêmes à la même bande, sinon le cadre
    // d'une vignette sortie de la liste (ou glissée sous le pied de page)
    // resterait dessiné par-dessus la marge.
    let band = list_band(bounds);
    if band.height <= 1.0 {
        return;
    }

    let factor = breath.factor;
    let middle = breath.pivot + breath.offset;
    let art = breath.art;
    let center = art.center() + breath.offset;
    let art = Rectangle::new(
        Point::new(
            middle.x + (center.x - middle.x) * factor - art.width * factor * 0.5,
            middle.y + (center.y - middle.y) * factor - art.height * factor * 0.5,
        ),
        Size::new(art.width * factor, art.height * factor),
    );

    shapes.with_clip(band, |layer| {
        let path = Path::rounded_rectangle(
            art.position(),
            art.size(),
            (breath.radius * FRAME_RADIUS_RATIO * factor).into(),
        );
        layer.stroke(
            &path,
            Stroke::default()
                .with_color(breath.frame_color)
                .with_width(breath.frame_width),
        );
    });
}

/// Bande occupée par la liste : le canvas, lui, couvre toute la fenêtre.
fn list_band(bounds: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(0.0, PAGE_PADDING_Y),
        Size::new(
            bounds.width,
            (bounds.height - PAGE_PADDING_Y - FOOTER_BAND).max(0.0),
        ),
    )
}

/// Pose le sprite `size × size` centré sur `(x, y)`.
fn draw_sprite(frame: &mut Frame, sprite: &Handle, x: f32, y: f32, size: f32, alpha: f32, rotation: f32) {
    if alpha < 0.006 || size <= 0.0 {
        return;
    }
    let bounds = Rectangle::new(
        Point::new(x - size * 0.5, y - size * 0.5),
        Size::new(size, size),
    );
    frame.draw_image(
        bounds,
        Image::new(sprite.clone())
            .opacity(alpha)
            .rotation(rotation),
    );
}

/// Opacité du voile en dessous de laquelle la respiration peut tourner : au
/// lancement, la cover ne doit pas « flotter » dans un écran noir. Partagé avec
/// `App::breath_factor`, qui décide de la pause.
pub const VEIL_CLEAR: f32 = 0.02;

/// Journal du coût de dessin, une ligne toutes les 100 images (≈ 5 s).
fn report(nanos: u64) {
    let n = DRAWN.fetch_add(1, Ordering::Relaxed) + 1;
    let total = NANOS.fetch_add(nanos, Ordering::Relaxed) + nanos;
    if n % 100 == 0 {
        eprintln!(
            "glauncher : particules, {:.2} ms par image (moyenne sur {n})",
            total as f64 / n as f64 / 1e6
        );
    }
}

/// Une particule telle qu'elle est à l'instant `t`.
struct Fleck {
    x: f32,
    y: f32,
    size: f32,
    alpha: f32,
    rot: f32,
}

/// Paramètres d'une particule, calculés à partir de son seul numéro : pas de
/// tableau à parcourir, pas d'allocation, et un rendu identique d'une image à
/// l'autre (donc un scintillement continu, sans saut).
fn fleck(i: u32, t: f32, width: f32, height: f32, scroll: f32, scale: f32) -> Fleck {
    // Trois familles : poussière fine (nombreuse), étoiles moyennes, et
    // quelques grands halos très pâles qui donnent l'air scintillant.
    let family = hash(i, 11);
    let (size_min, size_max, alpha_min, alpha_max, speed_min, speed_max, parallax, twinkle_min, twinkle_max, spin) =
        if family < 0.60 {
            (3.0, 11.0, 0.10, 0.32, 6.0, 18.0, 0.10, 0.10, 0.45, 0.9)
        } else if family < 0.94 {
            (12.0, 32.0, 0.30, 0.72, 9.0, 26.0, 0.18, 0.16, 0.70, 0.55)
        } else {
            (56.0, 132.0, 0.04, 0.10, 2.0, 6.0, 0.06, 0.04, 0.16, 0.12)
        };

    let size = lerp(size_min, size_max, hash(i, 12)) * scale;
    let base_alpha = lerp(alpha_min, alpha_max, hash(i, 13));
    let vx = lerp(speed_min, speed_max, hash(i, 14)) * sign(hash(i, 15));
    let vy = lerp(speed_min, speed_max, hash(i, 16)) * sign(hash(i, 17));
    let phase = hash(i, 18) * std::f32::consts::TAU;
    let twinkle = lerp(twinkle_min, twinkle_max, hash(i, 19));

    // Dérive : translation continue + petite oscillation, repliée aux bords.
    let wobble = width * 0.005 * (0.3 + hash(i, 20));
    let x = wrap(
        hash(i, 21) * width + vx * t + wobble * (t * 0.30 + phase).sin(),
        width,
    );
    let y = wrap(
        hash(i, 22) * height + vy * t - scroll * parallax
            + wobble * (t * 0.21 + phase * 1.7).cos(),
        height,
    );

    // Scintillement : sinus élevé au cube → de longs creux et de brefs éclats.
    let pulse = 0.5 + 0.5 * (t * twinkle * std::f32::consts::TAU + phase).sin();
    let alpha = base_alpha * (0.32 + 0.68 * pulse * pulse * pulse);

    Fleck {
        x,
        y,
        size,
        alpha,
        rot: (t * spin * 0.35 + phase).rem_euclid(std::f32::consts::TAU),
    }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn sign(value: f32) -> f32 {
    if value < 0.5 { -1.0 } else { 1.0 }
}

fn wrap(value: f32, extent: f32) -> f32 {
    value.rem_euclid(extent.max(1.0))
}

/// Bruit déterministe dans `0.0..1.0` : même graine, même particule, à chaque
/// image et à chaque lancement.
fn hash(i: u32, salt: u32) -> f32 {
    let mut x = i
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(salt.wrapping_mul(0x85EB_CA6B));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2545_F491);
    x ^= x >> 13;
    x = x.wrapping_mul(0x27D4_EB2F);
    x ^= x >> 16;
    (x & 0x00FF_FFFF) as f32 / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_particules_restent_dans_le_cadre() {
        let (width, height) = (1920.0, 1080.0);
        for i in 0..130 {
            for step in 0..600 {
                let f = fleck(i, step as f32 * 0.05, width, height, 400.0, 1.0);
                assert!(f.x >= 0.0 && f.x <= width, "x hors cadre : {}", f.x);
                assert!(f.y >= 0.0 && f.y <= height, "y hors cadre : {}", f.y);
                assert!(f.alpha >= 0.0 && f.alpha <= 1.0, "alpha : {}", f.alpha);
                assert!(f.size > 0.0 && f.size < 200.0, "taille : {}", f.size);
            }
        }
    }

    #[test]
    fn la_respiration_reste_dans_ses_bornes() {
        // facteur 1.0 au repos, jamais au-delà de l'amplitude, et continu
        assert_eq!(breath_factor(1.234, false), 1.0);
        assert_eq!(breath_factor(0.0, true), 1.0);
        let mut previous = 1.0;
        for step in 0..400 {
            let factor = breath_factor(step as f32 * 0.05, true);
            assert!(factor >= 1.0, "facteur : {factor}");
            assert!(factor <= 1.0 + BREATH_AMPLITUDE + f32::EPSILON, "facteur : {factor}");
            // pas de saut d'une image à l'autre (0,05 s d'écart)
            assert!((factor - previous).abs() < 0.005, "saut : {previous} → {factor}");
            previous = factor;
        }
        // la respiration atteint bien son sommet sur une période
        let top = (0..200)
            .map(|step| breath_factor(step as f32 * BREATH_PERIOD / 200.0, true))
            .fold(1.0, f32::max);
        assert!((top - (1.0 + BREATH_AMPLITUDE)).abs() < 0.001, "sommet : {top}");
    }

    #[test]
    fn la_poussiere_bouge_et_scintille() {
        // une particule bouge entre deux instants, et son opacité varie
        let a = fleck(7, 0.0, 1920.0, 1080.0, 0.0, 1.0);
        let b = fleck(7, 4.0, 1920.0, 1080.0, 0.0, 1.0);
        assert!((a.x - b.x).abs() > 1.0 || (a.y - b.y).abs() > 1.0);
        let alphas: Vec<f32> = (0..400)
            .map(|s| fleck(7, s as f32 * 0.05, 1920.0, 1080.0, 0.0, 1.0).alpha)
            .collect();
        let low = alphas.iter().cloned().fold(f32::MAX, f32::min);
        let high = alphas.iter().cloned().fold(0.0, f32::max);
        assert!(high > low * 1.5, "scintillement : {low} → {high}");
    }

    #[test]
    fn la_graine_est_stable() {
        for i in 0..50 {
            assert_eq!(hash(i, 3), hash(i, 3));
        }
        // deux particules voisines n'ont pas les mêmes paramètres
        assert_ne!(hash(1, 3), hash(2, 3));
    }
}
