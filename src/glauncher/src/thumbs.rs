//! Miniatures des covers pour l'affichage en vignettes.
//!
//! Pourquoi : `iced_wgpu` décode les images **sur un seul thread**, une fois par
//! image et par session, au premier affichage. Une cover SteamGridDB fait
//! 600×900 (≈12 ms de décodage, mesuré), donc une vue très dézoomée (150
//! vignettes) mettait ~1,8 s à se remplir. Une miniature de 320 px de large
//! décode ~4× plus vite et occupe 4× moins de mémoire GPU.
//!
//! Deux niveaux de priorité, comme le worker de covers :
//! - `now` : les vignettes des jeux à l'écran (réponse immédiate) ;
//! - `sweep` : balayage discret de toute la bibliothèque, remplacé à chaque
//!   rafraîchissement, avec une pause entre deux images.
//!
//! Le cache est `<assets>/thumbs/<slug>.jpg` (JPEG qualité 85 : une vignette de
//! 320×480 pèse ~35 Ko). Une miniature plus ancienne que sa cover est refaite —
//! changer d'image met donc la vignette à jour.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use image::imageops::FilterType;

/// Largeur maximale d'une miniature. Au-delà, la vue utilise la cover pleine
/// (aucune image n'est agrandie).
pub const MAX_WIDTH: u32 = 320;

/// Pause entre deux miniatures du balayage de fond (reste discret).
const SWEEP_GAP: Duration = Duration::from_millis(40);

/// Qualité JPEG des miniatures.
const QUALITY: u8 = 80;

/// Signature des réglages, écrite dans `<thumbs>/.settings` : la changer
/// invalide tout le cache (largeur ou qualité différente), sinon les anciennes
/// miniatures resteraient pour toujours — leur mtime ne bouge pas.
const SETTINGS: &str = "w320-q80";

#[derive(Debug)]
pub enum ThumbEvent {
    Ready { slug: String, bytes: Vec<u8> },
}

/// Une cover à réduire.
#[derive(Debug, Clone)]
pub struct Job {
    slug: String,
    source: PathBuf,
}

impl Job {
    pub fn new(slug: impl Into<String>, source: impl Into<PathBuf>) -> Self {
        Self {
            slug: slug.into(),
            source: source.into(),
        }
    }
}

#[derive(Default)]
struct Work {
    /// Jeux à l'écran.
    front: VecDeque<Job>,
    /// Bibliothèque entière (balayage).
    sweep: VecDeque<Job>,
}

/// Poignée d'envoi vers le worker de miniatures.
#[derive(Clone)]
pub struct Queue {
    shared: Arc<(Mutex<Work>, Condvar)>,
}

impl Queue {
    /// Miniatures à faire tout de suite (jeux visibles).
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

    /// Balayage de fond : remplace la liste en attente.
    pub fn sweep(&self, jobs: Vec<Job>) {
        if let Ok(mut work) = self.shared.0.lock() {
            work.sweep = jobs.into();
            self.shared.1.notify_all();
        }
    }
}

/// Démarre le worker (un thread, séquentiel).
pub fn spawn<F>(dir: PathBuf, emit: F) -> Queue
where
    F: Fn(ThumbEvent) + Send + 'static,
{
    // purge SYNCHRONE avant le scan de la bibliothèque : un changement de
    // largeur/qualité ne doit pas laisser d'anciennes miniatures
    reset_if_settings_changed(&dir);

    let queue = Queue {
        shared: Arc::new((Mutex::new(Work::default()), Condvar::new())),
    };
    let handle = queue.clone();

    let _ = std::thread::Builder::new()
        .name("glauncher-thumbs".into())
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

                match build(&dir, &job) {
                    Ok(Some(bytes)) => emit(ThumbEvent::Ready {
                        slug: job.slug.clone(),
                        bytes,
                    }),
                    Ok(None) => {}
                    Err(error) => eprintln!("glauncher : miniature {} : {error}", job.slug),
                }
            }
        });

    queue
}

/// Vide le cache si les réglages ont changé depuis la dernière fois.
fn reset_if_settings_changed(dir: &Path) {
    let marker = dir.join(".settings");
    if std::fs::read_to_string(&marker).is_ok_and(|text| text.trim() == SETTINGS) {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("jpg" | "png" | "tmp")
            ) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&marker, SETTINGS);
    eprintln!("glauncher : miniatures régénérées (réglages {SETTINGS})");
}

/// Chemin de la miniature (jpg ou png d'une génération précédente).
pub fn cached(dir: &Path, slug: &str) -> Option<PathBuf> {
    ["jpg", "png"]
        .into_iter()
        .map(|ext| dir.join(format!("{slug}.{ext}")))
        .find(|path| path.is_file())
}

