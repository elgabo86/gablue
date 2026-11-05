"""Point d'entrée de Gablue Manager.
"""

import os
import sys

from PySide6.QtGui import QIcon
from PySide6.QtWidgets import QApplication

from .window import MainWindow

HELP = """Usage : gablue-manager

Gablue Manager : postes SSH et machine courante au même endroit.

Postes distants : liste des machines dont la clé a été installée, état en ligne/
hors ligne et latence, bureau distant (gremote), fichiers (Dolphin en sftp://),
terminal SSH, réveil Wake-on-LAN, arrêt, redémarrage et mise à jour, découverte
mDNS, import de ~/.ssh/config, ajout d'un poste avec copie de la clé.

Cette machine : serveur SSH et annonce mDNS, mises à jour (image Gablue,
Flatpaks, gwine), réglages du noyau (protections du processeur, correctif CPUID,
mode ERTM, émulation CPUID fault), options de gwine (DXVK, affichage, manettes),
priorité au premier plan et Tailscale (lien du serveur demandé) — les
changements de chaque dialogue partent ensemble. Les opérations s'ouvrent dans
leur propre fenêtre : plusieurs postes peuvent être traités en même temps.

Configuration : ~/.config/gablue/ssh-hosts.json (mode 0600)
Clé locale    : ~/.ssh/id_ed25519 (créée au besoin, sans passphrase)
"""

def main() -> int:
    if any(arg in ("-h", "--help") for arg in sys.argv[1:]):
        print(HELP)
        return 0
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        print("gablue-manager : aucun affichage (Wayland/X11) disponible.",
              file=sys.stderr)
        return 1

    app = QApplication(sys.argv)
    app.setApplicationName("gablue-manager")
    app.setApplicationDisplayName("Gablue Manager")
    app.setDesktopFileName("gablue-manager")
    app.setOrganizationName("Gablue")
    app.setWindowIcon(QIcon.fromTheme("network-server"))

    window = MainWindow()
    window.show()
    return app.exec()
