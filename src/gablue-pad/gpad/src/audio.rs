//! Volume par flux audio : sortie générale, jeu en cours, application compagne.
//!
//! `pactl list sink-inputs` est la seule source qui donne, pour CHAQUE flux,
//! son `application.process.id` et son volume. C'est ce qui permet de régler le
//! jeu sans toucher à la musique de fond, ni au son d'une vidéo lancée à côté :
//! la sortie par défaut (VOLUME GENERAL) n'est qu'un des flux.
//!
//! Aucun démon, aucune dépendance : on lit la sortie texte de `pactl` (locale
//! C, format stable) et on écrit par `pactl set-sink-input-volume`.

/// Chemins absolus : `gpad` tourne dans une session où le `PATH` n'est pas
/// garanti (même choix que `pactl` dans `refresh_volume`).
const PACTL: &str = "/usr/bin/pactl";

/// Un flux de lecture (une application qui émet du son).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flux {
    /// Index `Sink Input #N` — c'est lui que `pactl` attend pour agir.
    pub index: u32,
    /// PID applicatif (`application.process.id`), 0 si absent.
    pub pid: i32,
    /// Nom de l'application (`application.name`), pour l'affichage.
    pub nom: String,
    pub pourcent: u8,
    pub mute: bool,
}

/// Applications compagnes reconnues : leur son ne doit pas être pris pour
/// celui du jeu. Comparaison insensible à la casse, en sous-chaîne — un
/// flatpak annonce parfois son nom d'app id (`rocks.shy.VacuumTube`).
const COMPAGNES: [&str; 5] = ["vacuumtube", "jellyfin", "gablue-tv", "tvqt", "youtube"];

/// Ce nom d'application est-il une application compagne (vidéo lancée à côté
/// du jeu) ?
pub fn est_compagne(nom: &str) -> bool {
    let minuscule = nom.to_lowercase();
    COMPAGNES.iter().any(|connue| minuscule.contains(connue))
}

/// Valeur d'une ligne `clé = "valeur"` de la section `Properties`.
fn valeur(ligne: &str, cle: &str) -> Option<String> {
    let reste = ligne.strip_prefix(cle)?.trim_start().strip_prefix('=')?.trim();
    Some(reste.trim_matches('"').to_string())
}

/// Premier pourcentage d'une ligne `Volume: front-left: … / 45% / …`.
fn pourcent(ligne: &str) -> Option<u8> {
    ligne
        .split_whitespace()
        .find_map(|mot| mot.strip_suffix('%').and_then(|chiffres| chiffres.parse::<f32>().ok()))
        .map(|valeur| valeur.round().clamp(0.0, 100.0) as u8)
}

/// Bloc brut de `pactl list sink-inputs` : PID du flux s'il est publié par
/// l'application, sinon celui du client (voir `parser_clients`).
struct Brut {
    index: u32,
    client: u32,
    pid: i32,
    nom: String,
    pourcent: u8,
    mute: bool,
}

/// Analyse de `pactl list sink-inputs`. Pure : le banc l'essaie sur une sortie
/// réelle figée, sans toucher au son du poste.
fn parser_brut(texte: &str) -> Vec<Brut> {
    let mut flux: Vec<Brut> = Vec::new();
    let mut courant: Option<Brut> = None;
    for ligne in texte.lines() {
        let texte = ligne.trim();
        if let Some(reste) = texte.strip_prefix("Sink Input #") {
            flux.extend(courant.take());
            courant = reste.trim().parse().ok().map(|index| Brut {
                index,
                client: 0,
                pid: 0,
                nom: String::new(),
                // 100 % tant qu'aucune ligne `Volume:` n'est lue : un flux
                // muet sans volume connu ne doit pas apparaître à 0 %
                pourcent: 100,
                mute: false,
            });
            continue;
        }
        let Some(flux) = courant.as_mut() else {
            continue;
        };
        if let Some(reste) = texte.strip_prefix("Client:") {
            flux.client = reste.trim().parse().unwrap_or(0);
        } else if let Some(valeur) = valeur(texte, "application.process.id") {
            flux.pid = valeur.parse().unwrap_or(0);
        } else if let Some(valeur) = valeur(texte, "application.name") {
            flux.nom = valeur;
        } else if let Some(reste) = texte.strip_prefix("Mute:") {
            flux.mute = reste.trim() == "yes";
        } else if texte.starts_with("Volume:")
            && let Some(valeur) = pourcent(texte)
        {
            flux.pourcent = valeur;
        }
    }
    flux.extend(courant);
    flux
}

