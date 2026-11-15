//! Aiguillage HTTP : routes `/api/*`, `/f/*` (fichier, avec Range) et `/a/*`
//! (dossier en tar). Aucune écriture, aucune session.

use crate::config::{Config, Share};
use crate::files::{self, RangeSpec};
use crate::tarstream::TarStream;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use tiny_http::{Header, Method, Request, Response, StatusCode};

pub type Body = Box<dyn Read + Send>;

const LIST_DEFAULT: usize = 500;
const LIST_MAX: usize = 5000;

/// En-tête ASCII ; les valeurs produites ici le sont toujours (noms encodés).
fn h(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("en-tête ASCII valide")
}

fn body_response(
    status: u16,
    headers: Vec<Header>,
    body: Body,
    length: Option<usize>,
) -> Response<Body> {
    let response = Response::new(StatusCode(status), headers, body, length, None);
    // Sans ce plafond, tiny_http passe en chunked tout corps > 32 Kio : plus de
    // Content-Length, donc plus de taille annoncée pour la reprise (le HEAD
    // d'un gros fichier ne dit plus sa taille). Les réponses à longueur connue
    // déclarent toujours leur longueur ; le tar (longueur inconnue) reste chunked.
    if length.is_some() {
        response.with_chunked_threshold(usize::MAX)
    } else {
        response
    }
}

fn json_response(status: u16, value: &Value) -> Response<Body> {
    let text = value.to_string();
    let length = text.len();
    body_response(
        status,
        vec![
            h("Content-Type", "application/json; charset=utf-8"),
            h("Cache-Control", "no-store"),
        ],
        Box::new(Cursor::new(text.into_bytes())),
        Some(length),
    )
}

fn text_response(status: u16, text: String) -> Response<Body> {
    let length = text.len();
    body_response(
        status,
        vec![
            h("Content-Type", "text/plain; charset=utf-8"),
            h("Cache-Control", "no-store"),
        ],
        Box::new(Cursor::new(text.into_bytes())),
        Some(length),
    )
}

fn error(status: u16, message: &str) -> Response<Body> {
    json_response(status, &json!({ "error": message }))
}

/// Réponse 403 utilisée par le filtre d'adresses (appliqué dans `lib.rs`).
pub fn forbidden() -> Response<Body> {
    error(403, "accès réservé au réseau local et au tailnet")
}

fn split_url(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((path, query)) => (path, query),
        None => (url, ""),
    }
}

/// En-tête portant l'empreinte du mot de passe d'un partage verrouillé.
///
/// Le client ne transmet **jamais** le mot de passe : il envoie la valeur
/// dérivée (`pbkdf2` avec le sel publié), que le démon compare à celle qu'il
/// garde. Rejouer cet en-tête ouvre le partage tant que le mot de passe n'a pas
/// changé — sans TLS sur le réseau local, un mot de passe en clair serait de
/// toute façon lisible par la même oreille indiscrète.
const KEY_HEADER: &str = "X-Gablue-Key";

fn key_header(req: &Request) -> Option<String> {
    req.headers()
        .iter()
        .find(|header| header.field.equiv(KEY_HEADER))
        .map(|header| header.value.as_str().to_string())
}

fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some(parts) => parts,
            None => (pair, ""),
        };
        if name == key {
            // `application/x-www-form-urlencoded` : `+` vaut une espace dans une
            // valeur de requête (le client encode les espaces ainsi). Un `+`
            // littéral dans un nom de fichier arrive, lui, en `%2B` — d'où la
            // conversion ici et nulle part ailleurs (le chemin d'URL, lui,
            // garde ses `+`).
            return files::percent_decode(&value.replace('+', " "));
        }
    }
    None
}

