//! Découverte des paquets `.wgp` / `.lgp` (récursive), icône du paquet et
//! cover en cache.
//!
//! Le scan disque complet (`walk_packs`) est réservé au démarrage et à la
//! touche R ; le reste du temps ce sont les changements signalés par le
//! veilleur qui sont appliqués à l'index (`apply_changes`). Rien n'est jamais
//! écrit dans les dossiers de jeux : ils peuvent être en lecture seule.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use iced::widget::image;

use crate::config::Config;
use crate::covers;
use crate::index::{Index, Snapshot};

/// Format d'un paquet de jeu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wgp,
    Lgp,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Wgp => "wgp",
            Kind::Lgp => "lgp",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "lgp" => Kind::Lgp,
            _ => Kind::Wgp,
        }
    }

    fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        match ext.as_str() {
            "wgp" => Some(Kind::Wgp),
            "lgp" => Some(Kind::Lgp),
            _ => None,
        }
    }
}

/// Extensions d'image acceptées (les covers SGDB peuvent être jpg/webp).
const IMAGE_EXT: [&str; 3] = ["png", "jpg", "webp"];

/// Une ligne de l'index : un paquet sur le disque.
#[derive(Debug, Clone)]
pub struct PackRow {
    pub path: PathBuf,
    /// Dossier parent (clé de regroupement dans l'index).
    pub dir: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub mtime: i64,
    pub size: u64,
}

/// Résultat d'un parcours de dossiers : les paquets trouvés et la date de
/// modification de chaque dossier visité.
#[derive(Debug, Default)]
pub struct Walk {
    pub rows: Vec<PackRow>,
    /// (chemin, parent, mtime) de chaque dossier visité.
    pub dirs: Vec<(PathBuf, Option<PathBuf>, i64)>,
}

impl PackRow {
    pub fn slug(&self) -> String {
        slugify(&self.name)
    }
}

/// Visuels mémorisés d'un jeu (évite de relire les fichiers à chaque scan).
#[derive(Debug, Clone, Default)]
pub struct Artwork {
    pub icon: Option<image::Handle>,
    pub cover: Option<image::Handle>,
    pub hero: Option<image::Handle>,
    pub logo: Option<image::Handle>,
    /// Miniature 320 px de la cover : ce que la grille affiche.
    pub thumb: Option<image::Handle>,
}

pub type ArtworkCache = HashMap<String, Artwork>;

#[derive(Debug, Clone)]
pub struct Game {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub slug: String,
    /// Date de modification du paquet (index) : tri « récents d'abord ».
    pub mtime: i64,
    /// Icône du paquet (icône de l'exe, PNG posé à côté…) : la petite vignette.
    pub icon: Option<image::Handle>,
    /// Grille portrait (SteamGridDB) : la vignette du launcher.
    pub cover: Option<image::Handle>,
    /// Bannière large (SteamGridDB) : le fond de la page du jeu.
    pub hero: Option<image::Handle>,
    /// Logo transparent (SteamGridDB).
    pub logo: Option<image::Handle>,
    /// Miniature 320 px de la cover (décodage 4× plus rapide en grille).
    pub thumb: Option<image::Handle>,
}

/// Construit la liste affichable depuis les lignes de l'index.
///
/// `changed` liste les chemins modifiés : un visuel mémorisé dont le nom
/// correspond est oublié (icône remplacée à côté du paquet…).
pub fn from_rows(
    config: &Config,
    rows: &[PackRow],
    cache: &mut ArtworkCache,
    changed: &[PathBuf],
) -> Vec<Game> {
    let icons_cache = config.icon_cache_dir();
    let covers_dir = config.cover_cache_dir();
    let heroes_dir = config.hero_cache_dir();
    let logos_dir = config.logo_cache_dir();
    let thumbs_dir = config.thumb_dir();

    for path in changed {
        if let Some(stem) = path.file_stem() {
            cache.remove(&slugify(&stem.to_string_lossy()));
        }
    }

    let mut games = Vec::with_capacity(rows.len());
    for row in dedupe(rows) {
        let slug = row.slug();
        // AUCUN accès au dossier du paquet ici (voir `icon_cached`) : le
        // voisinage est lu au second plan par `assets::spawn`
        let artwork = cache.entry(slug.clone()).or_insert_with(|| Artwork {
            icon: icon_cached(
                &slug,
                &icons_cache,
                &config.pack_icon_dir(),
                &config.marks_dir("icons"),
            ),
            cover: art_for(&covers_dir, &slug),
            hero: art_for(&heroes_dir, &slug),
            logo: art_for(&logos_dir, &slug),
            thumb: art_for(&thumbs_dir, &slug),
        });

        games.push(Game {
            path: row.path.clone(),
            name: row.name.clone(),
            kind: row.kind,
            mtime: row.mtime,
            slug,
            icon: artwork.icon.clone(),
            cover: artwork.cover.clone(),
            hero: artwork.hero.clone(),
            logo: artwork.logo.clone(),
            thumb: artwork.thumb.clone(),
        });
    }

    games.sort_by_cached_key(|game| game.name.to_lowercase());
    games
}

