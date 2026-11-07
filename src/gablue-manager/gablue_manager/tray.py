"""Icône de barre d'état : Gablue Manager accessible sans sa fenêtre.

`gablue-manager --tray` ne charge **que** l'icône : pas de `MainWindow`, pas
de registre des postes, aucune sonde SSH. La fenêtre complète est lancée à la
demande, dans son propre processus (`instance.py` garantit l'unicité).

L'icône porte une **pastille rouge** dès qu'un événement attend l'utilisateur —
un redémarrage requis, ou des mises à jour disponibles — et son infobulle dit
lesquelles. La vérification des mises à jour est **économe** : jamais hors
ligne (`QNetworkInformation`, alimenté par NetworkManager), selon les domaines
et la fréquence choisis dans les préférences, et à la demande par le menu.
"""

import sys
import time

from PySide6.QtCore import QProcess, QTimer, Qt
from PySide6.QtGui import QColor, QIcon, QPainter, QPen, QPixmap
from PySide6.QtNetwork import QNetworkInformation
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon

from . import instance, prefs, updates
from .uiutil import app_icon, network_information, network_online
from .workers import RebootWorker, UpdateCheckWorker

REBOOT_MS = 300_000           # 5 min : assez frais, sans réveil inutile
BADGE_COLOR = "#e01b24"       # rouge KDE, lisible sur l'icône bleue
MIN_RECHECK = 300             # pas de re-sonde sur un flottement de réseau

# Référence du tray vivant : `run()` en garde une, sinon Python peut libérer
# l'objet pendant que la boucle Qt tourne (le menu et le timer avec lui).
_TRAY: "Tray | None" = None

