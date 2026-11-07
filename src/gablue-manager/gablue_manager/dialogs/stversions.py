"""Anciennes versions Syncthing (`.stversions`) : voir, mesurer, ouvrir.

Syncthing garde une copie des fichiers modifiés ou supprimés dans un dossier
`.stversions` par dossier synchronisé (ou dans le `versionsPath` configuré).
Cette section, intégrée à « Nettoyer le système », ne supprime **rien** : elle
liste, mesure la taille et ouvre le dossier choisi dans Dolphin, pour que le
ménage se fasse à la main, fichier par fichier.
"""

import shutil

from PySide6.QtCore import QProcess, Qt, QThread, Signal
from PySide6.QtWidgets import (
    QFrame, QHBoxLayout, QLabel, QScrollArea, QVBoxLayout, QWidget,
)

from .. import local

# Mesures en fond : `du` sur des dizaines de gigaoctets prend du temps. Le
# worker vit sans parent pour survivre à la fermeture du dialogue.
_ACTIVE_WORKERS: set = set()


class _SizeWorker(QThread):
    """Taille de chaque dossier `.stversions`, une mesure par ligne."""

    done = Signal(object)          # dict[chemin, octets | None]

    def __init__(self, paths: list[str], parent=None):
        super().__init__(parent)
        self._paths = list(paths)

    def run(self) -> None:
        sizes: dict[str, int | None] = {}
        for path in self._paths:
            sizes[path] = local.versions_dir_size(path)
        self.done.emit(sizes)


class VersionRow(QFrame):
    """Une ligne cliquable : ouvre son dossier dans le gestionnaire de fichiers."""

    def __init__(self, entry: dict, parent=None):
        super().__init__(parent)
        self.path = entry["versions"]
        self.setObjectName("hostCard")
        self.setCursor(Qt.PointingHandCursor)
        self.setToolTip("Ouvrir ce dossier dans Dolphin")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 10, 16, 10)
        layout.setSpacing(4)

        top = QHBoxLayout()
        top.setSpacing(8)
        name = QLabel(entry.get("label") or entry.get("path") or "Dossier",
                      self)
        name.setObjectName("hostName")
        top.addWidget(name)
        top.addStretch(1)
        self.size_label = QLabel("…", self)
        self.size_label.setObjectName("hint")
        top.addWidget(self.size_label)
        layout.addLayout(top)

        detail = QLabel(f"{entry['path']} · {entry['versions']}", self)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

    def size_text(self, size: int | None) -> None:
        self.size_label.setText(local.format_size(size))

    def mouseReleaseEvent(self, event) -> None:
        if event.button() == Qt.LeftButton:
            self.open()
            return
        super().mouseReleaseEvent(event)

    def open(self) -> None:
        dolphin = shutil.which("dolphin")
        if dolphin:
            QProcess.startDetached(dolphin, [self.path])
        else:
            QProcess.startDetached("xdg-open", [self.path])


class StVersionsSection(QFrame):
    """Section « Anciennes versions Syncthing » du dialogue de nettoyage."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("hostCard")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        title = QLabel("Anciennes versions Syncthing", self)
        title.setObjectName("hostName")
        layout.addWidget(title)

        self.note = QLabel(self)
        self.note.setObjectName("hostDetail")
        self.note.setWordWrap(True)
        layout.addWidget(self.note)

        self.rows_widget = QWidget(self)
        self.rows_layout = QVBoxLayout(self.rows_widget)
        self.rows_layout.setContentsMargins(0, 0, 0, 0)
        self.rows_layout.setSpacing(6)
        self.scroll = QScrollArea(self)
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setWidget(self.rows_widget)
        # Le panneau occupe tout l'onglet : la liste prend l'espace libre.
        # Sans ce facteur d'étirement, Qt répartissait le vide entre le titre,
        # la note et la liste — gros trous au milieu du panneau (vécu 07/11/2026).
        layout.addWidget(self.scroll, 1)

        self.rows: list[VersionRow] = []
        self.sizes: dict[str, int | None] = {}
        self._size_worker = None
        self._note_base = ""

    def refresh(self) -> bool:
        """Remplit la section ; False si Syncthing est inactif ou sans dossier."""
        if not local.syncthing_running():
            self.note.setText("Syncthing n'est pas actif sur ce poste.")
            return False
        entries = local.syncthing_version_dirs()
        if not entries:
            self.note.setText(
                "Syncthing est actif, mais aucun dossier .stversions n'a été "
                "trouvé."
            )
            return False
        self._note_base = (
            "Fichiers modifiés ou supprimés conservés par Syncthing. Cliquez "
            "un dossier pour l'ouvrir dans Dolphin et faire le ménage à la "
            "main — rien n'est supprimé ici."
        )
        self.note.setText(self._note_base)
        for entry in entries:
            row = VersionRow(entry, self.rows_widget)
            self.rows_layout.addWidget(row)
            self.rows.append(row)
        self.rows_layout.addStretch(1)
        self._start_sizes([entry["versions"] for entry in entries])
        return True

    def _start_sizes(self, paths: list[str]) -> None:
        worker = _SizeWorker(paths)
        worker.done.connect(self._sizes_done)
        worker.finished.connect(lambda: _ACTIVE_WORKERS.discard(worker))
        worker.finished.connect(worker.deleteLater)
        _ACTIVE_WORKERS.add(worker)
        self._size_worker = worker
        worker.start()

    def _sizes_done(self, sizes: dict) -> None:
        self.sizes = dict(sizes or {})
        total = 0
        for row in self.rows:
            size = self.sizes.get(row.path)
            row.size_text(size)
            total += size or 0
        if total:
            self.note.setText(
                f"{self._note_base} — {local.format_size(total)} au total."
            )
