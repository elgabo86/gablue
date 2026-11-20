//! Clavier virtuel de l'overlay (commandes `kbd` de l'IPC).
//!
//! La table des touches est **dérivée du keymap XKB** de la session : aucune
//! table en dur, les quatre niveaux (base, Maj, AltGr, Maj+AltGr) sont
//! affichés, les accents morts sont marqués, et ch-FR comme ch-DE marchent
//! sans code particulier.
//!
//! La frappe sort par un clavier **uinput** : la surface de l'overlay ne prend
//! jamais le focus clavier (`keyboard_interactivity = None` + région d'entrée
//! vide), donc les touches arrivent dans la fenêtre active — jeu ou champ de
//! saisie — exactement comme un clavier USB.
//!
//! Disposition calquée sur les OSK console (Steam Deck, PS5) : croix tape,
//! carré efface, triangle espace, rond ferme, L1/R1 = curseur de texte,
//! L2 = Maj maintenue (donc L2+L1/R1 = sélection, deux index, pouces libres),
//! R2 = Entrée, L3 = Maj collante, SELECT = AltGr, R3 = Ctrl, START = Tab.

use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

use xkbcommon::xkb;

use crate::{Canvas, argb, argb_p};

/* ------------------------------ géométrie ------------------------------ */

const KEY_W: i32 = 52;
const KEY_H: i32 = 50;
const GAP: i32 = 5;
const PAD: i32 = 14;
const HEADER: i32 = 30;
const FOOTER: i32 = 34;

const FOND_HAUT: u32 = argb_p(0xc4, 0x1b, 0x23, 0x2c);
const FOND_BAS: u32 = argb_p(0xcc, 0x0c, 0x11, 0x17);
const BORD: u32 = argb_p(0x90, 0x3a, 0x47, 0x56);
const CLE: u32 = argb_p(0xa4, 0x25, 0x30, 0x3c);
const CLE_BORD: u32 = argb_p(0x86, 0x35, 0x43, 0x53);
const TXT: u32 = argb(0xff, 0xf2, 0xf6, 0xfa);
const DIM: u32 = argb(0xff, 0x9a, 0xa9, 0xb8);
const ACCENT: u32 = argb(0xff, 0x1a, 0x9f, 0xff);
const SEL: u32 = argb_p(0xf2, 0xf2, 0xf6, 0xfa);
const SEL_TXT: u32 = argb(0xff, 0x0c, 0x11, 0x17);
const SEL_DIM: u32 = argb(0xff, 0x46, 0x55, 0x63);

/// Déplacement au stick droit : mêmes constantes que `gpad-mouse` (zone morte
/// 0,08, 2 → 12 px par frame à 60 Hz), pour que le clavier glisse exactement
/// comme le pointeur.
const ZONE_MORTE: f64 = 0.08;
const VITESSE_MIN: f64 = 2.0;
const VITESSE_MAX: f64 = 12.0;
const REPETE_DELAI: Duration = Duration::from_millis(400);
const REPETE_PERIODE: Duration = Duration::from_millis(80);
const REPETE_NAV: Duration = Duration::from_millis(110);

/* ------------------------------- uinput -------------------------------- */

pub const CTRL: u16 = 29;
pub const SHIFT: u16 = 42;
pub const ALTGR: u16 = 100;
pub const LEFT: u16 = 105;
pub const RIGHT: u16 = 106;
pub const BACKSPACE: u16 = 14;
pub const ENTER: u16 = 28;
pub const SPACE: u16 = 57;
pub const TAB: u16 = 15;

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    sec: i64,
    usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const SYN_REPORT: u16 = 0;

const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

/// Clavier virtuel du noyau (`/dev/uinput`).
pub struct Injection {
    f: File,
    /// modificateurs TENUS (L2 = Maj…) : une frappe composée ne doit jamais
    /// les relâcher, sinon elle annule la tenue en cours.
    tenus: std::cell::RefCell<Vec<u16>>,
}

