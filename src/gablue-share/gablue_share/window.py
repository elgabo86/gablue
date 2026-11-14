"""Fenêtre principale : « Mon partage » à gauche, postes à droite, transferts en bas."""

import subprocess
from pathlib import Path

from PySide6.QtCore import Qt, QThread, QUrl, Signal
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (
    QComboBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QMainWindow,
    QMenu,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QScrollArea,
    QStackedWidget,
    QStyle,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from . import api, config
from .constants import APP_NAME, PORT, download_dir
from .daemon import Daemon
from .discovery import Discovery, discovery_available
from .downloads import Downloads, Job, unique_target
from .util import human_date, human_size


# ---------------------------------------------------------------------------
# Aides d'affichage
# ---------------------------------------------------------------------------
def label(text: str, name: str = "") -> QLabel:
    widget = QLabel(text)
    if name:
        widget.setObjectName(name)
    widget.setWordWrap(True)
    return widget


# ---------------------------------------------------------------------------
# Colonne gauche : les dossiers partagés (persistants, sur disque)
# ---------------------------------------------------------------------------
class ShareRow(QFrame):
    remove_requested = Signal(str)

    def __init__(self, share: dict, parent=None):
        super().__init__(parent)
        self.setObjectName("row")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 8, 0, 8)
        layout.setSpacing(8)
        texts = QVBoxLayout()
        texts.setSpacing(2)
        texts.addWidget(label(share.get("name", "Partage"), "section"))
        full_path = share.get("path", "")
        short = full_path if len(full_path) <= 34 else "…" + full_path[-33:]
        path = label(short, "hint")
        path.setWordWrap(False)
        path.setToolTip(full_path)
        texts.addWidget(path)
        layout.addLayout(texts, 1)
        button = QPushButton("✕")
        button.setObjectName("flat")
        button.setFixedWidth(32)
        button.setToolTip("Ne plus partager ce dossier")
        button.clicked.connect(lambda: self.remove_requested.emit(share.get("id", "")))
        layout.addWidget(button)


class SharePanel(QFrame):
    """Liste éditable des dossiers partagés — c'est elle qui est permanente."""

    changed = Signal()

    def __init__(self, document: dict, parent=None):
        super().__init__(parent)
        self.document = document
        self.setObjectName("panel")
        self.setFixedWidth(340)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(10)
        layout.addWidget(label("Mon partage", "title"))
        self.status = label("", "muted")
        layout.addWidget(self.status)

        self.rows = QVBoxLayout()
        self.rows.setSpacing(0)
        layout.addLayout(self.rows)
        layout.addStretch(1)

        add = QPushButton("Ajouter un dossier…")
        add.setObjectName("primary")
        add.clicked.connect(self.add_folder)
        layout.addWidget(add)

        self.addresses = label("", "hint")
        layout.addWidget(self.addresses)
        self.reload()

    # --- liste ----------------------------------------------------------
    def reload(self) -> None:
        while self.rows.count():
            item = self.rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        shares = self.document.get("shares", [])
        for share in shares:
            row = ShareRow(share)
            row.remove_requested.connect(self.remove_share)
            self.rows.addWidget(row)
        if shares:
            self.status.setText(
                f"{len(shares)} dossier{'s' if len(shares) > 1 else ''} partagé"
                f"{'s' if len(shares) > 1 else ''} — visible des autres postes."
            )
        else:
            self.status.setText(
                "Aucun dossier partagé : les autres postes ne vous voient pas. "
                "Ajoutez un dossier pour commencer."
            )
        self._refresh_addresses()

    def _refresh_addresses(self) -> None:
        addresses: list[str] = []
        try:
            output = subprocess.run(
                ["ip", "-o", "-4", "addr", "show"],
                capture_output=True,
                text=True,
                timeout=3,
                check=False,
            ).stdout
            for line in output.splitlines():
                parts = line.split()
                if len(parts) < 4:
                    continue
                interface, address = parts[1], parts[3].split("/")[0]
                if interface == "lo" or interface.startswith(("docker", "veth", "br-", "virbr")):
                    continue
                addresses.append(address)
        except (OSError, subprocess.SubprocessError):
            pass
        if not addresses:
            self.addresses.setText("")
            return
        suffix = "" if self.document.get("shares") else " · rien n'est annoncé"
        self.addresses.setText(
            f"Port {PORT} · {', '.join(addresses)}{suffix}"
        )

    # --- actions --------------------------------------------------------
    def add_folder(self) -> None:
        chosen = QFileDialog.getExistingDirectory(
            self, "Choisir un dossier à partager", str(Path.home())
        )
        if not chosen:
            return
        ok, message = config.add_share(self.document, chosen)
        if not ok:
            QMessageBox.warning(self, APP_NAME, message)
            return
        self.reload()
        self.changed.emit()

    def remove_share(self, share_id: str) -> None:
        if config.remove_share(self.document, share_id):
            self.reload()
            self.changed.emit()