/// `pactl list clients` → client → PID applicatif.
///
/// **Indispensable** : sous PipeWire, un flux `mpv` ne publie PAS
/// `application.process.id` dans ses propriétés (constaté le 22/11 sur cette
/// machine : ni mpv, ni une session PipeWire standard ne l'exposent). Le PID
/// est en revanche sur le **client** (`application.process.id`, à défaut
/// `pipewire.sec.pid`), que le flux désigne par `Client: N`.
pub fn parser_clients(texte: &str) -> std::collections::HashMap<u32, i32> {
    let mut carte = std::collections::HashMap::new();
    let mut client: Option<u32> = None;
    let mut pid = 0;
    for ligne in texte.lines() {
        let texte = ligne.trim();
        if let Some(reste) = texte.strip_prefix("Client #") {
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
        if let Some(valeur) = valeur(texte, "application.process.id") {
            pid = valeur.parse().unwrap_or(0);
        } else if pid == 0
            && let Some(valeur) = valeur(texte, "pipewire.sec.pid")
        {
            pid = valeur.parse().unwrap_or(0);
        }
    }
    if let Some(client) = client {
        carte.insert(client, pid);
    }
    carte
}

/// Fusionne les deux sorties : le PID d'un flux vient de ses propres
/// propriétés, sinon de son client.
pub fn parser(
    texte: &str,
    clients: &std::collections::HashMap<u32, i32>,
) -> Vec<Flux> {
    parser_brut(texte)
        .into_iter()
        .map(|brut| Flux {
            index: brut.index,
            pid: if brut.pid > 0 {
                brut.pid
            } else {
                clients.get(&brut.client).copied().unwrap_or(0)
            },
            nom: brut.nom,
            pourcent: brut.pourcent,
            mute: brut.mute,
        })
        .collect()
}

/// Flux en cours de lecture sur le poste.
///
/// `LC_ALL=C` est **obligatoire** : pactl est traduit (« Entrée de la
/// destination #88 », « Bouchonné : non », « Sourdine : non »), et un poste en
/// français ne produisait alors AUCUN flux — les lignes VOLUME JEU et VOLUME
/// APP n'apparaissaient jamais (constaté 22/11 sur la session française, le
/// banc à fixtures anglaises ne le voyait pas).
pub fn flux() -> Vec<Flux> {
    let lire = |quoi: &str| {
        std::process::Command::new(PACTL)
            .args(["list", quoi])
            .env("LC_ALL", "C")
            .output()
            .map(|sortie| String::from_utf8_lossy(&sortie.stdout).to_string())
            .unwrap_or_default()
    };
    let clients = parser_clients(&lire("clients"));
    parser(&lire("sink-inputs"), &clients)
}

/// Règle le volume d'un flux (0..100 %).
pub fn regler(index: u32, pourcent: u8) -> bool {
    std::process::Command::new(PACTL)
        .args([
            "set-sink-input-volume",
            &index.to_string(),
            &format!("{}%", pourcent.min(100)),
        ])
        .env("LC_ALL", "C")
        .status()
        .is_ok_and(|statut| statut.success())
}

/// Pose le muet d'un flux, valeur **explicite** (contrairement à
/// `basculer_mute`, rejouable sans dépendre de l'état courant).
pub fn muet(index: u32, muet: bool) -> bool {
    std::process::Command::new(PACTL)
        .args([
            "set-sink-input-mute",
            &index.to_string(),
            if muet { "1" } else { "0" },
        ])
        .env("LC_ALL", "C")
        .status()
        .is_ok_and(|statut| statut.success())
}

/// Bascule le muet d'un flux.
pub fn basculer_mute(index: u32) -> bool {
    std::process::Command::new(PACTL)
        .args(["set-sink-input-mute", &index.to_string(), "toggle"])
        .env("LC_ALL", "C")
        .status()
        .is_ok_and(|statut| statut.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SORTIE: &str = "\
Sink Input #63
	Driver: protocol-native.c
	Owner Module: 12
	Client: 145
	Sink: 0
	Mute: no
	Volume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB
	        balance 0.00
	Properties:
		media.name = \"playStream\"
		application.name = \"VacuumTube\"
		application.process.id = \"3895\"
		application.icon_name = \"rocks.shy.VacuumTube\"

Sink Input #71
	Mute: yes
	Volume: front-left: 32768 / 50% / -18.06 dB,   front-right: 32768 / 50% / -18.06 dB
	Properties:
		application.name = \"gwine\"
		application.process.id = \"4242\"
";

    /// Aucune table de clients : les PID viennent des propriétés du flux.
    fn seuls() -> std::collections::HashMap<u32, i32> {
        std::collections::HashMap::new()
    }

    #[test]
    fn deux_flux_lus() {
        let flux = parser(SORTIE, &seuls());
        assert_eq!(flux.len(), 2);
        assert_eq!(
            flux[0],
            Flux {
                index: 63,
                pid: 3895,
                nom: "VacuumTube".to_string(),
                pourcent: 100,
                mute: false,
            }
        );
        assert_eq!(flux[1].index, 71);
        assert_eq!(flux[1].pid, 4242);
        assert_eq!(flux[1].pourcent, 50);
        assert!(flux[1].mute);
    }

    #[test]
    fn sortie_traduite_non_interpretable() {
        // un poste en français : c'est pourquoi `flux()` force LC_ALL=C
        let francais = "\
Entrée de la destination #88\n\tBouchonné : non\n\tSourdine : non\n\tVolume : mono: 65536 / 100% / 0.00 dB\n";
        assert!(parser(francais, &seuls()).is_empty());
    }

    #[test]
    fn sortie_vide_ou_bruitee() {
        assert!(parser("", &seuls()).is_empty());
        assert!(
            parser("Sink Input #x\n\tMute: no\n", &seuls()).is_empty(),
            "index illisible"
        );
        assert!(
            parser("Volume: front-left: 1 / 3% / x\n", &seuls()).is_empty(),
            "hors bloc"
        );
    }

    #[test]
    fn pourcentage_arrondi_et_borne() {
        assert_eq!(pourcent("Volume: front-left: 1 / 44% / x"), Some(44));
        assert_eq!(pourcent("Volume: front-left: 1 / 140% / x"), Some(100));
        assert_eq!(pourcent("Volume: sans valeur"), None);
    }

    #[test]
    fn pid_du_client_quand_le_flux_ne_le_publie_pas() {
        // constaté le 22/11 sur cette machine : sous PipeWire, `mpv` ne met
        // PAS `application.process.id` dans les propriétés de son flux. Sans
        // le client, le flux du jeu n'était rattaché à aucun PID — les lignes
        // VOLUME JEU / VOLUME APP restaient absentes.
        let fluxs = "\
Sink Input #117
\tClient: 125
\tMute: no
\tVolume: front-center: 65536 / 100% / 0.00 dB
\tProperties:
\t\tapplication.name = \"mpv\"

Sink Input #118
\tClient: 126
\tProperties:
\t\tapplication.name = \"gwine\"
\t\tapplication.process.id = \"4242\"
";
        let clients = "\
Client #125
\tProperties:
\t\tpipewire.sec.pid = \"4241\"
\t\tapplication.process.id = \"4241\"
\t\tapplication.name = \"mpv\"

Client #126
\tProperties:
\t\tpipewire.sec.pid = \"4242\"
\t\tapplication.process.id = \"4242\"
";
        let carte = parser_clients(clients);
        assert_eq!(carte.get(&125), Some(&4241));
        assert_eq!(carte.get(&126), Some(&4242));
        let flux = parser(fluxs, &carte);
        assert_eq!(flux.len(), 2);
        assert_eq!(flux[0].pid, 4241, "PID pris sur le client");
        assert_eq!(flux[1].pid, 4242, "propriété du flux prioritaire");
    }

    #[test]
    fn client_sans_pid_retombe_sur_le_sec_pid() {
        let clients = "\
Client #9
\tProperties:
\t\tpipewire.sec.pid = \"3141\"
";
        assert_eq!(parser_clients(clients).get(&9), Some(&3141));
        assert!(parser_clients("").is_empty());
    }

    #[test]
    fn applications_compagnes() {
        assert!(est_compagne("VacuumTube"));
        assert!(est_compagne("rocks.shy.VacuumTube"));
        assert!(est_compagne("Jellyfin Desktop"));
        assert!(est_compagne("tvqt"));
        assert!(!est_compagne("gwine"));
        assert!(!est_compagne("plasmashell"));
    }
}
