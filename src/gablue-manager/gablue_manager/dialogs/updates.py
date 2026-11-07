"""Mises à jour automatiques : opt-in, domaines et rythme.

L'image Gablue livre tous les timers **désactivés** : c'est l'utilisateur qui
décide ici ce qui se met à jour tout seul (image, Flatpaks, Homebrew) et à
quel rythme (quotidien ou hebdomadaire). Le helper privilégié
`/usr/libexec/gablue-auto-update` applique les timers système ; le timer
Flatpak du compte est basculé sans privilège (`systemctl --user`).
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QVBoxLayout,
)

from ..local import (
    auto_update_available, auto_update_script, auto_update_state,
    parse_gablue_progress,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

# Libellé de la case, puis description affichée dessous.
DOMAIN_LABELS = {
    "image": (
        "Image Gablue",
        "Téléchargée en arrière-plan, appliquée au redémarrage.",
    ),
    "flatpak": (
        "Flatpaks",
        "Applications Flatpak du système et de votre compte.",
    ),
    "brew": (
        "Homebrew",
        "Formules installées par brew (mises à jour, pas d'installation).",
    ),
}

FREQUENCIES = (
    ("daily", "Tous les jours"),
    ("weekly", "Chaque semaine"),
)

class AutoUpdatesDialog(QDialog):
    """Trois domaines, un rythme, un seul « Appliquer ».

    Rien n'est activé par défaut : tant que l'utilisateur ne coche rien,
    aucun timer ne tourne. Cocher un domaine active à la fois les timers
    système (helper, sudo -n) et, pour les Flatpaks, celui de la session.
    """

    ROWS = ("image", "flatpak", "brew")

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Mises à jour automatiques")
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Mises à jour automatiques", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Désactivées par défaut. Choisissez ce qui se met à jour tout "
            "seul et à quel rythme ; les mises à jour ont lieu la nuit, sans "
            "réveiller la machine.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)
        self.frames: dict[str, QFrame] = {}
        self.checks: dict[str, QCheckBox] = {}
        for key in self.ROWS:
            self._add_row(key)

        # Rythme commun aux trois domaines : un seul calendrier à retenir.
        schedule = QFrame(self)
        schedule.setObjectName("hostCard")
        schedule_layout = QHBoxLayout(schedule)
        schedule_layout.setContentsMargins(16, 12, 16, 12)
        schedule_layout.setSpacing(10)
        schedule_title = QLabel("Rythme", schedule)
        schedule_title.setObjectName("hostName")
        schedule_layout.addWidget(schedule_title)
        schedule_layout.addStretch(1)
        self.frequency_combo = QComboBox(schedule)
        self.frequency_combo.setCursor(Qt.PointingHandCursor)
        for value, label in FREQUENCIES:
            self.frequency_combo.addItem(label, value)
        self.frequency_combo.currentIndexChanged.connect(self._update_apply)
        schedule_layout.addWidget(self.frequency_combo)
        root_layout.addWidget(schedule)

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

        self.state: dict = {"ok": False}
        self.initial: dict[str, bool] = {}
        self.initial_frequency = "weekly"
        self.refresh()
        fit_dialog_height(self, 680)

    # -- construction -------------------------------------------------------
    def _add_row(self, key: str) -> None:
        label, description = DOMAIN_LABELS[key]
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        check = QCheckBox(label, frame)
        check.setCursor(Qt.PointingHandCursor)
        check.toggled.connect(self._update_apply)
        layout.addWidget(check)
        self.checks[key] = check

        detail = QLabel(description, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        layout.addStretch(1)
        self.frames[key] = frame
        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        available = auto_update_available()
        self.state = auto_update_state() if available else {"ok": False}
        choices = self.state if self.state.get("ok") else {}
        self.initial = {
            key: bool(choices.get(key)) for key in self.ROWS
        }
        self.initial_frequency = self.state.get("frequency", "weekly")
        for key, check in self.checks.items():
            check.blockSignals(True)
            check.setChecked(self.initial[key])
            check.blockSignals(False)
            check.setEnabled(available)
        index = self.frequency_combo.findData(self.initial_frequency)
        self.frequency_combo.blockSignals(True)
        self.frequency_combo.setCurrentIndex(index if index >= 0 else 1)
        self.frequency_combo.blockSignals(False)
        self.frequency_combo.setEnabled(available)
        if not available:
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
            )
        self._update_apply()

    def changed(self) -> list[str]:
        """Domaines dont la case diffère de l'état courant."""
        if not self.state.get("ok"):
            return []
        return [
            key for key in self.ROWS
            if self.checks[key].isChecked() != self.initial[key]
        ]

    def frequency_changed(self) -> bool:
        return (
            bool(self.state.get("ok"))
            and self.frequency_combo.currentData() != self.initial_frequency
        )

    def _update_apply(self) -> None:
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            return
        changed = self.changed()
        rhythm = self.frequency_changed()
        self.apply_button.setEnabled(bool(changed) or rhythm)
        if changed:
            count = len(changed)
            text = f"{count} domaine{'s' if count > 1 else ''} à appliquer"
            if rhythm:
                text += " et le rythme"
            self.hint_label.setText(text)
        elif rhythm:
            self.hint_label.setText("Nouveau rythme à appliquer")
        else:
            self.hint_label.setText("Aucun changement.")

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        keys = self.changed()
        frequency = self.frequency_combo.currentData()
        frequency_changed = self.frequency_changed()
        if not keys and not frequency_changed:
            return
        details = [
            f"• {DOMAIN_LABELS[key][0]} : "
            f"{'activé' if self.checks[key].isChecked() else 'désactivé'}"
            for key in keys
        ]
        if frequency_changed:
            details.append(f"• Rythme : {self.frequency_combo.currentText()}")
        answer = QMessageBox.question(
            self, "Mises à jour automatiques",
            "Appliquer ces réglages ?\n\n" + "\n".join(details) + "\n\n"
            "Le téléchargement de l'image ne prend effet qu'au redémarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        changes = {key: self.checks[key].isChecked() for key in keys}
        desired = {
            key: self.checks[key].isChecked() for key in self.ROWS
        }
        dialog = LocalTaskDialog(
            self, title="Mises à jour automatiques",
            subtitle="Journal des opérations.",
            argv=[
                "/usr/bin/bash", "-c",
                auto_update_script(changes, desired, frequency),
            ],
            # Le script annonce chaque domaine (`::gablue:step`) : la barre
            # avance par réglage appliqué, le journal reste derrière
            # « Terminal ».
            progress_parser=parse_gablue_progress,
            progress_text="Application des réglages…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        # Fermer avec des cases modifiées ne doit pas jeter les choix en
        # silence : l'application se fait uniquement par « Appliquer ».
        if self.changed() or self.frequency_changed():
            answer = QMessageBox.question(
                self, "Réglages non appliqués",
                "Fermer sans appliquer les réglages modifiés ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