/// Même nom de paquet dans plusieurs dossiers : on ne garde que le **plus
/// récent** (`mtime`), sinon le launcher affiche deux fois le même jeu — et
/// l'utilisateur ne sait pas lequel démarre.
fn dedupe(rows: &[PackRow]) -> Vec<&PackRow> {
    let mut best: HashMap<String, &PackRow> = HashMap::new();
    for row in rows {
        let key = row.name.to_lowercase();
        match best.get(&key) {
            Some(previous) if previous.mtime >= row.mtime => {}
            _ => {
                best.insert(key, row);
            }
        }
    }
    best.into_values().collect()
}

/// Scan disque récursif des dossiers configurés, en parallèle.
///
/// Les dossiers dont la date de modification n'a pas bougé depuis le dernier
/// scan ne sont ni relus (`readdir`) ni `stat`és fichier par fichier : leurs
/// paquets viennent de l'index. Combiné à une profondeur limitée et à une
/// liste de dossiers ignorés, un disque de 170 000 fichiers se scanne en
/// quelques centaines de millisecondes.
pub fn walk_packs(config: &Config, snapshot: &Snapshot) -> Walk {
    let depth_limit = config.depth_limit();
    let skip = config.skip_names();

    let state = Mutex::new(State {
        queue: Vec::new(),
        pending: 0,
        closed: false,
    });
    let signal = Condvar::new();
    let rows = Mutex::new(Vec::new());
    let dirs = Mutex::new(Vec::new());
    let visited = Mutex::new(HashSet::new());

    for root in config.game_dirs() {
        if root.is_dir() {
            push_job(&state, &signal, (root, None, 0));
        }
    }

    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 8);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((dir, parent, depth)) = next_job(&state, &signal) {
                    walk_one(
                        &dir,
                        parent.as_deref(),
                        depth,
                        depth_limit,
                        &skip,
                        snapshot,
                        &visited,
                        &rows,
                        &dirs,
                        &state,
                        &signal,
                    );
                    finish_job(&state, &signal);
                }
            });
        }
    });

    let mut rows = rows.into_inner().unwrap_or_default();
    let mut seen = HashSet::new();
    rows.retain(|row| seen.insert(row.path.clone()));

    Walk {
        rows,
        dirs: dirs.into_inner().unwrap_or_default(),
    }
}

#[derive(Default)]
struct State {
    queue: Vec<(PathBuf, Option<PathBuf>, usize)>,
    pending: usize,
    closed: bool,
}

fn push_job(state: &Mutex<State>, signal: &Condvar, job: (PathBuf, Option<PathBuf>, usize)) {
    if let Ok(mut state) = state.lock() {
        state.queue.push(job);
        state.pending += 1;
    }
    signal.notify_one();
}

fn next_job(
    state: &Mutex<State>,
    signal: &Condvar,
) -> Option<(PathBuf, Option<PathBuf>, usize)> {
    let mut guard = state.lock().ok()?;
    loop {
        if let Some(job) = guard.queue.pop() {
            return Some(job);
        }
        if guard.closed {
            return None;
        }
        guard = signal.wait(guard).ok()?;
    }
}

