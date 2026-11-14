//! Démon de partage Gablue : sert des dossiers en lecture seule sur le réseau
//! local et le tailnet, sans authentification. Aucun état client, aucune
//! écriture : il peut être tué à tout moment.

pub mod announce;
pub mod config;
pub mod files;
pub mod net;
pub mod routes;
pub mod tarstream;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, RwLock};
use std::time::{Duration, SystemTime};

pub struct Options {
    /// Hôte d'écoute (`0.0.0.0` par défaut, `::` accepté).
    pub host: String,
    /// Port explicite ; sinon celui de la configuration.
    pub port: Option<u16>,
    pub threads: usize,
    /// Tests et dépannage uniquement : accepte n'importe quelle adresse.
    pub allow_nonlocal: bool,
}

/// Démarre le serveur et bloque. `ready` reçoit l'adresse réellement liée
/// (utile pour le port 0 des tests).
pub fn run(
    config_path: PathBuf,
    options: Options,
    ready: Option<mpsc::Sender<SocketAddr>>,
) -> Result<(), String> {
    let initial = config::load(&config_path)?;
    let state: Shared = Arc::new(RwLock::new(Arc::new(initial)));
    let port = options.port.unwrap_or(state.read().unwrap().port);
    let address = if options.host.contains(':') && !options.host.starts_with('[') {
        format!("[{}]:{port}", options.host)
    } else {
        format!("{}:{port}", options.host)
    };
    let server = Arc::new(
        tiny_http::Server::http(&address)
            .map_err(|err| format!("écoute impossible sur {address} : {err}"))?,
    );
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| "adresse d'écoute illisible".to_string())?;
    if let Some(tx) = ready {
        let _ = tx.send(bound);
    }
    eprintln!("gablue-shared : écoute sur {bound}");

    spawn_reloader(config_path, Arc::clone(&state));
    announce::spawn(Arc::clone(&state), bound.port());

    let workers = options.threads.max(1);
    for _ in 1..workers {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        let allow = options.allow_nonlocal;
        std::thread::spawn(move || worker(&server, &state, allow));
    }
    worker(&server, &state, options.allow_nonlocal);
    Ok(())
}

pub type Shared = Arc<RwLock<Arc<config::Config>>>;

fn worker(server: &tiny_http::Server, state: &Shared, allow_nonlocal: bool) {
    loop {
        match server.recv() {
            Ok(request) => serve(request, state, allow_nonlocal),
            Err(_) => return,
        }
    }
}

fn serve(request: tiny_http::Request, state: &Shared, allow_nonlocal: bool) {
    if let Some(ip) = request.remote_addr().map(SocketAddr::ip) {
        if !net::peer_allowed(ip, allow_nonlocal) {
            eprintln!("gablue-shared : refus de {ip} (hors réseau local et tailnet)");
            let _ = request.respond(routes::forbidden());
            return;
        }
    }
    let snapshot = Arc::clone(&state.read().unwrap_or_else(|poison| poison.into_inner()));
    let response = routes::handle(&request, &snapshot);
    if let Err(err) = request.respond(response) {
        eprintln!("gablue-shared : réponse interrompue ({err})");
    }
}

/// Surveille `share.json` : la GUI écrit, le démon recharge sans redémarrer,
/// donc sans couper un transfert en cours.
fn spawn_reloader(path: PathBuf, state: Shared) {
    std::thread::spawn(move || {
        let mut last = mtime(&path);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = mtime(&path);
            if now == last {
                continue;
            }
            last = now;
            match config::load(&path) {
                Ok(cfg) => {
                    *state.write().unwrap_or_else(|poison| poison.into_inner()) = Arc::new(cfg);
                    eprintln!("gablue-shared : configuration rechargée");
                }
                Err(err) => eprintln!("gablue-shared : rechargement ignoré ({err})"),
            }
        }
    });
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|meta| meta.modified().ok())
}
