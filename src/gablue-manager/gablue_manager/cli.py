"""Modes CLI de Gablue Manager : progression, confirmation, choix, mise à jour.

gwine délègue ses fenêtres au manager : ouvrir un `.exe`/`.wgp` sans préfixe
Wine lance l'initialisation **dans la fenêtre de progression** du manager, sans
ouvrir la fenêtre principale ni ses sondes SSH. Trois modes, sans interface
autre que la boîte demandée :

    gablue-manager --progress --title … [--subtitle …] [--auto-close-success]
                   -- commande [args…]
    gablue-manager --confirm --title … --text … [--yes …] [--no …]
    gablue-manager --choose --title … --text … --item valeur [--item …]

`--update` applique les mises à jour locales d'un domaine (ou de tous) avec la
même fenêtre de progression, dans son propre processus : c'est le menu de la
barre d'état qui s'en sert, pour mettre à jour **sans** ouvrir la fenêtre
principale. Un poste = une mise à jour à la fois (`instance.update_socket`).

`--progress` renvoie le code de sortie de la commande (130 si annulée),
`--confirm` 0 pour oui / 1 pour non, `--choose` imprime le choix (1 si annulé),
`--update` 0 si tout a réussi, 1 en échec, 130 si annulé et 4 si une autre mise
à jour tourne déjà. Sans affichage, `--progress` exécute la commande telle
quelle, `--confirm` répond non et `--update` refuse : gwine reste utilisable en
terminal.
"""

import os
import subprocess
import sys

from . import apps, local_updates, updates
from .local import parse_gablue_progress
from .style import make_qss
from .tasks import LocalTaskDialog, SequentialTaskDialog
from .uiutil import app_icon

MODES = ("progress", "confirm", "choose", "update")

# Codes de sortie de `--update` : 3 n'est pas utilisé (usage = 2).
EXIT_OK = 0
EXIT_FAILED = 1
EXIT_CANCELLED = 130
EXIT_BUSY = 4

class UsageError(Exception):
    """Option inconnue ou incomplète."""

def parse_args(argv: list[str]) -> dict:
    """Analyse `--progress|--confirm|--choose|--update` (parsing manuel).

    `--` termine les options : tout ce qui suit est la commande, y compris ses
    propres `--…` — indispensable pour `gablue-manager --progress -- gwine
    --init`.
    """
    if not argv or argv[0].lstrip("-") not in MODES:
        raise UsageError(
            "mode attendu : --progress, --confirm, --choose ou --update"
        )
    if argv[0].lstrip("-") == "update":
        return _parse_update(argv[1:])
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

def _parse_update(argv: list[str]) -> dict:
    """`--update <all|image|flatpak|gwine|apps|brew> [--auto-close-success]`.

    Un seul domaine, positionnel : la barre d'état lance une mise à jour par
    clic, et une valeur inconnue est refusée ici plutôt que de partir en
    « tout mettre à jour » par accident.
    """
    opts = {"mode": "update", "domain": "", "auto_close_success": False}
    for arg in argv:
        if arg == "--auto-close-success":
            opts["auto_close_success"] = True
        elif arg.startswith("-"):
            raise UsageError(f"option inconnue : {arg}")
        elif opts["domain"]:
            raise UsageError("un seul domaine attendu après --update")
        else:
            opts["domain"] = arg
    if not opts["domain"]:
        raise UsageError(
            "--update attend un domaine : "
            + ", ".join(local_updates.DOMAIN_CHOICES)
        )
    if opts["domain"] not in local_updates.DOMAIN_CHOICES:
        raise UsageError(
            f"domaine inconnu : {opts['domain']} (attendu : "
            + ", ".join(local_updates.DOMAIN_CHOICES) + ")"
        )
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

