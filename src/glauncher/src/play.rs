//! Lecture plein écran d'un enregistrement ou d'un replay.
//!
//! La vidéo est confiée à `mpv` (déjà utilisé pour la musique de fond) : iced
//! n'a aucun décodeur vidéo, et embarquer une fenêtre étrangère sous Wayland
//! n'est pas possible. `mpv` s'ouvre en plein écran, et le launcher le pilote
//! par sa **socket IPC** : pause, avance/recul, sortie. Les commandes du pad
//! continuent d'arriver au launcher (lecture evdev directe), même quand la
//! fenêtre de `mpv` a le focus.
//!
//! Le processus reçoit `PR_SET_PDEATHSIG` : un launcher tué ne laisse pas un
//! film qui continue seul. La surveillance de fin se fait par `kill(pid, 0)`
//! (le processus n'appartient plus vraiment au launcher), d'où l'absence de
//! `wait()` bloquant sur le fil de l'interface.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Pas d'avance/recul d'un appui sur gauche/droite (secondes).
pub const PAS: f32 = 5.0;

pub struct Lecteur {
    child: Child,
    stream: Option<UnixStream>,
    /// Instant de la demande de sortie : au-delà de `GRACE`, on tue. Une
    /// fenêtre qui refuse de se fermer ne doit pas garder le launcher bloqué
    /// hors de sa galerie.
    quitte_a: Option<Instant>,
}

const GRACE: Duration = Duration::from_secs(3);

impl Lecteur {
    /// Lance `mpv` en plein écran sur ce fichier. `None` = binaire absent ou
    /// socket muette (le launcher le dit, il ne fait pas semblant).
    pub fn lance(fichier: &Path) -> Option<Self> {
        // numéro monotone : deux lectures dans la même session (ou un banc)
        // ne doivent jamais partager le même chemin de socket
        static SOCKETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let numero = SOCKETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let socket = std::env::temp_dir().join(format!(
            "glauncher-film-{}-{numero}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&socket);
        let mut command = Command::new(mpv_binary());
        command
            // jamais la config du compte : la galerie doit se comporter pareil
            // partout, et un `input.conf` local pourrait avaler les touches
            .arg("--no-config")
            .arg("--no-terminal")
            .arg("--really-quiet")
            .arg("--fullscreen")
            .arg("--no-border")
            // la barre de progression de mpv revient au clavier/souris : ici
            // tout passe par la manette, donc pas d'OSC (mais l'OSD reste pour
            // afficher le temps après une avance)
            .arg("--osc=no")
            .arg("--osd-level=1")
            .arg("--osd-duration=800")
            .arg("--keep-open=no")
            .arg("--no-resume-playback")
            .arg(format!("--input-ipc-server={}", socket.display()))
            .arg("--title=glauncher")
            .arg(fichier)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("glauncher : lecture vidéo impossible ({err})");
                return None;
            }
        };
        // la socket apparaît ~30 ms après le lancement ; au-delà d'une seconde
        // mpv a échoué (périphérique vidéo absent)
        let stream = (0..50).find_map(|_| match UnixStream::connect(&socket) {
            Ok(stream) => Some(stream),
            Err(_) => {
                std::thread::sleep(Duration::from_millis(20));
                None
            }
        });
        let lecteur = Self {
            child,
            stream,
            quitte_a: None,
        };
        if lecteur.stream.is_none() {
            eprintln!("glauncher : mpv muet (socket IPC absente)");
        }
        Some(lecteur)
    }

    /// Le processus est-il terminé (récolté au passage, sans bloquer) ?
    pub fn fini(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Pause / reprise (`cycle pause`).
    pub fn pause(&mut self) {
        self.envoie("{\"command\":[\"cycle\",\"pause\"]}");
    }

    /// Avance (`> 0`) ou recul (`< 0`) relatif, en secondes.
    pub fn seek(&mut self, delta: f32) {
        let commande = format!("{{\"command\":[\"seek\",{delta:.1},\"relative\"]}}");
        self.envoie(&commande);
    }

    /// Demande la fermeture ; la surveillance tue le processus si `mpv` ne
    /// répond pas dans `GRACE`.
    pub fn quitter(&mut self) {
        self.quitte_a = Some(Instant::now());
        self.envoie("{\"command\":[\"quit\"]}");
    }

    /// Vrai quand le délai de grâce est dépassé : l'appelant peut alors tuer.
    pub fn en_retard(&self) -> bool {
        self.quitte_a
            .is_some_and(|quand| quand.elapsed() > GRACE)
    }

    pub fn tuer(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn envoie(&mut self, ligne: &str) {
        if let Some(stream) = self.stream.as_mut() {
            // aucune réponse n'est lue : ces commandes n'en attendent pas
            let _ = writeln!(stream, "{ligne}");
        }
    }
}

impl Drop for Lecteur {
    fn drop(&mut self) {
        // sortie de l'application (ou fin normale) : jamais de film orphelin
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn mpv_binary() -> String {
    std::env::var("GLAUNCHER_MPV").unwrap_or_else(|_| "mpv".to_string())
}
