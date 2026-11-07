"""Dossiers de versions Syncthing (`.stversions`) : voir, mesurer, ouvrir.

Syncthing garde une copie des fichiers modifiés ou supprimés dans un dossier
`.stversions` par dossier synchronisé (ou dans le `versionsPath` configuré).
Cet outil ne supprime **rien** : il liste, mesure la taille et ouvre le dossier
choisi dans Dolphin, pour que le ménage se fasse à la main, fichier par
fichier.
"""

import shutil

from PySide6.QtCore import QProcess, Qt, QThread, Signal
from PySide6.QtWidgets import (
    QDialog, QFrame, QHBoxLayout, QLabel, QPushButton, QScrollArea,
    QVBoxLayout, QWidget,
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


class _VersionRow(QFrame):
    """Une ligne cliquable : ouvre son dossier dans le gestionnaire de fichiers."""

    def __init__(self, entry: dict, parent=None):
        super().__init__(parent)
        self.path = entry["versions"]
        self.setObjectName("hostCard")
        self.setCursor(Qt.PointingHandCursor)
        self.setToolTip("Ouvrir ce dossier dans Dolphin")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 12, 16, 12)
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


class StVersionsDialog(QDialog):
    """Liste des `.stversions` d'une session Syncthing active."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Versions Syncthing")
        self.setModal(True)
        self.setMinimumWidth(720)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Versions Syncthing", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        self.note = QLabel(self)
        self.note.setObjectName("subtitle")
        self.note.setWordWrap(True)
        root_layout.addWidget(self.note)

        self.rows_widget = QWidget(self)
        self.rows_layout = QVBoxLayout(self.rows_widget)
        self.rows_layout.setContentsMargins(0, 0, 0, 0)
        self.rows_layout.setSpacing(8)
        self.scroll = QScrollArea(self)
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setWidget(self.rows_widget)
        root_layout.addWidget(self.scroll, 1)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        buttons.addWidget(self.hint_label)
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.setDefault(True)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        root_layout.addLayout(buttons)

        self.rows: list[_VersionRow] = []
        self.sizes: dict[str, int | None] = {}
        self._size_worker = None
        self.refresh()
        self.adjustSize()

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        if not local.syncthing_running():
            self.note.setText(
                "Syncthing n'est pas actif : rien à lister pour l'instant."
            )
            self.hint_label.setText("Démarrez Syncthing puis rouvrez cet outil.")
            self.scroll.setVisible(False)
            return
        entries = local.syncthing_version_dirs()
        if not entries:
            self.note.setText(
                "Syncthing est actif, mais aucun dossier .stversions n'a été "
                "trouvé."
            )
            self.scroll.setVisible(False)
            self.hint_label.setText("")
            return
        self.note.setText(
            "Syncthing est actif. Cliquez un dossier pour l'ouvrir dans "
            "Dolphin et faire le ménage à la main — rien n'est supprimé ici."
        )
        for entry in entries:
            row = _VersionRow(entry, self.rows_widget)
            self.rows_layout.addWidget(row)
            self.rows.append(row)
        self.rows_layout.addStretch(1)
        self._start_sizes([entry["versions"] for entry in entries])

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
            self.hint_label.setText(
                f"{len(self.rows)} dossier{'s' if len(self.rows) > 1 else ''}"
                f" · {local.format_size(total)} au total"
            )
        else:
            self.hint_label.setText(
                f"{len(self.rows)} dossier{'s' if len(self.rows) > 1 else ''}"
            )
