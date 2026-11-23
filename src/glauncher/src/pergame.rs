//! Options de lancement **propres à un jeu**, et propres à la machine.
//!
//! Elles vivent dans `~/.local/share/glauncher/games.txt`, **jamais** dans
//! `assets/` : ce dossier est partagé par Syncthing (visuels, années,
//! identifications), alors que « ce jeu se lance dans gamescope en 900p » est un
//! réglage du poste — une TV 2K n'appelle pas les mêmes réglages qu'un
//! portable, et un ami ne doit pas hériter des miens.
//!
//! Format : une ligne par jeu, `<slug><TAB>gamescope=1<TAB>res=1080`. Le slug
//! est celui du paquet (comme le temps de jeu), donc stable tant qu'on ne
//! renomme pas le fichier. Écriture **atomique** et **fusion** : le fichier est
//! relu avant d'être réécrit, une ligne inconnue (version future, jeu d'une
//! autre session) est conservée.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Cycle des résolutions proposées au pad, en hauteur (16:9) : `None` = celle
/// du jeu / de l'écran (on ne force rien), puis du plus grand au plus petit.
pub const RESOLUTIONS: [Option<u32>; 5] = [None, Some(1080), Some(900), Some(720), Some(480)];

/// Réglages de lancement d'un jeu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Lancer dans `gamescope` (compositeur imbriqué) plutôt qu'en direct.
    pub gamescope: bool,
    /// Hauteur forcée (1080, 900, 720, 480), `None` = sans forçage.
    pub resolution: Option<u32>,
    /// `.wgp` seulement : manettes présentées en mode Xbox (`gwine --xbox`),
    /// **toutes** les manettes, sans distinction de modèle.
    pub xbox: bool,
}

impl Options {
    /// Vrai si rien n'est réglé : la ligne peut disparaître du fichier.
    pub fn est_defaut(&self) -> bool {
        !self.gamescope && self.resolution.is_none() && !self.xbox
    }

    /// Arguments ajoutés à `gwine` / `lgplaunch`.
    ///
    /// Les deux lanceurs comprennent `--gamescope` et `--resolution <h>` : la
    /// launcher n'a donc qu'une seule forme à connaître, quel que soit le
    /// format du paquet.
    pub fn arguments(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.gamescope {
            args.push("--gamescope".to_string());
        }
        if let Some(hauteur) = self.resolution {
            args.push("--resolution".to_string());
            args.push(hauteur.to_string());
        }
        // `gwine --xbox` : filtre « all », donc toutes les manettes
        if self.xbox {
            args.push("--xbox".to_string());
        }
        args
    }

    /// Passe à la résolution suivante du cycle (Croix sur la ligne).
    pub fn resolution_suivante(&mut self) {
        let rang = RESOLUTIONS
            .iter()
            .position(|hauteur| *hauteur == self.resolution)
            .unwrap_or(0);
        self.resolution = RESOLUTIONS[(rang + 1) % RESOLUTIONS.len()];
    }

    /// Libellé de la résolution (« 1080p », « pareil »).
    pub fn resolution_label(&self) -> String {
        match self.resolution {
            Some(hauteur) => format!("{hauteur}p"),
            None => "celle du jeu".to_string(),
        }
    }
}

/// Fichier des options, dans `XDG_DATA_HOME` (donc **hors** du dossier partagé).
pub fn chemin() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("glauncher").join("games.txt")
}

/// Lit les options d'un jeu. Fichier absent ou illisible : défauts (aucune
/// erreur visible, un réglage absent n'est pas un problème).
pub fn lire(slug: &str) -> Options {
    lire_dans(&chemin(), slug)
}

/// Écrit les options d'un jeu (fusion avec le reste du fichier).
pub fn ecrire(slug: &str, options: Options) -> Result<(), String> {
    ecrire_dans(&chemin(), slug, options)
}

pub fn lire_dans(path: &Path, slug: &str) -> Options {
    let Ok(texte) = std::fs::read_to_string(path) else {
        return Options::default();
    };
    parse(&texte).get(slug).copied().unwrap_or_default()
}

pub fn ecrire_dans(path: &Path, slug: &str, options: Options) -> Result<(), String> {
    let texte = std::fs::read_to_string(path).unwrap_or_default();
    let mut table = parse(&texte);
    if options.est_defaut() {
        table.remove(slug);
    } else {
        table.insert(slug.to_string(), options);
    }
    ecrire_table(path, &table)
}

/// Retire les réglages des jeux qui ne sont plus présents. Renvoie le nombre de
/// lignes retirées. Sert à « Oublier les jeux disparus ».
pub fn oublier(garde: &std::collections::HashSet<String>) -> usize {
    oublier_dans(&chemin(), garde)
}

/// Même chose sur un fichier donné (banc d'essai).
pub fn oublier_dans(path: &Path, garde: &std::collections::HashSet<String>) -> usize {
    let Ok(texte) = std::fs::read_to_string(path) else {
        return 0;
    };
    let mut table = parse(&texte);
    let avant = table.len();
    table.retain(|slug, _| garde.contains(slug));
    let retires = avant - table.len();
    if retires > 0 {
        let _ = ecrire_table(path, &table);
    }
    retires
}

