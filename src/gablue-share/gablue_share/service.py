"""Mode permanent : le démon tourne dans une unité `systemd --user`.

La GUI n'est alors qu'un client : la fermer ne coupe pas le partage. La vérité
est l'état systemd, jamais un champ du fichier de configuration.
"""

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


def enabled() -> bool:
    return _systemctl("is-enabled", UNIT).returncode == 0


def active() -> bool:
    return _systemctl("is-active", UNIT).returncode == 0


def enable() -> tuple[bool, str]:
    ok, message = ensure_unit()
    if not ok:
        return False, message
    result = _systemctl("enable", "--now", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "activation impossible"
    return True, ""


def disable() -> tuple[bool, str]:
    result = _systemctl("disable", "--now", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "désactivation impossible"
    return True, ""


def start() -> tuple[bool, str]:
    result = _systemctl("start", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "démarrage impossible"
    return True, ""