# ---------------------------------------------------------------------------
# Liste des postes
# ---------------------------------------------------------------------------
class PeerRow(QFrame):
    opened = Signal(object)

    def __init__(self, peer, parent=None):
        super().__init__(parent)
        self.peer = peer
        self.setObjectName("card")
        self.setCursor(Qt.PointingHandCursor)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(14, 12, 14, 12)
        layout.setSpacing(12)

        texts = QVBoxLayout()
        texts.setSpacing(3)
        texts.addWidget(label(peer.name, "section"))
        texts.addWidget(label(peer.describe(), "muted"))
        layout.addLayout(texts, 1)
        layout.addWidget(label(peer.label(), "hint"))

    def mouseReleaseEvent(self, event) -> None:  # noqa: N802 (API Qt)
        self.opened.emit(self.peer)


class PeerPage(QWidget):
    open_peer = Signal(object)
    refresh_requested = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 16, 20, 16)
        layout.setSpacing(12)

        header = QHBoxLayout()
        header.addWidget(label("Postes à proximité", "title"), 1)
        refresh = QPushButton("Actualiser")
        refresh.clicked.connect(self.refresh_requested.emit)
        header.addWidget(refresh)
        layout.addLayout(header)

        self.hint = label("", "hint")
        layout.addWidget(self.hint)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        self.body = QWidget()
        self.rows = QVBoxLayout(self.body)
        self.rows.setContentsMargins(0, 0, 0, 0)
        self.rows.setSpacing(8)
        self.rows.addStretch(1)
        scroll.setWidget(self.body)
        layout.addWidget(scroll, 1)

        if not discovery_available():
            self.hint.setText(
                "Ni Avahi ni Tailscale ne sont disponibles : la découverte est limitée."
            )

    def set_peers(self, peers) -> None:
        while self.rows.count():
            item = self.rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        if not peers:
            empty = label(
                "Personne ne partage pour l'instant sur votre réseau local ou votre tailnet.\n"
                "Un poste apparaît dès qu'il ouvre Gablue Share avec au moins un dossier partagé.",
                "muted",
            )
            empty.setAlignment(Qt.AlignCenter)
            self.rows.addWidget(empty)
        else:
            for peer in peers:
                row = PeerRow(peer)
                row.opened.connect(self.open_peer.emit)
                self.rows.addWidget(row)
        self.rows.addStretch(1)


# ---------------------------------------------------------------------------
# Navigation chez un poste
# ---------------------------------------------------------------------------
class ListLoader(QThread):
    loaded = Signal(int, dict)
    failed = Signal(int, str)

    def __init__(self, token: int, base: str, share_id: str, path: str, parent=None):
        super().__init__(parent)
        self.token = token
        self.base = base
        self.share_id = share_id
        self.path = path

    def run(self) -> None:
        try:
            self.loaded.emit(self.token, api.list_dir(self.base, self.share_id, self.path))
        except api.ApiError as error:
            self.failed.emit(self.token, str(error))


