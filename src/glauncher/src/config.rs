//! Configuration de glauncher : fichier TOML + options de ligne de commande.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::library;

/// Disposition de la bibliothèque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    /// Vignettes en grille.
    #[default]
    Grid,
    /// Lignes avec la vignette à gauche.
    List,
    /// Mosaïque façon Switch Online : covers collées, sans nom.
    Mosaic,
}

impl ViewMode {
    /// Disposition suivante (V / Tab / Y) : grille → liste → mosaïque.
    /// Les favoris ne sont pas une disposition mais un tri (menu `T`).
    pub fn flipped(self) -> Self {
        match self {
            ViewMode::Grid => ViewMode::List,
            ViewMode::List => ViewMode::Mosaic,
            ViewMode::Mosaic => ViewMode::Grid,
        }
    }
}

/// Configuration effective du launcher.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Démarre en plein écran (le mode normal sur Gablue).
    pub fullscreen: bool,
    /// Nombre de colonnes de la grille de jeux.
    pub columns: usize,
    /// Largeur d'une vignette en pixels (à régler selon la TV / l'écran).
    pub cell_size: u32,
    /// Affichage : `grid` (vignettes) ou `list` (lignes avec vignette).
    pub view: ViewMode,
    /// Dossiers scannés à la recherche de `.wgp` / `.lgp`.
    pub game_dirs: Vec<String>,
    /// Profondeur maximale du parcours (0 = illimitée).
    pub scan_depth: usize,
    /// Dossiers ignorés par leur nom exact (données de jeu, préfixes Wine…).
    pub scan_skip: Vec<String>,
    /// Dossiers où chercher l'icône d'un jeu (fichier `<slug>.png`).
    pub icon_dirs: Vec<String>,
    /// Racine du cache (`icons/` et `covers/`) : rien n'est jamais écrit dans
    /// les dossiers de jeux, qui peuvent être en lecture seule.
    pub cache_dir: String,
    /// Télécharge les covers manquantes (SteamGridDB) à la volée.
    pub covers: bool,
    /// Musique de fond du launcher (piste embarquée, jouée par `mpv` en
    /// boucle) : elle s'efface en fondu au lancement d'un jeu et revient au
    /// retour. Coupée pendant une partie, dans tous les cas.
    pub music: bool,
    /// Volume de la musique de fond, en pourcentage (0 = muet, 100 = maximum).
    /// Réglé dans Gablue Manager → Lanceur de jeux.
    pub music_volume: u8,
    /// Poussière d'étoiles du fond (particules scintillantes, façon PS3) :
    /// animée à 20 Hz seulement quand le launcher est à l'écran, coupée
    /// autrement (voir `src/sparkles.rs`).
    pub sparkles: bool,
    /// Clé API SteamGridDB v2 (facultative ; sinon API publique sans compte).
    pub steamgriddb_key: String,
    /// Commande de lancement des paquets Windows.
    pub launch_wgp: String,
    /// Commande de lancement des paquets Linux.
    pub launch_lgp: String,
    /// Remplace toute commande de lancement (utilisé par `--simulate`).
    #[serde(skip)]
    pub launch_override: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fullscreen: true,
            // 6 colonnes par défaut : la grille respire, le fond (poussière
            // d'étoiles) reste visible entre les boîtes (demandé le 22/11 —
            // à 5, les covers mangeaient l'écran).
            columns: 6,
            cell_size: 220,
            view: ViewMode::default(),
            // Aucun dossier par défaut : le launcher ne scanne QUE ce que
            // `config.toml` nomme. Liste vide = aucune bibliothèque, et c'est
            // voulu (le manager écrit les dossiers choisis par l'utilisateur).
            game_dirs: Vec::new(),
            scan_depth: 3,
            scan_skip: vec![
                "Prefixes".into(),
                "drive_c".into(),
                "dosdevices".into(),
                "Content".into(),
                "Saved".into(),
                "Config".into(),
                "Engine".into(),
                "Binaries".into(),
                "node_modules".into(),
            ],
            icon_dirs: vec![
                "~/.local/share/glauncher/icons".into(),
                "~/.local/share/icons".into(),
            ],
            cache_dir: "~/.local/share/glauncher".into(),
            covers: true,
            music: true,
            music_volume: crate::music::DEFAULT_VOLUME,
            sparkles: true,
            steamgriddb_key: String::new(),
            launch_wgp: "gwine".into(),
            launch_lgp: "lgplaunch".into(),
            launch_override: None,
        }
    }
}