fn finish_job(state: &Mutex<State>, signal: &Condvar) {
    let mut done = false;
    if let Ok(mut state) = state.lock() {
        state.pending = state.pending.saturating_sub(1);
        if state.pending == 0 {
            state.closed = true;
            done = true;
        }
    }
    if done {
        signal.notify_all();
    } else {
        signal.notify_one();
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_one(
    dir: &Path,
    parent: Option<&Path>,
    depth: usize,
    depth_limit: Option<usize>,
    skip: &HashSet<String>,
    snapshot: &Snapshot,
    visited: &Mutex<HashSet<PathBuf>>,
    rows: &Mutex<Vec<PackRow>>,
    dirs: &Mutex<Vec<(PathBuf, Option<PathBuf>, i64)>>,
    state: &Mutex<State>,
    signal: &Condvar,
) {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if let Ok(mut visited) = visited.lock()
        && !visited.insert(canonical)
    {
        return;
    }

    let Ok(meta) = std::fs::metadata(dir) else {
        return;
    };
    let mtime = mtime_of(&meta);
    if let Ok(mut dirs) = dirs.lock() {
        dirs.push((dir.to_path_buf(), parent.map(Path::to_path_buf), mtime));
    }

    // dossier inchangé : ni readdir ni stat, on reprend l'arbre de l'index
    if let Some(known) = snapshot.dirs.get(dir)
        && known.mtime == mtime
    {
        if let Ok(mut rows) = rows.lock()
            && let Some(cached) = snapshot.packs.get(dir)
        {
            rows.extend(cached.iter().cloned());
        }
        for child in &known.children {
            push_job(state, signal, (child.clone(), Some(dir.to_path_buf()), depth + 1));
        }
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let is_dir = if file_type.is_symlink() {
            std::fs::metadata(&path)
                .map(|meta| meta.is_dir())
                .unwrap_or(false)
        } else {
            file_type.is_dir()
        };

        if is_dir {
            if skip.contains(&name) {
                continue;
            }
            if depth_limit.is_some_and(|limit| depth + 1 >= limit) {
                continue;
            }
            push_job(state, signal, (path, Some(dir.to_path_buf()), depth + 1));
        } else if let Some(row) = row_for(&path)
            && let Ok(mut rows) = rows.lock()
        {
            rows.push(row);
        }
    }
}

/// Applique une rafale de changements signalés par le veilleur à l'index,
/// puis renvoie la liste complète à jour. Aucun parcours global : on ne lit
/// que les chemins concernés (ou leur sous-arbre s'il s'agit d'un dossier).
pub fn apply_changes(
    config: &Config,
    index: &mut Index,
    changed: &[PathBuf],
) -> Result<Vec<PackRow>, String> {
    let roots: Vec<PathBuf> = config.game_dirs();

    for path in changed {
        // hors des dossiers suivis (ex. cache d'icônes) : rien à indexer
        let inside = roots.iter().any(|root| path.starts_with(root));
        if !inside {
            continue;
        }

        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                let walk = walk_subtree(config, path);
                index.replace_subtree(path, &walk)?;
            }
            Ok(_) => match row_for(path) {
                Some(row) => index.upsert(&row)?,
                None => index.remove_subtree(path)?,
            },
            Err(_) => index.remove_subtree(path)?,
        }
    }

    index.all()
}

/// Une entrée d'index pour un fichier, s'il s'agit d'un paquet.
pub fn row_for(path: &Path) -> Option<PackRow> {
    let kind = Kind::from_path(path)?;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().to_string();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs() as i64)
        .unwrap_or_default();

    Some(PackRow {
        path: path.to_path_buf(),
        dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        name,
        kind,
        mtime,
        size: meta.len(),
    })
}

/// Parcours d'un seul sous-arbre (dossier modifié), sans index : tout est relu.
fn walk_subtree(config: &Config, root: &Path) -> Walk {
    let depth_limit = config.depth_limit();
    let skip = config.skip_names();
    let state = Mutex::new(State {
        queue: vec![(root.to_path_buf(), None, 0)],
        pending: 1,
        closed: false,
    });
    let signal = Condvar::new();
    let rows = Mutex::new(Vec::new());
    let dirs = Mutex::new(Vec::new());
    let visited = Mutex::new(HashSet::new());
    let snapshot = Snapshot::default();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            while let Some((dir, parent, depth)) = next_job(&state, &signal) {
                walk_one(
                    &dir,
                    parent.as_deref(),
                    depth,
                    depth_limit,
                    &skip,
                    &snapshot,
                    &visited,
                    &rows,
                    &dirs,
                    &state,
                    &signal,
                );
                finish_job(&state, &signal);
            }
        });
    });

    Walk {
        rows: rows.into_inner().unwrap_or_default(),
        dirs: dirs.into_inner().unwrap_or_default(),
    }
}

fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs() as i64)
        .unwrap_or_default()
}

/// Icône du paquet : `<nom>.png` à côté du paquet, puis `icons/<slug>.png`,
/// puis les `icon_dirs`, puis le cache. Toute icône trouvée est copiée dans le
/// cache (elle survit à la disparition du jeu).
/// Icône disponible **sans toucher aux dossiers de jeux** : choix de
/// l'utilisateur, icône extraite du paquet (`icons/pack/`), icône SteamGridDB.
///
/// C'est la seule lecture d'icône faite au démarrage : le voisinage du paquet
/// vit souvent sur un disque externe, et 666 paquets × 6 `stat` réveillaient un
/// disque endormi (plusieurs secondes avant la première image).
fn icon_cached(
    slug: &str,
    icons_cache: &Path,
    pack_dir: &Path,
    marks: &Path,
) -> Option<image::Handle> {
    // 1. l'utilisateur a explicitement choisi une icône SteamGridDB : elle gagne
    if let Some(path) = covers::chosen(icons_cache, marks, slug)
        && let Ok(bytes) = std::fs::read(path)
    {
        return Some(image::Handle::from_bytes(bytes));
    }

    // 2. icône déjà extraite du paquet (cache local) ou icône SteamGridDB
    if let Some(handle) = art_for(pack_dir, slug) {
        return Some(handle);
    }
    art_for(icons_cache, slug)
}

