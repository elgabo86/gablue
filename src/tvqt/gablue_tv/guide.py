"""Vue guide TV : lignes virtualisées et navigation D-pad."""

import time

from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QFontMetrics
from PySide6.QtWidgets import (
    QFrame, QLabel, QProgressBar, QVBoxLayout, QWidget,
)

from .constants import (
    BASE, GUIDE_BLOCK_H, GUIDE_BUFFER_ROWS, GUIDE_CHAN_W, GUIDE_MAX_PROGS,
    GUIDE_ROW_H, GUIDE_ROW_PITCH, SUFFIX,
)

# =============================================================================
# ZONE DE PROGRAMMES D'UNE LIGNE DU GUIDE
# =============================================================================

class _GuideContent(QWidget):
    """Conteneur des lignes du guide (géométrie manuelle, virtualisées).

    Prévient la fenêtre quand sa largeur change (apparition de la barre
    de défilement verticale, redimensionnement) pour recaler les lignes
    déjà construites."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.on_resize = None

    def resizeEvent(self, event):
        super().resizeEvent(event)
        if self.on_resize:
            self.on_resize()


class GuideMixin:
    """Guide TV virtualisé : seules les lignes visibles sont construites."""
    def _guide_play_current(self):
        """Lance la chaîne de la ligne courante du guide.

        Le bloc sélectionné (en cours ou à venir) ne change rien : une
        chaîne live diffuse toujours son programme présent."""
        if not self._guide_data or self._guide_row >= len(self._guide_data):
            return
        row = self._guide_data[self._guide_row]
        url = f"{BASE}/{row['ch_id']}{SUFFIX}/{row['ch_id']}.m3u8"
        self._play(url, row["label"])

    # =========================================================================
    # GUIDE TV (construction + navigation)
    # =========================================================================

    def _rebuild_guide(self):
        """Recalcule les métadonnées du guide (lignes/programmes) puis ne
        construit que les lignes visibles (virtualisation).

        La ligne sélectionnée est restaurée par ch_id et le bloc sélectionné
        par position — le focus reste sur le programme choisi même s'il
        n'est pas celui en cours."""
        prev_ch = prev_slot = None
        if 0 <= self._guide_row < len(self._guide_data):
            prev_ch = self._guide_data[self._guide_row]["ch_id"]
            prev_slot = self._guide_slot

        self._dispose_all_guide_rows()
        self._guide_data = []
        self._guide_next_flip = 0.0

        now = time.time()
        for (i, (ch_id, label, _c)) in self._filtered:
            entries = self._epg.get(ch_id)
            if not entries:
                continue
            # premier programme non terminé
            start_idx = 0
            for j, (s, e, _t, _c2) in enumerate(entries):
                if e > now:
                    start_idx = j
                    break
            shown = entries[start_idx:start_idx + GUIDE_MAX_PROGS]
            if not shown:
                continue
            # prochain changement de programme (reconstruction du guide)
            flip = shown[0][0] if shown[0][0] > now else shown[0][1]
            self._guide_next_flip = (
                min(self._guide_next_flip, flip) if self._guide_next_flip else flip
            )
            self._guide_data.append({
                "ch_id": ch_id, "label": label, "progs": shown,
            })

        n = len(self._guide_data)
        self._guide_content.setMinimumHeight(max(1, n) * GUIDE_ROW_PITCH + 8)

        # restaure la sélection (ligne par ch_id, bloc par position)
        target = 0
        if prev_ch:
            for i, d in enumerate(self._guide_data):
                if d["ch_id"] == prev_ch:
                    target = i
                    break
        self._guide_row = target
        if n:
            slot = prev_slot if prev_slot is not None else 0
            self._guide_slot = max(
                0, min(slot, len(self._guide_data[target]["progs"]) - 1))

        self._guide_dirty = False
        self._guide_ensure_rows()
        if n:
            self._guide_reveal_row(self._guide_row)
        self._refresh_guide_status()

    def _on_guide_scrolled(self, _value):
        """Défilement vertical du guide → construit les lignes entrantes."""
        self._guide_ensure_rows()

    def _guide_ensure_rows(self):
        """Construit les lignes visibles (+ marge), détruit les lointaines.

        Le guide complet ferait ~90 widgets par chaîne × 79 : seules les
        lignes dans la fenêtre de visualisation existent réellement."""
        if self._guide_dirty or not self._guide_data:
            return
        n = len(self._guide_data)
        sb = self._guide_scroll.verticalScrollBar()
        view_h = self._guide_scroll.viewport().height()
        i0 = max(0, int(sb.value() // GUIDE_ROW_PITCH) - GUIDE_BUFFER_ROWS)
        i1 = min(n - 1, int((sb.value() + view_h) // GUIDE_ROW_PITCH)
                 + GUIDE_BUFFER_ROWS)
        for idx in [i for i in self._guide_row_widgets if i < i0 or i > i1]:
            self._dispose_guide_row(idx)
        for idx in range(i0, i1 + 1):
            if idx not in self._guide_row_widgets:
                self._build_guide_row(idx)
        self._guide_fix_widths()
        self._guide_apply_row_highlight()
        self._guide_apply_slot()

    def _guide_fix_widths(self):
        """Recale la largeur des lignes construites sur le contenu."""
        cw = self._guide_content.width()
        for w in self._guide_row_widgets.values():
            if w["frame"].width() != cw:
                self._guide_resize_row(w, cw)

    def _dispose_guide_row(self, idx):
        """Détruit les widgets d'une ligne hors écran (les données restent)."""
        w = self._guide_row_widgets.pop(idx, None)
        if w:
            w["frame"].hide()
            w["frame"].deleteLater()

    def _dispose_all_guide_rows(self):
        for idx in list(self._guide_row_widgets):
            self._dispose_guide_row(idx)

    def _build_guide_row(self, idx):
        """Construit les widgets d'une ligne du guide à géométrie manuelle."""
        d = self._guide_data[idx]
        now = time.time()
        cw = max(200, self._guide_content.width())

        frame = QFrame(self._guide_content)
        frame.setObjectName("guideRow")
        frame.setFixedHeight(GUIDE_ROW_H)
        frame.move(0, idx * GUIDE_ROW_PITCH)
        frame.resize(cw, GUIDE_ROW_H)
        # enfant créé après l'affichage du contenu → show() explicite requis
        # (sans lui, la ligne existe mais reste cachée)
        frame.show()

        # --- Cellule chaîne (cliquable) ---
        cell = QWidget(frame)
        cell.setGeometry(8, 6, GUIDE_CHAN_W, GUIDE_ROW_H - 12)
        logo_lbl = QLabel(cell)
        logo_lbl.setFixedSize(56, 38)
        logo_lbl.setAlignment(Qt.AlignCenter)
        logo_lbl.move((GUIDE_CHAN_W - 56) // 2, 2)
        logo_lbl.setPixmap(self._fallback_icon.pixmap(QSize(56, 38)))
        self._load_logo(d["ch_id"], logo_lbl, QSize(56, 38))
        logo_lbl.show()
        name_lbl = QLabel(QFontMetrics(self.font()).elidedText(
            d["label"], Qt.ElideRight, GUIDE_CHAN_W - 8))
        name_lbl.setObjectName("guideChanName")
        name_lbl.setAlignment(Qt.AlignCenter)
        name_lbl.setToolTip(d["label"])
        name_lbl.setGeometry(2, 42, GUIDE_CHAN_W - 4, 24)
        name_lbl.show()

        def on_cell(event=None, cid=d["ch_id"], lab=d["label"]):
            if event is None or event.button() == Qt.LeftButton:
                self._play(f"{BASE}/{cid}{SUFFIX}/{cid}.m3u8", lab)

        cell.mousePressEvent = on_cell
        cell.show()

        # --- Programmes : widget de clipping + blocs positionnés à la main
        # (remplace la QScrollArea par ligne : 3× moins de widgets) ---
        clip = QWidget(frame)
        clip.setGeometry(GUIDE_CHAN_W + 18, 6,
                         max(50, cw - GUIDE_CHAN_W - 26), GUIDE_ROW_H - 12)
        clip.show()

        blocks, xs, widths, bars = [], [], [], []
        x = 2
        for (s, e, title, cat) in d["progs"]:
            block, bar = self._make_prog_block(
                s, e, title, cat, now, d["ch_id"], d["label"])
            block.setParent(clip)
            block.move(x, 0)
            block.show()
            blocks.append(block)
            xs.append(x)
            widths.append(block.width())
            if bar:
                bars.append((bar, s, e))
            x += block.width() + 6

        w = {
            "frame": frame, "cell": cell, "clip": clip, "blocks": blocks,
            "xs": xs, "widths": widths, "total_w": x - 6, "hoffset": 0,
            "bars": bars,
        }

        # Molette : verticale = défile le guide en suivant la sélection (le
        # focus passe à la première ligne visible, sur son programme en
        # cours) ; horizontale (ou Maj+molette) = décale les blocs
        def on_wheel(event, row=w):
            delta = event.angleDelta()
            if event.modifiers() & Qt.ShiftModifier:
                h = delta.y() or delta.x()
            elif delta.y() == 0 and delta.x() != 0:
                h = delta.x()
            else:
                sb = self._guide_scroll.verticalScrollBar()
                sb.setValue(max(0, min(sb.maximum(), sb.value() - delta.y())))
                self._guide_follow_viewport()
                event.accept()
                return
            vis = row["clip"].width()
            max_off = max(0, row["total_w"] - vis)
            self._guide_set_offset(row, max(0, min(max_off, row["hoffset"] - h)))
            event.accept()

        frame.wheelEvent = on_wheel
        self._guide_row_widgets[idx] = w

    def _guide_set_offset(self, row, offset):
        """Décale horizontalement les blocs d'une ligne (défilement manuel)."""
        row["hoffset"] = offset
        for i, b in enumerate(row["blocks"]):
            b.move(row["xs"][i] - offset, 0)

    def _guide_resize_row(self, w, cw):
        """Met à jour la largeur d'une ligne construite (fenêtre redim.)."""
        w["frame"].resize(cw, GUIDE_ROW_H)
        clip_w = max(50, cw - GUIDE_CHAN_W - 26)
        w["clip"].resize(clip_w, GUIDE_ROW_H - 12)
        self._guide_set_offset(
            w, min(w["hoffset"], max(0, w["total_w"] - clip_w)))

    def _guide_reveal_row(self, idx):
        """Scrolle juste assez pour rendre la ligne sélectionnée visible."""
        sb = self._guide_scroll.verticalScrollBar()
        y = idx * GUIDE_ROW_PITCH
        page = self._guide_scroll.viewport().height()
        if y < sb.value():
            sb.setValue(y)
        elif y + GUIDE_ROW_H > sb.value() + page:
            sb.setValue(y + GUIDE_ROW_H - page)

    def _guide_follow_viewport(self):
        """Molette verticale : la sélection suit la première ligne visible
        et revient à son programme en cours (comme un D-pad haut/bas)."""
        if self._guide_dirty or not self._guide_data:
            return
        sb = self._guide_scroll.verticalScrollBar()
        idx = min(len(self._guide_data) - 1,
                  max(0, int((sb.value() + 4) // GUIDE_ROW_PITCH)))
        if idx != self._guide_row:
            self._guide_clear_sel(self._guide_row)
            self._guide_row = idx
            self._guide_slot = 0
            self._guide_ensure_rows()
            self._guide_apply_row_highlight()
            self._guide_apply_slot()

    def _make_prog_block(self, s, e, title, cat, now, ch_id, label):
        """Bloc programme : horaires, titre, catégorie, progression (en cours).

        Retourne (bloc, barre_de_progression|None). Le clic lance la chaîne
        quel que soit le programme (en cours ou à venir)."""
        current = s <= now < e
        block = QFrame()
        block.setObjectName("progBlock")
        if current:
            block.setProperty("now", True)
        minutes = max(1, (e - s) // 60)
        block.setFixedSize(max(116, min(300, minutes * 4)), GUIDE_BLOCK_H)

        v = QVBoxLayout(block)
        v.setContentsMargins(8, 6, 8, 6)
        v.setSpacing(2)

        t_lbl = QLabel(
            f"{time.strftime('%H:%M', time.localtime(s))} – "
            f"{time.strftime('%H:%M', time.localtime(e))}"
        )
        t_lbl.setObjectName("progTime")
        v.addWidget(t_lbl)

        title_lbl = QLabel(title)
        title_lbl.setObjectName("progTitle")
        title_lbl.setWordWrap(True)
        title_lbl.setToolTip(title)
        v.addWidget(title_lbl, 1)

        if cat:
            cat_lbl = QLabel(cat)
            cat_lbl.setObjectName("progCat")
            v.addWidget(cat_lbl)

        bar = None
        if current:
            bar = QProgressBar()
            bar.setObjectName("progBar")
            bar.setTextVisible(False)
            bar.setRange(0, 1000)
            bar.setValue(int(1000 * max(0.0, min(1.0, (now - s) / (e - s)))))
            v.addWidget(bar)

        def on_click(event=None, cid=ch_id, lab=label):
            if event is None or event.button() == Qt.LeftButton:
                self._play(f"{BASE}/{cid}{SUFFIX}/{cid}.m3u8", lab)

        block.mousePressEvent = on_click
        return block, bar

    def _guide_apply_row_highlight(self):
        """Met à jour la surbrillance des lignes construites.

        Ne re-polit que les lignes dont l'état change (le polish QSS est
        coûteux : 11 lignes × 2 à chaque défilement sinon)."""
        for idx, w in self._guide_row_widgets.items():
            active = (idx == self._guide_row)
            if w["frame"].property("active") != active:
                w["frame"].setProperty("active", active)
                w["frame"].style().unpolish(w["frame"])
                w["frame"].style().polish(w["frame"])

    def _guide_apply_slot(self):
        """Met à jour le bloc sélectionné + décale la ligne pour le garder
        visible (défilement horizontal manuel). Ne re-polit que les blocs
        dont la sélection change."""
        w = self._guide_row_widgets.get(self._guide_row)
        if w is None:
            return
        for j, b in enumerate(w["blocks"]):
            sel = (j == self._guide_slot)
            if b.property("sel") != sel:
                b.setProperty("sel", sel)
                b.style().unpolish(b)
                b.style().polish(b)
        if 0 <= self._guide_slot < len(w["blocks"]):
            x0 = w["xs"][self._guide_slot]
            x1 = x0 + w["widths"][self._guide_slot]
            vis = w["clip"].width()
            off = w["hoffset"]
            if x0 < off:              # bloc à gauche du viewport → revenir
                off = max(0, x0 - 8)
            elif x1 > off + vis:      # bloc à droite → avancer
                off = x1 - vis
            off = max(0, min(off, max(0, w["total_w"] - vis)))
            if off != w["hoffset"]:
                self._guide_set_offset(w, off)

    def _guide_set_row(self, idx):
        """Change de ligne de guide (construit au besoin + défilement).

        Le focus revient au programme en cours de la nouvelle ligne :
        monter/descendre ne doit pas conserver le décalage horizontal
        (gauche/droite, lui, le conserve tant qu'on reste sur la ligne).
        La ligne quittée perd son cadre jaune et revient à son début."""
        if not self._guide_data:
            return
        idx = max(0, min(idx, len(self._guide_data) - 1))
        if idx != self._guide_row:
            self._guide_clear_sel(self._guide_row)
        self._guide_row = idx
        self._guide_slot = 0
        self._guide_ensure_rows()
        self._guide_apply_row_highlight()
        self._guide_apply_slot()
        self._guide_reveal_row(idx)

    def _guide_clear_sel(self, idx):
        """Ligne quittée : retire le cadre jaune du bloc sélectionné et
        redécale les blocs au début (programme en cours visible)."""
        w = self._guide_row_widgets.get(idx)
        if w is None:
            return
        for b in w["blocks"]:
            if b.property("sel"):
                b.setProperty("sel", False)
                b.style().unpolish(b)
                b.style().polish(b)
        if w["hoffset"]:
            self._guide_set_offset(w, 0)

    def _guide_step(self, direction):
        """Un pas de navigation D-pad dans le guide."""
        if not self._guide_data:
            return
        if direction == "up":
            if self._guide_row > 0:
                self._guide_set_row(self._guide_row - 1)
        elif direction == "down":
            if self._guide_row + 1 < len(self._guide_data):
                self._guide_set_row(self._guide_row + 1)
        elif direction == "left":
            if self._guide_slot > 0:
                self._guide_slot -= 1
                self._guide_apply_slot()
        elif direction == "right":
            progs = self._guide_data[self._guide_row]["progs"]
            if self._guide_slot + 1 < len(progs):
                self._guide_slot += 1
                self._guide_apply_slot()

    def _refresh_guide_status(self):
        """Met à jour la ligne de statut du guide (téléchargement/erreur/compte)."""
        if not hasattr(self, "_guide_status"):
            return
        total = len(self._filtered)
        with_epg = sum(1 for (_i, (cid, _l, _c)) in self._filtered if cid in self._epg)
        suffix = f"   ·   {with_epg}/{total} chaînes filtrées avec guide"
        self._guide_status.setText(self._epg_status + suffix)
        self._guide_status.setProperty("failed", self._epg_failed)
        self._guide_status.style().unpolish(self._guide_status)
        self._guide_status.style().polish(self._guide_status)