impl Config {
    /// Lit un fichier TOML ; les clés absentes gardent leur valeur par défaut.
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw =
            std::fs::read_to_string(path).map_err(|err| format!("{} : {err}", path.display()))?;
        toml::from_str(&raw).map_err(|err| format!("{} : {err}", path.display()))
    }

    /// Emplacement du fichier de config : `--config`, puis `$GLAUNCHER_CONFIG`,
    /// puis `./config.toml` (pratique en dev), puis la config utilisateur XDG.
    pub fn locate(explicit: Option<&Path>) -> Option<PathBuf> {
        if let Some(path) = explicit {
            return Some(path.to_path_buf());
        }
        if let Ok(path) = std::env::var("GLAUNCHER_CONFIG") {
            return Some(PathBuf::from(path));
        }
        let local = PathBuf::from("config.toml");
        if local.is_file() {
            return Some(local);
        }
        let xdg = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        let user = xdg.join("glauncher").join("config.toml");
        user.is_file().then_some(user)
    }

    /// Fichier à **écrire** quand les dossiers de jeux changent depuis le
    /// launcher : celui qui est lu s'il existe, sinon l'emplacement XDG (créé
    /// à la première écriture). Un `--config` ou `$GLAUNCHER_CONFIG` explicite
    /// reste prioritaire : c'est le fichier que l'utilisateur a demandé.
    pub fn write_path(explicit: Option<&Path>) -> PathBuf {
        if let Some(path) = Self::locate(explicit) {
            return path;
        }
        let xdg = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        xdg.join("glauncher").join("config.toml")
    }

    /// Profondeur 0 = sans limite.
    pub fn depth_limit(&self) -> Option<usize> {
        (self.scan_depth > 0).then_some(self.scan_depth)
    }

    pub fn skip_names(&self) -> std::collections::HashSet<String> {
        self.scan_skip.iter().cloned().collect()
    }

    pub fn game_dirs(&self) -> Vec<PathBuf> {
        self.game_dirs.iter().map(|dir| expand(dir)).collect()
    }

    pub fn icon_dirs(&self) -> Vec<PathBuf> {
        self.icon_dirs.iter().map(|dir| expand(dir)).collect()
    }

    pub fn cache_dir(&self) -> PathBuf {
        expand(&self.cache_dir)
    }

    /// Un index par jeu de dossiers : un test avec `--dir demo-games` ne doit
    /// pas écraser l'index des vrais jeux (et inversement).
    pub fn index_path(&self) -> PathBuf {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for dir in self.game_dirs() {
            for byte in dir.to_string_lossy().bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
            hash ^= b'/' as u64;
        }
        self.cache_dir().join(format!("index-{hash:016x}.sqlite"))
    }

    /// Icônes de paquets copiées dans le cache.
    pub fn icon_cache_dir(&self) -> PathBuf {
        self.assets_dir().join("icons")
    }

    /// Liste des favoris : un slug par ligne (`~/.local/share/glauncher/
    /// favorites.txt`). Elle vit dans le cache, pas dans les dossiers de jeux.
    pub fn favorites_path(&self) -> PathBuf {
        self.cache_dir().join("favorites.txt")
    }

    /// Ancien emplacement du temps de jeu (`<cache>/playtime.txt`) : il est
    /// **migré** au démarrage vers les dossiers de sauvegardes (voir
    /// [`Config::playtime_path`]). Conservé pour les jeux momentanément
    /// absents de la liste (disque débranché).
    pub fn legacy_playtime_path(&self) -> PathBuf {
        self.cache_dir().join("playtime.txt")
    }

    /// Temps de jeu cumulé par jeu (`slug<TAB>secondes`), dans un dossier
    /// dédié **aux côtés des sauvegardes** : `lgp-saves/glauncher/playtime.txt`
    /// pour les `.lgp` (même racine que `lgplaunch`, souvent synchronisée) et
    /// `LocalSavesWGP/glauncher/playtime.txt` pour les `.wgp` (même racine que
    /// `gwine`). Le temps suit donc les sauvegardes au lieu de vivre dans le
    /// cache du launcher, qui peut être vidé.
    pub fn playtime_path(&self, kind: &str) -> PathBuf {
        let root = if kind == library::Kind::Lgp.as_str() {
            self.lgp_saves_dir()
        } else {
            self.wgp_saves_dir()
        };
        root.join("glauncher").join("playtime.txt")
    }

    /// Racine des sauvegardes `.lgp` (`~/.local/share/lgp-saves`, la même que
    /// `lgplaunch` ; sur cette machine c'est un lien vers Syncthing).
    pub fn lgp_saves_dir(&self) -> PathBuf {
        home_dir().join(".local/share/lgp-saves")
    }

    /// Racine des sauvegardes `.wgp` (`~/Windows/UserData/<user>/
    /// LocalSavesWGP`, la même que `gwine`).
    pub fn wgp_saves_dir(&self) -> PathBuf {
        let home = home_dir();
        let user = std::env::var("USER")
            .ok()
            .filter(|user| !user.is_empty())
            .or_else(|| {
                home.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .unwrap_or_default();
        home.join("Windows/UserData")
            .join(user)
            .join("LocalSavesWGP")
    }

    /// Dernier type de vue et dernier tri choisis (`clé=valeur`).
    pub fn state_path(&self) -> PathBuf {
        self.cache_dir().join("state.txt")
    }

    /// Racine de TOUS les visuels téléchargés ou extraits : `assets/` regroupe
    /// grilles, héros, logos et icônes sous `~/.local/share/glauncher`.
    pub fn assets_dir(&self) -> PathBuf {
        self.cache_dir().join("assets")
    }

    /// Grilles (covers) SteamGridDB.
    pub fn cover_cache_dir(&self) -> PathBuf {
        self.assets_dir().join("grids")
    }

    /// Bannières (héros) SteamGridDB.
    pub fn hero_cache_dir(&self) -> PathBuf {
        self.assets_dir().join("heroes")
    }

    /// Logos SteamGridDB.
    pub fn logo_cache_dir(&self) -> PathBuf {
        self.assets_dir().join("logos")
    }

    /// Miniatures des covers (320 px de large) : c'est ce que la grille
    /// affiche, la cover pleine restant pour la page et l'aperçu. **Hors de
    /// `assets/`** : dérivées des covers, régénérées à chaque changement de
    /// réglage, elles n'ont rien à faire dans un dossier partagé.
    pub fn thumb_dir(&self) -> PathBuf {
        self.cache_dir().join("thumbs")
    }

    /// Icônes extraites des paquets. **Hors de `assets/`** : elles viennent des
    /// paquets de CETTE machine, pas de SteamGridDB.
    pub fn pack_icon_dir(&self) -> PathBuf {
        self.cache_dir().join("pack-icons")
    }

    /// Marqueurs `.miss` (introuvable) et `.choice` (choix manuel), par type de
    /// visuel. **Hors de `assets/`** : ils sont propres à une machine et à une
    /// session, et un `.miss` synchronisé masquerait un visuel pendant 30 jours
    /// sur l'autre machine.
    pub fn marks_dir(&self, kind: &str) -> PathBuf {
        self.cache_dir().join("marks").join(kind)
    }

    /// Range l'ancienne disposition (`covers/`, `icons/` à la racine du cache)
    /// sous `assets/`. Sans ça, les covers déjà choisies seraient perdues.
    pub fn migrate_cache(&self) {
        let cache = self.cache_dir();
        let assets = self.assets_dir();
        if !assets.is_dir() {
            let _ = std::fs::create_dir_all(&assets);
            for (old, new) in [("covers", "grids"), ("icons", "icons")] {
                let from = cache.join(old);
                if from.is_dir() {
                    let _ = std::fs::rename(&from, assets.join(new));
                }
            }
        }

        // `assets/` ne doit contenir QUE des images SteamGridDB : c'est le
        // dossier destiné au partage (Syncthing). Miniatures, icônes de paquets
        // et marqueurs en sortent ici, une fois pour toutes.
        for (from, to) in [
            (assets.join("thumbs"), cache.join("thumbs")),
            (assets.join("icons").join("pack"), cache.join("pack-icons")),
        ] {
            move_tree(&from, &to);
        }
        for kind in [ART_GRIDS, ART_HEROES, ART_LOGOS, ART_ICONS] {
            let dir = assets.join(kind);
            let marks = cache.join("marks").join(kind);
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !(name.ends_with(".miss") || name.ends_with(".choice")) {
                    continue;
                }
                let _ = std::fs::create_dir_all(&marks);
                let _ = std::fs::rename(&path, marks.join(name));
            }
        }
    }

    /// Clé API : config, sinon `SGDB_API_KEY`.
    pub fn steamgriddb_key(&self) -> Option<String> {
        if !self.steamgriddb_key.trim().is_empty() {
            return Some(self.steamgriddb_key.clone());
        }
        std::env::var("SGDB_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
    }
}

/// Sous-dossiers de `assets/`.
const ART_GRIDS: &str = "grids";
const ART_HEROES: &str = "heroes";
const ART_LOGOS: &str = "logos";
const ART_ICONS: &str = "icons";

/// Déplace un dossier (ou son contenu s'il existe déjà) sans jamais écraser.
fn move_tree(from: &std::path::Path, to: &std::path::Path) {
    if !from.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(to);
    for entry in std::fs::read_dir(from).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        let target = to.join(name);
        if !target.exists() {
            let _ = std::fs::rename(&path, &target);
        }
    }
    let _ = std::fs::remove_dir(from);
}

