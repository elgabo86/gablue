"""Icône de barre d'état : Gablue Manager accessible sans sa fenêtre.

`gablue-manager --tray` ne charge **que** l'icône : pas de `MainWindow`, pas
de registre des postes, aucune sonde SSH. La fenêtre complète est lancée à la
demande, dans son propre processus (`instance.py` garantit l'unicité).

L'icône porte une **pastille rouge** dès qu'un événement attend l'utilisateur
— aujourd'hui un redémarrage requis (`pending_reboot_reasons`) ; les futurs
contrôles de mises à jour s'ajoutent à `Tray.events`. Le menu donne l'accès
direct et « Quitter ».
"""

import sys

from PySide6.QtCore import QProcess, QTimer, Qt
from PySide6.QtGui import QColor, QIcon, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon

from . import instance, prefs
from .uiutil import app_icon
from .workers import RebootWorker

REFRESH_MS = 30_000
BADGE_COLOR = "#e01b24"        # rouge KDE, lisible sur l'icône bleue

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
        self._server = None

        self.menu = QMenu()
        self.open_action = self.menu.addAction("Ouvrir Gablue Manager")
        self.open_action.triggered.connect(self.open_manager)
        self.event_action = self.menu.addAction("")
        self.event_action.setEnabled(False)
        self.event_action.setVisible(False)
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
        self.timer.setInterval(REFRESH_MS)
        self.timer.timeout.connect(self.refresh)
        self.timer.start()
        self.app.aboutToQuit.connect(self._shutdown)

    def _shutdown(self) -> None:
        """Laisse la sonde en cours finir avant de détruire son QThread."""
        self.timer.stop()
        if self._worker is not None:
            self._worker.wait(5000)
            self._worker = None

    # -- accès ------------------------------------------------------------
    def open_manager(self) -> None:
        """Ramène la fenêtre existante, ou la lance dans son processus."""
        if instance.send(instance.gui_socket(), b"raise"):
            return
        command = instance.manager_command()
        QProcess.startDetached(command[0], command[1:])

    def _activated(self, reason) -> None:
        if reason in (QSystemTrayIcon.Trigger, QSystemTrayIcon.DoubleClick):
            self.open_manager()

    # -- événements (redémarrage requis, mises à jour à venir) ------------
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
        self._update()

    def set_events(self, events: list[str]) -> None:
        """Point d'entrée des futurs contrôles (mises à jour, etc.)."""
        self.events = [str(event) for event in events]
        self._update()

    def _update(self) -> None:
        alert = bool(self.events)
        self.icon.setIcon(badge_icon(self.base_icon, alert))
        tooltip = "Gablue Manager"
        if alert:
            tooltip += " — " + " · ".join(self.events)
        self.icon.setToolTip(tooltip)
        if alert:
            self.event_action.setText("Redémarrage requis — " + self.events[0])
        self.event_action.setVisible(alert)

def _quit_request(server, app: QApplication) -> None:
    """La fenêtre a désactivé l'icône : elle demande au tray de s'arrêter."""
    socket = server.nextPendingConnection()
    if socket is None:
        return
    socket.disconnected.connect(socket.deleteLater)
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
    server.newConnection.connect(lambda: _quit_request(server, app))
    tray.icon.show()
    tray.refresh()
    _TRAY = tray
    return app.exec()
