//! Surveillance des dossiers de jeux (inotify via `notify`).
//!
//! Un seul évènement est émis par rafale : la bibliothèque est rescannée
//! entièrement, ce qui reste très bon marché (un `.wgp` de 30 Go ne pèse rien
//! dans un scan, seuls les noms de fichiers sont lus).

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};

/// Délai avant la pose des veilles : le disque des jeux ne doit pas être
/// réveillé avant que le launcher soit à l'écran.
const START_DELAY: Duration = Duration::from_secs(5);

/// Poignée de surveillance : la liste des dossiers peut changer **à chaud**
/// (choix des dossiers depuis les réglages du launcher) sans relancer le fil,
/// qui vérifie la liste toutes les 2 s de toute façon.
#[derive(Clone)]
pub struct Veille {
    dirs: Arc<Mutex<Vec<PathBuf>>>,
}

impl Veille {
    /// Remplace la liste surveillée. Retirer un dossier le dé-surveille sans
    /// toucher au disque ; en ajouter un le surveille à la prochaine passe.
    pub fn definir(&self, dirs: Vec<PathBuf>) {
        if let Ok(mut courante) = self.dirs.lock() {
            *courante = dirs;
        }
    }

    fn copie(&self) -> Vec<PathBuf> {
        self.dirs.lock().map(|dirs| dirs.clone()).unwrap_or_default()
    }
}

/// Écoute les dossiers donnés et appelle `emit` avec les chemins touchés
/// après chaque rafale. Les dossiers absents ne sont pas une erreur : ils sont
/// re-testés toutes les 2 s (disque externe rebranché).
pub fn spawn<F>(dirs: Vec<PathBuf>, emit: F) -> Veille
where
    F: Fn(Vec<PathBuf>) + Send + 'static,
{
    let veille = Veille {
        dirs: Arc::new(Mutex::new(dirs)),
    };
    let pour_fil = veille.clone();
    let _ = std::thread::Builder::new()
        .name("glauncher-watch".into())
        .spawn(move || run(pour_fil, emit));
    veille
}

