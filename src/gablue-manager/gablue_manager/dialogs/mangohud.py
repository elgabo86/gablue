"""MangoHud : un seul réglage, pour tous les lanceurs.

Le Manager proposait ces trois clés deux fois — une dans les options de gwine,
une dans celles de LGP — alors qu'elles ne décrivent qu'une chose : l'overlay
des performances de `~/.config/MangoHud/MangoHud.conf`, celui que le pad règle
depuis sa page RÉGLAGES. Au lancement, les clés des options partaient en
`MANGOHUD_CONFIG` et **gagnaient clé par clé** sur ce fichier : deux réglages
pour la même chose, dont un masquait l'autre sans le dire. D'où ce dialogue
unique, qui écrit la **même source que l'overlay** avec la même sémantique
(`local.write_mangohud_conf` ↔ `mangohud.sh` du pad).
"""

import base64

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QSlider, QVBoxLayout,
)

from ..constants import _which
from ..local import (
    MANGOHUD_LEGACY_KEYS, gwine_options_path, lgp_options_path,
    mangohud_conf_path, mangohud_state, strip_mangohud_options,
    write_mangohud_conf,
)
from ..ssh import ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import fit_dialog_height
from ..util import address_text, display_name

# Mêmes vues que l'overlay (curseur à trois crans, page RÉGLAGES).
VIEWS = (
    ("masque", "Masqué"),
    ("defaut", "Par défaut (disposition de la config)"),
    ("horizontal", "Horizontal (barre)"),
)

CONF = "$HOME/.config/MangoHud/MangoHud.conf"
GWINE_OPTIONS = "$HOME/.local/share/gwine/options"
LGP_OPTIONS = "$HOME/.config/lgplaunch/options"


