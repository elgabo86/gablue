//! Autre son que celui du launcher : la musique de fond s'efface.
//!
//! `pactl list sink-inputs` donne, pour chaque flux, son PID applicatif, son
//! état muet/bouchonné et son volume. La règle est volontairement large : la
//! musique du launcher est un **fond sonore**, donc dès qu'une autre
//! application joue du son (vidéo à côté d'un jeu, navigateur, lecteur), elle
//! n'a pas à se superposer. Le flux du launcher lui-même est exclu par le PID
//! de son `mpv` — jamais par son nom : impossible de se confondre avec un
//! `mpv` lancé par l'utilisateur.
//!
//! **Le PID d'un flux ne suffit pas** : sous PipeWire, un flux `mpv` ne publie
//! pas `application.process.id` (constaté le 22/11 sur cette machine). Le PID
//! se lit alors sur son **client** (`pactl list clients`, `Client: N` →
//! `application.process.id`, à défaut `pipewire.sec.pid`) : sans ça, notre
//! propre musique passait pour « une autre application » et se coupait
//! elle-même toutes les deux secondes.
//!
//! `LC_ALL=C` est obligatoire : la sortie de pactl est traduite, et un poste en
//! français ne produirait sinon rien d'exploitable.

use std::collections::HashMap;

/// Un flux de lecture, réduit à ce qui décide de l'effacement.
struct Flux {
    /// `Client: N` du flux — le PID s'y trouve quand le flux ne le publie pas.
    client: u32,
    /// `application.process.id` du flux (0 = absent).
    pid: u32,
    /// Bouchonné (pause) ou muet : ce flux ne fait pas de bruit.
    actif: bool,
    /// Volume audible (un flux à 0 % est silencieux).
    audible: bool,
}

impl Flux {
    fn retenu(&self, clients: &HashMap<u32, u32>, exclure: Option<u32>) -> bool {
        let pid = if self.pid > 0 {
            self.pid
        } else {
            clients.get(&self.client).copied().unwrap_or(0)
        };
        self.actif && self.audible && Some(pid) != exclure
    }
}

/// `pactl list clients` → client → PID applicatif (`application.process.id`,
/// à défaut `pipewire.sec.pid`). Pure : testée sur une sortie figée.
pub fn clients(texte: &str) -> HashMap<u32, u32> {
    let mut carte = HashMap::new();
    let mut client: Option<u32> = None;
    let mut pid = 0;
    for ligne in texte.lines().map(str::trim) {
        if let Some(reste) = ligne.strip_prefix("Client #") {
            if let Some(client) = client {
                carte.insert(client, pid);
            }
            client = reste.trim().parse().ok();
            pid = 0;
            continue;
        }
        if client.is_none() {
            continue;
        }
        if let Some(valeur) = propriete(ligne, "application.process.id") {
            pid = valeur.parse().unwrap_or(0);
        } else if pid == 0
            && let Some(valeur) = propriete(ligne, "pipewire.sec.pid")
        {
            pid = valeur.parse().unwrap_or(0);
        }
    }
    if let Some(client) = client {
        carte.insert(client, pid);
    }
    carte
}

/// Valeur d'une ligne `clé = "valeur"`.
fn propriete(ligne: &str, cle: &str) -> Option<String> {
    let reste = ligne.strip_prefix(cle)?.trim_start().strip_prefix('=')?.trim();
    Some(reste.trim_matches('"').to_string())
}