/// Dossier personnel (même source que `~` dans les chemins de config).
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Remplace un `~` initial par le dossier personnel.
pub fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// Options de ligne de commande, appliquées après le fichier de config.
#[derive(Debug, Default)]
pub struct Options {
    pub config: Option<PathBuf>,
    pub windowed: bool,
    pub dirs: Vec<PathBuf>,
    pub columns: Option<usize>,
    pub cell_size: Option<u32>,
    pub simulate: Option<u64>,
    /// Lance directement le jeu n° N au démarrage (crochet de test).
    pub play: Option<usize>,
    pub no_covers: bool,
    /// Reconstruit l'index et sort (pas de fenêtre).
    pub reindex: bool,
    /// Télécharge les visuels manquants de tous les jeux, puis sort.
    pub update: bool,
    /// Avec `--update` (ou seul) : n'apprend que les années de sortie.
    pub dates: bool,
    /// Liste numérotée des jeux indexés, puis sort.
    pub list: bool,
    /// Récupère le logo et l'icône d'UN paquet (overlay manette), puis sort.
    pub art: Option<PathBuf>,
    /// Ramène tout le cache `assets/` au format et à la taille cibles, puis sort.
    pub shrink: bool,
    /// Ouvre le menu « choisir une image » au démarrage (crochet de test).
    pub picker: bool,
    /// Liste les périphériques d'entrée et leurs axes, puis sort.
    pub pads: bool,
    pub help: bool,
}

