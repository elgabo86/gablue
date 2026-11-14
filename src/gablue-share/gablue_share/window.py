"""Fenêtre principale : « Mon partage » à gauche, postes à droite, transferts en bas."""

import subprocess
from pathlib import Path

from PySide6.QtCore import Qt, QThread, QTimer, QUrl, Signal
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QHeaderView,
    QInputDialog,
    QLabel,
    QLineEdit,
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

from . import api, config, service
from .constants import APP_NAME, PORT, download_dir
from .daemon import Daemon
from .discovery import Discovery, discovery_available
from .downloads import Downloads, Job, unique_target
from .util import human_date, human_eta, human_size, human_speed, notify


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
    rename_requested = Signal(str)

    def __init__(self, share: dict, available: bool, parent=None):
        super().__init__(parent)
        self.share = share
        self.setObjectName("row")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 8, 0, 8)
        layout.setSpacing(8)
        texts = QVBoxLayout()
        texts.setSpacing(2)
        name = label(share.get("name", "Partage"), "section")
        if not available:
            name.setObjectName("hint")
        texts.addWidget(name)
        full_path = share.get("path", "")
        short = full_path if len(full_path) <= 34 else "…" + full_path[-33:]
        if not available:
            short = "Disque débranché · " + short
        path = label(short, "hint")
        path.setWordWrap(False)
        path.setToolTip(
            full_path
            if available
            else f"{full_path}\nDossier absent : ce partage n'est ni annoncé ni servi."
        )
        texts.addWidget(path)
        layout.addLayout(texts, 1)
        button = QPushButton("✕")
        button.setObjectName("flat")
        button.setFixedWidth(32)
        button.setToolTip("Ne plus partager ce dossier")
        button.clicked.connect(lambda: self.remove_requested.emit(share.get("id", "")))
        layout.addWidget(button)

    def contextMenuEvent(self, event) -> None:  # noqa: N802 (API Qt)
        menu = QMenu(self)
        rename = menu.addAction("Renommer…")
        remove = menu.addAction("Ne plus partager")
        chosen = menu.exec(event.globalPos())
        if chosen is rename:
            self.rename_requested.emit(self.share.get("id", ""))
        elif chosen is remove:
            self.remove_requested.emit(self.share.get("id", ""))


