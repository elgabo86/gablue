"""Vue grille : pastilles de pays et cartes de chaînes."""

from PySide6.QtCore import QSize, Qt
from PySide6.QtWidgets import (
    QFrame, QGridLayout, QLabel, QPushButton, QVBoxLayout,
)

from .channels import CHANNELS, COUNTRIES_ORDER
from .constants import BASE, CARD_H, CARD_W, GRID_MARGIN, GRID_SPACING, SUFFIX

LOGO_ICON_SIZE = QSize(84, 84)


class GridMixin:
    """Grille de chaînes et pastilles de filtrage par pays."""
    def _build_country_pills(self, countries=None):
        """Crée les pastilles de filtrage par pays.

        La liste varie selon la vue : en guide, seuls les pays ayant au
        moins une chaîne guidée sont proposés (`_sync_pills_for_view`)."""
        if countries is None:
            countries = COUNTRIES_ORDER
        self._countries_current = list(countries)

        # Nettoie tout sauf le stretch final
        while self._pills_layout.count() > 1:
            item = self._pills_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

        self._country_pills = []

        # Pastille "Toutes"
        pill_all = QPushButton("Toutes")
        pill_all.setObjectName("countryPill")
        pill_all.setCursor(Qt.PointingHandCursor)
        pill_all.setFocusPolicy(Qt.NoFocus)
        pill_all.clicked.connect(lambda: self._select_country(0))
        self._pills_layout.insertWidget(0, pill_all)
        self._country_pills.append(pill_all)

        for i, country in enumerate(countries):
            pill = QPushButton(country)
            pill.setObjectName("countryPill")
            pill.setCursor(Qt.PointingHandCursor)
            pill.setFocusPolicy(Qt.NoFocus)
            idx = i + 1
            pill.clicked.connect(lambda checked, n=idx: self._select_country(n))
            self._pills_layout.insertWidget(self._pills_layout.count() - 1, pill)
            self._country_pills.append(pill)

    def _select_country(self, idx):
        """Change le filtre pays (index dans la liste des pastilles actives)."""
        self._country_idx = idx
        self._filter_channels()

    def _guide_countries(self):
        """Pays proposés en vue guide : ceux avec au moins une chaîne EPG."""
        if not self._epg:
            return []
        return [c for c in COUNTRIES_ORDER
                if any(ch[0] in self._epg for ch in CHANNELS if ch[2] == c)]

    def _sync_pills_for_view(self, force=False):
        """Adapte les pastilles pays à la vue courante.

        En guide, seuls les pays avec au moins une chaîne guidée sont
        montrés. Le pays sélectionné est conservé quand c'est possible ;
        une sélection de grille sans guide est mise de côté et restituée
        au retour en grille."""
        idx = self._stack.currentIndex()
        countries = self._guide_countries() if (idx == 2 and self._epg) else COUNTRIES_ORDER
        if countries == self._countries_current and not force:
            return

        current = (self._countries_current[self._country_idx - 1]
                   if 0 < self._country_idx <= len(self._countries_current) else None)
        self._build_country_pills(countries)

        if idx == 2:
            if current and current in countries:
                self._country_idx = countries.index(current) + 1
            else:
                self._guide_country_stash = current
                self._country_idx = 0
        else:
            if current and current in COUNTRIES_ORDER:
                self._country_idx = COUNTRIES_ORDER.index(current) + 1
            elif self._guide_country_stash in COUNTRIES_ORDER:
                self._country_idx = COUNTRIES_ORDER.index(self._guide_country_stash) + 1
            else:
                self._country_idx = 0
            self._guide_country_stash = None

        # Ne refiltre que si le filtre effectif change : d'un aller-retour
        # grille ↔ guide, la sélection (souvent « Toutes ») ne bouge pas et
        # la grille ne doit PAS être reconstruite (110 ms à chaque retour).
        new_country = (None if self._country_idx == 0
                       else self._countries_current[self._country_idx - 1])
        if new_country != self._active_country or force:
            self._filter_channels()
        else:
            self._refresh_pills()

    def _refresh_pills(self):
        """Met à jour le style actif/inactif des pastilles."""
        target_pill = (
            self._country_pills[self._country_idx]
            if self._country_idx < len(self._country_pills)
            else None
        )
        for i, pill in enumerate(self._country_pills):
            active = (i == self._country_idx)
            pill.setProperty("active", active)
            pill.style().unpolish(pill)
            pill.style().polish(pill)
        # Scroll horizontal pour rendre visible la pastille active
        # (l'utilisateur peut aussi scroller manuellement avec la souris)
        if target_pill:
            self._pills_scroll.ensureWidgetVisible(target_pill, 8, 4)

    # =========================================================================
    # GRILLE DE CHAÎNES
    # =========================================================================

    def _filter_channels(self):
        """Applique le filtre pays et reconstruit la grille."""
        if self._country_idx == 0:
            self._filtered = list(enumerate(CHANNELS))
            self._active_country = None
        else:
            country = self._countries_current[self._country_idx - 1]
            self._filtered = [
                (i, ch) for i, ch in enumerate(CHANNELS) if ch[2] == country
            ]
            self._active_country = country

        self._count_label.setText(f"{len(self._filtered)} chaînes")
        self._cursor = 0
        self._grid_dirty = True
        self._guide_dirty = True
        self._rebuild_visible_page()
        self._refresh_pills()

    def _rebuild_grid(self):
        """Reconstruit la grille de cartes."""
        # Nettoyage
        while self._grid.count():
            item = self._grid.takeAt(0)
            if item.widget():
                item.widget().setParent(None)

        self._buttons = []
        self._card_epg_labels = {}
        self._card_epg_state = {}

        # Calcul du nombre de colonnes
        vw = self._scroll.viewport().width()
        if vw < 100:
            vw = self.width() - 24
        self._cols = max(1, (vw - GRID_MARGIN * 2 + GRID_SPACING) // (CARD_W + GRID_SPACING))

        for idx, (_, (ch_id, label, country)) in enumerate(self._filtered):
            card = self._make_card(ch_id, label, country)
            row = idx // self._cols
            col = idx % self._cols
            self._grid.addWidget(card, row, col, Qt.AlignCenter)
            self._buttons.append(card)

        # Titres « en cours » (une seule passe, pas dans la boucle)
        self._update_card_epg()

        # Espaceur en bas
        if self._buttons:
            last_row = (len(self._buttons) - 1) // self._cols
            self._grid.setRowStretch(last_row + 1, 1)

        self._update_card_focus()
        self._grid_dirty = False

    def _make_card(self, ch_id, label, country):
        """Carte chaîne : logo, nom et programme « en cours » (si guide)."""
        card = QFrame()
        card.setObjectName("channelCard")
        card.setFixedSize(CARD_W, CARD_H)
        card.setCursor(Qt.PointingHandCursor)

        layout = QVBoxLayout(card)
        layout.setContentsMargins(4, 8, 4, 4)
        layout.setSpacing(2)
        layout.setAlignment(Qt.AlignCenter)

        icon_lbl = QLabel()
        icon_lbl.setFixedSize(84, 84)
        icon_lbl.setAlignment(Qt.AlignCenter)
        icon_lbl.setPixmap(self._fallback_icon.pixmap(LOGO_ICON_SIZE))
        layout.addWidget(icon_lbl, 0, Qt.AlignCenter)
        self._load_logo(ch_id, icon_lbl, LOGO_ICON_SIZE)

        text_lbl = QLabel(label)
        text_lbl.setAlignment(Qt.AlignCenter)
        text_lbl.setWordWrap(True)
        text_lbl.setStyleSheet("color: #c8c8e0; font-size: 12px; font-weight: 600; background: transparent;")
        layout.addWidget(text_lbl, 0, Qt.AlignCenter)

        # Programme en cours (rempli quand le guide est disponible)
        epg_lbl = QLabel()
        epg_lbl.setObjectName("epgNow")
        epg_lbl.setAlignment(Qt.AlignCenter)
        epg_lbl.setFixedHeight(16)
        epg_lbl.hide()
        layout.addWidget(epg_lbl, 0, Qt.AlignCenter)
        self._card_epg_labels[ch_id] = epg_lbl

        card.setToolTip(f"{label}\n{country}")

        def on_click(event=None):
            url = f"{BASE}/{ch_id}{SUFFIX}/{ch_id}.m3u8"
            self._play(url, label)

        card.mousePressEvent = lambda e: on_click() if e.button() == Qt.LeftButton else None
        return card

    def _update_card_focus(self):
        """Met à jour visuellement la carte active."""
        for i, btn in enumerate(self._buttons):
            active = (i == self._cursor)
            btn.setProperty("active", active)
            btn.style().unpolish(btn)
            btn.style().polish(btn)

    def _ensure_visible(self):
        """Scrolle pour rendre visible la carte active."""
        if not self._buttons or self._cursor >= len(self._buttons):
            return
        btn = self._buttons[self._cursor]
        self._scroll.ensureWidgetVisible(btn, 80, 80)

    def _scroll_to_top(self):
        """Remonte tout en haut de la grille."""
        sb = self._scroll.verticalScrollBar()
        if sb:
            sb.setValue(0)
