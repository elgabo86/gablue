//! Visuels SteamGridDB : grilles (covers), héros, logos et icônes.
//!
//! Deux chemins :
//! - **clé API v2** (`steamgriddb_key` en config ou `SGDB_API_KEY`) : API
//!   officielle ;
//! - **sans compte** (défaut) : API interne du site + CDN public, avec le
//!   cookie visiteur posé par la page d'accueil (`X-Requested-With` requis).
//!
//! Un seul worker, deux voies de priorité : les actions de l'utilisateur (menu
//! d'image) passent devant, et les visuels de fond sont remplacés à chaque
//! changement de vue (ce qui est à l'écran d'abord).
//!
//! L'ordre renvoyé par SteamGridDB est l'ordre « top » : on n'en réordonne
//! aucun, on prend le premier et on propose la même liste dans le menu. Les
//! votes ne sont pas exposés par l'API publique, donc reclasser sur les votes
//! (ancien comportement) faisait justement perdre la première image du top.
//!
//! Tout est écrit dans le cache (`<cache>/covers`, `heroes`, `logos`, `icons`),
//! jamais dans les dossiers de jeux : les sources restent en lecture seule.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

const BASE: &str = "https://www.steamgriddb.com";
const AGENT: &str = "glauncher/0.1 (Gablue)";
/// Nombre d'images proposées dans le menu.
const MAX_CHOICES: usize = 12;
/// Délai entre deux requêtes (le service est gratuit, on reste poli).
const THROTTLE: Duration = Duration::from_millis(250);
/// Un « pas trouvé » n'est réessayé qu'après ce délai.
const MISS_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// Extensions acceptées pour un visuel.
const IMAGE_EXT: [&str; 3] = ["png", "jpg", "webp"];

/// Type de visuel SteamGridDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtKind {
    /// Grille / cover portrait (2:3), la vignette du launcher.
    Grid,
    /// Bannière large (héros), fond de la page d'un jeu.
    Hero,
    /// Logo transparent du jeu.
    Logo,
    /// Icône carrée.
    Icon,
}

impl ArtKind {
    pub const ALL: [ArtKind; 4] = [ArtKind::Grid, ArtKind::Hero, ArtKind::Logo, ArtKind::Icon];

    /// Sous-dossier du cache.
    pub fn dir(self) -> &'static str {
        match self {
            ArtKind::Grid => "grids",
            ArtKind::Hero => "heroes",
            ArtKind::Logo => "logos",
            ArtKind::Icon => "icons",
        }
    }

    /// Nom du type côté API.
    fn api(self) -> &'static str {
        match self {
            ArtKind::Grid => "grid",
            ArtKind::Hero => "hero",
            ArtKind::Logo => "logo",
            ArtKind::Icon => "icon",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArtKind::Grid => "Grid",
            ArtKind::Hero => "Héros",
            ArtKind::Logo => "Logo",
            ArtKind::Icon => "Icône",
        }
    }
}

/// Ce que le worker doit faire.
#[derive(Clone)]
pub enum Query {
    /// Télécharge le **premier** visuel de chaque type demandé (ordre « top »
    /// de SteamGridDB, sans reclassement). `id` vient de
    /// `assets/identities.txt` (jeu identifié) : les visuels sont alors ceux
    /// de CE jeu, sans repasser par une recherche par nom.
    Auto {
        slug: String,
        name: String,
        id: Option<u64>,
        kinds: Vec<ArtKind>,
    },
    /// Renvoie toutes les images possibles d'un type, vignettes comprises
    /// (menu « choisir une image »).
    Choices {
        slug: String,
        name: String,
        id: Option<u64>,
        kind: ArtKind,
    },
    /// Recherche les **jeux** qui correspondent à un nom (identification) :
    /// l'autocomplete de SteamGridDB, sans télécharger d'image. C'est la liste
    /// où l'utilisateur choisit le bon jeu quand le premier résultat est faux
    /// (constaté 20/11 : « Marsupilami 2 » tombait sur « Hoobadventure »).
    Search { slug: String, name: String },
    /// Enregistre le choix de l'utilisateur. `url` peut être une URL du CDN ou
    /// un `file://` (icône extraite du paquet) : le fichier écrit dans le cache
    /// fait foi et n'est jamais remplacé automatiquement.
    Pick {
        slug: String,
        kind: ArtKind,
        url: String,
        ext: String,
    },
    /// Enregistre une **image locale fournie par l'utilisateur** (dialogue
    /// « choisir un fichier »). Contrairement à [`Query::Pick`], l'image est
    /// recadrée au ratio du type visuel avant d'être écrite : une photo 16:9
    /// choisie comme cover doit remplir la vignette, pas laisser des bandes.
    UseFile {
        slug: String,
        kind: ArtKind,
        path: PathBuf,
    },
    /// Extrait l'icône du paquet lui-même (`.icon.png` à la racine du squashfs,
    /// comme le fait `create-game-shortcut.sh` de Gablue).
    Icon { slug: String, pack: PathBuf },
    /// Année de sortie du jeu (page d'un jeu) : une seule requête
    /// d'autocomplete, indépendante des visuels — sans ça, un jeu dont toutes
    /// les images sont déjà en cache n'aurait jamais son année. Avec `id`, la
    /// date vient de la fiche du jeu identifié et non du premier résultat.
    Year {
        slug: String,
        name: String,
        id: Option<u64>,
    },
}

/// Jeu trouvé par l'autocomplete de SteamGridDB (menu d'identification).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGame {
    pub id: u64,
    pub name: String,
    /// Année de sortie, quand SteamGridDB la donne.
    pub year: Option<u32>,
}

/// Identification d'un jeu chez SteamGridDB : l'identifiant exact **choisi par
/// l'utilisateur**, avec le nom qui va avec (pour les recherches et la mise à
/// jour de la date).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: u64,
    pub name: String,
}

