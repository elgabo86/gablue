"""Fenêtre principale : barre de l'application, partages trouvés, navigation, transferts."""

import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from PySide6.QtCore import (
    QEasingCurve,
    QEvent,
    QObject,
    QPoint,
    QPropertyAnimation,
    Qt,
    QThread,
    QTimer,
    QUrl,
    Signal,
)
from PySide6.QtGui import (
    QColor,
    QDesktopServices,
    QKeySequence,
    QPainter,
    QPalette,
    QPen,
    QShortcut,
)
from PySide6.QtNetwork import QLocalServer
from PySide6.QtWidgets import (
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
from .constants import APP_NAME, CONTROL_SOCKET, RUNTIME_DIR, download_dir
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
    option_changed = Signal(str, str, bool)  # partage, option, valeur

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
        options = label(self._options_text(), "hint")
        options.setWordWrap(False)
        texts.addWidget(options)
        layout.addLayout(texts, 1)
        button = QPushButton("✕")
        button.setObjectName("flat")
        button.setFixedWidth(32)
        button.setToolTip("Ne plus partager ce dossier")
        button.clicked.connect(lambda: self.remove_requested.emit(share.get("id", "")))
        layout.addWidget(button)

    def _options_text(self) -> str:
        """Ce que les autres postes verront, en deux mots."""
        caches = "cachés affichés" if self.share.get("hidden") else "cachés masqués"
        sous = (
            "sous-dossiers inclus"
            if self.share.get("recursive", True)
            else "racine seulement"
        )
        return f"{caches} · {sous}"

    def contextMenuEvent(self, event) -> None:  # noqa: N802 (API Qt)
        menu = QMenu(self)
        rename = menu.addAction("Renommer…")
        menu.addSeparator()
        hidden = menu.addAction("Afficher les fichiers cachés")
        hidden.setCheckable(True)
        hidden.setChecked(bool(self.share.get("hidden")))
        hidden.setToolTip(
            "Les noms qui commencent par un point.\n"
            "Un dossier téléchargé les emporte de toute façon."
        )
        recursive = menu.addAction("Inclure les sous-dossiers")
        recursive.setCheckable(True)
        recursive.setChecked(bool(self.share.get("recursive", True)))
        recursive.setToolTip(
            "Décoché : seuls les fichiers de la racine du partage sont proposés.\n"
            "Le téléchargement du partage entier reste complet."
        )
        menu.addSeparator()
        remove = menu.addAction("Ne plus partager")
        chosen = menu.exec(event.globalPos())
        identifiant = self.share.get("id", "")
        if chosen is rename:
            self.rename_requested.emit(identifiant)
        elif chosen is hidden:
            self.option_changed.emit(identifiant, "hidden", hidden.isChecked())
        elif chosen is recursive:
            self.option_changed.emit(identifiant, "recursive", recursive.isChecked())
        elif chosen is remove:
            self.remove_requested.emit(identifiant)


class SharePanel(QFrame):
    """Liste éditable des dossiers partagés, ouverte en tiroir depuis la navigation."""

    changed = Signal()
    mode_changed = Signal(str)
    visibility_changed = Signal(str)

    def __init__(self, document: dict, parent=None):
        super().__init__(parent)
        self.document = document
        self._known_availability: list[bool] = []
        # Message d'un autre compte qui occupe le poste ("" = rien à signaler).
        self._blocked_message = ""
        self.setObjectName("panel")
        self.setFixedWidth(340)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(10)
        layout.addWidget(label("Mes partages", "title"))
        # Bannière : un autre compte du poste occupe déjà le partage. Elle est
        # dans le tiroir, mais aussi rappelée par le bouton de la barre — le
        # tiroir est fermé par défaut, donc la bannière seule ne suffit pas.
        self.banner = label("", "alert")
        self.banner.hide()
        layout.addWidget(self.banner)
        self.status = label("", "muted")
        layout.addWidget(self.status)

        self.rows = QVBoxLayout()
        self.rows.setSpacing(0)
        layout.addLayout(self.rows)
        layout.addStretch(1)

        add = QPushButton("Ajouter un dossier…")
        add.setObjectName("primary")
        add.clicked.connect(self.add_folder)
        self.add_button = add
        layout.addWidget(add)

        demarrage = QHBoxLayout()
        demarrage.addWidget(label("Démarrage", "hint"))
        self.permanent = QComboBox()
        for cle, libelle in service.MODES.items():
            self.permanent.addItem(libelle, cle)
        index = self.permanent.findData(service.current_mode())
        self.permanent.setCurrentIndex(max(0, index))
        self.permanent.setToolTip(
            "Quand le partage reste actif sans la fenêtre :\n"
            "· Avec la session : service systemd utilisateur, lancé à "
            "l'ouverture de session ;\n"
            "· Au démarrage du poste : idem, plus « linger » — systemd démarre "
            "le service dès le boot, même sans ouvrir de session."
        )
        self.permanent.currentIndexChanged.connect(self._mode_changed)
        demarrage.addWidget(self.permanent, 1)
        layout.addLayout(demarrage)

        visibilite = QHBoxLayout()
        visibilite.addWidget(label("Visible sur", "hint"))
        self.visibility = QComboBox()
        for cle, libelle in config.VISIBILITIES.items():
            self.visibility.addItem(libelle, cle)
        index = self.visibility.findData(config.visibility(document))
        self.visibility.setCurrentIndex(max(0, index))
        self.visibility.setToolTip(
            "Réseau local : les postes du même réseau (annonce mDNS).\n"
            "Tailscale : le tailnet seulement, sans annonce locale.\n"
            "Les deux : la connexion la plus rapide l'emporte."
        )
        self.visibility.currentIndexChanged.connect(self._visibility_changed)
        visibilite.addWidget(self.visibility, 1)
        layout.addLayout(visibilite)

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
            row.option_changed.connect(self.change_option)
            self.rows.addWidget(row)
        index = self.visibility.findData(config.visibility(self.document))
        if index >= 0 and index != self.visibility.currentIndex():
            self.visibility.blockSignals(True)
            self.visibility.setCurrentIndex(index)
            self.visibility.blockSignals(False)
        present = sum(1 for available in availability if available)
        missing = len(shares) - present
        if self._blocked_message:
            # Dire « 1 dossier partagé, visible sur… » alors qu'un autre compte
            # occupe le poste serait faux : rien n'est servi.
            nombre = len(shares)
            self.status.setText(
                f"{nombre} dossier{'s' if nombre > 1 else ''} déclaré"
                f"{'s' if nombre > 1 else ''} — rien n'est partagé pour l'instant."
            )
        elif not shares:
            self.status.setText(
                "Aucun dossier partagé. Ajoutez un dossier pour commencer."
            )
        elif present == 0:
            self.status.setText(
                f"{len(shares)} dossier(s) déclaré(s), aucun disponible. "
                "Branchez le disque ou vérifiez le chemin."
            )
        else:
            suffix = f" — {missing} débranché(s), non annoncé(s)" if missing else ""
            visibilite = config.visibility(self.document)
            ou = {
                "both": "visible sur le réseau local et le tailnet",
                "lan": "visible sur le réseau local seulement (rien depuis le tailnet)",
                "tailscale": "visible sur le tailnet seulement (aucune annonce locale)",
            }[visibilite]
            self.status.setText(
                f"{present} dossier{'s' if present > 1 else ''} partagé"
                f"{'s' if present > 1 else ''} — {ou}{suffix}."
            )

    def refresh_availability(self) -> None:
        """Un disque branché/débranché change la liste, pas le fichier de config."""
        if self._availability() != getattr(self, "_known_availability", None):
            self.reload()

    def set_blocked(self, message: str) -> None:
        """Un autre compte occupe le poste : on empêche de « partager » à vide.

        Ajouter un dossier, choisir la permanence ou la visibilité produirait un
        effet ici et **aucun** sur le réseau : c'est le démon de l'autre compte
        qui écoute. Ces commandes sont désactivées, avec la raison en infobulle,
        plutôt que de laisser croire qu'elles agissent.
        """
        self.banner.setText(message)
        self.banner.setVisible(bool(message))
        for widget in (self.add_button, self.permanent, self.visibility):
            widget.setEnabled(not message)
        self.add_button.setToolTip(message)
        self._blocked_message = message
        self.reload()

    def _mode_changed(self, index: int) -> None:
        mode = self.permanent.itemData(index)
        if mode:
            self.mode_changed.emit(mode)

    def revert_mode(self) -> None:
        """Remet le sélecteur sur l'état réel (systemd fait foi)."""
        index = self.permanent.findData(service.current_mode())
        if index >= 0 and index != self.permanent.currentIndex():
            self.permanent.blockSignals(True)
            self.permanent.setCurrentIndex(index)
            self.permanent.blockSignals(False)

    def _visibility_changed(self, index: int) -> None:
        value = self.visibility.itemData(index)
        if not value:
            return
        ok, message = config.set_visibility(self.document, value)
        if not ok:
            QMessageBox.warning(self, APP_NAME, message)
            return
        self.visibility_changed.emit(value)
        self.reload()

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

    def change_option(self, share_id: str, key: str, value: bool) -> None:
        """Le démon relit le fichier à chaud (~3 s) : rien à redémarrer."""
        ok, message = config.set_share_option(self.document, share_id, key, value)
        if not ok:
            QMessageBox.warning(self, APP_NAME, message)
            self.reload()
            return
        self.reload()
        self.changed.emit()

    def remove_share(self, share_id: str) -> None:
        if config.remove_share(self.document, share_id):
            self.reload()
            self.changed.emit()


# ---------------------------------------------------------------------------
# Partages trouvés chez les autres postes
# ---------------------------------------------------------------------------
class RemoteShareRow(QFrame):
    """Un partage d'un autre poste, avec le nom du poste qui le sert.

    Pas d'écran intermédiaire « postes à proximité » : ce qu'on cherche, c'est
    un dossier, pas une machine.
    """

    opened = Signal(object, object)  # poste, partage

    def __init__(self, peer, share: dict, parent=None):
        super().__init__(parent)
        self.peer = peer
        self.share = share
        self.setObjectName("card")
        self.setCursor(Qt.PointingHandCursor)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(14, 12, 14, 12)
        layout.setSpacing(12)
        texts = QVBoxLayout()
        texts.setSpacing(3)
        texts.addWidget(label(share.get("name") or share.get("id", "Partage"), "section"))
        texts.addWidget(label(self._origine(), "muted"))
        layout.addLayout(texts, 1)
        layout.addWidget(label("Ouvrir ›", "hint"))

    def _origine(self) -> str:
        if getattr(self.peer, "local", False):
            return "Ce poste"
        return f"{self.peer.name} · {self.peer.label()}"

    def mouseReleaseEvent(self, event) -> None:  # noqa: N802 (API Qt)
        self.opened.emit(self.peer, self.share)


class GlobalSearchLoader(QThread):
    """Recherche dans **tous** les partages trouvés, en parallèle.

    Un seul fil pour la GUI (une frappe = un fil), mais les partages sont
    interrogés en parallèle : sur dix partages, une recherche séquentielle
    coûterait dix fois le budget de 400 ms du serveur. Le total reste borné
    par le nombre de partages et le plafond de résultats par partage.
    """

    done = Signal(int, list, bool)  # token, résultats, tronqué

    MAX_PARALLELE = 6
    PAR_PARTAGE = 60

    def __init__(self, token: int, targets, query: str, parent=None):
        super().__init__(parent)
        self.token = token
        self.targets = list(targets)  # [(poste, partage)]
        self.query = query

    def run(self) -> None:
        rows: list[dict] = []
        truncated = False
        with ThreadPoolExecutor(max_workers=self.MAX_PARALLELE) as pool:
            futures = {}
            for peer, share in self.targets:
                base = peer.base_url()
                if not base:
                    continue
                futures[
                    pool.submit(
                        api.find, base, str(share.get("id", "")), self.query, "", self.PAR_PARTAGE
                    )
                ] = (peer, share)
            for future in as_completed(futures):
                if self.isInterruptionRequested():
                    return
                peer, share = futures[future]
                try:
                    payload = future.result()
                except api.ApiError:
                    continue  # un partage muet ne prive pas des autres
                except Exception:  # noqa: BLE001 (recherche : jamais bloquante)
                    continue
                if payload.get("truncated"):
                    truncated = True
                for entry in payload.get("entries", []):
                    rows.append(
                        {
                            "name": entry.get("name", ""),
                            "path": entry.get("path", ""),
                            "type": entry.get("type", "file"),
                            "size": entry.get("size", 0),
                            "mtime": entry.get("mtime", 0),
                            "peer": peer,
                            "share": share,
                        }
                    )
        rows.sort(key=lambda row: (row["name"].lower(), row["path"].lower()))
        if not self.isInterruptionRequested():
            self.done.emit(self.token, rows, truncated)


class ResultsSizeLoader(QThread):
    """Taille des dossiers apparus dans les résultats, à la demande.

    Chaque taille est un parcours **borné à 600 ms côté serveur** : sans limite
    de concurrence, une recherche qui remonte trente dossiers lancerait trente
    parcours simultanés chez le poste d'en face. Trois à la fois, et on
    s'arrête dès qu'une nouvelle recherche prend la main.
    """

    ready = Signal(int, str, str, str, dict)  # token, poste, partage, chemin, réponse

    MAX_PARALLELE = 3

    def __init__(self, token: int, jobs, parent=None):
        super().__init__(parent)
        self.token = token
        self.jobs = list(jobs)  # [(peer_id, base, share_id, path)]

    def run(self) -> None:
        with ThreadPoolExecutor(max_workers=self.MAX_PARALLELE) as pool:
            futures = {}
            for peer_id, base, share_id, path in self.jobs:
                futures[pool.submit(api.folder_size, base, share_id, path)] = (
                    peer_id,
                    share_id,
                    path,
                )
            for future in as_completed(futures):
                if self.isInterruptionRequested():
                    return
                peer_id, share_id, path = futures[future]
                try:
                    payload = future.result()
                except Exception:  # noqa: BLE001 (une taille manquante n'est pas une erreur)
                    continue
                self.ready.emit(self.token, peer_id, share_id, path, payload)


class Spinner(QWidget):
    """Arc qui tourne : « je cherche », sans texte à lire.

    Peint à la main plutôt qu'une barre de progression indéterminée : sur 26 px,
    la barre de Qt ressemble à un trait qui saute, pas à une recherche en cours.
    Il ne tourne que tant que l'écran vide est affiché.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedSize(26, 26)
        self._angle = 0
        self._timer = QTimer(self)
        self._timer.setInterval(80)
        self._timer.timeout.connect(self._turn)

    def start(self) -> None:
        if not self._timer.isActive():
            self._timer.start()

    def stop(self) -> None:
        self._timer.stop()

    def _turn(self) -> None:
        self._angle = (self._angle + 30) % 360
        self.update()

    def paintEvent(self, event) -> None:  # noqa: N802 (API Qt)
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        rect = self.rect().adjusted(4, 4, -4, -4)
        accent = self.palette().color(QPalette.Highlight)
        # Piste discrète : l'arc se lit comme un tour complet, sans clignoter.
        piste = QColor(accent)
        piste.setAlpha(60)
        pen = QPen(piste, 3)
        pen.setCapStyle(Qt.RoundCap)
        painter.setPen(pen)
        painter.drawEllipse(rect)
        pen.setColor(accent)
        painter.setPen(pen)
        painter.drawArc(rect, -self._angle * 16, 100 * 16)  # 100° qui tournent


class Scrim(QWidget):
    """Voile sous le tiroir : un clic à côté le referme, sans bouton à viser."""

    clicked = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("scrim")
        # Sans cet attribut, un QWidget nu ignore le fond de la feuille de
        # style : le voile était invisible (constaté 15/11/2026).
        self.setAttribute(Qt.WA_StyledBackground, True)
        self.setCursor(Qt.ArrowCursor)

    def mousePressEvent(self, event) -> None:  # noqa: N802 (API Qt)
        self.clicked.emit()


class ColumnFitter(QObject):
    """La colonne « Nom » absorbe la largeur restante du tableau.

    Sans cela la grille s'arrêtait à « Modifié » et laissait un vide à droite
    (vécu 15/11/2026). Les colonnes restent réglables : élargir « Taille » ou
    « Modifié » rogne « Nom », qui reste la plus large, et une fenêtre plus
    large lui redonne le surplus. Le suivi passe par un filtre d'événements sur
    le tableau, donc l'apparition d'une barre de défilement (qui rétrécit la
    zone utile) est prise en compte comme un redimensionnement.
    """

    MINIMUM = 140

    def __init__(self, table: QTableWidget, parent=None):
        super().__init__(parent or table)
        self.table = table
        self._fitting = False
        table.installEventFilter(self)
        # QTableWidget expose `horizontalHeader()`, QTreeWidget seulement `header()`.
        entete = getattr(table, "horizontalHeader", None)
        entete = entete() if callable(entete) else table.header()
        entete.sectionResized.connect(self._section_resized)

    def fit(self) -> None:
        if self._fitting:
            return
        self._fitting = True
        try:
            autres = sum(
                self.table.columnWidth(colonne)
                for colonne in range(1, self.table.columnCount())
                if not self.table.isColumnHidden(colonne)
            )
            largeur = max(self.table.viewport().width() - autres - 2, self.MINIMUM)
            changed = self.table.columnWidth(0) != largeur
            if changed:
                self.table.setColumnWidth(0, largeur)
        finally:
            self._fitting = False
        if changed:
            # La mise en page peut encore bouger après ce passage (une barre de
            # défilement qui apparaît, une marge) : un second essai une fois la
            # boucle rendue, et il s'arrête de lui-même quand plus rien ne change.
            QTimer.singleShot(0, self.fit)

    def _section_resized(self, index: int, old: int, new: int) -> None:
        if index != 0:  # toucher une autre colonne rogne « Nom »
            self.fit()

    def eventFilter(self, obj, event) -> bool:  # noqa: N802 (API Qt)
        if obj is self.table and event.type() == QEvent.Resize:
            self.fit()
        return super().eventFilter(obj, event)


class PeerPage(QWidget):
    open_share = Signal(object, object)  # poste, partage
    open_result = Signal(object, object, str)  # poste, partage, chemin visé
    #: poste, partage, chemin, est un dossier, choisir la destination
    download_requested = Signal(object, object, str, bool, bool)
    refresh_requested = Signal()

    COLONNES = ["Nom", "Dossier", "Partage", "Poste", "Taille"]

    def __init__(self, parent=None):
        super().__init__(parent)
        self.targets: list[tuple[object, dict]] = []
        self._token = 0
        self._loader: GlobalSearchLoader | None = None
        #: Tailles déjà calculées, par (poste, partage, chemin).
        self._sizes: dict[tuple[str, str, str], dict] = {}
        self._size_loader: ResultsSizeLoader | None = None

        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 16, 20, 16)
        layout.setSpacing(12)

        header = QHBoxLayout()
        self.title = label("Partages trouvés", "title")
        header.addWidget(self.title, 1)
        self.refresh = QPushButton("Actualiser")
        self.refresh.clicked.connect(self.refresh_requested.emit)
        header.addWidget(self.refresh)
        layout.addLayout(header)

        # Recherche globale : tous les partages à la fois, sans choisir un poste.
        self.search = QLineEdit()
        self.search.setPlaceholderText(
            "Rechercher un fichier ou un dossier dans tous les partages…"
        )
        self.search.setClearButtonEnabled(True)
        self.search.textChanged.connect(self._search_changed)
        layout.addWidget(self.search)

        self.hint = label("", "hint")
        layout.addWidget(self.hint)

        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.body = QWidget()
        self.rows = QVBoxLayout(self.body)
        self.rows.setContentsMargins(0, 0, 0, 0)
        self.rows.setSpacing(8)
        self.rows.addStretch(1)
        self.scroll.setWidget(self.body)
        layout.addWidget(self.scroll, 1)

        # Rien trouvé : un arc qui tourne et deux lignes. La découverte rescane
        # toutes les 5 s tant que la liste est vide — l'animation dit qu'on
        # cherche encore, sans qu'on ait à deviner si l'application est figée
        # (demandé le 15/11/2026). Elle s'arrête dès le premier partage trouvé.
        self.empty = QWidget()
        vide = QVBoxLayout(self.empty)
        vide.setContentsMargins(0, 0, 0, 0)
        vide.setSpacing(10)
        vide.addStretch(1)
        self.spinner = Spinner(self.empty)
        vide.addWidget(self.spinner, 0, Qt.AlignHCenter)
        self.empty_main = label(
            "Recherche de partages sur le réseau local et le tailnet…", "muted"
        )
        self.empty_main.setAlignment(Qt.AlignCenter)
        vide.addWidget(self.empty_main)
        self.empty_hint = label(
            "Un partage apparaît dès qu'un poste ouvre Gablue Share avec au moins un dossier.",
            "hint",
        )
        self.empty_hint.setAlignment(Qt.AlignCenter)
        vide.addWidget(self.empty_hint)
        vide.addStretch(1)
        layout.addWidget(self.empty, 1)
        self.empty.hide()

        self.results = QTableWidget(0, len(self.COLONNES))
        self.results.setHorizontalHeaderLabels(self.COLONNES)
        self.results.verticalHeader().setVisible(False)
        self.results.setShowGrid(False)
        self.results.setSelectionBehavior(QTableWidget.SelectRows)
        self.results.setSelectionMode(QTableWidget.ExtendedSelection)
        self.results.setEditTriggers(QTableWidget.NoEditTriggers)
        self.results.setSortingEnabled(False)
        entetes = self.results.horizontalHeader()
        entetes.setDefaultAlignment(Qt.AlignLeft | Qt.AlignVCenter)
        entetes.setSectionResizeMode(QHeaderView.Interactive)
        entetes.setStretchLastSection(False)
        for colonne, largeur in enumerate((275, 115, 130, 80, 90)):
            self.results.setColumnWidth(colonne, largeur)
        self.results.itemDoubleClicked.connect(self._result_activated)
        self.results.setContextMenuPolicy(Qt.CustomContextMenu)
        self.results.customContextMenuRequested.connect(self._results_menu)
        self.results.hide()
        layout.addWidget(self.results, 1)
        self.columns = ColumnFitter(self.results, self)

        # La frappe se calme (300 ms) avant d'interroger : une requête par pause,
        # pas une par caractère.
        self._search_timer = QTimer(self)
        self._search_timer.setSingleShot(True)
        self._search_timer.setInterval(300)
        self._search_timer.timeout.connect(self._run_search)

        if not discovery_available():
            self.hint.setText(
                "Ni Avahi ni Tailscale ne sont disponibles : la découverte est limitée."
            )

    # --- liste des partages ---------------------------------------------
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
        # Un poste peut servir plusieurs dossiers : chaque partage est une
        # entrée. Groupés par poste (puis par nom) pour garder le contexte.
        ordered = sorted(
            peers, key=lambda item: (not getattr(item, "local", False), item.name.lower())
        )
        entries = [(peer, share) for peer in ordered for share in peer.shares]
        entries.sort(
            key=lambda item: (
                not getattr(item[0], "local", False),
                item[0].name.lower(),
                str(item[1].get("name", "")).lower(),
            )
        )
        self.targets = entries
        # Rien trouvé : l'écran ne garde que l'explication. Un titre, une barre
        # de recherche et un « Actualiser » au-dessus du vide n'apprennent rien —
        # et la rescousse est automatique (voir `MainWindow._scan_quick`).
        vide = not entries
        for widget in (self.title, self.refresh, self.search):
            widget.setVisible(not vide)
        if vide:
            if self.search.text():
                self.search.clear()  # remet la liste au propre, sans résultats
            self.hint.clear()
        if not entries:
            self._show_empty()
        else:
            self._show_list()
            for peer, share in entries:
                row = RemoteShareRow(peer, share)
                row.opened.connect(self.open_share.emit)
                self.rows.addWidget(row)
        self.rows.addStretch(1)
        if entries:
            postes = len({peer.peer_id for peer, _ in entries})
            miens = sum(1 for peer, _ in entries if getattr(peer, "local", False))
            self.hint.setText(
                f"{len(entries)} partage{'s' if len(entries) > 1 else ''} sur "
                f"{postes} poste{'s' if postes > 1 else ''}"
            )
        # La liste a bougé : une recherche en cours doit voir les nouveaux
        # partages (un poste qui arrive pendant qu'on tape).
        if self.search.text().strip():
            self._run_search()

    # --- écrans ---------------------------------------------------------
    def _show_empty(self) -> None:
        """Aucun partage trouvé : l'arc tourne tant que c'est vide."""
        self.scroll.hide()
        self.results.hide()
        self.empty.show()
        self.spinner.start()

    def _show_list(self) -> None:
        self.empty.hide()
        self.spinner.stop()
        self.results.hide()
        self.scroll.show()

    # --- recherche globale ----------------------------------------------
    def _search_changed(self, text: str) -> None:
        if not text.strip():
            self._search_timer.stop()
            self._token += 1  # invalide une réponse encore en vol
            self.results.setRowCount(0)
            if self.targets:
                self._show_list()
            else:
                self._show_empty()
            self.set_peers_targets()
            return
        self._search_timer.start()

    def set_peers_targets(self) -> None:
        """Rétablit l'en-tête de la vue « liste » après une recherche."""
        if not self.targets:
            return
        postes = len({peer.peer_id for peer, _ in self.targets})
        self.hint.setText(
            f"{len(self.targets)} partage{'s' if len(self.targets) > 1 else ''} sur "
            f"{postes} poste{'s' if postes > 1 else ''}"
        )

    def _run_search(self) -> None:
        query = self.search.text().strip()
        if not query:
            return
        if not self.targets:
            self._show_empty()
            return
        self.empty.hide()
        self.spinner.stop()
        self.scroll.hide()
        self.results.show()
        self._token += 1
        postes = len({peer.peer_id for peer, _ in self.targets})
        self.hint.setText(
            f"Recherche de « {query} » dans {len(self.targets)} partage(s) "
            f"de {postes} poste(s)…"
        )
        loader = GlobalSearchLoader(self._token, self.targets, query, self)
        loader.done.connect(self._search_done)
        self._loader = loader
        loader.start()

    def _search_done(self, token: int, rows: list, truncated: bool) -> None:
        if token != self._token:
            return  # une frappe plus récente a pris la main
        self.results.setRowCount(0)
        for row in rows:
            ligne = self.results.rowCount()
            self.results.insertRow(ligne)
            nom = QTableWidgetItem(row.get("name", ""))
            nom.setIcon(
                self.style().standardIcon(
                    QStyle.SP_DirIcon if row.get("type") == "dir" else QStyle.SP_FileIcon
                )
            )
            nom.setData(Qt.UserRole, row)
            chemin = row.get("path", "")
            dossier = chemin.rsplit("/", 1)[0] if "/" in chemin else ""
            self.results.setItem(ligne, 0, nom)
            self.results.setItem(ligne, 1, QTableWidgetItem(dossier))
            self.results.setItem(
                ligne, 2, QTableWidgetItem(str(row["share"].get("name") or row["share"].get("id", "")))
            )
            self.results.setItem(ligne, 3, QTableWidgetItem(row["peer"].name))
            self.results.setItem(ligne, 4, QTableWidgetItem(self._size_text(row)))
        self._request_sizes()
        self.columns.fit()
        partages = len({(row["peer"].peer_id, row["share"].get("id")) for row in rows})
        suffixe = " (résultats partiels : affinez la recherche)" if truncated else ""
        if not rows:
            self.hint.setText(f"Aucun résultat pour « {self.search.text().strip()} »{suffixe}.")
        else:
            self.hint.setText(
                f"{len(rows)} résultat{'s' if len(rows) > 1 else ''} dans "
                f"{partages} partage{'s' if partages > 1 else ''}{suffixe}."
            )

    def _size_key(self, row: dict) -> tuple[str, str, str]:
        return (
            row["peer"].peer_id,
            str(row["share"].get("id", "")),
            row.get("path", ""),
        )

    def _size_text(self, row: dict, ligne: int | None = None) -> str:
        if row.get("type") != "dir":
            return human_size(row.get("size", 0))
        known = self._sizes.get(self._size_key(row))
        if known is None:
            return "…"  # la taille arrive, elle est calculée à la demande
        texte = human_size(known.get("bytes", 0))
        if not known.get("complete", True):
            texte += "+"  # minorant : calcul borné dans le temps
        return texte

    def _request_sizes(self) -> None:
        """Demande les tailles des dossiers visibles, une fois par resultat."""
        jobs = []
        for ligne in range(self.results.rowCount()):
            item = self.results.item(ligne, 0)
            row = item.data(Qt.UserRole) if item else None
            if not row or row.get("type") != "dir":
                continue
            if self._size_key(row) in self._sizes:
                continue
            base = row["peer"].base_url()
            if not base:
                continue
            jobs.append((row["peer"].peer_id, base, str(row["share"].get("id", "")), row.get("path", "")))
        if not jobs:
            return
        loader = ResultsSizeLoader(self._token, jobs, self)
        loader.ready.connect(self._size_ready)
        self._size_loader = loader
        loader.start()

    def _size_ready(
        self, token: int, peer_id: str, share_id: str, path: str, payload: dict
    ) -> None:
        if token != self._token:
            return  # une recherche plus récente a pris la main
        self._sizes[(peer_id, share_id, path)] = payload
        for ligne in range(self.results.rowCount()):
            item = self.results.item(ligne, 0)
            row = item.data(Qt.UserRole) if item else None
            if not row or row.get("type") != "dir":
                continue
            if self._size_key(row) == (peer_id, share_id, path):
                self.results.setItem(ligne, 4, QTableWidgetItem(self._size_text(row)))

    def _selected_results(self) -> list[dict]:
        rows = sorted({index.row() for index in self.results.selectedIndexes()})
        entries = []
        for row in rows:
            item = self.results.item(row, 0)
            if item is not None and item.data(Qt.UserRole):
                entries.append(item.data(Qt.UserRole))
        return entries

    def _results_menu(self, position) -> None:
        """Un résultat de recherche se télécharge comme une ligne de dossier."""
        row = self.results.rowAt(position.y())
        if row >= 0:
            item = self.results.item(row, 0)
            if item is not None and not item.isSelected():
                self.results.clearSelection()
                self.results.selectRow(row)
        selection = self._selected_results()
        menu = QMenu(self)
        tout = menu.addAction("Tout sélectionner")
        menu.addSeparator()
        ouvrir = menu.addAction("Ouvrir le dossier")
        menu.addSeparator()
        telecharger = menu.addAction("Télécharger")
        enregistrer = menu.addAction("Enregistrer sous…")
        for action in (ouvrir, telecharger, enregistrer):
            action.setEnabled(bool(selection))
        chosen = menu.exec(self.results.viewport().mapToGlobal(position))
        if chosen is None:
            return
        if chosen is tout:
            self.results.selectAll()
        elif chosen is ouvrir:
            item = self.results.item(self.results.currentRow(), 0)
            if item is not None:
                self._result_activated(item)
        elif chosen is telecharger:
            self._download_results(choose=False)
        elif chosen is enregistrer:
            self._download_results(choose=True)

    def _download_results(self, choose: bool) -> None:
        for row in self._selected_results():
            self.download_requested.emit(
                row["peer"],
                row["share"],
                row.get("path", ""),
                row.get("type") == "dir",
                choose,
            )

    def _result_activated(self, item: QTableWidgetItem) -> None:
        row = item.data(Qt.UserRole) or {}
        if not row:
            return
        chemin = row.get("path", "")
        if row.get("type") != "dir" and "/" in chemin:
            chemin = chemin.rsplit("/", 1)[0]  # on ouvre le dossier qui le contient
        elif row.get("type") != "dir":
            chemin = ""
        self.open_result.emit(row["peer"], row["share"], chemin)


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
        # Tri par défaut : le plus récemment modifié en tête. C'est ce qu'on
        # cherche neuf fois sur dix (la photo, le document qu'on vient de
        # déposer), et le nom reste à un clic sur l'en-tête.
        self._sort = "mtime"
        self._desc = True
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
        header.setSpacing(8)
        # Deux gestes distincts, deux boutons : revenir d'où l'on vient, et
        # remonter d'un dossier. Un seul ← pour les deux laissait croire qu'on
        # ne pouvait pas sortir d'un sous-dossier (vécu 15/11/2026).
        self.back = QPushButton("←")
        self.back.setObjectName("chip")
        self.back.setToolTip("Revenir à la page d'où l'on vient")
        self.back.setFixedWidth(38)
        self.back.clicked.connect(self.closed.emit)
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

        search_row = QHBoxLayout()
        self.search = QLineEdit()
        self.search.setPlaceholderText("Rechercher dans ce partage (nom de fichier ou dossier)…")
        self.search.setClearButtonEnabled(True)
        self.search.textChanged.connect(self._search_changed)
        search_row.addWidget(self.search, 1)
        layout.addLayout(search_row)

        # « Dossier parent » vit avec le chemin, juste au-dessus de la liste :
        # c'est là qu'on regarde où l'on est, donc là qu'on veut remonter.
        chemin = QHBoxLayout()
        chemin.setSpacing(8)
        self.up = QPushButton("↑ Dossier parent")
        self.up.setObjectName("chip")
        self.up.setToolTip("Remonter d'un dossier")
        self.up.clicked.connect(self._go_parent)
        chemin.addWidget(self.up)
        self.breadcrumb = label("/", "hint")
        chemin.addWidget(self.breadcrumb, 1)
        layout.addLayout(chemin)

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
        # Colonnes réglables à la main : un nom long ou une date qu'on veut
        # entière ne doivent pas dépendre d'une largeur décidée par Qt.
        header_view.setSectionResizeMode(QHeaderView.Interactive)
        header_view.setStretchLastSection(False)
        for colonne, largeur in enumerate((300, 150, 90, 130)):
            self.table.setColumnWidth(colonne, largeur)
        header_view.setSortIndicatorShown(True)
        header_view.setSortIndicator(3, Qt.DescendingOrder)  # colonne « Modifié »
        header_view.sectionClicked.connect(self._header_clicked)
        self.table.setColumnHidden(1, True)
        self.table.itemDoubleClicked.connect(self._activated)
        self.table.itemSelectionChanged.connect(self._selection_changed)
        self.table.setContextMenuPolicy(Qt.CustomContextMenu)
        self.table.customContextMenuRequested.connect(self._context_menu)
        self.table.verticalScrollBar().valueChanged.connect(self._maybe_more)
        layout.addWidget(self.table, 1)
        self.columns = ColumnFitter(self.table, self)

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
    def open(self, peer, share: dict | None = None, path: str = "") -> None:
        self.peer = peer
        self.title.setText(peer.name)
        keep = share.get("id") if share else (self.share.get("id") if self.share else None)
        self._populate(keep)
        if self.share is None:
            return
        self.search.clear()
        self.load(path)

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
        # Réaction immédiate : à la racine il n'y a pas de parent à remonter.
        self.up.setEnabled(bool(path))
        self.table.setColumnHidden(1, True)
        self.columns.fit()
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
        # Au premier niveau d'un partage, il n'y a pas de parent : le bouton
        # le dit au lieu de ne rien faire.
        self.up.setEnabled(bool(self.path) and not self._searching)

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
        self.columns.fit()

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
        self.columns.fit()
        suffix = " (résultats partiels)" if payload.get("truncated") else ""
        self.selection.setText(
            f"Recherche « {payload.get('query', '')} » : {len(entries)} résultat(s){suffix}."
        )
        self.breadcrumb.setText(
            f"/{self.path + '  ' if self.path else ''}recherche dans tout le partage"
        )
        self.up.setEnabled(False)  # une recherche n'a pas de dossier parent

    # --- navigation -----------------------------------------------------
    def _go_parent(self) -> None:
        """Remonte d'un dossier — et sort d'une recherche, qui n'a pas de parent."""
        if self._searching:
            self.search.clear()
            self.load(self.path)
            return
        if not self.path:
            return
        parent = self.path.rsplit("/", 1)[0] if "/" in self.path else ""
        self.load(parent)

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

    def _context_menu(self, position) -> None:
        # Le menu s'ouvre même sans sélection : « Tout sélectionner » n'aurait
        # aucun sens s'il fallait déjà avoir sélectionné quelque chose.
        menu = QMenu(self)
        select_all = menu.addAction("Tout sélectionner")
        menu.addSeparator()
        download = menu.addAction("Télécharger")
        save_as = menu.addAction("Enregistrer sous…")
        selection = bool(self.table.selectedItems())
        download.setEnabled(selection)
        save_as.setEnabled(selection)
        chosen = menu.exec(self.table.viewport().mapToGlobal(position))
        if chosen is select_all:
            self.table.selectAll()
        elif chosen is download:
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
        self.pause.setObjectName("chip")
        self.pause.setToolTip("Suspendre ; la progression est gardée sur disque")
        top.addWidget(self.pause)
        self.retry = QPushButton("Relancer")
        self.retry.setObjectName("chip")
        top.addWidget(self.retry)
        self.open = QPushButton("Ouvrir")
        self.open.setObjectName("chip")
        self.open.clicked.connect(self._open_folder)
        top.addWidget(self.open)
        self.remove = QPushButton("Retirer")
        self.remove.setObjectName("chip")
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
    """La file, sous la navigation : défilante et repliable.

    Une file de vingt transferts prenait toute la hauteur de la fenêtre et
    repoussait la navigation dans le décor. Trois lignes au plus, le reste au
    défilement, et le chevron replie la liste en gardant l'en-tête (compte,
    débit, temps restant) : c'est lui qui informe, pas les lignes.
    """

    MAX_LIGNES = 3

    def __init__(self, downloads: Downloads, parent=None):
        super().__init__(parent)
        self.downloads = downloads
        self.setObjectName("panel")
        self._rows: dict[int, TransferRow] = {}
        self._collapsed = False
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 10, 16, 12)
        layout.setSpacing(6)
        head = QHBoxLayout()
        head.setSpacing(8)
        self.toggle = QPushButton("▾")
        self.toggle.setObjectName("chevron")
        self.toggle.setFixedWidth(26)
        self.toggle.setCursor(Qt.PointingHandCursor)
        self.toggle.clicked.connect(lambda: self.set_collapsed(not self._collapsed))
        head.addWidget(self.toggle)
        self.header = label("Transferts", "section")
        head.addWidget(self.header, 1)
        self.clear = QPushButton("Nettoyer les terminés")
        self.clear.setObjectName("chip")
        self.clear.setToolTip(
            "Retirer de la liste les transferts réussis\n(les fichiers téléchargés restent)"
        )
        self.clear.clicked.connect(self.downloads.clear_finished)
        head.addWidget(self.clear)
        layout.addLayout(head)

        self.scroll = QScrollArea(self)
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.scroll.setVerticalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        conteneur = QWidget()
        self.body = QVBoxLayout(conteneur)
        self.body.setContentsMargins(0, 0, 0, 0)
        self.body.setSpacing(0)
        self.scroll.setWidget(conteneur)
        layout.addWidget(self.scroll)
        self.set_collapsed(False)
        self.hide()

    def set_collapsed(self, collapsed: bool) -> None:
        self._collapsed = collapsed
        self.scroll.setVisible(not collapsed)
        self.toggle.setText("▸" if collapsed else "▾")
        self.toggle.setToolTip(
            "Déplier la liste des transferts"
            if collapsed
            else "Replier la liste des transferts (l'en-tête garde le débit et le temps restant)"
        )
        self.updateGeometry()  # la hauteur rendue à la navigation, tout de suite

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
        # Le bouton n'apparaît que s'il y a quelque chose à nettoyer.
        self.clear.setVisible(any(job.state == "terminé" for job in jobs))
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
        self._fit_height()

    def _fit_height(self) -> None:
        """Trois lignes entières au plus : au-delà, on défile.

        On additionne la hauteur réelle des lignes affichées plutôt que de
        multiplier une hauteur moyenne : une ligne « en cours » porte une barre
        de progression, une ligne « terminé » non, et la moyenne coupait le bas
        de la troisième.
        """
        hauteurs = []
        for rang in range(min(self.body.count(), self.MAX_LIGNES)):
            widget = self.body.itemAt(rang).widget()
            if widget is not None:
                hauteurs.append(max(widget.sizeHint().height(), 44))
        # +8 : la feuille de style ajoute des marges que sizeHint() ignore, et
        # sans cette marge la troisième ligne était coupée en bas (vécu 15/11/2026).
        self.scroll.setFixedHeight(max(sum(hauteurs), 1) + 8)


