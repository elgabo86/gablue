"""Point d'entrée de Gablue Manager.

Sans argument : fenêtre principale. Les modes `--progress`, `--confirm` et
`--choose` (voir `cli.py`) ouvrent une seule boîte et rendent un code : gwine
s'en sert pour l'initialisation du préfixe Wine, à la place de kdialog.
"""

import os
import sys

HELP = """Usage : gablue-manager [mode]

Sans option : fenêtre principale (postes SSH et machine courante).

Postes distants : liste des machines dont la clé a été installée, état en ligne/
hors ligne et latence, bureau distant (gremote), fichiers (Dolphin en sftp://),
terminal SSH, arrêt, redémarrage et mise à jour, recherche
des postes sur le réseau local, ajout d'un poste avec copie de la clé.

Cette machine : serveur SSH et annonce locale, mises à jour (image Gablue,
Flatpaks, gwine), réglages du noyau (protections du processeur, correctif CPUID,
mode ERTM, émulation CPUID fault), options de gwine (DXVK, affichage, manettes),
priorité au premier plan et Tailscale (lien du serveur demandé) — les
changements de chaque dialogue partent ensemble. Applications AppImage, sur ce
poste comme à distance (menu ⋯ d'un poste) : installation, mise à jour et
suppression d'ES-DE, Azahar, Eden, Xenia, ShadPS4 et OpenCode 2 (configuration,
sauvegardes et sessions conservées). Les opérations s'ouvrent dans leur propre
fenêtre : plusieurs postes peuvent être traités en même temps.

Modes sans fenêtre principale (utilisés par gwine) :
  --progress --title T [--subtitle T] [--auto-close-success] -- commande…
      Exécute la commande dans la fenêtre de progression du manager, journal
      derrière le bouton « Terminal ». La commande publie ses étapes sur sa
      sortie : « ::gablue:step n total texte », « ::gablue:busy texte » quand
      le total est inconnu, « ::gablue:done » pour finir. --auto-close-success
      ferme la fenêtre quand la commande réussit (échec : elle reste ouverte
      avec le journal) ; --auto-close ferme dans tous les cas.
  --confirm --title T --text T [--yes T] [--no T]
      Pose une question ; sortie 0 = oui, 1 = non.
  --choose --title T --text T --item valeur [--item valeur…]
      Fait choisir une valeur (affichée après une tabulation si le libellé
      diffère) et l'imprime sur la sortie ; sortie 1 si annulé.

Configuration : ~/.config/gablue/ssh-hosts.json (mode 0600)
Clé locale    : ~/.ssh/id_ed25519 (créée au besoin, sans passphrase)
"""

def main() -> int:
    args = sys.argv[1:]
    # --help après « -- » appartient à la commande, pas à nous.
    head = args[:args.index("--")] if "--" in args else args
    if any(arg in ("-h", "--help") for arg in head):
        print(HELP)
        return 0
    if args:
        from .cli import run
        return run(args)
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        print("gablue-manager : aucun affichage (Wayland/X11) disponible.",
              file=sys.stderr)
        return 1

    from PySide6.QtWidgets import QApplication

    from .uiutil import app_icon
    from .window import MainWindow

    app = QApplication(sys.argv)
    app.setApplicationName("gablue-manager")
    app.setApplicationDisplayName("Gablue Manager")
    app.setDesktopFileName("gablue-manager")
    app.setOrganizationName("Gablue")
    app.setWindowIcon(app_icon())

    window = MainWindow()
    window.show()
    return app.exec()
