//! Dossiers de sortie de l'enregistreur et des captures d'écran.
//!
//! `gpad-rec`, `grec`, l'overlay manette et Gablue Manager partagent un seul
//! fichier, `~/.config/gablue/recorder.conf` (`clé=valeur`, relu à chaud par le
//! démon). Le launcher ne fait que lire et écrire **les deux clés de dossier** :
//!
//! - `dossier` : enregistrements et replays (`Video_*.mp4`, `Replay_*.mp4`) ;
//! - `dossier_screenshot` : captures d'écran (`screenshot_*.png`), lues par
//!   `screenshot.sh` de l'overlay.
//!
//! Pas de valeur par défaut écrite : un dossier « par défaut » est une clé
//! **absente** (le démon et le script connaissent leur propre défaut). C'est ce
//! qui rend « remettre par défaut » réversible et permet au défaut de changer
//! avec l'image sans qu'un ancien chemin traîne dans un fichier.
//!
//! L'écriture ne réécrit que la clé visée : commentaires, ordre et clés
//! inconnues sont conservés, comme pour `game_dirs` — plusieurs outils écrivent
//! ce fichier et doivent être interchangeables.

use std::path::PathBuf;

/// Dossier des enregistrements dans `recorder.conf`.
pub const CLE_CAPTURES: &str = "dossier";
/// Dossier des captures d'écran dans `recorder.conf`.
pub const CLE_SHOTS: &str = "dossier_screenshot";