/// Réécrit tout le fichier (écriture atomique).
fn ecrire_table(path: &Path, table: &BTreeMap<String, Options>) -> Result<(), String> {
    let mut sortie = String::new();
    for (cle, options) in table {
        sortie.push_str(cle);
        if options.gamescope {
            sortie.push_str("\tgamescope=1");
        }
        if let Some(hauteur) = options.resolution {
            sortie.push_str(&format!("\tres={hauteur}"));
        }
        if options.xbox {
            sortie.push_str("\txbox=1");
        }
        sortie.push('\n');
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{} : {err}", parent.display()))?;
    }
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, sortie).map_err(|err| format!("{} : {err}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|err| format!("{} : {err}", path.display()))
}

/// Parse pur du fichier : une ligne mal formée est ignorée, pas fatale.
pub fn parse(texte: &str) -> BTreeMap<String, Options> {
    let mut table = BTreeMap::new();
    for ligne in texte.lines() {
        let mut champs = ligne.split('\t');
        let Some(slug) = champs.next().map(str::trim).filter(|slug| !slug.is_empty()) else {
            continue;
        };
        let mut options = Options::default();
        // une ligne sans aucun champ reconnu n'est pas un réglage : la garder
        // ferait réapparaître des lignes vides au fil des réécritures
        let mut reconnu = false;
        for champ in champs {
            if let Some(valeur) = champ.strip_prefix("gamescope=") {
                options.gamescope = valeur.trim() == "1" || valeur.trim() == "true";
                reconnu = true;
            } else if let Some(valeur) = champ.strip_prefix("res=") {
                options.resolution = valeur.trim().parse::<u32>().ok().filter(|h| *h > 0);
                reconnu = true;
            } else if let Some(valeur) = champ.strip_prefix("xbox=") {
                options.xbox = valeur.trim() == "1" || valeur.trim() == "true";
                reconnu = true;
            }
        }
        if reconnu {
            table.insert(slug.to_string(), options);
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oublier_retire_les_jeux_absents() {
        let base =
            std::env::temp_dir().join(format!("glauncher-pg-oublier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let path = base.join("games.txt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "mario\tgamescope=1\nzelda\tres=720\n").unwrap();
        let garde: std::collections::HashSet<String> = ["mario".to_string()].into();
        assert_eq!(oublier_dans(&path, &garde), 1);
        let texte = std::fs::read_to_string(&path).unwrap();
        assert!(texte.contains("mario"));
        assert!(!texte.contains("zelda"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn aller_retour_et_fusion() {
        let base = std::env::temp_dir().join(format!("glauncher-pg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let path = base.join("glauncher").join("games.txt");
        // une ligne « d'une autre session » ne doit pas disparaître
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "autre\tgamescope=1\n").unwrap();

        ecrire_dans(
            &path,
            "mario",
            Options {
                gamescope: true,
                resolution: Some(900),
                xbox: true,
            },
        )
        .unwrap();
        assert_eq!(lire_dans(&path, "mario").resolution, Some(900));
        assert!(lire_dans(&path, "mario").gamescope);
        assert!(lire_dans(&path, "mario").xbox);
        assert!(lire_dans(&path, "autre").gamescope, "ligne conservée");

        ecrire_dans(
            &path,
            "mario",
            Options {
                gamescope: false,
                resolution: Some(720),
                xbox: false,
            },
        )
        .unwrap();
        assert!(!lire_dans(&path, "mario").gamescope);
        assert_eq!(lire_dans(&path, "mario").resolution, Some(720));

        // tout remettre par défaut retire la ligne, sans toucher aux autres
        ecrire_dans(&path, "mario", Options::default()).unwrap();
        assert_eq!(lire_dans(&path, "mario"), Options::default());
        let texte = std::fs::read_to_string(&path).unwrap();
        assert!(!texte.contains("mario"), "{texte}");
        assert!(texte.contains("autre"), "{texte}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn arguments_pour_les_deux_lanceurs() {
        let options = Options {
            gamescope: true,
            resolution: Some(1080),
            xbox: true,
        };
        assert_eq!(
            options.arguments(),
            ["--gamescope", "--resolution", "1080", "--xbox"]
        );
        assert_eq!(Options::default().arguments().len(), 0);
        assert_eq!(options.resolution_label(), "1080p");
        assert_eq!(Options::default().resolution_label(), "celle du jeu");
    }

    #[test]
    fn cycle_des_resolutions() {
        let mut options = Options::default();
        let mut vus = Vec::new();
        for _ in 0..RESOLUTIONS.len() {
            options.resolution_suivante();
            vus.push(options.resolution);
        }
        assert_eq!(
            vus,
            vec![Some(1080), Some(900), Some(720), Some(480), None],
            "le cycle revient au défaut"
        );
    }

    #[test]
    fn parse_tolerant() {
        let table = parse("a\tgamescope=1\tres=480\txbox=1\n\nligne-bidon\nb\tres=x\n");
        assert!(table["a"].gamescope);
        assert!(table["a"].xbox);
        assert_eq!(table["a"].resolution, Some(480));
        assert_eq!(table["b"].resolution, None, "res illisible ignorée");
        assert!(!table.contains_key("ligne-bidon"));
    }
}
