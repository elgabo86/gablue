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

/// Une entrée de dossier, scannée une seule fois pour le listing **et** pour
/// l'empreinte (pas de double `stat` sur un dossier de plusieurs milliers).
struct Entry {
    is_dir: bool,
    name: String,
    size: u64,
    mtime: u64,
    mtime_ns: u128,
}

/// Ce qu'un partage accepte de montrer — réglé par celui qui partage, appliqué
/// par le démon (le client ne peut pas le contourner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    /// Afficher les entrées dont le nom commence par un point.
    pub hidden: bool,
    /// Exposer les sous-dossiers (navigation et recherche récursive). Faux =
    /// seuls les fichiers de la racine du partage existent.
    pub subdirs: bool,
}

impl View {
    /// Tout montrer : c'est ce que voit un téléchargement de dossier (une
    /// archive inclut les sous-dossiers cachés même si le listing les masque).
    pub fn everything() -> View {
        View {
            hidden: true,
            subdirs: true,
        }
    }

    fn keeps(&self, name: &str, is_dir: bool) -> bool {
        if !self.hidden && name.starts_with('.') {
            return false;
        }
        if !self.subdirs && is_dir {
            return false;
        }
        true
    }
}

fn scan(root: &Path, dir: &Path, view: View) -> io::Result<Vec<Entry>> {
    let mut rows = Vec::new();
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
        if !view.keeps(&name, is_dir) {
            continue;
        }
        // Symlink sortant : invisible dans le listing et refusé en GET.
        let link = std::fs::symlink_metadata(entry.path())
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if link && !entry.path().canonicalize().map(|p| p.starts_with(root)).unwrap_or(false) {
            continue;
        }
        let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
        rows.push(Entry {
            is_dir,
            name,
            size: if is_dir { 0 } else { meta.len() },
            mtime: modified.map(|d| d.as_secs()).unwrap_or(0),
            mtime_ns: modified.map(|d| d.as_nanos()).unwrap_or(0),
        });
    }
    Ok(rows)
}

/// Empreinte d'un dossier : change dès qu'une entrée apparaît, disparaît,
/// change de nom, de taille ou de date. Sert d'`ETag` au listing, ce qui permet
/// à la GUI de demander « est-ce que ça a bougé ? » avec un `HEAD` de quelques
/// octets au lieu de relire le dossier entier toutes les trois secondes.
pub fn fingerprint(root: &Path, dir: &Path, view: View) -> io::Result<String> {
    Ok(fingerprint_of(&scan(root, dir, view)?))
}

/// Ordre demandé par le client (`?sort=`) ; les dossiers restent toujours en
/// tête, comme dans le listing par défaut.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Size,
    Mtime,
}

impl Sort {
    pub fn parse(raw: &str) -> Sort {
        match raw {
            "size" => Sort::Size,
            "mtime" => Sort::Mtime,
            _ => Sort::Name,
        }
    }
}

/// Contenu d'un dossier, trié (dossiers d'abord, puis nom insensible à la casse).
/// Les entrées inaccessibles hors du partage (symlink sortant) sont omises,
/// comme elles le seraient au téléchargement. Retourne aussi l'empreinte du
/// dossier complet, pour l'`ETag` de la réponse.
pub fn list(
    root: &Path,
    dir: &Path,
    offset: usize,
    limit: usize,
    sort: Sort,
    descending: bool,
    view: View,
) -> io::Result<(Vec<Value>, bool, String)> {
    let mut rows = scan(root, dir, view)?;
    rows.sort_by(|a, b| {
        let by_kind = b.is_dir.cmp(&a.is_dir);
        if by_kind != std::cmp::Ordering::Equal {
            return by_kind;
        }
        let ordering = match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Size => a.size.cmp(&b.size).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            Sort::Mtime => a.mtime.cmp(&b.mtime).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        };
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    let tag = fingerprint_of(&rows);
    let total = rows.len();
    let start = offset.min(total);
    let end = start.saturating_add(limit).min(total);
    let page = rows[start..end]
        .iter()
        .map(|entry| {
            json!({
                "name": entry.name,
                "type": if entry.is_dir { "dir" } else { "file" },
                "size": entry.size,
                "mtime": entry.mtime,
            })
        })
        .collect();
    Ok((page, end < total, tag))
}

/// Recherche récursive par nom (sous-chaîne, insensible à la casse).
///
/// Bornée dans le temps : un partage de 200 000 fichiers ne doit pas immobiliser
/// un fil du serveur. Retourne `(résultats, tronqué, entrées examinées)` ; le
/// client sait ainsi qu'il doit affiner ou relancer.
pub fn search(
    root: &Path,
    start: &Path,
    query: &str,
    deadline: std::time::Instant,
    limit: usize,
    view: View,
) -> (Vec<Value>, bool, u64) {
    let needle = query.to_lowercase();
    let mut found: Vec<(String, Value)> = Vec::new();
    let mut scanned = 0u64;
    let mut truncated = false;
    let mut stack = vec![start.to_path_buf()];
    while let Some(current) = stack.pop() {
        if found.len() >= limit || std::time::Instant::now() > deadline {
            truncated = true;
            break;
        }
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            let path = entry.path();
            // Même vue que le listing : un partage qui masque les fichiers
            // cachés ne les propose pas davantage par la recherche.
            if !view.keeps(&name, meta.is_dir()) {
                continue;
            }
            let link = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if link && !path.canonicalize().map(|p| p.starts_with(root)).unwrap_or(false) {
                continue; // symlink sortant : jamais proposé
            }
            scanned += 1;
            if name.to_lowercase().contains(&needle) {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                found.push((
                    relative.clone(),
                    json!({
                        "name": name,
                        "path": relative,
                        "type": if meta.is_dir() { "dir" } else { "file" },
                        "size": if meta.is_dir() { 0 } else { meta.len() },
                        "mtime": mtime,
                    }),
                ));
                if found.len() >= limit {
                    truncated = true;
                    break;
                }
            }
            if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    found.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    (found.into_iter().map(|(_, row)| row).collect(), truncated, scanned)
}

fn fingerprint_of(entries: &[Entry]) -> String {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let absorb = |value: u64, hash: &mut u64| {
        *hash ^= value;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for entry in sorted {
        for byte in entry.name.as_bytes() {
            absorb(*byte as u64, &mut hash);
        }
        absorb(entry.is_dir as u64, &mut hash);
        absorb(entry.size, &mut hash);
        absorb(entry.mtime_ns as u64, &mut hash);
    }
    format!("\"{}-{hash:x}\"", entries.len())
}

/// Nombre d'entrées d'un dossier, sans tri (réponse `truncated` rapide).
pub fn count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

/// Taille récursive, bornée pour ne pas bloquer le serveur sur un arbre géant.
pub fn tree_size(dir: &Path, deadline: std::time::Instant) -> (u64, u64, bool) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut complete = true;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if std::time::Instant::now() > deadline {
            complete = false;
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
    (files, bytes, complete)
}