impl Injection {
    pub fn new() -> Result<Self, String> {
        let f = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map_err(|e| format!("/dev/uinput : {e}"))?;
        let fd = f.as_raw_fd();
        unsafe {
            ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_ulong);
            ioctl(fd, UI_SET_EVBIT, EV_SYN as libc::c_ulong);
            for code in 1..0x2ffu16 {
                ioctl(fd, UI_SET_KEYBIT, code as libc::c_ulong);
            }
            let mut us = UinputSetup {
                id: InputId {
                    bustype: 0x03, // BUS_USB
                    vendor: 1,
                    product: 1,
                    version: 1,
                },
                name: [0; 80],
                ff_effects_max: 0,
            };
            let nom = b"gablue-clavier";
            us.name[..nom.len()].copy_from_slice(nom);
            if ioctl(fd, UI_DEV_SETUP, &us as *const _ as libc::c_ulong) < 0
                || ioctl(fd, UI_DEV_CREATE, 0) < 0
            {
                return Err(format!(
                    "uinput setup : {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        // laisse libinput/KWin voir le périphérique avant la première frappe
        std::thread::sleep(Duration::from_millis(350));
        Ok(Self {
            f,
            tenus: std::cell::RefCell::new(Vec::new()),
        })
    }

    fn emit(&self, type_: u16, code: u16, value: i32) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let ev = InputEvent {
            sec: now.as_secs() as i64,
            usec: now.subsec_micros() as i64,
            type_,
            code,
            value,
        };
        unsafe {
            libc::write(
                self.f.as_raw_fd(),
                &ev as *const _ as *const libc::c_void,
                std::mem::size_of::<InputEvent>(),
            );
        }
    }

    fn syn(&self) {
        self.emit(EV_SYN, SYN_REPORT, 0);
    }

    /// Modificateur tenu physiquement (L2 = Maj).
    pub fn modif(&self, code: u16, enfonce: bool) {
        let mut tenus = self.tenus.borrow_mut();
        if enfonce {
            if !tenus.contains(&code) {
                self.emit(EV_KEY, code, 1);
                self.syn();
                tenus.push(code);
            }
        } else if let Some(i) = tenus.iter().position(|c| *c == code) {
            self.emit(EV_KEY, code, 0);
            self.syn();
            tenus.remove(i);
        }
    }

    /// Relâche tout (fermeture du clavier : on ne laisse jamais une touche
    /// enfoncée derrière soi).
    pub fn tout_relacher(&self) {
        let tenus: Vec<u16> = self.tenus.borrow().clone();
        for m in tenus {
            self.modif(m, false);
        }
    }

    /// Une touche avec modificateurs. Ceux déjà tenus ne sont ni ré-enfoncés
    /// ni relâchés ici.
    pub fn combo(&self, code: u16, mods: &[u16]) {
        let mut ajoutes = Vec::new();
        {
            let mut tenus = self.tenus.borrow_mut();
            for m in mods {
                if !tenus.contains(m) {
                    self.emit(EV_KEY, *m, 1);
                    tenus.push(*m);
                    ajoutes.push(*m);
                }
            }
        }
        self.syn();
        std::thread::sleep(Duration::from_millis(10));
        self.emit(EV_KEY, code, 1);
        self.syn();
        std::thread::sleep(Duration::from_millis(10));
        self.emit(EV_KEY, code, 0);
        if !ajoutes.is_empty() {
            let mut tenus = self.tenus.borrow_mut();
            for m in ajoutes.iter().rev() {
                self.emit(EV_KEY, *m, 0);
                if let Some(i) = tenus.iter().position(|c| c == m) {
                    tenus.remove(i);
                }
            }
        }
        // TOUJOURS après le relâchement : sans lui la touche reste enfoncée
        // pour le noyau et KWin la répète (10 Entrées pour un appui, constaté).
        self.syn();
    }

    pub fn touche(&self, code: u16) {
        self.combo(code, &[]);
    }

    pub fn touche_couches(&self, code: u16, maj: bool, altgr: bool, ctrl: bool) {
        let mut mods = Vec::new();
        if ctrl {
            mods.push(CTRL);
        }
        if maj {
            mods.push(SHIFT);
        }
        if altgr {
            mods.push(ALTGR);
        }
        self.combo(code, &mods);
    }
}

impl Drop for Injection {
    fn drop(&mut self) {
        self.tout_relacher();
        unsafe {
            ioctl(self.f.as_raw_fd(), UI_DEV_DESTROY, 0);
        }
    }
}

unsafe fn ioctl(fd: i32, req: libc::c_ulong, arg: libc::c_ulong) -> i32 {
    unsafe { libc::ioctl(fd, req as _, arg) }
}

/* ------------------------------- keymap -------------------------------- */

#[derive(Clone, Default)]
pub struct Sym {
    pub txt: String,
    pub morte: bool,
}

#[derive(Clone)]
pub struct Touche {
    /// code evdev (= keycode XKB − 8)
    pub code: u16,
    /// niveaux : base, Maj, AltGr, Maj+AltGr
    pub s: [Option<Sym>; 4],
}

/// Un accent mort s'affiche par son signe ; le nom XKB ne dit rien à l'écran.
fn signe_mort(nom: &str) -> Option<&'static str> {
    Some(match nom {
        "dead_grave" => "`",
        "dead_acute" => "´",
        "dead_circumflex" => "^",
        "dead_tilde" => "~",
        "dead_diaeresis" => "¨",
        "dead_ogonek" => "˛",
        "dead_caron" => "ˇ",
        "dead_macron" => "¯",
        "dead_breve" => "˘",
        "dead_abovering" => "˚",
        "dead_doubleacute" => "˝",
        "dead_hook" => "̉",
        "dead_horn" => "̛",
        "dead_stroke" => "̸",
        "dead_belowdot" => "̣",
        "dead_abovedot" => "˙",
        "dead_belowcomma" => "̦",
        "dead_currency" => "¤",
        "dead_greek" => "µ",
        "dead_psili" => "’",
        "dead_dasia" => "‘",
        "dead_voiced_sound" => "゛",
        "dead_semivoiced_sound" => "゜",
        _ => return None,
    })
}

fn lire_sym(keymap: &xkb::Keymap, code: u16, niveau: u32) -> Option<Sym> {
    let kc = xkb::Keycode::new(code as u32 + 8);
    let syms = keymap.key_get_syms_by_level(kc, 0, niveau);
    let sym = *syms.first()?;
    let nom_brut = sym.name().unwrap_or("");
    // xkeysym renvoie « XK_dead_acute »
    let nom = nom_brut.strip_prefix("XK_").unwrap_or(nom_brut);
    if let Some(signe) = signe_mort(nom) {
        return Some(Sym {
            txt: signe.to_string(),
            morte: true,
        });
    }
    if let Some(c) = sym.key_char() {
        if !c.is_control() {
            return Some(Sym {
                txt: c.to_string(),
                morte: false,
            });
        }
    }
    // repli : nom XKB abrégé plutôt que rien
    let court: String = nom.chars().take(4).collect();
    if court.is_empty() {
        None
    } else {
        Some(Sym {
            txt: court,
            morte: false,
        })
    }
}

/// Rangées physiques d'un PC suisse (QWERTZ) en codes evdev.
const RANGEES_EVDEV: [&[u16]; 4] = [
    &[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
    &[16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27],
    &[30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40],
    &[44, 45, 46, 47, 48, 49, 50, 51, 52, 53],
];

/// Disposition de la session : surcharge d'environnement (banc), sinon
/// `kxkbrc` (KDE), sinon le défaut suisse de l'image.
pub fn layout_session() -> (String, String) {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    if let Some(l) = env("GPAD_CLAVIER_LAYOUT") {
        return (l, env("GPAD_CLAVIER_VARIANT").unwrap_or_default());
    }
    if let Some(l) = env("XKB_DEFAULT_LAYOUT") {
        return (l, env("XKB_DEFAULT_VARIANT").unwrap_or_default());
    }
    let kxkbrc = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/kxkbrc"));
    if let Some(p) = kxkbrc {
        if let Ok(txt) = std::fs::read_to_string(p) {
            let mut layout = None;
            let mut variant = None;
            for l in txt.lines() {
                if let Some(v) = l.strip_prefix("LayoutList=") {
                    layout = v.split(',').next().map(|s| s.trim().to_string());
                } else if let Some(v) = l.strip_prefix("VariantList=") {
                    variant = Some(v.split(',').next().unwrap_or("").trim().to_string());
                }
            }
            if let Some(l) = layout {
                return (l, variant.unwrap_or_default());
            }
        }
    }
    ("ch".to_string(), "fr".to_string())
}

/// Charge la table depuis le keymap XKB.
pub fn charger(layout: &str, variant: &str) -> Result<Vec<Vec<Touche>>, String> {
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &ctx,
        "evdev",
        "pc105",
        layout,
        variant,
        None,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or_else(|| format!("keymap {layout}/{variant} introuvable"))?;
    let mut rangees = Vec::new();
    for codes in RANGEES_EVDEV {
        let mut r = Vec::new();
        for &code in codes {
            let kc = xkb::Keycode::new(code as u32 + 8);
            let niveaux = keymap.num_levels_for_key(kc, 0).min(4);
            let mut s: [Option<Sym>; 4] = Default::default();
            for n in 0..niveaux {
                s[n as usize] = lire_sym(&keymap, code, n);
            }
            r.push(Touche { code, s });
        }
        rangees.push(r);
    }
    if rangees.iter().all(|r| r.iter().all(|t| t.s[0].is_none())) {
        return Err(format!("keymap {layout}/{variant} vide"));
    }
    Ok(rangees)
}

/* -------------------------------- état --------------------------------- */

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Genre {
    Physique(u16),
    Maj,
    Altgr,
    Ctrl,
    Espace,
    Retour,
    Tab,
    Entree,
}

pub struct Case {
    pub genre: Genre,
    pub w: i32,
    pub s: [Option<Sym>; 4],
}

struct Repete {
    prochaine: Option<Instant>,
}

impl Repete {
    fn neuf() -> Self {
        Self { prochaine: None }
    }
    fn demarrer(&mut self, t: Instant) {
        self.prochaine = Some(t + REPETE_DELAI);
    }
    fn arreter(&mut self) {
        self.prochaine = None;
    }
    fn echeance(&self) -> bool {
        matches!(self.prochaine, Some(t) if Instant::now() >= t)
    }
    fn relancer(&mut self, t: Instant, periode: Duration) {
        self.prochaine = Some(t + periode);
    }
    fn proche(&self) -> bool {
        self.prochaine.is_some()
    }
}

pub struct Etat {
    pub rangees: Vec<Vec<Case>>,
    pub r: usize,
    pub c: usize,
    pub maj: bool,
    pub altgr: bool,
    pub ctrl: bool,
    /// Maj maintenue (L2) : n'affecte que l'affichage, la touche est déjà
    /// enfoncée dans le clavier virtuel.
    pub maj_tenue: bool,
    pub x: i32,
    pub y: i32,
    pub largeur: i32,
    pub hauteur: i32,
    pub titre: String,
}

impl Etat {
    pub fn neuf(rangees: Vec<Vec<Touche>>, layout: &str, variant: &str, ecran: (i32, i32)) -> Self {
        let mut r: Vec<Vec<Case>> = rangees
            .into_iter()
            .map(|rangee| {
                rangee
                    .into_iter()
                    .map(|t| Case {
                        genre: Genre::Physique(t.code),
                        w: KEY_W,
                        s: t.s,
                    })
                    .collect()
            })
            .collect();
        // ÉCHAP n'est pas ici : Rond ferme, le clavier n'en a pas besoin.
        let speciales: &[(Genre, i32)] = &[
            (Genre::Maj, 70),
            (Genre::Altgr, 70),
            (Genre::Ctrl, 70),
            (Genre::Espace, 150),
            (Genre::Retour, 92),
            (Genre::Tab, 70),
            (Genre::Entree, 94),
        ];
        r.push(
            speciales
                .iter()
                .map(|(g, w)| Case {
                    genre: *g,
                    w: *w,
                    s: Default::default(),
                })
                .collect(),
        );

        let besoin = r
            .iter()
            .map(|rangee| rangee.iter().map(|c| c.w).sum::<i32>() + GAP * (rangee.len() as i32 - 1))
            .max()
            .unwrap_or(0);
        let largeur = besoin + PAD * 2;
        let hauteur = PAD * 2 + HEADER + 4 * KEY_H + 3 * GAP + GAP + KEY_H + GAP + FOOTER;
        let titre = if variant.is_empty() {
            format!("CLAVIER {}", layout.to_uppercase())
        } else {
            format!("CLAVIER {}-{}", layout.to_uppercase(), variant.to_uppercase())
        };
        let mut etat = Self {
            rangees: r,
            r: 0,
            c: 0,
            maj: false,
            altgr: false,
            ctrl: false,
            maj_tenue: false,
            x: (ecran.0 - largeur) / 2,
            y: ecran.1 - hauteur - 80,
            largeur,
            hauteur,
            titre,
        };
        etat.borner(ecran);
        etat
    }

    pub fn borner(&mut self, ecran: (i32, i32)) {
        self.x = self.x.clamp(8, (ecran.0 - self.largeur - 8).max(8));
        self.y = self.y.clamp(8, (ecran.1 - self.hauteur - 8).max(8));
    }

    pub fn deplacer(&mut self, dx: i32, dy: i32, ecran: (i32, i32)) {
        self.x += dx;
        self.y += dy;
        self.borner(ecran);
    }

    fn case_courante(&self) -> &Case {
        &self.rangees[self.r][self.c]
    }

    fn niveau(&self) -> usize {
        match (self.maj || self.maj_tenue, self.altgr) {
            (true, true) => 3,
            (false, true) => 2,
            (true, false) => 1,
            (false, false) => 0,
        }
    }

    fn naviguer(&mut self, dx: i32, dy: i32) {
        if dy != 0 {
            let r = self.r as i32 + dy;
            if r >= 0 && (r as usize) < self.rangees.len() {
                self.r = r as usize;
                self.c = self.c.min(self.rangees[self.r].len() - 1);
            }
        }
        if dx != 0 {
            let n = self.rangees[self.r].len() as i32;
            self.c = (self.c as i32 + dx).clamp(0, n - 1) as usize;
        }
    }

    fn libelle_actif(&self, k: &Case) -> String {
        k.s[self.niveau()]
            .as_ref()
            .or(k.s[0].as_ref())
            .map(|s| s.txt.clone())
            .unwrap_or_default()
    }

    /// Position (x, y) et taille du panneau, pour la persistance.
    pub fn panneau(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.largeur, self.hauteur)
    }

    pub fn dessiner(&self, cv: &mut Canvas) {
        let (px, py) = (self.x, self.y);
        let (lw, lh) = (self.largeur, self.hauteur);

        // cadre + fond translucide
        cv.rrect(px, py, lw, lh, 16, BORD);
        cv.rrect_v(px + 1, py + 1, lw - 2, lh - 2, 15, FOND_HAUT, FOND_BAS);

        // en-tête : disposition + pastilles des modificateurs
        cv.text_brut(px + PAD, py + PAD - 4, &self.titre, 2, TXT);
        let mut pill_x = px + lw - PAD;
        for (actif, nom) in [
            (self.ctrl, "CTRL"),
            (self.altgr, "ALTGR"),
            (self.maj || self.maj_tenue, "MAJ"),
        ] {
            let w = cv.largeur(nom, 1) + 16;
            pill_x -= w;
            let fond = if actif { ACCENT } else { argb_p(0x90, 0x2c, 0x38, 0x46) };
            let txt = if actif { SEL_TXT } else { DIM };
            cv.rrect(pill_x, py + PAD - 6, w, 22, 11, fond);
            let lw_txt = cv.largeur(nom, 1);
            cv.text_brut(pill_x + (w - lw_txt) / 2, py + PAD - 2, nom, 1, txt);
            pill_x -= 6;
        }

        // touches
        let y0 = py + PAD + HEADER;
        for (ir, rangee) in self.rangees.iter().enumerate() {
            let total: i32 =
                rangee.iter().map(|c| c.w).sum::<i32>() + GAP * (rangee.len() as i32 - 1);
            let mut kx = px + (lw - total) / 2;
            let ky = y0 + ir as i32 * (KEY_H + GAP)
                + if ir == self.rangees.len() - 1 { GAP } else { 0 };
            for (ic, k) in rangee.iter().enumerate() {
                self.dessiner_case(cv, k, kx, ky, ir == self.r && ic == self.c);
                kx += k.w + GAP;
            }
        }

        // pied : les gestes, deux lignes (découvrabilité sans doc)
        let ligne1 = "X TAPER   CARRE EFFACER   TRIANGLE ESPACE   ROND FERMER   R2 ENTREE   START TAB   D-PAD/STICK G: TOUCHES";
        let ligne2 = "L1/R1 CURSEUR TEXTE   L2 MAJ (L2+L1/R1: SELECTION)   L3 MAJ COLLEE   SELECT ALTGR   R3 CTRL   STICK D: DEPLACER";
        let y_pied = py + lh - FOOTER + 1;
        cv.text_brut(px + PAD, y_pied, ligne1, 1, DIM);
        cv.text_brut(px + PAD, y_pied + 13, ligne2, 1, DIM);
    }

    fn dessiner_case(&self, cv: &mut Canvas, k: &Case, x: i32, y: i32, sel: bool) {
        let (w, h) = (k.w, KEY_H);
        let actif = match k.genre {
            Genre::Maj => self.maj || self.maj_tenue,
            Genre::Altgr => self.altgr,
            Genre::Ctrl => self.ctrl,
            _ => false,
        };
        cv.rrect(x, y, w, h, 10, if sel { SEL } else { CLE_BORD });
        cv.rrect(
            x + 1,
            y + 1,
            w - 2,
            h - 2,
            9,
            if sel {
                SEL
            } else if actif {
                ACCENT
            } else {
                CLE
            },
        );
        let (c_txt, c_dim) = if sel { (SEL_TXT, SEL_DIM) } else { (TXT, DIM) };

        match k.genre {
            Genre::Physique(_) => {
                let txt = self.libelle_actif(k);
                let morte = k.s[self.niveau()].as_ref().map(|s| s.morte).unwrap_or(false);
                let scale = if txt.chars().count() > 2 { 1 } else { 2 };
                let lw = cv.largeur(&txt, scale);
                let avec_sec = k.s.iter().skip(1).any(|s| s.is_some());
                let yy = if avec_sec {
                    y + 8
                } else {
                    y + (h - 7 * scale) / 2
                };
                cv.text_brut(x + (w - lw) / 2, yy, &txt, scale, c_txt);
                if morte {
                    cv.rect(
                        x + (w + lw) / 2 + 2,
                        yy + 1,
                        3,
                        3,
                        if sel { SEL_TXT } else { ACCENT },
                    );
                }
                // couches secondaires : Maj en haut à droite, AltGr en bas à
                // droite, Maj+AltGr en bas à gauche
                let sec = |n: usize| k.s[n].as_ref().map(|s| s.txt.clone()).unwrap_or_default();
                let (haut, bas_d, bas_g) = (sec(1), sec(2), sec(3));
                if !haut.is_empty() {
                    let w2 = cv.largeur(&haut, 1);
                    cv.text_brut(x + w - w2 - 5, y + 3, &haut, 1, c_dim);
                }
                if !bas_g.is_empty() {
                    cv.text_brut(x + 5, y + h - 13, &bas_g, 1, c_dim);
                }
                if !bas_d.is_empty() {
                    let w2 = cv.largeur(&bas_d, 1);
                    cv.text_brut(x + w - w2 - 5, y + h - 13, &bas_d, 1, c_dim);
                }
            }
            _ => {
                let nom = match k.genre {
                    Genre::Maj => "MAJ",
                    Genre::Altgr => "ALTGR",
                    Genre::Ctrl => "CTRL",
                    Genre::Espace => "ESPACE",
                    Genre::Retour => "RETOUR",
                    Genre::Tab => "TAB",
                    Genre::Entree => "ENTREE",
                    Genre::Physique(_) => "",
                };
                let c = if actif { SEL_TXT } else { c_txt };
                let scale = if cv.largeur(nom, 2) <= w - 8 { 2 } else { 1 };
                let lw = cv.largeur(nom, scale);
                cv.text_brut(x + (w - lw) / 2, y + (h - 7 * scale) / 2, nom, scale, c);
            }
        }
    }
}

/* ------------------------------- clavier ------------------------------- */

/// Courbe d'accélération du pointeur (**copie** de `gpad-mouse` :
/// `apply_easing_quadratic_curve`). Le clavier doit glisser exactement comme
/// la souris, d'où la même courbe plutôt qu'une approximation.
fn courbe_pointeur(v: f64) -> f64 {
    let abs = v.abs();
    let signe = if v > 0.0 { 1.0 } else { -1.0 };
    let mult = if abs <= 0.35 {
        0.2
    } else if abs <= 0.75 {
        let t = (abs - 0.35) / 0.4;
        0.2 + 0.5 * t
    } else {
        let t = (abs - 0.75) / 0.25;
        0.7 + 0.3 * t * t
    };
    signe * mult * abs
}

pub struct Clavier {
    pub etat: Etat,
    pub inj: Option<Injection>,
    ecran: (i32, i32),
    carry: (f64, f64),
    dernier_mouvement: Instant,
    nav_hat: (i32, i32),
    stick_dir: Option<(i32, i32)>,
    /// derniers axes normalisés des deux sticks (le noyau envoie X et Y
    /// séparément : il faut garder l'autre pour calculer une direction)
    lx: f64,
    ly: f64,
    rx: f64,
    ry: f64,
    // touches tenues (répétition)
    croix: bool,
    carre: bool,
    triangle: bool,
    l1: bool,
    r1: bool,
    rep_nav: Repete,
    rep_stick: Repete,
    rep_croix: Repete,
    rep_carre: Repete,
    rep_triangle: Repete,
    rep_l1: Repete,
    rep_r1: Repete,
}

impl Clavier {
    pub fn neuf(rangees: Vec<Vec<Touche>>, layout: &str, variant: &str, ecran: (i32, i32)) -> Self {
        Self {
            etat: Etat::neuf(rangees, layout, variant, ecran),
            inj: None,
            ecran,
            carry: (0.0, 0.0),
            dernier_mouvement: Instant::now(),
            nav_hat: (0, 0),
            stick_dir: None,
            lx: 0.0,
            ly: 0.0,
            rx: 0.0,
            ry: 0.0,
            croix: false,
            carre: false,
            triangle: false,
            l1: false,
            r1: false,
            rep_nav: Repete::neuf(),
            rep_stick: Repete::neuf(),
            rep_croix: Repete::neuf(),
            rep_carre: Repete::neuf(),
            rep_triangle: Repete::neuf(),
            rep_l1: Repete::neuf(),
            rep_r1: Repete::neuf(),
        }
    }

