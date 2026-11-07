"""Fenêtres d'opération non modales : plusieurs postes en parallèle.
"""

import html
import os
import shutil

from PySide6.QtCore import (
    QProcess, QProcessEnvironment, QSocketNotifier, QTimer, Qt, Signal,
)
from PySide6.QtGui import QGuiApplication
from PySide6.QtWidgets import (
    QDialog, QHBoxLayout, QLabel, QMessageBox, QPlainTextEdit, QPushButton,
    QVBoxLayout, QWidget,
)

from .local import parse_progress_event, privileged_prefix
from .util import display_name, ssh_target
from .widgets import BusyProgressBar

def _display_command(argv: list[str]) -> str:
    """Commande lisible pour le journal : un script multi-ligne est résumé.

    Les scripts `bash -c` de plusieurs lignes (Flatpak, Homebrew, réglages)
    déversaient tout leur contenu en tête du journal, y compris les lignes de
    protocole `::gablue:` — illisible, et trompeur dans la vue « Terminal ».
    """
    if any("\n" in arg for arg in argv):
        fixed = " ".join(arg for arg in argv if "\n" not in arg)
        return f"{fixed} <script>"
    return " ".join(argv)

class _AuthLinkRow(QWidget):
    """Lien d'enregistrement (Tailscale) : cliquable, copiable, masquable.

    `tailscale up` lancé en root n'ouvre pas de navigateur : il écrit le lien
    d'authentification sur sa sortie. Le poser dans la fenêtre évite de
    dérouler le journal, et le bouton « Copier » sert quand le navigateur ne
    peut pas être ouvert (session distante, root sans xdg-open).
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(8)
        self._url = ""
        self.label = QLabel(self)
        self.label.setObjectName("hostDetail")
        self.label.setTextFormat(Qt.RichText)
        self.label.setOpenExternalLinks(True)
        self.label.setWordWrap(True)
        self.label.setTextInteractionFlags(
            Qt.TextSelectableByMouse | Qt.LinksAccessibleByMouse
        )
        layout.addWidget(self.label, 1)
        self.copy_button = QPushButton("Copier le lien", self)
        self.copy_button.setObjectName("ghost")
        self.copy_button.clicked.connect(self._copy)
        layout.addWidget(self.copy_button, 0, Qt.AlignTop)
        self.setVisible(False)

    def show_url(self, url: str) -> None:
        self._url = url
        escaped = html.escape(url, quote=True)
        self.label.setText(
            f"Lien d'enregistrement : <a href=\"{escaped}\">{escaped}</a>"
        )
        self.setVisible(True)

    def _copy(self) -> None:
        if not self._url:
            return
        QGuiApplication.clipboard().setText(self._url)
        self.copy_button.setText("Copié")
        QTimer.singleShot(
            1500, lambda: self.copy_button.setText("Copier le lien")
        )

class RemoteTaskDialog(QDialog):
    """Commande distante avec journal en direct (arrêt, redémarrage, mise à jour)."""

    completed = Signal(bool)     # (succès) — émis à la fin, avant fermeture

    def __init__(self, parent=None, host: dict | None = None, title: str = "",
                 subtitle: str = "", command: str = "",
                 ok_if_disconnect: bool = False, hint_from_output=None,
                 password: str = "",
                 progress_mode: str = "auto", progress_text: str = "",
                 auth_link_parser=None):
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
        # Réglages à distance : rien de mesurable ne remonte de la commande,
        # mais la fenêtre mérite la même présentation que les réglages locaux
        # (barre indéterminée, journal derrière « Terminal »).
        self._progress_busy = progress_mode == "busy"
        self._auth_link_parser = auth_link_parser
        self._auth_url = ""

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

        self.progress_label = QLabel(progress_text or "Application en cours…", self)
        self.progress_label.setObjectName("hostDetail")
        self.progress_label.setWordWrap(True)
        self.progress_label.setVisible(self._progress_busy)
        root.addWidget(self.progress_label)

        self.progress_bar = BusyProgressBar(self)
        self.progress_bar.setObjectName("taskProgress")
        self.progress_bar.setTextVisible(True)
        self.progress_bar.setRange(0, 0)
        self.progress_bar.setVisible(self._progress_busy)
        root.addWidget(self.progress_bar)

        self.link_row = _AuthLinkRow(self)
        root.addWidget(self.link_row)

        self.log_view = QPlainTextEdit(self)
        self.log_view.setReadOnly(True)
        self.log_view.setMinimumHeight(260)
        self.log_view.setMaximumBlockCount(5000)
        self.log_view.setVisible(not self._progress_busy)
        root.addWidget(self.log_view, 1)

        self.result_label = QLabel(self)
        self.result_label.setWordWrap(True)
        self.result_label.setVisible(False)
        root.addWidget(self.result_label)

        buttons = QHBoxLayout()
        if self._progress_busy:
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
        root.addLayout(buttons)

        self.log_view.appendPlainText(
            f"→ {ssh_target(self._host)} (port {self._host['port']})\n"
            f"$ {command}\n\n"
        )
        QTimer.singleShot(60, self._start)

    def _start(self) -> None:
        # Import tardif : les tâches locales (progression CLI, mises à jour de
        # gwine) n'ont pas besoin du module SSH, qui n'a donc aucune raison
        # d'être chargé en mode `--progress`.
        from .workers import SshTaskWorker

        worker = SshTaskWorker(
            self._host, self._command, self, password=self._password
        )
        worker.log.connect(self._append)
        worker.done.connect(self._finished)
        self._worker = worker
        worker.start()

    def _append(self, text: str) -> None:
        self._output.append(text)
        if self._auth_link_parser is not None and not self._auth_url:
            url = self._auth_link_parser("".join(self._output)) or ""
            if url:
                self._auth_url = url
                self.link_row.show_url(url)
                self.adjustSize()
        self.log_view.insertPlainText(text)
        bar = self.log_view.verticalScrollBar()
        bar.setValue(bar.maximum())

    def _toggle_log(self, shown: bool) -> None:
        self.log_view.setVisible(shown)
        if self.log_toggle is not None:
            self.log_toggle.setToolTip(
                "Masquer le journal" if shown
                else "Afficher le journal complet"
            )
        self.adjustSize()

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

        if self._progress_busy:
            # Rien à mesurer : la barre s'arrête sur un état lisible plutôt
            # que de continuer à balayer derrière « Terminé ».
            self.progress_bar.setRange(0, 100)
            self.progress_bar.setValue(100 if ok else 0)
            self.progress_label.setText(
                "Terminé." if ok else "Interrompu."
            )
            if not ok and self.log_toggle is not None:
                self.log_toggle.setChecked(True)    # montrer la cause

        if ok or self._cancelled:
            self.link_row.setVisible(False)     # validé ou abandonné

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
                 progress_parser=None, env_extra: dict | None = None,
                 close_on_success: bool = False,
                 reveal_log_on_error: bool = False,
                 progress_mode: str = "auto", progress_text: str = "",
                 auth_link_parser=None, auth_check_argv: list[str] | None = None,
                 auth_check_parser=None):
        super().__init__(parent)
        self._argv = list(argv or [])
        self._root = root
        self._hint_from_output = hint_from_output
        self._env_extra = dict(env_extra or {})
        self._close_on_success = close_on_success
        self._reveal_log_on_error = reveal_log_on_error
        self._process: QProcess | None = None
        self._output: list[str] = []
        self._cancelled = False
        self._running = True
        # Code de sortie de la commande (None tant qu'elle n'a pas rendu la
        # main) : le mode CLI de Gablue Manager le renvoie à l'appelant.
        self.exit_code: int | None = None
        # Progression structurée (bootc) : FIFO écrit par le helper root.
        self._progress_fifo = progress_fifo
        self._progress_fd = -1
        self._progress_notifier: QSocketNotifier | None = None
        self._progress_tries = 0
        self._progress_buffer = ""
        # En mode « busy » la barre est voulue dès le départ, même si la
        # commande n'écrit rien : elle doit rester visible à la fin.
        self._progress_seen = progress_mode == "busy"
        # Progression lue dans le journal (flatpak, brew) : la barre remplace le
        # journal, qui reste accessible par le bouton « Terminal ».
        self._progress_parser = progress_parser
        # Mode « busy » : la commande ne publie aucune progression mesurable
        # (réglages, bascules de service) mais l'action mérite la même fenêtre
        # épurée — barre indéterminée et journal derrière « Terminal ».
        self._progress_ui = (
            progress_mode == "busy"
            or bool(progress_fifo or progress_parser)
        )
        self._line_buffer = ""
        # Enregistrement Tailscale : lien lu dans la sortie, validé par une
        # sonde périodique (l'appareil est confié à l'opérateur par le helper).
        self._auth_link_parser = auth_link_parser
        self._auth_check_argv = list(auth_check_argv or [])
        self._auth_check_parser = auth_check_parser
        self._auth_url = ""
        self._auth_timer: QTimer | None = None
        self._auth_process: QProcess | None = None

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

        self.progress_bar = BusyProgressBar(self)
        self.progress_bar.setObjectName("taskProgress")
        self.progress_bar.setTextVisible(True)
        self.progress_bar.setRange(0, 0)     # indéterminé tant qu'aucun événement
        self.progress_bar.setVisible(self._progress_ui)
        root_layout.addWidget(self.progress_bar)
        if progress_fifo:
            self.progress_label.setText("Préparation de la mise à jour…")
        elif progress_parser is not None:
            self.progress_label.setText(
                progress_text or "Recherche de mises à jour…"
            )
        elif progress_mode == "busy":
            self.progress_label.setText(
                progress_text or "Application des réglages…"
            )

        # Lien d'enregistrement (Tailscale) : visible dès que la commande
        # l'écrit, masqué quand la validation est constatée.
        self.link_row = _AuthLinkRow(self)
        root_layout.addWidget(self.link_row)

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

    @property
    def cancelled(self) -> bool:
        """L'utilisateur a-t-il annulé l'opération ?"""
        return self._cancelled

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

        self.log_view.appendPlainText("$ " + _display_command(argv) + "\n\n")
        process = QProcess(self)
        process.setProcessChannelMode(QProcess.MergedChannels)
        if self._env_extra:
            env = QProcessEnvironment.systemEnvironment()
            for key, value in self._env_extra.items():
                if value is None:
                    env.remove(key)
                else:
                    env.insert(key, value)
            process.setProcessEnvironment(env)
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

    def _read_progress_lines(self, text: str) -> str:
        """Découpe la sortie en lignes complètes et en tire la progression.

        `QProcess` livre des morceaux arbitraires : une ligne peut arriver en
        deux fois, d'où le tampon. Les lignes de protocole sont consommées
        (`hide`) et ne vont pas au journal ; les autres sont rendues à
        l'appelant, seul chargé de les afficher.
        """
        if self._progress_parser is None:
            return text
        self._line_buffer += text
        kept: list[str] = []
        while "\n" in self._line_buffer:
            line, _, self._line_buffer = self._line_buffer.partition("\n")
            info = self._progress_parser(line)
            if info:
                self._apply_line_progress(info)
                if info.get("hide"):
                    continue
            kept.append(line + "\n")
        return "".join(kept)

    def _flush_progress_lines(self) -> None:
        if not self._line_buffer:
            return
        leftover, self._line_buffer = self._line_buffer, ""
        info = (self._progress_parser(leftover)
                if self._progress_parser is not None else {})
        if info:
            self._apply_line_progress(info)
            if info.get("hide"):
                return
        if leftover:
            self.log_view.insertPlainText(leftover)

    # -- lien d'enregistrement (Tailscale) ---------------------------------
    def _watch_link(self) -> None:
        if self._auth_link_parser is None or self._auth_url:
            return
        url = self._auth_link_parser("".join(self._output)) or ""
        if not url:
            return
        self._auth_url = url
        self.link_row.show_url(url)
        self.adjustSize()
        self._start_auth_poll()

    def _start_auth_poll(self) -> None:
        if not self._auth_check_argv or self._auth_check_parser is None:
            return
        if self._auth_timer is not None:
            return
        timer = QTimer(self)
        timer.setInterval(1500)
        timer.timeout.connect(self._poll_auth)
        self._auth_timer = timer
        timer.start()

    def _poll_auth(self) -> None:
        """Demande à l'outil si l'enregistrement est validé (sans bloquer)."""
        if not self._running or self._auth_process is not None:
            return
        process = QProcess(self)
        process.setProgram(self._auth_check_argv[0])
        process.setArguments(self._auth_check_argv[1:])
        process.finished.connect(
            lambda _code, _status: self._auth_checked(process)
        )
        process.errorOccurred.connect(lambda _error: self._auth_checked(process))
        self._auth_process = process
        process.start()

    def _auth_checked(self, process: QProcess) -> None:
        if self._auth_process is not process:
            return          # finished() et errorOccurred() peuvent doubler
        self._auth_process = None
        text = bytes(process.readAllStandardOutput()).decode("utf-8", "replace")
        if self._auth_check_parser(text):
            self._auth_registered()

    def _auth_registered(self) -> None:
        """Validation constatée : le lien n'a plus lieu d'être."""
        if self._auth_link_parser is None:
            return
        self._stop_auth_poll()
        self.link_row.setVisible(False)
        self.progress_label.setText("Appareil enregistré.")
        self.adjustSize()

    def _stop_auth_poll(self) -> None:
        if self._auth_timer is not None:
            self._auth_timer.stop()
            self._auth_timer = None
        if self._auth_process is not None:
            process, self._auth_process = self._auth_process, None
            process.kill()

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
        if self._auth_link_parser is not None:
            self._watch_link()
        shown = self._read_progress_lines(text)
        if not shown:
            return
        self.log_view.insertPlainText(shown)
        bar = self.log_view.verticalScrollBar()
        bar.setValue(bar.maximum())

    def _cancel(self) -> None:
        if not self._running or self._process is None:
            return
        self._cancelled = True
        self.cancel_button.setEnabled(False)
        self._stop_auth_poll()
        self._append("\n[annulation…]\n")
        # SIGTERM d'abord : gwine restaure l'ancien préfixe sur ce signal, un
        # SIGKILL immédiat le laisserait à moitié construit. SIGKILL seulement
        # si le programme s'accroche.
        self._process.terminate()
        QTimer.singleShot(4000, self._kill_if_running)

    def _kill_if_running(self) -> None:
        if self._running and self._process is not None:
            self._process.kill()

    def _start_failed(self, error) -> None:
        # finished() n'est pas émis quand le programme n'a pas pu démarrer.
        if error != QProcess.FailedToStart:
            return
        self._append(f"\nLancement impossible ({self._argv[0]}).\n")
        self._finished(127)

    def _finished(self, code: int) -> None:
        self._running = False
        self.exit_code = int(code)
        self._stop_progress()
        self._stop_auth_poll()
        self._flush_progress_lines()
        output = "".join(self._output)
        ok = code == 0
        if ok:
            self._auth_registered()
        elif self._cancelled:
            # Annulation : le lien d'enregistrement n'a plus d'objet.
            self.link_row.setVisible(False)
        if self._progress_ui:
            # Sans aucun événement, il n'y avait rien à télécharger (bootc
            # répond « No changes in… », flatpak « Nothing to do. ») : la barre
            # n'aurait rien à montrer.
            self.progress_bar.setVisible(self._progress_seen)
            self.progress_label.setVisible(self._progress_seen)
            if self._progress_seen and ok:
                self.progress_bar.setRange(0, 100)
                self.progress_bar.setValue(100)
        if not ok and self._reveal_log_on_error and self.log_toggle is not None:
            # En CLI (gwine), la fenêtre est le seul canal : montrer le journal
            # au lieu d'un simple « Échec (code N) » sans explication.
            self.log_toggle.setChecked(True)
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
        if self._close_on_success and ok and not self._cancelled:
            # Mode CLI : l'appelant (gwine) enchaîne aussitôt, la fenêtre ne
            # doit pas rester devant le jeu.
            QTimer.singleShot(0, self.accept)

    def abort(self) -> None:
        """Annule et ferme sans demander (fermeture de la fenêtre principale)."""
        if self._running and self._process is not None:
            self._cancelled = True
            self._process.terminate()
            if not self._process.waitForFinished(3000):
                self._process.kill()
                self._process.waitForFinished(1000)
        self._stop_progress()
        self._stop_auth_poll()
        self.accept()

    def reject(self) -> None:
        """Fermeture demandée par l'utilisateur, avec confirmation."""
        if self._running and self._process is not None:
            answer = QMessageBox.question(
                self, "Action en cours",
                "L'action n'est pas terminée. L'annuler et fermer ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
            self._cancelled = True
            self._process.terminate()
            if not self._process.waitForFinished(3000):
                self._process.kill()
                self._process.waitForFinished(1000)
        self._stop_progress()
        self._stop_auth_poll()
        super().reject()

class SequentialTaskDialog(QDialog):
    """Plusieurs commandes locales à la suite, avec une barre **globale**.

    Chaque étape garde son propre analyseur (bootc par FIFO, flatpak, gwine,
    Homebrew) ; la barre vaut `(étapes terminées + fraction de l'étape
    courante) / total`, donc elle progresse aussi pendant une étape et ne
    dépasse jamais 100 %. Une étape en échec arrête la suite et laisse la
    fenêtre ouverte avec le journal.

    `steps` : liste de dicts `{label, argv, root?, hint?, fifo?, parser?, env?}`.
    """

    completed = Signal(bool)     # (succès) — émis à la fin, avant fermeture

    def __init__(self, parent=None, title: str = "", subtitle: str = "",
                 steps: list[dict] | None = None):
        super().__init__(parent)
        self._steps = [dict(step) for step in (steps or [])]
        self._index = -1
        self._process: QProcess | None = None
        self._output: list[str] = []
        self._step_output: list[str] = []
        self._hints: list[str] = []
        self._cancelled = False
        self._running = False
        self._parser = None
        self._line_buffer = ""
        self._step_percent: float | None = None
        self._progress_fifo = ""
        self._progress_fd = -1
        self._progress_notifier: QSocketNotifier | None = None
        self._progress_tries = 0
        self._progress_buffer = ""

        self.setWindowTitle(title or "Mise à jour complète")
        self.setModal(False)
        self.setMinimumWidth(620)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel(title or "Mise à jour complète", self)
        heading.setObjectName("title")
        root.addWidget(heading)

        if subtitle:
            detail = QLabel(subtitle, self)
            detail.setObjectName("subtitle")
            detail.setWordWrap(True)
            root.addWidget(detail)

        self.step_label = QLabel(self)
        self.step_label.setObjectName("hostName")
        self.step_label.setWordWrap(True)
        root.addWidget(self.step_label)

        self.detail_label = QLabel(self)
        self.detail_label.setObjectName("hostDetail")
        self.detail_label.setWordWrap(True)
        root.addWidget(self.detail_label)

        self.progress_bar = BusyProgressBar(self)
        self.progress_bar.setObjectName("taskProgress")
        self.progress_bar.setTextVisible(True)
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        root.addWidget(self.progress_bar)

        self.log_view = QPlainTextEdit(self)
        self.log_view.setReadOnly(True)
        self.log_view.setMinimumHeight(300)
        self.log_view.setMaximumBlockCount(20000)
        self.log_view.setVisible(False)
        root.addWidget(self.log_view, 1)

        self.result_label = QLabel(self)
        self.result_label.setWordWrap(True)
        self.result_label.setVisible(False)
        root.addWidget(self.result_label)

        buttons = QHBoxLayout()
        self.log_toggle = QPushButton("Terminal", self)
        self.log_toggle.setObjectName("ghost")
        self.log_toggle.setCheckable(True)
        self.log_toggle.setToolTip("Afficher le journal complet")
        self.log_toggle.toggled.connect(self._toggle_log)
        buttons.addWidget(self.log_toggle)
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

        QTimer.singleShot(60, self._start_next)

    @property
    def cancelled(self) -> bool:
        return self._cancelled

    def _toggle_log(self, shown: bool) -> None:
        self.log_view.setVisible(shown)
        self.log_toggle.setToolTip(
            "Masquer le journal" if shown else "Afficher le journal complet"
        )
        self.adjustSize()

    # -- séquence -----------------------------------------------------------
    def _start_next(self) -> None:
        self._index += 1
        if self._index >= len(self._steps):
            self._finish_all()
            return
        step = self._steps[self._index]
        self._step_percent = None
        self._parser = step.get("parser")
        self._line_buffer = ""
        self._step_output = []
        self._progress_buffer = ""
        self._progress_fifo = step.get("fifo") or ""
        self._progress_tries = 0
        self._running = True

        total = len(self._steps)
        self.step_label.setText(
            f"Étape {self._index + 1}/{total} — {step['label']}"
        )
        self.detail_label.setText("En cours…")
        self._apply_global()
        self.progress_bar.setVisible(True)

        argv = list(step["argv"])
        if step.get("root"):
            prefix = privileged_prefix()
            if prefix == ["pkexec"] and shutil.which("pkexec") is None:
                self._append(
                    "Ni sudo sans mot de passe ni pkexec : action impossible.\n"
                )
                self._fail(127)
                return
            argv = [*prefix, *argv]

        self.log_view.appendPlainText(
            f"\n== {step['label']} ==\n$ " + _display_command(argv) + "\n"
        )
        process = QProcess(self)
        process.setProcessChannelMode(QProcess.MergedChannels)
        env_extra = step.get("env")
        if env_extra:
            env = QProcessEnvironment.systemEnvironment()
            for key, value in env_extra.items():
                if value is None:
                    env.remove(key)
                else:
                    env.insert(key, value)
            process.setProcessEnvironment(env)
        process.setProgram(argv[0])
        process.setArguments(argv[1:])
        process.readyReadStandardOutput.connect(self._drain)
        process.finished.connect(self._step_finished)
        process.errorOccurred.connect(self._start_failed)
        if hasattr(process, "setChildProcessModifier"):
            process.setChildProcessModifier(os.setsid)
        self._process = process
        process.start()
        if self._progress_fifo:
            QTimer.singleShot(0, self._open_fifo)

    def _apply_global(self) -> None:
        total = max(1, len(self._steps))
        done = max(0, min(self._index, total))
        percent = self._step_percent
        fraction = 0.0 if percent is None else max(0.0, min(100.0, percent)) / 100.0
        value = 100.0 * (done + fraction) / total
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(int(round(value)))

    def _step_finished(self, code: int) -> None:
        self._running = False
        self._stop_progress()
        self._flush_lines()
        if self._cancelled:
            self._finish_with("Action annulée : les étapes suivantes ne sont pas lancées.",
                              ok=False)
            return
        step = self._steps[self._index]
        if code == 0:
            hint = step.get("hint")
            if hint is not None:
                extra = hint("".join(self._step_output)) or ""
                if extra:
                    self._hints.append(f"{step['label']} : {extra}")
            self._process = None
            QTimer.singleShot(0, self._start_next)
            return
        self._process = None
        # L'étape en échec n'est pas terminée : la barre globale reste au
        # plancher des étapes accomplies.
        self._step_percent = None
        self._apply_global()
        self._finish_with(
            f"Échec de « {step['label']} » (code {code}). "
            "Les étapes suivantes ne sont pas lancées.",
            ok=False,
        )
        if self.log_toggle is not None:
            self.log_toggle.setChecked(True)      # montrer la cause

    def _finish_all(self) -> None:
        self._running = False
        self._step_percent = 100.0
        self._index = len(self._steps) - 1
        self._apply_global()
        self.step_label.setText(
            f"Étape {len(self._steps)}/{len(self._steps)} — Terminé"
        )
        self.detail_label.setText("Toutes les mises à jour sont terminées.")
        extra = " ".join(self._hints)
        self.result_label.setObjectName("ok")
        self.result_label.setText("Terminé." + (f" {extra}" if extra else ""))
        self.result_label.style().unpolish(self.result_label)
        self.result_label.style().polish(self.result_label)
        self.result_label.setVisible(True)
        self.cancel_button.setVisible(False)
        self.close_button.setVisible(True)
        self.close_button.setDefault(True)
        self.completed.emit(True)

    def _finish_with(self, message: str, ok: bool) -> None:
        self.result_label.setObjectName("ok" if ok else "warn")
        self.result_label.setText(message)
        self.result_label.style().unpolish(self.result_label)
        self.result_label.style().polish(self.result_label)
        self.result_label.setVisible(True)
        self.cancel_button.setVisible(False)
        self.close_button.setVisible(True)
        self.close_button.setDefault(True)
        self.completed.emit(ok)

    def _start_failed(self, error) -> None:
        if error != QProcess.FailedToStart:
            return
        self._append(f"\nLancement impossible ({self._steps[self._index]['argv'][0]}).\n")
        self._fail(127)

    def _fail(self, code: int) -> None:
        self._running = False
        self._stop_progress()
        self._step_percent = None
        self._apply_global()
        label = self._steps[self._index]["label"] if self._index >= 0 else "étape"
        self._finish_with(
            f"Échec de « {label} » (code {code}). "
            "Les étapes suivantes ne sont pas lancées.",
            ok=False,
        )
        self.log_toggle.setChecked(True)

    # -- sortie -------------------------------------------------------------
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
        self._step_output.append(text)
        shown = self._read_lines(text)
        if not shown:
            return
        self.log_view.insertPlainText(shown)
        bar = self.log_view.verticalScrollBar()
        bar.setValue(bar.maximum())

    def _read_lines(self, text: str) -> str:
        if self._parser is None:
            return text
        self._line_buffer += text
        kept: list[str] = []
        while "\n" in self._line_buffer:
            line, _, self._line_buffer = self._line_buffer.partition("\n")
            info = self._parser(line)
            if info:
                self._apply_line(info)
                if info.get("hide"):
                    continue
            kept.append(line + "\n")
        return "".join(kept)

    def _flush_lines(self) -> None:
        if not self._line_buffer:
            return
        leftover, self._line_buffer = self._line_buffer, ""
        info = self._parser(leftover) if self._parser is not None else {}
        if info:
            self._apply_line(info)
            if info.get("hide"):
                return
        if leftover:
            self.log_view.insertPlainText(leftover)

    def _apply_line(self, info: dict) -> None:
        label = info.get("label")
        if label:
            self.detail_label.setText(label)
        percent = info.get("percent")
        self._step_percent = None if percent is None else float(percent)
        self._apply_global()

    # -- FIFO de bootc ------------------------------------------------------
    def _open_fifo(self) -> None:
        if self._progress_fd >= 0 or not self._progress_fifo or not self._running:
            return
        if not os.path.exists(self._progress_fifo):
            if self._progress_tries < 50:      # ~5 s, le temps que sudo démarre
                self._progress_tries += 1
                QTimer.singleShot(100, self._open_fifo)
            return
        try:
            fd = os.open(self._progress_fifo, os.O_RDONLY | os.O_NONBLOCK)
        except OSError:
            if self._progress_tries < 50:
                self._progress_tries += 1
                QTimer.singleShot(100, self._open_fifo)
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
                self._apply_progress_event(info)

    def _apply_progress_event(self, info: dict) -> None:
        label = info.get("label") or "Mise à jour de l'image…"
        total = int(info.get("bytes_total") or 0)
        if total > 0:
            label += " — " + _human_bytes(int(info.get("bytes") or 0)) \
                     + " / " + _human_bytes(total)
        self.detail_label.setText(label)
        self._step_percent = info.get("percent")
        self._apply_global()

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

    # -- annulation / fermeture --------------------------------------------
    def _cancel(self) -> None:
        if not self._running or self._process is None:
            return
        self._cancelled = True
        self.cancel_button.setEnabled(False)
        self._append("\n[annulation…]\n")
        self._process.terminate()
        QTimer.singleShot(4000, self._kill_if_running)

    def _kill_if_running(self) -> None:
        if self._running and self._process is not None:
            self._process.kill()

    def abort(self) -> None:
        if self._running and self._process is not None:
            self._cancelled = True
            self._process.terminate()
            if not self._process.waitForFinished(3000):
                self._process.kill()
                self._process.waitForFinished(1000)
        self._stop_progress()
        self.accept()

    def reject(self) -> None:
        if self._running and self._process is not None:
            answer = QMessageBox.question(
                self, "Mise à jour en cours",
                "Les mises à jour ne sont pas terminées. Les annuler et fermer ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                return
            self._cancelled = True
            self._process.terminate()
            if not self._process.waitForFinished(3000):
                self._process.kill()
                self._process.waitForFinished(1000)
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
