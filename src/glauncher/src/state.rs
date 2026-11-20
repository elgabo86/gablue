//! État d'interface retenu d'une session à l'autre.
//!
//! Le type de vue et l'ordre de tri ne vivent pas dans `config.toml` : ce
//! fichier appartient à `gablue-manager` (qui écrit les dossiers de jeux) et le
//! launcher ne l'écrit jamais. Un petit fichier texte dans le cache
//! (`<cache>/state.txt`, `clé=valeur` par ligne) garde donc le dernier choix de
//! l'utilisateur ; la valeur de `config.toml` ne sert que de valeur par défaut
//! au premier lancement. Écriture atomique (`.part` + renommage), comme les
//! favoris : un arrêt brutal ne doit pas laisser un fichier tronqué.

use std::path::Path;

use crate::config::ViewMode;

/// Ordre d'affichage de la bibliothèque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    /// A → Z (insensible à la casse).
    #[default]
    Name,
    /// Paquets modifiés le plus récemment d'abord.
    Modified,
}

impl SortBy {
    /// Nom court, tel qu'écrit dans le fichier d'état.
    fn key(self) -> &'static str {
        match self {
            SortBy::Name => "nom",
            SortBy::Modified => "recents",
        }
    }

    /// Libellé du pied de page : l'ordre courant doit se voir sans menu.
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Name => "A→Z",
            SortBy::Modified => "Récents",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "nom" => Some(SortBy::Name),
            "recents" => Some(SortBy::Modified),
            _ => None,
        }
    }
}

impl ViewMode {
    /// Nom court, tel qu'écrit dans le fichier d'état.
    fn key(self) -> &'static str {
        match self {
            ViewMode::Grid => "grille",
            ViewMode::List => "liste",
            ViewMode::Mosaic => "mosaique",
            ViewMode::Favorites => "favoris",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "grille" => Some(ViewMode::Grid),
            "liste" => Some(ViewMode::List),
            "mosaique" => Some(ViewMode::Mosaic),
            "favoris" => Some(ViewMode::Favorites),
            _ => None,
        }
    }
}

/// Ce que le launcher retient. `view` absent = garder `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    pub view: Option<ViewMode>,
    pub sort: SortBy,
}

/// Lit le fichier d'état. Absent ou illisible → valeurs par défaut (aucune
/// erreur : un état perdu n'est pas un problème, on repart de `config.toml`).
pub fn load(path: &Path) -> State {
    let Ok(text) = std::fs::read_to_string(path) else {
        return State::default();
    };
    let mut state = State::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "vue" => state.view = ViewMode::parse(value),
            "tri" => {
                if let Some(sort) = SortBy::parse(value) {
                    state.sort = sort;
                }
            }
            _ => {}
        }
    }
    state
}

/// Écrit l'état (temporaire + renommage).
pub fn save(path: &Path, state: State) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut text = String::new();
    if let Some(view) = state.view {
        text.push_str(&format!("vue={}\n", view.key()));
    }
    text.push_str(&format!("tri={}\n", state.sort.key()));

    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bac_sable(nom: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("glauncher-state-{}-{nom}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("state.txt")
    }

    #[test]
    fn aller_retour() {
        let path = bac_sable("retour");
        let mut state = load(&path);
        assert_eq!(state, State::default(), "fichier absent = défauts");

        state = State {
            view: Some(ViewMode::Mosaic),
            sort: SortBy::Modified,
        };
        save(&path, state);
        assert_eq!(load(&path), state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fichier_abime_ignore() {
        let path = bac_sable("abime");
        std::fs::write(&path, "n'importe quoi\nvue=mosaique\ntri=???\n").unwrap();
        let state = load(&path);
        assert_eq!(state.view, Some(ViewMode::Mosaic));
        assert_eq!(state.sort, SortBy::Name, "tri inconnu = défaut");
        let _ = std::fs::remove_file(&path);
    }
}
