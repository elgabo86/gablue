"""Ajout / modification d'un poste et choix des candidats.
"""

import getpass
import time
import uuid

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QHBoxLayout, QLabel, QLineEdit,
    QListWidget, QListWidgetItem, QPushButton, QSpinBox, QVBoxLayout,
    QWidget,
)

from ..hosts import detect_mac
from ..util import display_name, normalize_mac

class HostDialog(QDialog):
    """Saisie d'un poste ; en ajout, la clé SSH est copiée automatiquement."""

    def __init__(self, parent=None, host: dict | None = None,
                 existing: list[dict] | None = None):
        super().__init__(parent)
        self._host = host
        self._existing = existing or []
        self._password = ""

        self.setWindowTitle(
            "Modifier le poste" if host else "Ajouter un poste SSH"
        )
        self.setModal(True)
        self.setMinimumWidth(480)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(12)

        title = QLabel(
            "Modifier le poste" if host else "Ajouter un poste SSH"
        )
        title.setObjectName("title")
        root.addWidget(title)

        subtitle = QLabel(
            "La clé publique est copiée sur la machine dès l'ajout, pour "
            "éviter le mot de passe à chaque connexion. Si elle est déjà en "
            "place, aucun mot de passe n'est demandé : laissez le champ vide."
            if not host else
            "Le mot de passe et la clé ne sont pas modifiés ici."
        )
        subtitle.setObjectName("subtitle")
        subtitle.setWordWrap(True)
        root.addWidget(subtitle)
        root.addSpacing(6)

        form = QVBoxLayout()
        form.setSpacing(10)

        form.addWidget(self._field_label("Nom (facultatif)"))
        self.label_edit = QLineEdit(self)
        self.label_edit.setPlaceholderText("Salon, atelier, portable…")
        form.addWidget(self.label_edit)

        form.addWidget(self._field_label("Utilisateur"))
        self.user_edit = QLineEdit(self)
        self.user_edit.setPlaceholderText("gab")
        form.addWidget(self.user_edit)

        form.addWidget(self._field_label("Hôte ou adresse IP"))
        self.host_edit = QLineEdit(self)
        self.host_edit.setPlaceholderText("sucina.local ou 192.168.1.42")
        form.addWidget(self.host_edit)

        form.addWidget(self._field_label("Port SSH"))
        self.port_spin = QSpinBox(self)
        self.port_spin.setRange(1, 65535)
        self.port_spin.setValue(22)
        self.port_spin.setMaximumWidth(140)
        form.addWidget(self.port_spin)

        form.addWidget(self._field_label(
            "Adresse MAC (Wake-on-LAN, facultatif)"
        ))
        mac_row = QHBoxLayout()
        mac_row.setSpacing(8)
        self.mac_edit = QLineEdit(self)
        self.mac_edit.setPlaceholderText("AA:BB:CC:DD:EE:FF")
        mac_row.addWidget(self.mac_edit, 1)
        self.mac_button = QPushButton("Détecter", self)
        self.mac_button.setObjectName("ghost")
        self.mac_button.setCursor(Qt.PointingHandCursor)
        self.mac_button.clicked.connect(self._detect_mac)
        mac_row.addWidget(self.mac_button)
        form.addLayout(mac_row)
        self.mac_hint = QLabel(
            "Capturée automatiquement au premier contact réussi ; utile pour "
            "réveiller un poste éteint.", self,
        )
        self.mac_hint.setObjectName("hint")
        self.mac_hint.setWordWrap(True)
        form.addWidget(self.mac_hint)

        if not host:
            self.password_label = self._field_label(
                "Mot de passe SSH (si nécessaire)"
            )
            self.password_edit = QLineEdit(self)
            self.password_edit.setEchoMode(QLineEdit.Password)
            self.password_edit.setPlaceholderText(
                "utilisé pour copier la clé, jamais enregistré"
            )
            form.addWidget(self.password_label)
            form.addWidget(self.password_edit)
        else:
            # Aucun champ mot de passe en modification : un widget enfant non
            # ajouté au layout s'afficherait quand même dans le coin du dialogue.
            self.password_edit = None
        root.addLayout(form)

        hint = QLabel(
            "Le mot de passe ne sert qu'à déposer la clé publique dans "
            "~/.ssh/authorized_keys du poste (ssh-copy-id). Il n'est écrit nulle part."
        )
        hint.setObjectName("hint")
        hint.setWordWrap(True)
        if not host:
            root.addWidget(hint)

        self.error_label = QLabel(self)
        self.error_label.setObjectName("error")
        self.error_label.setWordWrap(True)
        self.error_label.setVisible(False)
        root.addWidget(self.error_label)

        root.addSpacing(4)

        buttons = QHBoxLayout()
        buttons.setSpacing(8)
        buttons.addStretch(1)
        cancel = QPushButton("Annuler", self)
        cancel.setObjectName("ghost")
        cancel.clicked.connect(self.reject)
        buttons.addWidget(cancel)

        if host:
            save = QPushButton("Enregistrer", self)
            save.setObjectName("accent")
            save.setDefault(True)
            save.clicked.connect(lambda: self._submit(False))
            buttons.addWidget(save)
        else:
            # Un seul bouton : la copie de la clé fait partie de l'ajout. Le
            # dialogue de clé vérifie d'abord si elle est déjà acceptée, et ne
            # demande un mot de passe que dans le cas contraire.
            self.key_button = QPushButton("Ajouter", self)
            self.key_button.setObjectName("accent")
            self.key_button.setDefault(True)
            self.key_button.clicked.connect(self._submit)
            buttons.addWidget(self.key_button)
        root.addLayout(buttons)

        if host:
            self.label_edit.setText(host.get("label", ""))
            self.user_edit.setText(host["user"])
            self.host_edit.setText(host["host"])
            self.port_spin.setValue(host["port"])
            self.mac_edit.setText(host.get("mac", ""))
        else:
            self.user_edit.setText(getpass.getuser())
        self.host_edit.textChanged.connect(self._sync_placeholder)
        self._sync_placeholder(self.host_edit.text())

    @staticmethod
    def _field_label(text: str) -> QLabel:
        label = QLabel(text)
        label.setObjectName("hostDetail")
        return label

    def _sync_placeholder(self, text: str) -> None:
        if not self.label_edit.text():
            self.label_edit.setPlaceholderText(text.strip() or "Salon, atelier…")

    def _detect_mac(self) -> None:
        address = self.host_edit.text().strip()
        if not address:
            self.mac_hint.setText("Indiquez d'abord l'hôte.")
            return
        mac = detect_mac({"host": address})
        if mac:
            self.mac_edit.setText(mac)
            self.mac_hint.setText("Adresse MAC lue dans la table ARP locale.")
        else:
            self.mac_hint.setText(
                "Introuvable pour l'instant : le poste doit avoir communiqué "
                "récemment sur le réseau. Elle sera capturée à la prochaine "
                "connexion réussie."
            )

    def _show_error(self, message: str) -> None:
        self.error_label.setText(message)
        self.error_label.setVisible(True)

    def _submit(self) -> None:
        self.error_label.setVisible(False)
        host = self.host_edit.text().strip()
        user = self.user_edit.text().strip()
        label = self.label_edit.text().strip()
        port = int(self.port_spin.value())
        mac_text = self.mac_edit.text().strip()
        mac = normalize_mac(mac_text)

        if not host:
            return self._show_error("Indiquez l'hôte ou l'adresse IP du poste.")
        if any(ch.isspace() for ch in host) or "/" in host:
            return self._show_error("L'hôte ne doit contenir ni espace ni « / ».")
        if "@" in host:
            return self._show_error(
                "Mettez l'utilisateur dans le champ « Utilisateur », pas dans l'hôte."
            )
        if not user:
            return self._show_error("Indiquez le nom d'utilisateur SSH.")
        if any(ch.isspace() for ch in user) or any(ch in user for ch in "@:/"):
            return self._show_error("Nom d'utilisateur invalide.")
        if mac_text and not mac:
            return self._show_error(
                "Adresse MAC invalide (format attendu : AA:BB:CC:DD:EE:FF)."
            )

        for other in self._existing:
            if self._host and other["id"] == self._host["id"]:
                continue
            if (other["user"], other["host"], other["port"]) == (user, host, port):
                return self._show_error(
                    f'{display_name(other)} utilise déjà {user}@{host}:{port}.'
                )

        self._password = (
            self.password_edit.text() if self.password_edit is not None else ""
        )
        self.result_record = {
            "id": (self._host or {}).get("id") or uuid.uuid4().hex,
            "label": label,
            "user": user,
            "host": host,
            "port": port,
            "mac": mac,
            "key_added": bool((self._host or {}).get("key_added", False)),
            "added_at": int((self._host or {}).get("added_at") or time.time()),
        }
        self.accept()

    def password(self) -> str:
        return self._password

