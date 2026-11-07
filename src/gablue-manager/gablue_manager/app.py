"""Point d'entrée de Gablue Manager.

Sans argument : fenêtre principale. Les modes `--progress`, `--confirm` et
`--choose` (voir `cli.py`) ouvrent une seule boîte et rendent un code : gwine
s'en sert pour l'initialisation du préfixe Wine, à la place de kdialog.
"""

import os
import sys

from . import journal

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

  --tray
      Icône de barre d'état seule (démarrage de session). Un clic ouvre la
      fenêtre ; une pastille rouge signale un redémarrage requis ou une mise
      à jour. La fenêtre n'est chargée qu'au clic : aucune sonde SSH ni
      registre des postes avant.

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
                ~/.config/gablue/preferences.json (barre d'état)
Clé locale    : ~/.ssh/id_ed25519 (créée au besoin, sans passphrase)
"""

def _has_display() -> bool:
    return bool(os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY"))

def _window_message(server, window) -> None:
    """Message d'une autre instance : `raise` la ramène, `toggle` la ferme.

    `toggle` vient d'un clic sur l'icône de barre d'état : fenêtre ouverte →
    elle se ferme (et l'app se termine, plus rien ne tourne en fond) ;
    fenêtre absente → elle s'ouvre. `raise` (menu du tray, second lancement)
    se contente de la ramener devant.
    """
    socket = server.nextPendingConnection()
    if socket is None:
        return
    if not socket.waitForReadyRead(200):
        # Ancien appelant qui n'écrit rien : comportement historique.
        socket.deleteLater()
        window.show()
        window.raise_()
        window.activateWindow()
        return
    message = bytes(socket.readAll()).strip()
    socket.deleteLater()
    if message == b"toggle" and window.isVisible():
        window.close()
        return
    window.show()
    window.raise_()
    window.activateWindow()

def main() -> int:
    # Une exception non attrapée doit laisser une trace : la fenêtre peut être
    # lancée depuis le menu, sans terminal pour lire sa sortie d'erreur.
    journal.install_excepthook()
    args = sys.argv[1:]
    # --help après « -- » appartient à la commande, pas à nous.
    head = args[:args.index("--")] if "--" in args else args
    if any(arg in ("-h", "--help") for arg in head):
        print(HELP)
        return 0
    if args == ["--tray"]:
        if not _has_display():
            return 0        # démarrage de session sans affichage : rien à faire
        from .tray import run
        return run()
    if args:
        from .cli import run
        return run(args)
    if not _has_display():
        print("gablue-manager : aucun affichage (Wayland/X11) disponible.",
              file=sys.stderr)
        return 1

    from PySide6.QtWidgets import QApplication

    from . import instance
    from .uiutil import app_icon
    from .window import MainWindow

    app = QApplication(sys.argv)
    app.setApplicationName("gablue-manager")
    app.setApplicationDisplayName("Gablue Manager")
    app.setDesktopFileName("gablue-manager")
    app.setOrganizationName("Gablue")
    app.setWindowIcon(app_icon())

    server = instance.claim(instance.gui_socket())
    if server is None:
        instance.send(instance.gui_socket(), b"raise")
        return 0
    window = MainWindow()
    server.newConnection.connect(lambda: _window_message(server, window))
    window.show()
    return app.exec()
