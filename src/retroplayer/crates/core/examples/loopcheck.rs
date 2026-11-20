//! Vérifie la fenêtre de boucle trouvée dans un rendu déjà décodé.
//!
//! cargo run -p retroplayer-core --example loopcheck -- /chemin/fichier.psf

use retroplayer_core::{decode, model::Track};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage : loopcheck <fichier>"))?;
    let track = Track {
        subsong: 1,
        title: String::new(),
        file: path.into(),
        length: None,
        loop_start: None,
        loop_end: None,
    };
    let t0 = std::time::Instant::now();
    let d = decode::decode(&retroplayer_core::zxtune_path(), &track)?;
    let dt = t0.elapsed().as_secs_f64();
    println!(
        "total {:.3}s, fenêtre {:?}, tours {:?} ({:.0} ms)",
        d.total,
        d.window,
        d.window_passes,
        dt * 1000.0
    );
    Ok(())
}