/// Visuel du « top » : URL d'origine, octets et extension.
type Best = (String, Vec<u8>, &'static str);

/// Une image proposée dans le menu, avec sa vignette déjà téléchargée.
#[derive(Debug, Clone)]
pub struct Choice {
    pub url: String,
    pub ext: String,
    pub thumb: Vec<u8>,
}

#[derive(Debug)]
pub enum CoverEvent {
    Found {
        slug: String,
        kind: ArtKind,
        bytes: Vec<u8>,
    },
    Missing {
        slug: String,
        kind: ArtKind,
    },
    Failed {
        slug: String,
        kind: ArtKind,
        error: String,
    },
    Choices {
        slug: String,
        kind: ArtKind,
        choices: Vec<Choice>,
    },
    /// Vignette prête pour UNE proposition du menu (identifiée par son `url`) :
    /// les cases sont affichées vides, puis remplies au fur et à mesure.
    Choice {
        slug: String,
        kind: ArtKind,
        url: String,
        thumb: Vec<u8>,
    },
    /// Icône extraite du paquet (voie locale, distincte des icônes SGDB).
    Icon {
        slug: String,
        bytes: Vec<u8>,
    },
    /// Réponse à `Query::Search` : les jeux proposés par l'autocomplete, ou
    /// l'erreur réseau (le menu d'identification doit pouvoir le dire).
    Games {
        slug: String,
        query: String,
        result: Result<Vec<FoundGame>, String>,
    },
    /// Année de sortie trouvée pour ce jeu (`None` = SteamGridDB n'en donne
    /// pas : c'est noté, on ne redemandera plus).
    Year {
        slug: String,
        year: Option<u32>,
    },
}

/// Travail en attente du worker, réparti en deux voies.
#[derive(Default)]
struct Job {
    /// Menu d'image et choix de l'utilisateur, puis extraction locale d'icône.
    urgent: VecDeque<Query>,
    /// Visuels des jeux à l'écran. Cette voie est REMPLACÉE à chaque
    /// changement de vue : aller loin dans la liste charge ce qui est là-bas.
    visible: VecDeque<Query>,
    /// Menu d'image ouvert : les visuels de fond sont suspendus.
    paused: bool,
}

/// Poignée d'envoi vers le worker.
#[derive(Clone)]
pub struct Queue {
    shared: Arc<(Mutex<Job>, Condvar)>,
}

impl Queue {
    /// Remplace les visuels de fond par ceux des jeux actuellement visibles.
    pub fn visible(&self, queries: Vec<Query>) {
        if let Ok(mut job) = self.shared.0.lock() {
            job.visible = queries.into();
            self.shared.1.notify_all();
        }
    }

    /// Action de l'utilisateur (menu d'image, choix) : passe devant tout.
    pub fn urgent(&self, query: Query) {
        if let Ok(mut job) = self.shared.0.lock() {
            job.urgent.push_front(query);
            self.shared.1.notify_all();
        }
    }

    /// Travail local rapide (icône d'un paquet) : devant les visuels de fond.
    pub fn local(&self, query: Query) {
        if let Ok(mut job) = self.shared.0.lock() {
            job.urgent.push_back(query);
            self.shared.1.notify_all();
        }
    }

    /// Suspend (ou relance) les visuels de fond — le menu d'image a la priorité.
    pub fn pause(&self, paused: bool) {
        if let Ok(mut job) = self.shared.0.lock() {
            job.paused = paused;
            if paused {
                job.visible.clear();
            }
            self.shared.1.notify_all();
        }
    }
}

/// Démarre le worker (un seul thread, séquentiel, throttlé sauf extraction
/// locale) et renvoie la file sur laquelle déposer le travail.
pub fn spawn<F>(cache: PathBuf, marks: PathBuf, api_key: Option<String>, emit: F) -> Queue
where
    F: Fn(CoverEvent) + Send + 'static,
{
    let queue = Queue {
        shared: Arc::new((Mutex::new(Job::default()), Condvar::new())),
    };
    let handle = queue.clone();

    let _ = std::thread::Builder::new()
        .name("glauncher-covers".into())
        .spawn(move || {
            let mut client = Client::new(api_key);
            let (lock, condvar) = &*handle.shared;

            loop {
                let query = {
                    let mut job = match lock.lock() {
                        Ok(job) => job,
                        Err(_) => return,
                    };
                    loop {
                        if let Some(query) = job.urgent.pop_front() {
                            break query;
                        }
                        if !job.paused
                            && let Some(query) = job.visible.pop_front()
                        {
                            break query;
                        }
                        job = match condvar.wait(job) {
                            Ok(job) => job,
                            Err(_) => return,
                        };
                    }
                };

                if !matches!(query, Query::Icon { .. }) {
                    std::thread::sleep(THROTTLE);
                }

                match query {
                    // --- premier visuel de chaque type (ordre « top ») ---
                    Query::Auto {
                        slug,
                        name,
                        id,
                        kinds,
                    } => {
                        for kind in kinds {
                            let dir = cache.join(kind.dir());
                            let marks = marks.join(kind.dir());
                            let fetched = client.top(&name, id, kind);
                            std::thread::sleep(THROTTLE);
                            match fetched {
                                Ok(Some((url, bytes, ext))) => {
                                    let path = save(kind, &dir, &marks, &slug, ext, &bytes, None);
                                    eprintln!(
                                        "glauncher : {} SGDB « {name} » → {}",
                                        kind.label(),
                                        path.display()
                                    );
                                    let _ = url;
                                    emit(CoverEvent::Found {
                                        slug: slug.clone(),
                                        kind,
                                        bytes,
                                    });
                                }
                                Ok(None) => {
                                    mark_missing(&dir, &marks, &slug);
                                    eprintln!(
                                        "glauncher : {} SGDB « {name} » introuvable",
                                        kind.label()
                                    );
                                    emit(CoverEvent::Missing {
                                        slug: slug.clone(),
                                        kind,
                                    });
                                }
                                Err(error) => {
                                    eprintln!(
                                        "glauncher : {} SGDB « {name} » : {error}",
                                        kind.label()
                                    );
                                    emit(CoverEvent::Failed {
                                        slug: slug.clone(),
                                        kind,
                                        error,
                                    });
                                }
                            }
                        }
                    }

                    // --- menu « choisir une image » ---
                    Query::Choices {
                        slug,
                        name,
                        id,
                        kind,
                    } => match client.choices(&name, id, kind) {
                        Ok(candidates) => {
                            let premiers: Vec<Candidate> =
                                candidates.into_iter().take(MAX_CHOICES).collect();
                            eprintln!(
                                "glauncher : {} {} proposé(s) pour « {name} », vignettes en cours",
                                premiers.len(),
                                kind.label().to_lowercase()
                            );
                            // Le menu s'ouvre TOUT DE SUITE, avec une case par
                            // proposition et sans vignette : elles se
                            // téléchargent une à une (un aller-retour chacune)
                            // et remplacent leur case dès qu'elles arrivent. Le
                            // menu attendait avant tout le lot — plusieurs
                            // secondes devant une grille vide (signalé 19/11).
                            emit(CoverEvent::Choices {
                                slug: slug.clone(),
                                kind,
                                choices: premiers
                                    .iter()
                                    .map(|candidate| Choice {
                                        url: candidate.url.clone(),
                                        ext: candidate.ext.to_string(),
                                        thumb: Vec::new(),
                                    })
                                    .collect(),
                            });
                            let mut pretes = 0;
                            for candidate in premiers {
                                if let Ok(thumb) = http_get(&candidate.thumb, &[]) {
                                    pretes += 1;
                                    emit(CoverEvent::Choice {
                                        slug: slug.clone(),
                                        kind,
                                        url: candidate.url,
                                        thumb,
                                    });
                                }
                            }
                            eprintln!("glauncher : {pretes} vignette(s) prête(s)");
                        }
                        Err(error) => {
                            eprintln!("glauncher : choix « {name} » : {error}");
                            emit(CoverEvent::Failed { slug, kind, error });
                        }
                    },

                    // --- image choisie par l'utilisateur ---
                    Query::Pick {
                        slug,
                        kind,
                        url,
                        ext,
                    } => {
                        let bytes = if let Some(path) = url.strip_prefix("file://") {
                            // icône extraite du paquet : copie locale
                            std::fs::read(path).map_err(|err| format!("lecture {path} : {err}"))
                        } else {
                            http_get(&url, &[])
                        };
                        match bytes {
                            Ok(bytes) => {
                                let dir = cache.join(kind.dir());
                                let marks = marks.join(kind.dir());
                                let path =
                                    save(kind, &dir, &marks, &slug, &ext, &bytes, Some(&url));
                                eprintln!(
                                    "glauncher : {} choisi → {}",
                                    kind.label(),
                                    path.display()
                                );
                                emit(CoverEvent::Found { slug, kind, bytes });
                            }
                            Err(error) => {
                                eprintln!("glauncher : choix ({url}) : {error}");
                                emit(CoverEvent::Failed { slug, kind, error });
                            }
                        }
                    }

                    // --- image locale fournie par l'utilisateur ---
                    Query::UseFile { slug, kind, path } => {
                        let dir = cache.join(kind.dir());
                        let marks = marks.join(kind.dir());
                        match std::fs::read(&path) {
                            Ok(bytes) => match adapter(kind, &bytes) {
                                Some((adaptees, ext)) => {
                                    // marqueur `.choice` : cette image fait foi,
                                    // elle ne sera jamais remplacée toute seule
                                    let url = format!("file://{}", path.display());
                                    let ecrit =
                                        ecrire(&dir, &marks, &slug, ext, &adaptees, Some(&url));
                                    eprintln!(
                                        "glauncher : {} perso « {} » → {}",
                                        kind.label(),
                                        path.display(),
                                        ecrit.display()
                                    );
                                    emit(CoverEvent::Found {
                                        slug,
                                        kind,
                                        bytes: adaptees,
                                    });
                                }
                                None => {
                                    let error = format!(
                                        "image illisible (png, jpg ou webp) : {}",
                                        path.display()
                                    );
                                    eprintln!("glauncher : {error}");
                                    emit(CoverEvent::Failed { slug, kind, error });
                                }
                            },
                            Err(error) => {
                                let error = format!("lecture {} : {error}", path.display());
                                eprintln!("glauncher : {error}");
                                emit(CoverEvent::Failed { slug, kind, error });
                            }
                        }
                    }

                    // --- identification : la liste des jeux (sans visuel) ---
                    Query::Search { slug, name } => match client.games(&name) {
                        Ok(games) => {
                            let found = if games.is_empty() {
                                "aucun jeu".to_string()
                            } else {
                                format!("{} jeu(x)", games.len())
                            };
                            eprintln!("glauncher : autocomplete « {name} » : {found}");
                            emit(CoverEvent::Games {
                                slug,
                                query: name,
                                result: Ok(games),
                            });
                        }
                        Err(error) => {
                            eprintln!("glauncher : autocomplete « {name} » : {error}");
                            emit(CoverEvent::Games {
                                slug,
                                query: name,
                                result: Err(error),
                            });
                        }
                    },

                    // --- année de sortie (page d'un jeu) ---
                    Query::Year { slug, name, id } => match client.release_year(&name, id) {
                        Ok(year) => {
                            if year.is_none() {
                                eprintln!("glauncher : année de « {name} » inconnue");
                            }
                            emit(CoverEvent::Year { slug, year });
                        }
                        // erreur réseau : on ne note RIEN, la question pourra
                        // être reposée (contrairement à « SGDB n'a pas de date »)
                        Err(error) => {
                            eprintln!("glauncher : année « {name} » : {error}")
                        }
                    },

                    // --- icône du paquet ---
                    Query::Icon { slug, pack } => match pack_icon(&pack) {
                        Some(bytes) => {
                            let dir = cache.join("icons").join("pack");
                            let _ = std::fs::create_dir_all(&dir);
                            let path = dir.join(format!("{slug}.png"));
                            let _ = std::fs::write(&path, &bytes);
                            emit(CoverEvent::Icon { slug, bytes });
                        }
                        None => {
                            eprintln!(
                                "glauncher : pas d'icône dans {}",
                                pack.file_name().unwrap_or_default().to_string_lossy()
                            );
                        }
                    },
                }
            }
        });

    queue
}

/// Bilan d'un `--update`.
#[derive(Debug, Default)]
pub struct UpdateReport {
    pub downloaded: usize,
    pub missing: usize,
    pub failed: usize,
    pub skipped: usize,
    /// Années de sortie apprises (une requête par jeu, une seule fois).
    pub years: usize,
}

/// Fichier des identifications, dans `assets/` : l'identifiant SteamGridDB
/// choisi pour un paquet, partagé par Syncthing entre postes (comme les
/// visuels et `years.txt`). C'est ce qui empêche un jeu mal reconnu de
/// retomber sur « le premier résultat de l'autocomplete » ailleurs.
///
/// Format : `slug=<id>` puis, après une tabulation, le nom SteamGridDB —
/// `marsupilami-2-salsa-palombia=5334897    Marsupilami`. Le nom n'est pas
/// indispensable au téléchargement (l'id suffit), mais il donne une ligne
/// lisible dans le fichier partagé et sert à la mise à jour de la date.
pub fn identities_path(assets: &Path) -> PathBuf {
    assets.join("identities.txt")
}

/// Lit `identities.txt` (fichier absent ou lignes illisibles ignorées).
pub fn load_identities(path: &Path) -> BTreeMap<String, Identity> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut identities = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((slug, value)) = line.split_once('=') else {
            continue;
        };
        let slug = slug.trim();
        if slug.is_empty() {
            continue;
        }
        let value = value.trim();
        let (id, name) = match value.split_once('\t') {
            Some((id, name)) => (id.trim(), name.trim()),
            None => match value.split_once(' ') {
                Some((id, name)) => (id.trim(), name.trim()),
                None => (value, ""),
            },
        };
        let Ok(id) = id.parse::<u64>() else {
            continue;
        };
        identities.insert(
            slug.to_string(),
            Identity {
                id,
                name: name.to_string(),
            },
        );
    }
    identities
}