class MangoHudDialog(QDialog):
    """MangoHud du compte : masqué / par défaut / horizontal, et limite fps.

    Un seul fichier écrit (`~/.config/MangoHud/MangoHud.conf`), donc un seul
    réglage pour gwine, LGP et les jeux natifs. « Appliquer » réécrit ce
    fichier en préservant le reste (couleurs, autres clés, commentaires) et
    retire, si on le demande, les clés MangoHud héritées des options de gwine
    et de LGP — sans quoi elles masqueraient encore ce réglage au lancement.
    """

    FPS_MAX = 240

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host or {})
        remote = bool(self.host)
        self.setWindowTitle(
            "MangoHud" + (f" — {display_name(self.host)}" if remote else "")
        )
        self.setModal(True)
        self.setMinimumWidth(660)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("MangoHud", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        if remote:
            note_text = (
                f"Réglage de {address_text(self.host)}, par SSH. Effet au "
                "prochain lancement."
            )
        else:
            note_text = (
                "Overlay des performances de tous les jeux (gwine, LGP, jeux "
                "natifs) : un seul réglage, le même que celui du pad."
            )
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

        self.view_combo = QComboBox(frame)
        for value, label in VIEWS:
            self.view_combo.addItem(label, value)
        self.view_combo.currentIndexChanged.connect(self._update_apply)

        top = QHBoxLayout()
        top.setSpacing(8)
        top.addWidget(QLabel("Affichage", frame))
        top.addWidget(self.view_combo)
        top.addStretch(1)
        frame_layout.addLayout(top)

        detail = QLabel(
            "Masqué n'affiche rien, Horizontal met la barre, Par défaut laisse "
            "la disposition choisie dans le fichier (~/.config/MangoHud/"
            "MangoHud.conf). Les couleurs et les autres clés ne sont pas "
            "touchées.",
            frame,
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        frame_layout.addWidget(detail)

        self.fps_slider = QSlider(Qt.Horizontal, frame)
        self.fps_slider.setRange(0, self.FPS_MAX)
        self.fps_slider.setSingleStep(1)
        self.fps_slider.setPageStep(10)
        self.fps_slider.setTickPosition(QSlider.TicksBelow)
        self.fps_slider.setTickInterval(60)
        self.fps_slider.valueChanged.connect(self._update_fps_label)
        self.fps_slider.valueChanged.connect(lambda _value: self._update_apply())
        self.fps_label = QLabel(frame)
        self.fps_label.setMinimumWidth(70)

        fps_row = QHBoxLayout()
        fps_row.setSpacing(8)
        fps_row.addWidget(QLabel("Limite fps", frame))
        fps_row.addWidget(self.fps_slider, 1)
        fps_row.addWidget(self.fps_label)
        frame_layout.addLayout(fps_row)
        frame_layout.addStretch(1)
        self.rows_layout.addWidget(frame)

        # Clés héritées : carte montrée seulement s'il y en a (rien à décider
        # sinon). Décochée par défaut serait un piège : tant qu'elles sont là,
        # elles masquent ce dialogue au lancement.
        self.legacy_frame = QFrame(self)
        self.legacy_frame.setObjectName("hostCard")
        legacy_layout = QVBoxLayout(self.legacy_frame)
        legacy_layout.setContentsMargins(16, 12, 16, 12)
        legacy_layout.setSpacing(6)
        self.legacy_check = QCheckBox(
            "Retirer les réglages MangoHud de gwine et de LGP", self.legacy_frame
        )
        self.legacy_check.setCursor(Qt.PointingHandCursor)
        self.legacy_check.setChecked(True)
        self.legacy_check.toggled.connect(lambda _checked: self._update_apply())
        legacy_layout.addWidget(self.legacy_check)
        legacy_detail = QLabel(
            "Ces anciennes options (gwine et LGP) sont passées à MangoHud au "
            "lancement et prennent le pas sur le réglage ci-dessus : tant "
            "qu'elles existent, changer l'affichage ici n'a aucun effet sur "
            "ces jeux.",
            self.legacy_frame,
        )
        legacy_detail.setObjectName("hostDetail")
        legacy_detail.setWordWrap(True)
        legacy_layout.addWidget(legacy_detail)
        self.rows_layout.addWidget(self.legacy_frame)
        self.legacy_frame.setVisible(False)

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

        self.state: dict = {"ok": False, "legacy": False}
        self.initial: dict = {"vue": "defaut", "fps_limit": 0}
        self.refresh()
        fit_dialog_height(self, 660)

    # -- état ---------------------------------------------------------------
    def read_state(self) -> dict:
        if self.host:
            code, output = ssh_capture(
                self.host,
                f'cat "{CONF}" 2>/dev/null; echo "---"; '
                f'cat "{GWINE_OPTIONS}" 2>/dev/null; echo "---"; '
                f'cat "{LGP_OPTIONS}" 2>/dev/null',
            )
            if code != 0:
                return {"ok": False}
            conf, _, rest = output.partition("---")
            gwine, _, lgp = rest.partition("---")
        else:
            try:
                conf = mangohud_conf_path().read_text(encoding="utf-8")
            except OSError:
                conf = ""
            try:
                gwine = gwine_options_path().read_text(encoding="utf-8")
            except OSError:
                gwine = ""
            try:
                lgp = lgp_options_path().read_text(encoding="utf-8")
            except OSError:
                lgp = ""
        state = mangohud_state(conf)
        state.update({
            "ok": True,
            "conf": conf,
            "gwine": gwine,
            "lgp": lgp,
            "legacy": self._has_legacy(gwine, lgp),
        })
        return state

    @staticmethod
    def _has_legacy(gwine: str, lgp: str) -> bool:
        for text in (gwine, lgp):
            for line in text.splitlines():
                if line.partition("=")[0].strip() in MANGOHUD_LEGACY_KEYS:
                    return True
        return False

    def refresh(self) -> None:
        self.state = self.read_state()
        ok = bool(self.state.get("ok"))
        self.initial = {
            "vue": self.state.get("vue", "defaut"),
            "fps_limit": int(self.state.get("fps_limit", 0)),
        }
        widgets = (self.view_combo, self.fps_slider)
        for widget in widgets:
            widget.blockSignals(True)
        index = self.view_combo.findData(self.initial["vue"])
        self.view_combo.setCurrentIndex(index if index >= 0 else 1)
        self.fps_slider.setValue(self.initial["fps_limit"])
        for widget in widgets:
            widget.blockSignals(False)
            widget.setEnabled(ok)
        self._update_fps_label()
        self.legacy_frame.setVisible(ok and bool(self.state.get("legacy")))
        self.legacy_check.blockSignals(True)
        self.legacy_check.setChecked(True)
        self.legacy_check.blockSignals(False)
        self.legacy_check.setEnabled(ok)
        self._update_apply()

    def desired(self) -> dict:
        return {
            "vue": self.view_combo.currentData() or "defaut",
            "fps_limit": self.fps_slider.value(),
        }

    def changed(self) -> bool:
        if not self.state.get("ok"):
            return False
        if self.desired() != self.initial:
            return True
        return self._strip_requested()

    def _strip_requested(self) -> bool:
        return bool(self.state.get("legacy")) and self.legacy_check.isChecked()

    def _update_apply(self) -> None:
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — réglage non modifiable.")
            return
        dirty = self.changed()
        self.apply_button.setEnabled(dirty)
        self.hint_label.setText(
            "Réglage à appliquer" if dirty else "Aucun changement."
        )

    def _update_fps_label(self) -> None:
        value = self.fps_slider.value()
        self.fps_label.setText("illimitée" if value == 0 else f"{value} fps")

    # -- application --------------------------------------------------------
    @staticmethod
    def _write_cmd(path: str, content: str) -> str:
        """Écrit `content` dans `path` sans quoting : tout passe en base64.

        Le contenu est la config du compte (couleurs, commentaires) : une
        heredoc shell le massacrerait au premier `$` ou backtick.
        """
        encoded = base64.b64encode(content.encode("utf-8")).decode("ascii")
        return (
            f'printf \'%s\' \'{encoded}\' | base64 -d > "{path}.gablue.tmp"'
            f' && mv "{path}.gablue.tmp" "{path}"\n'
        )

    def script(self) -> str:
        desired = self.desired()
        conf = write_mangohud_conf(
            self.state.get("conf", ""), desired["vue"], desired["fps_limit"]
        )
        lines = [
            "exec < /dev/null",
            f'mkdir -p "$(dirname "{CONF}")"',
            self._write_cmd(CONF, conf).rstrip("\n"),
        ]
        if self._strip_requested():
            for path, key in ((GWINE_OPTIONS, "gwine"), (LGP_OPTIONS, "lgp")):
                text = self.state.get(key, "")
                cleaned = strip_mangohud_options(text)
                if cleaned == text:
                    continue
                # `if` et non `[ -f ] &&` : un fichier absent laisserait un
                # code retour 1 en fin de script, donc une tâche « en échec ».
                lines.append(
                    f'if [ -f "{path}" ]; then\n'
                    f"    {self._write_cmd(path, cleaned).rstrip()}\n"
                    f"fi"
                )
        return "\n".join(lines) + "\n"

    def describe(self) -> str:
        desired = self.desired()
        labels = []
        if desired != self.initial:
            labels.append(
                "Affichage : "
                + dict(VIEWS).get(desired["vue"], desired["vue"])
            )
            fps = desired["fps_limit"]
            labels.append(
                "Limite fps : " + ("illimitée" if fps == 0 else f"{fps} fps")
            )
        if self._strip_requested():
            labels.append("Réglages MangoHud de gwine et de LGP : retirés")
        return "\n".join(f"• {label}" for label in labels)

    def apply_changes(self) -> None:
        if not self.changed():
            return
        answer = QMessageBox.question(
            self, "MangoHud",
            f"Appliquer ce réglage ?\n\n{self.describe()}\n\n"
            "Effet au prochain lancement des jeux.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        script = self.script()
        if self.host:
            dialog = RemoteTaskDialog(
                self, host=self.host, title="MangoHud",
                subtitle="Effet au prochain lancement des jeux.",
                command=script,
                progress_mode="busy",
                progress_text="Application du réglage MangoHud…",
            )
        else:
            dialog = LocalTaskDialog(
                self, title="MangoHud",
                subtitle="Effet au prochain lancement des jeux.",
                argv=[_which("bash"), "-c", script],
                progress_mode="busy",
                progress_text="Application du réglage MangoHud…",
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