    /// Crée le clavier noyau au premier usage (350 ms d'enregistrement).
    pub fn preparer_injection(&mut self) {
        if self.inj.is_none() {
            match Injection::new() {
                Ok(i) => self.inj = Some(i),
                Err(e) => eprintln!("clavier : {e} (frappe indisponible)"),
            }
        }
    }

    pub fn ecran(&mut self, ecran: (i32, i32)) {
        if self.ecran != ecran {
            self.ecran = ecran;
            self.etat.borner(ecran);
        }
    }

    pub fn position(&self) -> (i32, i32) {
        (self.etat.x, self.etat.y)
    }

    /// Rectangle occupé par le panneau (x, y, largeur, hauteur).
    pub fn panneau(&self) -> (i32, i32, i32, i32) {
        self.etat.panneau()
    }

    pub fn placer(&mut self, x: i32, y: i32) {
        self.etat.x = x;
        self.etat.y = y;
        self.etat.borner(self.ecran);
    }

    pub fn dessiner(&self, cv: &mut Canvas) {
        self.etat.dessiner(cv);
    }

    fn taper(&mut self) {
        let genre = self.etat.case_courante().genre;
        let (maj, altgr, ctrl) = (self.etat.maj, self.etat.altgr, self.etat.ctrl);
        match genre {
            Genre::Physique(code) => {
                if let Some(inj) = &self.inj {
                    inj.touche_couches(code, maj, altgr, ctrl);
                }
            }
            Genre::Maj => self.etat.maj = !self.etat.maj,
            Genre::Altgr => self.etat.altgr = !self.etat.altgr,
            Genre::Ctrl => self.etat.ctrl = !self.etat.ctrl,
            autre => self.speciale(autre),
        }
    }

