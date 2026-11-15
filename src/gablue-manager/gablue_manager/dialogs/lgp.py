"""Options LGP : overlay MangoHud des jeux Linux (packs .lgp et exécutables).
"""

import shutil
from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QMessageBox, QPushButton, QVBoxLayout,
)

from ..constants import _which
from ..local import (
    lgp_binary, lgp_options_path, mangohud_conf_path, mangohud_effective,
    parse_lgp_options, parse_mangohud_conf,
)
from ..ssh import ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..util import address_text, display_name
from ..widgets import MangoHudSection

MANGOHUD_KEYS = ("mangohud", "mangohud_layout", "fps_limit")

class LgpOptionsDialog(QDialog):
    """Options LGP, communes à tous les lancements de jeux Linux.

    Un seul réglage persistant pour l'instant : l'overlay MangoHud, stocké
    dans `~/.config/lgplaunch/options` (`mangohud`, `mangohud_layout`,
    `fps_limit`). « Appliquer » passe par lgplaunch lui-même (`--mangohud`,
    `--mangohud-layout`, `--fps-limit`) : le `~/.config/MangoHud/MangoHud.conf`
    du poste n'est jamais modifié, l'overlay est surchargé au lancement
    (`MANGOHUD_CONFIG`). Un poste distant est lu et modifié par SSH.

    L'interface dit « LGP » : `lgplaunch` est le nom du binaire, pas celui du
    format que l'utilisateur manipule.
    """

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host or {})
        remote = bool(self.host)
        self.setWindowTitle(
            "Options LGP" + (f" — {display_name(self.host)}" if remote else "")
        )
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Options LGP", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        if remote:
            note_text = (
                f"Réglages de {address_text(self.host)}, par SSH. Effet au "
                "prochain lancement."
            )
        else:
            note_text = (
                "Réglages communs aux jeux Linux (packs .lgp et exécutables "
                "directs). Effet au prochain lancement."
            )
        note = QLabel(note_text, self)
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.mangohud = MangoHudSection(self)
        self.mangohud.changed.connect(self._update_apply)
        root_layout.addWidget(self.mangohud)
        root_layout.addStretch(1)

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

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        if self.host:
            code, output = ssh_capture(
                self.host,
                "cat ~/.config/lgplaunch/options 2>/dev/null; echo '---'; "
                "command -v lgplaunch || true; echo '---'; "
                "cat ~/.config/MangoHud/MangoHud.conf 2>/dev/null || true",
            )
            if code != 0:
                return {"ok": False}
            config, _, rest = output.partition("---")
            binary, _, mangohud_conf = rest.partition("---")
            state = parse_lgp_options(config)
            state.update(parse_mangohud_conf(mangohud_conf))
            state["installed"] = bool(binary.strip())
        else:
            try:
                text = lgp_options_path().read_text(encoding="utf-8")
            except OSError:
                text = ""
            try:
                mangohud_conf = mangohud_conf_path().read_text(encoding="utf-8")
            except OSError:
                mangohud_conf = ""
            state = parse_lgp_options(text)
            state.update(parse_mangohud_conf(mangohud_conf))
            state["installed"] = (
                shutil.which("lgplaunch") is not None
                or Path("/usr/bin/lgplaunch").exists()
            )
        state["ok"] = True
        return state

    def refresh(self) -> None:
        self.state = self.read_state()
        ok = bool(self.state.get("ok"))
        installed = ok and bool(self.state.get("installed"))
        self.initial = mangohud_effective(self.state) if installed else {}
        self.mangohud.set_enabled(installed)
        self.mangohud.set_values(
            {key: self.initial.get(key) for key in MANGOHUD_KEYS}
        )
        self._update_apply()

    def desired(self) -> dict:
        return self.mangohud.values()

    def changed(self) -> dict:
        """Ce qui diffère de la configuration actuelle de LGP."""
        desired = self.desired()
        return {
            key: desired[key]
            for key in MANGOHUD_KEYS
            if desired[key] != self.initial.get(key)
        }

    def _update_apply(self) -> None:
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — options non modifiables.")
            return
        if not self.state.get("installed"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("LGP n'est pas installé sur ce poste.")
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
        """Arguments lgplaunch correspondant aux changements demandés."""
        changes = self.changed()
        args: list[str] = []
        if "mangohud" in changes:
            args.append("--mangohud on" if changes["mangohud"] else "--mangohud off")
        if "mangohud_layout" in changes:
            args.append(f"--mangohud-layout {changes['mangohud_layout']}")
        if "fps_limit" in changes:
            args.append(f"--fps-limit {changes['fps_limit']}")
        return args

    def describe(self, changes: dict) -> str:
        labels = []
        if "mangohud" in changes:
            labels.append(
                "MangoHud : " + ("affiché" if changes["mangohud"] else "masqué")
            )
        if "mangohud_layout" in changes:
            labels.append(
                "Disposition MangoHud : "
                + ("horizontale" if changes["mangohud_layout"] == "horizontal"
                   else "rectangle")
            )
        if "fps_limit" in changes:
            value = changes["fps_limit"]
            labels.append(
                "Limite fps : " + ("illimitée" if value == 0 else f"{value} fps")
            )
        return "\n".join(f"• {label}" for label in labels)

    def apply_changes(self) -> None:
        changes = self.changed()
        if not changes:
            return
        answer = QMessageBox.question(
            self, "Options LGP",
            f"Appliquer ces options ?\n\n{self.describe(changes)}\n\n"
            "Effet au prochain lancement.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        binary = "lgplaunch" if self.host else lgp_binary()
        script = "exec < /dev/null\n" + " && ".join(
            f"{binary} {argument}" for argument in self.commands()
        ) + "\n"
        if self.host:
            dialog = RemoteTaskDialog(
                self, host=self.host, title="Options LGP",
                subtitle="Effet au prochain lancement.",
                command=script,
                progress_mode="busy",
                progress_text="Application des options LGP…",
            )
        else:
            dialog = LocalTaskDialog(
                self, title="Options LGP",
                subtitle="Effet au prochain lancement.",
                argv=[_which("bash"), "-c", script],
                progress_mode="busy",
                progress_text="Application des options LGP…",
                reveal_log_on_error=True,
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
