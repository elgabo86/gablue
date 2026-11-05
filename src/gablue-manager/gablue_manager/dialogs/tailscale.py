"""Activation de Tailscale, serveur demandé à l'utilisateur.
"""

import subprocess

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QInputDialog, QLabel, QLineEdit,
    QMessageBox, QPushButton, QVBoxLayout,
)

from ..constants import _which
from ..local import (
    TAILSCALE_STATE_SCRIPT, parse_tailscale_state, remote_helpers_nopasswd,
    tailscale_apply_script, valid_tailscale_server,
)
from ..ssh import ask_remote_password, remote_sudo_prefix, ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import fit_dialog_height, restyle_label
from ..util import address_text, display_name

class TailscaleDialog(QDialog):
    """Activer ou couper Tailscale, avec le serveur de connexion demandé.

    `ujust tailscale-up` codait le serveur en dur : ici le lien est saisi à
    l'activation (prérempli avec celui déjà configuré) et validé avant de
    partir. L'application passe par le helper NOPASSWD, localement ou par SSH.
    """

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host or {})
        remote = bool(self.host)
        self.setWindowTitle(
            "Tailscale" + (f" — {display_name(self.host)}" if remote else "")
        )
        self.setModal(True)
        self.setMinimumWidth(660)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Tailscale", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            f"État de {address_text(self.host)}, lu et modifié par SSH."
            if remote else
            "Connexion de ce poste à un réseau privé Tailscale ou headscale."
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
        self.check = QCheckBox("Connecter ce poste à Tailscale", frame)
        self.check.setCursor(Qt.PointingHandCursor)
        self.check.toggled.connect(self._update_apply)
        frame_layout.addWidget(self.check)
        detail = QLabel(
            "Le serveur de connexion (headscale ou Tailscale) est demandé à "
            "l'activation : aucun lien n'est codé en dur. `tailscale up` "
            "affiche ensuite la page de validation dans le navigateur. Le "
            "service reste actif après un redémarrage ; décocher le coupe et "
            "le désactive.",
            frame,
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        frame_layout.addWidget(detail)
        self.recommendation = QLabel(frame)
        self.recommendation.setObjectName("hint")
        frame_layout.addWidget(self.recommendation)
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
        self.initial = False
        self.refresh()
        fit_dialog_height(self, 660)

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        if self.host:
            code, output = ssh_capture(self.host, TAILSCALE_STATE_SCRIPT)
        else:
            try:
                result = subprocess.run(
                    [_which("bash"), "-c", TAILSCALE_STATE_SCRIPT],
                    capture_output=True, text=True, timeout=10,
                )
                code, output = result.returncode, result.stdout
            except (OSError, subprocess.TimeoutExpired):
                code, output = 1, ""
        if code != 0 and not output.strip():
            return {"ok": False}
        return {"ok": True, **parse_tailscale_state(output)}

    def refresh(self) -> None:
        self.state = self.read_state()
        ok = bool(self.state.get("ok"))
        installed = ok and bool(self.state.get("installed"))
        self.initial = bool(self.state.get("enabled")) if installed else False
        self.check.blockSignals(True)
        self.check.setChecked(self.initial)
        self.check.blockSignals(False)
        self.check.setEnabled(installed)
        self._update_apply()

    def changed(self) -> bool:
        return self.check.isEnabled() and self.check.isChecked() != self.initial

    def _update_apply(self) -> None:
        differs = bool(self.state.get("ok")) and self.check.isChecked()
        message = "Recommandé : désactivé"
        if differs:
            message += " — votre choix diffère"
        self.recommendation.setText(message)
        restyle_label(self.recommendation, "warn" if differs else "hint")
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — réglage non modifiable.")
            return
        if not self.state.get("installed"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText(
                "tailscale n'est pas installé sur ce poste."
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
        server = ""
        if enabled:
            current = self.state.get("server") or "https://"
            text, ok = QInputDialog.getText(
                self, "Serveur Tailscale",
                "Lien du serveur de connexion (headscale ou Tailscale) :",
                QLineEdit.Normal, current,
            )
            if not ok:
                return
            server = text.strip()
            if not valid_tailscale_server(server):
                QMessageBox.warning(
                    self, "Serveur Tailscale",
                    "Le lien doit commencer par http:// ou https://, sans "
                    "espace ni guillemet.",
                )
                return
        answer = QMessageBox.question(
            self, "Tailscale",
            (
                f"Connecter ce poste au serveur\n{server} ?\n\n"
                "Une page de validation peut s'ouvrir dans le navigateur."
            ) if enabled else
            "Couper Tailscale et désactiver son service au démarrage ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        if self._start(enabled, server):
            self.accept()

    def _start(self, enabled: bool, server: str) -> bool:
        script = tailscale_apply_script(enabled, server)
        subtitle = (
            "tailscale up — validation dans le navigateur."
            if enabled else
            "tailscale down — service arrêté et désactivé."
        )
        if self.host:
            password = ""
            if not remote_helpers_nopasswd(self.host):
                password = ask_remote_password(self, self.host)
                if password is None:
                    return False
            command = remote_sudo_prefix(password) + script
            dialog = RemoteTaskDialog(
                self, host=self.host, title="Tailscale",
                subtitle=subtitle, command=command, password=password,
            )
        else:
            dialog = LocalTaskDialog(
                self, title="Tailscale", subtitle=subtitle,
                argv=[_which("bash"), "-c", script],
            )
        start_task(self, dialog)
        return True
