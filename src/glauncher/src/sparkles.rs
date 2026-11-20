//! Poussière d'étoiles posée sur l'interface, façon fond de PS3 : quelques
//! dizaines de particules qui dérivent lentement et scintillent.
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

use iced::widget::canvas::{Canvas, Frame, Geometry, Image, Program};
use iced::widget::image::Handle;
use iced::{Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::Message;

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

impl Sparkles {
    pub fn new() -> Self {
        Self {
            sprite: Handle::from_bytes(SPRITE),
            time: 0.0,
        }
    }

    /// Avance le temps d'une image du minuteur.
    pub fn advance(&mut self) {
        self.time += interval_ms() as f32 / 1000.0;
        // bornée : après une heure la précision de f32 sur des sinus se dégrade
        if self.time > 3600.0 {
            self.time -= 3600.0;
        }
    }

    /// Couche transparente à poser au-dessus de l'interface. `fade` est
    /// l'opacité du voile noir du lancement : les particules s'éteignent avec
    /// lui, `scroll` fait glisser la poussière plus lentement que la grille
    /// (parallaxe).
    pub fn view(&self, scroll: f32, fade: f32) -> Element<'_, Message> {
        Canvas::new(Field {
            sprite: self.sprite.clone(),
            time: self.time,
            scroll,
            fade,
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
        let mut frame = Frame::new(renderer, bounds.size());

        // Le voile noir du lancement éteint tout : inutile de dessiner.
        let light = (1.0 - self.fade).clamp(0.0, 1.0);
        if light > 0.01 && bounds.width > 1.0 && bounds.height > 1.0 {
            let width = bounds.width;
            let height = bounds.height;
            let count = ((width * height) / (1920.0 * 1080.0) * 132.0).clamp(40.0, 150.0);
            let scale = (height / 1080.0).clamp(0.62, 1.7);

            for i in 0..count as u32 {
                let fleck = fleck(i, self.time, width, height, self.scroll, scale, light);
                if fleck.alpha < 0.006 {
                    continue;
                }
                let size = fleck.size;
                let bounds = Rectangle::new(
                    Point::new(fleck.x - size * 0.5, fleck.y - size * 0.5),
                    Size::new(size, size),
                );
                frame.draw_image(
                    bounds,
                    Image::new(self.sprite.clone())
                        .opacity(fleck.alpha)
                        .rotation(fleck.rot),
                );
            }
        }

        if std::env::var_os("GLAUNCHER_FX_DEBUG").is_some() {
            report(started.elapsed().as_nanos() as u64);
        }

        vec![frame.into_geometry()]
    }
}

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
fn fleck(i: u32, t: f32, width: f32, height: f32, scroll: f32, scale: f32, light: f32) -> Fleck {
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
    let alpha = base_alpha * (0.32 + 0.68 * pulse * pulse * pulse) * light;

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
                let f = fleck(i, step as f32 * 0.05, width, height, 400.0, 1.0, 1.0);
                assert!(f.x >= 0.0 && f.x <= width, "x hors cadre : {}", f.x);
                assert!(f.y >= 0.0 && f.y <= height, "y hors cadre : {}", f.y);
                assert!(f.alpha >= 0.0 && f.alpha <= 1.0, "alpha : {}", f.alpha);
                assert!(f.size > 0.0 && f.size < 200.0, "taille : {}", f.size);
            }
        }
    }

    #[test]
    fn le_voile_eteint_les_particules() {
        for i in 0..130 {
            let f = fleck(i, 12.0, 1920.0, 1080.0, 0.0, 1.0, 0.0);
            assert_eq!(f.alpha, 0.0);
        }
    }

    #[test]
    fn la_poussiere_bouge_et_scintille() {
        // une particule bouge entre deux instants, et son opacité varie
        let a = fleck(7, 0.0, 1920.0, 1080.0, 0.0, 1.0, 1.0);
        let b = fleck(7, 4.0, 1920.0, 1080.0, 0.0, 1.0, 1.0);
        assert!((a.x - b.x).abs() > 1.0 || (a.y - b.y).abs() > 1.0);
        let alphas: Vec<f32> = (0..400)
            .map(|s| fleck(7, s as f32 * 0.05, 1920.0, 1080.0, 0.0, 1.0, 1.0).alpha)
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
