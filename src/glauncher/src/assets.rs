//! Visuels posés **à côté des paquets**, lus au second plan.
//!
//! `<nom du paquet>.png`, `icons/<slug>.png`, `<slug>.png` dans les
//! `icon_dirs` : ces fichiers vivent avec les jeux, donc souvent sur un disque
//! externe qui dort. Les chercher au démarrage pour 666 paquets (≈6 `stat`
//! chacun) réveillait le disque et repoussait la première image de plusieurs
//! secondes.
//!
//! Le démarrage ne lit donc que le cache local (`icons/pack/`, `icons/`) ; ce
//! worker complète ensuite, d'abord pour les jeux à l'écran (`now`), puis en
//! balayage discret du reste de la bibliothèque (`sweep`).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use iced::widget::image;

use crate::library;

/// Pause entre deux paquets du balayage : le disque reste utilisable.
const SWEEP_GAP: Duration = Duration::from_millis(40);

#[derive(Debug)]
pub enum Event {
    /// Icône cherchée à côté du paquet (`None` = rien trouvé, l'extraction
    /// depuis le squashfs peut alors être tentée).
    Icon {
        slug: String,
        icon: Option<image::Handle>,
    },
}

/// Un paquet à examiner.
#[derive(Debug, Clone)]
pub struct Job {
    slug: String,
    pack: PathBuf,
}

impl Job {
    pub fn new(slug: impl Into<String>, pack: impl Into<PathBuf>) -> Self {
        Self {
            slug: slug.into(),
            pack: pack.into(),
        }
    }
}

#[derive(Default)]
struct Work {
    front: VecDeque<Job>,
    sweep: VecDeque<Job>,
}

#[derive(Clone)]
pub struct Queue {
    shared: Arc<(Mutex<Work>, Condvar)>,
}

impl Queue {
    /// Jeux à l'écran : tout de suite.
    pub fn now(&self, jobs: impl IntoIterator<Item = Job>) {
        if let Ok(mut work) = self.shared.0.lock() {
            let mut queued = false;
            for job in jobs {
                work.front.push_back(job);
                queued = true;
            }
            if queued {
                self.shared.1.notify_all();
            }
        }
    }

    /// Reste de la bibliothèque (remplacé à chaque réindexation).
    pub fn sweep(&self, jobs: Vec<Job>) {
        if let Ok(mut work) = self.shared.0.lock() {
            work.sweep = jobs.into();
            self.shared.1.notify_all();
        }
    }
}

/// Démarre le worker.
pub fn spawn<F>(icon_dirs: Vec<PathBuf>, pack_cache: PathBuf, emit: F) -> Queue
where
    F: Fn(Event) + Send + 'static,
{
    let queue = Queue {
        shared: Arc::new((Mutex::new(Work::default()), Condvar::new())),
    };
    let handle = queue.clone();

    let _ = std::thread::Builder::new()
        .name("glauncher-assets".into())
        .spawn(move || {
            let (lock, condvar) = &*handle.shared;
            loop {
                let (job, priority) = {
                    let mut work = match lock.lock() {
                        Ok(work) => work,
                        Err(_) => return,
                    };
                    loop {
                        if let Some(job) = work.front.pop_front() {
                            break (job, true);
                        }
                        if let Some(job) = work.sweep.pop_front() {
                            break (job, false);
                        }
                        work = match condvar.wait(work) {
                            Ok(work) => work,
                            Err(_) => return,
                        };
                    }
                };

                if !priority {
                    std::thread::sleep(SWEEP_GAP);
                }

                let icon = library::icon_near_pack(&job.pack, &job.slug, &icon_dirs, &pack_cache);
                emit(Event::Icon {
                    slug: job.slug,
                    icon,
                });
            }
        });

    queue
}
