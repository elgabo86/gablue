//! Indicateurs d'état du poste, dans le coin haut gauche : manettes
//! connectées et réseau (Wi-Fi ou filaire).
//!
//! Aucune commande externe : l'état du lien se lit dans `/sys/class/net`
//! (`operstate`, présence du dossier `wireless`), les manettes viennent déjà
//! des évènements `evdev` de `gamepad`. Les icônes sont **dessinées** au
//! canvas (comme la roue et la poussière) : pas d'image à embarquer, une
//! couleur par famille de manette, et rien à télécharger.

use std::time::Duration;

use iced::widget::canvas::{Canvas, Frame, Geometry, Path, Program, Style, Stroke, path::Arc};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Theme, mouse};

use crate::gamepad::PadKind;

/// État du lien réseau, tel que l'icône le montre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reseau {
    /// Aucune interface physique active.
    #[default]
    HorsLigne,
    Filaire,
    Wifi,
}

/// Vert des manettes Xbox, bleu des PlayStation : la famille se lit à la
/// couleur en plus de la forme.
pub fn teinte(kind: PadKind) -> Color {
    match kind {
        PadKind::Xbox => Color::from_rgb(0.32, 0.72, 0.28),
        PadKind::Playstation => Color::from_rgb(0.28, 0.52, 0.92),
        // manette non reconnue : gris neutre, on ne prétend pas savoir
        PadKind::Other => Color::from_rgb(0.72, 0.72, 0.76),
    }
}

/// État du lien, lu dans `/sys/class/net` : une interface **physique** (elle a
/// un dossier `device`) et `operstate == up`. Les interfaces virtuelles
/// (`docker0`, `virbr0`, `tailscale0`, `veth…`) sont actives sans qu'aucun
/// câble ne soit branché — les compter ferait croire à un réseau en ligne.
pub fn reseau() -> Reseau {
    let Ok(entrees) = std::fs::read_dir("/sys/class/net") else {
        return Reseau::HorsLigne;
    };
    let mut wifi = false;
    let mut filaire = false;
    for entree in entrees.flatten() {
        let nom = entree.file_name();
        if nom == "lo" {
            continue;
        }
        let base = entree.path();
        if !base.join("device").exists() {
            continue;
        }
        let actif = std::fs::read_to_string(base.join("operstate"))
            .map(|etat| etat.trim() == "up")
            .unwrap_or(false);
        if !actif {
            continue;
        }
        if base.join("wireless").exists() {
            wifi = true;
        } else {
            filaire = true;
        }
    }
    if wifi {
        Reseau::Wifi
    } else if filaire {
        Reseau::Filaire
    } else {
        Reseau::HorsLigne
    }
}

/// Guetteur du réseau : relève l'état toutes les 3 s et prévient quand il
/// change. Une lecture de `/sys` est gratuite (pas de sonde réseau), donc pas
/// besoin d'inotify ni de NetworkManager par DBus.
pub fn surveiller<F>(emit: F)
where
    F: Fn(Reseau) + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("glauncher-reseau".into())
        .spawn(move || {
            let mut avant = reseau();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let maintenant = reseau();
                if maintenant != avant {
                    avant = maintenant;
                    emit(maintenant);
                }
            }
        });
}

/// Icône du réseau : arcs du Wi-Fi, prise du filaire, arcs barrés hors ligne.
pub fn vue_reseau<Message: 'static>(etat: Reseau, couleur: Color) -> Element<'static, Message> {
    Canvas::new(Icone {
        dessin: Dessin::Reseau(etat),
        couleur,
    })
    .width(Length::Fixed(RESEAU_L))
    .height(Length::Fixed(RESEAU_H))
    .into()
}

/// Icône d'une manette connectée, teintée par famille.
pub fn vue_manette<Message: 'static>(kind: PadKind) -> Element<'static, Message> {
    Canvas::new(Icone {
        dessin: Dessin::Manette(kind),
        couleur: teinte(kind),
    })
    .width(Length::Fixed(MANETTE_L))
    .height(Length::Fixed(MANETTE_H))
    .into()
}

/// Largeur × hauteur des deux icônes (px).
const RESEAU_L: f32 = 22.0;
const RESEAU_H: f32 = 18.0;
const MANETTE_L: f32 = 26.0;
const MANETTE_H: f32 = 18.0;

enum Dessin {
    Reseau(Reseau),
    Manette(PadKind),
}

struct Icone {
    dessin: Dessin,
    couleur: Color,
}

