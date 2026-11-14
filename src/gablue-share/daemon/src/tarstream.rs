//! Flux tar (ustar) généré à la volée pour les dossiers à beaucoup de petits
//! fichiers : un seul flux, pas une requête par fichier. Pas de reprise
//! possible sur un tar — c'est le compromis assumé, la file le dit.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const BLOCK: usize = 512;

struct Entry {
    name: String,
    path: PathBuf,
    size: u64,
    mtime: u64,
    dir: bool,
}

enum Phase {
    Header,
    Data,
    Pad(u64),
    Tail,
    Done,
}

pub struct TarStream {
    entries: VecDeque<Entry>,
    pending: Vec<u8>,
    pos: usize,
    current: Option<(File, u64)>,
    written: u64,
    phase: Phase,
}

impl TarStream {
    /// `base` = dossier à archiver, `prefix` = nom de ce dossier dans l'archive.
    pub fn new(base: &Path, prefix: &str) -> io::Result<Self> {
        let mut entries = VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        collect(base, prefix, 0, &mut seen, &mut entries)?;
        Ok(Self {
            entries,
            pending: Vec::new(),
            pos: 0,
            current: None,
            written: 0,
            phase: Phase::Header,
        })
    }
}

fn collect(
    base: &Path,
    name: &str,
    depth: usize,
    seen: &mut std::collections::HashSet<PathBuf>,
    out: &mut VecDeque<Entry>,
) -> io::Result<()> {
    if depth > 64 {
        return Ok(());
    }
    let canonical = base.canonicalize()?;
    if !seen.insert(canonical) {
        return Ok(()); // boucle de symlinks
    }
    let meta = std::fs::metadata(base)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if name.is_empty() {
        // Racine demandée : on descend sans créer d'entrée parasite.
        return descend(base, name, depth, seen, out);
    }

    out.push_back(Entry {
        name: if meta.is_dir() {
            format!("{name}/")
        } else {
            name.to_string()
        },
        path: base.to_path_buf(),
        size: if meta.is_dir() { 0 } else { meta.len() },
        mtime,
        dir: meta.is_dir(),
    });
    if meta.is_dir() {
        descend(base, name, depth, seen, out)?;
    }
    Ok(())
}

fn descend(
    base: &Path,
    name: &str,
    depth: usize,
    seen: &mut std::collections::HashSet<PathBuf>,
    out: &mut VecDeque<Entry>,
) -> io::Result<()> {
    let mut children: Vec<_> = std::fs::read_dir(base)?.flatten().collect();
    children.sort_by_key(|e| e.file_name());
    for child in children {
        let child_name = if name.is_empty() {
            child.file_name().to_string_lossy().to_string()
        } else {
            format!("{name}/{}", child.file_name().to_string_lossy())
        };
        // `collect` ignore les erreurs individuelles via son `?` ; une entrée
        // illisible ne doit pas tuer tout l'archive.
        let _ = collect(&child.path(), &child_name, depth + 1, seen, out);
    }
    Ok(())
}

impl Read for TarStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.pos < self.pending.len() {
                let n = (self.pending.len() - self.pos).min(out.len());
                out[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            self.pending.clear();
            self.pos = 0;

            match self.phase {
                Phase::Header => match self.entries.pop_front() {
                    Some(entry) => {
                        self.pending = header(&entry);
                        if entry.dir {
                            continue;
                        }
                        match File::open(&entry.path) {
                            Ok(file) => {
                                self.written = 0;
                                self.current = Some((file, entry.size));
                                self.phase = Phase::Data;
                            }
                            Err(_) => continue, // disparu entre-temps : sauté
                        }
                    }
                    None => {
                        self.pending = vec![0u8; 2 * BLOCK];
                        self.phase = Phase::Tail;
                    }
                },
                Phase::Data => match self.current.as_mut() {
                    Some((file, left)) if *left > 0 => {
                        let want = (*left).min(64 * 1024) as usize;
                        self.pending.resize(want, 0);
                        let n = file.read(&mut self.pending)?;
                        if n == 0 {
                            // Fichier tronqué pendant le transfert : on termine
                            // proprement (le client verra un tar tronqué).
                            self.pending.clear();
                            self.current = None;
                            let slack = (BLOCK as u64 - self.written % BLOCK as u64) % BLOCK as u64;
                            self.phase = Phase::Pad(slack);
                        } else {
                            self.pending.truncate(n);
                            self.written += n as u64;
                            *left -= n as u64;
                            if *left == 0 {
                                self.current = None;
                                let slack =
                                    (BLOCK as u64 - self.written % BLOCK as u64) % BLOCK as u64;
                                self.phase = Phase::Pad(slack);
                            }
                        }
                    }
                    _ => {
                        let slack = (BLOCK as u64 - self.written % BLOCK as u64) % BLOCK as u64;
                        self.phase = Phase::Pad(slack);
                    }
                },
                Phase::Pad(slack) => {
                    if slack == 0 {
                        self.phase = Phase::Header;
                        continue;
                    }
                    self.pending = vec![0u8; slack as usize];
                    self.phase = Phase::Header;
                }
                Phase::Tail => self.phase = Phase::Done,
                Phase::Done => return Ok(0),
            }
        }
    }
}

