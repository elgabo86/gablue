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
use std::io::Write as _;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
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
        if ic.w <= 0 || ic.h <= 0 || taille <= 0 {
            return;
        }
        for dy in 0..taille {
            let sy0 = dy * ic.h / taille;
            let sy1 = ((dy + 1) * ic.h / taille).max(sy0 + 1).min(ic.h);
            for dx in 0..taille {
                let sx0 = dx * ic.w / taille;
                let sx1 = ((dx + 1) * ic.w / taille).max(sx0 + 1).min(ic.w);
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entree {
    Action(&'static str),
    /// application/media : ouvre si fermé, ferme si ouvert
    Appli(&'static str),
    Volume,
    Mango,
    Fps,
    /// durée du buffer de replay (secondes)
    Duree,
    /// en-tête = fenêtre active : A ouvre la liste des fenêtres
    Fenetre,
    /// une fenêtre de la liste (index dans `fenetres`)
    Autre(usize),
    /// séparateur de section ("SON", "AFFICHAGE"…) : un titre + un filet,
    /// jamais sélectionnable, sauté par la navigation
    Groupe(&'static str),
    /// style visuel (NU / DECK / BREEZE), appliqué à chaud
    Style,
}

/// Pages du panneau : accueil (actions + fenêtre active), réglages (tous les
/// réglages, par sections) et applications. « Fenetres » est la liste
/// d'aller-retour ouverte depuis l'en-tête.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Accueil,
    Reglages,
    Applis,
    /// actions de la fenêtre active (ancienne « boîte de choix ») : une PAGE,
    /// comme la liste des fenêtres — même présentation, même retour
    Fenetre,
    Fenetres,
}

impl Entree {
    fn nom(self) -> &'static str {
        match self {
            Entree::Action(n) | Entree::Appli(n) => n,
            Entree::Volume => "VOLUME",
            Entree::Mango => "MANGOHUD",
            Entree::Fps => "FPS",
            Entree::Duree => "DUREE REPLAY",
            Entree::Fenetre => "FENETRE ACTIVE",
            Entree::Autre(_) => "FENETRE",
            Entree::Groupe(t) => t,
            Entree::Style => "STYLE",
        }
    }
    /// ligne de réglage (curseur) plutôt qu'une action ponctuelle
    fn reglage(self) -> bool {
        matches!(
            self,
            Entree::Volume | Entree::Mango | Entree::Fps | Entree::Duree
        )
    }
    /// le curseur peut s'y poser (tout sauf un séparateur)
    fn selectionnable(self) -> bool {
        !matches!(self, Entree::Groupe(_))
    }
}

/// Ce qu'on lance une fois le panneau retiré.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Apres {
    Record,
    Capture,
    /// clavier virtuel : idem (pont EVIOCGRAB + clavier KWin)
    Clavier,
    /// sauvegarde du replay : après fermeture, pour ne pas se filmer
    Replay,
    /// bascule du mode replay (Home+haut) : après fermeture, pour que le bip
    /// de départ ne soit ni filmé par le buffer ni masqué par le panneau
    ReplayToggle,
    /// menu console : redémarrer / éteindre (le panneau doit avoir disparu)
    Reboot,
    Eteindre,
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
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_TL2: u16 = 0x138;
const BTN_TR2: u16 = 0x139;

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
    /// page RÉGLAGES : sections (titre + filet) et lignes de réglage
    reglages: Vec<Entree>,
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
    fondu_ms: u64,
    closing: bool,
    slid_in: bool,
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
    /// node de la manette à passer à gpad-kbd (relâché au grab, donc mémorisé)
    combo_node: Option<String>,
    /// mode souris actif (processus `gpad-mouse` vivant)
    mouse_active: bool,
    /// buffer de replay actif (gpu-screen-recorder lancé avec -r)
    replay_active: bool,
    /// VRR automatique actif sur les sorties capables (`vrr.sh get`)
    vrr_on: bool,
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
    /// plage réelle des axes du stick (EVIOCGABS) : les seuils en dépendent
    abs_x: (i32, i32),
    abs_y: (i32, i32),
    fps_idx: usize,
    mh_idx: usize,
    /// position dans REPR_OPTS (30 s par défaut)
    repr_idx: usize,
    vol: i32,
    /// sortie muette (A sur la ligne VOLUME bascule)
    vol_mute: bool,
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
    /// enregistreur actif (lu sur le fichier d'état de recorder.sh)
    rec_active: bool,
    /// décalage horizontal du panneau en pixels : les marges layer-shell
    /// négatives sont ignorées par KWin (mesuré), donc on décale le DESSIN.
    off: i32,
    /// changement de page en cours : (ancienne, nouvelle, départ, sens)
    pdep: Option<(Page, Page, Instant, i32)>,
    /// fermeture automatique après inactivité manette (0 = jamais)
    inactivite: Duration,
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
        "EMULATIONSTATION" => &["esde", "es-de", "emulationstation"],
        "YOUTUBE" => &["vacuumtube"],
        "GABLUE TV" => &["tvqt", "gablue_tv", "gablue-tv"],
        "JELLYFIN" => &["jellyfin"],
        _ => &[],
    }
}

/// Entrées qui exigent un second appui : on ne déconnecte pas, ne redémarre
/// pas et n'éteint pas sur un appui malheureux.
fn demande_confirmation(nom: &str) -> bool {
    matches!(
        nom,
        "FERMER"
            | "DECONNECTER LA MANETTE"
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
        let chemin = format!("/proc/{pid}/task/{pid}/children");
        let Ok(txt) = std::fs::read_to_string(&chemin) else {
            continue;
        };
        for tok in txt.split_whitespace() {
            if let Ok(c) = tok.parse::<i32>() {
                out.push(c);
                pile.push(c);
            }
        }
    }
    out
}

/// `systemctl <action>` (redémarrer / éteindre). `REBOOT`/`POWEROFF` permettent
/// de remplacer la commande : le banc ne doit JAMAIS éteindre la machine.
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
            accueil: vec![
                // Page ACCUEIL : la fenêtre active et ses actions juste sous
                // son nom (fermer, plein écran), puis les actions de session
                // (enregistrement, capture, replay) et les entrées
                // inhabituelles (manette -> souris/clavier). Les réglages sont
                // sur la page REGLAGES.
                Entree::Fenetre,
                Entree::Action("ENREGISTRER UNE VIDEO"),
                Entree::Action("CAPTURE ECRAN"),
                Entree::Action("REPLAY"),
                Entree::Action("MODE SOURIS"),
                Entree::Action("CLAVIER VIRTUEL"),
            ],
            // page RÉGLAGES : sections, séparateurs non sélectionnables
            reglages: vec![
                // l'en-tête est sélectionnable sur TOUTES les pages : A y
                // propose de fermer la fenêtre ou de basculer son plein écran
                Entree::Fenetre,
                Entree::Groupe("SON"),
                Entree::Volume,
                Entree::Groupe("AFFICHAGE"),
                Entree::Mango,
                Entree::Fps,
                Entree::Action("VRR"),
                Entree::Groupe("REPLAY"),
                Entree::Duree,
                Entree::Groupe("INTERFACE"),
                Entree::Style,
            ],
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
            closing: false,
            slid_in: false,
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
            vrr_on: false,
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
            abs_x: (-32768, 32767),
            abs_y: (-32768, 32767),
            fps_idx: 0,
            mh_idx: 0,
            repr_idx: 1,
            vol: 50,
            vol_mute: false,
            evdev_node: None,
            hidraw_node: None,
            last_scan: Instant::now(),
            gate_sock: None,
            gate_nodes: 0,
            gate_on: false,
            slide_ms: 0,
            off: 0,
            pdep: None,
            inactivite: Duration::from_secs(10),
            derniere_activite: Instant::now(),
            derniere_nav: Instant::now() - Duration::from_secs(1),
            style_idx: 0,
            th: theme_retro(),
            police: None,
            glyphes: Vec::new(),
            rec_active: false,
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
        for (code, slot) in [(0u32, true), (1u32, false)] {
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
                if slot {
                    self.abs_x = (ai.minimum, ai.maximum);
                } else {
                    self.abs_y = (ai.minimum, ai.maximum);
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
                        self.request_close();
                    }
                }
                continue;
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
                // (sinon le panneau se filme dans la capture/le clip).
                // Home+gauche = sauver le replay SI le mode replay est actif,
                // sinon basculer l'enregistrement : une seule gâchette pour
                // les deux, elles sont exclusives. Home+haut = bascule du mode
                // replay (même geste que panneau fermé, vu par gpad-trigger).
                if self.home_down && matches!(a, Action::Left | Action::Right | Action::Up) {
                    self.home_down = false;
                    let apres = match (a, self.replay_active) {
                        (Action::Up, _) => Apres::ReplayToggle,
                        (Action::Left, true) => Apres::Replay,
                        (Action::Left, false) => Apres::Record,
                        _ => Apres::Capture,
                    };
                    self.combo_apres = Some(apres);
                    self.status = match apres {
                        Apres::Record => "COMBO: REC APRES FERMETURE",
                        Apres::Capture => "COMBO: CAPTURE APRES FERMETURE",
                        Apres::Clavier => "COMBO: CLAVIER APRES FERMETURE",
                        Apres::Replay => "COMBO: REPLAY APRES FERMETURE",
                        Apres::ReplayToggle => "COMBO: REPLAY BASCULE",
                        // jamais posés par un combo (menu console seulement)
                        Apres::Reboot => "COMBO: REBOOT",
                        Apres::Eteindre => "COMBO: EXTINCTION",
                        Apres::ToutFermer => "COMBO: TOUT FERMER",
                    }
                    .to_string();
                    println!(
                        "COMBO {} -> fermeture puis action",
                        match a {
                            Action::Left => "Home+gauche",
                            Action::Up => "Home+haut",
                            _ => "Home+droite",
                        }
                    );
                    self.request_close();
                    continue;
                }
                self.apply(a);
            }
        }
    }

    /// La liste affichée : accueil, réglages, applications ou fenêtres.
    fn liste(&self) -> &Vec<Entree> {
        match self.page {
            Page::Accueil => &self.accueil,
            Page::Reglages => &self.reglages,
            Page::Applis => &self.applis,
            Page::Fenetre => &self.fen_menu,
            Page::Fenetres => &self.fen_items,
        }
    }

    /// Position de la page dans le cycle (les sous-pages FENETRE / FENETRES
    /// appartiennent à la page qui les a ouvertes : c'est elle qui s'allume).
    fn page_cycle(&self) -> usize {
        let ordre = [Page::Accueil, Page::Reglages, Page::Applis];
        let base = if matches!(self.page, Page::Fenetres | Page::Fenetre) {
            self.fen_retour
        } else {
            self.page
        };
        ordre.iter().position(|p| *p == base).unwrap_or(0)
    }

    /// Suivi de page en bas : trois **ronds**, le courant plein et un peu plus
    /// gros, les autres en `rail`. (Des carrés à l'origine — `pas de carré`
    /// pour le titre, mais le suivi reste, en ronds.)
    fn points_pages(&self, cv: &mut Canvas, x_centre: i32, y: i32) {
        const D: i32 = 9;
        const GAP: i32 = 9;
        let actif = self.page_cycle();
        let mut x = x_centre - (3 * D + 2 * GAP) / 2;
        for i in 0..3 {
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

    /// CLAVIER VIRTUEL : pont gpad-kbd (grab evdev + clavier virtuel KWin).
    /// Il grabbe la manette : le panneau doit disparaître avant.
    fn clavier_virtuel(&mut self) {
        self.combo_node = self.pad_path.clone();
        self.combo_apres = Some(Apres::Clavier);
        self.status = "CLAVIER VIRTUEL: DEMARRAGE".to_string();
        println!("KBDNAV START");
        self.request_close();
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
            Entree::Fps => {
                let max = FPS_OPTS.len() as i32 - 1;
                let i = (self.fps_idx as i32 + sens).clamp(0, max) as usize;
                if i == self.fps_idx {
                    self.status = format!("FPS: {}", FPS_OPTS[i].0);
                    return;
                }
                self.fps_idx = i;
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
                println!("REPR_IDX {} ({})", i, REPR_OPTS[i].1);
            }
            // réglé par A (interrupteur), pas par gauche/droite
            Entree::Style => self.basculer_style(),
            Entree::Action(_)
            | Entree::Appli(_)
            | Entree::Fenetre
            | Entree::Autre(_)
            | Entree::Groupe(_) => {}
        }
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
            format!("ACTIVE: {}", f.caption)
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

    fn refresh_volume(&mut self) {
        self.vol_mute = Self::lire_mute();
        if let Ok(o) = std::process::Command::new("/usr/bin/pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
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

    /// Lit les réglages MangoHud pour positionner les vues sur l'état réel.
    fn refresh_mangohud(&mut self) {
        if let Ok(p) = std::env::var("MANGOHUD") {
            if let Ok(o) = std::process::Command::new(&p).arg("get").output() {
                let t = String::from_utf8_lossy(&o.stdout);
                for l in t.lines() {
                    if let Some(v) = l.strip_prefix("fps=") {
                        if let Ok(f) = v.trim().parse::<i32>() {
                            if let Some(i) = FPS_OPTS.iter().position(|(_, x)| *x == f) {
                                self.fps_idx = i;
                            }
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
                    return;
                }
                Action::Right => {
                    self.regler(1);
                    return;
                }
                _ => {}
            }
        }
        match a {
            Action::Page(sens) => {
                // R1 avance, L1 recule (les deux allaient dans le même sens) ;
                // la liste des fenêtres est un aller-retour : L1/R1 en sortent
                let ordre = [Page::Accueil, Page::Reglages, Page::Applis];
                let base = if self.page == Page::Fenetres {
                    self.fen_retour
                } else {
                    self.page
                };
                let i = ordre.iter().position(|p| *p == base).unwrap_or(0) as i32;
                let j = (i + sens).rem_euclid(ordre.len() as i32) as usize;
                self.aller_page(ordre[j], sens);
            }
            Action::Up => {
                self.selected = self.pas_selectable(self.selected, -1);
                println!("NAV UP {}", self.selected);
            }
            Action::Down => {
                self.selected = self.pas_selectable(self.selected, 1);
                println!("NAV DOWN {}", self.selected);
            }
            Action::Left | Action::Right => {
                self.status = format!("{a:?} SANS EFFET");
                println!("NAV {a:?}");
            }
            Action::Cycle(sens) => self.cycler_fenetre(sens),
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
            Page::Accueil => "MENU: ACTIONS".to_string(),
            Page::Reglages => "MENU: REGLAGES".to_string(),
            Page::Applis => "MENU: APPLICATIONS".to_string(),
            Page::Fenetre => "MENU: FENETRE".to_string(),
            Page::Fenetres => "MENU: FENETRES".to_string(),
        };
        println!("PAGE {:?} sens={sens}", p);
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
            Entree::Fps => self.status = "FPS: GAUCHE/DROITE".to_string(),
            Entree::Duree => self.status = "DUREE REPLAY: GAUCHE/DROITE".to_string(),
            Entree::Mango => self.regler(1),
            Entree::Style => self.basculer_style(),
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
                    return;
                }
                let script = match nom {
                    "EMULATIONSTATION" => "toggle-es",
                    "YOUTUBE" => "toggle-yt",
                    "GABLUE TV" => "toggle-tv",
                    "JELLYFIN" => "toggle-jellyfin",
                    _ => "",
                };
                self.status = if script.is_empty() {
                    format!("{nom}: RIEN A FAIRE")
                } else {
                    lancer_appli(script)
                };
            }
            // un séparateur n'agit pas (il n'est pas sélectionnable)
            Entree::Groupe(_) => {}
            // A sur le nom de la fenêtre : proposer de la fermer ou de
            // basculer son plein écran (sur toutes les pages)
            // sans fenêtre active, la boîte n'aurait rien à fermer : on va
            // directement chercher une fenêtre
            Entree::Fenetre if self.win.caption.is_empty() => {
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
                    // Les deux modes sont exclusifs : activer le buffer pendant
                    // un enregistrement le transformait en « SAUVER REPLAY »
                    // alors qu'une capture tournait encore (deux modes à la
                    // fois, illisible). La ligne est grisée, on refuse aussi A.
                    if self.rec_active {
                        self.status = "REPLAY: BLOQUE (REC)".to_string();
                        println!("REPLAY refuse (enregistrement en cours)");
                        return;
                    }
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
                    println!("REPLAY {}", if self.replay_active { "start" } else { "stop" });
                }
                // replay actif : les deux sont exclusifs, la ligne devient la
                // sauvegarde du replay (même place dans le menu)
                "ENREGISTRER UNE VIDEO" if self.replay_active => {
                    let node = self.pad_path.clone().unwrap_or_default();
                    lancer_script_env("REPLAY", &["save"], &[("PAD_NODE", &node)]);
                    self.status = "REPLAY: SAUVEGARDE".to_string();
                    println!("REPLAY save (ligne enregistrement)");
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
                "ETEINDRE" => {
                    self.status = "EXTINCTION".to_string();
                    self.combo_apres = Some(Apres::Eteindre);
                    println!("CONSOLE poweroff");
                    self.request_close();
                }
                "DECONNECTER LA MANETTE" => {
                    // la manette part -> gpad-trigger enleve le panneau, gate referme
                    let mac = self.pad_bt.clone().unwrap_or_default();
                    println!("BT DISCONNECT {mac}");
                    self.status = bluetooth_disconnect(&mac);
                }
                "CLAVIER VIRTUEL" => self.clavier_virtuel(),
                "MODE SOURIS" => self.mouse_toggle(),
                _ => self.status = format!("{nom}: RIEN A FAIRE"),
            },
        }
    }

    /// Début de l'enregistrement en cours, écrit par `recorder.sh`
    /// (`start=<epoch>` dans le fichier d'état).
    fn rec_debut() -> Option<u64> {
        let txt = std::fs::read_to_string("/tmp/opencode/recorder.state").ok()?;
        txt.lines()
            .find_map(|l| l.strip_prefix("start="))
            .and_then(|v| v.trim().parse::<u64>().ok())
    }

    /// recorder.sh tient un fichier d'état : présence = enregistrement en cours.
    fn refresh_recorder(&mut self) {
        // Le FICHIER d'état peut survivre à un arrêt brutal (killall pendant un
        // test) : c'est le processus qui fait foi. Un enregistrement est un
        // gpu-screen-recorder SANS -r (le replay en a un).
        let etat = std::path::Path::new("/tmp/opencode/recorder.state").exists()
            && processus_actif("gpu-screen-recorder")
            && !replay_actif();
        if etat != self.rec_active {
            self.rec_active = etat;
            self.dirty = true;
        }
        // compteur : la source est le `start=` du recorder (juste même si le
        // panneau a été fermé/rouvert pendant l'enregistrement)
        self.rec_start = if etat { Self::rec_debut() } else { None };
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
        if self.inactivite.is_zero() || self.closing {
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
        if self.closing {
            return;
        }
        self.closing = true;
        if self.side {
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

    /// Texte aligné à droite (police à chasse fixe : 6 px par caractère).
    fn valeur_droite(&self, cv: &mut Canvas, x_droite: i32, y: i32, s: &str, scale: i32, color: u32) {
        let w = cv.largeur(s, scale);
        cv.text_fit(x_droite - w, y, s, scale, color, w + 6);
    }

    /// Nombre de lignes visibles sur la page « fenêtres » (lignes de 46 px).
    fn fen_visibles(&self) -> usize {
        let (_, h) = self.size;
        let ph = if self.side { h as i32 } else { 640.min(h as i32 - 120) };
        // 180 = départ de liste (sous l'en-tête et le titre de page),
        // 90 = pied + marges
        (((ph - 90) - 180) / 46).max(1) as usize
    }

    /// Recale le défilement pour que la sélection soit toujours visible.
    /// Appelé avant le dessin (le canevas emprunte `self.px` : on ne peut plus
    /// toucher à `self` ensuite).
    fn ajuster_defilement(&mut self) {
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
            Page::Accueil => "ACTIONS",
            Page::Reglages => "REGLAGES",
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
        for (i, e) in self.liste().iter().enumerate() {
            if self.page == Page::Fenetres && i < self.fen_top {
                continue;
            }
            if self.page == Page::Fenetres && i >= self.fen_top + visibles {
                break;
            }
            // une ligne de réglage est plus haute : le curseur vit dessous
            // l'en-tête (Entree::Fenetre) est dessiné plus haut : hauteur nulle
            let h = match e {
                Entree::Fenetre => 0,
                // un séparateur respire : titre + filet, puis un peu d'air
                Entree::Groupe(_) => 52,
                _ if e.reglage() => 86,
                _ => 46,
            };
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
                    // enregistrement et replay sont exclusifs : la ligne est
                    // grisée pendant un enregistrement
                    let c_txt = if self.rec_active { cc_dim } else { color };
                    cv.text_fit(px0 + pad + 36, y, "REPLAY", 3, c_txt, inner - 36 - 52);
                    if self.rec_active {
                        self.valeur_droite(&mut cv, px0 + pad + inner, y, "BLOQUE (REC)", 2, cc_dim);
                    } else {
                        self.interrupteur(
                            &mut cv,
                            px0 + pad + inner,
                            y,
                            self.replay_active,
                            false,
                        );
                    }
                }
                Entree::Action("PLEIN ECRAN") => {
                    cv.text_fit(px0 + pad + 36, y, "PLEIN ECRAN", 3, color, inner - 36 - 52);
                    self.interrupteur(&mut cv, px0 + pad + inner, y, self.win.fullscreen, false);
                }
                // séparateur de section : titre en petit + filet qui court
                // jusqu'au bord (largeur d'un caractère à l'échelle 2 = 12 px)
                Entree::Groupe(titre) => {
                    let largeur = titre.len() as i32 * 12;
                    cv.rect(px0 + pad, y + 11, 4, 4, th.accent);
                    cv.text(px0 + pad + 12, y + 10, titre, 2, th.accent);
                    cv.filet_fondu(px0 + pad + largeur + 22, y + 16, inner - largeur - 26, 0x70, cc_dim);
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
                    // l'entrée d'enregistrement dit ce qu'elle FERA ; en mode
                    // replay elle devient la sauvegarde du replay (même place,
                    // les deux modes sont exclusifs)
                    let (label, c_txt) = if nom == "ENREGISTRER UNE VIDEO" {
                        if self.replay_active {
                            ("SAUVER REPLAY", cc_ok)
                        } else if self.rec_active {
                            ("STOP ENREGISTRER", color)
                        } else {
                            (nom, color)
                        }
                    } else {
                        (nom, color)
                    };
                    // place pour le compteur à droite pendant l'enregistrement
                    let max = if nom == "ENREGISTRER UNE VIDEO" && self.rec_active && !self.replay_active {
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
                    if nom == "ENREGISTRER UNE VIDEO" && self.rec_active && !self.replay_active {
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
                Entree::Volume => {
                    let (v, vc) = if self.vol_mute {
                        ("MUET".to_string(), cc_warn)
                    } else {
                        (format!("{}%", self.vol), color)
                    };
                    cv.text_fit(px0 + pad + 36, y, "VOLUME", 3, color, inner - 36);
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
                            f.caption.clone(),
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
                Entree::Fps => {
                    cv.text_fit(px0 + pad + 36, y, "FPS", 3, color, inner - 36);
                    let v = FPS_OPTS[self.fps_idx].0;
                    let v = if v == "SANS LIMITE" { "INF" } else { v };
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
            }
            y += h;
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
            let (pw, ph) = if self.console {
                (520.min(w - 80), 360.min(h - 120))
            } else {
                (760.min(w - 80), 640.min(h - 120))
            };
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

        // fenêtre/game actif : ce sur quoi « plein écran » et « tuer » agissent
        // (l'en-tête est lui-même une entrée : A ouvre la liste des fenêtres)
        if !self.console {
            let entete_sel = matches!(self.liste().get(self.selected), Some(Entree::Fenetre));
            // même surbrillance que les lignes, même règle de lisibilité
            let (c_cap, c_cls) = (th.sur(th.txt, entete_sel), th.sur(th.dim, entete_sel));
            if entete_sel {
                let (sx, sy, sw, sh) = (px0 + pad - 8, py0 + 50, inner + 16, 64);
                cv.rrect_v(sx, sy, sw, sh, th.rayon_sel, th.sel_haut, th.sel_bas);
                if th.barre > 0 {
                    cv.rect(sx, sy + 3, th.barre, sh - 6, th.accent);
                }
            }
            let (cap, cls) = if self.win.caption.is_empty() {
                ("AUCUNE FENETRE".to_string(), String::new())
            } else {
                (self.win.caption.clone(), self.win.class.clone())
            };
            let decal = if let Some(ic) = &self.icone {
                cv.icone(px0 + pad, py0 + 56, 34, ic);
                44
            } else {
                0
            };
            cv.text_fit(px0 + pad + decal, py0 + 62, &cap, 3, c_cap, inner - decal);
            if !cls.is_empty() {
                // classe de l'appli : sa propre ligne, détachée du nom (elle le
                // collait au bord de la pastille en MODERNE)
                cv.text_fit(px0 + pad + decal, py0 + 94, &cls, 2, c_cls, inner - decal);
            }
        }

        // nom du menu courant, juste SOUS le nom de la fenêtre active
        if !self.console {
            self.nom_menu(
                &mut cv,
                px0 + pw / 2,
                py0 + 134,
                px0 + pad,
                px0 + pad + inner,
            );
        }

        // 146 et non 124 : l'en-tête (nom + classe) garde une vraie respiration
        // avant le premier choix, sinon on ne distingue pas « la fenêtre » des
        // actions (constaté à l'usage)
        let y_liste = if self.console { py0 + 62 } else { py0 + 180 };
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
            self.points_pages(&mut cv, px0 + pw / 2, py0 + ph - 66);
        }

        // bas du panneau : juste les commandes, pas de journal technique
        // le pied laisse la place à la barre des tâches (panneau plein écran) ;
        // dans le menu système le cadre est court, l'aide reste près du bas
        let sy = py0 + ph - if self.console { 74 } else { 124 };
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

        // fondu : on applique l'opacité courante à tout le tampon (menu
        // système seulement — le panneau latéral, lui, glisse)
        let op = self.opacite;
        if !self.side {
            cv.appliquer_opacite(op);
        }

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
        .output()
    {
        Ok(o) => {
            let t = String::from_utf8_lossy(&o.stdout);
            let l = t
                .lines()
                .rev()
                .map(|l| l.trim())
                .find(|l| !l.is_empty())
                .unwrap_or("");
            if l.is_empty() {
                format!("BT: DECONNEXION {mac}")
            } else {
                format!("BT: {}", clip(l, 34))
            }
        }
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

/// Processus (comm, pid) qui tiennent ce node ouvert.

/// "nom:pid, ..." lisible, tronqué

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('.');
        t
    }
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

/// Un buffer de replay tourne-t-il ? Le binaire s'appelle `gpu-screen-recorder`
/// dans les deux cas (replay ET enregistrement) : c'est l'option `-r <sec>`
/// qui distingue le replay, et le binaire n'accepte qu'une instance.
fn replay_actif() -> bool {
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
        let Ok(raw) = std::fs::read(p.join("cmdline")) else { continue };
        let args: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
        let est_gsr = args
            .first()
            .map(|a| a.ends_with(b"gpu-screen-recorder"))
            .unwrap_or(false);
        if est_gsr && args.iter().any(|a| *a == b"-r") {
            return true;
        }
    }
    false
}

/// Nom de processus d'un binaire configuré par variable (MOUSE, KBDNAV…).
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

/// Ce que le script KWin pousse : la fenêtre active et, sur demande, la liste.
#[derive(Default)]
struct Partage {
    info: WinInfo,
    liste: Vec<Fenetre>,
    /// nombre d'entrées annoncé par `Fin(n)` : 0 = liste pas (encore) arrivée
    fin: usize,
    /// au moins un `Info` reçu (preuve que le script KWin tourne vraiment)
    recu: bool,
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
    std::fs::write(&dest, txt.replace("__BUS__", bus)).ok()?;
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

/// Envoie une commande au démon gpad-gate. Une connexion par commande : le démon
/// traite un message par connexion, et ça survit à un redémarrage du démon.
/// Échec silencieux (socket absente) : l'overlay doit rester utilisable seul.
fn gate_cmd(sock: &str, msg: &str) -> bool {
    use std::os::unix::net::UnixStream;
    match UnixStream::connect(sock) {
        Ok(mut s) => {
            let mut buf = String::with_capacity(msg.len() + 1);
            buf.push_str(msg);
            buf.push('\n');
            s.write_all(buf.as_bytes()).is_ok()
        }
        Err(_) => false,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
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

    // style : --style > PAD_STYLE (banc) > fichier (choix fait dans REGLAGES)
    // > **MODERNE** (défaut). Un nom inconnu retombe sur RETRO, jamais d'échec.
    let style_nom = arg_value(&args, "--style")
        .or_else(|| std::env::var("PAD_STYLE").ok())
        .or_else(lire_style)
        .unwrap_or_else(|| "moderne".to_string());
    app.style_idx = style_normalise(&style_nom);
    app.th = theme_pour(app.style_idx);
    app.police = if app.style_idx == 1 { charger_police() } else { None };
    println!(
        "STYLE {} ({}) police={}",
        STYLE_OPTS[app.style_idx],
        STYLE_KEYS[app.style_idx],
        if app.police.is_some() { "ttf" } else { "bitmap" }
    );

    let compositor: wl_compositor::WlCompositor =
        globals.bind(&qh, 4..=6, ()).expect("wl_compositor");
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals
        .bind(&qh, 1..=4, ())
        .expect("zwlr_layer_shell_v1");
    app.shm = Some(shm);

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

    if let Ok(seat) = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=9, ()) {
        let _kb = seat.get_keyboard(&qh, ());
    }

    let pad = if args.iter().any(|a| a == "--no-grab") {
        None
    } else {
        arg_value(&args, "--grab").or_else(find_pad_event)
    };
    if let Some(p) = pad {
        app.detecter_manette(&p);
        app.grab_pad(&p);
    } else {
        eprintln!("aucune manette trouvée (aucun grab)");
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
    // `--focus` (Home+bas) impose sa ligne : ne pas la déplacer ensuite
    let mut focus_force = false;
    if let Some(f) = arg_value(&args, "--focus") {
        // combo Home+haut/bas : le panneau s'ouvre avec la ligne déjà choisie
        let cible = match f.as_str() {
            "fps" => Some(Entree::Fps),
            "mango" => Some(Entree::Mango),
            "volume" => Some(Entree::Volume),
            _ => None,
        };
        if let Some(c) = cible {
            // ces lignes vivent sur la page RÉGLAGES : on s'y place
            app.page = Page::Reglages;
            if let Some(i) = app.reglages.iter().position(|e| *e == c) {
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

    if app.console {
        // menu système (`--console`) : que des actions irréversibles
        let mut v = vec![Entree::Action("TOUT FERMER")];
        if app.pad_bt.is_some() {
            v.push(Entree::Action("DECONNECTER LA MANETTE"));
        }
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
            app.request_close();
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

        // multiplexage : fd manette + fd wayland, jamais de roundtrip bloquant
        let guard = conn.prepare_read();
        let pad_fd = app.pad.as_ref().map(|f| f.as_raw_fd());
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
        ];
        // timeout court pendant l'animation : ~120 Hz pour un glissement fluide
        let timeout_ms = if app.anim.is_some() || app.pdep.is_some() || app.fondu.is_some() {
            8
        } else {
            100
        };
        unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };

        if fds[0].revents & libc::POLLIN != 0 {
            app.pump_pad();
        }
        // maintien de Home : vérifié à chaque tour (poll 100 ms au repos)
        app.surveiller_home();
        app.surveiller_inactivite();
        if let Some(g) = guard {
            if fds[1].revents != 0 {
                let _ = g.read();
            }
        }
        // glissement / fondu d'entrée, une seule fois, dès que la surface est
        // configurée — et AVANT la première image, sinon le fondu serait déjà
        // terminé quand le menu paraît (wayland + script KWin + icônes prennent
        // bien plus que 160 ms : constaté, « ça ne fait pas la fondue »)
        if app.configured && !app.slid_in {
            app.slid_in = true;
            if app.side {
                app.slide(app.panel_w, 0, 180);
                println!("SLIDE IN");
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
        if app.anim.is_some() {
            app.tick_anim();
        }
        app.tick_page();
        app.tick_fondu();
        if app.closing && app.anim.is_none() && app.fondu.is_none() {
            app.running = false;
        }

        // fenêtre active : relue à chaque tour (poll 100 ms) pour que le nom
        // reste juste même si le panneau est ouvert longtemps
        if let Some(st) = &app.win_etat {
            if let Ok(p) = st.lock() {
                if p.info != app.win {
                    app.win = p.info.clone();
                    app.dirty = true;
                    println!(
                        "WIN pid={} fs={} uuid={} {} | {}",
                        app.win.pid,
                        app.win.fullscreen,
                        app.win.uuid,
                        app.win.caption,
                        app.win.class
                    );
                    let cle = format!("{}|{}", app.win.desktop, app.win.class);
                    if cle != app.icone_cle {
                        app.icone_cle = cle;
                        app.icone =
                            charger_icone(&app.win.desktop, &app.win.class, &app.win.caption);
                        println!("ICONE {}", if app.icone.is_some() { "trouvee" } else { "aucune" });
                    }
                }
            }
        }
        // Page d'ouverture, décidée **une fois** que l'info de fenêtre est
        // arrivée (D-Bus, donc après le démarrage) ou après une grâce de
        // 600 ms : fenêtre active -> ACTIONS avec le curseur sur son nom ;
        // aucune fenêtre -> APPLICATIONS, la page ACTIONS n'aurait aucune cible.
        if !app.page_initiale && app.configured {
            let recu = app
                .win_etat
                .as_ref()
                .and_then(|st| st.lock().ok().map(|p| p.recu))
                .unwrap_or(false);
            if recu || app.depart.elapsed() >= Duration::from_millis(600) {
                app.page_initiale = true;
                if !app.console && !app.focus_impose {
                    if app.win.caption.is_empty() {
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

        if app.last_scan.elapsed() > Duration::from_millis(1000) {
            app.refresh_recorder();
            // ces états changent hors du panneau : sans `dirty`, l'affichage
            // garde l'ancienne valeur (le mode replay restait « ARRET »
            // alors qu'il tournait)
            let souris = processus_actif(&nom_binaire("MOUSE", "gpad-mouse"));
            if souris != app.mouse_active {
                app.mouse_active = souris;
                app.dirty = true;
            }
            let replay = replay_actif();
            if replay != app.replay_active {
                app.replay_active = replay;
                app.dirty = true;
            }
            app.last_scan = Instant::now();
        }
        if app.dirty {
            app.draw();
            let _ = conn.flush();
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
    if let Some(a) = app.combo_apres.take() {
        println!("FENETRE RETIREE -> action differee");
        match a {
            Apres::Record => lancer_script("RECORDER", &["toggle"]),
            Apres::Capture => lancer_script("SCREENSHOT", &[]),
            Apres::Clavier => {
                let node = app.combo_node.clone().unwrap_or_default();
                lancer_script("KBDNAV", &[&node])
            }
            Apres::Replay => {
                let node = app.combo_node.clone().unwrap_or_default();
                lancer_script_env("REPLAY", &["save"], &[("PAD_NODE", &node)]);
            }
            Apres::ReplayToggle => {
                let node = app.combo_node.clone().unwrap_or_default();
                lancer_script_env("REPLAY", &["toggle"], &[("PAD_NODE", &node)]);
            }
            Apres::Reboot => lancer_systeme("REBOOT", "reboot"),
            Apres::Eteindre => lancer_systeme("POWEROFF", "poweroff"),
            Apres::ToutFermer => {
                // jeux Windows/émulateurs (killthemall) PUIS les applis du
                // menu Applications : c'est ce que « tout fermer » veut dire
                lancer_script("KILLTHEMALL", &[]);
                lancer_script("FERMERAPPLIS", &[]);
            }
        }
    }

    println!("GPAD_DOWN");
}
