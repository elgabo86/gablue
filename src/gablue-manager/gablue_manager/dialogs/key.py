"""Copie de la clé SSH vers un poste.
"""

from PySide6.QtCore import QTimer, Signal
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QLineEdit, QMessageBox, QPlainTextEdit, QPushButton,
    QVBoxLayout, QWidget,
)

from ..constants import PUBLIC_KEY
from ..uiutil import restyle_label
from ..util import address_text, display_name
from ..workers import KeyInstallWorker, KeyProbeWorker
from .host import HostDialog

class KeyInstallDialog(QDialog):
    """Copie la clé publique, avec invite de mot de passe et journal."""

    installed = Signal()

    def __init__(self, parent=None, host: dict | None = None,
                 password: str = ""):
        super().__init__(parent)
        self._host = dict(host or {})
        # Mot de passe éventuellement saisi dans le dialogue d'ajout : il sert
        # à lancer la copie sans redemander quoi que ce soit.
        self._initial_password = password
        self._auto_started = False
        self._worker: KeyInstallWorker | None = None
        self._probe_worker: KeyProbeWorker | None = None
        self._success = False

        self.setWindowTitle(f"Clé SSH — {display_name(self._host)}")
        self.setModal(True)
        self.setMinimumWidth(520)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(12)

        self.title = QLabel(f"Copier la clé vers {display_name(self._host)}")
        self.title.setObjectName("title")
        root.addWidget(self.title)

        self.explanation = QLabel(
            f'La clé publique {PUBLIC_KEY.name} sera ajoutée à '
            f'~/.ssh/authorized_keys sur {address_text(self._host)}. '
            "Ensuite, plus de mot de passe : connexion, bureau distant et "
            "fichiers passent par la clé."
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
        self.password_edit = QLineEdit(self.form_box)
        self.password_edit.setEchoMode(QLineEdit.Password)
        self.password_edit.setPlaceholderText(
            "mot de passe du compte distant (jamais enregistré)"
        )
        self.password_edit.setText(password)
        form.addWidget(self.password_edit)

        hint = QLabel(
            "Le mot de passe sert uniquement à ssh-copy-id, il n'est ni écrit "
            "sur le disque ni stocké dans la configuration.",
            self.form_box,
        )
        hint.setObjectName("hint")
        hint.setWordWrap(True)
        form.addWidget(hint)
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

    def _probe_done(self, ok: bool, detail: str) -> None:
        self._probe_worker = None
        self.close_button.setEnabled(True)
        if ok:
            self.title.setText(
                f"Clé déjà installée sur {display_name(self._host)}"
            )
            self.explanation.setVisible(False)
            self.status_label.setText(
                "Cette clé est déjà acceptée par le poste : aucune action "
                "nécessaire, aucun mot de passe à saisir."
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
        if detail:
            self.status_label.setText(
                f"La clé n'est pas encore acceptée ({detail})."
            )
        else:
            self.status_label.setText(
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

        worker = KeyInstallWorker(self._host, password, self)
        worker.log.connect(self._append_log)
        worker.confirm_host_key.connect(self._ask_host_key)
        worker.done.connect(self._finished)
        self._worker = worker
        worker.start()

    def _ask_host_key(self, name: str, fingerprints: list) -> None:
        """Premier contact : montrer l'empreinte, ne jamais l'accepter en douce."""
        details = "\n".join(f"    {item}" for item in fingerprints) or "    (aucune)"
        text = (
            f"Le poste « {name} » n'est pas encore dans ~/.ssh/known_hosts.\n\n"
            f"Empreintes annoncées par la machine :\n{details}\n\n"
            "Vérifiez-les sur le poste, par exemple :\n"
            "    ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub\n\n"
            "Accepter cette empreinte pour les prochaines connexions ?"
        )
        answer = QMessageBox.question(
            self, "Premier contact — vérifier l'empreinte", text,
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        worker = self._worker
        if worker is not None:
            worker.trust_answer = answer == QMessageBox.Yes
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
