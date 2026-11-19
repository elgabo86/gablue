//! Lecture des manettes en direct via evdev.
//!
//! Pas de libudev, pas de SDL : on lit `/dev/input/event*` (permissions uaccess
//! de logind, comme Steam), ce qui évite toute dépendance système à installer
//! sur Kinoite. Les boutons sont normalisés (BTN_SOUTH/BTN_EAST…), donc une
//! DualSense et une manette Xbox produisent les mêmes évènements.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};

/// Famille de manette, pour choisir le jeu de glyphes affiché.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    Xbox,
    Playstation,
    Other,
}

/// Boutons utiles au launcher (normalisés).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadButton {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Menu,
    /// Bascule grille / liste.
    View,
    /// L2 : vignettes plus petites.
    ZoomOut,
    /// R2 : vignettes plus grandes.
    ZoomIn,
    /// L1 : page précédente.
    PageUp,
    /// R1 : page suivante.
    PageDown,
}

#[derive(Debug, Clone)]
pub enum PadEvent {
    Connected { kind: PadKind, name: String },
    Disconnected { name: String },
    Button(PadButton),
}

type Emit = Arc<dyn Fn(PadEvent) + Send + Sync>;

/// Seuils du stick, en fraction de la demi-course RÉELLE du périphérique
/// (calibrée sur `absinfo`) : ~40 % pour engager, plus bas pour relâcher
/// (hystérésis), sinon un stick qui tremble fait des allers-retours.
///
/// Ne jamais coder un seuil en valeur absolue : selon le pilote et le mode de
/// connexion, une manette expose `-32768..32767` (DualSense en filaire,
/// manette Xbox) ou `0..255` centré sur 128 (constaté sur une DualSense en
/// Bluetooth) — un seuil fixe à 13000 ne se déclenche alors jamais.
const STICK_ENGAGE: f32 = 0.40;
const STICK_RELEASE: f32 = 0.25;
/// Délai avant la répétition quand on maintient le stick, puis cadence.
const STICK_DELAY: Duration = Duration::from_millis(350);
const STICK_REPEAT: Duration = Duration::from_millis(130);
/// Période de réveil du thread de lecture (le stick ne génère des évènements
/// qu'au changement : sans réveil, impossible de répéter).
const POLL: Duration = Duration::from_millis(8);

/// Démarre la surveillance des manettes : un thread superviseur (branchement à
/// chaud, re-scan toutes les 2 s) et un thread de lecture par manette.
pub fn spawn<F>(emit: F)
where
    F: Fn(PadEvent) + Send + Sync + 'static,
{
    let emit: Emit = Arc::new(emit);
    let _ = std::thread::Builder::new()
        .name("glauncher-pads".into())
        .spawn(move || supervise(emit));
}

