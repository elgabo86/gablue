//! En-tête `.spc` (SNES Sound Player) : le ripper y écrit la **boucle
//! officielle**.
//!
//! Le tag ID666 texte porte trois choses qui nous intéressent : « seconds until
//! loop » (3 chiffres à 0xA9) et la longueur du fondu (5 chiffres à 0xAC, en
//! ms). zxtune rend exactement une boucle puis le fondu (vérifié : 595 s de
//! boucle + 10 s de fondu donnent un WAV de 605 s), donc la fenêtre de boucle
//! est `(0, loop)` — rien à deviner à l'oreille.
//!
//! 78 % des `.spc` de joshw.info portent ce champ (300 sur 386 mesurés le
//! 21/11/2026) ; les autres retombent sur la durée du `.m3u` et la détection
//! d'enveloppe (`loopdetect`).

use std::path::Path;

/// Signature des 27 premiers octets d'un dump SPC.
const MAGIC: &[u8] = b"SNES-SPC700 Sound File Data";
/// Début du tag ID666 texte.
const TAG: usize = 0x2E;
/// « seconds until loop » dans le tag.
const LOOP_AT: usize = TAG + 123;
/// Longueur du fondu (ms) dans le tag.
const FADE_AT: usize = TAG + 126;

/// Fenêtre de boucle `(début, fin)` en secondes, telle que le fichier la
/// déclare. `None` si le fichier n'est pas un `.spc` ou si le ripper n'a pas
/// rempli le champ.
pub fn loop_window(path: &Path) -> Option<(f64, f64)> {
    if !path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("spc"))
        .unwrap_or(false)
    {
        return None;
    }
    // le tag tient dans les 0xD8 premiers octets : pas besoin de lire 64 Ko
    let mut head = [0u8; FADE_AT + 5];
    let mut file = std::fs::File::open(path).ok()?;
    use std::io::Read as _;
    file.read_exact(&mut head).ok()?;
    if &head[..MAGIC.len()] != MAGIC {
        return None;
    }
    // les rippers bourrent ces champs de NUL **ou** d'espaces : les deux se
    // retirent avant de lire le nombre (vécu 21/11 : « 5\0\0 » ne se parsait
    // pas et la boucle officielle était ignorée sur ces pistes)
    let digits = |at: usize, len: usize| -> Option<u32> {
        let raw = std::str::from_utf8(&head[at..at + len]).ok()?;
        let s = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        if s.is_empty() {
            return None;
        }
        s.parse::<u32>().ok()
    };
    let loop_secs = digits(LOOP_AT, 3)? as f64;
    if loop_secs < 2.0 {
        return None;
    }
    // le fondu n'est pas une partie musicale : il reste hors de la boucle
    let _fade_ms = digits(FADE_AT, 5).unwrap_or(0);
    Some((0.0, loop_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// En-tête SPC minimal : signature, drapeau ID666, tag texte.
    fn dump(loop_secs: &str, fade: &str) -> Vec<u8> {
        let mut d = vec![0u8; FADE_AT + 5];
        d[..MAGIC.len()].copy_from_slice(MAGIC);
        d[0x23] = 0x1A; // tag ID666 présent
        d[TAG..TAG + 32].copy_from_slice(b"Woody Land (Day)\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
        d[LOOP_AT..LOOP_AT + 3].copy_from_slice(loop_secs.as_bytes());
        d[FADE_AT..FADE_AT + 5].copy_from_slice(fade.as_bytes());
        d
    }

    #[test]
    fn boucle_id666() {
        let dir = std::env::temp_dir().join("gtracker-spc-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.spc");
        std::fs::write(&p, dump("595", "10000")).unwrap();
        assert_eq!(loop_window(&p), Some((0.0, 595.0)));
        // champ vide (ripper sans boucle) : rien à proposer
        std::fs::write(&p, dump("  0", "10000")).unwrap();
        assert_eq!(loop_window(&p), None);
        std::fs::write(&p, dump("   ", "10000")).unwrap();
        assert_eq!(loop_window(&p), None);
        // champs bourrés de NUL : la forme réelle des rips joshw
        std::fs::write(&p, dump("5\0\0", "5000\0")).unwrap();
        assert_eq!(loop_window(&p), Some((0.0, 5.0)));
        // pas un SPC : aucun risque de lire un autre format
        let q = dir.join("t.nsf");
        std::fs::write(&q, dump("595", "10000")).unwrap();
        assert_eq!(loop_window(&q), None);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&q);
    }
}
