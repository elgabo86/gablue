"""Compression Btrfs : un drapeau par système de fichiers monté.

Le réglage vient de `btrfs property set <montage> compression zstd|""` : il vaut
pour les fichiers créés **ensuite**, dans ce montage et ses sous-dossiers. Les
données déjà écrites ne sont pas recompressées — le dialogue le dit, sinon la
case cochée laisse croire à un gain immédiat.

Un point de montage = une ligne : sur ostree, `/var` et `/var/home` sont deux
sous-volumes du même disque, avec chacun son drapeau.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..local import (
    btrfs_apply_script, btrfs_filesystems, btrfs_helper_available,
    parse_gablue_progress,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height


class BtrfsDialog(QDialog):
    """Cases à cocher des montages Btrfs, un seul « Appliquer »."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Compression Btrfs")
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Compression Btrfs", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Drapeau de compression des systèmes de fichiers Btrfs montés. "
            "Il s'applique aux nouveaux fichiers ; les données existantes ne "
            "sont pas recompressées.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)

        self.checks: dict[str, QCheckBox] = {}
        self.initial: dict[str, bool] = {}

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

        self.state: dict = {"ok": False, "filesystems": []}
        self.refresh()
        fit_dialog_height(self, 680)

    # -- construction -------------------------------------------------------
    def _add_row(self, filesystem: dict) -> None:
        target = filesystem["target"]
        label = filesystem.get("label") or target
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        check = QCheckBox(label, frame)
        check.setCursor(Qt.PointingHandCursor)
        check.toggled.connect(self._update_apply)
        layout.addWidget(check)

        detail = QLabel(frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        compression = filesystem.get("compression") or ""
        detail.setText(
            f"Monté sur {target} · "
            + (f"compression {compression} active" if compression
               else "sans compression")
        )
        layout.addWidget(detail)

        self.checks[target] = check
        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        available = btrfs_helper_available()
        filesystems = btrfs_filesystems() if available else []
        self.state = {"ok": available, "filesystems": filesystems}
        for filesystem in filesystems:
            self._add_row(filesystem)
            self.initial[filesystem["target"]] = bool(
                filesystem.get("compression")
            )
            check = self.checks[filesystem["target"]]
            check.blockSignals(True)
            check.setChecked(self.initial[filesystem["target"]])
            check.blockSignals(False)
            check.setEnabled(available)
        if not available:
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
            )
        elif not filesystems:
            self.hint_label.setText("Aucun système de fichiers Btrfs monté.")
        self._update_apply()

    def changed(self) -> dict[str, bool]:
        if not self.state.get("ok"):
            return {}
        return {
            target: check.isChecked()
            for target, check in self.checks.items()
            if check.isChecked() != bool(self.initial.get(target))
        }

    def _update_apply(self) -> None:
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            return
        changed = self.changed()
        self.apply_button.setEnabled(bool(changed))
        if changed:
            self.hint_label.setText(
                f"{len(changed)} système{'s' if len(changed) > 1 else ''} "
                "à modifier"
            )
        elif self.state.get("filesystems"):
            self.hint_label.setText("Aucun changement.")

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        changes = self.changed()
        if not changes:
            return
        details = "\n".join(
            f"• {target} : "
            + ("activer la compression zstd" if enabled
               else "désactiver la compression")
            for target, enabled in changes.items()
        )
        answer = QMessageBox.question(
            self, "Compression Btrfs",
            f"Appliquer ces changements ?\n\n{details}\n\n"
            "Les fichiers existants ne sont pas recompressés.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title="Compression Btrfs",
            subtitle="Drapeau de compression des montages.",
            argv=["/usr/bin/bash", "-c", btrfs_apply_script(changes)],
            progress_parser=parse_gablue_progress,
            progress_text="Application des drapeaux…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        if self.changed():
            answer = QMessageBox.question(
                self, "Réglages non appliqués",
                "Fermer sans appliquer les changements ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
