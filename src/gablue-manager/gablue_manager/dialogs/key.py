"""Copie de la clé SSH vers un poste.

Le poste n'entre dans la liste qu'une fois la clé acceptée **et** l'empreinte
d'hôte épinglée : ce dialogue ne porte donc plus de bouton « Installer la
clé ». Il demande le mot de passe seulement quand il en faut un (Entrée lance
la copie) et démarre seul l'épinglage quand la clé est déjà acceptée.
"""

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QLineEdit, QMessageBox, QPlainTextEdit,
    QPushButton, QVBoxLayout, QWidget,
)

from ..ssh import key_ready
from ..uiutil import restyle_label
from ..util import address_text, display_name
from ..workers import KeyInstallWorker, KeyProbeWorker
from .host import HostDialog

class KeyInstallDialog(QDialog):
    """Copie la clé publique, avec invite de mot de passe et journal."""

    installed = Signal()

    def __init__(self, parent=None, host: dict | None = None,
                 password: str = "", prescan: dict | None = None):
        super().__init__(parent)
        self._host = dict(host or {})
        # Mot de passe éventuellement saisi dans le dialogue d'ajout : il sert
        # à lancer la copie sans redemander quoi que ce soit.
        self._initial_password = password
        self._auto_started = False
        self._worker: KeyInstallWorker | None = None
        self._probe_worker: KeyProbeWorker | None = None
        self._prescan = prescan
        self._success = False
        self._message = ""

        self.setWindowTitle(f"Clé SSH — {display_name(self._host)}")
        self.setModal(True)
        # Assez large pour que l'adresse et la consigne tiennent sur une ligne,
        # et pour que le champ de mot de passe ne soit pas écrasé.
        self.setMinimumWidth(640)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(12)

        self.title = QLabel(f"Copier la clé vers {display_name(self._host)}")
        self.title.setObjectName("title")
        root.addWidget(self.title)

        self.explanation = QLabel(
            f"Votre clé publique sera ajoutée sur {address_text(self._host)}. "
            "Ensuite, plus de mot de passe."
        )
        self.explanation.setObjectName("subtitle")
        self.explanation.setWordWrap(True)
        root.addWidget(self.explanation)
        root.addSpacing(4)

        # Rien n'est demandé tant qu'on ne sait pas si la clé est déjà là : un
        # poste qui l'accepte déjà n'a besoin d'aucun mot de passe, et le
        # champ n'apparaît qu'une fois la vérification terminée.
        self.status_label = QLabel("Vérification de la clé sur le poste…", self)
        self.status_label.setObjectName("hint")
        self.status_label.setWordWrap(True)
        root.addWidget(self.status_label)

        self.form_box = QWidget(self)
        form = QVBoxLayout(self.form_box)
        form.setContentsMargins(0, 0, 0, 0)
        form.setSpacing(6)
        form.addWidget(HostDialog._field_label("Mot de passe SSH"))
        # La consigne est AU-DESSUS du champ : sous le champ, elle se lisait
        # comme une légende des boutons.
        hint = QLabel(
            "Sert une fois, jamais enregistré. Entrée lance la copie.",
            self.form_box,
        )
        hint.setObjectName("hint")
        hint.setWordWrap(True)
        form.addWidget(hint)
        self.password_edit = QLineEdit(self.form_box)
        self.password_edit.setEchoMode(QLineEdit.Password)
        self.password_edit.setMinimumHeight(34)
        self.password_edit.setMinimumWidth(480)
        self.password_edit.setPlaceholderText(
            "mot de passe du compte distant (jamais enregistré)"
        )
        self.password_edit.setText(password)
        form.addWidget(self.password_edit)
        self.form_box.setVisible(False)
        root.addWidget(self.form_box)

        self.result_label = QLabel(self)
        self.result_label.setWordWrap(True)
        self.result_label.setVisible(False)
        root.addWidget(self.result_label)

        self.log_view = QPlainTextEdit(self)
        self.log_view.setReadOnly(True)
        self.log_view.setFixedHeight(150)
        self.log_view.setVisible(False)
        root.addWidget(self.log_view)

        buttons = QHBoxLayout()
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.clicked.connect(self.reject)
        buttons.addWidget(self.close_button)
        root.addLayout(buttons)

        self.password_edit.returnPressed.connect(self._start)
        if self._prescan is not None:
            # Sonde déjà faite par l'appelant (import, carte) : on réutilise
            # son résultat au lieu d'ouvrir une connexion de plus.
            QTimer.singleShot(0, lambda: self._probe_done(self._prescan))
        elif self._initial_password:
            # Le mot de passe vient du dialogue d'ajout : la sonde préalable
            # serait une connexion de plus, donc une offense de plus pour un
            # sshd qui limite les essais. `install_key` vérifie déjà si la clé
            # est acceptée avant de demander quoi que ce soit.
            self.form_box.setVisible(True)
            self.status_label.setText("Copie de la clé en cours…")
            self._auto_started = True
            QTimer.singleShot(0, self._start)
        else:
            QTimer.singleShot(60, self._probe)

    # -- vérification préalable ---------------------------------------------
    def _probe(self) -> None:
        # La vérification est bornée par les délais de ssh (4 s) : laisser la
        # fenêtre se fermer pendant ce temps détruirait le thread en vol.
        self.close_button.setEnabled(False)
        worker = KeyProbeWorker(self._host, self)
        worker.done.connect(self._probe_done)
        self._probe_worker = worker
        worker.start()

    def _probe_done(self, info: dict) -> None:
        self._probe_worker = None
        # Gardé pour l'installation : une seule connexion aura servi à la fois
        # à connaître l'empreinte et à savoir que la clé est refusée.
        self._prescan = info
        self.close_button.setEnabled(True)
        authenticated = bool(info.get("authenticated"))
        if key_ready(info):
            self.title.setText(
                f"Clé déjà installée sur {display_name(self._host)}"
            )
            self.explanation.setVisible(False)
            self.status_label.setText(
                "Clé déjà acceptée et empreinte enregistrée : rien à faire."
            )
            restyle_label(self.status_label, "ok")
            self.form_box.setVisible(False)
            self.close_button.setDefault(True)
            self.close_button.setFocus()
            self._success = True
            self.installed.emit()
            self.adjustSize()
            return
        restyle_label(self.status_label, "hint")
        state = info.get("state", "")
        error = (info.get("error") or "").strip()
        low = error.lower()
        last = error.splitlines()[-1] if error else ""
        if state == "unknown":
            # Clé déjà acceptée mais empreinte non épinglée : il ne reste que
            # la question de confiance, aucun mot de passe à demander.
            self.status_label.setText(
                "Empreinte d'hôte inconnue : elle sera vérifiée et enregistrée "
                "avant toute copie."
            )
            self._message = "empreinte d'hôte non acceptée"
        elif state == "changed":
            self.status_label.setText(
                "L'empreinte enregistrée ne correspond plus à celle du poste : "
                "copie refusée par sécurité. Vérifiez la machine."
            )
            restyle_label(self.status_label, "warn")
            self._message = "empreinte d'hôte changée"
        elif "not allowed at this time" in low or "connection reset" in low:
            self.status_label.setText(
                "Le poste limite temporairement les connexions de cette "
                "machine (sshd). Patientez une minute, puis réessayez."
            )
            self._message = "poste temporairement limité (sshd)"
        elif state == "unreachable":
            self.status_label.setText(
                "Poste injoignable pour l'instant : réessayez dans un instant."
            )
            self._message = "poste injoignable"
        else:
            self.status_label.setText(
                f"La clé n'est pas encore acceptée ({last})." if last else
                "La clé n'est pas encore acceptée par ce poste."
            )
            self._message = last or "clé refusée par le poste"
        # La clé déjà acceptée n'a besoin d'aucun mot de passe : seul
        # l'épinglage reste à faire, il démarre tout seul. Sinon le champ
        # apparaît et Entrée lance la copie.
        self.form_box.setVisible(not authenticated)
        if authenticated:
            self._auto_start()
        else:
            self.password_edit.setEnabled(True)
            self.password_edit.setFocus()
            # Le mot de passe vient du dialogue d'ajout : l'utilisateur a déjà
            # demandé la copie en ajoutant le poste.
            if self._initial_password:
                self._auto_start()
        self.adjustSize()

    def _auto_start(self) -> None:
        """Démarre la copie une seule fois, sans clic supplémentaire."""
        if self._auto_started:
            return
        self._auto_started = True
        QTimer.singleShot(0, self._start)

    def _start(self) -> None:
        if self._worker is not None and self._worker.isRunning():
            return
        password = self.password_edit.text()
        authenticated = bool((self._prescan or {}).get("authenticated"))
        if not authenticated and not password:
            self._show_result("Mot de passe requis pour copier la clé.", False)
            self.password_edit.setFocus()
            return
        self.password_edit.setEnabled(False)
        self.status_label.setText("Copie de la clé en cours…")
        restyle_label(self.status_label, "hint")
        self.result_label.setVisible(False)
        self.log_view.setVisible(True)
        self.log_view.clear()
        self.log_view.appendPlainText(
            f"→ {address_text(self._host)}\n"
        )

        worker = KeyInstallWorker(self._host, password, self, self._prescan)
        worker.log.connect(self._append_log)
        worker.confirm_host_key.connect(self._ask_host_key)
        worker.done.connect(self._finished)
        self._worker = worker
        worker.start()

    def _ask_host_key(self, name: str, fingerprints: list) -> None:
        """Premier contact : montrer l'empreinte, ne jamais l'accepter en douce.

        Le bouton dit ce qu'il fait (« Accepter l'empreinte ») : un « Oui »
        générique dans une boîte posée par-dessus un journal se lisait mal, et
        l'empreinte doit être épinglée sous TOUTES les adresses du poste (LAN
        et Tailscale), sinon un changement d'adresse redemande la confiance.
        """
        details = "\n".join(f"    {item}" for item in fingerprints) or "    (aucune)"
        text = (
            f"Le poste « {name} » n'est pas encore dans ~/.ssh/known_hosts.\n\n"
            f"Empreintes annoncées par la machine :\n{details}\n\n"
            "Vérifiez-les sur le poste, par exemple :\n"
            "    ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub\n\n"
            "Accepter ces empreintes pour les prochaines connexions ?"
        )
        box = QMessageBox(self)
        box.setWindowTitle("Premier contact — vérifier l'empreinte")
        box.setIcon(QMessageBox.Question)
        box.setText(text)
        accept = box.addButton("Accepter l'empreinte", QMessageBox.AcceptRole)
        refuse = box.addButton("Refuser", QMessageBox.RejectRole)
        box.setDefaultButton(refuse)
        box.setWindowModality(Qt.ApplicationModal)
        box.show()
        box.raise_()
        box.activateWindow()
        box.exec()
        answer = box.clickedButton() is accept
        worker = self._worker
        if worker is not None:
            worker.trust_answer = answer
            worker.trust_event.set()

    def _append_log(self, text: str) -> None:
        self.log_view.insertPlainText(text)
        bar = self.log_view.verticalScrollBar()
        bar.setValue(bar.maximum())

    def _show_result(self, text: str, ok: bool) -> None:
        self.result_label.setObjectName("ok" if ok else "warn")
        self.result_label.setText(text)
        self.result_label.setVisible(True)
        self.result_label.style().unpolish(self.result_label)
        self.result_label.style().polish(self.result_label)

    def _finished(self, ok: bool, message: str) -> None:
        self._success = ok
        self._message = message
        authenticated = bool((self._prescan or {}).get("authenticated"))
        if ok:
            self._show_result(f"Clé installée — {message}.", True)
            self.status_label.setText(
                "Clé installée, empreinte enregistrée : plus de mot de passe "
                "ni de question au prochain contact."
            )
            restyle_label(self.status_label, "ok")
            self.password_edit.setText("")
            self.form_box.setVisible(False)
            self.close_button.setDefault(True)
            self.close_button.setFocus()
            self.installed.emit()
        else:
            self._show_result(f"Échec — {message}.", False)
            self.status_label.setText(
                "Corrigez si besoin, puis Entrée pour réessayer."
            )
            restyle_label(self.status_label, "hint")
            self.password_edit.setEnabled(True)
            self.form_box.setVisible(not authenticated)
            self.password_edit.setFocus()

    def failure_text(self) -> str:
        """Dernière raison d'échec, pour l'appelant qui écarte le poste."""
        return self._message or "clé SSH non installée"

    def abort(self) -> None:
        """Annule et ferme sans demander (fermeture de la fenêtre principale)."""
        if self._probe_worker is not None and self._probe_worker.isRunning():
            self._probe_worker.wait(5000)
        if self._worker is not None and self._worker.isRunning():
            self._worker.cancel.set()
            self._worker.trust_event.set()   # débloque l'attente d'empreinte
            self._worker.wait(3000)
        self.accept()

    def reject(self) -> None:
        if self._worker is not None and self._worker.isRunning():
            self._worker.cancel.set()
            self._worker.trust_event.set()   # débloque l'attente d'empreinte
            self._worker.wait(3000)
        super().reject()

    def key_installed(self) -> bool:
        return self._success
