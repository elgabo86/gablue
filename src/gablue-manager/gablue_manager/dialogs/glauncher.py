"""Lanceur de jeux (glauncher) : dossiers scannés, plein écran, visuels.

Le launcher lit `~/.config/glauncher/config.toml` au démarrage et ne l'écrit
**jamais** : ce qu'on règle en cours d'utilisation (type de vue, tri, taille des
vignettes, favoris, temps de jeu) vit dans son cache et se change au clavier ou
à la manette. Ce dialogue ne présente donc que ce qui ne se règle **que** là :

- les dossiers où chercher les paquets `.wgp` / `.lgp` (il n'y a pas de
  sélection de dossier dans une interface plein écran pilotée à la manette) ;
- le plein écran au démarrage ;
- le téléchargement des visuels SteamGridDB et la clé API facultative.
"""

from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFileDialog, QFrame, QHBoxLayout, QLabel, QLineEdit,
    QListWidget, QListWidgetItem, QPushButton, QVBoxLayout,
)

from ..local import (
    glauncher_config_path, parse_glauncher_config, write_glauncher_config,
)
from ..uiutil import fit_dialog_height


def _short_path(path: str) -> str:
    """Chemin affiché et écrit : `~/…` sous le dossier personnel."""
    home = str(Path.home())
    if path == home:
        return "~"
    if path.startswith(home + "/"):
        return "~" + path[len(home):]
    return path


def _expanded(path: str) -> Path:
    return Path(path).expanduser()


