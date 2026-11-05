"""Fenêtre principale : liste des postes et actions.
"""

import os
import shutil
import signal
import subprocess
import tempfile
import time
import uuid
from pathlib import Path

from PySide6.QtCore import QEvent, QProcess, QSize, QTimer, Qt
from PySide6.QtGui import QIcon, QKeySequence, QPixmap, QShortcut
from PySide6.QtWidgets import (
    QDialog, QFrame, QHBoxLayout, QLabel, QMainWindow, QMenu, QMessageBox,
    QProgressBar, QPushButton, QScrollArea, QStackedWidget, QVBoxLayout, QWidget,
)

from .constants import (
    AVAHI_BROWSE, COLOR_AUTH, COLOR_OFFLINE, COLOR_ONLINE, HOSTS_FILE, PULSE_MS,
    REFRESH_MS, _which,
)
from .dialogs.apps import AppsDialog
from .dialogs.booster import ForegroundBoosterDialog
from .dialogs.gwine import GwineOptionsDialog
from .dialogs.host import CandidateDialog, HostDialog
from .dialogs.kernel import LocalSettingsDialog, RemoteSettingsDialog
from .dialogs.key import KeyInstallDialog
from .dialogs.tailscale import TailscaleDialog
from .hosts import detect_mac, load_hosts, parse_ssh_config, save_hosts, wake_on_lan
from .local import (
    _sudo_run, avahi_publish_args, avahi_publish_state, flatpak_update_script,
    local_sshd_state, set_local_sshd_sudo, write_avahi_service_temp,
)
from .ssh import ask_remote_password, remote_sudo_nopasswd, remote_sudo_prefix
from .style import make_qss
from .tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from .util import address_text, display_name, dolphin_url, normalize_mac, ssh_target
from .widgets import HostCard, LocalCard, StatusDot, _make_tool_button
from .workers import AvahiWorker, CheckWorker, KeyProbeWorker

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Gablue Manager")
        self.resize(980, 680)
        self.setMinimumSize(780, 520)
        self.setWindowIcon(QIcon.fromTheme("network-server"))

        self._hosts = load_hosts()
        self._states: dict[str, tuple[str, str]] = {}
        self._latency: dict[str, float] = {}
        self._cards: dict[str, HostCard] = {}
        self._dots: list[StatusDot] = []
        self._checker: CheckWorker | None = None
        self._jobs: list[dict] = []
        self._tasks: list[QDialog] = []      # fenêtres d'opération en cours
        self._key_queue: list[dict] = []     # postes à équiper après un import
        self._key_workers: set = set()       # sondes de clé en vol
        self._last_check: float | None = None
        self._display_order: list[str] = []
        self._discovering = False
        self._avahi_worker: AvahiWorker | None = None
        self._sshd_process: QProcess | None = None
        self._avahi_process: QProcess | None = None
        self._avahi_temp = ""

        self._build_ui()
        self.setStyleSheet(make_qss())
        self._rebuild_cards()
        self._update_sshd_button()
        self._update_avahi_button()

        self._pulse_timer = QTimer(self)
        self._pulse_timer.setInterval(PULSE_MS)
        self._pulse_timer.timeout.connect(self._tick_pulse)
        self._pulse_timer.start()

        self._refresh_timer = QTimer(self)
        self._refresh_timer.setInterval(REFRESH_MS)
        self._refresh_timer.timeout.connect(self.refresh_all)
        self._refresh_timer.start()

        self._job_timer = QTimer(self)
        self._job_timer.setInterval(1200)
        self._job_timer.timeout.connect(self._poll_jobs)
        self._job_timer.start()

        QTimer.singleShot(150, self.refresh_all)

    # -- construction -------------------------------------------------------
    def _build_ui(self) -> None:
        central = QWidget(self)
        self.setCentralWidget(central)
        root = QVBoxLayout(central)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        header = QWidget(central)
        header_layout = QHBoxLayout(header)
        header_layout.setContentsMargins(26, 22, 26, 14)
        header_layout.setSpacing(12)

        titles = QVBoxLayout()
        titles.setSpacing(0)
        title = QLabel("Gablue Manager", header)
        title.setObjectName("title")
        subtitle = QLabel(
            "Postes SSH, mises à jour et configuration", header
        )
        subtitle.setObjectName("subtitle")
        titles.addWidget(title)
        titles.addWidget(subtitle)
        header_layout.addLayout(titles)
        header_layout.addStretch(1)

        self.refresh_button = _make_tool_button(header, "view-refresh", "⟳")
        self.refresh_button.setToolTip("Vérifier maintenant (F5)")
        self.refresh_button.clicked.connect(self.refresh_all)
        header_layout.addWidget(self.refresh_button)

        # Un seul point d'entrée pour peupler la liste : la découverte mDNS et
        # l'import ~/.ssh/config sont des façons d'ajouter un poste, pas des
        # commandes à part dans un menu « ⋯ » séparé.
        self.add_button = QPushButton("Ajouter un poste", header)
        self.add_button.setObjectName("accent")
        self.add_button.setCursor(Qt.PointingHandCursor)
        self.add_button.setIcon(QIcon.fromTheme("list-add"))
        add_menu = QMenu(self.add_button)
        add_menu.addAction("Saisir un poste à la main…", self.add_host)
        add_menu.addAction(
            "Rechercher sur le réseau (mDNS)…", self.discover_hosts
        )
        add_menu.addAction(
            "Importer depuis ~/.ssh/config…", self.import_ssh_config
        )
        self.add_button.setMenu(add_menu)
        header_layout.addWidget(self.add_button)
        root.addWidget(header)

        self.busy = QProgressBar(central)
        self.busy.setObjectName("busy")
        self.busy.setRange(0, 0)
        self.busy.setTextVisible(False)
        self.busy.setFixedHeight(3)
        self.busy.setVisible(False)
        root.addWidget(self.busy)

        # Carte « Ce poste » : toujours visible, indépendante de la liste.
        self.local_card = LocalCard(central)
        self.local_card.update_requested.connect(self.local_update)
        self.local_card.apps_requested.connect(self.open_apps)
        self.local_card.config_requested.connect(self.open_settings)
        self.local_card.gwine_requested.connect(self.open_local_gwine_options)
        self.local_card.booster_requested.connect(
            self.open_local_foreground_booster
        )
        self.local_card.tailscale_requested.connect(
            self.open_local_tailscale
        )
        local_holder = QWidget(central)
        local_layout = QHBoxLayout(local_holder)
        local_layout.setContentsMargins(26, 0, 26, 6)
        local_layout.addWidget(self.local_card)
        root.addWidget(local_holder)

        self.stack = QStackedWidget(central)
        root.addWidget(self.stack, 1)

        # Page vide
        empty = QWidget(self.stack)
        empty_layout = QVBoxLayout(empty)
        empty_layout.setContentsMargins(40, 40, 40, 60)
        empty_layout.setSpacing(10)
        empty_layout.addStretch(1)
        icon = QLabel(empty)
        pixmap = QIcon.fromTheme("network-server").pixmap(QSize(72, 72))
        if pixmap.isNull():
            pixmap = QPixmap(72, 72)
            pixmap.fill(Qt.transparent)
        icon.setPixmap(pixmap)
        icon.setAlignment(Qt.AlignCenter)
        empty_layout.addWidget(icon)
        empty_title = QLabel("Aucun poste SSH", empty)
        empty_title.setObjectName("emptyTitle")
        empty_title.setAlignment(Qt.AlignCenter)
        empty_layout.addWidget(empty_title)
        empty_text = QLabel(
            "Ajoutez une machine pour y déposer votre clé, lancer son bureau "
            "distant (gremote) ou parcourir ses fichiers dans Dolphin.",
            empty,
        )
        empty_text.setObjectName("emptyText")
        empty_text.setAlignment(Qt.AlignCenter)
        empty_text.setWordWrap(True)
        empty_text.setMaximumWidth(520)
        # QLabel en wordWrap rapporte une hauteur d'une ligne : sans plancher,
        # le texte est rogné et les boutons se collent dessus.
        empty_text.setMinimumHeight(48)
        empty_layout.addWidget(empty_text, 0, Qt.AlignHCenter)
        empty_layout.addSpacing(8)
        empty_buttons = QHBoxLayout()
        empty_buttons.setSpacing(8)
        empty_buttons.addStretch(1)
        empty_add = QPushButton("Ajouter un poste", empty)
        empty_add.setObjectName("accent")
        empty_add.setCursor(Qt.PointingHandCursor)
        empty_add.clicked.connect(self.add_host)
        empty_buttons.addWidget(empty_add)
        empty_discover = QPushButton("Rechercher sur le réseau", empty)
        empty_discover.setCursor(Qt.PointingHandCursor)
        empty_discover.clicked.connect(self.discover_hosts)
        empty_buttons.addWidget(empty_discover)
        empty_buttons.addStretch(1)
        empty_layout.addLayout(empty_buttons)
        empty_layout.addStretch(2)
        self.stack.addWidget(empty)

        # Page liste
        self.scroll = QScrollArea(self.stack)
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.scroll.viewport().setAutoFillBackground(False)
        content = QWidget(self.scroll)
        content.setAutoFillBackground(False)
        self.cards_layout = QVBoxLayout(content)
        self.cards_layout.setContentsMargins(26, 6, 26, 18)
        self.cards_layout.setSpacing(10)
        self.cards_layout.addStretch(1)
        self.scroll.setWidget(content)
        self.stack.addWidget(self.scroll)

        footer = QWidget(central)
        footer_layout = QHBoxLayout(footer)
        footer_layout.setContentsMargins(26, 6, 26, 14)
        self.footer_label = QLabel(footer)
        self.footer_label.setObjectName("footer")
        footer_layout.addWidget(self.footer_label)
        footer_layout.addStretch(1)
        self.sshd_button = QPushButton(footer)
        self.sshd_button.setObjectName("pill")
        self.sshd_button.setCursor(Qt.PointingHandCursor)
        self.sshd_button.clicked.connect(self.toggle_local_sshd)
        footer_layout.addWidget(self.sshd_button)
        footer_layout.addSpacing(6)

        self.avahi_button = QPushButton(footer)
        self.avahi_button.setObjectName("pill")
        self.avahi_button.setCursor(Qt.PointingHandCursor)
        self.avahi_button.clicked.connect(self.toggle_avahi)
        footer_layout.addWidget(self.avahi_button)
        footer_layout.addSpacing(10)
        self.auto_label = QLabel(
            f"Vérification automatique toutes les {REFRESH_MS // 1000} s",
            footer,
        )
        self.auto_label.setObjectName("footer")
        footer_layout.addWidget(self.auto_label)
        root.addWidget(footer)

        for sequence, slot in (("Ctrl+N", self.add_host),
                               ("F5", self.refresh_all)):
            shortcut = QShortcut(QKeySequence(sequence), self)
            shortcut.activated.connect(slot)

        self._update_footer()

    # -- registre -----------------------------------------------------------
    def _save(self) -> None:
        try:
            save_hosts(self._hosts)
        except OSError as exc:
            QMessageBox.critical(
                self, "Enregistrement impossible",
                f"Le registre {HOSTS_FILE} n'a pas pu être écrit :\n{exc}",
            )

    def _host(self, host_id: str) -> dict | None:
        for host in self._hosts:
            if host["id"] == host_id:
                return host
        return None

    # -- cartes -------------------------------------------------------------
    def _rebuild_cards(self) -> None:
        while self.cards_layout.count():
            item = self.cards_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        self._cards.clear()
        self._dots.clear()

        for host in self._sorted_hosts():
            card = HostCard(host, self.scroll.widget())
            card.remote_requested.connect(self.launch_remote)
            card.remote_stop_requested.connect(self.stop_remote)
            card.files_requested.connect(self.open_files)
            card.key_requested.connect(self.install_key_dialog)
            card.edit_requested.connect(self.edit_host)
            card.delete_requested.connect(self.delete_host)
            card.terminal_requested.connect(self.open_terminal)
            card.wake_requested.connect(self.wake_host)
            card.update_requested.connect(self.update_host)
            card.settings_requested.connect(self.open_remote_settings)
            card.gwine_requested.connect(self.open_remote_gwine_options)
            card.booster_requested.connect(self.open_remote_foreground_booster)
            card.tailscale_requested.connect(self.open_remote_tailscale)
            card.apps_requested.connect(self.open_host_apps)
            card.reboot_requested.connect(self.reboot_host)
            card.shutdown_requested.connect(self.shutdown_host)
            state, detail = self._states.get(host["id"], ("unknown", ""))
            card.set_state(state, detail, self._latency.get(host["id"], 0.0))
            card.set_remote_active(self._remote_job(host["id"]) is not None)
            self.cards_layout.addWidget(card)
            self._cards[host["id"]] = card
            self._dots.append(card.dot)

        self.cards_layout.addStretch(1)
        self._display_order = [host["id"] for host in self._sorted_hosts()]
        self.stack.setCurrentIndex(0 if not self._hosts else 1)
        self._update_footer()

    def _sorted_hosts(self) -> list[dict]:
        """Postes en ligne d'abord, puis par nom."""
        def key(host: dict):
            state = self._states.get(host["id"], ("unknown", ""))[0]
            return (0 if state == "online" else 1, display_name(host).casefold())
        return sorted(self._hosts, key=key)

    def _tick_pulse(self) -> None:
        for dot in self._dots:
            dot.tick()

    # -- vérification -------------------------------------------------------
    def refresh_all(self) -> None:
        self._update_sshd_button()
        self._update_avahi_button()
        if self._checker is not None and self._checker.isRunning():
            return
        if not self._hosts:
            self._update_footer()
            return
        for host in self._hosts:
            self._states[host["id"]] = ("checking", "")
        self._apply_states()
        self.busy.setVisible(True)

        checker = CheckWorker(self._hosts, self)
        checker.result.connect(self._on_check_result)
        checker.finished_all.connect(self._on_check_done)
        self._checker = checker
        checker.start()

    def _on_check_result(self, host_id: str, state: str, detail: str,
                         latency: float, mac: str) -> None:
        if state == "unknown":
            return
        self._states[host_id] = (state, detail)
        self._latency[host_id] = latency
        host = self._host(host_id)
        if host is not None and mac and not host.get("mac"):
            # MAC capturée au premier contact réussi : indispensable au réveil
            # (WoL) une fois le poste éteint. Enregistrée une seule fois.
            host["mac"] = mac
            self._save()
        card = self._cards.get(host_id)
        if card is not None:
            card.set_state(state, detail, latency)
        self._update_footer()

    def _on_check_done(self) -> None:
        self.busy.setVisible(False)
        self._last_check = time.time()
        self._update_footer()
        if self._checker is not None:
            # finished_all part de run() : on laisse le thread sortir avant de
            # détruire l'objet, sinon « QThread: Destroyed while running ».
            self._checker.wait(2000)
            self._checker.deleteLater()
            self._checker = None
        # Tri « en ligne d'abord » : on ne réordonne qu'à la fin d'une passe,
        # pas pendant, sinon les cartes sautent sous le curseur.
        if [host["id"] for host in self._sorted_hosts()] != self._display_order:
            self._rebuild_cards()

    def _apply_states(self) -> None:
        for host_id, card in self._cards.items():
            state, detail = self._states.get(host_id, ("unknown", ""))
            card.set_state(state, detail, self._latency.get(host_id, 0.0))

    def _update_footer(self) -> None:
        total = len(self._hosts)
        online = sum(
            1 for host in self._hosts
            if self._states.get(host["id"], ("", ""))[0] == "online"
        )
        if total == 0:
            self.footer_label.setText("Aucun poste enregistré")
        else:
            noun = "poste" if total == 1 else "postes"
            self.footer_label.setText(f"{total} {noun} · {online} en ligne")
        if self._last_check:
            self.auto_label.setText(
                "Vérifié à "
                + time.strftime("%H:%M:%S", time.localtime(self._last_check))
                + f" · auto toutes les {REFRESH_MS // 1000} s"
            )

    # -- serveur SSH local --------------------------------------------------
    def _pending(self, process: QProcess | None) -> bool:
        """Une bascule privilégiée attend PolicyKit : le libellé reste figé."""
        return process is not None and process.state() != QProcess.NotRunning

    def _update_sshd_button(self) -> None:
        if self._pending(self._sshd_process):
            return
        active, enabled = local_sshd_state()
        if active:
            self.sshd_button.setText("Serveur SSH : activé")
            self.sshd_button.setStyleSheet(f"color: {COLOR_ONLINE};")
        else:
            self.sshd_button.setText("Serveur SSH : désactivé")
            self.sshd_button.setStyleSheet(f"color: {COLOR_OFFLINE};")
        boot = "démarre au boot" if enabled else "ne démarre pas au boot"
        self.sshd_button.setToolTip(
            f"Serveur SSH de cette machine ({boot})\n"
            + ("Cliquer pour le couper" if active else "Cliquer pour l'activer")
        )

    def _update_avahi_button(self) -> None:
        if self._pending(self._avahi_process):
            return
        published, daemon = avahi_publish_state()
        if not daemon:
            self.avahi_button.setText("Annonce mDNS : avahi arrêté")
            self.avahi_button.setStyleSheet(f"color: {COLOR_AUTH};")
        elif published:
            self.avahi_button.setText("Annonce mDNS : activée")
            self.avahi_button.setStyleSheet(f"color: {COLOR_ONLINE};")
        else:
            self.avahi_button.setText("Annonce mDNS : désactivée")
            self.avahi_button.setStyleSheet(f"color: {COLOR_OFFLINE};")
        self.avahi_button.setToolTip(
            "Annonce ce poste en SSH (_ssh._tcp, port 22) en mDNS.\n"
            "Visible uniquement depuis le même réseau local (nom d'hôte, "
            "adresse IP et port — aucun compte ni clé).\n"
            + ("Cliquer pour ne plus l'annoncer" if published
               else "Cliquer pour l'annoncer")
        )

    def toggle_local_sshd(self) -> None:
        active, _enabled = local_sshd_state()
        if active:
            answer = QMessageBox.question(
                self, "Serveur SSH local",
                "Couper le serveur SSH de cette machine ?\n\n"
                "Les connexions entrantes seront refusées : bureau distant "
                "(gremote) et fichiers depuis un autre poste, ainsi que SSH "
                "vers ce poste, ne fonctionneront plus.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
        else:
            answer = QMessageBox.question(
                self, "Serveur SSH local",
                "Activer le serveur SSH de cette machine ?\n\n"
                "Ce poste acceptera les connexions SSH entrantes (avec la clé "
                "publique de l'autre poste) et pourra servir de cible à "
                "gremote et au partage de fichiers.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
        if answer != QMessageBox.Yes:
            return
        enabled = not active

        # Chemin rapide : sudo sans mot de passe (image Gablue). Sinon PolicyKit.
        ok, _detail = set_local_sshd_sudo(enabled)
        if ok:
            self._update_sshd_button()
            return

        if shutil.which("pkexec") is None:
            QMessageBox.critical(
                self, "Serveur SSH local",
                "Ni sudo sans mot de passe ni pkexec ne sont disponibles sur "
                "ce poste : bascule impossible.",
            )
            return

        action = "enable" if enabled else "disable"
        self.sshd_button.setEnabled(False)
        self.sshd_button.setText("Serveur SSH local : autorisation…")
        process = QProcess(self)
        process.setProgram("pkexec")
        process.setArguments(
            [_which("systemctl"), action, "sshd.service", "--now"]
        )
        process.finished.connect(self._sshd_toggled)
        process.errorOccurred.connect(self._sshd_start_failed)
        self._sshd_process = process
        process.start()

    def _sshd_start_failed(self, error) -> None:
        # finished() n'est PAS émis quand le programme n'a pas pu démarrer.
        if error != QProcess.FailedToStart:
            return
        self._sshd_process = None
        self.sshd_button.setEnabled(True)
        QMessageBox.critical(
            self, "Serveur SSH local", "pkexec n'a pas pu démarrer.",
        )
        self._update_sshd_button()

    def _sshd_toggled(self, code: int, _status) -> None:
        self._sshd_process = None
        self.sshd_button.setEnabled(True)
        if code != 0:
            reasons = {
                126: "autorisation refusée",
                127: "non autorisé (PolicyKit)",
                125: "dialogue PolicyKit absent ou annulé",
            }
            QMessageBox.critical(
                self, "Serveur SSH local",
                "Échec de la bascule : "
                + reasons.get(code, f"code {code}") + ".",
            )
        self._update_sshd_button()

    # -- annonce mDNS de ce poste -------------------------------------------
    def toggle_avahi(self) -> None:
        published, daemon = avahi_publish_state()
        if not daemon:
            QMessageBox.information(
                self, "Annonce mDNS",
                "avahi-daemon n'est pas actif sur ce poste : l'annonce ne "
                "servirait à rien.\n\nL'activer avec :\n"
                "    sudo systemctl enable --now avahi-daemon",
            )
            return
        if published:
            answer = QMessageBox.question(
                self, "Annonce mDNS",
                "Ne plus annoncer ce poste en SSH sur le réseau local ?\n\n"
                "La découverte automatique depuis les autres postes ne le "
                "trouvera plus ; l'accès SSH reste possible en saisissant "
                "l'adresse à la main.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
        else:
            answer = QMessageBox.question(
                self, "Annonce mDNS",
                "Annoncer ce poste en SSH (_ssh._tcp) sur le réseau local ?\n\n"
                "Les autres postes du même réseau verront son nom d'hôte, son "
                "adresse IP et le port 22. Aucun compte ni clé n'est publié, "
                "et rien ne sort du réseau local.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
        if answer != QMessageBox.Yes:
            return

        enabled = not published
        temp_path = write_avahi_service_temp() if enabled else ""
        argv = avahi_publish_args(enabled, temp_path)
        argv[0] = _which(argv[0])

        ok, _detail = _sudo_run(argv)
        if ok:
            self._cleanup_avahi_temp(temp_path)
            self._update_avahi_button()
            return

        if shutil.which("pkexec") is None:
            self._cleanup_avahi_temp(temp_path)
            QMessageBox.critical(
                self, "Annonce mDNS",
                "Ni sudo sans mot de passe ni pkexec ne sont disponibles sur "
                "ce poste : bascule impossible.",
            )
            return

        self._avahi_temp = temp_path
        self.avahi_button.setEnabled(False)
        self.avahi_button.setText("Annonce mDNS : autorisation…")
        process = QProcess(self)
        process.setProgram("pkexec")
        process.setArguments(argv)
        process.finished.connect(self._avahi_toggled)
        process.errorOccurred.connect(self._avahi_start_failed)
        self._avahi_process = process
        process.start()

    def _cleanup_avahi_temp(self, path: str = "") -> None:
        path = path or self._avahi_temp
        self._avahi_temp = ""
        if path:
            try:
                os.unlink(path)
            except OSError:
                pass

    def _avahi_toggled(self, code: int, _status) -> None:
        self._cleanup_avahi_temp()
        self._avahi_process = None
        self.avahi_button.setEnabled(True)
        if code != 0:
            reasons = {
                126: "autorisation refusée",
                127: "non autorisé (PolicyKit)",
                125: "dialogue PolicyKit absent ou annulé",
            }
            QMessageBox.critical(
                self, "Annonce mDNS",
                "Échec de la bascule : "
                + reasons.get(code, f"code {code}") + ".",
            )
        self._update_avahi_button()

    def _avahi_start_failed(self, error) -> None:
        if error != QProcess.FailedToStart:
            return
        self._cleanup_avahi_temp()
        self._avahi_process = None
        self.avahi_button.setEnabled(True)
        QMessageBox.critical(
            self, "Annonce mDNS", "pkexec n'a pas pu démarrer.",
        )
        self._update_avahi_button()

    # -- actions ------------------------------------------------------------
    def add_host(self) -> None:
        dialog = HostDialog(self, existing=self._hosts)
        if dialog.exec() != QDialog.Accepted:
            return
        host = dialog.result_record
        self._hosts.append(host)
        self._save()
        self._states[host["id"]] = ("unknown", "")
        self._rebuild_cards()
        self.refresh_all()
        # L'ajout inclut la copie de la clé : le dialogue de clé vérifie
        # d'abord si elle est déjà acceptée (aucun mot de passe dans ce cas).
        self.install_key_dialog(host["id"], password=dialog.password())

    def edit_host(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        dialog = HostDialog(self, host=host, existing=self._hosts)
        if dialog.exec() != QDialog.Accepted:
            return
        updated = dialog.result_record
        self._hosts = [
            updated if item["id"] == host_id else item for item in self._hosts
        ]
        self._save()
        self._rebuild_cards()
        self._states.pop(host_id, None)
        self.refresh_all()

    def delete_host(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        answer = QMessageBox.question(
            self, "Supprimer le poste",
            f"Retirer « {display_name(host)} » ({address_text(host)}) de la "
            "liste ?\n\nLa clé reste sur la machine distante et dans "
            "~/.ssh/known_hosts ; rien d'autre n'est supprimé.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        job = self._remote_job(host_id)
        if job is not None:
            self._stop_job(job)
        self._hosts = [item for item in self._hosts if item["id"] != host_id]
        self._states.pop(host_id, None)
        self._save()
        self._rebuild_cards()
        self._update_footer()

    def install_key_dialog(self, host_id: str, password: str = "") -> None:
        host = self._host(host_id)
        if host is None:
            return
        dialog = KeyInstallDialog(self, host=host, password=password)
        dialog.installed.connect(lambda: self._key_installed(host_id))
        start_task(self, dialog)

    def _install_keys(self, hosts: list[dict]) -> None:
        """Copie la clé sur des postes importés, sans fenêtre si elle est déjà là.

        Une sonde muette par poste (thread, jamais le thread GUI) : la clé est
        enregistrée comme installée quand elle passe, et le dialogue de copie ne
        s'ouvre que pour les postes qui la refusent encore. Les dialogues sont
        enchaînés (une fenêtre à la fois) pour ne pas empiler les mots de passe.
        """
        self._key_queue = list(hosts)
        self._probe_next_key()

    def _probe_next_key(self) -> None:
        if not self._key_queue:
            return
        host = self._key_queue.pop(0)
        worker = KeyProbeWorker(host, self)
        self._key_workers.add(worker)
        worker.finished.connect(lambda w=worker: self._key_workers.discard(w))
        worker.done.connect(
            lambda ok, _detail, h=dict(host): self._key_probed(h, ok)
        )
        worker.start()

    def _key_probed(self, host: dict, ok: bool) -> None:
        if ok:
            self._key_installed(host["id"])
            self._probe_next_key()
            return
        dialog = KeyInstallDialog(self, host=host)
        dialog.installed.connect(lambda h=host["id"]: self._key_installed(h))
        # La suite attend la fermeture de cette fenêtre : un mot de passe à la
        # fois, et l'utilisateur peut renoncer sans bloquer les suivants.
        dialog.finished.connect(lambda _code: self._probe_next_key())
        start_task(self, dialog)

    def _key_installed(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        host["key_added"] = True
        self._save()
        self._states[host_id] = ("checking", "")
        self._apply_states()
        self.refresh_all()

    def launch_remote(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        card = self._cards.get(host_id)
        gremote = shutil.which("gremote")
        if gremote is None and Path("/usr/bin/gremote").exists():
            gremote = "/usr/bin/gremote"
        if gremote is None:
            QMessageBox.critical(
                self, "gremote introuvable",
                "gremote n'est pas installé sur ce poste.",
            )
            return
        command = [gremote]
        if host["port"] != 22:
            command += ["-p", str(host["port"])]
        command.append(ssh_target(host))
        # Chaque clic ouvre une nouvelle fenêtre, même si une session tourne
        # déjà : KRdp ne garde qu'une session par utilisateur, le nouveau
        # client coupe le précédent et sort en 2 (STATE_RUN_QUIT) — ce n'est
        # pas un échec à annoncer. 131 = sdl-freerdp à la fermeture normale.
        launched = self._launch(
            command, f"Bureau distant — {display_name(host)}",
            ignore_codes={2, 131}, host_id=host_id, kind="remote",
        )
        if launched is not None and card is not None:
            card.set_remote_active(True)

    def stop_remote(self, host_id: str) -> None:
        """Ferme les sessions RDP lancées par l'app (menu ⋯ de la carte)."""
        host = self._host(host_id)
        jobs = [
            job for job in self._jobs
            if job.get("kind") == "remote" and job.get("host_id") == host_id
            and job["process"].poll() is None
        ]
        if host is None or not jobs:
            return
        answer = QMessageBox.question(
            self, "Fermer la session",
            f"Fermer la session de bureau distant ouverte vers "
            f"« {display_name(host)} » ?\n\nLa fenêtre RDP va se fermer.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        for job in jobs:
            self._stop_job(job)
        card = self._cards.get(host_id)
        if card is not None:
            card.set_remote_active(False)
        self.auto_label.setText(f"Session fermée — {display_name(host)}")

    def open_files(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        url = dolphin_url(host)
        if shutil.which("dolphin"):
            command = ["dolphin", url]
        else:
            command = ["xdg-open", url]
        self._launch(command, f"Fichiers — {display_name(host)}")

    def open_terminal(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        ssh = shutil.which("ssh") or "ssh"
        remote = [ssh]
        if host["port"] != 22:
            remote += ["-p", str(host["port"])]
        remote.append(ssh_target(host))
        if shutil.which("konsole"):
            command = ["konsole", "-e", *remote]
        elif shutil.which("xdg-terminal-exec"):
            command = ["xdg-terminal-exec", *remote]
        else:
            QMessageBox.critical(
                self, "Terminal SSH",
                "Aucun terminal trouvé (konsole ou xdg-terminal-exec).",
            )
            return
        self._launch(command, f"Terminal — {display_name(host)}")

    def wake_host(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        if not host.get("mac"):
            QMessageBox.information(
                self, "Wake-on-LAN",
                "Aucune adresse MAC n'est enregistrée pour ce poste.\n\n"
                "Elle est capturée automatiquement dès qu'une connexion "
                "réussit ; sinon renseignez-la dans « Modifier ».",
            )
            return
        ok, error = wake_on_lan(host)
        if not ok:
            QMessageBox.warning(
                self, "Wake-on-LAN", f"Envoi du paquet magique impossible :\n{error}"
            )
            return
        self.auto_label.setText(f"Réveil envoyé à {display_name(host)}…")
        self._states[host_id] = ("checking", "réveil envoyé")
        self._apply_states()
        # Démarrage + sshd : on re-vérifie plusieurs fois plutôt qu'une seule.
        for delay in (3000, 7000, 12000, 20000, 30000):
            QTimer.singleShot(delay, self.refresh_all)

    def update_host(self, host_id: str, kind: str = "gablue") -> None:
        host = self._host(host_id)
        if host is None:
            return
        if kind == "gablue":
            self.update_remote_gablue(host)
        elif kind == "flatpak":
            self.update_remote_flatpaks(host)
        elif kind == "gwine":
            self.update_remote_gwine(host)

    def update_remote_gablue(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour Gablue",
            f"Télécharger la nouvelle image Gablue de « {display_name(host)} » "
            f"({address_text(host)}) ?\n\n"
            "Le poste continue de tourner ; l'image s'applique à son prochain "
            "redémarrage. Les Flatpaks se mettent à jour séparément.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        script = (
            "sudo -n /usr/bin/bootc upgrade\n"
            "rc=$?\n"
            "echo '---STATUS---'\n"
            "sudo -n /usr/bin/bootc status 2>/dev/null || true\n"
            "exit $rc\n"
        )
        self._run_remote_script(
            host, f"Mise à jour — {display_name(host)}",
            "bootc upgrade, appliqué au prochain redémarrage.", script,
            root=True,
            hint_from_output=lambda text: (
                "Redémarrage nécessaire pour appliquer la nouvelle image."
                if "staged" in text.lower() else "Le poste est à jour."
            ),
        )

    def update_remote_flatpaks(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour les Flatpaks",
            f"Mettre à jour les applications Flatpak de "
            f"« {display_name(host)} » ({address_text(host)}) ?\n\n"
            "Compte et système. La partie système est autorisée sans mot de "
            "passe sur un poste Gablue (règle sudoers de l'image) ; si elle "
            "manque, le journal le dit au lieu de demander le mot de passe.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Flatpaks — {display_name(host)}",
            "flatpak update --user puis --system.", flatpak_update_script(),
        )

    def update_remote_gwine(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour gwine",
            f"Mettre à jour gwine sur « {display_name(host)} » "
            f"({address_text(host)}) ?\n\n"
            "gwine --init réinstalle le runner et ses composants (DXVK, VKD3D) "
            "puis vérifie le préfixe. Le téléchargement peut prendre plusieurs "
            "minutes.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"gwine — {display_name(host)}",
            "gwine --init — runner et composants Windows.",
            "gwine --init\n",
        )

    def open_remote_settings(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        RemoteSettingsDialog(self, host).exec()

    def _run_remote_script(self, host: dict, title: str, subtitle: str,
                           script: str, root: bool = False,
                           ok_if_disconnect: bool = False,
                           hint_from_output=None) -> None:
        """Exécute un script distant, en demandant le mot de passe sudo si besoin."""
        password = ""
        if root and not remote_sudo_nopasswd(host):
            password = ask_remote_password(self, host)
            if password is None:
                return
        command = (remote_sudo_prefix(password) if root else "") + script
        dialog = RemoteTaskDialog(
            self, host=host, title=title, subtitle=subtitle, command=command,
            password=password, ok_if_disconnect=ok_if_disconnect,
            hint_from_output=hint_from_output,
        )
        start_task(self, dialog)

    def reboot_host(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        answer = QMessageBox.question(
            self, "Redémarrer le poste",
            f"Redémarrer « {display_name(host)} » ({address_text(host)}) ?\n\n"
            "Toutes les sessions ouvertes sur ce poste seront interrompues.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Redémarrage — {display_name(host)}",
            "systemctl reboot via SSH.", "sudo -n /usr/bin/systemctl reboot\n",
            root=True, ok_if_disconnect=True,
        )

    def shutdown_host(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        answer = QMessageBox.question(
            self, "Éteindre le poste",
            f"Éteindre « {display_name(host)} » ({address_text(host)}) ?\n\n"
            "Toutes les sessions ouvertes sur ce poste seront interrompues. "
            "Il ne redémarrera qu'à la main ou par Wake-on-LAN.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Extinction — {display_name(host)}",
            "systemctl poweroff via SSH.",
            "sudo -n /usr/bin/systemctl poweroff\n",
            root=True, ok_if_disconnect=True,
        )

    def _run_remote(self, host: dict, title: str, subtitle: str, command: str,
                    ok_if_disconnect: bool = False,
                    hint_from_output=None) -> None:
        dialog = RemoteTaskDialog(
            self, host=host, title=title, subtitle=subtitle, command=command,
            ok_if_disconnect=ok_if_disconnect,
            hint_from_output=hint_from_output,
        )
        start_task(self, dialog)

    # -- machine courante : mises à jour et configuration --------------------
    def local_update(self, kind: str) -> None:
        if kind == "gablue":
            self.update_local_gablue()
        elif kind == "flatpak":
            self.update_local_flatpaks()
        elif kind == "gwine":
            self.update_local_gwine()

    def update_local_gablue(self) -> None:
        """Image système Gablue (bootc), appliquée au prochain démarrage."""
        answer = QMessageBox.question(
            self, "Mise à jour de Gablue",
            "Télécharger la nouvelle image système Gablue ?\n\n"
            "Le poste continue de tourner normalement ; l'image s'applique au "
            "prochain redémarrage. Les Flatpaks se mettent à jour séparément.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_local_task(
            "Mise à jour de Gablue",
            "bootc upgrade — l'image téléchargée s'applique au prochain "
            "redémarrage.",
            [_which("bootc"), "upgrade"], root=True,
            hint_from_output=lambda text: (
                "Redémarrage nécessaire pour appliquer la nouvelle image."
                if "staged" in text.lower() or "staging" in text.lower()
                else ""
            ),
        )

    def update_local_flatpaks(self) -> None:
        flatpak = shutil.which("flatpak")
        if flatpak is None:
            QMessageBox.critical(
                self, "Flatpak", "flatpak n'est pas installé sur ce poste."
            )
            return
        answer = QMessageBox.question(
            self, "Mise à jour des Flatpaks",
            "Mettre à jour les applications Flatpak ?\n\n"
            "Les applications du compte et du système sont concernées. La "
            "partie système passe par sudo : sur Gablue elle est autorisée "
            "sans mot de passe, ailleurs PolicyKit le demandera.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_local_task(
            "Mise à jour des Flatpaks",
            "flatpak update --user puis --system.",
            [_which("bash"), "-c", flatpak_update_script()],
        )

    def update_local_gwine(self) -> None:
        gwine = shutil.which("gwine") or (
            "/usr/bin/gwine" if Path("/usr/bin/gwine").exists() else ""
        )
        if not gwine:
            QMessageBox.critical(
                self, "gwine", "gwine n'est pas installé sur ce poste."
            )
            return
        answer = QMessageBox.question(
            self, "Mise à jour de gwine",
            "Mettre à jour gwine ?\n\n"
            "gwine --init réinstalle le runner Wine et ses composants "
            "(DXVK, VKD3D) puis vérifie le préfixe. Le téléchargement peut "
            "prendre plusieurs minutes.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_local_task(
            "Mise à jour de gwine",
            "gwine --init — runner et composants Windows.",
            [gwine, "--init"],
        )

    def open_apps(self) -> None:
        # Non modal : les installations tournent dans leurs propres fenêtres et
        # on peut en lancer plusieurs de suite, ou continuer ailleurs.
        AppsDialog(self).show()

    def open_host_apps(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        AppsDialog(self, host).show()

    def open_settings(self) -> None:
        LocalSettingsDialog(self).exec()

    def open_local_gwine_options(self) -> None:
        GwineOptionsDialog(self).exec()

    def open_remote_gwine_options(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        GwineOptionsDialog(self, host).exec()

    def open_local_foreground_booster(self) -> None:
        ForegroundBoosterDialog(self).exec()

    def open_remote_foreground_booster(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        ForegroundBoosterDialog(self, host).exec()

    def open_local_tailscale(self) -> None:
        TailscaleDialog(self).exec()

    def open_remote_tailscale(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        TailscaleDialog(self, host).exec()

    def _run_local_task(self, title: str, subtitle: str, argv: list[str],
                        root: bool = False, hint_from_output=None) -> None:
        if argv and shutil.which(argv[0]) is None and not Path(argv[0]).exists():
            QMessageBox.critical(
                self, title,
                f"« {argv[0]} » introuvable sur ce poste.",
            )
            return
        dialog = LocalTaskDialog(
            self, title=title, subtitle=subtitle, argv=argv, root=root,
            hint_from_output=hint_from_output,
        )
        start_task(self, dialog)

    # -- découverte / import ------------------------------------------------
    def discover_hosts(self) -> None:
        if self._discovering:
            return
        if AVAHI_BROWSE == "avahi-browse" and not shutil.which("avahi-browse"):
            QMessageBox.information(
                self, "Découverte Avahi",
                "avahi-browse n'est pas installé (paquet avahi-tools).",
            )
            return
        self._discovering = True
        self.busy.setVisible(True)
        self.auto_label.setText("Recherche de services SSH sur le réseau…")
        worker = AvahiWorker(self)
        worker.found.connect(self._discovery_done)
        worker.finished.connect(worker.deleteLater)
        self._avahi_worker = worker
        worker.start()

    def _discovery_done(self, hosts: list) -> None:
        self._discovering = False
        self._avahi_worker = None
        self.busy.setVisible(False)
        self._update_footer()
        if not hosts:
            QMessageBox.information(
                self, "Découverte Avahi",
                "Aucun service SSH annoncé sur le réseau local.\n\n"
                "Vérifiez qu'avahi-daemon tourne sur les postes et qu'ils sont "
                "sur le même réseau.",
            )
            return
        self._pick_candidates(
            "Postes découverts sur le réseau",
            "Services SSH annoncés en mDNS (Avahi). Rien n'est ajouté sans "
            "votre accord : cochez les postes à inscrire dans votre liste.",
            hosts,
        )

    def import_ssh_config(self) -> None:
        candidates = parse_ssh_config()
        if not candidates:
            QMessageBox.information(
                self, "Import ~/.ssh/config",
                "Aucun hôte concret trouvé dans ~/.ssh/config\n"
                "(les motifs * ? ! sont ignorés).",
            )
            return
        self._pick_candidates(
            "Importer depuis ~/.ssh/config",
            "Hôtes déclarés dans ~/.ssh/config. Cochez ceux à ajouter.",
            candidates,
        )

    def _pick_candidates(self, title: str, subtitle: str,
                        candidates: list[dict]) -> None:
        dialog = CandidateDialog(self, title, subtitle, candidates, self._hosts)
        if dialog.exec() != QDialog.Accepted:
            return
        added: list[dict] = []
        for candidate in dialog.selected:
            record = self._add_record(candidate)
            if record is not None:
                added.append(record)
        if added:
            self._save()
            self._rebuild_cards()
            self.refresh_all()
            # La clé est copiée automatiquement sur les postes importés.
            self._install_keys(added)

    def _add_record(self, candidate: dict) -> dict | None:
        host = {
            "id": uuid.uuid4().hex,
            "label": str(candidate.get("label") or "").strip(),
            "user": str(candidate["user"]).strip(),
            "host": str(candidate["host"]).strip(),
            "port": int(candidate.get("port", 22)),
            "mac": normalize_mac(candidate.get("mac", "")) or detect_mac(candidate),
            "key_added": False,
            "added_at": int(time.time()),
        }
        if not host["user"] or not host["host"]:
            return None
        for other in self._hosts:
            if (other["user"], other["host"], other["port"]) == (
                host["user"], host["host"], host["port"]
            ):
                return None
        self._hosts.append(host)
        self._states[host["id"]] = ("unknown", "")
        return host

    def _launch(self, command: list[str], label: str,
                ignore_codes: set[int] | None = None,
                host_id: str = "", kind: str = "") -> dict | None:
        """Lance un outil externe et signale un échec sans bloquer la session.

        `ignore_codes` : codes de sortie qui ne sont pas des échecs (ex. la
        fermeture d'une fenêtre RDP), pour ne pas afficher de faux dialogue.
        `host_id`/`kind` : permettent de retrouver une session déjà ouverte
        vers ce poste (bureau distant) et de l'arrêter depuis la carte.
        """
        executable = shutil.which(command[0]) or (
            command[0] if Path(command[0]).exists() else None
        )
        if executable is None:
            QMessageBox.critical(
                self, label, f"« {command[0]} » introuvable sur ce poste."
            )
            return None
        command[0] = executable
        handle = tempfile.NamedTemporaryFile(
            prefix="gablue-manager-", suffix=".log", delete=False
        )
        try:
            process = subprocess.Popen(
                command, stdin=subprocess.DEVNULL, stdout=handle,
                stderr=subprocess.STDOUT, start_new_session=True,
                close_fds=True,
            )
        except OSError as exc:
            handle.close()
            try:
                os.unlink(handle.name)
            except OSError:
                pass
            QMessageBox.critical(self, label, f"Lancement impossible :\n{exc}")
            return None
        finally:
            handle.close()
        job = {
            "process": process, "path": handle.name, "label": label,
            "ignore": set(ignore_codes or ()), "host_id": host_id,
            "kind": kind, "stopped": False,
        }
        self._jobs.append(job)
        return job

    def _remote_job(self, host_id: str) -> dict | None:
        """Session de bureau distant encore vivante pour ce poste."""
        for job in self._jobs:
            if job.get("kind") != "remote" or job.get("host_id") != host_id:
                continue
            if job["process"].poll() is None:
                return job
        return None

    def _stop_job(self, job: dict) -> None:
        """Ferme une session lancée par l'app (groupe de processus complet).

        SIGTERM d'abord : `gremote` nettoie son tunnel SSH, son puits audio et
        son serveur RDP. SIGKILL seulement s'il traîne encore après 3 s.
        """
        job["stopped"] = True
        process = job["process"]
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGTERM)
        except OSError:
            return

        def force() -> None:
            if process.poll() is None:
                try:
                    os.killpg(os.getpgid(process.pid), signal.SIGKILL)
                except OSError:
                    pass

        QTimer.singleShot(3000, force)

    def _poll_jobs(self) -> None:
        for job in list(self._jobs):
            code = job["process"].poll()
            if code is None:
                continue
            self._jobs.remove(job)
            path = Path(job["path"])
            if job.get("kind") == "remote":
                card = self._cards.get(job.get("host_id", ""))
                if card is not None:
                    card.set_remote_active(
                        self._remote_job(job.get("host_id", "")) is not None
                    )
            if (code != 0 and code not in job["ignore"]
                    and not job.get("stopped") and self.isVisible()):
                try:
                    lines = path.read_text(
                        encoding="utf-8", errors="replace"
                    ).splitlines()[-15:]
                    tail = "\n".join(line[:160] for line in lines)
                    tail = tail or "(aucune sortie)"
                except OSError:
                    tail = "(journal illisible)"
                QMessageBox.warning(
                    self, job["label"],
                    f"La commande a échoué (code {code}).\n\n{tail}",
                )
            try:
                path.unlink()
            except OSError:
                pass

    # -- focus / fermeture --------------------------------------------------
    def changeEvent(self, event) -> None:
        # Revenir sur la fenêtre rafraîchit l'état sans attendre les 30 s.
        if event.type() == QEvent.ActivationChange and self.isActiveWindow():
            if not self._last_check or time.time() - self._last_check > 8:
                self.refresh_all()
        super().changeEvent(event)

    def closeEvent(self, event) -> None:
        # Les tâches sont maintenant non modales : elles peuvent encore tourner
        # quand la fenêtre principale se ferme. Les laisser derrière soi
        # détruirait leur QProcess/QThread en vol.
        if self._tasks:
            answer = QMessageBox.question(
                self, "Opérations en cours",
                f"{len(self._tasks)} opération"
                f"{'s' if len(self._tasks) > 1 else ''} encore en cours."
                "\n\nLes annuler et quitter ?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                event.ignore()
                return
            for task in list(self._tasks):
                abort = getattr(task, "abort", None)
                if abort is not None:
                    abort()
                else:
                    task.accept()
            self._tasks.clear()

        self._refresh_timer.stop()
        self._pulse_timer.stop()
        self._job_timer.stop()
        if self._checker is not None and self._checker.isRunning():
            self._checker.stop()
            self._checker.wait(4000)
        if self._avahi_worker is not None and self._avahi_worker.isRunning():
            self._avahi_worker.stop.set()
            self._avahi_worker.wait(3000)
        if (self._sshd_process is not None
                and self._sshd_process.state() != QProcess.NotRunning):
            self._sshd_process.kill()
        if (self._avahi_process is not None
                and self._avahi_process.state() != QProcess.NotRunning):
            self._avahi_process.kill()
        self._cleanup_avahi_temp()
        super().closeEvent(event)
