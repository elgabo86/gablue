//! Repli sur `vgmstream-cli` pour les formats que zxtune ne lit pas
//! (`.txtp`, `.vgmstream`, `.ss2`… — sets PS1/PS2 de joshw.info).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Extensions qu'on confie à vgmstream (zxtune ne les décode pas).
///
/// Volontairement **absents** : `.vag` et `.mp3`, que zxtune joue déjà très bien
/// (mesuré 20/11 sur un set PS3 `psf3.joshw.info` : VAG et MP3 reconnus par
/// `zxtune123`) — inutile de payer un sous-processus de plus.
/// `.fsb` (FMOD, sets Switch/WiiU/PC) et `.btsnd`/`.bfwav` (Wii U) sont des
/// conteneurs de morceaux entiers : vgmstream est le seul à les lire.
const EXTENSIONS: &[&str] = &[
    "txtp", "vgmstream", "ss2", "ss", "adx", "brstm", "bcstm", "bfstm", "bfwav", "bfstp",
    "btsnd", "fsb", "hca", "at3", "at9", "xma", "xma2", "binka", "aa3", "oma", "opu", "sli",
    "kns", "dcs", "wsi", "wavm",
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

/// Bornes de boucle annoncées par vgmstream (0, 0 quand il n'y en a pas).
fn loop_points(text: &str) -> (u64, u64) {
    let Some(i) = text.find("\"loopingInfo\"") else {
        return (0, 0);
    };
    let rest = &text[i..];
    let block = match rest.find('}') {
        Some(end) => &rest[..end],
        None => rest,
    };
    (
        field(block, "start").unwrap_or(0),
        field(block, "end").unwrap_or(0),
    )
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
pub fn info(bin: &Path, path: &Path, subsong: u32) -> Result<Info> {
    let mut cmd = Command::new(bin);
    cmd.arg("-I");
    if subsong > 1 {
        cmd.arg("-s").arg(subsong.to_string());
    }
    let out = cmd
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
    // `loopingInfo` est un sous-objet (`{"start":N,"end":M}`) ou `null` ; les
    // clés sont `start`/`end`, pas `loopStart`/`loopEnd` (vérifié 20/11 sur un
    // `.btsnd` Wii U : la boucle était purement et simplement perdue).
    let (loop_start, loop_end) = loop_points(&text);
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
pub fn decode(bin: &Path, path: &Path, out: &Path, subsong: u32) -> Result<()> {
    let out = if out.exists() {
        std::fs::remove_file(out)?;
        out.to_path_buf()
    } else {
        out.to_path_buf()
    };
    let mut cmd = Command::new(bin);
    cmd.arg("-o").arg(&out);
    if subsong > 1 {
        cmd.arg("-s").arg(subsong.to_string());
    }
    let res = cmd
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