    fn speciale(&mut self, genre: Genre) {
        let Some(inj) = &self.inj else { return };
        let code = match genre {
            Genre::Espace => SPACE,
            Genre::Retour => BACKSPACE,
            Genre::Tab => TAB,
            Genre::Entree => ENTER,
            _ => return,
        };
        inj.touche(code);
    }

    /// L1/R1 : curseur de texte (Ctrl collant = mot par mot ; L2 tenue ajoute
    /// Maj côté noyau, donc sélection — deux index, aucun pouce déplacé).
    fn curseur(&mut self, sens: i32) {
        let Some(inj) = &self.inj else { return };
        let code = if sens < 0 { LEFT } else { RIGHT };
        let mods: Vec<u16> = if self.etat.ctrl { vec![CTRL] } else { Vec::new() };
        inj.combo(code, &mods);
    }

    /// Une touche de la manette. `true` = le clavier demande sa fermeture.
    pub fn touche(&mut self, code: u16, presse: bool) -> bool {
        let maintenant = Instant::now();
        if !presse {
            match code {
                crate::BTN_SOUTH => {
                    self.croix = false;
                    self.rep_croix.arreter();
                }
                crate::BTN_WEST => {
                    self.carre = false;
                    self.rep_carre.arreter();
                }
                crate::BTN_NORTH => {
                    self.triangle = false;
                    self.rep_triangle.arreter();
                }
                crate::BTN_TL => {
                    self.l1 = false;
                    self.rep_l1.arreter();
                }
                crate::BTN_TR => {
                    self.r1 = false;
                    self.rep_r1.arreter();
                }
                crate::BTN_TL2 => {
                    if let Some(inj) = &self.inj {
                        inj.modif(SHIFT, false);
                    }
                    self.etat.maj_tenue = false;
                }
                _ => {}
            }
            return false;
        }
        match code {
            crate::BTN_SOUTH => {
                self.taper();
                self.croix = true;
                self.rep_croix.demarrer(maintenant);
            }
            crate::BTN_WEST => {
                self.speciale(Genre::Retour);
                self.carre = true;
                self.rep_carre.demarrer(maintenant);
            }
            crate::BTN_NORTH => {
                self.speciale(Genre::Espace);
                self.triangle = true;
                self.rep_triangle.demarrer(maintenant);
            }
            crate::BTN_EAST => return true,
            crate::BTN_TL => {
                self.curseur(-1);
                self.l1 = true;
                self.rep_l1.demarrer(maintenant);
            }
            crate::BTN_TR => {
                self.curseur(1);
                self.r1 = true;
                self.rep_r1.demarrer(maintenant);
            }
            crate::BTN_TL2 => {
                if let Some(inj) = &self.inj {
                    inj.modif(SHIFT, true);
                }
                self.etat.maj_tenue = true;
            }
            crate::BTN_TR2 => self.speciale(Genre::Entree),
            crate::BTN_THUMBL => self.etat.maj = !self.etat.maj,
            crate::BTN_THUMBR => self.etat.ctrl = !self.etat.ctrl,
            crate::BTN_SELECT => self.etat.altgr = !self.etat.altgr,
            crate::BTN_START => self.speciale(Genre::Tab),
            _ => {}
        }
        false
    }