/// Écrit `identities.txt` en **fusionnant** ce qui est déjà sur le disque
/// (temporaire + renommage) : `--update` et l'interface peuvent écrire tous
/// les deux, et `assets/` est synchronisé entre machines.
pub fn save_identities(path: &Path, identities: &BTreeMap<String, Identity>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut all = load_identities(path);
    for (slug, identity) in identities {
        all.insert(slug.clone(), identity.clone());
    }
    let mut text = String::from("# identification SteamGridDB : slug=<id>\t<nom>\n");
    for (slug, identity) in &all {
        if identity.name.is_empty() {
            text.push_str(&format!("{slug}={}\n", identity.id));
        } else {
            text.push_str(&format!("{slug}={}\t{}\n", identity.id, identity.name));
        }
    }
    write_atomic(path, text.as_bytes());
}

/// Oublie les visuels d'un jeu (identification changée, jeu mal reconnu) :
/// les quatre fichiers et les marqueurs de CETTE machine. La miniature est
/// refaite à partir de la nouvelle cover ; l'appelant efface les handles en
/// mémoire et redemande les visuels du jeu identifié.
pub fn forget(assets: &Path, marks_root: &Path, slug: &str) {
    for kind in ArtKind::ALL {
        let dir = assets.join(kind.dir());
        for ext in IMAGE_EXT {
            let _ = std::fs::remove_file(dir.join(format!("{slug}.{ext}")));
        }
        let marks = marks_root.join(kind.dir());
        let _ = std::fs::remove_file(marks.join(format!("{slug}.miss")));
        let _ = std::fs::remove_file(marks.join(format!("{slug}.choice")));
    }
}

/// Fichier des années de sortie, dans `assets/` : c'est le dossier partagé
/// (Syncthing), donc les années suivent les visuels chez un autre poste.
///
/// Format : `slug=1998` par ligne, trié par slug ; `slug=` veut dire
/// « regardé, SteamGridDB ne donne pas de date » — une année connue n'est
/// **jamais** redemandée, ni par l'interface ni par `--update`.
pub fn years_path(assets: &Path) -> PathBuf {
    assets.join("years.txt")
}

/// Lit `years.txt` (un fichier absent ou illisible donne une table vide).
pub fn load_years(path: &Path) -> BTreeMap<String, Option<u32>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut years = BTreeMap::new();
    for line in text.lines() {
        let Some((slug, value)) = line.split_once('=') else {
            continue;
        };
        let slug = slug.trim();
        if slug.is_empty() {
            continue;
        }
        let value = value.trim();
        years.insert(
            slug.to_string(),
            if value.is_empty() {
                None
            } else {
                value.parse::<u32>().ok()
            },
        );
    }
    years
}

/// Écrit `years.txt` (temporaire + renommage), en **fusionnant** ce qui est
/// déjà sur le disque : `--update` et l'interface peuvent écrire tous les deux.
pub fn save_years(path: &Path, years: &BTreeMap<String, Option<u32>>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut all = load_years(path);
    for (slug, year) in years {
        all.insert(slug.clone(), *year);
    }
    let mut text = String::new();
    for (slug, year) in &all {
        match year {
            Some(year) => text.push_str(&format!("{slug}={year}\n")),
            None => text.push_str(&format!("{slug}=\n")),
        }
    }
    write_atomic(path, text.as_bytes());
}

/// Nom et identifiant à utiliser pour un paquet : l'identification partagée
/// s'il y en a une, sinon le nom du paquet (comportement historique, premier
/// résultat de l'autocomplete).
fn who(identities: &BTreeMap<String, Identity>, slug: &str, name: &str) -> (String, Option<u64>) {
    match identities.get(slug) {
        Some(identity) if !identity.name.is_empty() => (identity.name.clone(), Some(identity.id)),
        Some(identity) => (name.to_string(), Some(identity.id)),
        None => (name.to_string(), None),
    }
}

/// Apprend l'année d'un jeu si elle manque, et l'écrit tout de suite (arrêt
/// brutal = rien de perdu). Rend `true` si une requête a eu lieu : l'appelant
/// gère l'attente entre deux requêtes.
#[allow(clippy::too_many_arguments)]
fn learn_year<F>(
    client: &mut Client,
    path: &Path,
    years: &mut BTreeMap<String, Option<u32>>,
    slug: &str,
    name: &str,
    id: Option<u64>,
    totals: &mut UpdateReport,
    report: &mut F,
) -> bool
where
    F: FnMut(String),
{
    if years.contains_key(slug) {
        return false;
    }
    match client.release_year(name, id) {
        Ok(year) => {
            years.insert(slug.to_string(), year);
            totals.years += 1;
            save_years(path, years);
            report(match year {
                Some(year) => format!("année « {name} » → {year}"),
                None => format!("année « {name} » inconnue"),
            });
        }
        Err(error) => {
            totals.failed += 1;
            report(format!("année « {name} » : {error}"));
        }
    }
    true
}

/// `--date` : n'apprend que les années de sortie manquantes, sans toucher aux
/// visuels. Une requête par jeu inconnu, 250 ms entre deux.
pub fn update_years<F>(
    assets: &Path,
    api_key: Option<String>,
    games: &[(String, String)],
    report: &mut F,
) -> UpdateReport
where
    F: FnMut(String),
{
    let mut client = Client::new(api_key);
    let mut totals = UpdateReport::default();
    let path = years_path(assets);
    let mut years = load_years(&path);
    let identities = load_identities(&identities_path(assets));

    for (slug, name) in games {
        let (name, id) = who(&identities, slug, name);
        if !learn_year(
            &mut client,
            &path,
            &mut years,
            slug,
            &name,
            id,
            &mut totals,
            report,
        ) {
            totals.skipped += 1;
            continue;
        }
        std::thread::sleep(THROTTLE);
    }
    totals
}

/// Télécharge ce qui manque pour TOUS les jeux listés (mode `--update`).
///
/// Même politique que l'interface : un visuel présent n'est jamais redemandé,
/// un « introuvable » récent n'est pas réessayé, et l'icône du paquet est
/// tentée avant celle de SteamGridDB. `report` reçoit une ligne par étape.
pub fn update_all<F>(
    assets: &Path,
    marks_root: &Path,
    pack_icons: &Path,
    api_key: Option<String>,
    games: &[(String, String, PathBuf)],
    report: &mut F,
) -> UpdateReport
where
    F: FnMut(String),
{
    let mut client = Client::new(api_key);
    let mut totals = UpdateReport::default();
    // années déjà connues (fichier partagé) : une seule requête par jeu
    let years_file = years_path(assets);
    let mut years = load_years(&years_file);
    let identities = load_identities(&identities_path(assets));

    for (slug, name, pack) in games {
        // jeu identifié : ce sont SES visuels et SA date, pas ceux du premier
        // résultat de la recherche par nom
        let (name, id) = who(&identities, slug, name);
        // --- année de sortie : une requête, puis plus jamais ---
        if learn_year(
            &mut client,
            &years_file,
            &mut years,
            slug,
            &name,
            id,
            &mut totals,
            report,
        ) {
            std::thread::sleep(THROTTLE);
        }
        for kind in ArtKind::ALL {
            let dir = assets.join(kind.dir());
            let marks = marks_root.join(kind.dir());
            if cached(&dir, slug).is_some() {
                totals.skipped += 1;
                continue;
            }
            if kind == ArtKind::Icon {
                // icône du paquet d'abord : locale, gratuite et souvent la bonne
                if let Some(bytes) = pack_icon(pack) {
                    let _ = std::fs::create_dir_all(pack_icons);
                    write_atomic(&pack_icons.join(format!("{slug}.png")), &bytes);
                    totals.skipped += 1;
                    continue;
                }
            }
            if recently_missed(&marks, slug) {
                totals.skipped += 1;
                continue;
            }
            match client.top(&name, id, kind) {
                Ok(Some((url, bytes, ext))) => {
                    let path = save(kind, &dir, &marks, slug, ext, &bytes, None);
                    let _ = url;
                    totals.downloaded += 1;
                    report(format!("{} « {name} » → {}", kind.label(), path.display()));
                }
                Ok(None) => {
                    mark_missing(&dir, &marks, slug);
                    totals.missing += 1;
                    report(format!("{} « {name} » introuvable", kind.label()));
                }
                Err(error) => {
                    totals.failed += 1;
                    report(format!("{} « {name} » : {error}", kind.label()));
                }
            }
            std::thread::sleep(THROTTLE);
        }
    }
    totals
}

