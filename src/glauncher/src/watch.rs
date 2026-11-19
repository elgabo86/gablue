//! Surveillance des dossiers de jeux (inotify via `notify`).
//!
//! Un seul évènement est émis par rafale : la bibliothèque est rescannée
//! entièrement, ce qui reste très bon marché (un `.wgp` de 30 Go ne pèse rien
//! dans un scan, seuls les noms de fichiers sont lus).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};

/// Délai avant la pose des veilles : le disque des jeux ne doit pas être
/// réveillé avant que le launcher soit à l'écran.
const START_DELAY: Duration = Duration::from_secs(5);

/// Écoute les dossiers donnés et appelle `emit` avec les chemins touchés
/// après chaque rafale. Les dossiers absents ne sont pas une erreur : ils sont
/// re-testés toutes les 2 s (disque externe rebranché).
pub fn spawn<F>(dirs: Vec<PathBuf>, emit: F)
where
    F: Fn(Vec<PathBuf>) + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("glauncher-watch".into())
        .spawn(move || run(dirs, emit));
}

fn run<F>(dirs: Vec<PathBuf>, emit: F)
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

    let mut watched: HashSet<PathBuf> = HashSet::new();
    reconcile(&mut watcher, &dirs, &mut watched);

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
                reconcile(&mut watcher, &dirs, &mut watched);
                emit(paths);
            }
            Ok(Err(_)) => continue,
            // toutes les 2 s : un dossier disparu est signalé, un dossier
            // recréé est de nouveau surveillé
            Err(RecvTimeoutError::Timeout) => {
                let touched = reconcile(&mut watcher, &dirs, &mut watched);
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
fn reconcile(
    watcher: &mut impl Watcher,
    dirs: &[PathBuf],
    watched: &mut HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut touched = Vec::new();

    watched.retain(|dir| {
        if dir.is_dir() {
            return true;
        }
        let _ = watcher.unwatch(dir);
        eprintln!(
            "glauncher : {} a disparu, jeux retirés de la liste",
            dir.display()
        );
        touched.push(dir.clone());
        false
    });

    for dir in dirs {
        if watched.contains(dir) || !dir.is_dir() {
            continue;
        }
        match watcher.watch(dir, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(dir.clone());
                // (re)surveillé : on réindexe ce dossier
                touched.push(dir.clone());
            }
            Err(err) => {
                // un sous-dossier illisible (préfixe Wine, lien cassé) fait
                // échouer la pose récursive : on se rabat sur le 1er niveau
                match watcher.watch(dir, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        watched.insert(dir.clone());
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

fn interesting(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) | EventKind::Any
    )
}
