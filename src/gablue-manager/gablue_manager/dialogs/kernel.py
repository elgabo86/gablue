"""Réglages du noyau et émulation CPUID (local et distant).
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..constants import _which
from ..local import (
    cpuid_emu_active, kargs_choices, kargs_pending, kernel_apply_script,
    remote_helpers_nopasswd,
)
from ..ssh import ask_remote_password, remote_sudo_prefix, ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import (
    fit_dialog_height, recommendation_badge, set_recommendation,
)
from ..util import address_text, display_name

# Libellé de la case, puis description affichée dessous.
CHOICE_LABELS = {
    "mitigations": (
        "Protections du processeur",
        "Contre Spectre et Meltdown. À laisser activé.",
    ),
    "cpuid": (
        "Correctif CPUID",
        "Débloque certains jeux qui plantent au lancement.",
    ),
    "ertm": (
        "Désactiver le mode ERTM du Bluetooth",
        "Stabilise certaines manettes et casques.",
    ),
    "hdmi21": (
        "HDMI 2.1 (GPU AMD)",
        "Active le HDMI 2.1 (FreeSync inclus). Peut scintiller.",
    ),
    "cpuid_emu": (
        "Émulation CPUID fault",
        "Jeux AMD AM4 / Steam Deck. KVM déchargé pendant l'émulation.",
    ),
}

# Mot affiché dans la confirmation, selon l'état de la case.
CHOICE_STATE = {
    "mitigations": {True: "activées", False: "désactivées"},
    "cpuid": {True: "activé", False: "désactivé"},
    "ertm": {True: "désactivé", False: "activé"},
    "hdmi21": {True: "activé", False: "désactivé"},
    "cpuid_emu": {True: "activée", False: "désactivée"},
}

# Valeur recommandée par case : ce que Gablue conseille d'avoir coché.
CHOICE_RECOMMENDED = {
    "mitigations": True,    # garder les protections : sécurité d'abord
    "cpuid": True,          # débloque des jeux, sans coût ailleurs
    "ertm": True,           # réglage recommandé par Gablue
    "hdmi21": False,        # scintillements possibles : opt-in explicite
    "cpuid_emu": False,     # niche AMD : KVM doit rester disponible
}

class SettingsDialog(QDialog):
    """Options avancées : des cases à cocher, un seul bouton « Appliquer ».

    Trois réglages sont des paramètres du noyau, accumulés puis envoyés en
    **un seul** appel à `/usr/libexec/gablue-kargs` — un déploiement, sans
    mot de passe grâce à la règle sudoers — au lieu d'un aller-retour par
    case ; le quatrième charge l'émulation CPUID fault. L'état lu est celui du
    prochain démarrage, donc les cases restent justes une fois appliquées.

    La lecture et l'exécution viennent des sous-classes : machine courante
    (`LocalSettingsDialog`) ou poste distant par SSH (`RemoteSettingsDialog`).
    """

    ROWS = ("mitigations", "cpuid", "ertm", "hdmi21", "cpuid_emu")

    def __init__(self, parent=None, title: str = "Configuration avancée",
                 subtitle: str = ""):
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setModal(True)
        self.setMinimumWidth(640)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel(title, self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(subtitle, self)
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)
        self.checks: dict[str, QCheckBox] = {}
        self.recommendations: dict[str, QLabel] = {}
        for key in self.ROWS:
            label, description = CHOICE_LABELS[key]
            self._add_row(key, label, description)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        buttons.addWidget(self.hint_label)
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        # « Fermer » passe par reject() : avec des cases modifiées, il demande
        # confirmation au lieu de jeter les choix en silence.
        self.close_button.clicked.connect(self.reject)
        buttons.addWidget(self.close_button)
        self.apply_button = QPushButton("Appliquer", self)
        self.apply_button.setObjectName("accent")
        self.apply_button.setCursor(Qt.PointingHandCursor)
        self.apply_button.clicked.connect(self.apply_changes)
        buttons.addWidget(self.apply_button)
        root_layout.addLayout(buttons)

        self.state: dict = {"ok": False}
        self.initial: dict[str, bool] = {}
        self.refresh()
        fit_dialog_height(self, 640)

    def _add_row(self, key: str, label: str, description: str) -> None:
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        top = QHBoxLayout()
        top.setSpacing(8)
        check = QCheckBox(label, frame)
        check.setCursor(Qt.PointingHandCursor)
        check.toggled.connect(self._update_apply)
        top.addWidget(check)
        if key in CHOICE_RECOMMENDED:
            badge = recommendation_badge(frame)
            top.addWidget(badge)
            self.recommendations[key] = badge
        top.addStretch(1)
        layout.addLayout(top)
        self.checks[key] = check

        detail = QLabel(description, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        layout.addStretch(1)

        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        self.state = self.read_state()
        ok = bool(self.state.get("ok"))
        choices = (
            kargs_choices(
                self.state.get("kargs", set()),
                bool(self.state.get("cpuid_emu")),
            )
            if ok else {}
        )
        self.initial = dict(choices)
        for key, check in self.checks.items():
            check.blockSignals(True)
            check.setChecked(bool(choices.get(key)))
            check.blockSignals(False)
            check.setEnabled(ok)
        self._update_apply()

    def changed(self) -> list[str]:
        """Clés dont la case diffère de l'état du noyau.

        État illisible : `initial` est vide, une case décochée ne doit pas
        passer pour un changement (même piège que dans `ServicesDialog`).
        """
        if not self.state.get("ok"):
            return []
        return [
            key for key, check in self.checks.items()
            if check.isChecked() != bool(self.initial.get(key))
        ]

    def _update_recommendations(self) -> None:
        ok = bool(self.state.get("ok"))
        for key, badge in self.recommendations.items():
            recommended = CHOICE_RECOMMENDED[key]
            differs = ok and self.checks[key].isChecked() != recommended
            set_recommendation(
                badge,
                "à activer" if recommended else "à désactiver",
                differs, ok,
                advice=CHOICE_STATE[key][recommended],
            )

    def _update_apply(self) -> None:
        self._update_recommendations()
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText("État illisible — réglages non modifiables.")
            return
        changed = self.changed()
        self.apply_button.setEnabled(bool(changed))
        if changed:
            count = len(changed)
            self.hint_label.setText(
                f"{count} réglage{'s' if count > 1 else ''} à appliquer"
            )
        else:
            self.hint_label.setText("Aucun changement.")

    # -- application groupée ------------------------------------------------
    def apply_changes(self) -> None:
        keys = self.changed()
        if not keys:
            return
        details = "\n".join(
            f"• {CHOICE_LABELS[key][0]} : "
            f"{CHOICE_STATE[key][self.checks[key].isChecked()]}"
            for key in keys
        )
        answer = QMessageBox.question(
            self, "Réglages du noyau",
            f"Appliquer ces réglages ?\n\n{details}\n\n"
            "Ils prennent effet au prochain démarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        changes = {key: self.checks[key].isChecked() for key in keys}
        # La tâche s'ouvre dans sa propre fenêtre : ce dialogue n'a plus rien à
        # montrer (son état serait lu avant la fin de l'opération) et le fermer
        # rend la main tout de suite pour un autre poste.
        if self.apply(changes):
            self.accept()

    def reject(self) -> None:
        # Fermer avec des cases modifiées ne doit pas jeter les choix en
        # silence : l'application se fait uniquement par « Appliquer ».
        if self.changed():
            answer = QMessageBox.question(
                self, "Réglages non appliqués",
                "Fermer sans appliquer les réglages modifiés ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
        super().reject()

    # -- à fournir par la sous-classe ---------------------------------------
    def read_state(self) -> dict:
        raise NotImplementedError

    def apply(self, changes: dict[str, bool]) -> bool:
        raise NotImplementedError

class LocalSettingsDialog(SettingsDialog):
    """Options avancées de la machine courante."""

    def __init__(self, parent=None):
        super().__init__(
            parent, "Configuration avancée",
            "Réglages du noyau. Effet au prochain démarrage.",
        )

    def read_state(self) -> dict:
        return {
            "ok": True,
            "kargs": kargs_pending(),
            "cpuid_emu": cpuid_emu_active(),
        }

    def apply(self, changes: dict[str, bool]) -> bool:
        # `sudo -n` : sur l'image Gablue les helpers sont autorisés sans mot de
        # passe (sudoers), donc aucune invite. Hors image, l'échec est affiché
        # dans le journal plutôt que de faire surgir un dialogue root.
        dialog = LocalTaskDialog(
            self, title="Réglages du noyau",
            subtitle="Effet au prochain démarrage.",
            argv=[_which("bash"), "-c", kernel_apply_script(changes)],
        )
        start_task(self, dialog)
        return True

class RemoteSettingsDialog(SettingsDialog):
    """Options avancées d'un poste distant, lues et appliquées par SSH."""

    def __init__(self, parent=None, host: dict | None = None):
        self.host = dict(host or {})
        super().__init__(
            parent, f"Configuration avancée — {display_name(self.host)}",
            f"Réglages de {address_text(self.host)}, par SSH. Effet à son "
            "prochain démarrage.",
        )

    def read_state(self) -> dict:
        code, output = ssh_capture(self.host, (
            "rpm-ostree kargs 2>/dev/null || cat /proc/cmdline\n"
            "if [ -f /etc/modules-load.d/gablue-cpuid-emu.conf ]; "
            "then echo gablue-cpuid-emu=on; else echo gablue-cpuid-emu=off; fi\n"
        ))
        lines = [line for line in output.splitlines() if line.strip()]
        if code != 0 or len(lines) < 2:
            return {"ok": False}
        emu = lines[-1].strip().endswith("=on")
        return {
            "ok": True,
            "kargs": set(lines[0].split()),
            "cpuid_emu": emu,
        }

    def apply(self, changes: dict[str, bool]) -> bool:
        password = ""
        if not remote_helpers_nopasswd(self.host):
            password = ask_remote_password(self, self.host)
            if password is None:
                return False
        command = remote_sudo_prefix(password) + kernel_apply_script(changes)
        dialog = RemoteTaskDialog(
            self, host=self.host, title="Réglages du noyau",
            subtitle="Effet à son prochain démarrage.",
            command=command, password=password,
        )
        start_task(self, dialog)
        return True