/// Décode `pactl list sink-inputs` en flux (pure). Le PID n'est pas encore
/// résolu : `Flux::retenu` s'en charge avec la table des clients.
fn parser(texte: &str) -> Vec<Flux> {
    let mut flux: Option<Flux> = None;
    let mut sortie: Vec<Flux> = Vec::new();

    for ligne in texte.lines().map(str::trim) {
        let Some(reste) = ligne.strip_prefix("Sink Input #") else {
            let Some(courant) = flux.as_mut() else {
                continue;
            };
            if let Some(reste) = ligne.strip_prefix("Corked:") {
                if reste.trim() != "no" {
                    courant.actif = false;
                }
            } else if let Some(reste) = ligne.strip_prefix("Mute:") {
                if reste.trim() == "yes" {
                    courant.actif = false;
                }
            } else if let Some(reste) = ligne.strip_prefix("Volume:") {
                courant.audible = reste
                    .split_whitespace()
                    .find_map(|mot| mot.strip_suffix('%')?.parse::<f32>().ok())
                    .is_some_and(|pourcent| pourcent > 0.5);
            } else if let Some(reste) = ligne.strip_prefix("Client:") {
                courant.client = reste.trim().parse().unwrap_or(0);
            } else if let Some(valeur) = propriete(ligne, "application.process.id") {
                courant.pid = valeur.parse().unwrap_or(0);
            }
            continue;
        };
        // nouveau bloc : on range le précédent
        sortie.extend(flux.take());
        if reste.trim().parse::<u32>().is_ok() {
            flux = Some(Flux {
                client: 0,
                pid: 0,
                actif: true,
                audible: false,
            });
        }
    }
    sortie.extend(flux);
    sortie
}

/// Y a-t-il un flux audible dont le PID (résolu par la table des clients)
/// n'est pas `exclure` ?
pub fn joue(texte: &str, clients: &HashMap<u32, u32>, exclure: Option<u32>) -> bool {
    parser(texte)
        .iter()
        .any(|flux| flux.retenu(clients, exclure))
}

