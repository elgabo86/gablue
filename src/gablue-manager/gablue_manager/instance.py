"""Instance unique : une fenêtre et une icône de barre d'état par session.

Le tray et la fenêtre sont deux processus distincts, mais chacun doit rester
unique : un second clic sur l'icône doit ramener la fenêtre existante, pas en
ouvrir une deuxième. Le canal est un `QLocalServer` par utilisateur — pas de
D-Bus à installer, et une socket laissée par un processus tué est reprise.
"""

import os
import subprocess
import sys

from PySide6.QtNetwork import QLocalServer, QLocalSocket

def gui_socket() -> str:
    """Nom du canal de la fenêtre principale."""
    return f"gablue-manager-{os.getuid()}"

def tray_socket() -> str:
    """Nom du canal de l'icône de barre d'état."""
    return f"gablue-manager-tray-{os.getuid()}"

def claim(name: str) -> QLocalServer | None:
    """Écoute sur ce nom ; None si une instance vivante l'occupe déjà.

    Une socket orpheline (processus tué sans ménage) ne répond pas : elle est
    retirée puis reprise, sinon le tray ne redémarrerait plus jamais après un
    plantage.
    """
    server = QLocalServer()
    if server.listen(name):
        return server
    probe = QLocalSocket()
    probe.connectToServer(name)
    if probe.waitForConnected(300):
        probe.abort()
        return None
    QLocalServer.removeServer(name)
    return server if server.listen(name) else None

def send(name: str, message: bytes = b"raise") -> bool:
    """Prévient l'instance déjà lancée ; False si personne n'écoute."""
    socket = QLocalSocket()
    socket.connectToServer(name)
    if not socket.waitForConnected(300):
        return False
    socket.write(message)
    socket.flush()
    socket.waitForBytesWritten(300)
    socket.disconnectFromServer()
    return True

def package_root() -> str:
    """Dossier à mettre dans `PYTHONPATH` (parent du paquet `gablue_manager`)."""
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def manager_command() -> list[str]:
    """Commande qui relance CETTE version de Gablue Manager.

    On n'appelle pas `gablue-manager` trouvé dans le PATH : sur une image
    antérieure à la version courante, ce binaire ignore ses arguments et ouvre
    la fenêtre principale même pour `--tray`. Un « Appliquer » sans changement
    dans les préférences relançait donc un tray qui affichait une seconde
    fenêtre (vécu 07/11/2026). Le module importé par ce processus est la seule
    version dont on connaît le comportement.
    """
    return [sys.executable, "-m", "gablue_manager"]

def spawn(argv: list[str]) -> bool:
    """Lance la commande détachée avec le bon `PYTHONPATH` ; False si échec."""
    env = dict(os.environ)
    root = package_root()
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = root if not existing else root + os.pathsep + existing
    try:
        subprocess.Popen(
            argv, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        return False
    return True
