"""Logos de chaînes : cache disque, cache mémoire et téléchargement."""

import os

import shiboken6
from PySide6.QtCore import Qt, QUrl
from PySide6.QtGui import QPixmap
from PySide6.QtNetwork import QNetworkReply, QNetworkRequest
from PySide6.QtWidgets import QLabel

from .constants import LOGO_BASE, LOGO_DIR


class LogosMixin:
    """Logos de chaînes : mémoire → disque → réseau, cibles multiples."""
    # =========================================================================
    # LOGOS
    # =========================================================================

    def _load_logo(self, ch_id, widget, size):
        """Charge un logo (mémoire → disque → réseau), multi-cibles.

        Un même logo peut alimenter plusieurs widgets (carte + ligne du
        guide) : cibles suivies par chaîne, téléchargement dédupliqué.
        Le pixmap décodé est gardé en mémoire : les reconstructions du
        guide ne relisent plus les fichiers PNG du disque."""
        pm = self._logo_pixmem.get(ch_id)
        if pm is not None:
            self._set_logo(widget, pm, size)
            return
        cache_path = os.path.join(LOGO_DIR, f"{ch_id}.png")
        if os.path.exists(cache_path):
            pm = QPixmap(cache_path)
            if not pm.isNull():
                self._logo_pixmem[ch_id] = pm
                self._set_logo(widget, pm, size)
                return
        self._logo_targets.setdefault(ch_id, []).append((widget, size))
        if ch_id in self._logo_pending:
            return
        self._logo_pending.add(ch_id)
        self._net.get(QNetworkRequest(QUrl(f"{LOGO_BASE}/{ch_id}.png")))

    @staticmethod
    def _set_logo(widget, pixmap, size):
        """Applique un pixmap redimensionné à un QLabel cible (si vivant)."""
        if isinstance(widget, QLabel) and shiboken6.isValid(widget):
            widget.setPixmap(
                pixmap.scaled(size, Qt.KeepAspectRatio, Qt.SmoothTransformation)
            )

    def _on_logo_downloaded(self, reply):
        """Callback de téléchargement d'un logo."""
        url_path = reply.url().path()
        ch_id = os.path.splitext(os.path.basename(url_path))[0]
        if reply.error() != QNetworkReply.NetworkError.NoError:
            return
        cache_path = os.path.join(LOGO_DIR, f"{ch_id}.png")
        data = reply.readAll().data()
        try:
            with open(cache_path, "wb") as f:
                f.write(data)
        except OSError:
            return
        self._logo_pending.discard(ch_id)
        targets = self._logo_targets.pop(ch_id, [])
        pixmap = QPixmap(cache_path)
        if not pixmap.isNull():
            self._logo_pixmem[ch_id] = pixmap
        for widget, size in targets:
            self._set_logo(widget, pixmap, size)
