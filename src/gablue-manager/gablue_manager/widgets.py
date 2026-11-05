"""Pastille d'état, cartes des postes et carte « Ce poste ».
"""

import getpass
import math
import socket

from PySide6.QtCore import QPointF, QSize, Qt, Signal
from PySide6.QtGui import QColor, QIcon, QPainter, QPen
from PySide6.QtWidgets import (
    QFrame, QHBoxLayout, QLabel, QMenu, QPushButton, QSizePolicy, QToolButton,
    QVBoxLayout, QWidget,
)

from .constants import COLOR_OFFLINE, KEY_STATES, STATE_COLOR, STATE_TEXT
from .local import local_distro
from .util import address_text, display_name, dolphin_url, ssh_target

class StatusDot(QWidget):
    """Pastille colorée ; halo qui respire quand le poste est en ligne."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedSize(26, 26)
        self._state = "unknown"
        self._phase = 0.0

    def set_state(self, state: str) -> None:
        if state == self._state:
            return
        self._state = state
        self.update()

    def animated(self) -> bool:
        return self._state in ("online", "checking")

    def tick(self) -> None:
        if self.animated():
            self._phase += 0.14
            self.update()

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        center = QPointF(self.width() / 2.0, self.height() / 2.0)
        color = QColor(STATE_COLOR.get(self._state, COLOR_OFFLINE))

        if self._state == "online":
            halo = QColor(color)
            halo.setAlpha(int(40 + 55 * (0.5 + 0.5 * math.sin(self._phase))))
            painter.setPen(Qt.NoPen)
            painter.setBrush(halo)
            painter.drawEllipse(center, 9.5, 9.5)
        elif self._state == "checking":
            halo = QColor(color)
            halo.setAlpha(55)
            painter.setPen(Qt.NoPen)
            painter.setBrush(halo)
            radius = 6.5 + 2.5 * (0.5 + 0.5 * math.sin(self._phase))
            painter.drawEllipse(center, radius, radius)
        elif self._state in ("auth", "hostkey", "nokey"):
            halo = QColor(color)
            halo.setAlpha(45)
            painter.setPen(Qt.NoPen)
            painter.setBrush(halo)
            painter.drawEllipse(center, 8.5, 8.5)

        painter.setPen(QPen(QColor(0, 0, 0, 60), 1))
        painter.setBrush(color)
        painter.drawEllipse(center, 5.0, 5.0)
        painter.end()

def _make_tool_button(parent, icon_name: str, fallback: str) -> QToolButton:
    """Bouton-icône compact, avec repli texte si le thème n'a pas l'icône."""
    button = QToolButton(parent)
    button.setObjectName("more")
    button.setCursor(Qt.PointingHandCursor)
    button.setIconSize(QSize(18, 18))
    icon = QIcon.fromTheme(icon_name)
    if icon.isNull():
        button.setText(fallback)
    else:
        button.setIcon(icon)
    return button

class HostCard(QFrame):
    """Une ligne graphique : état, identité, actions."""

    remote_requested = Signal(str)
    remote_stop_requested = Signal(str)
    files_requested = Signal(str)
    edit_requested = Signal(str)
    delete_requested = Signal(str)
    key_requested = Signal(str)
    terminal_requested = Signal(str)
    wake_requested = Signal(str)
    update_requested = Signal(str, str)   # id du poste, type de mise à jour
    settings_requested = Signal(str)
    gwine_requested = Signal(str)
    booster_requested = Signal(str)
    tailscale_requested = Signal(str)
    apps_requested = Signal(str)
    reboot_requested = Signal(str)
    shutdown_requested = Signal(str)

    def __init__(self, host: dict, parent=None):
        super().__init__(parent)
        self.host = host
        self.setObjectName("hostCard")
        self.setAttribute(Qt.WA_Hover, True)
        self.setMinimumHeight(92)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(18, 14, 16, 14)
        layout.setSpacing(14)

        self.dot = StatusDot(self)
        layout.addWidget(self.dot, 0, Qt.AlignVCenter)

        texts = QVBoxLayout()
        texts.setSpacing(2)
        self.name_label = QLabel(display_name(host), self)
        self.name_label.setObjectName("hostName")
        self.detail_label = QLabel(address_text(host), self)
        self.detail_label.setObjectName("hostDetail")
        self.status_label = QLabel(STATE_TEXT["unknown"], self)
        self.status_label.setObjectName("hostStatus")
        texts.addWidget(self.name_label)
        texts.addWidget(self.detail_label)
        texts.addSpacing(3)
        texts.addWidget(self.status_label)
        layout.addLayout(texts, 1)

        self.wake_button = QPushButton("Réveiller", self)
        self.wake_button.setObjectName("ghost")
        self.wake_button.setCursor(Qt.PointingHandCursor)
        self.wake_button.setToolTip(
            "Envoyer un paquet magique Wake-on-LAN à ce poste"
        )
        self.wake_button.setVisible(False)
        self.wake_button.clicked.connect(
            lambda: self.wake_requested.emit(self.host["id"])
        )
        layout.addWidget(self.wake_button, 0, Qt.AlignVCenter)

        self.key_button = QPushButton("Installer la clé", self)
        self.key_button.setObjectName("ghost")
        self.key_button.setCursor(Qt.PointingHandCursor)
        self.key_button.setVisible(False)
        self.key_button.clicked.connect(
            lambda: self.key_requested.emit(self.host["id"])
        )
        layout.addWidget(self.key_button, 0, Qt.AlignVCenter)

        self.remote_button = QPushButton("Bureau distant", self)
        self.remote_button.setObjectName("accent")
        self.remote_button.setCursor(Qt.PointingHandCursor)
        self.remote_button.setIcon(QIcon.fromTheme("computer"))
        self.remote_button.setToolTip("Ouvrir le bureau distant de ce poste")
        self.remote_button.clicked.connect(
            lambda: self.remote_requested.emit(self.host["id"])
        )
        layout.addWidget(self.remote_button, 0, Qt.AlignVCenter)
        self._remote_active = False

        self.files_button = QPushButton("Fichiers", self)
        self.files_button.setCursor(Qt.PointingHandCursor)
        self.files_button.setIcon(QIcon.fromTheme("folder-remote"))
        self.files_button.setToolTip(f'Parcourir {dolphin_url(host)}')
        self.files_button.clicked.connect(
            lambda: self.files_requested.emit(self.host["id"])
        )
        layout.addWidget(self.files_button, 0, Qt.AlignVCenter)

        self.terminal_button = QPushButton("Terminal", self)
        self.terminal_button.setCursor(Qt.PointingHandCursor)
        self.terminal_button.setIcon(QIcon.fromTheme("utilities-terminal"))
        self.terminal_button.setToolTip(
            f"Ouvrir un terminal SSH sur {ssh_target(host)}"
        )
        self.terminal_button.clicked.connect(
            lambda: self.terminal_requested.emit(self.host["id"])
        )
        layout.addWidget(self.terminal_button, 0, Qt.AlignVCenter)

        self.more_button = _make_tool_button(self, "view-more-symbolic", "⋮")
        self.more_button.setPopupMode(QToolButton.InstantPopup)
        menu = QMenu(self.more_button)
        self.remote_stop_action = menu.addAction(
            "Fermer la session de bureau distant"
        )
        self.remote_stop_action.setEnabled(False)
        self.remote_stop_action.triggered.connect(
            lambda: self.remote_stop_requested.emit(self.host["id"])
        )
        menu.addSeparator()
        self.wake_action = menu.addAction("Réveiller le poste")
        self.wake_action.triggered.connect(
            lambda: self.wake_requested.emit(self.host["id"])
        )
        menu.addSeparator()
        update_menu = menu.addMenu("Mettre à jour")
        for kind, label in (
            ("gablue", "Gablue (image système)…"),
            ("flatpak", "Flatpaks…"),
            ("gwine", "Gwine (composants)…"),
        ):
            update_menu.addAction(
                label,
                lambda _checked=False, k=kind: self.update_requested.emit(
                    self.host["id"], k
                ),
            )
        settings_action = menu.addAction("Réglages du noyau…")
        settings_action.triggered.connect(
            lambda: self.settings_requested.emit(self.host["id"])
        )
        gwine_action = menu.addAction("Options de gwine…")
        gwine_action.triggered.connect(
            lambda: self.gwine_requested.emit(self.host["id"])
        )
        booster_action = menu.addAction("Priorité au premier plan…")
        booster_action.triggered.connect(
            lambda: self.booster_requested.emit(self.host["id"])
        )
        tailscale_action = menu.addAction("Tailscale…")
        tailscale_action.triggered.connect(
            lambda: self.tailscale_requested.emit(self.host["id"])
        )
        apps_action = menu.addAction("Applications…")
        apps_action.triggered.connect(
            lambda: self.apps_requested.emit(self.host["id"])
        )
        reboot_action = menu.addAction("Redémarrer…")
        reboot_action.triggered.connect(
            lambda: self.reboot_requested.emit(self.host["id"])
        )
        shutdown_action = menu.addAction("Éteindre…")
        shutdown_action.triggered.connect(
            lambda: self.shutdown_requested.emit(self.host["id"])
        )
        menu.addSeparator()
        self.key_action = menu.addAction("Installer la clé SSH…")
        self.key_action.triggered.connect(
            lambda: self.key_requested.emit(self.host["id"])
        )
        edit_action = menu.addAction("Modifier…")
        edit_action.triggered.connect(
            lambda: self.edit_requested.emit(self.host["id"])
        )
        menu.addSeparator()
        delete_action = menu.addAction("Supprimer…")
        delete_action.triggered.connect(
            lambda: self.delete_requested.emit(self.host["id"])
        )
        self.more_button.setMenu(menu)
        layout.addWidget(self.more_button, 0, Qt.AlignVCenter)

        self.set_state("unknown", "")

    # -- état ---------------------------------------------------------------
    @property
    def remote_active(self) -> bool:
        return self._remote_active

    def set_remote_active(self, active: bool) -> None:
        """État de la session suivie : active l'entrée « Fermer » du menu ⋯.

        Le bouton, lui, relance toujours une connexion (nouvelle fenêtre) :
        c'est le comportement attendu, un second client RDP coupant le
        précédent côté KRdp.
        """
        self._remote_active = active
        self.remote_stop_action.setEnabled(active)

    def set_state(self, state: str, detail: str = "",
                  latency: float = 0.0) -> None:
        self.dot.set_state(state)
        text = STATE_TEXT.get(state, state)
        if state == "error" and detail:
            text = f"Erreur : {detail}"
        if state == "online" and latency:
            text = f"{text} · {latency:.0f} ms"
        self.status_label.setText(text)
        color = STATE_COLOR.get(state, COLOR_OFFLINE)
        self.status_label.setStyleSheet(f"color: {color};")
        self.key_button.setVisible(state in KEY_STATES)
        self.wake_button.setVisible(
            state in ("refused", "timeout", "noroute", "dns", "error")
            and bool(self.host.get("mac"))
        )
        self.wake_action.setEnabled(bool(self.host.get("mac")))
        if state == "nokey":
            self.key_button.setText("Créer la clé")
        elif state == "hostkey":
            self.key_button.setText("Accepter et installer")
        else:
            self.key_button.setText("Installer la clé")
        self.key_action.setText(
            "Réinstaller la clé SSH…" if self.host.get("key_added")
            else "Installer la clé SSH…"
        )
        if detail:
            self.setToolTip(f"{display_name(self.host)}\n{detail}")
        else:
            self.setToolTip("")

    # -- survol -------------------------------------------------------------
    def _set_hover(self, hovered: bool) -> None:
        self.setProperty("hover", hovered)
        self.style().unpolish(self)
        self.style().polish(self)

    def enterEvent(self, event) -> None:
        self._set_hover(True)
        super().enterEvent(event)

    def leaveEvent(self, event) -> None:
        self._set_hover(False)
        super().leaveEvent(event)

class LocalCard(QFrame):
    """La machine courante : mises à jour et options avancées."""

    update_requested = Signal(str)   # "gablue" | "flatpak" | "gwine"
    apps_requested = Signal()
    config_requested = Signal()
    gwine_requested = Signal()
    booster_requested = Signal()
    tailscale_requested = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("localCard")
        # Même hauteur et même amorce que les cartes de postes : la pastille
        # d'état y fait 26 px, l'icône doit occuper la même largeur, sinon le
        # texte et les boutons de cette carte sont décalés de 2 px.
        self.setMinimumHeight(92)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(18, 14, 16, 14)
        layout.setSpacing(14)

        icon = QLabel(self)
        icon.setFixedSize(26, 26)
        icon.setAlignment(Qt.AlignCenter)
        pixmap = QIcon.fromTheme("computer").pixmap(QSize(20, 20))
        if not pixmap.isNull():
            icon.setPixmap(pixmap)
        layout.addWidget(icon, 0, Qt.AlignVCenter)

        texts = QVBoxLayout()
        texts.setSpacing(2)
        self.name_label = QLabel("Ce poste", self)
        self.name_label.setObjectName("hostName")
        host = socket.gethostname()
        distro = local_distro()
        detail = f"{getpass.getuser()}@{host}"
        if distro:
            detail += f" · {distro}"
        self.detail_label = QLabel(detail, self)
        self.detail_label.setObjectName("hostDetail")
        self.hint_label = QLabel("Mises à jour et options de cette machine", self)
        self.hint_label.setObjectName("hostStatus")
        texts.addWidget(self.name_label)
        texts.addWidget(self.detail_label)
        texts.addSpacing(3)
        texts.addWidget(self.hint_label)
        layout.addLayout(texts, 1)

        self.update_button = QPushButton("Mettre à jour", self)
        self.update_button.setObjectName("accent")
        self.update_button.setCursor(Qt.PointingHandCursor)
        self.update_button.setIcon(QIcon.fromTheme("system-software-update"))
        menu = QMenu(self.update_button)
        menu.addAction(
            "Gablue (image système)…", lambda: self.update_requested.emit("gablue")
        )
        menu.addAction(
            "Flatpaks…", lambda: self.update_requested.emit("flatpak")
        )
        menu.addAction(
            "Gwine (composants)…", lambda: self.update_requested.emit("gwine")
        )
        self.update_button.setMenu(menu)
        layout.addWidget(self.update_button, 0, Qt.AlignVCenter)

        # Les applications AppImage (GearLever) sont des programmes, pas des
        # réglages : entrée à part, à côté des mises à jour.
        self.apps_button = QPushButton("Applications", self)
        self.apps_button.setObjectName("ghost")
        self.apps_button.setCursor(Qt.PointingHandCursor)
        self.apps_button.setIcon(QIcon.fromTheme("applications-utilities"))
        self.apps_button.setToolTip(
            "Installer, mettre à jour ou supprimer ES-DE, Azahar, Eden, "
            "Xenia, ShadPS4 et OpenCode 2"
        )
        self.apps_button.clicked.connect(self.apps_requested.emit)
        layout.addWidget(self.apps_button, 0, Qt.AlignVCenter)

        # Un bouton, un menu : réglages du noyau (kargs + émulation CPUID),
        # options de gwine (préfixe Windows), priorité au premier plan et
        # Tailscale. Chacun s'applique à un endroit différent, d'où des
        # entrées séparées plutôt qu'un dialogue fourre-tout.
        self.config_button = QPushButton("Configuration", self)
        self.config_button.setObjectName("ghost")
        self.config_button.setCursor(Qt.PointingHandCursor)
        self.config_button.setIcon(QIcon.fromTheme("settings-configure"))
        config_menu = QMenu(self.config_button)
        config_menu.addAction("Réglages du noyau…", self.config_requested.emit)
        config_menu.addAction("Options de gwine…", self.gwine_requested.emit)
        config_menu.addAction(
            "Priorité au premier plan…", self.booster_requested.emit
        )
        config_menu.addAction("Tailscale…", self.tailscale_requested.emit)
        self.config_button.setMenu(config_menu)
        layout.addWidget(self.config_button, 0, Qt.AlignVCenter)
