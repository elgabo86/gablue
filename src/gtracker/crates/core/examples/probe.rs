//! Sonde zxtune : durée, fin de boucle et tags, sans rendu complet.
//!
//! cargo run -p gtracker-core --example probe -- /chemin/fichier.psf

use gtracker_core::decode;
use gtracker_core::model::Track;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage : probe <fichier>"))?;
    let track = Track {
        subsong: 1,
        title: String::new(),
        file: path.into(),
        length: None,
        loop_start: None,
        loop_end: None,
    };
    let started = std::time::Instant::now();
    let p = decode::probe(&gtracker_core::zxtune_path(), &track)?;
    println!(
        "total {:.2}s, boucle {:?}, titre {:?}, auteur {:?} ({:.0} ms)",
        p.total,
        p.window,
        p.title,
        p.author,
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}
