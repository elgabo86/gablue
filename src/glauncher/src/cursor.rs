//! Pointeur : le cacher tant qu'il n'a pas bougé.
//!
//! Demandé le 20/11 : un launcher qu'on pilote à la manette n'a que faire d'une
//! flèche immobile posée au milieu de l'écran — mais elle doit revenir dès que
//! la souris sert.
//!
//! iced ne connaît le pointeur que par `mouse::Interaction` : `Hidden` est la
//! seule valeur qui appelle `set_cursor_visible(false)` (dans `iced_winit`), et
//! n'importe quelle autre la rétablit. D'où cette enveloppe : elle laisse
//! l'interface décider de l'interaction (main sur une pastille, curseur de
//! texte…) et ne rend `Hidden` qu'après [`IDLE`] sans mouvement.
//!
//! Trois pièges (`iced_runtime`/`iced_winit`, plus un du compositeur), constatés
//! le 20/11 :
//!
//! - l'interaction n'est calculée que pendant une passe de **mise à jour**
//!   (`user_interface::update`), jamais au repos : sans message, l'horloge ne
//!   tourne pas et le pointeur ne se cache jamais. C'est pourquoi `App::
//!   subscription` arme une relève [`TICK`] tant que la fenêtre a le focus et
//!   que rien d'autre ne redessine (elle ne force aucune image, seulement une
//!   passe de mise à jour) ;
//! - iced n'appelle `set_cursor_visible` que lorsque l'interaction **change** :
//!   il faut donc repasser par une interaction visible au moindre mouvement
//!   (c'est le rôle de l'horodatage), sinon le pointeur resterait caché après
//!   une sortie puis une ré-entrée de la souris dans la fenêtre — winit
//!   restaurerait le curseur du thème sans qu'iced le sache ;
//! - KWin envoie un **mouvement synthétique** quand une surface apparaît sous
//!   le pointeur (`CursorMoved` à +66 ms à l'ouverture d'un plein écran,
//!   mesuré) : le compter comme un geste rallumait la flèche 2 s à chaque
//!   ouverture. La **première** position vue depuis une entrée n'est donc
//!   qu'une référence, seul un changement réveille le pointeur.
//!
//! `GLAUNCHER_CURSOR_DEBUG=1` trace chaque évènement et chaque bascule
//! (horodatés) : c'est le moyen de vérifier ce que le compositeur envoie sans
//! deviner.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Layout, Shell, layout, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, mouse};

/// Délai sans mouvement avant de cacher le pointeur. Assez long pour ne pas
/// disparaître entre deux gestes de la souris, assez court pour ne pas rester
/// planté au milieu de l'écran pendant qu'on navigue à la manette.
pub const IDLE: Duration = Duration::from_millis(2000);

/// Cadence de la relève quand rien d'autre ne redessine. Grossière à dessein :
/// elle ne sert qu'à donner l'heure au repos, et le pointeur se cache alors au
/// plus 250 ms après [`IDLE`]. Pendant une animation (ou la poussière
/// d'étoiles), les images existent déjà et le minuteur n'est pas lancé.
pub const TICK: Duration = Duration::from_millis(250);

/// Le minuteur doit-il donner l'heure ? Piloté par `App::subscription`.
static TICKING: AtomicBool = AtomicBool::new(false);

/// Dernière interaction rendue (seul l'état caché/visible est retenu) : sert à
/// ne tracer qu'aux bascules.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// Demande (ou non) une passe de mise à jour périodique.
pub fn set_ticking(ticking: bool) {
    TICKING.store(ticking, Ordering::Relaxed);
}

/// Démarre la relève : un fil endormi qui ne réveille l'UI que lorsque
/// `set_ticking(true)` est en place. Même principe que le minuteur des
/// particules — un fil de plus qui dort ne coûte rien, et `Message::Nothing`
/// ne demande aucune image : il ne force qu'un calcul d'interaction.
pub fn spawn_ticker() {
    START.get_or_init(Instant::now);
    let _ = std::thread::Builder::new()
        .name("glauncher-cursor".into())
        .spawn(|| {
            loop {
                std::thread::sleep(TICK);
                if TICKING.load(Ordering::Relaxed) {
                    crate::app::post(crate::app::Message::Nothing);
                }
            }
        });
}

