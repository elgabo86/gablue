//! Système de fichiers : décodage d'URL, confinement des chemins, plages
//! HTTP, listing et ETag.

use serde_json::{json, Value};
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Décode `%XX`. `+` reste `+` (sémantique de chemin, pas de formulaire).
pub fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let high = hex(bytes[i + 1])?;
                let low = hex(bytes[i + 2])?;
                out.push(high * 16 + low);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Encode pour `filename*=UTF-8''…` (RFC 5987).
pub fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// Découpe un chemin d'URL en segments sûrs. `None` = tentative d'évasion.
pub fn split_path(path: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for raw in path.split('/') {
        if raw.is_empty() {
            continue;
        }
        let segment = percent_decode(raw)?;
        if segment == "." {
            continue;
        }
        // `..` est refusé, y compris encodé : on ne normalise jamais, on rejette.
        if segment == ".." || segment.contains('/') || segment.contains('\0') {
            return None;
        }
        out.push(segment);
    }
    Some(out)
}

/// Résout `segments` sous `root` et vérifie que le résultat reste dedans.
/// `root` doit être canonique (voir `config::load`).
pub fn resolve(root: &Path, segments: &[String]) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    let real = path.canonicalize().ok()?;
    if real.starts_with(root) {
        Some(real)
    } else {
        None
    }
}

/// ETag stable et gratuit : mtime en nanosecondes + taille.
pub fn etag(meta: &Metadata) -> String {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("\"{nanos}-{}\"", meta.len())
}

#[derive(Debug, PartialEq, Eq)]
pub enum RangeSpec {
    Full,
    /// Bornes incluses.
    Bytes { start: u64, end: u64 },
}

/// Analyse un `Range` mono-plage. Plusieurs plages = réponse complète
/// (licite : le client retombe sur un téléchargement entier).
/// `Err` = 416.
pub fn parse_range(value: &str, size: u64) -> Result<RangeSpec, ()> {
    if size == 0 {
        return Ok(RangeSpec::Full);
    }
    let list = value.trim().strip_prefix("bytes=").ok_or(())?;
    if list.contains(',') {
        return Ok(RangeSpec::Full);
    }
    let (first, last) = list.split_once('-').ok_or(())?;
    let first = first.trim();
    let last = last.trim();
    if first.is_empty() {
        let wanted: u64 = last.parse().map_err(|_| ())?;
        if wanted == 0 {
            return Err(());
        }
        let wanted = wanted.min(size);
        return Ok(RangeSpec::Bytes {
            start: size - wanted,
            end: size - 1,
        });
    }
    let start: u64 = first.parse().map_err(|_| ())?;
    if start >= size {
        return Err(());
    }
    let end = if last.is_empty() {
        size - 1
    } else {
        last.parse::<u64>().map_err(|_| ())?.min(size - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(RangeSpec::Bytes { start, end })
}

/// Contenu d'un dossier, trié (dossiers d'abord, puis nom insensible à la casse).
/// Les entrées inaccessibles hors du partage (symlink sortant) sont omises,
/// comme elles le seraient au téléchargement.
pub fn list(
    root: &Path,
    dir: &Path,
    offset: usize,
    limit: usize,
) -> io::Result<(Vec<Value>, bool)> {
    let mut rows: Vec<(bool, String, Value)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        // Symlink sortant : invisible dans le listing et refusé en GET.
        let link = std::fs::symlink_metadata(entry.path())
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if link && !entry.path().canonicalize().map(|p| p.starts_with(root)).unwrap_or(false) {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        rows.push((is_dir, name.clone(), json!({
            "name": name,
            "type": if is_dir { "dir" } else { "file" },
            "size": if is_dir { 0 } else { meta.len() },
            "mtime": mtime,
        })));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let total = rows.len();
    let start = offset.min(total);
    let end = start.saturating_add(limit).min(total);
    let page = rows
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|(_, _, row)| row)
        .collect();
    Ok((page, end < total))
}

/// Nombre d'entrées d'un dossier, sans tri (réponse `truncated` rapide).
pub fn count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

/// Taille récursive, bornée pour ne pas bloquer le serveur sur un arbre géant.
pub fn tree_size(dir: &Path, deadline: std::time::Instant) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if std::time::Instant::now() > deadline {
            break;
        }
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}