pub const USAGE: &str = "\
glauncher — launcher de jeux Gablue (.wgp / .lgp)

Usage : glauncher [options]

  --config <fichier>   fichier de config à utiliser
  --dir <dossier>      dossier de jeux (répétable, remplace la config)
  --columns <n>        nombre de colonnes de la grille
  --cell <pixels>      largeur d'une vignette
  --windowed           fenêtre au lieu du plein écran
  --simulate [secondes]
                       lance un faux jeu (sleep) pour tester l'écran noir
  --play <n>           lance le jeu n° n (1 = premier) dès le démarrage
  --no-covers          ne télécharge aucune cover (mode hors ligne)
  --reindex            reconstruit l'index des dossiers puis quitte
  --update             télécharge les visuels manquants (covers, héros,
                       logos, icônes) de TOUS les jeux, puis quitte
  --date               n'apprend que les années de sortie manquantes
                       (avec --update, ou seul), puis quitte
  --list               liste numérotée des jeux (pour --play), puis quitte
  --art <paquet>       récupère le logo et l'icône d'un seul paquet pour
                       l'overlay manette (PNG dans le cache), puis quitte
  --shrink             allège le cache (héros 1920 px en JPEG, grilles
                       600x900, logos 1000 px, icônes 256 px), puis quitte
  --picker             ouvre le menu de choix d'image au démarrage
  --pads               liste les manettes et leurs axes (diagnostic), puis quitte
  --help               cette aide

