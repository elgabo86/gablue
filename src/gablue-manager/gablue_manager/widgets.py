"""Barre d'activité, pastille d'état, cartes des postes et menus de configuration.
"""

import getpass
import math
import socket

from PySide6.QtCore import QEvent, QPointF, QRectF, QSize, Qt, QTimer, Signal
from PySide6.QtGui import (
    QColor, QIcon, QPainter, QPainterPath, QPalette, QPen,
)
from PySide6.QtWidgets import (
    QButtonGroup, QFrame, QGridLayout, QHBoxLayout, QLabel, QMenu,
    QProgressBar, QPushButton, QSizePolicy, QTabWidget, QToolButton,
    QVBoxLayout, QWidget,
)

from .constants import (
    COLOR_AUTH, COLOR_OFFLINE, COLOR_ONLINE, KEY_STATES, STATE_COLOR,
    STATE_TEXT,
)
from .local import local_distro, ryzenadj_available
from .style import accent_color
from .updates import (
    DOMAINS, days_text, domain_count, summary_lines, total_updates,
)
from .util import active_kind, address_text, display_name, dolphin_url, ssh_target

class CenteredTabs(QTabWidget):
    """QTabWidget à deux onglets, centrés, avec un en-tête maison.

    Ni Qt ni la feuille de style ne centrent une `QTabBar` : le seul conseil
    prévu (`SH_TabBar_Alignment`) est ignoré dès qu'un QSS est appliqué, et
    un `QProxyStyle` posé sur la barre fait planter Qt à la fermeture
    (segfault constaté 07/11/2026). On masque donc la barre native et on
    affiche nos propres boutons d'onglet, dans un `header()` que la fenêtre
    pose au-dessus du widget. Même API pour le reste du code (`addTab`,
    `setCurrentIndex`, `setTabText`, `currentChanged`).

    `reclicked` distingue le clic sur l'onglet **déjà** courant : QTabWidget
    n'émet rien dans ce cas, et c'est ainsi qu'on referme la page d'un poste.
    """

    reclicked = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.tabBar().hide()
        self._buttons: list[QPushButton] = []
        self._group = QButtonGroup(self)
        self._group.setExclusive(True)
        self._header = QWidget()
        header = QHBoxLayout(self._header)
        header.setContentsMargins(0, 6, 0, 0)
        header.setSpacing(4)
        # Un ressort de chaque côté : les onglets restent centrés.
        header.addStretch(1)
        header.addStretch(1)
        self.currentChanged.connect(self._sync_buttons)

    def header(self) -> QWidget:
        """La rangée de boutons, à poser au-dessus du QTabWidget."""
        return self._header

    def addTab(self, widget, icon, label) -> int:
        index = super().addTab(widget, icon, label)
        row = self._header.layout()
        button = QPushButton(label, self._header)
        button.setObjectName("tabButton")
        button.setCheckable(True)
        button.setCursor(Qt.PointingHandCursor)
        if not icon.isNull():
            button.setIcon(icon)
        button.clicked.connect(
            lambda checked=False, i=index: self._tab_clicked(i)
        )
        # Insérer avant le ressort de droite pour garder le centrage.
        row.insertWidget(index + 1, button)
        self._buttons.append(button)
        self._group.addButton(button, index)
        self._sync_buttons()
        return index

    def setTabText(self, index: int, text: str) -> None:
        super().setTabText(index, text)
        if 0 <= index < len(self._buttons):
            self._buttons[index].setText(text)

    def _tab_clicked(self, index: int) -> None:
        if index == self.currentIndex():
            self.reclicked.emit(index)
            return
        self.setCurrentIndex(index)

    def _sync_buttons(self, index: int = 0) -> None:
        for position, button in enumerate(self._buttons):
            button.setChecked(position == index)


