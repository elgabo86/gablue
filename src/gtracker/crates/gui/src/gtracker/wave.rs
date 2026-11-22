//! Textures de formes d'onde pour les clips.
//!
//! Une texture par clip, à largeur FIXE (1024 px), étirée à l'affichage : une
//! régénération par image pendant un zoom rendrait l'application inutilisable
//! (vécu de la Découpe, 21/11). Elle n'est refaite que si la plage de contenu
//! ou la teinte change.

use std::collections::HashMap;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use gtracker_core::peaks::Wave;

/// Largeur/hauteur de la texture : indépendantes de la taille affichée.
const TEX_W: usize = 1024;
const TEX_H: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WaveKey {
    pub source: usize,
    pub start: usize,
    pub end: usize,
    pub tint: [u8; 4],
}

#[derive(Default)]
pub struct WaveCache {
    map: HashMap<WaveKey, TextureHandle>,
    order: Vec<WaveKey>,
    pub built: usize,
}

impl WaveCache {
    pub fn get_or_build(
        &mut self,
        ctx: &egui::Context,
        key: WaveKey,
        wave: &Wave,
        tint: Color32,
    ) -> TextureHandle {
        if let Some(t) = self.map.get(&key) {
            return t.clone();
        }
        let mut pixels = vec![Color32::TRANSPARENT; TEX_W * TEX_H];
        let span = key.end.saturating_sub(key.start).max(1);
        let env = wave.range(key.start, key.end, TEX_W);
        let mid = TEX_H as f32 / 2.0;
        for (x, (lo, hi)) in env.iter().enumerate() {
            // 1 px de marge en haut/bas : une onde pleine échelle ne touche
            // jamais le bord du clip
            let y0 = (mid * (1.0 - hi.clamp(-1.0, 1.0))).round() as isize;
            let y1 = (mid * (1.0 - lo.clamp(-1.0, 1.0))).round() as isize;
            let (y0, y1) = (
                y0.clamp(0, TEX_H as isize - 1),
                y1.clamp(0, TEX_H as isize - 1),
            );
            let (a, b) = (y0.min(y1), y0.max(y1));
            for y in a..=b {
                pixels[y as usize * TEX_W + x] = tint;
            }
        }
        let _ = span;
        self.built += 1;
        let img = ColorImage::new([TEX_W, TEX_H], pixels);
        let tex = ctx.load_texture(
            format!("wave-{}-{}-{}", key.source, key.start, key.end),
            img,
            TextureOptions::LINEAR,
        );
        self.map.insert(key, tex.clone());
        self.order.push(key);
        // cache borné : 64 textures ≈ 16 Mo, un projet de 100 clips tient
        while self.order.len() > 64 {
            let old = self.order.remove(0);
            self.map.remove(&old);
        }
        tex
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}
