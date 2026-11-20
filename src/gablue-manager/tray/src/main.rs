//! Icône de barre d'état de Gablue Manager — StatusNotifierItem par DBus.
//!
//! Un binaire de quelques mégaoctets, sans Qt : l'icône Qt du manager pesait
//! 111 Mo mesurés (~75 Mo pour QtWidgets seul, quel que soit le code). Ici,
//! tout ce qui demande de la logique — raisons de redémarrage, sondes de mises
//! à jour, phrases de l'infobulle, du menu et des notifications — est calculé
//! par un processus Python **éphémère** (`gablue-manager --tray-state`, voir
//! `gablue_manager/traystate.py`) qui rend du JSON. L'icône ne fait que
//! l'afficher, et reste donc sans règle métier.
//!
//! Trois cadences :
//! - **5 min** : état léger (aucune sonde réseau) — raisons de redémarrage et
//!   préférences relues, au cas où la fenêtre les a changées ;
//! - **intervalle des préférences** (2 h par défaut) : sonde des mises à jour ;
//! - **à la demande** : clic sur « Vérifier les mises à jour », ou message
//!   `updates` de la fenêtre (une mise à jour vient de se terminer).
//!
//! Rien n'est sondé hors ligne : l'état connu est conservé et l'infobulle le
//! dit. Le canal avec la fenêtre est la socket `gablue-manager-tray-<uid>`
//! (celle de `instance.py`, au même chemin : Qt la met dans le dossier
//! temporaire) : `quit`, `reload`, `updates`, `state:<json>`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{MenuItem, ToolTip, Tray as Icone};

const LIGHT_SECS: u64 = 300;      // état léger : raisons de redémarrage, préférences
const MIN_CHECK_SECS: u64 = 60;   // jamais deux sondes collées (fenêtre qui pousse un état)
const GRACE_SECS: u64 = 90;       // watcher absent depuis ce temps : on part

// --- chemins ---------------------------------------------------------------

/// Dossier temporaire de Qt : c'est là que `QLocalServer` met ses sockets pour
/// un nom relatif, et donc là que la fenêtre les cherche (`instance.py`).
fn temp_dir() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn tray_socket() -> PathBuf {
    temp_dir().join(format!("gablue-manager-tray-{}", uid()))
}

fn gui_socket() -> PathBuf {
    temp_dir().join(format!("gablue-manager-{}", uid()))
}

fn state_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"));
            home.join(".local/state")
        });
    base.join("gablue-manager/tray-state.json")
}

// --- état affiché ----------------------------------------------------------

/// Ce que l'icône affiche, tel que le processus Python le rend.
#[derive(Default, Clone)]
struct Display {
    alert: bool,
    alerts: Vec<String>,
    domains: Vec<String>,
    tooltip: Vec<String>,
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn strings(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn display_from(document: &serde_json::Value) -> Display {
    Display {
        alert: document.get("alert").and_then(|v| v.as_bool()).unwrap_or(false),
        alerts: strings(document, "alerts"),
        domains: strings(document, "domains"),
        tooltip: strings(document, "tooltip"),
    }
}

// --- enregistreur (gpad-rec, src/gablue-rec) -------------------------------

/// Socket de contrôle du démon d'enregistrement (même chemin que l'overlay).
fn rec_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(Path::new(&runtime).join("gablue/gpad-rec.sock"))
}

/// Une commande, une réponse (JSON sur une ligne). `None` : démon absent —
/// ce n'est pas une erreur, il ne tourne que pendant une capturé ou un replay.
fn rec_cmd(commande: &str) -> Option<String> {
    let mut stream = UnixStream::connect(rec_socket()?).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(400)))
        .ok()?;
    stream.write_all(commande.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut ligne = String::new();
    BufReader::new(stream).read_line(&mut ligne).ok()?;
    Some(ligne)
}

/// Enregistrement en cours ? La pastille rouge de l'icône en dépend.
fn rec_active() -> bool {
    rec_cmd("{\"cmd\":\"status\"}").is_some_and(|r| r.contains("\"recording\":true"))
}

