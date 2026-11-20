//! Temps de jeu cumulé par jeu.
//!
//! Le fichier vit **avec les sauvegardes**, pas dans le cache du launcher :
//! `lgp-saves/glauncher/playtime.txt` pour les `.lgp` et
//! `LocalSavesWGP/glauncher/playtime.txt` pour les `.wgp` — mêmes racines que
//! `lgplaunch` et `gwine`, donc le temps suit les sauvegardes (et le dossier
//! Syncthing sur cette machine) au lieu de disparaître avec le cache.
//!
//! Une ligne par jeu : `slug<TAB>secondes`. Même principe que les favoris :
//! c'est la seule mémoire, un jeu momentanément absent garde son temps (dans
//! l'ancien fichier du cache, en attendant de pouvoir être rangé). Écriture
//! atomique (`.part` puis renommage) : un arrêt brutal ne doit pas laisser un
//! fichier tronqué.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Les fichiers du temps de jeu : un par format, plus l'ancien emplacement
/// dans le cache (migré au démarrage, conservé pour les jeux absents).
#[derive(Debug, Clone)]
pub struct Paths {
    pub lgp: PathBuf,
    pub wgp: PathBuf,
    /// `<cache>/playtime.txt` d'avant le déplacement.
    pub legacy: PathBuf,
}

impl Paths {
    /// Fichier du format donné (`lgp`/`wgp`).
    pub fn for_kind(&self, kind: &str) -> &Path {
        if kind == "lgp" { &self.lgp } else { &self.wgp }
    }
}

/// Charge les temps de jeu de tous les emplacements. Le plus grand total gagne
/// quand un même slug apparaît plusieurs fois (format changé, fichier copié) :
/// additionner des fichiers synchronisés compterait deux fois une partie.
pub fn load(paths: &Paths) -> HashMap<String, u64> {
    let mut times: HashMap<String, u64> = HashMap::new();
    for path in [&paths.legacy, &paths.lgp, &paths.wgp] {
        for (slug, seconds) in load_one(path) {
            times
                .entry(slug)
                .and_modify(|known| *known = (*known).max(seconds))
                .or_insert(seconds);
        }
    }
    times
}

/// Lit un fichier (absent ou illisible = table vide).
fn load_one(path: &Path) -> HashMap<String, u64> {
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

/// Ajoute `seconds` au temps de jeu de `slug` et réécrit **son** fichier : le
/// fichier de l'autre format n'est pas recopié (pas de doublons). Le total de
/// départ est le plus grand des deux (mémoire et fichier), pour ne pas repartir
/// sous un ancien total venu du cache.
pub fn add(paths: &Paths, times: &mut HashMap<String, u64>, slug: &str, kind: &str, seconds: u64) {
    let path = paths.for_kind(kind);
    let mut file = load_one(path);
    let base = file
        .get(slug)
        .copied()
        .unwrap_or(0)
        .max(times.get(slug).copied().unwrap_or(0));
    let total = base + seconds;
    file.insert(slug.to_string(), total);
    save(path, &file);
    times.insert(slug.to_string(), total);
}

/// Range l'ancien `<cache>/playtime.txt` dans les fichiers des sauvegardes,
/// quand la liste des jeux (donc le format de chaque slug) est connue. Un slug
/// absent de la liste reste dans l'ancien fichier : le jeu reviendra (disque
/// rebranché) et sera rangé à ce moment-là. Rien n'est jamais perdu.
pub fn migrate(paths: &Paths, games: &[(String, &'static str)]) {
    let legacy = load_one(&paths.legacy);
    if legacy.is_empty() {
        return;
    }
    let mut rest = legacy.clone();
    for (slug, kind) in games {
        let Some(seconds) = legacy.get(slug).copied() else {
            continue;
        };
        let path = paths.for_kind(kind);
        let mut file = load_one(path);
        if file.get(slug).copied().unwrap_or(0) < seconds {
            file.insert(slug.clone(), seconds);
            save(path, &file);
        }
        rest.remove(slug);
    }
    if rest.is_empty() {
        let _ = std::fs::remove_file(&paths.legacy);
    } else {
        save(&paths.legacy, &rest);
    }
}

/// Écrit un fichier (trié par slug, écriture temporaire + renommage).
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
    let tmp = PathBuf::from(tmp);
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

    fn paths(dir: &Path) -> Paths {
        Paths {
            lgp: dir.join("lgp-saves/glauncher/playtime.txt"),
            wgp: dir.join("LocalSavesWGP/glauncher/playtime.txt"),
            legacy: dir.join("cache/playtime.txt"),
        }
    }

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
    fn cumul_dans_le_dossier_du_format() {
        let dir = std::env::temp_dir().join(format!("glauncher-play-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = paths(&dir);

        let mut times = load(&paths);
        assert!(times.is_empty());
        add(&paths, &mut times, "celeste", "lgp", 120);
        add(&paths, &mut times, "celeste", "lgp", 180);
        add(&paths, &mut times, "hades", "wgp", 60);

        // chacun est écrit chez son format, sans recopier l'autre
        assert!(paths.lgp.is_file());
        assert!(paths.wgp.is_file());
        let lgp = load_one(&paths.lgp);
        let wgp = load_one(&paths.wgp);
        assert_eq!(lgp.get("celeste"), Some(&300));
        assert!(!lgp.contains_key("hades"));
        assert_eq!(wgp.get("hades"), Some(&60));
        assert!(!wgp.contains_key("celeste"));

        let reread = load(&paths);
        assert_eq!(reread.get("celeste"), Some(&300));
        assert_eq!(reread.get("hades"), Some(&60));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn l_ancien_fichier_est_range_par_format() {
        let dir = std::env::temp_dir().join(format!("glauncher-play-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = paths(&dir);

        let mut avant = HashMap::new();
        avant.insert("celeste".to_string(), 3600);
        avant.insert("hades".to_string(), 60);
        avant.insert("jeu-absent".to_string(), 10);
        save(&paths.legacy, &avant);

        let games = [("celeste".to_string(), "lgp"), ("hades".to_string(), "wgp")];
        migrate(&paths, &games);

        assert_eq!(load_one(&paths.lgp).get("celeste"), Some(&3600));
        assert_eq!(load_one(&paths.wgp).get("hades"), Some(&60));
        // le jeu absent n'est pas perdu : il reste dans l'ancien fichier
        assert_eq!(load_one(&paths.legacy).get("jeu-absent"), Some(&10));
        assert_eq!(load_one(&paths.legacy).len(), 1);
        // la lecture globale donne bien les trois
        let times = load(&paths);
        assert_eq!(times.get("celeste"), Some(&3600));
        assert_eq!(times.get("jeu-absent"), Some(&10));

        // tout est rangé : l'ancien fichier disparaît
        let games = [("jeu-absent".to_string(), "lgp")];
        migrate(&paths, &games);
        assert!(!paths.legacy.exists());
        assert_eq!(load_one(&paths.lgp).get("jeu-absent"), Some(&10));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn le_plus_grand_total_gagne_entre_deux_fichiers() {
        let dir = std::env::temp_dir().join(format!("glauncher-play-max-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = paths(&dir);

        let mut lgp = HashMap::new();
        lgp.insert("meme-jeu".to_string(), 900);
        save(&paths.lgp, &lgp);
        let mut wgp = HashMap::new();
        wgp.insert("meme-jeu".to_string(), 300);
        save(&paths.wgp, &wgp);

        assert_eq!(load(&paths).get("meme-jeu"), Some(&900));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
