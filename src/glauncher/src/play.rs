//! Lecture plein écran d'un enregistrement ou d'un replay.
//!
//! La vidéo est confiée à `mpv` (déjà utilisé pour la musique de fond) : iced
//! n'a aucun décodeur vidéo, et embarquer une fenêtre étrangère sous Wayland
//! n'est pas possible. `mpv` s'ouvre en plein écran, et le launcher le pilote
//! par sa **socket IPC** : pause, avance/recul, sortie. Les commandes du pad
//! continuent d'arriver au launcher (lecture evdev directe), même quand la
//! fenêtre de `mpv` a le focus.
//!
//! La **barre de progression** est dessinée par `osd-overlay` (ASS) et non par
//! l'OSD natif de `mpv` : capsule arrondie en bas de l'écran, remplissage à la
//! couleur d'accent, temps lisible au-dessus, effacement en fondu au bout de
//! quelques secondes. L'OSD natif (`--osd-level=0`) est coupé, sinon son
//! rectangle blanc et son texte en haut à gauche reviendraient par-dessus.
//!
//! Le processus reçoit `PR_SET_PDEATHSIG` : un launcher tué ne laisse pas un
//! film qui continue seul. La surveillance de fin se fait par `kill(pid, 0)`
//! (le processus n'appartient plus vraiment au launcher), d'où l'absence de
//! `wait()` bloquant sur le fil de l'interface.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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
    /// Demande d'affichage de la barre de progression. Un fil dédié lit
    /// `time-pos` sur sa **propre** connexion IPC : l'interface n'attend
    /// jamais une réponse de `mpv`.
    osd: Option<Sender<()>>,
}

const GRACE: Duration = Duration::from_secs(3);

/// Barre affichée à chaque interaction, puis effacée en fondu.
const OSD_HOLD: Duration = Duration::from_millis(2400);
const OSD_FADE_MS: u64 = 70;
/// Alphas de l'effacement (0 = opaque, 255 = invisible).
const OSD_FADE: [u8; 3] = [0x55, 0x99, 0xD0];