class CandidateDialog(QDialog):
    """Liste à cocher de postes à ajouter (découverte mDNS ou ssh_config).

    Une ligne par **machine** : Avahi annonce une adresse par interface (LAN,
    Tailscale, pont docker…), le filtrage et le regroupement sont faits en
    amont (`hosts.group_candidates`) ; quand plusieurs adresses joignables
    restent, une liste déroulante choisit celle qu'on enregistre.
    """

    KIND_LABELS = {"lan": "LAN", "tailscale": "Tailscale"}

    def __init__(self, parent=None, title: str = "", subtitle: str = "",
                 candidates: list[dict] | None = None,
                 existing: list[dict] | None = None):
        super().__init__(parent)
        known = {
            (host["user"], host["host"], host["port"]) for host in (existing or [])
        }

        def already_known(item: dict) -> bool:
            addresses = [address for address, _kind in item.get("hosts", [])] or [
                item["host"]
            ]
            return any(
                (item["user"], address, item["port"]) in known
                for address in addresses
            )

        self._candidates = [
            item for item in (candidates or []) if not already_known(item)
        ]
        self.selected: list[dict] = []

        self.setWindowTitle(title or "Postes à ajouter")
        self.setModal(True)
        self.setMinimumWidth(580)
        self.setMinimumHeight(420)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel(title or "Postes à ajouter", self)
        heading.setObjectName("title")
        root.addWidget(heading)

        detail = QLabel(subtitle, self)
        detail.setObjectName("subtitle")
        detail.setWordWrap(True)
        root.addWidget(detail)

        self.list_widget = QListWidget(self)
        self.list_widget.setSpacing(2)
        for candidate in self._candidates:
            self._add_row(candidate)
        root.addWidget(self.list_widget, 1)

        tools = QHBoxLayout()
        tools.setSpacing(8)
        self.all_button = QPushButton("Tout cocher", self)
        self.all_button.setObjectName("ghost")
        self.all_button.setCursor(Qt.PointingHandCursor)
        self.all_button.setEnabled(bool(self._candidates))
        self.all_button.clicked.connect(self._toggle_all)
        tools.addWidget(self.all_button)
        tools.addStretch(1)
        self.count_label = QLabel(self)
        self.count_label.setObjectName("hint")
        tools.addWidget(self.count_label)
        root.addLayout(tools)

        self.note = QLabel(
            "La clé SSH est copiée sur chaque poste ajouté ; le mot de passe "
            "n'est demandé que s'il la refuse encore. Une adresse LAN est "
            "préférée à une adresse Tailscale quand les deux existent."
            if self._candidates else
            "Aucun nouveau poste à ajouter : les adresses de cette machine "
            "sont écartées, et un poste déjà enregistré ne s'affiche pas.",
            self,
        )
        self.note.setObjectName("hint")
        self.note.setWordWrap(True)
        root.addWidget(self.note)

        buttons = QHBoxLayout()
        buttons.addStretch(1)
        cancel = QPushButton("Annuler", self)
        cancel.setObjectName("ghost")
        cancel.clicked.connect(self.reject)
        buttons.addWidget(cancel)

        self.add_button = QPushButton("Ajouter", self)
        self.add_button.setObjectName("accent")
        self.add_button.setDefault(True)
        self.add_button.clicked.connect(self._accept)
        buttons.addWidget(self.add_button)
        root.addLayout(buttons)

        self._sync_selection()

    # -- lignes -------------------------------------------------------------
    def _add_row(self, candidate: dict) -> None:
        addresses = list(candidate.get("hosts") or [(candidate["host"], "")])
        port = int(candidate.get("port", 22))
        text = f'{candidate.get("label") or candidate["host"]} — ' \
               f'{candidate["user"]}'
        if port != 22:
            text += f" (port {port})"

        widget = QWidget(self.list_widget)
        row = QHBoxLayout(widget)
        row.setContentsMargins(8, 4, 8, 4)
        row.setSpacing(12)

        check = QCheckBox(text, widget)
        # Rien n'est coché d'avance : la découverte propose, elle n'ajoute pas.
        check.setCheckState(Qt.Unchecked)
        check.stateChanged.connect(self._sync_selection)
        row.addWidget(check, 1)

        combo = None
        if len(addresses) > 1:
            combo = QComboBox(widget)
            for address, kind in addresses:
                label = self.KIND_LABELS.get(kind, kind)
                combo.addItem(f"{address} ({label})" if label else address,
                              address)
            combo.setToolTip(
                "Adresse enregistrée pour ce poste (la première est choisie "
                "automatiquement)."
            )
            row.addWidget(combo)
        else:
            address, kind = addresses[0]
            label = self.KIND_LABELS.get(kind, kind)
            detail = QLabel(f"{address} ({label})" if label else address, widget)
            detail.setObjectName("hint")
            row.addWidget(detail)

        item = QListWidgetItem(self.list_widget)
        item.setData(Qt.UserRole, {
            "candidate": candidate, "check": check, "combo": combo,
        })
        item.setSizeHint(widget.sizeHint())
        self.list_widget.setItemWidget(item, widget)

    # -- sélection ----------------------------------------------------------
    def _checked(self) -> list[dict]:
        chosen = []
        for index in range(self.list_widget.count()):
            payload = self.list_widget.item(index).data(Qt.UserRole)
            if not payload["check"].isChecked():
                continue
            record = dict(payload["candidate"])
            if payload["combo"] is not None:
                record["host"] = payload["combo"].currentData()
            chosen.append(record)
        return chosen

    def _toggle_all(self) -> None:
        target = Qt.Unchecked if self._checked() else Qt.Checked
        for index in range(self.list_widget.count()):
            payload = self.list_widget.item(index).data(Qt.UserRole)
            payload["check"].setCheckState(target)

    def _sync_selection(self, *_args) -> None:
        count = len(self._checked())
        total = self.list_widget.count()
        self.all_button.setText("Tout décocher" if count == total and total
                                else "Tout cocher")
        self.count_label.setText(
            f"{count} sélectionné{'s' if count > 1 else ''} sur {total}"
        )
        self.add_button.setEnabled(count > 0)
        self.add_button.setText(f"Ajouter ({count})" if count else "Ajouter")

    def _accept(self) -> None:
        chosen = self._checked()
        if not chosen:
            self.note.setText("Cochez au moins un poste.")
            return
        self.selected = chosen
        self.accept()
