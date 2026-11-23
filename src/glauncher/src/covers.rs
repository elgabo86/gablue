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
//! Tout est écrit dans le dossier partagé `assets/` (`grids/`, `heroes/`,
//! `logos/`, `icons/`), jamais dans les dossiers de jeux : les sources restent
//! en lecture seule.
//!
//! Les fichiers sont nommés d'après **le jeu SteamGridDB** et non d'après le
//! paquet (voir [`key`]) : deux postes dont les `.wgp`/`.lgp` portent des noms
//! différents (ou deux amis par Syncthing) tombent donc sur les mêmes fichiers
//! et ne téléchargent qu'une fois.

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
    /// de CE jeu, sans repasser par une recherche par nom. `key` est le nom
    /// partagé des fichiers ([`key`]) quand l'identification est **déjà
    /// connue** ; sinon le worker résout le jeu lui-même et retient
    /// l'identification (c'est elle qui donne la clé).
    Auto {
        slug: String,
        name: String,
        id: Option<u64>,
        key: Option<String>,
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
    /// fait foi et n'est jamais remplacé automatiquement. `key` = nom partagé
    /// du fichier ([`key`]), `slug` = paquet (marqueurs locaux).
    Pick {
        slug: String,
        key: String,
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
        key: String,
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
        key: Option<String>,
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

/// Nom de fichier d'un visuel dans `assets/` : **le jeu SteamGridDB**, pas le
/// paquet — `<id>-<nom slugifié>` (`8103-doom.jpg`).
///
/// Pourquoi : le dossier `assets/` est partagé par Syncthing (entre postes et
/// entre amis). Deux `.wgp`/`.lgp` du même jeu dont les noms diffèrent un peu
/// (`Marsupilami 2 Salsa Palombia.lgp` / `Marsupilami 2 (FR).lgp`) tombent sur
/// la **même** clé, donc sur les mêmes images : le second poste ne télécharge
/// rien. L'id en tête évite les collisions de noms (« Doom » 1993 et 2016
/// portent le même nom chez SteamGridDB). Un jeu jamais identifié garde son
/// slug de paquet : ses visuels restent alors propres à la machine.
pub fn key(identity: Option<&Identity>, slug: &str) -> String {
    let Some(identity) = identity else {
        return slug.to_string();
    };
    let name = identity.name.trim();
    if name.is_empty() {
        return identity.id.to_string();
    }
    format!("{}-{}", identity.id, crate::library::slugify(name))
}

/// [`key`] pour un paquet, d'après les identifications partagées.
pub fn key_of(identities: &BTreeMap<String, Identity>, slug: &str) -> String {
    key(identities.get(slug), slug)
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
    /// pas : c'est noté, on ne redemandera plus). `key` = sous quel nom elle
    /// est rangée dans `assets/years.txt` (le jeu, pas le paquet).
    Year {
        slug: String,
        key: String,
        year: Option<u32>,
    },
    /// Le jeu SteamGridDB d'un paquet vient d'être appris (premier résultat de
    /// l'autocomplete, ou fiche d'un id) : le choix est retenu dans
    /// `assets/identities.txt` et c'est lui qui donne la clé partagée des
    /// visuels. L'interface met sa table à jour, sans réécrire le fichier.
    Identified { slug: String, identity: Identity },
    /// Aucune fiche SteamGridDB pour ce jeu : la recherche négative est
    /// datée dans `assets/absents.txt` (partagé), pour ne pas la refaire à
    /// chaque démarrage et pour l'afficher dans les options (revérifiable).
    Absent { slug: String, requete: String },
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
                        key: known,
                        kinds,
                    } => {
                        // Quel jeu SteamGridDB ? Avec une identification, l'id
                        // suffit ; sans, on la résout ici et on la retient (une
                        // seule requête) : c'est elle qui donne la clé partagée
                        // des fichiers, sans quoi deux postes retéléchargeraient
                        // tout.
                        let resolved = match (&known, id) {
                            (Some(key), Some(id)) => Ok(Some((Identity { id, name: name.clone() }, key.clone()))),
                            _ => client.find(&name, id).map(|found| {
                                found.map(|game| {
                                    let identity = Identity {
                                        id: game.id,
                                        name: game.name.clone(),
                                    };
                                    (identity.clone(), key(Some(&identity), &slug))
                                })
                            }),
                        };
                        let resolved = match resolved {
                            Ok(Some((identity, key))) => {
                                // identification encore inconnue : on la
                                // partage (elle sert au prochain démarrage et
                                // aux autres postes)
                                if known.is_none() {
                                    learn_identity(&cache, &slug, &identity, &emit);
                                }
                                Some((identity, key))
                            }
                            Ok(None) => None,
                            Err(error) => {
                                for kind in kinds {
                                    eprintln!(
                                        "glauncher : {} SGDB « {name} » : {error}",
                                        kind.label()
                                    );
                                    emit(CoverEvent::Failed {
                                        slug: slug.clone(),
                                        kind,
                                        error: error.clone(),
                                    });
                                }
                                continue;
                            }
                        };
                        let Some((identity, key)) = resolved else {
                            // jeu inconnu de SteamGridDB : rien à télécharger,
                            // et la recherche négative est datée (absents.txt)
                            emit(CoverEvent::Absent {
                                slug: slug.clone(),
                                requete: name.clone(),
                            });
                            for kind in kinds {
                                let dir = cache.join(kind.dir());
                                let marks = marks.join(kind.dir());
                                mark_missing(&dir, &marks, &key(None, &slug), &slug);
                                eprintln!(
                                    "glauncher : {} SGDB « {name} » introuvable",
                                    kind.label()
                                );
                                emit(CoverEvent::Missing {
                                    slug: slug.clone(),
                                    kind,
                                });
                            }
                            continue;
                        };
                        for kind in kinds {
                            let dir = cache.join(kind.dir());
                            let marks = marks.join(kind.dir());
                            // Déjà là sous la clé du jeu : téléchargé par un
                            // autre poste et arrivé par Syncthing. On le sert
                            // sans réseau — c'est tout l'intérêt du nommage.
                            if let Some(path) = cached_for(&dir, Some(&identity), &slug)
                                && let Ok(bytes) = std::fs::read(&path)
                            {
                                eprintln!(
                                    "glauncher : {} « {} » déjà partagé → {}",
                                    kind.label(),
                                    identity.name,
                                    path.display()
                                );
                                emit(CoverEvent::Found {
                                    slug: slug.clone(),
                                    kind,
                                    bytes,
                                });
                                continue;
                            }
                            let fetched = client.top(identity.id, kind);
                            std::thread::sleep(THROTTLE);
                            match fetched {
                                Ok(Some((url, bytes, ext))) => {
                                    let path =
                                        save(kind, &dir, &marks, &key, &slug, ext, &bytes, None);
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
                                    mark_missing(&dir, &marks, &key, &slug);
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
                    } => {
                        // même résolution que `Auto` : l'identification apprise
                        // ici est celle sous laquelle le choix sera écrit
                        let game = match id {
                            Some(id) => FoundGame {
                                id,
                                name: name.clone(),
                                year: None,
                            },
                            None => match client.find(&name, None) {
                                Ok(Some(game)) => {
                                    let identity = Identity {
                                        id: game.id,
                                        name: game.name.clone(),
                                    };
                                    learn_identity(&cache, &slug, &identity, &emit);
                                    game
                                }
                                Ok(None) => {
                                    emit(CoverEvent::Choices {
                                        slug: slug.clone(),
                                        kind,
                                        choices: Vec::new(),
                                    });
                                    continue;
                                }
                                Err(error) => {
                                    eprintln!("glauncher : choix « {name} » : {error}");
                                    emit(CoverEvent::Failed { slug, kind, error });
                                    continue;
                                }
                            },
                        };
                        match client.assets(game.id, kind) {
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
                        }
                    }

                    // --- image choisie par l'utilisateur ---
                    Query::Pick {
                        slug,
                        key,
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
                                let path = save(
                                    kind,
                                    &dir,
                                    &marks,
                                    &key,
                                    &slug,
                                    &ext,
                                    &bytes,
                                    Some(&url),
                                );
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
                    Query::UseFile {
                        slug,
                        key,
                        kind,
                        path,
                    } => {
                        let dir = cache.join(kind.dir());
                        let marks = marks.join(kind.dir());
                        match std::fs::read(&path) {
                            Ok(bytes) => match adapter(kind, &bytes) {
                                Some((adaptees, ext)) => {
                                    // marqueur `.choice` : cette image fait foi,
                                    // elle ne sera jamais remplacée toute seule
                                    let url = format!("file://{}", path.display());
                                    let ecrit = ecrire(
                                        &dir,
                                        &marks,
                                        &key,
                                        &slug,
                                        ext,
                                        &adaptees,
                                        Some(&url),
                                    );
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
                    Query::Year {
                        slug,
                        name,
                        id,
                        key: known,
                    } => {
                        // la fiche du jeu donne à la fois son nom et sa date :
                        // une seule requête, et l'identification apprise ici
                        // range l'année sous la clé du jeu (partagée)
                        let found = match client.find(&name, id) {
                            Ok(found) => found,
                            // erreur réseau : on ne note RIEN, la question
                            // pourra être reposée (contrairement à « SGDB n'a
                            // pas de date »)
                            Err(error) => {
                                eprintln!("glauncher : année « {name} » : {error}");
                                continue;
                            }
                        };
                        let (key, year) = match found {
                            Some(game) => {
                                let identity = Identity {
                                    id: game.id,
                                    name: game.name.clone(),
                                };
                                if known.is_none() {
                                    learn_identity(&cache, &slug, &identity, &emit);
                                }
                                if game.year.is_none() {
                                    eprintln!("glauncher : année de « {name} » inconnue");
                                }
                                (key(Some(&identity), &slug), game.year)
                            }
                            // jeu inconnu de SteamGridDB : « pas de date » sous
                            // le nom du paquet, on ne redemandera plus
                            None => (key(None, &slug), None),
                        };
                        emit(CoverEvent::Year { slug, key, year });
                    }

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

/// Range les visuels déjà téléchargés sous la **clé du jeu** plutôt que sous le
/// slug du paquet : sans ça, apprendre l'identification d'un jeu ferait
/// retélécharger toutes ses images (le cache ne les retrouverait plus).
///
/// Renommage seulement, jamais d'écrasement : si le fichier de la clé existe
/// déjà (image arrivée d'un autre poste par Syncthing), l'ancien reste où il
/// est et la nouvelle clé gagne à la lecture. Rend le nombre de fichiers
/// déplacés.
fn rename_to_key(assets: &Path, slug: &str, identity: &Identity) -> usize {
    let cle = key(Some(identity), slug);
    if cle == slug {
        return 0;
    }
    let mut bouges = 0;
    for kind in ArtKind::ALL {
        let dir = assets.join(kind.dir());
        for ext in IMAGE_EXT {
            let from = dir.join(format!("{slug}.{ext}"));
            let to = dir.join(format!("{cle}.{ext}"));
            if from.is_file() && !to.exists() && std::fs::rename(&from, &to).is_ok() {
                bouges += 1;
            }
        }
    }
    bouges
}

/// Retient une identification apprise **automatiquement** (premier résultat de
/// l'autocomplete, ou fiche d'un jeu) dans `assets/identities.txt`, le fichier
/// partagé, et prévient l'interface.
///
/// Pourquoi l'écrire : c'est l'identification qui donne la **clé** des visuels
/// ([`key`]). Tant qu'elle n'est pas notée, un redémarrage ne saurait plus sous
/// quel nom les fichiers ont été rangés (le démarrage ne fait aucune requête
/// réseau). Une identification choisie à la main passe par l'interface
/// (`apply_identity`), avec la question « remplacer les visuels ? ».
fn learn_identity<F>(assets: &Path, slug: &str, identity: &Identity, emit: &F)
where
    F: Fn(CoverEvent),
{
    let mut map = BTreeMap::new();
    map.insert(slug.to_string(), identity.clone());
    save_identities(&identities_path(assets), &map);
    // visuels déjà là, rangés sous le nom du paquet (installation antérieure,
    // ou téléchargés avant l'identification) : on les renomme au lieu de les
    // retélécharger
    let bouges = rename_to_key(assets, slug, identity);
    if bouges > 0 {
        eprintln!("glauncher : {bouges} visuel(s) de {slug} renommé(s) d'après le jeu");
    }
    emit(CoverEvent::Identified {
        slug: slug.to_string(),
        identity: identity.clone(),
    });
}

/// Oublie les visuels d'un jeu (identification changée, jeu mal reconnu) :
/// les fichiers de la clé donnée, l'ancien nom en `<id>` s'il existe, et les
/// marqueurs de CETTE machine (eux, par paquet). La miniature est refaite à
/// partir de la nouvelle cover ; l'appelant efface les handles en mémoire et
/// redemande les visuels du jeu identifié.
pub fn forget(assets: &Path, marks_root: &Path, slug: &str, key: &str) {
    let mut noms = vec![key.to_string()];
    // clé `<id>-<nom>` : le même jeu a pu être écrit sous son seul id
    if let Some((id, _)) = key.split_once('-') {
        noms.push(id.to_string());
    }
    for kind in ArtKind::ALL {
        let dir = assets.join(kind.dir());
        for nom in &noms {
            for ext in IMAGE_EXT {
                let _ = std::fs::remove_file(dir.join(format!("{nom}.{ext}")));
            }
        }
        let marks = marks_root.join(kind.dir());
        let _ = std::fs::remove_file(marks.join(format!("{slug}.miss")));
        let _ = std::fs::remove_file(marks.join(format!("{slug}.choice")));
    }
    let _ = std::fs::remove_file(alias_mark(marks_root, slug));
}

/// Marque « introuvable sur SteamGridDB », **datée** : la recherche n'est pas
/// une affirmation sur le monde, seulement sur le dernier essai. Elle expire
/// au bout de `ABSENT_TTL` et se lève dès qu'une identification, un résultat
/// de recherche ou une image choisie à la main la contredit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Absent {
    /// Date du dernier essai, `AAAA-MM-JJ` (locale).
    pub verifie: String,
    /// Nom réellement cherché ce jour-là (une nouvelle requête mérite un
    /// nouvel essai).
    pub requete: String,
}

/// Délai de validité d'une marque « introuvable ».
const ABSENT_TTL_JOURS: i64 = 30;

/// Fichier partagé (`assets/`) des « introuvables » datés.
pub fn absents_path(assets: &Path) -> PathBuf {
    assets.join("absents.txt")
}

/// Lit `absents.txt` : `slug<TAB>verifie=AAAA-MM-JJ<TAB>requete=nom`.
/// Les commentaires et lignes illisibles sont ignorés.
pub fn load_absents(path: &Path) -> BTreeMap<String, Absent> {
    let texte = std::fs::read_to_string(path).unwrap_or_default();
    let mut absents = BTreeMap::new();
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let mut champs = ligne.splitn(3, '\t');
        let Some(slug) = champs.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let mut verifie = None;
        let mut requete = None;
        for champ in champs {
            let (nom, valeur) = match champ.split_once('=') {
                Some((nom, valeur)) => (nom.trim(), valeur.trim()),
                None => continue,
            };
            match nom {
                "verifie" => verifie = Some(valeur.to_string()),
                "requete" => requete = Some(valeur.to_string()),
                _ => {}
            }
        }
        let (Some(verifie), Some(requete)) = (verifie, requete) else {
            continue;
        };
        absents.insert(
            slug.to_string(),
            Absent {
                verifie,
                requete,
            },
        );
    }
    absents
}

/// Écrit (ou rafraîchit) la marque de `slug` avec la date du jour, en
/// **préservant** le reste du fichier (commentaires, autres jeux). Rend la
/// marque posée pour que l'interface tienne sa table à jour sans relire.
pub fn marquer_absent(path: &Path, slug: &str, requete: &str) -> Absent {
    let requete = requete.replace(['\t', '\n', '\r'], " ");
    let texte = std::fs::read_to_string(path).unwrap_or_default();
    let mut lignes: Vec<String> = texte
        .lines()
        .filter(|ligne| {
            let corps = ligne.trim();
            !corps.is_empty()
                && corps
                    .split_once('\t')
                    .map(|(nom, _)| nom.trim())
                    .is_none_or(|nom| nom != slug)
        })
        .map(str::to_string)
        .collect();
    if lignes.is_empty() {
        lignes.push(
            "# jeux introuvables sur SteamGridDB : slug<TAB>verifie=AAAA-MM-JJ<TAB>requete=nom"
                .to_string(),
        );
    }
    lignes.push(format!(
        "{slug}\tverifie={}\trequete={requete}",
        date_aujourd_hui()
    ));
    write_atomic(path, format!("{}\n", lignes.join("\n")).as_bytes());
    Absent {
        verifie: date_aujourd_hui(),
        requete: requete.to_string(),
    }
}

/// Lève la marque de `slug` (identification, résultat trouvé, image choisie).
pub fn clear_absent(path: &Path, slug: &str) {
    let Ok(texte) = std::fs::read_to_string(path) else {
        return;
    };
    let garde: Vec<&str> = texte
        .lines()
        .filter(|ligne| {
            let corps = ligne.trim();
            corps.starts_with('#')
                || corps
                    .split_once('\t')
                    .map(|(nom, _)| nom.trim())
                    .is_none_or(|nom| nom != slug)
        })
        .collect();
    // le fichier ne contient plus que le commentaire (ou des blancs) : on
    // retire les lignes vides pour ne pas laisser un fichier fantôme
    let utile = garde.iter().any(|ligne| {
        let corps = ligne.trim();
        !corps.is_empty() && !corps.starts_with('#')
    });
    if !utile {
        let _ = std::fs::remove_file(path);
        return;
    }
    write_atomic(path, format!("{}\n", garde.join("\n")).as_bytes());
}

/// Marque encore fraîche ? Une marque périmée est ignorée : la prochaine
/// demande de visuels refait une recherche et met la date à jour.
pub fn absent_frais(absent: &Absent) -> bool {
    jours_depuis_date(&absent.verifie)
        .and_then(|jours| jours_aujourd_hui().checked_sub(jours))
        .is_some_and(|age| (0..=ABSENT_TTL_JOURS).contains(&age))
}

/// Date de la marque en clair, pour les options : `23 nov. 2026`.
pub fn date_verifie(absent: &Absent) -> String {
    let Some(jours) = jours_depuis_date(&absent.verifie) else {
        return absent.verifie.clone();
    };
    let (annee, mois, jour) = date_depuis_jours(jours);
    const MOIS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
        "nov.", "déc.",
    ];
    let nom = MOIS.get(mois.saturating_sub(1) as usize).copied().unwrap_or("");
    format!("{jour} {nom} {annee}")
}

/// Retire les marqueurs `.miss` (introuvable par type) d'un jeu : « revérifier
/// maintenant » doit redonner sa chance à SteamGridDB sans attendre 30 jours.
pub fn oublier_introuvables(marks_root: &Path, slug: &str) {
    for kind in ArtKind::ALL {
        let _ = std::fs::remove_file(marks_root.join(kind.dir()).join(format!("{slug}.miss")));
    }
}

/// Marqueur `.<slug>.alias` : les visuels de ce jeu ont été repris d'un autre
/// jeu (ou choisis), donc aucune recherche automatique ne doit les renommer.
pub fn alias_mark(marks_root: &Path, slug: &str) -> PathBuf {
    marks_root.join(format!("{slug}.alias"))
}

/// Vrai si le jeu a des visuels repris d'un autre jeu (alias visuel).
pub fn est_alias(marks_root: &Path, slug: &str) -> bool {
    alias_mark(marks_root, slug).is_file()
}

/// Copie les visuels d'un jeu vers un autre (alias visuel) : la réponse pour
/// un jeu absent de SteamGridDB — un fangame reprend l'art de sa série. Les
/// fichiers copiés sont marqués `.choice` pour le jeu cible (une recherche
/// automatique ne les remplacera pas) et ses marques « introuvable » tombent.
/// Rend le nombre de visuels copiés.
pub fn alias_visuels(
    assets: &Path,
    marks_root: &Path,
    source_key: &str,
    target_key: &str,
    slug: &str,
) -> usize {
    if source_key == target_key {
        // deux paquets du MÊME jeu partagent déjà leurs visuels : copier un
        // fichier sur lui-même le tronquerait
        return 0;
    }
    let mut copies = 0;
    // marque d'alias : un jeu aux visuels repris ne doit plus être
    // auto-identifié (la recherche par nom renommerait les fichiers copiés
    // sous la clé d'un jeu qui n'est peut-être pas le bon)
    let _ = std::fs::create_dir_all(marks_root);
    write_atomic(&alias_mark(marks_root, slug), b"");
    for kind in ArtKind::ALL {
        let dir = assets.join(kind.dir());
        let marks = marks_root.join(kind.dir());
        let mut copie = None;
        for ext in IMAGE_EXT {
            let source = dir.join(format!("{source_key}.{ext}"));
            if !source.is_file() {
                continue;
            }
            let cible = dir.join(format!("{target_key}.{ext}"));
            if std::fs::copy(&source, &cible).is_ok() {
                copie = Some(ext);
                copies += 1;
            }
        }
        let Some(garde) = copie else {
            continue;
        };
        // un seul fichier fait foi pour ce type
        for ext in IMAGE_EXT {
            if ext != garde {
                let _ = std::fs::remove_file(dir.join(format!("{target_key}.{ext}")));
            }
        }
        let _ = std::fs::create_dir_all(&marks);
        write_atomic(&marks.join(format!("{slug}.choice")), b"");
        let _ = std::fs::remove_file(marks.join(format!("{slug}.miss")));
    }
    copies
}

/// `AAAA-MM-JJ` → jours depuis l'époque (algorithme civil_from_days inverse).
fn jours_depuis_date(date: &str) -> Option<i64> {
    let mut morceaux = date.split('-');
    let annee: i64 = morceaux.next()?.parse().ok()?;
    let mois: i64 = morceaux.next()?.parse().ok()?;
    let jour: i64 = morceaux.next()?.parse().ok()?;
    if !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
        return None;
    }
    let y = if mois <= 2 { annee - 1 } else { annee };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mois + 9) % 12;
    let doy = (153 * mp + 2) / 5 + jour - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Date **locale** du poste : les marques sont écrites en heure locale, donc
/// les comparer à une date UTC faisait expirer une marque du jour même entre
/// 23 h et minuit (constaté au banc le 23/11 à 00:45 CET).
fn aujourd_hui_local() -> (i64, i64, i64) {
    let secondes = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duree| duree.as_secs())
        .unwrap_or(0);
    let instant = secondes as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if !unsafe { libc::localtime_r(&instant, &mut tm) }.is_null() {
        return (
            tm.tm_year as i64 + 1900,
            tm.tm_mon as i64 + 1,
            tm.tm_mday as i64,
        );
    }
    date_depuis_jours((secondes / 86_400) as i64)
}

fn jours_aujourd_hui() -> i64 {
    let (annee, mois, jour) = aujourd_hui_local();
    jours_depuis_date(&format!("{annee:04}-{mois:02}-{jour:02}")).unwrap_or(0)
}

/// Jours depuis l'époque → `(année, mois, jour)`.
fn date_depuis_jours(jours: i64) -> (i64, i64, i64) {
    let z = jours + 719_468;
    let ere = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + ere * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let jour = doy - (153 * mp + 2) / 5 + 1;
    let mois = if mp < 10 { mp + 3 } else { mp - 9 };
    (if mois <= 2 { y + 1 } else { y }, mois, jour)
}

fn date_aujourd_hui() -> String {
    let (annee, mois, jour) = aujourd_hui_local();
    format!("{annee:04}-{mois:02}-{jour:02}")
}

/// Fichier des années de sortie, dans `assets/` : c'est le dossier partagé
/// (Syncthing), donc les années suivent les visuels chez un autre poste.
///
/// Format : `<clé>=1998` par ligne, trié ; la clé est celle des **visuels**
/// ([`key`], le jeu SteamGridDB) et non le paquet, sinon deux noms de paquet
/// légèrement différents redemanderaient la date. `<clé>=` veut dire « regardé,
/// SteamGridDB ne donne pas de date » — une année connue n'est **jamais**
/// redemandée, ni par l'interface ni par `--update`.
/// Retire d'un fichier `slug=…` (`years.txt`, `identities.txt`) les lignes des
/// jeux qui ne sont plus présents. Commentaires et lignes vides sont conservés,
/// le fichier est réécrit atomiquement. Les IMAGES ne sont jamais touchées :
/// un jeu rebranché retrouve ses visuels. Sert à « Oublier les jeux disparus ».
pub fn oublier_slugs(path: &Path, garde: &std::collections::HashSet<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut sortie = String::with_capacity(text.len());
    let mut change = false;
    for ligne in text.lines() {
        let garde_ligne = match ligne.split_once('=') {
            Some((slug, _)) if !ligne.starts_with('#') => garde.contains(slug.trim()),
            // commentaire, ligne vide : rien à jeter
            _ => true,
        };
        if garde_ligne {
            sortie.push_str(ligne);
            sortie.push('\n');
        } else {
            change = true;
        }
    }
    if change {
        write_atomic(path, sortie.as_bytes());
    }
}

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
#[cfg(test)]
fn who(identities: &BTreeMap<String, Identity>, slug: &str, name: &str) -> (String, Option<u64>) {
    match identities.get(slug) {
        Some(identity) if !identity.name.is_empty() => (identity.name.clone(), Some(identity.id)),
        Some(identity) => (name.to_string(), Some(identity.id)),
        None => (name.to_string(), None),
    }
}

/// Identification complète déjà connue pour un paquet, s'il y en a une.
fn known(identities: &BTreeMap<String, Identity>, slug: &str) -> Option<Identity> {
    identities
        .get(slug)
        .filter(|identity| !identity.name.is_empty())
        .cloned()
}

/// Retient une identification apprise hors interface (`--update`, `--date`,
/// `--art`) : fusion dans le fichier partagé et table en mémoire.
fn remember_identity(
    assets: &Path,
    identities: &mut BTreeMap<String, Identity>,
    slug: &str,
    identity: &Identity,
) {
    let mut map = BTreeMap::new();
    map.insert(slug.to_string(), identity.clone());
    save_identities(&identities_path(assets), &map);
    // visuels déjà là, rangés sous le nom du paquet : renommés, pas retéléchargés
    let bouges = rename_to_key(assets, slug, identity);
    if bouges > 0 {
        eprintln!("glauncher : {bouges} visuel(s) de {slug} renommé(s) d'après le jeu");
    }
    identities.insert(slug.to_string(), identity.clone());
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
    let mut identities = load_identities(&identities_path(assets));

    for (slug, name) in games {
        // identification locale : elle donne la clé sans aucune requête
        let local = known(&identities, slug);
        let cle = key(local.as_ref(), slug);
        if years.contains_key(&cle) {
            totals.skipped += 1;
            continue;
        }
        // résolution : fiche du jeu identifié (nom + date), ou autocomplete
        let (identity, year) = match local {
            Some(identity) => match client.find(&identity.name, Some(identity.id)) {
                Ok(found) => (Some(identity), found.and_then(|game| game.year)),
                Err(error) => {
                    totals.failed += 1;
                    report(format!("année « {name} » : {error}"));
                    std::thread::sleep(THROTTLE);
                    continue;
                }
            },
            None => match client.find(name, None) {
                Ok(Some(game)) => {
                    let identity = Identity {
                        id: game.id,
                        name: game.name.clone(),
                    };
                    remember_identity(assets, &mut identities, slug, &identity);
                    (Some(identity), game.year)
                }
                Ok(None) => (None, None),
                Err(error) => {
                    totals.failed += 1;
                    report(format!("année « {name} » : {error}"));
                    std::thread::sleep(THROTTLE);
                    continue;
                }
            },
        };
        let cle = key(identity.as_ref(), slug);
        years.insert(cle, year);
        totals.years += 1;
        save_years(&path, &years);
        report(match year {
            Some(year) => format!("année « {name} » → {year}"),
            None => format!("année « {name} » inconnue"),
        });
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
    let mut identities = load_identities(&identities_path(assets));

    for (slug, name, pack) in games {
        // Identification et clé : l'identification locale ne coûte rien, un jeu
        // jamais identifié passe par l'autocomplete — son résultat est retenu,
        // c'est lui qui donne les fichiers partagés. Avec l'identification, ce
        // sont SES visuels et SA date, pas ceux du premier résultat.
        let (identity, autocomplete_year) = match known(&identities, slug) {
            Some(identity) => (identity, None),
            None => match client.find(name, None) {
                Ok(Some(game)) => {
                    let identity = Identity {
                        id: game.id,
                        name: game.name.clone(),
                    };
                    remember_identity(assets, &mut identities, slug, &identity);
                    std::thread::sleep(THROTTLE);
                    (identity, Some(game.year))
                }
                Ok(None) => {
                    totals.missing += 1;
                    report(format!("« {name} » inconnu de SteamGridDB"));
                    continue;
                }
                Err(error) => {
                    totals.failed += 1;
                    report(format!("« {name} » : {error}"));
                    continue;
                }
            },
        };
        let cle = key(Some(&identity), slug);
        // --- année de sortie : une requête, puis plus jamais ---
        if !years.contains_key(&cle) {
            let year = match autocomplete_year {
                Some(year) => year,
                None => match client.find(&identity.name, Some(identity.id)) {
                    Ok(found) => found.and_then(|game| game.year),
                    Err(error) => {
                        totals.failed += 1;
                        report(format!("année « {name} » : {error}"));
                        std::thread::sleep(THROTTLE);
                        continue;
                    }
                },
            };
            years.insert(cle.clone(), year);
            totals.years += 1;
            save_years(&years_file, &years);
            report(match year {
                Some(year) => format!("année « {name} » → {year}"),
                None => format!("année « {name} » inconnue"),
            });
            std::thread::sleep(THROTTLE);
        }
        for kind in ArtKind::ALL {
            let dir = assets.join(kind.dir());
            let marks = marks_root.join(kind.dir());
            if cached_for(&dir, Some(&identity), slug).is_some() {
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
            match client.top(identity.id, kind) {
                Ok(Some((url, bytes, ext))) => {
                    let path = save(kind, &dir, &marks, &cle, slug, ext, &bytes, None);
                    let _ = url;
                    totals.downloaded += 1;
                    report(format!(
                        "{} « {} » → {}",
                        kind.label(),
                        identity.name,
                        path.display()
                    ));
                }
                Ok(None) => {
                    mark_missing(&dir, &marks, &cle, slug);
                    totals.missing += 1;
                    report(format!("{} « {} » introuvable", kind.label(), identity.name));
                }
                Err(error) => {
                    totals.failed += 1;
                    report(format!("{} « {} » : {error}", kind.label(), identity.name));
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
fn deja_bon(kind: ArtKind, dir: &Path, nom: &str) -> bool {
    let (ext, max_w, max_h) = cible(kind);
    let Ok(bytes) = std::fs::read(dir.join(format!("{nom}.{ext}"))) else {
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
    // identification partagée : le logo et l'icône sont ceux du jeu choisi —
    // et c'est elle qui donne le nom partagé des fichiers. Sans elle, on
    // résout le jeu ici et on la retient (gpad relit `identities.txt`).
    let mut identities = load_identities(&identities_path(assets));
    let identity = match known(&identities, slug) {
        Some(identity) => Some(identity),
        None => match client.find(name, None) {
            Ok(Some(game)) => {
                let identity = Identity {
                    id: game.id,
                    name: game.name.clone(),
                };
                remember_identity(assets, &mut identities, slug, &identity);
                std::thread::sleep(THROTTLE);
                Some(identity)
            }
            Ok(None) | Err(_) => None,
        },
    };
    let nom = identity
        .as_ref()
        .map(|identity| identity.name.as_str())
        .unwrap_or(name);
    let cle = key(identity.as_ref(), slug);

    for kind in OVERLAY_KINDS {
        let dir = assets.join(kind.dir());
        let marks = marks_root.join(kind.dir());
        // déjà au bon format et à la bonne taille : rien à faire
        if deja_bon(kind, &dir, &cle) {
            totals.skipped += 1;
            continue;
        }
        // un visuel existe en un autre format (ou trop grand) : on le réécrit
        if let Some(autre) = cached_for(&dir, identity.as_ref(), slug)
            && let Ok(bytes) = std::fs::read(&autre)
            && normaliser(kind, &bytes).is_some()
        {
            let ext = autre
                .extension()
                .map(|ext| ext.to_string_lossy().to_string())
                .unwrap_or_default();
            let path = save(kind, &dir, &marks, &cle, slug, &ext, &bytes, None);
            totals.downloaded += 1;
            report(format!(
                "{} « {nom} » mis au format de l'overlay → {}",
                kind.label(),
                path.display()
            ));
            continue;
        }
        if kind == ArtKind::Icon {
            // l'icône choisie par l'utilisateur gagne, puis celle du paquet
            if chosen(&dir, &marks, &cle, slug).is_some() {
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
                report(format!("Icône « {nom} » extraite du paquet"));
                continue;
            }
        }
        if recently_missed(&marks, slug) {
            totals.skipped += 1;
            continue;
        }
        let Some(identity) = identity.as_ref() else {
            // jeu inconnu de SteamGridDB : seul le visuel du paquet reste
            totals.missing += 1;
            report(format!("{} « {nom} » introuvable", kind.label()));
            continue;
        };
        match client.top(identity.id, kind) {
            Ok(Some((_url, bytes, ext))) => {
                if normaliser(kind, &bytes).is_none() {
                    totals.failed += 1;
                    report(format!("{} « {nom} » : image illisible", kind.label()));
                    continue;
                }
                let path = save(kind, &dir, &marks, &cle, slug, ext, &bytes, None);
                totals.downloaded += 1;
                report(format!("{} « {nom} » → {}", kind.label(), path.display()));
            }
            Ok(None) => {
                mark_missing(&dir, &marks, &cle, slug);
                totals.missing += 1;
                report(format!("{} « {nom} » introuvable", kind.label()));
            }
            Err(error) => {
                totals.failed += 1;
                report(format!("{} « {nom} » : {error}", kind.label()));
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
    // clé partagée → slug du paquet, pour retrouver la miniature d'une grille
    // (les miniatures sont locales, nommées d'après le paquet)
    let mut paquets: BTreeMap<String, String> = BTreeMap::new();
    for (slug, identity) in load_identities(&identities_path(assets)) {
        paquets.insert(key(Some(&identity), &slug), slug);
    }

    for kind in ArtKind::ALL {
        let dir = assets.join(kind.dir());
        let Ok(entrees) = std::fs::read_dir(&dir) else {
            continue;
        };
        let (mut faits, mut avant, mut apres) = (0usize, 0u64, 0u64);
        for entree in entrees.flatten() {
            let chemin = entree.path();
            let Some(cle) = chemin
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
            if deja_bon(kind, &dir, &cle) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&chemin) else {
                continue;
            };
            let Some((normalisees, nouvelle)) = normaliser(kind, &bytes) else {
                continue;
            };
            let destination = dir.join(format!("{cle}.{nouvelle}"));
            write_atomic(&destination, &normalisees);
            if destination != chemin {
                let _ = std::fs::remove_file(&chemin);
            }
            // la vignette est dérivée de la grille : elle sera refaite
            if kind == ArtKind::Grid {
                let slug = paquets.get(&cle).cloned().unwrap_or_else(|| cle.clone());
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

/// Visuel en cache sous un nom donné (png, jpg ou webp).
pub fn cached(dir: &Path, nom: &str) -> Option<PathBuf> {
    IMAGE_EXT
        .into_iter()
        .map(|ext| dir.join(format!("{nom}.{ext}")))
        .find(|path| path.is_file())
}

/// Visuel d'un jeu : sa **clé partagée** d'abord ([`key`]), puis son seul id
/// (`8103.jpg`), au cas où un autre poste l'a écrit avant de connaître le nom
/// exact du jeu. Deux `stat` de plus, jamais de lecture de dossier.
pub fn cached_for(dir: &Path, identity: Option<&Identity>, slug: &str) -> Option<PathBuf> {
    let cle = key(identity, slug);
    if let Some(path) = cached(dir, &cle) {
        return Some(path);
    }
    match identity {
        Some(identity) if cle != identity.id.to_string() => cached(dir, &identity.id.to_string()),
        _ => None,
    }
}

/// Visuel **choisi par l'utilisateur** : il y a un marqueur `.choice` dans le
/// dossier des marqueurs (jamais à côté de l'image : `assets/` est destiné à
/// être partagé par Syncthing, un marqueur personnel n'y a pas sa place).
pub fn chosen(dir: &Path, marks: &Path, cle: &str, slug: &str) -> Option<PathBuf> {
    if !marks.join(format!("{slug}.choice")).is_file() {
        return None;
    }
    cached(dir, cle)
}

/// Vrai si au moins un des quatre visuels existe déjà pour ce jeu : c'est ce
/// qui déclenche la question « remplacer les visuels ? » quand on identifie un
/// paquet qui a déjà des images (elles peuvent avoir été choisies à la main).
pub fn has_art(assets: &Path, identity: Option<&Identity>, slug: &str) -> bool {
    ArtKind::ALL
        .iter()
        .any(|kind| cached_for(&assets.join(kind.dir()), identity, slug).is_some())
}

/// Range les visuels déjà téléchargés sous la **clé du jeu** et remet les
/// années sous cette clé aussi (`--shrink`/mise à jour d'une installation
/// antérieure, où les fichiers portaient le slug du paquet).
///
/// Sans ça, changer le nommage orphelinerait tout un cache partagé (110 Mo sur
/// ce poste). Renommage seulement, jamais d'écrasement : si la clé existe déjà
/// (image arrivée d'un autre poste), le fichier ancien reste où il est. Rend le
/// nombre de visuels renommés.
pub fn migrate_names(assets: &Path, marks_root: &Path) -> usize {
    let identities = load_identities(&identities_path(assets));
    if identities.is_empty() {
        return 0;
    }
    let mut renommes = 0;
    for (slug, identity) in &identities {
        renommes += rename_to_key(assets, slug, identity);
    }
    // les marqueurs (`.miss`, `.choice`) restent par paquet : rien à renommer
    let _ = marks_root;

    // années : la table partagée suit les visuels
    let path = years_path(assets);
    let old = load_years(&path);
    let mut new: BTreeMap<String, Option<u32>> = BTreeMap::new();
    let mut bouge = false;
    for (nom, year) in old {
        let cle = match identities.get(&nom) {
            Some(identity) => key(Some(identity), &nom),
            None => nom.clone(),
        };
        bouge |= cle != nom;
        new.entry(cle).or_insert(year);
    }
    if bouge {
        // réécriture directe : `save_years` fusionnerait avec l'ancien contenu
        let mut text = String::new();
        for (nom, year) in &new {
            match year {
                Some(year) => text.push_str(&format!("{nom}={year}\n")),
                None => text.push_str(&format!("{nom}=\n")),
            }
        }
        write_atomic(&path, text.as_bytes());
    }
    renommes
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
/// l'autre machine via Syncthing. `cle` = nom du fichier dans `assets/`
/// (partagé), `slug` = paquet (marqueurs locaux).
fn save(
    kind: ArtKind,
    dir: &Path,
    marks: &Path,
    cle: &str,
    slug: &str,
    ext: &str,
    bytes: &[u8],
    url: Option<&str>,
) -> PathBuf {
    let (bytes, ext) = match normaliser(kind, bytes) {
        Some((normalisees, cible)) => (normalisees, cible),
        None => (bytes.to_vec(), ext),
    };
    ecrire(dir, marks, cle, slug, ext, &bytes, url)
}

/// Écriture commune d'un visuel : un seul fichier par type (les autres
/// extensions sont retirées), marqueur `.choice` quand c'est un choix de
/// l'utilisateur, écriture atomique.
fn ecrire(
    dir: &Path,
    marks: &Path,
    cle: &str,
    slug: &str,
    ext: &str,
    bytes: &[u8],
    url: Option<&str>,
) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    for other in IMAGE_EXT {
        if other != ext {
            let _ = std::fs::remove_file(dir.join(format!("{cle}.{other}")));
        }
    }
    let path = dir.join(format!("{cle}.{ext}"));
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

fn mark_missing(dir: &Path, marks: &Path, cle: &str, slug: &str) {
    let _ = std::fs::create_dir_all(dir);
    for ext in IMAGE_EXT {
        let _ = std::fs::remove_file(dir.join(format!("{cle}.{ext}")));
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

    /// Jeu SteamGridDB visé par un paquet : sa **fiche** quand l'id est connu
    /// (nom exact + année), sinon le premier résultat de l'autocomplete sur le
    /// nom. Une seule requête, c'est elle qui donne la clé des visuels.
    fn find(&mut self, name: &str, id: Option<u64>) -> Result<Option<FoundGame>, String> {
        match (self.key.clone(), id) {
            (Some(key), Some(id)) => {
                let auth = format!("Bearer {key}");
                let url = format!("{BASE}/api/v2/games/id/{id}");
                let body = http_get_text(&url, &[("Authorization", auth.as_str())])?;
                let json = parse_json(&body)?;
                Ok(Some(game_from(&json["data"], id, name)))
            }
            (Some(key), None) => {
                let auth = format!("Bearer {key}");
                let url = format!("{BASE}/api/v2/search/autocomplete/{}", urlencode(name));
                let body = http_get_text(&url, &[("Authorization", auth.as_str())])?;
                let json = parse_json(&body)?;
                Ok(games_from(&json["data"]).into_iter().next())
            }
            (None, Some(id)) => {
                let cookie = self.visitor_cookie()?.to_string();
                let headers = [
                    ("Cookie", cookie.as_str()),
                    ("X-Requested-With", "XMLHttpRequest"),
                    ("Referer", BASE),
                ];
                let url = format!("{BASE}/api/public/game/{id}");
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                Ok(Some(game_from(&json["data"]["game"], id, name)))
            }
            (None, None) => {
                let cookie = self.visitor_cookie()?.to_string();
                let headers = [
                    ("Cookie", cookie.as_str()),
                    ("X-Requested-With", "XMLHttpRequest"),
                    ("Referer", BASE),
                ];
                let url = format!("{BASE}/api/public/search/autocomplete?term={}", urlencode(name));
                let body = http_get_text(&url, &headers)?;
                let json = parse_json(&body)?;
                Ok(games_from(&json["data"]).into_iter().next())
            }
        }
    }

    /// Premier visuel du « top » pour ce type, téléchargé. `id` = jeu
    /// SteamGridDB : aucune recherche par nom n'est refaite.
    fn top(&mut self, id: u64, kind: ArtKind) -> Result<Option<Best>, String> {
        let Some(best) = self.assets(id, kind)?.into_iter().next() else {
            return Ok(None);
        };
        let bytes = http_get(&best.url, &[])?;
        if bytes.len() < 512 {
            return Err(format!("{} vide ({})", kind.label(), best.url));
        }
        Ok(Some((best.url, bytes, best.ext)))
    }

    /// Toutes les images d'un type, dans l'ordre « top » de SteamGridDB.
    fn assets(&mut self, id: u64, kind: ArtKind) -> Result<Vec<Candidate>, String> {
        match self.key.clone() {
            Some(key) => self.candidates_official(&key, id, kind),
            None => self.candidates_public(id, kind),
        }
    }

    // --- API interne (sans compte) ---

    /// Recherche sans compte, avec **une seconde chance** : quand la session
    /// visiteur est périmée (Cloudflare répond une page vide → « réponse
    /// vide »), on en reprend une et on réessaie une fois. Sans ça, un jeu
    /// restait sans résultat — y compris dans le menu « changer d'image ».
    fn candidates_public(&mut self, id: u64, kind: ArtKind) -> Result<Vec<Candidate>, String> {
        match self.candidates_public_once(id, kind) {
            Ok(list) => Ok(list),
            Err(first) => {
                eprintln!("glauncher : SGDB jeu {id} : {first} — nouvelle session, nouvel essai");
                self.cookie = None;
                std::thread::sleep(Duration::from_millis(600));
                self.candidates_public_once(id, kind)
            }
        }
    }

    fn candidates_public_once(
        &mut self,
        game_id: u64,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        let cookie = self.visitor_cookie()?.to_string();
        let headers = [
            ("Cookie", cookie.as_str()),
            ("X-Requested-With", "XMLHttpRequest"),
            ("Referer", BASE),
        ];

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

    fn candidates_official(&mut self, key: &str, id: u64, kind: ArtKind) -> Result<Vec<Candidate>, String> {
        let auth = format!("Bearer {key}");
        let headers = [("Authorization", auth.as_str())];

        let url = format!(
            "{BASE}/api/v2/{}/{id}?types=static&nsfw=false&humor=false",
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

/// Fiche d'un jeu (API v2 ou `data.game` de l'API publique) : id, nom et année.
/// Le nom retombe sur celui demandé si la réponse ne le porte pas — la clé des
/// visuels ne doit jamais être vide.
fn game_from(game: &Value, id: u64, name: &str) -> FoundGame {
    let nom = game
        .get("name")
        .and_then(Value::as_str)
        .filter(|nom| !nom.trim().is_empty())
        .unwrap_or(name);
    FoundGame {
        id: game.get("id").and_then(Value::as_u64).unwrap_or(id),
        name: nom.to_string(),
        year: game_year(game),
    }
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
mod tests_oublier {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn oublier_slugs_garde_les_presents_et_les_commentaires() {
        let dir = std::env::temp_dir().join(format!("glauncher-oublier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("years.txt");
        std::fs::write(&path, "# commentaire\nmario=1996\nzelda=1998\n").unwrap();
        let garde: HashSet<String> = ["mario".to_string()].into();
        oublier_slugs(&path, &garde);
        let texte = std::fs::read_to_string(&path).unwrap();
        assert!(texte.contains("# commentaire"));
        assert!(texte.contains("mario=1996"));
        assert!(!texte.contains("zelda"));
        let _ = std::fs::remove_dir_all(&dir);
    }
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
        assert!(has_art(&assets, None, "jeu"));
        assert!(!has_art(&assets, None, "autre"));

        // identifié : la clé est celle du jeu SteamGridDB, pas le paquet
        let identite = Identity {
            id: 8103,
            name: "Doom".to_string(),
        };
        std::fs::write(exe.join("8103-doom.jpg"), b"x").unwrap();
        assert!(has_art(&assets, Some(&identite), "paquet-dont-le-nom-differe"));
        let _ = std::fs::remove_dir_all(&assets);
    }

    #[test]
    fn la_cle_des_fichiers_est_le_jeu_pas_le_paquet() {
        // pas d'identification : le paquet, et ses visuels ne sont pas partagés
        assert_eq!(key(None, "marsupilami-2-fr"), "marsupilami-2-fr");
        // identifié : id + nom SteamGridDB, identiques sur tous les postes même
        // quand les `.wgp`/`.lgp` portent des noms différents
        let identite = Identity {
            id: 5334897,
            name: "Marsupilami 2: Salsa Palombia".to_string(),
        };
        assert_eq!(
            key(Some(&identite), "marsupilami-2-fr"),
            "5334897-marsupilami-2-salsa-palombia"
        );
        assert_eq!(
            key(Some(&identite), "marsupilami-2-salsa-palombia"),
            "5334897-marsupilami-2-salsa-palombia"
        );
        // deux jeux du même nom (« Doom » 1993 et 2016) ne se marchent pas
        // dessus : l'id est en tête
        let autre = Identity {
            id: 32969,
            name: "Doom".to_string(),
        };
        assert_ne!(key(Some(&identite), "x"), key(Some(&autre), "x"));
        // identification sans nom (ligne tapée à la main) : l'id seul, pour
        // rester la même clé partout
        let sans_nom = Identity {
            id: 8103,
            name: String::new(),
        };
        assert_eq!(key(Some(&sans_nom), "doom-2016"), "8103");
    }

    #[test]
    fn un_visuel_trouve_sous_son_id_suffit() {
        let dir = std::env::temp_dir().join(format!("glauncher-idseul-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let identite = Identity {
            id: 8103,
            name: "Doom".to_string(),
        };
        // écrit par un poste qui ne connaissait pas encore le nom exact
        std::fs::write(dir.join("8103.jpg"), b"x").unwrap();
        assert_eq!(
            cached_for(&dir, Some(&identite), "doom-2016"),
            Some(dir.join("8103.jpg"))
        );
        // la clé complète passe devant
        std::fs::write(dir.join("8103-doom.jpg"), b"x").unwrap();
        assert_eq!(
            cached_for(&dir, Some(&identite), "doom-2016"),
            Some(dir.join("8103-doom.jpg"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn les_anciens_noms_migrent_vers_la_cle_du_jeu() {
        let dir = std::env::temp_dir().join(format!("glauncher-migre-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let assets = dir.join("assets");
        let marks = dir.join("marks");
        std::fs::create_dir_all(assets.join("grids")).unwrap();
        std::fs::create_dir_all(assets.join("logos")).unwrap();
        std::fs::create_dir_all(&marks).unwrap();

        let mut identities = BTreeMap::new();
        identities.insert(
            "doom-2016".to_string(),
            Identity {
                id: 8103,
                name: "Doom".to_string(),
            },
        );
        save_identities(&identities_path(&assets), &identities);
        std::fs::write(assets.join("grids/doom-2016.jpg"), b"grille").unwrap();
        std::fs::write(assets.join("logos/doom-2016.png"), b"logo").unwrap();
        // un jeu jamais identifié garde son nom de paquet
        std::fs::write(assets.join("grids/mon-homebrew.jpg"), b"grille").unwrap();
        // années : la table partagée suit les visuels
        let mut years = BTreeMap::new();
        years.insert("doom-2016".to_string(), Some(2016));
        years.insert("mon-homebrew".to_string(), Some(2024));
        save_years(&years_path(&assets), &years);

        let renommes = migrate_names(&assets, &marks);
        assert_eq!(renommes, 2, "grille et logo renommés");
        assert!(assets.join("grids/8103-doom.jpg").is_file());
        assert!(assets.join("logos/8103-doom.png").is_file());
        assert!(!assets.join("grids/doom-2016.jpg").exists());
        assert!(assets.join("grids/mon-homebrew.jpg").is_file());

        let years = load_years(&years_path(&assets));
        assert_eq!(years.get("8103-doom"), Some(&Some(2016)));
        assert_eq!(years.get("mon-homebrew"), Some(&Some(2024)));
        assert!(!years.contains_key("doom-2016"));

        // idempotent : rien à refaire au démarrage suivant
        assert_eq!(migrate_names(&assets, &marks), 0);
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(chosen(&dir, &marks, "jeu", "jeu"), Some(path));
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

    /// Sonde réseau (ignorée) : la fiche d'un jeu donne son nom et son année.
    #[test]
    #[ignore]
    fn annee_reseau() {
        let mut client = Client::new(std::env::var("SGDB_API_KEY").ok());
        for (name, annee) in [("Wario Land 4", 2001), ("Wario Land II", 1998)] {
            let game = client
                .find(name, None)
                .unwrap_or_else(|err| panic!("{name} : {err}"))
                .unwrap_or_else(|| panic!("{name} : aucun jeu"));
            eprintln!("{name} → {} ({:?})", game.name, game.year);
            assert_eq!(game.year, Some(annee), "{name}");
            assert!(!game.name.is_empty(), "le nom sert à la clé des fichiers");
        }
    }

    /// Sonde réseau (ignorée) : le top doit être proposé pour chaque type.
    #[test]
    #[ignore]
    fn choix_multiples() {
        let mut client = Client::new(std::env::var("SGDB_API_KEY").ok());
        let game = client
            .find("Beyond Good & Evil", None)
            .expect("autocomplete")
            .expect("jeu connu");
        for kind in ArtKind::ALL {
            let choices = client
                .assets(game.id, kind)
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

#[cfg(test)]
mod tests_absents {
    use super::*;

    #[test]
    fn dates_aller_retour() {
        assert_eq!(jours_depuis_date("1970-01-01"), Some(0));
        assert_eq!(date_depuis_jours(0), (1970, 1, 1));
        let jours = jours_depuis_date("2026-11-23").expect("date valide");
        assert_eq!(date_depuis_jours(jours), (2026, 11, 23));
    }

    #[test]
    fn date_locale_coherente() {
        // la marque écrite aujourd'hui doit être considérée fraîche, même
        // quand UTC est encore la veille (banc à 00:45 CET)
        assert_eq!(
            jours_depuis_date(&date_aujourd_hui()),
            Some(jours_aujourd_hui())
        );
    }

    #[test]
    fn marque_absent_datee_puis_levee() {
        let dir = std::env::temp_dir().join(format!("glauncher-absents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dossier de test");
        let path = dir.join("absents.txt");
        let marque = marquer_absent(&path, "jeu-inconnu", "Jeu Inconnu");
        assert!(absent_frais(&marque), "{marque:?}");
        assert_eq!(load_absents(&path).get("jeu-inconnu"), Some(&marque));
        // une autre marque ne l'écrase pas
        marquer_absent(&path, "autre", "Autre");
        assert_eq!(load_absents(&path).len(), 2);
        clear_absent(&path, "jeu-inconnu");
        let relu = load_absents(&path);
        assert!(!relu.contains_key("jeu-inconnu"));
        assert!(relu.contains_key("autre"));
        clear_absent(&path, "autre");
        assert!(!path.exists(), "plus aucune marque = fichier retiré");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alias_copie_et_marque_le_choix() {
        let dir = std::env::temp_dir().join(format!("glauncher-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let assets = dir.join("assets");
        let marks = dir.join("marks");
        std::fs::create_dir_all(assets.join("grids")).expect("dossier grids");
        std::fs::write(assets.join("grids").join("source.png"), b"png").expect("source");
        let copies = alias_visuels(&assets, &marks, "source", "cible", "slug-cible");
        assert_eq!(copies, 1);
        assert_eq!(
            std::fs::read(assets.join("grids").join("cible.png")).expect("copie"),
            b"png"
        );
        assert!(marks.join("grids").join("slug-cible.choice").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
