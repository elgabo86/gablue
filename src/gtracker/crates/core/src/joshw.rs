use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use regex::Regex;

use crate::db::Db;
use crate::util::decode_entities;

/// Catalogue joshw.info : 24 sites, un par famille de formats.
pub const SITES: &[(&str, &str)] = &[
    ("https://spc.joshw.info", "SNES"),
    ("https://nsf.joshw.info", "NES"),
    ("https://usf.joshw.info", "N64"),
    ("https://smd.joshw.info", "GEN"),
    ("https://psf.joshw.info", "PS1"),
    ("https://psf2.joshw.info", "PS2"),
    ("https://psf3.joshw.info", "PS3"),
    ("https://psf4.joshw.info", "PS4"),
    ("https://gbs.joshw.info", "GB"),
    ("https://gsf.joshw.info", "GBA"),
    ("https://2sf.joshw.info", "NDS"),
    ("https://3sf.joshw.info", "3DS"),
    ("https://dsf.joshw.info", "DC"),
    ("https://wii.joshw.info", "Wii"),
    ("https://wiiu.joshw.info", "WiiU"),
    ("https://gcn.joshw.info", "GCN"),
    ("https://switch.joshw.info", "SWITCH"),
    ("https://ssf.joshw.info", "SAT"),
    ("https://xbox.joshw.info", "XBX"),
    ("https://x360.joshw.info", "X360"),
    ("https://psp.joshw.info", "PSP"),
    ("https://vita.joshw.info", "VITA"),
    ("https://pc.joshw.info", "PC"),
    ("https://3do.joshw.info", "3DO"),
];

pub fn short_name(site: &str) -> &'static str {
    SITES
        .iter()
        .find(|(s, _)| *s == site)
        .map(|(_, n)| *n)
        .unwrap_or("UNK")
}

fn folders() -> Vec<String> {
    let mut f = vec!["0-9".to_string()];
    for c in b'a'..=b'z' {
        f.push((c as char).to_string());
    }
    f.push("zzz_prototypes".into());
    f.push("zzz_unlicensed".into());
    f
}

type Row = (String, String, String, String);

/// Rafraîchit la base (696 requêtes, un thread par site) et renvoie le nombre d'ajouts.
pub fn update(db: &mut Db, progress: Arc<dyn Fn(usize, usize) + Send + Sync>) -> Result<usize> {
    let folders = folders();
    let total = SITES.len() * folders.len();
    let done = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel::<Vec<Row>>();

    let mut handles = Vec::new();
    for (site, _) in SITES {
        let tx = tx.clone();
        let done = done.clone();
        let progress = progress.clone();
        let folders = folders.clone();
        let site = site.to_string();
        handles.push(thread::spawn(move || {
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .user_agent("gtracker/0.1")
                .build();
            for folder in folders {
                let url = format!("{site}/{folder}/");
                if let Ok(html) = fetch(&agent, &url) {
                    let rows = parse_links(&site, &folder, &html);
                    if !rows.is_empty() {
                        let _ = tx.send(rows);
                    }
                }
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(n, total);
            }
        }));
    }
    drop(tx);

    let mut inserted = 0usize;
    for rows in rx {
        inserted += db.insert_rows(&rows)?;
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(inserted)
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<String> {
    let resp = agent.get(url).call()?;
    let mut buf = String::new();
    resp.into_reader()
        .take(2 * 1024 * 1024)
        .read_to_string(&mut buf)?;
    Ok(buf)
}

fn parse_links(site: &str, folder: &str, html: &str) -> Vec<Row> {
    let re = Regex::new(r#"href="([^"]*\.7z)">([^<]*)"#).expect("regex");
    let mut out = Vec::new();
    for cap in re.captures_iter(html) {
        let path = decode_entities(&cap[1]);
        let name = decode_entities(cap[2].trim());
        out.push((
            site.to_string(),
            folder.to_string(),
            name,
            path,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ligne() {
        let html = r#"<a href="Super%20Mario%20Bros.%20%5bNES%5d.7z">Super Mario Bros. [NES].7z</a>"#;
        let rows = parse_links("https://nsf.joshw.info", "s", html);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3, "Super%20Mario%20Bros.%20%5bNES%5d.7z");
        assert_eq!(rows[0].2, "Super Mario Bros. [NES].7z");
    }
}
