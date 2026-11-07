"""Limite de température Tctl des CPU Ryzen (`ryzenadj -f`).

`ryzenadj -f <n>` fixe la limite Tctl (°C) ; le réglage est perdu au reboot.
L'outil écrit donc la valeur dans `/etc/gablue/ryzenadj.conf` et active
`gablue-ryzenadj.service` (oneshot), qui réapplique la limite à chaque
démarrage, avant l'ouverture de session. Tout passe par le helper
`/usr/libexec/gablue-ryzenadj` en `sudo -n` (règles exactes, aucune invite).

Le dialogue n'est proposé que sur un poste où `ryzenadj` et un CPU Ryzen sont
présents ; la limite est locale, un poste distant n'en a pas.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QSpinBox, QVBoxLayout,
)

from ..local import (
    RYZENADJ_MAX_TEMP, RYZENADJ_MIN_TEMP, parse_gablue_progress,
    ryzenadj_helper_available, ryzenadj_script, ryzenadj_status,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

# Presets courants : 85 °C garde de la marge sous le throttle des X3D,
# 95 °C privilégie les performances. « Personnalisé » ouvre les bornes 45…110.
PRESETS = (
    (85, "85 °C (silencieux)"),
    (90, "90 °C"),
    (95, "95 °C (performances)"),
)


def state_text(state: dict) -> str:
    """Phrase d'état lisible ; l'illisible est dit, jamais invité à réessayer."""
    if not state:
        return "État illisible — helper absent ou privilèges manquants."
    limit = state.get("limit")
    if limit is None:
        return (
            "Actuelle : aucune limite — le firmware du CPU décide seul de la "
            "température maximale."
        )
    return (
        f"Actuelle : {limit} °C (Tctl), réappliquée à chaque démarrage par "
        "gablue-ryzenadj.service."
    )


class RyzenDialog(QDialog):
    """Case « limiter » + seuil, un seul « Appliquer »."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Limite de température CPU")
        self.setModal(True)
        self.setMinimumWidth(700)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Limite de température CPU", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Plafond Tctl appliqué au CPU Ryzen par ryzenadj -f. Réglage "
            "persistant : la limite est réappliquée à chaque démarrage, avant "
            "toute session. Ce réglage ne change pas le TDP, qui reste celui "
            "de limitw.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.state_label = QLabel(self)
        self.state_label.setObjectName("hostStatus")
        self.state_label.setWordWrap(True)
        root_layout.addWidget(self.state_label)

        card = QFrame(self)
        card.setObjectName("hostCard")
        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(16, 12, 16, 12)
        card_layout.setSpacing(6)

        self.enable = QCheckBox("Limiter la température Tctl", card)
        self.enable.setCursor(Qt.PointingHandCursor)
        card_layout.addWidget(self.enable)

        detail = QLabel(
            "Le CPU ralentit avant d'atteindre ce seuil, au lieu de laisser "
            "le firmware décider.",
            card,
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        card_layout.addWidget(detail)

        controls = QHBoxLayout()
        controls.setSpacing(8)
        self.combo = QComboBox(card)
        for value, label in PRESETS:
            self.combo.addItem(label, value)
        self.combo.addItem("Personnalisé", None)
        self.spin = QSpinBox(card)
        self.spin.setRange(RYZENADJ_MIN_TEMP, RYZENADJ_MAX_TEMP)
        self.spin.setSuffix(" °C Tctl")
        controls.addWidget(self.combo)
        controls.addWidget(self.spin)
        controls.addStretch(1)
        card_layout.addLayout(controls)
        root_layout.addWidget(card, 1)

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

        self.state: dict = {}
        self.initial: int | None = None

        self.enable.toggled.connect(self._enable_changed)
        self.combo.currentIndexChanged.connect(self._preset_chosen)
        self.spin.valueChanged.connect(self._spin_changed)

        self.refresh()
        fit_dialog_height(self, 700)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        self.state = ryzenadj_status() if ryzenadj_helper_available() else {}
        self.initial = self.state.get("limit")
        self.state_label.setText(state_text(self.state))

        if self.initial is None:
            value = PRESETS[0][0]
        else:
            value = self.initial
        self.spin.blockSignals(True)
        self.spin.setValue(value)
        self.spin.blockSignals(False)
        self._select_matching(value)

        ok = bool(self.state.get("supported"))
        self.enable.blockSignals(True)
        self.enable.setChecked(self.initial is not None)
        self.enable.blockSignals(False)
        self.enable.setEnabled(ok)
        self.combo.setEnabled(ok and self.enable.isChecked())
        self.spin.setEnabled(ok and self.enable.isChecked())

        if not ryzenadj_helper_available():
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
            )
        elif not self.state:
            self.hint_label.setText(
                "État illisible : privilèges insuffisants pour lire la limite."
            )
        elif not self.state.get("supported"):
            self.hint_label.setText(
                "ryzenadj ne répond pas sur ce CPU — limite non réglable."
            )
        self._update_apply()

    def _select_matching(self, value: int) -> None:
        for index in range(self.combo.count()):
            if self.combo.itemData(index) == value:
                self.combo.setCurrentIndex(index)
                return
        self.combo.setCurrentIndex(self.combo.count() - 1)   # « Personnalisé »

    def _enable_changed(self, checked: bool) -> None:
        ok = bool(self.state.get("supported"))
        self.combo.setEnabled(ok and checked)
        self.spin.setEnabled(ok and checked)
        self._update_apply()

    def _preset_chosen(self, _index: int) -> None:
        value = self.combo.currentData()
        if value is None:
            return
        self.spin.blockSignals(True)
        self.spin.setValue(int(value))
        self.spin.blockSignals(False)
        self._update_apply()

    def _spin_changed(self, value: int) -> None:
        self.combo.blockSignals(True)
        self._select_matching(value)
        self.combo.blockSignals(False)
        self._update_apply()

    # -- sélection ----------------------------------------------------------
    def selection(self) -> int | None:
        if not self.enable.isChecked():
            return None
        return self.spin.value()

    def changed(self) -> dict:
        """`{}` sans changement, sinon `{"limit": n | None}`."""
        if not self.state.get("supported"):
            return {}
        selected = self.selection()
        if selected == self.initial:
            return {}
        return {"limit": selected}

    def _update_apply(self) -> None:
        changes = self.changed()
        self.apply_button.setEnabled(bool(changes))
        if not self.state.get("supported"):
            return
        if changes:
            limit = changes["limit"]
            self.hint_label.setText(
                f"Limiter à {limit} °C" if limit is not None
                else "Retirer la limite"
            )
        else:
            self.hint_label.setText("Aucun changement.")

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        changes = self.changed()
        if not changes:
            return
        limit = changes["limit"]
        if limit is None:
            effect = (
                "La limite est retirée : le firmware reprend la main au "
                "prochain démarrage."
            )
        else:
            effect = (
                f"La limite de {limit} °C est appliquée tout de suite et "
                "réappliquée à chaque démarrage."
            )
        answer = QMessageBox.question(
            self, "Limite de température CPU",
            f"Appliquer ce réglage ?\n\n{effect}",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title="Limite de température CPU",
            subtitle="Limite Tctl via ryzenadj.",
            argv=["/usr/bin/bash", "-c", ryzenadj_script(limit)],
            progress_parser=parse_gablue_progress,
            progress_text="Application de la limite…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        if self.changed():
            answer = QMessageBox.question(
                self, "Réglage non appliqué",
                "Fermer sans appliquer le réglage modifié ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
