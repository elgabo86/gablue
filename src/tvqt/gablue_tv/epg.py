"""Guide TV (XMLTV) : mapping des chaînes, parsing SAX et cache disque.

Le parsing est volontairement hors Qt (stdlib seule) : il tourne dans un
thread dédié et ne touche jamais aux widgets.
"""

import gzip
import io
import os
import xml.sax
from datetime import datetime, timezone

from .constants import CONFIG_DIR

# =============================================================================
# GUIDE TV (EPG)
# =============================================================================
# Source identique à l'appli Android : XMLTV francophone (fichier gzippé,
# ~150 Mo décompressés, parse SAX en streaming). Le mapping ci-dessous relie
# les ids XMLTV aux ids de flux de cette appli (base = mapping Android validé,
# complété de CARAC4 et Canal9 vérifiées dans le fichier).

EPG_URL = "https://xmltvfr.fr/xmltv/xmltv.xml.gz"
EPG_CACHE_GZ = os.path.join(CONFIG_DIR, "epg.xml.gz")
EPG_CACHE_TSV = os.path.join(CONFIG_DIR, "epg_parsed.tsv")
EPG_TTL_S = 12 * 3600  # durée de validité du cache parsé

# Correspondances id XMLTV → id appli, format "xmltv:appli" (curatif : repris
# de l'appli Android + CARAC4 et Canal9 vérifiées dans le fichier). Les ids
# étant irréguliers (CStar.fr→d17, Trek.fr→escales, Sat1.de→sateins…), une
# déduction automatique mapperait de mauvaises chaînes : table explicite.
EPG_MAP_RAW = """
    RTSUn.ch:rts1hd RTSDeux.ch:rts2hd SRF1.ch:srf1hd SRF2.ch:srf2hd
    SRFinfo.ch:srfinfo RSI1.ch:rsila1hd RSI2.ch:rsila2hd
    TeleBielingue.ch:telebielingue LemanBleu.ch:lemanbleu LaTele.ch:latele
    RougeTV.ch:rougetv ONETV.ch:onetv LFMTV.ch:lfmtv BLUEZOOM.ch:bluezoomfr
    CanalAlphaJura.ch:canalalphaju CARAC4.ch:tvsuisseplus Canal9.fr:canal9
    8MontBlanc.fr:tv8montblanc TF1.fr:tf1hd France2.fr:france2hd
    France3.fr:france3hd France4.fr:france4hd France5.fr:france5hd
    M6.fr:m6hd Arte.fr:artehd TMC.fr:tmc W9.fr:w9 NT1.fr:nt1 T18.fr:t18
    CStar.fr:d17 RTL9.fr:rtl9 AB1.fr:ab1 AB3.fr:ab3 TF1SeriesFilms.fr:hd1
    6ter.fr:6ter Cherie25.fr:cherie25 NOVO19.fr:novo19 Numero23.fr:numero23
    LEquipe21.fr:lequipe21 Trek.fr:escales RMCDecouverte.fr:rmcdecouverte
    PlanetePlus.fr:planeteplus Animaux.fr:animaux TCM.fr:tcm
    CartoonNetwork.fr:cartoonnetwork Gulli.fr:gulli MTV.fr:mtvfrance
    KTO.fr:kto CanalPlus.fr:canalplusclair CanalJ.fr:canalj BFMTV.fr:bfmtv
    BFMBusiness.fr:bfmbusiness CNews.fr:itele LCI.fr:lci LCP100.fr:lcp
    France24.fr:france24 FranceInfo.fr:franceinfo TV5Monde.fr:tv5mondefbs
    Euronews.fr:euronews Mezzo.fr:mezzo ComedyCentral.fr:comedycentral
    ZDF.de:zdfhd 3SAT.de:3sathd Phoenix.de:phoenixhd KIKA.de:kikahd
    SWR.de:swrbw RTLNitroTV.de:rtlnitro RTL2.de:rtl2 SuperRTL.de:superrtl
    Vox.de:vox DasErste.de:daserstehd KabelEins.de:kabeleins PRO7.de:pro7
    Sat1.de:sateins BBC2.uk:bbc2 BBCNews.uk:bbcworld Channel4.uk:channel4
    SkyNews.us:skynews AlJazeeraEnglish.uk:aljazeera Bloomberg.uk:bloomberg
"""
EPG_CHANNEL_MAP = dict(pair.split(":", 1) for pair in EPG_MAP_RAW.split())