class SharePanel(QFrame):
    """Liste éditable des dossiers partagés — c'est elle qui est permanente."""

    changed = Signal()
    permanent_changed = Signal(bool)

    def __init__(self, document: dict, parent=None):
        super().__init__(parent)
        self.document = document
        self._known_availability: list[bool] = []
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

        self.permanent = QCheckBox("Permanent (démarre avec la session)")
        self.permanent.setToolTip(
            "Le partage reste actif quand Gablue Share est fermé, et reprend au "
            "prochain démarrage de la session."
        )
        self.permanent.setChecked(service.enabled())
        self.permanent.toggled.connect(self.permanent_changed.emit)
        layout.addWidget(self.permanent)

        self.addresses = label("", "hint")
        layout.addWidget(self.addresses)
        self.reload()

    # --- liste ----------------------------------------------------------
    def _availability(self) -> list[bool]:
        return [config.share_available(share.get("path", "")) for share in self.document.get("shares", [])]

    def reload(self) -> None:
        while self.rows.count():
            item = self.rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                # setParent(None) d'abord : sinon l'ancienne ligne reste peinte
                # jusqu'au prochain tour de boucle (lignes fantômes après un
                # ajout/retrait de partage).
                widget.setParent(None)
                widget.deleteLater()
        shares = self.document.get("shares", [])
        availability = self._availability()
        self._known_availability = availability
        for share, available in zip(shares, availability):
            row = ShareRow(share, available)
            row.remove_requested.connect(self.remove_share)
            row.rename_requested.connect(self.rename_share)
            self.rows.addWidget(row)
        present = sum(1 for available in availability if available)
        missing = len(shares) - present
        if not shares:
            self.status.setText(
                "Aucun dossier partagé : les autres postes ne vous voient pas. "
                "Ajoutez un dossier pour commencer."
            )
        elif present == 0:
            self.status.setText(
                f"{len(shares)} dossier(s) déclaré(s), aucun disponible : les autres "
                "postes ne vous voient pas. Branchez le disque ou vérifiez le chemin."
            )
        else:
            suffix = f" — {missing} débranché(s), non annoncé(s)" if missing else ""
            self.status.setText(
                f"{present} dossier{'s' if present > 1 else ''} partagé"
                f"{'s' if present > 1 else ''} — visible des autres postes{suffix}."
            )
        self._refresh_addresses()

    def refresh_availability(self) -> None:
        """Un disque branché/débranché change la liste, pas le fichier de config."""
        if self._availability() != getattr(self, "_known_availability", None):
            self.reload()

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
        # Le nom est proposé, jamais imposé : vide = nom du dossier (et c'est le
        # démon qui retombe dessus s'il n'y a pas de nom dans le fichier).
        default = Path(chosen).name or "Partage"
        name, accepted = QInputDialog.getText(
            self,
            "Nom du partage",
            "Nom affiché chez les autres postes\n(laisser vide : nom du dossier) :",
            text=default,
        )
        if not accepted:
            return
        ok, message = config.add_share(self.document, chosen, name)
        if not ok:
            QMessageBox.warning(self, APP_NAME, message)
            return
        self.reload()
        self.changed.emit()

    def rename_share(self, share_id: str) -> None:
        entry = next(
            (share for share in self.document.get("shares", []) if share.get("id") == share_id),
            None,
        )
        if entry is None:
            return
        name, accepted = QInputDialog.getText(
            self,
            "Renommer le partage",
            "Nom affiché chez les autres postes\n(laisser vide : nom du dossier) :",
            text=entry.get("name", ""),
        )
        if not accepted:
            return
        ok, message = config.rename_share(self.document, share_id, name)
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
                # setParent(None) d'abord : sinon l'ancienne ligne reste peinte
                # jusqu'au prochain tour de boucle (lignes fantômes après un
                # ajout/retrait de partage).
                widget.setParent(None)
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
    loaded = Signal(int, dict, str)
    failed = Signal(int, str)

    def __init__(self, token, base, share_id, path, offset, limit, sort, descending, parent=None):
        super().__init__(parent)
        self.token = token
        self.base = base
        self.share_id = share_id
        self.path = path
        self.offset = offset
        self.limit = limit
        self.sort = sort
        self.descending = descending

    def run(self) -> None:
        try:
            payload, tag = api.list_dir(
                self.base, self.share_id, self.path, self.offset, self.limit, self.sort, self.descending
            )
            self.loaded.emit(self.token, payload, tag)
        except api.ApiError as error:
            self.failed.emit(self.token, str(error))


class TagLoader(QThread):
    """Sonde d'une requête : « ce dossier a-t-il bougé ? » (HEAD conditionnel)."""

    done = Signal(int, str, str)  # token, empreinte (vide = inchangé), erreur

    def __init__(self, token: int, base: str, share_id: str, path: str, etag: str, parent=None):
        super().__init__(parent)
        self.token = token
        self.base = base
        self.share_id = share_id
        self.path = path
        self.etag = etag

    def run(self) -> None:
        try:
            self.done.emit(self.token, api.list_tag(self.base, self.share_id, self.path, self.etag), "")
        except api.ApiError as error:
            self.done.emit(self.token, "", str(error))


class SearchLoader(QThread):
    """Recherche récursive côté serveur (bornée en temps par le démon)."""

    found = Signal(int, dict)
    failed = Signal(int, str)

    def __init__(self, token: int, base: str, share_id: str, query: str, path: str, parent=None):
        super().__init__(parent)
        self.token = token
        self.base = base
        self.share_id = share_id
        self.query = query
        self.path = path

    def run(self) -> None:
        try:
            self.found.emit(self.token, api.find(self.base, self.share_id, self.query, self.path))
        except api.ApiError as error:
            self.failed.emit(self.token, str(error))


class SizeLoader(QThread):
    """Taille d'un dossier, à la demande (jamais pour tout un listing)."""

    done = Signal(str, str, dict)  # partage, chemin, réponse
    failed = Signal(str, str)

    def __init__(self, base: str, share_id: str, path: str, parent=None):
        super().__init__(parent)
        self.base = base
        self.share_id = share_id
        self.path = path

    def run(self) -> None:
        try:
            self.done.emit(self.share_id, self.path, api.folder_size(self.base, self.share_id, self.path))
        except api.ApiError:
            self.failed.emit(self.share_id, self.path)


