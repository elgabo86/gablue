"""Fenêtre principale : liste des postes et actions.
"""

import os
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import uuid
from pathlib import Path

from PySide6.QtCore import QEvent, QProcess, QSize, QTimer, Qt
from PySide6.QtGui import QIcon, QKeySequence, QPixmap, QShortcut
from PySide6.QtWidgets import (
    QDialog, QFrame, QGridLayout, QHBoxLayout, QLabel, QMainWindow, QMenu,
    QMessageBox, QPushButton, QScrollArea, QStackedWidget, QTabWidget,
    QToolButton, QVBoxLayout, QWidget,
)

from .constants import (
    AVAHI_BROWSE, HOSTS_FILE, NO_AUTO_PROBE_STATES, PULSE_MS, REFRESH_MS,
    UPDATE_HELPER, UPDATE_PROGRESS_FIFO, _which,
)
from .dialogs.apps import AppsDialog
from .dialogs.booster import ForegroundBoosterDialog
from .dialogs.boot import BootDialog
from .dialogs.btrfs import BtrfsDialog
from .dialogs.clean import CleanDialog
from .dialogs.grub import GrubDialog
from .dialogs.gwine import GwineOptionsDialog
from .dialogs.host import CandidateDialog, HostDialog
from .dialogs.kernel import LocalSettingsDialog, RemoteSettingsDialog
from .dialogs.key import KeyInstallDialog
from .dialogs.prefs import PrefsDialog
from .dialogs.ryzen import RyzenDialog
from .dialogs.secureboot import SecureBootDialog
from .dialogs.services import ServicesDialog
from .dialogs.tailscale import TailscaleDialog
from .dialogs.thermal import ThermalDialog
from .dialogs.updates import AutoUpdatesDialog
from .dialogs.variant import VariantDialog
from .hosts import load_hosts, save_hosts
from . import instance, prefs, remote_updates, updates

from .apps import names_for as apps_names
from .apps import update_script as apps_update_script
from .local import (
    _sudo_run, avahi_apply_args, avahi_publish_state, brew_available,
    brew_progress_parser, brew_update_hint, brew_update_script, disk_rows,
    flatpak_update_hint, flatpak_update_script, format_memory_modules,
    gablue_update_hint, hardware_summary, local_sshd_state,
    parse_flatpak_progress, parse_gablue_progress, privileged_prefix,
    set_local_sshd_sudo,
)
from .ssh import (
    ask_remote_password, key_ready, remote_sudo_nopasswd, remote_sudo_prefix,
)
from .style import make_qss
from .uiutil import app_icon, network_online
from .tasks import (
    LocalTaskDialog, RemoteBatchDialog, RemoteTaskDialog, SequentialTaskDialog,
    start_task,
)
from .util import (
    address_text, display_name, dolphin_url, has_address_kind, ssh_target,
)
from .widgets import (
    AccessPanel, BusyProgressBar, CenteredTabs, HostCard, InfoPanel, LocalCard,
    LocalConfigMenu, NoticeBar, StatusDot, UpdatesPanel, _make_tool_button,
)
from .workers import (
    AddressWorker, AppsUpdateWorker, AvahiWorker, CheckWorker, KeyProbeWorker,
    HardwareWorker, RebootWorker, RemoteAppsWorker, RemoteHardwareWorker,
    RemoteUpdatesWorker, UpdateCheckWorker,
)

def _is_running(worker) -> bool:
    """`isRunning()` d'une sonde, sans exploser si son objet C++ est détruit."""
    try:
        return bool(worker.isRunning())
    except RuntimeError:
        return False


