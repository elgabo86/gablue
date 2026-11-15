"""Mode permanent : le démon tourne dans une unité `systemd --user`.

La GUI n'est alors qu'un client : la fermer ne coupe pas le partage. La vérité
est l'état systemd, jamais un champ du fichier de configuration.

Deux durées de vie, réglées par le même sélecteur :

- **session** : l'unité est activée, lancée à l'ouverture de session ;
- **boot** : en plus, `loginctl enable-linger` pour l'utilisateur — systemd
  démarre alors le gestionnaire de session (et donc l'unité) au démarrage du
  poste, sans attendre une ouverture de session. C'est ce que permet
  `org.freedesktop.login1.set-self-linger`, autorisé sans mot de passe
  (`allow_any=yes`, vérifié le 15/11/2026).
"""

import os
import pwd
import subprocess
from pathlib import Path

from .constants import APP_ID, CONFIG_PATH, daemon_binary

UNIT = f"{APP_ID}.service"
USER_UNITS = Path.home() / ".config" / "systemd" / "user"


def _systemctl(*arguments: str) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            ["systemctl", "--user", *arguments],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return subprocess.CompletedProcess(["systemctl"], 1, "", "systemctl injoignable")


def unit_available() -> bool:
    if USER_UNITS.joinpath(UNIT).exists():
        return True
    return _systemctl("cat", UNIT).returncode == 0


def unit_content() -> str:
    """Contenu de l'unité utilisateur (aussi livrée par l'image en M4)."""
    binary = daemon_binary() or "/usr/libexec/gablue-shared"
    return (
        "[Unit]\n"
        "Description=Gablue Share — partage de dossiers (LAN et Tailscale)\n"
        "After=network.target\n\n"
        "[Service]\n"
        "Type=simple\n"
        f"ExecStart={binary} --config {CONFIG_PATH}\n"
        "Restart=on-failure\n"
        "RestartSec=3\n\n"
        "[Install]\n"
        "WantedBy=default.target\n"
    )


def ensure_unit() -> tuple[bool, str]:
    """Crée l'unité utilisateur si l'image ne la fournit pas encore (M4)."""
    if unit_available():
        return True, ""
    if not daemon_binary():
        return False, "Démon introuvable : impossible de créer le service."
    try:
        USER_UNITS.mkdir(parents=True, exist_ok=True)
        USER_UNITS.joinpath(UNIT).write_text(unit_content(), encoding="utf-8")
    except OSError as error:
        return False, f"Écriture de l'unité impossible : {error}"
    _systemctl("daemon-reload")
    return True, ""


#: Les trois états du sélecteur, dans l'ordre d'affichage.
MODES = {
    "off": "Non",
    "session": "Avec la session",
    "boot": "Au démarrage du poste",
}


def _user() -> str:
    try:
        return pwd.getpwuid(os.getuid()).pw_name
    except KeyError:
        return os.environ.get("USER", "")


def _loginctl(*arguments: str) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            ["loginctl", *arguments],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return subprocess.CompletedProcess(["loginctl"], 1, "", "loginctl injoignable")


def linger_enabled() -> bool:
    """Le gestionnaire systemd de l'utilisateur démarre-t-il dès le boot ?"""
    result = _loginctl("show-user", _user(), "--property=Linger", "--value")
    return result.returncode == 0 and result.stdout.strip() == "yes"


def enable_linger() -> tuple[bool, str]:
    result = _loginctl("enable-linger", _user())
    if result.returncode != 0 or not linger_enabled():
        message = (result.stderr or result.stdout).strip()
        return False, message or "linger impossible (démarrage au boot non activé)"
    return True, ""


def disable_linger() -> tuple[bool, str]:
    result = _loginctl("disable-linger", _user())
    if result.returncode != 0:
        message = (result.stderr or result.stdout).strip()
        return False, message or "linger non désactivé"
    return True, ""


def current_mode() -> str:
    """État réel : jamais un champ du fichier, toujours systemd + loginctl."""
    if not enabled():
        return "off"
    return "boot" if linger_enabled() else "session"


def set_mode(mode: str) -> tuple[bool, str]:
    """Applique un des trois états. (ok, message)"""
    if mode not in MODES:
        return False, "Mode inconnu."
    if mode == "off":
        # On ne touche pas à linger : d'autres services de l'utilisateur
        # peuvent en dépendre, et sans notre unité il ne partage rien.
        return disable()
    ok, message = enable()
    if not ok:
        return False, message
    if mode == "boot":
        return enable_linger()
    # « session » : on retire le démarrage au boot s'il était actif, sinon le
    # choix ne serait pas respecté au prochain redémarrage.
    if linger_enabled():
        return disable_linger()
    return True, ""


def enabled() -> bool:
    return _systemctl("is-enabled", UNIT).returncode == 0


def active() -> bool:
    return _systemctl("is-active", UNIT).returncode == 0


def enable() -> tuple[bool, str]:
    """Réglage seul : le serveur en cours n'est ni démarré ni arrêté.

    La permanence dit **quand le serveur redémarre**, pas ce qu'il fait
    maintenant : `enable` sans `--now` laisse le démon en cours tel quel, et le
    service prendra le relais à la prochaine session (ou au prochain boot).
    """
    ok, message = ensure_unit()
    if not ok:
        return False, message
    result = _systemctl("enable", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "activation impossible"
    return True, ""


def disable() -> tuple[bool, str]:
    """Réglage seul, comme `enable` : le serveur en cours continue de servir."""
    result = _systemctl("disable", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "désactivation impossible"
    return True, ""


def stop() -> tuple[bool, str]:
    """Arrête le service sans toucher à son activation.

    L'unité reste activée : le partage revient à la prochaine session (ou au
    prochain boot), et « Quitter le partage » n'est pas un déréglage.
    """
    result = _systemctl("stop", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "arrêt impossible"
    return True, ""


def start() -> tuple[bool, str]:
    result = _systemctl("start", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "démarrage impossible"
    return True, ""
