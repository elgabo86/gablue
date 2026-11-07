"""Démarrer une seule fois sur un autre système (BootNext).

Le dialogue liste les disques amorçables **autres que celui du système en
cours** : entrées EFI déjà connues du firmware, et disques (souvent USB) dont
l'ESP contient un chargeur mais que le firmware n'a jamais enregistrés. Le
système principal n'apparaît jamais, même sous un autre libellé.

Le démarrage est **ponctuel** : le helper `/usr/libexec/gablue-bootnext` arme
`BootNext` (création d'une entrée NVRAM temporaire au besoin) puis redémarre.
Le firmware consomme `BootNext` une fois ; au démarrage suivant, l'ordre
permanent reprend et Gablue revient.

Lecture et action passent par `sudo -n` (règles sudoers exactes, jamais
d'invite de mot de passe) ; UEFI uniquement.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QListWidget, QListWidgetItem,
    QMessageBox, QPushButton, QVBoxLayout,
)

from ..local import boot_options, boot_select_script, parse_gablue_progress
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height, restyle_label

STATE_TEXT = {
    "absent": "Helper absent de cette image — reconstruction requise.",
    "unsupported": "Ce poste ne démarre pas en mode UEFI : le choix d'un "
                   "autre système n'est pas disponible.",
    "error": "Lecture des disques amorçables impossible "
             "(privilèges ou NVRAM).",
    "empty": "Aucun autre disque amorçable détecté.",
}


class BootDialog(QDialog):
    """Une liste de systèmes amorçables, un seul démarrage, une confirmation."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Démarrer sur un autre système")
        self.setModal(True)
        self.setMinimumWidth(720)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Démarrer sur un autre système", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Le poste redémarre sur le système choisi, une seule fois : au "
            "démarrage suivant, Gablue revient. Le système principal n'est "
            "pas proposé.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.state_label = QLabel(self)
        self.state_label.setObjectName("hostDetail")
        self.state_label.setWordWrap(True)
        root_layout.addWidget(self.state_label)

        self.list = QListWidget(self)
        self.list.setMinimumHeight(150)
        self.list.setSelectionMode(QListWidget.SingleSelection)
        self.list.currentItemChanged.connect(
            lambda *_args: self._update_button()
        )
        root_layout.addWidget(self.list, 1)

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
        self.start_button = QPushButton("Redémarrer dessus", self)
        self.start_button.setObjectName("accent")
        self.start_button.setCursor(Qt.PointingHandCursor)
        self.start_button.clicked.connect(self.start_selected)
        buttons.addWidget(self.start_button)
        root_layout.addLayout(buttons)

        self.state = ""
        self.options: list[dict] = []
        self.refresh()
        fit_dialog_height(self, 720)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        result = boot_options()
        self.state = result["state"]
        self.options = result["options"]
        self.list.clear()
        for option in self.options:
            label = option["label"]
            detail = option["detail"]
            item = QListWidgetItem(
                f"{label}  —  {detail}" if detail else label
            )
            item.setData(Qt.UserRole, option["id"])
            item.setToolTip(
                f"{label}\n{detail}" if detail else label
            )
            self.list.addItem(item)
        if self.options:
            self.list.setCurrentRow(0)

        if self.state == "ok":
            count = len(self.options)
            text = (
                f"{count} autre disque amorçable détecté."
                if count == 1
                else f"{count} autres disques amorçables détectés."
            )
            restyle_label(self.state_label, "hostDetail")
        else:
            text = STATE_TEXT.get(self.state, "État du démarrage illisible.")
            restyle_label(
                self.state_label,
                "hint" if self.state == "empty" else "warn",
            )
        self.state_label.setText(text)
        self.list.setEnabled(bool(self.options))
        self._update_button()

    def _update_button(self) -> None:
        self.start_button.setEnabled(self.list.currentItem() is not None)
        self.hint_label.setText(
            "Le poste redémarre immédiatement."
            if self.list.currentItem() is not None else ""
        )

    def chosen_id(self) -> str:
        item = self.list.currentItem()
        return str(item.data(Qt.UserRole) or "") if item is not None else ""

    # -- application --------------------------------------------------------
    def start_selected(self) -> None:
        ident = self.chosen_id()
        if not ident:
            return
        item = self.list.currentItem()
        label = item.text() if item is not None else ident
        answer = QMessageBox.question(
            self, "Démarrer sur un autre système",
            f"Redémarrer maintenant sur :\n{label} ?\n\n"
            "Un seul démarrage : au démarrage suivant, Gablue revient. Les "
            "sessions ouvertes seront interrompues.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        task = LocalTaskDialog(
            self, title="Démarrer sur un autre système",
            subtitle="Configuration du démarrage unique puis redémarrage.",
            argv=["/usr/bin/bash", "-c", boot_select_script(ident)],
            progress_parser=parse_gablue_progress,
            progress_text="Préparation du démarrage…",
            reveal_log_on_error=True,
            close_on_success=True,
        )
        start_task(self, task)
        # Dialogue modal : rester ouvert désactiverait la fenêtre de
        # progression (dont « Fermer » deviendrait inerte), et le redémarrage
        # rend de toute façon le résultat visible au prochain boot.
        self.accept()
