"""Cycle de vie du guide TV : téléchargement, cache, thread de parsing.

Vue et données séparées : ce module ne fait que piloter réseau + cache et
tenir à jour la ligne « en cours » des cartes.
"""

import os
import sys
import threading
import time

import shiboken6
from PySide6.QtCore import Qt, QUrl
from PySide6.QtNetwork import QNetworkReply, QNetworkRequest

from .constants import CARD_W
from .epg import (
    EPG_CACHE_GZ, EPG_CACHE_TSV, EPG_CHANNEL_MAP, EPG_TTL_S, EPG_URL,
    _read_epg_tsv, _write_epg_tsv, parse_xmltv,
)


class EpgViewMixin:
    """Cycle de vie du guide : téléchargement, cache, titres « en cours »."""
    # =========================================================================
    # EPG : CHARGEMENT, CACHE, CYCLE DE VIE
    # =========================================================================

    def _epg_init(self):
        """Charge le guide au démarrage : cache TSV frais → direct ; sinon
        ancien cache affiché puis mise à jour téléchargée en arrière-plan."""
        progs = []
        if os.path.exists(EPG_CACHE_TSV):
            try:
                progs = _read_epg_tsv(EPG_CACHE_TSV)
            except OSError:
                progs = []
        fresh = (
            os.path.exists(EPG_CACHE_TSV)
            and (time.time() - os.path.getmtime(EPG_CACHE_TSV)) < EPG_TTL_S
        )
        if progs:
            self._apply_epg(progs)
        if fresh:
            return
        self._epg_download()

    def _epg_download(self):
        """Télécharge le fichier XMLTV (asynchrone, GUI thread)."""
        if self._epg_downloading:
            return
        self._epg_downloading = True
        self._epg_failed = False
        self._epg_status = "⏳ Téléchargement du guide…"
        self._refresh_guide_status()
        request = QNetworkRequest(QUrl(EPG_URL))
        request.setTransferTimeout(60000)
        request.setAttribute(
            QNetworkRequest.Attribute.RedirectPolicyAttribute,
            QNetworkRequest.RedirectPolicy.NoLessSafeRedirectPolicy,
        )
        request.setHeader(
            QNetworkRequest.KnownHeaders.UserAgentHeader, "GablueTV/1.0"
        )
        reply = self._net_epg.get(request)
        reply.finished.connect(self._on_epg_downloaded)

    def _on_epg_downloaded(self):
        """Fin du téléchargement XMLTV : cache brut puis parse en thread."""
        self._epg_downloading = False
        reply = self.sender()
        if reply is None:
            return
        reply.deleteLater()
        if reply.error() != QNetworkReply.NetworkError.NoError:
            self._epg_fail(f"Réseau : {reply.errorString()}")
            return
        data = bytes(reply.readAll())
        if len(data) < 1000:
            self._epg_fail(f"Réponse trop courte ({len(data)} o)")
            return
        try:
            tmp = EPG_CACHE_GZ + ".part"
            with open(tmp, "wb") as f:
                f.write(data)
            os.replace(tmp, EPG_CACHE_GZ)
        except OSError:
            pass
        self._epg_status = "⏳ Analyse du guide…"
        self._refresh_guide_status()
        threading.Thread(
            target=self._epg_parse_worker, args=(data,), daemon=True
        ).start()

    def _epg_parse_worker(self, data):
        """Parsing XMLTV hors thread GUI (~2 s sur le fichier complet)."""
        try:
            progs = parse_xmltv(data, EPG_CHANNEL_MAP)
        except Exception as exc:
            print(f"[EPG] erreur de parsing : {exc}", file=sys.stderr)
            progs = []
        self._epg_ready.emit(progs)

    def _on_epg_parsed(self, progs):
        """Réception des programmes parsés (thread GUI) : cache + application."""
        if not progs:
            self._epg_fail("Guide vide ou illisible")
            return
        try:
            tmp = EPG_CACHE_TSV + ".part"
            _write_epg_tsv(tmp, progs)
            os.replace(tmp, EPG_CACHE_TSV)
        except OSError:
            pass
        self._apply_epg(progs)

    def _epg_fail(self, msg):
        """Échec EPG : garde les anciennes données si présentes."""
        if self._epg:
            self._epg_status = f"⚠ {msg} — guide depuis le cache"
        else:
            self._epg_failed = True
            self._epg_status = f"❌ {msg}"
        self._refresh_guide_status()

    def _apply_epg(self, progs):
        """Applique les programmes : index par chaîne, tri, rafraîchissements."""
        mapping = {}
        for (cid, start, stop, title, cat) in progs:
            mapping.setdefault(cid, []).append((start, stop, title, cat))
        for lst in mapping.values():
            lst.sort(key=lambda x: x[0])
        self._epg = mapping
        self._epg_failed = False
        self._epg_status = f"📡 {len(mapping)} chaînes avec guide"
        self._guide_dirty = True
        self._update_card_epg()
        self._refresh_guide_status()
        if self._stack.currentIndex() == 2:
            # pastilles pays + guide reconstruits avec les nouvelles données
            self._sync_pills_for_view(force=True)

    def _current_programme(self, ch_id, now=None):
        """Programme en cours d'une chaîne → (start, stop, titre, catégorie)."""
        entries = self._epg.get(ch_id)
        if not entries:
            return None
        if now is None:
            now = time.time()
        for (start, stop, title, cat) in entries:
            if start <= now < stop:
                return (start, stop, title, cat)
            if start > now:
                break
        return None

    def _update_card_epg(self, now=None):
        """Met à jour la ligne « en cours » des cartes.

        Ne touche qu'aux cartes dont le programme a changé (le remplissage
        complet des 174 cartes coûte ~100 ms, inacceptable à chaque retour
        grille / tick de 30 s)."""
        if now is None:
            now = time.time()
        for ch_id, lbl in self._card_epg_labels.items():
            if not shiboken6.isValid(lbl):
                continue
            prog = self._current_programme(ch_id, now)
            key = (prog[0], prog[1]) if prog else None
            if self._card_epg_state.get(ch_id) == key:
                continue
            self._card_epg_state[ch_id] = key
            if prog:
                fm = lbl.fontMetrics()
                lbl.setText(fm.elidedText(prog[2], Qt.ElideRight, CARD_W - 24))
                nxt = self._next_programme(ch_id, prog[1])
                tip = f"Maintenant : {prog[2]}"
                if nxt:
                    tip += f"\nEnsuite ({time.strftime('%H:%M', time.localtime(nxt[0]))}) : {nxt[2]}"
                lbl.setToolTip(tip)
                lbl.show()
            else:
                lbl.hide()

    def _next_programme(self, ch_id, after):
        """Premier programme commençant après le timestamp donné."""
        for (start, stop, title, cat) in self._epg.get(ch_id, []):
            if start >= after:
                return (start, stop, title, cat)
        return None

    def _epg_tick(self):
        """Horloge EPG (30 s) : cartes, barres de progression, bascule de
        programme dans le guide (reconstruction seulement si visible)."""
        now = time.time()
        if self._guide_next_flip and now >= self._guide_next_flip:
            self._guide_next_flip = 0.0
            self._guide_dirty = True
        idx = self._stack.currentIndex()
        if idx == 0:
            self._update_card_epg(now)
        elif idx == 2:
            if self._guide_dirty:
                self._rebuild_visible_page()
            else:
                # barres des lignes construites seulement
                for w in self._guide_row_widgets.values():
                    for bar, start, stop in w["bars"]:
                        if shiboken6.isValid(bar) and stop > start:
                            bar.setValue(int(1000 * max(0.0, min(1.0, (now - start) / (stop - start)))))
