//! Vérification headless du téléchargement/extraction/lecture des .m3u.
//!
//! cargo run -p gtracker-core --example open -- "super mario bros. (1985"

use gtracker_core::archive;
use gtracker_core::db::Db;
use gtracker_core::util;

fn main() -> anyhow::Result<()> {
    let needle = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "super mario bros. (1985".into())
        .to_lowercase();
    let db = Db::open(&util::db_path())?;
    let entry = db
        .all_entries()?
        .into_iter()
        .find(|e| e.name.to_lowercase().contains(&needle))
        .ok_or_else(|| anyhow::anyhow!("aucune archive ne contient {needle:?}"))?;
    println!("archive {} : {} → {}", entry.id, entry.display_name(), entry.url());

    let mut cb = |p: f32, label: &str| println!("  {label} {:.0} %", p * 100.0);
    let a = archive::ensure(&entry, &db, &mut cb)?;
    println!("{} pistes, compositeur {:?}, album {:?}", a.tracks.len(), a.composer, a.album);
    for t in a.tracks.iter().take(6) {
        println!("  ?#{:<3} {:?}  {}  ({:?})", t.subsong, t.title, t.file.display(), t.length);
    }
    Ok(())
}
