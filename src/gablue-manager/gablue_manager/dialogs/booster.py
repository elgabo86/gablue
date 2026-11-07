"""Priorité au premier plan de Plasma.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..constants import _which
from ..local import foreground_booster_path, parse_foreground_booster
from ..ssh import ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import (
    fit_dialog_height, recommendation_badge, set_recommendation,
)
from ..util import address_text, display_name

class ForegroundBoosterDialog(QDialog):
    """Priorité au premier plan de Plasma (ex-`ujust foreground-booster-*`).

    Une case, appliquée tout de suite : `kwriteconfig6` écrit `autostart` dans
    `~/.config/kcgroupsrc` (réglage persistant, relu à chaque ouverture de
    session) et le service utilisateur `plasma-foreground-booster` est relancé
    ou arrêté. Aucun privilège, aucun redémarrage de la machine.
    """

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host or {})
        remote = bool(self.host)
        self.setWindowTitle(
            "Priorité au premier plan"
            + (f" — {display_name(self.host)}" if remote else "")
        )
        self.setModal(True)
        self.setMinimumWidth(660)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Priorité au premier plan", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        if remote:
            note_text = f"Réglage de {address_text(self.host)}, par SSH."
        else:
            note_text = "Appliqué tout de suite, sans redémarrage."
        note = QLabel(note_text, self)
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)

        frame = QFrame(self)
        frame.setObjectName("hostCard")
        frame_layout = QVBoxLayout(frame)
        frame_layout.setContentsMargins(16, 12, 16, 12)
        frame_layout.setSpacing(6)
        top = QHBoxLayout()
        top.setSpacing(8)
        self.check = QCheckBox("Booster l'application au premier plan", frame)
        self.check.setCursor(Qt.PointingHandCursor)
        self.check.toggled.connect(self._update_apply)
        top.addWidget(self.check)
        self.recommendation = recommendation_badge(frame)
        top.addWidget(self.recommendation)
        top.addStretch(1)
        frame_layout.addLayout(top)
        detail = QLabel(
            "Priorité à la fenêtre active. À couper si vous compilez ou "
            "encodez en arrière-plan.",
            frame,
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        frame_layout.addWidget(detail)
        frame_layout.addStretch(1)
        self.rows_layout.addWidget(frame)

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
        self.initial = True
        self.refresh()
        fit_dialog_height(self, 660)

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        if self.host:
            code, output = ssh_capture(
                self.host,
                "cat ~/.config/kcgroupsrc 2>/dev/null; echo '---'; "
                "command -v kwriteconfig6 || true",
            )
            if code != 0:
                return {"ok": False}
            config, _, binary = output.partition("---")
            return {
                "ok": True,
                "installed": bool(binary.strip()),
                "enabled": parse_foreground_booster(config),
            }
        try:
            text = foreground_booster_path().read_text(encoding="utf-8")
        except OSError:
            text = ""
        return {
            "ok": True,
            "installed": _which("kwriteconfig6") is not None,
            "enabled": parse_foreground_booster(text),
        }

    def refresh(self) -> None:
        self.state = self.read_state()
        ok = bool(self.state.get("ok"))
        installed = ok and bool(self.state.get("installed"))
        self.initial = bool(self.state.get("enabled", True)) if installed else True
        self.check.blockSignals(True)
        self.check.setChecked(self.initial if installed else False)
        self.check.blockSignals(False)
        self.check.setEnabled(installed)
        self._update_apply()

    def changed(self) -> bool:
        return self.check.isEnabled() and self.check.isChecked() != self.initial

    def _update_apply(self) -> None:
        ok = bool(self.state.get("ok"))
        differs = ok and not self.check.isChecked()
        set_recommendation(
            self.recommendation, "à activer", differs, ok, advice="activé"
        )
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — réglage non modifiable.")
            return
        if not self.state.get("installed"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText(
                "kwriteconfig6 est absent : impossible de régler ce poste."
            )
            return
        dirty = self.changed()
        self.apply_button.setEnabled(dirty)
        self.hint_label.setText(
            "Réglage à appliquer" if dirty else "Aucun changement."
        )

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        if not self.changed():
            return
        enabled = self.check.isChecked()
        answer = QMessageBox.question(
            self, "Priorité au premier plan",
            ("Activer le boost du premier plan ?" if enabled
             else "Désactiver le boost du premier plan ?") + "\n\n"
            + ("La fenêtre active passe prioritaire." if enabled else
               "Les tâches de fond retrouvent leur part."),
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        value = "true" if enabled else "false"
        action = "restart" if enabled else "stop"
        script = (
            "exec < /dev/null\n"
            "kwriteconfig6 --file kcgroupsrc --group 'Foreground Booster' "
            f"--key autostart {value}\n"
            f"systemctl --user {action} plasma-foreground-booster.service "
            "2>/dev/null || true\n"
        )
        if self.host:
            dialog = RemoteTaskDialog(
                self, host=self.host, title="Priorité au premier plan",
                subtitle="Effet immédiat en session locale.",
                command=script,
                progress_mode="busy",
                progress_text="Application du réglage…",
            )
        else:
            dialog = LocalTaskDialog(
                self, title="Priorité au premier plan",
                subtitle="Effet immédiat.",
                argv=[_which("bash"), "-c", script],
                progress_mode="busy",
                progress_text="Application du réglage…",
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
