//! Lancement d'un paquet : `gwine` pour un `.wgp`, `lgplaunch` pour un `.lgp`.

use std::path::Path;

use crate::config::Config;
use crate::kwin;
use crate::library::{Game, Kind};

#[derive(Debug, Clone)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
}

impl Launch {
    pub fn describe(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Construit la commande de lancement (surchargeable pour les tests).
///
/// Surcharge : `launch_override` (posé par `--simulate`) puis la variable
/// d'environnement `GLAUNCHER_LAUNCH_CMD`.
pub fn build(config: &Config, game: &Game) -> Launch {
    let override_cmd = config
        .launch_override
        .clone()
        .or_else(|| std::env::var("GLAUNCHER_LAUNCH_CMD").ok())
        .filter(|cmd| !cmd.trim().is_empty());

    if let Some(override_cmd) = override_cmd {
        return Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), override_cmd],
        };
    }

    let program = match game.kind {
        Kind::Wgp => config.launch_wgp.clone(),
        Kind::Lgp => config.launch_lgp.clone(),
    };
    Launch {
        program,
        args: vec![game.path.display().to_string()],
    }
}

/// Lance le jeu et attend sa fin ; renvoie le code de sortie.
///
/// Le jeu est mis en plein écran par KWin dès que sa fenêtre apparaît (voir
/// [`crate::kwin`]) : beaucoup de jeux `gwine` / `lgplaunch` s'ouvrent sinon
/// dans une fenêtre 1280×720 au milieu du bureau. La même surveillance publie
/// le jeu en cours (`running.txt`) pour l'overlay manette, qui y lit le slug et
/// les PID du jeu (et le logo s'il est en cache).
///
/// Bloquant : à appeler depuis une tâche iced, jamais depuis l'UI.
pub fn run(launch: &Launch, cache: &Path, slug: &str, nom: &str) -> Result<i32, String> {
    eprintln!("glauncher : lancement de {}", launch.describe());

    let mut child = std::process::Command::new(&launch.program)
        .args(&launch.args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("lancement impossible ({}) : {err}", launch.program))?;

    let kwin_stop = kwin::watch(child.id(), cache, slug, nom);

    let status = child
        .wait()
        .map_err(|err| format!("attente de la fin du jeu : {err}"))?;

    kwin_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    kwin::release();

    Ok(status.code().unwrap_or(-1))
}