/// Icône posée **à côté du paquet** : `<nom>.png`, `icons/<slug>.png` ou
/// `<slug>.png` dans les `icon_dirs`. Différé au second plan (disque externe) ;
/// une icône trouvée est copiée dans `icons/pack/`.
pub fn icon_near_pack(
    path: &Path,
    slug: &str,
    icon_dirs: &[PathBuf],
    pack_dir: &Path,
) -> Option<image::Handle> {
    let base = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem()?.to_string_lossy().to_string();

    let mut candidates = Vec::new();
    for ext in IMAGE_EXT {
        candidates.push(base.join(format!("{stem}.{ext}")));
        candidates.push(base.join("icons").join(format!("{slug}.{ext}")));
        for dir in icon_dirs {
            candidates.push(dir.join(format!("{slug}.{ext}")));
        }
    }

    for candidate in candidates {
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        cache_icon(&candidate, pack_dir, slug, &bytes);
        return Some(image::Handle::from_bytes(bytes));
    }
    None
}

/// Cover en cache (téléchargée par `covers`), s'il y en a une.
fn art_for(dir: &Path, slug: &str) -> Option<image::Handle> {
    let path = covers::cached(dir, slug)?;
    let bytes = std::fs::read(path).ok()?;
    Some(image::Handle::from_bytes(bytes))
}

/// Copie une icône dans le cache si la source est plus récente.
/// Échec silencieux : un cache non inscriptible ne casse pas le scan.
fn cache_icon(source: &Path, icons_cache: &Path, slug: &str, bytes: &[u8]) {
    let ext = source
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_else(|| "png".into());
    let cached = icons_cache.join(format!("{slug}.{ext}"));

    let stale = match (
        std::fs::metadata(&cached).ok(),
        std::fs::metadata(source).ok(),
    ) {
        (Some(cached), Some(source)) => match (cached.modified(), source.modified()) {
            (Ok(cached), Ok(source)) => source > cached,
            _ => false,
        },
        (None, _) => true,
        _ => false,
    };
    if !stale {
        return;
    }
    let _ = std::fs::create_dir_all(icons_cache);
    let _ = std::fs::write(cached, bytes);
}

/// « Skyward Odyssey » → « skyward-odyssey » (nom de fichier d'icône).
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            slug.extend(ch.to_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_est_stable() {
        assert_eq!(slugify("Skyward Odyssey"), "skyward-odyssey");
        assert_eq!(slugify("  Conker's BFD!! "), "conker-s-bfd");
        assert_eq!(slugify("Pokémon Écarlate"), "pokémon-écarlate");
    }

    #[test]
    fn extension_reconnue() {
        assert_eq!(Kind::from_path(Path::new("a/B.wgp")), Some(Kind::Wgp));
        assert_eq!(Kind::from_path(Path::new("a/C.LGP")), Some(Kind::Lgp));
        assert_eq!(Kind::from_path(Path::new("a/D.exe")), None);
    }

    #[test]
    fn parcours_recursif_et_index() {
        let base = std::env::temp_dir().join(format!("glauncher-walk-{}", std::process::id()));
        let deep = base.join("a/b/c/d/e/f");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(base.join("un.wgp"), b"x").unwrap();
        std::fs::write(deep.join("deux.lgp"), b"x").unwrap();
        std::fs::write(base.join("ignore.txt"), b"x").unwrap();

        let mut config = Config::default();
        config.game_dirs = vec![base.display().to_string()];
        // profondeur par défaut (3) : le paquet à 6 niveaux n'est pas vu
        let shallow = walk_packs(&config, &Snapshot::default());
        assert_eq!(shallow.rows.len(), 1);

        config.scan_depth = 0;
        let walk = walk_packs(&config, &Snapshot::default());

        let mut names: Vec<_> = walk.rows.iter().map(|row| row.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["deux", "un"]);
        assert!(walk.dirs.len() >= 7, "tous les dossiers visités");

        let mut index = Index::open(&base.join("index.sqlite")).unwrap();
        index.replace_all(&walk).expect("écriture index");
        assert_eq!(index.all().unwrap().len(), 2);

        // deuxième passage : dossiers inchangés, paquets repris de l'index
        let snapshot = index.snapshot().unwrap();
        let again = walk_packs(&config, &snapshot);
        assert_eq!(again.rows.len(), 2);

        // suppression d'un paquet, puis d'un sous-arbre entier
        index
            .remove_subtree(&deep.join("deux.lgp"))
            .expect("suppression");
        assert_eq!(index.all().unwrap().len(), 1);
        index.remove_subtree(&base.join("a")).expect("suppression");
        assert_eq!(index.all().unwrap().len(), 1);

        std::fs::remove_dir_all(&base).ok();
    }
}