class BusyProgressBar(QProgressBar):
    """Barre qui balaie de gauche à droite quand le total est inconnu.

    `QProgressBar` en mode indéterminé (minimum == maximum) s'en remet au style
    Qt : sous une feuille de style maison, l'animation ne part pas et la barre
    reste inerte — or c'est justement la phase où l'on veut montrer que ça
    travaille (téléchargements de brew, bootc sans total). On peint donc
    nous-mêmes un segment qui traverse la piste, à 60 images/s ; le mode
    déterminé garde le rendu natif (texte du pourcentage compris).
    """

    # Un balayage complet (sortie à droite, réentrée à gauche) par 1,2 s.
    PERIOD_MS = 1200
    STEP_MS = 16

    def __init__(self, parent=None, track: bool = True):
        super().__init__(parent)
        self._phase = 0.0
        self._track = track
        self._active = True
        self._timer = QTimer(self)
        self._timer.setInterval(self.STEP_MS)
        self._timer.timeout.connect(self._advance)

    def _indeterminate(self) -> bool:
        return self.maximum() == self.minimum()

    def set_active(self, active: bool) -> None:
        """Allume/éteint le balayage sans changer la visibilité.

        Le bandeau de la fenêtre garde ainsi sa place dans la mise en page au
        repos : l'afficher/masquer faisait descendre l'interface de 3 px à
        chaque vérification (constaté 07/11/2026).
        """
        self._active = bool(active)
        self._sync_timer()
        self.update()

    def setRange(self, minimum: int, maximum: int) -> None:
        was_busy = self._indeterminate()
        super().setRange(minimum, maximum)
        if self._indeterminate() and not was_busy:
            self._phase = 0.0          # départ à gauche, jamais au milieu
        self._sync_timer()
        self.update()

    def showEvent(self, event) -> None:
        super().showEvent(event)
        self._sync_timer()

    def hideEvent(self, event) -> None:
        # Un timer qui tourne sur une barre cachée ne fait que du calcul.
        self._timer.stop()
        super().hideEvent(event)

    def _sync_timer(self) -> None:
        if self._indeterminate() and self.isVisible() and self._active:
            if not self._timer.isActive():
                self._timer.start()
        else:
            self._timer.stop()

    def _advance(self) -> None:
        self._phase = (self._phase + self.STEP_MS / self.PERIOD_MS) % 1.0
        self.update()

    def paintEvent(self, event) -> None:
        if not self._active:
            return
        if not self._indeterminate():
            super().paintEvent(event)
            return

        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        rect = QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5)
        if rect.width() <= 0 or rect.height() <= 0:
            return
        radius = min(rect.height() / 2.0, 5.0)
        track = QPainterPath()
        track.addRoundedRect(rect, radius, radius)

        # QProgressBar ne peint son fond QSS que dans son propre paintEvent :
        # ici on le remplace, la piste est donc à redessiner, avec les mêmes
        # teintes que @barTrack@ (thème clair comme sombre). Le bandeau de la
        # fenêtre s'en passe : seul le segment doit se voir, sans boîte.
        if self._track:
            window = self.palette().color(QPalette.Window)
            tint = (QColor(255, 255, 255, 26) if window.lightness() < 128
                    else QColor(0, 0, 0, 22))
            painter.fillPath(track, tint)

        width = max(rect.width() * 0.32, min(48.0, rect.width()))
        x = rect.left() + (rect.width() + width) * self._phase - width
        segment = QPainterPath()
        segment.addRoundedRect(
            QRectF(x, rect.top(), width, rect.height()), radius, radius
        )
        painter.setClipPath(track)     # pas de débordement des coins arrondis
        painter.fillPath(segment, accent_color())
        painter.end()

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

# L'ordre fait la lecture : matériel d'abord, puis le système.
INFO_FIELDS = (
    ("model", "Modèle"),
    ("bios", "BIOS"),
    ("cpu", "Processeur"),
    ("gpu", "Carte graphique"),
    ("memory", "Mémoire"),
    ("modules", "Barrettes"),
    ("kernel", "Noyau"),
    ("image", "Image Gablue"),
    ("system", "Système"),
    ("uptime", "Démarrage"),
    ("secure_boot", "Secure Boot"),
)