/// Visuels dont l'overlay manette a besoin : le logo (bandeau au-dessus de
/// l'en-tête) et l'icône (à la place de celle de la fenêtre).
pub const OVERLAY_KINDS: [ArtKind; 2] = [ArtKind::Logo, ArtKind::Icon];

/// Format et dimensions **cibles** d'un visuel du cache.
///
/// SteamGridDB sert du très lourd : un héros fait 3840×1240 et pèse jusqu'à
/// **12 Mo** (mesuré 19/11 sur 103 héros = 205 Mo), une grille 1024×1536. Or
/// une bande de héros fait la largeur du launcher (1280, 1920 au plus) et une
/// vignette 300 px. D'où : héros et grilles en **JPEG** (opaques, la qualité 88
/// est invisible à ces tailles), logo et icône en **PNG** — l'overlay `gpad` ne
/// décode que le PNG, et il faut l'alpha.
fn cible(kind: ArtKind) -> (&'static str, Option<u32>, Option<u32>) {
    match kind {
        ArtKind::Grid => ("jpg", Some(600), Some(900)),
        ArtKind::Hero => ("jpg", Some(1920), None),
        ArtKind::Logo => ("png", Some(1000), None),
        ArtKind::Icon => ("png", Some(256), Some(256)),
    }
}

/// Ré-encode `image` au format cible (`jpg` qualité 88, sinon PNG).
fn encoder(image: &image::DynamicImage, ext: &str) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    if ext == "jpg" {
        let rgb = image.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
            .encode_image(&rgb)
            .ok()?;
    } else {
        image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    }
    Some(out.into_inner())
}

/// Réduit une image à la boîte `max_w × max_h` sans la déformer (jamais
/// agrandie).
fn reduire(
    image: image::DynamicImage,
    max_w: Option<u32>,
    max_h: Option<u32>,
) -> image::DynamicImage {
    let (largeur, hauteur) = (image.width(), image.height());
    let mut echelle = 1.0f64;
    if let Some(max) = max_w.filter(|max| largeur > *max) {
        echelle = echelle.min(max as f64 / largeur as f64);
    }
    if let Some(max) = max_h.filter(|max| hauteur > *max) {
        echelle = echelle.min(max as f64 / hauteur as f64);
    }
    if echelle >= 1.0 {
        return image;
    }
    let largeur = ((largeur as f64 * echelle).round() as u32).max(1);
    let hauteur = ((hauteur as f64 * echelle).round() as u32).max(1);
    image.resize(largeur, hauteur, image::imageops::FilterType::Lanczos3)
}

/// **Remplit** exactement `w × h` : recadrage centré au ratio de la cible puis
/// redimensionnement (agrandissement compris). C'est ce qu'il faut pour une
/// image fournie par l'utilisateur : une photo 16:9 posée en cover garderait
/// sinon deux bandes noires dans la vignette.
fn remplir(image: image::DynamicImage, w: u32, h: u32) -> image::DynamicImage {
    let (largeur, hauteur) = (image.width(), image.height());
    let cible = w as f64 / h as f64;
    let actuel = largeur as f64 / hauteur as f64;
    // trop large : on rogne les côtés ; trop haute : on rogne haut et bas
    let (garde_w, garde_h) = if actuel > cible {
        (((hauteur as f64 * cible).round() as u32).max(1), hauteur)
    } else {
        (largeur, ((largeur as f64 / cible).round() as u32).max(1))
    };
    let x = (largeur - garde_w) / 2;
    let y = (hauteur - garde_h) / 2;
    image
        .crop_imm(x, y, garde_w, garde_h)
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
}

/// Adapte une image **fournie par l'utilisateur** au type visuel : recadrée
/// (cover 2:3, héros 1920×620, icône carrée) puis mise à la taille cible.
/// Le **logo** fait exception : jamais recadré (un logo rogné perd son texte),
/// seulement réduit s'il est trop large. `None` si l'image n'est pas lisible.
fn adapter(kind: ArtKind, bytes: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    let (ext, max_w, max_h) = cible(kind);
    let image = image::load_from_memory(bytes).ok()?;
    let image = match kind {
        ArtKind::Grid => remplir(image, 600, 900),
        ArtKind::Hero => remplir(image, 1920, 620),
        ArtKind::Icon => remplir(image, 256, 256),
        ArtKind::Logo => reduire(image, max_w, max_h),
    };
    Some((encoder(&image, ext)?, ext))
}

/// Ré-encode un visuel au format et à la taille cibles (ratio conservé par
/// ajustement dans la boîte). `None` si l'image n'est pas décodable : l'appelant
/// garde alors l'original, jamais de perte.
fn normaliser(kind: ArtKind, bytes: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    let (ext, max_w, max_h) = cible(kind);
    let image = image::load_from_memory(bytes).ok()?;
    let image = reduire(image, max_w, max_h);
    Some((encoder(&image, ext)?, ext))
}

/// Visuel déjà au format et à la taille cibles — on ne réécrit pas : un JPEG
/// ré-encodé à chaque passe perdrait de la qualité pour rien. Seules les
/// dimensions sont lues (en-tête), pas l'image entière.
fn deja_bon(kind: ArtKind, dir: &Path, slug: &str) -> bool {
    let (ext, max_w, max_h) = cible(kind);
    let Ok(bytes) = std::fs::read(dir.join(format!("{slug}.{ext}"))) else {
        return false;
    };
    let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|lecteur| lecteur.into_dimensions().ok());
    match dimensions {
        Some((largeur, hauteur)) => {
            max_w.is_none_or(|max| largeur <= max) && max_h.is_none_or(|max| hauteur <= max)
        }
        None => false,
    }
}

/// Télécharge les visuels **manquants d'un seul paquet** (`--art <paquet>`,
/// lancé en fond par gwine/lgplaunch au démarrage du jeu).
///
/// L'icône du paquet (`.icon.png` du squashfs) passe devant celle de
/// SteamGridDB, comme dans le launcher. Un visuel déjà au bon format n'est
/// jamais redemandé ; un « introuvable » récent n'est pas réessayé. Tout est
/// ramené au format de l'overlay (PNG pour logo et icône) et à une taille
/// raisonnable (voir [`cible`]).
pub fn fetch_game<F>(
    assets: &Path,
    marks_root: &Path,
    pack_icons: &Path,
    api_key: Option<String>,
    slug: &str,
    name: &str,
    pack: &Path,
    report: &mut F,
) -> UpdateReport
where
    F: FnMut(String),
{
    let mut client = Client::new(api_key);
    let mut totals = UpdateReport::default();
    // identification partagée : le logo et l'icône sont ceux du jeu choisi
    let identities = load_identities(&identities_path(assets));
    let (name, id) = who(&identities, slug, name);
    let name = name.as_str();

    for kind in OVERLAY_KINDS {
        let dir = assets.join(kind.dir());
        let marks = marks_root.join(kind.dir());
        // déjà au bon format et à la bonne taille : rien à faire
        if deja_bon(kind, &dir, slug) {
            totals.skipped += 1;
            continue;
        }
        // un visuel existe en un autre format (ou trop grand) : on le réécrit
        if let Some(autre) = cached(&dir, slug)
            && let Ok(bytes) = std::fs::read(&autre)
            && normaliser(kind, &bytes).is_some()
        {
            let ext = autre
                .extension()
                .map(|ext| ext.to_string_lossy().to_string())
                .unwrap_or_default();
            let path = save(kind, &dir, &marks, slug, &ext, &bytes, None);
            totals.downloaded += 1;
            report(format!(
                "{} « {name} » mis au format de l'overlay → {}",
                kind.label(),
                path.display()
            ));
            continue;
        }
        if kind == ArtKind::Icon {
            // l'icône choisie par l'utilisateur gagne, puis celle du paquet
            if chosen(&dir, &marks, slug).is_some() {
                totals.skipped += 1;
                continue;
            }
            if pack_icons.join(format!("{slug}.png")).is_file() {
                totals.skipped += 1;
                continue;
            }
            if let Some(bytes) = pack_icon(pack) {
                let _ = std::fs::create_dir_all(pack_icons);
                write_atomic(&pack_icons.join(format!("{slug}.png")), &bytes);
                totals.downloaded += 1;
                report(format!("Icône « {name} » extraite du paquet"));
                continue;
            }
        }
        if recently_missed(&marks, slug) {
            totals.skipped += 1;
            continue;
        }
        match client.top(name, id, kind) {
            Ok(Some((_url, bytes, ext))) => {
                if normaliser(kind, &bytes).is_none() {
                    totals.failed += 1;
                    report(format!("{} « {name} » : image illisible", kind.label()));
                    continue;
                }
                let path = save(kind, &dir, &marks, slug, ext, &bytes, None);
                totals.downloaded += 1;
                report(format!("{} « {name} » → {}", kind.label(), path.display()));
            }
            Ok(None) => {
                mark_missing(&dir, &marks, slug);
                totals.missing += 1;
                report(format!("{} « {name} » introuvable", kind.label()));
            }
            Err(error) => {
                totals.failed += 1;
                report(format!("{} « {name} » : {error}", kind.label()));
            }
        }
        std::thread::sleep(THROTTLE);
    }
    totals
}