class BrowserPage(QWidget):
    download_requested = Signal(object, object, str, bool, bool)  # peer, partage, chemin, dossier, choisir
    closed = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.peer = None
        self.share = None
        self.payload: dict = {}
        self._token = 0
        self._loader: ListLoader | None = None

        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 16, 20, 16)
        layout.setSpacing(10)

        header = QHBoxLayout()
        self.back = QPushButton("←")
        self.back.setObjectName("flat")
        self.back.setFixedWidth(36)
        self.back.clicked.connect(self.go_back)
        header.addWidget(self.back)
        self.title = label("", "title")
        header.addWidget(self.title, 1)
        self.shares = QComboBox()
        self.shares.setMinimumWidth(180)
        self.shares.currentIndexChanged.connect(self._share_changed)
        header.addWidget(self.shares)
        refresh = QPushButton("Actualiser")
        refresh.clicked.connect(lambda: self.load(self.path))
        header.addWidget(refresh)
        layout.addLayout(header)

        self.breadcrumb = label("/", "hint")
        layout.addWidget(self.breadcrumb)

        self.table = QTableWidget(0, 3)
        self.table.setHorizontalHeaderLabels(["Nom", "Taille", "Modifié"])
        self.table.verticalHeader().setVisible(False)
        self.table.setShowGrid(False)
        self.table.setSelectionBehavior(QTableWidget.SelectRows)
        self.table.setSelectionMode(QTableWidget.ExtendedSelection)
        self.table.setEditTriggers(QTableWidget.NoEditTriggers)
        self.table.setSortingEnabled(False)
        header_view = self.table.horizontalHeader()
        header_view.setDefaultAlignment(Qt.AlignLeft | Qt.AlignVCenter)
        header_view.setSectionResizeMode(0, QHeaderView.Stretch)
        header_view.setSectionResizeMode(1, QHeaderView.ResizeToContents)
        header_view.setSectionResizeMode(2, QHeaderView.ResizeToContents)
        self.table.itemDoubleClicked.connect(self._activated)
        self.table.itemSelectionChanged.connect(self._selection_changed)
        self.table.setContextMenuPolicy(Qt.CustomContextMenu)
        self.table.customContextMenuRequested.connect(self._context_menu)
        layout.addWidget(self.table, 1)

        footer = QHBoxLayout()
        self.selection = label("", "muted")
        footer.addWidget(self.selection, 1)
        self.save = QPushButton("Enregistrer sous…")
        self.save.setEnabled(False)
        self.save.clicked.connect(lambda: self._request_download(choose=True))
        footer.addWidget(self.save)
        self.download = QPushButton("Télécharger")
        self.download.setObjectName("primary")
        self.download.setEnabled(False)
        self.download.clicked.connect(lambda: self._request_download(choose=False))
        footer.addWidget(self.download)
        layout.addLayout(footer)

        self.path = ""

    # --- ouverture ------------------------------------------------------
    def open(self, peer, share: dict | None = None) -> None:
        self.peer = peer
        self.title.setText(peer.name)
        previous = self.share.get("id") if self.share else None
        self.shares.blockSignals(True)
        self.shares.clear()
        for entry in peer.shares:
            self.shares.addItem(entry.get("name") or entry.get("id"), entry)
        index = 0
        if previous:
            for position, entry in enumerate(peer.shares):
                if entry.get("id") == previous:
                    index = position
        self.shares.setCurrentIndex(index)
        self.shares.blockSignals(False)
        self.share = share or (peer.shares[index] if peer.shares else None)
        if self.share is None:
            return
        self.load("")

    def _share_changed(self, index: int) -> None:
        if 0 <= index < len(self.peer.shares):
            self.share = self.peer.shares[index]
            self.load("")

    # --- chargement -----------------------------------------------------
    def load(self, path: str) -> None:
        if not self.peer or not self.share:
            return
        self.path = path
        self.breadcrumb.setText("/" + path if path else "/")
        self._token += 1
        token = self._token
        loader = ListLoader(token, self.peer.base_url(), self.share.get("id", ""), path, self)
        loader.loaded.connect(self._loaded)
        loader.failed.connect(self._failed)
        self._loader = loader
        loader.start()

    def _loaded(self, token: int, payload: dict) -> None:
        if token != self._token:
            return  # réponse périmée
        self.payload = payload
        self.table.setRowCount(0)
        for entry in payload.get("entries", []):
            row = self.table.rowCount()
            self.table.insertRow(row)
            item = QTableWidgetItem(entry.get("name", ""))
            item.setData(Qt.UserRole, entry)
            if entry.get("type") == "dir":
                item.setIcon(self.style().standardIcon(QStyle.SP_DirIcon))
            else:
                item.setIcon(self.style().standardIcon(QStyle.SP_FileIcon))
            self.table.setItem(row, 0, item)
            size = "" if entry.get("type") == "dir" else human_size(entry.get("size", 0))
            size_item = QTableWidgetItem(size)
            size_item.setTextAlignment(Qt.AlignRight | Qt.AlignVCenter)
            self.table.setItem(row, 1, size_item)
            self.table.setItem(row, 2, QTableWidgetItem(human_date(entry.get("mtime", 0))))
        if payload.get("truncated"):
            self.breadcrumb.setText(self.breadcrumb.text() + "  (liste tronquée)")
        self._selection_changed()

    def _failed(self, token: int, message: str) -> None:
        if token != self._token:
            return
        self.table.setRowCount(0)
        self.selection.setText(f"Impossible de lire ce dossier : {message}")

    # --- navigation -----------------------------------------------------
    def go_back(self) -> None:
        parent = self.payload.get("parent")
        if parent is None:
            self.closed.emit()
        else:
            self.load(parent)

    def _activated(self, item: QTableWidgetItem) -> None:
        entry = item.data(Qt.UserRole)
        if entry.get("type") == "dir":
            self.load(_join(self.path, entry.get("name", "")))

    # --- sélection ------------------------------------------------------
    def _selected(self) -> list[dict]:
        rows = sorted({index.row() for index in self.table.selectedIndexes()})
        entries = []
        for row in rows:
            item = self.table.item(row, 0)
            if item is not None:
                entries.append(item.data(Qt.UserRole))
        return entries

    def _selection_changed(self) -> None:
        entries = self._selected()
        self.download.setEnabled(bool(entries))
        self.save.setEnabled(bool(entries))
        if not entries:
            self.selection.setText("Sélectionnez un fichier ou un dossier.")
            return
        if len(entries) == 1:
            entry = entries[0]
            kind = "Dossier" if entry.get("type") == "dir" else human_size(entry.get("size", 0))
            self.selection.setText(f"{entry.get('name', '')} · {kind}")
        else:
            files = [entry for entry in entries if entry.get("type") != "dir"]
            total = sum(entry.get("size", 0) for entry in files)
            self.selection.setText(f"{len(entries)} éléments · {human_size(total)}")

    # --- téléchargement -------------------------------------------------
    def _request_download(self, choose: bool) -> None:
        if not self.peer or not self.share:
            return
        for entry in self._selected():
            is_dir = entry.get("type") == "dir"
            self.download_requested.emit(
                self.peer,
                self.share,
                _join(self.path, entry.get("name", "")),
                is_dir,
                choose,
            )

    def _context_menu(self, position) -> None:
        if not self.table.selectedItems():
            return
        menu = QMenu(self)
        download = menu.addAction("Télécharger")
        save_as = menu.addAction("Enregistrer sous…")
        chosen = menu.exec(self.table.viewport().mapToGlobal(position))
        if chosen is download:
            self._request_download(choose=False)
        elif chosen is save_as:
            self._request_download(choose=True)


