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

def update_socket() -> str:
    """Nom du canal qui réserve « une mise à jour à la fois » sur ce poste.

    Le mode `--update` le revendique pour toute sa durée : deux `bootc upgrade`
    ou deux `flatpak update` simultanés se marcheraient dessus (verrou dpkg-like
    d'ostree, cache de brew), et la fenêtre principale peut lancer la même mise
    à jour que le tray.
    """
    return f"gablue-manager-update-{os.getuid()}"

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

def release(server) -> None:
    """Rend un canal pris par `claim` : le ferme et retire sa socket.

    Utilisé par la fenêtre pour le verrou de mise à jour, qu'elle garde le temps
    d'une tâche (le mode `--update`, lui, le garde jusqu'à la fin du processus).
    """
    if server is None:
        return
    name = server.serverName()
    server.close()
    QLocalServer.removeServer(name)

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

def manager_argv(*args: str) -> list[str]:
    """`manager_command()` suivi d'arguments : un mode CLI à relancer."""
    return [*manager_command(), *args]

def package_environment() -> dict:
    """Environnement d'un enfant qui doit importer CE paquet (`PYTHONPATH`)."""
    env = dict(os.environ)
    root = package_root()
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = root if not existing else root + os.pathsep + existing
    return env

def spawn(argv: list[str]) -> bool:
    """Lance la commande détachée avec le bon `PYTHONPATH` ; False si échec."""
    try:
        subprocess.Popen(
            argv, env=package_environment(), stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        return False
    return True
