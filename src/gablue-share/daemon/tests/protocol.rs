//! Tests d'intégration du protocole : vrai serveur sur 127.0.0.1:0, vraies
//! requêtes TCP, vrai système de fichiers.

use gablue_shared::{run, Options};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    addr: SocketAddr,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> Fixture {
    fixture_with(false)
}

/// `detached` : ajoute un second partage dont le dossier n'existe pas encore,
/// comme un disque externe débranché.
fn fixture_with(detached: bool) -> Fixture {
    let index = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "gablue-share-test-{}-{index}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("donnees/sub")).unwrap();
    std::fs::write(root.join("donnees/a.txt"), b"hello").unwrap();
    std::fs::write(root.join("donnees/sub/b.bin"), vec![7u8; 700]).unwrap();
    std::fs::write(root.join("donnees/gros.bin"), vec![3u8; 64 * 1024]).unwrap();
    std::fs::create_dir_all(root.join("donnees/vide")).unwrap();
    std::fs::create_dir_all(root.join("donnees/avec espace")).unwrap();
    std::fs::write(root.join("donnees/avec espace/f.txt"), b"espace").unwrap();
    std::fs::write(root.join("donnees/a+b.txt"), b"plus").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join("donnees/fuite")).unwrap();
    std::os::unix::fs::symlink("a.txt", root.join("donnees/lien-interne")).unwrap();

    let config = root.join("share.json");
    let mut shares = format!(
        r#"{{"id":"s1","name":"Données","path":"{}"}}"#,
        root.join("donnees").display()
    );
    if detached {
        shares.push_str(&format!(
            r#",{{"id":"s2","name":"Disque amovible","path":"{}"}}"#,
            root.join("amovible").display()
        ));
    }
    std::fs::write(
        &config,
        format!(
            r#"{{"version":1,"peer_id":"test-peer","name":"poste-test","port":0,
               "shares":[{shares}]}}"#
        ),
    )
    .unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let options = Options {
        host: "127.0.0.1".to_string(),
        port: Some(0),
        threads: 2,
        allow_nonlocal: false,
    };
    std::thread::spawn(move || {
        let _ = run(config, options, Some(tx));
    });
    let addr = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("le démon n'a pas démarré");
    Fixture { root, addr }
}

fn raw(fixture: &Fixture, request: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(fixture.addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut data = Vec::new();
    stream.read_to_end(&mut data).unwrap();
    let split = data
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("réponse sans en-têtes complets");
    let head = String::from_utf8_lossy(&data[..split]).to_string();
    let body = data[split + 4..].to_vec();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("ligne de statut illisible");
    (status, head, body)
}

fn get(fixture: &Fixture, path: &str) -> (u16, String, Vec<u8>) {
    raw(
        fixture,
        &format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"),
    )
}

#[test]
fn info_lists_the_peer_and_its_shares() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/api/info");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"peer_id\":\"test-peer\""), "{text}");
    assert!(text.contains("\"id\":\"s1\""), "{text}");
    assert!(text.contains("Données"), "{text}");
}

#[test]
fn list_root_and_subdirectory() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/api/list/s1");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("a.txt"), "{text}");
    assert!(text.contains("\"sub\""), "{text}");
    assert!(text.contains("\"truncated\":false"), "{text}");

    let (status, _, body) = get(&fixture, "/api/list/s1?path=sub");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("b.bin"), "{text}");
    assert!(text.contains("\"parent\":null"), "{text}");
}

#[test]
fn list_hides_the_escaping_symlink_but_keeps_the_internal_one() {
    let fixture = fixture();
    let (_, _, body) = get(&fixture, "/api/list/s1");
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains("fuite"), "{text}");
    assert!(text.contains("lien-interne"), "{text}");
}

#[test]
fn list_truncates_on_limit() {
    let fixture = fixture();
    let (_, _, body) = get(&fixture, "/api/list/s1?limit=1");
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"truncated\":true"), "{text}");
}

#[test]
fn file_is_served_with_metadata() {
    let fixture = fixture();
    let (status, head, body) = get(&fixture, "/f/s1/a.txt");
    assert_eq!(status, 200);
    assert_eq!(body, b"hello");
    assert!(head.to_lowercase().contains("etag"), "{head}");
    assert!(head.contains("Accept-Ranges: bytes"), "{head}");
    assert!(
        head.contains("Content-Disposition: attachment; filename=\"a.txt\""),
        "{head}"
    );
}

#[test]
fn internal_symlink_is_followed() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/f/s1/lien-interne");
    assert_eq!(status, 200);
    assert_eq!(body, b"hello");
}

