"""Session graphique : celle que plasmalogin ouvre au démarrage.

Trois choix, écrits par `/usr/libexec/gablue-session-mode` — le MÊME helper que
le menu Home de l'overlay manette (`MODE JEU` / `MODE BUREAU` / `FERMER LA
SESSION`), donc un seul mécanisme et un seul fichier :

- « Écran de connexion » retire l'autologin : au démarrage (et après une
  déconnexion), le greeter demande qui se connecte ;
- « Bureau » autologin la session Plasma ;
- « Mode jeu » autologin la session console (KWin nu + launcher + overlay
  manette), ce qui est exactement ce que fait `MODE JEU` depuis le canapé.

Le choix APPLIQUE TOUT DE SUITE : le helper redémarre plasmalogin, donc la
session en cours est arrêtée. L'écran de connexion (ou la session choisie)
revient une seconde plus tard et le réglage survit aux redémarrages suivants.
Gablue Manager meurt avec la session, c'est annoncé avant d'appliquer.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QButtonGroup, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QRadioButton, QVBoxLayout,
)

from ..constants import _which
from ..local import (
    session_mode_available, session_mode_command, session_mode_state,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

CHOIX = (
    (
        "greeter",
        "Écran de connexion",
        "Aucune session automatique : plasmalogin demande qui se connecte, "
        "puis la session choisie dans le menu du greeter.",
    ),
    (
        "bureau",
        "Bureau (Plasma)",
        "Démarre directement sur le bureau : c'est le choix pour un poste "
        "qui sert aussi à tout le reste.",
    ),
    (
        "console",
        "Mode jeu (session console)",
        "Démarre directement sur l'accueil des jeux : KWin nu, lanceur plein "
        "écran et overlay manette, sans bureau dessous.",
    ),
)


class SessionModeDialog(QDialog):
    """La session du prochain démarrage (et de la prochaine déconnexion)."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Session graphique")
        self.setModal(True)
        self.setMinimumWidth(680)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel("Session graphique", self)
        heading.setObjectName("title")
        root.addWidget(heading)

        note = QLabel(
            "La session que Gablue ouvre au démarrage. C'est le même réglage "
            "que MODE JEU / MODE BUREAU du menu Home de la manette : un seul "
            "fichier, aucune divergence entre les deux chemins.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root.addWidget(note)

        card = QFrame(self)
        card.setObjectName("hostCard")
        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(16, 12, 16, 12)
        card_layout.setSpacing(8)

        self.group = QButtonGroup(self)
        self.radios: dict[str, QRadioButton] = {}
        for cle, titre, detail in CHOIX:
            radio = QRadioButton(titre, card)
            radio.setCursor(Qt.PointingHandCursor)
            self.group.addButton(radio)
            card_layout.addWidget(radio)
            sous = QLabel(detail, card)
            sous.setObjectName("hostDetail")
            sous.setWordWrap(True)
            sous.setContentsMargins(24, 0, 0, 6)
            card_layout.addWidget(sous)
            self.radios[cle] = radio
            radio.toggled.connect(lambda _v: self._update_apply())
        root.addWidget(card)

        self.change_note = QLabel("", self)
        self.change_note.setObjectName("hostDetail")
        self.change_note.setWordWrap(True)
        root.addWidget(self.change_note)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint = QLabel(self)
        self.hint.setObjectName("hostStatus")
        buttons.addWidget(self.hint)
        buttons.addStretch(1)
        fermer = QPushButton("Fermer", self)
        fermer.setObjectName("ghost")
        fermer.setCursor(Qt.PointingHandCursor)
        fermer.clicked.connect(self.accept)
        buttons.addWidget(fermer)
        self.apply_button = QPushButton("Appliquer", self)
        self.apply_button.setObjectName("accent")
        self.apply_button.setCursor(Qt.PointingHandCursor)
        self.apply_button.clicked.connect(self.apply_changes)
        buttons.addWidget(self.apply_button)
        root.addLayout(buttons)

        self.disponible = session_mode_available()
        self.initial = ""
        for radio in self.radios.values():
            radio.setEnabled(self.disponible)
        self.apply_button.setEnabled(False)
        self.hint.setText(
            "Appliquer redémarre la session : l'écran de connexion (ou la "
            "session choisie) revient aussitôt."
        )
        self._reload()
        fit_dialog_height(self, 640)

    # -- état ---------------------------------------------------------------
    def _reload(self) -> None:
        self.initial = session_mode_state()
        if self.initial in self.radios:
            self.radios[self.initial].setChecked(True)
        if not self.disponible:
            self.hint.setText(
                "Helper absent (image antérieure) : la session ne peut pas "
                "être réglée ici."
            )
        self.change_note.setText(
            ""
            if self.initial == "greeter"
            else "Au prochain démarrage, aucune demande de mot de passe : la "
            "session choisie s'ouvre directement."
        )
        self._update_apply()

    def chosen(self) -> str:
        for cle, radio in self.radios.items():
            if radio.isChecked():
                return cle
        return ""

    def changed(self) -> bool:
        return self.disponible and self.chosen() not in ("", self.initial)

    def _update_apply(self) -> None:
        if not self.disponible:
            self.apply_button.setEnabled(False)
            return
        self.apply_button.setEnabled(self.chosen() not in ("", self.initial))

    # -- application --------------------------------------------------------
    def apply_changes(self) -> None:
        mode = self.chosen()
        if not self.disponible or not mode or mode == self.initial:
            return
        libelle = next(t for c, t, _d in CHOIX if c == mode)
        answer = QMessageBox.question(
            self, "Changer de session",
            f"Passer à « {libelle} » ?\n\n"
            "La session en cours est arrêtée maintenant : Gablue Manager se "
            "ferme avec elle et la nouvelle session apparaît une seconde plus "
            "tard.\n\n"
            "Le réglage reste en place aux démarrages suivants.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        # pkexec EXPLICITE : la règle polkit de l'image autorise le groupe
        # wheel sans mot de passe pour ce helper exact. `root=True` passerait
        # par `sudo -n`, et le sudoers de l'image n'ouvre pas ce programme — la
        # bascule échouerait silencieusement.
        pkexec = _which("pkexec")
        if pkexec is None:
            QMessageBox.warning(
                self, "Session graphique",
                "PolicyKit (pkexec) est absent : la session ne peut pas être "
                "changée depuis Gablue Manager.",
            )
            return
        dialog = LocalTaskDialog(
            self, title="Session graphique",
            subtitle="Bascule demandée.",
            argv=[pkexec, *session_mode_command(mode)[1:]],
            progress_mode="busy",
            progress_text="Bascule de session…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()
