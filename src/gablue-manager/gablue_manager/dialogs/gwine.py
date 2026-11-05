"""Options de gwine : DXVK, affichage, manettes.
"""

import shutil
from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QVBoxLayout,
)

from ..constants import _which
from ..local import gwine_binary, gwine_options_path, parse_gwine_options
from ..ssh import ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import recommendation_badge, set_recommendation
from ..util import address_text, display_name

GWINE_DISPLAYS = (
    ("x11", "X11 (XWayland)"),
    ("wayland", "Wayland natif (expérimental)"),
)

# Valeur recommandée de chaque réglage (pastille, valeur).
GWINE_RECOMMENDED = {
    "dxvk_async": ("décocher", False),
    "wayland": ("X11 (XWayland)", False),
    "xbox": ("décocher", False),
}

class GwineOptionsDialog(QDialog):
    """Options de gwine, communes à tous les lancements de jeux Windows.

    Trois réglages persistants de `~/.local/share/gwine/options` : le DXVK
    asynchrone (GPLAsync), le mode d'affichage (X11 par défaut, Wayland natif
    expérimental) et l'émulation Xbox 360 des manettes Sony (ds2xbox, tout ou
    rien : toutes les manettes Sony). « Appliquer » passe par gwine lui-même —
    il garde le fichier au bon format et réinstalle les DLL DXVK du préfixe —
    donc le réglage vaut dès le prochain lancement. Un poste distant est lu et
    modifié par SSH.
    """

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host or {})
        remote = bool(self.host)
        self.setWindowTitle(
            "Options de gwine" + (f" — {display_name(self.host)}" if remote else "")
        )
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Options de gwine", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        if remote:
            note_text = (
                f"Réglages de {address_text(self.host)}, par SSH. Effet au "
                "prochain lancement."
            )
        else:
            note_text = (
                "Réglages communs aux jeux Windows. Effet au prochain "
                "lancement."
            )
        note = QLabel(note_text, self)
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)
        self.recommendations: dict[str, QLabel] = {}

        self.dxvk_check = QCheckBox("DXVK asynchrone (GPLAsync)", self)
        self.dxvk_check.setCursor(Qt.PointingHandCursor)
        self.dxvk_check.toggled.connect(self._update_apply)
        self._add_card(
            self.dxvk_check,
            "Compile les shaders en fond (DirectX 9-11, pas DX12) : moins de "
            "saccades ; artefacts possibles.",
            key="dxvk_async",
        )

        display_title = QLabel("Affichage des jeux", self)
        display_title.setObjectName("hostName")
        self.display_combo = QComboBox(self)
        for value, label in GWINE_DISPLAYS:
            self.display_combo.addItem(label, value)
        self.display_combo.currentIndexChanged.connect(self._update_apply)
        display_box = QHBoxLayout()
        display_box.setSpacing(8)
        display_box.addWidget(self.display_combo)
        self._add_card(
            display_title,
            "X11 est le plus sûr. Wayland natif : certains jeux et overlays "
            "peuvent mal réagir.",
            extra=display_box, key="wayland",
        )

        self.xbox_check = QCheckBox("Manettes Sony en mode Xbox 360", self)
        self.xbox_check.setCursor(Qt.PointingHandCursor)
        self.xbox_check.toggled.connect(self._update_apply)
        self._add_card(
            self.xbox_check,
            "Les manettes Sony passent en Xbox 360 pour les jeux XInput.",
            key="xbox",
        )

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
        root_layout.addLayout(buttons)

        self.state: dict = {"ok": False}
        self.initial: dict = {}
        self.refresh()

    def _add_card(self, title_widget, description: str,
                  extra: QHBoxLayout | None = None, key: str = "") -> None:
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        top = QHBoxLayout()
        top.setSpacing(10)
        top.addWidget(title_widget)
        if extra is not None:
            top.addLayout(extra)
        if key:
            badge = recommendation_badge(frame)
            top.addWidget(badge)
            self.recommendations[key] = badge
        top.addStretch(1)
        layout.addLayout(top)

        detail = QLabel(description, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        layout.addStretch(1)

        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        if self.host:
            code, output = ssh_capture(
                self.host,
                "cat ~/.local/share/gwine/options 2>/dev/null; echo '---'; "
                "command -v gwine || true",
            )
            if code != 0:
                return {"ok": False}
            config, _, binary = output.partition("---")
            state = parse_gwine_options(config)
            state["installed"] = bool(binary.strip())
        else:
            try:
                text = gwine_options_path().read_text(encoding="utf-8")
            except OSError:
                text = ""
            state = parse_gwine_options(text)
            state["installed"] = (
                shutil.which("gwine") is not None
                or Path("/usr/bin/gwine").exists()
            )
        state["ok"] = True
        return state

    def refresh(self) -> None:
        self.state = self.read_state()
        ok = bool(self.state.get("ok"))
        installed = ok and bool(self.state.get("installed"))
        self.initial = (
            {
                "dxvk_async": bool(self.state.get("dxvk_async")),
                "wayland": bool(self.state.get("wayland")),
                "xbox": bool(self.state.get("xbox")),
            }
            if installed else {}
        )
        for check in (self.dxvk_check, self.xbox_check):
            check.blockSignals(True)
            check.setChecked(False)
            check.setEnabled(installed)
            check.blockSignals(False)
        self.display_combo.blockSignals(True)
        self.display_combo.setCurrentIndex(0)
        self.display_combo.blockSignals(False)
        self.display_combo.setEnabled(installed)
        if installed:
            self.dxvk_check.blockSignals(True)
            self.dxvk_check.setChecked(self.initial["dxvk_async"])
            self.dxvk_check.blockSignals(False)
            self.xbox_check.blockSignals(True)
            self.xbox_check.setChecked(self.initial["xbox"])
            self.xbox_check.blockSignals(False)
            self._select(self.display_combo, "wayland" if self.initial["wayland"] else "x11")
        self._update_apply()

    @staticmethod
    def _select(combo: QComboBox, value: str) -> None:
        index = combo.findData(value)
        combo.blockSignals(True)
        combo.setCurrentIndex(index if index >= 0 else 0)
        combo.blockSignals(False)

    def desired(self) -> dict:
        return {
            "dxvk_async": self.dxvk_check.isChecked(),
            "wayland": (self.display_combo.currentData() or "x11") == "wayland",
            "xbox": self.xbox_check.isChecked(),
        }

    def changed(self) -> dict:
        """Ce qui diffère de la configuration actuelle de gwine."""
        desired = self.desired()
        changes = {}
        for key in ("dxvk_async", "wayland", "xbox"):
            if desired[key] != self.initial.get(key):
                changes[key] = desired[key]
        return changes

    def _update_recommendations(self) -> None:
        ok = bool(self.state.get("ok"))
        desired = self.desired()
        for key, badge in self.recommendations.items():
            hint, value = GWINE_RECOMMENDED[key]
            differs = ok and desired[key] != value
            set_recommendation(badge, hint, differs, ok)

    def _update_apply(self) -> None:
        self._update_recommendations()
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — options non modifiables.")
            return
        if not self.state.get("installed"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("gwine n'est pas installé sur ce poste.")
            return
        changes = self.changed()
        self.apply_button.setEnabled(bool(changes))
        if changes:
            count = len(changes)
            self.hint_label.setText(
                f"{count} option{'s' if count > 1 else ''} à appliquer"
            )
        else:
            self.hint_label.setText("Aucun changement.")

    # -- application --------------------------------------------------------
    def commands(self) -> list[str]:
        """Arguments gwine correspondant aux changements demandés."""
        changes = self.changed()
        args: list[str] = []
        if "dxvk_async" in changes:
            args.append("--dxvk-async" if changes["dxvk_async"] else "--dxvk")
        if "wayland" in changes:
            args.append("--wayland" if changes["wayland"] else "--x11")
        if "xbox" in changes:
            args.append("--xbox-on" if changes["xbox"] else "--xbox-off")
        return args

    def describe(self, changes: dict) -> str:
        labels = []
        if "dxvk_async" in changes:
            labels.append(
                "DXVK asynchrone : "
                + ("activé" if changes["dxvk_async"] else "désactivé")
            )
        if "wayland" in changes:
            labels.append(
                "Affichage : "
                + ("Wayland natif (expérimental)" if changes["wayland"]
                   else "X11 (XWayland)")
            )
        if "xbox" in changes:
            labels.append(
                "Manettes Sony en Xbox 360 : "
                + ("activé" if changes["xbox"] else "désactivé")
            )
        return "\n".join(f"• {label}" for label in labels)

    def apply_changes(self) -> None:
        changes = self.changed()
        if not changes:
            return
        answer = QMessageBox.question(
            self, "Options de gwine",
            f"Appliquer ces options ?\n\n{self.describe(changes)}\n\n"
            "Effet au prochain lancement.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        binary = "gwine" if self.host else gwine_binary()
        script = "exec < /dev/null\n" + " && ".join(
            f"{binary} {argument}" for argument in self.commands()
        ) + "\n"
        if self.host:
            dialog = RemoteTaskDialog(
                self, host=self.host, title="Options de gwine",
                subtitle="Effet au prochain lancement.",
                command=script,
            )
        else:
            dialog = LocalTaskDialog(
                self, title="Options de gwine",
                subtitle="Effet au prochain lancement.",
                argv=[_which("bash"), "-c", script],
            )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        if self.changed():
            answer = QMessageBox.question(
                self, "Options non appliquées",
                "Fermer sans appliquer les options modifiées ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
