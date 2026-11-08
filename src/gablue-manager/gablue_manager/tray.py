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

from PySide6.QtCore import QTimer, Qt
from PySide6.QtGui import QColor, QIcon, QPainter, QPen, QPixmap
from PySide6.QtNetwork import QNetworkInformation
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon

from . import instance, prefs, updates
from .uiutil import app_icon, network_information, network_online
from .workers import RebootWorker, UpdateCheckWorker

REBOOT_MS = 300_000           # 5 min : assez frais, sans réveil inutile
BADGE_COLOR = "#e01b24"       # rouge KDE, lisible sur l'icône bleue
MIN_RECHECK = 300             # pas de re-sonde sur un flottement de réseau
NOTIFY_MS = 8000              # notification brève : le temps de lire une ligne

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
        # 34 % : lisible sans manger l'icône (42 % faisait une tache).
        side = max(5, int(pixmap.width() * 0.34))
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

# -- présentation de l'état ------------------------------------------------
#
# L'infobulle et le menu montrent les mêmes informations, mais l'infobulle
# n'accepte que du texte : lignes courtes, une par élément. Les raisons de
# redémarrage arrivent en minuscules depuis les sondes ; on les met en phrase
# et on supprime le doublon avec la ligne « Image Gablue » (l'image staged y
# est déjà décrite avec sa version).

def _sentence(text: str) -> str:
    text = (text or "").strip()
    return text[:1].upper() + text[1:] if text else ""


def _staged_image(state: dict | None) -> tuple[bool, str]:
    """Image déjà téléchargée ? (bool, version de l'image staged)."""
    image = ((state or {}).get("domains") or {}).get("image") or {}
    if image.get("state") != "available" or not image.get("staged"):
        return False, ""
    version = str(image.get("staged_version") or image.get("version") or "")
    return True, version


