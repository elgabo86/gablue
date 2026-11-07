"""Variante de l'image Gablue (bascule par rebase, effet au redémarrage).
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..constants import REBASE_VARIANTS
from ..local import local_variant, rebase_helper_available, rebase_script
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

class VariantDialog(QDialog):
    """Variante de l'image : une liste et un bouton « Basculer ».

    Le rebase télécharge une autre image et ne prend effet qu'au redémarrage ;
    il n'a donc ni case à cocher ni « Appliquer » groupé — un choix, une
    confirmation explicite, une tâche.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Variante de l'image")
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Variante de l'image", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        self.note = QLabel(
            "Télécharge une autre variante de Gablue (standard, DX, NVIDIA…). "
            "Effet au redémarrage, fichiers conservés.",
            self,
        )
        self.note.setObjectName("subtitle")
        self.note.setWordWrap(True)
        root_layout.addWidget(self.note)

        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        title = QLabel("Variante de Gablue", frame)
        title.setObjectName("hostName")
        layout.addWidget(title)

        row = QHBoxLayout()
        row.setSpacing(10)
        self.variant_combo = QComboBox(frame)
        self.variant_combo.setCursor(Qt.PointingHandCursor)
        for variant, label in REBASE_VARIANTS:
            self.variant_combo.addItem(label, variant)
        self.variant_combo.currentIndexChanged.connect(self._update_rebase)
        row.addWidget(self.variant_combo, 1)

        self.current_label = QLabel(frame)
        self.current_label.setObjectName("hint")
        row.addWidget(self.current_label)

        self.rebase_button = QPushButton("Basculer", frame)
        self.rebase_button.setObjectName("ghost")
        self.rebase_button.setCursor(Qt.PointingHandCursor)
        self.rebase_button.clicked.connect(self.rebase)
        row.addWidget(self.rebase_button)
        layout.addLayout(row)
        root_layout.addWidget(frame)

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
        root_layout.addLayout(buttons)

        self.state: dict = {"ok": False, "variant": ""}
        self.refresh()
        fit_dialog_height(self, 680)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        available = rebase_helper_available()
        self.state = {"ok": available, "variant": local_variant()}
        self._update_variant()
        if not available:
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
            )

    def _update_variant(self) -> None:
        current = self.state.get("variant", "")
        index = self.variant_combo.findData(current)
        if index >= 0:
            self.variant_combo.blockSignals(True)
            self.variant_combo.setCurrentIndex(index)
            self.variant_combo.blockSignals(False)
            self.current_label.setText("variante actuelle")
        else:
            # Image non Gablue ou format inconnu : ne pas laisser croire qu'une
            # variante est sélectionnée.
            self.variant_combo.blockSignals(True)
            self.variant_combo.setCurrentIndex(-1)
            self.variant_combo.blockSignals(False)
            self.current_label.setText("variante inconnue")
        self._update_rebase()

    def _update_rebase(self) -> None:
        current = self.state.get("variant", "")
        choice = self.variant_combo.currentData()
        available = bool(self.state.get("ok"))
        self.rebase_button.setEnabled(
            available and bool(choice) and choice != current
        )

    # -- application --------------------------------------------------------
    def rebase(self) -> None:
        variant = self.variant_combo.currentData()
        current = self.state.get("variant", "")
        if not variant or variant == current:
            return
        label = self.variant_combo.currentText()
        answer = QMessageBox.question(
            self, "Changer de variante",
            f"Basculer vers « {label} » ?\n\n"
            "Téléchargement, puis redémarrage. Fichiers conservés.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title=f"Basculer vers {variant}",
            subtitle="Téléchargement, puis redémarrage.",
            argv=["/usr/bin/bash", "-c", rebase_script(variant)],
            # rpm-ostree ne publie pas de pourcentage ici : barre indéterminée,
            # journal derrière « Terminal ».
            progress_mode="busy",
            progress_text="Téléchargement de la nouvelle image…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()
