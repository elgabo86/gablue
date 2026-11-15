"""Partage local : visibilité et permanence du partage Gablue Share.

Les dossiers se choisissent dans Gablue Share (c'est son métier) ; ici on règle
ce qui se décide au niveau du poste :

- **visible sur** : réseau local, tailnet, ou les deux — écrit dans
  `share.json`, appliqué par le démon (qui relit le fichier à chaud) ;
- **démarrage** : jamais, à l'ouverture de session, ou dès le boot du poste
  (unité `systemd --user` + `loginctl enable-linger`).

La vérité de la permanence est l'état systemd, jamais un champ du fichier ; la
visibilité, elle, n'existe que dans le fichier — c'est le démon qui la fait
respecter.
"""

import json
import os
import pwd
import shutil
import subprocess
import tempfile
from pathlib import Path

from PySide6.QtCore import QProcess
from PySide6.QtWidgets import (
    QComboBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from ..uiutil import fit_dialog_height

UNIT = "gablue-share.service"
CONFIG = Path.home() / ".config" / "gablue" / "share.json"

#: Qui peut voir le poste. Le démon applique la règle, on ne fait que l'écrire.
VISIBILITIES = {
    "both": "Réseau local et Tailscale",
    "lan": "Réseau local seulement",
    "tailscale": "Tailscale seulement",
}

#: Quand le service tourne sans la fenêtre.
MODES = {
    "off": "Non",
    "session": "Avec la session",
    "boot": "Au démarrage du poste",
}


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


def _loginctl(*arguments: str) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            ["loginctl", *arguments],
            capture_output=True, text=True, timeout=15, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return subprocess.CompletedProcess(["loginctl"], 1, "", "loginctl injoignable")


def _user() -> str:
    try:
        return pwd.getpwuid(os.getuid()).pw_name
    except KeyError:
        return os.environ.get("USER", "")


def _document() -> dict:
    """Contenu de `share.json` (vide s'il n'existe pas encore)."""
    try:
        document = json.loads(CONFIG.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return document if isinstance(document, dict) else {}


def _shares() -> int:
    """Nombre de dossiers partagés, lu dans le fichier de Gablue Share."""
    shares = _document().get("shares")
    return len(shares) if isinstance(shares, list) else 0


def _visibility() -> str:
    value = str(_document().get("visibility") or "both").strip().lower()
    return value if value in VISIBILITIES else "both"


def _write_visibility(value: str) -> tuple[bool, str]:
    """Écrit la visibilité sans toucher au reste (atomic : tmp + replace)."""
    if value not in VISIBILITIES:
        return False, "Visibilité inconnue."
    document = _document()
    document["visibility"] = value
    try:
        CONFIG.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        handle, temporary = tempfile.mkstemp(dir=str(CONFIG.parent), prefix=".share-", suffix=".json")
        with os.fdopen(handle, "w", encoding="utf-8") as stream:
            json.dump(document, stream, ensure_ascii=False, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, CONFIG)
    except OSError as error:
        return False, f"Écriture impossible : {error}"
    return True, ""


def _linger() -> bool:
    """Le gestionnaire systemd de l'utilisateur démarre-t-il dès le boot ?"""
    result = _loginctl("show-user", _user(), "--property=Linger", "--value")
    return result.returncode == 0 and result.stdout.strip() == "yes"


def _mode() -> str:
    if _systemctl("is-enabled", UNIT).returncode != 0:
        return "off"
    return "boot" if _linger() else "session"


def _set_mode(mode: str) -> tuple[bool, str]:
    if mode not in MODES:
        return False, "Mode inconnu."
    # `enable`/`disable` **sans `--now`** : la permanence dit quand le serveur
    # redémarre, pas ce qu'il fait maintenant. Le démon en cours continue de
    # servir — « Quitter le partage » (icône de zone de notification) l'arrête.
    # Même règle que `gablue_share.service.set_mode`, pour que les deux portes
    # d'entrée fassent la même chose de la même unité.
    if mode == "off":
        # Linger n'est pas touché : d'autres services de l'utilisateur peuvent
        # en dépendre, et sans notre unité il ne partage rien.
        result = _systemctl("disable", UNIT)
        if result.returncode != 0:
            return False, (result.stderr or result.stdout).strip() or "désactivation impossible"
        return True, ""
    result = _systemctl("enable", UNIT)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip() or "activation impossible"
    if mode == "boot":
        result = _loginctl("enable-linger", _user())
        if result.returncode != 0 or not _linger():
            return False, (result.stderr or result.stdout).strip() or "linger impossible"
        return True, ""
    # « session » : retirer le démarrage au boot, sinon le choix ne tiendrait
    # pas au prochain redémarrage.
    if _linger():
        result = _loginctl("disable-linger", _user())
        if result.returncode != 0:
            return False, (result.stderr or result.stdout).strip() or "linger non désactivé"
    return True, ""


class ShareDialog(QDialog):
    """Visibilité et permanence du partage local."""

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
            "Dossiers partagés avec les autres postes Gablue. Aucun mot de "
            "passe : seuls les postes que vous autorisez voient le partage, en "
            "lecture seule.",
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

        visibilite = QHBoxLayout()
        visibilite.addWidget(QLabel("Visible sur", card))
        self.visibility = QComboBox(card)
        for cle, libelle in VISIBILITIES.items():
            self.visibility.addItem(libelle, cle)
        self.visibility.setToolTip(
            "Réseau local : les postes du même réseau (annonce mDNS).\n"
            "Tailscale : le tailnet seulement, sans annonce locale.\n"
            "Les deux : la connexion la plus rapide l'emporte."
        )
        self.visibility.currentIndexChanged.connect(self._apply_visibility)
        visibilite.addWidget(self.visibility, 1)
        card_layout.addLayout(visibilite)

        demarrage = QHBoxLayout()
        demarrage.addWidget(QLabel("Démarrage", card))
        self.permanent = QComboBox(card)
        for cle, libelle in MODES.items():
            self.permanent.addItem(libelle, cle)
        self.permanent.setToolTip(
            "Quand le partage reste actif sans la fenêtre :\n"
            "· Avec la session : service systemd utilisateur, lancé à "
            "l'ouverture de session ;\n"
            "· Au démarrage du poste : idem, plus « linger » — le service "
            "tourne dès le boot, sans ouvrir de session."
        )
        self.permanent.currentIndexChanged.connect(self._apply_mode)
        demarrage.addWidget(self.permanent, 1)
        card_layout.addLayout(demarrage)

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
        self.visibility.setEnabled(bool(_document()))
        self.visibility.setToolTip(
            self.visibility.toolTip()
            + ("\nLancez Gablue Share une fois pour créer le fichier de partages."
               if not _document() else "")
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
        index = self.permanent.findData(_mode())
        if index >= 0:
            self.permanent.setCurrentIndex(index)
        self.permanent.blockSignals(False)
        self.visibility.blockSignals(True)
        index = self.visibility.findData(_visibility())
        if index >= 0:
            self.visibility.setCurrentIndex(index)
        self.visibility.blockSignals(False)
        count = _shares()
        accord = "s" if count > 1 else ""
        dossiers = f"{count} dossier{accord} partagé{accord}"
        if not count:
            dossiers = "aucun dossier partagé (invisible des autres postes)"
        service = "en marche" if active else ("activé, arrêté" if enabled else "arrêté")
        if _linger():
            service += ", démarré au boot"
        ou = {
            "both": "réseau local et tailnet",
            "lan": "réseau local",
            "tailscale": "tailnet",
        }[_visibility()]
        self.state.setText(f"Service : {service} · {dossiers} · visible sur le {ou}.")

    # --- actions --------------------------------------------------------
    def _apply_mode(self, _index: int) -> None:
        mode = self.permanent.currentData()
        if not mode:
            return
        ok, message = _set_mode(mode)
        if not ok:
            QMessageBox.warning(self, "Partage local", message or "Opération impossible.")
        self._refresh()

    def _apply_visibility(self, _index: int) -> None:
        value = self.visibility.currentData()
        if not value:
            return
        ok, message = _write_visibility(value)
        if not ok:
            QMessageBox.warning(self, "Partage local", message or "Écriture impossible.")
        self._refresh()

    def _open_app(self) -> None:
        binary = _binary("gablue-share")
        if binary:
            QProcess.startDetached(binary, [])
            self.accept()
