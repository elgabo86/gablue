use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::Accessor;
use lofty::tag::{Tag, TagType};

use crate::model::Track;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    /// MP3 à débit variable, qualité maximale (`-V 0`, ≈ 245 kb/s) : meilleur
    /// rapport taille/qualité pour une écoute (demande user 21/11).
    Mp3Vbr,
    /// MP3 à débit constant 320 kb/s : taille prévisible, compatible partout.
    Mp3Cbr,
    Flac,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Mp3Vbr | Format::Mp3Cbr => "mp3",
            Format::Flac => "flac",
        }
    }

    /// Libellé d'interface (boutons d'export).
    pub fn label(self) -> &'static str {
        match self {
            Format::Mp3Vbr => "MP3 VBR",
            Format::Mp3Cbr => "MP3 320",
            Format::Flac => "FLAC",
        }
    }

    /// Aide du bouton : ce que le choix coûte en taille.
    pub fn hint(self) -> &'static str {
        match self {
            Format::Mp3Vbr => {
                "MP3 à débit variable, qualité maximale (≈ 245 kb/s) : \
                              le plus léger à qualité équivalente"
            }
            Format::Mp3Cbr => {
                "MP3 à débit constant 320 kb/s : taille prévisible, \
                               compatible partout"
            }
            Format::Flac => "FLAC sans perte (≈ 2× plus gros qu'un MP3 320)",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

/// Exporte une piste avec les backends natifs de zxtune (MP3 ou FLAC).
pub fn export(
    zxtune: &Path,
    track: &Track,
    out_dir: &Path,
    format: Format,
    tags: &Tags,
) -> Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let mut name = sanitize(&track.title);
    if name.is_empty() {
        name = "track".into();
    }
    let mut target = out_dir.join(format!("{name}.{}", format.ext()));
    let mut n = 2;
    while target.exists() {
        target = out_dir.join(format!("{name} ({n}).{}", format.ext()));
        n += 1;
    }

    let source = if track.subsong > 1 {
        format!("{}?#{}", track.file.display(), track.subsong)
    } else {
        track.file.display().to_string()
    };
    // zxtune prend les options du backend MP3 en **une seule** map séparée par
    // des virgules ; y mettre `filename=` casse un titre qui contient déjà une
    // virgule (« a,b.mp3 » s'arrête à « a » — mesuré le 21/11). On encode donc
    // dans un fichier temporaire au nom sûr, puis on renomme (même dossier =
    // renommage atomique).
    // Le FLAC est touché aussi : `filename=` passe par la même map, une
    // virgule dans le titre coupe le nom quel que soit le backend.
    let dest = temp_target(out_dir, format.ext());
    let mut wav: Option<PathBuf> = None;
    let mut opts: Vec<String> = Vec::new();
    match format {
        Format::Mp3Vbr | Format::Mp3Cbr => {
            let mode = match format {
                Format::Mp3Vbr => "vbr,quality=0",
                _ => "cbr,bitrate=320",
            };
            opts.push(format!("--mp3=mode={mode},filename={}", dest.display()));
        }
        Format::Flac => {
            // Ce zxtune n'a **pas** de backend FLAC (`--flac` → « unrecognised
            // option », constaté 21/11) : on rend en WAV puis ffmpeg encode,
            // comme le fait déjà l'export de suite.
            let w = temp_target(out_dir, "wav");
            opts.push("--wav".to_string());
            opts.push(format!("filename={}", w.display()));
            wav = Some(w);
        }
    }
    let mut args: Vec<&str> = opts.iter().map(|s| s.as_str()).collect();
    args.push("--quiet");
    args.push(&source);
    let stderr = crate::decode::run_zxtune(zxtune, &args)?;
    if let Some(w) = &wav {
        if !w.exists() {
            let _ = std::fs::remove_file(w);
            bail!("export {} impossible : {}", format.ext(), stderr.trim());
        }
        let st = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(w)
            .args(["-c:a", "flac", "-compression_level", "8"])
            .arg(&dest)
            .status()
            .context("lancement de ffmpeg (FLAC)")?;
        let _ = std::fs::remove_file(w);
        if !st.success() {
            let _ = std::fs::remove_file(&dest);
            bail!("encodage FLAC impossible (ffmpeg {st})");
        }
    }
    if !dest.exists() {
        let _ = std::fs::remove_file(&dest);
        bail!("export {} impossible : {}", format.ext(), stderr.trim());
    }
    std::fs::rename(&dest, &target)
        .with_context(|| format!("renommage {} → {}", dest.display(), target.display()))?;
    let _ = tag(&target, format, tags);
    Ok(target)
}