/// Une autre application joue-t-elle du son maintenant ?
pub fn autre_flux(exclure: Option<u32>) -> bool {
    let lire = |quoi: &str| {
        std::process::Command::new("/usr/bin/pactl")
            .args(["list", quoi])
            .env("LC_ALL", "C")
            .output()
            .map(|sortie| String::from_utf8_lossy(&sortie.stdout).to_string())
            // pas de pactl (session sans PipeWire/Pulse) : rien ne joue, la
            // musique reprend son cours habituel
            .unwrap_or_default()
    };
    joue(
        &lire("sink-inputs"),
        &clients(&lire("clients")),
        exclure,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Aucune table de clients : les PID viennent des propriétés du flux.
    fn seuls() -> HashMap<u32, u32> {
        HashMap::new()
    }

    const DEUX: &str = "\
Sink Input #63
\tCorked: no
\tMute: no
\tVolume: front-left: 65536 / 100% / 0.00 dB
\tProperties:
\t\tapplication.process.id = \"3895\"

Sink Input #71
\tCorked: yes
\tMute: no
\tVolume: front-left: 32768 / 50% / -18.06 dB
\tProperties:
\t\tapplication.process.id = \"4242\"
";

    #[test]
    fn une_video_qui_joue_bloque_la_musique() {
        assert!(joue(DEUX, &seuls(), None));
        // notre propre flux est exclu : l'autre bloc est bouchonné (vidéo en
        // pause), donc plus rien ne joue
        assert!(!joue(DEUX, &seuls(), Some(3895)));
    }

    #[test]
    fn muet_ou_volume_nul_ne_compte_pas() {
        let muet = "\
Sink Input #1
\tCorked: no
\tMute: yes
\tVolume: mono: 65536 / 100% / 0.00 dB
\tProperties:
\t\tapplication.process.id = \"10\"
";
        assert!(!joue(muet, &seuls(), None));
        let nul = "\
Sink Input #2
\tCorked: no
\tMute: no
\tVolume: mono: 0 / 0% / -inf dB
\tProperties:
\t\tapplication.process.id = \"11\"
";
        assert!(!joue(nul, &seuls(), None));
    }

    #[test]
    fn sortie_vide_ou_traduite() {
        assert!(!joue("", &seuls(), None));
        // poste en français sans LC_ALL=C : aucune ligne reconnue, donc aucune
        // conclusion — c'est pourquoi la commande force la locale C
        assert!(!joue(
            "Entrée de la destination #88\n\tBouchonné : non\n\tSourdine : non\n",
            &seuls(),
            None
        ));
    }

    #[test]
    fn pid_absent_compte_comme_un_autre_flux() {
        let sans_pid = "\
Sink Input #5
\tCorked: no
\tMute: no
\tVolume: mono: 65536 / 80% / 0.00 dB
";
        // ni propriété ni client connu : on ne peut pas l'attribuer à notre
        // mpv, donc il compte (mieux vaut taire un fond sonore que le
        // superposer à ce que l'utilisateur écoute)
        assert!(joue(sans_pid, &seuls(), Some(1234)));
    }

    #[test]
    fn notre_mpv_sans_pid_est_reconnu_par_son_client() {
        // constaté le 22/11 : sous PipeWire, le flux `mpv` ne publie PAS
        // `application.process.id` ; sans la table des clients, la musique du
        // launcher se croyait concurrencée par elle-même et se coupait.
        let nôtre = "\
Sink Input #117
\tClient: 125
\tCorked: no
\tMute: no
\tVolume: front-center: 65536 / 100% / 0.00 dB
\tProperties:
\t\tapplication.name = \"mpv\"
";
        let clients_txt = "\
Client #125
\tProperties:
\t\tpipewire.sec.pid = \"77422\"
\t\tapplication.process.id = \"77422\"
\t\tapplication.name = \"mpv\"
";
        let table = clients(clients_txt);
        assert_eq!(table.get(&125), Some(&77422));
        assert!(!joue(nôtre, &table, Some(77422)), "notre propre flux");
        assert!(joue(nôtre, &table, Some(999)), "un autre mpv compte");

        // une vidéo à côté, elle, reste un autre flux
        let video = "\
Sink Input #118
\tClient: 126
\tCorked: no
\tMute: no
\tVolume: front-left: 65536 / 70% / -9.00 dB
\tProperties:
";
        let mut table = table;
        table.insert(126, 3895);
        assert!(joue(video, &table, Some(77422)));
    }

    /// Vérification RÉELLE du poste (audio, pas de simulation) : un `mpv`
    /// joue un son à 1 % et on interroge `pactl` pour de vrai. Ignoré par
    /// défaut (il faut une session PipeWire/Pulse et une sortie audio) :
    /// `cargo test flux_reel -- --ignored --nocapture`.
    #[test]
    #[ignore = "touche au son du poste : vérification manuelle de la chaîne pactl"]
    fn flux_reel_du_poste() {
        let mut enfant = std::process::Command::new("/usr/bin/mpv")
            .args([
                "--no-config",
                "--no-video",
                "--really-quiet",
                "--no-terminal",
                "--ao=pipewire,pulse",
                // inaudible à l'oreille, mais audible pour le parseur (> 0,5 %)
                "--volume=1",
                "av://lavfi:sine=frequency=440",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("mpv doit être installé");
        let mien = enfant.id();

        // relit le poste : flux ET table des clients, comme `autre_flux`
        let lire = |quoi: &str| {
            std::process::Command::new("/usr/bin/pactl")
                .args(["list", quoi])
                .env("LC_ALL", "C")
                .output()
                .map(|sortie| String::from_utf8_lossy(&sortie.stdout).to_string())
                .unwrap_or_default()
        };
        let pid_de = |flux: &Flux, table: &HashMap<u32, u32>| {
            if flux.pid > 0 {
                flux.pid
            } else {
                table.get(&flux.client).copied().unwrap_or(0)
            }
        };
        // le flux met quelques centaines de ms à apparaître côté PipeWire
        let mut attribue = false;
        for _ in 0..50 {
            let table = clients(&lire("clients"));
            attribue = parser(&lire("sink-inputs"))
                .iter()
                .any(|flux| pid_de(flux, &table) == mien);
            if attribue {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let propre = autre_flux(Some(mien));
        let etranger = autre_flux(Some(mien + 1));
        let _ = enfant.kill();
        let _ = enfant.wait();
        assert!(
            attribue,
            "le flux de mpv doit être rattaché à son PID (propriété ou client)"
        );
        assert!(!propre, "notre propre flux ne compte pas comme un autre");
        assert!(etranger, "vu d'un autre PID, ce flux est un autre flux");
    }

    #[test]
    fn client_sans_pid_retombe_sur_le_sec_pid() {
        let texte = "\
Client #9
\tProperties:
\t\tpipewire.sec.pid = \"3141\"
";
        assert_eq!(clients(texte).get(&9), Some(&3141));
        assert!(clients("").is_empty());
    }
}
