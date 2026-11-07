"""Modes CLI de Gablue Manager : progression, confirmation, choix.

gwine délègue ses fenêtres au manager : ouvrir un `.exe`/`.wgp` sans préfixe
Wine lance l'initialisation **dans la fenêtre de progression** du manager, sans
ouvrir la fenêtre principale ni ses sondes SSH. Trois modes, sans interface
autre que la boîte demandée :

    gablue-manager --progress --title … [--subtitle …] [--auto-close-success]
                   -- commande [args…]
    gablue-manager --confirm --title … --text … [--yes …] [--no …]
    gablue-manager --choose --title … --text … --item valeur [--item …]

`--progress` renvoie le code de sortie de la commande (130 si annulée),
`--confirm` 0 pour oui / 1 pour non, `--choose` imprime le choix (1 si annulé).
Sans affichage, `--progress` exécute la commande telle quelle et `--confirm`
répond non : gwine reste utilisable en terminal.
"""

import os
import subprocess
import sys

from .local import parse_gablue_progress
from .style import make_qss
from .tasks import LocalTaskDialog
from .uiutil import app_icon

MODES = ("progress", "confirm", "choose")

class UsageError(Exception):
    """Option inconnue ou incomplète."""

def parse_args(argv: list[str]) -> dict:
    """Analyse `--progress|--confirm|--choose` (parsing manuel, sans argparse).

    `--` termine les options : tout ce qui suit est la commande, y compris ses
    propres `--…` — indispensable pour `gablue-manager --progress -- gwine
    --init`.
    """
    if not argv or argv[0].lstrip("-") not in MODES:
        raise UsageError("mode attendu : --progress, --confirm ou --choose")
    opts = {
        "mode": argv[0].lstrip("-"),
        "title": "",
        "subtitle": "",
        "text": "",
        "yes": "Oui",
        "no": "Non",
        "items": [],
        "auto_close": False,
        "auto_close_success": False,
        "command": [],
    }
    values = {
        "--title": "title", "--subtitle": "subtitle", "--text": "text",
        "--yes": "yes", "--no": "no",
    }
    index = 1
    while index < len(argv):
        arg = argv[index]
        if arg == "--":
            opts["command"] = argv[index + 1:]
            break
        if arg in values:
            if index + 1 >= len(argv):
                raise UsageError(f"{arg} attend une valeur")
            opts[values[arg]] = argv[index + 1]
            index += 2
            continue
        if arg == "--item":
            if index + 1 >= len(argv):
                raise UsageError("--item attend une valeur")
            opts["items"].append(argv[index + 1])
            index += 2
            continue
        if arg == "--auto-close":
            opts["auto_close"] = True
        elif arg == "--auto-close-success":
            opts["auto_close_success"] = True
        else:
            raise UsageError(f"option inconnue : {arg}")
        index += 1
    return opts

def _has_display() -> bool:
    return bool(os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY"))

def _qt_app():
    """QApplication minimale : le dialogue seul, aucun sondage de postes."""
    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv[:1])
    app.setApplicationName("gablue-manager")
    app.setApplicationDisplayName("Gablue Manager")
    app.setDesktopFileName("gablue-manager")
    app.setOrganizationName("Gablue")
    app.setWindowIcon(app_icon())
    app.setStyleSheet(make_qss())
    return app

def run_progress(opts: dict) -> int:
    command = opts["command"]
    if not command:
        raise UsageError("--progress attend une commande après « -- »")
    if not _has_display():
        # Sans session graphique, le dialogue n'existe pas : la commande tourne
        # directement (et sans protocole, pour un journal lisible).
        env = dict(os.environ)
        env.pop("GABLUE_PROGRESS", None)
        try:
            return subprocess.run(command, env=env).returncode
        except FileNotFoundError:
            print(f"gablue-manager : commande introuvable : {command[0]}",
                  file=sys.stderr)
            return 127
        except OSError as exc:
            print(f"gablue-manager : lancement impossible : {exc}", file=sys.stderr)
            return 126

    app = _qt_app()
    dialog = LocalTaskDialog(
        None,
        title=opts["title"] or "Opération en cours",
        subtitle=opts["subtitle"],
        argv=command,
        progress_parser=parse_gablue_progress,
        # gwine émet son protocole `::gablue:` et sa version lisible en clair.
        env_extra={"GABLUE_PROGRESS": "1"},
        close_on_success=opts["auto_close_success"],
        reveal_log_on_error=True,
    )
    if opts["auto_close"]:
        dialog.completed.connect(lambda _ok: dialog.accept())
    dialog.finished.connect(lambda _code=0: app.quit())
    dialog.show()
    dialog.raise_()
    dialog.activateWindow()
    app.exec()
    if dialog.cancelled:
        return 130
    if dialog.exit_code is None:
        return 1
    return dialog.exit_code

def run_confirm(opts: dict) -> int:
    if not _has_display():
        print(opts["text"], file=sys.stderr)
        return 1
    from PySide6.QtWidgets import QMessageBox

    app = _qt_app()                  # noqa: F841 — garde l'application en vie
    box = QMessageBox()
    box.setWindowTitle(opts["title"] or "Gablue Manager")
    box.setText(opts["text"])
    box.setIcon(QMessageBox.Question)
    yes = box.addButton(opts["yes"], QMessageBox.YesRole)
    no = box.addButton(opts["no"], QMessageBox.NoRole)
    box.setDefaultButton(no)
    box.exec()
    return 0 if box.clickedButton() is yes else 1

def run_choose(opts: dict) -> int:
    if not opts["items"]:
        raise UsageError("--choose attend au moins un --item")
    if not _has_display():
        return 1
    from PySide6.QtWidgets import QInputDialog

    app = _qt_app()                  # noqa: F841 — garde l'application en vie
    values: list[str] = []
    labels: list[str] = []
    for item in opts["items"]:
        value, _sep, label = item.partition("\t")
        values.append(value)
        labels.append(label or value)
    choice, ok = QInputDialog.getItem(
        None,
        opts["title"] or "Gablue Manager",
        opts["text"] or "Choisir :",
        labels, 0, False,
    )
    if not ok:
        return 1
    print(values[labels.index(choice)])
    return 0

_HANDLERS = {
    "progress": run_progress,
    "confirm": run_confirm,
    "choose": run_choose,
}

def run(argv: list[str]) -> int:
    """Point d'entrée des modes CLI ; renvoie le code de sortie du processus."""
    try:
        opts = parse_args(argv)
        return _HANDLERS[opts["mode"]](opts)
    except UsageError as exc:
        print(f"gablue-manager : {exc}", file=sys.stderr)
        print("Voir gablue-manager --help.", file=sys.stderr)
        return 2
