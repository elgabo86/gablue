"""Fenêtre principale : composition des vues, événements, clavier."""

import json
import os

from PySide6.QtCore import QEvent, Qt, QTimer, Signal
from PySide6.QtGui import QIcon
from PySide6.QtNetwork import QNetworkAccessManager
from PySide6.QtWidgets import (
    QFrame, QGridLayout, QHBoxLayout, QLabel, QMainWindow, QPushButton,
    QScrollArea, QStackedWidget, QVBoxLayout, QWidget,
)

from .channels import COUNTRIES_ORDER
from .constants import (
    BASE, CARD_W, CONFIG_DIR, CONFIG_FILE, GRID_MARGIN, GRID_SPACING,
    LOGO_DIR, SUFFIX,
)
from .epgview import EpgViewMixin
from .grid import GridMixin
from .guide import GuideMixin, _GuideContent
from .input import InputMixin
from .logos import LogosMixin
from .mpv import MpvWidget
from .video import VideoMixin


# =============================================================================
# FENÊTRE PRINCIPALE
# =============================================================================

class TVWindow(
    GridMixin, GuideMixin, EpgViewMixin, LogosMixin, VideoMixin, InputMixin,
    QMainWindow,
):
    """Interface TV Gablue avec navigation manette complète."""

    # Signal EPG émis depuis le thread de parsing, reçu dans le thread GUI.
    _epg_ready = Signal(list)

    def __init__(self):
        super().__init__()
        self.setWindowTitle("Gablue TV")
        self.setWindowIcon(QIcon.fromTheme("video-display"))
        self.resize(1120, 720)
        self.setMinimumSize(540, 400)

        # --- Paramètres persistants ---
        self._settings = self._load_settings()
        os.makedirs(LOGO_DIR, exist_ok=True)

        # --- Réseau (logos) ---
        self._net = QNetworkAccessManager(self)
        self._net.finished.connect(self._on_logo_downloaded)
        self._logo_targets = {}   # app_id -> [(widget, taille)] en attente du logo
        self._logo_pending = set()
        self._fallback_icon = QIcon.fromTheme("video-display")

        # --- Navigation ---
        self._cursor = 0
        self._cols = 5
        self._filtered = []
        self._buttons = []
        self._country_idx = 0
        self._country_pills = []
        self._countries_current = list(COUNTRIES_ORDER)  # pays des pastilles actives
        self._active_country = None   # pays du filtre effectivement appliqué
        self._guide_country_stash = None  # pays grille sans guide, restitué au retour
        self._return_view = 0  # vue d'origine (grille/guide) pour le retour vidéo

        # --- MPV embarqué (libmpv + OpenGL) ---
        # (MpvWidget créé dans _setup_ui)
        self._loading_dots = 0
        self._loading_timer = QTimer(self)
        self._loading_timer.setSingleShot(True)
        self._loading_timer.timeout.connect(self._hide_loading)

        # --- Masquage automatique du curseur pendant la lecture ---
        self._cursor_hidden = False
        self._cursor_timer = QTimer(self)
        self._cursor_timer.setSingleShot(True)
        self._cursor_timer.timeout.connect(self._hide_cursor)

        # --- Manettes : TOUTES les manettes connectées pilotent l'app ---
        # dict chemin -> evdev.InputDevice, re-scan incrémental toutes les
        # 2 s (branchement à chaud, renumérotation des nodes après une
        # reconnexion BT). État par device (hat, axes, fronts boutons) :
        # deux manettes ne se marchent pas dessus.
        self._gamepads = {}
        self._pad_states = {}
        self._last_scan = 0.0
        self._last_input = 0
        self._had_gamepad = False

        # --- Accélération navigation (maintien direction) ---
        self._hold_dir = None          # None, 'up', 'down', 'left', 'right'
        self._hold_start = 0.0         # timestamp ms début du maintien
        self._last_repeat = 0.0        # timestamp ms dernière répétition auto

        # --- Inhibition veille écran (portail freedesktop, D-Bus) ---
        self._inhibit_handle = None

        # --- Guide TV (EPG) ---
        self._epg = {}              # app_id -> [(start, stop, titre, catégorie)] triée
        self._epg_status = "⏳ Chargement du guide…"
        self._epg_failed = False
        self._epg_downloading = False
        self._card_epg_labels = {}  # app_id -> QLabel « en cours » de la carte
        self._card_epg_state = {}   # app_id -> (start, stop) du titre affiché
        self._guide_data = []       # métadonnées des lignes (chaîne + programmes)
        self._guide_row_widgets = {}  # idx -> widgets construits (lignes visibles)
        self._guide_row = 0
        self._guide_slot = 0
        self._guide_next_flip = 0.0 # timestamp du prochain changement de programme
        self._grid_dirty = False    # grille à reconstruire au prochain affichage
        self._guide_dirty = False   # guide à reconstruire au prochain affichage
        self._logo_pixmem = {}      # app_id -> QPixmap (cache mémoire des logos)
        self._net_epg = QNetworkAccessManager(self)

        # --- Construction de l'interface ---
        self._setup_ui()
        self._build_country_pills()
        self._filter_channels()
        self._find_gamepads()

        # --- Guide TV : chargement initial + horloge de rafraîchissement ---
        self._epg_ready.connect(self._on_epg_parsed)
        self._epg_init()
        self._epg_clock = QTimer(self)
        self._epg_clock.timeout.connect(self._epg_tick)
        self._epg_clock.start(30000)

        # --- Timers ---
        self._pad_timer = QTimer(self)
        self._pad_timer.timeout.connect(self._poll_gamepad)
        self._pad_timer.start(16)

        self._load_anim = QTimer(self)
        self._load_anim.timeout.connect(self._animate_loading)

    # =========================================================================
    # PERSISTANCE
    # =========================================================================

    def _load_settings(self):
        try:
            os.makedirs(CONFIG_DIR, exist_ok=True)
            with open(CONFIG_FILE) as f:
                return json.load(f)
        except (FileNotFoundError, json.JSONDecodeError):
            return {}

    def _save_settings(self):
        os.makedirs(CONFIG_DIR, exist_ok=True)
        with open(CONFIG_FILE, "w") as f:
            json.dump(self._settings, f)

    # =========================================================================
    # CONSTRUCTION DE L'INTERFACE
    # =========================================================================

    def _setup_ui(self):
        """Construit toute l'arborescence de widgets."""
        central = QWidget()
        self.setCentralWidget(central)
        root = QVBoxLayout(central)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        # --- Barre supérieure ---
        self._top_bar = QFrame()
        self._top_bar.setObjectName("topBar")
        self._top_bar.setFixedHeight(54)
        top_layout = QHBoxLayout(self._top_bar)
        top_layout.setContentsMargins(18, 0, 18, 0)
        top_layout.setSpacing(14)

        title = QLabel("GABLUE  TV")
        title.setObjectName("titleLabel")
        top_layout.addWidget(title)

        # --- Bascule Chaînes / Guide TV ---
        self._view_toggle = QPushButton("📅 Guide TV")
        self._view_toggle.setObjectName("viewToggle")
        self._view_toggle.setCursor(Qt.PointingHandCursor)
        self._view_toggle.setFocusPolicy(Qt.NoFocus)
        self._view_toggle.clicked.connect(self._toggle_guide)
        top_layout.addWidget(self._view_toggle)

        # Conteneur scrollable des pastilles
        self._pills_scroll = QScrollArea()
        self._pills_scroll.setFixedHeight(44)
        self._pills_scroll.setWidgetResizable(True)
        self._pills_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        self._pills_scroll.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self._pills_scroll.setFrameShape(QFrame.NoFrame)
        self._pills_scroll.setStyleSheet("QScrollArea { background: transparent; border: none; }")

        pills_widget = QWidget()
        self._pills_layout = QHBoxLayout(pills_widget)
        self._pills_layout.setContentsMargins(0, 0, 0, 0)
        self._pills_layout.setSpacing(8)
        self._pills_layout.addStretch()
        self._pills_scroll.setWidget(pills_widget)
        top_layout.addWidget(self._pills_scroll, 1)

        self._count_label = QLabel()
        self._count_label.setObjectName("countLabel")
        top_layout.addWidget(self._count_label)

        root.addWidget(self._top_bar)

        # --- Stack : alterne entre grille et vidéo intégrée ---
        self._stack = QStackedWidget()
        self._stack.setStyleSheet("background-color: #080818;")

        # --- Zone de contenu (grille) ---
        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.NoFrame)
        self._scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)

        self._grid_container = QWidget()
        self._grid = QGridLayout(self._grid_container)
        self._grid.setSpacing(GRID_SPACING)
        self._grid.setContentsMargins(GRID_MARGIN, GRID_MARGIN, GRID_MARGIN, GRID_MARGIN)
        self._grid.setAlignment(Qt.AlignTop | Qt.AlignHCenter)
        self._scroll.setWidget(self._grid_container)

        self._stack.addWidget(self._scroll)  # index 0 : grille

        # --- Widget vidéo intégré (libmpv + OpenGL) ---
        self._video_widget = MpvWidget()
        self._video_widget._right_click_cb = self._stop_video
        # Suivi souris + filtre d'événements pour le masquage auto du curseur
        self._video_widget.setMouseTracking(True)
        self._video_widget.installEventFilter(self)
        self._stack.addWidget(self._video_widget)  # index 1 : vidéo

        # --- Vue Guide TV (EPG) ---
        guide_page = QWidget()
        gp_layout = QVBoxLayout(guide_page)
        gp_layout.setContentsMargins(16, 10, 16, 8)
        gp_layout.setSpacing(6)

        self._guide_status = QLabel(self._epg_status)
        self._guide_status.setObjectName("guideStatus")
        self._guide_status.setWordWrap(True)
        gp_layout.addWidget(self._guide_status)

        self._guide_scroll = QScrollArea()
        self._guide_scroll.setWidgetResizable(True)
        self._guide_scroll.setFrameShape(QFrame.NoFrame)
        self._guide_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)

        # Contenu à géométrie manuelle : seules les lignes visibles sont
        # construites (virtualisation — ~7000 widgets sinon, transitions
        # lentes). Défilement vertical natif, lignes hors écran détruites.
        self._guide_content = _GuideContent()
        self._guide_content.on_resize = self._guide_fix_widths
        self._guide_scroll.setWidget(self._guide_content)
        self._guide_scroll.verticalScrollBar().valueChanged.connect(
            self._on_guide_scrolled)
        gp_layout.addWidget(self._guide_scroll, 1)

        self._stack.addWidget(guide_page)  # index 2 : guide TV

        root.addWidget(self._stack, 1)

        # --- Barre OSD manette ---
        self._osd_bar = QFrame()
        self._osd_bar.setObjectName("osdBar")
        self._osd_bar.setFixedHeight(30)
        self._osd_bar.hide()
        osd_layout = QHBoxLayout(self._osd_bar)
        osd_layout.setContentsMargins(22, 0, 22, 0)
        self._osd_label = QLabel(
            "[A] Lancer/Pause    [B] Retour    [Select] Plein écran    [L1/R1] Pays    "
            "[D-Pad] Naviguer    [Haut/Bas] Vol.    [Gauche/Droite] Seek"
        )
        self._osd_label.setObjectName("osdLabel")
        osd_layout.addWidget(self._osd_label)
        root.addWidget(self._osd_bar)

        # --- Focus Wayland ---
        self._window_active = True  # suivi fiable via changeEvent

        # --- Overlay de chargement ---
        self._loading_overlay = QWidget(central)
        self._loading_overlay.setObjectName("loadingOverlay")
        self._loading_overlay.hide()
        load_layout = QVBoxLayout(self._loading_overlay)
        load_layout.setAlignment(Qt.AlignCenter)
        load_layout.setSpacing(8)

        self._loading_title = QLabel()
        self._loading_title.setObjectName("loadingTitle")
        self._loading_title.setAlignment(Qt.AlignCenter)
        load_layout.addWidget(self._loading_title)

        self._loading_subtitle = QLabel("Connexion au flux...")
        self._loading_subtitle.setObjectName("loadingSubtitle")
        self._loading_subtitle.setAlignment(Qt.AlignCenter)
        load_layout.addWidget(self._loading_subtitle)

        # L'overlay couvre la vidéo au démarrage : suivi souris + filtre
        # pour réafficher le curseur même pendant le chargement
        self._loading_overlay.setMouseTracking(True)
        self._loading_overlay.installEventFilter(self)

    # =========================================================================
    # VUES : GRILLE / VIDÉO / GUIDE
    # =========================================================================

    def _show_view(self, idx):
        """Bascule de vue centralisée (0 grille, 1 vidéo, 2 guide)."""
        prev = self._stack.currentIndex()
        self._stack.setCurrentIndex(idx)
        self._top_bar.setVisible(idx != 1)
        self._view_toggle.setText("📺 Chaînes" if idx == 2 else "📅 Guide TV")
        if (idx == 2) != (prev == 2):
            self._sync_pills_for_view()
        self._rebuild_visible_page()
        if idx == 0:
            self._update_card_epg()
        elif idx == 2:
            self._guide_ensure_rows()  # largeurs/viewport à jour (resize caché)
        self._update_osd()

    def _rebuild_visible_page(self):
        """Ne reconstruit QUE la page visible si ses données ont changé.

        Les pages cachées sont marquées « sales » et reconstruites au
        moment de leur affichage — plus jamais de grille + guide reconstruits
        ensemble à chaque transition."""
        idx = self._stack.currentIndex()
        if idx == 0 and self._grid_dirty:
            self._rebuild_grid()
        elif idx == 2 and self._guide_dirty:
            self._rebuild_guide()

    def _toggle_guide(self):
        """Bascule grille ↔ guide TV (ignoré pendant la lecture).

        Aucune reconstruction ici : les données sont marquées sales par
        `_filter_channels`/`_apply_epg` et la page visible se reconstruit
        seule dans `_show_view`. Position de défilement conservée."""
        idx = self._stack.currentIndex()
        if idx == 1:
            return
        self._show_view(0 if idx == 2 else 2)
        # guide indisponible (échec réseau) → nouvelle tentative à l'ouverture
        if idx != 2 and self._epg_failed and not self._epg \
                and not self._epg_downloading:
            self._epg_download()

    # =========================================================================
    # FOCUS WAYLAND
    # =========================================================================

    def changeEvent(self, event):
        """Suit l'état d'activation de la fenêtre (fiable sous Wayland)."""
        if event.type() == QEvent.ActivationChange:
            self._window_active = self.isActiveWindow()
        super().changeEvent(event)

    # =========================================================================
    # CURSEUR (masquage automatique en lecture)
    # =========================================================================

    def eventFilter(self, obj, event):
        """Détecte l'activité souris sur la zone vidéo (curseur auto-masqué).

        Filtre installé sur le widget vidéo et l'overlay de chargement :
        tout mouvement/clic pendant la lecture réaffiche le curseur et
        reprogramme son masquage.
        """
        if event.type() in (
            QEvent.MouseMove,
            QEvent.MouseButtonPress,
            QEvent.MouseButtonRelease,
            QEvent.MouseButtonDblClick,
            QEvent.Wheel,
        ):
            self._on_user_activity()
        return super().eventFilter(obj, event)

    # =========================================================================
    # CLAVIER
    # =========================================================================

    def keyPressEvent(self, event):
        """Raccourcis clavier (grille / guide / lecture)."""
        self._on_user_activity()  # une touche pressée réaffiche le curseur
        key = event.key()
        mode = self._stack.currentIndex()  # 0 grille, 1 vidéo, 2 guide
        watching = mode == 1
        in_guide = mode == 2

        if key == Qt.Key_Escape:
            if watching:
                self._stop_video()
            elif in_guide:
                self._show_view(0)
            else:
                self.close()

        elif key == Qt.Key_F11:
            self._toggle_fullscreen()

        elif key == Qt.Key_G:
            self._toggle_guide()

        elif key == Qt.Key_Return or key == Qt.Key_Enter:
            if watching:
                self._pause_toggle()
            elif in_guide:
                self._guide_play_current()
            elif self._buttons and self._cursor < len(self._filtered):
                # Les cartes sont des QFrame (pas de .click()) : lancer
                # directement la chaîne sous le curseur
                _, (ch_id, label, _) = self._filtered[self._cursor]
                self._play(f"{BASE}/{ch_id}{SUFFIX}/{ch_id}.m3u8", label)

        elif key == Qt.Key_Space:
            if watching:
                self._pause_toggle()
            elif in_guide:
                self._guide_play_current()
            elif self._buttons and self._cursor < len(self._filtered):
                _, (ch_id, label, _) = self._filtered[self._cursor]
                self._play(f"{BASE}/{ch_id}{SUFFIX}/{ch_id}.m3u8", label)

        elif key == Qt.Key_Backspace:
            if watching:
                self._stop_video()
            elif in_guide:
                self._show_view(0)

        elif key == Qt.Key_Up:
            if watching:
                self._volume_up()
            elif in_guide:
                self._guide_step("up")
            else:
                new = self._cursor - self._cols
                if new >= 0:
                    self._cursor = new
                    self._update_card_focus()
                    self._ensure_visible()

        elif key == Qt.Key_Down:
            if watching:
                self._volume_down()
            elif in_guide:
                self._guide_step("down")
            else:
                new = self._cursor + self._cols
                if new < len(self._buttons):
                    self._cursor = new
                    self._update_card_focus()
                    self._ensure_visible()

        elif key == Qt.Key_Left:
            if watching:
                self._seek(-10)
            elif in_guide:
                self._guide_step("left")
            elif self._cursor % self._cols > 0:
                self._cursor -= 1
                self._update_card_focus()
                self._ensure_visible()

        elif key == Qt.Key_Right:
            if watching:
                self._seek(10)
            elif in_guide:
                self._guide_step("right")
            elif (self._cursor % self._cols < self._cols - 1
                  and self._cursor + 1 < len(self._buttons)):
                self._cursor += 1
                self._update_card_focus()
                self._ensure_visible()

        elif key == Qt.Key_PageUp:
            self._country_idx = (self._country_idx - 1) % (len(self._countries_current) + 1)
            self._select_country(self._country_idx)

        elif key == Qt.Key_PageDown:
            self._country_idx = (self._country_idx + 1) % (len(self._countries_current) + 1)
            self._select_country(self._country_idx)

        elif key == Qt.Key_Home:
            if in_guide:
                self._guide_set_row(0)
                sb = self._guide_scroll.verticalScrollBar()
                if sb:
                    sb.setValue(0)
            else:
                self._cursor = 0
                self._update_card_focus()
                self._ensure_visible()

        elif key == Qt.Key_End:
            if in_guide:
                self._guide_set_row(len(self._guide_data) - 1)
            else:
                self._cursor = max(0, len(self._buttons) - 1)
                self._update_card_focus()
                self._ensure_visible()

        else:
            super().keyPressEvent(event)

    # =========================================================================
    # REDIMENSIONNEMENT
    # =========================================================================

    def resizeEvent(self, event):
        """Adapte la grille au redimensionnement."""
        super().resizeEvent(event)
        if self._loading_overlay.isVisible():
            self._loading_overlay.setGeometry(self.centralWidget().rect())
        vw = self._scroll.viewport().width()
        if vw > 100:
            new_cols = max(1, (vw - GRID_MARGIN * 2 + GRID_SPACING) // (CARD_W + GRID_SPACING))
            if new_cols != self._cols and self._buttons:
                self._cols = new_cols
                self._grid_dirty = True
                self._rebuild_visible_page()
        # guide visible → largeurs des lignes construites à jour
        if self._stack.currentIndex() == 2:
            self._guide_ensure_rows()

    # =========================================================================
    # FERMETURE
    # =========================================================================

    def closeEvent(self, event):
        """Nettoie les ressources."""
        self._stop_video()
        self._pad_timer.stop()
        self._load_anim.stop()
        self._epg_clock.stop()
        for device in list(self._gamepads.values()):
            try:
                device.close()
            except (OSError, IOError):
                pass
        self._gamepads.clear()
        self._pad_states.clear()
        super().closeEvent(event)
