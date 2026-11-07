"""Secure Boot : état du poste, clés Gablue et enrôlement au prochain démarrage.

Les modules du noyau Gablue (kmods ublue-os, modules signés localement) ne se
chargent avec Secure Boot que si la clé qui les signe figure dans la liste MOK.
L'enrôlement ne peut pas se faire à chaud : le certificat est d'abord *ajouté
à la demande* (variable EFI `MokNew`), puis le menu MokManager l'applique au
démarrage suivant, avant Linux, avec le mot de passe public « gablue ».

Le dialogue lit l'état par `/usr/libexec/gablue-secureboot status` et programme
l'enrôlement par `… enroll`, les deux en `sudo -n` (règles sudoers exactes,
aucune invite de mot de passe). Il ne redémarre pas : c'est l'utilisateur qui
décide quand, et le menu peut être décliné.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout, QWidget,
)

from ..constants import COLOR_AUTH, COLOR_OFFLINE, COLOR_ONLINE
from ..local import (
    parse_gablue_progress, secure_boot_state, secureboot_enroll_script,
    secureboot_helper_available, secureboot_status,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

# (libellé, couleur) par état de Secure Boot dans le firmware.
SB_STATES = {
    "enabled": ("Secure Boot est activé.", COLOR_ONLINE),
    "disabled": ("Secure Boot est désactivé — à activer dans le firmware.",
                 COLOR_AUTH),
    "unsupported": ("Démarrage BIOS : ce poste n'a pas de Secure Boot.",
                    COLOR_OFFLINE),
}

# (libellé, couleur) par état d'un certificat Gablue.
KEY_STATES = {
    "enrolled": ("enrôlée", COLOR_ONLINE),
    "pending": ("demande en attente", COLOR_AUTH),
    "not-enrolled": ("non enrôlée", COLOR_AUTH),
    "blocked": ("révoquée dans le firmware (dbx)", COLOR_OFFLINE),
    "missing": ("absente de l'image", COLOR_OFFLINE),
    "unknown": ("illisible", COLOR_OFFLINE),
}


class SecureBootDialog(QDialog):
    """État courant + un bouton pour programmer l'enrôlement au prochain boot."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Secure Boot")
        self.setModal(True)
        self.setMinimumWidth(720)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Secure Boot", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Les modules du noyau Gablue sont signés par la clé Gablue. Pour "
            "qu'ils se chargent avec Secure Boot, cette clé doit être enrôlée "
            "dans la liste MOK — l'opération se confirme au démarrage suivant, "
            "avant Linux.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        state_card = QFrame(self)
        state_card.setObjectName("hostCard")
        state_layout = QVBoxLayout(state_card)
        state_layout.setContentsMargins(16, 12, 16, 12)
        state_layout.setSpacing(4)
        self.sb_label = QLabel(self)
        self.sb_label.setObjectName("hostName")
        self.sb_label.setWordWrap(True)
        state_layout.addWidget(self.sb_label)
        self.keys_layout = QVBoxLayout()
        self.keys_layout.setSpacing(2)
        state_layout.addLayout(self.keys_layout)
        root_layout.addWidget(state_card)

        self.steps_label = QLabel(self)
        self.steps_label.setObjectName("hostDetail")
        self.steps_label.setWordWrap(True)
        root_layout.addWidget(self.steps_label)

        root_layout.addStretch(1)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        self.hint_label.setWordWrap(True)
        buttons.addWidget(self.hint_label, 1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        self.apply_button = QPushButton("Programmer l'enrôlement", self)
        self.apply_button.setObjectName("accent")
        self.apply_button.setCursor(Qt.PointingHandCursor)
        self.apply_button.clicked.connect(self.apply_changes)
        buttons.addWidget(self.apply_button)
        root_layout.addLayout(buttons)

        self.status: dict = {}
        self.password = "gablue"
        self.key_rows: list[tuple[QLabel, QLabel]] = []
        self.refresh()
        fit_dialog_height(self, 720)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        available = secureboot_helper_available()
        status = secureboot_status() if available else {}
        self.status = status
        if status.get("password"):
            self.password = status["password"]

        secure_boot = status.get("secure_boot") or secure_boot_state()
        text, color = SB_STATES.get(
            secure_boot, ("État Secure Boot illisible.", COLOR_OFFLINE)
        )
        self.sb_label.setText(text)
        self.sb_label.setStyleSheet(f"color: {color};")

        # Les lignes vivent dans des widgets conteneurs : les supprimer suffit
        # (leurs libellés tombent avec eux) ; `key_rows` garde les libellés
        # d'état pour les tests hors écran.
        self.key_rows = []
        while self.keys_layout.count():
            item = self.keys_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        keys = status.get("keys") or []
        if not keys:
            label = QLabel(
                "Clés Gablue illisibles — helper absent de cette image."
                if not available else
                "Clés Gablue illisibles (privilèges ou certificats).",
                self,
            )
            label.setObjectName("hostDetail")
            label.setWordWrap(True)
            self.keys_layout.addWidget(label)
        for key in keys:
            row = QWidget(self)
            line = QHBoxLayout(row)
            line.setContentsMargins(0, 0, 0, 0)
            line.setSpacing(8)
            name = QLabel(key["label"], row)
            name.setObjectName("hostDetail")
            state_label = QLabel(row)
            state_label.setObjectName("hostStatus")
            state_text, state_color = KEY_STATES.get(
                key["state"], ("illisible", COLOR_OFFLINE)
            )
            state_label.setText(state_text)
            state_label.setStyleSheet(f"color: {state_color};")
            line.addWidget(name, 1)
            line.addWidget(state_label, 0, Qt.AlignRight)
            self.keys_layout.addWidget(row)
            self.key_rows.append((name, state_label))

        self.steps_label.setText(
            "Au prochain démarrage, le menu MokManager s'ouvre avant Gablue :\n"
            "« Enroll MOK » → « Continue » → mot de passe "
            f"« {self.password} ».\n\n"
            "Le menu utilise le clavier du firmware (QWERTY en pratique) et "
            "ignore la disposition choisie dans Gablue : « "
            f"{self.password} » est choisi pour se taper à l'identique en "
            "QWERTY US et en suisse français."
        )
        self._update_apply()

    def can_enroll(self) -> bool:
        return any(
            key["state"] == "not-enrolled"
            for key in self.status.get("keys") or []
        )

    def _update_apply(self) -> None:
        keys = self.status.get("keys") or []
        self.apply_button.setEnabled(self.can_enroll())
        if not secureboot_helper_available():
            self.hint_label.setText(
                "Helper absent de cette image — reconstruction requise."
            )
            return
        if not keys:
            self.hint_label.setText(
                "État illisible : privilèges insuffisants ou certificats absents."
            )
            return
        states = [key["state"] for key in keys]
        if "pending" in states:
            self.hint_label.setText(
                "Demande déjà en attente : elle sera proposée au prochain "
                "démarrage."
            )
        elif "not-enrolled" in states:
            self.hint_label.setText(
                "Clés à enrôler — effet au prochain démarrage."
            )
        elif "blocked" in states:
            self.hint_label.setText(
                "Clé révoquée dans le firmware (dbx) : Secure Boot ne peut "
                "pas la charger."
            )
        elif all(state == "missing" for state in states):
            self.hint_label.setText(
                "Certificats Gablue absents de cette image."
            )
        elif all(state == "enrolled" for state in states):
            self.hint_label.setText("Les clés Gablue sont déjà enrôlées.")
        else:
            self.hint_label.setText("Aucun enrôlement à programmer.")

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        if not self.can_enroll():
            return
        answer = QMessageBox.question(
            self, "Secure Boot",
            "Programmer l'enrôlement des clés Gablue ?\n\n"
            "Au prochain démarrage, le menu MokManager s'ouvrira avant "
            "Gablue :\n"
            f"« Enroll MOK » → « Continue » → mot de passe « {self.password} ».\n\n"
            "Le redémarrage n'est pas lancé ici. La demande peut être "
            "déclinée au menu ; elle sera alors reproposée à chaque démarrage "
            "tant qu'elle n'est ni acceptée ni annulée.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        task = LocalTaskDialog(
            self, title="Secure Boot",
            subtitle="Ajout des clés Gablue à la liste MOK.",
            argv=["/usr/bin/bash", "-c", secureboot_enroll_script()],
            progress_parser=parse_gablue_progress,
            progress_text="Programmation de l'enrôlement…",
            reveal_log_on_error=True,
        )
        start_task(self, task)
        # Le dialogue est modal (`exec()`) : rester ouvert désactiverait la
        # fenêtre de progression, dont « Fermer » deviendrait inerte. Le
        # résultat (demande en attente) se voit en rouvrant l'outil.
        self.accept()
