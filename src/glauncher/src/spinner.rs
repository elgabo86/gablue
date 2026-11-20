//! Roue de chargement discrète, posée dans un coin de l'écran pendant que le
//! launcher télécharge quelque chose (visuels SteamGridDB).
//!
//! Elle ne se voit **que** quand une requête est en cours ou vient d'en finir
//! (`covers::busy`) : au repos le canvas ne dessine rien et aucune image n'est
//! demandée au moteur.
//!
//! **Elle occupe un emplacement permanent** dans la pile racine de `view` :
//! ajouter ou retirer un enfant à cet endroit recréerait le sous-arbre, donc la
//! zone défilante, et la liste remonterait en haut à chaque téléchargement
//! (constaté 20/11). Le widget est donc toujours là, `visible` ne décide que du
//! dessin.
//!
//! Dessin : quelques points sur un cercle, d'autant plus transparents qu'ils
//! sont en arrière de la tête — pas d'arc à tesseller, pas de dégradé. L'angle
//! vient de `App::tick` : le module ne garde aucun temps.

use iced::widget::canvas::{Canvas, Frame, Geometry, Path, Program};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

/// Points de la roue, rayon du cercle et taille d'un point (px).
const DOTS: usize = 9;
const RADIUS: f32 = 8.0;
const DOT: f32 = 1.9;
/// Opacité de la tête et de la queue de la roue.
const HEAD: f32 = 0.85;
const TAIL: f32 = 0.08;

/// Roue de `size` pixels, tournée de `angle` radians. `visible` à faux : le
/// canvas reste en place mais ne dessine rien (forme d'arbre stable).
pub fn view<Message: 'static>(angle: f32, size: f32, visible: bool) -> Element<'static, Message> {
    Canvas::new(Spin {
        angle,
        size,
        visible,
    })
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

struct Spin {
    angle: f32,
    size: f32,
    visible: bool,
}

impl<Message> Program<Message> for Spin {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if !self.visible || bounds.width <= 1.0 {
            return vec![frame.into_geometry()];
        }

        // Coordonnées RELATIVES au canvas : `Canvas::draw` translate déjà le
        // renderer de `bounds.x/y` avant d'appeler ce `draw`. Utiliser
        // `bounds.center()` (position absolue) dessinait la roue deux fois trop
        // loin, donc hors de l'écran — c'est pourquoi elle n'apparaissait
        // jamais (constaté 20/11).
        let center = Point::new(bounds.width * 0.5, bounds.height * 0.5);
        let radius = (self.size * 0.5).min(RADIUS);
        for dot in 0..DOTS {
            // la tête est le point 0 ; la queue s'éteint DERRIÈRE elle. Le signe
            // compte : avec `+`, les points faibles passaient devant la tête et
            // la roue semblait tourner à l'envers (constaté 20/11).
            let angle = self.angle - dot as f32 / DOTS as f32 * std::f32::consts::TAU;
            let alpha = HEAD + (TAIL - HEAD) * dot as f32 / (DOTS - 1) as f32;
            let point = Point::new(
                center.x + angle.cos() * radius,
                center.y + angle.sin() * radius,
            );
            frame.fill(
                &Path::circle(point, DOT),
                Color {
                    a: alpha,
                    ..Color::WHITE
                },
            );
        }

        vec![frame.into_geometry()]
    }
}
