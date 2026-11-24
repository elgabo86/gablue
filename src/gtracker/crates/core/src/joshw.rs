use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
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

/// Résultat d'une mise à jour du catalogue.
pub struct UpdateReport {
    /// Archives nouvelles (les lignes existantes ne sont jamais supprimées).
    pub inserted: usize,
    /// Dossiers qui n'ont répondu à aucune tentative : le catalogue reste
    /// incomplet tant qu'ils manquent, il faut relancer la mise à jour.
    pub failed: Vec<String>,
}

/// Nombre de dossiers demandés en parallèle. Un thread par site (24) faisait
/// tomber des sites entiers en silence sur le petit serveur de joshw.info :
/// 4 requêtes à la fois suffisent et le catalogue en sort complet.
const WORKERS: usize = 4;

/// Tentatives par dossier : un hoquet réseau ou l'IPv6 injoignable de
/// joshw.info fait échouer une requête, pas les cinq.
const ATTEMPTS: usize = 5;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent("gtracker/0.1")
        .build()
}

/// Rafraîchit la base (696 requêtes) et renvoie les ajouts et les ratés.
pub fn update(
    db: &mut Db,
    progress: Arc<dyn Fn(usize, usize) + Send + Sync>,
) -> Result<UpdateReport> {
    let folders = folders();
    let total = SITES.len() * folders.len();
    let mut queue: VecDeque<(String, String, usize)> = VecDeque::new();
    for (site, _) in SITES {
        for folder in &folders {
            queue.push_back((site.to_string(), folder.clone(), 0));
        }
    }
    let queue = Arc::new(Mutex::new(queue));
    let failed = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel::<Vec<Row>>();

    let mut handles = Vec::new();
    for _ in 0..WORKERS {
        let tx = tx.clone();
        let done = done.clone();
        let progress = progress.clone();
        let queue = queue.clone();
        let failed = failed.clone();
        handles.push(thread::spawn(move || {
            let agent = agent();
            loop {
                let job = queue.lock().unwrap().pop_front();
                let Some((site, folder, tries)) = job else { break };
                let url = format!("{site}/{folder}/");
                match fetch(&agent, &url) {
                    Ok(html) => {
                        let rows = parse_links(&site, &folder, &html);
                        if !rows.is_empty() {
                            let _ = tx.send(rows);
                        }
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        progress(n, total);
                    }
                    // on remet le dossier en fin de file plutôt que de le perdre :
                    // un dossier raté = des jeux absents du catalogue, et rien
                    // ne le disait
                    Err(_) if tries + 1 < ATTEMPTS => {
                        thread::sleep(Duration::from_millis(500 * (tries as u64 + 1)));
                        queue.lock().unwrap().push_back((site, folder, tries + 1));
                    }
                    Err(e) => {
                        failed.lock().unwrap().push(format!("{url} : {e}"));
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        progress(n, total);
                    }
                }
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
    let failed = std::mem::take(&mut *failed.lock().unwrap());
    Ok(UpdateReport { inserted, failed })
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<String> {
    let resp = match agent.get(url).call() {
        Ok(r) => r,
        // 404 : ce site n'a pas ce dossier (tous n'ont pas `zzz_prototypes`).
        // Ce n'est pas un raté de mise à jour, juste un dossier en moins.
        Err(ureq::Error::Status(404, _)) => return Ok(String::new()),
        Err(e) => return Err(e.into()),
    };
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

    /// L'apostrophe de « Yoshi's Island » ne doit pas faire sauter la ligne :
    /// c'est le genre de nom qui disparaissait du catalogue.
    #[test]
    fn apostrophe_dans_le_nom() {
        let html = r#"<a href="Super%20Mario%20World%202%20-%20Yoshi's%20Island.7z">Super Mario World 2 - Yoshi's Island.7z</a>"#;
        let rows = parse_links("https://spc.joshw.info", "s", html);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, "Super Mario World 2 - Yoshi's Island.7z");
    }

    /// Un dossier muet n'est plus avalé : `fetch` renvoie une erreur, `update`
    /// la compte dans `failed` (port fermé sur la boucle locale, hors réseau).
    #[test]
    fn url_injoignable_est_signalee() {
        let agent = agent();
        assert!(fetch(&agent, "http://127.0.0.1:9/").is_err());
    }
}
