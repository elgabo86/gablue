//! Choix des dossiers de jeux depuis le launcher, à la manette.
//!
//! Deux écrans, deux gestes différents, donc deux logiques :
//!
//! - la **liste** des dossiers surveillés (`game_dirs` de `config.toml`) :
//!   on ajoute, on retire — retirer ne touche **jamais** au disque, c'est
//!   seulement « ne plus scanner ce dossier » ;
//! - le **navigateur** de dossiers : on descend, on remonte, puis on choisit
//!   le dossier courant.
//!
//! Ce module ne connaît ni iced ni la config : il lit le disque et fabrique
//! des chemins. Tout est testable sans manette ni fenêtre.

use std::path::{Path, PathBuf};

/// Une ligne du navigateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dossier {
    pub nom: String,
    pub chemin: PathBuf,
    /// Ligne « .. » : remonter d'un niveau (jamais un vrai dossier).
    pub remonter: bool,
}

/// Libellé d'un chemin pour l'écran : `~` remplace le dossier personnel.
pub fn affiche(chemin: &Path) -> String {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return chemin.display().to_string();
    };
    match chemin.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => chemin.display().to_string(),
    }
}

/// Le chemin à parcourir : résolu (`~`, liens) et vérifié comme dossier.
pub fn resolu(chemin: &Path) -> PathBuf {
    let expanse = crate::config::expand(&chemin.to_string_lossy());
    expanse.canonicalize().unwrap_or(expanse)
}

/// Sous-dossiers de `chemin`, triés, plus la ligne « remonter ».
///
/// Les entrées cachées (`.config`, `.cache`…) sont écartées : on choisit des
/// dossiers de jeux, et les faire défiler au milieu des 200 fichiers d'un
/// `~` les rendrait introuvables. Les liens vers un dossier comptent comme
/// des dossiers ; un lien cassé est ignoré.
pub fn lister(chemin: &Path) -> Vec<Dossier> {
    let mut dossiers: Vec<Dossier> = Vec::new();
    if let Some(parent) = chemin.parent() {
        dossiers.push(Dossier {
            nom: "..".to_string(),
            chemin: parent.to_path_buf(),
            remonter: true,
        });
    }
    let Ok(entries) = std::fs::read_dir(chemin) else {
        return dossiers;
    };
    for entry in entries.map_while(Result::ok) {
        let nom = entry.file_name().to_string_lossy().to_string();
        if nom.starts_with('.') {
            continue;
        }
        let chemin_entry = entry.path();
        // `is_dir` suit les liens ; un lien cassé répond false sans erreur
        if !chemin_entry.is_dir() {
            continue;
        }
        dossiers.push(Dossier {
            nom,
            chemin: chemin_entry,
            remonter: false,
        });
    }
    // tri insensible à la casse, puis sensible (deux dossiers ne différant que
    // par la casse gardent un ordre stable)
    // `!remonter` place « .. » en tête (false avant true)
    dossiers.sort_by(|a, b| {
        (!a.remonter, a.nom.to_lowercase(), &a.nom).cmp(&(!b.remonter, b.nom.to_lowercase(), &b.nom))
    });
    dossiers
}

/// Index du dossier choisi dans la liste, après un changement de répertoire :
/// on se pose sur le dossier d'où l'on vient, comme un gestionnaire de
/// fichiers. `None` = garder la première ligne.
pub fn index_retour(entrees: &[Dossier], ancien: &Path) -> Option<usize> {
    entrees
        .iter()
        .position(|dossier| dossier.chemin == ancien && !dossier.remonter)
}

/// Ajoute un dossier à la liste. Refuse un doublon (comparaison sur le chemin
/// résolu : `~/Games` et `/var/home/gab/Games` sont le même dossier) et un
/// fichier qui n'est pas un dossier.
///
/// Le chemin est mémorisé en `~/…` quand il est sous le dossier personnel :
/// c'est la convention déjà écrite par Gablue Manager, et elle survit à un
/// changement de compte.
pub fn ajouter(liste: &mut Vec<String>, chemin: &Path) -> Result<(), String> {
    let cible = resolu(chemin);
    if !cible.is_dir() {
        return Err(format!("{} n'est pas un dossier", affiche(&cible)));
    }
    let deja = liste.iter().any(|connu| resolu(Path::new(connu)) == cible);
    if deja {
        return Err(format!("{} est déjà dans la liste", affiche(&cible)));
    }
    liste.push(affiche(&cible));
    Ok(())
}

/// Retire une entrée de la liste (par index). Rend le chemin retiré, ou
/// `None` si l'index n'existe pas. **Aucun accès disque** : le dossier et son
/// contenu restent intacts.
pub fn retirer(liste: &mut Vec<String>, index: usize) -> Option<String> {
    (index < liste.len()).then(|| liste.remove(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bac(nom: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("glauncher-folders-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn liste_trie_dossiers_et_remonte() {
        let base = bac("liste");
        for nom in ["zebre", "Alpha", "beta"] {
            std::fs::create_dir(base.join(nom)).unwrap();
        }
        std::fs::write(base.join("fichier.txt"), b"x").unwrap();
        std::fs::create_dir(base.join(".cache")).unwrap();

        let entrees = lister(&base);
        let noms: Vec<&str> = entrees.iter().map(|d| d.nom.as_str()).collect();
        assert_eq!(noms, ["..", "Alpha", "beta", "zebre"], "tri + cachés écartés");
        assert!(entrees[0].remonter);
        assert_eq!(entrees[0].chemin, base.parent().unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn racine_sans_remonter() {
        let entrees = lister(Path::new("/"));
        assert!(entrees.iter().all(|d| !d.remonter));
    }

    #[test]
    fn ajout_refuse_doublon_et_fichier() {
        let base = bac("ajout");
        let jeux = base.join("Jeux");
        std::fs::create_dir(&jeux).unwrap();
        let mut liste: Vec<String> = Vec::new();
        ajouter(&mut liste, &jeux).unwrap();
        assert_eq!(liste.len(), 1);
        assert_eq!(ajouter(&mut liste, &jeux).unwrap_err().contains("déjà"), true);
        // la même cible écrite autrement est reconnue
        let alias = base.join(".").join("Jeux");
        assert!(ajouter(&mut liste, &alias).is_err());
        let fichier = base.join("jeu.lgp");
        std::fs::write(&fichier, b"x").unwrap();
        assert!(ajouter(&mut liste, &fichier).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn retrait_ne_touche_pas_au_disque() {
        let base = bac("retrait");
        let jeux = base.join("Jeux");
        std::fs::create_dir(&jeux).unwrap();
        std::fs::write(jeux.join("jeu.lgp"), b"x").unwrap();
        let mut liste = vec![affiche(&jeux)];
        assert_eq!(retirer(&mut liste, 0).is_some_and(|p| p.contains("Jeux")), true);
        assert!(liste.is_empty());
        assert!(jeux.join("jeu.lgp").exists(), "le dossier reste sur le disque");
        assert!(retirer(&mut liste, 3).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn retour_sur_le_dossier_quitte() {
        let base = bac("retour");
        let enfant = base.join("Roms");
        std::fs::create_dir(&enfant).unwrap();
        let entrees = lister(&base);
        let index = index_retour(&entrees, &enfant).unwrap();
        assert_eq!(entrees[index].nom, "Roms");
        assert_eq!(index_retour(&entrees, Path::new("/nulle/part")), None);
        let _ = std::fs::remove_dir_all(&base);
    }
}