/// Exporte une **suite** de tranches `[start, end]` d'une piste en un seul
/// fichier MP3/FLAC. Les tranches peuvent se répéter : c'est l'arrangement de
/// la découpe rythmique (« la même mesure deux fois, puis la suivante »).
///
/// Le PCM vient du cache mémoire (`decode`) : aucune piste n'est re-rendue si
/// elle vient d'être jouée. L'encodage passe par `ffmpeg`, qui reçoit les
/// échantillons en f32 LE sur stdin — rien d'intermédiaire sur le disque.
pub fn export_segments(
    zxtune: &Path,
    track: &Track,
    spans: &[(f64, f64)],
    out_dir: &Path,
    format: Format,
    tags: &Tags,
    name: &str,
) -> Result<PathBuf> {
    if spans.is_empty() {
        bail!("aucun segment à exporter");
    }
    let decoded = crate::decode::decode(zxtune, track)?;
    let ch = decoded.channels.max(1) as usize;
    let fps = decoded.rate as f64 * ch as f64;
    let head = crate::decode::head_silence(&decoded.pcm, ch, decoded.rate);
    let ranges = segment_ranges(spans, fps, ch, decoded.total, decoded.pcm.len(), head);
    if ranges.is_empty() {
        bail!("segments vides : rien à encoder");
    }

    std::fs::create_dir_all(out_dir)?;
    let mut base = sanitize(name);
    if base.is_empty() {
        base = "montage".into();
    }
    let mut target = out_dir.join(format!("{base}.{}", format.ext()));
    let mut n = 2;
    while target.exists() {
        target = out_dir.join(format!("{base} ({n}).{}", format.ext()));
        n += 1;
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "f32le",
        "-ar",
        &decoded.rate.to_string(),
        "-ac",
        &decoded.channels.to_string(),
        "-i",
        "pipe:0",
        "-map_metadata",
        "-1",
    ]);
    match format {
        // même échelle que zxtune : VBR qualité maximale, ou CBR 320
        Format::Mp3Vbr => cmd.args(["-c:a", "libmp3lame", "-q:a", "0"]),
        Format::Mp3Cbr => cmd.args(["-c:a", "libmp3lame", "-b:a", "320k"]),
        Format::Flac => cmd.args(["-c:a", "flac", "-compression_level", "8"]),
    };
    let mut child = cmd
        .arg(&target)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("lancement de ffmpeg (segments)")?;
    {
        let mut stdin = std::io::BufWriter::new(child.stdin.take().context("stdin ffmpeg")?);
        let mut buf = Vec::with_capacity(64 * 1024);
        for (a, b) in ranges {
            for v in &decoded.pcm[a..b] {
                buf.extend_from_slice(&v.to_le_bytes());
                if buf.len() >= 64 * 1024 {
                    stdin.write_all(&buf)?;
                    buf.clear();
                }
            }
        }
        stdin.write_all(&buf)?;
        stdin.flush()?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() || !target.exists() {
        bail!(
            "ffmpeg a échoué : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let _ = tag(&target, format, tags);
    Ok(target)
}

/// Encode un PCM déjà rendu (projet GTracker) en MP3/FLAC, avec les mêmes
/// règles de nommage et de tags que l'export d'un montage.
pub fn export_pcm(
    pcm: &[f32],
    channels: u16,
    rate: u32,
    out_dir: &Path,
    format: Format,
    tags: &Tags,
    name: &str,
) -> Result<PathBuf> {
    let ch = channels.max(1) as usize;
    let frames = pcm.len() / ch;
    if frames == 0 {
        bail!("rien à exporter");
    }
    std::fs::create_dir_all(out_dir)?;
    let mut base = sanitize(name);
    if base.is_empty() {
        base = "montage".into();
    }
    let mut target = out_dir.join(format!("{base}.{}", format.ext()));
    let mut n = 2;
    while target.exists() {
        target = out_dir.join(format!("{base} ({n}).{}", format.ext()));
        n += 1;
    }
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "f32le",
        "-ar",
        &rate.max(1).to_string(),
        "-ac",
        &channels.max(1).to_string(),
        "-i",
        "pipe:0",
        "-map_metadata",
        "-1",
    ]);
    match format {
        Format::Mp3Vbr => cmd.args(["-c:a", "libmp3lame", "-q:a", "0"]),
        Format::Mp3Cbr => cmd.args(["-c:a", "libmp3lame", "-b:a", "320k"]),
        Format::Flac => cmd.args(["-c:a", "flac", "-compression_level", "8"]),
    };
    let mut child = cmd
        .arg(&target)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("lancement de ffmpeg (projet)")?;
    {
        let mut stdin = std::io::BufWriter::new(child.stdin.take().context("stdin ffmpeg")?);
        let mut buf = Vec::with_capacity(64 * 1024);
        for v in &pcm[..frames * ch] {
            buf.extend_from_slice(&v.to_le_bytes());
            if buf.len() >= 64 * 1024 {
                stdin.write_all(&buf)?;
                buf.clear();
            }
        }
        stdin.write_all(&buf)?;
        stdin.flush()?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() || !target.exists() {
        bail!(
            "ffmpeg a échoué : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let _ = tag(&target, format, tags);
    Ok(target)
}

/// Bornes d'échantillons des tranches d'une suite, alignées sur des trames
/// complètes (un demi-échantillon inverserait les canaux du reste du fichier).
/// Le **blanc numérique de tête** du rendu (36 ms sur FF8 « Force Your Way »)
/// est retiré quand l'export commence à 0 — même règle qu'à la lecture ; au
/// milieu d'une suite, on n'y touche pas.
fn segment_ranges(
    spans: &[(f64, f64)],
    fps: f64,
    ch: usize,
    total: f64,
    pcm_len: usize,
    head_samples: usize,
) -> Vec<(usize, usize)> {
    let frame_of = |s: f64| -> usize { (((s.clamp(0.0, total) * fps).round() as usize) / ch) * ch };
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        let a = frame_of(*start);
        let b = frame_of(*end).min(pcm_len);
        if b > a {
            ranges.push((a, b));
        }
    }
    if let Some((a, b)) = ranges.first_mut() {
        if *a == 0 && head_samples > 0 {
            *a = head_samples.min(*b);
        }
    }
    ranges.retain(|(a, b)| b > a);
    ranges
}

fn tag(path: &Path, format: Format, tags: &Tags) -> Result<()> {
    let mut tagged = lofty::read_from_path(path).context("lecture des tags")?;
    let tag_type = match format {
        Format::Mp3Vbr | Format::Mp3Cbr => TagType::Id3v2,
        Format::Flac => TagType::VorbisComments,
    };
    let mut tag = tagged
        .remove(tag_type)
        .unwrap_or_else(|| Tag::new(tag_type));
    if let Some(t) = &tags.title {
        tag.set_title(t.clone());
    }
    if let Some(a) = &tags.artist {
        tag.set_artist(a.clone());
    }
    if let Some(a) = &tags.album {
        tag.set_album(a.clone());
    }
    tagged.insert_tag(tag);
    tagged.save_to_path(path, WriteOptions::default())?;
    Ok(())
}

/// Nom de fichier temporaire sûr (sans virgule) dans `dir`, pour l'encodage
/// MP3 de zxtune (les options forment une map séparée par des virgules).
fn temp_target(dir: &Path, ext: &str) -> PathBuf {
    let pid = std::process::id();
    let mut n = 0u32;
    loop {
        let p = dir.join(format!(".gtracker-{pid}-{n}.{ext}"));
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    cleaned.trim().trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'export d'une suite qui commence à la première tranche ne doit pas
    /// emporter le blanc numérique de tête du rendu (vécu 21/11, FF8).
    #[test]
    fn export_saute_le_blanc_de_tete() {
        // 1000 trames/s, stéréo : fps = 2000 échantillons par seconde
        let spans = [(0.0, 2.0), (2.0, 4.0)];
        let r = segment_ranges(&spans, 2000.0, 2, 10.0, 20000, 320);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, 320, "160 trames de blanc × 2 canaux");
        assert_eq!(r[0].1, 4000);
        assert_eq!(r[1], (4000, 8000), "la suite suivante est intacte");
        // une suite qui commence plus loin garde tout son début
        let r = segment_ranges(&[(3.0, 5.0)], 2000.0, 2, 10.0, 20000, 320);
        assert_eq!(r[0].0, 6000);
        // une tranche entièrement dans le blanc ne produit rien
        let r = segment_ranges(&[(0.0, 0.1)], 2000.0, 2, 10.0, 2000, 320);
        assert!(r.is_empty(), "{r:?}");
    }

    /// Les deux qualités MP3 restent du `.mp3` (même extension, même tag
    /// ID3v2) : c'est le mode d'encodage qui change.
    #[test]
    fn qualites_mp3_et_extension() {
        assert_eq!(Format::Mp3Vbr.ext(), "mp3");
        assert_eq!(Format::Mp3Cbr.ext(), "mp3");
        assert_eq!(Format::Flac.ext(), "flac");
        assert_eq!(Format::Mp3Vbr.label(), "MP3 VBR");
        assert_eq!(Format::Mp3Cbr.label(), "MP3 320");
        assert_eq!(Format::Flac.label(), "FLAC");
    }

    /// Le fichier temporaire d'encodage n'a ni virgule ni espace : zxtune
    /// reçoit ses options en map séparée par des virgules (un titre avec
    /// « , » coupait le nom).
    #[test]
    fn temporaire_sans_virgule() {
        let dir = std::env::temp_dir().join(format!("rp-tmp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = temp_target(&dir, "mp3");
        for p in [&a] {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert!(!name.contains(','), "{name}");
            assert!(!name.contains(' '), "{name}");
        }
        std::fs::write(&a, b"x").unwrap();
        assert_ne!(temp_target(&dir, "mp3"), a, "un nom pris est évité");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
