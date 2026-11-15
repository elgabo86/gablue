"""Chemins, constantes et localisation du binaire du démon."""

import os
import shutil
import subprocess
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

# Vitesses et délais
PROBE_TIMEOUT = 0.6
PROBE_INTERVAL = 30.0
EXPIRE_CYCLES = 3
LIST_TIMEOUT = 15.0


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
