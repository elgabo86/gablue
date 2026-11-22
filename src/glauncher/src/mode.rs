//! Mode du launcher : session console Gablue ou bureau Plasma.
//!
//! La session console est une session Wayland nue (KWin + ce launcher, sans
//! plasmashell). Elle porte son nom dans `XDG_SESSION_DESKTOP`, posé par le
//! `DesktopNames` de `wayland-sessions/gablue-gaming.desktop` : le mode vient
//! donc de l'environnement et jamais d'une option à écrire deux fois.
//!
//! Lancé depuis le bureau, le launcher garde le comportement historique :
//! fenêtre plein écran ordinaire, « retour » deux fois ferme la fenêtre.
//! En session console, il ne se ferme pas : c'est la session elle-même, et
//! sa sortie passe par le menu système de l'overlay manette (`gpad`).

use std::env;

/// Nom de session posé par `gablue-gaming.desktop` (`DesktopNames`).
pub const SESSION_NAME: &str = "GablueGaming";

/// Robinet de banc d'essai : `GLAUNCHER_MODE=console|desktop`.
const MODE_VAR: &str = "GLAUNCHER_MODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Session console : le launcher est la session, il ne s'auto-ferme pas.
    Console,
    /// Bureau Plasma : fenêtre ordinaire.
    Desktop,
}

impl Mode {
    /// Mode déduit de l'environnement de la session.
    pub fn detect() -> Self {
        let forced = env::var(MODE_VAR).ok();
        Self::from_env(
            forced.as_deref(),
            env::var("XDG_SESSION_DESKTOP").ok().as_deref(),
            env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        )
    }

    /// Cœur testable : forcé, puis `XDG_SESSION_DESKTOP`, puis la liste
    /// `XDG_CURRENT_DESKTOP` (séparateurs `:` et `;`, comme Wayland/SDDM).
    fn from_env(forced: Option<&str>, session: Option<&str>, current: Option<&str>) -> Self {
        match forced {
            Some("console") => return Self::Console,
            Some("desktop") => return Self::Desktop,
            _ => {}
        }
        let named = |value: &str| {
            value
                .split([':', ';'])
                .any(|name| name.trim() == SESSION_NAME)
        };
        if session.is_some_and(named) || current.is_some_and(named) {
            Self::Console
        } else {
            Self::Desktop
        }
    }

    pub fn is_console(self) -> bool {
        matches!(self, Self::Console)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_par_session() {
        assert_eq!(
            Mode::from_env(None, Some("GablueGaming"), None),
            Mode::Console
        );
        assert_eq!(
            Mode::from_env(None, None, Some("GablueGaming:KDE")),
            Mode::Console
        );
        assert_eq!(
            Mode::from_env(None, None, Some("GablueGaming;KDE")),
            Mode::Console
        );
    }

    #[test]
    fn bureau_par_defaut() {
        assert_eq!(Mode::from_env(None, Some("KDE"), Some("KDE")), Mode::Desktop);
        assert_eq!(Mode::from_env(None, None, None), Mode::Desktop);
        // un nom qui CONTIENT le nôtre ne compte pas
        assert_eq!(
            Mode::from_env(None, None, Some("NotGablueGaming")),
            Mode::Desktop
        );
    }

    #[test]
    fn robinet_de_banc_gagne() {
        assert_eq!(
            Mode::from_env(Some("console"), Some("KDE"), None),
            Mode::Console
        );
        assert_eq!(
            Mode::from_env(Some("desktop"), Some("GablueGaming"), None),
            Mode::Desktop
        );
        // valeur inconnue : on ne force rien
        assert_eq!(
            Mode::from_env(Some("autre"), Some("GablueGaming"), None),
            Mode::Console
        );
    }
}
