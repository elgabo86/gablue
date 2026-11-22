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
use std::path::{Path, PathBuf};
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
/// Publie aussi le jeu en cours pour l'overlay manette (voir [`running_body`]).
/// Renvoie le drapeau d'arrêt (armé par l'appelant quand le jeu se termine).
pub fn watch(root_pid: u32, cache: &Path, slug: &str, key: &str, nom: &str) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let cache = cache.to_path_buf();
    let slug = slug.to_string();
    let key = key.to_string();
    let nom = nom.to_string();

    let _ = std::thread::Builder::new()
        .name("glauncher-kwin".into())
        .spawn(move || {
            // Un launcher tué en pleine partie laisse son script dans KWin, et
            // KWin refuse de recharger un nom déjà pris (`loadScript` → -1) :
            // on décharge d'abord celui de la session précédente.
            drop_stale_script(&cache);

            let mut applied: Vec<u32> = Vec::new();
            let mut waited = 0u64;
            let mut published = String::new();

            while !flag.load(Ordering::Relaxed) {
                // On ne recharge que si un **nouveau client d'affichage**
                // apparaît : lui seul peut ouvrir une fenêtre, donc avoir besoin
                // d'être dans la liste. Wine forke en permanence (services,
                // conhost, overlay…) : recharger à chaque fork repartait d'un
                // état vierge et **réimposait le plein écran que l'utilisateur
                // venait de retirer** (constaté 18/11).
                let pids = game_pids(root_pid);
                // le jeu en cours (slug + PID) pour l'overlay manette : le
                // fichier ne bouge qu'avec la liste des PID (Wine forke)
                publier(&cache, &slug, &key, &nom, &pids, &mut published);
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

            release(&cache);

            // le jeu est fini : l'overlay ne doit plus afficher son logo. Le
            // fichier n'est retiré que s'il est encore le nôtre (deux
            // launchers peuvent tourner en parallèle).
            let chemin = running_path(&cache);
            if std::fs::read_to_string(&chemin).ok().as_deref() == Some(published.as_str()) {
                let _ = std::fs::remove_file(&chemin);
            }
        });

    stop
}

/// Fichier publiant le jeu en cours pour l'overlay manette (`gpad`) :
/// `<cache>/running.txt`.
pub fn running_path(cache: &Path) -> PathBuf {
    cache.join("running.txt")
}

/// Contenu de `running.txt` : le slug et le **nom affiché** du jeu, ses PID, le
/// chemin absolu de son logo et celui de son icône quand ces visuels existent
/// dans le cache. `key` est le nom **partagé** des visuels (`covers::key`, le
/// jeu SteamGridDB) : deux `.wgp`/`.lgp` du même jeu dont les noms diffèrent
/// pointent ainsi vers les mêmes fichiers.
///
/// L'overlay compare le PID de la fenêtre active à cette liste : c'est le seul
/// lien entre une fenêtre et un jeu, la classe d'une fenêtre Wine ne disant
/// rien du paquet lancé. Nom et icône voyagent avec, parce que la fenêtre d'un
/// jeu `gwine`/`lgplaunch` ne les porte pas : son titre est celui du binaire et
/// son icône celle de l'exécutable, pas celles du `.wgp`/`.lgp`.
fn running_body(cache: &Path, slug: &str, key: &str, nom: &str, pids: &[u32]) -> String {
    let mut texte = format!("slug={slug}\nnom={nom}\n");
    texte.push_str("pids=");
    for (i, pid) in pids.iter().enumerate() {
        if i > 0 {
            texte.push(',');
        }
        texte.push_str(&pid.to_string());
    }
    texte.push('\n');
    if let Some(logo) = visuel(cache, "logos", key) {
        texte.push_str(&format!("logo={}\n", logo.display()));
    }
    if let Some(icone) = icone_jeu(cache, key, slug) {
        texte.push_str(&format!("icone={}\n", icone.display()));
    }
    texte
}