/// Bilan d'un `--shrink`.
#[derive(Debug, Default)]
pub struct ShrinkReport {
    /// fichiers réécrits (format ou taille)
    pub rewritten: usize,
    /// octets avant / après
    pub before: u64,
    pub after: u64,
}

/// Ramène **tout le cache** `assets/` au format et à la taille de [`cible`]
/// (`--shrink`). Une passe unique : les téléchargements suivants sont déjà
/// normalisés, et un fichier déjà bon n'est jamais réécrit (pas de perte JPEG).
pub fn shrink_assets<F>(assets: &Path, thumbs: &Path, report: &mut F) -> ShrinkReport
where
    F: FnMut(String),
{
    let mut totals = ShrinkReport::default();

    for kind in ArtKind::ALL {
        let dir = assets.join(kind.dir());
        let Ok(entrees) = std::fs::read_dir(&dir) else {
            continue;
        };
        let (mut faits, mut avant, mut apres) = (0usize, 0u64, 0u64);
        for entree in entrees.flatten() {
            let chemin = entree.path();
            let Some(slug) = chemin
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
            else {
                continue;
            };
            let Some(ext) = chemin
                .extension()
                .map(|ext| ext.to_string_lossy().to_string())
            else {
                continue;
            };
            if !IMAGE_EXT.contains(&ext.to_ascii_lowercase().as_str()) {
                continue;
            }
            if deja_bon(kind, &dir, &slug) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&chemin) else {
                continue;
            };
            let Some((normalisees, nouvelle)) = normaliser(kind, &bytes) else {
                continue;
            };
            let destination = dir.join(format!("{slug}.{nouvelle}"));
            write_atomic(&destination, &normalisees);
            if destination != chemin {
                let _ = std::fs::remove_file(&chemin);
            }
            // la vignette est dérivée de la grille : elle sera refaite
            if kind == ArtKind::Grid {
                for autre in IMAGE_EXT {
                    let _ = std::fs::remove_file(thumbs.join(format!("{slug}.{autre}")));
                }
            }
            faits += 1;
            avant += bytes.len() as u64;
            apres += normalisees.len() as u64;
        }
        if faits > 0 {
            report(format!(
                "{} : {faits} réécrit(s), {} → {}",
                kind.label(),
                poids(avant),
                poids(apres)
            ));
        }
        totals.rewritten += faits;
        totals.before += avant;
        totals.after += apres;
    }
    totals
}

/// Taille lisible (Mo, Ko).
fn poids(octets: u64) -> String {
    if octets >= 1024 * 1024 {
        format!("{:.1} Mo", octets as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0} Ko", octets as f64 / 1024.0)
    }
}

/// Visuel en cache pour un jeu (png, jpg ou webp).
pub fn cached(dir: &Path, slug: &str) -> Option<PathBuf> {
    IMAGE_EXT
        .into_iter()
        .map(|ext| dir.join(format!("{slug}.{ext}")))
        .find(|path| path.is_file())
}

/// Visuel **choisi par l'utilisateur** : il y a un marqueur `.choice` dans le
/// dossier des marqueurs (jamais à côté de l'image : `assets/` est destiné à
/// être partagé par Syncthing, un marqueur personnel n'y a pas sa place).
pub fn chosen(dir: &Path, marks: &Path, slug: &str) -> Option<PathBuf> {
    if !marks.join(format!("{slug}.choice")).is_file() {
        return None;
    }
    cached(dir, slug)
}

/// Vrai si au moins un des quatre visuels existe déjà pour ce jeu : c'est ce
/// qui déclenche la question « remplacer les visuels ? » quand on identifie un
/// paquet qui a déjà des images (elles peuvent avoir été choisies à la main).
pub fn has_art(assets: &Path, slug: &str) -> bool {
    ArtKind::ALL
        .iter()
        .any(|kind| cached(&assets.join(kind.dir()), slug).is_some())
}

/// Vrai si un échec récent est déjà noté pour ce jeu.
pub fn recently_missed(marks: &Path, slug: &str) -> bool {
    let marker = marks.join(format!("{slug}.miss"));
    let Ok(modified) = std::fs::metadata(&marker).and_then(|meta| meta.modified()) else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age < MISS_TTL)
        .unwrap_or(false)
}

/// Écrit un visuel dans le cache. `url` non nul = choix de l'utilisateur : le
/// marqueur `.choice` est posé. L'image est ramenée au format et à la taille de
/// [`cible`] (un héros de 12 Mo ne sert à rien dans une bande de 1280 px) ; si
/// elle n'est pas décodable, elle est écrite telle quelle. Les autres formats du
/// même jeu sont retirés : un seul fichier fait foi, c'est aussi ce que voit
/// l'autre machine via Syncthing.
fn save(
    kind: ArtKind,
    dir: &Path,
    marks: &Path,
    slug: &str,
    ext: &str,
    bytes: &[u8],
    url: Option<&str>,
) -> PathBuf {
    let (bytes, ext) = match normaliser(kind, bytes) {
        Some((normalisees, cible)) => (normalisees, cible),
        None => (bytes.to_vec(), ext),
    };
    ecrire(dir, marks, slug, ext, &bytes, url)
}

/// Écriture commune d'un visuel : un seul fichier par type (les autres
/// extensions sont retirées), marqueur `.choice` quand c'est un choix de
/// l'utilisateur, écriture atomique.
fn ecrire(
    dir: &Path,
    marks: &Path,
    slug: &str,
    ext: &str,
    bytes: &[u8],
    url: Option<&str>,
) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    for other in IMAGE_EXT {
        if other != ext {
            let _ = std::fs::remove_file(dir.join(format!("{slug}.{other}")));
        }
    }
    let path = dir.join(format!("{slug}.{ext}"));
    // Écriture ATOMIQUE (`.part` puis rename) : le dossier `assets/` peut être
    // synchronisé (Syncthing) entre plusieurs machines, et un fichier à moitié
    // écrit se propagerait tel quel.
    write_atomic(&path, bytes);
    if let Some(url) = url {
        let _ = std::fs::create_dir_all(marks);
        write_atomic(&marks.join(format!("{slug}.choice")), url.as_bytes());
        let _ = std::fs::remove_file(marks.join(format!("{slug}.miss")));
    }
    path
}

/// Écrit un fichier par `.part` + rename : jamais de contenu partiel visible.
fn write_atomic(path: &Path, bytes: &[u8]) {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    if std::fs::write(&part, bytes).is_ok() {
        let _ = std::fs::rename(&part, path);
    } else {
        let _ = std::fs::remove_file(&part);
    }
}

fn mark_missing(dir: &Path, marks: &Path, slug: &str) {
    let _ = std::fs::create_dir_all(dir);
    for ext in IMAGE_EXT {
        let _ = std::fs::remove_file(dir.join(format!("{slug}.{ext}")));
    }
    let _ = std::fs::create_dir_all(marks);
    write_atomic(&marks.join(format!("{slug}.miss")), b"");
}

/// Icône contenue dans le paquet : `.icon.png` à la racine du squashfs, comme
/// le lit `create-game-shortcut.sh` de Gablue (`unsquashfs -cat` pour les
/// `.lgp`, montage squashfuse pour les `.wgp` — `-cat` marche aussi, et c'est
/// bien plus rapide : ~15 ms par paquet).
fn pack_icon(pack: &Path) -> Option<Vec<u8>> {
    let output = std::process::Command::new("unsquashfs")
        .args(["-no-progress", "-cat"])
        .arg(pack)
        .arg(".icon.png")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() || output.stdout.len() < 64 {
        return None;
    }
    Some(output.stdout)
}

// --- client ----------------------------------------------------------------

struct Client {
    key: Option<String>,
    cookie: Option<String>,
}

/// Candidat renvoyé par SteamGridDB.
#[derive(Debug, Clone)]
struct Candidate {
    url: String,
    thumb: String,
    ext: &'static str,
}

impl Client {
    fn new(key: Option<String>) -> Self {
        Self {
            key: key.filter(|key| !key.trim().is_empty()),
            cookie: None,
        }
    }

    /// Premier visuel du « top » pour ce type, téléchargé. `id` (jeu
    /// identifié) court-circuite la recherche par nom.
    fn top(&mut self, name: &str, id: Option<u64>, kind: ArtKind) -> Result<Option<Best>, String> {
        let Some(best) = self.choices(name, id, kind)?.into_iter().next() else {
            return Ok(None);
        };
        let bytes = http_get(&best.url, &[])?;
        if bytes.len() < 512 {
            return Err(format!("{} vide ({})", kind.label(), best.url));
        }
        Ok(Some((best.url, bytes, best.ext)))
    }

    /// Toutes les images d'un type, dans l'ordre « top » de SteamGridDB.
    fn choices(
        &mut self,
        name: &str,
        id: Option<u64>,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        match self.key.clone() {
            Some(key) => self.candidates_official(&key, name, id, kind),
            None => self.candidates_public(name, id, kind),
        }
    }

