"""Écran de connexion : pont manette et thème sombre.

Deux réglages de l'écran de connexion (greeter Plasma Login), écrits dans
`/etc/gablue/login.conf` par `/usr/libexec/gablue-login` :

- « Manette » : le pont tourne dans la session du greeter et n'affiche le
  clavier visuel que tant qu'une manette est présente ; le pointeur suit les
  deux sticks, X/R1 clique, Carré efface, Start valide.
- « Sombre » : le greeter lit son propre `kdeglobals` (Breeze Dark) ; sans
  réglage, il reste clair.

Les deux s'appliquent au **prochain démarrage du greeter** (déconnexion ou
démarrage), jamais dans la session en cours : le pont vit côté greeter, il
n'existe aucun moyen de le recharger à chaud sans couper la session.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..constants import _which
from ..local import login_screen_apply_script, login_screen_state
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height


class LoginScreenDialog(QDialog):
    """Manette et thème de l'écran de connexion (réglages du poste local)."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Écran de connexion")
        self.setModal(True)
        self.setMinimumWidth(660)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Écran de connexion", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Réglages du poste. Appliqués à la prochaine déconnexion (ou au "
            "prochain démarrage), pas dans la session en cours.",
            self,
        )
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

        self.pad_check = QCheckBox(
            "Piloter l'écran de connexion à la manette", frame
        )
        self.pad_check.setCursor(Qt.PointingHandCursor)
        self.pad_check.toggled.connect(self._update_apply)
        frame_layout.addWidget(self.pad_check)
        pad_detail = QLabel(
            "Le clavier visuel s'ouvre dès qu'une manette est présente : "
            "sticks = pointeur, X ou R1 = clic, Carré = retour arrière, "
            "Start = Entrée. Sans manette, rien ne change : un clic souris "
            "n'ouvre pas de clavier.",
            frame,
        )
        pad_detail.setObjectName("hostDetail")
        pad_detail.setWordWrap(True)
        frame_layout.addWidget(pad_detail)

        self.dark_check = QCheckBox("Écran de connexion sombre", frame)
        self.dark_check.setCursor(Qt.PointingHandCursor)
        self.dark_check.toggled.connect(self._update_apply)
        frame_layout.addWidget(self.dark_check)
        dark_detail = QLabel(
            "Applique le schéma Breeze Dark au compte du greeter : écran de "
            "connexion et clavier visuel en sombre. Le thème du bureau n'est "
            "pas touché.",
            frame,
        )
        dark_detail.setObjectName("hostDetail")
        dark_detail.setWordWrap(True)
        frame_layout.addWidget(dark_detail)

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

        self.state: dict = {}
        self.initial_pad = True
        self.initial_dark = False
        self.refresh()
        fit_dialog_height(self, 660)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        self.state = login_screen_state()
        available = bool(self.state.get("available"))
        self.initial_pad = bool(self.state.get("pad", True))
        self.initial_dark = bool(self.state.get("dark", False))
        for check, value in (
            (self.pad_check, self.initial_pad),
            (self.dark_check, self.initial_dark),
        ):
            check.blockSignals(True)
            check.setChecked(value)
            check.blockSignals(False)
            check.setEnabled(available)
        self._update_apply()

    def changed(self) -> bool:
        return (
            self.pad_check.isEnabled()
            and (
                self.pad_check.isChecked() != self.initial_pad
                or self.dark_check.isChecked() != self.initial_dark
            )
        )

    def _update_apply(self) -> None:
        if not self.state.get("available"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText(
                "Helper absent (image antérieure) : réglage non modifiable."
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
        pad = self.pad_check.isChecked()
        dark = self.dark_check.isChecked()
        answer = QMessageBox.question(
            self, "Écran de connexion",
            "Enregistrer ces réglages ?\n\n"
            f"Manette : {'activée' if pad else 'désactivée'}\n"
            f"Thème : {'sombre' if dark else 'clair'}\n\n"
            "Ils seront actifs à la prochaine déconnexion.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.Yes,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title="Écran de connexion",
            subtitle="Réglage enregistré.",
            argv=[_which("bash"), "-c", login_screen_apply_script(pad, dark)],
            progress_mode="busy",
            progress_text="Enregistrement du réglage…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()

    def reject(self) -> None:
        if self.changed():
            answer = QMessageBox.question(
                self, "Réglage non appliqué",
                "Fermer sans enregistrer les réglages modifiés ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()