/// `<cache>/assets/<type>/<clé>.png` s'il existe. Les visuels SteamGridDB
/// sont des PNG ; les autres extensions ne sont pas publiées, l'overlay ne
/// décode que le PNG (`lire_png`) et une icône JPEG serait un piège muet.
fn visuel(cache: &Path, genre: &str, key: &str) -> Option<PathBuf> {
    let chemin = cache
        .join("assets")
        .join(genre)
        .join(format!("{key}.png"));
    chemin.is_file().then_some(chemin)
}

/// Icône du jeu, dans le même ordre que le launcher (`library::icon_cached`) :
/// icône SteamGridDB de `assets/icons/<clé>` — celle qu'on a choisie y est, et
/// le dossier est partagé entre postes —, puis l'id seul (visuel écrit avant
/// de connaître le nom du jeu), puis l'icône extraite du paquet
/// (`pack-icons/<slug>`, propre à la machine). Toutes en PNG (l'overlay ne
/// décode que ça).
fn icone_jeu(cache: &Path, key: &str, slug: &str) -> Option<PathBuf> {
    let icons = cache.join("assets/icons");
    let mut essais = vec![icons.join(format!("{key}.png"))];
    if let Some((id, _)) = key.split_once('-') {
        essais.push(icons.join(format!("{id}.png")));
    }
    essais.push(cache.join("pack-icons").join(format!("{slug}.png")));
    essais.into_iter().find(|chemin| chemin.is_file())
}

