"""Fenêtres d'opération non modales : plusieurs postes en parallèle.
"""

import os
import shutil

from PySide6.QtCore import QProcess, QTimer, Qt, Signal
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QMessageBox, QPlainTextEdit, QPushButton,
    QVBoxLayout,
)

from .local import privileged_prefix
from .util import display_name, ssh_target
from .workers import SshTaskWorker

class RemoteTaskDialog(QDialog):
    """Commande distante avec journal en direct (arrêt, redémarrage, mise à jour)."""

    completed = Signal(bool)     # (succès) — émis à la fin, avant fermeture

    def __init__(self, parent=None, host: dict | None = None, title: str = "",
                 subtitle: str = "", command: str = "",
                 ok_if_disconnect: bool = False, hint_from_output=None,
                 password: str = ""):
        super().__init__(parent)
        self._host = dict(host or {})
        self._command = command
        self._password = password
        self._ok_if_disconnect = ok_if_disconnect
        self._hint_from_output = hint_from_output
        self._worker: SshTaskWorker | None = None
        self._output: list[str] = []
        self._cancelled = False
        self._running = True

        self.setWindowTitle(title or f"Action — {display_name(self._host)}")
        # Non modale : `start_task` l'affiche sans bloquer les autres postes.
        self.setModal(False)
        self.setMinimumWidth(560)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel(title or display_name(self._host), self)
        heading.setObjectName("title")
        root.addWidget(heading)

        if subtitle:
            detail = QLabel(subtitle, self)
            detail.setObjectName("subtitle")
            detail.setWordWrap(True)
            root.addWidget(detail)

        self.log_view = QPlainTextEdit(self)
        self.log_view.setReadOnly(True)
        self.log_view.setMinimumHeight(260)
        self.log_view.setMaximumBlockCount(5000)
        root.addWidget(self.log_view, 1)

        self.result_label = QLabel(self)
        self.result_label.setWordWrap(True)
        self.result_label.setVisible(False)
        root.addWidget(self.result_label)

        buttons = QHBoxLayout()
        buttons.addStretch(1)
        self.cancel_button = QPushButton("Annuler", self)
        self.cancel_button.setObjectName("ghost")
        self.cancel_button.clicked.connect(self._cancel)
        buttons.addWidget(self.cancel_button)

        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("accent")
        self.close_button.setVisible(False)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        root.addLayout(buttons)

        self.log_view.appendPlainText(
            f"→ {ssh_target(self._host)} (port {self._host['port']})\n"
            f"$ {command}\n\n"
        )
        QTimer.singleShot(60, self._start)

    def _start(self) -> None:
        worker = SshTaskWorker(
            self._host, self._command, self, password=self._password
        )
        worker.log.connect(self._append)
        worker.done.connect(self._finished)
        self._worker = worker
        worker.start()

    def _append(self, text: str) -> None:
        self._output.append(text)
        self.log_view.insertPlainText(text)
        bar = self.log_view.verticalScrollBar()
        bar.setValue(bar.maximum())

    def _cancel(self) -> None:
        if not self._running or self._worker is None:
            return
        self._cancelled = True
        self.cancel_button.setEnabled(False)
        self._worker.cancel.set()

    def _finished(self, code: int) -> None:
        self._running = False
        output = "".join(self._output)
        ok = code == 0
        if not ok and self._ok_if_disconnect and code == 255:
            low = output.lower()
            if any(mark in low for mark in (
                "closed by remote", "connection reset", "broken pipe",
                "disconnected", "connection to",
            )):
                ok = True

        self.result_label.setVisible(True)
        if self._cancelled:
            self.result_label.setObjectName("warn")
            self.result_label.setText("Action annulée.")
        elif ok:
            extra = ""
            if self._hint_from_output is not None:
                extra = self._hint_from_output(output) or ""
            self.result_label.setObjectName("ok")
            self.result_label.setText(
                "Terminé." + (f" {extra}" if extra else "")
            )
        else:
            self.result_label.setObjectName("warn")
            self.result_label.setText(f"Échec (code {code}).")
        self.result_label.style().unpolish(self.result_label)
        self.result_label.style().polish(self.result_label)

        self.cancel_button.setVisible(False)
        self.close_button.setVisible(True)
        self.close_button.setDefault(True)
        self.completed.emit(self._cancelled is False and ok)

    def abort(self) -> None:
        """Annule et ferme sans demander (fermeture de la fenêtre principale)."""
        if self._running and self._worker is not None:
            self._cancelled = True
            self._worker.cancel.set()
            self._worker.wait(3000)
        self.accept()

    def reject(self) -> None:
        if self._running and self._worker is not None:
            answer = QMessageBox.question(
                self, "Action en cours",
                "L'action n'est pas terminée. L'annuler et fermer ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
            self._cancelled = True
            self._worker.cancel.set()
            self._worker.wait(3000)
        super().reject()

class LocalTaskDialog(QDialog):
    """Commande locale en streaming (mises à jour, options système).

    Même déroulé que `RemoteTaskDialog`, mais via `QProcess` sur CETTE machine :
    sortie fusionnée en direct, annulation possible, et `sudo -n` / `pkexec`
    choisi automatiquement pour les commandes qui exigent root. Non modale, comme
    lui : plusieurs opérations peuvent tourner ensemble et la fenêtre principale
    reste utilisable.
    """

    completed = Signal(bool)     # (succès) — émis à la fin, avant fermeture

    def __init__(self, parent=None, title: str = "", subtitle: str = "",
                 argv: list[str] | None = None, root: bool = False,
                 hint_from_output=None):
        super().__init__(parent)
        self._argv = list(argv or [])
        self._root = root
        self._hint_from_output = hint_from_output
        self._process: QProcess | None = None
        self._output: list[str] = []
        self._cancelled = False
        self._running = True

        self.setWindowTitle(title or "Action sur ce poste")
        self.setModal(False)
        self.setMinimumWidth(620)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel(title or "Action sur ce poste", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        if subtitle:
            detail = QLabel(subtitle, self)
            detail.setObjectName("subtitle")
            detail.setWordWrap(True)
            root_layout.addWidget(detail)

        self.log_view = QPlainTextEdit(self)
        self.log_view.setReadOnly(True)
        self.log_view.setMinimumHeight(280)
        self.log_view.setMaximumBlockCount(20000)
        root_layout.addWidget(self.log_view, 1)

        self.result_label = QLabel(self)
        self.result_label.setWordWrap(True)
        self.result_label.setVisible(False)
        root_layout.addWidget(self.result_label)

        buttons = QHBoxLayout()
        buttons.addStretch(1)
        self.cancel_button = QPushButton("Annuler", self)
        self.cancel_button.setObjectName("ghost")
        self.cancel_button.clicked.connect(self._cancel)
        buttons.addWidget(self.cancel_button)

        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("accent")
        self.close_button.setVisible(False)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        root_layout.addLayout(buttons)

        QTimer.singleShot(60, self._start)

    def _start(self) -> None:
        argv = list(self._argv)
        if self._root:
            prefix = privileged_prefix()
            if prefix == ["pkexec"] and shutil.which("pkexec") is None:
                self._append(
                    "Ni sudo sans mot de passe ni pkexec : action impossible.\n"
                )
                self._finished(127)
                return
            argv = [*prefix, *argv]

        self.log_view.appendPlainText("$ " + " ".join(argv) + "\n\n")
        process = QProcess(self)
        process.setProcessChannelMode(QProcess.MergedChannels)
        process.setProgram(argv[0])
        process.setArguments(argv[1:])
        process.readyReadStandardOutput.connect(self._drain)
        process.finished.connect(self._finished)
        process.errorOccurred.connect(self._start_failed)
        if hasattr(process, "setChildProcessModifier"):
            # Propre session : l'annulation emporte les enfants (bootc, ssh…).
            process.setChildProcessModifier(os.setsid)
        self._process = process
        process.start()

    def _drain(self) -> None:
        if self._process is None:
            return
        chunk = bytes(self._process.readAllStandardOutput())
        if chunk:
            self._append(chunk.decode("utf-8", "replace"))

    def _append(self, text: str) -> None:
        if not text:
            return
        self._output.append(text)
        self.log_view.insertPlainText(text)
        bar = self.log_view.verticalScrollBar()
        bar.setValue(bar.maximum())

    def _cancel(self) -> None:
        if not self._running or self._process is None:
            return
        self._cancelled = True
        self.cancel_button.setEnabled(False)
        self._append("\n[annulation…]\n")
        self._process.kill()

    def _start_failed(self, error) -> None:
        # finished() n'est pas émis quand le programme n'a pas pu démarrer.
        if error != QProcess.FailedToStart:
            return
        self._append(f"\nLancement impossible ({self._argv[0]}).\n")
        self._finished(127)

    def _finished(self, code: int) -> None:
        self._running = False
        output = "".join(self._output)
        ok = code == 0
        self.result_label.setVisible(True)
        if self._cancelled:
            self.result_label.setObjectName("warn")
            self.result_label.setText("Action annulée.")
        elif ok:
            extra = ""
            if self._hint_from_output is not None:
                extra = self._hint_from_output(output) or ""
            self.result_label.setObjectName("ok")
            self.result_label.setText("Terminé." + (f" {extra}" if extra else ""))
        else:
            self.result_label.setObjectName("warn")
            reasons = {
                126: "autorisation refusée",
                127: "non autorisé ou commande absente",
                125: "dialogue PolicyKit absent ou annulé",
            }
            detail = reasons.get(code, "")
            self.result_label.setText(
                f"Échec (code {code})" + (f" — {detail}" if detail else "") + "."
            )
        self.result_label.style().unpolish(self.result_label)
        self.result_label.style().polish(self.result_label)

        self.cancel_button.setVisible(False)
        self.close_button.setVisible(True)
        self.close_button.setDefault(True)
        self._process = None
        self.completed.emit(ok and not self._cancelled)

    def abort(self) -> None:
        """Annule et ferme sans demander (fermeture de la fenêtre principale)."""
        if self._running and self._process is not None:
            self._cancelled = True
            self._process.kill()
        self.accept()

    def reject(self) -> None:
        if self._running and self._process is not None:
            answer = QMessageBox.question(
                self, "Action en cours",
                "L'action n'est pas terminée. L'annuler et fermer ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
            self._cancelled = True
            self._process.kill()
            self._process.waitForFinished(3000)
        super().reject()

def start_task(parent, dialog: QDialog) -> None:
    """Affiche une tâche sans bloquer la fenêtre principale.

    Une opération sur un poste n'a aucune raison d'empêcher celle d'un autre :
    chaque tâche est une fenêtre **non modale**, rattachée à la fenêtre
    principale (qui la garde en vie) et suivie jusqu'à la fin pour rafraîchir
    les cartes. Plusieurs `exec()` imbriqués, eux, bloquaient tout.
    """
    root = parent
    while root is not None and root.parent() is not None:
        root = root.parent()      # la fenêtre principale est la racine
    dialog.setModal(False)
    dialog.setWindowModality(Qt.NonModal)
    if root is not None and root is not dialog:
        dialog.setParent(root, dialog.windowFlags())
    tasks = getattr(root, "_tasks", None)
    if tasks is not None:
        tasks.append(dialog)
        dialog.finished.connect(lambda _=0: _task_closed(root, dialog))
    completed = getattr(dialog, "completed", None)
    if completed is not None:
        completed.connect(lambda _ok: _task_completed(root))
    dialog.show()
    dialog.raise_()
    dialog.activateWindow()

def _task_closed(root, dialog: QDialog) -> None:
    """Oublie la tâche fermée et libère sa fenêtre."""
    tasks = getattr(root, "_tasks", None)
    if tasks is not None and dialog in tasks:
        tasks.remove(dialog)
    dialog.deleteLater()

def _task_completed(root) -> None:
    """Rafraîchit les cartes quand une tâche se termine, sans attendre la fermeture."""
    refresh = getattr(root, "refresh_all", None)
    if refresh is None:
        return

    def run() -> None:
        try:
            refresh()
        except RuntimeError:      # fenêtre principale déjà fermée
            pass

    # Laisse au poste distant le temps de se remettre (extinction, reboot).
    QTimer.singleShot(1200, run)
