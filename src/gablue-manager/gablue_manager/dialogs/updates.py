"""Mises à jour automatiques : opt-in, domaines et fréquence par domaine.

L'image Gablue livre tous les timers **désactivés** : c'est l'utilisateur qui
décide ici ce qui se met à jour tout seul (image, Flatpaks, Homebrew) et à
quelle fréquence — chaque domaine a la sienne (quotidien ou hebdomadaire).
Le helper privilégié `/usr/libexec/gablue-auto-update` applique les timers
système ; Gablue ne met à jour que les Flatpaks `--system`, il n'y a aucun
timer du compte à basculer.
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
        "Applications Flatpak du système.",
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
    """Trois domaines, une fréquence chacun, un seul « Appliquer ».

    Rien n'est activé par défaut : tant que l'utilisateur ne coche rien,
    aucun timer ne tourne. Cocher un domaine active à la fois les timers
    système (helper, sudo -n) et, pour les Flatpaks, celui de la session.
    """

    ROWS = ("image", "flatpak", "brew")

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Mises à jour automatiques")
        self.setModal(True)
        self.setMinimumWidth(700)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Mises à jour automatiques", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Désactivées par défaut. Choisissez ce qui se met à jour tout "
            "seul et sa fréquence ; les mises à jour ont lieu la nuit, sans "
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
        self.combos: dict[str, QComboBox] = {}
        for key in self.ROWS:
            self._add_row(key)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        self.hint_label.setWordWrap(True)
        buttons.addWidget(self.hint_label, 1)
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
        self.initial_frequencies: dict[str, str] = {
            key: "weekly" for key in self.ROWS
        }
        self.refresh()
        fit_dialog_height(self, 700)

    # -- construction -------------------------------------------------------
    def _add_row(self, key: str) -> None:
        label, description = DOMAIN_LABELS[key]
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        top = QHBoxLayout()
        top.setSpacing(10)
        check = QCheckBox(label, frame)
        check.setCursor(Qt.PointingHandCursor)
        check.toggled.connect(self._update_apply)
        top.addWidget(check)
        top.addStretch(1)
        frequency_label = QLabel("Fréquence", frame)
        frequency_label.setObjectName("hostStatus")
        top.addWidget(frequency_label)
        combo = QComboBox(frame)
        combo.setCursor(Qt.PointingHandCursor)
        for value, text in FREQUENCIES:
            combo.addItem(text, value)
        combo.currentIndexChanged.connect(self._update_apply)
        top.addWidget(combo)
        layout.addLayout(top)
        self.checks[key] = check
        self.combos[key] = combo

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
        frequencies = self.state.get("frequencies") or {}
        self.initial_frequencies = {
            key: frequencies.get(key, "weekly") for key in self.ROWS
        }
        for key, check in self.checks.items():
            check.blockSignals(True)
            check.setChecked(self.initial[key])
            check.blockSignals(False)
            check.setEnabled(available)
        for key, combo in self.combos.items():
            index = combo.findData(self.initial_frequencies[key])
            combo.blockSignals(True)
            combo.setCurrentIndex(index if index >= 0 else 1)
            combo.blockSignals(False)
            combo.setEnabled(available)
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

    def frequency_changes(self) -> dict[str, str]:
        """Fréquences choisies qui diffèrent de l'état courant."""
        if not self.state.get("ok"):
            return {}
        return {
            key: self.combos[key].currentData()
            for key in self.ROWS
            if self.combos[key].currentData() != self.initial_frequencies[key]
        }

    def _update_apply(self) -> None:
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            return
        changed = self.changed()
        frequencies = self.frequency_changes()
        self.apply_button.setEnabled(bool(changed) or bool(frequencies))
        parts: list[str] = []
        if changed:
            count = len(changed)
            parts.append(
                f"{count} domaine{'s' if count > 1 else ''} "
                f"modifié{'s' if count > 1 else ''}"
            )
        if frequencies:
            count = len(frequencies)
            parts.append(
                f"{count} fréquence{'s' if count > 1 else ''} "
                f"modifiée{'s' if count > 1 else ''}"
            )
        self.hint_label.setText(
            " et ".join(parts) if parts else "Aucun changement."
        )

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        keys = self.changed()
        frequencies = self.frequency_changes()
        if not keys and not frequencies:
            return
        details = [
            f"• {DOMAIN_LABELS[key][0]} : "
            f"{'activé' if self.checks[key].isChecked() else 'désactivé'}"
            for key in keys
        ]
        for key, value in frequencies.items():
            text = next(
                label for candidate, label in FREQUENCIES
                if candidate == value
            )
            details.append(f"• {DOMAIN_LABELS[key][0]} : {text.lower()}")
        answer = QMessageBox.question(
            self, "Mises à jour automatiques",
            "Appliquer ces réglages ?\n\n" + "\n".join(details) + "\n\n"
            "Le téléchargement de l'image ne prend effet qu'au redémarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        changes = {key: self.checks[key].isChecked() for key in keys}
        dialog = LocalTaskDialog(
            self, title="Mises à jour automatiques",
            subtitle="Journal des opérations.",
            argv=[
                "/usr/bin/bash", "-c",
                auto_update_script(changes, frequencies),
            ],
            # Le script annonce chaque réglage (`::gablue:step`) : la barre
            # avance par opération, le journal reste derrière « Terminal ».
            progress_parser=parse_gablue_progress,
            progress_text="Application des réglages…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        # Fermer avec des réglages modifiés ne doit pas les jeter en silence :
        # l'application se fait uniquement par « Appliquer ».
        if self.changed() or self.frequency_changes():
            answer = QMessageBox.question(
                self, "Réglages non appliqués",
                "Fermer sans appliquer les réglages modifiés ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
