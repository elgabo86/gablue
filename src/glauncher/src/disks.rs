//! Disques externes montés : où chercher les jeux, sans jamais tout parcourir.
//!
//! Le launcher ne scanne que ce que `config.toml` nomme (`game_dirs`). Quand
//! `scan_disks` est actif, il ajoute les disques montés sous
//! `/run/media/<utilisateur>/` (udisks : Plasma **et** le service de session
//! jeu montent là) et `/media/<utilisateur>/`.
//!
//! La découverte est **bornée** : profondeur [`PROFONDEUR`] et au plus
//! [`BUDGET`] dossiers visités par disque. Un disque de 170 000 fichiers n'est
//! donc jamais parcouru en entier pour trois paquets. Le résultat est mémorisé
//! dans `<cache>/disks.txt` (`<montage>\t<racine>`) : le premier branchement
//! coûte une seconde ou deux, les suivants sont instantanés — c'est ce qui rend
//! le scan « rapide » d'un branchement à l'autre.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Profondeur maximale depuis la racine du disque : `home00/gab/Games` est à 3,
/// on laisse un cran de marge.
pub const PROFONDEUR: usize = 4;

/// Nombre de dossiers visités au plus lors d'une découverte (garde-fou).
pub const BUDGET: usize = 4000;

/// Dossiers jamais visités : ils ne contiennent pas de paquets de jeux.
const IGNORES: [&str; 9] = [
    "node_modules",
    ".git",
    "lost+found",
    "$RECYCLE.BIN",
    "System Volume Information",
    ".Trash-1000",
    "proc",
    "sys",
    "dev",
];

/// Fichier de mémoire des racines trouvées, dans le cache du launcher.
pub fn cache_path(cache: &Path) -> PathBuf {
    cache.join("disks.txt")
}

/// Points de montage des disques externes, lus dans `/proc/self/mountinfo`.
///
/// Aucune commande externe : le fichier est déjà à jour et sa lecture est
/// instantanée. On ne garde que `/run/media/…` et `/media/…` (le montage posé
/// par udisks), et pas le dossier utilisateur lui-même.
pub fn montees() -> Vec<PathBuf> {
    let Ok(texte) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };
    let mut points: Vec<PathBuf> = Vec::new();
    let mut vus: HashSet<PathBuf> = HashSet::new();
    for ligne in texte.lines() {
        // id parent maj:min racine point_de_montage options … - type source …
        let Some(point) = ligne.split(' ').nth(4) else {
            continue;
        };
        let externe = point.starts_with("/run/media/") || point.starts_with("/media/");
        // profondeur minimale : /run/media/<utilisateur>/<disque>
        if !externe || point.matches('/').count() < 4 {
            continue;
        }
        let chemin = PathBuf::from(point);
        if vus.insert(chemin.clone()) {
            points.push(chemin);
        }
    }
    points.sort();
    points
}

/// Guetteur des disques montés : appelle `emit` quand l'ensemble change.
///
/// Une lecture de `/proc/self/mountinfo` toutes les deux secondes — quelques
/// microsecondes, aucun parcours de dossier — plutôt qu'une veille inotify sur
/// `/run/media/<utilisateur>` : le point de montage y est créé *et* supprimé par
/// udisks, mais un disque rebranché peut reprendre le même nom, et inotify ne
/// dit rien de l'inode. Le fichier du noyau, lui, est la vérité, et il coûte
/// moins cher qu'un fil de veille à entretenir. Le premier relevé ne signale
/// rien : le scan de démarrage a déjà vu l'état courant.
pub fn surveiller<F>(emit: F)
where
    F: Fn() + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("glauncher-disks".into())
        .spawn(move || {
            let mut avant = montees();
            loop {
                std::thread::sleep(INTERVALLE);
                let maintenant = montees();
                if maintenant != avant {
                    avant = maintenant;
                    emit();
                }
            }
        });
}