    // --- API interne (sans compte) ---

    /// Recherche sans compte, avec **une seconde chance** : quand la session
    /// visiteur est périmée (Cloudflare répond une page vide → « réponse
    /// vide »), on en reprend une et on réessaie une fois. Sans ça, un jeu
    /// restait sans résultat — y compris dans le menu « changer d'image ».
    fn candidates_public(
        &mut self,
        name: &str,
        id: Option<u64>,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        match self.candidates_public_once(name, id, kind) {
            Ok(list) => Ok(list),
            Err(first) => {
                eprintln!("glauncher : SGDB « {name} » : {first} — nouvelle session, nouvel essai");
                self.cookie = None;
                std::thread::sleep(Duration::from_millis(600));
                self.candidates_public_once(name, id, kind)
            }
        }
    }

    fn candidates_public_once(
        &mut self,
        name: &str,
        id: Option<u64>,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        let cookie = self.visitor_cookie()?.to_string();
        let headers = [
            ("Cookie", cookie.as_str()),
            ("X-Requested-With", "XMLHttpRequest"),
            ("Referer", BASE),
        ];

        // jeu identifié : son id, sans recherche par nom (le nom du paquet
        // peut ne rien trouver ou trouver un autre épisode)
        let game_id = match id {
            Some(id) => id,
            None => {
                let term = urlencode(name);
                let url = format!("{BASE}/api/public/search/autocomplete?term={term}");
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                let Some(game_id) = first_game(&json) else {
                    return Ok(Vec::new());
                };
                game_id
            }
        };

        let payload = json!({
            "game_id": [game_id],
            "asset_type": kind.api(),
            "limit": 24,
            "page": 0,
        })
        .to_string();
        let mut post_headers = headers.to_vec();
        post_headers.push(("Content-Type", "application/json"));
        let body = http_post(
            &format!("{BASE}/api/public/search/assets"),
            &post_headers,
            &payload,
        )?;
        let json = parse_json(&body)?;
        let assets = &json["data"]["assets"];

        // Session visiteur périmée (Cloudflare) : la réponse est vide ou d'une
        // autre forme, `assets` n'est pas un tableau. C'est une ERREUR, pas un
        // « ce jeu n'a aucune image » — sinon on posait un `.miss` de 30 jours
        // sur des jeux qui ont bien une cover (constaté 18/11). Un tableau
        // vide, lui, veut bien dire « aucune image de ce type ».
        if !assets.is_array() {
            return Err("réponse vide (session périmée ?)".to_string());
        }

        Ok(candidates(assets, kind))
    }

    fn visitor_cookie(&mut self) -> Result<&str, String> {
        if self.cookie.is_none() {
            let response = agent()
                .get(BASE)
                .header("User-Agent", AGENT)
                .call()
                .map_err(|err| format!("accueil SGDB : {err}"))?;
            let cookie = response
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|value| value.to_str().ok())
                .filter_map(|value| value.split(';').next())
                .collect::<Vec<_>>()
                .join("; ");
            if cookie.is_empty() {
                return Err("cookie visiteur SGDB absent".into());
            }
            self.cookie = Some(cookie);
        }
        Ok(self.cookie.as_deref().unwrap_or_default())
    }

    /// Année de sortie du jeu, dans l'API en service (clé ou publique). Avec
    /// un `id` (jeu identifié), la date vient de la fiche de CE jeu ; sans,
    /// c'est celle du premier résultat de l'autocomplete. Une seule requête,
    /// sans visuels.
    fn release_year(&mut self, name: &str, id: Option<u64>) -> Result<Option<u32>, String> {
        match self.key.clone() {
            Some(key) => {
                let auth = format!("Bearer {key}");
                let headers = [("Authorization", auth.as_str())];
                let url = match id {
                    Some(id) => format!("{BASE}/api/v2/games/id/{id}"),
                    None => format!("{BASE}/api/v2/search/autocomplete/{}", urlencode(name)),
                };
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                Ok(if id.is_some() {
                    game_year(&json["data"])
                } else {
                    first_game_year(&json)
                })
            }
            None => {
                let cookie = self.visitor_cookie()?.to_string();
                let headers = [
                    ("Cookie", cookie.as_str()),
                    ("X-Requested-With", "XMLHttpRequest"),
                    ("Referer", BASE),
                ];
                let url = match id {
                    Some(id) => format!("{BASE}/api/public/game/{id}"),
                    None => format!(
                        "{BASE}/api/public/search/autocomplete?term={}",
                        urlencode(name)
                    ),
                };
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                Ok(if id.is_some() {
                    game_year(&json["data"]["game"])
                } else {
                    first_game_year(&json)
                })
            }
        }
    }

    /// Jeux proposés par l'autocomplete (identification). L'ordre est celui de
    /// SteamGridDB, on n'y touche pas : l'utilisateur choisit dans la liste.
    fn games(&mut self, name: &str) -> Result<Vec<FoundGame>, String> {
        let term = urlencode(name);
        match self.key.clone() {
            Some(key) => {
                let auth = format!("Bearer {key}");
                let url = format!("{BASE}/api/v2/search/autocomplete/{term}");
                let body = http_get_text(&url, &[("Authorization", auth.as_str())])?;
                let json = parse_json(&body)?;
                Ok(games_from(&json["data"]))
            }
            None => {
                let cookie = self.visitor_cookie()?.to_string();
                let headers = [
                    ("Cookie", cookie.as_str()),
                    ("X-Requested-With", "XMLHttpRequest"),
                    ("Referer", BASE),
                ];
                let url = format!("{BASE}/api/public/search/autocomplete?term={term}");
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                Ok(games_from(&json["data"]))
            }
        }
    }

    // --- API v2 officielle (clé) ---

    fn candidates_official(
        &mut self,
        key: &str,
        name: &str,
        id: Option<u64>,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        let auth = format!("Bearer {key}");
        let headers = [("Authorization", auth.as_str())];

        // jeu identifié : son id directement
        let game_id = match id {
            Some(id) => id,
            None => {
                let term = urlencode(name);
                let url = format!("{BASE}/api/v2/search/autocomplete/{term}");
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                let Some(game_id) = first_game(&json) else {
                    return Ok(Vec::new());
                };
                game_id
            }
        };

        let url = format!(
            "{BASE}/api/v2/{}/{game_id}?types=static&nsfw=false&humor=false",
            match kind {
                ArtKind::Grid => "grids/game",
                ArtKind::Hero => "heroes/game",
                ArtKind::Logo => "logos/game",
                ArtKind::Icon => "icons/game",
            }
        );
        let body = http_get_text(&url, &headers)?;
        let json = parse_json(&body)?;

        Ok(candidates(&json["data"], kind))
    }
}

/// Identifiant du premier jeu de l'autocomplete (`data[0]`).
fn first_game(json: &Value) -> Option<u64> {
    json["data"].as_array()?.first()?.get("id")?.as_u64()
}

/// Année de sortie du premier jeu de l'autocomplete (`data[0].release_date`,
/// en secondes Unix). SteamGridDB la donne dans les deux API (publique comme
/// v2) ; absente ou nulle pour quelques entrées, d'où `None`.
fn first_game_year(json: &Value) -> Option<u32> {
    let date = json["data"]
        .as_array()?
        .first()?
        .get("release_date")?
        .as_u64()?;
    year_from_unix(date)
}

/// Liste `data[]` de l'autocomplete → jeux proposés (identification).
fn games_from(data: &Value) -> Vec<FoundGame> {
    data.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|game| {
            Some(FoundGame {
                id: game.get("id")?.as_u64()?,
                name: game.get("name")?.as_str()?.to_string(),
                year: game
                    .get("release_date")
                    .and_then(Value::as_u64)
                    .and_then(year_from_unix),
            })
        })
        .collect()
}

/// Année de sortie d'une fiche de jeu (`release_date` en secondes Unix).
fn game_year(game: &Value) -> Option<u32> {
    game.get("release_date")
        .and_then(Value::as_u64)
        .and_then(year_from_unix)
}

/// Année civile d'un horodatage Unix. Petite boucle depuis 1970 (quelques
/// dizaines de tours) plutôt qu'une dépendance de calendrier : c'est une
/// conversion de date, pas un besoin d'arithmétique complète.
fn year_from_unix(seconds: u64) -> Option<u32> {
    // une date codée 0 (ou dans les premières heures de 1970) veut dire
    // « pas de date » chez SGDB, pas le 1er janvier 1970
    if seconds < 86_400 {
        return None;
    }
    let mut days = (seconds / 86_400) as i64;
    let mut year = 1970u32;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let length = if leap { 366 } else { 365 };
        if days < length {
            return Some(year);
        }
        days -= length;
        year += 1;
    }
}

