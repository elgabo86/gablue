"""Préférences de Gablue Manager : barre d'état et vérification des mises à jour.

Tout est local à l'utilisateur (`~/.config/gablue/preferences.json`) : la
barre d'état n'est pas imposée, et les vérifications de mises à jour — qui
touchent le réseau — se coupent ou se limitent à certains domaines.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QPushButton, QSpinBox,
    QVBoxLayout,
)

from .. import prefs
from ..uiutil import fit_dialog_height

DOMAIN_LABELS = {
    "image": ("Image Gablue", "Interroge le registre (quelques kilo-octets)."),
    "flatpak": ("Flatpaks", "Applications Flatpak système et du compte."),
    "brew": ("Homebrew", "Formules installées par brew."),
    "gwine": ("Composants gwine", "Runner, DXVK, VKD3D, D7VK, NVAPI."),
}

class PrefsDialog(QDialog):
    """Cases et fréquence ; rien n'est appliqué avant « Appliquer »."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Préférences")
        self.setModal(True)
        self.setMinimumWidth(640)

        self.initial = prefs.load_prefs()

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Préférences", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Réglages de cette session utilisateur. La barre d'état et les "
            "vérifications sont indépendantes : couper l'une ne coupe pas "
            "l'autre.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        # -- barre d'état ---------------------------------------------------
        tray_card = self._card(self)
        tray_layout = QVBoxLayout(tray_card)
        tray_layout.setContentsMargins(16, 12, 16, 12)
        tray_layout.setSpacing(6)
        self.tray_check = QCheckBox("Icône dans la barre d'état", tray_card)
        self.tray_check.setCursor(Qt.PointingHandCursor)
        self.tray_check.setChecked(bool(self.initial.get("tray", True)))
        tray_layout.addWidget(self.tray_check)
        tray_detail = QLabel(
            "Accès rapide au manager à l'ouverture de session. Une pastille "
            "rouge signale un redémarrage requis ou une mise à jour.",
            tray_card,
        )
        tray_detail.setObjectName("hostDetail")
        tray_detail.setWordWrap(True)
        tray_layout.addWidget(tray_detail)
        root_layout.addWidget(tray_card)

        # -- vérifications --------------------------------------------------
        check_card = self._card(self)
        check_layout = QVBoxLayout(check_card)
        check_layout.setContentsMargins(16, 12, 16, 12)
        check_layout.setSpacing(6)
        self.auto_check = QCheckBox(
            "Vérifier les mises à jour automatiquement", check_card
        )
        self.auto_check.setCursor(Qt.PointingHandCursor)
        self.auto_check.setChecked(bool(self.initial.get("check_auto", True)))
        self.auto_check.toggled.connect(self._update_enabled)
        check_layout.addWidget(self.auto_check)

        self.domain_checks: dict[str, QCheckBox] = {}
        for domain, (label, description) in DOMAIN_LABELS.items():
            check = QCheckBox(label, check_card)
            check.setCursor(Qt.PointingHandCursor)
            check.setChecked(bool(self.initial.get(f"check_{domain}", True)))
            check_layout.addWidget(check)
            detail = QLabel(description, check_card)
            detail.setObjectName("hostDetail")
            detail.setWordWrap(True)
            detail.setContentsMargins(26, 0, 0, 0)
            check_layout.addWidget(detail)
            self.domain_checks[domain] = check

        interval_row = QHBoxLayout()
        interval_row.setSpacing(10)
        interval_label = QLabel("Fréquence", check_card)
        interval_label.setObjectName("hostName")
        interval_row.addWidget(interval_label)
        interval_row.addStretch(1)
        self.interval_spin = QSpinBox(check_card)
        low, high = prefs.INTERVAL_RANGE
        self.interval_spin.setRange(low, high)
        self.interval_spin.setSingleStep(15)
        self.interval_spin.setSuffix(" minutes")
        self.interval_spin.setValue(int(
            self.initial.get("check_interval", prefs.DEFAULTS["check_interval"])
        ))
        interval_row.addWidget(self.interval_spin)
        check_layout.addLayout(interval_row)
        root_layout.addWidget(check_card)

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
        self.apply_button.clicked.connect(self.apply)
        buttons.addWidget(self.apply_button)
        root_layout.addLayout(buttons)

        self.tray_check.toggled.connect(self._update_apply)
        self.auto_check.toggled.connect(self._update_apply)
        for check in self.domain_checks.values():
            check.toggled.connect(self._update_apply)
        self.interval_spin.valueChanged.connect(self._update_apply)

        self._update_enabled()
        self._update_apply()
        fit_dialog_height(self, 640)

    @staticmethod
    def _card(parent) -> QFrame:
        frame = QFrame(parent)
        frame.setObjectName("hostCard")
        return frame

    # -- état ---------------------------------------------------------------
    def _update_enabled(self) -> None:
        """Domaines et fréquence ne servent à rien sans surveillance."""
        auto = self.auto_check.isChecked()
        for check in self.domain_checks.values():
            check.setEnabled(auto)
        self.interval_spin.setEnabled(auto)
        if auto:
            self.hint_label.setText("")
        else:
            self.hint_label.setText(
                "Vérification automatique coupée — le bouton « Vérifier les "
                "mises à jour » reste disponible."
            )

    def values(self) -> dict:
        values = dict(self.initial)
        values["tray"] = self.tray_check.isChecked()
        values["check_auto"] = self.auto_check.isChecked()
        for domain, check in self.domain_checks.items():
            values[f"check_{domain}"] = check.isChecked()
        values["check_interval"] = self.interval_spin.value()
        return values

    def changed(self) -> bool:
        """Un réglage diffère-t-il de ce qui est enregistré ?"""
        return self.values() != self.initial

    def _update_apply(self) -> None:
        changes = self.changed()
        self.apply_button.setEnabled(changes)
        if self.auto_check.isChecked():
            self.hint_label.setText(
                "Changements non appliqués." if changes else ""
            )

    def apply(self) -> None:
        # Rien de modifié : ne rien écrire et surtout ne relancer personne.
        # Le tray était auparavant démarré à chaque « Appliquer » ; avec une
        # image antérieure, le binaire du PATH ignore `--tray` et ouvre une
        # seconde fenêtre Gablue Manager (vécu 07/11/2026).
        if not self.changed():
            return
        self.accept()