class InfoPanel(QFrame):
    """Résumé matériel et système de la machine courante, en deux colonnes."""

    filelight_requested = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("panel")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(18, 14, 18, 16)
        layout.setSpacing(12)

        title = QLabel("Matériel et système", self)
        title.setObjectName("panelTitle")
        layout.addWidget(title)

        grid = QGridLayout()
        grid.setHorizontalSpacing(28)
        grid.setVerticalSpacing(10)
        self._values: dict[str, QLabel] = {}
        self._labels: dict[str, QLabel] = {}
        for index, (key, label) in enumerate(INFO_FIELDS):
            row, column = divmod(index, 2)
            cell = QVBoxLayout()
            cell.setSpacing(1)
            name = QLabel(label, self)
            name.setObjectName("specLabel")
            value = QLabel("—", self)
            value.setObjectName("specValue")
            # Copier une ligne (modèle de barrette, version) sans ouvrir un
            # terminal est le geste attendu sur un résumé.
            value.setTextInteractionFlags(Qt.TextSelectableByMouse)
            cell.addWidget(name)
            cell.addWidget(value)
            grid.addLayout(cell, row, column)
            self._values[key] = value
            self._labels[key] = name
        grid.setColumnStretch(0, 1)
        grid.setColumnStretch(1, 1)
        layout.addLayout(grid)

        # Les disques ne tiennent pas dans une case du tableau : une ligne par
        # périphérique, avec son pourcentage et le lien Filelight. Ils passent
        # donc sous la grille, sur toute la largeur.
        self._disks_title = QLabel("Disques", self)
        self._disks_title.setObjectName("specLabel")
        self._disks_title.setVisible(False)
        layout.addWidget(self._disks_title)
        self._disks_box = QVBoxLayout()
        self._disks_box.setContentsMargins(0, 0, 0, 0)
        self._disks_box.setSpacing(3)
        layout.addLayout(self._disks_box)
        self._disk_lines: list[QWidget] = []

    def set_values(self, values: dict[str, str]) -> None:
        """Met à jour les champs fournis ; un champ vide garde l'ancienne valeur."""
        for key, text in (values or {}).items():
            label = self._values.get(key)
            if label is not None and text:
                label.setText(str(text))

    def set_label(self, key: str, text: str) -> None:
        """Renomme un champ (« Modèle » → « Carte mère » selon la machine)."""
        label = self._labels.get(key)
        if label is not None and text:
            label.setText(text)

    def set_disks(self, rows: list[dict], links: bool = True) -> None:
        """Une ligne par disque : nom, tailles, pourcentage, Filelight.

        `links=False` pour un poste distant : Filelight ne lit que la machine
        locale, un bouton y ouvrirait le disque local du même nom.
        """
        for line in self._disk_lines:
            self._disks_box.removeWidget(line)
            line.deleteLater()
        self._disk_lines.clear()
        rows = [row for row in (rows or []) if row]
        self._disks_title.setVisible(bool(rows))
        for row in rows:
            line = QWidget(self)
            line_layout = QHBoxLayout(line)
            line_layout.setContentsMargins(0, 0, 0, 0)
            line_layout.setSpacing(8)
            name = QLabel(f"{row.get('label') or row.get('mount') or ''} :", line)
            name.setObjectName("specValue")
            detail = QLabel(str(row.get("text") or ""), line)
            detail.setObjectName("specLabel")
            detail.setTextInteractionFlags(Qt.TextSelectableByMouse)
            percent_value = int(row.get("percent") or 0)
            percent = QLabel(f"{percent_value} %", line)
            percent.setObjectName("specLabel")
            if percent_value >= 90:
                percent.setStyleSheet(f"color: {COLOR_OFFLINE};")
            line_layout.addWidget(name)
            line_layout.addWidget(detail)
            line_layout.addWidget(percent)
            if links and row.get("path"):
                button = QPushButton("Libérer de l'espace", line)
                button.setObjectName("ghost")
                button.setCursor(Qt.PointingHandCursor)
                path = str(row["path"])
                button.setToolTip(
                    f"Ouvrir Filelight sur {path} pour voir ce qui occupe "
                    "le disque"
                )
                button.clicked.connect(
                    lambda checked=False, p=path:
                    self.filelight_requested.emit(p)
                )
                line_layout.addWidget(button)
            # Le bouton suit immédiatement les chiffres du disque : poussé à
            # droite, il semblait appartenir à une autre ligne.
            line_layout.addStretch(1)
            self._disks_box.addWidget(line)
            self._disk_lines.append(line)

class UpdatesPanel(QFrame):
    """Détail des mises à jour disponibles, comme les lignes du tray.

    Le statut tient en une phrase (« 2 disponibles », « À jour · image
    déployée aujourd'hui ») ; ici on montre ce que le tray affiche : versions
    avant → après des composants gwine, image téléchargée, noms des Flatpaks
    et formules en retard. Les lignes « à jour » sont écartées (le statut le
    dit déjà), tout le reste est repris de `updates.summary_lines` — une seule
    source pour les deux affichages.
    """

    check_requested = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("panel")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(18, 12, 18, 14)
        layout.setSpacing(6)

        header = QHBoxLayout()
        header.setSpacing(10)
        title = QLabel("Mises à jour", self)
        title.setObjectName("panelTitle")
        header.addWidget(title)
        self.status = QLabel("Pas encore vérifié", self)
        self.status.setObjectName("specLabel")
        header.addWidget(self.status)
        header.addStretch(1)
        self.check_button = QPushButton("Vérifier", self)
        self.check_button.setObjectName("ghost")
        self.check_button.setCursor(Qt.PointingHandCursor)
        self.check_button.setIcon(QIcon.fromTheme("view-refresh"))
        self.check_button.setToolTip(
            "Vérifier les mises à jour de ce poste maintenant"
        )
        self.check_button.clicked.connect(self.check_requested.emit)
        header.addWidget(self.check_button)
        # Mise à jour au même endroit que « Vérifier » : la carte du poste
        # n'a plus à porter ce bouton, elle ne garde que son identité.
        self.update_button = QPushButton("Mettre à jour", self)
        self.update_button.setObjectName("accent")
        self.update_button.setCursor(Qt.PointingHandCursor)
        self.update_button.setIcon(QIcon.fromTheme("system-software-update"))
        self.update_button.setToolTip("Mettre à jour ce poste")
        header.addWidget(self.update_button)
        layout.addLayout(header)

        self.body = QVBoxLayout()
        self.body.setSpacing(2)
        layout.addLayout(self.body)
        self._lines: list[QLabel] = []

    def set_checking(self) -> None:
        self.check_button.setEnabled(False)
        self.status.setText("Vérification en cours…")

    def end_checking(self) -> None:
        self.check_button.setEnabled(True)

    def set_update_menu(self, entries: list) -> None:
        """Remplit le menu de « Mettre à jour » (None = séparateur).

        Les entrées sont fournies par la fenêtre : elle seule sait si le
        poste est local ou distant, et quelles actions existent.
        """
        menu = QMenu(self.update_button)
        for entry in entries or []:
            if entry is None:
                menu.addSeparator()
                continue
            label, callback = entry
            menu.addAction(label, callback)
        self.update_button.setMenu(menu)

    def set_updates(self, state: dict | None) -> None:
        self.check_button.setEnabled(True)
        total = total_updates(state)
        self.update_button.setText(
            f"Mettre à jour ({total})" if total else "Mettre à jour"
        )
        self.status.setText(self._status_text(state))
        if state is None:
            self._clear()
            return
        # Une ligne par domaine ; « à jour » est déjà dans le statut, inutile
        # de remplir le panneau de bonnes nouvelles. L'âge du déploiement ne
        # figure pas ici : il est dans « Matériel et système » (et dans le
        # statut), une deuxième fois serait du bruit (demandé le 08/11/2026).
        lines = [
            line for line in summary_lines(state)
            if not line.endswith(" : à jour")
            and not line.startswith("Dernière mise à jour de Gablue")
        ]
        self._clear()
        for text in lines:
            label = QLabel(f"• {text}", self)
            label.setObjectName("updateLine")
            label.setWordWrap(True)
            label.setTextInteractionFlags(Qt.TextSelectableByMouse)
            self.body.addWidget(label)
            self._lines.append(label)

    def _status_text(self, state: dict | None) -> str:
        """Verdict court, sans répéter le titre « Mises à jour ».

        « 2 disponibles » sous « Mises à jour » ; à jour, l'âge de l'image
        tient dans la même phrase — avant, le panneau disait « aucune mise à
        jour disponible · Gablue mis à jour aujourd'hui » puis répétait la
        date en dessous (vécu 07/11/2026).
        """
        if state is None:
            return "Pas encore vérifié"
        domains = state.get("domains") or {}
        total = total_updates(state)
        if total:
            return f"{total} disponible{'s' if total > 1 else ''}"
        if domains and all(
            (result or {}).get("state") == "skipped"
            for result in domains.values()
        ):
            return "Vérifications désactivées"
        if not state.get("online", True):
            return "Hors ligne — vérification impossible"
        unknown = [
            domain for domain in DOMAINS
            if (domains.get(domain) or {}).get("state")
            in ("unknown", "unavailable")
        ]
        if unknown and len(unknown) == len(DOMAINS):
            return "Vérification impossible"
        age = (domains.get("image") or {}).get("days")
        text = "À jour"
        if age is not None:
            text += f" · image déployée {days_text(age)}"
        return text

    def _clear(self) -> None:
        for label in self._lines:
            label.deleteLater()
        self._lines.clear()