/// Réduit la cover et renvoie le JPEG (ou les octets déjà en cache).
pub fn build(dir: &Path, job: &Job) -> Result<Option<Vec<u8>>, String> {
    let Ok(source_mtime) = std::fs::metadata(&job.source).and_then(|meta| meta.modified()) else {
        return Err(format!("{} introuvable", job.source.display()));
    };

    if let Some(path) = cached(dir, &job.slug)
        && is_fresh(&path, source_mtime)
    {
        // déjà à jour : on relit (quelques dizaines de Ko) pour l'afficher
        return Ok(Some(std::fs::read(&path).map_err(|err| format!("{} : {err}", path.display()))?));
    }

    let image = image::ImageReader::open(&job.source)
        .map_err(|err| format!("{} : {err}", job.source.display()))?
        .with_guessed_format()
        .map_err(|err| format!("{} : {err}", job.source.display()))?
        .decode()
        .map_err(|err| format!("décodage {} : {err}", job.source.display()))?;

    // jamais d'agrandissement : une petite cover reste telle quelle
    let thumb = if image.width() > MAX_WIDTH {
        image.resize(MAX_WIDTH, u32::MAX, FilterType::Triangle)
    } else {
        image
    }
    .to_rgb8();

    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, QUALITY);
    encoder
        .encode_image(&thumb)
        .map_err(|err| format!("encodage {} : {err}", job.slug))?;

    std::fs::create_dir_all(dir).map_err(|err| format!("{} : {err}", dir.display()))?;
    let path = dir.join(format!("{}.jpg", job.slug));
    // écriture atomique : une miniature tronquée ne doit jamais être affichée
    let tmp = dir.join(format!("{}.jpg.tmp", job.slug));
    std::fs::write(&tmp, &bytes).map_err(|err| format!("{} : {err}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|err| format!("{} : {err}", path.display()))?;

    Ok(Some(bytes))
}

fn is_fresh(thumb: &Path, source_mtime: SystemTime) -> bool {
    std::fs::metadata(thumb)
        .and_then(|meta| meta.modified())
        .map(|thumb_mtime| thumb_mtime >= source_mtime)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une cover 600×900 devient une miniature 320×480, et le cache est relu.
    #[test]
    fn reduit_et_cache() {
        let base = std::env::temp_dir().join(format!("glauncher-thumb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let covers = base.join("grids");
        let thumbs = base.join("thumbs");
        std::fs::create_dir_all(&covers).unwrap();

        let cover = covers.join("jeu.png");
        let image = image::RgbImage::from_fn(600, 900, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        image.save(&cover).unwrap();

        let job = Job::new("jeu", &cover);
        let bytes = build(&thumbs, &job).unwrap().expect("miniature générée");
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (MAX_WIDTH, 480));

        let path = thumbs.join("jeu.jpg");
        assert!(path.is_file(), "écrite dans le cache");
        assert_eq!(cached(&thumbs, "jeu"), Some(path.clone()));

        // deuxième passage : relu depuis le cache, sans redécodage
        let again = build(&thumbs, &job).unwrap().unwrap();
        assert_eq!(again, bytes);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Changer la cover refait la miniature (mtime plus récent).
    #[test]
    fn refaite_si_la_cover_change() {
        let base = std::env::temp_dir().join(format!("glauncher-thumb-maj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let thumbs = base.join("thumbs");
        std::fs::create_dir_all(&base).unwrap();

        let cover = base.join("jeu.png");
        image::RgbImage::from_fn(600, 900, |_, _| image::Rgb([200, 10, 10]))
            .save(&cover)
            .unwrap();
        let job = Job::new("jeu", &cover);
        build(&thumbs, &job).unwrap().unwrap();

        // nouvelle cover, plus récente que la miniature
        std::thread::sleep(std::time::Duration::from_millis(20));
        image::RgbImage::from_fn(600, 900, |_, _| image::Rgb([10, 10, 200]))
            .save(&cover)
            .unwrap();
        let bytes = build(&thumbs, &job).unwrap().unwrap();
        let pixel = image::load_from_memory(&bytes).unwrap().to_rgb8().get_pixel(5, 5).0;
        assert!(pixel[2] > pixel[0], "la miniature suit la nouvelle cover");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Sonde de performance (ignorée) : décodage d'une cover pleine contre une
    /// miniature, sur les fichiers réels du cache.
    #[test]
    #[ignore = "sonde de performance"]
    fn bench_decodage() {
        let base = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".local/share/glauncher/assets");
        for (label, dir) in [("cover  ", base.join("grids")), ("miniature", base.join("thumbs"))] {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| {
                            matches!(
                                path.extension().and_then(|ext| ext.to_str()),
                                Some("png" | "jpg" | "webp")
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            files.truncate(40);
            let start = std::time::Instant::now();
            let mut pixels = 0u64;
            for path in &files {
                let image = image::ImageReader::open(path)
                    .unwrap()
                    .with_guessed_format()
                    .unwrap()
                    .decode()
                    .unwrap();
                pixels += u64::from(image.width()) * u64::from(image.height());
            }
            let each = start.elapsed().as_secs_f64() * 1000.0 / files.len().max(1) as f64;
            eprintln!(
                "{label} : {} images, {each:.2} ms/image, {} px",
                files.len(),
                pixels / files.len().max(1) as u64
            );
        }
    }

    /// Une cover plus petite que la miniature n'est pas agrandie.
    #[test]
    fn pas_d_agrandissement() {
        let base = std::env::temp_dir().join(format!("glauncher-thumb-small-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let cover = base.join("petit.png");
        image::RgbImage::from_fn(160, 240, |_, _| image::Rgb([10, 20, 30]))
            .save(&cover)
            .unwrap();

        let bytes = build(&base.join("thumbs"), &Job::new("petit", &cover))
            .unwrap()
            .unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (160, 240));
        let _ = std::fs::remove_dir_all(&base);
    }
}