/// Fichier partagé avec `gpad-rec`, `grec` et Gablue Manager.
pub fn chemin() -> PathBuf {
    if let Some(explicit) = std::env::var_os("GLAUNCHER_RECORDER_CONF") {
        return PathBuf::from(explicit);
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    xdg.join("gablue").join("recorder.conf")
}

/// Défaut **affiché** des enregistrements — jamais écrit dans le fichier.
/// Le démon applique le même chemin (`config_defaut` de `gpad-rec`).
pub fn defaut_captures() -> String {
    home().join("Vidéos").join("Captures").display().to_string()
}

/// Défaut **affiché** des captures d'écran (`SCREENSHOT_DIR` de
/// `screenshot.sh`, absent du fichier).
pub fn defaut_shots() -> String {
    home().join("Images").join("Screenshots").display().to_string()
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Dossier enregistré pour cette clé ; `None` = défaut (clé absente ou vide).
/// La dernière occurrence gagne, comme `grec` (`tail -1`).
pub fn dossier(cle: &str) -> Option<String> {
    let texte = std::fs::read_to_string(chemin()).unwrap_or_default();
    valeur(&texte, cle)
}

fn valeur(texte: &str, cle: &str) -> Option<String> {
    texte
        .lines()
        .filter_map(|ligne| {
            let corps = ligne.trim();
            let (nom, reste) = corps.split_once('=')?;
            (nom.trim() == cle).then(|| reste.trim().to_string())
        })
        .next_back()
        .filter(|valeur| !valeur.is_empty())
}

/// Nouveau contenu du fichier : la clé prend `valeur`, ou **disparaît** quand
/// `valeur` est vide/absente (retour au défaut). Tout le reste est préservé.
fn reecrire(texte: &str, cle: &str, valeur: Option<&str>) -> String {
    let valeur = valeur.map(str::trim).filter(|valeur| !valeur.is_empty());
    let mut lignes: Vec<String> = Vec::new();
    let mut posee = false;
    for ligne in texte.lines() {
        let corps = ligne.trim();
        let nom = corps.split_once('=').map(|(nom, _)| nom.trim());
        if nom == Some(cle) {
            // une seule occurrence : les doublons éventuels disparaissent
            if let Some(valeur) = valeur
                && !posee
            {
                lignes.push(format!("{cle}={valeur}"));
                posee = true;
            }
            continue;
        }
        lignes.push(ligne.to_string());
    }
    if let Some(valeur) = valeur
        && !posee
    {
        if lignes.is_empty() {
            lignes.push("# réglages de gpad-rec et des captures d'écran".to_string());
        }
        lignes.push(format!("{cle}={valeur}"));
    }
    format!("{}\n", lignes.join("\n"))
}

/// Écrit (ou retire, si `valeur` est vide) une clé de dossier. Crée le dossier
/// de configuration au besoin, écrit en temporaire + renommage : le démon lit
/// ce fichier en continu et ne doit jamais voir un état à moitié écrit.
pub fn definir(cle: &str, valeur: Option<&str>) -> Result<(), String> {
    let path = chemin();
    let texte = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = reecrire(&texte, cle, valeur);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{} : {err}", parent.display()))?;
    }
    let tmp = path.with_extension("conf.tmp");
    std::fs::write(&tmp, updated).map_err(|err| format!("{} : {err}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|err| format!("{} : {err}", path.display()))
}

/// Sépare le défaut de la valeur réelle pour l'affichage : `(valeur ou défaut,
/// est_le_défaut)`.
pub fn affichage(valeur: Option<String>, defaut: String) -> (String, bool) {
    match valeur {
        Some(valeur) => (valeur, false),
        None => (defaut, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "\
# réglages de gpad-rec (écrit par Gablue Manager)
codec=h264
qualite=3
dossier=/home/gab/Vidéos/Captures

# le reste appartient au compte
auto_replay=0
";

    #[test]
    fn lecture_de_la_derniere_valeur() {
        assert_eq!(
            valeur(BASE, CLE_CAPTURES),
            Some("/home/gab/Vidéos/Captures".to_string())
        );
        assert_eq!(valeur(BASE, CLE_SHOTS), None);
        assert_eq!(valeur("dossier=/a\ndossier=/b\n", CLE_CAPTURES), Some("/b".to_string()));
        // valeur vide = défaut, pas un dossier racine
        assert_eq!(valeur("dossier=\n", CLE_CAPTURES), None);
    }

    #[test]
    fn remplacement_conserve_le_reste() {
        let sortie = reecrire(BASE, CLE_CAPTURES, Some("/mnt/t5/Captures"));
        assert!(sortie.contains("codec=h264"));
        assert!(sortie.contains("# le reste appartient au compte"));
        assert!(sortie.contains("auto_replay=0"));
        assert!(sortie.contains("dossier=/mnt/t5/Captures"));
        assert!(!sortie.contains("/home/gab/Vidéos/Captures"));
    }

    #[test]
    fn ajout_puis_retrait_pour_le_defaut() {
        let avec = reecrire(BASE, CLE_SHOTS, Some("/mnt/t5/Shots"));
        assert!(avec.contains("dossier_screenshot=/mnt/t5/Shots"));
        // retrait : la ligne disparaît, tout le reste reste
        let sans = reecrire(&avec, CLE_SHOTS, None);
        assert!(!sans.contains("dossier_screenshot"));
        assert!(sans.contains("dossier=/home/gab/Vidéos/Captures"));
        // retirer un dossier de captures rend le défaut au démon
        let defaut = reecrire(BASE, CLE_CAPTURES, None);
        assert!(!defaut.contains("dossier=/home/gab"));
        assert!(defaut.contains("qualite=3"));
    }

    #[test]
    fn doublons_reduits_a_une_ligne() {
        let texte = "dossier=/a\ndossier=/b\n";
        let sortie = reecrire(texte, CLE_CAPTURES, Some("/c"));
        assert_eq!(sortie.matches("dossier=").count(), 1);
        assert!(sortie.contains("dossier=/c"));
    }

    #[test]
    fn fichier_vide_garde_un_commentaire() {
        let sortie = reecrire("", CLE_CAPTURES, Some("/mnt/t5"));
        assert!(sortie.ends_with("dossier=/mnt/t5\n"));
        assert!(sortie.starts_with('#'));
    }
}
