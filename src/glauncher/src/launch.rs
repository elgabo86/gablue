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
    // options du jeu, **propres à ce poste** (`~/.local/share/glauncher/
    // games.txt`, jamais dans `assets/` partagé) : gamescope, résolution
    // forcée, manettes en mode Xbox. Elles passent devant le paquet, comme
    // n'importe quelle option de `gwine` / `lgplaunch`.
    let options = crate::pergame::lire(&game.slug);
    build_avec(program, game, options)
}

/// La commande, une fois les options du jeu connues (testable sans toucher au
/// fichier de l'utilisateur).
fn build_avec(program: String, game: &Game, options: crate::pergame::Options) -> Launch {
    let mut args = options.arguments();
    args.push(game.path.display().to_string());
    Launch { program, args }
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
///
/// `slug` = paquet (temps de jeu, `running.txt`), `key` = nom **partagé** de
/// ses visuels dans `assets/` (le jeu SteamGridDB, voir `covers::key`) : c'est
/// sous ce nom que l'overlay manette trouvera logo et icône.
pub fn run(launch: &Launch, cache: &Path, slug: &str, key: &str, nom: &str) -> Result<i32, String> {
    eprintln!("glauncher : lancement de {}", launch.describe());

    let mut child = std::process::Command::new(&launch.program)
        .args(&launch.args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("lancement impossible ({}) : {err}", launch.program))?;

    let kwin_stop = kwin::watch(child.id(), cache, slug, key, nom);

    let status = child
        .wait()
        .map_err(|err| format!("attente de la fin du jeu : {err}"))?;

    kwin_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    kwin::release(cache);

    Ok(status.code().unwrap_or(-1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Kind;

    fn jeu(kind: Kind) -> Game {
        Game {
            path: std::path::PathBuf::from("/tmp/Jeu.lgp"),
            name: "Jeu test".to_string(),
            kind,
            slug: "jeu-test-options".to_string(),
            mtime: 0,
            icon: None,
            cover: None,
            hero: None,
            logo: None,
            thumb: None,
        }
    }

    #[test]
    fn options_du_jeu_dans_la_commande() {
        let config = Config::default();
        // sans réglage : juste le paquet
        let launch = build_avec(
            config.launch_lgp.clone(),
            &jeu(Kind::Lgp),
            crate::pergame::Options::default(),
        );
        assert_eq!(launch.args, vec!["/tmp/Jeu.lgp"]);

        // réglages par jeu, devant le paquet (comme toute option du lanceur)
        let options = crate::pergame::Options {
            gamescope: true,
            resolution: Some(1080),
            xbox: true,
        };
        let launch = build_avec(config.launch_wgp.clone(), &jeu(Kind::Wgp), options);
        assert_eq!(launch.program, "gwine");
        assert_eq!(
            launch.args,
            vec!["--gamescope", "--resolution", "1080", "--xbox", "/tmp/Jeu.lgp"]
        );
    }
}
