//! Temps de jeu cumulé par jeu.
//!
//! Un fichier texte dans le cache (`<cache>/playtime.txt`), une ligne par jeu :
//! `slug<TAB>secondes`. Même principe que les favoris : c'est la seule mémoire,
//! un jeu momentanément absent garde son temps. Écriture atomique (`.part` puis
//! renommage) : un arrêt brutal ne doit pas laisser un fichier tronqué.

use std::collections::HashMap;
use std::path::Path;

/// Charge les temps de jeu. Un fichier absent ou illisible donne une table vide.
pub fn load(path: &Path) -> HashMap<String, u64> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut times = HashMap::new();
    for line in text.lines() {
        let Some((slug, seconds)) = line.split_once('\t') else {
            continue;
        };
        let slug = slug.trim();
        if slug.is_empty() {
            continue;
        }
        if let Ok(seconds) = seconds.trim().parse::<u64>() {
            times.insert(slug.to_string(), seconds);
        }
    }
    times
}

/// Ajoute `seconds` au temps de jeu de `slug` et réécrit le fichier.
pub fn add(path: &Path, times: &mut HashMap<String, u64>, slug: &str, seconds: u64) {
    let total = times.entry(slug.to_string()).or_insert(0);
    *total += seconds;
    save(path, times);
}

/// Écrit la table (triée par slug, écriture temporaire + renommage).
fn save(path: &Path, times: &HashMap<String, u64>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut keys: Vec<&String> = times.keys().collect();
    keys.sort();
    let mut text = String::new();
    for slug in keys {
        if let Some(seconds) = times.get(slug) {
            text.push_str(slug);
            text.push('\t');
            text.push_str(&seconds.to_string());
            text.push('\n');
        }
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Durée lisible : « 3 h 12 », « 45 min », « moins d'une minute », « jamais joué ».
pub fn format(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "jamais joué".to_string();
    };
    if seconds < 60 {
        return "moins d'une minute".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} min");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest == 0 {
        format!("{hours} h")
    } else {
        format!("{hours} h {rest:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duree_lisible() {
        assert_eq!(format(None), "jamais joué");
        assert_eq!(format(Some(0)), "moins d'une minute");
        assert_eq!(format(Some(59)), "moins d'une minute");
        assert_eq!(format(Some(60)), "1 min");
        assert_eq!(format(Some(45 * 60)), "45 min");
        assert_eq!(format(Some(3600)), "1 h");
        assert_eq!(format(Some(3600 + 12 * 60)), "1 h 12");
    }

    #[test]
    fn cumul_et_relecture() {
        let dir = std::env::temp_dir().join(format!("glauncher-play-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("playtime.txt");

        let mut times = load(&path);
        assert!(times.is_empty());
        add(&path, &mut times, "celeste", 120);
        add(&path, &mut times, "celeste", 180);
        add(&path, &mut times, "hades", 60);

        let reread = load(&path);
        assert_eq!(reread.get("celeste"), Some(&300));
        assert_eq!(reread.get("hades"), Some(&60));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
