//! Icône de zone de notification (StatusNotifierItem), hébergée par le démon.
//!
//! Pas de processus en plus, pas de Qt : le serveur qui partage est déjà là, il
//! porte l'icône par DBus (`ksni`). Une icône Qt coûtait ~50 Mo, le démon en
//! pèse 3,2 : l'icône doit coûter quelques mégaoctets, pas une boîte à outils
//! graphique complète.
//!
//! Elle apparaît et disparaît avec le démon — donc avec le partage : le démon
//! ne démarre plus sans au moins un dossier déclaré, et s'arrête de lui-même
//! quand le dernier disparaît. Un serveur permanent peut démarrer avant la
//! session graphique : l'icône attend le `StatusNotifierWatcher` et apparaît
//! quand le bureau est là (`assume_sni_available` + `watcher_offline` par
//! défaut, qui garde le service vivant).
//!
//! Trois gestes : un clic bascule la fenêtre (ouvre / ferme, le serveur reste),
//! « Démarrage » règle l'auto-démarrage (les trois mêmes états que le tiroir),
//! « Quitter le partage » ferme la fenêtre et arrête le serveur.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{MenuItem, ToolTip, Tray as Icone};

use crate::Shared;

/// États du sélecteur « Démarrage », dans l'ordre du menu.
const MODES: [&str; 3] = ["Non", "Avec la session", "Au démarrage du poste"];
const UNIT: &str = "gablue-share.service";

pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(format!("gablue-share-{}", unsafe { libc::getuid() }))
}

/// Ordre à la fenêtre ouverte ; `false` si elle ne répond pas.
fn tell_gui(command: &str) -> bool {
    match UnixStream::connect(runtime_dir().join("control.sock")) {
        Ok(mut flux) => {
            let _ = flux.write_all(format!("{command}\n").as_bytes());
            let _ = flux.flush();
            true
        }
        Err(_) => false,
    }
}

fn gui_open() -> bool {
    UnixStream::connect(runtime_dir().join("control.sock")).is_ok()
}

/// Dossiers réellement servis (dossier présent) : c'est ce qu'annonce l'icône.
fn served(state: &Shared) -> usize {
    state
        .read()
        .map(|config| config.available().count())
        .unwrap_or(0)
}

