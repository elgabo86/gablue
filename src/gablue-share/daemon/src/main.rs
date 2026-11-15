//! Lanceur du démon : `gablue-shared [--config FICHIER] [--host ADRESSE]
//! [--port PORT] [--launcher CHEMIN] [--threads N] [--allow-nonlocal]
//! [--no-tray]`.
//!
//! Le démon **survit à la fenêtre** : la GUI le lance détaché, le retrouve au
//! lancement suivant par `/api/info`, et il ne s'arrête qu'à l'extinction, à la
//! déconnexion, ou quand le dernier dossier partagé est retiré. `--parent-pid`
//! est encore accepté (GUI plus ancienne que lui) mais n'a plus d'effet.

use gablue_shared::{config, Options};
use std::path::PathBuf;

fn main() {
    let mut config_path: Option<PathBuf> = None;
    let mut host = "0.0.0.0".to_string();
    let mut port: Option<u16> = None;
    let mut threads = 4usize;
    let mut launcher: Option<String> = None;
    let mut allow_nonlocal = false;
    let mut tray = true;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        let value = |offset: usize| -> String {
            args.get(index + offset)
                .cloned()
                .unwrap_or_else(|| usage_error(&args[index]))
        };
        match args[index].as_str() {
            "--config" => config_path = Some(PathBuf::from(value(1))),
            "--host" => host = value(1),
            "--port" => {
                port = Some(value(1).parse().unwrap_or_else(|_| {
                    eprintln!("gablue-shared : port invalide « {} »", value(1));
                    std::process::exit(2)
                }))
            }
            "--threads" => {
                threads = value(1).parse().unwrap_or(4).max(1);
            }
            "--launcher" => launcher = Some(value(1)),
            // Ancienne option : le démon survit désormais à la fenêtre, on
            // l'accepte pour ne pas casser une GUI plus ancienne que lui.
            "--parent-pid" => {
                let _ = value(1);
            }
            "--allow-nonlocal" => allow_nonlocal = true,
            "--no-tray" => tray = false,
            "-h" | "--help" => {
                print_help();
                return;
            }
            other => {
                eprintln!("gablue-shared : argument inconnu « {other} »");
                std::process::exit(2);
            }
        }
        index += done_extra(&args[index]);
    }

    let path = config_path.unwrap_or_else(config::default_path);
    let options = Options {
        host,
        port,
        threads,
        allow_nonlocal,
        launcher,
        tray,
    };
    if let Err(err) = gablue_shared::run(path, options, None) {
        eprintln!("gablue-shared : {err}");
        std::process::exit(1);
    }
}

/// Nombre d'arguments consommés en plus du drapeau.
fn done_extra(flag: &str) -> usize {
    match flag {
        "--allow-nonlocal" | "--no-tray" | "-h" | "--help" => 1,
        _ => 2,
    }
}

fn usage_error(flag: &str) -> ! {
    eprintln!("gablue-shared : valeur manquante après « {flag} »");
    std::process::exit(2)
}

fn print_help() {
    println!(
        "gablue-shared — démon de partage Gablue (lecture seule, LAN et Tailscale)

Usage : gablue-shared [options]

Options :
  --config FICHIER   configuration (défaut : ~/.config/gablue/share.json)
  --host ADRESSE     adresse d'écoute (défaut : 0.0.0.0 ; « :: » pour IPv6)
  --port PORT        port explicite (défaut : celui de la configuration)
  --launcher CHEMIN  lanceur de la GUI (« Ouvrir Gablue Share » depuis l'icône)
  --no-tray          pas d'icône de notification (tests)
  --threads N        connexions servies en parallèle (défaut : 4)
  --parent-pid PID   s'arrête quand ce processus disparaît (mode éphémère)
  --allow-nonlocal   accepte toute adresse (tests seulement)
  -h, --help         cette aide

Le démon ne s'arrête pas sur inactivité : l'annonce mDNS meurt avec lui, donc
« vivant » et « visible » sont le même état."
    );
}
