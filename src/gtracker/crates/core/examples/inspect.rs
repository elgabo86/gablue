//! Vérification headless : base + décodage d'un fichier.
//!
//! cargo run -p gtracker-core --example inspect -- /chemin/fichier.vgz

use std::path::PathBuf;

use gtracker_core::db::Db;
use gtracker_core::decode;
use gtracker_core::model::Track;
use gtracker_core::util;

fn main() -> anyhow::Result<()> {
    let db = Db::open(&util::db_path())?;
    println!("base : {} ({})", db.path().display(), db.count()?);
    for e in db.all_entries()?.iter().take(3) {
        println!("  {}  {}  → {}", e.id, e.display_name(), e.url());
    }

    if let Some(arg) = std::env::args().nth(1) {
        let track = Track {
            subsong: std::env::args()
                .nth(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            title: "test".into(),
            file: PathBuf::from(arg),
            length: None,
            loop_start: None,
            loop_end: None,
        };
        let t0 = std::time::Instant::now();
        let d = decode::decode(&gtracker_core::zxtune_path(), &track)?;
        println!(
            "décodé en {:.2} s : {:.2} s audio, {} canaux, {} Hz, {} échantillons, boucle {:?}",
            t0.elapsed().as_secs_f64(),
            d.total,
            d.channels,
            d.rate,
            d.pcm.len(),
            d.window
        );
    }
    Ok(())
}