fn supervise(emit: Emit) {
    let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

    loop {
        for (path, device) in evdev::enumerate() {
            if !is_gamepad(&device) {
                continue;
            }
            {
                let mut open = match open.lock() {
                    Ok(open) => open,
                    Err(_) => return,
                };
                if !open.insert(path.clone()) {
                    continue;
                }
            }

            let kind = kind_of(&device);
            let name = device.name().unwrap_or("manette").to_string();
            emit(PadEvent::Connected {
                kind,
                name: name.clone(),
            });

            let emit = emit.clone();
            let open = open.clone();
            let thread_name = format!("glauncher-pad:{name}");
            let path_for_read = path.clone();
            let _ = std::thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    read(path_for_read, device, &emit);
                    emit(PadEvent::Disconnected { name });
                    if let Ok(mut open) = open.lock() {
                        open.remove(&path);
                    }
                });
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Boucle de lecture d'une manette ; rend la main quand elle est débranchée.
fn read(_path: PathBuf, mut device: Device, emit: &Emit) {
    // lecture non bloquante : on doit pouvoir répéter pendant que le stick est
    // maintenu, sans qu'aucun évènement n'arrive
    if device.set_nonblocking(true).is_err() {
        return;
    }

    let debug = std::env::var_os("GLAUNCHER_PAD_DEBUG").is_some();
    let mut stick = Stick::calibrate(&device);
    if debug {
        eprintln!(
            "glauncher : manette « {} » — stick X {}..{} (centre {}), Y {}..{} (centre {})",
            device.name().unwrap_or("?"),
            stick.x_min,
            stick.x_max,
            stick.x_center,
            stick.y_min,
            stick.y_max,
            stick.y_center
        );
    }
    let mut next_repeat: Option<std::time::Instant> = None;
    let mut hat = (0i32, 0i32);
    // Boutons maintenus qui se répètent (croix et L1/R1) : bouton + prochaine
    // répétition. L'analogique avait déjà sa répétition, pas les boutons —
    // maintenir R1 ne faisait qu'une page.
    let mut tenus: Vec<(PadButton, std::time::Instant)> = Vec::new();
    // La croix est souvent exposée en AXES (ABS_HAT0X/Y) : un seul évènement au
    // changement, donc rien à répéter sans minuteur.
    let mut hat_repeat: Option<std::time::Instant> = None;

    loop {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    match event.destructure() {
                        EventSummary::Key(_, code, value) => {
                            // 1 = appui, 0 = relâchement, 2 = répétition du
                            // noyau : on gère la nôtre (délai + cadence), pas
                            // celle du pilote
                            if let Some(button) = button_of(code) {
                                match value {
                                    1 => {
                                        emit(PadEvent::Button(button));
                                        if debug {
                                            eprintln!("glauncher : manette → {button:?} (bouton)");
                                        }
                                        if repeats(button) {
                                            tenus.push((
                                                button,
                                                std::time::Instant::now() + STICK_DELAY,
                                            ));
                                        }
                                    }
                                    0 => tenus.retain(|(tenu, _)| *tenu != button),
                                    _ => {}
                                }
                            }
                        }
                        EventSummary::AbsoluteAxis(_, axis, value) => match axis {
                            AbsoluteAxisCode::ABS_X => {
                                stick.x = value;
                                next_repeat = stick.update(emit, next_repeat, debug);
                            }
                            AbsoluteAxisCode::ABS_Y => {
                                stick.y = value;
                                next_repeat = stick.update(emit, next_repeat, debug);
                            }
                            AbsoluteAxisCode::ABS_HAT0X | AbsoluteAxisCode::ABS_HAT0Y => {
                                if axis == AbsoluteAxisCode::ABS_HAT0X {
                                    hat.0 = value;
                                } else {
                                    hat.1 = value;
                                }
                                match hat_button(hat) {
                                    Some(button) => {
                                        emit(PadEvent::Button(button));
                                        hat_repeat = Some(
                                            std::time::Instant::now() + STICK_DELAY,
                                        );
                                        if debug {
                                            eprintln!("glauncher : manette → {button:?} (croix)");
                                        }
                                    }
                                    None => hat_repeat = None,
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }

        let now = std::time::Instant::now();

        // répétition tant que le stick reste penché
        if let Some(direction) = stick.direction
            && next_repeat.is_some_and(|next| now >= next)
        {
            if debug {
                eprintln!("glauncher : manette → {direction:?} (stick, répétition)");
            }
            emit(PadEvent::Button(direction));
            next_repeat = Some(now + STICK_REPEAT);
        }

        // répétition des boutons maintenus (croix en boutons, L1/R1)
        for (button, next) in tenus.iter_mut() {
            if now < *next {
                continue;
            }
            if debug {
                eprintln!("glauncher : manette → {button:?} (bouton, répétition)");
            }
            emit(PadEvent::Button(*button));
            *next = now + STICK_REPEAT;
        }

        // croix exposée en axes : elle n'émet qu'au changement de direction
        let croix_en_boutons = tenus
            .iter()
            .any(|(button, _)| matches!(button, PadButton::Up | PadButton::Down | PadButton::Left | PadButton::Right));
        if !croix_en_boutons
            && let Some(direction) = hat_button(hat)
            && hat_repeat.is_some_and(|next| now >= next)
        {
            if debug {
                eprintln!("glauncher : manette → {direction:?} (croix, répétition)");
            }
            emit(PadEvent::Button(direction));
            hat_repeat = Some(now + STICK_REPEAT);
        }

        std::thread::sleep(POLL);
    }
}

/// Boutons qui se répètent quand on les MAINTIENT : la croix (navigation) et
/// L1/R1 (pages). Les autres (valider, retour, menu…) ne doivent pas se
/// répéter — appuyer sur retour verrait son « double appui » se déclencher
/// tout seul.
fn repeats(button: PadButton) -> bool {
    matches!(
        button,
        PadButton::Up
            | PadButton::Down
            | PadButton::Left
            | PadButton::Right
            | PadButton::PageUp
            | PadButton::PageDown
    )
}

/// Direction donnée par la croix directionnelle (ABS_HAT0X/Y).
fn hat_button(hat: (i32, i32)) -> Option<PadButton> {
    match hat {
        (-1, _) => Some(PadButton::Left),
        (1, _) => Some(PadButton::Right),
        (_, -1) => Some(PadButton::Up),
        (_, 1) => Some(PadButton::Down),
        _ => None,
    }
}

/// État du stick gauche, calibré sur les bornes réelles du périphérique.
struct Stick {
    x: i32,
    y: i32,
    x_min: i32,
    x_max: i32,
    x_center: i32,
    y_min: i32,
    y_max: i32,
    y_center: i32,
    direction: Option<PadButton>,
}

impl Default for Stick {
    /// Bornes par défaut : celles d'un stick `-32768..32767` centré sur 0.
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            x_min: -32768,
            x_max: 32767,
            x_center: 0,
            y_min: -32768,
            y_max: 32767,
            y_center: 0,
            direction: None,
        }
    }
}

impl Stick {
    /// Lit les bornes des axes du périphérique (`absinfo`). Un pilote peut
    /// exposer `0..255` au lieu de `-32768..32767` : sans cette calibration,
    /// aucun seuil raisonnable ne fonctionne sur les deux.
    fn calibrate(device: &Device) -> Self {
        let mut stick = Self::default();
        let Ok(state) = device.get_abs_state() else {
            return stick;
        };

        let info = |axis: AbsoluteAxisCode| {
            evdev::AbsInfo::from(state[axis.0 as usize])
        };
        let (x, y) = (info(AbsoluteAxisCode::ABS_X), info(AbsoluteAxisCode::ABS_Y));

        if x.maximum() > x.minimum() {
            stick.x_min = x.minimum();
            stick.x_max = x.maximum();
            stick.x_center = x.minimum() + (x.maximum() - x.minimum()) / 2;
            // au repos, la valeur observée est le meilleur centre si elle est
            // déjà dans les bornes (certains pilotes ne centrent pas pile)
            if x.value() >= x.minimum() && x.value() <= x.maximum() && x.minimum() < 0 {
                stick.x_center = x.value();
            }
            stick.x = x.value();
        }
        if y.maximum() > y.minimum() {
            stick.y_min = y.minimum();
            stick.y_max = y.maximum();
            stick.y_center = y.minimum() + (y.maximum() - y.minimum()) / 2;
            if y.value() >= y.minimum() && y.value() <= y.maximum() && y.minimum() < 0 {
                stick.y_center = y.value();
            }
            stick.y = y.value();
        }
        stick
    }

    /// Position normalisée d'un axe : -1 (gauche/haut) à +1 (droite/bas).
    fn norm(value: i32, min: i32, max: i32, center: i32) -> f32 {
        let half = if value >= center {
            (max - center).max(1) as f32
        } else {
            (center - min).max(1) as f32
        };
        ((value - center) as f32 / half).clamp(-1.0, 1.0)
    }

    /// Recalcule la direction ; émet un évènement au changement et programme la
    /// première répétition.
    fn update(
        &mut self,
        emit: &Emit,
        next_repeat: Option<std::time::Instant>,
        debug: bool,
    ) -> Option<std::time::Instant> {
        let (nx, ny) = (
            Self::norm(self.x, self.x_min, self.x_max, self.x_center),
            Self::norm(self.y, self.y_min, self.y_max, self.y_center),
        );
        if debug {
            eprintln!(
                "glauncher : stick x={} ({nx:+.2}) y={} ({ny:+.2})",
                self.x, self.y
            );
        }

        let threshold = if self.direction.is_some() {
            STICK_RELEASE
        } else {
            STICK_ENGAGE
        };

        let horizontal = nx.abs() >= threshold && nx.abs() >= ny.abs();
        let direction = if horizontal {
            Some(if nx > 0.0 {
                PadButton::Right
            } else {
                PadButton::Left
            })
        } else if ny.abs() >= threshold {
            // axe Y d'evdev : positif = vers le bas
            Some(if ny > 0.0 {
                PadButton::Down
            } else {
                PadButton::Up
            })
        } else {
            None
        };

        if direction == self.direction {
            return next_repeat;
        }
        self.direction = direction;

        match direction {
            Some(button) => {
                if debug {
                    eprintln!("glauncher : manette → {button:?} (stick)");
                }
                emit(PadEvent::Button(button));
                Some(std::time::Instant::now() + STICK_DELAY)
            }
            None => None,
        }
    }
}

fn button_of(code: KeyCode) -> Option<PadButton> {
    match code {
        KeyCode::BTN_SOUTH => Some(PadButton::Confirm),
        KeyCode::BTN_EAST => Some(PadButton::Back),
        KeyCode::BTN_NORTH => Some(PadButton::View),
        KeyCode::BTN_WEST => Some(PadButton::Menu),
        KeyCode::BTN_START => Some(PadButton::Menu),
        KeyCode::BTN_TL => Some(PadButton::PageUp),
        KeyCode::BTN_TR => Some(PadButton::PageDown),
        KeyCode::BTN_TL2 => Some(PadButton::ZoomOut),
        KeyCode::BTN_TR2 => Some(PadButton::ZoomIn),
        KeyCode::BTN_DPAD_UP => Some(PadButton::Up),
        KeyCode::BTN_DPAD_DOWN => Some(PadButton::Down),
        KeyCode::BTN_DPAD_LEFT => Some(PadButton::Left),
        KeyCode::BTN_DPAD_RIGHT => Some(PadButton::Right),
        _ => None,
    }
}

/// Une manette = un périphérique avec les boutons d'un gamepad (pas un clavier).
fn is_gamepad(device: &Device) -> bool {
    device
        .supported_keys()
        .is_some_and(|keys| keys.contains(KeyCode::BTN_SOUTH))
}

/// Diagnostic (`--pads`) : périphériques d'entrée, axes exposés avec bornes et
/// valeur au repos. C'est ce qui permet de voir qu'une manette sort son stick
/// en `0..255` et non en `-32768..32767` — un seuil en valeur absolue ne se
/// déclenche alors jamais.
pub fn describe() {
    use std::fmt::Write as _;

    let mut out = String::new();
    for (path, device) in evdev::enumerate() {
        let vendor = device.input_id().vendor();
        let product = device.input_id().product();
        let _ = writeln!(
            out,
            "{}  « {} »  {vendor:04x}:{product:04x}{}",
            path.display(),
            device.name().unwrap_or("?"),
            if is_gamepad(&device) { "  [manette]" } else { "" }
        );

        let state = device.get_abs_state().ok();
        if let Some(axes) = device.supported_absolute_axes() {
            for axis in axes.iter() {
                match state.map(|state| evdev::AbsInfo::from(state[axis.0 as usize])) {
                    Some(info) => {
                        let _ = writeln!(
                            out,
                            "    {axis:?}  {}..{}  repos {}  (valeur actuelle {})",
                            info.minimum(),
                            info.maximum(),
                            info.minimum() + (info.maximum() - info.minimum()) / 2,
                            info.value()
                        );
                    }
                    None => {
                        let _ = writeln!(out, "    {axis:?}");
                    }
                }
            }
        }
        if let Some(keys) = device.supported_keys() {
            let buttons: Vec<String> = keys
                .iter()
                .map(|key| format!("{key:?}"))
                .filter(|key| key.starts_with("BTN_"))
                .collect();
            if !buttons.is_empty() {
                let _ = writeln!(out, "    boutons : {}", buttons.join(" "));
            }
        }
    }

    // `| head` ferme le tube : on ignore l'erreur au lieu de paniquer
    use std::io::Write as _;
    let _ = std::io::stdout().write_all(out.as_bytes());
}

fn kind_of(device: &Device) -> PadKind {
    let vendor = device.input_id().vendor();
    let name = device.name().unwrap_or_default().to_lowercase();
    match vendor {
        0x054c => PadKind::Playstation,
        0x045e => PadKind::Xbox,
        0x057e => PadKind::Other,
        _ if name.contains("sony") || name.contains("dualsense") || name.contains("dualshock") => {
            PadKind::Playstation
        }
        _ if name.contains("xbox") || name.contains("x-box") => PadKind::Xbox,
        _ => PadKind::Other,
    }
}