#[test]
fn range_requests() {
    let fixture = fixture();
    let head_request = |range: &str| {
        raw(
            &fixture,
            &format!(
                "GET /f/s1/a.txt HTTP/1.1\r\nHost: t\r\nRange: {range}\r\nConnection: close\r\n\r\n"
            ),
        )
    };

    let (status, head, body) = head_request("bytes=1-3");
    assert_eq!(status, 206, "{head}");
    assert_eq!(body, b"ell");
    assert!(head.contains("Content-Range: bytes 1-3/5"), "{head}");

    let (status, _, body) = head_request("bytes=-2");
    assert_eq!(status, 206);
    assert_eq!(body, b"lo");

    let (status, head, body) = head_request("bytes=10-");
    assert_eq!(status, 416, "{head}");
    assert!(head.contains("Content-Range: bytes */5"), "{head}");
    assert!(body.is_empty());

    let (status, _, _) = head_request("bytes=0-1,3-4");
    assert_eq!(status, 200, "plusieurs plages = fichier entier");
}

#[test]
fn if_range_with_another_etag_sends_the_whole_file() {
    let fixture = fixture();
    let (_, head, _) = get(&fixture, "/f/s1/a.txt");
    let etag = head
        .lines()
        .find(|line| line.to_lowercase().starts_with("etag:"))
        .map(|line| line[5..].trim().to_string())
        .expect("ETag absente");
    let (status, _, body) = raw(
        &fixture,
        &format!(
            "GET /f/s1/a.txt HTTP/1.1\r\nHost: t\r\nRange: bytes=0-1\r\nIf-Range: \"perime\"\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(status, 200);
    assert_eq!(body, b"hello");

    let (status, _, body) = raw(
        &fixture,
        &format!(
            "GET /f/s1/a.txt HTTP/1.1\r\nHost: t\r\nRange: bytes=0-1\r\nIf-Range: {etag}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(status, 206);
    assert_eq!(body, b"he");
}

#[test]
fn large_files_declare_their_length_instead_of_chunking() {
    let fixture = fixture();
    let (status, head, body) = get(&fixture, "/f/s1/gros.bin");
    assert_eq!(status, 200);
    assert!(head.contains("Content-Length: 65536"), "{head}");
    assert!(!head.to_lowercase().contains("transfer-encoding"), "{head}");
    assert_eq!(body.len(), 65536);

    let (status, head, body) = raw(
        &fixture,
        "HEAD /f/s1/gros.bin HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(status, 200);
    assert!(head.contains("Content-Length: 65536"), "{head}");
    assert!(body.is_empty());
}

#[test]
fn head_sends_the_length_without_a_body() {
    let fixture = fixture();
    let (status, head, body) = raw(
        &fixture,
        "HEAD /f/s1/a.txt HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(status, 200);
    assert!(head.contains("Content-Length: 5"), "{head}");
    assert!(body.is_empty());
}

#[test]
fn path_traversal_and_outside_symlinks_are_refused() {
    let fixture = fixture();
    for path in [
        "/f/s1/../share.json",
        "/f/s1/%2e%2e/share.json",
        "/f/s1/..%2Fshare.json",
        "/f/s1/fuite",
        "/api/list/s1?path=../",
    ] {
        let (status, _, _) = get(&fixture, path);
        assert!(
            status == 400 || status == 404,
            "{path} devrait être refusé, reçu {status}"
        );
    }
}

#[test]
fn list_answers_a_conditional_head_so_the_gui_can_poll_cheaply() {
    let fixture = fixture();
    let (status, head, _) = get(&fixture, "/api/list/s1");
    assert_eq!(status, 200);
    let tag = head
        .lines()
        .find(|line| line.to_lowercase().starts_with("etag:"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_else(|| panic!("pas d'ETag dans :\n{head}"));

    // Rien n'a bougé : 304, corps vide.
    let (status, head, body) = raw(
        &fixture,
        &format!(
            "HEAD /api/list/s1 HTTP/1.1\r\nHost: t\r\nIf-None-Match: {tag}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(status, 304, "{head}");
    assert!(body.is_empty());

    // Un fichier apparaît : un GET sans condition donne une autre empreinte.
    std::fs::write(fixture.root.join("donnees/nouveau.txt"), b"x").unwrap();
    let (status, head, _) = raw(
        &fixture,
        &format!(
            "HEAD /api/list/s1 HTTP/1.1\r\nHost: t\r\nIf-None-Match: {tag}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(status, 200, "l'ancienne empreinte devait être périmée :\n{head}");
    assert!(!head.contains(&tag), "l'ETag n'a pas changé :\n{head}");
}

#[test]
fn detached_share_is_hidden_and_refused() {
    let fixture = fixture_with(true);
    let (status, _, body) = get(&fixture, "/api/info");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"id\":\"s1\""), "{text}");
    assert!(
        !text.contains("\"id\":\"s2\""),
        "un partage absent ne doit pas être annoncé : {text}"
    );

    let (status, _, body) = get(&fixture, "/api/list/s2");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 404, "{text}");
    assert!(text.contains("indisponible"), "{text}");
}

#[test]
fn detached_share_comes_back_when_its_folder_reappears() {
    let fixture = fixture_with(true);
    let before = String::from_utf8_lossy(&get(&fixture, "/api/info").2).to_string();
    assert!(!before.contains("\"id\":\"s2\""), "{before}");

    // Le disque est remonté : personne ne touche à share.json.
    std::fs::create_dir_all(fixture.root.join("amovible")).unwrap();
    std::fs::write(fixture.root.join("amovible/photos.bin"), vec![9u8; 32]).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    loop {
        let text = String::from_utf8_lossy(&get(&fixture, "/api/info").2).to_string();
        if text.contains("\"id\":\"s2\"") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "le partage n'est jamais revenu : {text}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    let (status, _, body) = get(&fixture, "/api/list/s2");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("photos.bin"));
}

#[test]
fn archive_prefix_can_be_renamed_but_traversal_names_are_ignored() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/a/s1?as=Photos");
    assert_eq!(status, 200);
    let head = String::from_utf8_lossy(&body[..512.min(body.len())]).to_string();
    assert!(head.contains("Photos/"), "{head}");

    // Nom de traversée : refusé, on retombe sur le nom physique du dossier.
    let (status, _, body) = get(&fixture, "/a/s1?as=..%2Fevade");
    assert_eq!(status, 200);
    let head = String::from_utf8_lossy(&body[..512.min(body.len())]).to_string();
    assert!(head.contains("donnees/"), "{head}");
    assert!(!head.contains("evade"), "{head}");
}

#[test]
fn recursive_search_finds_by_name_and_never_leaves_the_share() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/api/find/s1?q=B."); 
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"path\":\"sub/b.bin\""), "{text}");
    assert!(text.contains("\"scanned\":"), "{text}");

    // Insensible à la casse, et le symlink sortant (/etc/passwd) reste invisible.
    let (_, _, body) = get(&fixture, "/api/find/s1?q=PASSWD");
    assert!(!String::from_utf8_lossy(&body).contains("fuite"));

    let (status, _, body) = get(&fixture, "/api/find/s1?q=");
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
}

#[test]
fn folder_size_is_bounded_and_announces_its_completeness() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/api/size/s1?path=sub");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"files\":1"), "{text}");
    assert!(text.contains("\"bytes\":700"), "{text}");
    assert!(text.contains("\"complete\":true"), "{text}");
}

#[test]
fn listing_can_be_sorted_and_paginated_by_the_client() {
    let fixture = fixture();
    // Les dossiers restent en tête quelle que soit la clé demandée.
    let (_, _, body) = get(&fixture, "/api/list/s1?sort=size&desc=1&limit=50");
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let entries = payload["entries"].as_array().unwrap();
    assert_eq!(entries[0]["type"], "dir", "{payload}");
    let files: Vec<&str> = entries
        .iter()
        .filter(|entry| entry["type"] == "file")
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(files.first().copied(), Some("gros.bin"), "plus gros d'abord : {files:?}");

    // La pagination par offset rend la suite sans recalculer depuis zéro.
    let (_, _, page1) = get(&fixture, "/api/list/s1?limit=2");
    let (_, _, page2) = get(&fixture, "/api/list/s1?limit=2&offset=2");
    let first: serde_json::Value = serde_json::from_slice(&page1).unwrap();
    let second: serde_json::Value = serde_json::from_slice(&page2).unwrap();
    assert_ne!(first["entries"][0]["name"], second["entries"][0]["name"]);
    assert_eq!(first["truncated"], true);
}

#[test]
fn spaces_and_plus_signs_survive_the_query_encoding() {
    let fixture = fixture();
    // `+` dans une valeur de requête vaut une espace (encodage de formulaire).
    let (status, _, body) = get(&fixture, "/api/list/s1?path=avec+espace");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("f.txt"), "{text}");

    // Un `+` littéral dans un nom de fichier arrive en %2B et reste un `+`.
    let (status, _, body) = get(&fixture, "/f/s1/a%2Bb.txt");
    assert_eq!(status, 200);
    assert_eq!(body, b"plus");

    // Le dossier qui contient une espace se visite aussi.
    let (status, _, body) = get(&fixture, "/api/list/s1?path=avec%20espace");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("f.txt"));
}

#[test]
fn unknown_share_and_unknown_route() {
    let fixture = fixture();
    assert_eq!(get(&fixture, "/api/list/inconnu").0, 404);
    assert_eq!(get(&fixture, "/f/inconnu/x").0, 404);
    assert_eq!(get(&fixture, "/nimporte").0, 404);
}

#[test]
fn directory_download_is_refused_as_a_file() {
    let fixture = fixture();
    let (status, _, _) = get(&fixture, "/f/s1/sub");
    assert_eq!(status, 409);
}

#[test]
fn tar_stream_contains_the_tree() {
    let fixture = fixture();
    let (status, head, body) = get(&fixture, "/a/s1/sub");
    assert_eq!(status, 200, "{head}");
    assert!(head.contains("application/x-tar"), "{head}");
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("sub/b.bin"), "archive sans b.bin");
}

#[test]
fn only_get_and_head_are_accepted() {
    let fixture = fixture();
    let (status, _, _) = raw(
        &fixture,
        "POST /api/info HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(status, 405);
}

#[test]
fn root_page_helps_curl_users() {
    let fixture = fixture();
    let (status, _, body) = get(&fixture, "/");
    let text = String::from_utf8_lossy(&body);
    assert_eq!(status, 200);
    assert!(text.contains("/api/info"), "{text}");
    assert!(text.contains("Données"), "{text}");
}