    /// D-pad : sélection entre les touches. `axe` 0 = X, 1 = Y (le noyau envoie
    /// les deux axes séparément, il faut garder l'autre).
    pub fn hat(&mut self, axe: u8, v: i32) -> bool {
        let maintenant = Instant::now();
        let avant = self.nav_hat;
        if axe == 0 {
            self.nav_hat.0 = v;
        } else {
            self.nav_hat.1 = v;
        }
        let dir = match self.nav_hat {
            (x, _) if x != 0 => Some((if x < 0 { -1 } else { 1 }, 0)),
            (_, y) if y != 0 => Some((0, if y < 0 { -1 } else { 1 })),
            _ => None,
        };
        if self.nav_hat == avant {
            return false;
        }
        match dir {
            Some((dx, dy)) => {
                self.etat.naviguer(dx, dy);
                self.rep_nav.demarrer(maintenant);
                true
            }
            None => {
                self.rep_nav.arreter();
                false
            }
        }
    }

    /// Stick gauche : sélection entre les touches. `axe` 0 = X, 1 = Y, valeur
    /// normalisée (−1..1).
    pub fn stick_gauche(&mut self, axe: u8, v: f64) -> bool {
        if axe == 0 {
            self.lx = v;
        } else {
            self.ly = v;
        }
        let (nx, ny) = (self.lx, self.ly);
        // hystérésis : on arme à 0,55 et on réarme sous 0,35, sinon un stick
        // qui flotte autour du seuil saute de touche en touche
        let seuil = if self.stick_dir.is_some() { 0.35 } else { 0.55 };
        let dir = if nx.abs().max(ny.abs()) < seuil {
            None
        } else if nx.abs() > ny.abs() {
            Some((if nx < 0.0 { -1 } else { 1 }, 0))
        } else {
            Some((0, if ny < 0.0 { -1 } else { 1 }))
        };
        if dir == self.stick_dir {
            return false;
        }
        self.stick_dir = dir;
        match dir {
            Some((dx, dy)) => {
                self.etat.naviguer(dx, dy);
                self.rep_stick.demarrer(Instant::now());
                true
            }
            None => {
                self.rep_stick.arreter();
                false
            }
        }
    }