/// Arrêt de l'enregistrement (clic sur l'icône). Le démon finalise le mp4
/// avant de répondre : au retour, le fichier est complet.
fn rec_stop() {
    rec_cmd("{\"cmd\":\"record.stop\"}");
}

// --- icône -----------------------------------------------------------------

struct Tray {
    /// enregistrement en cours : icône rouge, clic = arrêt
    recording: Arc<Mutex<bool>>,
    display: Arc<Mutex<Display>>,
    checking: Arc<Mutex<bool>>,
    manager: PathBuf,
    gui_socket: PathBuf,
    requests: Sender<Request>,
    started: Instant,
}

enum Request {
    Reload,
    Force,
    State(serde_json::Value),
}

impl Tray {
    fn toggle_manager(&self) {
        if tell_gui(&self.gui_socket, "toggle") {
            return;
        }
        spawn_manager(&self.manager);
    }

    fn open_manager(&self) {
        if tell_gui(&self.gui_socket, "raise") {
            return;
        }
        spawn_manager(&self.manager);
    }
}

impl Icone for Tray {
    fn id(&self) -> String {
        "gablue-manager".into()
    }

    fn title(&self) -> String {
        "Gablue Manager".into()
    }

    fn icon_name(&self) -> String {
        // l'enregistrement prime : c'est un état en cours, pas une nouvelle
        if self.recording.lock().map(|r| *r).unwrap_or(false) {
            return "gablue-record".into();
        }
        let alert = self.display.lock().map(|d| d.alert).unwrap_or(false);
        if alert {
            "gablue-manager-alert".into()
        } else {
            "gablue-manager".into()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let display = match self.display.lock() {
            Ok(display) => display.clone(),
            Err(poison) => poison.into_inner().clone(),
        };
        let recording = self.recording.lock().map(|r| *r).unwrap_or(false);
        let title = if recording {
            "Enregistrement en cours".to_string()
        } else {
            "Gablue Manager".to_string()
        };
        let mut description = if display.tooltip.len() > 1 {
            display.tooltip[1..].join("\n")
        } else {
            String::new()
        };
        if recording {
            description = if description.is_empty() {
                "Cliquer pour arrêter".to_string()
            } else {
                format!("Cliquer pour arrêter\n{description}")
            };
        }
        ToolTip {
            icon_name: if recording {
                "gablue-record".into()
            } else if display.alert {
                "gablue-manager-alert".into()
            } else {
                "gablue-manager".into()
            },
            title,
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // pendant un enregistrement, le clic l'arrête : c'est le geste utile
        // (même sans ouvrir la fenêtre), et le fichier est finalisé avant
        // que l'infobulle ne change.
        let recording = self.recording.lock().map(|r| *r).unwrap_or(false);
        if recording {
            rec_stop();
            if let Ok(mut guard) = self.recording.lock() {
                *guard = false;
            }
            return;
        }
        self.toggle_manager();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let checking = self.checking.lock().map(|value| *value).unwrap_or(false);
        let display = match self.display.lock() {
            Ok(display) => display.clone(),
            Err(poison) => poison.into_inner().clone(),
        };
        let recording = self.recording.lock().map(|r| *r).unwrap_or(false);
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if recording {
            items.push(
                StandardItem {
                    label: "Arrêter l'enregistrement".into(),
                    activate: Box::new(|tray: &mut Self| {
                        rec_stop();
                        if let Ok(mut guard) = tray.recording.lock() {
                            *guard = false;
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }
        items.push(StandardItem {
            label: "Ouvrir Gablue Manager".into(),
            activate: Box::new(|tray: &mut Self| tray.open_manager()),
            ..Default::default()
        }
        .into());

        // Sections : un intitulé non cliquable, puis une ligne par élément —
        // même lecture que le menu Qt (qui n'a pas d'en-tête de section en
        // DBusMenu, seulement des entrées inactives).
        if !display.alerts.is_empty() {
            items.push(MenuItem::Separator);
            items.push(
                StandardItem {
                    label: "Redémarrage requis".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
            for text in &display.alerts {
                items.push(
                    StandardItem {
                        label: text.clone(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }
        if !display.domains.is_empty() {
            items.push(MenuItem::Separator);
            items.push(
                StandardItem {
                    label: "Mises à jour".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
            for text in &display.domains {
                items.push(
                    StandardItem {
                        label: text.clone(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }

        items.push(MenuItem::Separator);
        let requests = self.requests.clone();
        items.push(
            StandardItem {
                label: if checking {
                    "Vérification des mises à jour…".into()
                } else {
                    "Vérifier les mises à jour".into()
                },
                enabled: !checking,
                activate: Box::new(move |_tray: &mut Self| {
                    let _ = requests.send(Request::Force);
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Quitter".into(),
                activate: Box::new(|_tray: &mut Self| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );
        items
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // Un bureau sans watcher (pas de barre d'état) laisserait sinon un
        // processus invisible à vie : on laisse un délai, puis on part.
        self.started.elapsed() < Duration::from_secs(GRACE_SECS)
    }
}

// --- canaux ----------------------------------------------------------------

/// Ordre à la fenêtre ; `false` si elle ne tourne pas.
fn tell_gui(path: &Path, message: &str) -> bool {
    match UnixStream::connect(path) {
        Ok(mut stream) => {
            let _ = stream.write_all(message.as_bytes());
            let _ = stream.flush();
            true
        }
        Err(_) => false,
    }
}

fn spawn_manager(program: &Path) -> bool {
    let Ok(mut child) = Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    else {
        return false;
    };
    // Sans `wait`, chaque ouverture laissait un zombie sous l'icône.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    true
}

/// Connexion au canal de l'icône ; `None` si une autre instance le tient.
fn claim_socket(path: &Path) -> Option<UnixListener> {
    if let Ok(listener) = UnixListener::bind(path) {
        return Some(listener);
    }
    // Occupé : une icône vit-elle vraiment, ou la socket est-elle orpheline ?
    if UnixStream::connect(path).is_ok() {
        return None;
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path).ok()
}

fn listen(listener: UnixListener, requests: Sender<Request>) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = Vec::new();
            let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        buffer.extend_from_slice(&chunk[..count]);
                        if buffer.len() > (1 << 20) {
                            break;  // état invraisemblable : on ignore
                        }
                    }
                }
            }
            let message = String::from_utf8_lossy(&buffer).trim().to_string();
            if message == "quit" {
                std::process::exit(0);
            } else if message == "reload" {
                let _ = requests.send(Request::Reload);
            } else if message == "updates" {
                let _ = requests.send(Request::Force);
            } else if let Some(rest) = message.strip_prefix("state:") {
                if let Ok(state) = serde_json::from_str::<serde_json::Value>(rest) {
                    if state.get("domains").map(|d| d.is_object()).unwrap_or(false) {
                        let _ = requests.send(Request::State(state));
                    }
                }
            } else {
                // Message inconnu : comportement historique, on s'arrête.
                std::process::exit(0);
            }
        }
    });
}

// --- réseau ----------------------------------------------------------------

/// Une route par défaut existe-t-elle ? (lecture de `/proc/net/route`)
///
/// Remplace `QNetworkInformation` : on ne veut pas de Qt pour savoir si la
/// machine est en ligne, et une sonde réseau lancée hors ligne ne ramène rien.
fn online() -> bool {
    let Ok(table) = std::fs::read_to_string("/proc/net/route") else {
        return true;  // table illisible : ne pas bloquer les sondes
    };
    table.lines().skip(1).any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.len() > 3
            && fields[1] == "00000000"
            && u16::from_str_radix(fields[3], 16).map(|flags| flags & 0x1 != 0).unwrap_or(false)
    })
}

// --- processus Python ------------------------------------------------------

fn run_state(manager: &Path, state: Option<PathBuf>, check: bool, force: bool,
             offline: bool) -> Option<serde_json::Value> {
    let mut command = Command::new(manager);
    command.arg("--tray-state");
    if let Some(path) = state {
        command.arg("--state").arg(path);
    }
    if force {
        command.arg("--force");
    } else if check {
        command.arg("--check");
    }
    if offline {
        command.arg("--offline");
    }
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

fn write_state(path: &Path, state: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("json.part");
    match serde_json::to_vec(state) {
        Ok(bytes) => {
            if std::fs::write(&temporary, bytes).is_ok() {
                let _ = std::fs::rename(&temporary, path);
            }
        }
        Err(_) => {}
    }
}

fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["--app-name=Gablue Manager", "--icon=gablue-manager", title, body])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

// --- boucle ----------------------------------------------------------------

struct Options {
    manager: PathBuf,
    state: PathBuf,
}

fn refresh(handle: &Handle<Tray>, document: &serde_json::Value) {
    let display = display_from(document);
    let checking = false;
    handle.update(move |tray| {
        if let Ok(mut guard) = tray.display.lock() {
            *guard = display;
        }
        if let Ok(mut guard) = tray.checking.lock() {
            *guard = checking;
        }
        true
    });
}

fn schedule(
    handle: Arc<Handle<Tray>>,
    options: Options,
    receiver: std::sync::mpsc::Receiver<Request>,
    recording: Arc<Mutex<bool>>,
) {
    let mut next_light = Instant::now();
    let mut next_check: Option<Instant> = Some(Instant::now());
    let mut forced = false;

    loop {
        let now = Instant::now();
        let mut deadline = next_light;
        if let Some(check) = next_check {
            deadline = deadline.min(check);
        }
        let wait = deadline.saturating_duration_since(now).min(Duration::from_secs(1));
        match receiver.recv_timeout(wait) {
            Ok(Request::Reload) => {
                // Préférences changées : tout relire, et re-sonder (la
                // fréquence ou les domaines ont pu changer).
                next_light = now;
                next_check = Some(now);
            }
            Ok(Request::Force) => {
                forced = true;
                next_check = Some(now);
            }
            Ok(Request::State(state)) => {
                // La fenêtre vient de conclure : on range son état et on
                // réaffiche sans refaire les sondes (elle les a faites).
                write_state(&options.state, &state);
                next_light = now;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        // Enregistrement : interrogé à chaque tour (≤ 1 s). La socket répond
        // en quelques dixièmes de milliseconde quand le démon tourne, et
        // échoue immédiatement quand il est absent — pas de sonde coûteuse.
        let rec = rec_active();
        let rec_change = recording
            .lock()
            .map(|mut guard| {
                let change = *guard != rec;
                *guard = rec;
                change
            })
            .unwrap_or(false);
        if rec_change {
            handle.update(|_tray| true);
        }

        if next_check.is_some_and(|check| check <= Instant::now()) {
            let check_now = Instant::now();
            let reachable = online();
            // On affiche tout de suite « vérification en cours » : les sondes
            // peuvent prendre une dizaine de secondes (registre, brew, gwine).
            handle.update(|tray| {
                if let Ok(mut guard) = tray.checking.lock() {
                    *guard = reachable;
                }
                true
            });
            let document = run_state(
                &options.manager,
                Some(options.state.clone()),
                true,
                forced,
                !reachable,
            );
            let mut interval = 0u64;
            if let Some(document) = &document {
                if let Some(state) = document.get("state") {
                    if state.get("domains").is_some() {
                        write_state(&options.state, state);
                    }
                }
                interval = document.get("interval").and_then(|v| v.as_u64()).unwrap_or(0);
                refresh(&handle, document);
                if let Some(notification) = document.get("notification") {
                    if let (Some(title), Some(body)) = (
                        notification.get("title").and_then(|v| v.as_str()),
                        notification.get("body").and_then(|v| v.as_str()),
                    ) {
                        notify(title, body);
                    }
                }
                if document.get("tray_enabled").and_then(|v| v.as_bool()) == Some(false) {
                    std::process::exit(0);
                }
            }
            forced = false;
            next_check = if interval == 0 {
                None
            } else {
                Some(check_now + Duration::from_secs(interval.max(MIN_CHECK_SECS)))
            };
            next_light = check_now + Duration::from_secs(LIGHT_SECS);
            continue;
        }

        if next_light <= Instant::now() {
            let light_now = Instant::now();
            let document = run_state(
                &options.manager,
                Some(options.state.clone()),
                false,
                false,
                !online(),
            );
            if let Some(document) = &document {
                refresh(&handle, document);
                if document.get("tray_enabled").and_then(|v| v.as_bool()) == Some(false) {
                    std::process::exit(0);
                }
                // Cadence des sondes : celle des préférences, décalée de la
                // dernière sonde connue (la fenêtre a pu la faire pour nous).
                let interval = document.get("interval").and_then(|v| v.as_u64()).unwrap_or(0);
                let checked_at = document.get("checked_at").and_then(|v| v.as_f64()).unwrap_or(0.0);
                next_check = if interval == 0 {
                    None
                } else if checked_at > 0.0 {
                    let elapsed = (now_epoch() - checked_at).max(0.0) as u64;
                    let remaining = interval.saturating_sub(elapsed).max(MIN_CHECK_SECS);
                    Some(light_now + Duration::from_secs(remaining))
                } else {
                    Some(light_now)
                };
            }
            next_light = light_now + Duration::from_secs(LIGHT_SECS);
        }
    }
}

fn usage() {
    eprintln!(
        "gablue-manager-tray [options]\n\
         \n\
           --manager CHEMIN      lanceur de Gablue Manager (défaut /usr/bin/gablue-manager)\n\
           --state FICHIER       état partagé avec la fenêtre (défaut $XDG_STATE_HOME/gablue-manager/tray-state.json)\n\
           --socket CHEMIN       canal de l'icône (tests et dépannage)\n\
           --gui-socket CHEMIN   canal de la fenêtre (tests et dépannage)\n\
         \n\
         Icône de barre d'état seule : un clic bascule la fenêtre, une pastille\n\
         rouge signale un redémarrage requis ou une mise à jour disponible."
    );
}

fn main() {
    let mut manager = PathBuf::from("/usr/bin/gablue-manager");
    let mut state = state_path();
    let mut socket = tray_socket();
    let mut gui_socket = gui_socket();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--manager" if index + 1 < arguments.len() => {
                manager = PathBuf::from(&arguments[index + 1]);
                index += 2;
            }
            "--state" if index + 1 < arguments.len() => {
                state = PathBuf::from(&arguments[index + 1]);
                index += 2;
            }
            // Deux chemins de canal, pour tester l'icône en headless sans
            // toucher à la session de l'utilisateur.
            "--socket" if index + 1 < arguments.len() => {
                socket = PathBuf::from(&arguments[index + 1]);
                index += 2;
            }
            "--gui-socket" if index + 1 < arguments.len() => {
                gui_socket = PathBuf::from(&arguments[index + 1]);
                index += 2;
            }
            "-h" | "--help" => {
                usage();
                return;
            }
            other => {
                eprintln!("gablue-manager-tray : option inconnue « {other} »");
                usage();
                std::process::exit(2);
            }
        }
    }

    let Some(listener) = claim_socket(&socket) else {
        return;  // une icône tourne déjà dans cette session
    };
    let (requests, receiver) = channel::<Request>();
    listen(listener, requests.clone());

    let display = Arc::new(Mutex::new(Display::default()));
    let checking = Arc::new(Mutex::new(false));
    let recording = Arc::new(Mutex::new(false));
    let trayeur = Tray {
        recording: Arc::clone(&recording),
        display: Arc::clone(&display),
        checking,
        manager: manager.clone(),
        gui_socket,
        requests: requests.clone(),
        started: Instant::now(),
    };
    let handle = match trayeur.assume_sni_available(true).spawn() {
        Ok(handle) => Arc::new(handle),
        Err(error) => {
            eprintln!("gablue-manager-tray : icône indisponible ({error})");
            let _ = std::fs::remove_file(&socket);
            std::process::exit(0);
        }
    };

    // Premier affichage tout de suite (état léger), puis sondes : l'icône
    // apparaît avant le réseau, qui peut prendre une dizaine de secondes.
    let options = Options { manager, state };
    schedule(Arc::clone(&handle), options, receiver, recording);
    let _ = handle.shutdown().wait();
}
