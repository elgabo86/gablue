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
    /// Seuls les jeux marqués ★, dans l'ordre alphabétique.
    Favorites,
    /// Derniers jeux **lancés** d'abord (`recent.txt`), puis le reste A→Z :
    /// distinct de `Modified`, qui regarde la date du paquet, pas la partie.
    Recent,
}

impl SortBy {
    /// Les tris du menu, dans l'ordre d'affichage.
    pub const ALL: [SortBy; 4] =
        [SortBy::Name, SortBy::Modified, SortBy::Favorites, SortBy::Recent];

    /// Nom court, tel qu'écrit dans le fichier d'état.
    fn key(self) -> &'static str {
        match self {
            SortBy::Name => "nom",
            SortBy::Modified => "recents",
            SortBy::Favorites => "favoris",
            SortBy::Recent => "derniers",
        }
    }

    /// Libellé du menu et du pied de page : l'ordre courant doit se voir sans
    /// ouvrir le menu.
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Name => "A→Z",
            SortBy::Modified => "Récents",
            SortBy::Favorites => "Favoris",
            SortBy::Recent => "Derniers lancés",
        }
    }

    /// Position dans `ALL` (menu « tri »).
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&sort| sort == self).unwrap_or(0)
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "nom" => Some(SortBy::Name),
            "recents" => Some(SortBy::Modified),
            "favoris" => Some(SortBy::Favorites),
            "derniers" => Some(SortBy::Recent),
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
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "grille" => Some(ViewMode::Grid),
            "liste" => Some(ViewMode::List),
            "mosaique" => Some(ViewMode::Mosaic),
            _ => None,
        }
    }
}

/// D'où viennent les jeux affichés : l'ensemble de la bibliothèque, les seuls
/// dossiers choisis dans `config.toml`, ou les seuls dossiers trouvés sur les
/// disques externes montés (`scan_disks`). Répond à « où est ce jeu ? » quand
/// un disque branché ajoute 600 paquets à trois.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Tous,
    /// Uniquement les dossiers de `game_dirs`.
    Dossiers,
    /// Uniquement les racines trouvées sur un disque monté.
    Disques,
    /// Uniquement le partage `gablue-share` monté (`src/share.rs`).
    Partage,
}

impl Scope {
    fn key(self) -> &'static str {
        match self {
            Scope::Tous => "tous",
            Scope::Dossiers => "dossiers",
            Scope::Disques => "disques",
            Scope::Partage => "partage",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "tous" => Some(Scope::Tous),
            "dossiers" => Some(Scope::Dossiers),
            "disques" => Some(Scope::Disques),
            "partage" => Some(Scope::Partage),
            _ => None,
        }
    }

    /// Libellé du menu « Tri » : la valeur courante doit se lire sans ouvrir.
    pub fn label(self) -> &'static str {
        match self {
            Scope::Tous => "Tous les jeux",
            Scope::Dossiers => "Dossiers choisis",
            Scope::Disques => "Disques scannés",
            Scope::Partage => "Partage réseau",
        }
    }

    /// Suffixe de la pastille de tri, vide quand tout est affiché.
    pub fn short(self) -> &'static str {
        match self {
            Scope::Tous => "",
            Scope::Dossiers => "dossiers",
            Scope::Disques => "disques",
            Scope::Partage => "partage",
        }
    }

    /// Cran suivant (la ligne du menu cycle : trois valeurs, du plus large au
    /// plus étroit, puis retour).
    pub fn next(self) -> Self {
        match self {
            Scope::Tous => Scope::Dossiers,
            Scope::Dossiers => Scope::Disques,
            // `Partage` n'est PAS dans le cycle : un partage réseau s'ouvre
            // par le menu (écran à part), pas en tournant la portée de la
            // bibliothèque — sinon les jeux d'un poste se glisseraient dans
            // le parcours habituel.
            Scope::Disques => Scope::Tous,
            Scope::Partage => Scope::Tous,
        }
    }
}

/// Ce que le launcher retient. `view` absent = garder `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    pub view: Option<ViewMode>,
    pub sort: SortBy,
    /// Filtre d'année actif (`None` = toutes).
    pub year: Option<u32>,
    /// Portée de la bibliothèque (tous / dossiers choisis / disques scannés).
    pub scope: Scope,
    /// Taille des lignes de la vue LISTE (0 compact, 1 normal, 2 grand). La
    /// grille a ses propres crans (colonnes) : une seule valeur ne pouvait pas
    /// servir les deux, et la liste n'avait aucun réglage (signalé 23/11/2026).
    pub list_size: u8,
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
            "annee" => state.year = value.trim().parse().ok(),
            "portee" => {
                if let Some(scope) = Scope::parse(value) {
                    state.scope = scope;
                }
            }
            "liste_taille" => {
                state.list_size = value.trim().parse::<u8>().unwrap_or(1).min(2)
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
    if let Some(year) = state.year {
        text.push_str(&format!("annee={year}\n"));
    }
    text.push_str(&format!("liste_taille={}\n", state.list_size.min(2)));
    if state.scope != Scope::Tous {
        text.push_str(&format!("portee={}\n", state.scope.key()));
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
            year: Some(1994),
            scope: Scope::Disques,
            list_size: 2,
        };
        save(&path, state);
        assert_eq!(load(&path), state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn le_tri_favoris_survit() {
        let path = bac_sable("favoris");
        save(
            &path,
            State {
                view: Some(ViewMode::Grid),
                sort: SortBy::Favorites,
                year: None,
                scope: Scope::Tous,
                list_size: 1,
            },
        );
        assert_eq!(load(&path).sort, SortBy::Favorites);
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
