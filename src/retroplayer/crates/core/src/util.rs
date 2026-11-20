use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Racine du cache, alignée sur l'ancienne application : `~/.cache/retroplayer`.
pub fn cache_root() -> PathBuf {
    if let Ok(p) = std::env::var("RETROPLAYER_CACHE") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".cache").join("retroplayer")
}

pub fn db_path() -> PathBuf {
    cache_root().join("retroplayer.db")
}

pub fn archives_dir() -> PathBuf {
    cache_root().join("archives")
}

pub fn decoded_dir() -> PathBuf {
    cache_root().join("decoded")
}

pub fn hash_path(path: &Path, extra: u64) -> String {
    let mut h = DefaultHasher::new();
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .hash(&mut h);
    if let Ok(md) = std::fs::metadata(path) {
        md.len().hash(&mut h);
        if let Ok(t) = md.modified() {
            if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                d.as_secs().hash(&mut h);
            }
        }
    }
    extra.hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Durée écrite par zxtune/joshw : `88`, `M:SS`, `M:SS.mmm`, `H:MM:SS.mmm`.
pub fn parse_clock(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut total = 0.0;
    for (i, part) in parts.iter().enumerate() {
        let v: f64 = part.trim().parse().ok()?;
        if v < 0.0 {
            return None;
        }
        total += v * 60f64.powi(parts.len() as i32 - 1 - i as i32);
    }
    Some(total)
}

/// Durée lisible : `1:55`, ou `1:02:03` au-delà d'une heure.
pub fn fmt_time(s: f64) -> String {
    let s = s.max(0.0);
    let total = s as i64;
    let (h, m, sec) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// Durée au centième, pour les bornes de boucle A/B (précision 10 ms,
/// largement suffisante à l'écran ; la valeur stockée reste à l'échantillon).
pub fn fmt_time_cs(s: f64) -> String {
    let s = s.max(0.0);
    let total = s as i64;
    let (h, m, sec) = (total / 3600, (total % 3600) / 60, total % 60);
    let cs = ((s - total as f64) * 100.0).round() as u32;
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}.{cs:02}")
    } else {
        format!("{m}:{sec:02}.{cs:02}")
    }
}

const AUDIO_EXTS: &[&str] = &[
    "nsf", "nsfe", "spc", "usf", "miniusf", "smd", "psf", "minipsf", "psf2", "minipsf2", "psf3",
    "psf4", "gbs", "gsf", "minigsf", "2sf", "min2sf", "3sf", "dsf", "minidsf", "ssf", "minissf",
    "xbox", "x360", "psp", "vita", "pc", "3do", "vgm", "vgz", "sid", "ay", "aym", "hes", "kss",
    "sap", "gym", "mod", "xm", "it", "s3m", "mptm", "umx", "mtm", "v2m", "ftc", "stc", "stp",
    "pt3", "vtx", "ym", "s98", "kssx", "nsd", "sqd", "mus", "hvl", "ahx", "cop", "et1", "fc",
    "fc13", "fc14", "tf0", "tfe", "tf1", "tfc", "tfd", "afc", "psm", "dmm", "pt2", "pt1",
];

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_playlist(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(ref e) if e == "m3u" || e == "m3u8"
    )
}

pub fn is_vgm_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(ref e) if e == "vgm" || e == "vgz"
    )
}

/// Nom de jeu nettoyé : titre seul, date et marqueurs extraits à part.
#[derive(Clone, Debug, Default)]
pub struct CleanName {
    pub title: String,
    /// `AAAA-MM-JJ` (les champs absents valent `00`).
    pub date: Option<String>,
    /// `Prototype`, `Unreleased Alpha`, `Homebrew`…
    pub tags: Vec<String>,
}

/// Retire `.7z`, les balises de plateforme `[SNES]` et le bloc final
/// `(date)(dev)(éditeur)` des noms joshw.info.
pub fn clean_name(raw: &str) -> CleanName {
    let mut out = CleanName::default();
    // les vieilles entrées de base gardent les entités HTML du site (« AD&amp;D »)
    let mut s = decode_entities(raw)
        .trim()
        .trim_end_matches(".7z")
        .trim()
        .to_string();

    while let Some((i, inner)) = trailing_group(&s, '[', ']') {
        if !is_platform_tag(inner) {
            out.tags.insert(0, inner.to_string());
        }
        s.truncate(i);
        s = s.trim_end().to_string();
    }

    // Remonte les groupes finaux jusqu'à la date ; on ne coupe que si une date
    // est trouvée (sinon « Titre (sous-titre) » serait amputé).
    let mut cut = None;
    let mut probe = s.clone();
    for _ in 0..7 {
        match trailing_group(&probe, '(', ')') {
            Some((i, inner)) => {
                if let Some(d) = parse_date(inner) {
                    out.date = Some(d);
                    cut = Some(i);
                    break;
                }
                probe.truncate(i);
                probe = probe.trim_end().to_string();
            }
            None => break,
        }
    }
    if let Some(i) = cut {
        s.truncate(i);
        s = s.trim_end().to_string();
    }
    // dates en double, ex. « Titre (2002)(2015)(Éditeur) »
    loop {
        match trailing_group(&s, '(', ')') {
            Some((i, inner)) if parse_date(inner).is_some() => {
                out.date = parse_date(inner);
                s.truncate(i);
                s = s.trim_end().to_string();
            }
            _ => break,
        }
    }

    // Marqueurs restés en fin de titre : (Prototype), (Beta), (Homebrew)…
    loop {
        match trailing_group(&s, '(', ')') {
            Some((i, inner)) if is_marker(inner) => {
                out.tags.insert(0, inner.to_string());
                s.truncate(i);
                s = s.trim_end().to_string();
            }
            _ => break,
        }
    }

    out.title = if s.is_empty() {
        raw.trim().trim_end_matches(".7z").to_string()
    } else {
        s
    };
    out
}

