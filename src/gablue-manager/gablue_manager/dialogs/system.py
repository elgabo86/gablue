"""Services, pilotes et variante de la machine courante (ex-recettes ujust).
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QVBoxLayout,
)

from ..constants import REBASE_VARIANTS
from ..local import (
    docker_available, local_variant, rebase_script,
    system_apply_script, system_choices, system_helpers_available,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height, restyle_label

# Libellé de la case, puis description affichée dessous.
CHOICE_LABELS = {
    "displaylink": (
        "Prise en charge DisplayLink",
        "Démarre displaylink.service au boot : nécessaire pour les stations "
        "d'accueil et adaptateurs USB qui ajoutent des écrans. Inutile sans "
        "matériel DisplayLink, le service reste alors arrêté.",
    ),
    "docker": (
        "Docker",
        "Démarre docker.service au boot (conteneurs et images Docker). "
        "Réservé aux variantes DX de Gablue, qui embarquent le paquet.",
    ),
    "low_latency": (
        "Couche Vulkan low-latency",
        "Exporte LOW_LATENCY_LAYER=1 dans toutes les sessions (fichier "
        "/etc/profile.d/vulkan-low-latency.sh) : moins de latence dans les "
        "jeux compatibles. Une nouvelle session est nécessaire pour "
        "l'appliquer.",
    ),
    "broadcom_wl": (
        "Pilote Wi-Fi Broadcom wl",
        "Pour les vieux MacBooks (BCM4360) : blackliste les pilotes libres "
        "b43, bcma et brcmfmac au profit du pilote propriétaire wl. "
        "Redémarrage requis dans les deux sens.",
    ),
    "atuin": (
        "Historique atuin partagé",
        "Installe ble.sh et branche atuin dans ~/.bashrc (ex-`ujust "
        "atuin-setup`). La désactivation retire ce branchement et ble.sh, "
        "mais conserve l'historique (~/.local/share/atuin) et la "
        "configuration (~/.config/atuin).",
    ),
}

# Mot affiché dans la confirmation, selon l'état de la case.
CHOICE_STATE = {
    "displaylink": {True: "activé", False: "désactivé"},
    "docker": {True: "activé", False: "désactivé"},
    "low_latency": {True: "activée", False: "désactivée"},
    "broadcom_wl": {True: "activé", False: "désactivé"},
    "atuin": {True: "activée", False: "désactivée"},
}

# Valeur recommandée pour la machine : services inutiles éteints, confort
# Gablue allumé.
CHOICE_RECOMMENDED = {
    "displaylink": False,   # seulement avec du matériel DisplayLink
    "low_latency": True,    # réglage Gablue
    "broadcom_wl": False,   # matériel ancien et spécifique
    "atuin": True,          # historique partagé, déjà en place sur l'image
}

class SystemDialog(QDialog):
    """Services, pilotes et variante : des cases, un seul « Appliquer ».

    Les bascules passent par les helpers `/usr/libexec` (sudoers NOPASSWD) en
    **une seule** opération ; l'intégration atuin, elle, ne touche que les
    fichiers de l'utilisateur. La variante est un choix séparé : elle rebase
    l'image et ne prend effet qu'au redémarrage, donc elle a son propre bouton
    et sa propre confirmation.
    """

    ROWS = ("displaylink", "docker", "low_latency", "broadcom_wl", "atuin")

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Services et variante")
        self.setModal(True)
        self.setMinimumWidth(680)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Services et variante", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Réglages de cette machine, ex-recettes `ujust` : services, couche "
            "Vulkan, pilote Wi-Fi et historique atuin. Les changements partent "
            "ensemble ; la variante se bascule à part et demande un "
            "redémarrage.",
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

        self._build_variant_row(root_layout)

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

        check = QCheckBox(label, frame)
        check.setCursor(Qt.PointingHandCursor)
        check.toggled.connect(self._update_apply)
        layout.addWidget(check)
        self.checks[key] = check

        detail = QLabel(description, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        if key in CHOICE_RECOMMENDED:
            recommendation = QLabel(frame)
            recommendation.setObjectName("hint")
            layout.addWidget(recommendation)
            self.recommendations[key] = recommendation
        layout.addStretch(1)

        self.frames[key] = frame
        self.rows_layout.addWidget(frame)

    def _build_variant_row(self, root_layout: QVBoxLayout) -> None:
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        title = QLabel("Variante de Gablue", frame)
        title.setObjectName("hostName")
        layout.addWidget(title)

        detail = QLabel(
            "Bascule l'image vers une autre variante (standard, DX, NVIDIA) : "
            "le nouveau déploiement est téléchargé puis prend effet au "
            "redémarrage. Vos fichiers personnels sont conservés.",
            frame,
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        row = QHBoxLayout()
        row.setSpacing(10)
        self.variant_combo = QComboBox(frame)
        self.variant_combo.setCursor(Qt.PointingHandCursor)
        for variant, label in REBASE_VARIANTS:
            self.variant_combo.addItem(label, variant)
        self.variant_combo.currentIndexChanged.connect(self._update_rebase)
        row.addWidget(self.variant_combo, 1)

        self.current_label = QLabel(frame)
        self.current_label.setObjectName("hint")
        row.addWidget(self.current_label)

        self.rebase_button = QPushButton("Basculer", frame)
        self.rebase_button.setObjectName("ghost")
        self.rebase_button.setCursor(Qt.PointingHandCursor)
        self.rebase_button.clicked.connect(self.rebase)
        row.addWidget(self.rebase_button)
        layout.addLayout(row)

        self.variant_frame = frame
        root_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        helpers = system_helpers_available()
        variant = local_variant()
        self.state = {
            "ok": helpers,
            "variant": variant,
            "choices": system_choices() if helpers else {},
        }
        choices = self.state["choices"]
        self.initial = dict(choices)
        # Docker n'existe que sur une variante DX : ailleurs, la case n'est pas
        # affichée du tout (pas de paquet, pas de service).
        docker_ok = docker_available(variant)
        self.frames["docker"].setVisible(docker_ok)
        for key, check in self.checks.items():
            check.blockSignals(True)
            check.setChecked(bool(choices.get(key)))
            check.blockSignals(False)
            check.setEnabled(helpers)
        self._update_variant()
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
        for key, label in self.recommendations.items():
            recommended = CHOICE_RECOMMENDED[key]
            differs = ok and self.checks[key].isChecked() != recommended
            text = f"Recommandé : {CHOICE_STATE[key][recommended]}"
            if differs:
                text += " — votre choix diffère"
            label.setText(text)
            restyle_label(label, "warn" if differs else "hint")

    def _update_variant(self) -> None:
        current = self.state.get("variant", "")
        index = self.variant_combo.findData(current)
        if index >= 0:
            self.variant_combo.blockSignals(True)
            self.variant_combo.setCurrentIndex(index)
            self.variant_combo.blockSignals(False)
            self.current_label.setText("variante actuelle")
        else:
            # Image non Gablue ou format inconnu : ne pas laisser croire qu'une
            # variante est sélectionnée.
            self.variant_combo.blockSignals(True)
            self.variant_combo.setCurrentIndex(-1)
            self.variant_combo.blockSignals(False)
            self.current_label.setText("variante inconnue")
        self._update_rebase()

    def _update_rebase(self) -> None:
        current = self.state.get("variant", "")
        choice = self.variant_combo.currentData()
        available = bool(self.state.get("ok"))
        self.rebase_button.setEnabled(
            available and bool(choice) and choice != current
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
            "Les services et la couche Vulkan prennent effet tout de suite "
            "(nouvelle session pour le shell), le pilote Wi-Fi au "
            "redémarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        changes = {key: self.checks[key].isChecked() for key in keys}
        dialog = LocalTaskDialog(
            self, title="Services et pilotes",
            subtitle="Helpers Gablue — journal des opérations.",
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

    def rebase(self) -> None:
        variant = self.variant_combo.currentData()
        current = self.state.get("variant", "")
        if not variant or variant == current:
            return
        label = self.variant_combo.currentText()
        answer = QMessageBox.question(
            self, "Changer de variante",
            f"Basculer cette machine vers « {label} » ?\n\n"
            "L'image sera téléchargée puis le nouveau déploiement prendra "
            "effet au redémarrage. Vos fichiers personnels sont conservés, "
            "mais la session en cours ne change pas.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title=f"Basculer vers {variant}",
            subtitle="Téléchargement de l'image — redémarrage ensuite.",
            argv=["/usr/bin/bash", "-c", rebase_script(variant)],
        )
        start_task(self, dialog)
        self.accept()