Variables d'environnement :
  GLAUNCHER_CONFIG         idem --config
  GLAUNCHER_LAUNCH_CMD     remplace la commande de lancement (ex : \"sleep 5\")
";

impl Options {
    pub fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut opts = Self::default();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            index += 1;
            match arg {
                "--config" => {
                    opts.config = Some(PathBuf::from(
                        args.get(index).ok_or("--config attend un fichier")?,
                    ));
                    index += 1;
                }
                "--dir" => {
                    opts.dirs.push(PathBuf::from(
                        args.get(index).ok_or("--dir attend un dossier")?,
                    ));
                    index += 1;
                }
                "--columns" => {
                    let value = args.get(index).ok_or("--columns attend un nombre")?;
                    let columns: usize = value
                        .parse()
                        .map_err(|_| format!("--columns : {value} n'est pas un nombre"))?;
                    if columns == 0 {
                        return Err("--columns doit être ≥ 1".into());
                    }
                    opts.columns = Some(columns);
                    index += 1;
                }
                "--cell" => {
                    let value = args.get(index).ok_or("--cell attend un nombre de pixels")?;
                    let cell: u32 = value
                        .parse()
                        .map_err(|_| format!("--cell : {value} n'est pas un nombre"))?;
                    if cell < 80 {
                        return Err("--cell doit être ≥ 80".into());
                    }
                    opts.cell_size = Some(cell);
                    index += 1;
                }
                "--windowed" => opts.windowed = true,
                "--simulate" => {
                    // argument optionnel : on ne consomme le suivant que si c'est un nombre
                    let mut seconds = 5;
                    if let Some(value) = args.get(index).and_then(|arg| arg.parse::<u64>().ok()) {
                        seconds = value;
                        index += 1;
                    }
                    opts.simulate = Some(seconds);
                }
                "--play" => {
                    let value = args.get(index).ok_or("--play attend un numéro")?;
                    let number: usize = value
                        .parse()
                        .map_err(|_| format!("--play : {value} n'est pas un nombre"))?;
                    if number == 0 {
                        return Err("--play : la numérotation commence à 1".into());
                    }
                    opts.play = Some(number - 1);
                    index += 1;
                }
                "--no-covers" => opts.no_covers = true,
                "--reindex" => opts.reindex = true,
                "--update" => opts.update = true,
                "--date" => opts.dates = true,
                "--list" => opts.list = true,
                "--shrink" => opts.shrink = true,
                "--art" => {
                    opts.art = Some(PathBuf::from(
                        args.get(index)
                            .ok_or("--art attend un paquet (.wgp/.lgp)")?,
                    ));
                    index += 1;
                }
                "--picker" => opts.picker = true,
                "--pads" => opts.pads = true,
                "--help" | "-h" => opts.help = true,
                other => return Err(format!("option inconnue : {other}")),
            }
        }
        Ok(opts)
    }
}

/// Construit la configuration finale (fichier + ligne de commande).
pub fn resolve(opts: &Options) -> (Config, Option<String>) {
    let mut config = match Config::locate(opts.config.as_deref()) {
        Some(path) => match Config::load(&path) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("glauncher : config ignorée ({err})");
                Config::default()
            }
        },
        None => Config::default(),
    };
    if opts.windowed {
        config.fullscreen = false;
    }
    if !opts.dirs.is_empty() {
        config.game_dirs = opts
            .dirs
            .iter()
            .map(|dir| dir.display().to_string())
            .collect();
    }
    if let Some(columns) = opts.columns {
        config.columns = columns;
    }
    if let Some(cell_size) = opts.cell_size {
        config.cell_size = cell_size;
    }
    if opts.no_covers {
        config.covers = false;
    }
    if config.columns == 0 {
        config.columns = 6;
    }
    config.cell_size = config.cell_size.clamp(80, 640);
    let notice = opts
        .simulate
        .map(|seconds| format!("mode test : faux jeu de {seconds} s"));
    (config, notice)
}

