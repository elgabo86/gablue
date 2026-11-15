"""Chemins, constantes et localisation du binaire du démon."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

APP_NAME = "Gablue Share"
APP_ID = "gablue-share"
VERSION = "0.1.0"

PORT = 47823
SERVICE = "_gablueshare._tcp"
PROTOCOL = 1

# Le partage des dossiers vit ici et **survit à tout** : fermeture de l'app,
# redémarrage, mode permanent. C'est la seule source de vérité des partages.
CONFIG_DIR = Path(os.environ.get("XDG_CONFIG_HOME") or (Path.home() / ".config")) / "gablue"
CONFIG_PATH = CONFIG_DIR / "share.json"
STATE_DIR = (
    Path(os.environ.get("XDG_STATE_HOME") or (Path.home() / ".local" / "state"))
    / "gablue-share"
)
#: Petit canal entre l'icône de notification et la fenêtre : demander à la
#: fenêtre de se montrer, ou de se fermer proprement. Dans le répertoire
#: d'exécution (purgé à la déconnexion), avec l'uid dans le nom faute de
#: `XDG_RUNTIME_DIR` — deux comptes ne doivent pas se marcher dessus.
RUNTIME_DIR = Path(os.environ.get("XDG_RUNTIME_DIR") or "/tmp") / f"gablue-share-{os.getuid()}"
CONTROL_SOCKET = RUNTIME_DIR / "control.sock"

# Vitesses et délais
PROBE_TIMEOUT = 0.6
PROBE_INTERVAL = 30.0
EXPIRE_CYCLES = 3
LIST_TIMEOUT = 15.0


def launcher() -> str | None:
    """Le lanceur `gablue-share` — dépôt ou image.

    L'icône de notification (dans le démon) s'en sert pour « Ouvrir Gablue
    Share » : elle reçoit ce chemin à la naissance du démon, sinon elle retombe
    sur `/usr/bin/gablue-share`.
    """
    candidate = Path(sys.argv[0]).resolve()
    if candidate.name == APP_ID and candidate.exists():
        return str(candidate)
    return shutil.which(APP_ID)


def daemon_binary() -> str | None:
    """Binaire du démon : le build de développement d'abord, puis l'image.

    Lancée depuis les sources, la GUI doit utiliser le démon qu'elle vient de
    compiler — sinon on teste une interface neuve contre un démon périmé, et un
    champ ajouté (la visibilité) reste sans effet. Dans l'image, il n'y a pas
    d'arbre `daemon/` à côté du paquet : le repli sur `/usr/libexec` est immédiat.
    """
    dev = Path(__file__).resolve().parent.parent / "daemon" / "target" / "release" / "gablue-shared"
    if dev.exists():
        return str(dev)
    for candidate in ("/usr/libexec/gablue-shared", "/usr/bin/gablue-shared"):
        if os.path.exists(candidate):
            return candidate
    return shutil.which("gablue-shared")


def download_dir() -> Path:
    """Dossier de téléchargement XDG, sans sous-dossier (choix explicite)."""
    try:
        result = subprocess.run(
            ["xdg-user-dir", "DOWNLOAD"],
            capture_output=True,
            text=True,
            timeout=3,
            check=False,
        )
        path = result.stdout.strip()
        if path and path != str(Path.home()):
            return Path(path)
    except (OSError, subprocess.SubprocessError):
        pass
    return Path.home() / "Téléchargements"
