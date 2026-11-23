// Prototype d'overlay manette pour Gablue — banc de faisabilité.
//
// Ce que le banc démontre :
//   1. une surface zwlr_layer_shell_v1 (layer overlay) s'affiche par-dessus
//      toutes les fenêtres, sans voler le focus clavier (interactivité
//      clavier = none) et en laissant passer la souris (région d'entrée vide) ;
//   2. pendant qu'elle est ouverte, la manette est grabée en evdev
//      (EVIOCGRAB) : Plasma, les jeux et SDL/evdev ne reçoivent plus rien ;
//   3. l'overlay se pilote uniquement avec la manette, qu'il lit lui-même.
//
// Zéro toolkit graphique : wayland-client brut + shm + police bitmap 5x7
// embarquée. Rendu en double buffer (jamais de dessin dans un buffer encore
// affiché : c'était la cause du scintillement).
//
// Usage : gpad [--grab /dev/input/eventN] [--seconds N]

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::sync::{Arc, Mutex};
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_region, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

mod audio;
mod capture;
mod clavier;

/* ============================ police 5x7 ============================ */

/// Chaque glyphe = 5 colonnes de 7 bits (bit 0 = haut).
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [0x7e, 0x11, 0x11, 0x11, 0x7e],
        'B' => [0x7f, 0x49, 0x49, 0x49, 0x36],
        'C' => [0x3e, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7f, 0x41, 0x41, 0x22, 0x1c],
        'E' => [0x7f, 0x49, 0x49, 0x49, 0x41],
        'F' => [0x7f, 0x09, 0x09, 0x01, 0x01],
        'G' => [0x3e, 0x41, 0x49, 0x49, 0x7a],
        'H' => [0x7f, 0x08, 0x08, 0x08, 0x7f],
        'I' => [0x00, 0x41, 0x7f, 0x41, 0x00],
        'J' => [0x20, 0x40, 0x41, 0x3f, 0x01],
        'K' => [0x7f, 0x08, 0x14, 0x22, 0x41],
        'L' => [0x7f, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7f, 0x02, 0x0c, 0x02, 0x7f],
        'N' => [0x7f, 0x04, 0x08, 0x10, 0x7f],
        'O' => [0x3e, 0x41, 0x41, 0x41, 0x3e],
        'P' => [0x7f, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3e, 0x41, 0x51, 0x21, 0x5e],
        'R' => [0x7f, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7f, 0x01, 0x01],
        'U' => [0x3f, 0x40, 0x40, 0x40, 0x3f],
        'V' => [0x1f, 0x20, 0x40, 0x20, 0x1f],
        'W' => [0x3f, 0x40, 0x38, 0x40, 0x3f],
        'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
        'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        '0' => [0x3e, 0x51, 0x49, 0x45, 0x3e],
        '1' => [0x00, 0x42, 0x7f, 0x40, 0x00],
        '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        '3' => [0x21, 0x41, 0x45, 0x4b, 0x31],
        '4' => [0x18, 0x14, 0x12, 0x7f, 0x10],
        '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        '6' => [0x3c, 0x4a, 0x49, 0x49, 0x30],
        '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        '9' => [0x06, 0x49, 0x49, 0x29, 0x1e],
        '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        '/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        '>' => [0x41, 0x22, 0x14, 0x08, 0x00],
        '(' => [0x00, 0x1c, 0x22, 0x41, 0x00],
        ')' => [0x00, 0x41, 0x22, 0x1c, 0x00],
        '*' => [0x14, 0x08, 0x3e, 0x08, 0x14],
        '+' => [0x08, 0x08, 0x3e, 0x08, 0x08],
        '%' => [0x62, 0x64, 0x08, 0x13, 0x23],
        '?' => [0x02, 0x01, 0x59, 0x09, 0x06],
        '<' => [0x00, 0x08, 0x14, 0x22, 0x41],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00], // espace
    }
}

struct Canvas<'a> {
    px: &'a mut [u32],
    w: usize,
    h: usize,
    /// police vectorielle du style MODERNE ; `None` = bitmap 5×7 (RETRO)
    police: Option<&'a fontdue::Font>,
    /// décalage horizontal de TOUT le dessin : sert à faire glisser le
    /// contenu pendant un changement de page (l'en-tête reste immobile)
    ox: i32,
    /// zone de dessin (x0, y0, x1, y1) : le contenu qui glisse ne débordera
    /// ni sur l'en-tête, ni sur le pied, ni hors du panneau
    clip: Option<(i32, i32, i32, i32)>,
}

impl<'a> Canvas<'a> {
    /// Le pixel est-il dans la zone de découpe ? (les bornes du buffer sont
    /// déjà vérifiées par les boucles appelantes)
    fn ok(&self, x: i32, y: i32) -> bool {
        match self.clip {
            Some((x0, y0, x1, y1)) => x >= x0 && y >= y0 && x < x1 && y < y1,
            None => true,
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x = x + self.ox;
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                if self.clip.is_some() && !self.ok(xx, yy) {
                    continue;
                }
                self.px[yy as usize * self.w + xx as usize] = color;
            }
        }
    }

    /// Remplissage vertical en dégradé (deux couleurs ARGB de même alpha).
    fn rect_v(&mut self, x: i32, y: i32, w: i32, h: i32, haut: u32, bas: u32) {
        if h <= 0 {
            return;
        }
        for yy in 0..h {
            let t = if h > 1 { yy as f32 / (h - 1) as f32 } else { 0.0 };
            let mel = |a: u32, b: u32| -> u32 {
                (a as f32 + (b as f32 - a as f32) * t).round() as u32
            };
            let c = argb(
                mel((haut >> 24) & 0xff, (bas >> 24) & 0xff),
                mel((haut >> 16) & 0xff, (bas >> 16) & 0xff),
                mel((haut >> 8) & 0xff, (bas >> 8) & 0xff),
                mel(haut & 0xff, bas & 0xff),
            );
            self.rect(x, y + yy, w, 1, c);
        }
    }

    /// Rectangle à coins arrondis qui **n'efface rien** : les pixels hors du
    /// quart de cercle restent tels quels. À utiliser pour tout ce qui se pose
    /// SUR le panneau (sillon et poignée du curseur, interrupteur, cadre de la
    /// boîte de choix). Le cadre **extérieur** du panneau, lui, utilise
    /// `coins_fond()`, qui évide les coins vers la couleur du dessous.
    fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x = x + self.ox;
        let r = r.min(w / 2).min(h / 2);
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            // distance au bord arrondi le plus proche (0 dans la bande pleine)
            let dy = if yy < y + r {
                y + r - yy
            } else if yy > y + h - 1 - r {
                yy - (y + h - 1 - r)
            } else {
                0
            };
            let recul = if dy > 0 {
                r - (((r * r - dy * dy) as f64).sqrt()) as i32
            } else {
                0
            };
            for xx in (x + recul).max(0)..(x + w - recul).min(self.w as i32) {
                if self.clip.is_some() && !self.ok(xx, yy) {
                    continue;
                }
                self.px[yy as usize * self.w + xx as usize] = color;
            }
        }
    }

    /// Idem en dégradé vertical. C'est ce qu'il faut pour une **sélection** :
    /// `rect_v` + `coins()` évide les coins (transparent), donc sous un voile on
    /// voit la fenêtre derrière — une pastille « ronde » qui redevient un
    /// rectangle dès qu'une fenêtre blanche passe derrière le menu système.
    fn rrect_v(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, haut: u32, bas: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x = x + self.ox;
        let r = r.min(w / 2).min(h / 2);
        let interp = |t: f32, a: u32, b: u32| -> u32 {
            let f = |c: u32| (c & 0xff) as f32;
            let (aa, ab) = ((a >> 24) & 0xff, (b >> 24) & 0xff);
            let can = (aa as f32 + (ab as f32 - aa as f32) * t) as u32;
            let cr = (f(a >> 16) + (f(b >> 16) - f(a >> 16)) * t) as u32;
            let cg = (f(a >> 8) + (f(b >> 8) - f(a >> 8)) * t) as u32;
            let cb = (f(a) + (f(b) - f(a)) * t) as u32;
            argb(can.min(255), cr.min(255), cg.min(255), cb.min(255))
        };
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            let dy = if yy < y + r {
                y + r - yy
            } else if yy > y + h - 1 - r {
                yy - (y + h - 1 - r)
            } else {
                0
            };
            let recul = if dy > 0 {
                r - (((r * r - dy * dy) as f64).sqrt()) as i32
            } else {
                0
            };
            let t = (yy - y) as f32 / (h - 1).max(1) as f32;
            let c = interp(t, haut, bas);
            for xx in (x + recul).max(0)..(x + w - recul).min(self.w as i32) {
                if self.clip.is_some() && !self.ok(xx, yy) {
                    continue;
                }
                self.px[yy as usize * self.w + xx as usize] = c;
            }
        }
    }

    /// Comme `coins()`, mais les pixels hors du quart de cercle prennent
    /// `fond` au lieu de devenir transparents. Indispensable dès qu'un cadre
    /// arrondi est posé **sur un voile** : effacer à zéro y percerait un trou
    /// jusqu'au bureau (quatre encoches claires aux coins, constaté à l'usage).
    fn coins_fond(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, fond: u32) {
        if r <= 0 {
            return;
        }
        let mut poser = |cx: i32, cy: i32| {
            if cx >= 0 && cy >= 0 && cx < self.w as i32 && cy < self.h as i32 && self.ok(cx, cy) {
                self.px[cy as usize * self.w + cx as usize] = fond;
            }
        };
        for dy in 0..r {
            for dx in 0..r {
                let (ddx, ddy) = (r - dx, r - dy);
                if ddx * ddx + ddy * ddy > r * r {
                    poser(x + dx, y + dy);
                    poser(x + w - 1 - dx, y + dy);
                    poser(x + dx, y + h - 1 - dy);
                    poser(x + w - 1 - dx, y + h - 1 - dy);
                }
            }
        }
    }

    /// Filet horizontal qui s'efface vers la droite (séparateurs, en-tête).
    /// Même filet, mais il s'efface vers la **gauche** (symétrique du titre
    /// centré).
    fn filet_fondu_inv(&mut self, x: i32, y: i32, w: i32, a_max: u32, c: u32) {
        if w <= 0 {
            return;
        }
        let (r, g, b) = (
            ((c >> 16) & 0xff) as f32,
            ((c >> 8) & 0xff) as f32,
            (c & 0xff) as f32,
        );
        // 2 px d'épaisseur : à 1 px le filet disparaissait sur le papier peint
        // (constaté « pas assez visible »)
        for dy in 0..2 {
            for i in 0..w {
                let a = a_max * (i as u32 + 1) / w as u32;
                self.melanger_a(x + i, y + dy, r, g, b, a as f32 / 255.0);
            }
        }
    }

    /// Filet horizontal qui s'efface vers la droite. Il **se mélange** au pixel
    /// existant (`melanger_a`) au lieu de l'écraser : écrire un pixel translucide
    /// remplace le pixel du panneau, donc le trait trouait l'opacité et laissait
    /// passer le fond d'écran (rendu « ligne totalement transparente » en
    /// RÉGLAGES, teintée par le papier peint).
    fn filet_fondu(&mut self, x: i32, y: i32, w: i32, a_max: u32, c: u32) {
        if w <= 0 {
            return;
        }
        let (r, g, b) = (
            ((c >> 16) & 0xff) as f32,
            ((c >> 8) & 0xff) as f32,
            (c & 0xff) as f32,
        );
        for dy in 0..2 {
            for i in 0..w {
                let a = a_max - (a_max * i as u32) / w as u32;
                self.melanger_a(x + i, y + dy, r, g, b, a as f32 / 255.0);
            }
        }
    }

    /// Blit d'une icône RGBA réduite à `taille` (moyenne de boîte, alpha
    /// mélangé au fond). Pas de lissage bilinéaire : le panneau assume le
    /// rendu net.
    fn icone(&mut self, x: i32, y: i32, taille: i32, ic: &Icone) {
        self.image(x, y, taille, taille, ic);
    }

    /// Idem, à une taille libre : le logo d'un jeu garde son ratio (le caller
    /// calcule `tw`/`th`), une icône carrée passe par [`Canvas::icone`].
    fn image(&mut self, x: i32, y: i32, tw: i32, th: i32, ic: &Icone) {
        if ic.w <= 0 || ic.h <= 0 || tw <= 0 || th <= 0 {
            return;
        }
        for dy in 0..th {
            let sy0 = dy * ic.h / th;
            let sy1 = ((dy + 1) * ic.h / th).max(sy0 + 1).min(ic.h);
            for dx in 0..tw {
                let sx0 = dx * ic.w / tw;
                let sx1 = ((dx + 1) * ic.w / tw).max(sx0 + 1).min(ic.w);
                let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
                for sy in sy0..sy1 {
                    for sx in sx0..sx1 {
                        let i = ((sy * ic.w + sx) * 4) as usize;
                        let al = ic.px[i + 3] as u32;
                        r += ic.px[i] as u32 * al;
                        g += ic.px[i + 1] as u32 * al;
                        b += ic.px[i + 2] as u32 * al;
                        a += al;
                        n += 1;
                    }
                }
                if a == 0 || n == 0 {
                    continue;
                }
                self.melanger(
                    x + dx,
                    y + dy,
                    (a / n) as u8,
                    (r / a) as u8,
                    (g / a) as u8,
                    (b / a) as u8,
                );
            }
        }
    }

    /// Mélange alpha « source over » sur le pixel existant.
    fn melanger(&mut self, x: i32, y: i32, sa: u8, sr: u8, sg: u8, sb: u8) {
        let x = x + self.ox;
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || !self.ok(x, y) {
            return;
        }
        let i = y as usize * self.w + x as usize;
        let dst = self.px[i];
        let da = (dst >> 24) as u32;
        let sa = sa as u32;
        let out = sa + da * (255 - sa) / 255;
        let mix = |s: u8, d: u32| -> u32 {
            if out == 0 {
                return 0;
            }
            ((s as u32 * sa) + (d * da * (255 - sa) / 255)) / out
        };
        self.px[i] = argb(
            out & 0xff,
            mix(sr, (dst >> 16) & 0xff) & 0xff,
            mix(sg, (dst >> 8) & 0xff) & 0xff,
            mix(sb, dst & 0xff) & 0xff,
        );
    }

    /// Largeur réelle d'un texte, dans la police active (les deux styles n'ont
    /// pas la même : la bitmap est à chasse fixe, la TTF proportionnelle).
    fn largeur(&self, s: &str, scale: i32) -> i32 {
        match self.police {
            Some(f) => {
                let px = 9.0 * scale as f32;
                joli(s)
                    .chars()
                    .map(|c| f.metrics(c, px).advance_width)
                    .sum::<f32>()
                    .ceil() as i32
            }
            None => s.chars().count() as i32 * 6 * scale,
        }
    }

    /// Texte tronqué (« … » remplacé par un point) pour tenir dans `max_w`.
    fn text_fit(&mut self, x: i32, y: i32, s: &str, scale: i32, color: u32, max_w: i32) {
        if self.largeur(s, scale) <= max_w {
            self.text(x, y, s, scale, color);
            return;
        }
        let point = self.largeur(".", scale);
        let mut out = String::new();
        let mut w = 0;
        for ch in s.chars() {
            let mut tmp = [0u8; 4];
            let cw = self.largeur(ch.encode_utf8(&mut tmp), scale);
            if w + cw + point > max_w {
                break;
            }
            out.push(ch);
            w += cw;
        }
        out.push('.');
        self.text(x, y, &out, scale, color);
    }

    /// Texte qui **rétrécit** au lieu d'être tronqué : on descend l'échelle
    /// (3 → 2) tant que ça ne tient pas dans `max_w`, puis `text_fit` tronque
    /// en dernier recours. La légende de la fenêtre active (souvent un titre
    /// de jeu) dépassait 21 caractères à l'échelle 3 et finissait en « … » ;
    /// le nom du paquet publié par glauncher est encore plus long.
    fn text_reduit(
        &mut self,
        x: i32,
        y: i32,
        s: &str,
        scale_max: i32,
        scale_min: i32,
        color: u32,
        max_w: i32,
    ) {
        let mut scale = scale_max.max(scale_min);
        while scale > scale_min && self.largeur(s, scale) > max_w {
            scale -= 1;
        }
        self.text_fit(x, y, s, scale, color, max_w);
    }

    /// Texte : bitmap 5×7 (RETRO) ou police vectorielle (MODERNE).
    fn text(&mut self, x: i32, y: i32, s: &str, scale: i32, color: u32) {
        if let Some(f) = self.police {
            // MODERNE : casse de phrase (le RETRO reste en capitales bitmap)
            self.text_ttf(f, x, y, &joli(s), scale, color);
            return;
        }
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch);
            for (col, bits) in g.iter().enumerate() {
                for row in 0..7 {
                    if bits & (1 << row) != 0 {
                        self.rect(
                            cx + col as i32 * scale,
                            y + row as i32 * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
            cx += 6 * scale;
        }
    }

    /// Texte tel quel, sans la casse de phrase de [`joli`] : le clavier
    /// virtuel affiche des symboles et des libellés composés (« L2+L1/R1 »)
    /// que la casse de phrase abîmerait.
    fn text_brut(&mut self, x: i32, y: i32, s: &str, scale: i32, color: u32) {
        if let Some(f) = self.police {
            self.text_ttf(f, x, y, s, scale, color);
            return;
        }
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch);
            for (col, bits) in g.iter().enumerate() {
                for row in 0..7 {
                    if bits & (1 << row) != 0 {
                        self.rect(
                            cx + col as i32 * scale,
                            y + row as i32 * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
            cx += 6 * scale;
        }
    }

    /// Texte vectoriel, anticrénelé. `9 px × scale` par cran : la hauteur de
    /// capitale tombe autour de `7 × scale`, comme la bitmap, donc toutes les
    /// mises en page existantes tiennent sans retouche.
    fn text_ttf(&mut self, f: &fontdue::Font, x: i32, y: i32, s: &str, scale: i32, color: u32) {
        let px = 9.0 * scale as f32;
        // La bitmap 5×7 dessine ses capitales de `y` à `y + 7*scale`. La TTF,
        // calée sur son ASCENDANCE, posait son texte ~5 px plus bas : dans la
        // pastille de sélection il collait au bord inférieur. On cale donc la
        // ligne de base sur la hauteur d'une capitale, pour que les deux
        // polices occupent exactement la même boîte (mêmes marges partout).
        let cap = f.metrics('H', px).height as f32;
        let base = y as f32 + cap;
        let a = ((color >> 24) & 0xff) as f32 / 255.0;
        let cr = ((color >> 16) & 0xff) as f32;
        let cg = ((color >> 8) & 0xff) as f32;
        let cb = (color & 0xff) as f32;
        let mut cx = x as f32;
        for ch in s.chars() {
            let (m, bmp) = f.rasterize(ch, px);
            if m.width > 0 && m.height > 0 {
                let ox = cx.round() as i32 + m.xmin;
                let oy = base.round() as i32 - m.ymin - m.height as i32;
                for gy in 0..m.height {
                    for gx in 0..m.width {
                        let cov = bmp[gy * m.width + gx] as f32 / 255.0 * a;
                        if cov > 0.02 {
                            self.melanger_a(ox + gx as i32, oy + gy as i32, cr, cg, cb, cov);
                        }
                    }
                }
            }
            cx += m.advance_width;
        }
    }

    /// « over » prémultiplié d'un pixel (anticrénelage du texte vectoriel et
    /// des glyphes). Le buffer est en `Argb8888` prémultiplié.
    fn melanger_a(&mut self, x: i32, y: i32, r: f32, g: f32, b: f32, a: f32) {
        let x = x + self.ox;
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || !self.ok(x, y) {
            return;
        }
        let i = y as usize * self.w + x as usize;
        let d = self.px[i];
        let da = ((d >> 24) & 0xff) as f32 / 255.0;
        let dr = ((d >> 16) & 0xff) as f32;
        let dg = ((d >> 8) & 0xff) as f32;
        let db = (d & 0xff) as f32;
        let oa = a + da * (1.0 - a);
        let or = r * a + dr * (1.0 - a);
        let og = g * a + dg * (1.0 - a);
        let ob = b * a + db * (1.0 - a);
        let q = |v: f32| v.clamp(0.0, 255.0) as u32;
        self.px[i] = argb(q(oa * 255.0), q(or), q(og), q(ob));
    }

    /// Multiplie l'opacité de **tout** le tampon (fondu du menu système). Le
    /// format est prémultiplié, donc les quatre canaux prennent le même
    /// facteur : la composante alpha reste cohérente avec les couleurs.
    fn appliquer_opacite(&mut self, f: f32) {
        if f >= 0.999 {
            return;
        }
        let k = (f.clamp(0.0, 1.0) * 256.0) as u32;
        for p in self.px.iter_mut() {
            let v = *p;
            if v == 0 {
                continue;
            }
            let a = ((((v >> 24) & 0xff) * k) >> 8) << 24;
            let r = ((((v >> 16) & 0xff) * k) >> 8) << 16;
            let g = ((((v >> 8) & 0xff) * k) >> 8) << 8;
            let b = ((v & 0xff) * k) >> 8;
            *p = a | r | g | b;
        }
    }

}

/* ============================ couleurs ARGB ============================ */

const fn argb(a: u32, r: u32, g: u32, b: u32) -> u32 {
    (a << 24) | (r << 16) | (g << 8) | b
}

/// ARGB **prémultiplié** : c'est le format du buffer (`Argb8888`) — un pixel
/// translucide écrit en couleur pleine ressort sinon plus clair que voulu.
const fn argb_p(a: u32, r: u32, g: u32, b: u32) -> u32 {
    argb(a, r * a / 255, g * a / 255, b * a / 255)
}

/* ============================== styles ============================== */

/// Palette **et** géométrie du panneau : un style EST un `Theme`. Le dessin ne
/// cite aucune constante de couleur, sinon changer de style obligerait à
/// repasser sur chaque `rect()`.
#[derive(Clone, Copy)]
struct Theme {
    accent: u32,
    txt: u32,
    dim: u32,
    ok: u32,
    warn: u32,
    /// liseré intérieur du panneau
    edge: u32,
    /// fond du panneau, dégradé vertical (haut → bas)
    fond_haut: u32,
    fond_bas: u32,
    /// surbrillance de la ligne sélectionnée (dégradé)
    sel_haut: u32,
    sel_bas: u32,
    /// voile derrière le menu système
    shade: u32,
    /// curseur de réglage : creux, remplissage, graduations, poignée
    piste: u32,
    piste_plein: u32,
    /// rail de l'interrupteur éteint : assez clair pour se voir sur le panneau
    /// sombre (avec `piste`, on ne distinguait que le bouton)
    rail: u32,
    poignee: u32,
    poignee_bord: u32,
    /// curseur **sur la ligne sélectionnée** : la surbrillance y est claire,
    /// une poignée blanche sur pastille blanche disparaîtrait — ces variantes
    /// garantissent le contraste.
    curseur_piste_sur: u32,
    curseur_plein_sur: u32,
    curseur_poignee_sur: u32,
    curseur_bord_sur: u32,
    /// couleurs de texte **sur** la surbrillance : le style Deck éclaircit la
    /// ligne sélectionnée, ses gris doivent donc s'assombrir pour rester lisibles
    sel_txt: u32,
    sel_dim: u32,
    sel_ok: u32,
    sel_warn: u32,
    /// rayons des coins du panneau et de la surbrillance (0 = angles droits)
    rayon: i32,
    rayon_sel: i32,
    /// barre d'accent à gauche de la ligne sélectionnée (0 = aucune)
    barre: i32,
    /// barre d'accent en haut du panneau (0 = aucune)
    barre_haut: i32,
    /// ombre portée (0 = pas d'ombre)
    ombre: u32,
}

impl Theme {
    /// Couleur de texte à employer sur la ligne sélectionnée : identité quand
    /// la surbrillance est sombre, version sombre quand elle est claire (Deck).
    fn sur(&self, c: u32, sel: bool) -> u32 {
        if !sel {
            return c;
        }
        if c == self.txt {
            self.sel_txt
        } else if c == self.dim {
            self.sel_dim
        } else if c == self.ok {
            self.sel_ok
        } else if c == self.warn {
            self.sel_warn
        } else {
            c
        }
    }
}

/// **RETRO** (seul style, ex-DECK) : fond presque noir, accent bleu Steam,
/// **surbrillance claire en pastille**, police bitmap 5×7. C'est la signature
/// « console » ; MODERNE en reprend exactement couleurs et géométrie.
fn theme_retro() -> Theme {
    Theme {
        accent: argb(0xff, 0x1a, 0x9f, 0xff),
        txt: argb(0xff, 0xf2, 0xf6, 0xfa),
        dim: argb(0xff, 0x9a, 0xa9, 0xb8),
        ok: argb(0xff, 0x59, 0xd9, 0x8e),
        warn: argb(0xff, 0xff, 0x7b, 0x6b),
        edge: argb(0xff, 0x26, 0x30, 0x3b),
        fond_haut: argb(0xe8, 0x1b, 0x23, 0x2c),
        fond_bas: argb(0xe8, 0x0c, 0x11, 0x17),
        sel_haut: argb_p(0xf2, 0xf4, 0xf7, 0xfa),
        sel_bas: argb_p(0xe6, 0xd5, 0xdd, 0xe6),
        shade: argb_p(0xdc, 0x04, 0x06, 0x0a),
        // sillon visible sur le panneau sombre : avec 0x1e2832 il se confondait
        // avec le fond et l'on ne voyait que la poignée (constaté en RETRO)
        piste: argb(0xff, 0x2c, 0x38, 0x46),
        piste_plein: argb(0xff, 0x1a, 0x9f, 0xff),
        rail: argb(0xff, 0x3a, 0x47, 0x56),
        poignee: argb(0xff, 0xf2, 0xf6, 0xfa),
        poignee_bord: argb(0xff, 0x0a, 0x0e, 0x13),
        // pastille blanche : poignée bleue à contour sombre (une poignée
        // blanche sur blanc ne se voyait pas)
        curseur_piste_sur: argb(0xff, 0x2c, 0x38, 0x46),
        curseur_plein_sur: argb(0xff, 0x1a, 0x9f, 0xff),
        curseur_poignee_sur: argb(0xff, 0x1a, 0x9f, 0xff),
        curseur_bord_sur: argb(0xff, 0x0c, 0x11, 0x17),
        sel_txt: argb(0xff, 0x0c, 0x11, 0x17),
        sel_dim: argb(0xff, 0x3a, 0x46, 0x52),
        sel_ok: argb(0xff, 0x11, 0x69, 0x3c),
        sel_warn: argb(0xff, 0xa6, 0x25, 0x1c),
        rayon: 16,
        rayon_sel: 20,
        barre: 0,
        barre_haut: 0,
        ombre: argb_p(0x88, 0, 0, 0),
    }
}

/// Clés de style (ce qu'on écrit dans le fichier de config) et libellés (ce
/// qu'affiche la ligne `STYLE`). L'ordre est celui du curseur.
const STYLE_KEYS: [&str; 2] = ["retro", "moderne"];
const STYLE_OPTS: [&str; 2] = ["RETRO", "MODERNE"];

/// MODERNE n'est pas un autre thème : c'est le MÊME (couleurs et géométrie)
/// avec une police vectorielle et des glyphes de manette dessinés. Un seul
/// langage visuel, deux habillages.
fn theme_pour(_idx: usize) -> Theme {
    theme_retro()
}

/// « moderne »/« modern » → 1 ; tout le reste (retro, deck, steam, nu,
/// breeze — anciens noms) → 0. Jamais d'échec au démarrage.
fn style_normalise(nom: &str) -> usize {
    match nom.trim().to_ascii_lowercase().as_str() {
        "moderne" | "modern" | "ttf" => 1,
        _ => 0,
    }
}

/// Police de MODERNE, embarquée dans le binaire (Noto Sans, **licence Apache
/// 2.0** — `assets/LICENSE-NotoSans.txt`). Chargée une fois au démarrage, pas
/// à chaque dessin.
const POLICE_TTF: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");

fn charger_police() -> Option<fontdue::Font> {
    match fontdue::Font::from_bytes(POLICE_TTF, fontdue::FontSettings::default()) {
        Ok(f) => {
            println!("POLICE Noto Sans (TTF) chargee");
            Some(f)
        }
        Err(e) => {
            eprintln!("police: chargement impossible ({e}) — retour police bitmap");
            None
        }
    }
}

/// Fichier où le style choisi dans `REGLAGES` est mémorisé.
fn chemin_style() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|h| format!("{h}/.config/gablue/gpad-style"))
}

fn lire_style() -> Option<String> {
    std::fs::read_to_string(chemin_style()?).ok()
}

fn ecrire_style(cle: &str) {
    let Some(c) = chemin_style() else { return };
    if let Some(d) = std::path::Path::new(&c).parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Err(e) = std::fs::write(&c, format!("{cle}\n")) {
        eprintln!("style: ecriture {c}: {e}");
    }
    println!("STYLE SAUVE {cle}");
}

/* ============================ manette ============================ */

const FPS_OPTS: [(&str, i32); 7] = [
    ("30", 30),
    ("48", 48),
    ("60", 60),
    ("90", 90),
    ("120", 120),
    ("165", 165),
    ("SANS LIMITE", 0),
];
/// Durées de buffer de replay proposées (secondes), 30 s par défaut.
const REPR_OPTS: [(&str, u32); 5] = [
    ("15", 15),
    ("30", 30),
    ("60", 60),
    ("120", 120),
    ("300", 300),
];
// ordre croissant de gauche à droite : désactivé en premier
const MH_OPTS: [(&str, &str); 3] = [
    ("MASQUE", "masque"),
    ("PAR DEFAUT", "defaut"),
    ("HORIZONTAL", "horizontal"),
];

/// Une ligne du menu. Les réglages vivent **dans la liste** : pas de
/// sous-menu, pas de panneau secondaire — on se pose sur la ligne, et
/// gauche/droite change la valeur, appliquée aussitôt.
///
/// Exception, les **catégories** : l'accueil est un menu de sous-menus
/// (`SousMenu`), et chaque catégorie est une page à part. C'est un niveau de
/// navigation, pas un second panneau : même liste, même rendu, même retour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entree {
    Action(&'static str),
    /// application/media : ouvre si fermé, ferme si ouvert
    Appli(&'static str),
    /// sortie par défaut (« volume général »)
    Volume,
    /// volume du flux du jeu en cours (PID du lanceur et descendants)
    VolumeJeu,
    /// volume du flux d'une application compagne (vidéo lancée à côté)
    VolumeApp,
    /// sélecteur jeu ⇄ application compagne (n'existe que si une app est
    /// ouverte à côté d'un jeu)
    Basculer,
    Mango,
    Fps,
    /// durée du buffer de replay (secondes)
    Duree,
    /// en-tête = fenêtre active : A ouvre la liste des fenêtres
    Fenetre,
    /// une fenêtre de la liste (index dans `fenetres`)
    Autre(usize),
    /// entrée de l'accueil : ouvre la catégorie (libellé, catégorie)
    SousMenu(&'static str, Cat),
    /// style visuel (NU / DECK / BREEZE), appliqué à chaud
    Style,
    /// optimiser au premier plan : booster cgroup de Plasma (priorité CPU à la
    /// fenêtre active), basculé par A
    Booster,
    // --- capture (réglages de `gpad-rec`, fichier recorder.conf) ---
    Codec,
    Qualite,
    /// format et qualité des captures d'écran (lus par `gpad-shot`)
    ShotFormat,
    ShotQualite,
    Reso,
    FpsRec,
    Curseur,
    /// démarre le replay seul quand un jeu LGP/WGP est lancé (désactivé par défaut)
    AutoReplay,
}

/// L'accueil : les catégories, dans l'ordre du quotidien (le jeu d'abord, le
/// système en dernier — c'est le coin des actions irréversibles). En **tête**,
/// la bascule jeu ⇄ app quand elle est possible : c'est le geste du quotidien
/// et il ne doit pas se chercher dans une catégorie (demandé le 23/11/2026).
fn accueil_items(bascule: bool) -> Vec<Entree> {
    let mut items = Vec::with_capacity(9);
    if bascule {
        items.push(Entree::Basculer);
    }
    items.extend(vec![
        Entree::SousMenu("JEU", Cat::Jeu),
        Entree::SousMenu("FENETRES", Cat::Fenetres),
        Entree::SousMenu("PERF", Cat::Perf),
        Entree::SousMenu("CAPTURE", Cat::Capture),
        Entree::SousMenu("SON", Cat::Son),
        Entree::SousMenu("AFFICHAGE", Cat::Affichage),
        Entree::SousMenu("MANETTE", Cat::Manette),
        Entree::SousMenu("SYSTEME", Cat::Systeme),
    ]);
    items
}

/// Catégories de l'accueil : un sous-menu par domaine. Le contenu est le même
/// dans les deux sessions pour l'instant ; les différences bureau / console
/// viendront avec le sélecteur du mode jeu (jeu ⇄ application compagne).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cat {
    Jeu,
    Fenetres,
    Affichage,
    Perf,
    Capture,
    Son,
    Manette,
    Systeme,
}

/// Quel flux audio une ligne de volume pilote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cible {
    Jeu,
    App,
}

/// Ce qui existe au moment où une catégorie est construite : une ligne dont
/// l'objet n'existe pas (pas de replay, pas de manette BT, pas de flux audio)
/// n'est pas proposée du tout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Dispo {
    replay: bool,
    manette: bool,
    jeu: bool,
    app: bool,
    /// une application compagne est ouverte à côté du jeu : le sélecteur
    bascule: bool,
    /// session console (KWin nu) : le menu propose de revenir au bureau au
    /// lieu d'y passer
    console: bool,
}

impl Cat {
    fn nom(self) -> &'static str {
        match self {
            Cat::Jeu => "JEU",
            Cat::Fenetres => "FENETRES",
            Cat::Affichage => "AFFICHAGE",
            Cat::Perf => "PERF",
            Cat::Capture => "CAPTURE",
            Cat::Son => "SON",
            Cat::Manette => "MANETTE",
            Cat::Systeme => "SYSTEME",
        }
    }

    /// Les lignes de la catégorie, dans l'ordre d'affichage. L'en-tête de
    /// fenêtre est en tête de CHAQUE catégorie : il garde sa hauteur nulle et
    /// sa ligne propre, mais le curseur doit pouvoir s'y poser partout (A y
    /// ouvre les actions de la fenêtre active).
    ///
    /// `Dispo` décide des lignes conditionnelles (même règle qu'avant : une
    /// ligne annonçant « INACTIF » n'apprend rien).
    fn entrees(self, dispo: Dispo) -> Vec<Entree> {
        match self {
            Cat::Jeu => {
                let mut v = vec![Entree::Fenetre];
                // le geste du quotidien (regarder une vidéo à côté du jeu) :
                // en tête de JEU, là où le panneau s'ouvre — la ligne
                // n'existait que dans FENETRES, donc introuvable (signalé le
                // 23/11 : « pas de bouton pour basculer »)
                if dispo.bascule {
                    v.push(Entree::Basculer);
                }
                v.push(Entree::Action("FERMER"));
                v.push(Entree::Action("PLEIN ECRAN"));
                v.push(Entree::Action("TOUT FERMER"));
                v
            }
            // le sélecteur jeu ⇄ application compagne passe avant la liste :
            // c'est le geste du quotidien (regarder une vidéo à côté du jeu)
            Cat::Fenetres => {
                let mut v = vec![Entree::Fenetre];
                if dispo.bascule {
                    v.push(Entree::Basculer);
                }
                v.push(Entree::Action("CHOISIR FENETRE"));
                v
            }
            Cat::Affichage => vec![
                Entree::Fenetre,
                Entree::Action("VRR"),
                Entree::Style,
            ],
            Cat::Perf => vec![
                Entree::Fenetre,
                Entree::Mango,
                Entree::Fps,
                Entree::Booster,
            ],
            Cat::Capture => {
                let mut v = vec![
                    Entree::Fenetre,
                    Entree::Action("ENREGISTRER UNE VIDEO"),
                    Entree::Action("CAPTURE ECRAN"),
                    Entree::Action("REPLAY"),
                ];
                if dispo.replay {
                    v.push(Entree::Action("SAUVER REPLAY"));
                }
                v.extend([
                    Entree::Duree,
                    Entree::AutoReplay,
                    Entree::Qualite,
                    Entree::Codec,
                    Entree::ShotFormat,
                    Entree::ShotQualite,
                    Entree::Reso,
                    Entree::FpsRec,
                    Entree::Curseur,
                ]);
                v
            }
            // le général d'abord, puis le jeu et l'app compagne : trois flux
            // distincts, chacun réglable sans toucher aux autres
            Cat::Son => {
                let mut v = vec![Entree::Fenetre, Entree::Volume];
                if dispo.jeu {
                    v.push(Entree::VolumeJeu);
                }
                if dispo.app {
                    v.push(Entree::VolumeApp);
                }
                v
            }
            Cat::Manette => {
                let mut v = vec![
                    Entree::Fenetre,
                    Entree::Action("MODE SOURIS"),
                    Entree::Action("CLAVIER VIRTUEL"),
                ];
                if dispo.manette {
                    v.push(Entree::Action("DECONNECTER LA MANETTE"));
                }
                v
            }
            Cat::Systeme => vec![
                Entree::Fenetre,
                // la veille est la seule action RÉVERSIBLE de la page : elle
                // passe avant les sorties (demandé le 23/11)
                Entree::Action("MISE EN VEILLE"),
                // le mode d'à côté : changer de session n'est pas sortir, la
                // ligne reste donc AU-DESSUS des sorties (demandé le 23/11)
                Entree::Action(if dispo.console { "MODE BUREAU" } else { "MODE JEU" }),
                Entree::Action("FERMER LA SESSION"),
                Entree::Action("REDEMARRER"),
                Entree::Action("ETEINDRE"),
            ],
        }
    }
}

/// Pages du panneau : accueil (menu des catégories), une catégorie, la liste
/// des applications et les deux pages de fenêtres (aller-retour).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Accueil,
    Categorie(Cat),
    Applis,
    /// actions de la fenêtre active (ancienne « boîte de choix ») : une PAGE,
    /// comme la liste des fenêtres — même présentation, même retour
    Fenetre,
    Fenetres,
}

/// Pages de premier niveau, dans l'ordre de L1/R1. Les sous-pages (une
/// catégorie, la liste des fenêtres) n'en font pas partie : elles
/// appartiennent à la page qui les a ouvertes.
const ORDRE_PAGES: [Page; 2] = [Page::Accueil, Page::Applis];

impl Page {
    /// Page à hauteurs variables (catégories de 56 px, lignes de réglage de
    /// 86) : son défilement se compte en lignes, pas en fenêtres fixes.
    /// L'accueil en fait partie : 8 catégories débordent d'un écran 720p, et
    /// sans défilement les deux dernières seraient inatteignables.
    fn defilante(self) -> bool {
        matches!(self, Page::Accueil | Page::Categorie(_))
    }
}

impl Entree {
    fn nom(self) -> &'static str {
        match self {
            Entree::Action(n) | Entree::Appli(n) => n,
            Entree::Volume => "VOLUME GENERAL",
            Entree::VolumeJeu => "VOLUME JEU",
            Entree::VolumeApp => "VOLUME APP",
            // le libellé dit toujours où l'on va, jamais d'où l'on vient
            Entree::Basculer => "BASCULER JEU / APP",
            Entree::Mango => "MANGOHUD",
            Entree::Fps => "FPS MAX",
            Entree::Duree => "DUREE REPLAY",
            Entree::Fenetre => "FENETRE ACTIVE",
            Entree::Autre(_) => "FENETRE",
            Entree::SousMenu(t, _) => t,
            Entree::Style => "STYLE",
            // « priorité à la fenêtre active » : le libellé exact demandé
            // (« optimiser au premier plan », 24 caractères) déborderait à
            // l'échelle 3 (18 caractères tiennent), d'où ce libellé court —
            // la section PERFORMANCE dit le domaine.
            Entree::Booster => "PRIORITE ACTIVE",
            Entree::Codec => "CODEC",
            Entree::Qualite => "QUALITE",
            Entree::ShotFormat => "FORMAT PHOTO",
            Entree::ShotQualite => "QUALITE PHOTO",
            Entree::Reso => "RESOLUTION",
            Entree::FpsRec => "FPS",
            Entree::Curseur => "CURSEUR",
            Entree::AutoReplay => "REPLAY AUTO",
        }
    }
    /// ligne de réglage (curseur) plutôt qu'une action ponctuelle
    fn reglage(self) -> bool {
        matches!(
            self,
            Entree::Volume
                | Entree::VolumeJeu
                | Entree::VolumeApp
                | Entree::Mango
                | Entree::Fps
                | Entree::Duree
                | Entree::Codec
                | Entree::Qualite
                | Entree::ShotFormat
                | Entree::ShotQualite
                | Entree::Reso
                | Entree::FpsRec
                | Entree::Curseur
                | Entree::AutoReplay
        )
    }
    /// Hauteur de la ligne : les réglages à curseur respirent (86), les
    /// énumérations tiennent sur une seule ligne (46), une catégorie de
    /// l'accueil un peu plus (56 : c'est un point d'entrée, pas une action).
    fn hauteur(self) -> i32 {
        match self {
            Entree::Fenetre => 0,
            Entree::SousMenu(..) => 56,
            Entree::Volume
            | Entree::VolumeJeu
            | Entree::VolumeApp
            | Entree::Mango
            | Entree::Fps
            | Entree::Duree
            | Entree::Qualite
            | Entree::Codec
            | Entree::ShotFormat
            | Entree::ShotQualite
            | Entree::Reso
            | Entree::FpsRec => 86,
            _ => 46,
        }
    }
    /// le curseur peut s'y poser (toutes les lignes le sont : les séparateurs
    /// ont disparu avec les catégories)
    fn selectionnable(self) -> bool {
        true
    }
}

/// Jeton d'arbitrage des combos Home+direction.
///
/// Quand le panneau est ouvert, l'overlay tient le grab : c'est lui qui voit
/// le combo, puis il le relâche. Le déclencheur, qui lit le même node sans
/// grab, relit alors les mêmes appuis et rejouait l'action — double bip au
/// démarrage du replay, bascule d'enregistrement annulée dans la seconde
/// (constaté le 21/11/2026 : deux `replay.sh start` à la même seconde).
/// Le premier des deux à poser le jeton agit ; l'autre se tait. Au-delà de
/// 400 ms, c'est un nouvel appui : le jeton est repris.
fn combo_jeton() -> bool {
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
    let chemin = format!("{rt}/gablue/gpad-combo");
    for _ in 0..2 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&chemin)
        {
            Ok(mut f) => {
                let _ = f.write_all(format!("{}\n", std::process::id()).as_bytes());
                return true;
            }
            Err(_) => {
                let frais = std::fs::metadata(&chemin)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|d| d.as_millis() < 400);
                if frais {
                    return false;
                }
                let _ = std::fs::remove_file(&chemin);
            }
        }
    }
    true
}

/// Ce qu'on lance une fois le panneau retiré.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Apres {
    Record,
    Capture,
    /// sauvegarde du replay : après fermeture, pour ne pas se filmer
    Replay,
    /// Home+haut : démarre le buffer s'il est arrêté, sauvegarde le replay
    /// s'il tourne — après fermeture, pour ne pas se filmer.
    ReplayStart,
    /// Home+bas : arrête le buffer (le replay n'est plus exclusif de
    /// l'enregistrement : il se pilote comme une seconde piste).
    ReplayStop,
    /// menu console : redémarrer / éteindre (le panneau doit avoir disparu)
    Reboot,
    Eteindre,
    /// menu console : mise en veille (systemctl suspend), après fermeture
    Veille,
    /// menu console : déconnexion de la session (retour à l'écran de connexion)
    DeconnecterSession,
    /// menu système : passer à l'autre session (console ⇄ bureau)
    ModeJeu,
    ModeBureau,
    /// menu console : killthemall, après fermeture du menu
    ToutFermer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Cancel,
    /// L1 / R1 : page précédente (-1) / suivante (+1) — jamais le même sens
    Page(i32),
    /// L2/R2 : fenêtre précédente (-1) / suivante (+1)
    Cycle(i32),
}

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;
const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_WEST: u16 = 0x134;
const BTN_NORTH: u16 = 0x133;
const BTN_MODE: u16 = 0x13c;

/// Home maintenu = menu système. Même seuil que `HOME_LONG_MS` de `gpad-trigger` :
/// au-delà, le panneau latéral demande le menu système et se ferme ; en deçà,
/// un appui court ferme simplement le panneau (au relâchement).
const HOME_LONG_MS: u64 = 1200;

/// Deux appuis d'un **même bouton** plus rapprochés que ça sont une seule
/// frappe. Une membrane qui rebondit (Croix usée) ou un rapport dupliqué du
/// pilote livrait deux `EV_KEY … 1` pour une pression : sur l'en-tête, le
/// premier ouvre la page FENÊTRE et le second **armait** `FERMER`, si bien que
/// la confirmation était déjà à l'écran (constaté à l'usage). Un vrai double
/// appui humain est ≥ 100 ms, un rebond < 25 ms : 80 ms sépare les deux.
const FRAPPE_MIN_MS: u64 = 80;

const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_TL2: u16 = 0x138;
const BTN_TR2: u16 = 0x139;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_THUMBL: u16 = 0x13d;
const BTN_THUMBR: u16 = 0x13e;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    sec: i64,
    usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

/* ============================ état ============================ */

/// Glissement du panneau : on anime la marge droite du layer surface
/// (layer-shell n'a pas d'animation, c'est nous qui la produisons).
struct Anim {
    start: Instant,
    dur: Duration,
    from: i32,
    to: i32,
}

/// Fondu d'ouverture / fermeture du **menu système** : le panneau latéral
/// glisse, le menu centré n'a pas de bord par lequel sortir — il s'efface.
struct Fondu {
    start: Instant,
    dur: Duration,
    de: f32,
    vers: f32,
}

struct ShmBuffer {
    buffer: wl_buffer::WlBuffer,
    ptr: *mut u8,
    len: usize,
}

struct App {
    qh: QueueHandle<App>,
    running: bool,
    // menu
    /// page ACCUEIL : fenêtre active et actions courantes
    accueil: Vec<Entree>,
    /// lignes de la catégorie ouverte (reconstruites à l'entrée et quand leur
    /// contenu change : `SAUVER REPLAY` n'existe que si le buffer tourne)
    cat_items: Vec<Entree>,
    /// réglages de capture (recorder.conf partagé avec gpad-rec)
    cap: capture::Conf,
    /// première ligne affichée d'une page à hauteurs variables (défilement)
    reg_top: usize,
    /// page applications (L1/R1)
    applis: Vec<Entree>,
    page: Page,
    selected: usize,
    status: String,
    nav_count: u32,
    // surface
    wl_surface: Option<wl_surface::WlSurface>,
    layer: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    configured: bool,
    /// panneau latéral (défaut) ou plein écran assombri
    side: bool,
    panel_w: i32,
    anim: Option<Anim>,
    /// Fondu en cours (menu système) et opacité courante du dessin (0..1).
    fondu: Option<Fondu>,
    opacite: f32,
    /// durée du fondu du menu système (banc : `PAD_FONDU_MS`)
    /// attente avant de lancer le glissement d'entrée : KWin anime l'ouverture
    /// de cette surface pendant ce temps, et deux mouvements simultanés se
    /// cumulent (le panneau semblait se poser sur place)
    panneau_delai_ms: u64,
    /// instant où l'entrée visible doit commencer (attente du délai ci-dessus)
    entree: Option<Instant>,
    fondu_ms: u64,
    closing: bool,
    slid_in: bool,
    /// Mode résident (`--resident`) : le processus survit entre deux
    /// ouvertures. La surface reste mappée, transparente et hors écran, donc
    /// KWin ne l'anime plus à chaque ouverture (son animation d'ouverture n'a
    /// lieu qu'au map) et il n'y a plus d'`exec` à payer.
    resident: bool,
    /// panneau réellement à l'écran (faux au démarrage résident et entre deux
    /// ouvertures) : tout ce qui lit `/proc`, le gate ou la fenêtre active est
    /// coupé tant qu'il est faux.
    visible: bool,
    /// SIGTERM reçu : on sort après la fermeture, même en résident (sinon le
    /// processus resterait à l'écoute et systemd finirait par le tuer).
    quitter: bool,
    /// écoute des commandes du déclencheur (`--ipc <socket>`) : `open <node>
    /// <cibles…>`, `close`.
    ipc: Option<UnixListener>,
    /// chemin du socket (retiré à la sortie : pas de fichier mort à chaque
    /// redémarrage du service).
    ipc_path: Option<String>,
    /// chemin demandé par `--ipc`, écouté seulement une fois la surface prête.
    ipc_attente: Option<String>,
    /// menu système demandé pendant que Home était maintenu : on prévient le
    /// déclencheur **après** avoir rendu la manette, sinon il lance la console
    /// alors que notre `EVIOCGRAB` tient encore le node.
    voir_console: bool,
    /// relecture des réglages (volume, VRR, MangoHud, booster) demandée après
    /// la première image, pour ne pas retarder l'ouverture.
    a_rafraichir: bool,
    size: (i32, i32),
    shm: Option<wl_shm::WlShm>,
    // double buffering : dessine uniquement dans un buffer rendu par le compositeur
    buffers: Vec<ShmBuffer>,
    free: Vec<usize>,
    dirty: bool,
    // evdev
    pad: Option<OwnedFd>,
    pad_path: Option<String>,
    /// MAC si la manette qui pilote le panneau est en Bluetooth
    pad_bt: Option<String>,
    /// manette Sony : glyphes croix/rond au lieu de A/B
    pad_sony: bool,
    /// Home maintenu : les directions deviennent des combos, même panneau ouvert
    home_down: bool,
    /// Home maintenu a déjà demandé le menu système (une fois par appui)
    home_console: bool,
    /// pid du gpad-trigger qui nous a lancés (`GPAD_TRIGGER_PID`) : c'est lui qui
    /// ouvre le menu système, nous ne faisons que le prévenir
    declencheur: Option<i32>,
    home_at: Instant,
    /// action à lancer APRÈS la fermeture (le panneau ne doit pas être capturé)
    combo_apres: Option<Apres>,
    /// node de la manette mémorisé avant un relâchement du grab (replay)
    combo_node: Option<String>,
    /// mode souris actif (processus `gpad-mouse` vivant)
    mouse_active: bool,
    /// buffer de replay actif (gpu-screen-recorder lancé avec -r)
    replay_active: bool,
    /// limite d'attente après une bascule de replay : `replay.sh start` joue le
    /// bip puis lance le flatpak (et attend jusqu'à 4 s), `stop` envoie SIGINT
    /// et attend la sortie. Tant qu'elle n'est pas passée, la passe d'état
    /// d'une seconde ne contredit pas le choix de l'utilisateur — sans ça la
    /// ligne repassait « ARRET » le temps du démarrage avant de revenir
    /// « ACTIF » (constaté).
    replay_attente: Option<Instant>,
    /// VRR automatique actif sur les sorties capables (`vrr.sh get`)
    vrr_on: bool,
    /// booster « premier plan » de Plasma actif (`booster.sh get`)
    booster_on: bool,
    /// page d'ouverture déjà choisie (une seule fois)
    page_initiale: bool,
    /// `--focus` (Home+bas) impose sa ligne : on ne la déplace pas
    focus_impose: bool,
    /// instant de démarrage : grâce pour attendre la première info de fenêtre
    depart: Instant,
    /// fenêtre active (dernier état reçu du script KWin)
    win: WinInfo,
    /// état partagé avec le service D-Bus (si KWin est joignable)
    win_etat: Option<Arc<Mutex<Partage>>>,
    /// pilotage KWin (scripts plein écran / info)
    kbus: Option<KBus>,
    /// nom de l'entrée irréversible armée (FERMER, déco BT, redémarrer,
    /// éteindre) : le second A exécute, bouger dans la liste désarme
    arme: Option<String>,
    /// début de l'enregistrement en cours (epoch, lu dans recorder.state)
    rec_start: Option<u64>,
    /// secondes écoulées affichées (n'anime le panneau que si ça change)
    rec_secs: u64,
    /// icône de la fenêtre active (None = pas d'icône trouvée)
    icone: Option<Icone>,
    /// identité (desktop|class) pour laquelle l'icône a été cherchée
    icone_cle: String,
    /// jeu publié par glauncher (`<cache>/running.txt`) : slug et PID du jeu
    jeu: Option<JeuActif>,
    /// logo du jeu actif, lu dans le cache de glauncher (jamais inventé :
    /// fichier absent = pas de bandeau)
    logo: Option<Icone>,
    /// chemin du logo chargé : évite de relire l'image à chaque tour
    logo_cle: String,
    /// vue « menu système » (Home maintenu) : petit panneau centré, pas de
    /// réglages — le drapeau CLI reste `--console`
    console: bool,
    /// dernière application lancée : un appui répété pendant le démarrage ne
    /// doit pas ouvrir une seconde instance
    derniere_appli: Option<(String, Instant)>,
    /// nom D-Bus de CETTE instance (`org.gablue.Win.p<pid>`) : deux overlays
    /// peuvent coexister (celui de gpad-trigger et un de banc), sans se voler
    /// le service ni les scripts KWin
    bus: String,
    /// suffixe des scripts KWin (`gablue-wininfo-<pid>`)
    inst: String,
    /// compteur : chaque script KWin reçoit un nom de plugin UNIQUE (voir
    /// `KBus::lancer_nomme` : réutiliser un nom rend l'ancien script)
    seq: u64,
    /// noms de plugins KWin chargés, à décharger à la sortie
    plugins: Vec<String>,
    /// première ligne affichée de la page « fenêtres » (défilement)
    fen_top: usize,
    /// page d'où l'on a ouvert la liste des fenêtres : `B` et l'activation y
    /// reviennent (la liste est un aller-retour, pas une destination)
    fen_retour: Page,
    /// page d'où l'on a ouvert la page « fenêtre active » (retour par B)
    fen_page_retour: Page,
    /// icônes des fenêtres listées, par `desktop|class`
    icones: HashMap<String, Option<Icone>>,
    /// fenêtres proposées par la page « fenêtres »
    fenetres: Vec<Fenetre>,
    /// entrées de la page « fenêtres » (miroir de `fenetres`)
    fen_items: Vec<Entree>,
    /// entrées de la page « fenêtre active » : fermer / plein écran / choisir
    fen_menu: Vec<Entree>,
    stick_x_latched: bool,
    stick_y_latched: bool,
    /// état physique des boutons d'action, par code `EV_KEY` : un appui ne
    /// compte qu'après le relâchement du même bouton (voir `FRAPPE_MIN_MS`)
    touches_tenues: [bool; 0x140],
    /// (code, instant) de la dernière frappe retenue : jette le rebond
    derniere_frappe: Option<(u16, Instant)>,
    /// plage réelle des axes du stick (EVIOCGABS) : les seuils en dépendent
    abs_x: (i32, i32),
    abs_y: (i32, i32),
    /// sticks droits : déplacement du clavier virtuel
    abs_rx: (i32, i32),
    abs_ry: (i32, i32),
    fps_idx: usize,
    /// limite fps hors crans (posée à la main ou par Gablue Manager) : affichée
    /// telle quelle, sinon le curseur afficherait un cran qui n'est pas le vrai
    fps_reel: Option<i32>,
    mh_idx: usize,
    /// position dans REPR_OPTS (30 s par défaut)
    repr_idx: usize,
    vol: i32,
    /// sortie muette (A sur la ligne VOLUME GENERAL bascule)
    vol_mute: bool,
    /// flux audio du jeu en cours et d'une application compagne, s'ils
    /// existent : chacun a son propre volume, indépendant du général
    son_jeu: Option<audio::Flux>,
    son_app: Option<audio::Flux>,
    /// fenêtre de l'application compagne et du jeu, pour le sélecteur
    /// (rafraîchies à l'entrée de la catégorie FENETRES)
    bascule_app: Option<Fenetre>,
    bascule_jeu: Option<Fenetre>,
    /// l'application compagne a le focus (donc le sélecteur propose le jeu)
    bascule_sur_app: bool,
    /// le son du jeu est en sourdine parce qu'on est passé à l'app : il doit
    /// être rendu au retour, même si l'app a disparu entre-temps
    jeu_muet: bool,
    // diagnostic du canal : qui lit encore la manette pendant l'overlay
    evdev_node: Option<String>,
    hidraw_node: Option<String>,
    /// rafraîchissement de l'état de l'enregistreur (badge du menu)
    last_scan: Instant,
    // gate BPF : socket du démon + état (coupe hidraw/js pendant le menu)
    gate_sock: Option<String>,
    gate_nodes: usize,
    gate_on: bool,
    /// durée du glissement en ms ; 0 = défauts (180 entrée / 140 sortie)
    slide_ms: u64,
    /// enregistreur actif (état publié par la socket de `gpad-rec`)
    rec_active: bool,
    /// fichier de l'enregistrement en cours (donné par le démon)
    rec_fichier: String,
    /// un réglage écrit attend la fin de l'enregistrement pour s'appliquer
    rec_pending: bool,
    /// décalage horizontal du panneau en pixels : les marges layer-shell
    /// négatives sont ignorées par KWin (mesuré), donc on décale le DESSIN.
    off: i32,
    /// changement de page en cours : (ancienne, nouvelle, départ, sens)
    pdep: Option<(Page, Page, Instant, i32)>,
    /// prochaine tentative de (re)chargement de `wininfo.js` alors que le
    /// résident est caché et n'a encore reçu aucun `Info` (KWin redémarré, ou
    /// script jamais chargé). `None` = jamais tenté.
    kwin_retry_at: Option<Instant>,
    /// plugin de la dernière tentative de relance de `wininfo.js` : déchargé
    /// avant la suivante, sinon un KWin muet accumulerait un script/minute.
    kwin_retry_plugin: Option<String>,
    /// fermeture automatique après inactivité manette (0 = jamais)
    inactivite: Duration,
    /* ---------- clavier virtuel (commandes `kbd` de l'IPC) ---------- */
    /// handles wayland, gardés pour créer la surface du clavier à la demande
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    /// table + sélection + injection : reste alloué après fermeture (le
    /// clavier uinput est créé une fois par session)
    clavier: Option<clavier::Clavier>,
    kbd_surface: Option<wl_surface::WlSurface>,
    kbd_layer: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    kbd_buffers: Vec<ShmBuffer>,
    kbd_free: Vec<usize>,
    /// taille logique de l'écran, apprise par une sonde plein écran (une seule
    /// fois) : c'est elle qui borne la position du panneau
    kbd_ecran: (i32, i32),
    /// `kbd_size` = taille du tampon = taille du panneau (jamais l'écran :
    /// KWin reconvertit toute l'image à chaque tampon, 14 Mo en 1440p)
    kbd_size: (i32, i32),
    kbd_configured: bool,
    kbd_dirty: bool,
    /// un déplacement attend son `flush` (sinon la marge reste dans le tampon
    /// d'envoi jusqu'au prochain évènement)
    kbd_bouge: bool,
    /// le clavier est ouvert (la manette pilote les touches, pas le menu)
    kbd_ouvert: bool,
    ///Horodatage du dernier pas fait au **stick** : un stick qui repasse vite
    /// le seuil ne doit pas produire deux pas (sauts de 2-3 lignes, constaté)
    derniere_nav: Instant,
    /// dernier événement manette retenu (n'importe lequel, même ignoré)
    derniere_activite: Instant,
    /// style visuel courant (index dans `STYLE_KEYS`) et sa palette
    style_idx: usize,
    th: Theme,
    /// police TTF du style MODERNE (None = bitmap 5×7 de RETRO)
    police: Option<fontdue::Font>,
    /// glyphes de manette (famille du pad connecté), chargés au premier
    /// dessin en MODERNE
    glyphes: Vec<(&'static str, Icone)>,
}

/// Desktop (id `.desktop`) d'une application du menu : sert à retrouver son
/// icône. Vaut mieux que le nom du menu, qui n'a aucun rapport.
fn desktop_appli(nom: &str) -> &'static str {
    match nom {
        "GLAUNCHER" => "glauncher",
        "EMULATIONSTATION" => "esde",
        "YOUTUBE" => "rocks.shy.VacuumTube",
        "GABLUE TV" => "gablue-tv",
        "JELLYFIN" => "org.jellyfin.JellyfinDesktop",
        _ => "",
    }
}

/// Classes / desktopFileName par lesquels reconnaître la fenêtre d'une appli du
/// menu (minuscules, testés en sous-chaîne). Une appli absente d'ici n'a pas
/// d'activation possible : son script est toujours lancé.
fn motifs_appli(nom: &str) -> &'static [&'static str] {
    match nom {
        "GLAUNCHER" => &["glauncher"],
        "EMULATIONSTATION" => &["esde", "es-de", "emulationstation"],
        "YOUTUBE" => &["vacuumtube"],
        "GABLUE TV" => &["tvqt", "gablue_tv", "gablue-tv"],
        "JELLYFIN" => &["jellyfin"],
        _ => &[],
    }
}

/// Ce qu'un événement bouton devient : une frappe neuve (agir), ou la raison
/// pour laquelle c'est la **même** pression physique que la précédente.
#[derive(Debug, PartialEq, Eq)]
enum Frappe {
    Neuve,
    /// `value 1` sans relâchement entre les deux : rapport dupliqué
    DejaTenue,
    /// deux impulsions trop rapprochées : rebond de membrane
    Rebond,
}

/// Une frappe d'action est-elle neuve ? `tenues` et `derniere` portent l'état
/// d'un événement à l'autre (voir `App::touches_tenues`).
///
/// Un appui ne compte qu'**après le relâchement** du même bouton, et jamais
/// deux fois d'affilée en moins de `FRAPPE_MIN_MS`. Les relâchements (`value 0`)
/// passent toujours, de même que les valeurs qui ne sont pas des appuis
/// (`value 2`, l'autorépétition, déjà ignorée par la table d'actions).
fn frappe_nouvelle(
    tenues: &mut [bool; 0x140],
    derniere: &mut Option<(u16, Instant)>,
    code: u16,
    valeur: i32,
    quand: Instant,
) -> Frappe {
    let i = code as usize;
    if i >= tenues.len() {
        return Frappe::Neuve;
    }
    match valeur {
        1 => {
            let tenu = tenues[i];
            tenues[i] = true;
            let rebond = derniere.is_some_and(|(c, t)| {
                c == code
                    && quand.saturating_duration_since(t) < Duration::from_millis(FRAPPE_MIN_MS)
            });
            if tenu {
                Frappe::DejaTenue
            } else if rebond {
                Frappe::Rebond
            } else {
                *derniere = Some((code, quand));
                Frappe::Neuve
            }
        }
        0 => {
            tenues[i] = false;
            Frappe::Neuve
        }
        _ => Frappe::Neuve,
    }
}

/// Entrées qui exigent un second appui : on ne déconnecte pas, ne redémarre
/// pas et n'éteint pas sur un appui malheureux.
fn demande_confirmation(nom: &str) -> bool {
    matches!(
        nom,
        "FERMER"
            | "DECONNECTER LA MANETTE"
            | "MODE JEU"
            | "MODE BUREAU"
            | "FERMER LA SESSION"
            | "REDEMARRER"
            | "ETEINDRE"
            | "TOUT FERMER"
    )
}

/// PIDs descendants de `racine`, profondeur d'abord, lus dans
/// `/proc/<pid>/task/<pid>/children` (pas de scan complet de /proc, et le
/// noyau répond en O(1) par processus).
fn descendants(racine: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut pile = vec![racine];
    while let Some(pid) = pile.pop() {
        // **Tous les threads** du processus : un fork part souvent d'un thread
        // (WebEngine d'une appli flatpak, Wine), et `/proc/<pid>/task/<pid>/
        // children` ne liste que les enfants du thread principal — le
        // sous-processus cherché restait invisible.
        let dossier = format!("/proc/{pid}/task");
        let Ok(taches) = std::fs::read_dir(&dossier) else {
            continue;
        };
        let mut enfants = Vec::new();
        for tache in taches.flatten() {
            let chemin = std::path::Path::new(&dossier)
                .join(tache.file_name())
                .join("children");
            let Ok(txt) = std::fs::read_to_string(chemin) else {
                continue;
            };
            for tok in txt.split_whitespace() {
                if let Ok(c) = tok.parse::<i32>() {
                    enfants.push(c);
                }
            }
        }
        for c in enfants {
            out.push(c);
            pile.push(c);
        }
    }
    out
}

/// `systemctl <action>` (redémarrer / éteindre / mettre en veille).
/// `REBOOT`/`POWEROFF`/`SUSPEND` permettent de remplacer la commande : le banc
/// ne doit JAMAIS éteindre ni endormir la machine.
fn lancer_systeme(var: &str, action: &str) {
    let prog = std::env::var(var).unwrap_or_else(|_| "/usr/bin/systemctl".to_string());
    let mut c = std::process::Command::new(&prog);
    if prog.ends_with("systemctl") {
        c.arg(action);
    }
    match c.spawn() {
        Ok(_) => println!("SYSTEME {action}"),
        Err(e) => eprintln!("SYSTEME {action}: {e}"),
    }
}

/// Déconnexion de la session (« fermer la session », retour à l'écran de
/// connexion). Le service KDE ferme les applications proprement (Plasma 6,
/// `org.kde.Shutdown`) ; à défaut, logind termine la session du poste.
/// `LOGOUT` remplace la commande : le banc ne doit JAMAIS déconnecter.
fn lancer_deconnexion() {
    if let Ok(p) = std::env::var("LOGOUT") {
        match std::process::Command::new(&p).spawn() {
            Ok(_) => println!("SYSTEME logout ({p})"),
            Err(e) => eprintln!("SYSTEME logout {p}: {e}"),
        }
        return;
    }
    let qdbus = ["/usr/bin/qdbus6", "/usr/bin/qdbus"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists());
    if let Some(q) = qdbus {
        match std::process::Command::new(q)
            .args(["org.kde.Shutdown", "/Shutdown", "org.kde.Shutdown.logout"])
            .spawn()
        {
            Ok(_) => {
                println!("SYSTEME logout (org.kde.Shutdown)");
                return;
            }
            Err(e) => eprintln!("SYSTEME logout {q}: {e}"),
        }
    }
    match std::env::var("XDG_SESSION_ID") {
        Ok(id) if !id.is_empty() => match std::process::Command::new("/usr/bin/loginctl")
            .args(["terminate-session", &id])
            .spawn()
        {
            Ok(_) => println!("SYSTEME logout (loginctl {id})"),
            Err(e) => eprintln!("SYSTEME logout loginctl: {e}"),
        },
        _ => eprintln!("SYSTEME logout: ni qdbus ni XDG_SESSION_ID"),
    }
}

/// Configuration d'autologin écrite par `gablue-session-mode` : sa présence
/// signifie qu'une déconnexion relancerait la même session.
const AUTOLOGIN: &str = "/etc/plasmalogin.conf.d/zz-gablue-session.conf";

fn autologin_arme() -> bool {
    std::path::Path::new(AUTOLOGIN).exists()
}

/// Bascule de session (`console`, `bureau`, `greeter`). Le helper écrit dans
/// /etc et redémarre le gestionnaire de connexion : il tourne en root via
/// pkexec, la règle polkit de l'image l'autorisant pour le groupe wheel sans
/// mot de passe. `GABLUE_SESSION_MODE` remplace la commande — le banc ne doit
/// JAMAIS changer de session.
fn lancer_mode(mode: &str) {
    let mut c = match std::env::var("GABLUE_SESSION_MODE") {
        Ok(prog) => {
            let mut c = std::process::Command::new(prog);
            c.arg(mode);
            c
        }
        Err(_) => {
            let mut c = std::process::Command::new("/usr/bin/pkexec");
            c.arg("/usr/libexec/gablue-session-mode").arg(mode);
            c
        }
    };
    match c.spawn() {
        Ok(_) => println!("SYSTEME mode {mode}"),
        Err(e) => eprintln!("SYSTEME mode {mode}: {e}"),
    }
}

/// Durée d'enregistrement : `MM:SS`, puis `H:MM:SS` au-delà de l'heure.
fn chrono(secs: u64) -> String {
    let (h, m, sec) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

impl App {
    fn new(qh: QueueHandle<App>) -> Self {
        Self {
            qh,
            running: true,
            // ACCUEIL : un sous-menu par domaine (le détail vit dans la
            // catégorie, plus de page à rallonge) ; l'en-tête de fenêtre est
            // dans chaque catégorie, hauteur nulle.
            accueil: accueil_items(false),
            cat_items: Vec::new(),
            cap: capture::Conf::lire(),
            reg_top: 0,
            applis: Vec::new(),
            page: Page::Accueil,
            selected: 0,
            status: "PRET".to_string(),
            nav_count: 0,
            wl_surface: None,
            layer: None,
            configured: false,
            side: true,
            panel_w: 480,
            anim: None,
            fondu: None,
            opacite: 1.0,
            fondu_ms: 320,
            // Durée de l'animation d'ouverture de KWin (effet Scale :
            // `animationTime(200)`, défaut 200 ms), pas 250 : on restait
            // invisible 50 ms de trop (mesuré 19/11/2026).
            panneau_delai_ms: 200,
            entree: None,
            closing: false,
            slid_in: false,
            resident: false,
            visible: true,
            quitter: false,
            ipc: None,
            ipc_path: None,
            ipc_attente: None,
            voir_console: false,
            a_rafraichir: false,
            size: (0, 0),
            shm: None,
            buffers: Vec::new(),
            free: Vec::new(),
            dirty: true,
            pad: None,
            pad_path: None,
            pad_bt: None,
            pad_sony: false,
            home_down: false,
            home_console: false,
            declencheur: std::env::var("GPAD_TRIGGER_PID")
                .ok()
                .and_then(|s| s.parse::<i32>().ok()),
            home_at: Instant::now(),
            combo_apres: None,
            combo_node: None,
            mouse_active: false,
            replay_active: false,
            replay_attente: None,
            vrr_on: false,
            booster_on: false,
            page_initiale: false,
            focus_impose: false,
            depart: Instant::now(),
            win: WinInfo::default(),
            win_etat: None,
            kbus: None,
            arme: None,
            rec_start: None,
            rec_secs: 0,
            icone: None,
            icone_cle: String::new(),
            jeu: None,
            logo: None,
            logo_cle: String::new(),
            console: false,
            derniere_appli: None,
            bus: String::new(),
            inst: String::new(),
            seq: 0,
            plugins: Vec::new(),
            fen_top: 0,
            fen_retour: Page::Accueil,
            fen_page_retour: Page::Accueil,
            icones: HashMap::new(),
            fenetres: Vec::new(),
            fen_items: Vec::new(),
            fen_menu: vec![
                Entree::Action("FERMER"),
                Entree::Action("PLEIN ECRAN"),
                Entree::Action("CHOISIR FENETRE"),
            ],
            stick_x_latched: false,
            stick_y_latched: false,
            touches_tenues: [false; 0x140],
            derniere_frappe: None,
            abs_x: (-32768, 32767),
            abs_y: (-32768, 32767),
            abs_rx: (-32768, 32767),
            abs_ry: (-32768, 32767),
            fps_idx: 0,
            fps_reel: None,
            mh_idx: 0,
            repr_idx: 1,
            vol: 50,
            vol_mute: false,
            son_jeu: None,
            son_app: None,
            bascule_app: None,
            bascule_jeu: None,
            bascule_sur_app: false,
            jeu_muet: false,
            evdev_node: None,
            hidraw_node: None,
            last_scan: Instant::now(),
            gate_sock: None,
            gate_nodes: 0,
            gate_on: false,
            slide_ms: 0,
            off: 0,
            pdep: None,
            kwin_retry_at: None,
            kwin_retry_plugin: None,
            inactivite: Duration::from_secs(10),
            compositor: None,
            layer_shell: None,
            clavier: None,
            kbd_surface: None,
            kbd_layer: None,
            kbd_buffers: Vec::new(),
            kbd_free: Vec::new(),
            kbd_ecran: (0, 0),
            kbd_size: (0, 0),
            kbd_configured: false,
            kbd_dirty: false,
            kbd_bouge: false,
            kbd_ouvert: false,
            derniere_activite: Instant::now(),
            derniere_nav: Instant::now() - Duration::from_secs(1),
            style_idx: 0,
            th: theme_retro(),
            police: None,
            glyphes: Vec::new(),
            rec_active: false,
            rec_fichier: String::new(),
            rec_pending: false,
        }
    }

    /* ---------- grab evdev ---------- */

    /// Identité de la manette qui pilote : famille (glyphes croix/rond) et
    /// transport (entrée « déconnecter »). Indépendant du grab : un node non
    /// lisible ne doit pas changer l'interface.
    fn detecter_manette(&mut self, path: &str) {
        self.pad_sony = std::env::var("PAD_SONY").map(|v| v == "1").unwrap_or(false)
            || pad_est_sony(path);
        self.pad_bt = std::env::var("PAD_BT_MAC")
            .ok()
            .or_else(|| pad_bt_mac(path));
        if let Some(mac) = &self.pad_bt {
            // la déconnexion BT vit dans le menu console (Home maintenu), pas
            // dans le panneau latéral
            println!("BT {mac}");
        }
        println!("PAD {} sony={} bt={}", path, self.pad_sony, self.pad_bt.is_some());
    }

    fn grab_pad(&mut self, path: &str) {
        let file = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("grab: open {path}: {e}");
                return;
            }
        };
        let fd: OwnedFd = file.into();
        const EVIOCGRAB: libc::c_ulong = 0x4004_4590; // _IOW('E', 0x90, int)
        if unsafe { libc::ioctl(fd.as_raw_fd(), EVIOCGRAB, 1) } < 0 {
            eprintln!("grab: EVIOCGRAB {path}: {}", std::io::Error::last_os_error());
            return;
        }
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };

        // plage réelle des axes : un pad peut être 0..255, un autre -32768..32767.
        // Un seuil fixe (±14000) ne marcherait que sur le second.
        #[repr(C)]
        struct AbsInfo {
            value: i32,
            minimum: i32,
            maximum: i32,
            fuzz: i32,
            flat: i32,
            resolution: i32,
        }
        const EVIOCGABS: libc::c_ulong = 0x8018_4540; // _IOR('E', 0x40+abs, input_absinfo)
        for code in 0u32..4 {
            let mut ai = AbsInfo {
                value: 0,
                minimum: 0,
                maximum: 0,
                fuzz: 0,
                flat: 0,
                resolution: 0,
            };
            let rc = unsafe {
                libc::ioctl(
                    fd.as_raw_fd(),
                    EVIOCGABS + code as libc::c_ulong,
                    &mut ai as *mut AbsInfo,
                )
            };
            if rc == 0 && ai.maximum > ai.minimum {
                let plage = (ai.minimum, ai.maximum);
                match code {
                    0 => self.abs_x = plage,
                    1 => self.abs_y = plage,
                    3 => self.abs_rx = plage,
                    _ => self.abs_ry = plage,
                }
            }
        }
        self.pad_path = Some(path.to_string());
        self.pad = Some(fd);
        // Bluetooth -> on proposera de déconnecter cette manette.
        // PAD_BT_MAC force la détection pour les tests à la manette virtuelle.
        self.evdev_node = Some(path.to_string());
        self.hidraw_node = pad_uniq(path).and_then(|u| hidraw_for_uniq(&u));
        if self.hidraw_node.is_some() {
            println!("HIDRAW {}", self.hidraw_node.clone().unwrap_or_default());
        }
        println!("GRABBED {path}");
        let _ = std::io::stdout().flush();
    }

    fn release_pad(&mut self) {
        if let Some(fd) = self.pad.take() {
            const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
            unsafe { libc::ioctl(fd.as_raw_fd(), EVIOCGRAB, 0) };
            println!("RELEASED {}", self.pad_path.take().unwrap_or_default());
            let _ = std::io::stdout().flush();
        }
    }

    /// Home maintenu alors que le panneau est ouvert : `gpad-trigger` ne peut pas
    /// le voir (nous tenons le grab). On lui demande le menu système par
    /// SIGUSR1 et on se ferme ; il attend notre sortie (le pad doit être
    /// rendu, sinon `EVIOCGRAB` échoue côté menu système) puis lance l'overlay
    /// en mode console.
    fn surveiller_home(&mut self) {
        if self.console || !self.home_down || self.home_console {
            return;
        }
        if self.home_at.elapsed() < Duration::from_millis(HOME_LONG_MS) {
            return;
        }
        self.home_console = true;
        self.dirty = true;
        println!("HOME MAINTENU -> menu systeme");
        if self.resident {
            // en résident, le processus ne meurt pas : c'est `cacher()` qui
            // prévient le déclencheur, une fois le grab rendu (sinon la
            // console se lance sur un node encore grabé).
            self.voir_console = true;
            self.request_close();
            return;
        }
        match self.declencheur {
            Some(p) if p > 1 => {
                self.status = "MENU SYSTEME".to_string();
                unsafe { libc::kill(p, libc::SIGUSR1) };
            }
            _ => {
                // sans gpad-trigger (banc), personne ne lancera le menu système :
                // on ferme quand même, un panneau figé serait pire
                self.status = "MENU SYSTEME: SANS TRIGGER".to_string();
                eprintln!("gpad-trigger absent (GPAD_TRIGGER_PID) : menu systeme impossible");
            }
        }
        self.request_close();
    }

    fn pump_pad(&mut self) {
        let Some(raw_fd) = self.pad.as_ref().map(|f| f.as_raw_fd()) else {
            return;
        };
        // fermeture demandée depuis la boucle : on rend la manette APRÈS
        // l'avoir fini de lire (fermer libère le fd qu'on lit — le numéro
        // serait réutilisé et la lecture suivante lirait un autre objet)
        let mut fermer_apres = false;
        loop {
            let mut ev: InputEvent = unsafe { std::mem::zeroed() };
            let n = unsafe {
                libc::read(
                    raw_fd,
                    &mut ev as *mut InputEvent as *mut libc::c_void,
                    std::mem::size_of::<InputEvent>(),
                )
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                let transitoire = matches!(e.kind(), std::io::ErrorKind::WouldBlock)
                    || e.raw_os_error() == Some(libc::EAGAIN);
                if !transitoire {
                    // node détruit : manette débranchée / BT coupé pendant l'overlay.
                    // On ne laisse pas un panneau orphelin sans manette.
                    println!("PAD_GONE {e}");
                    let _ = std::io::stdout().flush();
                    self.status = "MANETTE PARTIE".to_string();
                    self.pad = None;
                    self.pad_path = None;
                    self.request_close();
                }
                break;
            }
            if n < std::mem::size_of::<InputEvent>() as isize {
                break;
            }
            // Seule une **vraie** entrée repousse la fermeture automatique.
            // Pas « tout événement » : un stick qui dérive émet 124↔125 en
            // continu (mesuré sur un DualShock 4), et le panneau ne se fermait
            // plus jamais — 11 min de grab constatés, manette morte pour le jeu.
            if self.entree_significative(ev.type_, ev.code, ev.value) {
                self.derniere_activite = Instant::now();
            }
            // Home (BTN_MODE) : appui bref = fermer. Maintenu, il transforme
            // la direction suivante en combinaison — y compris panneau OUVERT,
            // puisque c'est nous qui tenons le grab (gpad-trigger nous est aveugle).
            if ev.type_ == EV_KEY && ev.code == BTN_MODE {
                if ev.value == 1 {
                    self.home_down = true;
                    self.home_console = false;
                    self.home_at = Instant::now();
                } else if ev.value == 0 && self.home_down {
                    self.home_down = false;
                    if self.home_at.elapsed() < Duration::from_millis(600) {
                        println!("CLOSE (Home)");
                        if self.kbd_ouvert {
                            fermer_apres = true;
                            break;
                        }
                        self.request_close();
                    }
                }
                continue;
            }

            // Clavier virtuel : la manette pilote les touches, plus le menu.
            // Home garde son rôle (appui court = fermer, maintenu = menu
            // système), il est traité juste au-dessus.
            if self.kbd_ouvert {
                let change = match (ev.type_, ev.code, ev.value) {
                    (EV_SYN, _, _) => false,
                    (EV_KEY, code, v) if v == 0 || v == 1 => {
                        let fermer = self
                            .clavier
                            .as_mut()
                            .is_some_and(|c| c.touche(code, v == 1));
                        if fermer {
                            fermer_apres = true;
                            break;
                        }
                        true
                    }
                    (EV_ABS, ABS_HAT0X, v) => {
                        self.clavier.as_mut().is_some_and(|c| c.hat(0, v))
                    }
                    (EV_ABS, ABS_HAT0Y, v) => {
                        self.clavier.as_mut().is_some_and(|c| c.hat(1, v))
                    }
                    (EV_ABS, ABS_X, v) => {
                        let n = norme_axe(v, self.abs_x);
                        self.clavier.as_mut().is_some_and(|c| c.stick_gauche(0, n))
                    }
                    (EV_ABS, ABS_Y, v) => {
                        let n = norme_axe(v, self.abs_y);
                        self.clavier.as_mut().is_some_and(|c| c.stick_gauche(1, n))
                    }
                    (EV_ABS, ABS_RX, v) => {
                        let n = norme_axe(v, self.abs_rx);
                        if let Some(c) = &mut self.clavier {
                            c.regler_droite(0, n);
                        }
                        false
                    }
                    (EV_ABS, ABS_RY, v) => {
                        let n = norme_axe(v, self.abs_ry);
                        if let Some(c) = &mut self.clavier {
                            c.regler_droite(1, n);
                        }
                        false
                    }
                    _ => false,
                };
                if change {
                    self.kbd_dirty = true;
                }
                continue;
            }

            // Une frappe = un appui **après** relâchement, et jamais deux
            // d'affilée en moins de `FRAPPE_MIN_MS`. Un second `value 1` sans
            // `value 0` entre les deux est un rapport dupliqué, pas une
            // pression ; deux impulsions à 20 ms sont un rebond de membrane.
            // Sans cette garde, un appui unique sur l'en-tête ouvrait la page
            // FENÊTRE **et** armait FERMER (confirmation déjà affichée).
            // Le relâchement est toujours retenu, même quand l'appui est jeté.
            if ev.type_ == EV_KEY {
                match frappe_nouvelle(
                    &mut self.touches_tenues,
                    &mut self.derniere_frappe,
                    ev.code,
                    ev.value,
                    Instant::now(),
                ) {
                    Frappe::Neuve => {}
                    rejet => {
                        println!("FRAPPE ignoree (code 0x{:x} {rejet:?})", ev.code);
                        continue;
                    }
                }
            }

            let act = match (ev.type_, ev.code, ev.value) {
                (EV_KEY, BTN_SOUTH, 1) => Some(Action::Accept),
                (EV_KEY, BTN_EAST, 1) => Some(Action::Cancel),
                // L1 et R1 font la même chose : changer de page
                (EV_KEY, BTN_TL, 1) => Some(Action::Page(-1)),
                (EV_KEY, BTN_TR, 1) => Some(Action::Page(1)),
                // gâchettes : changer de fenêtre active sans ouvrir le panneau
                (EV_KEY, BTN_TL2, 1) => Some(Action::Cycle(-1)),
                (EV_KEY, BTN_TR2, 1) => Some(Action::Cycle(1)),
                (EV_KEY, BTN_NORTH, 1) => Some(Action::Up),
                (EV_KEY, BTN_WEST, 1) => Some(Action::Down),
                (EV_ABS, ABS_HAT0X, v) if v != 0 => {
                    Some(if v < 0 { Action::Left } else { Action::Right })
                }
                (EV_ABS, ABS_HAT0Y, v) if v != 0 => {
                    Some(if v < 0 { Action::Up } else { Action::Down })
                }
                (EV_ABS, ABS_X, v) => {
                    let (mn, mx) = self.abs_x;
                    let demi = (mx - mn) / 2;
                    let centre = mn + demi;
                    // Hystérésis : armé en franchissant 1/3 de la demi-plage,
                    // réarmé seulement sous 1/6 (avec un seul seuil, un stick
                    // qui flotte autour ré-arme et repart → sauts de 2-3 lignes
                    // « parfois », constaté). Plus un délai minimal entre deux
                    // pas, pour le stick qui traverse d'un coup.
                    let ecart = (v - centre).abs();
                    if ecart <= demi / 6 {
                        self.stick_x_latched = false;
                    }
                    if ecart > demi / 3
                        && !self.stick_x_latched
                        && self.derniere_nav.elapsed() >= PAS_MIN
                    {
                        self.stick_x_latched = true;
                        self.derniere_nav = Instant::now();
                        Some(if v < centre { Action::Left } else { Action::Right })
                    } else {
                        None
                    }
                }
                (EV_ABS, ABS_Y, v) => {
                    let (mn, mx) = self.abs_y;
                    let demi = (mx - mn) / 2;
                    let centre = mn + demi;
                    // même hystérésis que X (1/3 pour armer, 1/6 pour réarmer)
                    // et même délai minimal : un stick ne fait pas deux pas
                    let ecart = (v - centre).abs();
                    if ecart <= demi / 6 {
                        self.stick_y_latched = false;
                    }
                    if ecart > demi / 3
                        && !self.stick_y_latched
                        && self.derniere_nav.elapsed() >= PAS_MIN
                    {
                        self.stick_y_latched = true;
                        self.derniere_nav = Instant::now();
                        Some(if v < centre { Action::Up } else { Action::Down })
                    } else {
                        None
                    }
                }
                (EV_SYN, _, _) => None,
                _ => None,
            };
            if let Some(a) = act {
                // Home + direction = combo : on ferme d'abord, on agit ensuite
                // (sinon le panneau se filme dans la capture/le clip). Mêmes
                // gestes que panneau fermé (gpad-trigger) : gauche =
                // enregistrement, haut = replay (démarrer, ou sauver s'il
                // tourne), bas = arrêter le buffer, droite = capture.
                if self.home_down
                    && matches!(a, Action::Left | Action::Right | Action::Up | Action::Down)
                {
                    self.home_down = false;
                    if !combo_jeton() {
                        println!("COMBO ignore (traite par le declencheur)");
                        continue;
                    }
                    let apres = match a {
                        Action::Left => Apres::Record,
                        Action::Right => Apres::Capture,
                        Action::Up if self.replay_active => Apres::Replay,
                        Action::Up => Apres::ReplayStart,
                        _ => Apres::ReplayStop,
                    };
                    self.combo_apres = Some(apres);
                    self.status = match apres {
                        Apres::Record => "COMBO: REC APRES FERMETURE",
                        Apres::Capture => "COMBO: CAPTURE APRES FERMETURE",
                        Apres::Replay => "COMBO: REPLAY APRES FERMETURE",
                        Apres::ReplayStart => "COMBO: REPLAY DEMARRAGE",
                        Apres::ReplayStop => "COMBO: REPLAY ARRET",
                        // jamais posés par un combo (menu console seulement)
                        Apres::Reboot => "COMBO: REBOOT",
                        Apres::Eteindre => "COMBO: EXTINCTION",
                        Apres::Veille => "COMBO: VEILLE",
                        Apres::DeconnecterSession => "COMBO: DECONNEXION",
                        Apres::ModeJeu => "COMBO: MODE JEU",
                        Apres::ModeBureau => "COMBO: MODE BUREAU",
                        Apres::ToutFermer => "COMBO: TOUT FERMER",
                    }
                    .to_string();
                    println!(
                        "COMBO {} -> fermeture puis action",
                        match a {
                            Action::Left => "Home+gauche",
                            Action::Up => "Home+haut",
                            Action::Down => "Home+bas",
                            _ => "Home+droite",
                        }
                    );
                    self.request_close();
                    continue;
                }
                self.apply(a);
            }
        }
        if fermer_apres {
            self.request_close();
        }
    }

    /// La liste affichée : accueil, catégorie, applications ou fenêtres.
    fn liste(&self) -> &Vec<Entree> {
        match self.page {
            Page::Accueil => &self.accueil,
            Page::Categorie(_) => &self.cat_items,
            Page::Applis => &self.applis,
            Page::Fenetre => &self.fen_menu,
            Page::Fenetres => &self.fen_items,
        }
    }

    /// Listes dépendantes de l'état, reconstruites d'un seul point :
    /// l'accueil, et la catégorie ouverte si l'on y est (`SAUVER REPLAY`
    /// n'existe que si le buffer tourne — une ligne grisée « INACTIF »
    /// n'apprend rien, demandé le 21/11/2026). Appelée à chaque changement de
    /// `replay_active` ; le curseur est ramené dans les bornes si la ligne
    /// qu'il visait a disparu.
    fn maj_accueil(&mut self) {
        self.accueil = accueil_items(self.bascule_app.is_some());
        if let Page::Categorie(cat) = self.page {
            self.cat_items = cat.entrees(self.dispo());
        }
        let n = self.liste().len();
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    /// Position de la page dans le cycle (une catégorie appartient à
    /// l'accueil, les pages FENETRE / FENETRES à celle qui les a ouvertes :
    /// c'est elle qui s'allume).
    fn page_cycle(&self) -> usize {
        let base = match self.page {
            Page::Fenetres | Page::Fenetre => self.fen_retour,
            Page::Categorie(_) => Page::Accueil,
            p => p,
        };
        ORDRE_PAGES.iter().position(|p| *p == base).unwrap_or(0)
    }

    /// Suivi de page en bas : un **rond** par page de premier niveau, le
    /// courant plein et un peu plus gros, les autres en `rail`.
    fn points_pages(&self, cv: &mut Canvas, x_centre: i32, y: i32) {
        const D: i32 = 9;
        const GAP: i32 = 9;
        let n = ORDRE_PAGES.len() as i32;
        let actif = self.page_cycle() as i32;
        let mut x = x_centre - (n * D + (n - 1) * GAP) / 2;
        for i in 0..n {
            if i == actif {
                cv.rrect(x - 2, y - 2, D + 4, D + 4, (D + 4) / 2, self.th.accent);
            } else {
                cv.rrect(x, y, D, D, D / 2, self.th.rail);
            }
            x += D + GAP;
        }
    }

    /// Index de l'en-tête de fenêtre dans la page courante (il est en tête).
    fn index_entete(&self) -> Option<usize> {
        self.liste()
            .iter()
            .position(|e| matches!(e, Entree::Fenetre))
    }

    /// Une fenêtre active est-elle annoncée ? Le script KWin envoie une info
    /// **entièrement vide** quand il rejette la fenêtre active (pas de
    /// `normalWindow`, `skipTaskbar`…), pas une fenêtre réelle sans titre :
    /// Project Midas (titre SDL vide, classe `ProjectMidas`) passait donc pour
    /// « aucune fenêtre » — l'overlay annonçait « AUCUNE FENETRE » et s'ouvrait
    /// sur APPLICATIONS alors que le jeu tournait au premier plan (constaté
    /// 20/11/2026).
    fn a_fenetre(&self) -> bool {
        fenetre_annoncee(&self.win.caption, &self.win.class)
    }

    /// Un `Info` de KWin est-il déjà arrivé dans cette instance ? Distinct de
    /// « une fenêtre est active » : une info **vide** prouve aussi que le
    /// script tourne.
    fn recu_info(&self) -> bool {
        self.win_etat
            .as_ref()
            .and_then(|st| st.lock().ok().map(|p| p.recu))
            .unwrap_or(false)
    }

    /// Le script KWin pousse-t-il **encore** ? Il envoie un `Info` toutes les
    /// 300 ms : plusieurs secondes de silence signifient « script parti »
    /// (KWin redémarré), alors que `recu` reste vrai pour toujours.
    fn info_fraiche(&self) -> bool {
        self.win_etat
            .as_ref()
            .and_then(|st| st.lock().ok())
            .and_then(|p| p.dernier)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    }

    /// (Re)charge `wininfo.js` dans KWin alors qu'aucun `Info` n'est jamais
    /// arrivé (script non chargé au démarrage, KWin redémarré). Appelé
    /// **panneau caché**, une fois par minute : sans lui, le résident restait
    /// aveugle pour toute la session — plus de nom de fenêtre ni de logo, et
    /// la page d'ouverture retombait sur APPLICATIONS.
    fn relancer_wininfo(&mut self) {
        let Some(chemin) = script_instance("wininfo.js", &self.bus) else {
            return;
        };
        // une tentative précédente restée muette ne doit pas s'empiler dans
        // KWin : un script par minute pendant des jours, non merci
        let ancien = self.kwin_retry_plugin.take();
        let plugin = self.nom_plugin("wininfo");
        let ok = match self.kbus.as_ref() {
            Some(kbus) => {
                if let Some(a) = &ancien {
                    let _ = kbus.decharger(a);
                }
                kbus.demarrer_wininfo(&chemin, &plugin)
            }
            None => return,
        };
        if ok {
            self.kwin_retry_plugin = Some(plugin.clone());
            self.plugins.push(plugin);
        }
        println!("KWIN info={} (relance)", if ok { "active" } else { "refusee" });
    }

    /// Premier index sélectionnable qui n'est **pas** l'en-tête : une page
    /// s'ouvre sur son contenu, l'en-tête reste atteignable par le haut.
    fn premier_action(&self) -> usize {
        self.liste()
            .iter()
            .position(|e| e.selectionnable() && !matches!(e, Entree::Fenetre))
            .unwrap_or(0)
    }

    /// Index sélectionnable suivant/précédent : les séparateurs sont sautés.
    /// Sans entrée sélectionnable, on reste où on est.
    fn pas_selectable(&self, depuis: usize, sens: i32) -> usize {
        let l = self.liste();
        let n = l.len();
        if n == 0 {
            return depuis;
        }
        let mut i = depuis;
        for _ in 0..n {
            i = if sens > 0 {
                (i + 1) % n
            } else if i == 0 {
                n - 1
            } else {
                i - 1
            };
            if l[i].selectionnable() {
                return i;
            }
        }
        depuis
    }

    /// MODE SOURIS : actif -> on l'arrête ; inactif -> on le lance **sans
    /// fermer le panneau** (gpad-mouse ne grabbe pas la manette ; pendant que
    /// le panneau est ouvert, c'est notre grab qui le rend muet, il reprend à
    /// la fermeture).
    fn mouse_toggle(&mut self) {
        let nom = nom_binaire("MOUSE", "gpad-mouse");
        if processus_actif(&nom) {
            let n = arreter_processus(&nom);
            self.mouse_active = false;
            self.status = format!("MODE SOURIS: ARRETE ({n})");
            println!("MOUSE STOP {nom} ({n})");
        } else {
            lancer_script("MOUSE", &[]);
            self.mouse_active = true;
            self.status = "MODE SOURIS: ACTIF".to_string();
            println!("MOUSE START {nom}");
        }
    }

    /// CLAVIER VIRTUEL (depuis le panneau) : on garde la manette **et** le
    /// grab, on efface le panneau et on passe directement au clavier — aucun
    /// exec, aucun transfert de grab.
    fn clavier_virtuel(&mut self) {
        println!("CLAVIER (depuis le panneau)");
        let node = self.pad_path.clone().unwrap_or_default();
        self.ouvrir_clavier(&node);
    }

    /* ---------- clavier virtuel (surface dédiée) ---------- */

    /// Ouvre le clavier : grab de la manette (si elle n'est pas déjà tenue) et
    /// surface plein écran transparente. La table est construite au premier
    /// `configure` (elle a besoin de la taille de l'écran).
    fn ouvrir_clavier(&mut self, node: &str) {
        if self.kbd_ouvert {
            return;
        }
        if self.visible {
            // panneau ouvert : on l'efface sans animation — la manette reste
            // tenue, c'est le même processus et le même grab
            self.visible = false;
            self.closing = false;
            self.slid_in = true;
            self.anim = None;
            self.entree = None;
            self.fondu = None;
            self.opacite = 0.0;
            self.off = self.panel_w;
            self.dirty = true;
        }
        if !node.is_empty() && self.pad.is_none() {
            self.detecter_manette(node);
            self.grab_pad(node);
        }
        if self.pad.is_none() {
            eprintln!("clavier : aucune manette");
            return;
        }
        if let Some(c) = &mut self.clavier {
            c.relacher();
        }
        self.kbd_ouvert = true;
        self.closing = false;
        self.derniere_activite = Instant::now();
        self.creer_surface_clavier();
        if self.kbd_configured {
            self.pret_clavier();
        }
        println!("KBD OPEN {node}");
        let _ = std::io::stdout().flush();
    }

    /// Surface overlay **de la taille du panneau**, transparente. Le premier
    /// `configure` est demandé plein écran uniquement pour apprendre la taille
    /// logique de l'écran ; on rétrécit ensuite à la taille du panneau et on le
    /// déplace par **marges layer-shell** (ancré en haut à gauche, donc marges
    /// positives — KWin n'ignore que les négatives).
    ///
    /// Pourquoi pas une surface plein écran avec le panneau dessiné dedans : à
    /// chaque nouveau tampon KWin fait `im.convertTo()` sur **toute l'image**
    /// (`GLTexture::update`) — 3,7 M pixels par image en 1440p, mesuré comme
    /// la cause des saccades du déplacement ; un tampon de la taille du panneau
    /// fait 14 fois moins de travail, et un déplacement ne change même pas de
    /// tampon.
    fn creer_surface_clavier(&mut self) {
        if self.kbd_surface.is_some() {
            return;
        }
        let (Some(compositor), Some(layer_shell)) =
            (self.compositor.clone(), self.layer_shell.clone())
        else {
            eprintln!("clavier : compositeur wayland absent");
            return;
        };
        let surface = compositor.create_surface(&self.qh, KbdSurf);
        let layer = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "gpad-clavier".to_string(),
            &self.qh,
            KbdSurf,
        );
        layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer.set_size(0, 0); // 0 = toute la surface disponible
        layer.set_exclusive_zone(-1);
        // jamais le focus clavier : la frappe va à la fenêtre active
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        let region = compositor.create_region(&self.qh, ());
        surface.set_input_region(Some(&region)); // la souris traverse
        region.destroy();
        surface.commit();
        self.kbd_surface = Some(surface);
        self.kbd_layer = Some(layer);
        self.kbd_configured = false;
    }

    /// Phase 2 : la surface passe de « plein écran de sonde » à la taille du
    /// panneau, ancrée en haut à gauche, posée à la position mémorisée.
    fn reduire_surface_clavier(&mut self) {
        let Some(c) = &self.clavier else { return };
        let (pw, ph) = c.taille();
        if pw <= 0 || ph <= 0 {
            return;
        }
        let (x, y) = c.position();
        let (Some(layer), Some(surface)) = (self.kbd_layer.as_ref(), self.kbd_surface.as_ref())
        else {
            return;
        };
        layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left);
        layer.set_size(pw as u32, ph as u32);
        // ordre du protocole : top, right, bottom, left (pas celui du CSS)
        layer.set_margin(y.max(0), 0, 0, x.max(0));
        surface.commit();
        self.kbd_configured = false;
        println!("KBD surface {pw}x{ph} pos=({x},{y})");
    }

    /// Déplacement : seules les marges changent, donc **aucun tampon n'est
    /// renvoyé** (KWin repose l'item, sans reconvertir d'image).
    ///
    /// Cadencé par `wl_surface.frame` : au plus un envoi par image affichée.
    /// Sans ça, la manette émet 60 à 250 fois par seconde et KWin ré-arrange
    /// plus vite qu'il ne dessine — le mouvement paraît irrégulier. Si le
    /// compositeur ne rend pas la main (surface sans frame), on n'attend pas :
    /// au-delà de 100 ms on déplace quand même (jamais de clavier figé).
    fn placer_surface_clavier(&mut self) {
        let Some(c) = &self.clavier else { return };
        let (x, y) = c.position();
        let (Some(layer), Some(surface)) = (self.kbd_layer.as_ref(), self.kbd_surface.as_ref())
        else {
            return;
        };
        // ordre du protocole : top, right, bottom, left (pas celui du CSS)
        layer.set_margin(y.max(0), 0, 0, x.max(0));
        surface.commit();
        self.kbd_bouge = true;
    }

    fn detruire_surface_clavier(&mut self) {
        if let Some(l) = self.kbd_layer.take() {
            l.destroy();
        }
        if let Some(s) = self.kbd_surface.take() {
            s.destroy();
        }
        for b in self.kbd_buffers.drain(..) {
            unsafe { libc::munmap(b.ptr as *mut libc::c_void, b.len) };
            b.buffer.destroy();
        }
        self.kbd_free.clear();
        // la sonde plein écran sera refaite : l'écran a pu changer de taille
        self.kbd_ecran = (0, 0);
        self.kbd_size = (0, 0);
    }

    fn alloc_kbd_buffers(&mut self) {
        let Some(shm) = self.shm.clone() else { return };
        let (w, h) = self.kbd_size;
        if w <= 0 || h <= 0 {
            return;
        }
        for b in self.kbd_buffers.drain(..) {
            unsafe { libc::munmap(b.ptr as *mut libc::c_void, b.len) };
            b.buffer.destroy();
        }
        self.kbd_free.clear();
        let stride = w * 4;
        let len = (stride * h) as usize;
        let max = 64 << 20;
        for idx in 0..2usize {
            let name = c"gpad-clavier-shm";
            let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                eprintln!("memfd_create: {}", std::io::Error::last_os_error());
                return;
            }
            if len > max || unsafe { libc::ftruncate(fd, len as i64) } < 0 {
                eprintln!("ftruncate: {}", std::io::Error::last_os_error());
                unsafe { libc::close(fd) };
                return;
            }
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                eprintln!("mmap: {}", std::io::Error::last_os_error());
                unsafe { libc::close(fd) };
                return;
            }
            let pool =
                shm.create_pool(unsafe { BorrowedFd::borrow_raw(fd) }, len as i32, &self.qh, ());
            let buffer = pool.create_buffer(
                0,
                w,
                h,
                stride,
                wayland_client::protocol::wl_shm::Format::Argb8888,
                &self.qh,
                KbdBuf(idx),
            );
            pool.destroy();
            unsafe { libc::close(fd) };
            self.kbd_buffers.push(ShmBuffer {
                buffer,
                ptr: ptr as *mut u8,
                len,
            });
            self.kbd_free.push(idx);
        }
        self.kbd_dirty = true;
    }

    /// Construit (ou replace) le clavier une fois la taille d'écran connue.
    fn pret_clavier(&mut self) {
        if !self.kbd_ouvert {
            return;
        }
        let ecran = self.kbd_ecran;
        let (lw, lh) = ecran;
        if lw <= 0 || lh <= 0 {
            return;
        }
        if let Some(c) = &mut self.clavier {
            // rouvert : on garde l'injection (le clavier noyau coûte 350 ms)
            c.ecran(ecran);
            if let Some((x, y)) = lire_pos_clavier() {
                c.placer(x, y);
            }
            println!("KBD repris pos={:?}", c.position());
        } else {
            let (layout, variant) = clavier::layout_session();
            let rangees = match clavier::charger(&layout, &variant) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("clavier : {e}");
                    self.fermer_clavier();
                    return;
                }
            };
            let mut c = clavier::Clavier::neuf(rangees, &layout, &variant, ecran);
            c.preparer_injection();
            if let Some((x, y)) = lire_pos_clavier() {
                c.placer(x, y);
            }
            println!(
                "KBD {layout}/{variant} ecran={lw}x{lh} pos={:?}",
                c.position()
            );
            self.clavier = Some(c);
        }
        self.kbd_dirty = true;
    }

    fn draw_clavier(&mut self) {
        // `kbd_dirty` n'est ARMÉ qu'une fois l'image envoyée : sans tampon
        // libre on revient au tour suivant (sinon la frame serait perdue)
        if !self.kbd_configured || self.kbd_buffers.is_empty() {
            return;
        }
        let Some(idx) = self.kbd_free.pop() else { return };
        let (w, h) = self.kbd_size;
        let (ptr, len) = {
            let b = &self.kbd_buffers[idx];
            (b.ptr, b.len)
        };
        let buffer = self.kbd_buffers[idx].buffer.clone();
        let pixels =
            unsafe { std::slice::from_raw_parts_mut(ptr as *mut u32, len / 4) };
        // le tampon EST le panneau : tout repeindre coûte ~0,3 ms et rend le
        // déplacement gratuit (il ne change pas de tampon du tout)
        pixels.fill(0);
        if let Some(c) = &self.clavier {
            let mut cv = Canvas {
                px: pixels,
                w: w as usize,
                h: h as usize,
                police: self.police.as_ref(),
                ox: 0,
                clip: None,
            };
            c.dessiner(&mut cv);
        }
        if let Some(surface) = &self.kbd_surface {
            surface.attach(Some(&buffer), 0, 0);
            surface.damage_buffer(0, 0, w, h);
            surface.commit();
        }
        self.kbd_dirty = false;
    }

    fn tick_clavier(&mut self) {
        if !self.kbd_ouvert {
            return;
        }
        let (bouge, change) = {
            let Some(c) = &mut self.clavier else { return };
            (c.bouger(), c.tick())
        };
        if change {
            self.kbd_dirty = true;
        }
        if bouge {
            self.placer_surface_clavier();
        }
    }

    fn clavier_anime(&self) -> bool {
        self.kbd_ouvert && self.clavier.as_ref().is_some_and(|c| c.anime())
    }

    fn clavier_bouge(&self) -> bool {
        self.kbd_ouvert
            && self
                .clavier
                .as_ref()
                .is_some_and(|c| c.mouvement_actif())
    }

    /// Ferme le clavier : touches relâchées, position mémorisée, surface
    /// détruite (aucune surface plein écran ne reste posée sur le bureau),
    /// manette rendue.
    fn fermer_clavier(&mut self) {
        if !self.kbd_ouvert {
            return;
        }
        self.kbd_ouvert = false;
        self.kbd_dirty = false;
        if let Some(c) = &mut self.clavier {
            c.relacher();
            let (x, y) = c.position();
            ecrire_pos_clavier(x, y);
        }
        self.detruire_surface_clavier();
        self.cacher();
        // lancé par `--clavier` (repli du déclencheur) : un seul usage par
        // processus, sinon chaque appui laisserait un overlay orphelin
        if !self.resident {
            self.running = false;
        }
    }

    fn apply(&mut self, a: Action) {
        self.nav_count += 1;
        let avant = (self.page, self.selected);
        self.apply_menu(a);
        // bouger dans la liste désarme « FERMER » : on ne tue jamais par
        // inadvertance après avoir navigué
        if (self.page, self.selected) != avant {
            self.arme = None;
        }
        self.dirty = true;
        let _ = std::io::stdout().flush();
    }

    /// Un cran de réglage sur la ligne sélectionnée, appliqué **tout de
    /// suite** (aucune validation). Les valeurs butent aux extrémités :
    /// un curseur ne repart pas à zéro tout seul.
    fn regler(&mut self, sens: i32) {
        let entree = self.liste()[self.selected];
        match entree {
            Entree::Volume => self.set_vol(self.vol + 5 * sens),
            Entree::VolumeJeu => self.regler_flux(Cible::Jeu, 5 * sens),
            Entree::VolumeApp => self.regler_flux(Cible::App, 5 * sens),
            // action ponctuelle : rien à régler au curseur
            Entree::Basculer => {}
            Entree::Fps => {
                let max = FPS_OPTS.len() as i32 - 1;
                // Valeur hors crans : on part de la vraie valeur et on va au
                // premier cran au-delà (ou en deçà), pas un saut depuis un
                // cran qui n'était pas le vrai.
                let i = match self.fps_reel {
                    Some(reel) if sens > 0 => FPS_OPTS
                        .iter()
                        .position(|(_, x)| *x > reel)
                        .unwrap_or(max as usize),
                    Some(reel) => FPS_OPTS
                        .iter()
                        .rposition(|(_, x)| *x < reel && *x != 0)
                        .unwrap_or(0),
                    None => (self.fps_idx as i32 + sens).clamp(0, max) as usize,
                };
                if self.fps_reel.is_none() && i == self.fps_idx {
                    self.status = format!("FPS: {}", FPS_OPTS[i].0);
                    return;
                }
                self.fps_idx = i;
                self.fps_reel = None;
                let v = FPS_OPTS[i].1;
                self.status = run_script_args("MANGOHUD", &["set-fps", &v.to_string()]);
                println!("FPS_IDX {} ({})", i, v);
            }
            Entree::Mango => {
                let max = MH_OPTS.len() as i32 - 1;
                let i = (self.mh_idx as i32 + sens).clamp(0, max) as usize;
                if i == self.mh_idx {
                    self.status = format!("MANGOHUD: {}", MH_OPTS[i].0);
                    return;
                }
                self.mh_idx = i;
                let v = MH_OPTS[i].1;
                self.status = run_script_args("MANGOHUD", &["set-vue", v]);
                println!("MH_IDX {} ({})", i, v);
            }
            Entree::Duree => {
                let max = REPR_OPTS.len() as i32 - 1;
                let i = (self.repr_idx as i32 + sens).clamp(0, max) as usize;
                self.repr_idx = i;
                self.status = format!("DUREE REPLAY: {}s", REPR_OPTS[i].1);
                // la durée du buffer appartient au démon : même fichier
                self.cap.set("replay_sec", &REPR_OPTS[i].1.to_string());
                self.cap.ecrire();
                println!("REPR_IDX {} ({})", i, REPR_OPTS[i].1);
            }
            Entree::Codec => self.cap_cycle("codec", &capture::CODECS, sens),
            Entree::Qualite => self.cap_cycle("qualite", &capture::QUALITES, sens),
            Entree::ShotFormat => {
                self.cap_cycle("shot_format", &capture::SHOT_FORMATS, sens)
            }
            Entree::ShotQualite => {
                self.cap_cycle("shot_qualite", &capture::SHOT_QUALITES, sens)
            }
            Entree::Reso => self.cap_cycle("resolution", &capture::RESOS, sens),
            Entree::FpsRec => self.cap_cycle("fps", &capture::FPS, sens),
            Entree::Curseur => self.cap_cycle("curseur", &capture::CURSEURS, sens),
            Entree::AutoReplay => self.cap_bascule("auto_replay"),
            // réglé par A (interrupteur), pas par gauche/droite
            Entree::Style => self.basculer_style(),
            Entree::Booster => self.basculer_booster(),
            Entree::Action(_)
            | Entree::Appli(_)
            | Entree::Fenetre
            | Entree::Autre(_)
            | Entree::SousMenu(..) => {}
        }
    }

    /// Cycle une clé de `recorder.conf` : écrit le fichier et réveille le
    /// démon (il relit à chaud, aucun redémarrage).
    fn cap_cycle(&mut self, cle: &str, table: &[(&str, &str)], sens: i32) {
        let i = capture::idx(&self.cap, cle, table) as i32;
        let j = (i + sens).clamp(0, table.len() as i32 - 1) as usize;
        self.cap.set(cle, table[j].1);
        self.cap.ecrire();
        self.status = format!("{}: {}", cle.to_uppercase(), table[j].0);
        println!("CAP {}={}", cle, table[j].1);
    }

    fn cap_bascule(&mut self, cle: &str) {
        let v = if self.cap.get(cle) == Some("1") { "0" } else { "1" };
        self.cap.set(cle, v);
        self.cap.ecrire();
        self.status = format!("{}: {}", cle.to_uppercase(), if v == "1" { "OUI" } else { "NON" });
        println!("CAP {}={}", cle, v);
    }

    fn auto_replay(&self) -> bool {
        self.cap.get("auto_replay") == Some("1")
    }

    /// Ligne de réglage compacte : libellé à gauche, valeur à droite.
    /// Ligne de réglage « à crans » : titre, valeur courante à droite, et le
    /// slider gradué dessous — le langage de VOLUME / FPS / DURÉE REPLAY.
    /// Les énumérations de CAPTURE s'y alignent, sinon la section jure avec
    /// le reste du panneau.
    fn ligne_gradee(&self, cv: &mut Canvas, x: i32, pad: i32, inner: i32, y: i32,
                    titre: &str, color: u32, val: &str, nb: usize, idx: usize, sel: bool) {
        cv.text_fit(x + pad + 36, y, titre, 3, color, inner - 36);
        self.valeur_droite(cv, x + pad + inner, y, val, 2, color);
        self.curseur_ligne(cv, x + pad + 36, y + 40, inner - 36, nb, idx, sel);
    }

    /// Pixels disponibles pour la liste de la page courante.
    fn liste_budget(&self) -> i32 {
        let ph = self.hauteur_cadre();
        // mêmes repères que `draw()` : en console la liste commence à 62 px,
        // ailleurs à 180 px (+ logo) ; l'aide du bas commence à `ph - pied`.
        // Le budget s'arrête 8 px avant.
        let haut = if self.console {
            62
        } else {
            180 + self.decalage_logo()
        };
        let pied = if self.console { 74 } else { 114 };
        (ph - pied - 8 - haut).max(46)
    }

    /// Nom de plugin KWin unique pour un script jetable.
    fn nom_plugin(&mut self, base: &str) -> String {
        self.seq += 1;
        format!("gablue-{base}-{}-{}", self.inst, self.seq)
    }

    /// Remplit la page « fenêtres » depuis KWin (`lister.js` pousse une
    /// entrée par fenêtre puis `Fin`). L'attente est bornée : si KWin ne
    /// répond pas, on affiche une liste vide plutôt que de figer le panneau.
    /// Lit les fenêtres auprès de KWin, sans toucher à la page affichée : sert
    /// à la page « fenêtres » ET à l'activation d'une appli déjà ouverte.
    fn lire_fenetres(&mut self) -> Option<Vec<Fenetre>> {
        let plugin = self.nom_plugin("lister");
        let Some(kbus) = &self.kbus else {
            self.status = "FENETRES: KWIN INJOIGNABLE".to_string();
            return None;
        };
        let Some(chemin) = script_instance("lister.js", &self.bus) else {
            self.status = "FENETRES: SCRIPT ABSENT".to_string();
            return None;
        };
        // sentinelle : `fin` = usize::MAX tant que `Fin(n)` n'est pas arrivé
        if let Some(st) = &self.win_etat {
            if let Ok(mut p) = st.lock() {
                p.liste.clear();
                p.fin = usize::MAX;
            }
        }
        self.plugins.push(plugin.clone());
        if !kbus.lancer_nomme(&chemin, &plugin) {
            self.status = "FENETRES: ECHEC KWIN".to_string();
            return None;
        }
        let t0 = Instant::now();
        loop {
            let mut recu = false;
            if let Some(st) = &self.win_etat {
                if let Ok(p) = st.lock() {
                    recu = p.fin != usize::MAX;
                }
            }
            if recu || t0.elapsed() > Duration::from_millis(400) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // le script n'a plus rien à faire : la liste est arrivée (ou le délai
        // est écoulé) — on le décharge pour ne pas laisser de moteur JS vivre
        let _ = kbus.decharger(&plugin);
        let liste = match &self.win_etat {
            Some(st) => match st.lock() {
                Ok(p) => p.liste.clone(),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        };
        Some(liste)
    }

    /// Page « fenêtres » : lit la liste, précharge les icônes et l'affiche.
    fn charger_fenetres(&mut self) {
        let Some(liste) = self.lire_fenetres() else {
            return;
        };
        self.fenetres = liste;
        // trous possibles (un appel D-Bus perdu) : on n'indexe que le valide
        self.fen_items = self
            .fenetres
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.class.is_empty())
            .map(|(i, _)| Entree::Autre(i))
            .collect();
        // icônes préchargées : rasteriser un SVG pendant le dessin ferait
        // sauter le panneau (une seule fois par icône, cache disque ensuite)
        let cles: Vec<String> = self
            .fenetres
            .iter()
            .map(|f| format!("{}|{}", f.desktop, f.class))
            .collect();
        for (cle, f) in cles.iter().zip(self.fenetres.iter()) {
            if !self.icones.contains_key(cle) {
                let ic = charger_icone(&f.desktop, &f.class, &f.caption);
                self.icones.insert(cle.clone(), ic);
            }
        }

        if self.page != Page::Fenetres {
            self.fen_retour = self.page;
        }
        self.page = Page::Fenetres;
        self.selected = 0;
        self.fen_top = 0;
        self.arme = None;
        let n = self.fen_items.len();
        eprintln!("DBG apres attente : liste={} items={}", self.fenetres.len(), n);
        self.status = if n == 0 {
            "AUCUNE FENETRE".to_string()
        } else {
            format!("{n} FENETRE(S) - A POUR ACTIVER")
        };
        println!("FENETRES n={n}");
    }

    /// Rend active la fenêtre `i` de la liste (script KWin jetable sur son UUID).
    fn activer_fenetre(&mut self, i: usize) {
        let Some(f) = self.fenetres.get(i).cloned() else {
            self.status = "ACTIVER: FENETRE INCONNUE".to_string();
            return;
        };
        let ok = self.activer_par_uuid(&f.uuid);
        // on revient d'où l'on vient : la liste est un aller-retour
        self.page = self.fen_retour;
        self.selected = self.premier_action();
        self.status = if ok {
            format!("ACTIVE: {}", f.titre())
        } else {
            "ACTIVER: ECHEC KWIN".to_string()
        };
        println!("ACTIVER uuid={} ok={ok}", f.uuid);
    }

    /// Rend active la fenêtre d'UUID `uuid` (script KWin jetable).
    fn activer_par_uuid(&mut self, uuid: &str) -> bool {
        if uuid.is_empty() {
            self.status = "ACTIVER: UUID INCONNU".to_string();
            return false;
        }
        let plugin = self.nom_plugin("activer");
        let Some(kbus) = &self.kbus else {
            self.status = "ACTIVER: KWIN INJOIGNABLE".to_string();
            return false;
        };
        // UUID entre guillemets : c'est une chaîne `{…}` côté KWin
        let script = format!(
            "var ws = workspace.stackingOrder;\n             for (var i = 0; i < ws.length; i++) {{\n             \x20   if (String(ws[i].internalId) === \"{}\") {{ workspace.activeWindow = ws[i]; break; }}\n             }}\n",
            uuid
        );
        let chemin = format!(
            "/tmp/opencode/kwin-activer-{}.js",
            std::process::id()
        );
        let chemin = chemin.as_str();
        if std::fs::write(chemin, script).is_err() {
            self.status = "ACTIVER: ECRITURE IMPOSSIBLE".to_string();
            return false;
        }
        kbus.un_shot(chemin, &plugin)
    }

    /// Fenêtre d'une appli du menu, si elle est déjà ouverte. Un appui sur une
    /// appli qui tourne doit l'amener au premier plan, jamais la relancer ni la
    /// fermer (l'ancienne bascule la tuait).
    fn fenetre_appli(&mut self, nom: &str) -> Option<Fenetre> {
        let motifs = motifs_appli(nom);
        if motifs.is_empty() {
            return None;
        }
        let liste = self.lire_fenetres()?;
        liste.into_iter().find(|f| {
            let classe = f.class.to_lowercase();
            let desktop = f.desktop.to_lowercase();
            motifs
                .iter()
                .any(|m| classe.contains(m) || desktop.contains(m))
        })
    }

    /// Fenêtre précédente/suivante (L2/R2), sans ouvrir la liste.
    fn cycler_fenetre(&mut self, sens: i32) {
        let plugin = self.nom_plugin("cycle");
        let Some(kbus) = &self.kbus else {
            self.status = "FENETRE: KWIN INJOIGNABLE".to_string();
            return;
        };
        // Script **généré** : l'ancre est l'UUID de la fenêtre active (que
        // l'overlay connaît par wininfo), pas `workspace.activeWindow` — le
        // panneau peut être la fenêtre active, et `indexOf` renvoyait alors -1,
        // d'où un L2 qui retombait toujours sur la même fenêtre (constaté).
        // L'ordre de parcours est **stable** (gauche→droite, puis haut→bas) :
        // `stackingOrder` change à chaque activation, la fenêtre activée
        // remonte, donc il ne peut pas servir de repère (L2 rebondissait sur
        // les deux dernières fenêtres).
        let script = CYCLE_JS
            .replace("__CIBLE__", &self.win.uuid)
            .replace("__SENS__", if sens < 0 { "-1" } else { "1" });
        let chemin = format!("/tmp/opencode/kwin-cycle-{}.js", std::process::id());
        let chemin = chemin.as_str();
        if std::fs::write(chemin, script).is_err() {
            self.status = "FENETRE: ECRITURE IMPOSSIBLE".to_string();
            return;
        }
        let ok = kbus.un_shot(chemin, &plugin);
        self.status = if ok {
            if sens < 0 {
                "FENETRE: PRECEDENTE".to_string()
            } else {
                "FENETRE: SUIVANTE".to_string()
            }
        } else {
            "FENETRE: ECHEC KWIN".to_string()
        };
        println!("CYCLE {sens} ok={ok}");
    }

    /// Bascule le plein écran de la fenêtre active (script KWin jetable).
    fn basculer_plein_ecran(&mut self) {
        let plugin = self.nom_plugin("fs");
        let Some(kbus) = &self.kbus else {
            self.status = "PLEIN ECRAN: KWIN INJOIGNABLE".to_string();
            return;
        };
        let Some(chemin) = chemin_kwin("fullscreen.js") else {
            self.status = "PLEIN ECRAN: SCRIPT ABSENT".to_string();
            return;
        };
        let ok = kbus.un_shot(&chemin, &plugin);
        self.status = if ok {
            format!("PLEIN ECRAN: {}", if self.win.fullscreen { "SORTIE" } else { "ACTIVE" })
        } else {
            "PLEIN ECRAN: ECHEC".to_string()
        };
        println!("FULLSCREEN {ok}");
    }

    /// Ferme la fenêtre active : SIGKILL sur le processus **et ses descendants**.
    /// Un `kill` du seul PID de la fenêtre laissait les enfants vivants :
    /// émulateur lancé par ES-DE (enfant du frontend), sous-processus d'une
    /// AppImage, WebEngine d'une appli flatpak — reparentés à systemd, ils
    /// continuent de tourner sans fenêtre.
    fn fermer_fenetre(&mut self) {
        let pid = self.win.pid;
        self.arme = None;
        if pid <= 1 {
            self.status = "FERMER: PID INCONNU".to_string();
            return;
        }
        let enfants = descendants(pid);
        // Enfants d'abord : tuer le parent en premier les orphelinerait
        // (reparentés à systemd) avant qu'on ait pu les atteindre.
        let mut tues = 0;
        for p in enfants.iter().rev() {
            if unsafe { libc::kill(*p, libc::SIGKILL) } == 0 {
                tues += 1;
            }
        }
        let rc = unsafe { libc::kill(pid, libc::SIGKILL) };
        self.status = if rc == 0 {
            if tues > 0 {
                format!("FERMEE: PID {pid} +{tues} ENFANT(S)")
            } else {
                format!("FERMEE: PID {pid}")
            }
        } else {
            format!("FERMER: ECHEC ({})", std::io::Error::last_os_error())
        };
        println!("KILL {pid} enfants={tues} rc={rc}");
    }

    /// Volume du sink par défaut, appliqué en direct (5 % par cran).
    fn set_vol(&mut self, v: i32) {
        self.vol = v.clamp(0, 100);
        let _ = std::process::Command::new("/usr/bin/pactl")
            .args([
                "set-sink-volume",
                "@DEFAULT_SINK@",
                &format!("{}%", self.vol),
            ])
            .status();
        self.status = format!("VOLUME {}%", self.vol);
        println!("VOLUME {}", self.vol);
    }

    /// État « muet » du sink par défaut. `LC_ALL=C` : sinon la sortie est
    /// traduite (« Mute: non ») et un test sur « yes » rate en silence.
    fn lire_mute() -> bool {
        std::process::Command::new("/usr/bin/pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .env("LC_ALL", "C")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("yes"))
            .unwrap_or(false)
    }

    /// A sur la ligne VOLUME : muet / non muet (le niveau est conservé).
    fn toggle_mute(&mut self) {
        let _ = std::process::Command::new("/usr/bin/pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .env("LC_ALL", "C")
            .status();
        self.vol_mute = Self::lire_mute();
        self.status = if self.vol_mute {
            "VOLUME: MUET".to_string()
        } else {
            format!("VOLUME {}%", self.vol)
        };
        println!("MUTE {}", self.vol_mute);
    }

    /// Règle le volume d'un flux (jeu ou app compagne). On relit ensuite la
    /// valeur réelle : `pactl` peut borner ou refuser, l'écran ne doit pas
    /// afficher un chiffre inventé.
    fn regler_flux(&mut self, cible: Cible, delta: i32) {
        let flux = match cible {
            Cible::Jeu => self.son_jeu.clone(),
            Cible::App => self.son_app.clone(),
        };
        let Some(flux) = flux else {
            self.status = "CE FLUX N'EXISTE PLUS".to_string();
            self.refresh_sons();
            return;
        };
        let cible_pct = (flux.pourcent as i32 + delta).clamp(0, 100) as u8;
        if audio::regler(flux.index, cible_pct) {
            self.refresh_sons();
            self.status = format!(
                "{}: {}%",
                match cible {
                    Cible::Jeu => "VOLUME JEU",
                    Cible::App => "VOLUME APP",
                },
                cible_pct
            );
            println!("VOLUME FLUX {} {}%", flux.index, cible_pct);
        } else {
            self.status = "VOLUME: ECHEC".to_string();
        }
    }

    /// A sur une ligne de flux : muet / non muet, sans toucher aux autres.
    fn muter_flux(&mut self, cible: Cible) {
        let flux = match cible {
            Cible::Jeu => self.son_jeu.clone(),
            Cible::App => self.son_app.clone(),
        };
        let Some(flux) = flux else {
            self.status = "CE FLUX N'EXISTE PLUS".to_string();
            self.refresh_sons();
            return;
        };
        if audio::basculer_mute(flux.index) {
            self.refresh_sons();
            self.status = "MUET BASCULE".to_string();
        } else {
            self.status = "MUET: ECHEC".to_string();
        }
    }

    fn refresh_volume(&mut self) {
        self.vol_mute = Self::lire_mute();
        if let Ok(o) = std::process::Command::new("/usr/bin/pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .env("LC_ALL", "C")
            .output()
        {
            let t = String::from_utf8_lossy(&o.stdout);
            if let Some(p) = t.split('%').next().and_then(|h| h.rsplit(' ').next()) {
                if let Ok(v) = p.trim().parse::<i32>() {
                    self.vol = v.clamp(0, 100);
                }
            }
        }
    }

    /// Deux styles (RETRO / MODERNE) : un appui bascule, la ligne s'appelle
    /// « Style retro » / « Style moderne » et l'interrupteur dit lequel.
    fn basculer_style(&mut self) {
        let i = 1 - self.style_idx.min(1);
        self.style_idx = i;
        self.th = theme_pour(i);
        // MODERNE : la police n'est chargée qu'une fois
        self.police = if i == 1 {
            self.police.take().or_else(charger_police)
        } else {
            None
        };
        ecrire_style(STYLE_KEYS[i]);
        self.status = format!("STYLE: {}", STYLE_OPTS[i]);
        println!(
            "STYLE {} ({}) police={}",
            STYLE_OPTS[i],
            STYLE_KEYS[i],
            if self.police.is_some() { "ttf" } else { "bitmap" }
        );
    }

    /// Lit l'état du VRR (`vrr.sh get` : `vrr=auto|never|mixte|indispo`).
    /// `mixte` (une sortie en auto, une en never) compte comme actif : la ligne
    /// dit « quelque chose est synchronisé », et un appui repasse tout en auto.
    fn refresh_vrr(&mut self) {
        if let Ok(p) = std::env::var("VRR") {
            if let Ok(o) = std::process::Command::new(&p).arg("get").output() {
                let t = String::from_utf8_lossy(&o.stdout);
                self.vrr_on = matches!(t.trim(), "vrr=auto" | "vrr=mixte");
            }
        }
    }

    /// Lit l'état du booster « premier plan » (`booster.sh get` :
    /// `booster=on|off`). La clé absente de `kcgroupsrc` vaut **on** (défaut de
    /// Plasma, et c'est ainsi que Gablue Manager la lit).
    fn refresh_booster(&mut self) {
        if let Ok(p) = std::env::var("BOOSTER") {
            if let Ok(o) = std::process::Command::new(&p).arg("get").output() {
                let t = String::from_utf8_lossy(&o.stdout);
                self.booster_on = t.trim() == "booster=on";
            }
        }
    }

    /// Bascule le booster. Pas de relecture en boucle : le script écrit la clé
    /// et relance/arrête le service utilisateur, on relit son verdict.
    fn basculer_booster(&mut self) {
        self.status = run_script_args("BOOSTER", &["toggle"]);
        self.refresh_booster();
        println!(
            "BOOSTER {}",
            if self.booster_on { "actif" } else { "inactif" }
        );
    }

    /// Lit les réglages MangoHud pour positionner les vues sur l'état réel.
    fn refresh_mangohud(&mut self) {
        if let Ok(p) = std::env::var("MANGOHUD") {
            if let Ok(o) = std::process::Command::new(&p).arg("get").output() {
                let t = String::from_utf8_lossy(&o.stdout);
                for l in t.lines() {
                    if let Some(v) = l.strip_prefix("fps=") {
                        if let Ok(f) = v.trim().parse::<i32>() {
                            // Hors crans (MangoJuice, dialogue « MangoHud… » de
                            // Gablue Manager) : la valeur est gardée pour
                            // l'affichage et le curseur se pose sur le cran le
                            // plus proche — muet, il annonçait « 60 » pour un
                            // fichier à 144.
                            self.fps_reel = if FPS_OPTS.iter().any(|(_, x)| *x == f) {
                                None
                            } else {
                                Some(f)
                            };
                            self.fps_idx = FPS_OPTS
                                .iter()
                                .position(|(_, x)| *x == f)
                                .unwrap_or_else(|| {
                                    FPS_OPTS
                                        .iter()
                                        .enumerate()
                                        .min_by_key(|(_, (_, x))| (x - f).abs())
                                        .map(|(i, _)| i)
                                        .unwrap_or(0)
                                });
                        }
                    }
                    if let Some(v) = l.strip_prefix("vue=") {
                        if let Some(i) = MH_OPTS.iter().position(|(_, x)| *x == v.trim()) {
                            self.mh_idx = i;
                        }
                    }
                }
            }
        }
    }

    fn apply_menu(&mut self, a: Action) {
        // Sur une ligne de réglage, gauche/droite règle immédiatement ;
        // haut/bas continuent de naviguer : on se pose, on règle, on repart.
        // page vide (aucune fenêtre listée) : pas d'entrée à régler
        if self
            .liste()
            .get(self.selected)
            .map(|e| e.reglage())
            .unwrap_or(false)
        {
            match a {
                Action::Left => {
                    self.regler(-1);
                    self.ajuster_defilement();
                    return;
                }
                Action::Right => {
                    self.regler(1);
                    self.ajuster_defilement();
                    return;
                }
                _ => {}
            }
        }
        match a {
            Action::Page(sens) => {
                // R1 avance, L1 recule (les deux allaient dans le même sens) ;
                // une catégorie et la liste des fenêtres sont des allers-retours :
                // L1/R1 en sortent, vers la page de premier niveau
                let base = match self.page {
                    Page::Fenetres | Page::Fenetre => self.fen_retour,
                    Page::Categorie(_) => Page::Accueil,
                    p => p,
                };
                let i = ORDRE_PAGES.iter().position(|p| *p == base).unwrap_or(0) as i32;
                let j = (i + sens).rem_euclid(ORDRE_PAGES.len() as i32) as usize;
                self.aller_page(ORDRE_PAGES[j], sens);
            }
            Action::Up => {
                self.selected = self.pas_selectable(self.selected, -1);
                self.ajuster_defilement();
                println!("NAV UP {}", self.selected);
            }
            Action::Down => {
                self.selected = self.pas_selectable(self.selected, 1);
                self.ajuster_defilement();
                println!("NAV DOWN {}", self.selected);
            }
            Action::Left | Action::Right => {
                self.status = format!("{a:?} SANS EFFET");
                println!("NAV {a:?}");
            }
            // L2/R2 : panneau ouvert et application compagne présente → on
            // bascule jeu ⇄ app (le geste du quotidien : regarder une vidéo à
            // côté du jeu). Sinon, défilement des fenêtres comme avant.
            Action::Cycle(sens) => {
                if self.visible && !self.closing {
                    // les fenêtres ont pu changer depuis l'ouverture du panneau
                    self.refresh_bascule();
                }
                if self.visible && !self.closing && self.bascule_app.is_some() {
                    self.basculer();
                } else {
                    self.cycler_fenetre(sens);
                }
            }
            Action::Accept => self.run_item(),
            // Une confirmation est en attente : B ne fait QUE la désarmer.
            // Il ne doit ni revenir en arrière ni fermer le panneau (demandé) —
            // c'est le geste « annuler » de la ligne rouge.
            Action::Cancel if self.arme.is_some() => {
                if let Some(nom) = self.arme.take() {
                    println!("DESARME ({nom}) par B");
                }
                self.status = "PRET".to_string();
            }
            Action::Cancel if self.page == Page::Fenetre => {
                // on revient d'où l'on vient, **le curseur sur le nom de la
                // fenêtre** : c'est de là qu'on a ouvert la page, on doit
                // pouvoir relancer/repartir de là (demandé à l'usage)
                self.page = self.fen_page_retour;
                let sel = self
                    .index_entete()
                    .unwrap_or_else(|| self.premier_action());
                self.selected = sel;
                self.arme = None;
                self.status = "RETOUR".to_string();
                println!("FENETRE retour {:?} sel={sel}", self.page);
            }
            Action::Cancel if self.page == Page::Fenetres => {
                // la liste est un aller-retour : B revient sur la page qui l'a
                // ouverte au lieu de fermer tout le panneau (constaté : on
                // quittait le menu pour un simple retour en arrière)
                self.page = self.fen_retour;
                self.selected = self.premier_action();
                self.fen_top = 0;
                self.arme = None;
                self.status = "RETOUR".to_string();
                println!("FENETRES retour {:?}", self.page);
            }
            Action::Cancel if self.est_categorie() => {
                // B dans une catégorie : retour à l'accueil, **curseur sur la
                // catégorie** d'où l'on vient (on enchaîne sur la voisine)
                let cat = match self.page {
                    Page::Categorie(cat) => cat,
                    _ => unreachable!(),
                };
                self.page = Page::Accueil;
                self.selected = self
                    .accueil
                    .iter()
                    .position(|e| matches!(e, Entree::SousMenu(_, c) if *c == cat))
                    .unwrap_or(0);
                self.reg_top = 0;
                self.arme = None;
                self.status = "RETOUR".to_string();
                println!("CATEGORIE retour {:?}", cat);
            }
            Action::Cancel => {
                self.status = "FERMETURE".to_string();
                println!("CLOSE (manette)");
                self.request_close();
            }
        }
    }

    /// Glyphe embarqué de la famille du pad connecté (`None` tant que le
    /// style MODERNE n'a pas chargé ses images).
    fn glyphe(&self, nom: &str) -> Option<&Icone> {
        self.glyphes
            .iter()
            .find(|(n, _)| *n == nom)
            .map(|(_, ic)| ic)
    }

    /// Largeur qu'occuperait une suite d'items d'aide, sans rien dessiner.
    fn aide_largeur(&self, cv: &Canvas, items: &[(&str, &str)]) -> i32 {
        let mut w = 0;
        for (g, label) in items {
            // `l1|r1` = deux glyphes accolés (paire de gâchettes)
            for nom in g.split('|') {
                if self.glyphe(nom).is_some() {
                    w += TAILLE_GLYPHE + 3;
                }
            }
            if !label.is_empty() {
                w += 2 + cv.largeur(label, 2);
            }
            w += 16;
        }
        (w - 16).max(0)
    }

    /// Ligne d'aide « glyphe + libellé », dans la police active. Retourne la
    /// largeur consommée.
    fn aide_ligne(
        &self,
        cv: &mut Canvas,
        x: i32,
        y: i32,
        items: &[(&str, &str)],
        color: u32,
    ) -> i32 {
        let mut cx = x;
        for (g, label) in items {
            for nom in g.split('|') {
                if let Some(ic) = self.glyphe(nom) {
                    // glyphe dans SES couleurs (vert A, rouge B, bleu X…) :
                    // teinté d'une seule couleur, la lettre évidée du disque
                    // disparaît à cette taille (constaté, illisible)
                    cv.icone(cx, y - 7, TAILLE_GLYPHE, ic);
                    cx += TAILLE_GLYPHE + 3;
                }
            }
            if label.is_empty() {
                cx += 13; // sépare deux groupes de glyphes
                continue;
            }
            cx += 2;
            cv.text(cx, y, label, 2, color);
            cx += cv.largeur(label, 2) + 16;
        }
        cx - x
    }

    /// Change de page en lançant le glissement horizontal du **contenu**
    /// (l'en-tête de fenêtre reste en place, comme demandé).
    fn aller_page(&mut self, p: Page, sens: i32) {
        let ancienne = self.page;
        if p == ancienne {
            return;
        }
        self.pdep = Some((ancienne, p, Instant::now(), sens));
        self.page = p;
        self.selected = if p == Page::Fenetres {
            0
        } else {
            self.premier_action()
        };
        self.fen_top = 0;
        self.arme = None;
        self.status = match p {
            Page::Accueil => "MENU: CATEGORIES".to_string(),
            Page::Categorie(cat) => format!("MENU: {}", cat.nom()),
            Page::Applis => "MENU: APPLICATIONS".to_string(),
            Page::Fenetre => "MENU: FENETRE".to_string(),
            Page::Fenetres => "MENU: FENETRES".to_string(),
        };
        println!("PAGE {:?} sens={sens}", p);
    }

    /// Ouvre une catégorie. Son contenu est figé à l'entrée (reconstruit par
    /// `maj_accueil` s'il change), le curseur va sur la première action.
    ///
    /// **Pas de glissement** : l'animation de `aller_page` rejoue l'ancienne
    /// liste en changeant `page`, or toutes les catégories partagent le même
    /// `cat_items` — l'ancienne serait dessinée avec le contenu de la
    /// nouvelle (deux menus mélangés).
    fn entrer_categorie(&mut self, cat: Cat) {
        // les flux audio changent à chaque lancement de jeu ou de vidéo :
        // on relit à l'entrée de la catégorie qui les propose
        if cat == Cat::Fenetres {
            self.refresh_bascule();
        }
        if cat == Cat::Son {
            if std::env::var_os("GPAD_SON_DEMO").is_some() {
                // crochet de banc : deux flux factices, pour photographier les
                // trois lignes sans lancer un jeu et une vidéo
                self.son_jeu = Some(audio::Flux {
                    index: 9001,
                    pid: 1,
                    nom: "gwine".to_string(),
                    pourcent: 80,
                    mute: false,
                });
                self.son_app = Some(audio::Flux {
                    index: 9002,
                    pid: 2,
                    nom: "VacuumTube".to_string(),
                    pourcent: 35,
                    mute: false,
                });
            } else {
                self.refresh_sons();
            }
        }
        let items = cat.entrees(self.dispo());
        self.cat_items = items;
        self.page = Page::Categorie(cat);
        self.pdep = None;
        self.reg_top = 0;
        self.selected = self.premier_action();
        self.arme = None;
        self.status = format!("MENU: {}", cat.nom());
        println!("PAGE {:?}", cat);
    }

    fn est_categorie(&self) -> bool {
        matches!(self.page, Page::Categorie(_))
    }

    /// Ce qui existe maintenant : pilote les lignes conditionnelles.
    fn dispo(&self) -> Dispo {
        Dispo {
            replay: self.replay_active,
            manette: self.pad_bt.is_some(),
            jeu: self.son_jeu.is_some(),
            app: self.son_app.is_some(),
            bascule: self.bascule_app.is_some(),
            console: self.console,
        }
    }

    /// Relit les flux audio : celui du jeu (PID du lanceur et ses descendants)
    /// et celui d'une application compagne (vidéo lancée à côté). Un flux
    /// n'existe que pendant la lecture : une app ouverte mais muette n'en a
    /// pas, et sa ligne n'est alors pas proposée.
    /// Relit les fenêtres pour le sélecteur jeu ⇄ application compagne : celle
    /// du jeu (PID du paquet et descendants) et celle de l'app (nom reconnu
    /// par `audio::est_compagne`). Un flux audio n'est pas nécessaire — une
    /// app ouverte mais muette a aussi une fenêtre, et on peut vouloir y
    /// retourner.
    fn refresh_bascule(&mut self) {
        self.bascule_app = None;
        self.bascule_jeu = None;
        self.bascule_sur_app = false;
        // crochet de banc : deux fenêtres factices, pour photographier la
        // ligne sans jeu ni application ouverte
        if std::env::var_os("GPAD_BASCULE_DEMO").is_some() {
            let fenetre = |nom: &str, classe: &str, pid: i32, uuid: &str, active: bool| Fenetre {
                caption: nom.to_string(),
                class: classe.to_string(),
                desktop: classe.to_string(),
                pid,
                uuid: uuid.to_string(),
                active,
            };
            self.bascule_app = Some(fenetre(
                "VacuumTube",
                "rocks.shy.VacuumTube",
                4242,
                "{app}",
                false,
            ));
            self.bascule_jeu = Some(fenetre("Super Game", "gwine", 4243, "{jeu}", true));
            return;
        }
        let Some(liste) = self.lire_fenetres() else {
            return;
        };
        let compagne = |f: &Fenetre| {
            audio::est_compagne(&f.class)
                || audio::est_compagne(&f.desktop)
                || audio::est_compagne(&f.caption)
        };
        self.bascule_app = liste.iter().find(|f| compagne(f)).cloned();
        // Tous les jeux publiés, pas seulement celui de la fenêtre active :
        // une app compagne au premier plan fait `jeu_actif()` = None, et la
        // bascule retombait alors sur le cyclage des fenêtres au lieu de
        // reconnaître le jeu qui tourne derrière (constaté le 23/11/2026).
        let mut pids: Vec<i32> = Vec::new();
        for jeu in lire_jeux_actifs() {
            for racine in jeu.pids {
                pids.push(racine);
                pids.extend(descendants(racine));
            }
        }
        self.bascule_jeu = liste
            .iter()
            .find(|f| f.pid > 0 && pids.contains(&f.pid))
            .cloned();
        self.bascule_sur_app = liste
            .iter()
            .find(|f| f.active)
            .is_some_and(|f| compagne(f));
        println!(
            "BASCULE app={:?} jeu={:?} sur_app={}",
            self.bascule_app.as_ref().map(|f| f.titre().to_string()),
            self.bascule_jeu.as_ref().map(|f| f.titre().to_string()),
            self.bascule_sur_app
        );
    }

    /// Passe du jeu à l'application compagne, ou l'inverse, puis referme le
    /// panneau.
    ///
    /// Activer la fenêtre ne suffit PAS : celle du jeu est **plein écran**, et
    /// KWin la garde dans une couche au-dessus des fenêtres normales — l'app
    /// activée restait derrière et l'écran semblait revenir au jeu (constaté
    /// le 23/11/2026 avec Gablue TV et Jellyfin). Le plein écran du jeu est
    /// donc retiré en allant vers l'app et remis en revenant ; l'activation
    /// part dans le MÊME script, sinon deux chargements KWin se marchent
    /// dessus. `keepAbove` est levé au passage : le launcher s'en sert le temps
    /// du démarrage du jeu, jamais après.
    fn basculer(&mut self) {
        if self.bascule_jeu.is_none() || self.bascule_app.is_none() {
            self.status = "BASCULER: JEU OU APP ABSENT".to_string();
            return;
        }
        // sur l'app → on revient au jeu ; sur le jeu → on va à l'app
        let vers_app = !self.bascule_sur_app;
        if self.echanger(vers_app) {
            self.request_close();
        }
    }

    /// Cœur de la bascule jeu ⇄ app, sans panneau : script KWin (le plein
    /// écran du jeu est retiré en allant vers l'app, remis en revenant) puis
    /// le son du jeu. Renvoie `false` si KWin n'a pas pris le script.
    fn echanger(&mut self, vers_app: bool) -> bool {
        let (Some(jeu), Some(app)) = (self.bascule_jeu.clone(), self.bascule_app.clone()) else {
            return false;
        };
        let cible = if vers_app { &app } else { &jeu };
        let jeu_plein_ecran = !vers_app;
        let plugin = self.nom_plugin("bascule");
        let Some(kbus) = &self.kbus else {
            self.status = "BASCULER: KWIN INJOIGNABLE".to_string();
            return false;
        };
        let script = script_bascule(&jeu.uuid, &cible.uuid, jeu_plein_ecran);
        let chemin = format!("/tmp/opencode/kwin-bascule-{}.js", std::process::id());
        if std::fs::write(&chemin, script).is_err() {
            self.status = "BASCULER: ECRITURE IMPOSSIBLE".to_string();
            return false;
        }
        if !kbus.un_shot(&chemin, &plugin) {
            self.status = "BASCULER: ECHEC KWIN".to_string();
            return false;
        }
        // Le son suit la fenêtre : en allant vers l'app, le jeu passe en
        // sourdine (sinon les deux se superposent) ; au retour il est rendu, et
        // l'app continue de sonner — on entend les deux.
        let muets = self.muter_jeu(vers_app);
        self.jeu_muet = vers_app;
        self.status = format!(
            "BASCULE: {} ({} flux {})",
            cible.titre(),
            muets,
            if vers_app { "coupés" } else { "rendus" }
        );
        println!(
            "BASCULE -> {} (jeu plein écran: {}, {muets} flux en sourdine: {vers_app})",
            cible.titre(),
            jeu_plein_ecran
        );
        true
    }

    /// Bascule demandée hors panneau (Home+L2 / Home+R2, commande `bascule`
    /// du déclencheur). Deux cas :
    ///
    /// - un jeu tourne et une application compagne est ouverte → bascule jeu
    ///   ⇄ app, la **direction** choisissant la cible (R2 vers l'app, L2 vers
    ///   le jeu, comme dans le panneau) ; si la cible est déjà devant, rien ne
    ///   se passe ;
    /// - sinon (bureau, ou aucun jeu) → cyclage de **toutes** les fenêtres dans
    ///   l'ordre où elles sont empilées, R2 en avant, L2 en arrière.
    fn bascule_combo(&mut self, sens: i32) {
        // le jeu a pu démarrer depuis le dernier rafraîchissement du panneau :
        // sans relecture, `running.txt` n'était pas encore connu et la bascule
        // aurait cyclé les fenêtres au lieu de passer à l'app
        self.rafraichir_jeu();
        self.refresh_bascule();
        if self.bascule_jeu.is_some() && self.bascule_app.is_some() {
            // Pas de garde « déjà sur la cible » : une app lancée pendant une
            // partie devient active **sous** la fenêtre plein écran du jeu, donc
            // `bascule_sur_app` est vrai alors que l'écran montre le jeu. La
            // direction demandée s'applique toujours — rejouer la même est
            // idempotente (plein écran, activation, sourdine).
            self.echanger(sens > 0);
            return;
        }
        self.cycler_fenetres(sens);
    }

    /// Active la fenêtre suivante (`sens > 0`) ou précédente dans la pile —
    /// script KWin jetable, même ancre que le cyclage du panneau (UUID de la
    /// fenêtre active, jamais devinée).
    fn cycler_fenetres(&mut self, sens: i32) {
        let plugin = self.nom_plugin("cycle");
        let Some(kbus) = &self.kbus else {
            println!("CYCLE: KWIN INJOIGNABLE");
            return;
        };
        let script = CYCLE_JS
            .replace("__CIBLE__", &self.win.uuid)
            .replace("__SENS__", &sens.to_string());
        let chemin = format!("/tmp/opencode/kwin-cycle-{}.js", std::process::id());
        if std::fs::write(&chemin, script).is_err() {
            println!("CYCLE: ECRITURE IMPOSSIBLE");
            return;
        }
        println!("CYCLE sens={sens} ok={}", kbus.un_shot(&chemin, &plugin));
    }

    /// Met en sourdine (ou rend) le son du jeu, flux par flux.
    ///
    /// Muet et pas volume à 0 : le jeu garde son réglage, le retour est exact.
    /// Renvoie le nombre de flux touchés — 0 signifie « le jeu n'émet rien »
    /// (jeu en pause, son coupé dans le jeu), pas une erreur.
    fn muter_jeu(&mut self, muet: bool) -> usize {
        let Some(jeu) = self.bascule_jeu.clone() else {
            return 0;
        };
        if jeu.pid <= 1 {
            return 0;
        }
        let mut pids = vec![jeu.pid];
        pids.extend(descendants(jeu.pid));
        let flux = flux_du_jeu(&audio::flux(), &pids);
        let mut faits = 0;
        for f in &flux {
            if audio::muet(f.index, muet) {
                faits += 1;
            }
        }
        faits
    }

    fn refresh_sons(&mut self) {
        let mut pids: Vec<i32> = Vec::new();
        if let Some(jeu) = self.jeu_actif() {
            for racine in jeu.pids.clone() {
                pids.push(racine);
                pids.extend(descendants(racine));
            }
        }
        let flux = audio::flux();
        self.son_jeu = flux.iter().find(|f| f.pid > 0 && pids.contains(&f.pid)).cloned();
        self.son_app = flux
            .iter()
            .find(|f| !f.nom.is_empty() && audio::est_compagne(&f.nom))
            .cloned();
        println!(
            "SON jeu={:?} app={:?}",
            self.son_jeu.as_ref().map(|f| (f.index, f.pourcent)),
            self.son_app.as_ref().map(|f| (f.index, f.pourcent))
        );
    }

    /// Exécute l'entrée sélectionnée. Ce qui n'est pas câblé le dit
    /// franchement plutôt que de faire semblant.
    fn run_item(&mut self) {
        // une page peut être vide (aucune fenêtre listée) : ne rien faire
        if self.selected >= self.liste().len() {
            self.status = "LISTE VIDE".to_string();
            return;
        }
        let item = self.liste()[self.selected];
        println!("SELECT {}", item.nom());
        // Entrées irréversibles : le premier A arme (la ligne passe en rouge
        // avec « ? »), le second exécute. Naviguer désarme.
        if let Entree::Action(nom) = item {
            if demande_confirmation(nom) && self.arme.as_deref() != Some(nom) {
                self.arme = Some(nom.to_string());
                self.status = format!("{nom} ? (A pour confirmer)");
                println!("ARME {nom}");
                return;
            }
        }
        self.arme = None;
        match item {
            // les lignes de réglage n'ont rien à valider : gauche/droite agit
            Entree::Volume => self.toggle_mute(),
            Entree::VolumeJeu => self.muter_flux(Cible::Jeu),
            Entree::VolumeApp => self.muter_flux(Cible::App),
            Entree::Basculer => self.basculer(),
            Entree::Fps => self.status = "FPS: GAUCHE/DROITE".to_string(),
            Entree::Duree => self.status = "DUREE REPLAY: GAUCHE/DROITE".to_string(),
            Entree::Codec
            | Entree::Qualite
            | Entree::ShotFormat
            | Entree::ShotQualite
            | Entree::Reso
            | Entree::FpsRec
            | Entree::Curseur
            | Entree::AutoReplay => self.status = "GAUCHE/DROITE POUR REGLER".to_string(),
            Entree::Mango => self.regler(1),
            Entree::Style => self.basculer_style(),
            Entree::Booster => self.basculer_booster(),
            Entree::Appli(nom) => {
                // Appui répété pendant que l'appli démarre : elle n'est pas
                // encore visible par `pgrep`, donc le script la relancerait.
                // On ignore tout appui sur la même entrée pendant 2,5 s.
                if let Some((dernier, quand)) = &self.derniere_appli {
                    if dernier == nom && quand.elapsed() < Duration::from_millis(2500) {
                        self.status = format!("{nom}: DEJA EN COURS");
                        println!("APPLI {nom} ignoree (delai)");
                        return;
                    }
                }
                self.derniere_appli = Some((nom.to_string(), Instant::now()));
                // Déjà ouverte : on l'active. Un second appui ne doit JAMAIS
                // la fermer (contrairement à l'ancienne bascule).
                if let Some(f) = self.fenetre_appli(nom) {
                    let ok = self.activer_par_uuid(&f.uuid);
                    self.status = if ok {
                        format!("{nom}: ACTIVE")
                    } else {
                        format!("{nom}: ACTIVATION KO")
                    };
                    println!("APPLI {nom} active uuid={} ok={ok}", f.uuid);
                    // le panneau a fait son travail : il se retire, il ne
                    // reste pas par-dessus l'appli qu'on vient de choisir
                    if ok {
                        self.request_close();
                    }
                    return;
                }
                let script = match nom {
                    "GLAUNCHER" => "toggle-glauncher",
                    "EMULATIONSTATION" => "toggle-es",
                    "YOUTUBE" => "toggle-yt",
                    "GABLUE TV" => "toggle-tv",
                    "JELLYFIN" => "toggle-jellyfin",
                    _ => "",
                };
                if script.is_empty() {
                    self.status = format!("{nom}: RIEN A FAIRE");
                } else {
                    self.status = lancer_appli(script);
                    // choix fait : l'overlay se ferme tout de suite, l'appli
                    // démarre derrière (constaté : il restait affiché par-dessus
                    // le jeu lancé)
                    self.request_close();
                }
            }
            // A sur une catégorie : on l'ouvre (aucun redémarrage de liste,
            // juste la page qui change ; le gate BPF suit le panneau, pas la
            // profondeur du menu)
            Entree::SousMenu(_, cat) => self.entrer_categorie(cat),
            // A sur le nom de la fenêtre : proposer de la fermer ou de
            // basculer son plein écran (sur toutes les pages)
            // sans fenêtre active, la boîte n'aurait rien à fermer : on va
            // directement chercher une fenêtre
            Entree::Fenetre if !self.a_fenetre() => {
                self.status = "AUCUNE FENETRE: CHOISIR".to_string();
                println!("SELECT FENETRE (aucune)");
                self.charger_fenetres();
            }
            // fenêtre active : page dédiée (même présentation que la liste des
            // fenêtres — plus de boîte flottante)
            Entree::Fenetre => {
                if self.page != Page::Fenetre {
                    self.fen_page_retour = self.page;
                }
                self.aller_page(Page::Fenetre, 1);
            }
            Entree::Autre(i) => self.activer_fenetre(i),
            Entree::Action(nom) => match nom {
                // choix d'une fenêtre parmi celles d'KWin (l'ancien A sur
                // l'en-tête) : la boîte de l'en-tête ne sert plus qu'aux
                // actions de la fenêtre courante
                "CHOISIR FENETRE" => self.charger_fenetres(),
                "CAPTURE ECRAN" => {
                    // même script que le combo Home+droite : capture + son
                    match std::env::var("SCREENSHOT") {
                        Ok(sc) if std::path::Path::new(&sc).exists() => {
                            self.status = match std::process::Command::new("/usr/bin/sh")
                                .arg("-c")
                                .arg(format!("'{sc}'"))
                                .spawn()
                            {
                                Ok(_) => "CAPTURE: ENVOYEE".to_string(),
                                Err(e) => format!("CAPTURE KO: {e}"),
                            };
                        }
                        _ => self.status = "SCREENSHOT NON CONFIGURE".to_string(),
                    }
                }
                "VRR" => {
                    // bascule, puis on relit l'état réel (kscreen applique de
                    // façon asynchrone : le script attend avant de répondre)
                    self.status = run_script_args("VRR", &["toggle"]);
                    self.refresh_vrr();
                    println!("VRR {}", if self.vrr_on { "auto" } else { "desactive" });
                }
                "REPLAY" => {
                    // Indépendant de l'enregistrement : un seul pipeline encode,
                    // l'anneau TS et le mp4 écrivent côte à côte. On démarre ou
                    // on arrête le buffer même pendant une capture.
                    self.status = if self.replay_active {
                        "REPLAY: ARRET".to_string()
                    } else {
                        "REPLAY: DEMARRAGE".to_string()
                    };
                    if self.replay_active {
                        lancer_script("REPLAY", &["stop"]);
                    } else {
                        lancer_script_env(
                            "REPLAY",
                            &["start"],
                            &[("REPLAY_SEC", REPR_OPTS[self.repr_idx].0)],
                        );
                    }
                    self.replay_active = !self.replay_active;
                    self.maj_accueil();
                    self.replay_attente = Some(Instant::now() + Duration::from_secs(6));
                    println!("REPLAY {}", if self.replay_active { "start" } else { "stop" });
                }
                "SAUVER REPLAY" => {
                    // le buffer doit tourner : la ligne le dit (INACTIF) plutôt
                    // que de disparaître, on garde une place stable dans le menu
                    if !self.replay_active {
                        self.status = "REPLAY: INACTIF".to_string();
                        println!("SAUVER REPLAY refuse (buffer arrete)");
                        return;
                    }
                    let node = self.pad_path.clone().unwrap_or_default();
                    lancer_script_env("REPLAY", &["save"], &[("PAD_NODE", &node)]);
                    self.status = "REPLAY: SAUVEGARDE".to_string();
                    println!("REPLAY save (ligne sauver)");
                }
                "ENREGISTRER UNE VIDEO" => match std::env::var("RECORDER") {
                    Ok(r) if std::path::Path::new(&r).exists() => {
                        if self.rec_active {
                            // arrêt : rien ne se filme, le panneau peut rester
                            // (détaché : la finalisation du fichier peut attendre)
                            match std::process::Command::new("/usr/bin/sh")
                                .arg("-c")
                                .arg(format!("'{r}' toggle"))
                                .spawn()
                            {
                                Ok(_) => self.status = "ENREGISTRER: ARRET".to_string(),
                                Err(e) => self.status = format!("ENREGISTRER KO: {e}"),
                            }
                        } else {
                            // démarrage : le panneau se ferme AVANT, sinon il se
                            // filme lui-même (même règle que Home+gauche)
                            self.combo_apres = Some(Apres::Record);
                            self.status = "ENREGISTREMENT: FERMETURE".to_string();
                            println!("ENREGISTRER -> fermeture puis record");
                            self.request_close();
                        }
                    }
                    _ => self.status = "RECORDER NON CONFIGURE".to_string(),
                },
                "PLEIN ECRAN" => self.basculer_plein_ecran(),
                "FERMER" => self.fermer_fenetre(),
                "TOUT FERMER" => {
                    // killthemall : jeux Windows/Wine/émulateurs de l'utilisateur.
                    // Après fermeture : le menu ne doit pas rester à l'écran
                    // pendant que les jeux meurent.
                    self.status = "TOUT FERMER".to_string();
                    self.combo_apres = Some(Apres::ToutFermer);
                    println!("CONSOLE killthemall");
                    self.request_close();
                }
                "REDEMARRER" => {
                    self.status = "REDEMARRAGE".to_string();
                    self.combo_apres = Some(Apres::Reboot);
                    println!("CONSOLE reboot");
                    self.request_close();
                }
                "MISE EN VEILLE" => {
                    self.status = "VEILLE".to_string();
                    self.combo_apres = Some(Apres::Veille);
                    println!("CONSOLE suspend");
                    self.request_close();
                }
                "ETEINDRE" => {
                    self.status = "EXTINCTION".to_string();
                    self.combo_apres = Some(Apres::Eteindre);
                    println!("CONSOLE poweroff");
                    self.request_close();
                }
                "DECONNECTER LA MANETTE" => {
                    // Le menu se retire TOUT DE SUITE : `bluetoothctl` peut
                    // mettre une seconde à rendre la main, et l'ancien appel
                    // bloquant laissait le menu à l'écran jusqu'à la disparition
                    // effective de la manette (constaté). La commande part
                    // détachée, donc rien à différer : `gpad-trigger` peut tuer
                    // le résident dès que la manette part sans perdre l'action.
                    let mac = self.pad_bt.clone().unwrap_or_default();
                    println!("BT DISCONNECT {mac}");
                    self.status = bluetooth_disconnect(&mac);
                    self.request_close();
                }
                "MODE JEU" | "MODE BUREAU" => {
                    // la bascule tue la session : le panneau doit avoir
                    // disparu avant (même règle que les sorties)
                    let (apres, mode) = if nom == "MODE JEU" {
                        (Apres::ModeJeu, "console")
                    } else {
                        (Apres::ModeBureau, "bureau")
                    };
                    self.status = format!("BASCULE {nom}");
                    self.combo_apres = Some(apres);
                    println!("SYSTEME bascule {mode}");
                    self.request_close();
                }
                "FERMER LA SESSION" => {
                    // rien à filmer ici, mais le menu ne doit pas rester à
                    // l'écran pendant que les applications se ferment
                    self.status = "FERMETURE SESSION".to_string();
                    self.combo_apres = Some(Apres::DeconnecterSession);
                    println!("CONSOLE fermeture de session");
                    self.request_close();
                }
                "CLAVIER VIRTUEL" => self.clavier_virtuel(),
                "MODE SOURIS" => self.mouse_toggle(),
                _ => self.status = format!("{nom}: RIEN A FAIRE"),
            },
        }
    }

    /// État de l'enregistreur, lu sur la socket de `gpad-rec` (plus de
    /// `pgrep`, plus de fichier d'état : le démon est la seule vérité).
    fn refresh_recorder(&mut self) {
        let etat = capture::etat();
        let rec = etat.as_ref().is_some_and(|e| e.recording);
        if rec != self.rec_active {
            self.rec_active = rec;
            // à l'ouverture du panneau pendant une capture, on nomme le fichier
            if rec && let Some(e) = etat.as_ref() {
                let nom = std::path::Path::new(&e.file)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !nom.is_empty() {
                    self.status = format!("REC: {nom}");
                }
            }
            self.dirty = true;
        }
        self.rec_fichier = etat.as_ref().map(|e| e.file.clone()).unwrap_or_default();
        // réglage en attente : le démon garde la config jusqu'à la fermeture du
        // mp4 (changer le codec au milieu finaliserait le fichier en cours)
        let attente = etat.as_ref().is_some_and(|e| e.pending);
        if attente != self.rec_pending {
            self.rec_pending = attente;
            self.status = if attente {
                "REGLAGE: APPLIQUE APRES L'ENREGISTREMENT".to_string()
            } else {
                "REGLAGE: APPLIQUE".to_string()
            };
            self.dirty = true;
        }
        // durée du buffer : le démon fait foi (le déclencheur peut la changer)
        if let Some(s) = etat.as_ref().map(|e| e.replay_sec)
            && let Some(i) = REPR_OPTS.iter().position(|(_, v)| *v as i32 == s)
            && i != self.repr_idx
        {
            self.repr_idx = i;
            self.dirty = true;
        }
        // compteur : le démon donne l'âge de l'enregistrement (juste même si le
        // panneau a été fermé/rouvert pendant la capture)
        self.rec_start = etat
            .as_ref()
            .filter(|e| e.recording && e.t >= 1.0)
            .and_then(|e| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs().saturating_sub(e.t as u64))
            });
        let secs = match self.rec_start {
            Some(t0) => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().saturating_sub(t0))
                .unwrap_or(0),
            None => 0,
        };
        if secs != self.rec_secs {
            self.rec_secs = secs;
            self.dirty = true;
        }
    }


    /// Lance un glissement (marge droite : négatif = hors écran à droite).
    fn slide(&mut self, from: i32, to: i32, ms: u64) {
        if !self.side {
            return; // plein écran : pas de glissement, ça n'aurait pas de sens
        }
        let ms = if self.slide_ms > 0 { self.slide_ms } else { ms };
        self.anim = Some(Anim {
            start: Instant::now(),
            dur: Duration::from_millis(ms),
            from,
            to,
        });
    }

    /// Avance l'animation de changement de page ; true tant qu'elle tourne.
    fn tick_page(&mut self) -> bool {
        let Some((_, _, t0, _)) = self.pdep else {
            return false;
        };
        if t0.elapsed() >= Duration::from_millis(180) {
            self.pdep = None;
            self.dirty = true;
            return false;
        }
        self.dirty = true;
        true
    }

    /// Avance l'animation ; renvoie true tant qu'elle tourne.
    fn tick_anim(&mut self) -> bool {
        let Some(anim) = &self.anim else { return false };
        let t = anim.start.elapsed().as_secs_f32() / anim.dur.as_secs_f32();
        let (to, done) = (anim.to, t >= 1.0);
        if done {
            self.set_off(to);
            self.anim = None;
            return false;
        }
        // ease-out cubic : départ franc, arrivée douce
        let e = 1.0 - (1.0 - t).powi(3);
        let v = anim.from as f32 + (to - anim.from) as f32 * e;
        self.set_off(v.round() as i32);
        true
    }

    /// Avance le fondu ; `true` tant qu'il tourne.
    fn tick_fondu(&mut self) -> bool {
        let Some(f) = &self.fondu else { return false };
        let t = f.start.elapsed().as_secs_f32() / f.dur.as_secs_f32();
        let (de, vers) = (f.de, f.vers);
        // ease-out cubic, comme le glissement
        let e = 1.0 - (1.0 - t.min(1.0)).powi(3);
        self.opacite = de + (vers - de) * e;
        self.dirty = true;
        if t >= 1.0 {
            self.opacite = vers;
            self.fondu = None;
            return false;
        }
        true
    }

    /// Décalage horizontal du panneau. 0 = visible, panel_w = hors écran à droite.
    fn set_off(&mut self, v: i32) {
        if !self.side {
            return;
        }
        if self.off != v {
            self.off = v;
            self.dirty = true;
        }
    }

    /// Vrai si l'événement est une **entrée voulue** : un bouton appuyé, une
    /// direction de D-pad, ou un stick sorti de sa zone morte. Le bruit de
    /// repos d'un stick (±quelques unités sur 32 767) ne compte pas.
    fn entree_significative(&self, type_: u16, code: u16, value: i32) -> bool {
        match (type_, code) {
            (EV_KEY, _) => value == 1,
            (EV_ABS, ABS_HAT0X) | (EV_ABS, ABS_HAT0Y) => value != 0,
            (EV_ABS, ABS_X) => {
                let (mn, mx) = self.abs_x;
                let demi = (mx - mn) / 2;
                (value - (mn + demi)).abs() > demi / 5
            }
            (EV_ABS, ABS_Y) => {
                let (mn, mx) = self.abs_y;
                let demi = (mx - mn) / 2;
                (value - (mn + demi)).abs() > demi / 5
            }
            _ => false,
        }
    }

    /// Ferme le panneau (ou le menu système) si la manette n'a pas bougé depuis
    /// `inactivite`. Appelée à chaque tour de boucle.
    fn surveiller_inactivite(&mut self) {
        if self.inactivite.is_zero() || self.closing || !self.visible {
            return;
        }
        if self.derniere_activite.elapsed() >= self.inactivite {
            println!(
                "CLOSE (inactivite {}s)",
                self.inactivite.as_secs_f32().round() as u64
            );
            self.status = "INACTIVITE".to_string();
            self.request_close();
        }
    }

    fn request_close(&mut self) {
        // clavier virtuel : pas d'animation, pas de glissement de panneau —
        // on détruit sa surface et on rend la manette
        if self.kbd_ouvert {
            self.fermer_clavier();
            return;
        }
        if self.closing || !self.visible {
            return;
        }
        self.closing = true;
        if self.side {
            // fermé pendant l'attente d'entrée : rien n'est encore visible,
            // inutile de lancer le glissement d'entrée ensuite
            self.entree = None;
            // sortie : le panneau repart vers la DROITE, hors de l'écran
            self.slide(0, self.panel_w, 140);
        } else {
            // menu système : on s'efface AVANT de démonter la surface — sinon
            // KWin anime la fermeture de la fenêtre et on voit un dézoom
            // (constaté). `running = false` attend la fin du fondu.
            self.fondu = Some(Fondu {
                start: Instant::now(),
                dur: Duration::from_millis(self.fondu_ms),
                de: self.opacite,
                vers: 0.0,
            });
        }
    }

    /* ---------- mode résident (--resident) ---------- */

    /// Crée le socket d'écoute une fois la surface prête (premier `configure`).
    /// Le déclencheur attend son apparition : elle vaut donc « panneau prêt »,
    /// et aucune commande ne peut arriver avant que la surface ait une taille.
    fn demarrer_ipc(&mut self) {
        let Some(chemin) = self.ipc_attente.take() else {
            return;
        };
        let _ = std::fs::remove_file(&chemin);
        match UnixListener::bind(&chemin) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                self.ipc = Some(l);
                self.ipc_path = Some(chemin.clone());
                println!("IPC {chemin}");
            }
            Err(e) => eprintln!("ipc: bind {chemin}: {e}"),
        }
    }

    /// Ouvre le panneau demandé par le déclencheur : manette du node, cibles du
    /// gate, état neuf, **glissement immédiat**. La surface est mappée depuis
    /// le démarrage (transparente, hors écran) : KWin ne rejoue pas son
    /// animation d'ouverture (elle n'a lieu qu'au map), donc aucune attente.
    fn ouvrir(&mut self, node: &str, cibles: &[String]) {
        // clavier ouvert : il tient la manette, un `open` ne doit pas poser le
        // panneau par-dessus (les deux surfaces se superposeraient)
        if self.visible || self.kbd_ouvert {
            return;
        }
        if !node.is_empty() {
            self.detecter_manette(node);
            self.grab_pad(node);
        }
        if let Some(gs) = self.gate_sock.clone() {
            // les dev_t changent à chaque reconnexion : on repose la liste à
            // chaque ouverture (le démon est idempotent)
            gate_cmd(&gs, "clear");
            let mut ok = 0;
            for t in cibles {
                if gate_cmd(&gs, &format!("target {t}")) {
                    ok += 1;
                }
            }
            self.gate_nodes = ok;
            if gate_cmd(&gs, "open") {
                self.gate_on = true;
                println!("GATE open");
            }
        }
        // même point de départ qu'un processus fraîchement lancé
        self.visible = true;
        self.closing = false;
        self.slid_in = true; // pas d'attente : la surface est déjà mappée
        self.entree = None;
        self.anim = None;
        self.fondu = None;
        self.opacite = 1.0;
        self.off = self.panel_w;
        self.page = Page::Accueil;
        self.selected = 0;
        self.fen_top = 0;
        self.page_initiale = false;
        self.depart = Instant::now();
        // le panneau a pu rester caché des minutes : sans ce reset, la passe
        // d'inactivité (10 s) le refermerait aussitôt ouvert
        self.derniere_activite = Instant::now();
        self.arme = None;
        // boutons : état neuf. Un bouton resté enfoncé à la fermeture (son
        // relâchement est parti au jeu) ne doit pas faire jeter le premier
        // appui de cette ouverture, ni hériter du rebond de la précédente
        self.touches_tenues = [false; 0x140];
        self.derniere_frappe = None;
        self.combo_apres = None;
        self.home_down = false;
        self.home_console = false;
        self.voir_console = false;
        self.status.clear();
        self.dirty = true;
        // les réglages (pactl, scripts) sont relus APRÈS la première image
        self.a_rafraichir = true;
        self.slide(self.panel_w, 0, 200);
        println!("OPEN {node}");
    }

    /// Referme sans quitter : surface transparente et hors écran, manette
    /// rendue, gate fermé. Le processus reste à l'écoute de `--ipc`.
    fn cacher(&mut self) {
        self.visible = false;
        self.closing = false;
        self.slid_in = true;
        self.anim = None;
        self.entree = None;
        self.fondu = None;
        self.opacite = 0.0;
        self.off = self.panel_w;
        self.arme = None;
        self.home_down = false;
        self.home_console = false;
        self.release_pad();
        if let Some(gs) = self.gate_sock.clone() {
            if self.gate_on {
                gate_cmd(&gs, "close");
                self.gate_on = false;
                println!("GATE closed");
            }
        }
        self.dirty = true;
        println!("HIDDEN");
        // le panneau est hors écran : les actions différées peuvent partir
        self.executer_apres();
        if self.voir_console {
            self.voir_console = false;
            if let Some(p) = self.declencheur {
                if p > 1 {
                    unsafe { libc::kill(p, libc::SIGUSR1) };
                }
            }
        }
    }

    /// Lit les commandes du déclencheur (`open <node> <cibles…>`, `close`).
    fn lire_ipc(&mut self) {
        // l'écoute sort de `self` le temps de la lecture : les commandes
        // appellent des méthodes `&mut self`
        let Some(listener) = self.ipc.take() else {
            return;
        };
        loop {
            let (mut flux, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("ipc: accept: {e}");
                    break;
                }
            };
            // le déclencheur écrit puis ferme : une lecture suffit
            let _ = flux.set_read_timeout(Some(Duration::from_millis(200)));
            let mut brut = [0u8; 1024];
            let n = flux.read(&mut brut).unwrap_or(0);
            let ligne = String::from_utf8_lossy(&brut[..n]).trim().to_string();
            if ligne.is_empty() {
                continue;
            }
            println!("IPC {ligne}");
            let mut mots = ligne.split_whitespace();
            match mots.next() {
                Some("open") => {
                    let node = mots.next().unwrap_or("").to_string();
                    let cibles: Vec<String> = mots.map(str::to_string).collect();
                    self.ouvrir(&node, &cibles);
                }
                Some("close") => self.request_close(),
                // Home+L2 / Home+R2 hors panneau : `bascule <sens>` (-1 en
                // arrière, +1 en avant). Le résident décide (jeu ⇄ app ou
                // cyclage des fenêtres) et n'ouvre rien.
                Some("bascule") => {
                    let sens = mots.next().and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
                    self.bascule_combo(sens);
                }
                // le déclencheur demande si le panneau est ouvert avant de
                // fermer/agir (il ne peut pas le savoir autrement)
                Some("state") => {
                    let etat = if self.visible { "open" } else { "closed" };
                    let _ = flux.write_all(format!("{etat}\n").as_bytes());
                }
                // clavier virtuel : `kbd <node>` (le résident tient déjà la
                // manette et garde son grab, aucun transfert de processus)
                Some("kbd") => {
                    let node = mots.next().unwrap_or("").to_string();
                    self.ouvrir_clavier(&node);
                }
                _ => {}
            }
        }
        self.ipc = Some(listener);
    }

    /// Action différée (enregistrement, capture, clavier, replay, extinction) :
    /// elle part seulement panneau retiré, sinon elle se filmerait dedans.
    fn executer_apres(&mut self) {
        let Some(a) = self.combo_apres.take() else {
            return;
        };
        println!("FENETRE RETIREE -> action differee");
        match a {
            Apres::Record => lancer_script("RECORDER", &["toggle"]),
            Apres::Capture => lancer_script("SCREENSHOT", &[]),
            Apres::Replay => {
                let node = self.combo_node.clone().unwrap_or_default();
                lancer_script_env("REPLAY", &["save"], &[("PAD_NODE", &node)]);
            }
            Apres::ReplayStart => {
                let node = self.combo_node.clone().unwrap_or_default();
                lancer_script_env("REPLAY", &["start"], &[("PAD_NODE", &node)]);
            }
            Apres::ReplayStop => {
                let node = self.combo_node.clone().unwrap_or_default();
                lancer_script_env("REPLAY", &["stop"], &[("PAD_NODE", &node)]);
            }
            Apres::Reboot => lancer_systeme("REBOOT", "reboot"),
            Apres::Eteindre => lancer_systeme("POWEROFF", "poweroff"),
            Apres::Veille => lancer_systeme("SUSPEND", "suspend"),
            Apres::DeconnecterSession => {
                // Un autologin armé (retour d'un MODE JEU / MODE BUREAU) relance
                // la même session à la déconnexion : plasmalogin garde
                // `Session=` en mémoire. Le helper retire la configuration et
                // redémarre le gestionnaire de connexion, seul chemin fiable
                // vers l'écran de connexion (constaté le 23/11).
                if autologin_arme() {
                    lancer_mode("greeter")
                } else {
                    lancer_deconnexion()
                }
            }
            Apres::ModeJeu => lancer_mode("console"),
            Apres::ModeBureau => lancer_mode("bureau"),
            Apres::ToutFermer => {
                // jeux Windows/émulateurs (killthemall) PUIS les applis du
                // menu Applications : c'est ce que « tout fermer » veut dire
                lancer_script("KILLTHEMALL", &[]);
                lancer_script("FERMERAPPLIS", &[]);
            }
        }
    }

    /* ---------- buffers ---------- */

    fn alloc_buffers(&mut self) {
        let Some(shm) = self.shm.clone() else { return };
        let (w, h) = self.size;
        if w <= 0 || h <= 0 {
            return;
        }
        // anciens buffers
        for b in self.buffers.drain(..) {
            unsafe { libc::munmap(b.ptr as *mut libc::c_void, b.len) };
            b.buffer.destroy();
        }
        self.free.clear();

        let stride = w * 4;
        let len = (stride * h) as usize;
        let max = 64 << 20;
        for idx in 0..2usize {
            let name = c"gpad-shm";
            let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                eprintln!("memfd_create: {}", std::io::Error::last_os_error());
                return;
            }
            if len > max || unsafe { libc::ftruncate(fd, len as i64) } < 0 {
                eprintln!("ftruncate: {}", std::io::Error::last_os_error());
                unsafe { libc::close(fd) };
                return;
            }
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                eprintln!("mmap: {}", std::io::Error::last_os_error());
                unsafe { libc::close(fd) };
                return;
            }
            let pool = shm.create_pool(unsafe { BorrowedFd::borrow_raw(fd) }, len as i32, &self.qh, ());
            let buffer = pool.create_buffer(
                0,
                w,
                h,
                stride,
                wayland_client::protocol::wl_shm::Format::Argb8888,
                &self.qh,
                idx, // data = index, renvoyé dans l'évènement Release
            );
            pool.destroy();
            unsafe { libc::close(fd) };
            self.buffers.push(ShmBuffer {
                buffer,
                ptr: ptr as *mut u8,
                len,
            });
            self.free.push(idx);
        }
        self.dirty = true;
    }

    /// Ligne de volume d'un flux audio (jeu, app compagne) : valeur à droite,
    /// curseur gradué dessous — même présentation que VOLUME GENERAL, pour
    /// qu'on voie d'un coup d'œil qu'on règle bien trois choses différentes.
    #[allow(clippy::too_many_arguments)]
    fn ligne_flux(
        &self,
        cv: &mut Canvas,
        px0: i32,
        pad: i32,
        inner: i32,
        y: i32,
        label: &str,
        flux: Option<&audio::Flux>,
        couleur: u32,
        muet_couleur: u32,
        sel: bool,
    ) {
        let (valeur, teinte) = match flux {
            Some(flux) if flux.mute => ("MUET".to_string(), muet_couleur),
            Some(flux) => (format!("{}%", flux.pourcent), couleur),
            None => ("—".to_string(), muet_couleur),
        };
        cv.text_fit(px0 + pad + 36, y, label, 3, couleur, inner - 36);
        self.valeur_droite(cv, px0 + pad + inner, y, &valeur, 2, teinte);
        self.curseur_ligne(
            cv,
            px0 + pad + 36,
            y + 40,
            inner - 36,
            21,
            flux.map(|flux| (flux.pourcent / 5) as usize).unwrap_or(0),
            sel,
        );
    }

    /// Texte aligné à droite (police à chasse fixe : 6 px par caractère).
    fn valeur_droite(&self, cv: &mut Canvas, x_droite: i32, y: i32, s: &str, scale: i32, color: u32) {
        let w = cv.largeur(s, scale);
        cv.text_fit(x_droite - w, y, s, scale, color, w + 6);
    }

    /// Taille du logo rendu : `(largeur, hauteur)`, ratio conservé, hauteur
    /// plafonnée à [`LOGO_MAX_H`], largeur bornée à la largeur utile du
    /// panneau. `(0, 0)` sans logo (aucun jeu glauncher actif, visuel absent).
    fn logo_taille(&self, inner: i32) -> (i32, i32) {
        let Some(ic) = &self.logo else {
            return (0, 0);
        };
        if self.console || ic.w <= 0 || ic.h <= 0 || inner <= 0 {
            return (0, 0);
        }
        let th = LOGO_MAX_H.min(inner * ic.h / ic.w).max(1);
        let tw = (th * ic.w / ic.h).min(inner).max(1);
        (tw, th)
    }

    /// Décalage vertical de l'en-tête et de la liste quand le logo est affiché.
    /// `fen_visibles` en a besoin hors du dessin (le canevas emprunte `self`).
    ///
    /// L'en-tête démarre normalement à 50 px du bord haut. Avec le logo, il
    /// part de `MARGE + logo + MARGE` : le décalage vaut donc
    /// `th + 2×MARGE − 50`, ce qui donne **le même air avant et après** l'image.
    ///
    /// Le bandeau n'est réservé que si la page tient encore : une page pleine
    /// (REGLAGES avec ses sept lignes hautes) n'a pas la place, et pousser sa
    /// dernière ligne hors du panneau serait pire que de cacher le logo. La
    /// page FENETRES défile, elle s'adapte d'elle-même (`fen_visibles`).
    fn decalage_logo(&self) -> i32 {
        let (w, _) = self.size;
        let pw = if self.side {
            w as i32
        } else {
            760.min(w as i32 - 80)
        };
        let (_, th) = self.logo_taille(pw - 52);
        if th == 0 {
            return 0;
        }
        // sous 18 px de haut, l'image n'a plus d'air à elle : rien à afficher
        let bande = th + 2 * LOGO_MARGE - 50;
        if bande <= 0 {
            return 0;
        }
        if self.page == Page::Fenetres {
            return bande;
        }
        let ph = self.hauteur_cadre();
        let place = (ph - 90) - 180 - self.hauteur_contenu();
        if place >= bande { bande } else { 0 }
    }

    /// Hauteur cumulée des lignes de la page courante (mêmes hauteurs que le
    /// dessin : séparateur 52, réglage 86, ligne 46, en-tête 0).
    fn hauteur_contenu(&self) -> i32 {
        self.liste().iter().map(|e| e.hauteur()).sum()
    }

    /// Hauteur du cadre non latéral. Le **menu système grandit avec ses
    /// lignes** : 360 px fixes laissaient « ETEINDRE » passer sous l'aide
    /// depuis l'ajout de MISE EN VEILLE (constaté le 23/11). 62 = début de
    /// liste, 8 = marge avant l'aide, 74 = pied.
    fn hauteur_cadre(&self) -> i32 {
        let (_, h) = self.size;
        if self.side {
            return h as i32;
        }
        let dispo = (h as i32 - 120).max(240);
        if self.console {
            (62 + self.hauteur_contenu() + 8 + 74).min(dispo)
        } else {
            640.min(dispo)
        }
    }

    /// Nombre de lignes visibles sur la page « fenêtres » (lignes de 46 px).
    fn fen_visibles(&self) -> usize {
        let ph = self.hauteur_cadre();
        // 180 = départ de liste (sous l'en-tête et le titre de page),
        // 90 = pied + marges ; le bandeau du logo décale tout d'autant
        (((ph - 90) - (180 + self.decalage_logo())) / 46).max(1) as usize
    }

    /// Jeu dont la fenêtre est active (`None` sinon) : la seule source du nom,
    /// de l'icône et du logo affichés en haut du panneau.
    fn jeu_actif(&self) -> Option<&JeuActif> {
        self.jeu.as_ref()
    }

    /// Relit les publications des lanceurs et met à jour le logo (bandeau
    /// au-dessus de l'en-tête) **et** l'icône de l'en-tête. Aucun visuel
    /// inventé : chemin absent du fichier ou image illisible = rien de changé.
    fn rafraichir_jeu(&mut self) {
        self.jeu = jeu_de(&lire_jeux_actifs(), self.win.pid);

        // logo du jeu : c'est lui qui décale la mise en page
        let cle_logo = self
            .jeu_actif()
            .map(|jeu| jeu.logo.clone())
            .unwrap_or_default();
        if cle_logo != self.logo_cle {
            self.logo_cle = cle_logo.clone();
            self.logo = if cle_logo.is_empty() {
                None
            } else {
                lire_png(std::path::Path::new(&cle_logo))
            };
            self.dirty = true;
            match (&self.jeu, &self.logo) {
                (Some(jeu), Some(_)) => println!("LOGO {} ({})", cle_logo, jeu.slug),
                _ => println!("LOGO aucun"),
            }
        }

        // icône de l'en-tête : celle du paquet quand un jeu tourne (la fenêtre
        // d'un jeu Wine n'en porte qu'une d'exécutable), celle de la fenêtre
        // sinon. La clé évite de refaire la résolution à chaque tour.
        let cle_icone = match self.jeu_actif() {
            Some(jeu) if !jeu.icone.is_empty() => format!("jeu:{}", jeu.icone),
            _ => format!("{}|{}", self.win.desktop, self.win.class),
        };
        if cle_icone != self.icone_cle {
            self.icone_cle = cle_icone;
            self.icone = match self.icone_cle.strip_prefix("jeu:") {
                Some(chemin) => lire_png(std::path::Path::new(chemin)),
                None => charger_icone(&self.win.desktop, &self.win.class, &self.win.caption),
            };
            self.dirty = true;
            println!(
                "ICONE {} ({})",
                if self.icone.is_some() { "trouvee" } else { "aucune" },
                self.icone_cle
            );
        }
    }

    /// Recale le défilement pour que la sélection soit toujours visible.
    /// Appelé avant le dessin (le canevas emprunte `self.px` : on ne peut plus
    /// toucher à `self` ensuite).
    ///
    /// Deux pages défilent : FENETRES (lignes de hauteur fixe) et REGLAGES,
    /// dont les lignes n'ont plus la même hauteur (curseurs 86 px, énumérations
    /// 46 px) — on cumule donc les hauteurs réelles, et on ne retire que les
    /// lignes entièrement sorties en haut.
    fn ajuster_defilement(&mut self) {
        if self.page.defilante() {
            self.fen_top = 0;
            let budget = self.liste_budget().max(46);
            let hauteurs: Vec<i32> = self.liste().iter().map(|e| e.hauteur()).collect();
            let mut top = self.reg_top.min(self.selected);
            loop {
                let mut y = 0;
                for h in &hauteurs[top..=self.selected.min(hauteurs.len() - 1)] {
                    y += *h;
                }
                if y <= budget || top >= self.selected {
                    break;
                }
                top += 1;
            }
            self.reg_top = top;
            return;
        }
        self.reg_top = 0;
        if self.page != Page::Fenetres {
            self.fen_top = 0;
            return;
        }
        let visibles = self.fen_visibles();
        if self.selected >= self.fen_top + visibles {
            self.fen_top = self.selected + 1 - visibles;
        }
        if self.selected < self.fen_top {
            self.fen_top = self.selected;
        }
    }

    /// Marqueur de sélection : un **trait d'accent vertical arrondi** en MODERNE
    /// (le chevron `>` fait très « terminal » à côté d'une police moderne),
    /// le chevron reste en RETRO où il est dans le langage du pixel art.
    fn marqueur(&self, cv: &mut Canvas, sx: i32, sy: i32, sh: i32, chevron_x: i32, y: i32, col: u32) {
        if self.style_idx == 1 {
            let bh = 20.min(sh - 10);
            cv.rrect(sx + 7, sy + (sh - bh) / 2, 4, bh, 2, self.th.accent);
        } else {
            cv.text(chevron_x, y, ">", 3, col);
        }
    }

    /// Nom du menu courant, **en bas** du panneau, centré : on sait où l'on est
    /// sans lire de raccourci (l'aide rappelle déjà les gâchettes).
    fn nom_menu(&self, cv: &mut Canvas, x_centre: i32, y: i32, x0: i32, x1: i32) {
        let nom = match self.page {
            Page::Accueil => "MENU",
            Page::Categorie(cat) => cat.nom(),
            Page::Applis => "APPLIS",
            Page::Fenetre => "FENETRE",
            Page::Fenetres => "FENETRES",
        };
        let th = self.th;
        // titre **centré**, un filet qui s'efface de chaque côté : on lit la
        // page d'un coup d'œil, sans pastille ni carré (préféré à l'usage)
        let w = cv.largeur(nom, 3);
        cv.text(x_centre - w / 2, y + 3, nom, 3, th.accent);
        let (g, d) = (x_centre - w / 2 - 14, x_centre + w / 2 + 14);
        cv.filet_fondu_inv(x0, y + 17, g - x0, 0xbb, th.accent);
        cv.filet_fondu(d, y + 17, x1 - d, 0xbb, th.accent);
    }

    /// Largeur du rappel « confirmer » (deuxième appui), glyphe compris.
    fn hint_ok_largeur(&self, cv: &Canvas) -> i32 {
        if self.style_idx == 1 {
            cv.largeur("CONFIRMER", 2) + 28
        } else if self.pad_sony {
            20 + cv.largeur("CONFIRMER", 2)
        } else {
            cv.largeur("A: CONFIRMER", 2)
        }
    }

    /// Rappel de confirmation à droite d'une ligne armée : le **glyphe du
    /// bouton qui confirme** (croix PS / A Xbox) suivi de « CONFIRMER », au lieu
    /// d'un « ? » (demandé). En RETRO la police bitmap n'a pas de glyphe pour
    /// Xbox — on écrit `A:` — et Sony garde la croix dessinée.
    fn dessiner_hint_ok(&self, cv: &mut Canvas, x_droite: i32, y: i32, color: u32) {
        let w = self.hint_ok_largeur(cv);
        let x = x_droite - w;
        if self.style_idx == 1 {
            // centrage sur la ligne : la ligne écrit en échelle 3 (capitales de
            // y à y+20), donc le glyphe de 22 px va de y-1 à y+21 et le rappel
            // en échelle 2 (12 px) de y+4 à y+16 — même centre, y+10.
            if let Some(ic) = self.glyphe("a") {
                cv.icone(x, y - 1, 22, ic);
            }
            cv.text(x + 28, y + 4, "CONFIRMER", 2, color);
        } else if self.pad_sony {
            glyphe_croix(cv, x, y + 7, 10, color);
            cv.text(x + 19, y + 4, "CONFIRMER", 2, color);
        } else {
            cv.text(x, y + 4, "A: CONFIRMER", 2, color);
        }
    }

    /// Curseur intégré à la ligne : piste, graduations, position courante.
    /// `sur` : la ligne est sélectionnée — on prend les variantes contrastées
    /// du curseur (voir `curseur_*_sur`), sinon un curseur accent-sur-accent
    /// s'efface en BREEZE et une poignée blanche sur pastille l'était en DECK.
    /// Curseur « Plasma » : sillon arrondi continu, rempli jusqu'à la poignée,
    /// poignée ronde de 16 px à liseré. **Pas de graduations** (Plasma n'en a
    /// pas ; la valeur est écrite à droite de la ligne). Les centres restent
    /// une série unique, partagée par le sillon, le remplissage et la poignée.
    fn curseur_ligne(
        &self,
        cv: &mut Canvas,
        x: i32,
        y: i32,
        w: i32,
        n: usize,
        idx: usize,
        sur: bool,
    ) {
        let th = self.th;
        let (c_piste, c_plein, c_poignee, c_bord) = if sur {
            (
                th.curseur_piste_sur,
                th.curseur_plein_sur,
                th.curseur_poignee_sur,
                th.curseur_bord_sur,
            )
        } else {
            (th.piste, th.piste_plein, th.poignee, th.poignee_bord)
        };
        let n = n.max(2) as i32;
        const DIAM: i32 = 16;
        let pas = (w - DIAM).max(1) / (n - 1);
        let centre = |i: i32| x + DIAM / 2 + i * pas;
        let (c0, cn, pos) = (centre(0), centre(n - 1), centre(idx as i32));
        // sillon : 6 px de haut, bouts arrondis, centré sur la poignée
        let gy = y + DIAM / 2 - 3;
        cv.rrect(c0 - DIAM / 2, gy, cn - c0 + DIAM, 6, 3, c_piste);
        // portion remplie : du bout gauche au centre de la poignée
        cv.rrect(c0 - DIAM / 2, gy, pos - c0 + DIAM / 2, 6, 3, c_plein);
        // poignée : disque à liseré sombre (relief), centre = `centre(idx)`
        cv.rrect(pos - DIAM / 2, y, DIAM, DIAM, DIAM / 2, c_bord);
        cv.rrect(
            pos - DIAM / 2 + 2,
            y + 2,
            DIAM - 4,
            DIAM - 4,
            (DIAM - 4) / 2,
            c_poignee,
        );
    }

    /// Interrupteur à deux états, façon Plasma, posé au bord droit de la ligne
    /// (`PLEIN ECRAN`, `REPLAY`, `MODE SOURIS`). `grise` = refusé (replay
    /// pendant un enregistrement).
    fn interrupteur(&self, cv: &mut Canvas, x_droite: i32, y: i32, actif: bool, grise: bool) {
        let th = self.th;
        const L: i32 = 44;
        const H: i32 = 22;
        let fond = if actif && !grise { th.piste_plein } else { th.rail };
        let bouton = th.poignee;
        let x = x_droite - L;
        let yy = y + 1;
        cv.rrect(x, yy, L, H, H / 2, fond);
        let kx = if actif { x + L - H + 1 } else { x + 1 };
        cv.rrect(kx, yy + 1, H - 2, H - 2, (H - 2) / 2, th.poignee_bord);
        cv.rrect(kx + 2, yy + 3, H - 6, H - 6, (H - 6) / 2, bouton);
    }



    /// Dessine la liste de la page courante à partir de `y0`. Extrait du
    /// `draw()` pour pouvoir la dessiner **deux fois** pendant un changement de
    /// page (l'ancienne qui sort, la nouvelle qui entre) avec un décalage
    /// horizontal et une découpe : l'en-tête, lui, ne bouge pas.
    fn dessiner_liste(
        &self,
        cv: &mut Canvas,
        px0: i32,
        pad: i32,
        inner: i32,
        y0: i32,
        selected: usize,
    ) {
        let mut cv = cv;
        let th = self.th;
        let mut y = y0;
        // page fenêtres : fenêtre glissante, toutes ses lignes font 46 px
        let visibles = self.fen_visibles();
        // page RÉGLAGES : hauteur des lignes déjà sorties en haut. Le test de
        // fin de fenêtre doit s'en retirer — sinon, défilé, il compte deux fois
        // ces lignes et coupe la liste trop tôt (mesuré : les trois dernières
        // sections disparaissaient dès que la page défilait).
        let dessus: i32 = if self.page.defilante() {
            self.liste().iter().take(self.reg_top).map(|e| e.hauteur()).sum()
        } else {
            0
        };
        for (i, e) in self.liste().iter().enumerate() {
            if self.page == Page::Fenetres && i < self.fen_top {
                continue;
            }
            if self.page == Page::Fenetres && i >= self.fen_top + visibles {
                break;
            }
            // une ligne de réglage est plus haute : le curseur vit dessous
            // l'en-tête (Entree::Fenetre) est dessiné plus haut : hauteur nulle
            let h = e.hauteur();
            // page RÉGLAGES : fenêtre glissante (elle ne tient plus à l'écran
            // depuis l'ajout de la section CAPTURE)
            // Lignes sorties en haut : elles ne doivent PAS avancer `y`.
            // Les compter faisait démarrer la fenêtre visible à `y0 + dessus`
            // au lieu de `y0` (mesuré 21/11/2026 : la dernière ligne passait
            // sous l'aide et sous les pastilles, et le test de fin ne pouvait
            // plus se déclencher).
            if self.page.defilante() && i < self.reg_top {
                continue;
            }
            if self.page.defilante() && y - y0 + h > self.liste_budget() {
                break;
            }
            // l'en-tête a déjà son propre surlignage (hauteur nulle ici)
            if matches!(e, Entree::Fenetre) {
                y += h;
                continue;
            }
            let sel = i == selected;
            // surbrillance claire (Deck) : les tons de texte de la ligne
            // sélectionnée passent en version sombre, sinon illisibles
            let (cc_txt, cc_dim, cc_ok, cc_warn) = (
                th.sur(th.txt, sel),
                th.sur(th.dim, sel),
                th.sur(th.ok, sel),
                th.sur(th.warn, sel),
            );
            if sel {
                let (sx, sy, sw, sh) = (px0 + pad - 8, y - 10, inner + 16, h - 8);
                cv.rrect_v(sx, sy, sw, sh, th.rayon_sel, th.sel_haut, th.sel_bas);
                // barre d'accent par-dessus l'arrondi, sinon ses bouts sont
                // rognés avec les coins
                if th.barre > 0 {
                    cv.rect(sx, sy + 3, th.barre, sh - 6, th.accent);
                }
                self.marqueur(&mut cv, sx, sy, sh, px0 + pad + 1, y, cc_txt);
            }
            let color = if sel { cc_txt } else { cc_dim };
            match *e {
                // inatteignable : l'en-tête est sauté plus haut
                Entree::Fenetre => {}
                Entree::Action("REPLAY") => {
                    // indépendant de l'enregistrement : l'interrupteur reste
                    // manipulable pendant une capture (c'est le seul chemin
                    // pour arrêter le buffer depuis le panneau).
                    cv.text_fit(px0 + pad + 36, y, "REPLAY", 3, color, inner - 36 - 52);
                    self.interrupteur(
                        &mut cv,
                        px0 + pad + inner,
                        y,
                        self.replay_active,
                        false,
                    );
                }
                // sélecteur jeu ⇄ app : action simple, libellé seul (le sens
                // est dans le libellé, voir `nom()`)
                Entree::Basculer => {
                    cv.text_fit(px0 + pad + 36, y, "BASCULER JEU / APP", 3, color, inner - 36);
                }
                Entree::Action("SAUVER REPLAY") => {
                    let c_txt = if self.replay_active { color } else { cc_dim };
                    cv.text_fit(px0 + pad + 36, y, "SAUVER REPLAY", 3, c_txt, inner - 36 - 90);
                    if self.replay_active {
                        self.valeur_droite(
                            &mut cv, px0 + pad + inner, y,
                            &format!("{}s", REPR_OPTS[self.repr_idx].1), 2, cc_ok,
                        );
                    } else {
                        self.valeur_droite(&mut cv, px0 + pad + inner, y, "INACTIF", 2, cc_dim);
                    }
                }
                Entree::Action("PLEIN ECRAN") => {
                    cv.text_fit(px0 + pad + 36, y, "PLEIN ECRAN", 3, color, inner - 36 - 52);
                    self.interrupteur(&mut cv, px0 + pad + inner, y, self.win.fullscreen, false);
                }
                // catégorie : le libellé et un chevron au bord droit — il y a
                // un niveau de plus derrière, on le voit sans essayer
                Entree::SousMenu(titre, _) => {
                    let dispo = inner - 36 - 28;
                    cv.text_fit(px0 + pad + 36, y, titre, 3, color, dispo);
                    let w = cv.largeur(">", 3);
                    cv.text(px0 + pad + inner - w, y, ">", 3, cc_dim);
                }
                Entree::Action("FERMER") => {
                    let arme = self.arme.as_deref() == Some("FERMER");
                    let c = if arme { cc_warn } else { color };
                    let max = if arme {
                        inner - 36 - self.hint_ok_largeur(&cv) - 10
                    } else {
                        inner - 36
                    };
                    cv.text_fit(px0 + pad + 36, y, "FERMER", 3, c, max);
                    if arme {
                        self.dessiner_hint_ok(&mut cv, px0 + pad + inner, y, cc_warn);
                    }
                }
                Entree::Action("MODE SOURIS") => {
                    cv.text_fit(px0 + pad + 36, y, "MODE SOURIS", 3, color, inner - 36 - 52);
                    self.interrupteur(&mut cv, px0 + pad + inner, y, self.mouse_active, false);
                }
                Entree::Action("VRR") => {
                    // VRR automatique sur les sorties capables (Adaptive Sync
                    // de KWin) : A bascule, l'interrupteur lit l'état réel
                    cv.text_fit(px0 + pad + 36, y, "VRR", 3, color, inner - 36 - 52);
                    self.interrupteur(&mut cv, px0 + pad + inner, y, self.vrr_on, false);
                }
                Entree::Action(nom) => {
                    // l'entrée d'enregistrement n'a qu'un sens (le buffer de
                    // replay tourne à côté s'il est lancé : sa sauvegarde a sa
                    // propre ligne, plus haut)
                    let (label, c_txt) = if nom == "ENREGISTRER UNE VIDEO" && self.rec_active {
                        ("STOP ENREGISTRER", color)
                    } else {
                        (nom, color)
                    };
                    // place pour le compteur à droite pendant l'enregistrement
                    let max = if nom == "ENREGISTRER UNE VIDEO" && self.rec_active {
                        inner - 36 - 72
                    } else {
                        inner - 36
                    };
                    // armé : rouge + « ? » (déco BT, redémarrer, éteindre)
                    let arme = self.arme.as_deref() == Some(nom);
                    let c_txt = if arme { cc_warn } else { c_txt };
                    let max = if arme {
                        (max - self.hint_ok_largeur(&cv) - 10).max(60)
                    } else {
                        max
                    };
                    cv.text_fit(px0 + pad + 36, y, label, 3, c_txt, max);
                    if arme {
                        self.dessiner_hint_ok(&mut cv, px0 + pad + inner, y, cc_warn);
                    }
                    if nom == "ENREGISTRER UNE VIDEO" && self.rec_active {
                        self.valeur_droite(
                            &mut cv,
                            px0 + pad + inner,
                            y,
                            &chrono(self.rec_secs),
                            2,
                            cc_warn,
                        );
                    }
                }
                Entree::VolumeJeu => self.ligne_flux(
                    &mut cv, px0, pad, inner, y, "VOLUME JEU", self.son_jeu.as_ref(), color, cc_warn, sel),
                Entree::VolumeApp => self.ligne_flux(
                    &mut cv, px0, pad, inner, y, "VOLUME APP", self.son_app.as_ref(), color, cc_warn, sel),
                Entree::Volume => {
                    let (v, vc) = if self.vol_mute {
                        ("MUET".to_string(), cc_warn)
                    } else {
                        (format!("{}%", self.vol), color)
                    };
                    cv.text_fit(px0 + pad + 36, y, "VOLUME GENERAL", 3, color, inner - 36);
                    // valeur au même corps que MANGOHUD / DURÉE / STYLE : à
                    // l'échelle 3 les nombres écrasaient les autres lignes
                    self.valeur_droite(&mut cv, px0 + pad + inner, y, &v, 2, vc);
                    self.curseur_ligne(&mut cv, px0 + pad + 36, y + 40, inner - 36, 21,
                                       (self.vol / 5) as usize, sel);
                }
                Entree::Appli(nom) => {
                    let cle = format!("{}|", desktop_appli(nom));
                    let mut dec = 0;
                    if let Some(Some(ic)) = self.icones.get(&cle) {
                        cv.icone(px0 + pad + 34, y - 6, 30, ic);
                        dec = 38;
                    }
                    cv.text_fit(px0 + pad + 36 + dec, y, nom, 3, color, inner - 36 - dec);
                }
                Entree::Autre(idx) => {
                    let (titre, actif, cle) = match self.fenetres.get(idx) {
                        Some(f) => (
                            f.titre().to_string(),
                            f.active,
                            format!("{}|{}", f.desktop, f.class),
                        ),
                        None => ("FENETRE DISPARUE".to_string(), false, String::new()),
                    };
                    // icône de la ligne (préchargée à l'ouverture de la liste)
                    let mut dec = 0;
                    if let Some(Some(ic)) = self.icones.get(&cle) {
                        cv.icone(px0 + pad + 34, y - 6, 30, ic);
                        dec = 38;
                    }
                    let max = inner - 36 - dec - if actif { 80 } else { 0 };
                    cv.text_fit(px0 + pad + 36 + dec, y, &titre, 3, color, max);
                    if actif {
                        self.valeur_droite(&mut cv, px0 + pad + inner, y, "ACTIVE", 2, cc_ok);
                    }
                }
                Entree::Duree => {
                    cv.text_fit(px0 + pad + 36, y, "DUREE REPLAY", 3, color, inner - 36);
                    self.valeur_droite(
                        &mut cv, px0 + pad + inner, y,
                        &format!("{}s", REPR_OPTS[self.repr_idx].1), 2, color,
                    );
                    self.curseur_ligne(&mut cv, px0 + pad + 36, y + 40, inner - 36,
                                       REPR_OPTS.len(), self.repr_idx, sel);
                }
                Entree::Qualite => self.ligne_gradee(
                    &mut cv, px0, pad, inner, y, "QUALITE", color,
                    capture::libelle(&self.cap, "qualite", &capture::QUALITES),
                    capture::QUALITES.len(),
                    capture::idx(&self.cap, "qualite", &capture::QUALITES), sel),
                Entree::Codec => self.ligne_gradee(
                    &mut cv, px0, pad, inner, y, "CODEC", color,
                    capture::libelle(&self.cap, "codec", &capture::CODECS),
                    capture::CODECS.len(),
                    capture::idx(&self.cap, "codec", &capture::CODECS), sel),
                Entree::ShotFormat => self.ligne_gradee(
                    &mut cv, px0, pad, inner, y, "FORMAT PHOTO", color,
                    capture::libelle(&self.cap, "shot_format", &capture::SHOT_FORMATS),
                    capture::SHOT_FORMATS.len(),
                    capture::idx(&self.cap, "shot_format", &capture::SHOT_FORMATS), sel),
                Entree::ShotQualite => self.ligne_gradee(
                    &mut cv, px0, pad, inner, y, "QUALITE PHOTO", color,
                    capture::libelle(&self.cap, "shot_qualite", &capture::SHOT_QUALITES),
                    capture::SHOT_QUALITES.len(),
                    capture::idx(&self.cap, "shot_qualite", &capture::SHOT_QUALITES), sel),
                Entree::Reso => self.ligne_gradee(
                    &mut cv, px0, pad, inner, y, "RESOLUTION", color,
                    capture::libelle(&self.cap, "resolution", &capture::RESOS),
                    capture::RESOS.len(),
                    capture::idx(&self.cap, "resolution", &capture::RESOS), sel),
                Entree::FpsRec => self.ligne_gradee(
                    &mut cv, px0, pad, inner, y, "FPS", color,
                    capture::libelle(&self.cap, "fps", &capture::FPS),
                    capture::FPS.len(),
                    capture::idx(&self.cap, "fps", &capture::FPS), sel),
                Entree::Curseur => {
                    // un interrupteur, comme VRR / REPLAY AUTO / PRIORITÉ :
                    // deux états, et le libellé du cran actif reste lisible
                    // au-dessus du rail
                    cv.text_fit(px0 + pad + 36, y, "CURSEUR", 3, color, inner - 36 - 60);
                    self.interrupteur(
                        &mut cv, px0 + pad + inner, y,
                        capture::idx(&self.cap, "curseur", &capture::CURSEURS) == 1,
                        false);
                }
                Entree::AutoReplay => {
                    cv.text_fit(px0 + pad + 36, y, "REPLAY AUTO", 3, color, inner - 36 - 60);
                    self.interrupteur(&mut cv, px0 + pad + inner, y, self.auto_replay(), false);
                }
                Entree::Fps => {
                    // limite MangoHud : « MAX » la distingue de la cadence
                    // d'enregistrement (section CAPTURE), qui s'appelle FPS
                    cv.text_fit(px0 + pad + 36, y, "FPS MAX", 3, color, inner - 36);
                    // La valeur réelle passe avant le cran : le curseur se pose
                    // sur le cran le plus proche, le chiffre dit la vérité.
                    let txt;
                    let v = match self.fps_reel {
                        Some(f) => {
                            txt = f.to_string();
                            &txt
                        }
                        None => {
                            let v = FPS_OPTS[self.fps_idx].0;
                            if v == "SANS LIMITE" { "INF" } else { v }
                        }
                    };
                    self.valeur_droite(&mut cv, px0 + pad + inner, y, v, 2, color);
                    self.curseur_ligne(&mut cv, px0 + pad + 36, y + 40, inner - 36,
                                       FPS_OPTS.len(), self.fps_idx, sel);
                }
                Entree::Mango => {
                    cv.text_fit(px0 + pad + 36, y, "MANGOHUD", 3, color, inner - 36);
                    self.valeur_droite(&mut cv, px0 + pad + inner, y, MH_OPTS[self.mh_idx].0, 2, color);
                    self.curseur_ligne(&mut cv, px0 + pad + 36, y + 40, inner - 36,
                                       MH_OPTS.len(), self.mh_idx, sel);
                }
                Entree::Style => {
                    // le nom du style est DANS le libellé ; l'interrupteur dit
                    // le même état d'un coup d'œil (ON = moderne)
                    let label = if self.style_idx == 1 {
                        "STYLE MODERNE"
                    } else {
                        "STYLE RETRO"
                    };
                    cv.text_fit(px0 + pad + 36, y, label, 3, color, inner - 36 - 52);
                    self.interrupteur(
                        &mut cv,
                        px0 + pad + inner,
                        y,
                        self.style_idx == 1,
                        false,
                    );
                }
                Entree::Booster => {
                    // « optimiser au premier plan » : l'interrupteur lit l'état
                    // écrit dans kcgroupsrc (absent = activé), pas un cache
                    cv.text_fit(
                        px0 + pad + 36,
                        y,
                        "PRIORITE ACTIVE",
                        3,
                        color,
                        inner - 36 - 52,
                    );
                    self.interrupteur(&mut cv, px0 + pad + inner, y, self.booster_on, false);
                }
            }
            y += h;
        }

        // Indicateur de défilement : la page RÉGLAGES ne tient plus à l'écran
        // (section CAPTURE). Un rail fin au bord droit et un curseur de la
        // taille de la fenêtre visible : on sait qu'il y a une suite, et où
        // l'on est. Muet quand tout tient.
        if self.page.defilante() {
            let hauteurs: Vec<i32> = self.liste().iter().map(|e| e.hauteur()).collect();
            let total: i32 = hauteurs.iter().sum();
            let budget = self.liste_budget().max(46);
            if total > budget + 4 {
                // 12 px : la surbrillance de sélection s'arrête à `+8`
                // (inner + 16 depuis pad - 8), la barre ne doit pas la toucher
                let x = px0 + pad + inner + 12;
                let piste = budget - 12;
                let hh = (piste * budget / total).max(22).min(piste);
                let course = (piste - hh).max(1);
                let pos = (course as f32 * dessus as f32 / (total - budget).max(1) as f32) as i32;
                cv.rrect(x, y0 + 6, 3, piste, 1, 0x30ffffff);
                cv.rrect(x, y0 + 6 + pos.clamp(0, course), 3, hh, 1, th.accent);
            }
        }
    }

    fn draw(&mut self) {
        // glyphes : une seule fois, quand MODERNE est réellement affiché
        if self.style_idx == 1 && self.glyphes.is_empty() {
            self.glyphes = charger_glyphes(self.pad_sony);
            println!("GLYPHES {} ({})", self.glyphes.len(), if self.pad_sony { "ps" } else { "xbox" });
        }
        if !self.configured || self.buffers.is_empty() {
            return;
        }
        self.ajuster_defilement();
        // aucun buffer libre = le compositeur affiche encore les deux :
        // on ne touche à rien (cause du scintillement précédent)
        let Some(idx) = self.free.pop() else { return };
        let (w, h) = self.size;
        let buf = &self.buffers[idx];

        let pixels =
            unsafe { std::slice::from_raw_parts_mut(buf.ptr as *mut u32, buf.len / 4) };
        let mut cv = Canvas {
            px: pixels,
            w: w as usize,
            h: h as usize,
            police: self.police.as_ref(),
            ox: 0,
            clip: None,
        };
        // le panneau glisse en décalant le dessin : la zone qu'il quitte doit
        // être transparente, sinon on voit le contenu de l'autre buffer
        cv.px.fill(0);
        let th = self.th;
        let (px0, py0, pw, ph) = if self.side {
            // panneau ancré au bord : la surface EST le panneau
            (self.off, 0, w, h)
        } else {
            cv.rect(0, 0, w, h, th.shade);
            // menu système (Home maintenu) : petit cadre au centre ; sinon
            // panneau de réglages
            let pw = if self.console {
                520.min(w - 80)
            } else {
                760.min(w - 80)
            };
            let ph = self.hauteur_cadre();
            ((w - pw) / 2, (h - ph) / 2, pw, ph)
        };
        // Sous le cadre : le voile en plein écran (menu système) ou rien du
        // tout (panneau latéral, où la surface EST le panneau). Les coins
        // arrondis doivent retrouver ce fond, pas devenir transparents.
        let dessous = if self.side { 0 } else { th.shade };
        // Ombre portée : **panneau latéral seulement**. Dans le menu système le
        // cadre est posé sur un voile translucide, et l'ombre translucide y
        // dessinerait un liseré d'une autre couleur en bas et à droite.
        if th.ombre != 0 && self.side {
            cv.rect(px0 + 6, py0 + 8, pw, ph, th.ombre);
            cv.coins_fond(px0 + 6, py0 + 8, pw, ph, th.rayon + 2, dessous);
        }
        // fond en dégradé + liseré fin : le cadre reste discret, c'est la
        // barre d'accent en haut qui donne le repère
        cv.rect_v(px0, py0, pw, ph, th.fond_haut, th.fond_bas);
        cv.rect(px0, py0, pw, 1, th.edge);
        cv.rect(px0, py0 + ph - 1, pw, 1, th.edge);
        cv.rect(px0, py0, 1, ph, th.edge);
        cv.rect(px0 + pw - 1, py0, 1, ph, th.edge);
        if th.barre_haut > 0 {
            cv.rect(px0 + 1, py0 + 1, pw - 2, th.barre_haut, th.accent);
        }
        cv.coins_fond(px0, py0, pw, ph, th.rayon, dessous);

        let pad = 26;
        let inner = pw - 2 * pad;
        // aucun titre : le menu système n'affiche que ses lignes, le panneau
        // latéral garde seulement l'indicateur de page

        // Logo du jeu lancé par glauncher, AU-DESSUS de l'en-tête de fenêtre.
        // Affiché seulement quand l'overlay l'a réellement chargé (le launcher
        // ne publie le chemin que pour un visuel présent dans son cache) et que
        // la page a la place : il décale alors l'en-tête, le titre et la liste.
        let dh = self.decalage_logo();
        if dh > 0 {
            let (lw, lh) = self.logo_taille(inner);
            if let Some(ic) = &self.logo {
                // même air au-dessus qu'en dessous (l'en-tête commence à
                // `dh + 50`, soit `MARGE + logo + MARGE`)
                cv.image(px0 + (pw - lw) / 2, py0 + LOGO_MARGE, lw, lh, ic);
            }
        }
        let haut = py0 + dh;

        // fenêtre/game actif : ce sur quoi « plein écran » et « tuer » agissent
        // (l'en-tête est lui-même une entrée : A ouvre la liste des fenêtres)
        if !self.console {
            let entete_sel = matches!(self.liste().get(self.selected), Some(Entree::Fenetre));
            // même surbrillance que les lignes, même règle de lisibilité
            let (c_cap, c_cls) = (th.sur(th.txt, entete_sel), th.sur(th.dim, entete_sel));
            if entete_sel {
                let (sx, sy, sw, sh) = (px0 + pad - 8, haut + 50, inner + 16, 64);
                cv.rrect_v(sx, sy, sw, sh, th.rayon_sel, th.sel_haut, th.sel_bas);
                if th.barre > 0 {
                    cv.rect(sx, sy + 3, th.barre, sh - 6, th.accent);
                }
            }
            let (cap, cls) = if let Some(jeu) = self.jeu_actif().filter(|jeu| !jeu.nom.is_empty()) {
                // le paquet publié nomme la fenêtre : sa classe d'appli
                // (souvent `steam_proton` pour un jeu Wine, ou le nom du
                // binaire) n'apprend rien, donc pas de 2e ligne
                (jeu.nom.clone(), String::new())
            } else if !self.a_fenetre() {
                ("AUCUNE FENETRE".to_string(), String::new())
            } else if self.win.caption.is_empty() {
                // fenêtre sans titre : sa classe est la seule identité
                (self.win.class.clone(), String::new())
            } else {
                (self.win.caption.clone(), self.win.class.clone())
            };
            let decal = if let Some(ic) = &self.icone {
                cv.icone(px0 + pad, haut + 56, 34, ic);
                44
            } else {
                0
            };
            // légende : elle rétrécit (3 → 2) plutôt que d'être coupée — un nom
            // de paquet long ne tient pas à l'échelle 3
            cv.text_reduit(px0 + pad + decal, haut + 62, &cap, 3, 2, c_cap, inner - decal);
            if !cls.is_empty() {
                // classe de l'appli : sa propre ligne, détachée du nom (elle le
                // collait au bord de la pastille en MODERNE)
                cv.text_fit(px0 + pad + decal, haut + 94, &cls, 2, c_cls, inner - decal);
            }
        }

        // nom du menu courant, juste SOUS le nom de la fenêtre active
        if !self.console {
            self.nom_menu(
                &mut cv,
                px0 + pw / 2,
                haut + 134,
                px0 + pad,
                px0 + pad + inner,
            );
        }

        // 146 et non 124 : l'en-tête (nom + classe) garde une vraie respiration
        // avant le premier choix, sinon on ne distingue pas « la fenêtre » des
        // actions (constaté à l'usage)
        let y_liste = if self.console {
            py0 + 62
        } else {
            haut + 180
        };
        // Changement de page : le contenu glisse horizontalement (l'ancienne
        // sort, la nouvelle entre), l'en-tête de fenêtre ne bouge pas.
        if let Some((ancienne, nouvelle, t0, sens)) = self.pdep {
            let t = (t0.elapsed().as_secs_f32() / 0.18).min(1.0);
            let e = 1.0 - (1.0 - t).powi(3);
            let largeur = (inner + 40) as f32;
            let decoupe = (
                px0 + pad - 8,
                y_liste - 16,
                px0 + pad + inner + 8,
                py0 + ph - 100,
            );
            cv.clip = Some(decoupe);
            self.page = ancienne;
            cv.ox = -(sens as f32 * e * largeur) as i32;
            self.dessiner_liste(&mut cv, px0, pad, inner, y_liste, usize::MAX);
            self.page = nouvelle;
            cv.ox = (sens as f32 * (1.0 - e) * largeur) as i32;
            self.dessiner_liste(&mut cv, px0, pad, inner, y_liste, self.selected);
            cv.ox = 0;
            cv.clip = None;
            self.page = nouvelle;
        } else {
            self.dessiner_liste(&mut cv, px0, pad, inner, y_liste, self.selected);
        }

        // suivi de page : entre la liste et l'aide du bas
        if !self.console {
            self.points_pages(&mut cv, px0 + pw / 2, py0 + ph - 54);
        }

        // bas du panneau : juste les commandes, pas de journal technique
        // le pied laisse la place à la barre des tâches (panneau plein écran) ;
        // dans le menu système le cadre est court, l'aide reste près du bas
        let sy = py0 + ph - if self.console { 74 } else { 114 };
        // MODERNE : vrais glyphes de manette (Xelu), deux lignes courtes —
        // une ligne unique avec cinq glyphes serait illisible à cette largeur.
        if self.style_idx == 1 && !self.glyphes.is_empty() {
            if self.console {
                // menu système : l'aide est centrée dans le cadre (plus joli
                // qu'alignée à gauche sous un menu qui l'est aussi)
                let items: &[(&str, &str)] =
                    &[("dpad", "NAVIGUER"), ("a", "VALIDER"), ("b", "FERMER")];
                let w = self.aide_largeur(&cv, items);
                self.aide_ligne(&mut cv, px0 + (pw - w) / 2, sy + 10, items, th.dim);
            } else {
                self.aide_ligne(
                    &mut cv,
                    px0 + pad,
                    sy,
                    &[
                        ("dpad", "NAVIGUER"),
                        ("l1|r1", "MENU"),
                        ("l2|r2", "FENETRE"),
                    ],
                    th.dim,
                );
                self.aide_ligne(
                    &mut cv,
                    px0 + pad,
                    sy + 28,
                    &[("a", "VALIDER"), ("b", "FERMER")],
                    th.dim,
                );
            }
        } else {
            // bitmap : 12 px par caractère, 428 px utiles → 35 caractères
            // maximum. Une seule ligne pour tout dépassait et se faisait
            // tronquer (« ... MENU . », constaté) : trois lignes courtes.
            let aide = if self.console {
                "DPAD: NAVIGUER  A: VALIDER  B: FERMER"
            } else {
                "DPAD/STICK: NAVIGUER"
            };
            let x_aide = if self.console {
                // menu système : aide centrée (le cadre n'a rien à gauche)
                px0 + (pw - cv.largeur(aide, 2)) / 2
            } else {
                px0 + pad
            };
            cv.text_fit(x_aide, sy, aide, 2, th.dim, inner);
            if self.console {
                // une seule ligne d'aide en console (déjà écrite au-dessus)
            } else {
                cv.text_fit(
                    px0 + pad,
                    sy + 22,
                    "L1/R1: MENU   L2/R2: FENETRE",
                    2,
                    th.dim,
                    inner,
                );
                if self.pad_sony {
                    // manette Sony : croix = valider, rond = fermer
                    glyphe_croix(&mut cv, px0 + pad, sy + 46, 11, th.dim);
                    cv.text_fit(px0 + pad + 20, sy + 44, "VALIDER", 2, th.dim, inner - 20);
                    glyphe_rond(&mut cv, px0 + pad + 124, sy + 46, 6, th.dim);
                    cv.text_fit(px0 + pad + 148, sy + 44, "FERMER", 2, th.dim, inner - 148);
                } else {
                    cv.text_fit(
                        px0 + pad,
                        sy + 44,
                        "A: VALIDER   B: FERMER",
                        2,
                        th.dim,
                        inner,
                    );
                }
            }
        }

        // fondu : opacité courante appliquée à tout le tampon — menu système,
        // et aussi entrée du panneau latéral (il glisse **et** remonte : c'est
        // ce qui masque l'animation d'ouverture de KWin). Le panneau n'était
        // pas concerné tant qu'il ne fondait pas ; la garde `!self.side` faisait
        // donc un fondu fantôme.
        cv.appliquer_opacite(self.opacite);

        if let Some(surface) = &self.wl_surface {
            surface.attach(Some(&buf.buffer), 0, 0);
            surface.damage_buffer(0, 0, w, h);
            surface.commit();
        }
        self.dirty = false;
    }
}

/* ============================ dispatch wayland ============================ */

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore_events {
    ($($proxy:ty),* $(,)?) => {$(
        impl Dispatch<$proxy, ()> for App {
            fn event(
                _: &mut Self,
                _: &$proxy,
                _: <$proxy as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore_events!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    wl_region::WlRegion,
    wl_callback::WlCallback,
    wl_seat::WlSeat,
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
);

impl Dispatch<wl_buffer::WlBuffer, usize> for App {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        data: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            // buffer de nouveau libre pour le prochain dessin
            state.free.push(*data);
            if state.dirty {
                state.draw();
            }
        }
    }
}

/// Objets wayland du **clavier virtuel** : une seconde surface overlay, avec
/// ses propres tampons, distincte de celle du panneau — les données typées
/// permettent de distinguer les deux dans les `Dispatch`.
struct KbdSurf;

struct KbdBuf(usize);

impl Dispatch<wl_surface::WlSurface, KbdSurf> for App {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &KbdSurf,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, KbdSurf> for App {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &KbdSurf,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                let taille = (width as i32, height as i32);
                if taille.0 <= 0 || taille.1 <= 0 {
                    return;
                }
                if state.kbd_ecran == (0, 0) {
                    // phase 1 : sonde plein écran -> taille logique de l'écran
                    state.kbd_ecran = taille;
                    state.pret_clavier();
                    state.reduire_surface_clavier();
                    return;
                }
                // phase 2 : la surface a la taille du panneau
                if taille != state.kbd_size || state.kbd_buffers.is_empty() {
                    state.kbd_size = taille;
                    state.alloc_kbd_buffers();
                }
                if !state.kbd_configured {
                    state.kbd_configured = true;
                } else if let Some(c) = &mut state.clavier {
                    // écran redimensionné : on borne la position
                    c.ecran(state.kbd_ecran);
                }
                state.kbd_dirty = true;
            }
            zwlr_layer_surface_v1::Event::Closed => state.fermer_clavier(),
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, KbdBuf> for App {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        data: &KbdBuf,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            state.kbd_free.push(data.0);
            if state.kbd_dirty {
                state.draw_clavier();
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            // codes evdev bruts : 1 = Echap, 103/108 = haut/bas, 28 = Entrée
            match key {
                1 => {
                    println!("CLOSE (clavier)");
                    state.request_close();
                }
                103 => state.apply(Action::Up),
                108 => state.apply(Action::Down),
                28 => state.apply(Action::Accept),
                _ => {}
            }
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for App {
    fn event(
        state: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                let new_size = (width as i32, height as i32);
                if new_size != state.size || state.buffers.is_empty() {
                    state.size = new_size;
                    state.alloc_buffers();
                }
                state.configured = true;
                state.dirty = true;
            }
            zwlr_layer_surface_v1::Event::Closed => state.running = false,
            // pas d'animation sur fermeture imposée par le compositeur
            _ => {}
        }
    }
}

/* ============================ utilitaires ============================ */

fn find_pad_event() -> Option<String> {
    let dir = std::fs::read_dir("/sys/class/input").ok()?;
    for e in dir.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("input") {
            continue;
        }
        let path = format!("/sys/class/input/{name}");
        let devname = std::fs::read_to_string(format!("{path}/name")).unwrap_or_default();
        let lower = devname.trim().to_lowercase();
        // la DS4 expose aussi "Wireless Controller Touchpad"/"Motion Sensors" :
        // ce sont d'autres nodes input, sans BTN_MODE ni grab utile
        if ["touchpad", "touch pad", "sensors", "motion"]
            .iter()
            .any(|k| lower.contains(k))
        {
            continue;
        }
        let is_pad = ["wireless controller", "dualsense", "dualshock", "gamepad", "xbox"]
            .iter()
            .any(|k| lower.contains(k));
        if !is_pad {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&path) {
            for f in entries.flatten() {
                let n = f.file_name().to_string_lossy().to_string();
                if n.starts_with("event") {
                    return Some(format!("/dev/input/{n}"));
                }
            }
        }
    }
    None
}

/// Uniq (MAC / serial) du périphérique derrière un node evdev.
fn pad_uniq(evdev: &str) -> Option<String> {
    let base = std::path::Path::new(evdev).file_name()?.to_string_lossy().to_string();
    let u = std::fs::read_to_string(format!("/sys/class/input/{base}/device/uniq")).ok()?;
    let u = u.trim().to_string();
    if u.is_empty() {
        None
    } else {
        Some(u)
    }
}

/// Node hidraw de la MÊME manette (même HID_UNIQ), s'il existe.
/// MAC si la manette est connectée en **Bluetooth**, None sinon (USB, filaire,
/// ou manette virtuelle). Un périphérique HID Bluetooth vit sous uhid ; c'est
/// la même règle que `padnodes.sh` (transport=bt).
/// Manette Sony ? (c'est le couple croix/rond qu'on veut afficher, pas A/B)
fn pad_est_sony(evdev: &str) -> bool {
    let Some(base) = std::path::Path::new(evdev).file_name() else { return false };
    let chemin = format!("/sys/class/input/{}/device/id/vendor", base.to_string_lossy());
    std::fs::read_to_string(chemin)
        .map(|v| v.trim().eq_ignore_ascii_case("054c"))
        .unwrap_or(false)
}

fn pad_bt_mac(evdev: &str) -> Option<String> {
    let base = std::path::Path::new(evdev).file_name()?.to_string_lossy().to_string();
    let dev = std::fs::canonicalize(format!("/sys/class/input/{base}/device")).ok()?;
    let chemin = dev.to_string_lossy();
    if chemin.contains("/virtual/misc/uhid/") {
        // uniq peut être vide sur certains firmwares : on garde la manette
        // « Bluetooth » sans adresse, la déconnexion dira pourquoi.
        Some(pad_uniq(evdev).unwrap_or_default())
    } else {
        None
    }
}

/// Déconnexion de la manette Bluetooth (bluetoothctl, sans root).
///
/// **Détaché, jamais attendu** : `bluetoothctl disconnect` met jusqu'à une
/// seconde (voire plus) à rendre la main, et l'ancien `.output()` bloquait le
/// panneau — le menu restait affiché jusqu'à la disparition effective de la
/// manette (constaté 22/11/2026). Le menu se retire d'abord, la commande vit
/// sa vie.
fn bluetooth_disconnect(mac: &str) -> String {
    if mac.is_empty() {
        return "BT: ADRESSE INCONNUE".to_string();
    }
    let outil = std::env::var("BLUETOOTHCTL").unwrap_or_else(|_| "/usr/bin/bluetoothctl".to_string());
    if !std::path::Path::new(&outil).exists() {
        return "BT: BLUETOOTHCTL ABSENT".to_string();
    }
    match std::process::Command::new(&outil)
        .args(["disconnect", mac])
        .spawn()
    {
        Ok(_) => format!("BT: DECONNEXION {mac}"),
        Err(e) => format!("BT KO: {e}"),
    }
}

fn hidraw_for_uniq(uniq: &str) -> Option<String> {
    let want = format!("HID_UNIQ={uniq}");
    for e in std::fs::read_dir("/sys/class/hidraw").ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let uevent =
            std::fs::read_to_string(format!("/sys/class/hidraw/{name}/device/uevent")).ok()?;
        if uevent.lines().any(|l| l == want) {
            return Some(format!("/dev/{name}"));
        }
    }
    None
}

static ARRET_DEMANDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn sur_signal(_sig: libc::c_int) {
    ARRET_DEMANDE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Lance `$<var> <arg>` (script du banc) et renvoie sa première ligne de sortie,
/// ou un message honnête si la variable n'est pas configurée.
fn run_script_args(var: &str, args: &[&str]) -> String {
    match std::env::var(var) {
        Ok(p) if std::path::Path::new(&p).exists() => {
            match std::process::Command::new(&p).args(args).output() {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout);
                    let l = s.lines().next().unwrap_or("").trim().to_string();
                    if l.is_empty() {
                        format!("{var}: OK")
                    } else {
                        l
                    }
                }
                Err(e) => format!("{var} KO: {e}"),
            }
        }
        _ => format!("{var} NON CONFIGURE"),
    }
}

/// Croix PlayStation (valider) : deux diagonales, dessinées à la main — la
/// police bitmap n'a ni ✕ ni ○.
fn glyphe_croix(cv: &mut Canvas, x: i32, y: i32, s: i32, color: u32) {
    for i in 0..=s {
        cv.rect(x + i, y + i, 2, 2, color);
        cv.rect(x + s - i, y + i, 2, 2, color);
    }
}

/// Rond PlayStation (fermer).
fn glyphe_rond(cv: &mut Canvas, x: i32, y: i32, r: i32, color: u32) {
    let n = 40;
    for i in 0..n {
        let a = i as f32 * std::f32::consts::TAU / n as f32;
        let px = x + r + (r as f32 * a.cos()) as i32;
        let py = y + r + (r as f32 * a.sin()) as i32;
        cv.rect(px, py, 2, 2, color);
    }
}

/// Lance un script exporté par `run-gpad.sh` sans attendre sa fin.
fn lancer_script(var: &str, args: &[&str]) {
    lancer_script_env(var, args, &[]);
}

/// Idem, avec des variables d'environnement pour ce seul appel.
fn lancer_script_env(var: &str, args: &[&str], envs: &[(&str, &str)]) {
    let Ok(p) = std::env::var(var) else {
        println!("{var} NON CONFIGURE");
        return;
    };
    if !std::path::Path::new(&p).exists() {
        println!("{var} ABSENT ({p})");
        return;
    }
    let mut cmd = String::new();
    for (k, v) in envs {
        cmd.push_str(&format!("{k}={v} "));
    }
    cmd.push_str(&format!("'{p}'"));
    for a in args {
        cmd.push(' ');
        cmd.push_str(a);
    }
    let _ = std::process::Command::new("/usr/bin/sh")
        .arg("-c")
        .arg(cmd)
        .spawn();
}

/// Jellyfin côté client seulement : sans le flatpak, l'entrée n'a pas de sens.
fn jellyfin_installe() -> bool {
    std::process::Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("org.jellyfin.JellyfinDesktop"))
        .unwrap_or(false)
}

/// Un processus de ce nom tourne-t-il ? (`/proc/<pid>/comm`, pas de fork)
fn processus_actif(nom: &str) -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    for e in dir.flatten() {
        let p = e.path();
        let est_pid = p
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false);
        if !est_pid {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(p.join("comm")) {
            // `comm` est tronqué à 15 octets par le noyau : comparer le nom
            // entier ne matche JAMAIS un binaire au nom plus long
            // (« gpu-screen-recorder » -> « gpu-screen-reco », constaté)
            let court = &nom[..nom.len().min(15)];
            if comm.trim() == court {
                return true;
            }
        }
    }
    false
}

/// Arrête tous les processus de ce nom (SIGTERM).
fn arreter_processus(nom: &str) -> usize {
    let Ok(dir) = std::fs::read_dir("/proc") else { return 0 };
    let mut n = 0;
    for e in dir.flatten() {
        let p = e.path();
        let Some(pid) = p
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        if let Ok(comm) = std::fs::read_to_string(p.join("comm")) {
            if comm.trim() == nom {
                unsafe { libc::kill(pid, libc::SIGTERM) };
                n += 1;
            }
        }
    }
    n
}

/// Nom de processus d'un binaire configuré par variable (MOUSE, RUMBLE…).
fn nom_binaire(var: &str, defaut: &str) -> String {
    std::env::var(var)
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| defaut.to_string())
}

/// Lance (ou ferme) une application/media du dossier `scripts/`. Détaché :
/// `flatpak ps` met quelques centaines de ms, le panneau ne doit pas attendre.
fn lancer_appli(script: &str) -> String {
    let Ok(dir) = std::env::var("SCRIPTS") else {
        return "SCRIPTS NON CONFIGURE".to_string();
    };
    let p = format!("{dir}/{script}");
    if !std::path::Path::new(&p).exists() {
        return format!("{script} ABSENT");
    }
    println!("APPLI {p}");
    match std::process::Command::new("/usr/bin/sh")
        .arg("-c")
        .arg(format!("'{p}'"))
        .spawn()
    {
        Ok(_) => format!("{}: LANCEMENT", script.trim_start_matches("toggle-").to_uppercase()),
        Err(e) => format!("KO: {e}"),
    }
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Option dont la valeur est facultative (`--clavier` seul = manette
/// détectée). `Some(None)` = option présente sans valeur, `None` = absente.
/// Sans ça, `--clavier` suivi d'une autre option prenait `--gate` pour un node.
fn arg_optionnel(args: &[String], flag: &str) -> Option<Option<String>> {
    let i = args.iter().position(|a| a == flag)?;
    match args.get(i + 1) {
        Some(v) if !v.starts_with("--") => Some(Some(v.clone())),
        _ => Some(None),
    }
}

/// toutes les occurrences d'une option répétable (ex. --gate-target)
fn arg_values(args: &[String], flag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == flag {
            out.push(args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/* ================= fenêtre active (script KWin -> D-Bus) ================= */

/// Jeu en cours, publié par un lanceur tant que la partie dure.
///
/// Deux fichiers, même format :
/// - `<cache de glauncher>/running.txt` : écrit par glauncher, avec `slug=`,
///   `nom=`, `pids=` et les chemins `logo=` / `icone=` **déjà résolus** ;
/// - `<dossier d'exécution>/gablue/running.txt` : écrit par gwine/lgplaunch
///   quand le paquet est lancé **sans** le launcher, avec `nom=`, `paquet=`
///   (chemin du `.wgp`/`.lgp`) et `pids=`.
///
/// C'est le **lien** entre la fenêtre active et le jeu : l'overlay ne devine
/// rien (ni classe de fenêtre, ni nom d'exécutable). Le nom et l'icône viennent
/// du **paquet** : la fenêtre d'un jeu `gwine`/`lgplaunch` porte le titre du
/// binaire et l'icône de l'exécutable, pas celles du `.wgp`/`.lgp`.
#[derive(Default, Clone, PartialEq)]
struct JeuActif {
    slug: String,
    /// nom affiché du paquet (nom du fichier sans extension)
    nom: String,
    /// chemin du `.wgp`/`.lgp` (vide quand seul glauncher publie : le lien par
    /// environnement s'appuie alors sur la cmdline du PID publié)
    paquet: String,
    pids: Vec<i32>,
    /// chemin absolu du logo (vide = visuel absent du cache : pas de bandeau)
    logo: String,
    /// chemin absolu de l'icône du paquet (vide = rien à substituer)
    icone: String,
}

/// Cache de glauncher (`GLAUNCHER_CACHE` au banc).
///
/// Le launcher lit le sien dans `config.toml` : l'overlay suit la même clé
/// (`cache_dir`, top-level) au lieu de supposer `~/.local/share`, sinon un
/// cache déplacé rendrait le logo invisible sans rien dire. Le défaut de
/// l'image reste `~/.local/share/glauncher`.
fn cache_glauncher() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("GLAUNCHER_CACHE") {
        return std::path::PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(&home).join(".config"));
    if let Ok(txt) = std::fs::read_to_string(xdg.join("glauncher").join("config.toml")) {
        for ligne in txt.lines() {
            let Some(reste) = ligne.trim().strip_prefix("cache_dir") else {
                continue;
            };
            let Some(valeur) = reste.trim().strip_prefix('=') else {
                continue;
            };
            let valeur = valeur.trim().trim_matches('"').trim();
            if !valeur.is_empty() {
                return match valeur.strip_prefix("~/") {
                    Some(reste) => std::path::PathBuf::from(&home).join(reste),
                    None => std::path::PathBuf::from(valeur),
                };
            }
        }
    }
    std::path::PathBuf::from(home).join(".local/share/glauncher")
}

/// Dossier où `gwine` et `lgplaunch` publient leur partie : celui de
/// l'exécution de session, exactement comme les deux scripts le calculent
/// (`$XDG_RUNTIME_DIR/gablue`, repli `/tmp/gablue-<uid>`).
fn dossier_lanceurs() -> std::path::PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => std::path::PathBuf::from(dir).join("gablue"),
        // SAFETY : geteuid ne peut pas échouer
        None => std::path::PathBuf::from(format!("/tmp/gablue-{}", unsafe {
            libc::geteuid()
        })),
    }
}

/// Les deux publications possibles, la plus riche d'abord (glauncher résout
/// lui-même logo et icône ; gwine/lgplaunch ne publient que le paquet).
fn fichiers_jeu() -> [std::path::PathBuf; 2] {
    [
        cache_glauncher().join("running.txt"),
        dossier_lanceurs().join("running.txt"),
    ]
}

/// Jeux publiés en ce moment (ceux dont au moins un PID vit encore).
fn lire_jeux_actifs() -> Vec<JeuActif> {
    let cache = cache_glauncher();
    fichiers_jeu()
        .iter()
        .filter_map(|chemin| lire_jeu(chemin, &cache))
        .collect()
}

/// Lit un `running.txt` ; `None` si absent, vide, ou si aucun PID publié
/// n'existe encore (fichier laissé par un lanceur tué en pleine partie).
fn lire_jeu(chemin: &std::path::Path, cache: &std::path::Path) -> Option<JeuActif> {
    let texte = std::fs::read_to_string(chemin).ok()?;
    let mut jeu = JeuActif::default();
    let mut paquet = String::new();
    for ligne in texte.lines() {
        let Some((cle, valeur)) = ligne.split_once('=') else {
            continue;
        };
        match cle.trim() {
            "slug" => jeu.slug = valeur.trim().to_string(),
            "nom" => jeu.nom = valeur.trim().to_string(),
            "logo" => jeu.logo = valeur.trim().to_string(),
            "icone" => jeu.icone = valeur.trim().to_string(),
            "paquet" => paquet = valeur.trim().to_string(),
            "pids" => {
                jeu.pids = valeur
                    .split(',')
                    .filter_map(|p| p.trim().parse::<i32>().ok())
                    .collect()
            }
            _ => {}
        }
    }
    // un PID mort ne désigne plus un jeu : le fichier d'une session tuée
    // (SIGKILL du launcher) ne doit pas faire apparaître un logo au hasard
    jeu.pids
        .retain(|pid| *pid > 1 && std::path::Path::new(&format!("/proc/{pid}")).exists());
    if jeu.pids.is_empty() {
        return None;
    }
    // gwine/lgplaunch publient le PAQUET : il donne le slug, donc les visuels
    // du cache. Relus à chaque passe (une fois par seconde) : un logo
    // téléchargé après le lancement par `glauncher --art` apparaît de
    // lui-même, sans que le lanceur ait à republier quoi que ce soit.
    // Sans paquet (gwine lancé sur un `.exe` seul), le nom publié sert de
    // base : l'en-tête montre au moins le nom du programme, sans visuel.
    let origine = if paquet.is_empty() {
        jeu.nom.clone()
    } else {
        nom_paquet(&paquet)
    };
    jeu.paquet = paquet;
    if jeu.slug.is_empty() {
        jeu.slug = slugify(&origine);
    }
    if jeu.nom.is_empty() {
        jeu.nom = origine;
    }
    // Les visuels sont nommés d'après le JEU SteamGridDB (`assets/identities.txt`
    // partagé par Syncthing), pas d'après le paquet : deux `.wgp`/`.lgp` du
    // même jeu dont les noms diffèrent tombent sur les mêmes images.
    let cle = cle_visuels(cache, &jeu.slug);
    if jeu.logo.is_empty() {
        jeu.logo = visuel_png(cache, "logos", &cle);
    }
    if jeu.icone.is_empty() {
        jeu.icone = icone_png(cache, &cle, &jeu.slug);
    }
    if jeu.nom.is_empty() {
        return None;
    }
    Some(jeu)
}

/// Nom d'un paquet : nom de fichier sans l'extension (`.wgp`, `.lgp`).
fn nom_paquet(chemin: &str) -> String {
    std::path::Path::new(chemin)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Même règle que `library::slugify` du launcher (c'est le nom de fichier des
/// visuels dans le cache : les deux doivent tomber d'accord).
fn slugify(nom: &str) -> String {
    let mut slug = String::with_capacity(nom.len());
    for ch in nom.chars() {
        if ch.is_alphanumeric() {
            slug.extend(ch.to_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

/// `<cache>/assets/<type>/<clé>.png` s'il existe. **PNG seulement** :
/// l'overlay ne décode que ça, les autres formats du cache sont convertis par
/// `glauncher --art`.
fn visuel_png(cache: &std::path::Path, genre: &str, cle: &str) -> String {
    if cle.is_empty() {
        return String::new();
    }
    let chemin = cache
        .join("assets")
        .join(genre)
        .join(format!("{cle}.png"));
    if chemin.is_file() {
        chemin.display().to_string()
    } else {
        String::new()
    }
}

/// Clé des visuels d'un paquet : celle de **son jeu SteamGridDB** quand il est
/// identifié, sinon son slug. Même règle que `covers::key` du launcher :
/// `<id>-<nom slugifié>` (`8103-doom`), ou `<id>` seul si le nom manque.
///
/// L'identification vit dans `assets/identities.txt` (dossier partagé par
/// Syncthing), format `slug=<id>` puis, après une tabulation, le nom SGDB. Un
/// paquet absent du fichier (jamais identifié) garde son slug : ses visuels
/// restent alors propres à la machine.
fn cle_visuels(cache: &std::path::Path, slug: &str) -> String {
    if slug.is_empty() {
        return String::new();
    }
    let Ok(texte) = std::fs::read_to_string(cache.join("assets/identities.txt")) else {
        return slug.to_string();
    };
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let Some((paquet, valeur)) = ligne.split_once('=') else {
            continue;
        };
        if paquet.trim() != slug {
            continue;
        }
        let valeur = valeur.trim();
        let (id, nom) = match valeur.split_once('\t') {
            Some((id, nom)) => (id.trim(), nom.trim()),
            None => match valeur.split_once(' ') {
                Some((id, nom)) => (id.trim(), nom.trim()),
                None => (valeur, ""),
            },
        };
        let Ok(id) = id.parse::<u64>() else {
            continue;
        };
        return if nom.is_empty() {
            id.to_string()
        } else {
            format!("{id}-{}", slugify(nom))
        };
    }
    slug.to_string()
}

/// Icône du jeu, même ordre que le launcher : **`assets/icons/<clé>.png`
/// d'abord** (icône SteamGridDB, y compris celle choisie à la main — le dossier
/// `assets/` est partagé entre postes, un choix fait ailleurs doit gagner),
/// puis `<id>.png` (visuel écrit avant de connaître le nom du jeu), puis
/// l'icône extraite du paquet (`pack-icons/<slug>.png`, propre à la machine).
fn icone_png(cache: &std::path::Path, cle: &str, slug: &str) -> String {
    let icons = cache.join("assets/icons");
    let mut essais = vec![icons.join(format!("{cle}.png"))];
    if let Some((id, _)) = cle.split_once('-') {
        essais.push(icons.join(format!("{id}.png")));
    }
    essais.push(cache.join("pack-icons").join(format!("{slug}.png")));
    essais
        .into_iter()
        .find(|chemin| chemin.is_file())
        .map(|chemin| chemin.display().to_string())
        .unwrap_or_default()
}

/// Session POSIX d'un processus (champ `session` de `/proc/<pid>/stat`).
fn session_of(pid: i32) -> Option<i32> {
    if pid < 1 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (comm) état ppid pgrp session …` : `comm` peut contenir des espaces
    // et des parenthèses, d'où la découpe après la dernière
    let reste = stat.rsplit_once(')').map(|(_, reste)| reste)?;
    let mut champs = reste.split_whitespace();
    champs.next()?; // état
    champs.next()?; // ppid
    champs.next()?; // pgrp
    champs.next()?.parse().ok()
}

/// Variables que gwine et lgplaunch laissent dans l'environnement du jeu, avec
/// le chemin du paquet lancé. C'est la seule trace qui survit à tous les
/// `fork`/`exec` d'un jeu.
const ENV_PAQUET: [&str; 2] = ["GWINE_WGP_FILE", "LGPACK_FILE"];

/// Chemin de paquet lu dans un environnement `KEY=valeur\0…` (vide = aucune
/// des deux variables, ou valeur vide).
fn paquet_dans_environ(brut: &[u8]) -> Option<String> {
    for entree in brut.split(|octet| *octet == 0) {
        let Ok(texte) = std::str::from_utf8(entree) else {
            continue;
        };
        for cle in ENV_PAQUET {
            if let Some(valeur) = texte
                .strip_prefix(cle)
                .and_then(|reste| reste.strip_prefix('='))
            {
                let valeur = valeur.trim();
                if !valeur.is_empty() {
                    return Some(valeur.to_string());
                }
            }
        }
    }
    None
}

/// Paquet annoncé par l'environnement d'un processus (`None` si illisible ou
/// sans marqueur).
fn paquet_du_processus(pid: i32) -> Option<String> {
    if pid < 1 {
        return None;
    }
    let brut = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    paquet_dans_environ(&brut)
}

/// Vrai si le chemin est un **argument entier** de la cmdline du processus.
/// Comparaison d'arguments, jamais de sous-chaîne : deux jeux peuvent partager
/// un préfixe de chemin.
fn cmdline_contient(pid: i32, chemin: &str) -> bool {
    if pid < 1 {
        return false;
    }
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|brut| {
            brut.split(|octet| *octet == 0)
                .any(|arg| arg == chemin.as_bytes())
        })
        .unwrap_or(false)
}

/// Le jeu auquel appartient cette fenêtre, s'il y en a un.
///
/// Quatre liens, du plus sûr au plus large :
/// 1. le PID de la fenêtre est publié tel quel ;
/// 2. il descend d'un PID publié (jeu natif, jeu dans son sandbox) ;
/// 3. l'**environnement** du processus nomme le paquet (`GWINE_WGP_FILE`,
///    `LGPACK_FILE`) et ce chemin est celui publié — ou un argument de la
///    cmdline d'un PID publié (cas où seul glauncher publie) ;
/// 4. il est dans la **même session** qu'un descendant publié : Wine reparente
///    ses processus à systemd, la fenêtre n'est donc plus dans l'arbre, mais
///    elle garde la session créée par `bwrap --new-session`.
///
/// Le lien 3 est celui qui sauve les jeux à launcher interne (Steam, Unity) :
/// leurs processus sont reparentés à systemd **et** prennent chacun leur propre
/// session (setsid du wineserver), donc ni l'arbre ni la session ne les
/// rattachent (vécu Sonic Superstars 19/11/2026). La session de gpad est
/// exclue du lien 4 : un paquet lancé depuis un terminal est dans la session
/// du bureau, et toutes les fenêtres matcheraient.
fn jeu_de(jeux: &[JeuActif], pid: i32) -> Option<JeuActif> {
    if pid < 1 {
        return None;
    }
    for jeu in jeux {
        if jeu.pids.contains(&pid) {
            return Some(jeu.clone());
        }
    }
    let ma_session = session_of(std::process::id() as i32);
    let paquet_fenetre = paquet_du_processus(pid);
    for jeu in jeux {
        let mut arbre: Vec<i32> = Vec::new();
        for racine in &jeu.pids {
            arbre.extend(descendants(*racine));
        }
        if arbre.contains(&pid) {
            return Some(jeu.clone());
        }
        if let Some(paquet) = &paquet_fenetre {
            let meme_paquet = if jeu.paquet.is_empty() {
                jeu.pids.iter().any(|p| cmdline_contient(*p, paquet))
            } else {
                jeu.paquet == *paquet
            };
            if meme_paquet {
                return Some(jeu.clone());
            }
        }
        let Some(session) = session_of(pid) else {
            continue;
        };
        if Some(session) == ma_session {
            continue;
        }
        let mut sessions: Vec<Option<i32>> = jeu.pids.iter().map(|p| session_of(*p)).collect();
        sessions.extend(arbre.iter().map(|p| session_of(*p)));
        if sessions.contains(&Some(session)) {
            return Some(jeu.clone());
        }
    }
    None
}

/// Une info KWin annonce-t-elle une **vraie** fenêtre ? Le script envoie une
/// info entièrement vide quand il rejette la fenêtre active (pas de
/// `normalWindow`, `skipTaskbar`…) ; une fenêtre réelle peut n'avoir **aucun
/// titre** (Project Midas : titre SDL vide) et n'est alors identifiable que par
/// sa classe. Tester le titre seul faisait passer un jeu au premier plan pour
/// « aucune fenêtre ».
fn fenetre_annoncee(caption: &str, class: &str) -> bool {
    !(caption.is_empty() && class.is_empty())
}

/// Fenêtre active telle que KWin la voit, poussée par `kwin/wininfo.js`.
#[derive(Default, Clone, PartialEq)]
struct WinInfo {
    caption: String,
    class: String,
    pid: i32,
    /// nom du .desktop (sert à retrouver l'icône)
    desktop: String,
    fullscreen: bool,
    /// UUID KWin (`internalId`) de la fenêtre active : **ancre** du cyclage
    /// L2/R2 (le script la cherche dans la liste, il ne devine pas)
    uuid: String,
}

/// Une fenêtre de la liste (pour la page « fenêtres »).
#[derive(Default, Clone, PartialEq)]
struct Fenetre {
    caption: String,
    class: String,
    pid: i32,
    /// desktopFileName : sert à retrouver l'icône de la ligne
    desktop: String,
    /// UUID KWin (`internalId`) : sert à réactiver la bonne fenêtre
    uuid: String,
    active: bool,
}

/// Script KWin de la bascule jeu ⇄ application compagne.
///
/// L'activation seule ne suffit pas : la fenêtre du jeu est plein écran, donc
/// dans la couche au-dessus des fenêtres normales — l'app restait derrière
/// (constaté le 23/11/2026). Le plein écran du jeu est donc piloté en même
/// temps que l'activation, dans un seul script (deux chargements KWin se
/// marcheraient dessus).
/// Flux audio attribuables au jeu : ceux dont le PID est celui de sa fenêtre
/// ou l'un de ses descendants (un émulateur délègue souvent le son à un enfant),
/// en excluant les applications compagnes — leur nom peut contenir celui du
/// jeu, et leur son ne doit jamais être coupé par la bascule.
fn flux_du_jeu(flux: &[audio::Flux], pids: &[i32]) -> Vec<audio::Flux> {
    flux.iter()
        .filter(|f| f.pid > 0 && pids.contains(&f.pid) && !audio::est_compagne(&f.nom))
        .cloned()
        .collect()
}

fn script_bascule(jeu: &str, cible: &str, jeu_plein_ecran: bool) -> String {
    format!(
        "var ws = workspace.stackingOrder;\n\
         for (var i = 0; i < ws.length; i++) {{\n\
         \x20   var w = ws[i];\n\
         \x20   if (String(w.internalId) === \"{jeu}\") {{\n\
         \x20       w.fullScreen = {plein};\n\
         \x20       w.keepAbove = false;\n\
         \x20   }}\n\
         \x20   if (String(w.internalId) === \"{cible}\") {{\n\
         \x20       w.keepAbove = false;\n\
         \x20       workspace.activeWindow = w;\n\
         \x20   }}\n\
         }}\n",
        plein = if jeu_plein_ecran { "true" } else { "false" },
    )
}

impl Fenetre {
    /// Nom affichable : le titre, ou la classe quand la fenêtre n'en a pas
    /// (Project Midas envoie un titre vide) — une ligne sans texte serait
    /// inutilisable dans la page FENETRES et dans le statut d'activation.
    fn titre(&self) -> &str {
        if self.caption.is_empty() {
            &self.class
        } else {
            &self.caption
        }
    }
}

/// Ce que le script KWin pousse : la fenêtre active et, sur demande, la liste.
#[derive(Default)]
struct Partage {
    info: WinInfo,
    liste: Vec<Fenetre>,
    /// nombre d'entrées annoncé par `Fin(n)` : 0 = liste pas (encore) arrivée
    fin: usize,
    /// au moins un `Info` reçu (preuve que le script KWin tourne vraiment)
    recu: bool,
    /// instant du dernier `Info` : le script pousse toutes les 300 ms, donc
    /// plusieurs secondes de silence = script déchargé (KWin redémarré)
    dernier: Option<Instant>,
}

struct WinPartage(Arc<Mutex<Partage>>);

#[zbus::interface(name = "org.gablue.Win")]
impl WinPartage {
    fn info(
        &self,
        caption: &str,
        class: &str,
        pid: i32,
        desktop: &str,
        fullscreen: bool,
        uuid: &str,
    ) {
        if let Ok(mut p) = self.0.lock() {
            p.info.caption = caption.to_string();
            p.info.class = class.to_string();
            p.info.pid = pid;
            p.info.desktop = desktop.to_string();
            p.info.fullscreen = fullscreen;
            p.info.uuid = uuid.to_string();
            p.recu = true;
            p.dernier = Some(Instant::now());
        }
    }

    /// Une fenêtre de la liste (`lister.js` en pousse une par appel).
    fn entree(
        &self,
        index: i32,
        caption: &str,
        class: &str,
        pid: i32,
        uuid: &str,
        desktop: &str,
        active: bool,
    ) {
        if let Ok(mut p) = self.0.lock() {
            let i = index.max(0) as usize;
            while p.liste.len() <= i {
                p.liste.push(Fenetre::default());
            }
            eprintln!("DBG entree[{i}] class={class}");
            p.liste[i] = Fenetre {
                caption: caption.to_string(),
                class: class.to_string(),
                pid,
                desktop: desktop.to_string(),
                uuid: uuid.to_string(),
                active,
            };
        }
    }

    /// Fin de liste : `total` = nombre de fenêtres poussées.
    fn fin(&self, total: i32) {
        if let Ok(mut p) = self.0.lock() {
            let total = total.max(0) as usize;
            p.liste.truncate(total);
            p.fin = total;
            eprintln!("DBG fin total={total} liste={}", p.liste.len());
        }
    }
}

/// Service D-Bus du panneau : KWin n'a pas de méthode « fenêtre active », un
/// script pousse donc l'info ici. Le thread de service vit tant que l'overlay
/// vit ; le script est déchargé à la fermeture (son QTimer part avec lui).
fn demarrer_wininfo(bus: &str) -> Option<Arc<Mutex<Partage>>> {
    let etat = Arc::new(Mutex::new(Partage::default()));
    // La connexion et le NOM sont pris ICI, avant tout script KWin : sinon le
    // premier `callDBus` du script (exécuté au chargement) part vers un nom
    // que personne ne possède encore — il échoue et le script s'arrête avant
    // d'avoir créé son QTimer (constaté : aucun « WIN », aucune erreur).
    let conn = zbus::blocking::Connection::session().ok()?;
    conn.object_server().at("/win", WinPartage(etat.clone())).ok()?;
    if conn.request_name(bus).is_err() {
        eprintln!("wininfo: nom {bus} déjà pris");
        return None;
    }
    // l'executor de zbus tourne dans son propre thread : il suffit de garder
    // la connexion vivante
    std::thread::spawn(move || {
        let _conn = conn;
        loop {
            std::thread::park();
        }
    });
    Some(etat)
}

/// Pilotage de KWin par D-Bus (`/Scripting`) : charger/exécuter/décharger.
struct KBus {
    conn: zbus::blocking::Connection,
}

impl KBus {
    fn nouveau() -> Option<Self> {
        zbus::blocking::Connection::session()
            .ok()
            .map(|conn| Self { conn })
    }

    fn charger(&self, chemin: &str, nom: &str) -> Option<i32> {
        let r = match self.conn.call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "loadScript",
            &(chemin, nom),
        ) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("kwin loadScript: {e}");
                return None;
            }
        };
        match r.body().deserialize::<i32>() {
            Ok(id) => {
                eprintln!("kwin: loadScript({chemin}, {nom}) -> id={id}");
                Some(id)
            }
            Err(e) => {
                eprintln!("kwin loadScript corps: {e}");
                None
            }
        }
    }

    /// Démarre les scripts chargés via `Scripting.start()`.
    ///
    /// On n'appelle PAS `Script{id}.run()` : côté KWin, `Script::run()` sort
    /// sans rien faire si `running()` ou `m_starting` est vrai, et le chemin
    /// `/Scripting/Script{id}` peut encore désigner le script PRÉCÉDENT (les
    /// ids sont réutilisés, `unloadScript` détruit en différé). Résultat
    /// observé : `loadScript` rend un id, `run` répond sans erreur, et le
    /// script ne s'exécute jamais. `start()` parcourt la vraie liste de
    /// scripts et ignore ceux qui tournent déjà.
    fn demarrer_scripts(&self) -> bool {
        match self.conn.call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "start",
            &(),
        ) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("kwin start: {e}");
                false
            }
        }
    }

    fn decharger(&self, nom: &str) -> bool {
        self.conn
            .call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                "unloadScript",
                &(nom,),
            )
            .is_ok()
    }

    /// Script jetable : chargé, exécuté, déchargé.
    fn un_shot(&self, chemin: &str, nom: &str) -> bool {
        let Some(_id) = self.charger(chemin, nom) else {
            return false;
        };
        let ok = self.demarrer_scripts();
        let _ = self.decharger(nom);
        ok
    }

    /// Charge et exécute sans décharger : pour un script qui émet des
    /// `callDBus` différés (la liste des fenêtres), le déchargement immédiat
    /// peut perdre les appels encore en file. L'appelant décharge après
    /// réception.
    fn lancer_nomme(&self, chemin: &str, nom: &str) -> bool {
        let Some(_id) = self.charger(chemin, nom) else {
            return false;
        };
        self.demarrer_scripts()
    }

    /// Charge le script qui pousse la fenêtre active (nom fixe -> on peut le
    /// décharger proprement à la fermeture).
    fn demarrer_wininfo(&self, chemin: &str, nom: &str) -> bool {
        let Some(_id) = self.charger(chemin, nom) else {
            return false;
        };
        self.demarrer_scripts()
    }
}

/* ====================== icône de la fenêtre active ====================== */

/// Image RGBA prête à blitter.
struct Icone {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

/// Hauteur maximale du logo d'un jeu au-dessus de l'en-tête (px). Au-delà, le
/// bandeau mangerait la liste : un logo SteamGridDB est une image large, sa
/// hauteur utile tient largement dans 96 px sur un panneau de 480.
const LOGO_MAX_H: i32 = 96;

/// Espace **identique** au-dessus et au-dessous du logo (demandé le 19/11 :
/// le premier jet laissait 10 px en haut et 60 en bas, l'écart venant des 50 px
/// de marge fixe de l'en-tête, d'où le calcul de `decalage_logo`).
const LOGO_MARGE: i32 = 16;

/// Cherche `<name>.{png,svg}` dans les thèmes/tailles usuels. Deux dispositions
/// coexistent : `hicolor/48x48/apps/x` et `breeze/apps/48/x`.
fn chercher_icone(nom: &str) -> Option<std::path::PathBuf> {
    if nom.is_empty() {
        return None;
    }
    // `Icon=` peut être un CHEMIN ABSOLU (ES-DE : ~/AppImages/.icons/esde, un SVG
    // sans extension) : on le prend tel quel avant de chercher dans les thèmes.
    if nom.starts_with('/') {
        for cand in [nom.to_string(), format!("{nom}.png"), format!("{nom}.svg")] {
            let p = std::path::PathBuf::from(cand);
            if p.is_file() {
                return Some(p);
            }
        }
        return None;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let racines = [
        format!("{home}/.local/share/icons"),
        "/var/lib/flatpak/exports/share/icons".to_string(),
        format!("{home}/.local/share/flatpak/exports/share/icons"),
        "/usr/share/icons".to_string(),
        "/usr/local/share/icons".to_string(),
    ];
    let themes = ["hicolor", "breeze", "breeze-dark", "Adwaita", "AdwaitaLegacy"];
    // « apps » ne suffit pas : video-display (Gablue TV) vit dans devices chez
    // Adwaita (scalable) et dans legacy chez AdwaitaLegacy.
    let categories = ["apps", "devices", "legacy"];
    // deux conventions : « 48x48 » (hicolor) et « 48 » (breeze/apps/48)
    let tailles = [
        "128x128", "128", "96x96", "96", "64x64", "64", "48x48", "48", "32x32", "32", "256x256",
        "256", "scalable",
    ];
    for r in &racines {
        for t in themes {
            for taille in tailles {
                for ext in ["png", "svg"] {
                    for cat in categories {
                        for cand in [
                            format!("{r}/{t}/{taille}/{cat}/{nom}.{ext}"),
                            format!("{r}/{t}/{cat}/{taille}/{nom}.{ext}"),
                        ] {
                            let p = std::path::PathBuf::from(cand);
                            if p.is_file() {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Décode un PNG en RGBA (les SVG ont été rastérisés avant).
fn lire_png(chemin: &std::path::Path) -> Option<Icone> {
    let donnees = std::fs::read(chemin).ok()?;
    lire_png_depuis(&donnees)
}

/// Idem depuis la mémoire : les glyphes de manette sont **embarqués** dans le
/// binaire (`include_bytes!`), pas lus sur le disque au démarrage.
fn lire_png_depuis(donnees: &[u8]) -> Option<Icone> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(donnees));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as i32, info.height as i32);
    let px = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 0xff])
            .collect(),
        png::ColorType::Grayscale => buf[..info.buffer_size()]
            .iter()
            .flat_map(|g| [*g, *g, *g, 0xff])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()]
            .chunks(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        _ => return None,
    };
    Some(Icone { w, h, px })
}

/// Délai minimal entre deux pas au **stick** (le D-pad n'est pas concerné) :
/// en dessous, un stick qui traverse le seuil d'un coup ou rebondit ferait
/// sauter deux ou trois lignes d'un même geste.
const PAS_MIN: Duration = Duration::from_millis(140);

/// Taille des glyphes de manette dans l'aide du bas (un peu plus grands
/// qu'avant : à 21 px les lettres des boutons restaient illisibles).
const TAILLE_GLYPHE: i32 = 24;

/// Script KWin jetable du cyclage L2/R2 : `__CIBLE__` = UUID de la fenêtre
/// active, `__SENS__` = -1 (précédente) / +1 (suivante).
const CYCLE_JS: &str = r#"var cible = "__CIBLE__";
var sens = __SENS__;
var ws = workspace.stackingOrder;
var l = [];
for (var i = 0; i < ws.length; i++) {
    var w = ws[i];
    if (!w.normalWindow || w.skipTaskbar || !w.resourceClass) {
        continue;
    }
    l.push(w);
}
l.sort(function (a, b) {
    var ga = a.frameGeometry, gb = b.frameGeometry;
    if (ga.x !== gb.x) { return ga.x - gb.x; }
    if (ga.y !== gb.y) { return ga.y - gb.y; }
    var ia = String(a.internalId), ib = String(b.internalId);
    return ia < ib ? -1 : (ia > ib ? 1 : 0);
});
if (l.length) {
    var j = -1;
    for (var i = 0; i < l.length; i++) {
        if (String(l[i].internalId) === cible) { j = i; break; }
    }
    if (j < 0) { j = l.indexOf(workspace.activeWindow); }
    if (j < 0) { j = sens > 0 ? -1 : 0; }
    j = (j + sens + l.length) % l.length;
    workspace.activeWindow = l[j];
}
"#;

/// Sigles qui restent en capitales dans les libellés de MODERNE.
const SIGLES: [&str; 13] = [
    "fps", "vrr", "tv", "hud", "rec", "usb", "bt", "ok", "dpad", "l1", "r1", "l2", "r2",
];

/// Mots accentués (le source est en capitales ASCII, la TTF sait afficher les
/// accents ; sans ça « FENETRE » et « DUREE » restent fautifs en français).
const ACCENTS: [(&str, &str); 22] = [
    ("fenetre", "fenêtre"),
    ("fenetres", "Fenêtres"),
    ("duree", "durée"),
    ("ecran", "écran"),
    ("reglages", "réglages"),
    ("arret", "arrêt"),
    ("arreter", "arrêter"),
    ("eteindre", "éteindre"),
    ("deconnecter", "déconnecter"),
    ("demarrer", "démarrer"),
    ("redemarrer", "redémarrer"),
    ("precedent", "précédent"),
    ("defaut", "défaut"),
    ("pret", "prêt"),
    ("video", "vidéo"),
    ("masque", "masqué"),
    ("depart", "départ"),
    ("mangohud", "MangoHud"),
    ("emulationstation", "EmulationStation"),
    ("jellyfin", "Jellyfin"),
    ("youtube", "YouTube"),
    ("retro", "Retro"),
];

/// Axe evdev → −1..1 (0 au centre), d'après la plage réelle du périphérique.
/// **Borné** : un périphérique peut annoncer une plage étroite (`0..1`) et
/// envoyer bien au-delà ; sans borne, un seul événement produisait un
/// déplacement de 2 Gpx (débordement constaté sur la manette de banc).
fn norme_axe(v: i32, plage: (i32, i32)) -> f64 {
    let (mn, mx) = plage;
    let demi = ((mx - mn) / 2).max(1) as f64;
    ((v as f64 - (mn as f64 + demi)) / demi).clamp(-1.0, 1.0)
}

/// Position mémorisée du clavier (`~/.config/gablue/gpad-clavier`), en pixels
/// écran : déplacé au stick droit, il doit revenir au même endroit.
fn pos_clavier_fichier() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/gablue/gpad-clavier"))
}

fn lire_pos_clavier() -> Option<(i32, i32)> {
    let txt = std::fs::read_to_string(pos_clavier_fichier()?).ok()?;
    let mut mots = txt.split_whitespace();
    Some((mots.next()?.parse().ok()?, mots.next()?.parse().ok()?))
}

fn ecrire_pos_clavier(x: i32, y: i32) {
    let Some(p) = pos_clavier_fichier() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("{x} {y}\n"));
}

/// Libellé lisible pour le style MODERNE : « CHOISIR FENETRE » → « Choisir
/// fenêtre ». Ne touche QUE nos libellés (entièrement en capitales) : les
/// légendes de fenêtres, les noms d'applis et les valeurs gardent leur casse.
/// Le RETRO garde ses capitales (police bitmap, aucune minuscule).
fn joli(s: &str) -> String {
    // jamais de minuscule = libellé maison ; sinon (légende, nom propre) : tel quel
    if s.chars().any(|c| c.is_lowercase()) || !s.chars().any(|c| c.is_alphabetic()) {
        return s.to_string();
    }
    // quelques libellés dont la casse de phrase serait ambiguë
    match s {
        "STOP ENREGISTRER" => return "Arrêter la vidéo".to_string(),
        "SAUVER REPLAY" => return "Sauver le replay".to_string(),
        "BLOQUE (REC)" => return "Bloqué (REC)".to_string(),
        "INACTIF" => return "Inactif".to_string(),
        _ => {}
    }
    let mut out = String::with_capacity(s.len());
    let mut premier = true;
    for (i, mot) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // ponctuation de tête et de queue : « (REC) », « A: », « ? »
        let debut = mot.len() - mot.trim_start_matches(|c: char| !c.is_alphanumeric()).len();
        let tete = &mot[..debut];
        let reste = &mot[debut..];
        // `queue` compte la ponctuation de FIN ; le cœur s'arrête avant elle
        // (`..fin` coupait à la longueur de la queue, donc cœur vide)
        let queue = reste.len() - reste.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
        let coeur = &reste[..reste.len() - queue];
        let queue = &reste[reste.len() - queue..];
        out.push_str(tete);
        if coeur.chars().count() < 2 || !coeur.chars().all(|c| c.is_ascii_alphabetic()) {
            // « L1 », « A », « 60 », « MM:SS » : rien à casser
            out.push_str(coeur);
            out.push_str(queue);
            continue;
        }
        let bas = coeur.to_ascii_lowercase();
        let mut rendu = if SIGLES.contains(&bas.as_str()) {
            coeur.to_string()
        } else {
            ACCENTS
                .iter()
                .find(|(k, _)| *k == bas)
                .map(|(_, v)| v.to_string())
                .unwrap_or(bas)
        };
        if premier {
            let mut cs = rendu.chars();
            if let Some(c) = cs.next() {
                rendu = c.to_uppercase().collect::<String>() + cs.as_str();
            }
            premier = false;
        }
        out.push_str(&rendu);
        out.push_str(queue);
    }
    out
}

/// Glyphes de manette embarqués (Xelu, CC0 — `assets/glyphes/README-Xelu-CC0.txt`).
/// Deux familles : `ps` (croix/rond/carré/triangle) et `xbox` (A/B/X/Y).
macro_rules! jeu_glyphes {
    ($dossier:literal) => {
        [
            ("a", include_bytes!(concat!("../assets/glyphes/", $dossier, "/a.png"))),
            ("b", include_bytes!(concat!("../assets/glyphes/", $dossier, "/b.png"))),
            ("x", include_bytes!(concat!("../assets/glyphes/", $dossier, "/x.png"))),
            ("y", include_bytes!(concat!("../assets/glyphes/", $dossier, "/y.png"))),
            ("dpad", include_bytes!(concat!("../assets/glyphes/", $dossier, "/dpad.png"))),
            ("l1", include_bytes!(concat!("../assets/glyphes/", $dossier, "/l1.png"))),
            ("r1", include_bytes!(concat!("../assets/glyphes/", $dossier, "/r1.png"))),
            ("l2", include_bytes!(concat!("../assets/glyphes/", $dossier, "/l2.png"))),
            ("r2", include_bytes!(concat!("../assets/glyphes/", $dossier, "/r2.png"))),
            ("start", include_bytes!(concat!("../assets/glyphes/", $dossier, "/start.png"))),
            ("select", include_bytes!(concat!("../assets/glyphes/", $dossier, "/select.png"))),
        ]
    };
}

const GLYPHES_PS: [(&str, &[u8]); 11] = jeu_glyphes!("ps");
const GLYPHES_XBOX: [(&str, &[u8]); 11] = jeu_glyphes!("xbox");

/// Décode les glyphes d'une famille. Un PNG illisible est simplement absent :
/// l'aide retombe sur son libellé texte, jamais sur un trou.
fn charger_glyphes(famille: bool) -> Vec<(&'static str, Icone)> {
    let jeu: &[(&'static str, &'static [u8])] = if famille {
        &GLYPHES_PS
    } else {
        &GLYPHES_XBOX
    };
    jeu.iter()
        .filter_map(|(nom, donnees)| lire_png_depuis(donnees).map(|ic| (*nom, ic)))
        .collect()
}

/// Nom d'icône déclaré par le `.desktop` (Konsole -> utilities-terminal) :
/// la plupart des applis KDE ne portent pas un icône du même nom que leur
/// desktopFileName.
fn icone_du_desktop(nom: &str) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let nom = nom.trim_end_matches(".desktop");
    let racines = [
        format!("{home}/.local/share/applications"),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        format!("{home}/.local/share/flatpak/exports/share/applications"),
        "/usr/share/applications".to_string(),
        "/usr/local/share/applications".to_string(),
    ];
    for r in &racines {
        let p = std::path::PathBuf::from(format!("{r}/{nom}.desktop"));
        let Ok(txt) = std::fs::read_to_string(&p) else {
            continue;
        };
        for ligne in txt.lines() {
            if let Some(v) = ligne.strip_prefix("Icon=") {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Icône d'une fenêtre : fichier du thème, SVG rastérisé une fois en cache.
fn charger_icone(desktop: &str, class: &str, legende: &str) -> Option<Icone> {
    let mut noms: Vec<String> = Vec::new();
    for n in [desktop, class] {
        if !n.is_empty() {
            let n = n.trim_end_matches(".desktop");
            noms.push(n.to_string());
            noms.push(n.to_lowercase());
        }
    }
    // repli : premier mot de la légende (jeux Wine, émulateurs)
    if let Some(mot) = legende.split([' ', ':', '-']).next() {
        if mot.len() > 2 {
            noms.push(mot.to_lowercase());
        }
    }
    // le nom déclaré par le .desktop passe en premier quand il existe
    let mut tous: Vec<String> = Vec::new();
    for n in &noms {
        if let Some(ic) = icone_du_desktop(n) {
            if !tous.contains(&ic) {
                tous.push(ic);
            }
        }
    }
    tous.extend(noms);

    for nom in tous {
        let Some(src) = chercher_icone(&nom) else {
            continue;
        };
        // PNG lisible tel quel, sinon rastérisation : se fier à l'extension
        // rate les SVG sans extension (icône ES-DE)
        if let Some(ic) = lire_png(&src) {
            return Some(ic);
        }
        {
            let cache = std::env::var("HOME").ok().map(|h| {
                std::path::PathBuf::from(h).join(".cache/opencode/icons")
            })?;
            let _ = std::fs::create_dir_all(&cache);
            let nom_sur = nom.replace('/', "_");
            let dest = cache.join(format!("{nom_sur}.png"));
            if !dest.is_file()
                && std::process::Command::new("/usr/bin/magick")
                    .args([
                        "-background",
                        "none",
                        &src.to_string_lossy(),
                        "-resize",
                        "64x64",
                        &dest.to_string_lossy(),
                    ])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
                    != true
            {
                continue;
            }
            if let Some(ic) = lire_png(&dest) {
                return Some(ic);
            }
        }
    }
    None
}

/// Chemin d'un script KWin livré à côté du binaire (`kwin/`).
fn chemin_kwin(nom: &str) -> Option<String> {
    if let Ok(dir) = std::env::var("KWIN_SCRIPTS") {
        return Some(format!("{dir}/{nom}"));
    }
    // le binaire vit dans gpad/target/release : on remonte jusqu'au
    // premier ancêtre qui contient kwin/<script> (robuste au profil de build)
    let exe = std::env::current_exe().ok()?;
    for ancetre in exe.ancestors().skip(1).take(5) {
        let candidat = ancetre.join("kwin").join(nom);
        if candidat.is_file() {
            return Some(candidat.to_string_lossy().to_string());
        }
    }
    None
}

/// Copie d'un script KWin où `__BUS__` devient le nom de service de cette
/// instance (`org.gablue.Win.p<pid>`). Deux overlays peuvent ainsi tourner en
/// même temps sans se voler leurs scripts ni leur service. L'interface D-Bus
/// n'est PAS touchée : elle reste `org.gablue.Win` pour toutes les instances.
fn script_instance(nom: &str, bus: &str) -> Option<String> {
    let src = chemin_kwin(nom)?;
    let txt = std::fs::read_to_string(&src).ok()?;
    let dest = format!(
        "/tmp/opencode/{}-{}.js",
        nom.trim_end_matches(".js"),
        std::process::id()
    );
    // Le dossier est en tmpfs, donc absent après chaque démarrage : sans le
    // créer, l'écriture échouait **en silence** et le service D-Bus restait
    // sans le moindre `Info` (aucune fenêtre connue, et le panneau tombait
    // dans la grâce de 600 ms en montrant ACTIONS avant APPLICATIONS —
    // vécu gen1 20/11/2026, `/tmp/opencode` né seulement au premier
    // `lister.js`).
    if let Err(e) = std::fs::create_dir_all("/tmp/opencode") {
        eprintln!("scripts KWin: /tmp/opencode inaccessible: {e}");
        return None;
    }
    if let Err(e) = std::fs::write(&dest, txt.replace("__BUS__", bus)) {
        eprintln!("scripts KWin: ecriture {dest}: {e}");
        return None;
    }
    Some(dest)
}

/// Copies jetables de scripts KWin (`/tmp/opencode/<nom>-<pid>.js`). Un overlay
/// tué sans passer par sa sortie propre (SIGKILL, banc) en laisse derrière lui :
/// on enlève au démarrage celles dont le **pid n'existe plus**. Jamais celles
/// d'une instance vivante — deux overlays coexistent (gpad-trigger + banc).
fn nettoyer_scripts_temp() {
    let Ok(dir) = std::fs::read_dir("/tmp/opencode") else {
        return;
    };
    for e in dir.flatten() {
        let nom = e.file_name();
        let Some(nom) = nom.to_str() else { continue };
        if !nom.ends_with(".js") {
            continue;
        }
        if !(nom.starts_with("wininfo-")
            || nom.starts_with("lister-")
            || nom.starts_with("kwin-activer-"))
        {
            continue;
        }
        let Some(pid) = nom
            .trim_end_matches(".js")
            .rsplit('-')
            .next()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Délai maximum d'une commande au démon de gate.
const GATE_TIMEOUT: Duration = Duration::from_millis(500);

/// Envoie une commande au démon gpad-gate. Une connexion par commande : le démon
/// traite un message par connexion, et ça survit à un redémarrage du démon.
/// Échec silencieux (socket absente) : l'overlay doit rester utilisable seul.
///
/// Le `connect` est borné à `GATE_TIMEOUT` : sur une file d'attente saturée
/// (démon bloqué par un client muet), `UnixStream::connect` attend
/// indéfiniment. L'overlay restait alors figé avec le grab evdev en main, la
/// surface retirée, et Home ne répondait plus du tout (vécu 18/11/2026) : une
/// commande perdue vaut mieux qu'un panneau qui ne se ferme jamais.
fn gate_cmd(sock: &str, msg: &str) -> bool {
    use std::os::unix::net::UnixStream;
    let (tx, rx) = std::sync::mpsc::channel();
    let (sock, ligne) = (sock.to_string(), format!("{msg}\n"));
    std::thread::spawn(move || {
        let ok = match UnixStream::connect(&sock) {
            Ok(mut s) => s.write_all(ligne.as_bytes()).is_ok(),
            Err(_) => false,
        };
        let _ = tx.send(ok);
    });
    rx.recv_timeout(GATE_TIMEOUT).unwrap_or(false)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Banc de rendu du clavier : aucun Wayland, aucune fenêtre, aucun grab —
    // mesure le coût d'une image (c'est lui qui fait saccader le déplacement).
    if args.iter().any(|a| a == "--bench-clavier") {
        let (layout, variant) = clavier::layout_session();
        let rangees = clavier::charger(&layout, &variant).expect("keymap");
        let c = clavier::Clavier::neuf(rangees, &layout, &variant, (2560, 1440));
        let police = charger_police();
        let (pw, ph) = c.taille();
        let (w, h) = (pw as usize, ph as usize);
        let mut px = vec![0u32; w * h];
        let n = 40;
        let t = Instant::now();
        for _ in 0..n {
            let mut cv = Canvas {
                px: &mut px,
                w,
                h,
                police: police.as_ref(),
                ox: 0,
                clip: None,
            };
            cv.px.fill(0);
            c.dessiner(&mut cv);
        }
        let dessin = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let t = Instant::now();
        for _ in 0..n {
            px.fill(0);
        }
        let vide = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!(
            "BENCH panneau {:.2} ms | surface videe {:.2} ms | {pw}x{ph} dans {w}x{h}",
            dessin, vide
        );
        return;
    }

    let auto_close: Option<Duration> = arg_value(&args, "--seconds")
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs);

    let conn = Connection::connect_to_env().expect("connexion wayland");
    let (globals, mut queue) = registry_queue_init::<App>(&conn).expect("registry");
    let qh: QueueHandle<App> = queue.handle();

    let mut app = App::new(qh.clone());
    // deux overlays peuvent tourner (gpad-trigger + banc) : nom de service et
    // scripts KWin propres à ce processus
    app.bus = format!("org.gablue.Win.p{}", std::process::id());
    app.inst = std::process::id().to_string();

    // Mode résident : la surface est mappée une seule fois (transparente, hors
    // écran) et le déclencheur ouvre/ferme par `--ipc`. Plus d'`exec` ni
    // d'animation KWin à chaque Home (vécu 19/11/2026 : ~470 ms perçus).
    // Le socket n'est créé qu'au premier `configure` (`demarrer_ipc`) : son
    // existence signifie donc « panneau prêt », et un `open` ne peut pas
    // arriver sur une surface qui n'a pas encore de taille.
    app.resident = args.iter().any(|a| a == "--resident");
    app.visible = !app.resident;
    if app.resident {
        match arg_value(&args, "--ipc") {
            Some(chemin) => app.ipc_attente = Some(chemin),
            None => eprintln!("--resident sans --ipc : aucune commande possible"),
        }
    }

    // style : --style > PAD_STYLE (banc) > fichier (choix fait dans REGLAGES)
    // > **MODERNE** (défaut). Un nom inconnu retombe sur RETRO, jamais d'échec.
    let style_nom = arg_value(&args, "--style")
        .or_else(|| std::env::var("PAD_STYLE").ok())
        .or_else(lire_style)
        .unwrap_or_else(|| "moderne".to_string());
    app.style_idx = style_normalise(&style_nom);
    app.th = theme_pour(app.style_idx);

    let compositor: wl_compositor::WlCompositor =
        globals.bind(&qh, 4..=6, ()).expect("wl_compositor");
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals
        .bind(&qh, 1..=4, ())
        .expect("zwlr_layer_shell_v1");
    app.shm = Some(shm);
    app.compositor = Some(compositor.clone());
    app.layer_shell = Some(layer_shell.clone());

    let surface = compositor.create_surface(&qh, ());
    let layer = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Overlay,
        "gpad".to_string(),
        &qh,
        (),
    );
    // déchets des instances tuées brutalement (mesure : 40+ copies)
    nettoyer_scripts_temp();

    let fullscreen = args.iter().any(|a| a == "--fullscreen");
    // Home maintenu : menu système, petit cadre au centre de l'écran
    app.console = args.iter().any(|a| a == "--console");
    if let Some(v) = std::env::var("PAD_FONDU_MS").ok().and_then(|v| v.parse().ok()) {
        app.fondu_ms = v; // banc : ralentir le fondu pour le mesurer
    }
    if let Some(v) = std::env::var("PAD_PANNEAU_DELAI_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        app.panneau_delai_ms = v; // banc : régler l'attente de l'animation KWin
    }
    app.side = !fullscreen && !app.console;
    app.panel_w = arg_value(&args, "--width")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(480);
    if let Some(v) = arg_value(&args, "--idle").and_then(|v| v.parse::<u64>().ok()) {
        app.inactivite = Duration::from_secs(v);
    }
    app.slide_ms = arg_value(&args, "--slide-ms")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0); // 0 = valeurs par défaut (180 entrée / 140 sortie)

    if fullscreen || app.console {
        layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer.set_size(0, 0); // 0 = toute la surface disponible
    } else {
        // panneau latéral droit, pleine hauteur, largeur fixe
        layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer.set_size(app.panel_w as u32, 0);
    }
    // banc : position du panneau par marge (test de faisabilité seulement)
    if let Some(m) = arg_value(&args, "--test-margin").and_then(|v| v.parse::<i32>().ok()) {
        layer.set_margin(0, m, 0, 0);
    }
    layer.set_exclusive_zone(-1); // ne réserve aucune place aux fenêtres
    layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    // région d'entrée vide => la souris traverse l'overlay
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    surface.commit();
    app.wl_surface = Some(surface);
    app.layer = Some(layer);

    // La police ne sert qu'au premier dessin (après le `configure`) : la
    // charger ici laisse KWin préparer la surface en parallèle — ~15 ms
    // mesurés de gagnés avant que le panneau puisse paraître.
    app.police = if app.style_idx == 1 { charger_police() } else { None };
    println!(
        "STYLE {} ({}) police={}",
        STYLE_OPTS[app.style_idx],
        STYLE_KEYS[app.style_idx],
        if app.police.is_some() { "ttf" } else { "bitmap" }
    );

    if let Ok(seat) = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=9, ()) {
        let _kb = seat.get_keyboard(&qh, ());
    }

    // résident : aucune manette au démarrage, le node arrive avec `open`.
    // `--clavier` non plus : il grabbe SON node, pas la première manette venue
    // (sinon un banc volerait la manette de l'utilisateur).
    let pad = if app.resident
        || args.iter().any(|a| a == "--no-grab" || a == "--clavier")
    {
        None
    } else {
        arg_value(&args, "--grab").or_else(find_pad_event)
    };
    if let Some(p) = pad {
        app.detecter_manette(&p);
        app.grab_pad(&p);
    } else if !app.resident && args.iter().all(|a| a != "--clavier") {
        eprintln!("aucune manette trouvée (aucun grab)");
    }

    // Clavier virtuel direct : `--clavier` seul prend la manette détectée,
    // `--clavier /dev/input/eventN` impose le node (banc, ou plusieurs
    // manettes). Un seul usage par processus (il quitte à la fermeture).
    if let Some(node) = arg_optionnel(&args, "--clavier") {
        let node = node
            .or_else(find_pad_event)
            .unwrap_or_else(|| {
                eprintln!("clavier : aucune manette (donner un node : --clavier /dev/input/eventN)");
                std::process::exit(1);
            });
        app.visible = false;
        app.ouvrir_clavier(&node);
        if !app.kbd_ouvert {
            eprintln!("clavier : ouverture impossible sur {node}, sortie");
            std::process::exit(1);
        }
    }

    // Gate BPF : on déclare les nodes à couper, puis on l'ouvrira à l'affichage.
    // banc d'essai : vérifier la résolution d'icône sans fenêtre
    if let Ok(nom) = std::env::var("PAD_ICONE_TEST") {
        let ic = charger_icone(&nom, "", "");
        println!("ICONE test {nom} -> {}", if ic.is_some() { "trouvee" } else { "aucune" });
    }

    // Fenêtre active : service D-Bus (le script KWin pousse dedans) + script.
    // Sans KWin joignable, l'overlay reste utilisable (en-tête vide).
    if !app.console
        && !args.iter().any(|a| a == "--no-kwin")
        && std::env::var("PAD_NO_KWIN").is_err()
    {
        if let Some(kbus) = KBus::nouveau() {
            app.win_etat = demarrer_wininfo(&app.bus);
            if app.win_etat.is_none() {
                eprintln!("wininfo: service indisponible (nom déjà pris ?)");
            }
            if let Some(chemin) = script_instance("wininfo.js", &app.bus) {
                // Le tout premier `loadScript` au démarrage n'exécute pas
                // toujours le script (observé : id rendu, `isScriptLoaded`
                // vrai, aucun `Info`). On vérifie donc qu'un push arrive et on
                // réessaie sous un nom neuf : la preuve est l'état, pas le
                // code de retour de D-Bus.
                for essai in 1..=3 {
                    let plugin = format!("gablue-wininfo-{}-{essai}", app.inst);
                    app.plugins.push(plugin.clone());
                    let ok = kbus.demarrer_wininfo(&chemin, &plugin);
                    println!("KWIN info={} (essai {essai})", if ok { "active" } else { "refusee" });
                    if !ok {
                        continue;
                    }
                    let t0 = Instant::now();
                    let mut recu = false;
                    while t0.elapsed() < Duration::from_millis(250) {
                        if let Some(st) = &app.win_etat {
                            if let Ok(p) = st.lock() {
                                recu = p.recu;
                            }
                        }
                        if recu {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    if recu {
                        break;
                    }
                    eprintln!("kwin: script d'info muet, nouvelle tentative");
                    let _ = kbus.decharger(&plugin);
                }
            }
            app.kbus = Some(kbus);
        } else {
            eprintln!("kwin: session D-Bus injoignable");
        }
    }

    if let Some(gs) = arg_value(&args, "--gate") {
        if app.resident {
            // les cibles arrivent avec chaque `open` (les dev_t changent à
            // chaque reconnexion) : rien à poser maintenant
            println!("GATE socket={gs} (resident)");
            app.gate_sock = Some(gs);
        } else {
            let targets = arg_values(&args, "--gate-target");
            // liste repartie de zéro : le démon peut tourner depuis le boot, et
            // les dev_t changent à chaque reconnexion (sinon les emplacements de
            // cibles s'épuisent au fil des sessions).
            gate_cmd(&gs, "clear");
            let mut ok = 0;
            for t in &targets {
                if gate_cmd(&gs, &format!("target {t}")) {
                    ok += 1;
                } else {
                    eprintln!("gate: cible ignorée {t}");
                }
            }
            println!("GATE socket={gs} cibles={ok}/{}", targets.len());
            app.gate_sock = Some(gs);
            app.gate_nodes = ok;
            app.dirty = true;
        }
    }

    // SIGTERM/SIGINT (gpad-trigger, Ctrl-C, session) : on ferme proprement, donc
    // on prévient le gate. Sans ça, une sortie brutale laisse les manettes coupées.
    unsafe {
        let h: extern "C" fn(libc::c_int) = sur_signal;
        libc::signal(libc::SIGTERM, h as usize);
        libc::signal(libc::SIGINT, h as usize);
    }

    app.applis = vec![
        // l'en-tête de fenêtre est présent sur cette page aussi
        Entree::Fenetre,
        Entree::Appli("GLAUNCHER"),
        Entree::Appli("EMULATIONSTATION"),
        Entree::Appli("YOUTUBE"),
        Entree::Appli("GABLUE TV"),
    ];
    if jellyfin_installe() {
        app.applis.push(Entree::Appli("JELLYFIN"));
    }
    println!(
        "APPLIS {}",
        app.applis.iter().filter(|e| matches!(e, Entree::Appli(_))).count()
    );

    // icônes des applis : préchargées (rastériser pendant le dessin fait sauter
    // le panneau), une seule fois par desktop
    let applis: Vec<Entree> = app.applis.clone();
    for e in applis {
        if let Entree::Appli(nom) = e {
            let d = desktop_appli(nom);
            let cle = format!("{d}|");
            if !app.icones.contains_key(&cle) {
                let ic = charger_icone(d, "", nom);
                println!(
                    "ICONE appli {nom} -> {}",
                    if ic.is_some() { "trouvee" } else { "aucune" }
                );
                app.icones.insert(cle, ic);
            }
        }
    }

    app.refresh_mangohud();
    app.refresh_volume();
    app.refresh_vrr();
    app.refresh_booster();
    // `--focus` (Home+bas) impose sa ligne : ne pas la déplacer ensuite
    let mut focus_force = false;
    if let Some(f) = arg_value(&args, "--focus") {
        // combo Home+haut/bas : le panneau s'ouvre dans la bonne CATÉGORIE,
        // avec la ligne déjà choisie (les réglages ont quitté la page
        // « RÉGLAGES » à plat pour les sous-menus)
        let cible = match f.as_str() {
            "fps" => Some((Cat::Perf, Entree::Fps)),
            "mango" => Some((Cat::Perf, Entree::Mango)),
            "volume" => Some((Cat::Son, Entree::Volume)),
            // section CAPTURE (enregistreur / replay) : banc visuel
            "capture" => Some((Cat::Capture, Entree::Qualite)),
            // sélecteur jeu ⇄ app compagne (banc : GPAD_BASCULE_DEMO=1)
            "bascule" => Some((Cat::Fenetres, Entree::Basculer)),
            // dernière ligne de la catégorie la plus longue : vérifie le bas
            // de liste (défilement)
            "fin" => Some((Cat::Capture, Entree::Curseur)),
            _ => None,
        };
        if f == "accueil" {
            // menu des catégories : la page d'ouverture est imposée, sinon la
            // boucle bascule sur APPLICATIONS faute de fenêtre active
            app.page = Page::Accueil;
            app.selected = 0;
            app.status = "MENU: CATEGORIES".to_string();
            println!("FOCUS accueil");
            app.dirty = true;
            focus_force = true;
        } else if let Some((cat, c)) = cible {
            app.entrer_categorie(cat);
            if let Some(i) = app.liste().iter().position(|e| *e == c) {
                app.selected = i;
            }
            app.refresh_mangohud();
            app.refresh_volume();
            println!("FOCUS {f}");
            app.status = format!("{}: GAUCHE/DROITE", c.nom());
            app.dirty = true;
            focus_force = true;
        }
    }

    app.off = if app.side { app.panel_w } else { 0 };
    // La page d'ouverture se décide dans la boucle : l'info de fenêtre arrive
    // par D-Bus APRÈS le démarrage (constaté : `WIN` 100 ms après le lancement),
    // décider ici ouvrait `APPLICATIONS` alors qu'une fenêtre était active.
    app.focus_impose = focus_force;
    app.depart = Instant::now();
    // le bloc d'info de fenêtre vient de tenter son chargement : une minute
    // avant de réessayer depuis la boucle (résident caché)
    app.kwin_retry_at = Some(Instant::now() + Duration::from_secs(60));

    if app.console {
        // menu système (`--console`, touche Home) : la veille d'abord — c'est
        // la seule action réversible — puis les sorties. Les jeux sont fermés
        // avec « tout fermer » avant toute action système.
        let mut v = vec![
            Entree::Action("MISE EN VEILLE"),
            Entree::Action("MODE BUREAU"),
            Entree::Action("TOUT FERMER"),
        ];
        if app.pad_bt.is_some() {
            v.push(Entree::Action("DECONNECTER LA MANETTE"));
        }
        v.push(Entree::Action("FERMER LA SESSION"));
        v.push(Entree::Action("REDEMARRER"));
        v.push(Entree::Action("ETEINDRE"));
        app.accueil = v;
        app.selected = 0;
        app.dirty = true;
        println!("CONSOLE items={}", app.accueil.len());
    }

    println!("GPAD_UP");
    let _ = std::io::stdout().flush();

    // le panneau est affiché : on coupe hidraw/js des manettes
    if let Some(gs) = app.gate_sock.clone() {
        if gate_cmd(&gs, "open") {
            app.gate_on = true;
            app.dirty = true;
            println!("GATE open");
        } else {
            eprintln!("gate: open refusé (démon absent ?)");
        }
    }

    let start = Instant::now();
    while app.running {
        if ARRET_DEMANDE.load(std::sync::atomic::Ordering::Relaxed) {
            println!("SIGNAL -> fermeture");
            ARRET_DEMANDE.store(false, std::sync::atomic::Ordering::Relaxed);
            // SIGTERM = quitter, même en résident (sinon le service systemd
            // attendrait son TimeoutStopSec puis SIGKILL). Caché : sortie
            // immédiate ; visible : glissement de sortie puis sortie.
            app.quitter = true;
            if app.visible {
                app.request_close();
            } else {
                app.running = false;
            }
        }
        if let Some(d) = auto_close {
            if start.elapsed() > d {
                println!("CLOSE (timeout)");
                app.request_close();
                if !app.side {
                    break;
                }
                // laisse le glissement de sortie se terminer (et REDESSINE :
                // le décalage est appliqué au dessin, pas à la surface)
                while app.anim.is_some() {
                    app.tick_anim();
                    if app.dirty {
                        app.draw();
                        let _ = conn.flush();
                    }
                    std::thread::sleep(Duration::from_millis(8));
                }
                app.running = false;
                break;
            }
        }

        // traite ce qui est déjà arrivé, puis vide la file sortante
        if let Err(e) = queue.dispatch_pending(&mut app) {
            eprintln!("dispatch: {e}");
            break;
        }
        let _ = conn.flush();

        // multiplexage : fd manette + fd wayland + socket du déclencheur,
        // jamais de roundtrip bloquant
        let guard = conn.prepare_read();
        let pad_fd = app.pad.as_ref().map(|f| f.as_raw_fd());
        let ipc_fd = app.ipc.as_ref().map(|l| l.as_raw_fd());
        let mut fds = [
            libc::pollfd {
                fd: pad_fd.unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: guard.as_ref().map(|g| g.connection_fd().as_raw_fd()).unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: ipc_fd.unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // timeout court pendant l'animation : ~120 Hz pour un glissement fluide.
        // Résident caché : rien à relire, on dort presque — mais 1 s, pas -1 :
        // `signal()` installe SA_RESTART, donc `poll(-1)` serait REPRIS après
        // SIGTERM et le processus ne sortirait jamais (mesuré).
        let timeout_ms = if app.kbd_ouvert {
            // répétition ou déplacement en cours : 8 ms (le mouvement est
            // calculé par tour, pas à l'arrivée des évènements) ; sinon 100 ms
            if app.clavier_anime() || app.clavier_bouge() { 8 } else { 100 }
        } else if app.resident && !app.visible {
            1000
        } else if app.anim.is_some()
            || app.pdep.is_some()
            || app.fondu.is_some()
            || app.entree.is_some()
        {
            8
        } else {
            100
        };
        unsafe { libc::poll(fds.as_mut_ptr(), 3, timeout_ms) };

        if fds[0].revents & libc::POLLIN != 0 {
            app.pump_pad();
        }
        if fds[2].revents & libc::POLLIN != 0 {
            app.lire_ipc();
        }
        // maintien de Home : vérifié à chaque tour (poll 100 ms au repos)
        app.surveiller_home();
        app.surveiller_inactivite();
        app.tick_clavier();
        if let Some(g) = guard {
            if fds[1].revents != 0 {
                let _ = g.read();
            }
        }
        // Traiter TOUT DE SUITE les événements lus : le `dispatch_pending` de
        // tête de boucle passe après le `poll` suivant, donc le tout premier
        // `configure` n'était vu qu'au tour d'après — 100 ms de `poll` au
        // repos entre les deux, mesurées au banc (configure à 57 ms, entrée
        // traitée à 160 ms). Le panneau les voit maintenant à ~60 ms.
        if let Err(e) = queue.dispatch_pending(&mut app) {
            eprintln!("dispatch (frais): {e}");
            break;
        }
        // glissement / fondu d'entrée, une seule fois, dès que la surface est
        // configurée — et AVANT la première image, sinon le fondu serait déjà
        // terminé quand le menu paraît (wayland + script KWin + icônes prennent
        // bien plus que 160 ms : constaté, « ça ne fait pas la fondue »)
        if app.configured && !app.slid_in {
            app.slid_in = true;
            if app.resident || app.kbd_ouvert {
                // Première (et unique) mise à l'écran : transparente et hors
                // écran. KWin anime cette ouverture une fois, invisible ; les
                // `open` suivants ne remappent rien et n'attendent donc plus
                // son animation. Le clavier a sa propre surface : le panneau
                // ne doit jamais apparaître au démarrage.
                app.opacite = 0.0;
                app.off = app.panel_w;
                app.dirty = true;
                if app.resident {
                    app.demarrer_ipc();
                    println!(
                        "RESIDENT pret ({})",
                        if app.ipc.is_some() { "ipc" } else { "sans ipc" }
                    );
                }
            } else if app.side {
                // KWin anime l'ouverture de cette surface comme celle d'une
                // fenêtre normale (effet Scale, `animationTime(200)`). Deux
                // mouvements simultanés se CUMULENT : avec notre glissement
                // par-dessus, le panneau arrivait à sa place en ~100 ms et
                // semblait « se poser sur place » (constaté 18/11/2026) ; sans
                // notre glissement, c'est l'animation de KWin qu'on voyait
                // (« ça zoome »). On reste donc transparent le temps de son
                // animation, PUIS on fait la nôtre — même remède que la
                // fermeture du menu système (faire animer une surface
                // invisible), mais l'entrée visible est bien un glissement.
                app.opacite = 0.0;
                app.entree =
                    Some(Instant::now() + Duration::from_millis(app.panneau_delai_ms));
                println!(
                    "SLIDE IN (attente {} ms puis glissement 200 ms)",
                    app.panneau_delai_ms
                );
            } else {
                app.opacite = 0.0;
                app.fondu = Some(Fondu {
                    start: Instant::now(),
                    // entrée un peu plus lente que la sortie : à 160 ms la
                    // montée ne durait que ~100 ms à l'écran, donc invisible
                    // (mesuré à l'enregistrement 60 fps)
                    dur: Duration::from_millis(app.fondu_ms + 380),
                    de: 0.0,
                    vers: 1.0,
                });
                println!("FONDU IN");
            }
        }
        if let Some(t) = app.entree {
            if Instant::now() >= t {
                app.entree = None;
                // Opaque d'un coup : à cet instant le contenu est encore
                // entièrement HORS écran (off = panel_w), donc rien ne
                // clignote — et le glissement qui suit est vu en entier. Un
                // fondu ici mangeait le mouvement : les deux easings étant
                // « ease-out », le panneau était à 87 % de sa course à 90 ms
                // alors qu'il n'était qu'à moitié opaque (« il apparaît sur
                // place », constaté 18/11/2026).
                app.opacite = 1.0;
                app.fondu = None;
                app.slide(app.panel_w, 0, 200);
            }
        }
        if app.anim.is_some() {
            app.tick_anim();
        }
        app.tick_page();
        app.tick_fondu();
        if app.closing && app.anim.is_none() && app.fondu.is_none() {
            if app.resident && !app.quitter {
                // on ne quitte pas : surface hors écran et transparente,
                // manette rendue, à l'écoute de la prochaine commande
                app.cacher();
            } else {
                app.running = false;
            }
        }

        // fenêtre active : relue à chaque tour (poll 100 ms) pour que le nom
        // reste juste même si le panneau est ouvert longtemps — inutile tant
        // que le panneau résident est caché
        let mut fenetre_changee = false;
        if app.visible {
            if let Some(st) = &app.win_etat {
                if let Ok(p) = st.lock() {
                    if p.info != app.win {
                        app.win = p.info.clone();
                        app.dirty = true;
                        fenetre_changee = true;
                        println!(
                            "WIN pid={} fs={} uuid={} {} | {}",
                            app.win.pid,
                            app.win.fullscreen,
                            app.win.uuid,
                            app.win.caption,
                            app.win.class
                        );
                    }
                }
            }
        }
        // jeu lancé par glauncher : logo, nom et icône suivent la fenêtre active
        if fenetre_changee {
            app.rafraichir_jeu();
        }
        // Résident caché sans le moindre `Info` reçu : on retente le script,
        // une fois par minute. KWin redémarré ou premier chargement raté
        // laissait sinon le panneau aveugle (en-tête vide, page d'ouverture
        // APPLICATIONS) jusqu'à la fin de la session. Caché, ça ne se voit pas.
        if app.resident && !app.visible && !app.info_fraiche() {
            let due = app.kwin_retry_at.map_or(true, |t| Instant::now() >= t);
            if due {
                app.kwin_retry_at = Some(Instant::now() + Duration::from_secs(60));
                app.relancer_wininfo();
            }
        }
        // Page d'ouverture, décidée **une fois** que l'info de fenêtre est
        // arrivée (D-Bus, donc après le démarrage) ou après une grâce de
        // 600 ms : fenêtre active -> ACTIONS avec le curseur sur son nom ;
        // aucune fenêtre -> APPLICATIONS, la page ACTIONS n'aurait aucune cible.
        // En résident, pas de grâce : le script pousse depuis le démarrage de
        // la session, attendre ne servirait qu'à montrer la page ACTIONS par
        // défaut (le panneau est déjà visible) avant de basculer.
        if !app.page_initiale && app.configured {
            let decide = app.recu_info()
                || app.resident
                || app.depart.elapsed() >= Duration::from_millis(600);
            if decide {
                app.page_initiale = true;
                if !app.console && !app.focus_impose {
                    if !app.a_fenetre() {
                        app.page = Page::Applis;
                        app.selected = app.premier_action();
                        app.status = "MENU: APPLICATIONS".to_string();
                        println!("PAGE APPLIS (aucune fenetre)");
                    } else {
                        app.selected = app.index_entete().unwrap_or(0);
                    }
                }
                app.dirty = true;
            }
        }

        if app.visible && app.last_scan.elapsed() > Duration::from_millis(1000) {
            app.refresh_recorder();
            // ces états changent hors du panneau : sans `dirty`, l'affichage
            // garde l'ancienne valeur (le mode replay restait « ARRET »
            // alors qu'il tournait)
            let souris = processus_actif(&nom_binaire("MOUSE", "gpad-mouse"));
            if souris != app.mouse_active {
                app.mouse_active = souris;
                app.dirty = true;
            }
            let replay = capture::etat().is_some_and(|e| e.replay);
            // le script démarre/arrête en quelques dixièmes de seconde (bip,
            // flatpak, SIGINT) : tant que l'état demandé n'est pas arrivé, la
            // passe ne le contredit pas, sinon la ligne affichait « ARRET »
            // une demi-seconde avant de repasser « ACTIF »
            let attente = app
                .replay_attente
                .is_some_and(|limite| Instant::now() < limite);
            if replay != app.replay_active {
                if !attente {
                    app.replay_active = replay;
                    app.maj_accueil();
                    app.replay_attente = None;
                    app.dirty = true;
                }
            } else {
                app.replay_attente = None;
            }
            // le visuel du jeu peut arriver dans le cache après le lancement :
            // relu ici, une fois par seconde, comme les autres états hors pad
            app.rafraichir_jeu();
            app.last_scan = Instant::now();
        }
        if app.dirty {
            app.draw();
            let _ = conn.flush();
        }
        if app.kbd_bouge {
            let _ = conn.flush();
            app.kbd_bouge = false;
        }
        // clavier virtuel : surface et tampons distincts du panneau
        if app.kbd_dirty {
            app.draw_clavier();
            let _ = conn.flush();
        }
        // Réglages relus APRÈS la première image : `pactl` et les scripts
        // prennent quelques dizaines de ms, qu'on ne veut pas payer avant que
        // le panneau paraisse. Le redessin qui suit est invisible (même
        // contenu, valeurs à jour).
        if app.a_rafraichir && app.visible {
            app.a_rafraichir = false;
            app.refresh_mangohud();
            app.refresh_volume();
            app.refresh_vrr();
            app.refresh_booster();
            // jeu et application compagne ouverts : la bascule apparaît en tête
            // de l'accueil. Relu après la première image, comme le reste
            // (`lister.js` prend jusqu'à 400 ms).
            app.refresh_bascule();
            app.maj_accueil();
            app.dirty = true;
        }
    }

    app.release_pad();
    // le script KWin part avec le panneau (sinon son QTimer continuerait)
    if let Some(k) = &app.kbus {
        for nom in &app.plugins {
            k.decharger(nom);
        }
    }
    // le panneau est fermé : on rend les manettes
    if let Some(gs) = app.gate_sock.clone() {
        gate_cmd(&gs, "close");
        println!("GATE closed");
    }
    // socket du déclencheur : on ne laisse pas de fichier mort dans
    // $XDG_RUNTIME_DIR (chaque redémarrage du service en créerait un)
    if let Some(p) = app.ipc_path.take() {
        let _ = std::fs::remove_file(&p);
    }
    for b in app.buffers.drain(..) {
        unsafe { libc::munmap(b.ptr as *mut libc::c_void, b.len) };
        b.buffer.destroy();
    }
    if let Some(l) = app.layer.take() {
        l.destroy();
    }
    if let Some(s) = app.wl_surface.take() {
        s.destroy();
    }
    let _ = conn.flush();

    // Combinaison demandée depuis le panneau : on agit SEULEMENT maintenant,
    // surface détruite — sinon le menu se filmerait dans l'enregistrement.
    // (En résident, c'est `cacher()` qui s'en charge, à la fermeture.)
    app.executer_apres();

    println!("GPAD_DOWN");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier jetable par test (aucune variable d'environnement touchée : les
    /// tests peuvent tourner en parallèle).
    fn bac(nom: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gpad-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// La bascule ne doit pas seulement activer : le plein écran du jeu est
    /// retiré en allant vers l'app, remis en revenant (sinon l'app reste
    /// derrière la fenêtre plein écran, et l'écran semble revenir au jeu).
    #[test]
    fn bascule_pilote_le_plein_ecran_du_jeu() {
        let vers_app = script_bascule("{jeu}", "{app}", false);
        assert!(vers_app.contains("w.fullScreen = false"));
        assert_eq!(vers_app.matches("workspace.activeWindow = w").count(), 1);
        assert!(vers_app.contains("\"{app}\""));

        let vers_jeu = script_bascule("{jeu}", "{app}", true);
        assert!(vers_jeu.contains("w.fullScreen = true"));
        assert!(!vers_jeu.contains("w.fullScreen = false"));
    }

    /// La sourdine du jeu ne touche que ses flux : l'application compagne
    /// (dont le nom peut contenir celui du jeu) et la musique de fond gardent
    /// leur son quand on passe à l'app.
    #[test]
    fn sourdine_limitee_aux_flux_du_jeu() {
        let flux = |index: u32, pid: i32, nom: &str| audio::Flux {
            index,
            pid,
            nom: nom.to_string(),
            pourcent: 80,
            mute: false,
        };
        let liste = vec![
            flux(10, 500, "MarioKart64Recompiled"),
            flux(11, 501, "tvqt"),
            flux(12, 900, "mpv"),
        ];
        let retenus: Vec<u32> = flux_du_jeu(&liste, &[500, 501])
            .iter()
            .map(|f| f.index)
            .collect();
        assert_eq!(retenus, vec![10]);
    }

    fn pid_mort() -> i32 {
        let mut enfant = std::process::Command::new("/bin/true").spawn().unwrap();
        let pid = enfant.id() as i32;
        enfant.wait().unwrap();
        pid
    }

    /// Une fenêtre réelle sans titre (Project Midas) n'est pas « aucune
    /// fenêtre » : l'info KWin n'est vide que quand le script rejette la
    /// fenêtre active. Sans ça, l'overlay annonçait « AUCUNE FENETRE » et
    /// s'ouvrait sur APPLICATIONS pendant une partie de .lgp.
    #[test]
    fn fenetre_sans_titre_n_est_pas_absente() {
        assert!(!fenetre_annoncee("", ""), "script KWin : aucun candidat");
        assert!(fenetre_annoncee("", "ProjectMidas"), "titre vide, classe présente");
        assert!(fenetre_annoncee("Celeste", ""), "titre seul (classe vide)");
        assert!(fenetre_annoncee("Celeste", "steam_proton"));

        let sans_titre = Fenetre {
            caption: String::new(),
            class: "ProjectMidas".to_string(),
            ..Fenetre::default()
        };
        assert_eq!(sans_titre.titre(), "ProjectMidas", "la classe sert de nom");
        let titree = Fenetre {
            caption: "Conker BFD: Reloaded".to_string(),
            class: "ConkerBFDReloaded".to_string(),
            ..Fenetre::default()
        };
        assert_eq!(titree.titre(), "Conker BFD: Reloaded");
    }

    /// Contrat de `running.txt` : le slug, les PID vivants seulement, et le
    /// chemin du logo tel qu'il est publié. Un PID mort (launcher tué en pleine
    /// partie) ne doit jamais désigner un jeu.
    #[test]
    fn running_txt_lu_et_pid_mort_ecarte() {
        let dir = bac("running");
        let fichier = dir.join("running.txt");
        let moi = std::process::id() as i32;
        let mort = pid_mort();

        std::fs::write(
            &fichier,
            format!(
                "slug=celeste\nnom=Celeste\npids={moi},{mort}\nlogo=/tmp/x.png\nicone=/tmp/i.png\n"
            ),
        )
        .unwrap();
        let jeu = lire_jeu(&fichier, &dir).expect("fichier lisible");
        assert_eq!(jeu.slug, "celeste");
        assert_eq!(jeu.nom, "Celeste", "nom affiché du paquet");
        assert_eq!(jeu.paquet, "", "glauncher ne publie pas de paquet");
        assert_eq!(jeu.pids, vec![moi], "un PID mort est écarté");
        assert_eq!(jeu.logo, "/tmp/x.png");
        assert_eq!(jeu.icone, "/tmp/i.png", "icône du paquet, pas de la fenêtre");

        std::fs::write(&fichier, format!("slug=hades\npids={mort}\n")).unwrap();
        assert!(
            lire_jeu(&fichier, &dir).is_none(),
            "aucun PID vivant = pas de jeu, donc pas de logo"
        );

        std::fs::remove_file(&fichier).unwrap();
        assert!(lire_jeu(&fichier, &dir).is_none(), "fichier absent = pas de jeu");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Le lien par environnement : les lanceurs nomment le paquet dans
    /// l'environnement du jeu, seule trace qui survit au reparentage à systemd
    /// et au `setsid` des jeux à launcher interne (Sonic Superstars).
    #[test]
    fn paquet_lu_dans_environ() {
        let brut = b"PATH=/usr/bin\0GWINE_WGP_FILE=/jeux/Sonic Superstars.wgp\0HOME=/home/x\0";
        assert_eq!(
            paquet_dans_environ(brut).as_deref(),
            Some("/jeux/Sonic Superstars.wgp")
        );
        assert_eq!(
            paquet_dans_environ(b"LGPACK_FILE=/jeux/Zelda.lgp\0").as_deref(),
            Some("/jeux/Zelda.lgp"),
            "lgplaunch exporte LGPACK_FILE"
        );
        assert_eq!(
            paquet_dans_environ(b"GWINE_WGP_FILE=\0LGPACK_FILE=\0"),
            None,
            "une valeur vide ne désigne aucun paquet"
        );
        assert_eq!(
            paquet_dans_environ(b"GWINE_WGP_FILEX=/x\0"),
            None,
            "la clé se compare entière, pas par préfixe"
        );
    }

    /// gwine/lgplaunch ne publient que le paquet (`nom=` + `paquet=` + `pids=`) :
    /// le slug en vient, et les visuels sont retrouvés dans le cache — y compris
    /// quand ils arrivent **après** le lancement (`glauncher --art` en fond), la
    /// publication n'étant relue qu'une fois par seconde.
    #[test]
    fn paquet_publie_donne_slug_et_visuels() {
        let dir = bac("paquet");
        let fichier = dir.join("running.txt");
        let moi = std::process::id() as i32;
        std::fs::write(
            &fichier,
            format!(
                "nom=Croc 2 Kingdom of the Gobbos\n\
                 paquet=/home/gab/Windows/Games/Croc 2 Kingdom of the Gobbos.wgp\n\
                 pids={moi}\n"
            ),
        )
        .unwrap();

        let jeu = lire_jeu(&fichier, &dir).expect("fichier lisible");
        assert_eq!(jeu.slug, "croc-2-kingdom-of-the-gobbos", "slug du paquet");
        assert_eq!(jeu.nom, "Croc 2 Kingdom of the Gobbos");
        assert_eq!(
            jeu.paquet,
            "/home/gab/Windows/Games/Croc 2 Kingdom of the Gobbos.wgp",
            "le paquet est gardé pour le lien par environnement"
        );
        assert_eq!(jeu.logo, "", "visuel absent du cache = aucun logo");
        assert_eq!(jeu.icone, "");

        // le téléchargement arrive dans le cache : la relecture le voit
        std::fs::create_dir_all(dir.join("assets/logos")).unwrap();
        std::fs::write(
            dir.join("assets/logos/croc-2-kingdom-of-the-gobbos.png"),
            b"png",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("pack-icons")).unwrap();
        std::fs::write(
            dir.join("pack-icons/croc-2-kingdom-of-the-gobbos.png"),
            b"png",
        )
        .unwrap();
        let jeu = lire_jeu(&fichier, &dir).expect("fichier lisible");
        assert!(
            jeu.logo.ends_with("assets/logos/croc-2-kingdom-of-the-gobbos.png"),
            "logo: {}",
            jeu.logo
        );
        assert!(
            jeu.icone.ends_with("pack-icons/croc-2-kingdom-of-the-gobbos.png"),
            "icône: {}",
            jeu.icone
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Les visuels suivent le **jeu SteamGridDB** (`assets/identities.txt`,
    /// partagé par Syncthing), pas le nom du paquet : deux `.wgp`/`.lgp` du
    /// même jeu dont les noms diffèrent tombent sur les mêmes fichiers. Un
    /// paquet jamais identifié garde son slug.
    #[test]
    fn visuels_nommes_d_apres_le_jeu() {
        let dir = bac("identites");
        std::fs::create_dir_all(dir.join("assets/logos")).unwrap();
        std::fs::create_dir_all(dir.join("assets/icons")).unwrap();
        std::fs::create_dir_all(dir.join("pack-icons")).unwrap();
        // identification apprise par le launcher (id + nom après tabulation)
        std::fs::write(
            dir.join("assets/identities.txt"),
            "# identification SteamGridDB : slug=<id>\t<nom>\ncroc-2-fr=1234\tCroc 2\nsans-nom=5678\n",
        )
        .unwrap();
        std::fs::write(dir.join("assets/logos/1234-croc-2.png"), b"png").unwrap();
        std::fs::write(dir.join("assets/icons/5678.png"), b"png").unwrap();

        assert_eq!(cle_visuels(&dir, "croc-2-fr"), "1234-croc-2");
        assert_eq!(cle_visuels(&dir, "sans-nom"), "5678", "id seul si le nom manque");
        assert_eq!(
            cle_visuels(&dir, "mon-homebrew"),
            "mon-homebrew",
            "jamais identifié : le paquet"
        );

        // deux paquets du même jeu : même clé, donc même logo
        assert_eq!(
            visuel_png(&dir, "logos", &cle_visuels(&dir, "croc-2-fr")),
            dir.join("assets/logos/1234-croc-2.png").display().to_string()
        );
        assert_eq!(
            icone_png(&dir, &cle_visuels(&dir, "sans-nom"), "sans-nom"),
            dir.join("assets/icons/5678.png").display().to_string(),
            "un visuel rangé sous l'id seul est retrouvé"
        );
        // icône extraite du paquet : dernier recours, propre à la machine
        std::fs::write(dir.join("pack-icons/mon-homebrew.png"), b"png").unwrap();
        assert_eq!(
            icone_png(&dir, &cle_visuels(&dir, "mon-homebrew"), "mon-homebrew"),
            dir.join("pack-icons/mon-homebrew.png").display().to_string()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Le nom d'un paquet suit la même règle que le launcher (sinon les visuels
    /// du cache ne seraient jamais retrouvés).
    #[test]
    fn slug_identique_au_launcher() {
        assert_eq!(slugify("Skyward Odyssey"), "skyward-odyssey");
        assert_eq!(slugify("  Conker's BFD!! "), "conker-s-bfd");
        assert_eq!(slugify("Pokémon Écarlate"), "pokémon-écarlate");
        assert_eq!(
            nom_paquet("/home/gab/Windows/Games/Croc 2 Kingdom of the Gobbos.wgp"),
            "Croc 2 Kingdom of the Gobbos"
        );
    }

    /// Le lien par descendance : la fenêtre d'un jeu natif est un enfant du
    /// lanceur publié. Un processus étranger ne doit pas matcher.
    #[test]
    fn fenetre_descendante_du_lanceur() {
        let mut enfant = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let jeu = JeuActif {
            slug: "celeste".to_string(),
            nom: "Celeste".to_string(),
            pids: vec![std::process::id() as i32],
            ..JeuActif::default()
        };
        let trouve = jeu_de(&[jeu.clone()], enfant.id() as i32).expect("descendant publié");
        assert_eq!(trouve.slug, "celeste");

        let inconnu = JeuActif {
            pids: vec![pid_mort()],
            ..JeuActif::default()
        };
        assert!(
            jeu_de(&[inconnu], enfant.id() as i32).is_none(),
            "un PID qui n'est ni publié ni descendant ne désigne aucun jeu"
        );
        let _ = enfant.kill();
        let _ = enfant.wait();
    }

    /// Les deux publications sont lues, et la fenêtre active retrouve **son**
    /// jeu par le PID : la publication dont le PID est mort (launcher tué) est
    /// écartée, et les visuels viennent du paquet.
    #[test]
    fn deux_publications_et_lien_par_pid() {
        let cache = bac("sources-cache");
        let runtime = bac("sources-rt");
        let lanceurs = runtime.join("gablue");
        std::fs::create_dir_all(&lanceurs).unwrap();

        // glauncher : un jeu qui n'est plus là
        std::fs::write(
            cache.join("running.txt"),
            format!("slug=autre\nnom=Autre\npids={}\n", pid_mort()),
        )
        .unwrap();

        // gwine/lgplaunch : le paquet, avec le PID d'un vrai processus fils
        let mut enfant = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        std::fs::write(
            lanceurs.join("running.txt"),
            format!(
                "nom=Croc 2 Kingdom of the Gobbos\n\
                 paquet=/home/gab/Windows/Games/Croc 2 Kingdom of the Gobbos.wgp\n\
                 pids={}\n",
                enfant.id()
            ),
        )
        .unwrap();
        std::fs::create_dir_all(cache.join("assets/logos")).unwrap();
        std::fs::write(
            cache.join("assets/logos/croc-2-kingdom-of-the-gobbos.png"),
            b"png",
        )
        .unwrap();

        // SAFETY : seuls ce test lit ces deux variables (les autres passent le
        // cache en argument)
        unsafe {
            std::env::set_var("GLAUNCHER_CACHE", &cache);
            std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        }
        let jeux = lire_jeux_actifs();
        assert_eq!(jeux.len(), 1, "la publication au PID mort est écartée");
        let jeu = jeu_de(&jeux, enfant.id() as i32).expect("enfant publié");
        assert_eq!(jeu.slug, "croc-2-kingdom-of-the-gobbos");
        assert!(
            jeu.logo.ends_with("assets/logos/croc-2-kingdom-of-the-gobbos.png"),
            "logo: {}",
            jeu.logo
        );
        assert!(
            jeu_de(&jeux, std::process::id() as i32).is_none(),
            "la fenêtre d'une autre application n'est pas ce jeu"
        );
        unsafe {
            std::env::remove_var("GLAUNCHER_CACHE");
            std::env::remove_var("XDG_RUNTIME_DIR");
        }

        let _ = enfant.kill();
        let _ = enfant.wait();
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&runtime);
    }

    /// gwine lancé sur un `.exe` seul ne publie pas de paquet : le nom sert de
    /// base, l'overlay montre le nom du programme et cherche son icône dans le
    /// cache (rien d'inventé si elle n'y est pas).
    #[test]
    fn exe_seul_sans_paquet() {
        let dir = bac("exe-seul");
        let fichier = dir.join("running.txt");
        let moi = std::process::id() as i32;
        std::fs::write(
            &fichier,
            format!("nom=Croc2\npids={moi}\n"),
        )
        .unwrap();
        let jeu = lire_jeu(&fichier, &dir).expect("nom publié");
        assert_eq!(jeu.slug, "croc2");
        assert_eq!(jeu.nom, "Croc2");
        assert_eq!(jeu.logo, "", "aucun visuel inventé");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// La session se lit dans `/proc/<pid>/stat` : c'est le repli qui rattache
    /// une fenêtre Wine reparentée à systemd au sandbox qui l'a lancée.
    #[test]
    fn session_lue_dans_proc() {
        let moi = std::process::id() as i32;
        let session = session_of(moi).expect("session du processus courant");
        assert!(session > 0, "session {session}");
        assert_eq!(session_of(1), Some(1), "systemd ouvre la session 1");

        let mut enfant = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        assert_eq!(
            session_of(enfant.id() as i32),
            Some(session),
            "un enfant direct reste dans notre session"
        );
        let _ = enfant.kill();
        let _ = enfant.wait();
    }

    /// Une seule pression physique = une seule frappe : un rapport dupliqué
    /// (deux `value 1` sans relâchement) et un rebond de membrane (deux
    /// impulsions à quelques ms) sont jetés, un vrai double appui passe.
    /// Sans ça, un appui sur l'en-tête ouvrait la page FENÊTRE puis armait
    /// FERMER du même geste (confirmation déjà affichée, constaté à l'usage).
    #[test]
    fn une_pression_physique_une_seule_frappe() {
        let (mut tenues, mut derniere) = ([false; 0x140], None);
        let t0 = Instant::now();
        let sud = BTN_SOUTH;

        // appui simple : accepté
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 1, t0),
            Frappe::Neuve
        );
        // rapport dupliqué, sans relâchement : jeté
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 1, t0),
            Frappe::DejaTenue
        );
        // relâchement : toujours retenu
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 0, t0),
            Frappe::Neuve
        );
        // rebond : 20 ms après, jeté (et le relâchement qui suit est retenu)
        let t1 = t0 + Duration::from_millis(20);
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 1, t1),
            Frappe::Rebond
        );
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 0, t1),
            Frappe::Neuve
        );
        // vrai double appui (au-delà de la fenêtre) : accepté
        let t2 = t0 + Duration::from_millis(FRAPPE_MIN_MS + 20);
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 1, t2),
            Frappe::Neuve
        );
        // un AUTRE bouton n'est pas concerné par la fenêtre du premier
        let t3 = t0 + Duration::from_millis(FRAPPE_MIN_MS / 2);
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, BTN_EAST, 1, t3),
            Frappe::Neuve
        );
        // l'autorépétition (`value 2`) n'est jamais une frappe à jeter ici
        assert_eq!(
            frappe_nouvelle(&mut tenues, &mut derniere, sud, 2, t3),
            Frappe::Neuve
        );
    }

    /// L'accueil est le menu des catégories : chacune une seule fois, toutes
    /// avec un contenu, et l'en-tête de fenêtre en tête — il a une hauteur
    /// nulle mais le curseur doit pouvoir s'y poser partout (A y ouvre les
    /// actions de la fenêtre active).
    #[test]
    fn accueil_est_le_menu_des_categories() {
        let accueil = accueil_items(false);
        // la bascule jeu ⇄ app n'apparaît qu'avec un jeu ET une app compagne :
        // en tête, et elle ne prend pas la place d'une catégorie
        let avec_bascule = accueil_items(true);
        assert_eq!(avec_bascule.first(), Some(&Entree::Basculer));
        assert_eq!(avec_bascule.len(), accueil.len() + 1);
        let cats: Vec<Cat> = accueil
            .iter()
            .map(|e| match e {
                Entree::SousMenu(_, cat) => *cat,
                autre => panic!("entrée {autre:?} à l'accueil : ce n'est pas une catégorie"),
            })
            .collect();
        assert_eq!(cats.len(), 8, "une catégorie par domaine");

        for cat in &cats {
            let items = cat.entrees(Dispo::default());
            assert_eq!(
                items.first(),
                Some(&Entree::Fenetre),
                "{cat:?} doit commencer par l'en-tête de fenêtre"
            );
            assert!(items.len() >= 2, "{cat:?} sans aucune action");
            assert!(!cat.nom().is_empty(), "{cat:?} sans libellé");
            assert!(
                items.iter().all(|e| e.selectionnable()),
                "{cat:?} : une ligne non sélectionnable"
            );
        }

        // une catégorie ne doit pas apparaître deux fois (le retour depuis une
        // catégorie retrouve son entrée par ce libellé)
        let mut uniques: Vec<String> = cats.iter().map(|c| format!("{c:?}")).collect();
        uniques.sort();
        uniques.dedup();
        assert_eq!(uniques.len(), cats.len(), "catégorie en double");
    }

    /// Présence conditionnelle : `SAUVER REPLAY` seulement quand le buffer
    /// tourne, la déconnexion seulement quand une manette Bluetooth est connue.
    #[test]
    fn entrees_conditionnelles_des_categories() {
        let sauvable = |replay| {
            Cat::Capture
                .entrees(Dispo {
                    replay,
                    ..Dispo::default()
                })
                .contains(&Entree::Action("SAUVER REPLAY"))
        };
        assert!(!sauvable(false), "pas de ligne morte au repos");
        assert!(sauvable(true));

        // la veille est proposée en tête du menu système : seule action
        // réversible de la page (demandé le 23/11)
        // (la ligne 0 est l'en-tête de fenêtre, commune à toutes les pages)
        let systeme = Cat::Systeme.entrees(Dispo::default());
        assert_eq!(systeme.get(1), Some(&Entree::Action("MISE EN VEILLE")));
        // le mode d'à côté dépend de la session courante, jamais les deux
        assert_eq!(systeme.get(2), Some(&Entree::Action("MODE JEU")));
        let console = Cat::Systeme.entrees(Dispo {
            console: true,
            ..Dispo::default()
        });
        assert_eq!(console.get(2), Some(&Entree::Action("MODE BUREAU")));
        assert!(!console.contains(&Entree::Action("MODE JEU")));
        assert!(!systeme.contains(&Entree::Action("MODE BUREAU")));
        assert!(systeme.contains(&Entree::Action("ETEINDRE")));

        let deconnectable = |pad| {
            Cat::Manette
                .entrees(Dispo {
                    manette: pad,
                    ..Dispo::default()
                })
                .contains(&Entree::Action("DECONNECTER LA MANETTE"))
        };
        assert!(!deconnectable(false));
        assert!(deconnectable(true));
    }
}
