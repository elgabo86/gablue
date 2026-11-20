//! Lecture des `.m3u` de joshw.info : ils portent les vrais titres de pistes.
//!
//! Format d'une ligne : `fichier.nsf::NSF,01,Titre,88,87,7`
//! (index de sous-piste, titre, puis longueur / fondu / boucles).

#[derive(Clone, Debug)]
pub struct M3uEntry {
    pub file: String,
    /// Index tel qu'écrit dans le .m3u (`01`, `$00`…), converti en numéro de sous-piste.
    pub subsong: Option<u32>,
    pub title: String,
    pub length: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct M3uDoc {
    pub composer: Option<String>,
    pub album: Option<String>,
    pub entries: Vec<M3uEntry>,
}

pub fn parse(text: &str) -> M3uDoc {
    let mut doc = M3uDoc::default();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(meta) = line.strip_prefix('#') {
            let meta = meta.trim();
            if let Some((k, v)) = meta.split_once(':') {
                let v = v.trim().to_string();
                match k.trim().to_ascii_lowercase().as_str() {
                    "composer" | "artist" | "author" => {
                        if !v.is_empty() {
                            doc.composer.get_or_insert(v);
                        }
                    }
                    "album" | "game" | "title" => {
                        if !v.is_empty() {
                            doc.album.get_or_insert(v);
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        if let Some(entry) = parse_line(line) {
            doc.entries.push(entry);
        }
    }
    doc
}

fn parse_line(line: &str) -> Option<M3uEntry> {
    let (file, info) = match line.split_once("::") {
        Some((f, i)) => (f.trim(), i.trim()),
        None => (line, ""),
    };
    if file.is_empty() {
        return None;
    }
    if info.is_empty() {
        return Some(M3uEntry {
            file: file.to_string(),
            subsong: None,
            title: file
                .rsplit('/')
                .next()
                .unwrap_or(file)
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(file)
                .to_string(),
            length: None,
        });
    }

    let fields: Vec<&str> = info.split(',').collect();
    // Longueur, fondu et boucles occupent au plus les 3 derniers champs.
    let mut end = fields.len();
    let mut taken = 0;
    while end > 2 && taken < 3 {
        let f = fields[end - 1].trim();
        if f.is_empty() || parse_len(f).is_some() {
            end -= 1;
            taken += 1;
        } else {
            break;
        }
    }
    let title = fields.get(2..end).map(|s| s.join(",")).unwrap_or_default();
    let title = if title.trim().is_empty() {
        file.rsplit('/')
            .next()
            .unwrap_or(file)
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(file)
            .to_string()
    } else {
        title.trim().to_string()
    };
    let length = fields.get(end).and_then(|s| parse_len(s)).filter(|v| *v > 0.0);

    Some(M3uEntry {
        file: file.to_string(),
        subsong: fields.get(1).and_then(|s| parse_index(s.trim())),
        title,
        length,
    })
}

/// Décode les `%20` que certains .m3u utilisent pour les noms de fichiers.
pub fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Durée : secondes (`88`), `M:SS`, `M:SS.mmm` ou `H:MM:SS.mmm` (joshw écrit
/// les deux formes selon les formats).
fn parse_len(s: &str) -> Option<f64> {
    crate::util::parse_clock(s)
}

/// `01` → 1 ; `$00` → 1 ; illisible → None.
fn parse_index(s: &str) -> Option<u32> {
    if let Some(hex) = s.strip_prefix('$') {
        return u32::from_str_radix(hex, 16).ok().map(|v| v + 1);
    }
    s.parse::<u32>().ok().filter(|v| *v > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nsf_avec_titres() {
        let doc = parse(
            "# Super Mario Bros.\n# Composer: Koji Kondo\n\
             Super Mario Bros..nsf::NSF,01,Running About,88,87,7\n\
             Super Mario Bros..nsf::NSF,13,Running About (Hurry!),57,58,7\n",
        );
        assert_eq!(doc.composer.as_deref(), Some("Koji Kondo"));
        assert_eq!(doc.entries.len(), 2);
        assert_eq!(doc.entries[0].subsong, Some(1));
        assert_eq!(doc.entries[0].length, Some(88.0));
        assert_eq!(doc.entries[1].subsong, Some(13));
        assert_eq!(doc.entries[1].title, "Running About (Hurry!)");
    }

    #[test]
    fn gbs_index_hexa() {
        let doc = parse("x.gbs::GBS,$00,BGM #01,4:43,,10\n");
        assert_eq!(doc.entries[0].subsong, Some(1));
        assert_eq!(doc.entries[0].title, "BGM #01");
        assert_eq!(doc.entries[0].length, Some(283.0)); // 4:43
    }

    #[test]
    fn duree_mm_ss_millieme() {
        let doc = parse(
            "d.nsf::NSF,1,Title Theme,0:01:55.087,,\n\
             d.nsf::NSF,5,Fever Clear,0:00:22.183,,0:00:00\n",
        );
        assert_eq!(doc.entries[0].title, "Title Theme");
        assert_eq!(doc.entries[0].length, Some(115.087));
        assert_eq!(doc.entries[1].title, "Fever Clear");
        assert_eq!(doc.entries[1].length, Some(22.183));
    }

    #[test]
    fn titre_avec_virgule() {
        let doc = parse("x.nsf::NSF,02,Bowser, Jr.,12,13,7\n");
        assert_eq!(doc.entries[0].title, "Bowser, Jr.");
        assert_eq!(doc.entries[0].length, Some(12.0));
    }
}
