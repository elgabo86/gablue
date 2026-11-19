//! Plein écran KWin pour le jeu lancé.
//!
//! Beaucoup de jeux lancés par `gwine` / `lgplaunch` s'ouvrent dans une simple
//! fenêtre : sans ça, ils apparaissent en 1280×720 au milieu du bureau. On
//! force donc l'attribut « plein écran » de KWin sur **les fenêtres du jeu**
//! avec un petit script KWin chargé par D-Bus, maintenu tant que le jeu tourne.
//!
//! Les PID visés ne sont pas seulement les descendants du lanceur : Wine
//! daemonise ses processus, et celui qui porte la fenêtre est reparenté à
//! systemd (constaté 18/11 sur Antonblast : aucun plein écran alors que le
//! script tournait). Ces processus gardent en revanche la **session** créée par
//! `bwrap --new-session` : voir [`game_pids`].
//!
//! Le script ne touche qu'aux fenêtres normales de ces PID : les dialogues, les
//! notifications et la fenêtre du launcher ne sont pas concernés.
//!
//! Le passage doit être **immédiat** : Plasma remontre ses panneaux dès que la
//! fenêtre active n'est plus en plein écran, donc chaque milliseconde où la
//! fenêtre du jeu reste en fenêtre se voit (barre qui clignote entre l'écran
//! noir et le jeu). D'où : relève du script **sans trou** (le nouveau est
//! chargé avant que l'ancien soit déchargé), relance périodique interne et
//! réaction à l'activation d'une fenêtre.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Préfixe du nom du script côté KWin. Chaque relève porte un numéro : le
/// nouveau script est chargé **avant** que l'ancien soit déchargé, sinon un
/// trou de quelques dizaines de millisecondes laissait passer la fenêtre du
/// jeu (le temps qu'elle arrive en fenêtre, la barre Plasma remontait).
const PLUGIN_PREFIX: &str = "glauncher-fullscreen";

/// Compteur de relèves, pour un nom de script jamais réutilisé.
static RELOAD: AtomicUsize = AtomicUsize::new(0);

/// Nom du script actuellement chargé (à décharger en fin de partie).
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// Surveille le jeu `root_pid` et force le plein écran de ses fenêtres.
/// Renvoie le drapeau d'arrêt (armé par l'appelant quand le jeu se termine).
pub fn watch(root_pid: u32, cache: &Path) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let cache = cache.to_path_buf();

    let _ = std::thread::Builder::new()
        .name("glauncher-kwin".into())
        .spawn(move || {
            // Un launcher tué en pleine partie laisse son script dans KWin, et
            // KWin refuse de recharger un nom déjà pris (`loadScript` → -1) :
            // on décharge d'abord celui de la session précédente.
            drop_stale_script(&cache);

            let mut applied: Vec<u32> = Vec::new();
            let mut waited = 0u64;

            while !flag.load(Ordering::Relaxed) {
                // On ne recharge que si un **nouveau client d'affichage**
                // apparaît : lui seul peut ouvrir une fenêtre, donc avoir besoin
                // d'être dans la liste. Wine forke en permanence (services,
                // conhost, overlay…) : recharger à chaque fork repartait d'un
                // état vierge et **réimposait le plein écran que l'utilisateur
                // venait de retirer** (constaté 18/11).
                let pids = game_pids(root_pid);
                let nouveau_client = pids
                    .iter()
                    .filter(|pid| !applied.contains(pid))
                    .any(|pid| owns_display(*pid));
                if (applied.is_empty() || nouveau_client) && pids != applied {
                    match apply(&cache, &pids) {
                        Ok(()) => {
                            eprintln!(
                                "glauncher : plein écran KWin forcé (pid {})",
                                join(&pids)
                            );
                            applied = pids;
                        }
                        Err(error) => {
                            eprintln!("glauncher : plein écran KWin impossible : {error}");
                            break;
                        }
                    }
                }

                // Cadence serrée pendant le démarrage du jeu : la fenêtre peut
                // apparaître juste après le fork de son processus, donc avant
                // que ce PID ne soit connu du script chargé. Ensuite on
                // relâche, le script se suffisant à lui-même.
                let step = if waited < 6_000 {
                    100
                } else if waited < 60_000 {
                    1_000
                } else {
                    5_000
                };
                std::thread::sleep(Duration::from_millis(step));
                waited += step;
            }

            release();
        });

    stop
}