class AccessPanel(QFrame):
    """Serveur SSH et annonce locale, en une rangée compacte.

    Ce que les autres postes peuvent joindre de celui-ci : les deux réglages
    vivent dans l'onglet « Postes SSH », à côté de la liste, et aussi dans
    **Configuration ▸ Réseau** de la carte « Ce poste » — même état, deux
    chemins. Une seule ligne, parce que ce sont deux interrupteurs, pas un
    formulaire.
    """

    sshd_requested = Signal()
    avahi_requested = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("panel")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(18, 10, 14, 10)
        layout.setSpacing(10)

        title = QLabel("Accès à ce poste", self)
        title.setObjectName("panelTitle")
        layout.addWidget(title)
        layout.addSpacing(10)

        self._sshd = self._cluster(layout, "Serveur SSH")
        self._sshd["button"].clicked.connect(self.sshd_requested.emit)
        layout.addSpacing(18)
        self._avahi = self._cluster(layout, "Annonce locale")
        self._avahi["button"].clicked.connect(self.avahi_requested.emit)
        layout.addStretch(1)

    def _cluster(self, layout: QHBoxLayout, title: str) -> dict:
        name = QLabel(title, self)
        name.setObjectName("specValue")
        detail = QLabel("—", self)
        detail.setObjectName("specLabel")
        button = QPushButton("Activer", self)
        button.setObjectName("ghost")
        button.setCursor(Qt.PointingHandCursor)
        layout.addWidget(name)
        layout.addWidget(detail)
        layout.addWidget(button, 0, Qt.AlignVCenter)
        return {"detail": detail, "button": button}

    # -- serveur SSH ---------------------------------------------------------
    def set_sshd(self, active: bool, enabled: bool) -> None:
        row = self._sshd
        row["detail"].setText("Activé" if active else "Désactivé")
        row["detail"].setStyleSheet(
            f"color: {COLOR_ONLINE if active else COLOR_OFFLINE};"
        )
        row["button"].setText("Désactiver" if active else "Activer")
        row["button"].setEnabled(True)
        boot = ("démarre avec le poste" if enabled
                else "ne démarre pas avec le poste")
        row["button"].setToolTip(
            f"Serveur SSH de cette machine ({boot}).\n"
            + ("Cliquer pour le couper : bureau distant, fichiers et "
               "terminal depuis un autre poste cesseront de fonctionner."
               if active else
               "Cliquer pour accepter les connexions SSH entrantes : bureau "
               "distant, fichiers et terminal depuis un autre poste.")
        )

    def set_sshd_pending(self) -> None:
        self._sshd["button"].setEnabled(False)
        self._sshd["button"].setText("Autorisation…")

    # -- annonce locale ------------------------------------------------------
    def set_avahi(self, published: bool, daemon: bool) -> None:
        row = self._avahi
        if not daemon:
            detail = "Service arrêté"
            color = COLOR_AUTH
        elif published:
            detail = "Activée"
            color = COLOR_ONLINE
        else:
            detail = "Désactivée"
            color = COLOR_OFFLINE
        row["detail"].setText(detail)
        row["detail"].setStyleSheet(f"color: {color};")
        row["button"].setText("Désactiver" if published else "Annoncer")
        row["button"].setEnabled(True)
        row["button"].setToolTip(
            "Publication mDNS de ce poste sur le réseau local (nom d'hôte, "
            "adresse IP, port 22 — aucun compte ni clé).\n"
            + ("Cliquer pour ne plus l'annoncer ; SSH reste possible en "
               "saisissant l'adresse à la main." if published else
               "Cliquer pour que les autres postes du réseau local le "
               "trouvent tout seuls.")
        )

    def set_avahi_pending(self) -> None:
        self._avahi["button"].setEnabled(False)
        self._avahi["button"].setText("Autorisation…")