/// Trace de diagnostic (`GLAUNCHER_CURSOR_DEBUG=1`) : chaque évènement de
/// pointeur et chaque bascule d'interaction, horodatés depuis le démarrage.
fn trace(args: std::fmt::Arguments<'_>) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("GLAUNCHER_CURSOR_DEBUG").is_some()) {
        return;
    }
    let start = START.get_or_init(Instant::now);
    eprintln!(
        "[curseur +{:>7.1} ms] {args}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

/// Instant du démarrage de la relève (repère des traces).
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Ce que l'enveloppe retient d'une image à l'autre.
#[derive(Debug, Default)]
struct Rest {
    /// Dernière position vue. `None` = aucune position connue depuis l'entrée
    /// dans la fenêtre (ou sortie) : la **première** reçue n'est qu'une
    /// référence — KWin envoie un mouvement synthétique à l'apparition de la
    /// surface — et une position identique n'est pas un geste non plus.
    position: Option<Point>,
    /// Instant du dernier mouvement. `None` = la souris n'a pas encore servi
    /// depuis l'ouverture : le pointeur démarre donc **caché** (demandé le
    /// 20/11 — un launcher ouvert à la manette n'a pas de flèche à montrer).
    last: Option<Instant>,
}

impl Rest {
    /// Le pointeur a bougé (ou on a cliqué, ou tourné la molette) : il
    /// redevient visible.
    fn awake(&mut self) {
        self.last = Some(Instant::now());
    }

    fn sleeping(&self) -> bool {
        self.last.is_none_or(|last| last.elapsed() >= IDLE)
    }
}

/// Enveloppe transparente (mêmes dimensions, même mise en page que son
/// contenu) qui cache le pointeur après [`IDLE`].
pub struct Idle<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    /// Message publié au premier **vrai** geste de souris (mouvement réel,
    /// clic, molette) : l'enveloppe est le seul endroit qui sait distinguer un
    /// mouvement du pointeur d'un `CursorMoved` synthétique du compositeur.
    on_geste: Option<Message>,
    /// Ne rien publier : la barre de défilement est déjà à l'écran, un message
    /// par mouvement ne servirait qu'à reconstruire la vue.
    muet: bool,
}

impl<'a, Message, Theme, Renderer> Idle<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_geste: None,
            muet: false,
        }
    }

    /// Message envoyé au premier geste de souris.
    pub fn on_geste(mut self, message: Message) -> Self {
        self.on_geste = Some(message);
        self
    }

    /// Taire les gestes (l'appelant n'en a plus besoin pour l'instant).
    pub fn muet(mut self, muet: bool) -> Self {
        self.muet = muet;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Idle<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Rest>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Rest::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
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
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        let rest = tree.state.downcast_mut::<Rest>();

        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => match rest.position {
                // première position connue depuis l'entrée dans la fenêtre :
                // le compositeur envoie un mouvement **synthétique** quand la
                // surface apparaît sous un pointeur qui n'a pas bougé
                // (constaté 20/11 : `CursorMoved` à +66 ms à l'ouverture d'un
                // plein écran, qui faisait réapparaître la flèche 2 s). On la
                // retient sans réveiller : seul un CHANGEMENT de position est
                // un geste.
                None => {
                    trace(format_args!(
                        "CursorMoved {position:?} (première position, sans réveil)"
                    ));
                    rest.position = Some(*position);
                }
                Some(previous) if previous != *position => {
                    trace(format_args!(
                        "CursorMoved {position:?} (précédent {previous:?}) → réveil"
                    ));
                    rest.position = Some(*position);
                    rest.awake();
                    // la vue est reconstruite dès que l'appelant réagit : le
                    // message ne part donc qu'une fois
                    if !self.muet
                        && let Some(message) = self.on_geste.take()
                    {
                        shell.publish(message);
                    }
                }
                Some(_) => {}
            },
            // un clic ou un cran de molette comptent comme un geste : la
            // souris est là, on la montre. `CursorEntered` ne compte **pas** :
            // une fenêtre qui s'ouvre sous un pointeur immobile ne doit pas
            // faire apparaître la flèche (demandé le 20/11) — et winit
            // réapplique de toute façon l'état caché à l'entrée
            // (`reload_cursor_style`), donc rien ne se dérègle.
            Event::Mouse(
                mouse::Event::ButtonPressed(_)
                | mouse::Event::ButtonReleased(_)
                | mouse::Event::WheelScrolled { .. },
            ) => {
                trace(format_args!("geste {event:?} → réveil"));
                rest.awake();
                if !self.muet
                    && let Some(message) = self.on_geste.take()
                {
                    shell.publish(message);
                }
            }
            // sortie de la fenêtre : la prochaine entrée vaudra mouvement
            Event::Mouse(mouse::Event::CursorLeft) => {
                trace(format_args!("CursorLeft"));
                rest.position = None;
            }
            Event::Mouse(mouse::Event::CursorEntered) => {
                trace(format_args!("CursorEntered (ignoré)"));
            }
            _ => {}
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        // pas besoin de savoir où est le pointeur : caché au repos vaut aussi
        // pour une fenêtre qui vient de s'ouvrir sous la souris, et `set_cursor_visible`
        // ne concerne que cette surface (les autres fenêtres gardent la leur)
        let resting = tree.state.downcast_ref::<Rest>().sleeping();

        if HIDDEN.swap(resting, Ordering::Relaxed) != resting {
            trace(format_args!(
                "interaction → {}",
                if resting { "Hidden" } else { "visible" }
            ));
        }

        if resting {
            mouse::Interaction::Hidden
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
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
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a, Message, Theme, Renderer> From<Idle<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(idle: Idle<'a, Message, Theme, Renderer>) -> Self {
        Element::new(idle)
    }
}