def _join(base: str, name: str) -> str:
    return f"{base}/{name}" if base else name


# ---------------------------------------------------------------------------
# Transferts
# ---------------------------------------------------------------------------
class TransferRow(QFrame):
    def __init__(self, job: Job, parent=None):
        super().__init__(parent)
        self.job = job
        self.setObjectName("row")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 8, 0, 8)
        layout.setSpacing(4)
        top = QHBoxLayout()
        self.title = label(job.title(), "section")
        top.addWidget(self.title, 1)
        self.percent = label("", "hint")
        top.addWidget(self.percent)
        self.open = QPushButton("Ouvrir")
        self.open.setObjectName("flat")
        self.open.clicked.connect(self._open_folder)
        top.addWidget(self.open)
        self.cancel = QPushButton("✕")
        self.cancel.setObjectName("flat")
        self.cancel.setFixedWidth(28)
        self.cancel.setToolTip("Annuler ce transfert")
        top.addWidget(self.cancel)
        layout.addLayout(top)
        self.subtitle = label(job.subtitle(), "hint")
        layout.addWidget(self.subtitle)
        self.bar = QProgressBar()
        self.bar.setMaximumHeight(6)
        self.bar.setTextVisible(False)
        layout.addWidget(self.bar)

    def _open_folder(self) -> None:
        QDesktopServices.openUrl(QUrl.fromLocalFile(str(self.job.dest.parent)))

    def update_job(self, job: Job) -> None:
        self.title.setText(job.title())
        self.subtitle.setText(job.subtitle())
        self.open.setVisible(job.state == "terminé")
        self.cancel.setVisible(job.state in ("en attente", "en cours"))
        if job.state == "terminé":
            self.bar.setVisible(False)
            self.percent.setText("Terminé")
        elif job.state in ("échec", "annulé"):
            self.bar.setVisible(False)
            self.percent.setText("Échec" if job.state == "échec" else "Annulé")
        else:
            self.bar.setVisible(True)
            self.bar.setRange(0, 1000)
            self.bar.setValue(int(job.fraction() * 1000))
            if job.total:
                self.percent.setText(f"{int(job.fraction() * 100)} %")
            else:
                self.percent.setText(human_size(job.done))


