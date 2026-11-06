"""Services et pilotes de la machine courante (ex-recettes ujust).
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..local import (
    auto_update_available, docker_available, local_variant, services_choices,
    services_helpers_available, system_apply_script,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import (
    fit_dialog_height, recommendation_badge, set_recommendation,
)

# Libellé de la case, puis description affichée dessous.
CHOICE_LABELS = {
    "displaylink": (
        "DisplayLink",
        "Pour les docks et adaptateurs d'écran en USB.",
    ),
    "docker": (
        "Docker",
        "Démarre le service Docker au boot.",
    ),
    "low_latency": (
        "Vulkan low-latency",
        "Réduit la latence des jeux Vulkan (nouvelle session).",
    ),
    "broadcom_wl": (
        "Wi-Fi Broadcom wl",
        "Pilote propriétaire pour vieux MacBooks. Redémarrage requis.",
    ),
    "atuin": (
        "Historique atuin",
        "Historique commun à vos terminaux. Ctrl-R pour l'ouvrir.",
    ),
    "auto_updates": (
        "Mises à jour automatiques",
        "Image Gablue (au redémarrage) et Flatpaks (système et compte).",
    ),
}

# Mot affiché dans la confirmation, selon l'état de la case.
CHOICE_STATE = {
    "displaylink": {True: "activé", False: "désactivé"},
    "docker": {True: "activé", False: "désactivé"},
    "low_latency": {True: "activée", False: "désactivée"},
    "broadcom_wl": {True: "activé", False: "désactivé"},
    "atuin": {True: "activée", False: "désactivée — historique conservé"},
    "auto_updates": {True: "activées", False: "désactivées"},
}

# Valeur recommandée pour la machine : les réglages de confort Gablue allumés.
# DisplayLink et le Wi-Fi Broadcom n'en ont pas : ils dépendent du matériel
# présent, pas d'un « bon » réglage — une pastille y dirait « décochez » à
# quelqu'un qui a justement le dock ou la carte.
CHOICE_RECOMMENDED = {
    "low_latency": True,    # réglage Gablue
    "atuin": True,          # historique partagé, déjà en place sur l'image
    "auto_updates": True,   # image et Flatpaks à jour sans y penser
}

class ServicesDialog(QDialog):
    """Services et pilotes : des cases à cocher, un seul « Appliquer ».

    Les bascules passent par les helpers `/usr/libexec` (sudoers NOPASSWD) en
    **une seule** opération ; l'intégration atuin, elle, ne touche que les
    fichiers de l'utilisateur. La variante d'image a son propre dialogue : elle
    rebase le poste et ne prend effet qu'au redémarrage.
    """

    ROWS = (
        "displaylink", "docker", "low_latency", "broadcom_wl", "atuin",
        "auto_updates",
    )

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Services et pilotes")
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Services et pilotes", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Services et pilotes de cette machine. Effet immédiat, sauf le "
            "Wi-Fi (redémarrage) et Vulkan/atuin (nouvelle session).",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)
        self.frames: dict[str, QFrame] = {}
        self.checks: dict[str, QCheckBox] = {}
        self.recommendations: dict[str, QLabel] = {}
        for key in self.ROWS:
            self._add_row(key)

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
        fit_dialog_height(self, 680)

    # -- construction -------------------------------------------------------
    def _add_row(self, key: str) -> None:
        label, description = CHOICE_LABELS[key]
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

        self.frames[key] = frame
        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        helpers = services_helpers_available()
        variant = local_variant()
        self.state = {
            "ok": helpers,
            "variant": variant,
            "choices": services_choices() if helpers else {},
        }
        choices = self.state["choices"]
        self.initial = dict(choices)
        # Docker n'existe que sur une variante DX : ailleurs, la case n'est pas
        # affichée du tout (pas de paquet, pas de service). Les mises à jour
        # automatiques demandent le helper de l'image : sur une image plus
        # ancienne, la ligne disparaît au lieu de proposer un réglage qui
        # échouerait.
        self.frames["docker"].setVisible(docker_available(variant))
        self.frames["auto_updates"].setVisible(auto_update_available())
        for key, check in self.checks.items():
            check.blockSignals(True)
            check.setChecked(bool(choices.get(key)))
            check.blockSignals(False)
            check.setEnabled(helpers)
        self._update_apply()

    def changed(self) -> list[str]:
        """Clés dont la case diffère de l'état courant.

        État illisible (helpers absents) : `initial` est vide, et comparer une
        case décochée à une clé absente ferait croire à un changement sur
        chaque ligne. Rien n'est modifiable dans ce cas, donc rien n'est
        « à appliquer ».
        """
        if not self.state.get("ok"):
            return []
        return [
            key for key, check in self.checks.items()
            if self.frames[key].isVisibleTo(self)
            and check.isChecked() != bool(self.initial.get(key))
        ]

    def _update_recommendations(self) -> None:
        ok = bool(self.state.get("ok"))
        for key, badge in self.recommendations.items():
            recommended = CHOICE_RECOMMENDED[key]
            differs = ok and self.checks[key].isChecked() != recommended
            set_recommendation(
                badge, "cocher" if recommended else "décocher", differs, ok,
                advice=CHOICE_STATE[key][recommended],
            )

    def _update_apply(self) -> None:
        self._update_recommendations()
        if not self.state.get("ok"):
            self.apply_button.setEnabled(False)
            self.hint_label.setText(
                "Helpers absents de cette image — reconstruction requise."
            )
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

    # -- application --------------------------------------------------------
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
            self, "Services et pilotes",
            f"Appliquer ces réglages ?\n\n{details}\n\n"
            "Wi-Fi : au redémarrage. Vulkan et atuin : nouvelle session.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        changes = {key: self.checks[key].isChecked() for key in keys}
        dialog = LocalTaskDialog(
            self, title="Services et pilotes",
            subtitle="Journal des opérations.",
            argv=["/usr/bin/bash", "-c", system_apply_script(changes)],
        )
        start_task(self, dialog)
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
