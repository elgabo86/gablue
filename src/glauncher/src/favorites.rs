//! Liste des favoris : un slug par ligne, dans le cache
//! (`~/.local/share/glauncher/favorites.txt`).
//!
//! Le fichier est volontairement lisible et stable (trié) : il survit aux
//! réindexations, à la disparition d'un paquet (le slug reste, le jeu revient
//! avec son étoile) et se modifie à la main sans casser le launcher.

use std::collections::HashSet;
use std::path::Path;

use crate::library::Game;

/// Charge la liste (fichier absent = aucun favori).
pub fn load(path: &Path) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Écrit la liste triée ; une erreur d'écriture n'interrompt pas le launcher
/// (les favoris de la session restent en mémoire).
pub fn save(path: &Path, slugs: &HashSet<String>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut list: Vec<&String> = slugs.iter().collect();
    list.sort();
    let mut body = String::with_capacity(list.len() * 32);
    for slug in list {
        body.push_str(slug);
        body.push('\n');
    }
    // écriture temporaire + rename : un arrêt en cours d'écriture ne laisse
    // pas un fichier tronqué
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|err| format!("favoris : {err}"))?;
    std::fs::rename(&tmp, path).map_err(|err| format!("favoris : {err}"))
}

/// Bascule l'étoile d'un jeu, écrit le fichier, et renvoie le nouvel état.
pub fn toggle(path: &Path, slugs: &mut HashSet<String>, game: &Game) -> bool {
    let added = if slugs.remove(&game.slug) {
        false
    } else {
        slugs.insert(game.slug.clone());
        true
    };
    if let Err(error) = save(path, slugs) {
        eprintln!("glauncher : {error}");
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_du_fichier() {
        let dir = std::env::temp_dir().join(format!("glauncher-fav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("favorites.txt");

        assert!(load(&path).is_empty(), "fichier absent = aucun favori");

        let mut set = HashSet::from(["zelda".to_string(), "another-world".to_string()]);
        save(&path, &set).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "another-world\nzelda\n",
            "liste triée, un slug par ligne"
        );
        assert_eq!(load(&path), set);

        assert!(set.remove("zelda"));
        save(&path, &set).unwrap();
        assert_eq!(load(&path), set);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
