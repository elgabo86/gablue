//! Vérifie la fenêtre de boucle trouvée dans un rendu déjà décodé.
//!
//! cargo run -p gtracker-core --example loopcheck -- /chemin/fichier [sous-piste] [durée .m3u]

use gtracker_core::{decode, model::Track};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage : loopcheck <fichier>"))?;
    // sous-piste et durée annoncée optionnelles : c'est ainsi qu'on vérifie le
    // recalage d'une durée `.m3u` sur le rendu (NSF/GBS…)
    let subsong = std::env::args()
        .nth(2)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1);
    let length = std::env::args().nth(3).and_then(|v| v.parse::<f64>().ok());
    let track = Track {
        subsong,
        title: String::new(),
        file: path.into(),
        length,
        loop_start: None,
        loop_end: None,
    };
    let t0 = std::time::Instant::now();
    let d = decode::decode(&gtracker_core::zxtune_path(), &track)?;
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
