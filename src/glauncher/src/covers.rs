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

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
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
    /// de SteamGridDB, sans reclassement).
    Auto {
        slug: String,
        name: String,
        kinds: Vec<ArtKind>,
    },
    /// Renvoie toutes les images possibles d'un type, vignettes comprises
    /// (menu « choisir une image »).
    Choices {
        slug: String,
        name: String,
        kind: ArtKind,
    },
    /// Enregistre le choix de l'utilisateur. `url` peut être une URL du CDN ou
    /// un `file://` (icône extraite du paquet) : le fichier écrit dans le cache
    /// fait foi et n'est jamais remplacé automatiquement.
    Pick {
        slug: String,
        kind: ArtKind,
        url: String,
        ext: String,
    },
    /// Extrait l'icône du paquet lui-même (`.icon.png` à la racine du squashfs,
    /// comme le fait `create-game-shortcut.sh` de Gablue).
    Icon { slug: String, pack: PathBuf },
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
    /// Icône extraite du paquet (voie locale, distincte des icônes SGDB).
    Icon { slug: String, bytes: Vec<u8> },
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
                    Query::Auto { slug, name, kinds } => {
                        for kind in kinds {
                            let dir = cache.join(kind.dir());
                            let marks = marks.join(kind.dir());
                            let fetched = client.top(&name, kind);
                            std::thread::sleep(THROTTLE);
                            match fetched {
                                Ok(Some((url, bytes, ext))) => {
                                    let path = save(&dir, &marks, &slug, ext, &bytes, None);
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
                    Query::Choices { slug, name, kind } => match client.choices(&name, kind) {
                        Ok(candidates) => {
                            let total = candidates.len();
                            eprintln!(
                                "glauncher : {total} {} proposé(s) pour « {name} », vignettes en cours",
                                kind.label().to_lowercase()
                            );
                            let mut choices = Vec::new();
                            for candidate in candidates.into_iter().take(MAX_CHOICES) {
                                if let Ok(thumb) = http_get(&candidate.thumb, &[]) {
                                    choices.push(Choice {
                                        url: candidate.url,
                                        ext: candidate.ext.to_string(),
                                        thumb,
                                    });
                                }
                            }
                            eprintln!("glauncher : {} vignette(s) prête(s)", choices.len());
                            emit(CoverEvent::Choices { slug, kind, choices });
                        }
                        Err(error) => {
                            eprintln!("glauncher : choix « {name} » : {error}");
                            emit(CoverEvent::Failed {
                                slug,
                                kind,
                                error,
                            });
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
                                let path = save(&dir, &marks, &slug, &ext, &bytes, Some(&url));
                                eprintln!(
                                    "glauncher : {} choisi → {}",
                                    kind.label(),
                                    path.display()
                                );
                                emit(CoverEvent::Found { slug, kind, bytes });
                            }
                            Err(error) => {
                                eprintln!("glauncher : choix ({url}) : {error}");
                                emit(CoverEvent::Failed {
                                    slug,
                                    kind,
                                    error,
                                });
                            }
                        }
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

    for (slug, name, pack) in games {
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
            match client.top(name, kind) {
                Ok(Some((url, bytes, ext))) => {
                    let path = save(&dir, &marks, slug, ext, &bytes, None);
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
/// marqueur `.choice` est posé et les autres formats du même jeu sont retirés.
fn save(
    dir: &Path,
    marks: &Path,
    slug: &str,
    ext: &str,
    bytes: &[u8],
    url: Option<&str>,
) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    if url.is_some() {
        for other in IMAGE_EXT {
            if other != ext {
                let _ = std::fs::remove_file(dir.join(format!("{slug}.{other}")));
            }
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

    /// Premier visuel du « top » pour ce type, téléchargé.
    fn top(&mut self, name: &str, kind: ArtKind) -> Result<Option<Best>, String> {
        let Some(best) = self.choices(name, kind)?.into_iter().next() else {
            return Ok(None);
        };
        let bytes = http_get(&best.url, &[])?;
        if bytes.len() < 512 {
            return Err(format!("{} vide ({})", kind.label(), best.url));
        }
        Ok(Some((best.url, bytes, best.ext)))
    }

    /// Toutes les images d'un type, dans l'ordre « top » de SteamGridDB.
    fn choices(&mut self, name: &str, kind: ArtKind) -> Result<Vec<Candidate>, String> {
        match self.key.clone() {
            Some(key) => self.candidates_official(&key, name, kind),
            None => self.candidates_public(name, kind),
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
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        match self.candidates_public_once(name, kind) {
            Ok(list) => Ok(list),
            Err(first) => {
                eprintln!(
                    "glauncher : SGDB « {name} » : {first} — nouvelle session, nouvel essai"
                );
                self.cookie = None;
                std::thread::sleep(Duration::from_millis(600));
                self.candidates_public_once(name, kind)
            }
        }
    }

    fn candidates_public_once(
        &mut self,
        name: &str,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        let cookie = self.visitor_cookie()?.to_string();
        let headers = [
            ("Cookie", cookie.as_str()),
            ("X-Requested-With", "XMLHttpRequest"),
            ("Referer", BASE),
        ];

        let term = urlencode(name);
        let url = format!("{BASE}/api/public/search/autocomplete?term={term}");
        let body = http_get_text(&url, &headers)?;
        let json = parse_json(&body)?;
        let Some(game_id) = first_game(&json) else {
            return Ok(Vec::new());
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

    // --- API v2 officielle (clé) ---

    fn candidates_official(
        &mut self,
        key: &str,
        name: &str,
        kind: ArtKind,
    ) -> Result<Vec<Candidate>, String> {
        let auth = format!("Bearer {key}");
        let headers = [("Authorization", auth.as_str())];

        let term = urlencode(name);
        let url = format!("{BASE}/api/v2/search/autocomplete/{term}");
        let body = http_get_text(&url, &headers)?;
        let json = parse_json(&body)?;
        let Some(game_id) = first_game(&json) else {
            return Ok(Vec::new());
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
            asset["width"].as_f64().or_else(|| asset["width"].as_i64().map(|v| v as f64)),
            asset["height"].as_f64().or_else(|| asset["height"].as_i64().map(|v| v as f64)),
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
        assert_eq!(grids.len(), 1, "seule la 2:3 reste dans le menu des grilles");
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
        let path = save(
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
        assert!(!dir.join("jeu.miss").exists(), "aucun marqueur dans assets/");
        assert!(!marks.join("jeu.miss").exists(), "le marqueur d'échec saute");
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

    /// Sonde réseau (ignorée) : le top doit être proposé pour chaque type.
    #[test]
    #[ignore]
    fn choix_multiples() {
        let mut client = Client::new(std::env::var("SGDB_API_KEY").ok());
        for kind in ArtKind::ALL {
            let choices = client
                .choices("Beyond Good & Evil", kind)
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
