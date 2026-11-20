//! Musique de fond du lanceur : lecture, fondus, arrêt pendant une partie.
//!
//! La piste est **embarquée** dans le binaire (Opus 96 kb/s, 1,6 Mo — 22 Mo en
//! WAV) puis recopiée une fois dans le cache : `mpv` doit lire un fichier
//! *cherchable* pour boucler sur une plage (`--ab-loop-a` cherche dans le
//! fichier ; un tube ne se cherche pas).
//!
//! La lecture est confiée à `mpv`, lancé à part, pour deux raisons : le
//! launcher reste sans décodeur ni client audio embarqués (ALSA/PipeWire en
//! en-têtes à la compilation), et son volume se pilote **à chaud** par sa
//! socket IPC — seul moyen d'obtenir un vrai fondu, une `--volume` de ligne de
//! commande étant figée au démarrage.
//!
//! Coût mesuré (Opus 96 kb/s, sortie PipeWire, release) : ~2 à 3 % d'un cœur
//! et ~85 Mo résidents — la musique est **coupée pendant une partie**, et à ce
//! prix un fondu vaut mieux qu'un `pw-play` (1 %) qui n'en a pas. Le fondu ne
//! bloque jamais l'UI : un seul fil tient `mpv`, sa socket et la rampe de volume.
//!
//! **Bouclage** : `mpv` boucle la piste elle-même sur `LOOP_START → LOOP_END`
//! (première note → fin de la 48ᵉ mesure), donc l'intro est jouée en entier au
//! démarrage et chaque tour repart sur la première note. Aucun mélange, aucun
//! décalage. Ce qu'on a écarté, mesures à l'appui (20/11) : replier la fin sur
//! le début faisait démarrer la musique sur son dénouement ; boucler le fichier
//! tel quel collait **170 ms de silence** à chaque tour (le morceau meurt à
//! −67 dB, la piste d'origine reprend par 128 ms de silence avant l'attaque) —
//! c'est la « petite coupure » entendue. `--gapless-audio=yes` est là aussi :
//! sans lui, la réouverture de la sortie audio ajoutait 31 ms de blanc par tour.
//! Vérifié sur la sortie audio réelle (capture du moniteur) : aucune plage
//! silencieuse aux jonctions, et le saut de bouclage vaut 173/32768 grâce aux
//! 8 ms de fondu de garde en fin de piste.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Piste embarquée (source : `assets/music/menu.opus`).
const TRACK: &[u8] = include_bytes!("../assets/music/menu.opus");
const TRACK_FILE: &str = "menu.opus";

/// Volume de lecture par défaut, en pourcentage : c'est un fond sonore, il
/// reste derrière un jeu et derrière une vidéo. Réglable dans Gablue Manager
/// (`music_volume` dans `config.toml`).
pub const DEFAULT_VOLUME: u8 = 60;
/// Fondu d'entrée : au démarrage et au retour d'une partie.
const FADE_IN_MS: u64 = 1200;
/// Fondu de sortie : la durée du voile noir du lancement (`FADE_SECONDS` dans
/// `app.rs`), pour que la musique s'éteigne pendant que l'écran s'assombrit.
const FADE_OUT_MS: u64 = 900;
/// Pas des fondus : 30 ms — inaudible, et assez fin pour qu'un arrêt ne
/// s'entende pas comme une coupure.
const STEP_MS: u64 = 30;

/// Boucle de lecture : **première note** du morceau, et fin de sa 48ᵉ mesure.
///
/// Le morceau est calé sur 100 BPM (mesure de 2,4 s) : ses 48 mesures font
/// 115,2 s, et la piste embarquée s'arrête là (les 8 dernières millisecondes
/// sont un fondu de garde contre le clic, jamais joué — `B` tombe avant EOF).
/// `mpv` boucle `A→B` : l'intro est jouée en entier au démarrage, puis chaque
/// tour repart **sur la première note**, sans rien mélanger ni décaler (retour
/// 20/11 : un repli fin→début faisait démarrer la musique sur son dénouement,
/// et la piste d'origine collait 170 ms de silence entre la fin du morceau,
/// qui meurt à −67 dB, et l'attaque — mesuré sur la sortie audio).
const LOOP_START: f32 = 0.133;
const LOOP_END: f32 = 115.289;

