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

fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some(parts) => parts,
            None => (pair, ""),
        };
        if name == key {
            return files::percent_decode(value);
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
            Some("list") if segments.len() == 3 => api_list(cfg, &segments[2], query),
            _ => error(404, "route inconnue"),
        },
        Some("f") if segments.len() >= 3 => {
            serve_file(req, cfg, &segments[1], &segments[2..], head)
        }
        Some("a") if segments.len() >= 2 => serve_tar(cfg, &segments[1], &segments[2..]),
        _ => error(404, "route inconnue"),
    }
}

fn share_or_404<'a>(cfg: &'a Config, id: &str) -> Result<&'a Share, Response<Body>> {
    cfg.share(id)
        .ok_or_else(|| error(404, "partage inconnu"))
}

fn api_info(cfg: &Config) -> Response<Body> {
    let shares: Vec<Value> = cfg
        .shares
        .iter()
        .map(|share| json!({ "id": share.id, "name": share.name }))
        .collect();
    json_response(
        200,
        &json!({
            "protocol": 1,
            "peer_id": cfg.peer_id,
            "name": cfg.name,
            "port": cfg.port,
            "shares": shares,
        }),
    )
}

fn api_list(cfg: &Config, share_id: &str, query: &str) -> Response<Body> {
    let share = match share_or_404(cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    let relative = query_param(query, "path").unwrap_or_default();
    let segments = match files::split_path(&relative) {
        Some(segments) => segments,
        None => return error(400, "chemin invalide"),
    };
    let dir = match files::resolve(&share.root, &segments) {
        Some(path) => path,
        None => return error(404, "dossier introuvable"),
    };
    if !dir.is_dir() {
        return error(404, "ce n'est pas un dossier");
    }
    let offset = query_param(query, "offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let limit = query_param(query, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(LIST_DEFAULT)
        .clamp(1, LIST_MAX);

    let (entries, truncated) = match files::list(&share.root, &dir, offset, limit) {
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
    json_response(
        200,
        &json!({
            "share": share.id,
            "path": segments.join("/"),
            "parent": parent,
            "entries": entries,
            "truncated": truncated,
        }),
    )
}

fn serve_file(req: &Request, cfg: &Config, share_id: &str, rel: &[String], head: bool) -> Response<Body> {
    let share = match share_or_404(cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
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

fn serve_tar(cfg: &Config, share_id: &str, rel: &[String]) -> Response<Body> {
    let share = match share_or_404(cfg, share_id) {
        Ok(share) => share,
        Err(response) => return response,
    };
    let path = match files::resolve(&share.root, rel) {
        Some(path) => path,
        None => return error(404, "dossier introuvable"),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
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
        text.push_str(&format!("  /{}/  {}\n", share.id, share.name));
    }
    text.push_str(
        "\nRoutes :\n\
         \x20 GET /api/info                        état du poste et partages\n\
         \x20 GET /api/list/<partage>?path=&limit=  contenu d'un dossier (JSON)\n\
         \x20 GET /f/<partage>/<chemin>            fichier (Range/ETag)\n\
         \x20 GET /a/<partage>/<chemin>            dossier en tar\n",
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