def badge_icon(base: QIcon, alert: bool) -> QIcon:
    """Icône du manager, avec une pastille rouge quand un événement attend."""
    if not alert:
        return base
    icon = QIcon()
    for size in (16, 22, 24, 32, 48, 64, 128, 256):
        pixmap = base.pixmap(size, size)
        if pixmap.isNull():
            continue
        canvas = QPixmap(pixmap.size())
        canvas.fill(Qt.transparent)
        painter = QPainter(canvas)
        painter.setRenderHint(QPainter.Antialiasing)
        painter.drawPixmap(0, 0, pixmap)
        # Pastille en bas à droite, proportionnelle à la taille demandée.
        side = max(5, int(pixmap.width() * 0.42))
        border = max(1, side // 8)
        painter.setPen(QPen(QColor(255, 255, 255, 220), border))
        painter.setBrush(QColor(BADGE_COLOR))
        painter.drawEllipse(
            pixmap.width() - side - border,
            pixmap.height() - side - border,
            side, side,
        )
        painter.end()
        icon.addPixmap(canvas)
    return icon if not icon.isNull() else base

class Tray:
    """Icône de barre d'état et état local, sans fenêtre principale."""

    def __init__(self, app: QApplication):
        self.app = app
        self.base_icon = app_icon()
        self.events: list[str] = []
        self._worker: RebootWorker | None = None
        self._update_worker: UpdateCheckWorker | None = None
        self.updates_state: dict | None = None
        self._last_check: float = 0.0
        self._offline = False
        self._server = None

        self.menu = QMenu()
        self.open_action = self.menu.addAction("Ouvrir Gablue Manager")
        self.open_action.triggered.connect(self.open_manager)
        self.event_action = self.menu.addAction("")
        self.event_action.setEnabled(False)
        self.event_action.setVisible(False)
        self.updates_action = self.menu.addAction("")
        self.updates_action.setEnabled(False)
        self.updates_action.setVisible(False)
        self.check_action = self.menu.addAction("Vérifier les mises à jour")
        self.check_action.triggered.connect(lambda: self.refresh_updates(True))
        self.menu.addSeparator()
        quit_action = self.menu.addAction("Quitter")
        quit_action.triggered.connect(self.app.quit)

        self.icon = QSystemTrayIcon(
            badge_icon(self.base_icon, False), self.app
        )
        self.icon.setToolTip("Gablue Manager")
        self.icon.setContextMenu(self.menu)
        self.icon.activated.connect(self._activated)

        self.timer = QTimer(self.app)
        self.timer.setInterval(REBOOT_MS)
        self.timer.timeout.connect(self.refresh)
        self.timer.start()

        # Fréquence de la sonde de mises à jour : relue dans les préférences.
        self.update_timer = QTimer(self.app)
        self.update_timer.timeout.connect(lambda: self.refresh_updates())
        self.schedule_updates()

        info = network_information()
        if info is not None:
            info.reachabilityChanged.connect(self._reachability_changed)

        self.app.aboutToQuit.connect(self._shutdown)

    def _shutdown(self) -> None:
        """Laisse les sondes en cours finir avant de détruire leurs QThread."""
        self.timer.stop()
        self.update_timer.stop()
        for worker in (self._worker, self._update_worker):
            if worker is not None:
                worker.wait(5000)
        self._worker = None
        self._update_worker = None

    # -- accès ------------------------------------------------------------
    def open_manager(self) -> None:
        """Ramène la fenêtre existante, ou la lance dans son processus."""
        if instance.send(instance.gui_socket(), b"raise"):
            return
        command = instance.manager_command()
        QProcess.startDetached(command[0], command[1:])

    def _activated(self, reason) -> None:
        if reason in (QSystemTrayIcon.Trigger, QSystemTrayIcon.DoubleClick):
            self.toggle_manager()

    def toggle_manager(self) -> None:
        """Clic sur l'icône : ferme la fenêtre si elle est ouverte, sinon l'ouvre.

        `toggle` est traité par la fenêtre elle-même (c'est elle qui sait si
        elle est visible) ; personne n'écoute → on la lance.
        """
        if instance.send(instance.gui_socket(), b"toggle"):
            return
        command = instance.manager_command()
        QProcess.startDetached(command[0], command[1:])

    # -- redémarrage requis -----------------------------------------------
    def refresh(self) -> None:
        if self._worker is not None and self._worker.isRunning():
            return
        worker = RebootWorker()
        worker.done.connect(self._reasons)
        worker.finished.connect(worker.deleteLater)
        self._worker = worker
        worker.start()

    def _reasons(self, reasons: list[str]) -> None:
        self._worker = None
        self.events = [str(reason) for reason in reasons]
        self._update_icon()

    # -- mises à jour disponibles -----------------------------------------
    def schedule_updates(self) -> None:
        """Cale la fréquence sur les préférences (minutes, bornée)."""
        values = prefs.load_prefs()
        self.update_timer.setInterval(prefs.check_interval_seconds(values))
        if prefs.checks_enabled(values):
            if not self.update_timer.isActive():
                self.update_timer.start()
        else:
            self.update_timer.stop()

    def refresh_updates(self, force: bool = False) -> None:
        """Sonde les mises à jour — jamais sans réseau, jamais sans demande.

        `force` vient du menu : il ignore l'interrupteur général et la
        fréquence, mais reste soumis à la connexion (hors ligne, on n'essaie
        même pas).
        """
        values = prefs.load_prefs()
        if not force and not prefs.checks_enabled(values):
            return
        if self._update_worker is not None and self._update_worker.isRunning():
            return
        online = network_online()
        if not online:
            # Aucune sonde hors ligne. On conserve le dernier état connu — une
            # image déjà téléchargée reste à redémarrer — et on ne date pas la
            # passe : le retour de la connexion re-sonde tout de suite.
            self._offline = True
            if self.updates_state is None:
                self.updates_state = {"online": False, "domains": {}}
            self._update_icon()
            return
        self._offline = False
        if not force and self._last_check and \
                time.time() - self._last_check < prefs.check_interval_seconds(values):
            return
        domains = prefs.check_domains(values) if force \
            else prefs.auto_check_domains(values)
        worker = UpdateCheckWorker(domains, online)
        self._update_worker = worker
        worker.done.connect(self._updates_done)
        worker.finished.connect(worker.deleteLater)
        self.check_action.setEnabled(False)
        self.check_action.setText("Vérification des mises à jour…")
        worker.start()

    def _updates_done(self, state: dict) -> None:
        self._update_worker = None
        self.updates_state = state
        self._offline = False
        self._last_check = (state or {}).get("checked_at") or time.time()
        self.check_action.setEnabled(True)
        self.check_action.setText("Vérifier les mises à jour")
        self._update_icon()

    def _reachability_changed(self, reachability) -> None:
        """La connexion revient : une sonde si le dernier état est ancien."""
        if reachability != QNetworkInformation.Reachability.Online:
            return
        if time.time() - self._last_check < MIN_RECHECK:
            return
        self.refresh_updates()

    def reload(self) -> None:
        """Les préférences ont changé (fenêtre) : relire et re-sonder."""
        self.schedule_updates()
        self._last_check = 0.0
        values = prefs.load_prefs()
        if not prefs.checks_enabled(values):
            self.updates_state = None
            self._update_icon()
            return
        self.refresh_updates()

    # -- affichage ---------------------------------------------------------
    def set_events(self, events: list[str]) -> None:
        """Point d'entrée d'autres événements (redémarrage, futur contrôle)."""
        self.events = [str(event) for event in events]
        self._update_icon()

    def _update_icon(self) -> None:
        pending = updates.has_updates(self.updates_state)
        alert = bool(self.events) or pending
        self.icon.setIcon(badge_icon(self.base_icon, alert))

        lines: list[str] = []
        if self.events:
            lines.extend(self.events)
        lines.extend(updates.summary_lines(self.updates_state))
        if self._offline:
            lines.append("Hors ligne — vérification des mises à jour en pause.")
        tooltip = "Gablue Manager"
        if lines:
            tooltip += "\n" + "\n".join(lines)
        self.icon.setToolTip(tooltip)

        if self.events:
            self.event_action.setText("Redémarrage requis — " + self.events[0])
        self.event_action.setVisible(bool(self.events))

        total = updates.total_updates(self.updates_state)
        if pending:
            text = f"{total} mise{'s' if total > 1 else ''} à jour disponible" \
                f"{'s' if total > 1 else ''}"
        elif self.updates_state and self.updates_state.get("online") is False:
            text = "Mises à jour : hors ligne"
        elif self.updates_state:
            text = "Aucune mise à jour disponible"
        else:
            text = ""
        self.updates_action.setText(text)
        self.updates_action.setVisible(bool(text))

def _tray_message(tray: Tray, app: QApplication, server) -> None:
    """Message de la fenêtre : `quit` (icône coupée) ou `reload` (préférences)."""
    socket = server.nextPendingConnection()
    if socket is None:
        return
    # Blocage court et borné : la fenêtre écrit puis ferme, 200 ms suffisent,
    # et `readyRead` n'est pas fiable pour des données déjà tamponnées.
    if not socket.waitForReadyRead(200):
        socket.deleteLater()
        return
    message = bytes(socket.readAll()).strip()
    socket.deleteLater()
    if message == b"reload":
        tray.reload()
    else:
        app.quit()

def run() -> int:
    """Mode `--tray` : icône seule, aucun chargement de la fenêtre."""
    global _TRAY
    app = QApplication(sys.argv)
    app.setApplicationName("gablue-manager")
    app.setApplicationDisplayName("Gablue Manager")
    app.setDesktopFileName("gablue-manager")
    app.setOrganizationName("Gablue")
    app.setWindowIcon(app_icon())
    app.setQuitOnLastWindowClosed(False)

    if not prefs.tray_enabled():
        return 0
    if not QSystemTrayIcon.isSystemTrayAvailable():
        return 0          # pas de barre d'état : ne pas rester fantôme

    tray = Tray(app)
    server = instance.claim(instance.tray_socket())
    if server is None:
        return 0          # une icône tourne déjà dans cette session
    tray._server = server
    server.newConnection.connect(lambda: _tray_message(tray, app, server))
    tray.icon.show()
    tray.refresh()
    tray.refresh_updates()          # à l'ouverture de session
    _TRAY = tray
    return app.exec()
