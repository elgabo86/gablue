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
    let index = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "gablue-share-test-{}-{index}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("donnees/sub")).unwrap();
    std::fs::write(root.join("donnees/a.txt"), b"hello").unwrap();
    std::fs::write(root.join("donnees/sub/b.bin"), vec![7u8; 700]).unwrap();
    std::fs::create_dir_all(root.join("donnees/vide")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join("donnees/fuite")).unwrap();
    std::os::unix::fs::symlink("a.txt", root.join("donnees/lien-interne")).unwrap();

    let config = root.join("share.json");
    std::fs::write(
        &config,
        format!(
            r#"{{"version":1,"peer_id":"test-peer","name":"poste-test","port":0,
               "shares":[{{"id":"s1","name":"Données","path":"{}"}}]}}"#,
            root.join("donnees").display()
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