/// Décharge le script (le jeu est terminé).
pub fn release() {
    let current = CURRENT.lock().ok().and_then(|mut slot| slot.take());
    if let Some(name) = current {
        unload(&name);
    }
    // relève d'une version précédente : le nom sans numéro est resté chargé
    // si le launcher a été mis à jour pendant une partie
    unload(PLUGIN_PREFIX);
}

/// Décharge le script laissé par une session précédente tuée en pleine partie.
fn drop_stale_script(cache: &Path) {
    let marker = cache.join("kwin-fullscreen.name");
    let Ok(name) = std::fs::read_to_string(&marker) else {
        return;
    };
    let name = name.trim();
    if name.is_empty() || CURRENT.lock().is_ok_and(|slot| slot.as_deref() == Some(name)) {
        return;
    }
    unload(name);
    let _ = std::fs::remove_file(marker);
}

/// Décharge un script par son nom.
fn unload(name: &str) {
    let _ = dbus(&[
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting.unloadScript",
        name,
    ]);
}

/// Écrit et lance le script pour ces PID.
fn apply(cache: &Path, pids: &[u32]) -> Result<(), String> {
    std::fs::create_dir_all(cache).map_err(|err| format!("{} : {err}", cache.display()))?;
    let script = cache.join("kwin-fullscreen.js");
    std::fs::write(&script, script_body(pids, std::process::id()))
        .map_err(|err| format!("{} : {err}", script.display()))?;

    // Le nouveau script est chargé et lancé AVANT de décharger l'ancien : les
    // deux se chevauchent quelques millisecondes (les gestionnaires en double
    // sont sans effet, `force` est idempotent), mais il n'y a plus jamais
    // d'instant sans surveillance.
    // nom unique par partie ET par relève : deux launchers ne se marchent pas
    // dessus, et un script oublié par une session tuée ne bloque pas la suivante
    let reload = RELOAD.fetch_add(1, Ordering::Relaxed) + 1;
    let name = format!("{PLUGIN_PREFIX}-{}-{reload}", std::process::id());

    let id = dbus(&[
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting.loadScript",
        &script.display().to_string(),
        &name,
    ])?;
    if id.trim() == "-1" || id.trim().is_empty() {
        return Err("KWin a refusé le script (nom déjà pris ?)".to_string());
    }
    // de quoi décharger ce script si le launcher est tué en pleine partie
    let _ = std::fs::write(script.with_extension("name"), &name);
    let _ = dbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"]);
    dbus(&[
        "org.kde.KWin",
        &format!("/Scripting/Script{}", id.trim()),
        "org.kde.kwin.Script.run",
    ])?;

    let previous = CURRENT.lock().ok().and_then(|mut slot| slot.replace(name));
    if let Some(previous) = previous {
        unload(&previous);
    }
    Ok(())
}