# ---------------------------------------------------------------------------
# Fenêtre
# ---------------------------------------------------------------------------
class MainWindow(QMainWindow):
    def __init__(self, autostart: bool = True):
        super().__init__()
        self.setWindowTitle(APP_NAME)
        self.resize(1120, 720)
        self.document = config.load()
        self._remote_peers: list = []
        # Message quand ce poste est déjà partagé par un autre compte (le port
        # est fixe : un seul démon par machine, tous comptes confondus).
        self._blocked = ""
        self._blocked_warned = False
        self.daemon = Daemon()
        self.discovery = Discovery(self)
        self.downloads = Downloads(self)

        central = QWidget()
        root = QVBoxLayout(central)
        root.setContentsMargins(16, 16, 16, 16)
        root.setSpacing(12)

        # « Mes partages » n'est plus une colonne fixe : c'est un tiroir qui
        # flotte au-dessus de la navigation (voir `_open_drawer`), donc la pile
        # reçoit toute la largeur.
        self.shares = SharePanel(self.document, central)

        # La barre de l'application ne porte qu'une chose : le tiroir
        # « Mes partages ». Il est au niveau de la fenêtre, pas d'une page —
        # à côté du titre « Partages trouvés », il se lisait comme une action
        # de cette page (vécu 15/11/2026).
        barre = QHBoxLayout()
        barre.setSpacing(8)
        self.shares_button = QPushButton("☰ Mes partages")
        self.shares_button.setObjectName("chip")
        self.shares_button.setToolTip("Ce que ce poste partage")
        self.shares_button.clicked.connect(self._toggle_drawer)
        barre.addWidget(self.shares_button)
        barre.addStretch(1)
        root.addLayout(barre)

        # Deux pages : les partages trouvés, et la navigation chez l'un d'eux.
        self.stack = QStackedWidget()
        self.peers = PeerPage()
        self.browser = BrowserPage()
        self.stack.addWidget(self.peers)
        self.stack.addWidget(self.browser)
        root.addWidget(self.stack, 1)

        self.show_shares()

        self.transfers = TransferPanel(self.downloads)
        root.addWidget(self.transfers)
        self.setCentralWidget(central)

        # Le tiroir et son voile sont des enfants du panneau central, hors
        # disposition : ils se superposent à la navigation, à géométrie fixée.
        self.scrim = Scrim(central)
        self.scrim.clicked.connect(self._close_drawer)
        self.scrim.hide()
        self.shares.hide()
        self._slide: QPropertyAnimation | None = None
        QShortcut(QKeySequence("Escape"), self, activated=self._close_drawer)

        # Canal de l'icône de notification : « montre-toi », « quitte », « change
        # le démarrage ». Les ordres sont appliqués **ici** : la fenêtre est
        # seule à savoir arrêter son démon éphémère avant de laisser la place
        # au service permanent.
        RUNTIME_DIR.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.control = QLocalServer(self)
        self.control.setSocketOptions(QLocalServer.SocketOption.UserAccessOption)
        if not self.control.listen(str(CONTROL_SOCKET)):
            QLocalServer.removeServer(str(CONTROL_SOCKET))  # socket d'une session morte
            self.control.listen(str(CONTROL_SOCKET))
        self.control.newConnection.connect(self._control_connection)

        # Un disque externe branché/débranché ne touche pas share.json : on
        # revérifie la présence des dossiers (un `stat` chacun) toutes les 2 s,
        # et seulement fenêtre visible. C'est le démon qui décide de servir et
        # d'annoncer ; ici on ne fait que dire la vérité dans la liste.
        self.availability = QTimer(self)
        self.availability.setInterval(2000)
        self.availability.timeout.connect(self._check_availability)

        # Aucun partage trouvé : on rescane toutes les 5 s au lieu des 30 s de
        # la boucle de découverte. C'est le premier lancement (ou le réseau qui
        # arrive juste) : attendre une demi-minute pour voir apparaître le
        # premier poste donnait l'impression que rien ne marchait.
        self.scan = QTimer(self)
        self.scan.setInterval(5000)
        self.scan.timeout.connect(self._scan_quick)

        # câblage
        self.shares.changed.connect(self._shares_changed)
        self.shares.mode_changed.connect(self._mode_changed)
        self.shares.visibility_changed.connect(self._visibility_changed)
        self.peers.open_share.connect(self._open_share)

        self.peers.refresh_requested.connect(self.discovery.refresh_now)
        self.peers.open_result.connect(self._open_result)
        self.peers.download_requested.connect(self._download)
        self.browser.closed.connect(self._browser_closed)
        self.browser.download_requested.connect(self._download)
        self.discovery.peers_changed.connect(self._peers_changed)
        self.downloads.changed.connect(self._downloads_changed)
        self.downloads.finished.connect(self._download_finished)

        # Première image de la liste (mes partages y figurent déjà) : sans cela
        # l'accueil restait vide jusqu'au premier événement de découverte.
        self._refresh_share_lists()

        if autostart:
            # La découverte ne dépend pas de notre démon : on voit les partages
            # des autres même sans rien partager soi-même.
            self.discovery.start()
            self._start_daemon()
        self._downloads_changed()
        self.availability.start()
        self.scan.start()

        # Le second compte du poste doit le savoir tout de suite : le tiroir est
        # fermé par défaut, une bannière dedans passerait inaperçue. Une fois par
        # lancement, après le premier rendu.
        if self._blocked:
            QTimer.singleShot(300, self._warn_blocked)

    # --- démon ----------------------------------------------------------
    def _start_daemon(self) -> bool:
        """Démarre (ou adopte) le démon ; dit pourquoi c'est impossible sinon.

        **Sans aucun dossier déclaré, rien ne démarre** — ni démon, ni icône de
        notification : un serveur qui ne sert rien et une icône qui n'annonce
        rien n'ont pas de raison d'être (demandé le 15/11/2026). Un démon que
        nous possédons est arrêté ; le service permanent, lui, reste réglé tel
        quel.
        """
        if not (self.document.get("shares") or []):
            self.daemon.stop()
            self._set_blocked("")
            return False
        ok, message = self.daemon.start()
        if ok:
            self._set_blocked("")
            if not self.discovery.active:
                self.discovery.start()
            return True
        if self.daemon.kind in ("other", "foreign"):
            # Ce poste est déjà pris : « partager » ici ne servirait à rien.
            self._set_blocked(message)
        else:
            self.shares.status.setText(
                f"{self.shares.status.text()}\n\nLe démon n'a pas démarré : {message}"
            )
        return False

    def _set_blocked(self, message: str) -> None:
        """État « ce poste est déjà partagé par un autre compte »."""
        if message == self._blocked:
            return
        self._blocked = message
        self.shares.set_blocked(message)
        self._update_shares_button()
        if message:
            notify(
                "Partage impossible sur ce poste",
                message,
            )
        else:
            notify("Gablue Share", "Le partage est de nouveau possible sur ce poste.")

    # --- canal de l'icône de notification ------------------------------
    def _control_connection(self) -> None:
        socket = self.control.nextPendingConnection()
        if socket is None:
            return

        def lire() -> None:
            ligne = bytes(socket.readLine()).decode("utf-8", "replace").strip()
            if ligne:
                self._control_apply(ligne)

        socket.readyRead.connect(lire)
        socket.disconnected.connect(socket.deleteLater)
        if socket.bytesAvailable():
            lire()

    def _control_apply(self, commande: str) -> None:
        """Les trois ordres de l'icône. Un ordre inconnu est ignoré."""
        if commande == "show":
            self.showNormal()
            self.raise_()
            self.activateWindow()
        elif commande == "quit":
            # La fenêtre se ferme, le serveur reste : c'est le clic sur
            # l'icône quand Gablue Share est ouvert.
            self.close()

    def _warn_blocked(self) -> None:
        """Une seule alerte modale par lancement : le reste vit dans le tiroir."""
        if not self._blocked or self._blocked_warned:
            return
        self._blocked_warned = True
        QMessageBox.warning(self, APP_NAME, self._blocked)

    # --- partages -------------------------------------------------------
    def _shares_changed(self) -> None:
        self.document = config.load()
        self.shares.document = self.document
        self.shares.reload()
        # Démarre s'il y a de quoi servir, arrête s'il n'y a plus rien.
        self._start_daemon()
        if not self.discovery.active:
            self.discovery.start()
        # La liste d'accueil contient aussi mes partages : elle suit.
        self._refresh_share_lists()

    # --- postes ---------------------------------------------------------
    def _peers_changed(self, peers) -> None:
        self._remote_peers = list(peers)
        self._refresh_share_lists()

    def _refresh_share_lists(self) -> None:
        """« Partages trouvés » ne montre que les **autres** postes.

        Son propre poste n'y figure pas : ce qu'il partage se règle dans le
        tiroir « Mes partages », et l'y voir laissait croire qu'on pouvait s'y
        connecter comme ailleurs (demandé le 15/11/2026).
        """
        peers = list(self._remote_peers)
        self.peers.set_peers(peers)
        self._update_shares_button()
        # C'est ici que la file se rattache : un poste qui revient relance ses
        # transferts en attente, sans intervention.
        self.downloads.sync_peers(peers)
        # Et si ce poste est celui qu'on parcourt, ses partages ont pu changer
        # (disque branché chez lui, partage renommé, ajouté, retiré).
        self.browser.sync_peer(peers)

    def _update_shares_button(self) -> None:
        """Le bouton dit l'état : un poste pris par un autre compte se voit là."""
        nombre = len(self.document.get("shares", []))
        texte = f"☰ Mes partages ({nombre})"
        if self._blocked:
            texte += " — partage indisponible"
        self.shares_button.setText(texte)
        self.shares_button.setToolTip(self._blocked or "Ce que ce poste partage")

    def _visibility_changed(self, value: str) -> None:
        """Le démon relit le fichier tout seul (mtime, ~3 s) : rien à redémarrer."""
        self.document = config.load()
        self.shares.document = self.document

    def _scan_quick(self) -> None:
        """Rescan rapide tant que la liste est vide.

        Dès qu'un partage est trouvé, la boucle ordinaire (30 s) suffit : un
        poste qui arrive met au pire une demi-minute, et mDNS reste à l'écoute
        en continu pour le réseau local.
        """
        if self.peers.targets or not self.discovery.active:
            return
        if not self.isVisible() or self.isMinimized():
            return  # rien ne tourne quand on ne regarde pas
        self.discovery.refresh_now()

    def _check_availability(self) -> None:
        if not self.isVisible() or self.isMinimized():
            return
        self.shares.refresh_availability()
        # Le compte qui occupait le port a peut-être fermé son application :
        # on réessaie, sans marteler (un `info` sur la boucle locale, 0,5 s).
        if self._blocked:
            self._start_daemon()
            self._refresh_share_lists()

    # --- tiroir « Mes partages » ---------------------------------------
    DRAWER_MARGIN = 12

    def _toggle_drawer(self) -> None:
        if self.shares.isVisible():
            self._close_drawer()
        else:
            self._open_drawer()

    def _open_drawer(self) -> None:
        self.shares.reload()
        self._slide_drawer(True)

    def _close_drawer(self) -> None:
        if self.shares.isVisible():
            self._slide_drawer(False)

    def _slide_drawer(self, opened: bool) -> None:
        """Ouvre ou referme le tiroir ; le voile part et revient avec lui."""
        central = self.centralWidget()
        self.scrim.setGeometry(central.rect())
        self.shares.setFixedHeight(max(central.height() - 2 * self.DRAWER_MARGIN, 240))
        if opened:
            self.shares.move(-self.shares.width(), self.DRAWER_MARGIN)
            self.scrim.show()
            self.scrim.raise_()
            self.shares.show()
            self.shares.raise_()
        cible = QPoint(
            self.DRAWER_MARGIN if opened else -self.shares.width(), self.DRAWER_MARGIN
        )
        if self._slide is not None:
            self._slide.stop()
        animation = QPropertyAnimation(self.shares, b"pos", self)
        animation.setDuration(160)
        animation.setEasingCurve(QEasingCurve.OutCubic)
        animation.setEndValue(cible)
        animation.finished.connect(lambda: self._drawer_settled(opened))
        self._slide = animation
        animation.start()

    def _drawer_settled(self, opened: bool) -> None:
        if not opened:
            self.shares.hide()
            self.scrim.hide()

    def resizeEvent(self, event) -> None:  # noqa: N802 (API Qt)
        super().resizeEvent(event)
        if self.shares.isVisible():
            central = self.centralWidget()
            self.scrim.setGeometry(central.rect())
            self.shares.setFixedHeight(
                max(central.height() - 2 * self.DRAWER_MARGIN, 240)
            )

    def show_shares(self) -> None:
        self.stack.setCurrentWidget(self.peers)

    def _browser_closed(self) -> None:
        self.stack.setCurrentWidget(self.peers)

    def _enter_browser(self) -> None:
        self.stack.setCurrentWidget(self.browser)

    def _open_share(self, peer, share) -> None:
        self.browser.open(peer, share)
        self._enter_browser()

    def _open_result(self, peer, share, path: str) -> None:
        """Résultat de la recherche globale : on ouvre le dossier qui le contient."""
        self.browser.open(peer, share, path)
        self._enter_browser()

    # --- permanence -----------------------------------------------------
    def _mode_changed(self, mode: str) -> None:
        """Non / avec la session / au démarrage du poste.

        La permanence dit **quand le serveur redémarre**, pas ce qu'il fait
        maintenant : le démon en cours n'est ni arrêté ni remplacé, il continue
        de servir jusqu'à l'extinction. Choisir « Non » n'arrête donc pas le
        partage — pour ça, il y a « Quitter le partage » depuis l'icône.
        """
        ok, message = service.set_mode(mode)
        if not ok:
            QMessageBox.warning(self, APP_NAME, message or "Opération impossible.")
        self.shares.revert_mode()
        self.shares.reload()

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
        # Ordre voulu : plus aucune découverte, le transfert en cours rend la
        # main (sinon un QThread détruit pendant qu'il tourne fait avorter le
        # processus), puis le démon de la GUI s'arrête.
        self.discovery.stop()
        self.downloads.shutdown()
        # Le démon n'est **pas** arrêté : le partage survit à la fenêtre
        # (jusqu'à l'extinction, ou « Quitter le partage » depuis l'icône).
        self.control.close()
        super().closeEvent(event)
