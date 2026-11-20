//! Sélecteur de fichiers du système pour les visuels perso.
//!
//! Le launcher tourne en Wayland natif : un sélecteur maison obligerait à
//! réimplémenter la navigation dans les dossiers, alors que `kdialog` (KDE,
//! donc présent sur Gablue) et `zenity` (GTK) sont les dialogues natifs de
//! toutes les machines visées. On prend le premier disponible ; le fil appelant
//! tourne hors de l'UI, donc l'utilisateur peut chercher son image sans figer
//! le launcher.
//!
//! Formats proposés : ceux que le décodeur connaît (`png`, `jpg`, `webp`).

use std::path::{Path, PathBuf};

/// Filtre commun aux deux dialogues : une seule ligne, formats du décodeur.
const FILTRES: &str = "Images (*.png *.jpg *.jpeg *.webp)";

/// Ouvre le sélecteur de fichiers et rend le chemin choisi.
///
/// `None` quand l'utilisateur annule, quand le dialogue n'existe pas sur la
/// machine, ou quand il échoue — dans tous ces cas le launcher ne change rien.
pub fn choisir_image(depart: &Path, titre: &str) -> Option<PathBuf> {
    let depart = depart.to_string_lossy().to_string();

    // kdialog d'abord (KDE) : options avant la commande, dossier de départ
    // puis filtre en arguments positionnels
    let kdialog = std::process::Command::new("kdialog")
        .args(["--title", titre])
        .args(["--getopenfilename", &depart, FILTRES])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    if let Ok(sortie) = kdialog {
        return reponse(&sortie);
    }

    // zenity (GTK) : même contrat, options différentes
    let zenity = std::process::Command::new("zenity")
        .arg("--file-selection")
        .arg(format!("--title={titre}"))
        .arg(format!("--filename={}/", depart.trim_end_matches('/')))
        .arg(format!("--file-filter={FILTRES}"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    if let Ok(sortie) = zenity {
        return reponse(&sortie);
    }

    eprintln!("glauncher : aucun sélecteur de fichiers (kdialog ou zenity)");
    None
}

/// Chemin écrit sur la sortie standard du dialogue, si celui-ci a réussi.
fn reponse(sortie: &std::process::Output) -> Option<PathBuf> {
    // annulation = code retour non nul et rien sur la sortie
    if !sortie.status.success() {
        return None;
    }
    let chemin = String::from_utf8_lossy(&sortie.stdout).trim().to_string();
    (!chemin.is_empty()).then(|| PathBuf::from(chemin))
}