    /// Stick droit : déplacement du panneau, courbe `gpad-mouse`. `axe` 0 = X,
    /// 1 = Y. `true` si le panneau a bougé (il faut redessiner).
    pub fn stick_droit(&mut self, axe: u8, v: f64) -> bool {
        if axe == 0 {
            self.rx = v;
        } else {
            self.ry = v;
        }
        let (nx, ny) = (self.rx, self.ry);
        let mag = (nx * nx + ny * ny).sqrt();
        if mag < ZONE_MORTE {
            self.carry = (0.0, 0.0);
            self.dernier_mouvement = Instant::now();
            return false;
        }
        let maintenant = Instant::now();
        let mut dt = (maintenant - self.dernier_mouvement).as_secs_f64();
        self.dernier_mouvement = maintenant;
        if dt > 0.1 {
            dt = 0.1;
        }
        let dir = (nx / mag, ny / mag);
        // bornes : une valeur hors plage ne doit pas produire une vitesse
        // absurde (la courbe est quadratique au-delà de 0,75)
        let m = ((mag - ZONE_MORTE) / (1.0 - ZONE_MORTE)).clamp(0.0, 1.0);
        let vitesse = VITESSE_MIN + (VITESSE_MAX - VITESSE_MIN) * courbe_pointeur(m).abs();
        self.carry.0 += dir.0 * vitesse * dt * 60.0;
        self.carry.1 += dir.1 * vitesse * dt * 60.0;
        let ix = self.carry.0.round() as i32;
        let iy = self.carry.1.round() as i32;
        if ix == 0 && iy == 0 {
            return false;
        }
        self.carry.0 -= ix as f64;
        self.carry.1 -= iy as f64;
        self.etat.deplacer(ix, iy, self.ecran);
        true
    }