def _probe_apps():
    """Liste des AppImages en retard, ou None si l'utilisateur annule.

    Une fenêtre d'attente indéterminée remplace la bande d'activité de la
    fenêtre principale : un flux de releases par application installée, donc
    plusieurs secondes (jusqu'à 90 s au pire) avant de savoir quoi lancer. La
    boucle d'événements locale est indispensable — `worker.wait()` figerait la
    fenêtre et n'afficherait rien pendant la sonde.
    """
    from PySide6.QtCore import QEventLoop, Qt
    from PySide6.QtWidgets import QProgressDialog

    from .workers import AppsUpdateWorker

    busy = QProgressDialog(
        "Recherche des applications à mettre à jour…", "Annuler", 0, 0, None,
    )
    busy.setWindowTitle("Mises à jour")
    busy.setWindowModality(Qt.ApplicationModal)
    busy.setMinimumDuration(0)
    busy.setAutoClose(False)
    busy.setAutoReset(False)

    worker = AppsUpdateWorker()
    result: dict = {}
    loop = QEventLoop()

    def _mark_cancelled() -> None:
        # Annuler n'interrompt pas un flux déjà parti (`PROBE_TIMEOUT` borne
        # chaque requête) : on cesse d'attendre les suivants et on le dit.
        result["cancelled"] = True
        busy.setLabelText("Annulation…")
        busy.setCancelButton(None)
        worker.stop()

    worker.done.connect(lambda value: (result.update(value=value), loop.quit()))
    busy.canceled.connect(_mark_cancelled)
    busy.show()
    worker.start()
    loop.exec()
    worker.wait(5000)
    busy.hide()
    pending, _unknown, _installed = result.get("value") or ([], True, 0)
    if result.get("cancelled"):
        return None
    return [str(key) for key in pending]

def run_update(opts: dict) -> int:
    """Empile les étapes d'un domaine (ou de tous) et les applique.

    Aucune fenêtre principale : la liste vient de `local_updates`, la même que
    celle de la fenêtre. Le canal `instance.update_socket()` réserve le poste —
    plusieurs `flatpak update` ou `bootc upgrade` simultanés se marcheraient
    dessus. En fin de passe, la barre d'état est prévenue (`updates`) : sa
    pastille ne doit pas garder l'état d'avant jusqu'à sa prochaine sonde.
    """
    domain = opts["domain"]
    if not _has_display():
        print("gablue-manager : mise à jour impossible sans affichage "
              "(Wayland/X11).", file=sys.stderr)
        return EXIT_FAILED

    from . import instance

    server = instance.claim(instance.update_socket())   # noqa: F841 — verrou
    if server is None:
        print("gablue-manager : une mise à jour est déjà en cours sur ce "
              "poste.", file=sys.stderr)
        return EXIT_BUSY

    app = _qt_app()
    pending = _probe_apps() if local_updates.needs_apps_probe(domain) else []
    if pending is None:
        return EXIT_CANCELLED

    title = local_updates.TITLES.get(domain, "Mises à jour")
    dialog = None
    if domain == "all":
        built = local_updates.steps(pending)
        if not built:
            print("gablue-manager : rien à mettre à jour sur ce poste.",
                  file=sys.stderr)
            return EXIT_OK
        dialog = SequentialTaskDialog(
            None, title=title,
            subtitle=f"{len(built)} mises à jour enchaînées sur ce poste.",
            steps=built,
            close_on_success=opts["auto_close_success"],
            offer_reboot=True,
        )
    else:
        step = local_updates.step_for(domain, pending)
        if step is None:
            print(f"gablue-manager : {updates.DOMAIN_LABELS[domain]} — outil "
                  "absent ou rien à mettre à jour.", file=sys.stderr)
            return EXIT_OK
        subtitle = local_updates.SUBTITLES.get(domain, "")
        if domain == "apps":
            count = len(pending)
            names = ", ".join(apps.names_for(pending))
            subtitle = (f"1 application : {names}." if count == 1
                        else f"{count} applications : {names}.")
        dialog = LocalTaskDialog(
            None, title=title, subtitle=subtitle,
            close_on_success=opts["auto_close_success"],
            reveal_log_on_error=True,
            offer_reboot=True,
            **local_updates.local_task_kwargs(step),
        )

    outcome = {"ok": False}
    dialog.completed.connect(lambda ok: outcome.update(ok=bool(ok)))
    dialog.finished.connect(lambda _code=None: app.quit())
    dialog.show()
    dialog.raise_()
    dialog.activateWindow()
    app.exec()

    # La fenêtre a sondé et s'affiche déjà : la pastille du tray reprend la
    # main sans attendre son créneau (message `updates`).
    instance.send(instance.tray_socket(), b"updates")
    if dialog.cancelled:
        return EXIT_CANCELLED
    return EXIT_OK if outcome["ok"] else EXIT_FAILED

_HANDLERS = {
    "progress": run_progress,
    "confirm": run_confirm,
    "choose": run_choose,
    "update": run_update,
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