/// Espace de dessin ASS : 1920×1080. `mpv` le met à l'échelle de la fenêtre,
/// donc la barre garde les mêmes proportions en 1080p, 2K ou 4K.
const OSD_RES: (i64, i64) = (1920, 1080);
/// Identifiant de l'overlay (espace de noms de notre connexion IPC).
const OSD_ID: i64 = 0x676c;
/// Couleur d'accent du launcher (#4F99FF) en BGR, notation ASS.
const OSD_ACCENT: &str = "&HFF994F&";
/// Capsule : largeur, hauteur, rayon et bas (56 px au-dessus du bord).
const OSD_X0: f64 = 580.0;
const OSD_X1: f64 = 1340.0;
const OSD_Y0: f64 = 1010.0;
const OSD_Y1: f64 = 1024.0;
const OSD_RADIUS: f64 = 7.0;
/// Bas du texte de temps, au-dessus de la capsule ; taille de police.
const OSD_TEXT_Y: f64 = 994.0;
const OSD_FONT: f64 = 56.0;

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
            // pas d'OSC ni d'OSD natif : la barre est dessinée par
            // `osd-overlay` (voir `barre_ass`), tout passe par la manette
            .arg("--osc=no")
            .arg("--osd-level=0")
            // **fin de vidéo : on ne ferme pas**. Le dernier plan reste à
            // l'écran (image figée) et c'est Rond qui referme — sinon la
            // fenêtre disparaissait au moment précis où l'on regarde la fin,
            // et le retour au launcher surprenait.
            .arg("--keep-open=yes")
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
        let osd = stream.as_ref().and_then(|_| fil_osd(socket));
        let lecteur = Self {
            child,
            stream,
            quitte_a: None,
            osd,
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

    /// Pause / reprise (`cycle pause`), puis la barre de progression.
    pub fn pause(&mut self) {
        self.envoie("{\"command\":[\"cycle\",\"pause\"]}");
        self.montre_progression();
    }

    /// Avance (`> 0`) ou recul (`< 0`) relatif, en secondes.
    pub fn seek(&mut self, delta: f32) {
        let commande = format!("{{\"command\":[\"seek\",{delta:.1},\"relative\"]}}");
        self.envoie(&commande);
        self.montre_progression();
    }

    /// Demande la fermeture ; la surveillance tue le processus si `mpv` ne
    /// répond pas dans `GRACE`.
    pub fn quitter(&mut self) {
        self.quitte_a = Some(Instant::now());
        self.envoie("{\"command\":[\"quit\"]}");
        self.osd = None;
    }

    /// Vrai quand le délai de grâce est dépassé : l'appelant peut alors tuer.
    pub fn en_retard(&self) -> bool {
        self.quitte_a.is_some_and(|quand| quand.elapsed() > GRACE)
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

    fn montre_progression(&self) {
        if let Some(osd) = &self.osd {
            let _ = osd.send(());
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

/// Fil de la barre de progression : une connexion IPC à lui (les overlays
/// appartiennent à la connexion qui les a posés, il lui faut donc la sienne
/// pour pouvoir les remplacer et les effacer).
fn fil_osd(socket: PathBuf) -> Option<Sender<()>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("glauncher-osd".into())
        .spawn(move || boucle_osd(&socket, rx))
        .ok()
        .map(|_| tx)
}

fn boucle_osd(socket: &Path, rx: Receiver<()>) {
    let debug = std::env::var_os("GLAUNCHER_OSD_DEBUG").is_some();
    let Some(mut ecriture) = connexion(socket) else {
        return;
    };
    let Ok(copie) = ecriture.try_clone() else {
        return;
    };
    let mut lecture = BufReader::new(copie);
    let _ = lecture
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(150)));
    let mut requete = 0u64;
    loop {
        // attend une interaction (avance, recul, pause)
        if rx.recv().is_err() {
            return;
        }
        'barre: loop {
            let Some((pos, duree)) = position(&mut ecriture, &mut lecture, &mut requete) else {
                // mpv ne répond pas (fichier pas encore chargé) : rien à
                // dessiner, on attend la prochaine interaction
                break 'barre;
            };
            if debug {
                eprintln!("glauncher : barre {pos:.1} / {duree:.1} affichée");
            }
            let _ = ecriture.write_all(&overlay(pos, duree, 0));
            let fin = Instant::now() + OSD_HOLD;
            loop {
                let reste = fin.saturating_duration_since(Instant::now());
                if reste.is_zero() {
                    break;
                }
                match rx.recv_timeout(reste) {
                    // nouvelle interaction : la barre repart, sans passer par
                    // un effacement qu'on ne verrait pas
                    Ok(()) => continue 'barre,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            // effacement en fondu
            for alpha in OSD_FADE {
                let _ = ecriture.write_all(&overlay(pos, duree, alpha));
                std::thread::sleep(Duration::from_millis(OSD_FADE_MS));
            }
            let _ = ecriture.write_all(&efface());
            if debug {
                eprintln!("glauncher : barre effacée");
            }
            break;
        }
    }
}

/// Connexion à la socket IPC de `mpv` (elle apparaît quelques dizaines de ms
/// après le lancement).
fn connexion(socket: &Path) -> Option<UnixStream> {
    (0..50).find_map(|_| match UnixStream::connect(socket) {
        Ok(stream) => Some(stream),
        Err(_) => {
            std::thread::sleep(Duration::from_millis(20));
            None
        }
    })
}

/// Position et durée lues à `mpv` (`time-pos`, `duration`). Les réponses aux
/// commandes précédentes (overlay, effacement) sont ignorées : chaque demande
/// porte son propre `request_id`.
fn position(
    ecriture: &mut UnixStream,
    lecture: &mut BufReader<UnixStream>,
    requete: &mut u64,
) -> Option<(f64, f64)> {
    let (id_pos, id_duree) = (*requete + 1, *requete + 2);
    *requete += 2;
    for (id, propriete) in [(id_pos, "time-pos"), (id_duree, "duration")] {
        let commande = serde_json::json!({
            "command": ["get_property", propriete],
            "request_id": id,
        });
        ecriture
            .write_all(format!("{commande}\n").as_bytes())
            .ok()?;
    }
    let (mut pos, mut duree) = (None, None);
    let (mut vu_pos, mut vu_duree) = (false, false);
    while !vu_pos || !vu_duree {
        let mut ligne = String::new();
        match lecture.read_line(&mut ligne) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        let Ok(valeur) = serde_json::from_str::<serde_json::Value>(ligne.trim()) else {
            continue;
        };
        let Some(id) = valeur.get("request_id").and_then(|id| id.as_u64()) else {
            continue;
        };
        if id == id_pos {
            vu_pos = true;
            pos = valeur.get("data").and_then(|valeur| valeur.as_f64());
        } else if id == id_duree {
            vu_duree = true;
            duree = valeur.get("data").and_then(|valeur| valeur.as_f64());
        }
    }
    match (pos, duree) {
        (Some(pos), Some(duree)) if duree.is_finite() && duree > 0.0 => Some((pos.max(0.0), duree)),
        // `null` = fichier pas encore chargé : inutile de dessiner une barre
        // à zéro
        _ => None,
    }
}

/// Ligne de commande `osd-overlay` complète (avec son saut de ligne), prête à
/// être écrite sur la socket.
fn overlay(pos: f64, duree: f64, alpha: u8) -> Vec<u8> {
    let commande = serde_json::json!({
        "command": [
            "osd-overlay",
            OSD_ID,
            "ass-events",
            barre_ass(pos, duree, alpha),
            OSD_RES.0,
            OSD_RES.1,
        ],
    });
    format!("{commande}\n").into_bytes()
}

fn efface() -> Vec<u8> {
    // les cinq paramètres positionnels sont obligatoires, même pour `none` :
    // avec seulement `id` et `none`, mpv ignore la commande (constaté le
    // 26/11) et la barre reste figée à son dernier alpha.
    let commande = serde_json::json!({
        "command": ["osd-overlay", OSD_ID, "none", "", 0, 0],
    });
    format!("{commande}\n").into_bytes()
}

/// Contenu ASS de la barre : piste sombre, remplissage d'accent, temps.
fn barre_ass(pos: f64, duree: f64, alpha: u8) -> String {
    // une position qui dépasse la durée (fin de fichier, arrondis de mpv) ne
    // doit pas afficher « 1:41 / 1:40 »
    let pos = pos.clamp(0.0, duree);
    let part = (pos / duree).clamp(0.0, 1.0);
    let fin = OSD_X0 + (OSD_X1 - OSD_X0) * part;
    let teinte = format!("\\alpha&H{alpha:02X}&");
    // la piste reste plus transparente que le remplissage (alpha ASS : 0 =
    // opaque, 255 = invisible), et s'efface avec lui
    let piste_alpha = (0x9C + u16::from(alpha) * (0xFF - 0x9C) / 0xFF) as u8;
    let mut lignes = vec![format!(
        "{{\\an7\\pos(0,0)\\p1\\c&H000000&\\alpha&H{piste_alpha:02X}&\\bord0\\shad0}}{}",
        capsule(OSD_X0, OSD_Y0, OSD_X1, OSD_Y1, OSD_RADIUS),
    )];
    if fin - OSD_X0 > 1.0 {
        let rayon = OSD_RADIUS.min((fin - OSD_X0) / 2.0);
        lignes.push(format!(
            "{{\\an7\\pos(0,0)\\p1\\c{OSD_ACCENT}{teinte}\\bord0\\shad0}}{}",
            capsule(OSD_X0, OSD_Y0, fin, OSD_Y1, rayon),
        ));
    }
    lignes.push(format!(
        "{{\\an2\\pos(960,{OSD_TEXT_Y:.0})\\fs{OSD_FONT:.0}\\c&HFFFFFF&{teinte}\\bord2\\shad1}}{}  /  {}",
        duree_texte(pos),
        duree_texte(duree),
    ));
    lignes.join("\n")
}

/// Capsule arrondie en notation de dessin ASS (`\p1`) : quatre côtés et
/// quatre quarts de cercle approchés par des courbes de Bézier.
fn capsule(x0: f64, y0: f64, x1: f64, y1: f64, rayon: f64) -> String {
    let r = rayon.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    let c = r * 0.5523;
    let mut dessin = String::new();
    dessin.push_str(&format!("m {:.0} {:.0} ", x0 + r, y0));
    dessin.push_str(&format!("l {:.0} {:.0} ", x1 - r, y0));
    dessin.push_str(&format!(
        "b {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} ",
        x1 - r + c,
        y0,
        x1,
        y0 + r - c,
        x1,
        y0 + r,
    ));
    dessin.push_str(&format!("l {:.0} {:.0} ", x1, y1 - r));
    dessin.push_str(&format!(
        "b {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} ",
        x1,
        y1 - r + c,
        x1 - r + c,
        y1,
        x1 - r,
        y1,
    ));
    dessin.push_str(&format!("l {:.0} {:.0} ", x0 + r, y1));
    dessin.push_str(&format!(
        "b {:.0} {:.0} {:.0} {:.0} {:.0} {:.0} ",
        x0 + r - c,
        y1,
        x0,
        y1 - r + c,
        x0,
        y1 - r,
    ));
    dessin.push_str(&format!("l {:.0} {:.0} ", x0, y0 + r));
    dessin.push_str(&format!(
        "b {:.0} {:.0} {:.0} {:.0} {:.0} {:.0}",
        x0,
        y0 + r - c,
        x0 + r - c,
        y0,
        x0 + r,
        y0,
    ));
    dessin
}

/// Temps lisible, comme `mpv` : `1:23` sous une heure, `1:02:03` au-delà.
fn duree_texte(secondes: f64) -> String {
    let total = secondes.max(0.0) as u64;
    let (heures, minutes, secondes) = (total / 3600, (total % 3600) / 60, total % 60);
    if heures > 0 {
        format!("{heures}:{minutes:02}:{secondes:02}")
    } else {
        format!("{minutes}:{secondes:02}")
    }
}

fn mpv_binary() -> String {
    std::env::var("GLAUNCHER_MPV").unwrap_or_else(|_| "mpv".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temps_lisible() {
        assert_eq!(duree_texte(0.0), "0:00");
        assert_eq!(duree_texte(9.7), "0:09");
        assert_eq!(duree_texte(83.0), "1:23");
        assert_eq!(duree_texte(3723.0), "1:02:03");
        assert_eq!(duree_texte(-4.0), "0:00");
    }

    #[test]
    fn barre_a_moitie() {
        let ass = barre_ass(50.0, 100.0, 0);
        // la piste, le remplissage et le texte : trois événements
        assert_eq!(ass.lines().count(), 3);
        assert!(ass.contains(OSD_ACCENT));
        assert!(ass.contains("0:50  /  1:40"));
    }

    #[test]
    fn barre_au_debut_sans_remplissage() {
        let ass = barre_ass(0.0, 100.0, 0);
        // au tout début, seule la piste est dessinée : un remplissage de
        // largeur nulle n'a rien à montrer
        assert_eq!(ass.lines().count(), 2);
        assert!(!ass.contains(OSD_ACCENT));
    }

    #[test]
    fn barre_bornee_a_la_fin() {
        let ass = barre_ass(120.0, 100.0, 0);
        assert!(ass.contains("1:40  /  1:40"));
    }

    #[test]
    fn capsule_fermee_et_arrondie() {
        let dessin = capsule(0.0, 0.0, 100.0, 20.0, 10.0);
        assert!(dessin.starts_with("m 10 0"));
        assert_eq!(dessin.matches('b').count(), 4);
        // rayon borné à la moitié de la hauteur
        let plat = capsule(0.0, 0.0, 100.0, 4.0, 10.0);
        assert!(plat.starts_with("m 2 0"));
    }
}