    /// Répétitions (une fois par tour de boucle). `true` = redessiner.
    pub fn tick(&mut self) -> bool {
        let maintenant = Instant::now();
        let mut change = false;
        if self.rep_nav.echeance() {
            let dir = match self.nav_hat {
                (-1, 0) => Some((-1, 0)),
                (1, 0) => Some((1, 0)),
                (0, -1) => Some((0, -1)),
                (0, 1) => Some((0, 1)),
                _ => None,
            };
            match dir {
                Some((dx, dy)) => {
                    self.etat.naviguer(dx, dy);
                    self.rep_nav.relancer(maintenant, REPETE_NAV);
                    change = true;
                }
                None => self.rep_nav.arreter(),
            }
        }
        if self.rep_stick.echeance() {
            match self.stick_dir {
                Some((dx, dy)) => {
                    self.etat.naviguer(dx, dy);
                    self.rep_stick.relancer(maintenant, REPETE_NAV);
                    change = true;
                }
                None => self.rep_stick.arreter(),
            }
        }
        if self.rep_croix.echeance() {
            self.taper();
            self.rep_croix.relancer(maintenant, REPETE_PERIODE);
            change = true;
        }
        if self.rep_carre.echeance() {
            self.speciale(Genre::Retour);
            self.rep_carre.relancer(maintenant, REPETE_PERIODE);
        }
        if self.rep_triangle.echeance() {
            self.speciale(Genre::Espace);
            self.rep_triangle.relancer(maintenant, REPETE_PERIODE);
        }
        if self.rep_l1.echeance() {
            self.curseur(-1);
            self.rep_l1.relancer(maintenant, REPETE_PERIODE);
        }
        if self.rep_r1.echeance() {
            self.curseur(1);
            self.rep_r1.relancer(maintenant, REPETE_PERIODE);
        }
        change
    }

    /// Une répétition est-elle armée (le `poll` doit alors descendre à 8 ms) ?
    pub fn anime(&self) -> bool {
        self.rep_nav.proche()
            || self.rep_stick.proche()
            || self.rep_croix.proche()
            || self.rep_carre.proche()
            || self.rep_triangle.proche()
            || self.rep_l1.proche()
            || self.rep_r1.proche()
    }

    /// Fermeture : relâche les touches encore tenues.
    pub fn relacher(&mut self) {
        if let Some(inj) = &self.inj {
            inj.tout_relacher();
        }
        self.etat.maj_tenue = false;
        self.croix = false;
        self.carre = false;
        self.triangle = false;
        self.l1 = false;
        self.r1 = false;
        for r in [
            &mut self.rep_nav,
            &mut self.rep_stick,
            &mut self.rep_croix,
            &mut self.rep_carre,
            &mut self.rep_triangle,
            &mut self.rep_l1,
            &mut self.rep_r1,
        ] {
            r.arreter();
        }
    }
}
