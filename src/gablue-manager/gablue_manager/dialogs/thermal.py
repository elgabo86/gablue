"""Limite thermique des SSD NVMe (HCTM), réglable par contrôleur.

HCTM laisse le système fixer deux seuils de throttle : TMT1 (doux) et TMT2
(agressif). Le réglage est persistant (`nvme set-feature … -s`), donc il survit
au reboot et ne dépend pas d'un service. La lecture comme l'écriture passent
par le helper `/usr/libexec/gablue-nvme-thermal` (sudo -n sans mot de passe) :
`nvme id-ctrl` et `get-feature` exigent root.

Presets de la recette ujust (65/70, 70/75, 75/80) filtrés par le minimum
réellement supporté (`mntmt`) et le maximum (`mxtmt`) du contrôleur.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QSpinBox, QVBoxLayout,
)

from ..local import (
    NVME_PRESETS, REMOTE_NVME_THERMAL_PROBE, nvme_thermal_devices,
    nvme_thermal_helper_available, nvme_thermal_remote_script,
    nvme_thermal_script, parse_gablue_progress,
    parse_remote_nvme_thermal_state, remote_helpers_nopasswd,
)
from ..ssh import (
    ask_remote_password, remote_sudo_prefix, ssh_capture,
)
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import fit_dialog_height
from ..util import display_name

PRESET_LABELS = {
    (65, 70): "65/70 °C (conservatif)",
    (70, 75): "70/75 °C (recommandé)",
    (75, 80): "75/80 °C (permissif)",
}


def presets_for(device: dict) -> list[tuple[int, int, str]]:
    """Presets valides pour ce contrôleur (bornes `mntmt`/`mxtmt`)."""
    minimum = int(device.get("min_tmt1") or 40)
    maximum_t2 = int(device.get("max_tmt2") or 90)
    items = [
        (first, second, PRESET_LABELS[(first, second)])
        for first, second in NVME_PRESETS
        if first >= minimum and second <= maximum_t2
    ]
    if items:
        return items
    # SSD au minimum élevé (ou max bas) : proposer la combinaison la plus
    # proche du supporté, jamais un seuil sous `mntmt`.
    first = min(max(minimum, 40), 85)
    second = min(max(first + 5, 45), max(maximum_t2, 45))
    if second <= first:
        second = min(90, first + 5)
    return [(first, second, f"minimum supporté ({first}/{second} °C)")]


class ThermalDialog(QDialog):
    """Un contrôleur par carte : case + seuils, un seul « Appliquer »."""

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host) if host else None
        self.setWindowTitle(
            "Limite thermique NVMe" if self.host is None
            else f"Limite thermique NVMe — {display_name(self.host)}"
        )
        self.setModal(True)
        self.setMinimumWidth(700)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Limite thermique NVMe", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Seuils de throttle du SSD (HCTM). TMT1 ralentit doucement, TMT2 "
            "plus fort. Réglage persistant, appliqué par le contrôleur."
            + ("" if self.host is None else " Poste distant, par SSH."),
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)

        self.rows: dict[str, dict] = {}
        self.initial: dict[str, tuple] = {}

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

        self.state: dict = {"ok": False, "devices": []}
        self.refresh()
        fit_dialog_height(self, 700)

    # -- construction -------------------------------------------------------
    def _add_row(self, device: dict) -> None:
        key = device["device"]
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        check = QCheckBox(f"{device.get('model') or 'SSD NVMe'} ({key})", frame)
        check.setCursor(Qt.PointingHandCursor)
        layout.addWidget(check)

        support = bool(device.get("hctm"))
        enabled = bool(device.get("enabled"))
        if not device.get("readable", True):
            detail_text = (
                "État illisible : privilèges ou nvme-cli indisponible."
            )
        elif support:
            detail_text = (
                f"HCTM actif — TMT1 {device.get('tmt1')} °C, "
                f"TMT2 {device.get('tmt2')} °C"
                if enabled else
                "HCTM désactivé — le firmware du SSD décide seul."
            )
        else:
            detail_text = "Ce SSD ne supporte pas HCTM (limite non réglable)."
        detail = QLabel(detail_text, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        controls = QHBoxLayout()
        controls.setSpacing(8)
        combo = QComboBox(frame)
        for first, second, label in (
            presets_for(device) if support else []
        ):
            combo.addItem(label, (first, second))
        combo.addItem("Personnalisé", None)

        minimum = min(int(device.get("min_tmt1") or 40), 85)
        maximum_t2 = int(device.get("max_tmt2") or 90)
        first_spin = QSpinBox(frame)
        first_spin.setRange(minimum, 85)
        first_spin.setSuffix(" °C TMT1")
        second_spin = QSpinBox(frame)
        second_spin.setRange(45, max(maximum_t2, 45))
        second_spin.setSuffix(" °C TMT2")
        controls.addWidget(combo)
        controls.addWidget(first_spin)
        controls.addWidget(second_spin)
        controls.addStretch(1)
        layout.addLayout(controls)

        # Valeurs initiales : celles du SSD, sinon le preset recommandé
        # disponible (jamais sous le minimum supporté).
        if enabled and device.get("tmt1"):
            first, second = int(device["tmt1"]), int(device["tmt2"])
        else:
            preset = presets_for(device) if support else [(70, 75, "")]
            first, second = preset[0][0], preset[0][1]
        first = max(minimum, min(first, 85))
        second = max(45, min(second, max(maximum_t2, 45)))

        check.blockSignals(True)
        check.setChecked(enabled and support)
        check.blockSignals(False)
        check.setEnabled(support)
        if not support:
            check.setToolTip("HCTM non supporté par ce contrôleur.")
        first_spin.setValue(first)
        second_spin.setValue(second)
        self._select_matching(combo, first, second)

        for widget in (combo, first_spin, second_spin):
            widget.setEnabled(support and check.isChecked())
        check.toggled.connect(
            lambda checked, row=key: self._set_editable(row, checked)
        )
        combo.currentIndexChanged.connect(
            lambda _index, row=key: self._preset_chosen(row)
        )
        first_spin.valueChanged.connect(
            lambda _value, row=key: self._thresholds_edited(row)
        )
        second_spin.valueChanged.connect(
            lambda _value, row=key: self._thresholds_edited(row)
        )

        self.rows[key] = {
            "check": check, "combo": combo, "tmt1": first_spin,
            "tmt2": second_spin, "detail": detail, "device": device,
        }
        self.initial[key] = (
            enabled and support,
            int(device.get("tmt1") or 0),
            int(device.get("tmt2") or 0),
        )
        self.rows_layout.addWidget(frame)

    @staticmethod
    def _select_matching(combo: QComboBox, first: int, second: int) -> None:
        for index in range(combo.count()):
            if combo.itemData(index) == (first, second):
                combo.setCurrentIndex(index)
                return
        combo.setCurrentIndex(combo.count() - 1)     # « Personnalisé »

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        """État HCTM : local, ou une sonde SSH (le helper lit en root)."""
        if self.host is None:
            available = nvme_thermal_helper_available()
            return {
                "ok": available,
                "devices": nvme_thermal_devices() if available else [],
            }
        code, text = ssh_capture(
            self.host, REMOTE_NVME_THERMAL_PROBE, timeout=30
        )
        if code != 0 and not text:
            return {"ok": False, "devices": []}
        return parse_remote_nvme_thermal_state(text)

    def refresh(self) -> None:
        self.state = self.read_state()
        devices = self.state.get("devices") or []
        available = bool(self.state.get("ok"))
        for device in devices:
            self._add_row(device)
        if not available:
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
                if self.host is None else
                "Helper absent du poste distant — image à reconstruire."
            )
        elif not devices:
            self.hint_label.setText(
                "Aucun SSD NVMe lisible (privilèges ou matériel absent)."
            )
        self._update_apply()

    def _set_editable(self, key: str, checked: bool) -> None:
        row = self.rows.get(key)
        if row is None:
            return
        support = bool(row["device"].get("hctm"))
        for widget in (row["combo"], row["tmt1"], row["tmt2"]):
            widget.setEnabled(support and checked)
        self._update_apply()

    def _preset_chosen(self, key: str) -> None:
        row = self.rows.get(key)
        if row is None:
            return
        values = row["combo"].currentData()
        if values is None:
            return
        first, second = values
        for widget, value in ((row["tmt1"], first), (row["tmt2"], second)):
            widget.blockSignals(True)
            widget.setValue(int(value))
            widget.blockSignals(False)
        self._update_apply()

    def _thresholds_edited(self, key: str) -> None:
        row = self.rows.get(key)
        if row is None:
            return
        first, second = row["tmt1"].value(), row["tmt2"].value()
        row["combo"].blockSignals(True)
        self._select_matching(row["combo"], first, second)
        row["combo"].blockSignals(False)
        self._update_apply()

    def _selection(self, key: str) -> tuple[bool, int, int]:
        row = self.rows[key]
        return (
            row["check"].isChecked(),
            row["tmt1"].value(),
            row["tmt2"].value(),
        )

    def changed(self) -> dict[str, tuple[int, int] | None]:
        if not self.state.get("ok"):
            return {}
        changes: dict[str, tuple[int, int] | None] = {}
        for key in self.rows:
            checked, first, second = self._selection(key)
            initial = self.initial.get(key, (False, 0, 0))
            if not checked:
                if initial[0]:
                    changes[key] = None          # désactivation demandée
                continue
            if not initial[0] or (first, second) != (initial[1], initial[2]):
                changes[key] = (first, second)
        return changes

    def _update_apply(self) -> None:
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            return
        changes = self.changed()
        self.apply_button.setEnabled(bool(changes))
        if changes:
            self.hint_label.setText(
                f"{len(changes)} SSD à modifier"
            )
        elif self.state.get("devices"):
            self.hint_label.setText("Aucun changement.")

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        changes = self.changed()
        if not changes:
            return
        details = []
        for device, thresholds in changes.items():
            if thresholds is None:
                details.append(f"• {device} : rendre la main au firmware")
            else:
                details.append(
                    f"• {device} : limiter à {thresholds[0]}/{thresholds[1]} °C"
                )
        answer = QMessageBox.question(
            self, "Limite thermique NVMe",
            "Appliquer ces réglages ?\n\n" + "\n".join(details)
            + "\n\nRéglage persistant, écrit dans le contrôleur.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        for device, thresholds in changes.items():
            if thresholds is not None and thresholds[0] >= thresholds[1]:
                QMessageBox.warning(
                    self, "Limite thermique NVMe",
                    f"{device} : TMT1 doit être inférieur à TMT2.",
                )
                return
        if self.host is not None:
            self._apply_remote(changes)
            return
        dialog = LocalTaskDialog(
            self, title="Limite thermique NVMe",
            subtitle="Seuils HCTM des contrôleurs.",
            argv=["/usr/bin/bash", "-c", nvme_thermal_script(changes)],
            progress_parser=parse_gablue_progress,
            progress_text="Application des seuils…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def _apply_remote(
        self, changes: dict[str, tuple[int, int] | None],
    ) -> None:
        """Applique par SSH ; mot de passe seulement si la règle manque."""
        password = ""
        if not remote_helpers_nopasswd(self.host):
            password = ask_remote_password(self, self.host)
            if password is None:
                return
        command = remote_sudo_prefix(password) + nvme_thermal_remote_script(
            changes
        )
        dialog = RemoteTaskDialog(
            self, host=self.host, title="Limite thermique NVMe",
            subtitle="Seuils HCTM des contrôleurs, par SSH.",
            command=command, password=password,
            progress_mode="busy",
            progress_text="Application des seuils…",
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        if self.changed():
            answer = QMessageBox.question(
                self, "Réglages non appliqués",
                "Fermer sans appliquer les réglages modifiés ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
