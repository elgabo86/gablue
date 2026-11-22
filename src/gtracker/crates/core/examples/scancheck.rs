//! Diagnostic : liste les pistes qu'une archive extraite du cache produit.
//!
//! `cargo run --release -p gtracker-core --example scancheck -- <id>`
//!
//! Utile quand l'interface dit « aucune piste reconnue » : montre ce que la
//! sonde de pistes voit vraiment (extensions, .m3u, fichiers vides).

use gtracker_core as core;

fn main() -> anyhow::Result<()> {
    let id: i64 = std::env::args().nth(1).unwrap_or_default().parse()?;
    let db = core::db::Db::open(&core::util::db_path())?;
    let entry = db
        .all_entries()?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("archive #{id} absente de la base"))?;
    let mut cb = |p: f32, label: &str| eprintln!("{p:.2} {label}");
    let ar = core::archive::ensure(&entry, &db, &mut cb)?;
    println!("{} pistes dans {}", ar.tracks.len(), ar.base.display());
    for t in ar.tracks.iter().take(8) {
        println!(
            "  {:>3}. {:<40} subsong {} · {:?}",
            t.subsong,
            t.title.chars().take(40).collect::<String>(),
            t.subsong,
            t.length
        );
    }
    Ok(())
}
