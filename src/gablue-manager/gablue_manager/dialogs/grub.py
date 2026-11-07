"""Visibilité du menu GRUB (`menu_auto_hide`), en trois choix.

Équivalent graphique de `ujust configure-grub` : toujours afficher, masquer
après un démarrage réussi sauf en double-boot, ou masquer dans tous les cas.
L'état courant est lu dans `grubenv` (illisible sans root) par le helper
`/usr/libexec/gablue-grub`, appliqué en `sudo -n` (aucune invite de mot de
passe) et prend effet au démarrage suivant.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QButtonGroup, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QRadioButton, QVBoxLayout,
)

from ..local import (
    grub_apply_script, grub_helper_available, grub_menu_state,
    parse_gablue_progress,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

# (valeur menu_auto_hide, libellé, explication)
CHOICES = (
    ("unset", "Toujours afficher le menu GRUB",
     "Le menu apparaît à chaque démarrage, sans délai caché."),
    ("1", "Masquer après un démarrage réussi, sauf en double démarrage",
     "Le menu est masqué, mais reste visible si plusieurs systèmes sont "
     "installés (ou si le dernier démarrage a échoué)."),
    ("2", "Masquer après un démarrage réussi, même en double démarrage",
     "Le menu est masqué dans tous les cas ; maintenir Échap au démarrage "
     "pour le faire apparaître."),
)

BEHAVIOUR = {"unset": "unset", "0": "unset", "1": "1", "2": "2"}


def state_text(value: str) -> str:
    """Phrase d'état lisible pour la valeur lue ("" si illisible)."""
    if value == "":
        return "État illisible — helper absent ou privilèges manquants."
    if value == "unset":
        return "Actuel : menu_auto_hide non défini — le menu s'affiche toujours."
    if value == "0":
        return "Actuel : menu_auto_hide=0 — le menu s'affiche toujours."
    if value == "1":
        return "Actuel : menu_auto_hide=1 — masqué, sauf en double démarrage."
    return "Actuel : menu_auto_hide=2 — masqué même en double démarrage."


class GrubDialog(QDialog):
    """Trois choix exclusifs, un seul « Appliquer »."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Menu GRUB")
        self.setModal(True)
        self.setMinimumWidth(700)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Menu GRUB", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Affichage du menu au démarrage. Le réglage est écrit dans "
            "grubenv et prend effet au prochain démarrage.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.state_label = QLabel(self)
        self.state_label.setObjectName("hostStatus")
        self.state_label.setWordWrap(True)
        root_layout.addWidget(self.state_label)

        self.group = QButtonGroup(self)
        self.choices: dict[str, QRadioButton] = {}
        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        for value, label, description in CHOICES:
            self._add_row(value, label, description)
        root_layout.addLayout(self.rows_layout, 1)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        buttons.addWidget(self.hint_label)
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.clicked.connect(self.reject)
        buttons.addWidget(self.close_button)
        self.apply_button = QPushButton("Appliquer", self)
        self.apply_button.setObjectName("accent")
        self.apply_button.setCursor(Qt.PointingHandCursor)
        self.apply_button.clicked.connect(self.apply_changes)
        buttons.addWidget(self.apply_button)
        root_layout.addLayout(buttons)

        self.state: dict = {"ok": False, "value": ""}
        self.initial = ""
        self.refresh()
        fit_dialog_height(self, 700)

    # -- construction -------------------------------------------------------
    def _add_row(self, value: str, label: str, description: str) -> None:
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(4)

        radio = QRadioButton(label, frame)
        radio.setCursor(Qt.PointingHandCursor)
        radio.toggled.connect(self._update_apply)
        self.group.addButton(radio)
        self.choices[value] = radio
        layout.addWidget(radio)

        detail = QLabel(description, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        available = grub_helper_available()
        value = grub_menu_state() if available else ""
        self.state = {"ok": available and value != "", "value": value}
        self.initial = BEHAVIOUR.get(value, "")
        self.state_label.setText(state_text(value if available else ""))
        if self.initial:
            self.choices[self.initial].setChecked(True)
        else:
            self.choices["unset"].setChecked(True)
        for radio in self.choices.values():
            radio.setEnabled(self.state["ok"])
        if not available:
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
            )
        elif not value:
            self.hint_label.setText(
                "État illisible : privilèges insuffisants pour lire grubenv."
            )
        self._update_apply()

    def chosen(self) -> str:
        for value, radio in self.choices.items():
            if radio.isChecked():
                return value
        return ""

    def changed(self) -> bool:
        if not self.state.get("ok"):
            return False
        return bool(self.chosen()) and self.chosen() != self.initial

    def _update_apply(self) -> None:
        self.apply_button.setEnabled(self.changed())
        if not self.state.get("ok"):
            return
        self.hint_label.setText(
            "Nouveau réglage à appliquer" if self.changed()
            else "Aucun changement."
        )

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        value = self.chosen()
        if not value or value == self.initial:
            return
        label = next(item[1] for item in CHOICES if item[0] == value)
        answer = QMessageBox.question(
            self, "Menu GRUB",
            f"Appliquer ce réglage ?\n\n{label}\n\n"
            "Effet au prochain démarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title="Menu GRUB",
            subtitle="Écriture de menu_auto_hide dans grubenv.",
            argv=["/usr/bin/bash", "-c", grub_apply_script(value)],
            progress_parser=parse_gablue_progress,
            progress_text="Application du réglage…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        if self.changed():
            answer = QMessageBox.question(
                self, "Réglage non appliqué",
                "Fermer sans appliquer le réglage ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
