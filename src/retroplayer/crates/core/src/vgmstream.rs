//! Repli sur `vgmstream-cli` pour les formats que zxtune ne lit pas
//! (`.txtp`, `.vgmstream`, `.ss2`… — sets PS1/PS2 de joshw.info).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Extensions qu'on confie à vgmstream (zxtune ne les décode pas).
const EXTENSIONS: &[&str] = &[
    "txtp", "vgmstream", "ss2", "ss", "adx", "brstm", "bcstm", "bfstm", "hca", "at3", "xma",
    "aa3", "oma", "opu", "sli", "kns", "dcs", "wsi", "wavm",
];

pub fn handles(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .map(|e| EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// Chemin de `vgmstream-cli` : `VGMSTREAM`, `bin/` du cache, PATH, brew.
pub fn binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VGMSTREAM") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let cached = crate::util::cache_root().join("bin").join("vgmstream-cli");
    if cached.exists() {
        return Some(cached);
    }
    for dir in [
        "/home/linuxbrew/.linuxbrew/bin",
        "/home/gab/.linuxbrew/bin",
        "/usr/local/bin",
        "/usr/bin",
    ] {
        let p = Path::new(dir).join("vgmstream-cli");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Ce que vgmstream annonce sur un fichier.
pub struct Info {
    pub rate: u32,
    pub channels: u16,
    pub samples: u64,
    /// Fin de boucle en échantillons (0 = pas de boucle connue).
    pub loop_start: u64,
    pub loop_end: u64,
}

impl Info {
    pub fn total(&self) -> f64 {
        self.samples as f64 / self.rate.max(1) as f64
    }
    /// Fenêtre de boucle `(début, fin)` en secondes, quand le format la donne.
    pub fn window(&self) -> Option<(f64, f64)> {
        if self.loop_end > self.loop_start && self.rate > 0 {
            Some((
                self.loop_start as f64 / self.rate as f64,
                self.loop_end as f64 / self.rate as f64,
            ))
        } else {
            None
        }
    }
}

fn field(text: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\":");
    let start = text.find(&pat)? + pat.len();
    let rest = &text[start..];
    let digits: String = rest
        .chars()
        .skip_while(|c| *c == ' ')
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Interroge vgmstream sans décoder (`-I` : métadonnées en JSON).
pub fn info(bin: &Path, path: &Path) -> Result<Info> {
    let out = Command::new(bin)
        .arg("-I")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("lancement de {}", bin.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() && text.trim().is_empty() {
        bail!(
            "vgmstream n'a pas lu {} : {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let rate = field(&text, "sampleRate").context("sampleRate absent")? as u32;
    let channels = field(&text, "channels").unwrap_or(2) as u16;
    let samples = field(&text, "numberOfSamples")
        .or_else(|| field(&text, "playSamples"))
        .context("nombre d'échantillons absent")?;
    let loop_start = field(&text, "loopStart").unwrap_or(0);
    let loop_end = field(&text, "loopEnd").unwrap_or(0);
    Ok(Info {
        rate,
        channels,
        samples,
        loop_start,
        loop_end,
    })
}

/// Décode un tour (intro comprise) sans fondu : le fondu est appliqué à la
/// lecture, et un rendu contenant N tours ferait réentendre la même partie.
pub fn decode(bin: &Path, path: &Path, out: &Path) -> Result<()> {
    let out = if out.exists() {
        std::fs::remove_file(out)?;
        out.to_path_buf()
    } else {
        out.to_path_buf()
    };
    let res = Command::new(bin)
        .arg("-o")
        .arg(&out)
        .arg("-l")
        .arg("1")
        .arg("-f")
        .arg("0")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("lancement de {}", bin.display()))?;
    if !out.exists() {
        bail!(
            "vgmstream n'a rien produit pour {} : {}",
            path.display(),
            String::from_utf8_lossy(&res.stderr).trim()
        );
    }
    Ok(())
}
