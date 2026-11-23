//! Historique des derniers jeux lancés, pour l'ordre « Derniers lancés ».
//!
//! Fichier `<cache>/recent.txt`, une ligne `slug<TAB>epoch` (secondes Unix),
//! écrit atomiquement (`.part` puis renommage) : un arrêt brutal ne doit pas
//! laisser un fichier tronqué. Il ne vit **pas** avec les sauvegardes
//! contrairement à `playtime.txt` : c'est un historique de poste, pas une
//! donnée de partie — l'effacer ne doit rien perdre du temps joué.
//!
//! Au lancement d'un jeu, son entrée passe en tête (elle est déplacée, jamais
//! dupliquée) et la liste est plafonnée : un fichier qui grossit à l'infini
//! serait relu à chaque démarrage pour rien.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Nombre d'entrées conservées (largement plus que ce qu'un écran montre).
pub const MAX: usize = 24;

/// Chemin du fichier, dans le cache du launcher.
pub fn path(cache: &Path) -> PathBuf {
    cache.join("recent.txt")
}

/// Lit l'historique, du plus récent au plus ancien.
///
/// Lignes illisibles, slugs dupliqués et epochs négatifs sont ignorés : un
/// fichier abîmé fait au pire une liste plus courte, jamais une erreur.
pub fn load(path: &Path) -> Vec<(String, u64)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries: Vec<(String, u64)> = Vec::new();
    for line in text.lines() {
        let Some((slug, epoch)) = line.split_once('\t') else {
            continue;
        };
        let slug = slug.trim();
        let Ok(epoch) = epoch.trim().parse::<u64>() else {
            continue;
        };
        if slug.is_empty() || !seen.insert(slug.to_string()) {
            continue;
        }
        entries.push((slug.to_string(), epoch));
    }
    entries.sort_by_key(|(_, epoch)| std::cmp::Reverse(*epoch));
    entries.truncate(MAX);
    entries
}

/// Note qu'un jeu vient d'être lancé (entrée déplacée en tête).
///
/// Renvoie `false` seulement si l'écriture a échoué : le lancement du jeu, lui,
/// ne dépend jamais de cet historique.
pub fn record(path: &Path, slug: &str, epoch: u64) -> bool {
    if slug.is_empty() {
        return false;
    }
    let mut entries: Vec<(String, u64)> = load(path);
    entries.retain(|(known, _)| known != slug);
    entries.insert(0, (slug.to_string(), epoch));
    entries.truncate(MAX);
    write(path, &entries)
}

/// Ne garde que les jeux encore présents (`garde`) et réécrit le fichier.
/// Renvoie le nombre d'entrées retirées. Sert à « Oublier les jeux disparus ».
pub fn retain(path: &Path, garde: &HashSet<String>) -> usize {
    let entries = load(path);
    let avant = entries.len();
    let garde_entries: Vec<(String, u64)> = entries
        .into_iter()
        .filter(|(slug, _)| garde.contains(slug))
        .collect();
    let retires = avant - garde_entries.len();
    if retires > 0 {
        write(path, &garde_entries);
    }
    retires
}

/// Vide l'historique. Le fichier disparaît : « aucun jeu lancé » et un fichier
/// absent sont le même état.
pub fn clear(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Écriture atomique de la liste complète.
fn write(path: &Path, entries: &[(String, u64)]) -> bool {
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    let mut text = String::new();
    for (slug, epoch) in entries {
        text.push_str(slug);
        text.push('\t');
        text.push_str(&epoch.to_string());
        text.push('\n');
    }
    let part = path.with_extension("part");
    if std::fs::write(&part, text).is_err() {
        return false;
    }
    std::fs::rename(&part, path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glauncher-recent-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("recent.txt")
    }

    #[test]
    fn record_met_en_tete_sans_doublon() {
        let path = bac("ordre");
        assert!(record(&path, "mario", 100));
        assert!(record(&path, "zelda", 200));
        assert!(record(&path, "mario", 300));
        let list = load(&path);
        assert_eq!(
            list,
            vec![("mario".to_string(), 300), ("zelda".to_string(), 200)]
        );
    }

    #[test]
    fn clear_rend_la_liste_vide() {
        let path = bac("clear");
        assert!(record(&path, "mario", 100));
        assert!(clear(&path));
        assert!(load(&path).is_empty());
        // effacer deux fois n'est pas une erreur
        assert!(clear(&path));
    }

    #[test]
    fn retain_ne_garde_que_les_jeux_presents() {
        let path = bac("retain");
        assert!(record(&path, "mario", 100));
        assert!(record(&path, "zelda", 200));
        assert!(record(&path, "metroid", 300));
        let garde: HashSet<String> = ["mario".to_string(), "metroid".to_string()].into();
        assert_eq!(retain(&path, &garde), 1);
        assert_eq!(
            load(&path),
            vec![("metroid".to_string(), 300), ("mario".to_string(), 100)]
        );
        // plus rien à retirer : aucun fichier réécrit
        assert_eq!(retain(&path, &garde), 0);
    }

    #[test]
    fn fichier_abime_ne_casse_rien() {
        let path = bac("abime");
        std::fs::write(&path, "pas de tabulation\nmario\tabc\nzelda\t42\nzelda\t10\n").unwrap();
        assert_eq!(load(&path), vec![("zelda".to_string(), 42)]);
    }
}