/// Candidats **dans l'ordre renvoyé** (celui du « top ») : on ne réordonne
/// pas, on écarte seulement ce qui n'est pas une image statique affichable.
///
/// Pour les grilles, on écarte aussi ce qui n'est pas au format portrait : le
/// type « grid » de SteamGridDB contient des 600×900 (2:3, la vignette), mais
/// aussi des carrés (512×512) et des formats larges (920×430, façon bannière
/// Steam). Vérifié 18/11 sur l'API publique — d'où l'impression de « pas que
/// des grilles » dans le menu. Si le tri ne laisse rien, on rend tout : mieux
/// vaut une image mal cadrée qu'un menu vide.
fn candidates(assets: &Value, kind: ArtKind) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut portraits = Vec::new();

    for asset in assets.as_array().map(Vec::as_slice).unwrap_or_default() {
        if truthy(asset, "nsfw") || truthy(asset, "humor") || truthy(asset, "is_animated") {
            continue;
        }
        let Some(url) = asset["url"].as_str() else {
            continue;
        };
        let Some(ext) = asset["mime"].as_str().and_then(extension) else {
            continue;
        };
        let thumb = asset["thumb"].as_str().unwrap_or(url).to_string();

        let portrait = match (
            asset["width"]
                .as_f64()
                .or_else(|| asset["width"].as_i64().map(|v| v as f64)),
            asset["height"]
                .as_f64()
                .or_else(|| asset["height"].as_i64().map(|v| v as f64)),
        ) {
            (Some(w), Some(h)) => h > w,
            // dimensions absentes : on ne peut pas juger, on garde
            _ => true,
        };

        let candidate = Candidate {
            url: url.to_string(),
            thumb,
            ext,
        };
        if portrait {
            portraits.push(candidate.clone());
        }
        out.push(candidate);
    }

    if kind == ArtKind::Grid && !portraits.is_empty() {
        return portraits;
    }
    out
}

fn truthy(asset: &Value, field: &str) -> bool {
    asset[field].as_bool().unwrap_or(false) || asset[field].as_i64().is_some_and(|v| v != 0)
}

/// Extension de fichier à partir du type MIME.
fn extension(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// Encodage d'un terme de recherche (`application/x-www-form-urlencoded`).
fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn parse_json(body: &str) -> Result<Value, String> {
    if body.trim().is_empty() {
        return Err("réponse vide (Cloudflare ?)".into());
    }
    serde_json::from_str(body).map_err(|err| format!("JSON invalide : {err}"))
}

/// Délai maximal d'une requête (connexion, envoi, réception du corps).
///
/// Sans lui, une connexion qui cale laisse le worker bloqué : le menu d'image
/// reste sur « recherche… » indéfiniment et TOUS les visuels de fond sont
/// suspendus derrière (constaté 18/11 sur une session SGDB restée pendue).
const TIMEOUT: Duration = Duration::from_secs(30);

/// Téléchargements : nombre de requêtes en cours, instant de la dernière
/// activité (début ou fin de requête), et réveil de l'interface. La roue de
/// chargement (`spinner`) lit `busy()` ; `wake()` est indispensable, sinon
/// l'interface ne redessinerait qu'à la FIN de la requête — donc n'afficherait
/// la roue que lorsque le téléchargement est déjà terminé (constaté 20/11).
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
/// Durée pendant laquelle la roue reste affichée après la dernière activité :
/// plus longue que le délai entre deux requêtes du worker (250 ms), sinon elle
/// clignoterait à chaque téléchargement.
const BUSY_WINDOW: Duration = Duration::from_millis(700);

static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Branche le réveil de l'interface (appelé par `App::boot`).
pub fn wake_with(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

fn wake() {
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

fn touch() {
    if let Ok(mut at) = LAST.lock() {
        *at = Some(std::time::Instant::now());
    }
}

/// Un téléchargement est-il en cours, ou vient-il de finir ?
pub fn busy() -> bool {
    if IN_FLIGHT.load(Ordering::Relaxed) > 0 {
        return true;
    }
    let last = LAST.lock().ok().and_then(|at| *at);
    last.is_some_and(|at| at.elapsed() < BUSY_WINDOW)
}

/// Garde du compteur : décrémente à la sortie, y compris sur `?` ou panique,
/// sinon une requête ratée laisserait la roue tourner pour toujours.
struct Busy;

impl Busy {
    fn new() -> Self {
        IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
        touch();
        wake();
        Self
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        touch();
    }
}

/// Client HTTP partagé, avec délais.
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .timeout_connect(Some(Duration::from_secs(10)))
            .build()
            .into()
    })
}

fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let _busy = Busy::new();
    let mut request = agent().get(url).header("User-Agent", AGENT);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.call().map_err(|err| format!("GET {url} : {err}"))?;
    response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_vec()
        .map_err(|err| format!("lecture {url} : {err}"))
}

fn http_get_text(url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    let bytes = http_get(url, headers)?;
    String::from_utf8(bytes).map_err(|err| format!("réponse non UTF-8 ({url}) : {err}"))
}

