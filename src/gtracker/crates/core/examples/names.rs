//! Contrôle du nettoyage des noms sur toute la base.
//!
//! cargo run -p gtracker-core --example names [n]

use gtracker_core::db::Db;
use gtracker_core::util;

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(25);
    let db = Db::open(&util::db_path())?;
    let entries = db.all_entries()?;
    let mut empty = 0;
    let mut dangling = 0;
    let mut dates_left = 0;
    for e in &entries {
        let c = util::clean_name(&e.name);
        if c.title.is_empty() {
            empty += 1;
        }
        if c.title.ends_with(')') {
            dangling += 1;
        }
        if c.title.contains("(19") || c.title.contains("(20") {
            dates_left += 1;
        }
    }
    println!(
        "{} noms — vides {}, finissent par ')': {}, dates restantes: {}",
        entries.len(),
        empty,
        dangling,
        dates_left
    );
    let mut missed = Vec::new();
    for e in &entries {
        let c = util::clean_name(&e.name);
        let t = &c.title;
        if t.ends_with(')') {
            let head = &t[..t.len() - 1];
            if let Some(i) = head.rfind('(') {
                let inner = &head[i + 1..];
                if inner.len() >= 4 && inner[..4].chars().all(|c| c.is_ascii_digit()) {
                    missed.push(e.name.clone());
                }
            }
        }
    }
    println!("dates non retirées : {}", missed.len());
    for m in missed.iter().take(8) {
        println!("  {m}");
    }
    for e in entries.iter().take(n) {
        let c = util::clean_name(&e.name);
        println!(
            "{:<7} {:<8} {}{}",
            joshw_short(&e.site),
            c.date.unwrap_or_default(),
            c.title,
            if c.tags.is_empty() {
                String::new()
            } else {
                format!("   [{}]", c.tags.join(", "))
            }
        );
    }
    Ok(())
}

fn joshw_short(site: &str) -> &'static str {
    gtracker_core::joshw::short_name(site)
}
