//! Copie d'un paquet `.wgp`/`.lgp` vers un autre dossier, en tâche de fond.
//!
//! Trois usages : envoyer un jeu sur le dossier de téléchargements (pour le
//! sortir de la machine), ranger un jeu trouvé sur un disque externe dans un
//! dossier de jeux choisi, ou l'inverse. Rien n'est jamais déplacé : la source
//! reste en place, on ne fait que dupliquer.
//!
//! **Écriture en `.part` puis renommage** : un fichier à moitié copié porte le
//! nom d'un paquet qu'il n'est pas — la bibliothèque l'indexerait et le
//! launcher tenterait de le lancer (même règle que les visuels de `covers`).
//!
//! Le fil de copie ne connaît ni l'interface ni le launcher : il rend des
//! [`Event`], l'appelant les poste sur le bus. Un gros `.wgp` fait plusieurs
//! Go, il est lu par morceaux de [`MORCEAU`] pour que la barre avance.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Taille d'un morceau lu/écrit : assez gros pour ne pas payer un appel
/// système par kilo-octet, assez petit pour que la barre bouge (8 Mio ≈ 1/100
/// d'un jeu de 1 Go).
const MORCEAU: usize = 8 * 1024 * 1024;

/// Une copie à faire : la source, la destination **fichier compris**, et sa
/// taille (connue avant de commencer, pour le pourcentage global).
#[derive(Debug, Clone)]
pub struct Job {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub size: u64,
    /// Nom du jeu, pour la barre (« copie de Zelda »).
    pub nom: String,
}

/// Ce qu'on peut faire d'un paquet vers un dossier donné.
#[derive(Debug, Clone)]
pub enum Prepare {
    /// Rien sur place : on copie.
    Copier(Job),
    /// Le fichier est là et **plus ancien** que la source : on propose la mise à
    /// jour (l'écrasement n'est jamais silencieux).
    Remplacer(Job),
    /// Le fichier est là, à jour (même date ou plus récent) : rien à faire.
    Existe(PathBuf),
}

/// Évènements du fil de copie.
#[derive(Debug, Clone)]
pub enum Event {
    /// Octets écrits **pour le jeu en cours** (le total global est tenu par
    /// l'appelant, qui empile les copies).
    Progress { fait: u64 },
    /// Copie terminée (le fichier définitif est en place).
    Done { size: u64 },
    Failed { nom: String, erreur: String },
}

/// Dossier de téléchargements de l'utilisateur : `XDG_DOWNLOAD_DIR` de
/// `~/.config/user-dirs.dirs` (le dossier que Dolphin ouvre sous « Téléchargements »),
/// sinon `~/Téléchargements`, `~/Downloads`, puis le dossier personnel.
///
/// Aucune commande externe : le fichier des dossiers XDG est plat et stable, et
/// `xdg-user-dir` n'existe pas forcément dans une session console.
pub fn telechargements() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let conf = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("user-dirs.dirs");
    if let Ok(texte) = std::fs::read_to_string(&conf) {
        for ligne in texte.lines() {
            let ligne = ligne.trim();
            let Some(valeur) = ligne.strip_prefix("XDG_DOWNLOAD_DIR=") else {
                continue;
            };
            let valeur = valeur.trim().trim_matches('"');
            if valeur.is_empty() {
                continue;
            }
            let chemin = valeur.replace("$HOME", &home.to_string_lossy());
            let chemin = PathBuf::from(chemin);
            if chemin.is_dir() {
                return chemin;
            }
        }
    }
    for essai in ["Téléchargements", "Downloads"] {
        let chemin = home.join(essai);
        if chemin.is_dir() {
            return chemin;
        }
    }
    home
}

/// Espace disponible sur le système de fichiers de `dossier` (octets), avec
/// `statvfs` : `f_bavail` est la place réellement offerte à l'utilisateur (les
/// blocs réservés à root n'en font pas partie).
pub fn libre(dossier: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let chemin = CString::new(dossier.as_os_str().as_bytes()).ok()?;
    // SAFETY : `statvfs` ne lit que la chaîne fournie et écrit dans la
    // structure ; les deux sont valides le temps de l'appel.
    unsafe {
        let mut infos: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(chemin.as_ptr(), &mut infos) != 0 {
            return None;
        }
        Some(infos.f_bavail as u64 * infos.f_frsize as u64)
    }
}

/// Taille lisible, en unités binaires (ce que Dolphin affiche).
pub fn poids(octets: u64) -> String {
    const GIO: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIO: f64 = 1024.0 * 1024.0;
    if octets as f64 >= GIO {
        format!("{:.1} Gio", octets as f64 / GIO)
    } else if octets as f64 >= MIO {
        format!("{:.0} Mio", octets as f64 / MIO)
    } else {
        format!("{} Kio", octets.div_ceil(1024))
    }
}

/// Que faire de `source` vers `dossier` ? La règle de date ne concerne que le
/// fichier de **même nom** : un autre jeu dans le dossier n'entre pas en jeu.
pub fn prepare(source: &Path, dossier: &Path) -> Result<Prepare, String> {
    let Some(fichier) = source.file_name() else {
        return Err(format!("{} : nom de fichier illisible", source.display()));
    };
    let dest = dossier.join(fichier);
    let meta = std::fs::metadata(source)
        .map_err(|err| format!("{} : {err}", source.display()))?;
    if !meta.is_file() {
        return Err(format!("{} : ce n'est pas un fichier", source.display()));
    }
    let nom = source
        .file_stem()
        .map(|nom| nom.to_string_lossy().to_string())
        .unwrap_or_else(|| fichier.to_string_lossy().to_string());
    let job = Job {
        source: source.to_path_buf(),
        dest: dest.clone(),
        size: meta.len(),
        nom,
    };
    let Ok(place) = std::fs::metadata(&dest) else {
        return Ok(Prepare::Copier(job));
    };
    // la source sert de référence : une copie faite avant la dernière mise à
    // jour du paquet est « plus vieille » et mérite d'être rafraîchie
    let source_date = meta.modified().ok();
    let dest_date = place.modified().ok();
    let plus_vieille = match (source_date, dest_date) {
        (Some(source), Some(dest)) => dest < source,
        // date illisible : on ne prétend pas savoir, donc rien à faire
        _ => false,
    };
    if plus_vieille {
        Ok(Prepare::Remplacer(job))
    } else {
        Ok(Prepare::Existe(dest))
    }
}