def tray_lines(events: list[str], state: dict | None,
               offline: bool = False) -> tuple[list[str], list[str]]:
    """(raisons de redémarrage, lignes de mises à jour) pour le tray."""
    staged, version = _staged_image(state)
    alerts: list[str] = []
    if staged:
        text = "Image Gablue"
        if version:
            text += f" — version {version}"
        alerts.append(text + " téléchargée")
    for reason in events:
        text = _sentence(reason)
        if not text:
            continue
        if staged and text.lower().startswith("nouvelle image"):
            continue
        alerts.append(text)
    domains: list[str] = []
    for line in updates.summary_lines(state):
        if staged and line.startswith("Image Gablue"):
            continue
        domains.append(line.replace(" : ", " — ", 1))
    if offline and not any("hors ligne" in line.lower() for line in domains):
        domains.append("Hors ligne — vérifications en pause")
    if not domains and state is not None:
        text = updates.compact_summary(state)
        if text:
            domains.append(text)
    return alerts, domains


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
        # La vérification en cours doit-elle parler en fin de passe ? Vrai
        # pour un clic sur « Vérifier les mises à jour », faux pour le timer.
        self._notify_result = False

        self._checking = False
        self.menu = QMenu()
        self.menu.aboutToShow.connect(self._build_menu)
        self.check_action = None
        self._build_menu()

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
        instance.spawn(instance.manager_command())

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
        instance.spawn(instance.manager_command())

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
        """Cale la fréquence sur les préférences (minutes, bornée).

        `QTimer` compte en **millisecondes** alors que les préférences sont en
        minutes : sans le ×1000, l'intervalle de 120 min valait 7,2 s et le
        tray relisait `preferences.json` toutes les 7 secondes (vécu
        08/11/2026, constaté au strace).
        """
        values = prefs.load_prefs()
        interval = prefs.check_interval_seconds(values) * 1000
        self.update_timer.setInterval(interval)
        if prefs.checks_enabled(values):
            if not self.update_timer.isActive():
                self.update_timer.start()
        else:
            self.update_timer.stop()

    def refresh_updates(self, force: bool = False,
                        notify: bool | None = None) -> None:
        """Sonde les mises à jour — jamais sans réseau, jamais sans demande.

        `force` vient du menu : il ignore l'interrupteur général et la
        fréquence, mais reste soumis à la connexion (hors ligne, on n'essaie
        même pas). `notify` choisit qui parle : par défaut une vérification
        demandée répond toujours, une passe automatique seulement si elle
        trouve du nouveau.
        """
        wanted = bool(force if notify is None else notify)
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
            if wanted:
                self._notify_updates({"online": False, "domains": {}},
                                     manual=True)
            return
        self._offline = False
        if not force and self._last_check and \
                time.time() - self._last_check < prefs.check_interval_seconds(values):
            return
        domains = prefs.check_domains(values) if force \
            else prefs.auto_check_domains(values)
        worker = UpdateCheckWorker(domains, online)
        self._update_worker = worker
        self._notify_result = wanted
        worker.done.connect(self._updates_done)
        worker.finished.connect(worker.deleteLater)
        self._checking = True
        self._build_menu()
        worker.start()

    def _updates_done(self, state: dict) -> None:
        previous = self.updates_state
        self._update_worker = None
        self.updates_state = state
        self._offline = False
        self._last_check = (state or {}).get("checked_at") or time.time()
        self._checking = False
        # Prochaine passe à un intervalle **plein** après la fin de celle-ci :
        # sans ce redémarrage, un tick tombé juste avant le garde `_last_check`
        # repoussait la sonde d'un intervalle de plus.
        if prefs.checks_enabled(prefs.load_prefs()):
            self.update_timer.start()
        self._update_icon()
        self._notify_updates(state, previous, manual=self._notify_result)
        self._notify_result = False

    def _notify_updates(self, state: dict | None, previous: dict | None = None,
                        manual: bool = False) -> None:
        """Notification brève : ce que la pastille dit, en une ligne de titre."""
        message = updates.notification_text(state, previous, manual)
        if not message:
            return
        title, body = message
        self.icon.showMessage(title, body, self.base_icon, NOTIFY_MS)

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

    def _build_menu(self) -> None:
        """Un menu lisible : sections, une ligne par élément, actions en bas.

        Reconstruit à chaque changement d'état : QMenu ne sait pas insérer
        proprement un bloc variable au milieu d'actions existantes.
        """
        self.menu.clear()
        open_action = self.menu.addAction("Ouvrir Gablue Manager")
        font = open_action.font()
        font.setBold(True)
        open_action.setFont(font)
        open_action.triggered.connect(self.open_manager)

        alerts, domains = tray_lines(
            self.events, self.updates_state, self._offline
        )
        if alerts:
            self.menu.addSection("Redémarrage requis")
            for text in alerts:
                self.menu.addAction(text).setEnabled(False)
        if domains:
            self.menu.addSection("Mises à jour")
            for text in domains:
                self.menu.addAction(text).setEnabled(False)

        self.menu.addSeparator()
        self.check_action = self.menu.addAction(
            "Vérification des mises à jour…" if self._checking
            else "Vérifier les mises à jour"
        )
        self.check_action.setEnabled(not self._checking)
        self.check_action.triggered.connect(lambda: self.refresh_updates(True))
        quit_action = self.menu.addAction("Quitter")
        quit_action.triggered.connect(self.app.quit)

    def _update_icon(self) -> None:
        pending = updates.has_updates(self.updates_state)
        alert = bool(self.events) or pending
        self.icon.setIcon(badge_icon(self.base_icon, alert))

        alerts, domains = tray_lines(
            self.events, self.updates_state, self._offline
        )
        lines = ["Gablue Manager"]
        if alerts:
            lines.append("Redémarrage requis")
            lines.extend("• " + text for text in alerts)
        lines.extend("• " + text for text in domains)
        self.icon.setToolTip("\n".join(lines))

        # Ne pas reconstruire un menu ouvert sous les doigts : `aboutToShow`
        # le rafraîchit à la prochaine ouverture.
        if not self.menu.isVisible():
            self._build_menu()

def _tray_message(tray: Tray, app: QApplication, server) -> None:
    """Message de la fenêtre : `quit`, `reload` (préférences) ou `updates`.

    `updates` = une mise à jour vient de se terminer : l'icône re-sonde tout
    de suite, sinon sa pastille rouge gardait l'état d'avant jusqu'au prochain
    créneau (vécu 07/11/2026).
    """
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
    elif message == b"updates":
        # La fenêtre vient de finir une mise à jour : elle affiche déjà le
        # résultat, le tray rafraîchit sa pastille sans doubler d'une bulle.
        tray.refresh_updates(force=True, notify=False)
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
