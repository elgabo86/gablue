use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::Accessor;
use lofty::tag::{Tag, TagType};

use crate::model::Track;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Mp3,
    Flac,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Mp3 => "mp3",
            Format::Flac => "flac",
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

    let flag = match format {
        Format::Mp3 => "--mp3",
        Format::Flac => "--flac",
    };
    let source = if track.subsong > 1 {
        format!("{}?#{}", track.file.display(), track.subsong)
    } else {
        track.file.display().to_string()
    };
    let stderr = crate::decode::run_zxtune(
        zxtune,
        &[flag, &format!("filename={}", target.display()), "--quiet", &source],
    )?;
    if !target.exists() {
        bail!(
            "export {} impossible : {}",
            format.ext(),
            stderr.trim()
        );
    }
    let _ = tag(&target, format, tags);
    Ok(target)
}

fn tag(path: &Path, format: Format, tags: &Tags) -> Result<()> {
    let mut tagged = lofty::read_from_path(path).context("lecture des tags")?;
    let tag_type = match format {
        Format::Mp3 => TagType::Id3v2,
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