fn run<F>(veille: Veille, emit: F)
where
    F: Fn(Vec<PathBuf>) + Send + 'static,
{
    // Pose récursive = un parcours complet des dossiers (170 000 fichiers sur
    // un disque externe) : on laisse d'abord la première image s'afficher et
    // les icônes des jeux visibles se charger. Rien n'est perdu : un paquet
    // ajouté pendant ces secondes est vu au scan de démarrage.
    std::thread::sleep(START_DELAY);
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<Event>| {
        let _ = sender.send(result);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            eprintln!("glauncher : surveillance indisponible : {err}");
            return;
        }
    };

    let mut watched: HashMap<PathBuf, (u64, u64)> = HashMap::new();
    reconcile(&mut watcher, &veille.copie(), &mut watched);

    loop {
        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(event)) => {
                if !interesting(&event.kind) {
                    continue;
                }
                let mut paths = event.paths.clone();
                // rafale : on attend que le calme revienne, en cumulant les chemins
                while let Ok(Ok(next)) = receiver.recv_timeout(Duration::from_millis(400)) {
                    paths.extend(next.paths);
                }
                paths.sort();
                paths.dedup();
                reconcile(&mut watcher, &veille.copie(), &mut watched);
                emit(paths);
            }
            Ok(Err(_)) => continue,
            // toutes les 2 s : un dossier disparu est signalé, un dossier
            // recréé est de nouveau surveillé
            Err(RecvTimeoutError::Timeout) => {
                let touched = reconcile(&mut watcher, &veille.copie(), &mut watched);
                if !touched.is_empty() {
                    emit(touched);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Réaligne les watches sur l'état réel des dossiers.
/// Renvoie les dossiers à réindexer : ceux qui ont disparu et ceux qui
/// viennent de réapparaître (disque externe rebranché).
///
/// L'identité (device + inode) est mémorisée : une watch inotify posée sur un
/// dossier **reste collée à l'ancien inode** quand un disque est monté
/// par-dessus (le point de montage existait déjà, vide). Le dossier est
/// toujours là, donc l'ancienne version ne le re-surveillait jamais et aucun
/// évènement n'arrivait : le disque branché ne chargeait pas ses jeux. Un
/// inode différent = on repose la watch et on réindexe.
fn reconcile(
    watcher: &mut impl Watcher,
    dirs: &[PathBuf],
    watched: &mut HashMap<PathBuf, (u64, u64)>,
) -> Vec<PathBuf> {
    let mut touched = Vec::new();

    watched.retain(|dir, identity| {
        // retiré de la liste (choix des dossiers) : on lâche la watch sans
        // réindexer — l'appelant a déjà déclenché le rescan
        if !dirs.iter().any(|connu| connu == dir) {
            let _ = watcher.unwatch(dir);
            return false;
        }
        let Some(current) = identity_of(dir) else {
            let _ = watcher.unwatch(dir);
            eprintln!(
                "glauncher : {} a disparu, jeux retirés de la liste",
                dir.display()
            );
            touched.push(dir.clone());
            return false;
        };
        if current == *identity {
            return true;
        }
        // même chemin, autre inode : un disque a été monté par-dessus (ou le
        // dossier a été remplacé). La watch de l'ancien inode ne sert plus.
        let _ = watcher.unwatch(dir);
        eprintln!(
            "glauncher : {} a été remplacé (disque monté ?), rechargement des jeux",
            dir.display()
        );
        touched.push(dir.clone());
        false
    });

    for dir in dirs {
        if watched.contains_key(dir) {
            continue;
        }
        let Some(identity) = identity_of(dir) else {
            continue;
        };
        match watcher.watch(dir, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(dir.clone(), identity);
                // (re)surveillé : on réindexe ce dossier
                touched.push(dir.clone());
            }
            Err(err) => {
                // un sous-dossier illisible (préfixe Wine, lien cassé) fait
                // échouer la pose récursive : on se rabat sur le 1er niveau
                match watcher.watch(dir, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        watched.insert(dir.clone(), identity);
                        touched.push(dir.clone());
                        eprintln!(
                            "glauncher : {} surveillé au 1er niveau seulement ({err})",
                            dir.display()
                        );
                    }
                    Err(fallback) => {
                        eprintln!("glauncher : {} non surveillé : {fallback}", dir.display())
                    }
                }
            }
        }
    }

    touched.sort();
    touched.dedup();
    touched
}

/// Identité du dossier (device + inode) : ce qui change quand un système de
/// fichiers est monté par-dessus un point de montage existant.
fn identity_of(path: &std::path::Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_dir() {
        return None;
    }
    Some((meta.dev(), meta.ino()))
}

fn interesting(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) | EventKind::Any
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_dossier_qui_apparait_ou_change_d_inode_est_signale() {
        let base = std::env::temp_dir().join(format!("glauncher-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = base.join("jeux");

        let mut watcher = notify::recommended_watcher(|_: notify::Result<Event>| {}).unwrap();
        let mut watched = HashMap::new();

        // absent au démarrage : rien à surveiller
        assert!(reconcile(&mut watcher, &[dir.clone()], &mut watched).is_empty());

        // le disque apparaît : signalé et surveillé
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            reconcile(&mut watcher, &[dir.clone()], &mut watched),
            [dir.clone()]
        );
        assert!(watched.contains_key(&dir));

        // inchangé : aucun travail
        assert!(reconcile(&mut watcher, &[dir.clone()], &mut watched).is_empty());

        // même chemin, nouvel inode (disque monté sur un point de montage) :
        // l'ancienne watch inotify ne verrait plus rien, on repose et on
        // signale
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            reconcile(&mut watcher, &[dir.clone()], &mut watched),
            [dir.clone()]
        );
        assert!(watched.contains_key(&dir));

        // disque débranché : signalé, watch retirée
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            reconcile(&mut watcher, &[dir.clone()], &mut watched),
            [dir.clone()]
        );
        assert!(!watched.contains_key(&dir));

        let _ = std::fs::remove_dir_all(&base);
    }
}