/// Intervalle du guetteur des disques.
const INTERVALLE: std::time::Duration = std::time::Duration::from_secs(2);

/// Racines de jeux des disques montés, **sans parcours quand c'est possible**.
///
/// Le cache (`<cache>/disks.txt`) mémorise `<montage>\t<racine>`. Un disque
/// déjà vu ne coûte qu'un `stat` par racine mémorisée : le rebranchement est
/// instantané, même avec des milliers de fichiers dessus. La mémoire des
/// disques **débranchés** est conservée, sinon chaque rebranchement repaierait
/// la découverte.
///
/// Un disque jamais vu (ou dont toutes les racines ont disparu) paie un
/// parcours borné — c'est [`chercher`], pas un `find` sur 170 000 fichiers.
pub fn racines(cache: &Path, profondeur: usize) -> Vec<PathBuf> {
    let memo = lire_cache(&cache_path(cache));
    let disques = montees();
    let mut sortie: Vec<PathBuf> = Vec::new();
    let mut garde: Vec<(PathBuf, PathBuf)> = Vec::new();

    for disque in &disques {
        let mut connues: Vec<PathBuf> = memo
            .iter()
            .filter(|(montage, _)| montage == disque)
            .map(|(_, racine)| racine.clone())
            .filter(|racine| racine.is_dir())
            .collect();
        if connues.is_empty() {
            connues = chercher(disque, profondeur);
        }
        for racine in connues {
            if !sortie.contains(&racine) {
                sortie.push(racine.clone());
            }
            garde.push((disque.clone(), racine));
        }
    }
    // disques absents : on garde leur mémoire pour le prochain branchement
    for (montage, racine) in &memo {
        if !disques.contains(montage) {
            garde.push((montage.clone(), racine.clone()));
        }
    }

    ecrire_cache(&cache_path(cache), &garde);
    sortie.sort();
    sortie
}

/// Dossiers porteurs de paquets présents sur les disques montés mais **hors**
/// des racines déjà connues : un dossier de jeux créé depuis le dernier
/// branchement. Coûte un parcours borné par disque, d'où l'appel séparé —
/// au branchement, pas à chaque réindexation.
pub fn nouveaux(connues: &[PathBuf], profondeur: usize) -> Vec<PathBuf> {
    let mut ajouts: Vec<PathBuf> = Vec::new();
    for disque in montees() {
        for racine in chercher(&disque, profondeur) {
            if !connues.contains(&racine) && !ajouts.contains(&racine) {
                ajouts.push(racine);
            }
        }
    }
    ajouts.sort();
    ajouts
}

/// Ajoute des racines au cache, sans perdre la mémoire des disques absents.
pub fn memoriser(cache: &Path, racines: &[PathBuf]) {
    let path = cache_path(cache);
    let memo = lire_cache(&path);
    let disques = montees();
    let mut garde: Vec<(PathBuf, PathBuf)> = Vec::new();
    for disque in &disques {
        for racine in racines {
            if racine.starts_with(disque) {
                garde.push((disque.clone(), racine.clone()));
            }
        }
    }
    for (montage, racine) in memo {
        if !disques.contains(&montage) {
            garde.push((montage, racine));
        }
    }
    ecrire_cache(&path, &garde);
}

