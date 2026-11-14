"""Partage local : permanence du partage Gablue Share depuis le manager.

Les dossiers se choisissent dans Gablue Share (c'est son métier) ; ici on ne
règle que la permanence : le partage reste actif même application fermée, et
reprend à la prochaine session. La vérité est l'état systemd de l'unité
`gablue-share.service` (session de l'utilisateur, aucun privilège).
"""

import json
import os
import shutil
import subprocess
from pathlib import Path

from PySide6.QtCore import QProcess
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..uiutil import fit_dialog_height

UNIT = "gablue-share.service"
CONFIG = Path.home() / ".config" / "gablue" / "share.json"


def _binary(name: str) -> str | None:
    """Chemin du binaire, ou None s'il n'est pas installé (`_which` rend le nom)."""
    found = shutil.which(name)
    if found:
        return found
    for candidate in (f"/usr/bin/{name}", f"/usr/local/bin/{name}"):
        if os.access(candidate, os.X_OK):
            return candidate
    return None


def _systemctl(*arguments: str) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            ["systemctl", "--user", *arguments],
            capture_output=True, text=True, timeout=15, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return subprocess.CompletedProcess(["systemctl"], 1, "", "systemctl injoignable")


def _shares() -> int:
    """Nombre de dossiers partagés, lu dans le fichier de Gablue Share."""
    try:
        document = json.loads(CONFIG.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return 0
    shares = document.get("shares") if isinstance(document, dict) else None
    return len(shares) if isinstance(shares, list) else 0


class ShareDialog(QDialog):
    """Permanence du partage local ; rien d'autre n'est réglé ici."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Partage local")
        self.setModal(True)
        self.setMinimumWidth(620)
        self.installed = _binary("gablue-share") is not None

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Partage local", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Dossiers partagés avec les autres postes Gablue, sur le réseau "
            "local et le tailnet. Aucun mot de passe : seuls les postes du "
            "même réseau voient le partage, en lecture seule.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        card = QFrame(self)
        card.setObjectName("card")
        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(18, 16, 18, 16)
        card_layout.setSpacing(8)

        self.state = QLabel("", card)
        self.state.setWordWrap(True)
        card_layout.addWidget(self.state)

        self.permanent = QCheckBox(
            "Permanent — le partage reste actif même Gablue Share fermé", card
        )
        self.permanent.setToolTip(
            "Le service démarre avec la session et sert les dossiers partagés "
            "en arrière-plan (environ 3 Mo de mémoire)."
        )
        self.permanent.toggled.connect(self._apply)
        card_layout.addWidget(self.permanent)

        hint = QLabel(
            "Les dossiers à partager se choisissent dans Gablue Share.", card
        )
        hint.setObjectName("hint")
        hint.setWordWrap(True)
        card_layout.addWidget(hint)
        root_layout.addWidget(card, 1)

        buttons = QHBoxLayout()
        buttons.addStretch(1)
        self.open_app = QPushButton("Ouvrir Gablue Share", self)
        self.open_app.setObjectName("primary")
        self.open_app.clicked.connect(self._open_app)
        buttons.addWidget(self.open_app)
        close = QPushButton("Fermer", self)
        close.clicked.connect(self.accept)
        buttons.addWidget(close)
        root_layout.addLayout(buttons)

        if not self.installed:
            self.permanent.setEnabled(False)
            self.open_app.setEnabled(False)
            self.state.setText(
                "Gablue Share n'est pas installé sur ce poste : rien à régler."
            )
        self._refresh()
        fit_dialog_height(self, 620)

    # --- état -----------------------------------------------------------
    def _refresh(self) -> None:
        if not self.installed:
            return
        enabled = _systemctl("is-enabled", UNIT).returncode == 0
        active = _systemctl("is-active", UNIT).returncode == 0
        self.permanent.blockSignals(True)
        self.permanent.setChecked(enabled)
        self.permanent.blockSignals(False)
        count = _shares()
        dossiers = f"{count} dossier partagé" + ("s" if count > 1 else "")
        if not count:
            dossiers = "aucun dossier partagé (invisible des autres postes)"
        service = "en marche" if active else ("activé, arrêté" if enabled else "arrêté")
        self.state.setText(f"Service : {service} · {dossiers}.")

    # --- actions --------------------------------------------------------
    def _apply(self, wanted: bool) -> None:
        result = _systemctl("enable" if wanted else "disable", "--now", UNIT)
        if result.returncode != 0:
            message = (result.stderr or result.stdout).strip()
            QMessageBox.warning(self, "Partage local", message or "Opération impossible.")
            self._refresh()
            return
        self._refresh()

    def _open_app(self) -> None:
        binary = _binary("gablue-share")
        if binary:
            QProcess.startDetached(binary, [])
            self.accept()