fn put(block: &mut [u8], offset: usize, value: &[u8]) {
    let len = value.len().min(block.len() - offset);
    block[offset..offset + len].copy_from_slice(&value[..len]);
}

fn octal(value: u64, width: usize) -> Vec<u8> {
    let text = format!("{value:0width$o}");
    let mut out = text.into_bytes();
    out.truncate(width);
    out
}

fn header(entry: &Entry) -> Vec<u8> {
    let mut block = [0u8; BLOCK];

    // ustar : nom ≤ 100, sinon préfixe (≤ 155) + nom, coupés sur un '/'.
    let (prefix, name) = split_name(&entry.name);
    put(&mut block, 0, name.as_bytes());
    if let Some(prefix) = prefix {
        put(&mut block, 345, prefix.as_bytes());
    }
    put(&mut block, 100, &octal(0o644, 7));
    put(&mut block, 107, b"\0");
    put(&mut block, 108, &octal(0, 7));
    put(&mut block, 115, b"\0");
    put(&mut block, 116, &octal(0, 7));
    put(&mut block, 123, b"\0");
    put(&mut block, 124, &octal(entry.size, 11));
    put(&mut block, 135, b"\0");
    put(&mut block, 136, &octal(entry.mtime, 11));
    put(&mut block, 147, b"\0");
    put(&mut block, 148, b"        ");
    block[156] = if entry.dir { b'5' } else { b'0' };
    put(&mut block, 257, b"ustar\0");
    put(&mut block, 263, b"00");

    let sum: u64 = block.iter().map(|b| *b as u64).sum();
    put(&mut block, 148, format!("{sum:06o}\0 ").as_bytes());
    block.to_vec()
}

/// Coupe un nom trop long en (préfixe, nom). Nom irrepresentable → tronqué.
fn split_name(name: &str) -> (Option<String>, String) {
    if name.len() <= 100 {
        return (None, name.to_string());
    }
    for (index, _) in name.match_indices('/').rev() {
        let (prefix, rest) = name.split_at(index);
        let rest = rest.trim_start_matches('/');
        if prefix.len() <= 155 && rest.len() <= 100 {
            return (Some(prefix.to_string()), rest.to_string());
        }
    }
    // Aucun découpage valable : on garde la fin, lisible.
    let start = name.len().saturating_sub(100);
    (None, name[start..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("gablue-tar-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"abc").unwrap();
        std::fs::write(root.join("sub/b.bin"), vec![7u8; 700]).unwrap();
        root
    }

    #[test]
    fn headers_carry_a_valid_checksum() {
        let entry = Entry {
            name: "x.txt".into(),
            path: PathBuf::from("/tmp/x.txt"),
            size: 3,
            mtime: 0,
            dir: false,
        };
        let block = header(&entry);
        assert_eq!(block.len(), BLOCK);
        assert_eq!(&block[257..262], b"ustar");
        // Recalcule la somme avec le champ checksum remis à huit espaces.
        let mut copy = block.clone();
        copy[148..156].copy_from_slice(b"        ");
        let sum: u64 = copy.iter().map(|b| *b as u64).sum();
        let declared = String::from_utf8_lossy(&block[148..154]).to_string();
        assert_eq!(u64::from_str_radix(declared.trim(), 8).unwrap(), sum);
    }

    #[test]
    fn streams_a_readable_archive() {
        let root = sample_dir("stream");
        let mut stream = TarStream::new(&root, "dossier").unwrap();
        let mut data = Vec::new();
        stream.read_to_end(&mut data).unwrap();
        assert_eq!(data.len() % BLOCK, 0);
        assert!(data.ends_with(&[0u8; 1024]));
        let text = String::from_utf8_lossy(&data[..2048]).to_string();
        assert!(text.contains("dossier/a.txt"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_directory_is_archived() {
        let root = std::env::temp_dir().join(format!("gablue-tar-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("vide")).unwrap();
        let mut stream = TarStream::new(&root, "racine").unwrap();
        let mut data = Vec::new();
        stream.read_to_end(&mut data).unwrap();
        assert!(data.len() >= 3 * BLOCK);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_is_never_empty_after_data() {
        // Garde-fou : un `read` ne doit pas boucler sur un tampon vide.
        let root = sample_dir("guard");
        let mut stream = TarStream::new(&root, "g").unwrap();
        let mut buf = [0u8; 1];
        let n = stream.read(&mut buf).unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