impl<Message> Program<Message> for Icone {
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
        // coordonnées RELATIVES au canvas : `Canvas::draw` translate déjà le
        // renderer de `bounds.x/y` (piège mesuré pour la roue, 20/11)
        let couleur = self.couleur;
        match self.dessin {
            Dessin::Reseau(etat) => dessiner_reseau(&mut frame, etat, couleur, bounds.size()),
            Dessin::Manette(kind) => dessiner_manette(&mut frame, kind, couleur, bounds.size()),
        }
        vec![frame.into_geometry()]
    }
}

fn dessiner_reseau(frame: &mut Frame, etat: Reseau, couleur: Color, taille: iced::Size) {
    let (l, h) = (taille.width, taille.height);
    let centre = Point::new(l / 2.0, h - 3.0);
    // trois arcs concentriques ouverts vers le haut (y descend : 225° → 315°)
    let mut arc = |rayon: f32, largeur: f32, teinte: Color| {
        let chemin = Path::new(|trace| {
            trace.arc(Arc {
                center: centre,
                radius: rayon,
                start_angle: Radians(225f32.to_radians()),
                end_angle: Radians(315f32.to_radians()),
            });
        });
        frame.stroke(
            &chemin,
            Stroke {
                width: largeur,
                style: Style::Solid(teinte),
                ..Stroke::default()
            },
        );
    };
    match etat {
        Reseau::Wifi | Reseau::HorsLigne => {
            // hors ligne : les arcs restent, en sourdine, et une barre les coupe
            let teinte = if etat == Reseau::Wifi {
                couleur
            } else {
                Color { a: 0.35, ..couleur }
            };
            for (rayon, largeur) in [(3.0, 1.5), (6.5, 1.5), (10.0, 1.5)] {
                arc(rayon, largeur, teinte);
            }
            frame.fill(&Path::circle(centre, 1.6), teinte);
            if etat == Reseau::HorsLigne {
                let barre = Path::line(Point::new(2.0, h - 1.0), Point::new(l - 2.0, 1.0));
                frame.stroke(
                    &barre,
                    Stroke {
                        width: 1.8,
                        style: Style::Solid(Color::from_rgb(0.85, 0.35, 0.30)),
                        ..Stroke::default()
                    },
                );
            }
        }
        Reseau::Filaire => {
            // prise : corps, deux broches, câble — lisible à 18 px
            let corps = Path::rounded_rectangle(
                Point::new(l / 2.0 - 5.0, 5.0),
                iced::Size::new(10.0, 7.0),
                1.5.into(),
            );
            frame.fill(&corps, couleur);
            for x in [l / 2.0 - 2.5, l / 2.0 + 2.5] {
                let broche = Path::line(Point::new(x, 1.5), Point::new(x, 5.0));
                frame.stroke(
                    &broche,
                    Stroke {
                        width: 1.5,
                        style: Style::Solid(couleur),
                        ..Stroke::default()
                    },
                );
            }
            let cable = Path::line(Point::new(l / 2.0, 12.0), Point::new(l / 2.0, h - 1.0));
            frame.stroke(
                &cable,
                Stroke {
                    width: 1.5,
                    style: Style::Solid(couleur),
                    ..Stroke::default()
                },
            );
        }
    }
}

fn dessiner_manette(frame: &mut Frame, _kind: PadKind, couleur: Color, taille: iced::Size) {
    let (l, h) = (taille.width, taille.height);
    // corps + deux poignées : trois remplissages de même teinte font une
    // silhouette de manette à cette taille
    let corps = Path::rounded_rectangle(
        Point::new(6.0, 2.0),
        iced::Size::new(l - 12.0, h - 8.0),
        3.0.into(),
    );
    frame.fill(&corps, couleur);
    frame.fill(&Path::circle(Point::new(5.0, h - 4.5), 3.6), couleur);
    frame.fill(&Path::circle(Point::new(l - 5.0, h - 4.5), 3.6), couleur);
    // croix à gauche, deux boutons à droite : ce qu'on reconnaît d'un coup d'œil
    let trou = Color {
        a: 0.55,
        ..Color::BLACK
    };
    let croix = Path::rounded_rectangle(
        Point::new(7.5, 9.5),
        iced::Size::new(6.0, 1.8),
        0.9.into(),
    );
    frame.fill(&croix, trou);
    let croix_v = Path::rounded_rectangle(
        Point::new(9.6, 7.4),
        iced::Size::new(1.8, 6.0),
        0.9.into(),
    );
    frame.fill(&croix_v, trou);
    frame.fill(&Path::circle(Point::new(l - 10.5, 8.6), 1.3), trou);
    frame.fill(&Path::circle(Point::new(l - 7.5, 10.8), 1.3), trou);
}
