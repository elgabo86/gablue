//! Sortie et entrée audio choisies depuis les Réglages du launcher.
//!
//! En **session console** il n'y a pas de plasmashell, donc pas d'applet audio :
//! changer de casque, de TV ou de micro demandait de sortir de la session. Les
//! deux lignes « Sortie audio » / « Entrée audio » des Réglages pilotent
//! PipeWire par `pactl`, et le choix **survit** : WirePlumber retient le
//! périphérique par défaut de l'utilisateur, aucun fichier de config du
//! launcher n'est écrit pour ça.
//!
//! `LC_ALL=C` pour la stabilité de la sortie, comme `sons.rs` ; les
//! descriptions restent celles de l'appareil (souvent localisées par ALSA) et
//! sont affichées telles quelles. Les **monitors** (sorties réinjectées en
//! entrée) ne sont jamais proposés comme micro.

use std::process::Command;

/// Un périphérique audio proposé : son nom PipeWire (`alsa_output.…`) et son
/// libellé lisible (« Ryzen HD Audio Controller Stéréo analogique »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peripherique {
    pub nom: String,
    pub libelle: String,
}

impl Peripherique {
    /// Nom affichable : le libellé, sinon le nom technique (un appareil peut
    /// n'avoir aucune description).
    pub fn affiche(&self) -> &str {
        if self.libelle.is_empty() {
            &self.nom
        } else {
            &self.libelle
        }
    }
}

/// `pactl list sinks|sources` → périphériques, dans l'ordre du serveur.
/// `sources` écarte les `*.monitor` : ce sont des copies de sortie, pas des
/// micros.
pub fn peripheriques(texte: &str, sources: bool) -> Vec<Peripherique> {
    let mut out = Vec::new();
    let mut nom: Option<String> = None;
    let mut libelle = String::new();
    for ligne in texte.lines().map(str::trim) {
        if let Some(reste) = ligne.strip_prefix("Name: ") {
            if let Some(ancien) = nom.take() {
                if !(sources && ancien.ends_with(".monitor")) {
                    out.push(Peripherique {
                        nom: ancien,
                        libelle: std::mem::take(&mut libelle),
                    });
                }
            }
            libelle.clear();
            nom = Some(reste.trim().to_string());
        } else if let Some(reste) = ligne.strip_prefix("Description: ") {
            if nom.is_some() {
                libelle = reste.trim().to_string();
            }
        }
    }
    if let Some(nom) = nom {
        if !(sources && nom.ends_with(".monitor")) {
            out.push(Peripherique { nom, libelle });
        }
    }
    out
}

fn pactl(args: &[&str]) -> Option<String> {
    let sortie = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .ok()?;
    if !sortie.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&sortie.stdout).into_owned())
}

pub fn sorties() -> Vec<Peripherique> {
    pactl(&["list", "sinks"])
        .map(|t| peripheriques(&t, false))
        .unwrap_or_default()
}

pub fn entrees() -> Vec<Peripherique> {
    pactl(&["list", "sources"])
        .map(|t| peripheriques(&t, true))
        .unwrap_or_default()
}

pub fn defaut_sortie() -> String {
    pactl(&["get-default-sink"]).unwrap_or_default().trim().to_string()
}

pub fn defaut_entree() -> String {
    pactl(&["get-default-source"]).unwrap_or_default().trim().to_string()
}

/// Applique le défaut. `false` = `pactl` a refusé (serveur absent, nom périmé).
pub fn choisir_sortie(nom: &str) -> bool {
    Command::new("pactl")
        .args(["set-default-sink", nom])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn choisir_entree(nom: &str) -> bool {
    Command::new("pactl")
        .args(["set-default-source", nom])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINKS: &str = "\
Sink #14651
\tState: SUSPENDED
\tName: alsa_output.pci-0000_c6_00.1.hdmi-stereo-extra1.99
\tDescription: Radeon High Definition Audio Controller Digital Stereo (HDMI 2)
\tDriver: PipeWire
Sink #14652
\tState: RUNNING
\tName: alsa_output.pci-0000_c6_00.6.analog-stereo.25
\tDescription: Ryzen HD Audio Controller Stéréo analogique
\tDriver: PipeWire
";

    const SOURCES: &str = "\
Source #14651
\tName: alsa_output.pci-0000_c6_00.1.hdmi-stereo-extra1.99.monitor
\tDescription: Monitor of Radeon High Definition Audio
Source #14653
\tName: alsa_input.pci-0000_c6_00.6.analog-stereo.27
\tDescription: Ryzen HD Audio Controller Stéréo analogique
";

    #[test]
    fn sorties_lues_avec_leur_description() {
        let s = peripheriques(SINKS, false);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].nom, "alsa_output.pci-0000_c6_00.1.hdmi-stereo-extra1.99");
        assert!(s[0].libelle.contains("Radeon"));
        assert_eq!(s[1].affiche(), "Ryzen HD Audio Controller Stéréo analogique");
    }

    #[test]
    fn monitors_ecartes_des_entrees() {
        let e = peripheriques(SOURCES, true);
        assert_eq!(e.len(), 1, "le monitor n'est pas un micro");
        assert!(e[0].nom.starts_with("alsa_input."));
        // en liste de SORTIES, le même texte ne filtre rien (ce ne sont pas
        // des monitors au sens de l'entrée, et le filtre ne s'applique qu'aux
        // sources)
        assert_eq!(peripheriques(SOURCES, false).len(), 2);
    }

    #[test]
    fn description_absente_repliee_sur_le_nom() {
        let texte = "Sink #1\n\tName: alsa_output.test\n";
        let s = peripheriques(texte, false);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].affiche(), "alsa_output.test");
    }
}
