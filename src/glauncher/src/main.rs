//! glauncher — launcher de jeux Gablue (.wgp / .lgp).

mod app;
mod assets;
mod config;
mod covers;
mod cursor;
mod dialog;
mod favorites;
mod gamepad;
mod index;
mod kwin;
mod launch;
mod library;
mod marquee;
mod mode;
mod music;
mod playtime;
mod pose;
mod sparkles;
mod spinner;
mod state;
mod thumbs;
mod updates;
mod watch;

use iced::window;

/// Horodatage de démarrage : deux repères dans le journal (app prête, premier
/// rendu) suffisent à voir si quelque chose retarde l'affichage.
pub fn start() {
    let _ = START.set(std::time::Instant::now());
}

pub fn stamp(what: &str) {
    let elapsed = START.get().map(|start| start.elapsed()).unwrap_or_default();
    eprintln!("glauncher : {what} en {} ms", elapsed.as_millis());
}

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// `--reindex` : reconstruit l'index sans ouvrir de fenêtre.
fn reindex(config: &config::Config) -> iced::Result {
    let started = std::time::Instant::now();
    let mut index = match index::Index::open(&config.index_path()) {
        Ok(index) => index,
        Err(err) => {
            eprintln!("glauncher : {err}");
            std::process::exit(1);
        }
    };
    let snapshot = index.snapshot().unwrap_or_default();
    let walk = library::walk_packs(config, &snapshot);
    if let Err(err) = index.replace_all(&walk) {
        eprintln!("glauncher : {err}");
        std::process::exit(1);
    }

    println!(
        "{} paquets, {} dossiers en {} ms",
        walk.rows.len(),
        walk.dirs.len(),
        started.elapsed().as_millis()
    );
    Ok(())
}

/// `--update` (et `--date`) : complète ce qui manque pour tous les jeux
/// indexés — visuels, ou seulement les années de sortie.
///
/// Utile pour amorcer un cache neuf (ou celui d'une autre machine via le
/// dossier `assets/` partagé) sans parcourir la bibliothèque à la main.
fn update(config: &config::Config, dates_only: bool) -> iced::Result {
    let index = match index::Index::open(&config.index_path()) {
        Ok(index) => index,
        Err(err) => {
            eprintln!("glauncher : {err}");
            std::process::exit(1);
        }
    };
    let mut rows = match index.all() {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("glauncher : {err}");
            std::process::exit(1);
        }
    };
    rows.sort_by_cached_key(|row| row.name.to_lowercase());

    let games: Vec<(String, String, std::path::PathBuf)> = rows
        .iter()
        .map(|row| (row.slug(), row.name.clone(), row.path.clone()))
        .collect();
    eprintln!("glauncher : {} jeux à compléter…", games.len());

    // `--date` : uniquement les années, aucun visuel
    if dates_only {
        let noms: Vec<(String, String)> = games
            .iter()
            .map(|(slug, name, _)| (slug.clone(), name.clone()))
            .collect();
        let mut report = |line: String| println!("{line}");
        let totals = covers::update_years(
            &config.assets_dir(),
            config.steamgriddb_key(),
            &noms,
            &mut report,
        );
        println!(
            "{} année(s) apprise(s), {} déjà connues, {} erreur(s) — {}",
            totals.years,
            totals.skipped,
            totals.failed,
            covers::years_path(&config.assets_dir()).display()
        );
        return Ok(());
    }

    let marks = config.cache_dir().join("marks");
    // visuels déjà là renommés d'après le jeu SteamGridDB (voir App::boot)
    covers::migrate_names(&config.assets_dir(), &marks);
    let mut report = |line: String| println!("{line}");
    let totals = covers::update_all(
        &config.assets_dir(),
        &marks,
        &config.pack_icon_dir(),
        config.steamgriddb_key(),
        &games,
        &mut report,
    );

    // miniatures des covers : la grille s'en sert, autant les préparer ici
    let mut thumbs = 0;
    let identities = covers::load_identities(&covers::identities_path(&config.assets_dir()));
    for (slug, _, _) in &games {
        let Some(cover) = covers::cached_for(
            &config.cover_cache_dir(),
            identities.get(slug),
            slug,
        ) else {
            continue;
        };
        let job = thumbs::Job::new(slug.clone(), cover);
        if thumbs::build(&config.thumb_dir(), &job).is_ok() {
            thumbs += 1;
        }
    }

    println!(
        "{} téléchargés, {} introuvables, {} erreurs, {} déjà là, {} années — {thumbs} miniatures",
        totals.downloaded, totals.missing, totals.failed, totals.skipped, totals.years
    );
    Ok(())
}

/// `--list` : liste numérotée dans l'ordre du launcher (nom, sans extension).
fn list(config: &config::Config) -> iced::Result {
    let index = match index::Index::open(&config.index_path()) {
        Ok(index) => index,
        Err(err) => {
            eprintln!("glauncher : {err}");
            std::process::exit(1);
        }
    };
    let mut rows = match index.all() {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("glauncher : {err}");
            std::process::exit(1);
        }
    };
    rows.sort_by_cached_key(|row| row.name.to_lowercase());

    // écriture tolérante à un tube fermé (`| head`) : pas de panique
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for (position, row) in rows.iter().enumerate() {
        if writeln!(out, "{}\t{}", position + 1, row.path.display()).is_err() {
            break;
        }
    }
    Ok(())
}