fn declared(state: &Shared) -> usize {
    state.read().map(|config| config.shares.len()).unwrap_or(0)
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn user() -> String {
    std::env::var("USER").unwrap_or_else(|_| unsafe { libc::getuid() }.to_string())
}

fn linger() -> bool {
    Command::new("loginctl")
        .args(["show-user", &user(), "--property=Linger", "--value"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "yes")
        .unwrap_or(false)
}

fn linger_set(on: bool) -> bool {
    let action = if on {
        "enable-linger"
    } else {
        "disable-linger"
    };
    Command::new("loginctl")
        .args([action, &user()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// État réel de l'auto-démarrage : jamais un choix mémorisé, toujours systemd.
fn current_mode() -> usize {
    if !systemctl(&["is-enabled", UNIT]) {
        return 0;
    }
    if linger() {
        2
    } else {
        1
    }
}

/// Applique un des trois états. Ne touche **pas** au serveur en cours : la
/// permanence dit quand il redémarre, pas ce qu'il fait maintenant.
fn set_mode(index: usize) -> bool {
    match index {
        0 => systemctl(&["disable", UNIT]),
        1 => systemctl(&["enable", UNIT]) && (!linger() || linger_set(false)),
        2 => systemctl(&["enable", UNIT]) && linger_set(true),
        _ => false,
    }
}

fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["--app-name=Gablue Share", "--icon=gablue-share", title, body])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Variables d'affichage que le démon n'a pas forcément à sa naissance.
///
/// L'unité `systemd --user` peut démarrer **avant le bureau** : son
/// environnement ne porte alors que `XDG_RUNTIME_DIR`, et un Qt lancé depuis
/// l'icône avorte (« This application failed to start because no Qt platform
/// plugin could be initialized », vécu le 15/11/2026 — quatre plantages de
/// suite en cliquant l'icône). Plasma importe ces variables dans le
/// gestionnaire systemd (`systemctl --user import-environment`) ; on les relit
/// là au moment du clic, et seulement celles qui manquent chez nous.
fn display_environment() -> Vec<(String, String)> {
    let sortie = match Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
    {
        Ok(sortie) if sortie.status.success() => sortie,
        _ => return Vec::new(),
    };
    parse_display_environment(&String::from_utf8_lossy(&sortie.stdout))
        .into_iter()
        // Ce que nous avons déjà vient du même bureau : on n'écrase pas.
        .filter(|(nom, _valeur)| std::env::var_os(nom).is_none())
        .collect()
}

/// Lit les variables d'affichage d'une sortie de `systemctl show-environment`.
fn parse_display_environment(texte: &str) -> Vec<(String, String)> {
    const NOMS: [&str; 3] = ["WAYLAND_DISPLAY", "DISPLAY", "XAUTHORITY"];
    let mut trouvees = Vec::new();
    for ligne in texte.lines() {
        let (nom, valeur) = match ligne.split_once('=') {
            Some(paire) => paire,
            None => continue,
        };
        if !NOMS.contains(&nom) || valeur.is_empty() {
            continue;
        }
        trouvees.push((nom.to_string(), valeur.to_string()));
    }
    trouvees
}

/// Lance l'application, détachée de nous.
fn spawn_gui(launcher: &Option<String>) -> bool {
    let programme = launcher
        .clone()
        .or_else(|| Some("/usr/bin/gablue-share".to_string()))
        .filter(|path| Path::new(path).exists());
    match programme.and_then(|path| {
        let mut commande = Command::new(path);
        for (nom, valeur) in display_environment() {
            commande.env(nom, valeur);
        }
        commande
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .ok()
    }) {
        Some(mut enfant) => {
            // Sans `wait`, chaque ouverture/fermeture de la fenêtre laissait un
            // zombie sous le démon jusqu'à son propre arrêt (vécu 15/11/2026).
            std::thread::spawn(move || {
                let _ = enfant.wait();
            });
            true
        }
        None => false,
    }
}

struct Partage {
    state: Shared,
    launcher: Option<String>,
    gui_open: bool,
    mode: usize,
}

impl Partage {
    /// Clic sur l'icône : la fenêtre s'ouvre si elle est fermée, se ferme si
    /// elle est ouverte. Le serveur, lui, ne bouge pas.
    fn toggle_gui(&mut self) {
        if tell_gui("quit") {
            self.gui_open = false;
        } else {
            self.gui_open = spawn_gui(&self.launcher);
            if !self.gui_open {
                notify(
                    "Gablue Share",
                    "Impossible d'ouvrir la fenêtre : lanceur introuvable.",
                );
            }
        }
    }

    fn quit_share(&mut self) {
        // La fenêtre d'abord (elle écrit sa file et se ferme proprement), puis
        // le serveur : plus de démon, donc plus d'icône.
        tell_gui("quit");
        std::process::exit(0);
    }
}

impl Icone for Partage {
    fn id(&self) -> String {
        "gablue-share".into()
    }

    fn title(&self) -> String {
        "Gablue Share".into()
    }

    fn icon_name(&self) -> String {
        "gablue-share".into()
    }

    fn tool_tip(&self) -> ToolTip {
        let n = served(&self.state);
        ToolTip {
            icon_name: "gablue-share".into(),
            title: "Gablue Share".into(),
            description: format!(
                "{n} dossier{} partagé{}",
                if n > 1 { "s" } else { "" },
                if n > 1 { "s" } else { "" }
            ),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.toggle_gui();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let n = served(&self.state);
        let etat = format!(
            "Partage actif — {n} dossier{} partagé{}",
            if n > 1 { "s" } else { "" },
            if n > 1 { "s" } else { "" }
        );
        vec![
            StandardItem {
                label: etat,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: if self.gui_open {
                    "Fermer la fenêtre".into()
                } else {
                    "Ouvrir Gablue Share".into()
                },
                activate: Box::new(|tray: &mut Self| tray.toggle_gui()),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Démarrage".into(),
                submenu: vec![RadioGroup {
                    selected: self.mode.min(2),
                    select: Box::new(|tray: &mut Self, index: usize| {
                        if set_mode(index) {
                            tray.mode = index;
                        } else {
                            notify(
                                "Démarrage non modifié",
                                "systemctl --user a refusé le changement.",
                            );
                        }
                    }),
                    options: MODES
                        .iter()
                        .map(|label| RadioItem {
                            label: (*label).into(),
                            ..Default::default()
                        })
                        .collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quitter le partage".into(),
                activate: Box::new(|tray: &mut Self| tray.quit_share()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Démarre l'icône en tâche de fond (un fil, aucun processus).
///
/// Sans dossier déclaré, il n'y a rien à annoncer : pas d'icône du tout.
pub fn spawn(state: Shared, launcher: Option<String>) {
    if declared(&state) == 0 {
        return;
    }
    std::thread::spawn(move || {
        let trayeur = Partage {
            state: Shared::clone(&state),
            launcher,
            gui_open: gui_open(),
            mode: current_mode(),
        };
        // Le bureau peut démarrer après nous (serveur permanent) : ksni garde
        // le service vivant et enregistre l'icône quand le watcher arrive.
        let handle = match trayeur.assume_sni_available(true).spawn() {
            Ok(handle) => handle,
            Err(err) => {
                eprintln!("gablue-shared : icône de notification indisponible ({err})");
                return;
            }
        };
        surveiller(handle, state);
    });
}

/// Tient l'icône à jour : fenêtre ouverte ou non, état réel de la permanence.
fn surveiller(handle: Handle<Partage>, state: Shared) {
    let mut tours = 0u32;
    loop {
        sleep(Duration::from_secs(2));
        let ouverte = gui_open();
        handle.update(move |tray| {
            let change = tray.gui_open != ouverte;
            tray.gui_open = ouverte;
            change
        });
        // `systemctl`/`loginctl` toutes les 10 s suffisent : la permanence
        // change rarement, et deux processus par 2 s ne se justifient pas.
        tours += 1;
        if tours % 5 == 0 {
            let mode = current_mode();
            handle.update(move |tray| {
                let change = tray.mode != mode;
                tray.mode = mode;
                change
            });
        }
        // Plus aucun dossier : l'icône n'annonce plus rien. Le démon s'arrête
        // de lui-même dans ce cas ; on retire l'icône sans attendre.
        if declared(&state) == 0 {
            let _ = handle.shutdown().wait();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_display_environment;

    #[test]
    fn display_variables_are_read_from_the_manager_environment() {
        let texte = concat!(
            "LANG=fr_FR.UTF-8\n",
            "WAYLAND_DISPLAY=wayland-1\n",
            "DISPLAY=:0\n",
            "XAUTHORITY=/run/user/1000/xauth_yhpzVs\n",
            "HOME=/home/gab\n",
        );
        assert_eq!(
            parse_display_environment(texte),
            vec![
                ("WAYLAND_DISPLAY".to_string(), "wayland-1".to_string()),
                ("DISPLAY".to_string(), ":0".to_string()),
                (
                    "XAUTHORITY".to_string(),
                    "/run/user/1000/xauth_yhpzVs".to_string()
                ),
            ]
        );
    }

    #[test]
    fn unknown_or_empty_variables_are_ignored() {
        assert!(parse_display_environment("").is_empty());
        assert!(parse_display_environment("sans egal\nWAYLAND_DISPLAY=\nPATH=/usr/bin\n")
            .is_empty());
        assert_eq!(
            parse_display_environment("DISPLAY=:9\n"),
            vec![("DISPLAY".to_string(), ":9".to_string())]
        );
    }
}