class NoticeBar(QFrame):
    """Bandeau d'alerte actionnable, en haut d'un onglet."""

    action_requested = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("notice")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(16, 10, 12, 10)
        layout.setSpacing(12)
        self.label = QLabel("", self)
        self.label.setObjectName("noticeText")
        self.label.setWordWrap(True)
        layout.addWidget(self.label, 1)
        self.button = QPushButton("", self)
        self.button.setObjectName("ghost")
        self.button.setCursor(Qt.PointingHandCursor)
        self.button.clicked.connect(self.action_requested.emit)
        layout.addWidget(self.button, 0, Qt.AlignVCenter)

    def set_message(self, text: str, tooltip: str = "",
                    action: str = "Redémarrer…") -> None:
        self.label.setText(text)
        self.label.setToolTip(tooltip)
        self.button.setVisible(bool(action))
        self.button.setText(action)
        self.button.setToolTip(tooltip)
        self.setVisible(True)

    def clear(self) -> None:
        self.label.setText("")
        self.setVisible(False)

class HostCard(QFrame):
    """Une ligne graphique : état, identité, actions."""

    remote_requested = Signal(str)
    remote_stop_requested = Signal(str)
    files_requested = Signal(str)
    edit_requested = Signal(str)
    delete_requested = Signal(str)
    key_requested = Signal(str)
    terminal_requested = Signal(str)
    open_requested = Signal(str)          # clic : ouvrir la page du poste
    update_requested = Signal(str, str)   # id du poste, type de mise à jour
    check_requested = Signal(str)         # id du poste : vérifier ses mises à jour
    settings_requested = Signal(str)
    clean_requested = Signal(str)
    btrfs_requested = Signal(str)
    thermal_requested = Signal(str)
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
        # Rangée des actions : la page d'un poste y ajoute sa croix.
        self.row = layout

        self.dot = StatusDot(self)
        layout.addWidget(self.dot, 0, Qt.AlignVCenter)

        texts = QVBoxLayout()
        texts.setSpacing(2)
        self.name_label = QLabel(display_name(host), self)
        self.name_label.setObjectName("hostName")
        self.name_label.setToolTip("Ouvrir la page de ce poste")
        self.detail_label = QLabel(address_text(host), self)
        self.detail_label.setObjectName("hostDetail")
        self.status_label = QLabel(STATE_TEXT["unknown"], self)
        self.status_label.setObjectName("hostStatus")
        texts.addWidget(self.name_label)
        texts.addWidget(self.detail_label)
        texts.addSpacing(3)
        texts.addWidget(self.status_label)
        layout.addLayout(texts, 1)

        self.key_button = QPushButton("Installer la clé", self)
        self.key_button.setObjectName("ghost")
        self.key_button.setCursor(Qt.PointingHandCursor)
        self.key_button.setVisible(False)
        self.key_button.clicked.connect(
            lambda: self.key_requested.emit(self.host["id"])
        )
        layout.addWidget(self.key_button, 0, Qt.AlignVCenter)

        # Ordre : l'action la plus fréquente d'abord. « Mettre à jour » ouvre un
        # menu (vérification à la demande, tout mettre à jour, domaines) ;
        # terminal et fichiers sont des icônes compactes, sinon la rangée
        # débordait avec cinq libellés.
        self.update_button = QPushButton("Mettre à jour", self)
        self.update_button.setObjectName("accent")
        self.update_button.setCursor(Qt.PointingHandCursor)
        self.update_button.setIcon(QIcon.fromTheme("system-software-update"))
        self.update_button.setToolTip("Mettre à jour ce poste")
        update_menu = QMenu(self.update_button)
        self.check_action = update_menu.addAction(
            "Vérifier les mises à jour",
            lambda: self.check_requested.emit(self.host["id"]),
        )
        update_menu.addAction(
            "Tout mettre à jour…",
            lambda: self.update_requested.emit(self.host["id"], "all"),
        )
        update_menu.addSeparator()
        self.update_actions: dict[str, tuple[object, str, str]] = {}
        for kind, domain, label in (
            ("gablue", "image", "Gablue…"),
            ("flatpak", "flatpak", "Flatpaks…"),
            ("gwine", "gwine", "Gwine…"),
            ("brew", "brew", "Brew…"),
        ):
            action = update_menu.addAction(label)
            action.triggered.connect(
                lambda _checked=False, k=kind: self.update_requested.emit(
                    self.host["id"], k
                )
            )
            self.update_actions[kind] = (action, label, domain)
        self.update_button.setMenu(update_menu)
        self.updates_state: dict | None = None
        layout.addWidget(self.update_button, 0, Qt.AlignVCenter)

        self.remote_button = QPushButton("Bureau distant", self)
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
        self.files_button.setToolTip(f"Parcourir {dolphin_url(host)}")
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

        # Un bouton libellé, comme « Configuration » dans l'onglet local : le
        # ⋯ seul ne disait pas ce qu'il ouvrait (constaté 07/11/2026). Tout
        # ce qui agit sur le poste (session distante, redémarrer, éteindre,
        # clé, modifier, supprimer) est rangé dans « Actions ▸ », pour ne pas
        # le mélanger avec la configuration du système.
        self.config_button = QPushButton("Configuration", self)
        self.config_button.setObjectName("ghost")
        self.config_button.setCursor(Qt.PointingHandCursor)
        self.config_button.setIcon(QIcon.fromTheme("settings-configure"))
        self.config_button.setToolTip("Configuration et gestion de ce poste")
        menu = QMenu(self.config_button)
        settings_menu = menu.addMenu("Réglages")
        settings_menu.addAction(
            "Noyau et matériel…",
            lambda: self.settings_requested.emit(self.host["id"]),
        )
        games_menu = settings_menu.addMenu("Jeux")
        games_menu.addAction(
            "Options de gwine…",
            lambda: self.gwine_requested.emit(self.host["id"]),
        )
        games_menu.addAction(
            "Priorité au premier plan…",
            lambda: self.booster_requested.emit(self.host["id"]),
        )
        network_menu = settings_menu.addMenu("Réseau")
        network_menu.addAction(
            "Tailscale…",
            lambda: self.tailscale_requested.emit(self.host["id"]),
        )
        # Compression et limite thermique sont des réglages du matériel,
        # pas de l'entretien : ils rejoignent « Réglages », comme sur le
        # poste local. GRUB, Secure Boot et le BIOS restent strictement
        # locaux (firmware et console).
        for label, signal in (
            ("Compression Btrfs…", self.btrfs_requested),
            ("Limite thermique NVMe…", self.thermal_requested),
        ):
            settings_menu.addAction(
                label,
                lambda _checked=False, target=signal: target.emit(
                    self.host["id"]
                ),
            )
        menu.addAction(
            "Nettoyer le système…",
            lambda: self.clean_requested.emit(self.host["id"]),
        )
        apps_action = menu.addAction("Applications…")
        apps_action.triggered.connect(
            lambda: self.apps_requested.emit(self.host["id"])
        )
        menu.addSeparator()
        host_menu = menu.addMenu("Actions")
        # Fermer la session ouverte d'abord : c'est l'action la plus
        # fréquente, et elle n'a de sens que tant qu'une session tourne.
        self.remote_stop_action = host_menu.addAction(
            "Fermer la session de bureau distant"
        )
        self.remote_stop_action.setEnabled(False)
        self.remote_stop_action.triggered.connect(
            lambda: self.remote_stop_requested.emit(self.host["id"])
        )
        host_menu.addSeparator()
        reboot_action = host_menu.addAction("Redémarrer…")
        reboot_action.triggered.connect(
            lambda: self.reboot_requested.emit(self.host["id"])
        )
        shutdown_action = host_menu.addAction("Éteindre…")
        shutdown_action.triggered.connect(
            lambda: self.shutdown_requested.emit(self.host["id"])
        )
        host_menu.addSeparator()
        self.key_action = host_menu.addAction("Installer la clé SSH…")
        self.key_action.triggered.connect(
            lambda: self.key_requested.emit(self.host["id"])
        )
        edit_action = host_menu.addAction("Modifier…")
        edit_action.triggered.connect(
            lambda: self.edit_requested.emit(self.host["id"])
        )
        host_menu.addSeparator()
        delete_action = host_menu.addAction("Supprimer…")
        delete_action.triggered.connect(
            lambda: self.delete_requested.emit(self.host["id"])
        )
        self.config_button.setMenu(menu)
        layout.addWidget(self.config_button, 0, Qt.AlignVCenter)

        self.set_state("unknown", "")

    def add_action_button(self, button: QWidget) -> None:
        """Ajoute un bouton à la fin de la rangée (croix de fermeture)."""
        self.row.addWidget(button, 0, Qt.AlignVCenter)

    # -- état ---------------------------------------------------------------
    @property
    def remote_active(self) -> bool:
        return self._remote_active

    def set_remote_active(self, active: bool) -> None:
        """État de la session suivie : active l'entrée « Fermer » de Configuration.

        Le bouton, lui, relance toujours une connexion (nouvelle fenêtre) :
        c'est le comportement attendu, un second client RDP coupant le
        précédent côté KRdp.
        """
        self._remote_active = active
        self.remote_stop_action.setEnabled(active)

    def set_state(self, state: str, detail: str = "",
                  latency: float = 0.0) -> None:
        self.dot.set_state(state)
        self._state = state
        self._state_detail = detail
        self._latency = latency
        self._refresh_status()
        color = STATE_COLOR.get(state, COLOR_OFFLINE)
        self.status_label.setStyleSheet(f"color: {color};")
        self.key_button.setVisible(state in KEY_STATES)
        # Adresse réellement utilisée : elle change quand le LAN ne répond plus
        # et que la sonde bascule sur Tailscale (ou l'inverse).
        detail_text = address_text(self.host)
        kind = active_kind(self.host)
        if kind:
            detail_text += f" · {kind}"
        self.detail_label.setText(detail_text)
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

    def _refresh_status(self) -> None:
        """Ligne d'état : joignabilité, puis résultat de la vérification.

        Les mises à jour d'un poste distant ne sont connues qu'après un clic
        sur « Vérifier les mises à jour » : tant que rien n'a été demandé, la
        ligne ne dit rien de plus que l'état du poste.
        """
        state = getattr(self, "_state", "unknown")
        detail = getattr(self, "_state_detail", "")
        latency = getattr(self, "_latency", 0.0)
        text = STATE_TEXT.get(state, state)
        if state == "error" and detail:
            text = f"Erreur : {detail}"
        if state == "online" and latency:
            text = f"{text} · {latency:.0f} ms"
        if self.updates_state is not None:
            total = total_updates(self.updates_state)
            if total:
                text += f" · {total} mise{'s' if total > 1 else ''} à jour"
            elif self.updates_state.get("online", True):
                text += " · à jour"
            else:
                text += " · mises à jour : hors ligne"
        self.status_label.setText(text)

    def set_updates(self, state: dict | None) -> None:
        """Résultat d'une vérification demandée (`None` = jamais vérifié)."""
        self.updates_state = state
        total = total_updates(state)
        self.update_button.setText(
            f"Mettre à jour ({total})" if total else "Mettre à jour"
        )
        lines = summary_lines(state) if state else []
        if total:
            lines.append("Vérifier à nouveau depuis le menu.")
        self.update_button.setToolTip(
            "\n".join(lines) or "Mettre à jour ce poste"
        )
        self.check_action.setEnabled(True)
        domains = (state or {}).get("domains") or {}
        for key, (action, label, domain) in self.update_actions.items():
            count = domain_count(domain, domains.get(domain) or {})
            action.setText(f"{label} — {count}" if count else label)
        self._refresh_status()

    def set_checking(self) -> None:
        """Vérification en cours : aucun verdict avant la réponse du poste."""
        self.check_action.setEnabled(False)
        self.update_button.setText("Vérification…")

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

    def mouseReleaseEvent(self, event) -> None:
        """Clic sur la carte : ouvre la page de ce poste.

        Les boutons de la carte gardent leur propre clic (ils consomment
        l'événement) ; seul le fond, le nom ou la ligne d'état déclenchent
        l'ouverture. Un simple clic suffit : la carte est une entrée de
        liste, pas une icône à double-cliquer.
        """
        if event.button() == Qt.LeftButton:
            self.open_requested.emit(self.host["id"])
            event.accept()
            return
        super().mouseReleaseEvent(event)