/// Lance la copie dans un fil et rend la main tout de suite. `deja` est le
/// nombre d'octets déjà comptés (copies précédentes de la file), pour que
/// l'appelant puisse afficher une progression **cumulée**.
pub fn spawn<F>(job: Job, deja: u64, emit: F)
where
    F: Fn(Event) + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("glauncher-copie".into())
        .spawn(move || {
            let nom = job.nom.clone();
            match copier(&job, deja, &emit) {
                Ok(()) => emit(Event::Done { size: job.size }),
                Err(erreur) => emit(Event::Failed { nom, erreur }),
            }
        });
}

fn copier<F>(job: &Job, deja: u64, emit: &F) -> Result<(), String>
where
    F: Fn(Event),
{
    let mut part = job.dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    if let Some(parent) = job.dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("{} : {err}", parent.display()))?;
    }
    let source = std::fs::File::open(&job.source)
        .map_err(|err| format!("{} : {err}", job.source.display()))?;
    let mut sortie = std::fs::File::create(&part)
        .map_err(|err| format!("{} : {err}", part.display()))?;
    let mut entree = std::io::BufReader::with_capacity(MORCEAU, source);
    let mut tampon = vec![0u8; MORCEAU];
    let mut fait = deja;
    let mut annonce = 0u64;
    loop {
        let lus = entree
            .read(&mut tampon)
            .map_err(|err| format!("{} : {err}", job.source.display()))?;
        if lus == 0 {
            break;
        }
        sortie
            .write_all(&tampon[..lus])
            .map_err(|err| format!("{} : {err}", part.display()))?;
        fait += lus as u64;
        // un message par morceau suffit ; le suivant n'arrive que si le
        // disque a réellement écrit, donc la barre ne ment pas
        if fait - annonce >= (MORCEAU as u64) / 2 || fait - deja >= job.size {
            annonce = fait;
            emit(Event::Progress { fait: fait - deja });
        }
    }
    sortie
        .flush()
        .map_err(|err| format!("{} : {err}", part.display()))?;
    drop(sortie);
    let meta = std::fs::metadata(&part).map_err(|err| format!("{} : {err}", part.display()))?;
    if meta.len() != job.size {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "taille inattendue pour {} ({} au lieu de {})",
            job.nom,
            meta.len(),
            job.size
        ));
    }
    std::fs::rename(&part, &job.dest)
        .map_err(|err| format!("{} : {err} — copie laissée en .part", job.dest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glauncher-copie-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prepare_copie_remplace_ou_laisse() {
        let base = bac("prepare");
        let dossier = base.join("cible");
        std::fs::create_dir_all(&dossier).unwrap();
        let source = base.join("mario.wgp");
        std::fs::write(&source, b"12345").unwrap();

        // rien sur place : on copie
        let Prepare::Copier(job) = prepare(&source, &dossier).unwrap() else {
            panic!("attendu : copie");
        };
        assert_eq!(job.size, 5);
        assert_eq!(job.dest, dossier.join("mario.wgp"));
        assert_eq!(job.nom, "mario");

        // copie plus ancienne que la source : on propose la mise à jour
        let copie = dossier.join("mario.wgp");
        std::fs::write(&copie, b"12345").unwrap();
        let ancien = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&copie)
            .unwrap()
            .set_modified(ancien)
            .unwrap();
        assert!(matches!(
            prepare(&source, &dossier).unwrap(),
            Prepare::Remplacer(_)
        ));

        // copie plus récente : on ne touche à rien
        let recent = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&copie)
            .unwrap()
            .set_modified(recent)
            .unwrap();
        assert!(matches!(
            prepare(&source, &dossier).unwrap(),
            Prepare::Existe(_)
        ));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn copie_ecrit_le_fichier_et_supprime_le_part() {
        let base = bac("copie");
        let source = base.join("zelda.lgp");
        let contenu = vec![7u8; MORCEAU + 1234];
        std::fs::write(&source, &contenu).unwrap();
        let dossier = base.join("jeux");
        let Prepare::Copier(job) = prepare(&source, &dossier).unwrap() else {
            panic!("attendu : copie");
        };
        let evenements = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collecte = evenements.clone();
        copier(&job, 0, &move |event| {
            collecte.lock().unwrap().push(event);
        })
        .unwrap();
        assert_eq!(std::fs::read(job.dest.clone()).unwrap(), contenu);
        assert!(!job.dest.with_extension("lgp.part").exists(), "aucun .part restant");
        let evenements = evenements.lock().unwrap();
        assert!(matches!(evenements.last(), Some(Event::Progress { fait, .. }) if *fait == job.size as u64));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn poids_lisible() {
        assert_eq!(poids(0), "0 Kio");
        assert_eq!(poids(2048), "2 Kio");
        assert_eq!(poids(3 * 1024 * 1024), "3 Mio");
        assert_eq!(poids(2 * 1024 * 1024 * 1024), "2.0 Gio");
    }

}
