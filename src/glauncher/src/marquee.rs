//! `Marquee` : un titre trop large pour la vignette, qui défile tout seul.
//!
//! Le texte est mis en page à sa largeur NATURELLE (limite infinie) puis
//! dessiné dans une bande de la largeur disponible, recadrée (`with_layer`) :
//! un titre long ne peut donc pas élargir la vignette ni empiéter sur la
//! voisine.
//!
//! Défilement « intelligent » : pause au début (on lit le nom), descente
//! jusqu'au dernier caractère, pause, retour — et seulement si le texte
//! dépasse vraiment. Un titre qui tient est simplement centré, sans
//! transformation ni découpe.
//!
//! L'horloge vient de `App::tick` (aucun temps stocké ici) : quand plus rien
//! ne défile, plus aucune image n'est demandée au moteur.

use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Layout, Shell, layout, renderer};
use iced::{Element, Length, Rectangle, Size, Transformation, mouse};

/// Pause à chaque extrémité (s) et vitesse de défilement (px par seconde).
const PAUSE: f32 = 1.2;
const SPEED: f32 = 34.0;

/// Enveloppe qui recadre son contenu et le fait défiler horizontalement quand
/// il ne tient pas dans la largeur offerte.
pub struct Marquee<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    elapsed: f32,
}

impl<'a, Message, Theme, Renderer> Marquee<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            elapsed: 0.0,
        }
    }

    /// Secondes écoulées depuis que ce titre est à l'écran (le défilement en
    /// dérive : pause, descente, pause, retour).
    pub fn elapsed(mut self, seconds: f32) -> Self {
        self.elapsed = seconds;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Marquee<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size {
            // la largeur est celle qu'on nous offre, jamais celle du texte
            width: Length::Fill,
            height: self.content.as_widget().size().height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let available = limits.max();
        // mise en page du texte SANS limite de largeur : sa taille naturelle
        let natural = layout::Limits::new(
            Size::ZERO,
            Size::new(f32::INFINITY, available.height),
        );
        let child = self
            .content
            .as_widget_mut()
            .layout(tree, renderer, &natural);

        let width = if available.width.is_finite() {
            available.width
        } else {
            child.size().width
        };
        layout::Node::with_children(Size::new(width, child.size().height), vec![child])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let child = match layout.children().next() {
            Some(child) => child,
            None => return,
        };
        let width = child.bounds().width;

        // le titre tient : centré, sans découpe (le cas courant)
        if width <= bounds.width + 0.5 {
            let dx = (bounds.width - width) * 0.5;
            if dx.abs() < 0.05 {
                self.content.as_widget().draw(
                    tree, renderer, theme, style, child, cursor, viewport,
                );
            } else {
                renderer.with_transformation(Transformation::translate(dx, 0.0), |renderer| {
                    self.content.as_widget().draw(
                        tree, renderer, theme, style, child, cursor, viewport,
                    );
                });
            }
            return;
        }

        // trop large : on découpe à la bande et on fait défiler
        let dx = -travel(width - bounds.width, self.elapsed);
        renderer.with_layer(bounds, |renderer| {
            renderer.with_transformation(Transformation::translate(dx, 0.0), |renderer| {
                self.content.as_widget().draw(
                    tree, renderer, theme, style, child, cursor, &bounds,
                );
            });
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }
}

/// Distance parcourue à `elapsed` secondes pour un débordement de `overflow`
/// pixels : 0 au départ, `overflow` au bout, puis retour. Aucun saut.
fn travel(overflow: f32, elapsed: f32) -> f32 {
    let cross = overflow / SPEED;
    let cycle = 2.0 * (PAUSE + cross);
    let t = elapsed.rem_euclid(cycle);
    if t < PAUSE {
        0.0
    } else if t < PAUSE + cross {
        (t - PAUSE) * SPEED
    } else if t < 2.0 * PAUSE + cross {
        overflow
    } else {
        overflow - (t - 2.0 * PAUSE - cross) * SPEED
    }
}

impl<'a, Message, Theme, Renderer> From<Marquee<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(marquee: Marquee<'a, Message, Theme, Renderer>) -> Self {
        Element::new(marquee)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_defilement_reste_dans_les_bornes() {
        let overflow = 120.0;
        // départ en pause, texte au début
        assert_eq!(travel(overflow, 0.0), 0.0);
        assert_eq!(travel(overflow, PAUSE - 0.01), 0.0);

        let mut previous: f32 = 0.0;
        let mut top: f32 = 0.0;
        for step in 0..6000 {
            let x = travel(overflow, step as f32 * 0.01);
            assert!((0.0..=overflow).contains(&x), "x : {x}");
            // pas de saut d'une image à l'autre (10 ms)
            assert!((x - previous).abs() <= SPEED * 0.011, "saut : {previous} → {x}");
            previous = x;
            top = top.max(x);
        }
        // le dernier caractère est bien atteint, puis on revient au début
        assert!((top - overflow).abs() < 0.01, "sommet : {top}");
        let cycle = 2.0 * (PAUSE + overflow / SPEED);
        assert!(travel(overflow, cycle - 0.001) < 0.05, "retour au début");
    }
}
