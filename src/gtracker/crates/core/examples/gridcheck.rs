//! Diagnostic : grille rythmique détectée pour un ou plusieurs fichiers.
//!
//! `cargo run --release -p gtracker-core --example gridcheck -- <fichier>…`
//!
//! Affiche la durée, la fenêtre de boucle et le tempo retenu (réseau ou
//! enveloppe, ou « AUCUN »). Sert à mesurer un lot de pistes sans ouvrir l'UI.

use std::path::PathBuf;

use gtracker_core as core;

fn main() -> anyhow::Result<()> {
    let zx = core::zxtune_path();
    for arg in std::env::args().skip(1) {
        let path = PathBuf::from(&arg);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let track = core::Track {
            subsong: 1,
            title: name.clone(),
            file: path.clone(),
            length: None,
            loop_start: None,
            loop_end: None,
        };
        match core::decode::decode(&zx, &track) {
            Ok(d) => {
                let t = match d.tempo {
                    Some(t) => format!(
                        "{:.1} BPM · beat {:.4} · phase {:.3} · conf {:.2}",
                        t.bpm, t.beat, t.phase, t.confidence
                    ),
                    None => "AUCUN TEMPO".to_string(),
                };
                println!("{name}\ttotal {:.1}\tfenêtre {:?}\t{t}", d.total, d.window);
            }
            Err(e) => println!("{name}\tERREUR {e:#}"),
        }
    }
    Ok(())
}
