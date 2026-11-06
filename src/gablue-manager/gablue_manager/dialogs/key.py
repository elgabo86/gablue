"""Copie de la clé SSH vers un poste.
"""

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QLineEdit, QMessageBox, QPlainTextEdit, QPushButton,
    QVBoxLayout, QWidget,
)

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
            "Sert une fois, jamais enregistré.", self.form_box,
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

        self.install_button = QPushButton("Installer la clé", self)
        self.install_button.setObjectName("accent")
        self.install_button.setDefault(True)
        self.install_button.clicked.connect(self._start)
        self.install_button.setVisible(False)
        buttons.addWidget(self.install_button)
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
            self.install_button.setVisible(True)
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
        if info.get("authenticated"):
            self.title.setText(
                f"Clé déjà installée sur {display_name(self._host)}"
            )
            self.explanation.setVisible(False)
            self.status_label.setText(
                "Clé déjà acceptée : rien à faire."
            )
            restyle_label(self.status_label, "ok")
            self.form_box.setVisible(False)
            self.install_button.setVisible(False)
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
        if state == "unknown":
            # Ce n'est pas la clé qui est refusée : c'est l'empreinte du poste
            # qui n'est pas encore connue. Le dire évite de croire que la copie
            # a échoué alors que rien n'a encore été tenté.
            self.status_label.setText(
                "Empreinte d'hôte inconnue : « Installer la clé » vous la fera "
                "vérifier avant toute écriture."
            )
        elif state == "changed":
            self.status_label.setText(
                "L'empreinte enregistrée ne correspond plus à celle du poste : "
                "copie refusée par sécurité. Vérifiez la machine."
            )
            restyle_label(self.status_label, "warn")
        elif "not allowed at this time" in low or "connection reset" in low:
            self.status_label.setText(
                "Le poste limite temporairement les connexions de cette "
                "machine (sshd). Patientez une minute, puis réessayez."
            )
        elif state == "unreachable":
            self.status_label.setText(
                "Poste injoignable pour l'instant : réessayez dans un instant."
            )
        else:
            last = error.splitlines()[-1] if error else ""
            self.status_label.setText(
                f"La clé n'est pas encore acceptée ({last})." if last else
                "La clé n'est pas encore acceptée par ce poste."
            )
        self.form_box.setVisible(True)
        self.install_button.setVisible(True)
        self.password_edit.setFocus()
        # Le mot de passe vient du dialogue d'ajout : l'utilisateur a déjà
        # demandé la copie en ajoutant le poste, on ne lui fait pas cliquer une
        # deuxième fois.
        if self._initial_password and not self._auto_started:
            self._auto_started = True
            QTimer.singleShot(0, self._start)

    def _start(self) -> None:
        if self._worker is not None and self._worker.isRunning():
            return
        password = self.password_edit.text()
        self.password_edit.setEnabled(False)
        self.install_button.setEnabled(False)
        self.install_button.setText("Installation…")
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

    def _finished(self, ok: bool, message: str) -> None:
        self._success = ok
        self.result_label.setVisible(True)
        if ok:
            self.result_label.setObjectName("ok")
            self.result_label.setText(f"Clé installée — {message}.")
            self.install_button.setVisible(False)
            self.close_button.setText("Fermer")
            self.close_button.setDefault(True)
            self.installed.emit()
        else:
            self.result_label.setObjectName("warn")
            self.result_label.setText(f"Échec — {message}.")
            self.password_edit.setEnabled(True)
            self.install_button.setEnabled(True)
            self.install_button.setText("Réessayer")
        self.result_label.style().unpolish(self.result_label)
        self.result_label.style().polish(self.result_label)
        # Succès : le secret n'a plus d'utilité, on le jette. Échec : on le
        # laisse en place (masqué) pour que « Réessayer » ne le redemande pas.
        if ok:
            self.password_edit.setText("")

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
