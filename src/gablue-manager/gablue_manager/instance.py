"""Instance unique : une fenêtre et une icône de barre d'état par session.

Le tray et la fenêtre sont deux processus distincts, mais chacun doit rester
unique : un second clic sur l'icône doit ramener la fenêtre existante, pas en
ouvrir une deuxième. Le canal est un `QLocalServer` par utilisateur — pas de
D-Bus à installer, et une socket laissée par un processus tué est reprise.
"""

import os
import shutil
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

def manager_command() -> list[str]:
    """Commande qui ouvre la fenêtre principale (image ou dépôt).

    L'image installe `/usr/bin/gablue-manager` ; en développement, on relance
    le lanceur du dépôt lui-même, et `python3 -m gablue_manager` en dernier
    recours.
    """
    installed = shutil.which("gablue-manager")
    if installed:
        return [installed]
    launcher = os.path.abspath(sys.argv[0]) if sys.argv and sys.argv[0] else ""
    if launcher and os.path.isfile(launcher) and os.access(launcher, os.X_OK):
        return [launcher]
    return [sys.executable, "-m", "gablue_manager"]
