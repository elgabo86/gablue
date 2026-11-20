//! `Pose` : dessiner un élément agrandi et déplacé, sans toucher à la mise en
//! page.
//!
//! iced n'a pas de transformation de sous-arbre accessible aux widgets
//! ordinaires : `iced::widget::float` existe mais dessine son contenu dans une
//! **surcouche**, au-dessus de toute la pile (voile noir compris) — inutilisable
//! pour un effet qui doit rester sous le voile. `Pose` applique la
//! `Transformation` au rendu, dans l'arbre normal : l'ordre des couches est
//! conservé et le découpage de la zone défilante s'applique.
//!
//! Sert à deux choses : la plongée de la page au lancement d'un jeu et le
//! retour des vignettes depuis les bords (`App::comeback`).

use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Layout, Shell, layout, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Transformation, Vector, mouse};

/// Enveloppe transparente : mêmes dimensions et même mise en page que son
/// contenu, dessiné avec une échelle et un décalage.
pub struct Pose<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    scale: f32,
    offset: Vector,
}

impl<'a, Message, Theme, Renderer> Pose<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            scale: 1.0,
            offset: Vector::ZERO,
        }
    }

    /// Échelle appliquée autour du centre du contenu (1 = taille normale).
    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Décalage en pixels, écran.
    pub fn offset(mut self, offset: Vector) -> Self {
        self.offset = offset;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Pose<'_, Message, Theme, Renderer>
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
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
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
        // au repos, rien à transformer : on dessine comme n'importe quel
        // conteneur (aucun coût quand l'animation est finie)
        if (self.scale - 1.0).abs() < 0.0001 && self.offset == Vector::ZERO {
            self.content.as_widget().draw(
                tree, renderer, theme, style, layout, cursor, viewport,
            );
            return;
        }

        let bounds = layout.bounds();
        let middle = bounds.center();
        let transformation = Transformation::translate(
            middle.x + self.offset.x,
            middle.y + self.offset.y,
        ) * Transformation::scale(self.scale)
            * Transformation::translate(-middle.x, -middle.y);

        renderer.with_transformation(transformation, |renderer| {
            self.content.as_widget().draw(
                tree, renderer, theme, style, layout, cursor, viewport,
            );
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

impl<'a, Message, Theme, Renderer> From<Pose<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(pose: Pose<'a, Message, Theme, Renderer>) -> Self {
        Element::new(pose)
    }
}
