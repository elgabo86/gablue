"""Lecture vidéo, overlay de chargement et inhibition de veille."""

import sys

from PySide6.QtCore import Qt
from PySide6.QtDBus import QDBusConnection, QDBusMessage
from PySide6.QtGui import QCursor
from PySide6.QtWidgets import QApplication

from .constants import CURSOR_HIDE_MS

# Inhibition veille / verrouillage, tenue pendant TOUTE la vie de l'appli
# (pas seulement en lecture : on zappe aussi, et la TV reste allumée).
INHIBIT_WHO = "Gablue TV"
INHIBIT_WHY = "Télévision en cours d'utilisation"
# Connexion D-Bus dédiée au verrouillage : la libération se fait en la coupant
# (kwin rend les cookies d'un appelant déconnecté, comme à la mort du process).
INHIBIT_CONNECTION = "gablue-tv-inhibit"
# Verrouillage de session : kwin (Plasma). Chemin historique en repli.
_SCREENSAVER_TARGETS = (
    ("org.freedesktop.ScreenSaver", "/ScreenSaver"),
    ("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver"),
)


class VideoMixin:
    """Lecture vidéo, overlay de chargement, veille/verrouillage et curseur."""
    # =========================================================================
    # LECTURE VIDÉO (libmpv embarqué)
    # =========================================================================

    def _play(self, url, label=""):
        """Lance la lecture (conserve la taille de fenêtre actuelle).

        La vue d'origine (grille ou guide) est mémorisée pour le retour."""
        self._return_view = self._stack.currentIndex()
        self._video_widget.command("stop")
        self._show_view(1)

        self._loading_title.setText(label)
        self._loading_subtitle.setText("Connexion au flux...")
        self._loading_dots = 0
        self._loading_overlay.setGeometry(self.centralWidget().rect())
        self._loading_overlay.raise_()
        self._loading_overlay.show()
        self._load_anim.start(450)

        self._video_widget.play(url)
        self._loading_timer.start(3000)
        self._cursor_timer.start(CURSOR_HIDE_MS)

    def _hide_loading(self):
        self._loading_overlay.hide()
        self._load_anim.stop()
        self._loading_timer.stop()

    def _pause_toggle(self):
        self._video_widget.command("cycle", "pause")

    def _stop_video(self):
        """Arrête la lecture et revient à la vue d'origine (grille ou guide)."""
        self._video_widget.command("stop")
        self._show_view(self._return_view if self._return_view in (0, 2) else 0)
        self._return_view = 0
        self._hide_loading()
        self._restore_cursor()
        # retour au guide : rafraîchit barres de progression / bascule de
        # programme (rebuild seulement si un programme a basculé)
        if self._stack.currentIndex() == 2:
            self._epg_tick()

    # =========================================================================
    # INHIBITION VEILLE / VERROUILLAGE (D-Bus)
    # =========================================================================

    @staticmethod
    def _dbus_call(bus, service, path, interface, method, args=()):
        """Appelle une méthode D-Bus, rend son premier argument, None si échec."""
        msg = QDBusMessage.createMethodCall(service, path, interface, method)
        msg.setArguments(list(args))
        reply = bus.call(msg)
        if reply.type() != QDBusMessage.ReplyMessage:
            print(f"[veille] {interface}.{method} ({service}) : "
                  f"{reply.errorMessage()}", file=sys.stderr)
            return None
        out = reply.arguments()
        return out[0] if out else None

    def _inhibit_screensaver(self):
        """Empêche verrouillage de session et mise en veille tant que l'appli vit.

        Deux inhibitions, chacune sur son service (Plasma/KDE) :
        - logind (`idle:sleep`, bus système, fd gardé ouvert) : action
          d'inactivité et suspension indisponibles ; PowerDevil lit cette liste
          et retire ses actions (extinction d'écran, veille) ;
        - kwin (`org.freedesktop.ScreenSaver`, connexion dédiée) : pas de
          verrouillage de session (`KSldApp::inhibit`).

        Libération : le fd relâché et la connexion coupée rendent les
        inhibitions ; idem si l'appli meurt sans fermer (client parti)."""
        if self._inhibit_logind_fd is None:
            fd = self._dbus_call(
                QDBusConnection.systemBus(),
                "org.freedesktop.login1", "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager", "Inhibit",
                ("idle:sleep", INHIBIT_WHO, INHIBIT_WHY, "block"),
            )
            if fd is not None:
                self._inhibit_logind_fd = fd
        if not self._inhibit_locker:
            QDBusConnection.connectToBus(QDBusConnection.BusType.SessionBus,
                                         INHIBIT_CONNECTION)
            dedicated = QDBusConnection(INHIBIT_CONNECTION)
            bus = (dedicated if dedicated.isConnected()
                   else QDBusConnection.sessionBus())
            for service, path in _SCREENSAVER_TARGETS:
                cookie = self._dbus_call(
                    bus, service, path,
                    "org.freedesktop.ScreenSaver", "Inhibit",
                    (INHIBIT_WHO, INHIBIT_WHY),
                )
                if cookie:
                    self._inhibit_locker = True
                    break

    def _uninhibit_screensaver(self):
        """Lève les inhibitions de veille / verrouillage."""
        self._inhibit_logind_fd = None   # fd fermé par le destructeur Qt
        if self._inhibit_locker:
            QDBusConnection.disconnectFromBus(INHIBIT_CONNECTION)
            self._inhibit_locker = False

    def _toggle_fullscreen(self):
        """Bascule plein écran / fenêtré (à tout moment), forcé sur l'écran du curseur."""
        if self.isFullScreen():
            self.showNormal()
        else:
            screen = QApplication.screenAt(QCursor.pos())
            if screen:
                self.setGeometry(screen.geometry())
            self.showFullScreen()

    def _volume_up(self):
        self._video_widget.command("add", "volume", "5")

    def _volume_down(self):
        self._video_widget.command("add", "volume", "-5")

    def _seek(self, seconds):
        self._video_widget.command("seek", str(seconds))

    # =========================================================================
    # OVERLAY DE CHARGEMENT
    # =========================================================================

    def _animate_loading(self):
        """Animation de chargement + détection du démarrage vidéo."""
        self._loading_dots = (self._loading_dots + 1) % 4
        dots = "." * self._loading_dots
        self._loading_subtitle.setText(f"Connexion au flux{dots}   ")
        if self._video_widget.is_rendering():
            self._hide_loading()

    def _on_user_activity(self):
        """Réaffiche le curseur et reprogramme son masquage (mode lecture)."""
        if self._stack.currentIndex() != 1:
            return
        if self._cursor_hidden:
            self._restore_cursor()
        self._cursor_timer.start(CURSOR_HIDE_MS)

    def _restore_cursor(self):
        """Réaffiche le curseur et arrête le timer de masquage."""
        self._cursor_timer.stop()
        if self._cursor_hidden:
            self._cursor_hidden = False
            self._video_widget.unsetCursor()

    def _hide_cursor(self):
        """Masque le curseur après inactivité pendant la lecture."""
        if self._stack.currentIndex() == 1 and not self._cursor_hidden:
            self._cursor_hidden = True
            self._video_widget.setCursor(Qt.BlankCursor)