fn trailing_group(s: &str, open: char, close: char) -> Option<(usize, &str)> {
    let t = s.trim_end();
    if !t.ends_with(close) {
        return None;
    }
    let mut depth = 0usize;
    let mut found = None;
    for (i, c) in t.char_indices().rev() {
        if c == close {
            depth += 1;
        } else if c == open {
            depth -= 1;
            if depth == 0 {
                found = Some(i);
                break;
            }
        }
    }
    let i = found?;
    Some((i, &t[i + open.len_utf8()..t.len() - close.len_utf8()]))
}

fn is_platform_tag(s: &str) -> bool {
    const KNOWN: &[&str] = &[
        "NES", "SNES", "N64", "GEN", "PS1", "PS2", "PS3", "PS4", "GB", "GBC", "GBA", "NDS",
        "3DS", "DC", "Wii", "WiiU", "GC", "GCN", "SAT", "XBX", "X360", "PSP", "VITA", "PC",
        "3DO", "Switch", "SGB", "FDS", "XBLA", "PSN", "VC", "PSV", "MSX", "C64", "ZX", "AMI",
        "TG16", "PCE", "NGP", "WS", "VB", "CDI", "JAG", "32X", "SCD", "MCD", "NES2",
    ];
    let t = s.trim();
    KNOWN.iter().any(|k| k.eq_ignore_ascii_case(t))
}

fn is_marker(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "prototype", "proto", "beta", "alpha", "demo", "sample", "unreleased", "homebrew",
        "unlicensed", "hack", "translation", "pirate", "bootleg", "soundtrack", "ost", "arrange",
    ];
    PREFIXES.iter().any(|p| t.starts_with(p))
}

fn parse_date(s: &str) -> Option<String> {
    let t = s.trim();
    let parts: Vec<&str> = t.split('-').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let y = parts[0];
    if y.len() != 4 || !y.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut out = y.to_string();
    for p in parts.iter().skip(1) {
        if p.is_empty() || p.len() > 2 || !p.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out.push('-');
        out.push_str(&format!("{:0>2}", p));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(raw: &str) -> String {
        clean_name(raw).title
    }

    #[test]
    fn noms_joshw() {
        assert_eq!(
            t("Super Mario Bros. (1985-09-13)(Nintendo EAD)(Nintendo)[NES].7z"),
            "Super Mario Bros."
        );
        assert_eq!(
            t("NBA Jam - Tournament Edition (1995-02-23)(Midway)(Iguana UK)(Acclaim)[SNES]"),
            "NBA Jam - Tournament Edition"
        );
        assert_eq!(
            t("Imperium [Kidou Soukou Dion] (1992-11)(Jorudan)(VIC Tokai)[SNES]"),
            "Imperium [Kidou Soukou Dion]"
        );
        assert_eq!(
            t("My Virtual Tutor - Reading Pre-K to Kindergarten (conv) (2009-09-15)(1st Playable Productions)(Mentor)[NDS]"),
            "My Virtual Tutor - Reading Pre-K to Kindergarten (conv)"
        );
        assert_eq!(
            t("Super Mario World - 'the (Coronation Day) (Homebrew)(2016-01-05)(Medic, Torchkas, underway)[SNES]"),
            "Super Mario World - 'the (Coronation Day)"
        );
        assert_eq!(t("Alone in the Dark (1992)(Infogrames)[PC]"), "Alone in the Dark");
        // sans date : on ne coupe pas un éventuel sous-titre
        assert_eq!(t("Foo (Bar) (Baz)"), "Foo (Bar) (Baz)");
        assert_eq!(
            t("Jazz Jackrabbit 3 (Unreleased Alpha) (1999)(Epic MegaGames)[PC]"),
            "Jazz Jackrabbit 3"
        );
        assert_eq!(
            t("Medal of Honor - Allied Assault (2002)(2015)(Electronic Arts)[PC]"),
            "Medal of Honor - Allied Assault"
        );
    }

    #[test]
    fn date_et_marqueurs() {
        let c = clean_name("Jazz Jackrabbit 3 (Unreleased Alpha) (1999)(Epic MegaGames)[PC]");
        assert_eq!(c.date.as_deref(), Some("1999"));
        assert_eq!(c.tags, vec!["Unreleased Alpha".to_string()]);
    }
}
