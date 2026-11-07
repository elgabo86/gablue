"""Fenêtres d'opération non modales : plusieurs postes en parallèle.
"""

import os
import shutil

from PySide6.QtCore import QProcess, QSocketNotifier, QTimer, Qt, Signal
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QMessageBox, QPlainTextEdit, QProgressBar,
    QPushButton, QVBoxLayout,
)

from .local import parse_progress_event, privileged_prefix
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

def _human_bytes(value: int) -> str:
    """Taille lisible (o, Kio, Mio, Gio) — journal de progression de bootc."""
    size = float(max(0, int(value)))
    for unit in ("o", "Kio", "Mio", "Gio"):
        if size < 1024 or unit == "Gio":
            return f"{size:.0f} {unit}" if unit == "o" else f"{size:.1f} {unit}"
        size /= 1024
    return f"{size:.1f} Gio"

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
                 hint_from_output=None, progress_fifo: str = "",
                 progress_parser=None):
        super().__init__(parent)
        self._argv = list(argv or [])
        self._root = root
        self._hint_from_output = hint_from_output
        self._process: QProcess | None = None
        self._output: list[str] = []
        self._cancelled = False
        self._running = True
        # Progression structurée (bootc) : FIFO écrit par le helper root.
        self._progress_fifo = progress_fifo
        self._progress_fd = -1
        self._progress_notifier: QSocketNotifier | None = None
        self._progress_tries = 0
        self._progress_buffer = ""
        self._progress_seen = False
        # Progression lue dans le journal (flatpak, brew) : la barre remplace le
        # journal, qui reste accessible par le bouton « Terminal ».
        self._progress_parser = progress_parser
        self._progress_ui = bool(progress_fifo or progress_parser)
        self._line_buffer = ""

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

        # Progression lisible quand la commande sait la publier (bootc via son
        # FIFO, flatpak en lisant ses lignes) : le journal passe alors au
        # second plan, derrière un bouton « Terminal ».
        self.progress_label = QLabel(self)
        self.progress_label.setObjectName("hostDetail")
        self.progress_label.setWordWrap(True)
        self.progress_label.setVisible(self._progress_ui)
        root_layout.addWidget(self.progress_label)

        self.progress_bar = QProgressBar(self)
        self.progress_bar.setTextVisible(True)
        self.progress_bar.setRange(0, 0)     # indéterminé tant qu'aucun événement
        self.progress_bar.setVisible(self._progress_ui)
        root_layout.addWidget(self.progress_bar)
        if progress_fifo:
            self.progress_label.setText("Préparation de la mise à jour…")
        elif progress_parser is not None:
            self.progress_label.setText("Recherche de mises à jour…")

        self.log_view = QPlainTextEdit(self)
        self.log_view.setReadOnly(True)
        self.log_view.setMinimumHeight(280)
        self.log_view.setMaximumBlockCount(20000)
        self.log_view.setVisible(not self._progress_ui)
        root_layout.addWidget(self.log_view, 1)

        self.result_label = QLabel(self)
        self.result_label.setWordWrap(True)
        self.result_label.setVisible(False)
        root_layout.addWidget(self.result_label)

        buttons = QHBoxLayout()
        if self._progress_ui:
            self.log_toggle = QPushButton("Terminal", self)
            self.log_toggle.setObjectName("ghost")
            self.log_toggle.setCheckable(True)
            self.log_toggle.setToolTip("Afficher le journal complet")
            self.log_toggle.toggled.connect(self._toggle_log)
            buttons.addWidget(self.log_toggle)
        else:
            self.log_toggle = None
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
        if self._progress_fifo:
            QTimer.singleShot(0, self._progress_open)

    # -- progression (bootc par FIFO, flatpak dans le journal) --------------
    def _toggle_log(self, shown: bool) -> None:
        self.log_view.setVisible(shown)
        if self.log_toggle is not None:
            self.log_toggle.setToolTip(
                "Masquer le journal" if shown
                else "Afficher le journal complet"
            )
        self.adjustSize()

    def _progress_open(self) -> None:
        """Ouvre le FIFO dès que le helper l'a créé (il tourne en root)."""
        if self._progress_fd >= 0 or not self._progress_fifo or not self._running:
            return
        if not os.path.exists(self._progress_fifo):
            if self._progress_tries < 50:      # ~5 s, le temps que sudo démarre
                self._progress_tries += 1
                QTimer.singleShot(100, self._progress_open)
            return
        try:
            fd = os.open(self._progress_fifo, os.O_RDONLY | os.O_NONBLOCK)
        except OSError:
            if self._progress_tries < 50:
                self._progress_tries += 1
                QTimer.singleShot(100, self._progress_open)
            return
        self._progress_fd = fd
        notifier = QSocketNotifier(fd, QSocketNotifier.Read, self)
        notifier.activated.connect(self._read_progress)
        self._progress_notifier = notifier

    def _read_progress(self) -> None:
        if self._progress_fd < 0:
            return
        try:
            chunk = os.read(self._progress_fd, 65536)
        except (BlockingIOError, InterruptedError):
            return
        except OSError:
            self._stop_progress()
            return
        if not chunk:                      # plus d'écrivain (bootc terminé)
            self._stop_progress()
            return
        self._progress_buffer += chunk.decode("utf-8", "replace")
        while "\n" in self._progress_buffer:
            line, _, self._progress_buffer = \
                self._progress_buffer.partition("\n")
            info = parse_progress_event(line)
            if info:
                self._apply_progress(info)

    def _apply_progress(self, info: dict) -> None:
        self._progress_seen = True
        label = info.get("label") or "Mise à jour en cours"
        total = int(info.get("bytes_total") or 0)
        if total > 0:
            label += " — " + _human_bytes(int(info.get("bytes") or 0)) \
                     + " / " + _human_bytes(total)
        self.progress_label.setText(label)
        percent = info.get("percent")
        if percent is None:
            self.progress_bar.setRange(0, 0)
        else:
            self.progress_bar.setRange(0, 100)
            self.progress_bar.setValue(int(round(float(percent))))

    def _apply_line_progress(self, info: dict) -> None:
        self._progress_seen = True
        self.progress_label.setText(info.get("label") or "Mise à jour en cours")
        percent = info.get("percent")
        if percent is None:
            # Travail en cours sans total connu (téléchargements de brew).
            self.progress_bar.setRange(0, 0)
            return
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(int(round(float(percent))))

    def _read_progress_lines(self, text: str) -> None:
        """Découpe la sortie en lignes complètes et en tire la progression.

        `QProcess` livre des morceaux arbitraires : une ligne peut arriver en
        deux fois, d'où le tampon. Les lignes non reconnues restent dans le
        journal, rien n'est consommé.
        """
        if self._progress_parser is None:
            return
        self._line_buffer += text
        while "\n" in self._line_buffer:
            line, _, self._line_buffer = self._line_buffer.partition("\n")
            info = self._progress_parser(line)
            if info:
                self._apply_line_progress(info)

    def _flush_progress_lines(self) -> None:
        if self._line_buffer:
            info = (self._progress_parser(self._line_buffer)
                    if self._progress_parser is not None else {})
            self._line_buffer = ""
            if info:
                self._apply_line_progress(info)

    def _stop_progress(self) -> None:
        if self._progress_notifier is not None:
            self._progress_notifier.setEnabled(False)
            self._progress_notifier = None
        if self._progress_fd >= 0:
            try:
                os.close(self._progress_fd)
            except OSError:
                pass
            self._progress_fd = -1

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
        self._read_progress_lines(text)

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
        self._stop_progress()
        self._flush_progress_lines()
        output = "".join(self._output)
        ok = code == 0
        if self._progress_ui:
            # Sans aucun événement, il n'y avait rien à télécharger (bootc
            # répond « No changes in… », flatpak « Nothing to do. ») : la barre
            # n'aurait rien à montrer.
            self.progress_bar.setVisible(self._progress_seen)
            self.progress_label.setVisible(self._progress_seen)
            if self._progress_seen and ok:
                self.progress_bar.setRange(0, 100)
                self.progress_bar.setValue(100)
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
        self._stop_progress()
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
        self._stop_progress()
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
