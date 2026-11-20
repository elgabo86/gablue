use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::db::Db;
use crate::model::{Entry, Track};
use crate::util::{archives_dir, is_audio, is_playlist};

pub struct Archive {
    pub base: PathBuf,
    pub tracks: Vec<Track>,
    pub composer: Option<String>,
    pub album: Option<String>,
}

/// Version du marqueur d'archive : à incrémenter quand l'analyse des pistes
/// change, pour que les caches existants soient réindexés sans retéléchargement.
const CACHE_VERSION: &str = "3";

/// Télécharge et extrait l'archive si besoin, puis renvoie la liste des pistes.
pub fn ensure(
    entry: &Entry,
    db: &Db,
    on_progress: &mut dyn FnMut(f32, &str),
) -> Result<Archive> {
    let dir = archives_dir().join(entry.id.to_string());
    let base = dir.join("x");
    let marker = dir.join(".complete");
    fs::create_dir_all(&dir)?;

    // « ok » = extraction d'une version précédente : les fichiers restent bons,
    // seul l'index des pistes est à refaire. Absent = tentative interrompue.
    let marker_txt = fs::read_to_string(&marker).ok();
    let fresh = marker_txt
        .as_deref()
        .map(|s| s.trim() == CACHE_VERSION)
        .unwrap_or(false);

    if fresh && base.exists() {
        let tracks = db.load_tracks(entry.id, &base)?;
        if !tracks.is_empty() {
            let (composer, album) = read_meta(&base);
            return Ok(Archive {
                base,
                tracks,
                composer,
                album,
            });
        }
    }

    if marker_txt.is_none() && base.exists() {
        // extraction partielle d'un lancement tué : on repart de zéro
        let _ = fs::remove_dir_all(&base);
    }

    if !base.exists() {
        let archive = dir.join("archive.7z");
        if !archive.exists() {
            on_progress(0.0, "téléchargement");
            download(&entry.url(), &archive, |p| on_progress(p * 0.75, "téléchargement"))?;
        }
        on_progress(0.76, "extraction");
        fs::create_dir_all(&base)?;
        sevenz_rust2::decompress_file(&archive, &base)
            .with_context(|| format!("extraction de {}", archive.display()))?;
        let _ = fs::remove_file(&archive);
    }

    on_progress(0.95, "analyse des pistes");
    let (tracks, composer, album) = scan(&base)?;
    if tracks.is_empty() {
        bail!("aucune piste reconnue dans {}", entry.name);
    }
    db.save_tracks(entry.id, &tracks, &base)?;
    fs::write(&marker, CACHE_VERSION)?;
    on_progress(1.0, "prêt");
    Ok(Archive {
        base,
        tracks,
        composer,
        album,
    })
}

fn download(url: &str, dest: &Path, mut on_progress: impl FnMut(f32)) -> Result<()> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .user_agent("retroplayer/0.1")
        .build();
    let resp = agent.get(url).call().with_context(|| format!("GET {url}"))?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();
    let part = dest.with_extension("part");
    let mut file = File::create(&part)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        got += n as u64;
        if let Some(t) = total {
            if t > 0 {
                on_progress(got as f32 / t as f32);
            }
        }
    }
    file.flush()?;
    drop(file);
    if got == 0 {
        let _ = fs::remove_file(&part);
        bail!("téléchargement vide : {url}");
    }
    fs::rename(&part, dest)?;
    Ok(())
}

fn scan(base: &Path) -> Result<(Vec<Track>, Option<String>, Option<String>)> {
    let mut audio = Vec::new();
    let mut playlists = Vec::new();
    walk(base, &mut audio, &mut playlists)?;

    if let Some(m3u) = playlists
        .iter()
        .max_by_key(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
    {
        let text = fs::read_to_string(m3u).unwrap_or_default();
        let doc = crate::m3u::parse(&text);
        let dir = m3u.parent().unwrap_or(base);
        let mut tracks = Vec::new();
        for e in &doc.entries {
            // le .m3u référence des noms relatifs, parfois encodés en %20
            let raw = crate::m3u::unescape(&e.file);
            let path = dir.join(&raw);
            let path = if path.exists() {
                path
            } else {
                match audio
                    .iter()
                    .find(|p| p.file_name() == path.file_name())
                    .cloned()
                {
                    Some(p) => p,
                    None => continue,
                }
            };
            tracks.push(Track {
                subsong: e.subsong.unwrap_or(1),
                title: e.title.clone(),
                file: path,
                length: e.length,
                loop_start: None,
                loop_end: None,
            });
        }
        if !tracks.is_empty() {
            return Ok((tracks, doc.composer, doc.album));
        }
    }

    audio.sort();
    let tracks = audio
        .into_iter()
        .map(|p| {
            let title = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "piste".into());
            Track {
                subsong: 1,
                title,
                file: p,
                length: None,
                loop_start: None,
                loop_end: None,
            }
        })
        .collect();
    Ok((tracks, None, None))
}

fn read_meta(base: &Path) -> (Option<String>, Option<String>) {
    let mut audio = Vec::new();
    let mut playlists = Vec::new();
    if walk(base, &mut audio, &mut playlists).is_err() {
        return (None, None);
    }
    if let Some(m3u) = playlists.first() {
        if let Ok(text) = fs::read_to_string(m3u) {
            let doc = crate::m3u::parse(&text);
            return (doc.composer, doc.album);
        }
    }
    (None, None)
}

fn walk(dir: &Path, audio: &mut Vec<PathBuf>, playlists: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // copies de travail (`.xxx.__nofade.psf`) : jamais des pistes
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        if path.is_dir() {
            walk(&path, audio, playlists)?;
        } else if is_audio(&path) {
            audio.push(path);
        } else if is_playlist(&path) {
            playlists.push(path);
        }
    }
    Ok(())
}

/// Archive déjà extraite dans le cache.
#[derive(Clone, Debug)]
pub struct Cached {
    pub entry_id: i64,
    pub bytes: u64,
    pub tracks: usize,
}

/// Liste les archives présentes dans `~/.cache/retroplayer/archives`.
pub fn cached(db: &Db) -> Result<Vec<Cached>> {
    let root = archives_dir();
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(id) = entry.file_name().to_string_lossy().parse::<i64>() else {
            continue;
        };
        if !path.join(".complete").exists() {
            continue;
        }
        out.push(Cached {
            entry_id: id,
            bytes: dir_size(&path).unwrap_or(0),
            tracks: db.track_count(id).unwrap_or(0),
        });
    }
    out.sort_by_key(|c| c.entry_id);
    Ok(out)
}

/// Supprime une archive du cache (extraction + WAV décodés associés).
pub fn delete_cached(entry_id: i64, db: &Db) -> Result<u64> {
    let dir = archives_dir().join(entry_id.to_string());
    let bytes = dir_size(&dir).unwrap_or(0);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    // les échantillons décodés vivent en mémoire : ils n'ont plus de raison
    // d'y rester une fois l'archive supprimée
    crate::decode::clear_ram_cache();
    db.forget_tracks(entry_id)?;
    Ok(bytes)
}

fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    if !path.exists() {
        return Ok(0);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        total += if meta.is_dir() {
            dir_size(&entry.path())?
        } else {
            meta.len()
        };
    }
    Ok(total)
}