/// Parcours en largeur borné : rend les dossiers contenant un `.lgp` / `.wgp`.
///
/// Profondeur [`PROFONDEUR`], [`BUDGET`] dossiers visités au plus, et les
/// dossiers de [`IGNORES`] sautés. Un paquet étant un fichier, descendre dans
/// un dossier de jeux ne coûte que ses sous-dossiers.
pub fn chercher(racine: &Path, profondeur: usize) -> Vec<PathBuf> {
    let mut trouves: Vec<PathBuf> = Vec::new();
    let mut a_voir: Vec<(PathBuf, usize)> = vec![(racine.to_path_buf(), 0)];
    let mut visites = 0usize;

    while let Some((dossier, niveau)) = a_voir.pop() {
        if visites >= BUDGET {
            break;
        }
        visites += 1;
        let Ok(entrees) = std::fs::read_dir(&dossier) else {
            continue;
        };
        let mut contient = false;
        let mut sous_dossiers: Vec<(PathBuf, usize)> = Vec::new();
        for entree in entrees.flatten() {
            let nom = entree.file_name();
            let nom = nom.to_string_lossy();
            let Ok(type_entree) = entree.file_type() else {
                continue;
            };
            if type_entree.is_dir() {
                if niveau < profondeur && !IGNORES.contains(&nom.as_ref()) {
                    sous_dossiers.push((entree.path(), niveau + 1));
                }
            } else if nom.ends_with(".lgp") || nom.ends_with(".wgp") {
                contient = true;
            }
        }
        if contient {
            trouves.push(dossier);
        }
        // On descend TOUJOURS (dans la limite de profondeur) : un `.lgp` posé
        // à la racine du disque ne doit pas masquer `home00/gab/Games`. Un
        // paquet est un FICHIER, ses données sont dans le squashfs : ses
        // voisins sont d'autres dossiers, pas ses données.
        a_voir.extend(sous_dossiers);
    }

    trouves.sort();
    trouves
}

fn lire_cache(path: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(texte) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    texte
        .lines()
        .filter_map(|ligne| ligne.split_once('\t'))
        .map(|(montage, racine)| (PathBuf::from(montage.trim()), PathBuf::from(racine.trim())))
        .filter(|(montage, racine)| !montage.as_os_str().is_empty() && !racine.as_os_str().is_empty())
        .collect()
}

fn ecrire_cache(path: &Path, entrees: &[(PathBuf, PathBuf)]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut texte = String::new();
    for (montage, racine) in entrees {
        texte.push_str(&montage.to_string_lossy());
        texte.push('\t');
        texte.push_str(&racine.to_string_lossy());
        texte.push('\n');
    }
    let tmp = path.with_extension("part");
    if std::fs::write(&tmp, texte).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glauncher-disks-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn chercher_trouve_les_dossiers_de_paquets_et_borne_le_parcours() {
        let disque = bac("chercher");
        // home00/gab/Games : profondeur 3
        let jeux = disque.join("home00/gab/Games");
        std::fs::create_dir_all(&jeux).unwrap();
        std::fs::write(jeux.join("mario.wgp"), b"x").unwrap();
        // un dossier de données avec un .lgp dedans ne doit pas être visité
        let caches = disque.join("node_modules/paquet");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::write(caches.join("faux.lgp"), b"x").unwrap();
        // un .lgp à la racine même
        std::fs::write(disque.join("zelda.lgp"), b"x").unwrap();

        let trouves = chercher(&disque, PROFONDEUR);
        assert!(trouves.contains(&disque), "paquet à la racine du disque");
        assert!(trouves.contains(&jeux), "dossier de jeux trouvé");
        assert!(
            !trouves.iter().any(|racine| racine.starts_with(&caches)),
            "node_modules jamais visité"
        );
        let _ = std::fs::remove_dir_all(&disque);
    }

    #[test]
    fn cache_evite_de_reparcourir() {
        let disque = bac("cache");
        let jeux = disque.join("Games");
        std::fs::create_dir_all(&jeux).unwrap();
        std::fs::write(jeux.join("mario.wgp"), b"x").unwrap();
        let cache = disque.join("cache");
        let chemin = cache_path(&cache);
        ecrire_cache(&chemin, &[(disque.clone(), jeux.clone())]);
        // disque non monté (/proc) : on vérifie seulement la relecture du cache
        let memo = lire_cache(&chemin);
        assert_eq!(memo, vec![(disque.clone(), jeux.clone())]);
        let _ = std::fs::remove_dir_all(&disque);
    }
}