/// Point d'entrée. Le filtre d'adresses est appliqué par l'appelant.
pub fn handle(req: &Request, cfg: &Config) -> Response<Body> {
    let method = req.method().clone();
    let (path, query) = split_url(req.url());
    let head = method == Method::Head;
    if method != Method::Get && !head {
        return error(405, "méthode non autorisée (GET et HEAD seulement)");
    }
    let segments = match files::split_path(path) {
        Some(segments) => segments,
        None => return error(400, "chemin invalide"),
    };

    match segments.first().map(String::as_str) {
        None => help(cfg),
        Some("api") => match segments.get(1).map(String::as_str) {
            Some("info") if segments.len() == 2 => api_info(cfg),
            Some("auth") if segments.len() == 3 => api_auth(req, cfg, &segments[2]),
            Some("list") if segments.len() == 3 => api_list(req, cfg, &segments[2], query, head),
            Some("find") if segments.len() == 3 => api_find(req, cfg, &segments[2], query),
            Some("size") if segments.len() == 3 => api_size(req, cfg, &segments[2], query),
            _ => error(404, "route inconnue"),
        },
        Some("f") if segments.len() >= 3 => {
            serve_file(req, cfg, &segments[1], &segments[2..], head)
        }
        Some("a") if segments.len() >= 2 => serve_tar(req, cfg, &segments[1], &segments[2..], query),
        _ => error(404, "route inconnue"),
    }
}

fn share_or_404<'a>(cfg: &'a Config, id: &str) -> Result<&'a Share, Response<Body>> {
    match cfg.share(id) {
        // Un disque débranché n'est pas « inconnu » : le dire évite de faire
        // croire à une faute de configuration côté pair.
        Some(share) if !share.available => {
            Err(error(404, "partage indisponible (dossier absent)"))
        }
        Some(share) => Ok(share),
        None => Err(error(404, "partage inconnu")),
    }
}

/// Réponse 401 : le partage existe, mais il demande son mot de passe.
fn locked_error(message: &str) -> Response<Body> {
    let mut response = error(401, message);
    // Un 401 doit annoncer son schéma ; le client de Gablue Share, lui, envoie
    // l'empreinte sans attendre cette invite.
    response.add_header(h("WWW-Authenticate", "Gablue-Share"));
    response
}

/// Comme `share_or_404`, mais exige l'empreinte du partage quand il est
/// protégé. Appliqué à **toutes** les routes qui livrent quelque chose (liste,
/// recherche, taille, fichier, archive) : une seule oubliée serait une porte.
fn share_or_denied<'a>(
    req: &Request,
    cfg: &'a Config,
    id: &str,
) -> Result<&'a Share, Response<Body>> {
    let share = share_or_404(cfg, id)?;
    if !share.accepts(key_header(req).as_deref()) {
        return Err(locked_error("mot de passe requis pour ce partage"));
    }
    Ok(share)
}

/// Vue d'un partage : ce qu'il accepte de montrer (réglé par celui qui partage).
fn view_of(share: &Share) -> files::View {
    files::View {
        hidden: share.hidden,
        subdirs: share.recursive,
    }
}

/// Un sous-chemin est-il hors du partage ? (partage limité à sa racine)
fn beyond_root(share: &Share, segments: &[String]) -> bool {
    !share.recursive && !segments.is_empty()
}

fn api_info(cfg: &Config) -> Response<Body> {
    let shares: Vec<Value> = cfg
        .available()
        .map(|share| {
            // `locked` voyage dès l'annonce : la GUI sait qu'il faudra un mot de
            // passe avant même d'ouvrir le partage, et la recherche globale
            // l'écarte tant qu'il n'est pas déverrouillé.
            let mut item = json!({
                "id": share.id,
                "name": share.name,
                "locked": share.locked(),
            });
            if let Some(password) = &share.password {
                // Sel et itérations sont publics : sans eux le client ne peut
                // pas recalculer l'empreinte. `hash` reste ici, jamais dehors.
                item["salt"] = json!(password.salt);
                item["rounds"] = json!(password.rounds);
                // Jeton de version : il change avec le mot de passe, et c'est
                // tout ce qu'un client mémorise pour savoir qu'il doit le
                // redemander (l'empreinte, elle, ne dit rien de l'ancien).
                item["revision"] = json!(password.revision);
            }
            item
        })
        .collect();
    // `uid` : à quel compte système appartient ce démon. Le port est fixe et
    // `peer_id` est le machine-id, donc deux comptes d'un même poste se
    // ressemblent trait pour trait — sans ce champ, la GUI du second prendrait
    // le démon de l'autre pour le sien et n'aurait aucun moyen de le dire.
    let uid = unsafe { libc::getuid() };
    json_response(
        200,
        &json!({
            "protocol": 1,
            "peer_id": cfg.peer_id,
            "name": cfg.name,
            "port": cfg.port,
            "uid": uid,
            "shares": shares,
        }),
    )
}