class LocalConfigMenu(QMenu):
    """Configuration de la machine courante, menu du bouton de la carte.

    Il porte toutes les actions du poste local : la carte n'a qu'un bouton
    « Configuration » et aucune action à plat, comme celle d'un poste
    distant. Même vocabulaire de part et d'autre (« Réglages »,
    « Maintenance »).
    """

    apps_requested = Signal()
    kernel_requested = Signal()
    updates_requested = Signal()
    services_requested = Signal()
    variant_requested = Signal()
    btrfs_requested = Signal()
    thermal_requested = Signal()
    ryzen_requested = Signal()
    clean_requested = Signal()
    grub_requested = Signal()
    secureboot_requested = Signal()
    bootselect_requested = Signal()
    firmware_requested = Signal()
    gwine_requested = Signal()
    booster_requested = Signal()
    tailscale_requested = Signal()
    sshd_requested = Signal()
    avahi_requested = Signal()

    def __init__(self, parent=None):
        super().__init__("Configuration", parent)

        settings_menu = self.addMenu("Réglages")
        settings_menu.addAction(
            "Noyau et matériel…", self.kernel_requested.emit
        )
        settings_menu.addAction(
            "Services et pilotes…", self.services_requested.emit
        )
        settings_menu.addAction(
            "Mises à jour automatiques…", self.updates_requested.emit
        )
        settings_menu.addAction(
            "Variante de l'image…", self.variant_requested.emit
        )

        # Tout ce qui se règle vit dans « Réglages », comme sur une carte de
        # poste distant (qui y met déjà Jeux et Réseau) : le démarrage n'est
        # pas une catégorie concurrente, c'est un réglage de la machine.
        boot_menu = settings_menu.addMenu("Démarrage")
        boot_menu.addAction("Menu GRUB…", self.grub_requested.emit)
        boot_menu.addAction("Secure Boot…", self.secureboot_requested.emit)


        games_menu = settings_menu.addMenu("Jeux")
        games_menu.addAction("Options de gwine…", self.gwine_requested.emit)
        games_menu.addAction(
            "Priorité au premier plan…", self.booster_requested.emit
        )

        network_menu = settings_menu.addMenu("Réseau")
        network_menu.addAction("Tailscale…", self.tailscale_requested.emit)
        network_menu.addSeparator()
        # Même état que le panneau « Accès à ce poste » : le libellé dit le
        # geste, jamais une case à cocher qui se désynchronise.
        self.sshd_action = network_menu.addAction("Activer le serveur SSH")
        self.sshd_action.triggered.connect(self.sshd_requested.emit)
        self.avahi_action = network_menu.addAction("Annoncer ce poste en SSH")
        self.avahi_action.triggered.connect(self.avahi_requested.emit)

        # Compression, limites thermiques : ce sont des réglages du matériel,
        # pas de l'entretien. Ils ferment « Réglages », comme sur un poste
        # distant ; « Nettoyer le système » reste seul, en action directe
        # (un sous-menu à une entrée n'apprenait rien).
        settings_menu.addAction(
            "Compression Btrfs…", self.btrfs_requested.emit
        )
        settings_menu.addAction(
            "Limite thermique NVMe…", self.thermal_requested.emit
        )
        # Uniquement là où ryzenadj agit : un CPU Ryzen avec l'outil présent.
        # Sur les autres postes, l'entrée n'a aucun sens et n'est pas proposée.
        if ryzenadj_available():
            settings_menu.addAction(
                "Limite de température CPU…", self.ryzen_requested.emit
            )

        # Même ordre et mêmes séparateurs qu'un poste distant : Réglages,
        # Nettoyer le système…, Applications…, puis Actions à part.
        self.addAction(
            "Nettoyer le système…", self.clean_requested.emit
        )
        self.addAction("Applications…", self.apps_requested.emit)
        self.addSeparator()
        actions_menu = self.addMenu("Actions")
        actions_menu.addAction(
            "Démarrer sur un autre système…",
            self.bootselect_requested.emit,
        )
        actions_menu.addAction(
            "Redémarrer dans le BIOS…", self.firmware_requested.emit
        )

    def set_access_state(self, sshd: tuple[bool, bool],
                         avahi: tuple[bool, bool]) -> None:
        """Reflète serveur SSH et annonce locale dans le menu Réseau."""
        active, _enabled = sshd
        self.sshd_action.setText(
            "Désactiver le serveur SSH" if active else "Activer le serveur SSH"
        )
        published, _daemon = avahi
        self.avahi_action.setText(
            "Ne plus annoncer ce poste" if published
            else "Annoncer ce poste en SSH"
        )