/// Écrit `running.txt` si son contenu change (écriture atomique `.part` +
/// renommage, comme le reste du cache).
fn publier(
    cache: &Path,
    slug: &str,
    key: &str,
    nom: &str,
    pids: &[u32],
    published: &mut String,
) {
    let contenu = running_body(cache, slug, key, nom, pids);
    if contenu == *published {
        return;
    }
    let chemin = running_path(cache);
    let mut tmp = chemin.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    if std::fs::write(&tmp, &contenu).is_ok() {
        let _ = std::fs::rename(&tmp, &chemin);
        *published = contenu;
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Décharge le script (le jeu est terminé) et rend la fenêtre du launcher au
/// bureau si le script n'a pas eu le temps de le faire.
pub fn release(cache: &Path) {
    // le script de surveillance est déchargé AVANT le nettoyage : son minuteur
    // reposait `keepAbove` sur l'écran noir (toutes les 20 ms), et le laisser
    // tourner juste après `uncurtain` pouvait remettre le launcher au-dessus de
    // tout jusqu'à la partie suivante
    let current = CURRENT.lock().ok().and_then(|mut slot| slot.take());
    if let Some(name) = current {
        unload(&name);
    }
    // relève d'une version précédente : le nom sans numéro est resté chargé
    // si le launcher a été mis à jour pendant une partie
    unload(PLUGIN_PREFIX);
    if let Err(error) = uncurtain(cache) {
        eprintln!("glauncher : nettoyage KWin impossible : {error}");
    }
    // plus de jeu : le fichier ne doit pas survivre et étiqueter une capture
    // de bureau avec le dernier jeu (métadonnée des captures, 23/11)
    let _ = std::fs::remove_file(running_path(cache));
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
/// Trois précautions, apprises à la mesure (18/11 et 22/11) :
/// - le plein écran n'est posé qu'une fois, pendant les 3 premières secondes
///   d'une fenêtre : retiré ensuite (F, overlay manette), il n'est **pas**
///   remis ;
/// - tant que la fenêtre du jeu n'est pas réellement plein écran, l'écran noir
///   du launcher reste la fenêtre active. Plasma ne cache ses panneaux que pour
///   une fenêtre active plein écran, et le client (wine) met quelques
///   centaines de ms à obéir : sans ça on voit la barre entre l'écran noir et
///   le jeu ;
/// - la barre se voit quand même **une ou deux images** au moment où la fenêtre
///   du jeu arrive (mesuré 22/11 : 2 images sur 599 à 20 i/s, ~100 ms, la
///   barre dessinée par-dessus l'écran noir). KWin ne réempile le plein écran
///   qu'à l'image suivante : on pose donc `keepAbove` sur l'écran noir et sur
///   la fenêtre du jeu le temps de la transition (la couche « Above » de KWin
///   passe devant les panneaux, y compris une barre toujours visible), puis on
///   le retire — aucun fichier de config Plasma n'est touché.
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
// délai pendant lequel une fenêtre plein écran reste au-dessus de la barre
// après être devenue active (le temps que KWin réempile)
const GRACE = 400;
// garde-fou : aucune fenêtre de jeu ne reste au-dessus au-delà
const MAX_AU_DESSUS = 8000;
let attente = null;
// fenêtres rendues au bureau : ne plus les poser au-dessus de la barre
const rendues = {{}};
// début du délai de grâce, par fenêtre
const auDessus = {{}};

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

// La couche « Above » de KWin est dessinée devant les panneaux (mesuré : une
// fenêtre keepAbove couvre la barre même sans plein écran). C'est ce qui évite
// l'image de barre au moment où la fenêtre du jeu arrive.
function devantLaBarre(w, oui) {{
    if (w && w.keepAbove !== oui) w.keepAbove = oui;
}}

// La fenêtre du launcher redevient une fenêtre comme les autres : le jeu prend
// le dessus.
function decouvrir() {{
    devantLaBarre(fenetreLauncher(), false);
}}

// Vrai si la fenêtre du jeu est maintenant plein écran : on lui rend la main.
function rendreMain(jeu) {{
    if (attente) {{
        attente.stop();
        attente = null;
    }}
    decouvrir();
    workspace.activeWindow = jeu;
}}

// Le jeu est devant : on lui rend l'état normal (Alt-Tab doit fonctionner).
function rendreAuBureau(w, k) {{
    devantLaBarre(w, false);
    rendues[k] = true;
    auDessus[k] = undefined;
    decouvrir();
    console.info("glauncher : barre Plasma rendue au bureau");
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
    const age = Date.now() - debut;
    // passé le démarrage, la fenêtre appartient à l'utilisateur : un plein
    // écran retiré (F, overlay manette) doit le rester
    if (age <= JEUNE && !w.fullScreen) {{
        console.info("glauncher : plein écran sur « " + w.caption + " »");
        w.fullScreen = true;
    }}
    if (!rendues[k] && !w.keepAbove) devantLaBarre(w, true);
    couvrir(w);
    if (rendues[k]) return;
    if (w.fullScreen && w === workspace.activeWindow) {{
        if (auDessus[k] === undefined) auDessus[k] = Date.now();
        else if (Date.now() - auDessus[k] > GRACE) rendreAuBureau(w, k);
    }} else {{
        auDessus[k] = undefined;
    }}
    if (age > MAX_AU_DESSUS) rendreAuBureau(w, k);
}}

function sweep() {{
    const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
    for (let i = 0; i < list.length; i++) traiter(list[i]);
}}

sweep();
// l'écran noir couvre la barre pendant tout le démarrage du jeu
devantLaBarre(fenetreLauncher(), true);
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

/// Remet la fenêtre du launcher dans le bureau (`keepAbove` retiré) : filet de
/// sécurité quand la partie se termine avant que le script n'ait rendu la main
/// (jeu qui plante aussitôt), sinon le launcher resterait au-dessus de tout.
///
/// Le nom du script porte un compteur : KWin refuse (-1) un nom déjà chargé, et
/// un même `release` peut suivre un lancement avorté (constaté 22/11 : le
/// nettoyage ne passait jamais, « KWin a refusé le script de nettoyage »).
fn uncurtain(cache: &Path) -> Result<(), String> {
    static RELOAD: AtomicUsize = AtomicUsize::new(0);
    let script = cache.join("kwin-uncurtain.js");
    std::fs::write(&script, uncurtain_body(std::process::id()))
        .map_err(|err| format!("{} : {err}", script.display()))?;
    let reload = RELOAD.fetch_add(1, Ordering::Relaxed) + 1;
    let name = format!("glauncher-uncurtain-{}-{reload}", std::process::id());
    let id = dbus(&[
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting.loadScript",
        &script.display().to_string(),
        &name,
    ])?;
    if id.trim() == "-1" || id.trim().is_empty() {
        return Err(format!("KWin a refusé le script de nettoyage ({name})"));
    }
    dbus(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"])?;
    let resultat = dbus(&[
        "org.kde.KWin",
        &format!("/Scripting/Script{}", id.trim()),
        "org.kde.kwin.Script.run",
    ]);
    // le script a fait son travail : ne pas en laisser un par partie dans KWin
    unload(&name);
    resultat.map(|_| ())
}

/// Script de nettoyage : le launcher redevient une fenêtre ordinaire.
fn uncurtain_body(launcher_pid: u32) -> String {
    format!(
        r#"// généré par glauncher — filet de sécurité (ne pas éditer)
const LAUNCHER = {launcher_pid};
const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
for (let i = 0; i < list.length; i++) {{
    const w = list[i];
    if (Number(w.pid) === LAUNCHER && w.keepAbove) {{
        w.keepAbove = false;
        console.info("glauncher : barre Plasma rendue au bureau");
    }}
}}
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
    fn script_couvre_la_barre_par_keep_above() {
        let body = script_body(&[12], 99);
        assert!(
            body.contains("function devantLaBarre"),
            "la barre se couvre par la couche Above, pas par la config Plasma"
        );
        assert!(body.contains("w.keepAbove = oui"), "le drapeau est posé ET retiré");
        assert!(body.contains("devantLaBarre(fenetreLauncher(), true)"), "écran noir au-dessus");
        assert!(body.contains("GRACE = 400"), "rendu au bureau après réempilement");
        assert!(body.contains("MAX_AU_DESSUS = 8000"), "garde-fou contre un Alt-Tab cassé");
        assert!(body.contains("function decouvrir"), "l'écran noir est découvert au passage");
        assert!(
            body.contains("rendues[k] = true"),
            "une fenêtre rendue au bureau n'est plus reposée au-dessus"
        );
    }

    #[test]
    fn nettoyage_retire_keep_above_du_launcher() {
        let body = uncurtain_body(4242);
        assert!(body.contains("const LAUNCHER = 4242;"));
        assert!(body.contains("w.keepAbove = false"));
        assert!(body.contains("Number(w.pid) === LAUNCHER"));
    }

    #[test]
    fn processus_sans_fenetre_non_detecte() {
        // PID 1 ne mappe ni libX11 ni libwayland-client : il ne doit pas
        // déclencher de relève du script
        assert!(!super::owns_display(1));
    }

    #[test]
    fn running_publie_slug_pid_et_logo() {
        let dir = std::env::temp_dir().join(format!("glauncher-running-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets/logos")).unwrap();
        std::fs::create_dir_all(dir.join("pack-icons")).unwrap();

        let corps = running_body(&dir, "celeste", "celeste", "Celeste", &[12, 34]);
        assert!(corps.contains("slug=celeste\n"));
        assert!(corps.contains("nom=Celeste\n"), "nom affiché du paquet");
        assert!(corps.contains("pids=12,34\n"));
        assert!(
            !corps.contains("logo="),
            "visuel absent du cache = aucun chemin publié"
        );
        assert!(!corps.contains("icone="));

        std::fs::write(dir.join("assets/logos/celeste.png"), b"x").unwrap();
        std::fs::write(dir.join("pack-icons/celeste.png"), b"x").unwrap();
        let corps = running_body(&dir, "celeste", "celeste", "Celeste", &[12]);
        assert!(corps.contains("logo="), "le logo en cache est publié");
        assert!(corps.contains("celeste.png"));
        assert!(
            corps.contains("pack-icons/celeste.png"),
            "l'icône du paquet est publiée"
        );
        assert!(running_path(&dir).ends_with("running.txt"));

        // une icône JPEG ne part pas : l'overlay ne décode que le PNG
        std::fs::remove_file(dir.join("pack-icons/celeste.png")).unwrap();
        std::fs::write(dir.join("pack-icons/celeste.jpg"), b"x").unwrap();
        let corps = running_body(&dir, "celeste", "celeste", "Celeste", &[12]);
        assert!(!corps.contains("icone="), "PNG seulement");

        let _ = std::fs::remove_dir_all(&dir);
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