def _ask_stop(worker) -> None:
    """Demande l'arrêt d'une sonde : `stop()` appelable ou `threading.Event`."""
    stop = getattr(worker, "stop", None)
    if callable(stop):
        try:
            stop()
        except Exception:            # une sonde d'agrément ne doit rien casser
            pass
    elif hasattr(stop, "set"):       # AvahiWorker : Event
        stop.set()
    cancel = getattr(worker, "cancel", None)
    if hasattr(cancel, "set"):       # SshTaskWorker
        cancel.set()


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Gablue Manager")
        self.resize(1120, 780)
        self.setMinimumSize(940, 600)
        self.setWindowIcon(app_icon())

        self._hosts = load_hosts()
        self._states: dict[str, tuple[str, str]] = {}
        self._latency: dict[str, float] = {}
        self._cards: dict[str, HostCard] = {}
        self._dots: list[StatusDot] = []
        self._checker: CheckWorker | None = None
        self._address_workers: set[AddressWorker] = set()
        self._address_queried: set[str] = set()
        self._jobs: list[dict] = []
        self._tasks: list[QDialog] = []      # fenêtres d'opération en cours
        # Un dialogue de nettoyage par cible (local ou poste), non modal : une
        # tâche lancée depuis un dialogue modal reste bloquée par la modalité.
        self._clean_dialogs: dict[str, CleanDialog] = {}
        self._key_queue: list[dict] = []     # {"host", "password", "pending"}
        self._key_rejected: list[tuple[str, str]] = []   # (nom, raison)
        self._key_workers: set = set()       # sondes de clé en vol
        self._last_check: float | None = None
        self._display_order: list[str] = []
        self._discovering = False
        self._avahi_worker: AvahiWorker | None = None
        self._sshd_process: QProcess | None = None
        self._avahi_process: QProcess | None = None
        self._reboot_reasons: list[str] = []
        self._reboot_worker: RebootWorker | None = None
        self._hardware_worker: HardwareWorker | None = None
        # Températures et verdict SMART des disques SATA : lus **une fois** par
        # le helper NOPASSWD, à l'ouverture. Un disque ne chauffe pas en
        # quelques minutes et `smartctl` coûte un processus par disque : pas de
        # rafraîchissement périodique, contrairement à /sys (gratuit).
        self._disk_temps: list[tuple] = []
        self._nvm_smart: dict[str, bool] = {}
        # Nom commercial du GPU lu par Vulkan une fois par session : le
        # résumé relu toutes les 30 s ne doit pas le réécraser avec le nom du
        # die que donne `lspci`.
        self._gpu_name = ""
        # Un onglet par poste distant, avec la même page que le poste local :
        # résumé matériel chargé à l'ouverture de l'onglet (cache par session),
        # et état/joignabilité sondé seulement quand on est sur cet onglet.
        self._panes: dict[str, dict] = {}
        self._hosts_page: QWidget | None = None
        self._remote_hardware: dict[str, dict] = {}
        self._remote_hardware_at: dict[str, float] = {}
        self._remote_hardware_workers: dict[str, RemoteHardwareWorker] = {}
        self._sshd_state: tuple[bool, bool] = (False, False)
        self._avahi_state: tuple[bool, bool] = (False, True)
        self._updates_state: dict | None = None
        self._updates_worker: UpdateCheckWorker | None = None
        self._apps_worker: AppsUpdateWorker | None = None
        # Mises à jour des postes distants : connues seulement après un clic
        # (« Vérifier les mises à jour »), une par poste, jamais en fond.
        self._remote_updates: dict[str, dict] = {}
        self._remote_check_queue: list[str] = []
        self._remote_check_workers: list[RemoteUpdatesWorker] = []
        # Sonde des AppImages d'un poste (une connexion SSH par clic) : la
        # liste des applications en retard décide de la commande à lancer.
        self._remote_apps_workers: list[RemoteAppsWorker] = []
        self._batch_host_ids: list[str] = []
        # Lot « tout mettre à jour » : postes retenus et chaîne propre à chacun,
        # le temps que les sondes d'AppImages répondent.
        self._batch_hosts: list[dict] = []
        self._batch_commands: dict[str, str] = {}
        self._updates_pending = False

        self._build_ui()
        self.setStyleSheet(make_qss())
        self._rebuild_cards()
        self._refresh_access()
        self._refresh_hardware()

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
        # Les barrettes passent par dmidecode (root) et les disques par
        # smartctl : hors thread GUI, **une seule fois** par session — la
        # mémoire change au reboot, et une température de disque n'a pas
        # besoin d'un `smartctl` toutes les cinq minutes.
        QTimer.singleShot(250, self._load_hardware)

    # -- construction -------------------------------------------------------
    def _build_ui(self) -> None:
        central = QWidget(self)
        self.setCentralWidget(central)
        root = QVBoxLayout(central)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        header = QWidget(central)
        # Grille à trois colonnes : une marge de la largeur du bouton à gauche,
        # le titre au centre exact de la fenêtre, le bouton à droite. Une
        # simple rangée décalait le titre de la moitié du bouton.
        header_layout = QGridLayout(header)
        header_layout.setContentsMargins(26, 20, 26, 8)
        header_layout.setHorizontalSpacing(0)

        title = QLabel("Gablue Manager", header)
        title.setObjectName("title")
        title.setAlignment(Qt.AlignCenter)
        header_layout.addWidget(title, 0, 1)

        # Préférences de l'app : barre d'état et vérification des mises à jour.
        self.settings_button = _make_tool_button(
            header, "settings-configure", "⚙"
        )
        self.settings_button.setToolTip("Préférences")
        # Sans InstantPopup, un clic rapide sur un QToolButton à menu ne fait
        # rien (il faut maintenir le bouton) : le menu ne s'ouvrait jamais.
        self.settings_button.setPopupMode(QToolButton.InstantPopup)
        settings_menu = QMenu(self.settings_button)
        settings_menu.addAction("Préférences…", self.open_prefs)
        settings_menu.addSeparator()
        settings_menu.addAction("Quitter Gablue Manager", self.close)
        self.settings_button.setMenu(settings_menu)
        header_layout.addWidget(
            self.settings_button, 0, 2, Qt.AlignRight | Qt.AlignVCenter
        )
        side = max(36, self.settings_button.sizeHint().width())
        # Largeur figée à la taille réelle : la colonne de droite ne peut pas
        # s'élargir au-delà, sinon le titre glisse vers la gauche.
        self.settings_button.setFixedWidth(side)
        header_layout.setColumnMinimumWidth(0, side)
        header_layout.setColumnMinimumWidth(2, side)
        header_layout.setColumnStretch(1, 1)
        root.addWidget(header)

        # Raisons d'allumer la bande : une sonde, une recherche de mises à
        # jour ou une autre tâche peuvent se chevaucher ; la bande reste
        # allumée tant qu'il en reste une.
        self._busy_reasons: set[str] = set()
        self.busy = BusyProgressBar(central, track=False)
        self.busy.setObjectName("busy")
        self.busy.setRange(0, 0)
        self.busy.setTextVisible(False)
        self.busy.setFixedHeight(3)
        # Toujours visible, mais muet au repos : sa place reste réservée et
        # l'interface ne descend plus de 3 px à chaque vérification.
        self.busy.set_active(False)
        root.addWidget(self.busy)

        # Deux onglets fixes, centrés : le poste local et « Postes SSH ».
        # La page d'un poste distant ne prend plus d'onglet — elle s'ouvre
        # dans « Postes SSH » (clic sur sa carte).
        self.tabs = CenteredTabs(central)
        self.tabs.setObjectName("mainTabs")
        # Pas de mode document : sa ligne de base pleine largeur flotte
        # au-dessus d'onglets centrés, et le QSS habille déjà la barre.
        self.tabs.addTab(
            self._build_local_tab(), QIcon.fromTheme("computer"),
            self._local_tab_title(),
        )
        self._hosts_page = self._build_hosts_tab()
        self.tabs.addTab(
            self._hosts_page, QIcon.fromTheme("network-server"),
            "Postes SSH",
        )
        root.addWidget(self.tabs.header())
        root.addWidget(self.tabs, 1)
        # Cliquer le titre « Postes SSH » alors qu'on regarde un poste ramène
        # à la liste : la barre native n'émet rien pour un onglet déjà courant.
        self.tabs.reclicked.connect(self._tab_reclicked)

        footer = QWidget(central)
        footer_layout = QHBoxLayout(footer)
        footer_layout.setContentsMargins(26, 4, 26, 12)
        self.footer_label = QLabel(footer)
        self.footer_label.setObjectName("footer")
        footer_layout.addWidget(self.footer_label)
        footer_layout.addStretch(1)
        # Libellé libre, sans horodatage : il n'affiche que des messages
        # ponctuels (recherche réseau, session fermée), effacés à la passe
        # suivante. La vérification automatique reste silencieuse.
        self.auto_label = QLabel("", footer)
        self.auto_label.setObjectName("footer")
        footer_layout.addWidget(self.auto_label)
        root.addWidget(footer)

        QShortcut(QKeySequence("Ctrl+N"), self).activated.connect(self.add_host)
        # F5 force la vérification de l'onglet courant : les postes distants
        # ne sont sondés que sur leur onglet (voir `refresh_all`).
        QShortcut(QKeySequence("F5"), self).activated.connect(
            lambda: self.refresh_all(force=True, activity=True))
        self.tabs.currentChanged.connect(self._tab_changed)

        self._update_footer()

    def _build_local_tab(self) -> QWidget:
        """La machine courante : carte, mises à jour, matériel et système.

        L'onglet est dans une zone défilante : le résumé matériel tient à
        l'aise sur un grand écran, et rien n'est rogné sur une fenêtre basse.
        """
        scroll = QScrollArea(self.tabs)
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        scroll.viewport().setAutoFillBackground(False)
        content = QWidget(scroll)
        content.setAutoFillBackground(False)
        layout = QVBoxLayout(content)
        layout.setContentsMargins(26, 16, 26, 18)
        layout.setSpacing(12)

        # « Redémarrage requis » : bandeau actionnable, pas une pastille au
        # pied de page — il concerne le poste qu'on a sous les yeux.
        self.reboot_bar = NoticeBar(content)
        self.reboot_bar.action_requested.connect(self.reboot_local)
        self.reboot_bar.setVisible(False)
        layout.addWidget(self.reboot_bar)

        # Toute la configuration du poste local vit dans un menu : la carte
        # n'en porte que le bouton, comme la carte d'un poste distant.
        self.local_config = LocalConfigMenu(content)
        self.local_config.apps_requested.connect(self.open_apps)
        self.local_config.kernel_requested.connect(self.open_settings)
        self.local_config.updates_requested.connect(self.open_local_updates)
        self.local_config.services_requested.connect(self.open_local_services)
        self.local_config.variant_requested.connect(self.open_local_variant)
        self.local_config.btrfs_requested.connect(self.open_local_btrfs)
        self.local_config.thermal_requested.connect(self.open_local_thermal)
        self.local_config.ryzen_requested.connect(self.open_local_ryzen)
        self.local_config.clean_requested.connect(self.open_clean_system)
        self.local_config.grub_requested.connect(self.open_grub)
        self.local_config.secureboot_requested.connect(self.open_secureboot)
        self.local_config.bootselect_requested.connect(self.open_boot_select)
        self.local_config.firmware_requested.connect(self.reboot_firmware)
        self.local_config.gwine_requested.connect(
            self.open_local_gwine_options
        )
        self.local_config.booster_requested.connect(
            self.open_local_foreground_booster
        )
        self.local_config.tailscale_requested.connect(
            self.open_local_tailscale
        )
        # Le même état que le panneau d'accès : menu Réseau et panneau se
        # mettent à jour ensemble, jamais l'un sans l'autre.
        self.local_config.sshd_requested.connect(self.toggle_local_sshd)
        self.local_config.avahi_requested.connect(self.toggle_avahi)

        # La carte du poste local est revenue (jugée plus jolie qu'un bouton
        # seul en haut de l'onglet, vécu 07/11/2026) : identité + bouton
        # Configuration, comme la carte d'un poste distant.
        self.local_card = LocalCard(self.local_config, content)
        layout.addWidget(self.local_card)

        self.updates_panel = UpdatesPanel(content)
        self.updates_panel.check_requested.connect(
            lambda: self.check_updates(force=True, activity=True)
        )
        # L'ordre suit la chaîne « tout mettre à jour » (applications avant
        # Homebrew, l'étape la plus longue et la plus fragile) : le menu dit
        # donc la séquence réelle, jamais un ordre qui la contredit.
        self.updates_panel.set_update_menu([
            ("Tout mettre à jour…", lambda: self.local_update("all")),
            None,
            ("Gablue…", lambda: self.local_update("gablue")),
            ("Flatpaks…", lambda: self.local_update("flatpak")),
            ("Gwine…", lambda: self.local_update("gwine")),
            ("Applications…", lambda: self.local_update("apps")),
            ("Brew…", lambda: self.local_update("brew")),
        ])
        layout.addWidget(self.updates_panel)

        self.info_panel = InfoPanel(content)
        self.info_panel.filelight_requested.connect(self.open_filelight)
        self.info_panel.refresh_requested.connect(self.refresh_info_local)
        layout.addWidget(self.info_panel)

        layout.addStretch(1)
        scroll.setWidget(content)
        return scroll

    def _build_hosts_tab(self) -> QWidget:
        """Les postes distants : ajout, recherche, état et mises à jour."""
        page = QWidget(self.tabs)
        layout = QVBoxLayout(page)
        layout.setContentsMargins(26, 16, 26, 8)
        layout.setSpacing(10)

        # Actions de l'onglet, pas de la fenêtre : ajouter et rechercher des
        # postes, rafraîchir leur état, lancer leurs mises à jour. La rangée
        # entière disparaît quand la page d'un poste est affichée : ses
        # actions sont dans la carte du poste, pas dans la barre de la liste.
        self.list_toolbar = QWidget(page)
        toolbar = QHBoxLayout(self.list_toolbar)
        toolbar.setContentsMargins(0, 0, 0, 0)
        toolbar.setSpacing(8)
        self.add_button = QPushButton("Ajouter un poste", self.list_toolbar)
        self.add_button.setObjectName("accent")
        self.add_button.setCursor(Qt.PointingHandCursor)
        self.add_button.setIcon(QIcon.fromTheme("list-add"))
        add_menu = QMenu(self.add_button)
        add_menu.addAction("Saisir un poste à la main…", self.add_host)
        add_menu.addAction(
            "Rechercher sur le réseau local…", self.discover_hosts
        )
        self.add_button.setMenu(add_menu)
        toolbar.addWidget(self.add_button)
        toolbar.addStretch(1)

        self.remote_updates_button = QPushButton(
            "Mises à jour des postes", page
        )
        self.remote_updates_button.setObjectName("accent")
        self.remote_updates_button.setCursor(Qt.PointingHandCursor)
        self.remote_updates_button.setIcon(
            QIcon.fromTheme("system-software-update")
        )
        self.remote_updates_button.setToolTip(
            "Vérifier ou lancer les mises à jour des postes en ligne — "
            "uniquement sur ce clic"
        )
        hosts_menu = QMenu(self.remote_updates_button)
        hosts_menu.addAction(
            "Vérifier les mises à jour", self.check_remote_updates
        )
        hosts_menu.addAction("Tout mettre à jour…", self.update_all_hosts)
        self.remote_updates_button.setMenu(hosts_menu)
        toolbar.addWidget(self.remote_updates_button)

        # Pas de bouton « Rafraîchir l'état » : la passe automatique (30 s,
        # retour sur la fenêtre) suffit au quotidien, et F5 reste là pour
        # forcer une sonde immédiate, bande animée comprise.
        layout.addWidget(self.list_toolbar)

        self.stack = QStackedWidget(page)
        layout.addWidget(self.stack, 1)

        # Page vide
        empty = QWidget(self.stack)
        empty_layout = QVBoxLayout(empty)
        empty_layout.setContentsMargins(40, 40, 40, 60)
        empty_layout.setSpacing(10)
        empty_layout.addStretch(1)
        icon = QLabel(empty)
        pixmap = app_icon().pixmap(QSize(72, 72))
        if pixmap.isNull():
            pixmap = QPixmap(72, 72)
            pixmap.fill(Qt.transparent)
        icon.setPixmap(pixmap)
        icon.setAlignment(Qt.AlignCenter)
        empty_layout.addWidget(icon)
        empty_title = QLabel("Aucun poste distant", empty)
        empty_title.setObjectName("emptyTitle")
        empty_title.setAlignment(Qt.AlignCenter)
        empty_layout.addWidget(empty_title)
        empty_text = QLabel(
            "Ajoutez un poste pour le bureau distant, les fichiers ou le "
            "terminal.",
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
        self.cards_layout.setContentsMargins(0, 4, 0, 18)
        self.cards_layout.setSpacing(10)
        self.cards_layout.addStretch(1)
        self.scroll.setWidget(content)
        self.stack.addWidget(self.scroll)

        # Ce que les autres postes peuvent joindre de celui-ci : compact, au
        # pied de l'onglet — c'est la contrepartie de la liste des postes, pas
        # un réglage du poste local.
        self.access_panel = AccessPanel(page)
        self.access_panel.sshd_requested.connect(self.toggle_local_sshd)
        self.access_panel.avahi_requested.connect(self.toggle_avahi)
        layout.addWidget(self.access_panel)
        return page

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
            card = self._make_host_card(host, self.scroll.widget())
            self.cards_layout.addWidget(card)
            self._cards[host["id"]] = card
            self._dots.append(card.dot)

        self.cards_layout.addStretch(1)
        self._display_order = [host["id"] for host in self._sorted_hosts()]
        # Un réordonnancement (un poste passe en ligne) ne doit pas éjecter
        # la page du poste qu'on regarde.
        if self._host_view_host() is None:
            self.stack.setCurrentIndex(0 if not self._hosts else 1)
        self._refresh_pane_titles()
        self._rebuild_pane_states()
        self._update_footer()

    def _make_host_card(self, host: dict, parent) -> HostCard:
        """Carte d'un poste, avec toutes ses actions câblées.

        La même carte sert dans la liste « Postes SSH » et dans l'onglet du
        poste : une seule définition des signaux, donc les deux vues se
        comportent pareil.
        """
        card = HostCard(host, parent)
        card.open_requested.connect(self.open_host_tab)
        card.remote_requested.connect(self.launch_remote)
        card.remote_stop_requested.connect(self.stop_remote)
        card.files_requested.connect(self.open_files)
        card.key_requested.connect(self.install_key_dialog)
        card.edit_requested.connect(self.edit_host)
        card.delete_requested.connect(self.delete_host)
        card.terminal_requested.connect(self.open_terminal)
        card.update_requested.connect(self.update_host)
        card.check_requested.connect(self.check_host_updates)
        card.settings_requested.connect(self.open_remote_settings)
        card.clean_requested.connect(self.open_remote_clean)
        card.btrfs_requested.connect(self.open_remote_btrfs)
        card.thermal_requested.connect(self.open_remote_thermal)
        card.gwine_requested.connect(self.open_remote_gwine_options)
        card.booster_requested.connect(self.open_remote_foreground_booster)
        card.tailscale_requested.connect(self.open_remote_tailscale)
        card.apps_requested.connect(self.open_host_apps)
        card.reboot_requested.connect(self.reboot_host)
        card.shutdown_requested.connect(self.shutdown_host)
        card.set_state(
            self._states.get(host["id"], ("unknown", ""))[0],
            self._states.get(host["id"], ("unknown", ""))[1],
            self._latency.get(host["id"], 0.0),
        )
        card.set_updates(self._remote_updates.get(host["id"]))
        card.set_remote_active(self._remote_job(host["id"]) is not None)
        return card

    # -- onglets par poste ---------------------------------------------------
    def _local_tab_title(self) -> str:
        """« Gigafed (local) » : le nom de la machine, pas un terme générique."""
        name = socket.gethostname().strip() or "Ce poste"
        return f"{name[:1].upper()}{name[1:]} (local)"

    def _hosts_tab_index(self) -> int:
        """L'onglet « Postes SSH » reste collé au poste local (position 1)."""
        if self._hosts_page is None:
            return -1
        return self.tabs.indexOf(self._hosts_page)

    def _host_view_host(self) -> dict | None:
        """Poste dont la page est affichée dans « Postes SSH », s'il y en a un."""
        widget = self.stack.currentWidget()
        for host_id, pane in self._panes.items():
            if pane["page"] is widget:
                return self._host(host_id)
        return None

    def _current_pane_host(self) -> dict | None:
        """Poste qu'on a sous les yeux (None pour le local ou la liste)."""
        if self.tabs.currentIndex() != self._hosts_tab_index():
            return None
        return self._host_view_host()

    def open_host_tab(self, host_id: str) -> None:
        """Affiche la page d'un poste dans l'onglet « Postes SSH ».

        Clic sur sa carte (le fond, le nom ou la ligne d'état) : la liste
        laisse la place à la page du poste, dans le même onglet. Pas d'onglet
        par poste — deux onglets suffisent, et la croix au-dessus de la carte
        ramène à la liste.
        """
        host = self._host(host_id)
        if host is None:
            return
        if self.tabs.currentIndex() != self._hosts_tab_index():
            self.tabs.setCurrentIndex(self._hosts_tab_index())
        pane = self._panes.get(host_id)
        if pane is None:
            pane = self._build_host_pane(host)
        if self.tabs.currentIndex() == self._hosts_tab_index() \
                and self.stack.currentWidget() is pane["page"]:
            return
        self.stack.setCurrentWidget(pane["page"])
        self.list_toolbar.setVisible(False)
        self.access_panel.setVisible(False)
        # Ouvrir la page vaut visite : état du poste et résumé matériel sont
        # chargés tout de suite (avant, c'est le changement d'onglet qui le
        # faisait ; sans cet appel, le panneau restait vide jusqu'au passage
        # du timer de 30 s).
        self.refresh_all()
        self._update_footer()

    def show_hosts_list(self) -> None:
        """Revient à la liste des postes (croix de la page, titre d'onglet)."""
        self.stack.setCurrentIndex(0 if not self._hosts else 1)
        self.list_toolbar.setVisible(True)
        self.access_panel.setVisible(True)
        self._update_footer()

    def _close_button(self) -> QToolButton:
        """Croix de fermeture de la page d'un poste (au-dessus de la carte).

        Plus grande que la croix des onglets : ici c'est la seule sortie
        visible vers la liste, elle doit se cliquer sans viser.
        """
        button = QToolButton(self)
        button.setObjectName("pageClose")
        button.setText("✕")
        button.setToolTip("Revenir à la liste des postes")
        button.setCursor(Qt.PointingHandCursor)
        button.setAutoRaise(True)
        button.setFixedSize(30, 30)
        button.clicked.connect(self.show_hosts_list)
        return button

    def _destroy_pane(self, host_id: str) -> None:
        """Retire et détruit la page d'un poste (poste supprimé)."""
        pane = self._panes.pop(host_id, None)
        if pane is None:
            return
        self.stack.removeWidget(pane["page"])
        pane["page"].deleteLater()

    def _refresh_pane_titles(self) -> None:
        """Nom de la carte d'un poste = nom courant (renommage, libellé)."""
        for host_id, pane in self._panes.items():
            host = self._host(host_id)
            if host is None:
                continue
            pane["card"].name_label.setText(display_name(host))

    def _build_host_pane(self, host: dict) -> dict:
        """Page d'un poste distant : carte, mises à jour, matériel et système.

        La page vit dans la pile de l'onglet « Postes SSH », à côté de la
        liste : même onglet, deux vues — aucune place perdue en barre
        d'onglets quand on a dix postes.
        """
        scroll = QScrollArea(self.stack)
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        scroll.viewport().setAutoFillBackground(False)
        content = QWidget(scroll)
        content.setAutoFillBackground(False)
        layout = QVBoxLayout(content)
        layout.setContentsMargins(26, 16, 26, 18)
        layout.setSpacing(12)

        card = self._make_host_card(host, content)
        # La mise à jour vit dans le panneau du dessous, à côté de
        # « Vérifier » : la carte ne garde que l'identité et les accès.
        card.update_button.setVisible(False)
        # Croix de fermeture de la page, seule au-dessus de la carte : elle
        # ramène à la liste des postes, sans alourdir la rangée d'actions.
        header = QHBoxLayout()
        header.setContentsMargins(0, 0, 0, 0)
        header.addStretch(1)
        header.addWidget(self._close_button())
        layout.addLayout(header)
        layout.addWidget(card)

        host_id = host["id"]
        panel = UpdatesPanel(content)
        panel.check_requested.connect(
            lambda checked=False, hid=host_id:
            self.check_host_updates(hid)
        )
        panel.set_update_menu([
            ("Tout mettre à jour…",
             lambda: self.update_host(host_id, "all")),
            None,
            ("Gablue…",
             lambda: self.update_host(host_id, "gablue")),
            ("Flatpaks…",
             lambda: self.update_host(host_id, "flatpak")),
            ("Gwine…",
             lambda: self.update_host(host_id, "gwine")),
            ("Applications…",
             lambda: self.update_host(host_id, "apps")),
            ("Brew…",
             lambda: self.update_host(host_id, "brew")),
        ])
        panel.set_updates(self._remote_updates.get(host_id))
        layout.addWidget(panel)

        info = InfoPanel(content)
        info.refresh_requested.connect(
            lambda hid=host_id: self.refresh_info_host(hid)
        )
        layout.addWidget(info)
        layout.addStretch(1)
        scroll.setWidget(content)
        self.stack.addWidget(scroll)
        self._panes[host["id"]] = {
            "page": scroll, "card": card, "updates": panel, "info": info,
        }
        values = self._remote_hardware.get(host["id"])
        if values:
            self._fill_info_pane(info, values, links=False)
        return self._panes[host["id"]]

    def _rebuild_pane_states(self) -> None:
        """État et compteurs connus, appliqués aux deux vues d'un poste."""
        for host_id, pane in self._panes.items():
            state, detail = self._states.get(host_id, ("unknown", ""))
            pane["card"].set_state(state, detail, self._latency.get(host_id, 0.0))
            pane["card"].set_updates(self._remote_updates.get(host_id))
            pane["updates"].set_updates(self._remote_updates.get(host_id))
            pane["card"].set_remote_active(
                self._remote_job(host_id) is not None
            )

    def _fill_info_pane(self, panel: InfoPanel, values: dict,
                        links: bool) -> None:
        values = dict(values or {})
        rows = values.pop("disk_rows", None)
        values.pop("disks", None)
        temperatures = values.pop("temperatures", "")
        model_label = values.pop("model_label", "")
        panel.set_values(values)
        if model_label:
            panel.set_label("model", model_label)
        if rows is not None:
            panel.set_disks(rows, links=links)
        panel.set_temperatures(temperatures)

    # -- matériel d'un poste distant ----------------------------------------
    def refresh_info_local(self) -> None:
        """Relire le matériel de CE poste à la demande (bouton du panneau).

        `_refresh_hardware` est instantané (/sys, uptime, disques) ; le reste
        (barrettes, nom Vulkan, SMART) passe par le helper en root, donc un
        thread, et c'est `_hardware_ready` qui rend la main au bouton.
        """
        self.info_panel.set_refreshing(True)
        self._set_busy("hardware", True)
        self._refresh_hardware()
        self._load_hardware()

    def refresh_info_host(self, host_id: str) -> None:
        """Relire le matériel d'un poste à la demande (une connexion SSH).

        Une seule si la sonde peut partir : un poste qui refuse la clé ou dont
        l'empreinte est inconnue n'en reçoit pas une de plus, et le bouton
        n'est pas laissé grisé pour une sonde qui n'a pas commencé.
        """
        pane = self._panes.get(host_id)
        if pane is None:
            return
        if not self._load_remote_hardware(host_id, force=True):
            return
        pane["info"].set_refreshing(True)
        self._set_busy("hardware", True)

    def _load_remote_hardware(self, host_id: str, force: bool = False) -> bool:
        """Charge le résumé matériel d'un poste, une fois par session.

        L'onglet déclenche la sonde ; une tentative ratée (poste injoignable)
        n'est pas rejouée avant une minute, pour ne pas ouvrir une connexion
        SSH à chaque passage du timer. Rend True quand une sonde est partie.
        """
        pane = self._panes.get(host_id)
        host = self._host(host_id)
        if pane is None or host is None:
            return False
        # Un poste qui refuse la clé ou dont l'empreinte est inconnue ne
        # reçoit aucune connexion de confort : chacune est une offense pour
        # `PerSourcePenalties` (voir `refresh_all`).
        state = self._states.get(host_id, ("unknown", ""))[0]
        if state not in ("unknown", "online"):
            return False
        if host_id in self._remote_hardware_workers:
            return False
        if not force:
            if host_id in self._remote_hardware:
                return False
            if time.time() - self._remote_hardware_at.get(host_id, 0.0) < 60:
                return False
        self._remote_hardware_at[host_id] = time.time()
        worker = RemoteHardwareWorker(host, self)
        worker.done.connect(self._remote_hardware_ready)
        worker.finished.connect(
            lambda w=worker, hid=host_id: self._remote_hardware_finished(hid, w)
        )
        self._remote_hardware_workers[host_id] = worker
        worker.start()
        return True

    def _remote_hardware_finished(self, host_id: str, worker) -> None:
        if self._remote_hardware_workers.get(host_id) is worker:
            self._remote_hardware_workers.pop(host_id, None)
        worker.deleteLater()
        # Filet : la sonde a répondu (ou a été abandonnée), la bande d'activité
        # et le bouton ne doivent pas rester bloqués.
        self._set_busy("hardware", False)

    def _remote_hardware_ready(self, host_id: str, values: dict) -> None:
        self._remote_hardware_at[host_id] = time.time()
        pane = self._panes.get(host_id)
        if pane is not None:
            pane["info"].set_refreshing(False)
        if not values or self._host(host_id) is None:
            return
        self._remote_hardware[host_id] = values
        if pane is not None:
            # Filelight ne lit que la machine locale : pas de lien ici.
            self._fill_info_pane(pane["info"], values, links=False)

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
    def refresh_all(self, force: bool = False,
                    activity: bool = False) -> None:
        """État de l'onglet courant seulement.

        Sur l'onglet d'un poste, seule cette machine est sondée (état, latence
        et, à la première visite, résumé matériel) ; sur « Postes SSH », tous
        les postes en ligne ; sur le poste local, rien ne part sur le réseau.
        F5 force une passe complète de l'onglet.

        `force` ignore le repos des états bloqués : on ne resonde PAS les
        postes dont la clé est refusée ou l'empreinte inconnue, car chaque
        tentative est une connexion fermée avant authentification, comptée
        comme une offense par `PerSourcePenalties` (OpenSSH 9.8+). Les sonder
        toutes les 30 s finissait par faire bannir cette machine par leur
        sshd — et empêchait ensuite l'empreinte d'être récupérée, donc la clé
        d'être installée. F5 force.

        `activity` allume la bande bleue de l'en-tête : réservée au geste
        explicite (F5). Une passe automatique — timer
        de 30 s, retour sur la fenêtre, ouverture d'une page — doit rester
        invisible (demandé le 07/11/2026).
        """
        self._refresh_local_state()
        host = self._current_pane_host()
        if host is not None:
            self._load_remote_hardware(host["id"])
            self._probe_hosts([host], force, activity)
            self._update_footer()
            return
        if self.tabs.currentIndex() != self._hosts_tab_index():
            self._update_footer()
            return
        self._probe_hosts(self._hosts, force, activity)
        self._update_footer()

    def _probe_hosts(self, hosts: list[dict], force: bool = False,
                     activity: bool = False) -> None:
        """Lance les sondes d'état des postes demandés (jamais périodiques ici)."""
        if self._checker is not None and self._checker.isRunning():
            return
        if not hosts:
            return
        targets = [
            host for host in hosts
            if force or self._states.get(host["id"], ("", ""))[0]
            not in NO_AUTO_PROBE_STATES
        ]
        if not targets:
            return
        # Aucun état « Vérification… » pendant la passe : le texte change de
        # largeur et la carte saute à chaque rafraîchissement. La pastille
        # pulse (`dot.tick`) et chaque poste garde « En ligne · N ms »
        # jusqu'à la réponse, qui ne réécrit que les millisecondes.
        self._apply_states()
        # Seul un rafraîchissement demandé montre la bande animée.
        if activity:
            self._set_busy("probe", True)

        checker = CheckWorker(targets, self)
        checker.result.connect(self._on_check_result)
        checker.finished_all.connect(self._on_check_done)
        self._checker = checker
        checker.start()

    def _refresh_local_state(self) -> None:
        """Sondes du poste courant : locales, sans réseau ni attente root."""
        self._refresh_access()
        self._refresh_reboot()
        self._refresh_hardware()
        self.check_updates()

    def _tab_reclicked(self, index: int) -> None:
        """Clic sur l'onglet déjà courant : « Postes SSH » referme la page."""
        if index == self._hosts_tab_index():
            self.show_hosts_list()

    def _tab_changed(self, index: int) -> None:
        """Changement d'onglet : sonde seulement la machine qu'on regarde.

        « Postes SSH » s'ouvre toujours sur la liste : la page d'un poste se
        rouvre d'un clic sur sa carte. Arriver sur la liste sonde tous
        les cartes ; revenir moins de 8 s plus tard ne relance rien.
        """
        self._update_footer()
        if index != self._hosts_tab_index():
            return
        if self._host_view_host() is not None:
            self.show_hosts_list()
        if not self._last_check or time.time() - self._last_check > 8:
            self.refresh_all(force=True)

    def _refresh_hardware(self) -> None:
        """Résumé matériel : lectures locales, `lspci` mis en cache."""
        values = hardware_summary(self._disk_temps, self._nvm_smart)
        values.pop("disks", None)
        temperatures = values.pop("temperatures", "")
        vram = values.pop("vram", "")
        # Le nom Vulkan (VBIOS) est plus précis que celui du die : il est lu
        # hors thread GUI au démarrage et conservé pour toutes les passes.
        if self._gpu_name:
            values["gpu"] = self._gpu_name
        if vram and values.get("gpu"):
            values["gpu"] = f"{values['gpu']} · {vram}"
        # La première ligne est un modèle constructeur ou, sur une machine
        # assemblée, la carte mère : le libellé suit ce qui est affiché.
        model_label = values.pop("model_label", "")
        self.info_panel.set_values(values)
        if model_label:
            self.info_panel.set_label("model", model_label)
        self.info_panel.set_disks(disk_rows())
        self.info_panel.set_temperatures(temperatures)

    def _load_hardware(self) -> None:
        """Charge barrettes et nom Vulkan du GPU hors thread GUI."""
        if self._hardware_worker is not None and self._hardware_worker.isRunning():
            return
        worker = HardwareWorker(self)
        self._hardware_worker = worker
        worker.done.connect(self._hardware_ready)
        worker.finished.connect(worker.deleteLater)
        worker.start()

    def _hardware_ready(self, values: dict) -> None:
        self._hardware_worker = None
        # Relecture demandée depuis le panneau (ou passe de démarrage) : la
        # sonde a rendu la main, le bouton et la bande d'activité suivent.
        self.info_panel.set_refreshing(False)
        self._set_busy("hardware", False)
        text = format_memory_modules((values or {}).get("modules") or [])
        if text:
            self.info_panel.set_values({"modules": text})
        gpu = str((values or {}).get("gpu") or "")
        if gpu:
            # Nom commercial exact (VBIOS via Mesa) : il remplace le nom du
            # die que `lspci` donnait à l'ouverture.
            self._gpu_name = gpu
        self._disk_temps = list((values or {}).get("disks") or [])
        self._nvm_smart = dict((values or {}).get("nvm") or {})
        # Une seule passe pour la ligne « Températures » : CPU/GPU recalculés
        # depuis /sys, disques SATA depuis le cache du helper.
        self._refresh_hardware()


    # -- préférences ---------------------------------------------------------
    def set_prefs(self, values: dict) -> None:
        """Applique le dialogue des préférences (barre d'état + vérifications).

        Le tray relit ses préférences à chaque sonde, mais on le prévient pour
        qu'il recalcule tout de suite (fréquence, domaines, pastille) ; s'il ne
        tourne pas encore, on le lance.
        """
        prefs.save_prefs(values)
        if not values.get("tray"):
            instance.send(instance.tray_socket(), b"quit")
        elif not instance.send(instance.tray_socket(), b"reload"):
            # Même version que cette fenêtre : un ancien binaire du PATH
            # ignorerait `--tray` et ouvrirait une seconde fenêtre.
            instance.spawn(instance.manager_command() + ["--tray"])
        self._updates_state = None
        self.check_updates(force=True)

    # -- mises à jour disponibles -------------------------------------------
    def check_updates(self, force: bool = False,
                      activity: bool = False) -> None:
        """Sonde les mises à jour, hors thread GUI, selon les préférences.

        Sans `force`, rien n'est lancé si l'utilisateur a coupé les
        vérifications, si une opération est en cours, ou si la dernière passe
        est plus récente que la fréquence choisie. Hors ligne, aucune sonde
        réseau : la carte l'affiche et s'arrête là.

        `activity` allume la bande de l'en-tête : un clic sur « Vérifier »
        doit se voir, la sonde automatique non.
        """
        values = prefs.load_prefs()
        online = network_online()
        if not force and not prefs.checks_enabled(values):
            return
        if self._updates_worker is not None and self._updates_worker.isRunning():
            return
        if not force and self._tasks:
            return
        if not force and not online:
            self._updates_state = {"online": False, "domains": {}}
            self.updates_panel.set_updates(self._updates_state)
            self._update_image_cell(self._updates_state)
            self._update_footer()
            return
        checked = (self._updates_state or {}).get("checked_at")
        if not force and checked and \
                time.time() - checked < prefs.check_interval_seconds(values):
            return
        domains = prefs.check_domains(values) if force \
            else prefs.auto_check_domains(values)
        worker = UpdateCheckWorker(domains, online, self)
        self._updates_worker = worker
        worker.done.connect(self._updates_done)
        self.updates_panel.set_checking()
        if activity:
            self._set_busy("updates", True)
        worker.start()

    def _updates_done(self, state: dict) -> None:
        self._updates_worker = None
        self._set_busy("updates", False)
        self._updates_state = state
        self.updates_panel.end_checking()
        self.updates_panel.set_updates(state)
        self._update_image_cell(state)
        self._update_footer()
        # Le tray est un autre processus : il reçoit l'état déjà calculé au
        # lieu de refaire les mêmes sondes, donc sa pastille et son menu
        # disent le même chiffre que la carte dès cette vérification-ci, pas
        # au prochain créneau de son timer (vécu 08/11/2026).
        instance.send(instance.tray_socket(), updates.state_message(state))
        if self._updates_pending:
            # Sonde lancée avant la fin d'une mise à jour : elle décrit
            # l'avant, on en refait une tout de suite.
            self._updates_pending = False
            QTimer.singleShot(0, lambda: self.check_updates(force=True))

    def _update_image_cell(self, state: dict | None) -> None:
        """Ligne « Image Gablue » du résumé : celle qui tourne, et l'attente.

        L'information vient de la sonde des mises à jour (lecture locale de
        `rpm-ostree status`) : aucune commande supplémentaire pour l'afficher,
        et rien n'est inventé tant que la sonde n'a pas répondu. Une image
        staged donne « actuelle X · en attente Y », jamais « nouvelle image »
        sans version.
        """
        image = ((state or {}).get("domains") or {}).get("image") or {}
        detail = updates.image_detail(image)
        if detail:
            self.info_panel.set_values({"image": detail})

    def _updates_after_task(self, ok: bool) -> None:
        """Mise à jour terminée : re-sonder sans attendre la fréquence.

        Sinon le compteur de la carte et la pastille du tray gardaient l'état
        d'avant la mise à jour jusqu'au prochain créneau (120 min par défaut) :
        gwine à jour affichait encore « 1 mise à jour » et le point rouge
        (vécu 07/11/2026).
        """
        if not ok:
            return
        instance.send(instance.tray_socket(), b"updates")
        if self._updates_worker is not None and self._updates_worker.isRunning():
            self._updates_pending = True
            return
        self.check_updates(force=True)

    # -- mises à jour des postes distants (à la demande) --------------------
    def check_host_updates(self, host_id: str) -> None:
        """Vérifier les mises à jour d'un poste, sur **ce clic** seulement."""
        host = self._host(host_id)
        if host is None:
            return
        self._set_host_checking(host_id)
        worker = RemoteUpdatesWorker(host, self)
        worker.done.connect(self._on_remote_updates)
        self._remote_check_workers.append(worker)
        worker.finished.connect(
            lambda w=worker: self._remote_check_finished(w)
        )
        worker.start()

    def _set_host_checking(self, host_id: str) -> None:
        """« Vérification… » sur les deux vues d'un poste, jamais un verdict."""
        card = self._cards.get(host_id)
        if card is not None:
            card.set_checking()
        pane = self._panes.get(host_id)
        if pane is not None:
            pane["card"].set_checking()
            pane["updates"].set_checking()

    def check_remote_updates(self) -> None:
        """Vérifier les mises à jour de tous les postes en ligne.

        Trois connexions à la fois : une sonde par poste, aucune périodicité.
        """
        online = self._online_hosts()
        if not online:
            QMessageBox.information(
                self, "Mises à jour des postes",
                "Aucun poste en ligne à vérifier.",
            )
            return
        for host in online:
            if host["id"] not in self._remote_check_queue:
                self._remote_check_queue.append(host["id"])
            self._set_host_checking(host["id"])
        self._pump_remote_checks()

    def _online_hosts(self) -> list[dict]:
        """Postes dont la dernière sonde d'état dit « en ligne »."""
        return [
            host for host in self._hosts
            if self._states.get(host["id"], ("", ""))[0] == "online"
        ]

    def _pump_remote_checks(self) -> None:
        while self._remote_check_queue and len(self._remote_check_workers) < 3:
            host_id = self._remote_check_queue.pop(0)
            host = self._host(host_id)
            if host is None:
                continue
            worker = RemoteUpdatesWorker(host, self)
            worker.done.connect(self._on_remote_updates)
            self._remote_check_workers.append(worker)
            worker.finished.connect(
                lambda w=worker: self._remote_check_finished(w)
            )
            worker.start()

    def _remote_check_finished(self, worker) -> None:
        if worker in self._remote_check_workers:
            self._remote_check_workers.remove(worker)
        worker.deleteLater()
        self._pump_remote_checks()

    def _on_remote_updates(self, host_id: str, state: dict) -> None:
        """Sonde distante terminée : carte à jour, état conservé pour la suite."""
        self._remote_updates[host_id] = state
        card = self._cards.get(host_id)
        if card is not None:
            card.set_updates(state)
        pane = self._panes.get(host_id)
        if pane is not None:
            pane["card"].set_updates(state)
            pane["updates"].end_checking()
            pane["updates"].set_updates(state)

    def update_all_hosts(self) -> None:
        """Mettre à jour tous les postes en ligne, en parallèle.

        Les commandes dépendent de ce que chaque poste a d'installé : une
        sonde d'AppImages part donc d'abord sur chaque poste (une connexion
        SSH, flux de releases lus d'ici), puis la fenêtre du lot reçoit la
        chaîne propre à chacun.
        """
        online = self._online_hosts()
        if not online:
            QMessageBox.information(
                self, "Mises à jour des postes",
                "Aucun poste en ligne à mettre à jour.",
            )
            return
        listed = "\n".join(
            f"• {display_name(host)}" for host in online
        )
        answer = QMessageBox.question(
            self, "Mises à jour des postes",
            f"Mettre à jour {len(online)} poste(s) en parallèle ?\n\n"
            f"{listed}\n\n"
            "Image Gablue, Flatpaks, gwine, applications et Homebrew. L'image "
            "s'applique au prochain redémarrage ; un échec n'arrête que le "
            "poste concerné.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        if self._batch_hosts:
            return          # une sonde de lot est déjà partie
        self._batch_hosts = list(online)
        self._batch_commands = {}
        for host in online:
            self._probe_remote_apps(host, self._batch_probe_ready)

    def _batch_probe_ready(self, host: dict, result) -> None:
        """Sonde d'un poste terminée : sa chaîne est prête, le lot attend les autres."""
        pending, _unknown, _installed, states = result
        self._batch_commands[host["id"]] = remote_updates.update_chain(
            pending, states
        )
        if len(self._batch_commands) < len(self._batch_hosts):
            return
        hosts = self._batch_hosts
        self._batch_hosts = []
        self._batch_host_ids = [entry["id"] for entry in hosts]
        dialog = RemoteBatchDialog(
            self, hosts=hosts, commands=self._batch_commands,
            subtitle=f"{len(hosts)} poste(s) mis à jour en parallèle.",
        )
        dialog.completed.connect(self._batch_done)
        start_task(self, dialog)

    def _batch_done(self, ok: bool) -> None:
        """Lot terminé : les compteurs connus de ces cartes sont périmés."""
        if not ok:
            return
        for host_id in self._batch_host_ids:
            self._forget_remote_updates(host_id)
        self._batch_host_ids = []

    def _remote_after_task(self, host_id: str, ok: bool) -> None:
        """Mise à jour d'un poste terminée : son compteur connu est périmé."""
        if not ok:
            return
        self._forget_remote_updates(host_id)

    def _forget_remote_updates(self, host_id: str) -> None:
        """Oublie un état de mises à jour périmé, dans les deux vues du poste."""
        if self._remote_updates.pop(host_id, None) is None:
            return
        card = self._cards.get(host_id)
        if card is not None:
            card.set_updates(None)
        pane = self._panes.get(host_id)
        if pane is not None:
            pane["card"].set_updates(None)
            pane["updates"].set_updates(None)

    def _on_check_result(self, host_id: str, state: str, detail: str,
                         latency: float, address: str) -> None:
        if state == "unknown":
            return
        self._states[host_id] = (state, detail)
        self._latency[host_id] = latency
        host = self._host(host_id)
        if host is not None and address:
            # L'adresse qui a répondu devient celle des actions : LAN à la
            # maison, Tailscale en déplacement, sans rien demander.
            active = "" if address == host["host"] else address
            if active != host.get("active", ""):
                host["active"] = active
                self._save()
            if state == "online" and not has_address_kind(host, "tailscale"):
                # Sonde tant que l'adresse Tailscale manque, même si des
                # adresses de secours existent déjà : la découverte mDNS n'en
                # donne qu'en LAN, et un poste sans Tailscale connu est
                # injoignable en déplacement.
                self._ask_addresses(host_id)
        card = self._cards.get(host_id)
        if card is not None:
            card.set_state(state, detail, latency)
        pane = self._panes.get(host_id)
        if pane is not None:
            pane["card"].set_state(state, detail, latency)
        self._update_footer()

    def _ask_addresses(self, host_id: str) -> None:
        """Demande au poste ses adresses (LAN + Tailscale), une fois par session."""
        if host_id in self._address_queried:
            return
        host = self._host(host_id)
        if host is None:
            return
        self._address_queried.add(host_id)
        worker = AddressWorker(host, self)
        self._address_workers.add(worker)
        worker.finished.connect(lambda w=worker: self._address_workers.discard(w))
        worker.done.connect(self._addresses_found)
        worker.start()

    def _addresses_found(self, host_id: str, found: list) -> None:
        host = self._host(host_id)
        if host is None or not found:
            return
        fresh = [
            item for item in found
            if isinstance(item, dict) and item.get("address")
        ]
        if not fresh:
            return
        # Le poste fait foi pour les types qu'il rapporte : une adresse
        # Tailscale périmée (nœud réauthentifié) ou un ancien LAN est remplacé,
        # au lieu de s'empiler et d'ajouter un délai à chaque vérification.
        # L'adresse principale enregistrée n'est jamais retirée : c'est le choix
        # de l'utilisateur, et `active` couvre le cas où elle a changé.
        fresh_kinds = {str(item.get("kind") or "") for item in fresh}
        kept = [
            item for item in host.get("addresses") or []
            if str(item.get("kind") or "") not in fresh_kinds
        ]
        merged = list(kept)
        for item in fresh:
            address = str(item["address"])
            kind = str(item.get("kind") or "")
            if address == host["host"] or address in {
                str(entry.get("address") or "") for entry in merged
            }:
                continue
            merged.append({"address": address, "kind": kind})
        if merged == (host.get("addresses") or []):
            return
        host["addresses"] = merged
        self._save()
        card = self._cards.get(host_id)
        if card is not None:
            state, detail = self._states.get(host_id, ("unknown", ""))
            card.set_state(state, detail, self._latency.get(host_id, 0.0))

    def _set_busy(self, reason: str, active: bool) -> None:
        """Bande d'activité partagée : allumée tant qu'une raison l'exige.

        Plusieurs tâches peuvent se chevaucher (sonde d'état, recherche de
        mises à jour, découverte réseau, applications) : la dernière à finir
        n'éteint pas la bande d'une autre.
        """
        if active:
            self._busy_reasons.add(reason)
        else:
            self._busy_reasons.discard(reason)
        self.busy.set_active(bool(self._busy_reasons))

    def _on_check_done(self) -> None:
        self._set_busy("probe", False)
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
        for host_id, pane in self._panes.items():
            state, detail = self._states.get(host_id, ("unknown", ""))
            pane["card"].set_state(
                state, detail, self._latency.get(host_id, 0.0)
            )

    def _update_footer(self) -> None:
        """Barre discrète + compteurs dans les titres d'onglets.

        Le pied ne parle plus que du contexte : aucun poste enregistré ou
        nombre de postes en ligne sur « Postes SSH », rien sur les postes.
        Les titres portent les compteurs utiles (postes, mises à jour).
        """
        total = len(self._hosts)
        online = sum(
            1 for host in self._hosts
            if self._states.get(host["id"], ("", ""))[0] == "online"
        )
        on_hosts = getattr(self, "tabs", None) is not None \
            and self.tabs.currentIndex() == self._hosts_tab_index()
        if not on_hosts:
            self.footer_label.setText("")
        elif total == 0:
            self.footer_label.setText("Aucun poste distant enregistré")
        else:
            noun = "poste" if total == 1 else "postes"
            self.footer_label.setText(f"{total} {noun} · {online} en ligne")
        if getattr(self, "tabs", None) is not None:
            index = self._hosts_tab_index()
            if index >= 0:
                self.tabs.setTabText(
                    index, f"Postes SSH ({total})" if total else "Postes SSH"
                )
            self.tabs.setTabText(0, self._local_tab_title())
        self.auto_label.setText("")

    # -- serveur SSH local --------------------------------------------------
    def _pending(self, process: QProcess | None) -> bool:
        """Une bascule privilégiée attend PolicyKit : le libellé reste figé."""
        return process is not None and process.state() != QProcess.NotRunning

    def _refresh_access(self) -> None:
        """Serveur SSH + annonce locale : un seul état, deux affichages.

        Le panneau « Accès depuis les autres postes » et le menu
        Configuration ▸ Réseau lisent le même couple : impossible d'en voir un
        à jour et l'autre périmé.
        """
        if not self._pending(self._sshd_process):
            self._sshd_state = local_sshd_state()
        if not self._pending(self._avahi_process):
            self._avahi_state = avahi_publish_state()
        self._apply_access_state()

    def _apply_access_state(self) -> None:
        self.access_panel.set_sshd(*self._sshd_state)
        self.access_panel.set_avahi(*self._avahi_state)
        self.local_config.set_access_state(
            self._sshd_state, self._avahi_state
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
            self._refresh_access()
            return

        if shutil.which("pkexec") is None:
            QMessageBox.critical(
                self, "Serveur SSH local",
                "Ni sudo sans mot de passe ni pkexec ne sont disponibles sur "
                "ce poste : bascule impossible.",
            )
            return

        action = "enable" if enabled else "disable"
        self.access_panel.set_sshd_pending()
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
        self._refresh_access()
        QMessageBox.critical(
            self, "Serveur SSH local", "pkexec n'a pas pu démarrer.",
        )

    def _sshd_toggled(self, code: int, _status) -> None:
        self._sshd_process = None
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
        self._refresh_access()

    # -- annonce locale de ce poste -----------------------------------------
    def toggle_avahi(self) -> None:
        published, daemon = avahi_publish_state()
        if not daemon:
            QMessageBox.information(
                self, "Annonce locale",
                "Le service d'annonce locale n'est pas actif sur ce poste : "
                "l'annonce ne servirait à rien.\n\nL'activer avec :\n"
                "    sudo systemctl enable --now avahi-daemon",
            )
            return
        if published:
            answer = QMessageBox.question(
                self, "Annonce locale",
                "Ne plus annoncer ce poste en SSH sur le réseau local ?\n\n"
                "La recherche automatique depuis les autres postes ne le "
                "trouvera plus ; l'accès SSH reste possible en saisissant "
                "l'adresse à la main.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
        else:
            answer = QMessageBox.question(
                self, "Annonce locale",
                "Annoncer ce poste en SSH sur le réseau local ?\n\n"
                "Les autres postes du même réseau verront son nom d'hôte, son "
                "adresse IP et le port 22. Aucun compte ni clé n'est publié, "
                "et rien ne sort du réseau local.",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
            )
        if answer != QMessageBox.Yes:
            return

        enabled = not published
        argv = avahi_apply_args(enabled)
        argv[0] = _which(argv[0])

        ok, _detail = _sudo_run(argv)
        if ok:
            self._refresh_access()
            return

        if shutil.which("pkexec") is None:
            QMessageBox.critical(
                self, "Annonce locale",
                "Ni sudo sans mot de passe ni pkexec ne sont disponibles sur "
                "ce poste : bascule impossible.",
            )
            return

        self.access_panel.set_avahi_pending()
        process = QProcess(self)
        process.setProgram("pkexec")
        process.setArguments(argv)
        process.finished.connect(self._avahi_toggled)
        process.errorOccurred.connect(self._avahi_start_failed)
        self._avahi_process = process
        process.start()

    def _avahi_toggled(self, code: int, _status) -> None:
        self._avahi_process = None
        if code != 0:
            reasons = {
                126: "autorisation refusée",
                127: "non autorisé (PolicyKit)",
                125: "dialogue PolicyKit absent ou annulé",
            }
            QMessageBox.critical(
                self, "Annonce locale",
                "Échec de la bascule : "
                + reasons.get(code, f"code {code}") + ".",
            )
        self._refresh_access()

    def _avahi_start_failed(self, error) -> None:
        if error != QProcess.FailedToStart:
            return
        self._avahi_process = None
        self._refresh_access()
        QMessageBox.critical(
            self, "Annonce locale", "pkexec n'a pas pu démarrer.",
        )

    # -- redémarrage en attente ---------------------------------------------
    def _refresh_reboot(self) -> None:
        """Calcule l'état « redémarrage requis » hors thread GUI.

        L'état est relu à chaque passe (et au démarrage) : une image staged ou
        des kargs en attente survivent à la fermeture de l'app, il ne doit donc
        pas vivre dans un drapeau de session.
        """
        if self._reboot_worker is not None and self._reboot_worker.isRunning():
            return
        worker = RebootWorker(self)
        self._reboot_worker = worker
        worker.done.connect(self._reboot_state)
        worker.start()

    def _reboot_state(self, reasons: list) -> None:
        self._reboot_reasons = [str(reason) for reason in reasons or []]
        self._update_reboot_bar()

    def _update_reboot_bar(self) -> None:
        """Bandeau « redémarrage requis » du poste local, actionnable."""
        reasons = self._reboot_reasons
        if not reasons:
            self.reboot_bar.clear()
            return
        detail = "\n".join(f"• {reason}" for reason in reasons)
        text = f"Redémarrage requis : {reasons[0]}"
        if len(reasons) > 1:
            text += f" (+{len(reasons) - 1})"
        self.reboot_bar.set_message(
            text,
            "En attente d'un redémarrage :\n" + detail
            + "\n\nCliquer sur « Redémarrer… » pour les appliquer.",
        )

    def reboot_local(self) -> None:
        reasons = self._reboot_reasons
        if not reasons:
            return
        detail = "\n".join(f"• {reason}" for reason in reasons)
        answer = QMessageBox.question(
            self, "Redémarrer ce poste",
            "Redémarrer maintenant pour appliquer :\n\n"
            f"{detail}\n\n"
            "Toutes les sessions ouvertes seront interrompues.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._launch(
            privileged_prefix() + ["systemctl", "reboot"], "Redémarrage"
        )

    # -- actions ------------------------------------------------------------
    def add_host(self) -> None:
        dialog = HostDialog(self, existing=self._hosts)
        if dialog.exec() != QDialog.Accepted:
            return
        # Rien n'entre au registre avant la vérification de la clé : un poste
        # dont la clé ne passe pas n'est jamais ajouté (demandé 08/11/2026).
        self._install_keys([dialog.result_record], dialog.password())

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
        self._remote_updates.pop(host_id, None)
        self._remote_hardware.pop(host_id, None)
        self._remote_hardware_at.pop(host_id, None)
        # Un poste supprimé n'a plus de page : la détruire avant que ses
        # widgets ne pointent sur un poste absent du registre.
        self._destroy_pane(host_id)
        self.show_hosts_list()
        self._save()
        self._rebuild_cards()
        self._update_footer()

    def install_key_dialog(self, host_id: str, password: str = "") -> None:
        host = self._host(host_id)
        if host is None:
            return
        dialog = KeyInstallDialog(self, host=host, password=password)
        item = {"host": host, "password": password, "pending": False}
        dialog.installed.connect(lambda it=item: self._key_installed(it))
        start_task(self, dialog)

    def _install_keys(self, hosts: list[dict], password: str = "") -> None:
        """Copie la clé des postes à ajouter, sans fenêtre si elle est déjà là.

        Une sonde muette par poste (thread, jamais le thread GUI). Un poste
        n'entre au registre qu'une fois la clé acceptée **et** l'empreinte
        épinglée : tant que ce n'est pas prouvé, il reste hors de `_hosts`,
        n'est pas écrit dans `ssh-hosts.json` et n'apparaît pas dans la liste.
        Le dialogue ne s'ouvre que pour ceux qui la refusent encore (mot de
        passe) ou dont l'empreinte reste à accepter ; les fenêtres sont
        enchaînées, une à la fois.
        """
        self._key_queue = [
            {"host": dict(host), "password": password, "pending": True}
            for host in hosts
        ]
        self._key_rejected = []
        self._probe_next_key()

    def _probe_next_key(self) -> None:
        if not self._key_queue:
            self._key_report()
            return
        item = self._key_queue.pop(0)
        worker = KeyProbeWorker(item["host"], self)
        self._key_workers.add(worker)
        worker.finished.connect(lambda w=worker: self._key_workers.discard(w))
        worker.done.connect(lambda info, it=item: self._key_probed(it, info))
        worker.start()

    def _key_probed(self, item: dict, info: dict) -> None:
        host = item["host"]
        if key_ready(info):
            self._key_installed(item)
            self._probe_next_key()
            return
        # La sonde est passée au dialogue : l'installation réutilise ses clés
        # au lieu d'ouvrir une connexion refusée de plus.
        dialog = KeyInstallDialog(
            self, host=host, password=item["password"], prescan=info
        )
        dialog.installed.connect(lambda it=item: self._key_installed(it))
        # La suite attend la fermeture de cette fenêtre : un mot de passe à la
        # fois, et l'utilisateur peut renoncer sans bloquer les suivants.
        dialog.finished.connect(
            lambda _code, d=dialog, it=item: self._key_dialog_closed(d, it)
        )
        start_task(self, dialog)

    def _key_dialog_closed(self, dialog: KeyInstallDialog, item: dict) -> None:
        """Fenêtre fermée sans succès : le poste en attente est écarté."""
        if not dialog.key_installed():
            self._key_rejected.append(
                (display_name(item["host"]), dialog.failure_text())
            )
        self._probe_next_key()

    def _key_report(self) -> None:
        """Dit quels postes n'ont pas été ajoutés, et pourquoi."""
        rejected = self._key_rejected
        self._key_rejected = []
        if not rejected:
            return
        lines = "\n".join(f"• {name} — {reason}" for name, reason in rejected)
        QMessageBox.warning(
            self, "Postes non ajoutés",
            "La clé SSH n'a pas pu être vérifiée sur :\n\n"
            f"{lines}\n\n"
            "Ces postes ne sont pas dans la liste. Réessayez l'ajout avec le "
            "mot de passe SSH du poste si la clé manque.",
        )

    def _key_installed(self, item: dict) -> None:
        host = item["host"]
        if item.get("pending"):
            # Premier ajout : c'est SEULEMENT maintenant que le registre, le
            # disque et la liste reçoivent le poste.
            if self._host(host["id"]) is not None:
                return
            host["key_added"] = True
            self._hosts.append(host)
            self._states[host["id"]] = ("checking", "")
            self._save()
            self._rebuild_cards()
        else:
            stored = self._host(host["id"])
            if stored is None:
                return
            stored["key_added"] = True
            self._save()
            self._states[host["id"]] = ("checking", "")
            self._apply_states()
        self.refresh_all()
        # La clé fonctionne : c'est le moment de demander au poste ses
        # adresses (dont Tailscale, jamais annoncée en mDNS).
        self._ask_addresses(host["id"])

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
        """Ferme les sessions RDP lancées par l'app (menu Configuration)."""
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

    def open_filelight(self, path: str) -> None:
        """Ouvre Filelight sur un dossier (le disque système → le home).

        Filelight ne lit que la machine locale : l'appel n'a de sens que pour
        le poste local, et l'échec est dit au lieu de rester silencieux.
        """
        target = Path(str(path or ""))
        if not target.is_dir():
            target = Path.home()
        binary = _which("filelight")
        if binary is None:
            QMessageBox.information(
                self, "Filelight",
                "Filelight n'est pas installé sur ce poste.",
            )
            return
        if not QProcess.startDetached(binary, [str(target)]):
            QMessageBox.warning(
                self, "Filelight",
                f"Filelight n'a pas pu démarrer sur {target}.",
            )

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

    def update_host(self, host_id: str, kind: str = "gablue") -> None:
        host = self._host(host_id)
        if host is None:
            return
        if kind == "all":
            self.update_remote_all(host)
        elif kind == "gablue":
            self.update_remote_gablue(host)
        elif kind == "flatpak":
            self.update_remote_flatpaks(host)
        elif kind == "gwine":
            self.update_remote_gwine(host)
        elif kind == "brew":
            self.update_remote_brew(host)
        elif kind == "apps":
            self.update_remote_apps(host)

    def _probe_remote_apps(self, host: dict, then) -> None:
        """Sonde les AppImages d'un poste (une connexion SSH), puis `then`.

        Les flux de releases se lisent d'ici : le poste ne fait que dire ce qui
        est installé. Aucune périodicité, la sonde part d'un clic.
        """
        worker = RemoteAppsWorker(host, self)
        worker.done.connect(
            lambda host_id, result: then(self._host(host_id) or host, result)
        )
        self._remote_apps_workers.append(worker)
        worker.finished.connect(
            lambda w=worker: self._remote_apps_finished(w)
        )
        worker.start()

    def _remote_apps_finished(self, worker) -> None:
        if worker in self._remote_apps_workers:
            self._remote_apps_workers.remove(worker)
        worker.deleteLater()

    def update_remote_all(self, host: dict) -> None:
        """Tout mettre à jour sur un poste : image, Flatpaks, gwine, applis, brew.

        La sonde des AppImages part d'abord : sans elle, la chaîne ne saurait
        pas quoi passer aux scripts d'installation, et la confirmation ne
        pourrait pas dire ce qui sera réellement mis à jour.
        """
        self._probe_remote_apps(host, self._offer_remote_all)

    def _offer_remote_all(self, host: dict, result) -> None:
        pending, _unknown, _installed, states = result
        detail = ""
        if pending:
            detail = (
                "\nApplications en retard : "
                + ", ".join(apps_names(pending)) + "."
            )
        answer = QMessageBox.question(
            self, "Tout mettre à jour",
            f"Mettre à jour « {display_name(host)} » en entier ?\n\n"
            "Image Gablue, Flatpaks, gwine, applications et Homebrew."
            f"{detail}\n\n"
            "L'image s'applique au prochain redémarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Tout mettre à jour — {display_name(host)}",
            "Image, Flatpaks, gwine, applications et Homebrew.",
            remote_updates.update_chain(pending, states), root=True,
        )

    def update_remote_apps(self, host: dict) -> None:
        """Applications (AppImages) d'un poste : sonde puis scripts du poste."""
        self._probe_remote_apps(host, self._offer_remote_apps)

    def _offer_remote_apps(self, host: dict, result) -> None:
        pending, unknown, installed, states = result
        if not installed:
            QMessageBox.information(
                self, "Applications",
                f"Aucune application n'est installée sur « "
                f"{display_name(host)} ».",
            )
            return
        if not pending:
            if unknown:
                QMessageBox.warning(
                    self, "Applications",
                    "Mise à jour impossible à déterminer : les flux de "
                    "versions n'ont pas répondu pour au moins une "
                    "application.",
                )
            else:
                QMessageBox.information(
                    self, "Applications",
                    "Toutes les applications installées sont à jour sur "
                    f"« {display_name(host)} ».",
                )
            return
        names = ", ".join(apps_names(pending))
        answer = QMessageBox.question(
            self, "Mettre à jour les applications",
            f"Mettre à jour {names} sur « {display_name(host)} » ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Mise à jour des applications — {display_name(host)}",
            names + ".", remote_updates.apps_chain(pending, states),
        )

    def update_remote_gablue(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour Gablue",
            f"Télécharger la nouvelle image Gablue de « {display_name(host)} » "
            f"({address_text(host)}) ?\n\n"
            "Elle s'applique à son prochain redémarrage.",
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
            "La nouvelle image s'applique au prochain redémarrage.", script,
            root=True,
            # Seul le compte rendu d'upgrade fait foi : `bootc status` décrit
            # aussi le déploiement en attente et contiendrait « staged » même
            # quand rien n'a changé.
            hint_from_output=lambda text: gablue_update_hint(
                text.split("---STATUS---")[0]
            ),
        )

    def update_remote_flatpaks(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour les Flatpaks",
            f"Mettre à jour les applications Flatpak de "
            f"« {display_name(host)} » ({address_text(host)}) ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Flatpaks — {display_name(host)}",
            "Applications Flatpak système.", flatpak_update_script(),
        )

    def update_remote_gwine(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour gwine",
            f"Mettre à jour gwine sur « {display_name(host)} » "
            f"({address_text(host)}) ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"gwine — {display_name(host)}",
            "Runner Wine et composants (DXVK, VKD3D, D7VK).",
            "gwine --init\n",
        )

    def update_remote_brew(self, host: dict) -> None:
        answer = QMessageBox.question(
            self, "Mettre à jour Homebrew",
            f"Mettre à jour les formules Homebrew de « {display_name(host)} » "
            f"({address_text(host)}) ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Homebrew — {display_name(host)}",
            "Formules installées par brew.", brew_update_script(),
        )

    def open_remote_settings(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        RemoteSettingsDialog(self, host).exec()

    def open_remote_btrfs(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        BtrfsDialog(self, host).exec()

    def open_remote_thermal(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        ThermalDialog(self, host).exec()

    def _run_remote_script(self, host: dict, title: str, subtitle: str,
                           script: str, root: bool = False,
                           ok_if_disconnect: bool = False,
                           hint_from_output=None,
                           close_on_success: bool = False) -> None:
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
            close_on_success=close_on_success,
        )
        # Une action distante peut changer l'état des mises à jour : les
        # compteurs connus de cette carte sont oubliés, jamais devinés.
        dialog.completed.connect(
            lambda ok, hid=host["id"]: self._remote_after_task(hid, ok)
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
            root=True, ok_if_disconnect=True, close_on_success=True,
        )

    def shutdown_host(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is None:
            return
        answer = QMessageBox.question(
            self, "Éteindre le poste",
            f"Éteindre « {display_name(host)} » ({address_text(host)}) ?\n\n"
            "Toutes les sessions ouvertes sur ce poste seront interrompues. "
            "Il ne redémarrera qu'à la main.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_remote_script(
            host, f"Extinction — {display_name(host)}",
            "systemctl poweroff via SSH.",
            "sudo -n /usr/bin/systemctl poweroff\n",
            root=True, ok_if_disconnect=True, close_on_success=True,
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
        # Les compteurs affichés vont changer : la prochaine passe re-sonde.
        self._updates_state = None
        if kind == "all":
            self.update_local_all()
        elif kind == "gablue":
            self.update_local_gablue()
        elif kind == "flatpak":
            self.update_local_flatpaks()
        elif kind == "gwine":
            self.update_local_gwine()
        elif kind == "brew":
            self.update_local_brew()
        elif kind == "apps":
            self.update_local_apps()

    def update_local_all(self) -> None:
        """Tout mettre à jour d'un trait, avec une barre **globale**.

        Chaque mise à jour garde son propre analyseur ; la fenêtre globale
        additionne les étapes terminées et la fraction de l'étape courante.
        L'ordre est celui des dépendances réelles : l'image d'abord (elle peut
        apporter un nouveau gwine), puis Flatpak, gwine et applications, qui ne
        dépendent pas de l'image mais prennent du temps ; Homebrew en dernier,
        parce que c'est l'étape la plus longue et la plus fragile — un échec ne
        doit pas laisser les AppImages en retard.

        Les applications n'ont pas d'état local : la liste des AppImages en
        retard vient du réseau (un flux de releases par application), donc la
        sonde part d'abord, hors fil graphique, et l'étape est omise quand tout
        est à jour.
        """
        self._probe_apps(self._offer_local_all)

    def _offer_local_all(self, result) -> None:
        """Sonde des applications terminée : proposer la séquence exacte."""
        pending = result[0]
        steps = self._local_update_steps(pending)
        if not steps:
            QMessageBox.information(
                self, "Tout mettre à jour",
                "Rien à mettre à jour sur ce poste : ni image Gablue "
                "accessible, ni flatpak, ni gwine, ni application, "
                "ni Homebrew.",
            )
            return
        listed = "\n".join(f"• {step['label']}" for step in steps)
        answer = QMessageBox.question(
            self, "Tout mettre à jour",
            "Mettre à jour, dans cet ordre :\n\n"
            f"{listed}\n\n"
            "Arrêt au premier échec ; l'image Gablue s'applique au prochain "
            "redémarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = SequentialTaskDialog(
            self,
            title="Tout mettre à jour",
            subtitle=f"{len(steps)} mises à jour enchaînées sur ce poste.",
            steps=steps,
        )
        dialog.completed.connect(self._updates_after_task)
        start_task(self, dialog)

    def _local_update_steps(self, pending_apps=None) -> list[dict]:
        """Étapes réellement possibles sur ce poste, dans l'ordre.

        Un outil absent n'est pas une erreur : il est simplement omis, sinon
        « tout mettre à jour » échouerait sur chaque poste où Homebrew ou
        flatpak n'est pas installé. `pending_apps` vient de la sonde réseau
        (`apps.pending_updates`) : vide, aucune étape applications.
        """
        steps: list[dict] = []
        if Path(UPDATE_HELPER).is_file():
            steps.append({
                "label": "Gablue (image système)",
                "argv": [UPDATE_HELPER],
                "root": True,
                "fifo": UPDATE_PROGRESS_FIFO,
                "hint": gablue_update_hint,
            })
        else:
            bootc = shutil.which("bootc")
            if bootc:
                steps.append({
                    "label": "Gablue (image système)",
                    "argv": [bootc, "upgrade"],
                    "root": True,
                    "hint": gablue_update_hint,
                    "busy": True,      # sans helper, bootc est muet
                })
        if shutil.which("flatpak"):
            steps.append({
                "label": "Flatpaks",
                "argv": [_which("bash"), "-c", flatpak_update_script()],
                "hint": flatpak_update_hint,
                "parser": parse_flatpak_progress,
            })
        gwine = shutil.which("gwine") or (
            "/usr/bin/gwine" if Path("/usr/bin/gwine").exists() else ""
        )
        if gwine:
            steps.append({
                "label": "gwine (composants)",
                "argv": [gwine, "--init"],
                "parser": parse_gablue_progress,
                "env": {"GABLUE_PROGRESS": "1"},
            })
        pending = list(pending_apps or [])
        if pending:
            names = ", ".join(apps_names(pending))
            steps.append({
                "label": f"Applications ({names})",
                "argv": [_which("bash"), "-c", apps_update_script(pending)],
                "parser": parse_gablue_progress,
                # Un AppImage se télécharge d'un bloc : aucune fraction à
                # afficher pendant le transfert, la barre balaie et le libellé
                # nomme l'application en cours (i/N).
                "busy": True,
            })
        if brew_available():
            steps.append({
                "label": "Homebrew",
                "argv": [_which("bash"), "-c", brew_update_script()],
                "hint": brew_update_hint,
                "parser": brew_progress_parser(),
            })
        return steps

    def update_local_gablue(self) -> None:
        """Image système Gablue (bootc), appliquée au prochain démarrage."""
        answer = QMessageBox.question(
            self, "Mise à jour de Gablue",
            "Télécharger la nouvelle image système Gablue ?\n\n"
            "Elle s'applique au prochain redémarrage.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        # Helper dédié : il publie la progression JSONL de bootc dans un FIFO
        # (sudo ferme les fd >= 3, l'app ne peut pas lui en passer un). Repli
        # sur bootc direct tant que l'image n'a pas le helper.
        if Path(UPDATE_HELPER).is_file():
            argv, fifo = [UPDATE_HELPER], UPDATE_PROGRESS_FIFO
        else:
            argv, fifo = [_which("bootc"), "upgrade"], ""
        self._run_local_task(
            "Mise à jour de Gablue",
            "La nouvelle image s'applique au prochain redémarrage.",
            argv, root=True,
            hint_from_output=gablue_update_hint,
            refresh_updates=True,
            progress_fifo=fifo,
            # Image antérieure au helper : bootc ne publie alors aucune
            # progression. Barre indéterminée plutôt que le journal brut, la
            # présentation reste la même (journal derrière « Terminal »).
            progress_mode="auto" if fifo else "busy",
            progress_text="Téléchargement de l'image système…",
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
            "Mettre à jour les applications Flatpak du système ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_local_task(
            "Mise à jour des Flatpaks",
            "Applications Flatpak système.",
            [_which("bash"), "-c", flatpak_update_script()],
            # Hors terminal, flatpak écrit « Updating 3/7… » puis des lignes
            # avec le pourcentage : la barre les lit, le journal reste derrière
            # le bouton « Terminal ».
            hint_from_output=flatpak_update_hint,
            progress_parser=parse_flatpak_progress,
            refresh_updates=True,
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
            "Mettre à jour gwine (runner Wine et composants) ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_local_task(
            "Mise à jour de gwine",
            "Runner Wine et composants (DXVK, VKD3D, D7VK).",
            [gwine, "--init"],
            # gwine publie ses étapes sur la sortie (« ::gablue:step ») :
            # une vraie barre remplace le journal, qui reste derrière
            # « Terminal ».
            progress_parser=parse_gablue_progress,
            env_extra={"GABLUE_PROGRESS": "1"},
            refresh_updates=True,
        )

    def update_local_brew(self) -> None:
        if not brew_available():
            QMessageBox.critical(
                self, "Homebrew", "Homebrew n'est pas installé sur ce poste."
            )
            return
        answer = QMessageBox.question(
            self, "Mise à jour de Homebrew",
            "Mettre à jour les formules Homebrew ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._run_local_task(
            "Mise à jour de Homebrew",
            "Formules installées par brew.",
            [_which("bash"), "-c", brew_update_script()],
            # Homebrew ne donne aucun pourcentage hors terminal (curl est muet) :
            # la barre avance par formule, d'après ses en-têtes.
            hint_from_output=brew_update_hint,
            progress_parser=brew_progress_parser(),
            refresh_updates=True,
        )

    def update_local_apps(self) -> None:
        """Mettre à jour les AppImages installées (scripts d'installation).

        La liste à mettre à jour vient du réseau (un flux de releases par
        application) : la sonde tourne hors du fil graphique, la barre
        d'activité tient l'utilisateur pendant ce temps.
        """
        answer = QMessageBox.question(
            self, "Mettre à jour les applications",
            "Mettre à jour les applications installées sur ce poste ?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._probe_apps(self._apps_update_ready)

    def _probe_apps(self, then) -> None:
        """Sonde des AppImages en retard, puis `then(result)` sur le fil GUI.

        `result` = `(clés à mettre à jour, doute, nombre installé)` de
        `apps.pending_updates`. La sonde interroge un flux de releases par
        application : elle tourne hors du fil graphique, la bande d'activité
        tient l'utilisateur pendant ce temps, et une sonde déjà partie n'est
        pas doublée.
        """
        if _is_running(self._apps_worker):
            return
        self._set_busy("apps", True)
        worker = AppsUpdateWorker(self)
        worker.done.connect(lambda result: self._apps_probed(result, then))
        self._apps_worker = worker
        worker.start()

    def _apps_probed(self, result, then) -> None:
        self._apps_worker = None
        self._set_busy("apps", False)
        then(result)

    def _apps_update_ready(self, result) -> None:
        """Sonde terminée : lancer les installations, ou dire pourquoi non."""
        pending, unknown, installed = result
        if not installed:
            QMessageBox.information(
                self, "Applications",
                "Aucune application n'est installée sur ce poste.",
            )
            return
        if not pending:
            if unknown:
                QMessageBox.warning(
                    self, "Applications",
                    "Mise à jour impossible à déterminer : les flux de "
                    "versions n'ont pas répondu pour au moins une "
                    "application.",
                )
            else:
                QMessageBox.information(
                    self, "Applications",
                    "Toutes les applications installées sont à jour.",
                )
            return
        names = ", ".join(apps_names(pending))
        count = len(pending)
        subtitle = (
            f"1 application : {names}."
            if count == 1 else f"{count} applications : {names}."
        )
        self._run_local_task(
            "Mise à jour des applications",
            subtitle,
            [_which("bash"), "-c", apps_update_script(pending)],
            progress_parser=parse_gablue_progress,
            progress_text="Mise à jour des applications…",
            refresh_updates=True,
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

    def open_prefs(self) -> None:
        dialog = PrefsDialog(self)
        if dialog.exec() != QDialog.Accepted:
            return
        self.set_prefs(dialog.values())

    def open_local_services(self) -> None:
        ServicesDialog(self).exec()

    def open_local_updates(self) -> None:
        AutoUpdatesDialog(self).exec()

    def open_local_variant(self) -> None:
        VariantDialog(self).exec()

    def open_local_btrfs(self) -> None:
        BtrfsDialog(self).exec()

    def open_local_thermal(self) -> None:
        ThermalDialog(self).exec()

    def open_local_ryzen(self) -> None:
        RyzenDialog(self).exec()

    def open_clean_system(self) -> None:
        """Nettoyer ce poste : dialogue non modal, une instance."""
        self._open_clean(None)

    def open_remote_clean(self, host_id: str) -> None:
        host = self._host(host_id)
        if host is not None:
            self._open_clean(host)

    def _open_clean(self, host: dict | None) -> None:
        """Ouvre le nettoyage sans bloquer la fenêtre de progression.

        Une tâche lancée depuis un dialogue **modal** s'affiche mais ne reçoit
        aucun clic : la modalité du dialogue parent (nettoyage d'un poste
        distant ouvert par `exec()`) bloquait « Fermer » sur la fenêtre de
        progression terminée (vécu 07/11/2026). Le dialogue reste donc ouvert
        sans modalité, comme pour le poste local, et une seule instance par
        cible évite d'en empiler.
        """
        key = host["id"] if host else ""
        existing = self._clean_dialogs.get(key)
        if existing is not None:
            try:
                existing.raise_()
                existing.activateWindow()
                return
            except RuntimeError:      # fermé et détruit entre-temps
                self._clean_dialogs.pop(key, None)
        dialog = CleanDialog(self, host) if host else CleanDialog(self)
        dialog.setModal(False)
        dialog.setAttribute(Qt.WA_DeleteOnClose, True)
        dialog.finished.connect(
            lambda _result=0, k=key: self._clean_dialogs.pop(k, None)
        )
        self._clean_dialogs[key] = dialog
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def open_grub(self) -> None:
        GrubDialog(self).exec()

    def open_secureboot(self) -> None:
        SecureBootDialog(self).exec()

    def open_boot_select(self) -> None:
        """Choisir un autre disque amorçable pour le prochain démarrage."""
        BootDialog(self).exec()

    def reboot_firmware(self) -> None:
        """Redémarre dans le BIOS/UEFI, après confirmation.

        `systemctl reboot --firmware-setup` pose la demande dans les
        variables EFI ; le menu du firmware s'ouvre au démarrage suivant, puis
        le démarrage normal reprend à sa fermeture. `sudo -n` et règle
        sudoers exacte : aucune invite (un échec de règle s'affiche dans
        le journal de la tâche, pas dans un dialogue de mot de passe).
        """
        if not Path("/sys/firmware/efi").is_dir():
            QMessageBox.information(
                self, "Redémarrer dans le BIOS",
                "Ce poste ne démarre pas en mode UEFI : le BIOS ne peut pas "
                "être ouvert depuis Gablue. Utilisez la touche du fabricant "
                "au démarrage.",
            )
            return
        answer = QMessageBox.question(
            self, "Redémarrer dans le BIOS",
            "Redémarrer maintenant dans le BIOS (UEFI) ?\n\n"
            "Le poste redémarre immédiatement et le menu du BIOS s'ouvre à "
            "la place de Gablue ; le démarrage normal reprend à sa "
            "fermeture.\n\n"
            "Les sessions ouvertes seront interrompues.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self._launch(
            ["sudo", "-n", "/usr/bin/systemctl", "reboot",
             "--firmware-setup"],
            "Redémarrer dans le BIOS",
        )

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
                        root: bool = False, hint_from_output=None,
                        progress_fifo: str = "",
                        progress_parser=None,
                        env_extra: dict | None = None,
                        progress_mode: str = "auto",
                        progress_text: str = "",
                        refresh_updates: bool = False) -> None:
        if argv and shutil.which(argv[0]) is None and not Path(argv[0]).exists():
            QMessageBox.critical(
                self, title,
                f"« {argv[0]} » introuvable sur ce poste.",
            )
            return
        dialog = LocalTaskDialog(
            self, title=title, subtitle=subtitle, argv=argv, root=root,
            hint_from_output=hint_from_output, progress_fifo=progress_fifo,
            progress_parser=progress_parser, env_extra=env_extra,
            progress_mode=progress_mode, progress_text=progress_text,
        )
        if refresh_updates:
            dialog.completed.connect(self._updates_after_task)
        start_task(self, dialog)

    # -- recherche sur le réseau local -------------------------------------
    def discover_hosts(self) -> None:
        if self._discovering:
            return
        if AVAHI_BROWSE == "avahi-browse" and not shutil.which("avahi-browse"):
            QMessageBox.information(
                self, "Recherche sur le réseau local",
                "L'outil de recherche réseau n'est pas installé "
                "(paquet avahi-tools).",
            )
            return
        self._discovering = True
        self._set_busy("discover", True)
        self.auto_label.setText("Recherche de postes sur le réseau local…")
        worker = AvahiWorker(self)
        worker.found.connect(self._discovery_done)
        worker.finished.connect(worker.deleteLater)
        self._avahi_worker = worker
        worker.start()

    def _discovery_done(self, hosts: list) -> None:
        self._discovering = False
        self._avahi_worker = None
        self._set_busy("discover", False)
        self._update_footer()
        if not hosts:
            QMessageBox.information(
                self, "Recherche sur le réseau local",
                "Aucun poste annoncé sur le réseau local.\n\n"
                "Vérifiez que l'annonce locale est active sur les postes et "
                "qu'ils sont sur le même réseau.",
            )
            return
        self._pick_candidates(
            "Postes trouvés sur le réseau local",
            "Cochez les postes à ajouter.",
            hosts,
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
            # Rien n'est enregistré ici : la clé est vérifiée d'abord, et un
            # poste dont elle ne passe pas n'est jamais ajouté.
            self._install_keys(added)

    def _add_record(self, candidate: dict) -> dict | None:
        """Fiche d'un poste découvert, sans l'ajouter au registre.

        L'ajout effectif (`_key_installed`) n'a lieu qu'une fois la clé
        vérifiée : la découverte ne suffit pas.
        """
        # La découverte connaît déjà toutes les adresses de la machine (LAN et
        # Tailscale) : on garde les autres en secours au lieu d'en jeter.
        addresses = [
            {"address": address, "kind": kind}
            for address, kind in candidate.get("hosts") or []
            if address and address != candidate.get("host")
        ]
        host = {
            "id": uuid.uuid4().hex,
            "label": str(candidate.get("label") or "").strip(),
            "user": str(candidate["user"]).strip(),
            "host": str(candidate["host"]).strip(),
            "port": int(candidate.get("port", 22)),
            "addresses": addresses,
            "active": "",
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
    def _background_workers(self) -> list:
        """Sondes de fond encore vivantes (toutes classes confondues)."""
        workers = [
            self._checker, self._updates_worker, self._apps_worker,
            self._avahi_worker, self._hardware_worker,
            *self._remote_hardware_workers.values(),
            *self._remote_check_workers,
            *self._remote_apps_workers,
            *self._address_workers, *self._key_workers,
        ]
        return [
            worker for worker in workers
            if worker is not None and _is_running(worker)
        ]

    def has_running_workers(self) -> bool:
        """Reste-t-il une sonde après la fermeture ?

        L'entrée du programme s'en sert pour sortir immédiatement
        (`os._exit`) : attendre un `brew update` de deux minutes après que la
        fenêtre a disparu, c'est ce qui faisait croire que le processus était
        bloqué (vécu 07/11/2026).
        """
        return bool(self._background_workers())

    def _settle_workers(self, entries, budget: float = 3.0) -> None:
        """Arrête les sondes de fond avant la fermeture de la fenêtre.

        Deux temps : on demande l'arrêt à toutes (annulation coopérative),
        puis on attend avec un budget global. Jamais `terminate()` — couper
        un thread Python en plein appel (pthread_cancel) laisse
        l'interpréteur dans un état incohérent et fait tomber le processus.
        Ce qui résiste est laissé vivant : `has_running_workers` fait alors
        sortir l'entrée du programme sans attendre.
        """
        entries = [
            (worker, timeout) for worker, timeout in entries
            if worker is not None and _is_running(worker)
        ]
        for worker, _timeout in entries:
            _ask_stop(worker)
        deadline = time.monotonic() + budget
        for worker, timeout in entries:
            remaining = min(timeout, deadline - time.monotonic())
            if remaining <= 0:
                break
            worker.wait(int(remaining * 1000))

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
        # Toutes les sondes de fond : arrêt demandé, attente courte, puis
        # coupure. Un QThread détruit en vol fait tomber l'application
        # (« QThread: Destroyed while thread is still running »), et une
        # commande de plusieurs minutes retenait le processus après la
        # fermeture (vécu 07/11/2026).
        # Un seul budget d'attente pour toutes les sondes : elles tournent en
        # parallèle, les attendre l'une après l'autre ferait payer la somme
        # des temps sur une fermeture.
        self._settle_workers([
            (self._checker, 2.0),
            (self._updates_worker, 2.0),
            (self._apps_worker, 1.0),
            (self._avahi_worker, 2.0),
            # La sonde Vulkan charge le pilote : quelques secondes.
            (self._hardware_worker, 4.0),
            *[(worker, 2.0)
              for worker in self._remote_hardware_workers.values()],
            *[(worker, 2.0) for worker in self._remote_check_workers],
            *[(worker, 2.0) for worker in self._remote_apps_workers],
            *[(worker, 2.0) for worker in self._address_workers],
            *[(worker, 2.0) for worker in self._key_workers],
        ])
        if (self._sshd_process is not None
                and self._sshd_process.state() != QProcess.NotRunning):
            self._sshd_process.kill()
        if (self._avahi_process is not None
                and self._avahi_process.state() != QProcess.NotRunning):
            self._avahi_process.kill()
        super().closeEvent(event)
