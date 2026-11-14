//! Lanceur du démon : `gablue-shared [--config FICHIER] [--host ADRESSE]
//! [--port PORT] [--parent-pid PID] [--threads N] [--allow-nonlocal]`.
//!
//! `--parent-pid` : le démon s'arrête dès que ce processus disparaît — c'est
//! le mode éphémère, la GUI le lance et le laisse mourir avec elle, crash
//! compris. Sans cette option (unité systemd), il tourne jusqu'à SIGTERM.

use gablue_shared::{config, Options};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() {
    let mut config_path: Option<PathBuf> = None;
    let mut host = "0.0.0.0".to_string();
    let mut port: Option<u16> = None;
    let mut threads = 4usize;
    let mut parent: Option<u32> = None;
    let mut allow_nonlocal = false;

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
            "--parent-pid" => {
                parent = Some(value(1).parse().unwrap_or_else(|_| {
                    eprintln!("gablue-shared : pid invalide « {} »", value(1));
                    std::process::exit(2)
                }))
            }
            "--allow-nonlocal" => allow_nonlocal = true,
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

    if let Some(pid) = parent {
        watch_parent(pid);
    }

    let path = config_path.unwrap_or_else(config::default_path);
    let options = Options {
        host,
        port,
        threads,
        allow_nonlocal,
    };
    if let Err(err) = gablue_shared::run(path, options, None) {
        eprintln!("gablue-shared : {err}");
        std::process::exit(1);
    }
}

/// Nombre d'arguments consommés en plus du drapeau.
fn done_extra(flag: &str) -> usize {
    match flag {
        "--allow-nonlocal" | "-h" | "--help" => 1,
        _ => 2,
    }
}

fn usage_error(flag: &str) -> ! {
    eprintln!("gablue-shared : valeur manquante après « {flag} »");
    std::process::exit(2)
}

/// Mode éphémère : sans GUI, plus de partage.
fn watch_parent(pid: u32) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if !Path::new(&format!("/proc/{pid}")).exists() {
            eprintln!("gablue-shared : le processus {pid} a disparu, arrêt");
            std::process::exit(0);
        }
    });
}

fn print_help() {
    println!(
        "gablue-shared — démon de partage Gablue (lecture seule, LAN et Tailscale)

Usage : gablue-shared [options]

Options :
  --config FICHIER   configuration (défaut : ~/.config/gablue/share.json)
  --host ADRESSE     adresse d'écoute (défaut : 0.0.0.0 ; « :: » pour IPv6)
  --port PORT        port explicite (défaut : celui de la configuration)
  --threads N        connexions servies en parallèle (défaut : 4)
  --parent-pid PID   s'arrête quand ce processus disparaît (mode éphémère)
  --allow-nonlocal   accepte toute adresse (tests seulement)
  -h, --help         cette aide

Le démon ne s'arrête pas sur inactivité : l'annonce mDNS meurt avec lui, donc
« vivant » et « visible » sont le même état."
    );
}