/// Script KWin : plein écran sur les fenêtres normales des PID du jeu.
///
/// Deux précautions, apprises à la mesure (18/11) :
/// - le plein écran n'est posé qu'une fois, pendant les 3 premières secondes
///   d'une fenêtre : retiré ensuite (F, overlay manette), il n'est **pas**
///   remis ;
/// - tant que la fenêtre du jeu n'est pas réellement plein écran, l'écran noir
///   du launcher reste la fenêtre active. Plasma ne cache ses panneaux que pour
///   une fenêtre active plein écran, et le client (wine) met quelques
///   centaines de ms à obéir : sans ça on voit la barre entre l'écran noir et
///   le jeu.
fn script_body(pids: &[u32], launcher_pid: u32) -> String {
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"// généré par glauncher — plein écran du jeu lancé (ne pas éditer)
const PIDS = [{list}];
const LAUNCHER = {launcher_pid};
// fenêtres déjà vues : date de la première prise en main
const vues = {{}};
// démarrage toléré pour imposer le plein écran (le client met du temps)
const JEUNE = 3000;
let attente = null;

function cle(w) {{
    return String(w.internalId || (w.caption + "|" + w.pid));
}}

function estJeu(w) {{
    if (!w || w.deleted === true || !w.normalWindow || w.popupMenu) return false;
    return w.pid !== undefined && PIDS.indexOf(w.pid) !== -1;
}}

function fenetreLauncher() {{
    const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
    for (let i = 0; i < list.length; i++) {{
        if (Number(list[i].pid) === LAUNCHER && list[i].normalWindow) return list[i];
    }}
    return null;
}}

// Vrai si la fenêtre du jeu est maintenant plein écran : on lui rend la main.
function rendreMain(jeu) {{
    if (attente) {{
        attente.stop();
        attente = null;
    }}
    workspace.activeWindow = jeu;
}}

// Garde l'écran noir actif tant que la fenêtre du jeu n'est pas plein écran.
function couvrir(jeu) {{
    if (jeu.fullScreen) {{
        if (attente) rendreMain(jeu);
        return;
    }}
    const launcher = fenetreLauncher();
    // launcher en fenêtre : rien à couvrir, la barre Plasma est légitime
    if (!launcher || !launcher.fullScreen) return;
    if (attente) return;
    workspace.activeWindow = launcher;
    const fin = Date.now() + 2000;
    attente = new QTimer();
    attente.interval = 30;
    attente.timeout.connect(function () {{
        if (!jeu.deleted && !jeu.fullScreen && Date.now() < fin) return;
        rendreMain(jeu);
    }});
    attente.start();
}}

function traiter(w) {{
    if (!estJeu(w)) return;
    const k = cle(w);
    const debut = vues[k] !== undefined ? vues[k] : (vues[k] = Date.now());
    // passé le démarrage, la fenêtre appartient à l'utilisateur : un plein
    // écran retiré (F, overlay manette) doit le rester
    if (Date.now() - debut > JEUNE) return;
    if (!w.fullScreen) {{
        console.info("glauncher : plein écran sur « " + w.caption + " »");
        w.fullScreen = true;
    }}
    couvrir(w);
}}

function sweep() {{
    const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
    for (let i = 0; i < list.length; i++) traiter(list[i]);
}}

sweep();
const added = workspace.windowAdded ? workspace.windowAdded : workspace.clientAdded;
if (added && added.connect) added.connect(traiter);
const activated = workspace.windowActivated ? workspace.windowActivated : workspace.clientActivated;
if (activated && activated.connect) activated.connect(traiter);

// une fenêtre créée pendant une relève du script n'attend pas la suivante
const timer = new QTimer();
timer.interval = 200;
timer.timeout.connect(sweep);
timer.start();
"#
    )
}

/// Appelle une méthode D-Bus de KWin (`qdbus6`, puis `qdbus`).
fn dbus(args: &[&str]) -> Result<String, String> {
    let mut last = String::from("qdbus introuvable");
    for tool in ["qdbus6", "qdbus"] {
        match Command::new(tool)
            .args(args)
            .stdin(Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
            Ok(output) => {
                last = String::from_utf8_lossy(&output.stderr).trim().to_string();
            }
            Err(err) => last = format!("{tool} : {err}"),
        }
    }
    Err(last)
}

/// Vrai si le processus a chargé la bibliothèque cliente de l'affichage
/// (X11 ou Wayland) : c'est le meilleur signe qu'il **possède une fenêtre**,
/// donc que le script doit l'ajouter à sa liste de PID.
///
/// Vérifié 18/11 : `notepad.exe` (la fenêtre) mappe `libX11`, `wineserver` et
/// les services de wine non — et c'est ce qui permet de ne recharger le script
/// que quand c'est utile. Recharger à chaque fork de wine repartait d'un état
/// vierge et **réimposait le plein écran que l'utilisateur venait de retirer**.
fn owns_display(pid: u32) -> bool {
    let Ok(maps) = std::fs::read_to_string(format!("/proc/{pid}/maps")) else {
        return false;
    };
    maps.lines().any(|line| {
        let Some(path) = line.split_whitespace().last() else {
            return false;
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        name.starts_with("libX11.so") || name.starts_with("libwayland-client.so")
    })
}

/// PID du jeu : ses descendants **et** les processus de leur session.
///
/// Wine daemonise ce qu'il lance : le processus qui porte la fenêtre est
/// reparenté à `systemd --user`, donc absent de l'arbre des descendants du
/// lanceur (vérifié 18/11 : `notepad.exe` ppid=systemd, `Antonblast.exe` idem —
/// le script tournait sans jamais reconnaître la fenêtre). En revanche ces
/// processus gardent la **session** ouverte par `bwrap --new-session`, celle de
/// `start.exe`, qui est bien un descendant. On ajoute donc les processus dont
/// la session est celle d'un descendant.
///
/// La session du launcher lui-même est exclue : sans sandbox (`--nosandbox`),
/// le jeu hérite de la session de la session graphique et on ne veut surtout
/// pas mettre en plein écran toutes ses fenêtres. Dans ce cas on en reste aux
/// descendants, comme avant.
fn game_pids(root: u32) -> Vec<u32> {
    let own_session = session_of(std::process::id());

    struct Info {
        ppid: u32,
        session: u32,
    }
    let mut infos: HashMap<u32, Info> = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return vec![root];
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // `pid (comm) état ppid pgrp session …` et `comm` peut contenir des
        // espaces et des parenthèses
        let Some(rest) = stat.rsplit_once(')').map(|(_, rest)| rest) else {
            continue;
        };
        let mut fields = rest.split_whitespace().skip(1);
        let (Some(ppid), Some(session)) = (
            fields.next().and_then(|n| n.parse().ok()),
            fields.next().and_then(|_| fields.next()).and_then(|n| n.parse().ok()),
        ) else {
            continue;
        };
        infos.insert(pid, Info { ppid, session });
    }

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, info) in &infos {
        children.entry(info.ppid).or_default().push(*pid);
    }

    let mut out = vec![root];
    let mut index = 0;
    while index < out.len() {
        let pid = out[index];
        index += 1;
        if let Some(kids) = children.get(&pid) {
            out.extend(kids.iter().copied());
        }
    }

    // sessions ouvertes par le jeu (celles des descendants), la nôtre exclue
    let mut sessions: Vec<u32> = out
        .iter()
        .filter_map(|pid| infos.get(pid).map(|info| info.session))
        .filter(|session| Some(*session) != own_session && *session > 1)
        .collect();
    sessions.sort_unstable();
    sessions.dedup();
    for (pid, info) in &infos {
        if sessions.contains(&info.session) {
            out.push(*pid);
        }
    }

    out.sort_unstable();
    out.dedup();
    out
}