/// Écrit la liste des dossiers de jeux dans `config.toml`, **sans toucher au
/// reste du fichier** : commentaires, ordre et clés inconnues sont conservés.
///
/// C'est le même contrat que l'écrivain de Gablue Manager
/// (`write_glauncher_config` en Python) : deux écrivains sur un fichier de
/// compte doivent être interchangeables. Réécrire tout le TOML depuis
/// `Config` perdrait les commentaires de l'utilisateur et toutes les clés que
/// cette version ne connaît pas.
pub fn save_game_dirs(path: &Path, dirs: &[String]) -> Result<(), String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let updated = rewrite_game_dirs(&text, dirs);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{} : {err}", parent.display()))?;
    }
    // temporaire + rename : le manager lit ce fichier, il ne doit jamais voir
    // un état à moitié écrit
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, updated).map_err(|err| format!("{} : {err}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|err| format!("{} : {err}", path.display()))
}

/// Valeur TOML d'un tableau de chaînes, sur une ligne.
fn toml_list(dirs: &[String]) -> String {
    let items: Vec<String> = dirs
        .iter()
        .map(|dir| {
            let escaped = dir.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect();
    format!("[{}]", items.join(", "))
}

/// Clé d'une ligne `clé = valeur` ; `None` pour un commentaire ou du bruit.
fn assignment_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    (!key.is_empty()).then_some(key)
}

/// Vrai tant qu'un tableau TOML n'est pas refermé (valeur sur plusieurs lignes).
fn brackets_open(text: &str) -> bool {
    text.matches('[').count() > text.matches(']').count()
}

/// Contenu de `config.toml` avec `game_dirs` remplacé (ou ajouté).
pub fn rewrite_game_dirs(text: &str, dirs: &[String]) -> String {
    let replacement = format!("game_dirs = {}", toml_list(dirs));
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut done = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if assignment_key(line) != Some("game_dirs") || done {
            out.push(line.to_string());
            continue;
        }
        // une valeur peut tenir sur plusieurs lignes (tableau) : on avale
        // jusqu'à ce que les crochets soient équilibrés
        let mut block = line.to_string();
        while brackets_open(&block) && index < lines.len() {
            block.push('\n');
            block.push_str(lines[index]);
            index += 1;
        }
        out.push(replacement.clone());
        done = true;
    }
    if !done {
        // clé absente : en fin de clés de tête, avant la première section
        let insert_at = out
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .unwrap_or(out.len());
        let mut block = vec![replacement, String::new()];
        if insert_at > 0 && !out[insert_at - 1].trim().is_empty() {
            block.insert(0, String::new());
        }
        out.splice(insert_at..insert_at, block);
    }
    let mut result = out.join("\n").trim_end_matches('\n').to_string();
    result.push('\n');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn remplace_la_ligne_en_gardant_le_reste() {
        let text = "\
# mes dossiers
game_dirs = [\"~/Games\"]
fullscreen = true

[section inconnue]
x = 1
";
        let out = rewrite_game_dirs(text, &dirs(&["~/Jeux", "/run/media/gab/SSD/Roms"]));
        assert!(out.starts_with("# mes dossiers\n"));
        assert!(out.contains("game_dirs = [\"~/Jeux\", \"/run/media/gab/SSD/Roms\"]"));
        assert!(out.contains("fullscreen = true"));
        assert!(out.contains("[section inconnue]\nx = 1"));
        assert!(!out.contains("~/Games"));
    }

    #[test]
    fn tableau_multiligne_avale() {
        let text = "game_dirs = [\n  \"~/A\",\n  \"~/B\",\n]\nfullscreen = false\n";
        let out = rewrite_game_dirs(text, &dirs(&["~/C"]));
        assert_eq!(out, "game_dirs = [\"~/C\"]\nfullscreen = false\n");
    }

    #[test]
    fn cle_absente_ajoutee_avant_une_section() {
        let text = "fullscreen = true\n\n[autre]\ny = 2\n";
        let out = rewrite_game_dirs(text, &dirs(&["~/J"]));
        assert!(out.contains("fullscreen = true\n\ngame_dirs = [\"~/J\"]\n\n[autre]"));
    }

    #[test]
    fn cle_absente_sans_ligne_vide() {
        let text = "fullscreen = true\n[autre]\ny = 2\n";
        let out = rewrite_game_dirs(text, &dirs(&["~/J"]));
        assert_eq!(
            out,
            "fullscreen = true\n\ngame_dirs = [\"~/J\"]\n\n[autre]\ny = 2\n"
        );
    }

    #[test]
    fn fichier_vide_et_guillemets() {
        assert_eq!(rewrite_game_dirs("", &dirs(&[])), "game_dirs = []\n");
        let out = rewrite_game_dirs("", &dirs(&["/tmp/a\"b"]));
        assert_eq!(out, "game_dirs = [\"/tmp/a\\\"b\"]\n");
    }

    #[test]
    fn cle_similaire_intacte() {
        let text = "scan_skip = [\"a\"]\ngame_dirs = []\n";
        let out = rewrite_game_dirs(text, &dirs(&["~/G"]));
        assert_eq!(out, "scan_skip = [\"a\"]\ngame_dirs = [\"~/G\"]\n");
    }

    #[test]
    fn ecriture_sur_disque() {
        let base = std::env::temp_dir().join(format!("glauncher-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let path = base.join("glauncher").join("config.toml");
        save_game_dirs(&path, &dirs(&["~/Games"])).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "game_dirs = [\"~/Games\"]\n"
        );
        // les commentaires d'un fichier existant survivent
        std::fs::write(&path, "# garde-moi\ngame_dirs = [\"~/Games\"]\n").unwrap();
        save_game_dirs(&path, &dirs(&["~/A", "~/B"])).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# garde-moi\ngame_dirs = [\"~/A\", \"~/B\"]\n"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
