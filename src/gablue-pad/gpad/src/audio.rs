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

/// Analyse de `pactl list sink-inputs`. Pure : le banc l'essaie sur une sortie
/// réelle figée, sans toucher au son du poste.
pub fn parser(texte: &str) -> Vec<Flux> {
    let mut flux: Vec<Flux> = Vec::new();
    let mut courant: Option<Flux> = None;
    for ligne in texte.lines() {
        let texte = ligne.trim();
        if let Some(reste) = texte.strip_prefix("Sink Input #") {
            flux.extend(courant.take());
            courant = reste.trim().parse().ok().map(|index| Flux {
                index,
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
        if let Some(valeur) = valeur(texte, "application.process.id") {
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

/// Flux en cours de lecture sur le poste.
pub fn flux() -> Vec<Flux> {
    std::process::Command::new(PACTL)
        .args(["list", "sink-inputs"])
        .output()
        .map(|sortie| parser(&String::from_utf8_lossy(&sortie.stdout)))
        .unwrap_or_default()
}

/// Règle le volume d'un flux (0..100 %).
pub fn regler(index: u32, pourcent: u8) -> bool {
    std::process::Command::new(PACTL)
        .args([
            "set-sink-input-volume",
            &index.to_string(),
            &format!("{}%", pourcent.min(100)),
        ])
        .status()
        .is_ok_and(|statut| statut.success())
}

/// Bascule le muet d'un flux.
pub fn basculer_mute(index: u32) -> bool {
    std::process::Command::new(PACTL)
        .args(["set-sink-input-mute", &index.to_string(), "toggle"])
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

    #[test]
    fn deux_flux_lus() {
        let flux = parser(SORTIE);
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
    fn sortie_vide_ou_bruitee() {
        assert!(parser("").is_empty());
        assert!(parser("Sink Input #x\n\tMute: no\n").is_empty(), "index illisible");
        assert!(parser("Volume: front-left: 1 / 3% / x\n").is_empty(), "hors bloc");
    }

    #[test]
    fn pourcentage_arrondi_et_borne() {
        assert_eq!(pourcent("Volume: front-left: 1 / 44% / x"), Some(44));
        assert_eq!(pourcent("Volume: front-left: 1 / 140% / x"), Some(100));
        assert_eq!(pourcent("Volume: sans valeur"), None);
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