/// Musique de fond du launcher.
///
/// Toutes les commandes passent par un fil : l'UI n'attend jamais `mpv` ni le
/// disque, et deux fondus ne peuvent pas se marcher dessus.
pub struct Music {
    commands: std::sync::mpsc::Sender<Order>,
}

#[derive(Debug)]
enum Order {
    /// (Re)lance la lecture, en fondu d'entrée.
    Play,
    /// Fondu de sortie puis arrêt de `mpv` (partie lancée).
    Stop,
}

impl Music {
    /// Démarre la musique. `None` quand la piste ne peut pas être écrite :
    /// le launcher continue sans musique. `mpv` absent est vu par le fil de
    /// lecture (il ne démarre rien et le dit une fois).
    pub fn start(cache: &Path, volume: u8) -> Option<Self> {
        let track = write_track(cache)?;
        let (commands, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("glauncher-music".into())
            .spawn(move || runner(track, receiver, volume))
            .ok()?;
        let music = Self { commands };
        music.resume();
        Some(music)
    }

    /// Relance la musique en fondu (démarrage, retour de partie).
    pub fn resume(&self) {
        let _ = self.commands.send(Order::Play);
    }

    /// Fondu de sortie puis arrêt : le jeu qui démarre n'a pas de musique
    /// par-dessus son propre son.
    pub fn fade_out(&self) {
        let _ = self.commands.send(Order::Stop);
    }
}

/// Recopie la piste embarquée dans le cache (`<cache>/music/menu.opus`).
///
/// Jamais dans `assets/` : ce dossier est partagé entre postes et ne contient
/// que des images de jeux. Une piste de même taille est considérée comme la
/// bonne — un réencodage change forcément la taille, donc le cache suit l'image.
fn write_track(cache: &Path) -> Option<PathBuf> {
    let dir = cache.join("music");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(TRACK_FILE);
    let up_to_date = std::fs::metadata(&path).is_ok_and(|meta| meta.len() as usize == TRACK.len());
    if !up_to_date {
        // `.part` + rename : jamais de piste à moitié écrite, même si le
        // launcher est tué pendant la copie
        let part = dir.join(format!("{TRACK_FILE}.part"));
        std::fs::write(&part, TRACK).ok()?;
        std::fs::rename(&part, &path).ok()?;
    }
    Some(path)
}

/// Fil de lecture : lance `mpv`, tient sa socket IPC, applique les fondus.
///
/// Le fil meurt quand `Music` est lâchée ; il tue alors `mpv` (et `mpv` reçoit
/// en plus `SIGTERM` si le launcher disparaît sans lâcher proprement).
fn runner(track: PathBuf, commands: std::sync::mpsc::Receiver<Order>, volume: u8) {
    // le volume vient de la configuration (0 à 100) : au-delà, `mpv` sature
    let volume = volume.min(100) as f32;
    // deux rampes : montée plus douce que la descente
    let in_step = volume / (FADE_IN_MS as f32 / STEP_MS as f32);
    let out_step = volume / (FADE_OUT_MS as f32 / STEP_MS as f32);
    let mut player: Option<Player> = None;
    let mut round = 0;
    let mut level = 0.0f32;
    let mut target = 0.0f32;

    loop {
        let order = commands.recv_timeout(Duration::from_millis(STEP_MS));
        if std::env::var_os("GLAUNCHER_MUSIC_DEBUG").is_some() {
            eprintln!(
                "glauncher : musique — {order:?} niveau {level:.1} cible {target:.1} mpv {}",
                player.is_some()
            );
        }
        match order {
            Ok(Order::Play) => {
                if player.is_none() {
                    round += 1;
                    match Player::open(&track, round) {
                        Some(opened) => {
                            player = Some(opened);
                            level = 0.0;
                        }
                        // `mpv` absent ou sortie audio cassée : inutile de
                        // réessayer à chaque partie, le fil se retire
                        None => return,
                    }
                }
                target = volume;
            }
            Ok(Order::Stop) => target = 0.0,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // `Music` lâchée (fermeture du launcher) : on s'arrête net
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if player.is_none() {
            continue;
        }
        if (level - target).abs() <= 0.05 {
            if target == 0.0 {
                // fondu de sortie terminé : `mpv` rend la main (et sa mémoire)
                if let Some(mut done) = player.take() {
                    done.quit();
                }
            }
            continue;
        }
        let step = if level < target { in_step } else { out_step };
        level = if level < target {
            (level + step).min(target)
        } else {
            (level - step).max(target)
        };
        if let Some(current) = player.as_mut() {
            current.set(level);
        }
    }

    // sortie du launcher : fin immédiate, sans fondu (rien pour l'écouter)
    if let Some(mut current) = player.take() {
        current.quit();
    }
}

/// Un `mpv` et sa socket.
struct Player {
    child: Child,
    stream: Option<UnixStream>,
}

impl Player {
    /// Lance `mpv` muet, bouclé sur la piste, et attend sa socket IPC.
    fn open(track: &Path, round: u32) -> Option<Self> {
        let socket = std::env::temp_dir().join(format!(
            "glauncher-music-{}-{round}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&socket);

        let mut command = Command::new("mpv");
        command
            // jamais la config, les scripts ni les raccourcis du compte : la
            // musique de fond doit sonner pareil sur toutes les machines
            .arg("--no-config")
            .arg("--no-video")
            .arg("--no-terminal")
            .arg("--really-quiet")
            // boucle sur la section musicale : voir LOOP_START / LOOP_END
            .arg(format!("--ab-loop-a={LOOP_START}"))
            .arg(format!("--ab-loop-b={LOOP_END}"))
            .arg("--volume=0")
            .arg("--ao=pipewire,pulse,alsa")
            .arg("--audio-display=no")
            // pas de fermeture/réouverture de la sortie audio entre deux tours :
            // c'est elle qui collerait un silence à chaque bouclage
            .arg("--gapless-audio=yes")
            // ~1 % de cœur de moins, et il n'y a de toute façon rien d'autre
            // à faire du fichier pendant la lecture
            .arg("--demuxer-thread=no")
            .arg(format!("--input-ipc-server={}", socket.display()))
            .arg(track)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Le launcher meurt (plantage compris) → `mpv` reçoit `SIGTERM` au lieu
        // de continuer à jouer seul. Le signal suit le **fil** qui a lancé
        // `mpv` : ce fil-ci, qui vit exactement aussi longtemps que la musique.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }

        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("glauncher : musique de fond indisponible ({err})");
                return None;
            }
        };
        // la socket apparaît ~30 ms après le lancement ; au-delà d'une seconde,
        // `mpv` a échoué (périphérique audio absent) : la musique se tait
        let stream = (0..50).find_map(|_| {
            match UnixStream::connect(&socket) {
                Ok(stream) => Some(stream),
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(20));
                    None
                }
            }
        });
        if stream.is_none() {
            eprintln!("glauncher : musique de fond muette (socket IPC absente)");
        }
        Some(Self { child, stream })
    }

    /// Volume courant, en pourcentage (`set_property volume`).
    fn set(&mut self, level: f32) {
        if let Some(stream) = self.stream.as_mut() {
            // les réponses de `mpv` ne sont pas lues : il en faut ~40 par
            // fondu, très en dessous de ce que la socket absorbe
            let _ = writeln!(
                stream,
                "{{\"command\":[\"set_property\",\"volume\",{level:.1}]}}"
            );
        }
    }

    /// Arrête `mpv` proprement et attend sa sortie (pas de zombie).
    fn quit(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            let _ = writeln!(stream, "{{\"command\":[\"quit\"]}}");
        }
        let _ = self.child.wait();
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