class LocalCard(QFrame):
    """La machine courante : identité et configuration, comme une carte de poste.

    Même hauteur et mêmes marges qu'une carte de poste (92 px, icône dans un
    emplacement de 26 px comme la pastille d'état), et le même bouton
    **Configuration** libellé : les deux onglets se ressemblent. La carte ne
    porte ni mise à jour ni applications — le panneau « Mises à jour » et le
    menu de configuration les portent déjà.
    """

    def __init__(self, config_menu: QMenu, parent=None):
        super().__init__(parent)
        self.setObjectName("hostCard")
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
        host = socket.gethostname()
        self.name_label = QLabel(host, self)
        self.name_label.setObjectName("hostName")
        detail = f"{getpass.getuser()}@{host}"
        distro = local_distro()
        if distro:
            detail += f" · {distro}"
        self.detail_label = QLabel(detail, self)
        self.detail_label.setObjectName("hostDetail")
        # Troisième ligne comme sur une carte de poste (nom, adresse, état) :
        # la hauteur de la carte reste la même d'un onglet à l'autre.
        self.state_label = QLabel("Ce poste", self)
        self.state_label.setObjectName("hostStatus")
        texts.addWidget(self.name_label)
        texts.addWidget(self.detail_label)
        texts.addSpacing(3)
        texts.addWidget(self.state_label)
        layout.addLayout(texts, 1)

        self.config_button = QPushButton("Configuration", self)
        self.config_button.setObjectName("ghost")
        self.config_button.setCursor(Qt.PointingHandCursor)
        self.config_button.setIcon(QIcon.fromTheme("settings-configure"))
        self.config_button.setToolTip("Configuration de ce poste")
        self.config_button.setMenu(config_menu)
        layout.addWidget(self.config_button, 0, Qt.AlignVCenter)
