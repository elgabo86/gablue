"""Lecture vidéo et overlay de chargement (libmpv embarqué)."""

from PySide6.QtCore import Qt
from PySide6.QtDBus import QDBusConnection, QDBusMessage
from PySide6.QtGui import QCursor
from PySide6.QtWidgets import QApplication

from .constants import CURSOR_HIDE_MS


class VideoMixin:
    """Lecture vidéo, overlay de chargement, veille et curseur."""
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

        # Empêche l'extinction d'écran pendant la lecture (portail freedesktop)
        self._inhibit_screensaver()

    def _hide_loading(self):
        self._loading_overlay.hide()
        self._load_anim.stop()
        self._loading_timer.stop()

    def _pause_toggle(self):
        self._video_widget.command("cycle", "pause")
        if self._inhibit_handle:
            self._uninhibit_screensaver()
        else:
            self._inhibit_screensaver()

    def _stop_video(self):
        """Arrête la lecture et revient à la vue d'origine (grille ou guide)."""
        self._video_widget.command("stop")
        self._show_view(self._return_view if self._return_view in (0, 2) else 0)
        self._return_view = 0
        self._hide_loading()
        self._uninhibit_screensaver()
        self._restore_cursor()
        # retour au guide : rafraîchit barres de progression / bascule de
        # programme (rebuild seulement si un programme a basculé)
        if self._stack.currentIndex() == 2:
            self._epg_tick()

    def _inhibit_screensaver(self):
        """Empêche l'extinction d'écran via org.freedesktop.PowerManagement."""
        if self._inhibit_handle is not None:
            return
        msg = QDBusMessage.createMethodCall(
            "org.freedesktop.PowerManagement",
            "/org/freedesktop/PowerManagement/Inhibit",
            "org.freedesktop.PowerManagement.Inhibit",
            "Inhibit",
        )
        msg.setArguments(["Gablue TV", "Lecture en cours"])
        reply = QDBusConnection.sessionBus().call(msg)
        if reply.type() == QDBusMessage.ReplyMessage and reply.arguments():
            self._inhibit_handle = reply.arguments()[0]

    def _uninhibit_screensaver(self):
        """Lève l'inhibition d'écran."""
        if self._inhibit_handle is None:
            return
        msg = QDBusMessage.createMethodCall(
            "org.freedesktop.PowerManagement",
            "/org/freedesktop/PowerManagement/Inhibit",
            "org.freedesktop.PowerManagement.Inhibit",
            "UnInhibit",
        )
        msg.setArguments([self._inhibit_handle])
        QDBusConnection.sessionBus().call(msg)
        self._inhibit_handle = None

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