fn http_post(url: &str, headers: &[(&str, &str)], body: &str) -> Result<String, String> {
    let _busy = Busy::new();
    let mut request = agent().post(url).header("User-Agent", AGENT);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .send(body)
        .map_err(|err| format!("POST {url} : {err}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_vec()
        .map_err(|err| format!("lecture {url} : {err}"))?;
    String::from_utf8(bytes).map_err(|err| format!("réponse non UTF-8 ({url}) : {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grilles_portrait_seulement() {
        // ce que rend réellement l'API publique pour un jeu (18/11) :
        // 600x900 (2:3), 512x512 et 1024x1024 (carrés), 920x430 (large)
        let assets = serde_json::json!([
            {"url": "https://cdn/a.png", "thumb": "https://cdn/a.png", "mime": "image/png", "width": 600, "height": 900},
            {"url": "https://cdn/b.png", "thumb": "https://cdn/b.png", "mime": "image/png", "width": 920, "height": 430},
            {"url": "https://cdn/c.png", "thumb": "https://cdn/c.png", "mime": "image/png", "width": 512, "height": 512}
        ]);
        let grids = candidates(&assets, ArtKind::Grid);
        assert_eq!(
            grids.len(),
            1,
            "seule la 2:3 reste dans le menu des grilles"
        );
        assert_eq!(grids[0].url, "https://cdn/a.png");

        // les autres types ne sont pas filtrés sur les dimensions
        assert_eq!(candidates(&assets, ArtKind::Hero).len(), 3);

        // aucune portrait : on rend tout plutôt qu'un menu vide
        let larges = serde_json::json!([
            {"url": "https://cdn/x.png", "thumb": "https://cdn/x.png", "mime": "image/png", "width": 920, "height": 430}
        ]);
        assert_eq!(candidates(&larges, ArtKind::Grid).len(), 1);
    }

    #[test]
    fn normalisation_allege_et_respecte_le_format() {
        // une image bruitée, comme une vraie bannière : le PNG est lourd
        let (largeur, hauteur) = (2560u32, 830u32);
        let mut graine = 12345u32;
        let pixels: Vec<u8> = (0..largeur * hauteur)
            .flat_map(|_| {
                graine = graine.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = (graine >> 16) as u8;
                [v, v.wrapping_add(40), v.wrapping_add(90), 255]
            })
            .collect();
        let image = image::RgbaImage::from_raw(largeur, hauteur, pixels).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let brut = png.into_inner();

        // héros : JPEG 1920 de large, plusieurs fois plus léger
        let (heros, ext) = normaliser(ArtKind::Hero, &brut).expect("décodable");
        assert_eq!(ext, "jpg", "un héros est opaque : JPEG, pas PNG");
        let dimensions = image::load_from_memory(&heros).unwrap();
        assert_eq!(dimensions.width(), 1920, "largeur bornée à l'écran");
        assert!(
            heros.len() < brut.len() / 4,
            "{} vs {}",
            heros.len(),
            brut.len()
        );

        // logo : PNG conservé (alpha, l'overlay ne décode que ça), 1000 px
        let (logo, ext) = normaliser(ArtKind::Logo, &brut).unwrap();
        assert_eq!(ext, "png");
        let logo = image::load_from_memory(&logo).unwrap();
        assert!(
            (990..=1000).contains(&logo.width()),
            "largeur bornée à 1000 px : {}",
            logo.width()
        );

        // icône : boîte 256×256, ratio conservé, PNG
        let (icone, ext) = normaliser(ArtKind::Icon, &brut).unwrap();
        assert_eq!(ext, "png");
        let icone = image::load_from_memory(&icone).unwrap();
        assert!(icone.width() <= 256 && icone.height() <= 256);
        assert_eq!(icone.width() as f32 / icone.height() as f32, 2560.0 / 830.0);
    }

    #[test]
    fn une_image_perso_remplit_le_format_du_type() {
        // photo 16:9 fournie par l'utilisateur : en cover (2:3) elle doit être
        // RECADRÉE et remplie, pas posée avec des bandes ; le logo, lui, garde
        // son ratio (un logo rogné perd son texte)
        let image = image::RgbaImage::from_pixel(1600, 900, image::Rgba([200, 40, 40, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let brut = png.into_inner();

        for (kind, largeur, hauteur) in [
            (ArtKind::Grid, 600, 900),
            (ArtKind::Hero, 1920, 620),
            (ArtKind::Icon, 256, 256),
        ] {
            let (adaptees, _) = adapter(kind, &brut).expect("image décodable");
            let lue = image::load_from_memory(&adaptees).unwrap();
            assert_eq!(
                (lue.width(), lue.height()),
                (largeur, hauteur),
                "{kind:?} rempli exactement"
            );
        }

        let (logo, ext) = adapter(ArtKind::Logo, &brut).expect("logo");
        assert_eq!(ext, "png");
        let logo = image::load_from_memory(&logo).unwrap();
        assert_eq!(logo.width(), 1000);
        assert_eq!(logo.height(), 563, "ratio du logo conservé (16:9)");

        // une image illisible n'est pas silencieusement copiée : l'appelant
        // annonce l'échec et ne touche pas au visuel en place
        assert!(adapter(ArtKind::Grid, b"pas une image").is_none());
    }

    #[test]
    fn un_paquet_avec_un_visuel_est_signale() {
        let assets = std::env::temp_dir().join(format!("glauncher-hasart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&assets);
        let exe = assets.join("grids");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join("jeu.jpg"), b"x").unwrap();
        assert!(has_art(&assets, "jeu"));
        assert!(!has_art(&assets, "autre"));
        let _ = std::fs::remove_dir_all(&assets);
    }

    #[test]
    fn encodage_du_terme() {
        assert_eq!(urlencode("A Hat in Time"), "A+Hat+in+Time");
        assert_eq!(urlencode("Beyond Good & Evil"), "Beyond+Good+%26+Evil");
    }

    #[test]
    fn choix_persistant_dans_le_cache() {
        let dir = std::env::temp_dir().join(format!("glauncher-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("jeu.jpg"), b"ancienne").unwrap();

        let marks = dir.join("marques");
        std::fs::create_dir_all(&marks).unwrap();
        std::fs::write(marks.join("jeu.miss"), b"").unwrap();
        #[allow(clippy::needless_borrow)]
        // octets illisibles : le fichier est écrit tel quel, sans normalisation
        let path = save(
            ArtKind::Logo,
            &dir,
            &marks,
            "jeu",
            "png",
            b"nouvelle",
            Some("https://cdn/x.png"),
        );

        assert_eq!(path, dir.join("jeu.png"));
        assert_eq!(std::fs::read(&path).unwrap(), b"nouvelle");
        assert!(!dir.join("jeu.jpg").exists(), "l'ancien format est retiré");
        assert!(
            !dir.join("jeu.miss").exists(),
            "aucun marqueur dans assets/"
        );
        assert!(
            !marks.join("jeu.miss").exists(),
            "le marqueur d'échec saute"
        );
        assert_eq!(
            std::fs::read_to_string(marks.join("jeu.choice")).unwrap(),
            "https://cdn/x.png"
        );
        assert_eq!(cached(&dir, "jeu"), Some(path.clone()));
        assert_eq!(chosen(&dir, &marks, "jeu"), Some(path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ordre_du_top_conserve() {
        // l'API renvoie déjà le « top » : le premier reste le premier, même si
        // un autre visuel a de meilleurs votes ou un meilleur ratio
        let assets = serde_json::json!([
            {"url": "premier.png", "mime": "image/png", "width": 920, "height": 430, "upvotes": 0},
            {"url": "second.png", "mime": "image/png", "width": 600, "height": 900, "upvotes": 500},
            {"url": "nsfw.png", "mime": "image/png", "nsfw": true}
        ]);
        let list = candidates(&assets, ArtKind::Hero);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].url, "premier.png");
        assert_eq!(list[0].ext, "png");
        assert_eq!(list[1].url, "second.png");
    }

    #[test]
    fn dossiers_par_type() {
        assert_eq!(ArtKind::Grid.dir(), "grids");
        assert_eq!(ArtKind::Hero.dir(), "heroes");
        assert_eq!(ArtKind::Logo.dir(), "logos");
        assert_eq!(ArtKind::Icon.dir(), "icons");
    }

    #[test]
    fn les_annees_font_un_aller_retour_et_fusionnent() {
        let dir = std::env::temp_dir().join(format!("glauncher-years-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("years.txt");

        let mut years = BTreeMap::new();
        years.insert("wario-land-4".to_string(), Some(2001));
        years.insert("inconnu".to_string(), None);
        save_years(&path, &years);

        let relu = load_years(&path);
        assert_eq!(relu.get("wario-land-4"), Some(&Some(2001)));
        assert_eq!(relu.get("inconnu"), Some(&None), "« pas de date » est noté");

        // une autre source (--update, autre poste) ne perd pas l'existant
        let mut autres = BTreeMap::new();
        autres.insert("zelda".to_string(), Some(1998));
        save_years(&path, &autres);
        let relu = load_years(&path);
        assert_eq!(relu.len(), 3);
        assert_eq!(relu.get("zelda"), Some(&Some(1998)));
        assert_eq!(relu.get("wario-land-4"), Some(&Some(2001)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identifications_aller_retour_et_fusion() {
        let dir = std::env::temp_dir().join(format!("glauncher-who-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("identities.txt");

        let mut identities = BTreeMap::new();
        identities.insert(
            "marsupilami-2-salsa-palombia".to_string(),
            Identity {
                id: 5334897,
                name: "Marsupilami".to_string(),
            },
        );
        save_identities(&path, &identities);

        let relu = load_identities(&path);
        assert_eq!(
            relu.get("marsupilami-2-salsa-palombia"),
            Some(&Identity {
                id: 5334897,
                name: "Marsupilami".to_string()
            })
        );

        // une autre machine (Syncthing) ajoute la sienne : la fusion garde tout
        let mut autres = BTreeMap::new();
        autres.insert(
            "un-autre-jeu".to_string(),
            Identity {
                id: 42,
                name: "Un autre jeu".to_string(),
            },
        );
        save_identities(&path, &autres);
        let relu = load_identities(&path);
        assert_eq!(relu.len(), 2, "aucune identification perdue");
        assert_eq!(relu["un-autre-jeu"].name, "Un autre jeu");

        // ligne d'id seul (sans nom), comme une correction minimale
        std::fs::write(&path, "# commentaire\nseul=1234\nmal-forme\nvide=\n").unwrap();
        let relu = load_identities(&path);
        assert_eq!(relu.len(), 1);
        assert_eq!(relu["seul"].id, 1234);
        assert!(relu["seul"].name.is_empty());

        // le nom et l'id retenus pour un jeu identifié
        let (name, id) = who(&relu, "seul", "Nom du paquet");
        assert_eq!((name.as_str(), id), ("Nom du paquet", Some(1234)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn les_jeux_de_l_autocomplete_sont_proposes() {
        // réponse réelle pour « Marsupilami 2 Salsa Palombia » (20/11) : le
        // premier résultat est « Hoobadventure » (2021), d'où la cover et la
        // date fausses — la liste d'identification montre les deux
        let data = serde_json::json!([
            {"id": 5285976, "name": "Marsupilami: Hoobadventure", "verified": true,
             "types": ["steam"], "release_date": 1637020800u64},
            {"id": 5334897, "name": "Marsupilami", "verified": true,
             "types": [], "release_date": 832032000u64}
        ]);
        let games = games_from(&data);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "Marsupilami: Hoobadventure");
        assert_eq!(games[0].year, Some(2021));
        assert_eq!(
            games[1],
            FoundGame {
                id: 5334897,
                name: "Marsupilami".to_string(),
                year: Some(1996),
            }
        );

        // la fiche d'un jeu identifié donne la même conversion
        let fiche = serde_json::json!({"id": 5334897, "name": "Marsupilami",
                                       "release_date": 832032000u64});
        assert_eq!(game_year(&fiche), Some(1996));
        assert_eq!(game_year(&serde_json::json!({})), None);
    }

    #[test]
    fn l_annee_vient_de_la_date_de_sortie() {
        // Wario Land II (1998), Wario Land 3 (2001), Wario Land: Shake It! (2008)
        assert_eq!(year_from_unix(889_401_600), Some(1998));
        assert_eq!(year_from_unix(998_352_000), Some(2001));
        assert_eq!(year_from_unix(1_216_857_600), Some(2008));
        // au réveillon : c'est le jour qui décide, pas l'approximation
        assert_eq!(year_from_unix(946_684_799), Some(1999)); // 31/12/1999 23:59:59
        assert_eq!(year_from_unix(946_684_800), Some(2000)); // 01/01/2000 00:00
        // 29 février d'une année bissextile
        assert_eq!(year_from_unix(951_782_400), Some(2000)); // 29/02/2000
        // pas de date
        assert_eq!(year_from_unix(0), None);
        assert_eq!(year_from_unix(86_399), None);
    }

    /// Sonde réseau (ignorée) : l'année vient bien de l'autocomplete.
    #[test]
    #[ignore]
    fn annee_reseau() {
        let mut client = Client::new(std::env::var("SGDB_API_KEY").ok());
        for (name, annee) in [("Wario Land 4", 2001), ("Wario Land II", 1998)] {
            let year = client
                .release_year(name, None)
                .unwrap_or_else(|err| panic!("{name} : {err}"));
            eprintln!("{name} : {year:?}");
            assert_eq!(year, Some(annee), "{name}");
        }
    }

    /// Sonde réseau (ignorée) : le top doit être proposé pour chaque type.
    #[test]
    #[ignore]
    fn choix_multiples() {
        let mut client = Client::new(std::env::var("SGDB_API_KEY").ok());
        for kind in ArtKind::ALL {
            let choices = client
                .choices("Beyond Good & Evil", None, kind)
                .unwrap_or_else(|err| panic!("{:?} : {err}", kind));
            eprintln!(
                "{} : {} propositions, 1re = {}",
                kind.label(),
                choices.len(),
                choices[0].url
            );
            assert!(!choices.is_empty(), "{kind:?}");
        }
    }
}