class TransferPanel(QFrame):
    def __init__(self, downloads: Downloads, parent=None):
        super().__init__(parent)
        self.downloads = downloads
        self.setObjectName("panel")
        self._rows: dict[int, TransferRow] = {}
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 10, 16, 12)
        layout.setSpacing(0)
        self.header = label("Transferts", "section")
        layout.addWidget(self.header)
        self.body = QVBoxLayout()
        self.body.setSpacing(0)
        layout.addLayout(self.body)
        self.hide()

    def refresh(self, jobs: list[Job]) -> None:
        if not jobs:
            self.hide()
            return
        self.show()
        active = sum(1 for job in jobs if job.state in ("en attente", "en cours"))
        self.header.setText(f"Transferts ({active} en cours)" if active else "Transferts")
        for job in jobs:
            row = self._rows.get(id(job))
            if row is None:
                row = TransferRow(job)
                row.cancel.clicked.connect(lambda _=False, item=job: self.downloads.cancel(item))
                self._rows[id(job)] = row
                self.body.addWidget(row)
            row.update_job(job)
        for key in list(self._rows):
            if all(id(job) != key for job in jobs):
                widget = self._rows.pop(key)
                widget.deleteLater()


# ---------------------------------------------------------------------------
# Fenêtre
# ---------------------------------------------------------------------------
class MainWindow(QMainWindow):
    def __init__(self, autostart: bool = True):
        super().__init__()
        self.setWindowTitle(APP_NAME)
        self.resize(1120, 720)
        self.document = config.load()
        self.daemon = Daemon()
        self.discovery = Discovery(self)
        self.downloads = Downloads(self)

        central = QWidget()
        root = QVBoxLayout(central)
        root.setContentsMargins(16, 16, 16, 16)
        root.setSpacing(12)

        columns = QHBoxLayout()
        columns.setSpacing(12)
        self.shares = SharePanel(self.document)
        columns.addWidget(self.shares)

        self.stack = QStackedWidget()
        self.peers = PeerPage()
        self.browser = BrowserPage()
        self.stack.addWidget(self.peers)
        self.stack.addWidget(self.browser)
        columns.addWidget(self.stack, 1)
        root.addLayout(columns, 1)

        self.transfers = TransferPanel(self.downloads)
        root.addWidget(self.transfers)
        self.setCentralWidget(central)

        # câblage
        self.shares.changed.connect(self._shares_changed)
        self.peers.open_peer.connect(self._open_peer)
        self.peers.refresh_requested.connect(self.discovery.refresh_now)
        self.browser.closed.connect(lambda: self.stack.setCurrentWidget(self.peers))
        self.browser.download_requested.connect(self._download)
        self.discovery.peers_changed.connect(self.peers.set_peers)
        self.downloads.changed.connect(self._downloads_changed)

        ok, message = self.daemon.start() if autostart else (True, "")
        if not ok:
            self.shares.status.setText(
                f"{self.shares.status.text()}\n\nLe démon n'a pas démarré : {message}"
            )
        elif autostart:
            self.discovery.start()

    # --- partages -------------------------------------------------------
    def _shares_changed(self) -> None:
        self.document = config.load()
        self.shares.document = self.document
        self.shares.reload()
        if not self.daemon.running():
            self.daemon.start()
        if not self.discovery.active:
            self.discovery.start()

    # --- postes ---------------------------------------------------------
    def _open_peer(self, peer) -> None:
        self.browser.open(peer)
        self.stack.setCurrentWidget(self.browser)

    # --- transferts -----------------------------------------------------
    def _download(self, peer, share, path: str, is_dir: bool, choose: bool) -> None:
        name = path.rsplit("/", 1)[-1] or share.get("name", "téléchargement")
        if choose:
            if is_dir:
                chosen = QFileDialog.getExistingDirectory(
                    self, "Enregistrer le dossier dans…", str(download_dir())
                )
                if not chosen:
                    return
                destination = Path(chosen) / name
            else:
                chosen, _ = QFileDialog.getSaveFileName(
                    self, "Enregistrer sous…", str(download_dir() / name)
                )
                if not chosen:
                    return
                destination = Path(chosen)
        else:
            destination = unique_target(download_dir(), name, is_dir)
        job = Job(peer, share.get("id", ""), share.get("name", ""), path, name, is_dir, destination)
        self.downloads.enqueue(job)

    def _downloads_changed(self) -> None:
        self.transfers.refresh(self.downloads.jobs)

    # --- fermeture ------------------------------------------------------
    def closeEvent(self, event) -> None:  # noqa: N802 (API Qt)
        self.discovery.stop()
        self.downloads.shutdown()
        self.daemon.stop()
        super().closeEvent(event)