def _parse_xmltv_time(t):
    """Convertit une date XMLTV ('20261003200000 +0200') en timestamp UTC.

    Portage du parseur Android : la date est lue comme UTC puis l'offset du
    fuseau est soustrait ('+0200' = heure locale = UTC-2h)."""
    if not t or len(t) < 14:
        return 0
    try:
        dt = datetime.strptime(t[:14], "%Y%m%d%H%M%S")
    except ValueError:
        return 0
    ts = dt.replace(tzinfo=timezone.utc).timestamp()
    tz = t[14:].strip()
    if tz[:1] in ("+", "-"):
        try:
            h = int(tz[1:3])
            m = int(tz[3:5]) if len(tz) > 3 else 0
        except ValueError:
            return ts
        ts += (-1 if tz[0] == "+" else 1) * (h * 3600 + m * 60)
    return ts


class _EpgSaxHandler(xml.sax.ContentHandler):
    """Extracteur SAX des programmes XMLTV (chaînes mappées uniquement).

    Streaming : mémoire constante même sur le fichier complet (~150 Mo)."""

    def __init__(self, channel_map):
        super().__init__()
        self._map = channel_map
        self.programmes = []
        self._in_prog = False
        self._collect = None   # "title" / "category" / None
        self._buf = []
        self._chan = None
        self._start = 0
        self._stop = 0
        self._title = None
        self._cat = None

    def startElement(self, name, attrs):
        if name == "programme":
            chan = attrs.get("channel")
            self._in_prog = chan in self._map
            if self._in_prog:
                self._chan = chan
                self._start = _parse_xmltv_time(attrs.get("start"))
                self._stop = _parse_xmltv_time(attrs.get("stop"))
                self._title = None
                self._cat = None
                self._collect = None
        elif self._in_prog and name in ("title", "category"):
            self._collect = name
            self._buf = []

    def characters(self, content):
        if self._collect:
            self._buf.append(content)

    def endElement(self, name):
        if self._collect == name:
            text = "".join(self._buf).strip()
            if name == "title":
                self._title = text
            elif self._cat is None:
                self._cat = text or None
            self._collect = None
        elif name == "programme" and self._in_prog:
            self._in_prog = False
            if self._title and self._start > 0 and self._stop > self._start:
                self.programmes.append((
                    self._map[self._chan], self._start, self._stop,
                    self._title, self._cat or "",
                ))


def parse_xmltv(data, channel_map):
    """Parse un document XMLTV (brut ou gzip) → [(app_id, start, stop, titre, catégorie)]."""
    stream = io.BytesIO(data)
    if data[:2] == b"\x1f\x8b":
        stream = gzip.GzipFile(fileobj=stream)
    handler = _EpgSaxHandler(channel_map)
    xml.sax.parse(stream, handler)
    return handler.programmes


def _tsv_clean(s):
    """Neutralise tabulations/retours de ligne pour le cache TSV."""
    return (s or "-").replace("\t", " ").replace("\n", " ").replace("\r", " ")


def _write_epg_tsv(path, progs):
    """Écrit le cache parsé (format identique au cache Android)."""
    with open(path, "w", encoding="utf-8") as f:
        for (cid, start, stop, title, cat) in progs:
            f.write("\t".join((
                cid, str(int(start)), str(int(stop)),
                _tsv_clean(title), _tsv_clean(cat), "-",
            )) + "\n")


def _read_epg_tsv(path):
    """Lit le cache parsé → [(app_id, start, stop, titre, catégorie)]."""
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            cols = line.rstrip("\r\n").split("\t")
            if len(cols) < 6:
                continue
            try:
                start, stop = int(cols[1]), int(cols[2])
            except ValueError:
                continue
            title = cols[3]
            if title and title != "-" and start > 0 and stop > start:
                out.append((cols[0], start, stop, title,
                            "" if cols[4] == "-" else cols[4]))
    return out
