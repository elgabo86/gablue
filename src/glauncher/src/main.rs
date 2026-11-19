//! glauncher — launcher de jeux Gablue (.wgp / .lgp).

mod app;
mod assets;
mod config;
mod covers;
mod favorites;
mod gamepad;
mod index;
mod kwin;
mod launch;
mod playtime;
mod state;
mod library;
mod thumbs;
mod watch;

use iced::window;

/// Horodatage de démarrage : deux repères dans le journal (app prête, premier
/// rendu) suffisent à voir si quelque chose retarde l'affichage.
pub fn start() {
    let _ = START.set(std::time::Instant::now());
}

pub fn stamp(what: &str) {
    let elapsed = START.get().map(|start| start.elapsed()).unwrap_or_default();
    eprintln!(
        "glauncher : {what} en {} ms",
        elapsed.as_millis()
    );
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

/// `--update` : télécharge les visuels manquants de tous les jeux indexés.
///
/// Utile pour amorcer un cache neuf (ou celui d'une autre machine via le
/// dossier `assets/` partagé) sans parcourir la bibliothèque à la main.
fn update(config: &config::Config) -> iced::Result {
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

    let marks = config.cache_dir().join("marks");
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
    for (slug, _, _) in &games {
        let Some(cover) = covers::cached(&config.cover_cache_dir(), slug) else {
            continue;
        };
        let job = thumbs::Job::new(slug.clone(), cover);
        if thumbs::build(&config.thumb_dir(), &job).is_ok() {
            thumbs += 1;
        }
    }

    println!(
        "{} téléchargés, {} introuvables, {} erreurs, {} déjà là — {thumbs} miniatures",
        totals.downloaded, totals.missing, totals.failed, totals.skipped
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
    if options.update {
        return update(&config);
    }
    if options.list {
        return list(&config);
    }
    if options.pads {
        gamepad::describe();
        return Ok(());
    }

    let window = window::Settings {
        size: iced::Size::new(1280.0, 720.0),
        fullscreen: config.fullscreen,
        decorations: !config.fullscreen,
        ..window::Settings::default()
    };

    iced::application(
        move || app::App::boot(config.clone(), notice.clone(), options.play, options.picker),
        app::App::update,
        app::App::view,
    )
    .title("glauncher")
    .subscription(app::App::subscription)
    .theme(app::App::theme)
    .window(window)
    .antialiasing(true)
    .run()
}