#: Taille d'une page de listing. Le serveur borne à 5000 ; 500 tient largement
#: dans une fenêtre et garde chaque requête instantanée.
PAGE = 500


class BrowserPage(QWidget):
    download_requested = Signal(object, object, str, bool, bool)  # peer, partage, chemin, dossier, choisir
    closed = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.peer = None
        self.share = None
        self.payload: dict = {}
        self.path = ""
        self._token = 0
        self._offset = 0
        self._truncated = False
        self._loading = False
        self._searching = False
        self._sort = "name"
        self._desc = False
        self._etag = ""
        self._loader: ListLoader | None = None
        self._tagger: TagLoader | None = None
        self._tagging = False
        self._search_loader: SearchLoader | None = None
        self._size_loader: SizeLoader | None = None
        self._sizes: dict[tuple[str, str], dict] = {}

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
        self.whole = QPushButton("Tout le partage")
        self.whole.setToolTip("Télécharger l'intégralité de ce partage (archive)")
        self.whole.clicked.connect(self._download_whole_share)
        header.addWidget(self.whole)
        refresh = QPushButton("Actualiser")
        refresh.clicked.connect(lambda: self.load(self.path))
        header.addWidget(refresh)
        layout.addLayout(header)

        search_row = QHBoxLayout()
        self.search = QLineEdit()
        self.search.setPlaceholderText("Rechercher dans ce partage (nom de fichier ou dossier)…")
        self.search.setClearButtonEnabled(True)
        self.search.textChanged.connect(self._search_changed)
        search_row.addWidget(self.search, 1)
        layout.addLayout(search_row)

        self.breadcrumb = label("/", "hint")
        layout.addWidget(self.breadcrumb)

        self.table = QTableWidget(0, 4)
        self.table.setHorizontalHeaderLabels(["Nom", "Dossier", "Taille", "Modifié"])
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
        header_view.setSectionResizeMode(3, QHeaderView.ResizeToContents)
        header_view.setSortIndicatorShown(True)
        header_view.sectionClicked.connect(self._header_clicked)
        self.table.setColumnHidden(1, True)
        self.table.itemDoubleClicked.connect(self._activated)
        self.table.itemSelectionChanged.connect(self._selection_changed)
        self.table.setContextMenuPolicy(Qt.CustomContextMenu)
        self.table.customContextMenuRequested.connect(self._context_menu)
        self.table.verticalScrollBar().valueChanged.connect(self._maybe_more)
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

        # Sonde de fraîcheur : un HEAD conditionnel toutes les 3 s quand on
        # regarde la page (10 s si la fenêtre existe mais n'a pas le focus),
        # rien du tout sinon.
        self.watch = QTimer(self)
        self.watch.setInterval(3000)
        self.watch.timeout.connect(self._poll)

        # Recherche : on attend que la frappe se calme (300 ms) avant d'interroger.
        self._search_timer = QTimer(self)
        self._search_timer.setSingleShot(True)
        self._search_timer.setInterval(300)
        self._search_timer.timeout.connect(self._run_search)
        self._idle_ticks = 0

    # --- ouverture ------------------------------------------------------
    def open(self, peer, share: dict | None = None) -> None:
        self.peer = peer
        self.title.setText(peer.name)
        keep = share.get("id") if share else (self.share.get("id") if self.share else None)
        self._populate(keep)
        if self.share is None:
            return
        self.search.clear()
        self.load("")

    def _populate(self, keep_id: str | None) -> None:
        """Reconstruit la liste des partages du poste en gardant le choix fait."""
        peer = self.peer
        if peer is None:
            return
        self.shares.blockSignals(True)
        self.shares.clear()
        for entry in peer.shares:
            self.shares.addItem(entry.get("name") or entry.get("id"), entry)
        index = 0
        if keep_id:
            for position, entry in enumerate(peer.shares):
                if entry.get("id") == keep_id:
                    index = position
        self.shares.setCurrentIndex(index)
        self.shares.blockSignals(False)
        self.share = peer.shares[index] if peer.shares else None

    def sync_peer(self, peers) -> None:
        """La découverte a republié sa liste : noms, partages et adresses à jour."""
        if self.peer is None:
            return
        for peer in peers:
            if peer.peer_id != self.peer.peer_id:
                continue
            changed = peer.shares != self.peer.shares
            self.peer = peer
            if not changed:
                return
            current = self.share.get("id") if self.share else None
            self.title.setText(peer.name)
            self._populate(current)
            if self.share is None:
                self.table.setRowCount(0)
                self.selection.setText("Ce poste ne partage plus rien.")
                self._etag = ""
                return
            if self.share.get("id") != current:
                self.search.clear()
                self.load("")  # le partage affiché a disparu : on bascule
            else:
                self.load(self.path)  # même partage, contenu peut-être nouveau
            return

    def _share_changed(self, index: int) -> None:
        if 0 <= index < len(self.peer.shares):
            self.share = self.peer.shares[index]
            self.search.clear()
            self.load("")

    # --- fraîcheur ------------------------------------------------------
    def showEvent(self, event) -> None:  # noqa: N802 (API Qt)
        super().showEvent(event)
        self.watch.start()

    def hideEvent(self, event) -> None:  # noqa: N802 (API Qt)
        super().hideEvent(event)
        self.watch.stop()

    def _poll(self) -> None:
        if self._tagging or not self.peer or not self.share or self._searching:
            return
        window = self.window()
        if not self.isVisible() or not window.isVisible() or window.isMinimized():
            return  # rien ne tourne quand on ne regarde pas
        # Fenêtre visible mais au second plan : on ralentit (10 s au lieu de 3).
        self._idle_ticks += 1
        if not window.isActiveWindow() and self._idle_ticks % 3:
            return
        self._tagging = True
        tagger = TagLoader(
            self._token,
            self.peer.base_url(),
            self.share.get("id", ""),
            self.path,
            self._etag,
            self,
        )
        tagger.done.connect(self._tag_ready)
        self._tagger = tagger
        tagger.start()

    def _tag_ready(self, token: int, tag: str, error: str) -> None:
        self._tagging = False
        if token != self._token:
            return  # la page a changé pendant la sonde
        if error:
            self.selection.setText(f"Poste injoignable : {error}")
            return
        if tag and tag != self._etag:
            self.load(self.path)  # le dossier a bougé : un seul listing, à jour

    # --- chargement -----------------------------------------------------
    def load(self, path: str) -> None:
        if not self.peer or not self.share:
            return
        self.path = path
        self._offset = 0
        self._truncated = False
        self._searching = False
        self.breadcrumb.setText("/" + path if path else "/")
        self.table.setColumnHidden(1, True)
        self._token += 1
        self._etag = ""
        self._fetch()

    def _fetch(self) -> None:
        if not self.peer or not self.share or self._loading:
            return
        self._loading = True
        loader = ListLoader(
            self._token,
            self.peer.base_url(),
            self.share.get("id", ""),
            self.path,
            self._offset,
            PAGE,
            self._sort,
            self._desc,
            self,
        )
        loader.loaded.connect(self._page_loaded)
        loader.failed.connect(self._failed)
        self._loader = loader
        loader.start()

    def _maybe_more(self, value: int) -> None:
        """Défilement arrivé en bas : page suivante, sans bouton à cliquer."""
        if not self._truncated or self._loading or self._searching:
            return
        bar = self.table.verticalScrollBar()
        if value >= bar.maximum() - bar.pageStep():
            self._fetch()

    def _page_loaded(self, token: int, payload: dict, etag: str) -> None:
        self._loading = False
        if token != self._token:
            return  # réponse périmée
        if self._offset == 0:
            self.payload = payload
            self._etag = etag
            self.table.setRowCount(0)
        entries = payload.get("entries", [])
        self._fill(entries, append=self._offset > 0)
        self._offset += len(entries)
        self._truncated = bool(payload.get("truncated"))
        if self._offset == 0:
            self.selection.setText("Ce dossier est vide.")
        elif self._truncated and len(entries) == 0:
            self._truncated = False  # garde-fou : plus rien à demander
        self._update_breadcrumb()
        self._selection_changed()

    def _update_breadcrumb(self) -> None:
        base = "/" + self.path if self.path else "/"
        if self._truncated:
            base += f"  ({self._offset} entrées affichées, la suite arrive en défilant)"
        self.breadcrumb.setText(base)

    def _fill(self, entries: list[dict], append: bool) -> None:
        for entry in entries:
            entry.setdefault("path", _join(self.path, entry.get("name", "")))
            row = self.table.rowCount()
            self.table.insertRow(row)
            name = QTableWidgetItem(entry.get("name", ""))
            name.setData(Qt.UserRole, entry)
            if entry.get("type") == "dir":
                name.setIcon(self.style().standardIcon(QStyle.SP_DirIcon))
            else:
                name.setIcon(self.style().standardIcon(QStyle.SP_FileIcon))
            self.table.setItem(row, 0, name)
            parent_relative = entry.get("path", "").rsplit("/", 1)[0] if "/" in entry.get("path", "") else ""
            self.table.setItem(row, 1, QTableWidgetItem(parent_relative))
            self.table.setItem(row, 2, QTableWidgetItem(self._size_text(entry)))
            self.table.setItem(row, 3, QTableWidgetItem(human_date(entry.get("mtime", 0))))

    def _size_text(self, entry: dict) -> str:
        if entry.get("type") != "dir":
            return human_size(entry.get("size", 0))
        known = self._sizes.get((self.share.get("id", "") if self.share else "", entry.get("path", "")))
        return human_size(known["bytes"]) + ("+" if known and not known.get("complete", True) else "") if known else ""

    def _failed(self, token: int, message: str) -> None:
        self._loading = False
        if token != self._token:
            return
        self.table.setRowCount(0)
        self.selection.setText(f"Impossible de lire ce dossier : {message}")

    # --- tri ------------------------------------------------------------
    def _header_clicked(self, index: int) -> None:
        column = {0: "name", 2: "size", 3: "mtime"}.get(index)
        if column is None:
            return
        if column == self._sort:
            self._desc = not self._desc
        else:
            self._sort = column
            self._desc = False
        self.table.horizontalHeader().setSortIndicator(
            index, Qt.DescendingOrder if self._desc else Qt.AscendingOrder
        )
        if self._searching:
            self._run_search()
        else:
            self.load(self.path)

    # --- recherche ------------------------------------------------------
    def _search_changed(self, text: str) -> None:
        if not text.strip():
            self._search_timer.stop()
            if self._searching:
                self.load(self.path)
            return
        self._search_timer.start()

    def _run_search(self) -> None:
        text = self.search.text().strip()
        if not text or not self.peer or not self.share:
            return
        self._token += 1
        self._searching = True
        loader = SearchLoader(
            self._token, self.peer.base_url(), self.share.get("id", ""), text, self.path, self
        )
        loader.found.connect(self._search_done)
        loader.failed.connect(self._failed)
        self._search_loader = loader
        loader.start()

    def _search_done(self, token: int, payload: dict) -> None:
        if token != self._token:
            return
        entries = payload.get("entries", [])
        self.table.setRowCount(0)
        self.table.setColumnHidden(1, False)
        self._fill(entries, append=False)
        suffix = " (résultats partiels)" if payload.get("truncated") else ""
        self.selection.setText(
            f"Recherche « {payload.get('query', '')} » : {len(entries)} résultat(s){suffix}."
        )
        self.breadcrumb.setText(
            f"/{self.path + '  ' if self.path else ''}recherche dans tout le partage"
        )

    # --- navigation -----------------------------------------------------
    def go_back(self) -> None:
        if self._searching:
            self.search.clear()
            self.load(self.path)
            return
        parent = self.payload.get("parent")
        if parent is None:
            self.closed.emit()
        else:
            self.load(parent)

    def _activated(self, item: QTableWidgetItem) -> None:
        entry = item.data(Qt.UserRole) or {}
        if entry.get("type") == "dir":
            target = entry.get("path", "")
            self.search.blockSignals(True)
            self.search.clear()
            self.search.blockSignals(False)
            self.load(target)

    # --- sélection ------------------------------------------------------
    def _selected(self) -> list[dict]:
        rows = sorted({index.row() for index in self.table.selectedIndexes()})
        entries = []
        for row in rows:
            item = self.table.item(row, 0)
            if item is not None:
                entries.append(item.data(Qt.UserRole) or {})
        return entries

    def _selection_changed(self) -> None:
        entries = self._selected()
        self.download.setEnabled(bool(entries))
        self.save.setEnabled(bool(entries))
        if not entries:
            if not self._searching:
                self.selection.setText("Sélectionnez un fichier ou un dossier.")
            return
        if len(entries) == 1:
            entry = entries[0]
            if entry.get("type") == "dir":
                self.selection.setText(f"{entry.get('name', '')} · dossier")
                self._request_size(entry)
            else:
                self.selection.setText(
                    f"{entry.get('name', '')} · {human_size(entry.get('size', 0))}"
                )
        else:
            files = [entry for entry in entries if entry.get("type") != "dir"]
            total = sum(entry.get("size", 0) for entry in files)
            self.selection.setText(f"{len(entries)} éléments · {human_size(total)}")

    def _request_size(self, entry: dict) -> None:
        if not self.share or self._size_loader is not None:
            return
        key = (self.share.get("id", ""), entry.get("path", ""))
        if key in self._sizes:
            known = self._sizes[key]
            suffix = " (au moins)" if not known.get("complete", True) else ""
            self.selection.setText(
                f"{entry.get('name', '')} · {known['files']} fichier(s) · "
                f"{human_size(known['bytes'])}{suffix}"
            )
            return
        loader = SizeLoader(self.peer.base_url(), key[0], key[1], self)
        loader.done.connect(self._size_ready)
        loader.failed.connect(lambda *_: None)
        self._size_loader = loader
        loader.start()

    def _size_ready(self, share_id: str, path: str, payload: dict) -> None:
        self._size_loader = None
        self._sizes[(share_id, path)] = payload
        # Rafraîchit la colonne Taille et le pied de page si la ligne est visible.
        for row in range(self.table.rowCount()):
            item = self.table.item(row, 0)
            entry = item.data(Qt.UserRole) if item else None
            if entry and entry.get("path") == path:
                self.table.setItem(row, 2, QTableWidgetItem(self._size_text(entry)))
                break
        entries = self._selected()
        if len(entries) == 1 and entries[0].get("path") == path:
            self._selection_changed()

    # --- téléchargement -------------------------------------------------
    def _request_download(self, choose: bool) -> None:
        if not self.peer or not self.share:
            return
        for entry in self._selected():
            is_dir = entry.get("type") == "dir"
            self.download_requested.emit(
                self.peer,
                self.share,
                entry.get("path", ""),
                is_dir,
                choose,
            )

    def _download_whole_share(self) -> None:
        if self.peer and self.share:
            self.download_requested.emit(self.peer, self.share, "", True, False)

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
        self.percent.setMinimumWidth(84)  # « Terminé » ne doit pas être coupé
        top.addWidget(self.percent)
        self.pause = QPushButton("Pause")
        self.pause.setObjectName("flat")
        self.pause.setToolTip("Suspendre ; la progression est gardée sur disque")
        top.addWidget(self.pause)
        self.retry = QPushButton("Relancer")
        self.retry.setObjectName("flat")
        top.addWidget(self.retry)
        self.open = QPushButton("Ouvrir")
        self.open.setObjectName("flat")
        self.open.clicked.connect(self._open_folder)
        top.addWidget(self.open)
        self.remove = QPushButton("Retirer")
        self.remove.setObjectName("flat")
        self.remove.setToolTip("Retirer de la liste et effacer le fichier partiel (.part)")
        top.addWidget(self.remove)
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
        self.pause.setVisible(job.state in ("en attente", "en cours", "en pause"))
        self.pause.setText("Reprendre" if job.state == "en pause" else "Pause")
        self.retry.setVisible(job.state == "échec" or job.waiting())
        self.remove.setVisible(job.state not in ("en cours",))
        if job.state == "terminé":
            self.bar.setVisible(False)
            self.percent.setText("Terminé")
        elif job.state == "en pause":
            self.bar.setVisible(True)
            self.bar.setRange(0, 1000)
            self.bar.setValue(int(job.fraction() * 1000))
            self.percent.setText("En pause")
        elif job.waiting():
            self.bar.setVisible(False)
            self.percent.setText("En attente")
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
        speed, remaining, eta = self.downloads.summary()
        titre = f"Transferts ({active} en cours)" if active else "Transferts"
        if speed > 0:
            titre += f" · {human_speed(speed)}"
            if remaining:
                titre += f" · reste {human_size(remaining)} (~{human_eta(eta)})"
        self.header.setText(titre)
        for job in jobs:
            row = self._rows.get(id(job))
            if row is None:
                row = TransferRow(job)
                row.retry.clicked.connect(lambda _=False, item=job: self.downloads.retry(item))
                row.remove.clicked.connect(lambda _=False, item=job: self.downloads.remove(item))
                row.pause.clicked.connect(
                    lambda _=False, item=job: self.downloads.resume(item)
                    if item.state == "en pause"
                    else self.downloads.pause(item)
                )
                self._rows[id(job)] = row
                self.body.addWidget(row)
            row.update_job(job)
        for key in list(self._rows):
            if all(id(job) != key for job in jobs):
                widget = self._rows.pop(key)
                widget.setParent(None)
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

        # Un disque externe branché/débranché ne touche pas share.json : on
        # revérifie la présence des dossiers (un `stat` chacun) toutes les 2 s,
        # et seulement fenêtre visible. C'est le démon qui décide de servir et
        # d'annoncer ; ici on ne fait que dire la vérité dans la liste.
        self.availability = QTimer(self)
        self.availability.setInterval(2000)
        self.availability.timeout.connect(self._check_availability)

        # câblage
        self.shares.changed.connect(self._shares_changed)
        self.shares.permanent_changed.connect(self._permanent_changed)
        self.peers.open_peer.connect(self._open_peer)
        self.peers.refresh_requested.connect(self.discovery.refresh_now)
        self.browser.closed.connect(lambda: self.stack.setCurrentWidget(self.peers))
        self.browser.download_requested.connect(self._download)
        self.discovery.peers_changed.connect(self._peers_changed)
        self.downloads.changed.connect(self._downloads_changed)
        self.downloads.finished.connect(self._download_finished)

        ok, message = self.daemon.start() if autostart else (True, "")
        if not ok:
            self.shares.status.setText(
                f"{self.shares.status.text()}\n\nLe démon n'a pas démarré : {message}"
            )
        elif autostart:
            self.discovery.start()
        self._downloads_changed()
        self.availability.start()

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
    def _peers_changed(self, peers) -> None:
        self.peers.set_peers(peers)
        # C'est ici que la file se rattache : un poste qui revient relance ses
        # transferts en attente, sans intervention.
        self.downloads.sync_peers(peers)
        # Et si ce poste est celui qu'on parcourt, ses partages ont pu changer
        # (disque branché chez lui, partage renommé, ajouté, retiré).
        self.browser.sync_peer(peers)

    def _check_availability(self) -> None:
        if not self.isVisible() or self.isMinimized():
            return
        self.shares.refresh_availability()

    def _open_peer(self, peer) -> None:
        self.browser.open(peer)
        self.stack.setCurrentWidget(self.browser)

    # --- permanence -----------------------------------------------------
    def _permanent_changed(self, wanted: bool) -> None:
        if wanted:
            # Le démon de la GUI laisse la place à l'unité systemd.
            self.daemon.stop()
            ok, message = service.enable()
            if not ok:
                self.daemon.start()
        else:
            ok, message = service.disable()
            if ok:
                ok, message = self.daemon.start()
        if not ok:
            QMessageBox.warning(self, APP_NAME, message or "Opération impossible.")
            self.shares.permanent.blockSignals(True)
            self.shares.permanent.setChecked(not wanted)
            self.shares.permanent.blockSignals(False)
            self.shares._refresh_addresses()

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

    def _download_finished(self, job, success: bool) -> None:
        """Notification de bureau : inutile si la fenêtre a le focus."""
        if self.isActiveWindow() and not self.isMinimized():
            return
        if success:
            notify("Transfert terminé", job.title())
        else:
            notify("Transfert en échec", f"{job.title()}\n{job.error}")

    # --- fermeture ------------------------------------------------------
    def closeEvent(self, event) -> None:  # noqa: N802 (API Qt)
        self.discovery.stop()
        self.downloads.shutdown()
        self.daemon.stop()
        super().closeEvent(event)
