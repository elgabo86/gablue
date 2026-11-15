"""Point d'entrée de Gablue Share (GUI)."""

import os
import sys

from PySide6.QtWidgets import QApplication

from . import __version__, config
from .constants import APP_NAME
from .style import stylesheet
from .window import MainWindow

HELP = f"""{APP_NAME} {__version__} — partage de dossiers sur le réseau local et le tailnet

Usage : gablue-share [options]

  --check    construit la fenêtre puis quitte (validation sans affichage)
  -h, --help cette aide

La liste des dossiers partagés vit dans ~/.config/gablue/share.json : elle
survit à la fermeture de l'application et au redémarrage du poste.
"""


def main() -> int:
    arguments = sys.argv[1:]
    if "-h" in arguments or "--help" in arguments:
        print(HELP)
        return 0

    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    app.setApplicationVersion(__version__)
    app.setDesktopFileName("gablue-share")
    app.setStyleSheet(stylesheet(app.palette()))

    window = MainWindow(autostart="--check" not in arguments)
    if "--check" in arguments:
        window.close()
        print("gablue-share : fenêtre construite, sortie de vérification")
        return 0

    # Premier lancement : le fichier de partages est créé pour que le démon
    # puisse démarrer même sans aucun dossier.
    config.load()
    window.show()
    return _leave(app, window, app.exec())


def _leave(app: QApplication, window: MainWindow, code: int) -> int:
    """Quitte sans laisser Qt démonter les fils de transfert.

    Un transfert en cours peut être bloqué dans une connexion que rien ne peut
    annuler (poste muet plutôt que « connexion refusée »). Qt détruirait alors
    le `QThread` encore vivant à la sortie du processus, ce qui l'**avorte**
    (SIGABRT, constaté le 15/11/2026). La file est déjà écrite et synchronisée
    (`fsync`), et le démon est lié à la vie de la fenêtre : on sort directement.
    """
    try:
        window.downloads.save()
    except OSError:
        pass  # disque plein ou chemin disparu : partir quand même
    app.processEvents()
    sys.stdout.flush()
    sys.stderr.flush()
    os._exit(code)
