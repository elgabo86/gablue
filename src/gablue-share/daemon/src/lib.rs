//! Démon de partage Gablue : sert des dossiers en lecture seule sur le réseau
//! local et le tailnet, sans authentification. Aucun état client, aucune
//! écriture : il peut être tué à tout moment.

pub mod announce;
pub mod config;
pub mod files;
pub mod net;
pub mod routes;
pub mod tarstream;
pub mod tray;

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
    /// Lanceur de l'application, pour « Ouvrir Gablue Share » depuis l'icône
    /// de notification (le dépôt et l'image n'ont pas le même chemin).
    pub launcher: Option<String>,
    /// Icône de notification. Les tests la coupent : chaque serveur de test
    /// faisait apparaître une icône sur le bureau de l'utilisateur (vécu
    /// 15/11/2026, une vingtaine d'icônes d'un coup).
    pub tray: bool,
}

/// Démarre le serveur et bloque. `ready` reçoit l'adresse réellement liée
/// (utile pour le port 0 des tests).
pub fn run(
    config_path: PathBuf,
    options: Options,
    ready: Option<mpsc::Sender<SocketAddr>>,
) -> Result<(), String> {
    let initial = config::load(&config_path)?;
    // Aucun dossier déclaré : ni écoute, ni annonce, ni icône. Le serveur ne
    // démarre pas du tout (la GUI ne le lance déjà pas dans ce cas ; c'est le
    // filet pour un lancement direct ou une unité systemd restée activée).
    if initial.shares.is_empty() {
        eprintln!("gablue-shared : aucun partage déclaré, rien à servir");
        return Ok(());
    }
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
    // L'icône vit dans le serveur : elle apparaît avec lui et meurt avec lui.
    if options.tray {
        tray::spawn(Arc::clone(&state), options.launcher.clone());
    }

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
    // La visibilité est lue à chaque requête, pas au démarrage : passer de
    // « les deux » à « tailnet seulement » prend effet immédiatement, sans
    // redémarrer le démon ni couper un transfert.
    let snapshot = Arc::clone(&state.read().unwrap_or_else(|poison| poison.into_inner()));
    if let Some(ip) = request.remote_addr().map(SocketAddr::ip) {
        if !net::peer_allowed(ip, allow_nonlocal, snapshot.visibility) {
            eprintln!(
                "gablue-shared : refus de {ip} (visibilité « {:?} »)",
                snapshot.visibility
            );
            let _ = request.respond(routes::forbidden());
            return;
        }
    }
    let response = routes::handle(&request, &snapshot);
    if let Err(err) = request.respond(response) {
        eprintln!("gablue-shared : réponse interrompue ({err})");
    }
}

/// Surveille `share.json` **et** la présence des dossiers partagés.
///
/// - la GUI écrit le fichier → rechargement à chaud, sans redémarrer et sans
///   couper un transfert ;
/// - un disque externe monté ou débranché ne change pas le fichier : on re-vérifie
///   donc la disponibilité de chaque partage (un `stat` par partage, toutes les
///   3 s) et on recharge seulement si elle a bougé. Coût négligeable, et c'est ce
///   qui fait apparaître/disparaître un partage sans intervention.
fn spawn_reloader(path: PathBuf, state: Shared) {
    std::thread::spawn(move || {
        let mut last_mtime = mtime(&path);
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let now = mtime(&path);
            let file_changed = now != last_mtime;
            let availability_changed = {
                let snapshot = Arc::clone(&state.read().unwrap_or_else(|poison| poison.into_inner()));
                snapshot
                    .shares
                    .iter()
                    .any(|share| config::is_available(&share.path) != share.available)
            };
            if !file_changed && !availability_changed {
                continue;
            }
            last_mtime = now;
            match config::load(&path) {
                Ok(cfg) => {
                    let shares = cfg.available().count();
                    // Plus aucun dossier déclaré : le serveur n'a plus de raison
                    // d'être, ni icône à montrer. Il s'arrête — la GUI le
                    // relancera au premier dossier ajouté.
                    if cfg.shares.is_empty() {
                        eprintln!("gablue-shared : plus aucun partage déclaré, arrêt");
                        std::process::exit(0);
                    }
                    *state.write().unwrap_or_else(|poison| poison.into_inner()) = Arc::new(cfg);
                    eprintln!(
                        "gablue-shared : configuration rechargée ({shares} partage(s) disponible(s))"
                    );
                }
                Err(err) => eprintln!("gablue-shared : rechargement ignoré ({err})"),
            }
        }
    });
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|meta| meta.modified().ok())
}