class GlauncherDialog(QDialog):
    """Réglages du lanceur de jeux, écrits dans `config.toml`."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Lanceur de jeux")
        self.setModal(True)
        self.setMinimumWidth(660)

        self.path = glauncher_config_path()
        self.text = ""
        self.initial: dict = dict(parse_glauncher_config(""))

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel("Lanceur de jeux", self)
        heading.setObjectName("title")
        root.addWidget(heading)

        note = QLabel(
            "glauncher affiche les paquets .wgp (gwine) et .lgp (lgplaunch) en "
            "plein écran, à la manette. Le type de vue, le tri et la taille des "
            "vignettes se changent dans le lanceur lui-même ; ici, ce qui ne se "
            "règle que dans ce fichier.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root.addWidget(note)

        # -- dossiers de jeux -------------------------------------------------
        dirs_frame = QFrame(self)
        dirs_frame.setObjectName("hostCard")
        dirs_layout = QVBoxLayout(dirs_frame)
        dirs_layout.setContentsMargins(16, 12, 16, 12)
        dirs_layout.setSpacing(8)

        dirs_title = QLabel("Dossiers de jeux", dirs_frame)
        dirs_layout.addWidget(dirs_title)

        self.dirs_list = QListWidget(dirs_frame)
        self.dirs_list.setMinimumHeight(110)
        self.dirs_list.setMaximumHeight(170)
        self.dirs_list.itemSelectionChanged.connect(self._update_apply)
        dirs_layout.addWidget(self.dirs_list)

        dirs_buttons = QHBoxLayout()
        dirs_buttons.setSpacing(8)
        self.add_button = QPushButton("Ajouter un dossier…", dirs_frame)
        self.add_button.setObjectName("ghost")
        self.add_button.setCursor(Qt.PointingHandCursor)
        self.add_button.clicked.connect(self.add_directory)
        dirs_buttons.addWidget(self.add_button)
        self.remove_button = QPushButton("Retirer", dirs_frame)
        self.remove_button.setObjectName("ghost")
        self.remove_button.setCursor(Qt.PointingHandCursor)
        self.remove_button.clicked.connect(self.remove_directory)
        dirs_buttons.addWidget(self.remove_button)
        dirs_buttons.addStretch(1)
        dirs_layout.addLayout(dirs_buttons)

        dirs_detail = QLabel(
            "Ces dossiers sont parcourus à la recherche des paquets, sur "
            "3 niveaux. Un dossier absent (disque externe débranché) est ignoré "
            "au lancement et revient dès qu'il est remonté.",
            dirs_frame,
        )
        dirs_detail.setObjectName("hostDetail")
        dirs_detail.setWordWrap(True)
        dirs_layout.addWidget(dirs_detail)
        root.addWidget(dirs_frame)

        # -- démarrage --------------------------------------------------------
        start_frame = QFrame(self)
        start_frame.setObjectName("hostCard")
        start_layout = QVBoxLayout(start_frame)
        start_layout.setContentsMargins(16, 12, 16, 12)
        start_layout.setSpacing(6)

        self.fullscreen_check = QCheckBox(
            "Ouvrir en plein écran (le mode normal sur Gablue)", start_frame
        )
        self.fullscreen_check.setCursor(Qt.PointingHandCursor)
        self.fullscreen_check.toggled.connect(self._update_apply)
        start_layout.addWidget(self.fullscreen_check)
        root.addWidget(start_frame)

        # -- visuels ----------------------------------------------------------
        art_frame = QFrame(self)
        art_frame.setObjectName("hostCard")
        art_layout = QVBoxLayout(art_frame)
        art_layout.setContentsMargins(16, 12, 16, 12)
        art_layout.setSpacing(6)

        self.covers_check = QCheckBox(
            "Télécharger les visuels manquants (SteamGridDB)", art_frame
        )
        self.covers_check.setCursor(Qt.PointingHandCursor)
        self.covers_check.toggled.connect(self._update_apply)
        art_layout.addWidget(self.covers_check)

        key_row = QHBoxLayout()
        key_row.setSpacing(8)
        key_row.addWidget(QLabel("Clé API v2", art_frame))
        self.key_edit = QLineEdit(art_frame)
        self.key_edit.setPlaceholderText("facultative — API publique sans compte")
        self.key_edit.textChanged.connect(self._update_apply)
        key_row.addWidget(self.key_edit, 1)
        art_layout.addLayout(key_row)

        art_detail = QLabel(
            "Sans clé, l'API publique de SteamGridDB suffit : les visuels déjà "
            "présents ne sont jamais redemandés. « --update » télécharge tout "
            "d'un coup.",
            art_frame,
        )
        art_detail.setObjectName("hostDetail")
        art_detail.setWordWrap(True)
        art_layout.addWidget(art_detail)
        root.addWidget(art_frame)

        file_note = QLabel(
            "Réglages écrits dans "
            f"{_short_path(str(self.path))} — le launcher les relit au démarrage.",
            self,
        )
        file_note.setObjectName("hostDetail")
        file_note.setWordWrap(True)
        root.addWidget(file_note)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        buttons.addWidget(self.hint_label)
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        self.apply_button = QPushButton("Appliquer", self)
        self.apply_button.setObjectName("accent")
        self.apply_button.setCursor(Qt.PointingHandCursor)
        self.apply_button.clicked.connect(self.apply_changes)
        buttons.addWidget(self.apply_button)
        root.addLayout(buttons)

        self.reload()
        fit_dialog_height(self, 660)

    # -- état ---------------------------------------------------------------
    def reload(self) -> None:
        try:
            self.text = self.path.read_text(encoding="utf-8")
        except OSError:
            self.text = ""
        state = parse_glauncher_config(self.text)
        self.initial = {
            "game_dirs": list(state["game_dirs"]),
            "fullscreen": bool(state["fullscreen"]),
            "covers": bool(state["covers"]),
            "steamgriddb_key": str(state["steamgriddb_key"]),
        }

        self.dirs_list.clear()
        for directory in self.initial["game_dirs"]:
            self._add_item(directory)

        widgets = (self.fullscreen_check, self.covers_check, self.key_edit)
        for widget in widgets:
            widget.blockSignals(True)
        self.fullscreen_check.setChecked(self.initial["fullscreen"])
        self.covers_check.setChecked(self.initial["covers"])
        self.key_edit.setText(self.initial["steamgriddb_key"])
        for widget in widgets:
            widget.blockSignals(False)
        self._update_apply()

    def _add_item(self, directory: str) -> None:
        exists = _expanded(directory).is_dir()
        label = directory if exists else f"{directory}   (introuvable)"
        item = QListWidgetItem(label)
        item.setData(Qt.UserRole, directory)
        if not exists:
            item.setForeground(Qt.gray)
        self.dirs_list.addItem(item)

    def directories(self) -> list[str]:
        return [
            self.dirs_list.item(row).data(Qt.UserRole)
            for row in range(self.dirs_list.count())
        ]

    def desired(self) -> dict:
        return {
            "game_dirs": self.directories(),
            "fullscreen": self.fullscreen_check.isChecked(),
            "covers": self.covers_check.isChecked(),
            "steamgriddb_key": self.key_edit.text().strip(),
        }

    def changed(self) -> bool:
        return self.desired() != self.initial

    def _update_apply(self) -> None:
        dirty = self.changed()
        empty = not self.directories()
        self.apply_button.setEnabled(dirty and not empty)
        self.remove_button.setEnabled(bool(self.dirs_list.selectedItems()))
        if empty:
            self.hint_label.setText("Au moins un dossier est nécessaire.")
        elif dirty:
            self.hint_label.setText("Réglages à appliquer")
        else:
            self.hint_label.setText("Aucun changement.")

    # -- actions ------------------------------------------------------------
    def add_directory(self) -> None:
        chosen = QFileDialog.getExistingDirectory(
            self, "Dossier de jeux", str(Path.home())
        )
        if not chosen:
            return
        directory = _short_path(chosen)
        if directory in self.directories():
            self.hint_label.setText("Ce dossier est déjà dans la liste.")
            return
        self._add_item(directory)
        self._update_apply()

    def remove_directory(self) -> None:
        row = self.dirs_list.currentRow()
        if row < 0:
            self.hint_label.setText("Sélectionne un dossier à retirer.")
            return
        self.dirs_list.takeItem(row)
        self._update_apply()

    def apply_changes(self) -> None:
        values = self.desired()
        if not values["game_dirs"]:
            return
        text = write_glauncher_config(self.text, values)
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.path.write_text(text, encoding="utf-8")
        except OSError as error:
            self.hint_label.setText(f"Écriture impossible : {error}")
            return
        self.text = text
        self.initial = values
        self._update_apply()
        self.hint_label.setText(
            "Enregistré — effectif au prochain lancement du lanceur."
        )
