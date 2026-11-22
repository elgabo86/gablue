//! En-têtes `.genh` (VGMSystem) : le format donne le point de boucle exact.
//!
//! Contrairement à PSF, la boucle ne repart pas de zéro : `loop_start` et
//! `loop_end` sont en échantillons, à convertir avec le `sample_rate` du fichier.

use std::path::Path;

/// `(start, end)` en secondes, si le fichier en déclare une.
pub fn loop_window(path: &Path) -> Option<(f64, f64)> {
    if !path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("genh"))
        .unwrap_or(false)
    {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    if data.len() < 0x18 || &data[0..4] != b"GENH" {
        return None;
    }
    let word = |off: usize| -> u32 {
        u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
    };
    let rate = word(0x0C) as f64;
    let start = word(0x10) as f64;
    let end = word(0x14) as f64;
    if rate < 1000.0 || end <= start || start < 0.0 {
        return None;
    }
    Some((start / rate, end / rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entete_genh() {
        let dir = std::env::temp_dir().join("gtracker-genh-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.genh");
        let mut d = Vec::new();
        d.extend_from_slice(b"GENH");
        for v in [2u32, 32768, 42732, 2293760, 3153920, 0, 4096, 4096] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&p, &d).unwrap();
        let (s, e) = loop_window(&p).unwrap();
        assert!((s - 53.6778).abs() < 0.01, "start {s}");
        assert!((e - 73.8070).abs() < 0.01, "end {e}");
        // un fichier sans boucle ne doit rien donner
        let mut d2 = d.clone();
        d2[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
        std::fs::write(&p, &d2).unwrap();
        assert_eq!(loop_window(&p), None);
        let _ = std::fs::remove_file(&p);
    }
}