/// `--art <paquet>` : récupère le logo et l'icône **d'un seul paquet** pour
/// l'overlay manette, puis sort.
///
/// Lancé en fond par gwine/lgplaunch au démarrage du jeu, quand le paquet n'est
/// pas passé par le launcher : gpad n'a alors ni logo (bandeau) ni icône
/// (en-tête) pour un jeu qui n'a jamais été scanné. Le téléchargement se fait
/// **ici**, jamais dans l'overlay ; gpad relit le cache et le visuel apparaît
/// dès qu'il est écrit.
fn art(config: &config::Config, pack: &std::path::Path) -> iced::Result {
    let Some(stem) = pack
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
    else {
        eprintln!("glauncher : --art attend un paquet (.wgp/.lgp)");
        std::process::exit(2);
    };
    let slug = library::slugify(&stem);
    config.migrate_cache();
    // visuels déjà là renommés d'après le jeu SteamGridDB (voir App::boot)
    covers::migrate_names(&config.assets_dir(), &config.cache_dir().join("marks"));
    let mut report = |line: String| eprintln!("glauncher : {line}");
    let totals = covers::fetch_game(
        &config.assets_dir(),
        &config.cache_dir().join("marks"),
        &config.pack_icon_dir(),
        config.steamgriddb_key(),
        &slug,
        &stem,
        pack,
        &mut report,
    );
    eprintln!(
        "glauncher : {slug} — {} téléchargé(s), {} introuvable(s), {} erreur(s), {} déjà là",
        totals.downloaded, totals.missing, totals.failed, totals.skipped
    );
    Ok(())
}

/// `--shrink` : ramène **tout le cache** au format et à la taille cibles.
///
/// Une passe unique après coup : les visuels déjà là (héros de 12 Mo,
/// grilles 1024×1536) pèsent des centaines de Mo dans le dossier partagé par
/// Syncthing, alors qu'ils s'affichent dans une bande de 1280 px et des
/// vignettes de 300. Les téléchargements suivants sont normalisés d'office
/// (`covers::save`) ; un fichier déjà bon n'est jamais réécrit.
fn shrink(config: &config::Config) -> iced::Result {
    // les fichiers déjà là prennent d'abord leur nom de jeu partagé
    covers::migrate_names(&config.assets_dir(), &config.cache_dir().join("marks"));
    let mut report = |line: String| println!("{line}");
    let totals = covers::shrink_assets(&config.assets_dir(), &config.thumb_dir(), &mut report);
    println!(
        "{} fichier(s) réécrit(s) : {:.1} Mo → {:.1} Mo",
        totals.rewritten,
        totals.before as f64 / (1024.0 * 1024.0),
        totals.after as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

fn main() -> iced::Result {
    start();
    let options = match config::Options::parse(std::env::args().skip(1).collect()) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("glauncher : {err}\n\n{}", config::USAGE);
            std::process::exit(2);
        }
    };

    if options.help {
        print!("{}", config::USAGE);
        return Ok(());
    }

    let (mut config, notice) = config::resolve(&options);
    if let Some(seconds) = options.simulate {
        config.launch_override = Some(format!("sleep {seconds}"));
    }

    if options.reindex {
        return reindex(&config);
    }
    if options.update || options.dates {
        return update(&config, options.dates);
    }
    if options.list {
        return list(&config);
    }
    if let Some(pack) = &options.art {
        return art(&config, pack);
    }
    if options.shrink {
        return shrink(&config);
    }
    if options.pads {
        gamepad::describe();
        return Ok(());
    }

    let window = window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        fullscreen: config.fullscreen,
        // TOUJOURS décorées, même en plein écran : sous Wayland les
        // décorations sont négociées à la création (winit demande client-side
        // quand `decorations` est faux, et `toggle_decorations` ne peut plus
        // rien y changer) → sortir du plein écran rendait une fenêtre sans
        // barre de titre (20/11). En plein écran KWin n'en dessine pas de
        // toute façon.
        decorations: true,
        platform_specific: iced::window::settings::PlatformSpecific {
            // app_id Wayland = nom du `.desktop` : sans lui, KWin ne rattache
            // pas la fenêtre à `glauncher.desktop` et affiche l'icône générique
            // Wayland (constaté 20/11).
            application_id: "glauncher".into(),
            ..Default::default()
        },
        ..window::Settings::default()
    };

    iced::application(
        move || {
            app::App::boot(
                config.clone(),
                notice.clone(),
                options.play,
                options.picker,
                mode::Mode::detect(),
            )
        },
        app::App::update,
        app::App::view,
    )
    .title("glauncher")
    .subscription(app::App::subscription)
    .theme(app::App::theme)
    // Police de l'aide du pied de page (`app::HELP_FONT`) : embarquée pour ne
    // pas dépendre de ce qui est installé chez l'utilisateur.
    .font(&include_bytes!("../assets/fonts/Lato-Regular.ttf")[..])
    .window(window)
    .antialiasing(true)
    .run()
}