/// Session d'un processus (`/proc/<pid>/stat`, champ 6), sans dépendance.
fn session_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')').map(|(_, rest)| rest)?;
    rest.split_whitespace().skip(1).nth(2)?.parse().ok()
}

/// PID du jeu et de tous ses descendants (via `/proc`, sans dépendance).
#[cfg(test)]
fn descendants(root: u32) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return vec![root];
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // `pid (comm) état ppid …` et `comm` peut contenir espaces et parenthèses
        let Some(rest) = stat.rsplit_once(')').map(|(_, rest)| rest) else {
            continue;
        };
        let Some(ppid) = rest.split_whitespace().nth(1).and_then(|n| n.parse().ok()) else {
            continue;
        };
        children.entry(ppid).or_default().push(pid);
    }

    let mut out = vec![root];
    let mut index = 0;
    while index < out.len() {
        let pid = out[index];
        index += 1;
        if let Some(kids) = children.get(&pid) {
            out.extend(kids.iter().copied());
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn join(pids: &[u32]) -> String {
    pids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_liste_les_pid() {
        let body = script_body(&[12, 34], 99);
        assert!(body.contains("const PIDS = [12, 34];"));
        assert!(body.contains("const LAUNCHER = 99;"), "la fenêtre du launcher est connue");
        assert!(body.contains("JEUNE = 3000"), "un plein écran retiré n'est pas remis");
        assert!(body.contains("workspace.activeWindow = launcher"), "l'écran noir couvre");
        assert!(body.contains("w.fullScreen = true"));
        assert!(body.contains("windowAdded"), "les fenêtres tardives sont visées");
        assert!(
            body.contains("windowActivated"),
            "une fenêtre activée est mise en plein écran dans l'image"
        );
        assert!(body.contains("timer.interval = 200"), "relance périodique");
        assert!(body.contains("timer.start()"), "la relance est armée");
    }

    #[test]
    fn processus_sans_fenetre_non_detecte() {
        // PID 1 ne mappe ni libX11 ni libwayland-client : il ne doit pas
        // déclencher de relève du script
        assert!(!super::owns_display(1));
    }

    #[test]
    fn game_pids_contient_le_processus_courant() {
        let me = std::process::id();
        assert!(game_pids(me).contains(&me));
    }

    #[test]
    fn game_pids_ne_prend_pas_toute_la_session() {
        // la session du launcher (lancé depuis un shell) ne doit jamais être
        // aspirée : sinon un jeu sans sandbox mettrait tout le bureau en plein
        // écran. Le processus de test est dans la session du shell : la liste
        // reste bornée à son arbre.
        let me = std::process::id();
        let pids = game_pids(me);
        assert!(pids.len() < 200, "{} PID retenus", pids.len());
    }

    #[test]
    fn descendants_d_un_processus_reel() {
        // le processus de test est son propre point de départ
        let me = std::process::id();
        assert!(descendants(me).contains(&me));
        assert!(!descendants(1).is_empty(), "init a toujours des descendants");
    }
}