/// Le mot de passe fourni ouvre-t-il ce partage ?
///
/// Sert à la GUI pour vérifier une saisie **avant** d'ouvrir quoi que ce soit :
/// un mauvais mot de passe se dit dans la boîte de dialogue, pas par une liste
/// vide ou une erreur de navigation.
fn api_auth(req: &Request, cfg: &Config, share_id: &str) -> Response<Body> {
    let share = match share_or_404(cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    if !share.accepts(key_header(req).as_deref()) {
        return locked_error("mot de passe incorrect");
    }
    json_response(200, &json!({ "share": share.id, "locked": share.locked() }))
}

fn api_list(req: &Request, cfg: &Config, share_id: &str, query: &str, head: bool) -> Response<Body> {
    let share = match share_or_denied(req, cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    let relative = query_param(query, "path").unwrap_or_default();
    let segments = match files::split_path(&relative) {
        Some(segments) => segments,
        None => return error(400, "chemin invalide"),
    };
    if beyond_root(share, &segments) {
        return error(404, "ce partage ne contient que sa racine");
    }
    let dir = match files::resolve(&share.root, &segments) {
        Some(path) => path,
        None => return error(404, "dossier introuvable"),
    };
    if !dir.is_dir() {
        return error(404, "ce n'est pas un dossier");
    }
    let view = view_of(share);

    // `HEAD` = simple empreinte du dossier : la GUI demande « ça a bougé ? »
    // toutes les quelques secondes sans relire le listing (quelques centaines
    // d'octets au lieu de plusieurs centaines de Kio sur un gros dossier).
    if head {
        let tag = match files::fingerprint(&share.root, &dir, view) {
            Ok(tag) => tag,
            Err(err) => return error(500, &format!("lecture impossible : {err}")),
        };
        let unchanged = req
            .headers()
            .iter()
            .find(|header| header.field.equiv("If-None-Match"))
            .map(|header| header.value.as_str().trim() == tag)
            .unwrap_or(false);
        let status = if unchanged { 304 } else { 200 };
        return body_response(
            status,
            vec![h("ETag", &tag), h("Cache-Control", "no-store")],
            Box::new(std::io::empty()),
            Some(0),
        );
    }

    let offset = query_param(query, "offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let limit = query_param(query, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(LIST_DEFAULT)
        .clamp(1, LIST_MAX);
    let sort = files::Sort::parse(&query_param(query, "sort").unwrap_or_default());
    let descending = query_param(query, "desc").map(|value| value == "1").unwrap_or(false);

    let (entries, truncated, tag) =
        match files::list(&share.root, &dir, offset, limit, sort, descending, view) {
        Ok(page) => page,
        Err(err) => return error(500, &format!("lecture impossible : {err}")),
    };
    let parent = {
        let mut parts = segments.clone();
        parts.pop();
        if parts.is_empty() {
            Value::Null
        } else {
            Value::String(parts.join("/"))
        }
    };
    let body = json!({
        "share": share.id,
        "path": segments.join("/"),
        "parent": parent,
        "entries": entries,
        "truncated": truncated,
    })
    .to_string();
    let length = body.len();
    body_response(
        200,
        vec![
            h("Content-Type", "application/json; charset=utf-8"),
            h("Cache-Control", "no-store"),
            // Le même ETag que le HEAD : sans lui, la première lecture ne
            // laisserait aucune référence et la sonde suivante relancerait un
            // listing complet pour rien.
            h("ETag", &tag),
        ],
        Box::new(Cursor::new(body.into_bytes())),
        Some(length),
    )
}

/// Recherche récursive par nom dans un partage (ou un sous-dossier).
///
/// Budget de temps fixe : c'est ce qui garantit qu'un partage de 200 000
/// fichiers ne bloque pas le serveur. `truncated` dit au client que le résultat
/// est partiel, `scanned` lui donne une idée du coût.
fn api_find(req: &Request, cfg: &Config, share_id: &str, query: &str) -> Response<Body> {
    let share = match share_or_denied(req, cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    let needle = match query_param(query, "q") {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => return error(400, "paramètre q manquant"),
    };
    let relative = query_param(query, "path").unwrap_or_default();
    let segments = match files::split_path(&relative) {
        Some(segments) => segments,
        None => return error(400, "chemin invalide"),
    };
    if beyond_root(share, &segments) {
        return error(404, "ce partage ne contient que sa racine");
    }
    let start = match files::resolve(&share.root, &segments) {
        Some(path) if path.is_dir() => path,
        _ => return error(404, "dossier introuvable"),
    };
    let limit = query_param(query, "limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(200)
        .clamp(1, 1000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
    let (entries, truncated, scanned) =
        files::search(&share.root, &start, &needle, deadline, limit, view_of(share));
    json_response(
        200,
        &json!({
            "share": share.id,
            "query": needle,
            "path": segments.join("/"),
            "entries": entries,
            "truncated": truncated,
            "scanned": scanned,
        }),
    )
}

/// Taille d'un dossier : calculée à la demande, bornée dans le temps.
///
/// Coûteux sur un arbre géant, donc jamais calculée pour tout un listing —
/// seulement quand l'utilisateur désigne un dossier. `complete: false` signifie
/// « au moins » : les chiffres sont un minorant.
fn api_size(req: &Request, cfg: &Config, share_id: &str, query: &str) -> Response<Body> {
    let share = match share_or_denied(req, cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    let relative = query_param(query, "path").unwrap_or_default();
    let segments = match files::split_path(&relative) {
        Some(segments) => segments,
        None => return error(400, "chemin invalide"),
    };
    if beyond_root(share, &segments) {
        return error(404, "ce partage ne contient que sa racine");
    }
    let dir = match files::resolve(&share.root, &segments) {
        Some(path) => path,
        None => return error(404, "dossier introuvable"),
    };
    if !dir.is_dir() {
        return error(404, "ce n'est pas un dossier");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(600);
    let (files, bytes, complete) = files::tree_size(&dir, deadline);
    json_response(
        200,
        &json!({
            "share": share.id,
            "path": segments.join("/"),
            "files": files,
            "bytes": bytes,
            "complete": complete,
        }),
    )
}

fn serve_file(req: &Request, cfg: &Config, share_id: &str, rel: &[String], head: bool) -> Response<Body> {
    let share = match share_or_denied(req, cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    // Partage limité à sa racine : un fichier de sous-dossier n'est pas servi.
    if !share.recursive && rel.len() > 1 {
        return error(404, "ce partage ne contient que sa racine");
    }
    let path = match files::resolve(&share.root, rel) {
        Some(path) => path,
        None => return error(404, "fichier introuvable"),
    };
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return error(404, "fichier introuvable"),
    };
    if meta.is_dir() {
        return error(409, "dossier : utiliser /a/<partage>/<chemin> (tar)");
    }
    let size = meta.len();
    let tag = files::etag(&meta);

    let if_range = req
        .headers()
        .iter()
        .find(|header| header.field.equiv("If-Range"))
        .map(|header| header.value.as_str().to_string());
    let range = req
        .headers()
        .iter()
        .find(|header| header.field.equiv("Range"))
        .map(|header| header.value.as_str().to_string());

    // If-Range : la reprise n'est valable que si l'ETag n'a pas changé.
    let conditional = match (&range, &if_range) {
        (Some(range), Some(candidate)) if candidate.trim() != tag => {
            let _ = range;
            true // ETag différent : on sert le fichier entier
        }
        (Some(_), Some(_)) => false,
        (Some(_), None) => false,
        (None, _) => false,
    };

    let spec = if conditional {
        RangeSpec::Full
    } else {
        match &range {
            Some(value) => match files::parse_range(value, size) {
                Ok(spec) => spec,
                Err(()) => {
                    return body_response(
                        416,
                        vec![
                            h("Content-Range", &format!("bytes */{size}")),
                            h("Content-Type", "text/plain; charset=utf-8"),
                        ],
                        Box::new(Cursor::new(Vec::new())),
                        Some(0),
                    );
                }
            },
            None => RangeSpec::Full,
        }
    };

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| share.name.clone());
    let mut headers = vec![
        h("Content-Type", mime(&path)),
        h("ETag", &tag),
        h("Accept-Ranges", "bytes"),
        h("Content-Disposition", &disposition(&name)),
    ];

    match spec {
        RangeSpec::Full => {
            if head {
                return body_response(200, headers, Box::new(std::io::empty()), Some(size as usize));
            }
            match File::open(&path) {
                Ok(file) => body_response(200, headers, Box::new(file), Some(size as usize)),
                Err(_) => error(500, "ouverture impossible"),
            }
        }
        RangeSpec::Bytes { start, end } => {
            let length = end - start + 1;
            headers.push(h(
                "Content-Range",
                &format!("bytes {start}-{end}/{size}"),
            ));
            if head {
                return body_response(206, headers, Box::new(std::io::empty()), Some(length as usize));
            }
            match File::open(&path) {
                Ok(mut file) => {
                    if file.seek(SeekFrom::Start(start)).is_err() {
                        return error(500, "lecture impossible");
                    }
                    body_response(206, headers, Box::new(file.take(length)), Some(length as usize))
                }
                Err(_) => error(500, "ouverture impossible"),
            }
        }
    }
}

/// Nom de dossier acceptable dans une archive : jamais de traversée ni de nom
/// caché, sinon une entrée `../` sortirait du dossier de destination.
fn safe_archive_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
    {
        return None;
    }
    Some(name.to_string())
}

fn serve_tar(req: &Request, cfg: &Config, share_id: &str, rel: &[String], query: &str) -> Response<Body> {
    let share = match share_or_denied(req, cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    if !share.recursive && !rel.is_empty() {
        return error(404, "ce partage ne contient que sa racine");
    }
    let path = match files::resolve(&share.root, rel) {
        Some(path) => path,
        None => return error(404, "dossier introuvable"),
    };
    // `?as=` : nom du dossier dans l'archive. Sans lui, le tar prend le nom
    // physique du dossier ; pour un partage entier (téléchargé sous le nom du
    // partage), le client impose ce nom-là. Un nom de traversée est refusé.
    let name = query_param(query, "as")
        .and_then(|raw| safe_archive_name(&raw))
        .or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| share.name.clone());
    let stream = match TarStream::new(&path, &name) {
        Ok(stream) => stream,
        Err(err) => return error(500, &format!("archive impossible : {err}")),
    };
    body_response(
        200,
        vec![
            h("Content-Type", "application/x-tar"),
            h("Content-Disposition", &disposition(&format!("{name}.tar"))),
            h("Cache-Control", "no-store"),
        ],
        Box::new(stream),
        None,
    )
}

fn help(cfg: &Config) -> Response<Body> {
    let mut text = String::new();
    text.push_str(&format!("Gablue Share — {} ({})\n\n", cfg.name, cfg.peer_id));
    text.push_str("Partages :\n");
    if cfg.shares.is_empty() {
        text.push_str("  (aucun)\n");
    }
    for share in &cfg.shares {
        // Dire qu'un partage est verrouillé évite de croire à une panne quand
        // `curl` reçoit un 401.
        let verrou = if share.locked() {
            "  (mot de passe requis)"
        } else {
            ""
        };
        text.push_str(&format!("  /{}/  {}{verrou}\n", share.id, share.name));
    }
    text.push_str(
        "\nRoutes :\n\
         \x20 GET /api/info                        état du poste et partages (avec l'uid qui le sert)\n\
         \x20 GET /api/auth/<partage>              vérifie le mot de passe (en-tête X-Gablue-Key)\n\
         \x20 GET /api/list/<partage>?path=&limit=  contenu d'un dossier (JSON)\n\
         \x20 GET /f/<partage>/<chemin>            fichier (Range/ETag)\n\
         \x20 GET /a/<partage>/<chemin>            dossier en tar\n\
         \nUn partage protégé exige l'en-tête X-Gablue-Key sur toutes les routes\n\
         qui livrent quelque chose : l'empreinte pbkdf2 du mot de passe avec le\n\
         sel publié par /api/info (le mot de passe lui-même ne circule jamais).\n",
    );
    text_response(200, text)
}

fn mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "txt" | "log" | "md" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "iso" | "img" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

/// Nom de fichier pour `curl -OJ` : repli ASCII + `filename*` UTF-8.
fn disposition(name: &str) -> String {
    let fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_ascii() {
        format!("attachment; filename=\"{fallback}\"")
    } else {
        format!(
            "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
            files::percent_encode(name)
        )
    }
}
