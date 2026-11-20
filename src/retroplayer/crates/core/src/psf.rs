//! Tags PSF/PSF2 : bloc `[TAG]` en fin de fichier, `clé=valeur` en UTF-8.
//!
//! `length` (durée rendue, fondu compris) et `fade` (fondu final, inclus dans
//! `length`) sont les deux clés utiles : zxtune applique le fondu *dans* la
//! durée rendue, donc une boucle naïve rejouerait le fondu à chaque tour.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Formats PSF (le bloc `[TAG]` n'existe que là).
fn is_psf_like(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("psf") | Some("minipsf") | Some("psf2") | Some("minipsf2")
    )
}

/// Bloc `[TAG]` : toujours en fin de fichier, on ne lit que la queue.
const TAIL: u64 = 256 * 1024;

fn read_tail(path: &Path) -> Option<(Vec<u8>, usize)> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let skip = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(skip)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some((buf, skip as usize))
}

/// Toutes les paires `clé=valeur` du bloc `[TAG]`.
pub fn tags(path: &Path) -> Vec<(String, String)> {
    if !is_psf_like(path) {
        return Vec::new();
    }
    let Some((data, _)) = read_tail(path) else {
        return Vec::new();
    };
    let Some(pos) = find_last(&data, b"[TAG]") else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&data[pos + 5..]);
    let mut out = Vec::new();
    for line in text.lines() {
        // le bloc commence juste après « [TAG] » : première ligne vide à sauter,
        // et certaines archives laissent des octets nuls en fin de fichier
        let line = line.trim_matches(['\r', '\0']).trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    out
}

pub fn value(path: &Path, key: &str) -> Option<String> {
    tags(path)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// `(length, fade)` en secondes, quand le rip les déclare.
pub fn length_fade(path: &Path) -> Option<(f64, f64)> {
    let tags = tags(path);
    let get = |key: &str| {
        tags.iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| crate::util::parse_clock(v))
    };
    let length = get("length")?;
    if length <= 0.0 {
        return None;
    }
    Some((length, get("fade").unwrap_or(0.0)))
}

/// Écrit à côté du fichier une copie dont le tag `fade` vaut 0, pour que
/// zxtune rende la piste entière sans fondu final (le fondu est appliqué *dans*
/// la durée rendue, donc incompatible avec une boucle). La copie doit rester
/// dans le même dossier : `_lib=` est un chemin relatif. À supprimer après usage.
pub fn no_fade_copy(path: &Path) -> Option<std::path::PathBuf> {
    if !is_psf_like(path) {
        return None;
    }
    let (length, fade) = length_fade(path)?;
    if fade <= 0.05 || fade >= length {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    let pos = find_last(&data, b"[TAG]")?;
    let text = String::from_utf8_lossy(&data[pos + 5..]).to_string();
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_matches(['\r', '\0']).trim();
        if line.is_empty() {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("fade=") {
            lines.push("fade=0".to_string());
        } else {
            lines.push(line.to_string());
        }
    }
    let mut out = data[..pos + 5].to_vec();
    out.extend_from_slice(b"\n");
    out.extend_from_slice(lines.join("\n").as_bytes());
    out.extend_from_slice(b"\n");

    let parent = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "psf".into());
    // nom caché : `walk()` ignore les fichiers commençant par un point
    let copy = parent.join(format!(".{stem}.__nofade.{ext}"));
    std::fs::write(&copy, &out).ok()?;
    Some(copy)
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloc_tag() {
        let dir = std::env::temp_dir().join("retroplayer-psf-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.psf");
        let mut data = b"PSF\x01 donnes".to_vec();
        data.extend_from_slice(b"\n[TAG]\nlength=95\nfade=7\ntitle=Caverne\n");
        std::fs::write(&p, &data).unwrap();
        assert_eq!(length_fade(&p), Some((95.0, 7.0)));
        assert_eq!(value(&p, "title").as_deref(), Some("Caverne"));
        let _ = std::fs::remove_file(&p);
    }
}
