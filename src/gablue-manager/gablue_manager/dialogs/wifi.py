"""Veille radio Wi-Fi (power save) : latence stable, en local ou à distance.
"""

import subprocess

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QPushButton,
    QVBoxLayout,
)

from ..constants import _which
from ..local import (
    WIFI_POWERSAVE_STATE_SCRIPT, parse_wifi_powersave_state,
    remote_helpers_nopasswd, wifi_powersave_apply_script,
)
from ..ssh import ask_remote_password, remote_sudo_prefix, ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import fit_dialog_height
from ..util import address_text, display_name

class WifiPowersaveDialog(QDialog):
    """Désactiver la veille radio Wi-Fi, sur ce poste ou sur un poste distant.

    La veille radio endort l'antenne entre deux trames : la latence moyenne
    reste correcte mais les pics dépassent la centaine de millisecondes, ce qui
    fait saccader le son de gremote (tampon du tunnel ~25 ms) et rend le bureau
    distant nerveux. Le choix est écrit dans un drop-in NetworkManager, donc il
    survit aux reconnexions — contrairement à un `iw ... set power_save off`.
    """

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host or {})
        remote = bool(self.host)
        self.setWindowTitle(
            "Veille Wi-Fi" + (f" — {display_name(self.host)}" if remote else "")
        )
        self.setModal(True)
        self.setMinimumWidth(660)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Veille Wi-Fi", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            f"État de {address_text(self.host)}, par SSH."
            if remote else
            "Veille radio (power save) des interfaces sans fil."
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        frame = QFrame(self)
        frame.setObjectName("hostCard")
        frame_layout = QVBoxLayout(frame)
        frame_layout.setContentsMargins(16, 12, 16, 12)
        frame_layout.setSpacing(6)
        self.check = QCheckBox("Désactiver la veille Wi-Fi", frame)
        self.check.setCursor(Qt.PointingHandCursor)
        self.check.toggled.connect(self._update_apply)
        frame_layout.addWidget(self.check)
        detail = QLabel(
            "Coupée, l'antenne reste éveillée : latence stable (quelques "
            "millisecondes au lieu de pics à 100-200 ms), ce qui nettoie le "
            "bureau distant et les jeux en réseau. Coût : un peu plus de "
            "consommation, négligeable sur secteur.",
            frame,
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        frame_layout.addWidget(detail)

        # Une ligne par interface : l'état au moment de l'ouverture, pour voir
        # que le réglage s'applique tout de suite (pas seulement au reboot).
        self.info = QLabel(frame)
        self.info.setObjectName("hostDetail")
        self.info.setWordWrap(True)
        self.info.setTextInteractionFlags(Qt.TextSelectableByMouse)
        frame_layout.addWidget(self.info)
        root_layout.addWidget(frame, 1)

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
        self.initial = False
        self.refresh()
        fit_dialog_height(self, 660)

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        if self.host:
            code, output = ssh_capture(self.host, WIFI_POWERSAVE_STATE_SCRIPT)
        else:
            try:
                result = subprocess.run(
                    [_which("bash"), "-c", WIFI_POWERSAVE_STATE_SCRIPT],
                    capture_output=True, text=True, timeout=10,
                )
                code, output = result.returncode, result.stdout
            except (OSError, subprocess.TimeoutExpired):
                code, output = 1, ""
        if code != 0 and not output.strip():
            return {}
        return parse_wifi_powersave_state(output)

    def refresh(self) -> None:
        self.state = self.read_state()
        present = bool(self.state.get("present"))
        self.initial = bool(self.state.get("disabled")) if present else False
        self.check.blockSignals(True)
        self.check.setChecked(self.initial)
        self.check.blockSignals(False)
        self.check.setEnabled(present)
        self._update_info()
        self._update_apply()

    def _update_info(self) -> None:
        ifaces = self.state.get("ifaces") or {}
        if not ifaces:
            self.info.setText("")
            self.info.setVisible(False)
            return
        parts = [
            f"{name} : {'veille coupée' if state == 'off' else 'veille active'}"
            for name, state in sorted(ifaces.items())
        ]
        self.info.setText(" · ".join(parts))
        self.info.setVisible(True)

    def changed(self) -> bool:
        return self.check.isEnabled() and self.check.isChecked() != self.initial

    def _update_apply(self) -> None:
        if not self.state:
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — réglage non modifiable.")
            return
        if not self.state.get("present"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("Aucune interface Wi-Fi sur ce poste.")
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
        disabled = self.check.isChecked()
        script = wifi_powersave_apply_script(disabled)
        subtitle = (
            "Veille radio coupée : latence stable."
            if disabled else
            "Veille radio réactivée (réglage par défaut)."
        )
        if self.host:
            password = ""
            if not remote_helpers_nopasswd(self.host):
                password = ask_remote_password(self, self.host)
                if password is None:
                    return
            command = remote_sudo_prefix(password) + script
            dialog = RemoteTaskDialog(
                self, host=self.host, title="Veille Wi-Fi",
                subtitle=subtitle, command=command, password=password,
                progress_mode="busy",
                progress_text="Application du réglage…",
            )
        else:
            dialog = LocalTaskDialog(
                self, title="Veille Wi-Fi", subtitle=subtitle,
                argv=[_which("bash"), "-c", script],
                progress_mode="busy",
                progress_text="Application du réglage…",
                reveal_log_on_error=True,
            )
        start_task(self, dialog)
        self.accept()
