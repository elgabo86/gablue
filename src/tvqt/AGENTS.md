# Gablue TV — paquet `gablue_tv` (src/tvqt)

TV Gablue en Python (PySide6 + libmpv) : chaînes HLS suisses/européennes,
navigation manette D-pad, lecteur intégré (libmpv + `QOpenGLWidget`), guide TV
XMLTV virtualisé, logos en cache, filtre par pays. Nom affiché « Gablue TV » ;
binaire et `.desktop` = `tvqt` / `gablue-tv.desktop` (le nom du binaire ne
change pas : menu, ES-DE « Gablue TV », `killthemall`).

## Paquet

Sources = ce dossier (`src/tvqt/`), installées par
`files/scripts/build-tvqt` :

| Fichier | Rôle |
|---|---|
| `tvqt` | lanceur `/usr/bin/tvqt` : ajoute `src/tvqt` (développement) ou `/usr/lib/tvqt` au `sys.path` puis `gablue_tv.app.main()` |
| `gablue_tv/app.py` | `main()` : format OpenGL 3.3 core (compatibilité mpv), QApplication, police TV, QSS, fenêtre, `--fullscreen`/`-f` |
| `gablue_tv/constants.py` | flux (`BASE`, `SUFFIX`), chemins de cache `~/.cache/gablue-tv`, géométrie et durées |
| `gablue_tv/channels.py` | `CHANNELS` (id, nom, pays) et `COUNTRIES_ORDER` (ordre d'apparition) |
| `gablue_tv/epg.py` | données XMLTV : mapping statique, parsing SAX streaming, cache TSV |
| `gablue_tv/style.py` | `APP_STYLESHEET` (QSS) |
| `gablue_tv/mpv.py` | chargement de libmpv, détection de VM, `MpvWidget` (`mpv_render_context` OpenGL) |
| `gablue_tv/grid.py` | `GridMixin` : pastilles de pays, cartes, filtre, focus |
| `gablue_tv/guide.py` | `GuideMixin` + `_GuideContent` : lignes virtualisées, blocs programme, navigation |
| `gablue_tv/epgview.py` | `EpgViewMixin` : téléchargement/cache/thread de parsing, titres « en cours », tick 30 s |
| `gablue_tv/logos.py` | `LogosMixin` : cache disque + mémoire, téléchargement dédupliqué |
| `gablue_tv/video.py` | `VideoMixin` : lecture, overlay de chargement, inhibition de veille, curseur |
| `gablue_tv/input.py` | `InputMixin` : evdev multi-manettes, accélération D-pad, actions |
| `gablue_tv/window.py` | `TVWindow` = mixins + `QMainWindow` : `__init__`, interface, vues, focus, clavier, fermeture |

**Règle** : le paquet est la source de vérité ; ne pas réintroduire de fichier
unique. Une nouvelle vue → un module mixin ajouté aux bases de `TVWindow`. Les
imports vont dans un seul sens (`constants/channels/epg/style/mpv` ←
`grid/guide/epgview/logos/video/input` ← `window` ← `app`). Les mixins sont de
simples classes (pas de `QObject`) : seul `TVWindow` porte le `QObject` et le
signal `_epg_ready` (reçu en `QueuedConnection` depuis le thread de parsing).

## Build

- `Containerfile-gablue` : `COPY src/tvqt /src/tvqt`, puis
  `RUN /ctx/build-tvqt` juste après `build-manager`.
- `files/scripts/build-tvqt` : copie `gablue_tv/` dans `/usr/lib/tvqt/`,
  installe le lanceur en `/usr/bin/tvqt` (755) et retire les `__pycache__`
  (le Python du build n'est pas celui du poste, `/usr` est en lecture seule
  sur composefs).
- Développement : `src/tvqt/tvqt` marche depuis le dépôt ;
  `PYTHONPATH=src/tvqt python3 -c "import gablue_tv.app"`.
- L'image doit être reconstruite pour valider l'installation (pas l'ISO).

## Comportement

- **Multi-manettes** (`input.py`) : TOUTES les manettes connectées pilotent
  l'app (dict devices + état par device, fronts boutons par device, direction
  fusionnée up>down>left>right) ; re-scan incrémental 2 s (hotplug,
  renumérotation des nodes après reconnexion BT), device retiré sur erreur
  lecture. Axes normalisés par device via `absinfo` — les pads virtuels
  ds2xbox (-32768..32767, repos 0) étaient lus collés haut/gauche (volume qui
  montait en boucle en lecture). Entrée/Espace clavier = lecture directe de la
  chaîne sous le curseur (cartes = QFrame, pas de `.click()`).
- **Focus Wayland** : evdev lit les manettes même sans focus → suivi
  activation via `changeEvent(QEvent.ActivationChange)` (fiable sous Wayland :
  événements du compositor KWin). Fenêtre active → manettes OK ; autre app au
  premier plan → ignorées.
- **Lecteur intégré** (`mpv.py`/`video.py`, refonte 2026) : `libmpv` +
  `QOpenGLWidget` (API `mpv_render_context`) — **Python 3.14 : `c_void_p`
  retourne un int → wrapper explicite `c_void_p(handle)` après `mpv_create()`,
  sinon segfault (handle passé en 32-bit)**. Plus de fenêtre/sous-process mpv
  externe. [A] lance/stop, [B] grille, D-pad = volume/seek ; fullscreen auto au
  lancement d'une chaîne (double-clic = toggle, clic droit = grille) ; GUI
  masquée en vidéo (barre + OSD cachés) ; curseur auto-masqué après 2,5 s
  (`CURSOR_HIDE_MS`, tout mouvement le réaffiche, restauré à la grille).
  En VM, décodage logiciel forcé (`hwdec=no`, `LIBGL_ALWAYS_SOFTWARE=1`,
  `llvmpipe`) : virgl fuit les descripteurs de fences en rendu GL soutenu et
  casse le contexte au bout de ~15 s ; les variables sont posées à l'import de
  `mpv.py`, avant tout contexte GL.
- **Guide TV — données** (`epg.py`/`epgview.py`, 10/2026) : source xmltvfr.fr
  (`xmltv.xml.gz` ~150 Mo décompressés) parse SAX streaming ~2 s, thread +
  signal `_epg_ready` (jamais de parse sur le thread GUI). Mapping **statique
  compact** `EPG_MAP_RAW` (80 paires `xmltv:appli`, repris de l'appli Android +
  CARAC4/Canal9 vérifiées) : ids netplus irréguliers (`CStar.fr`→`d17`,
  `Trek.fr`→`escales`…) → auto-match refusé (faux positifs mesurés :
  `Sat1.de`→`sat1gold`, `RTLNitroTV.de`→`rtl`, `BLUEZOOM.ch`→`bluezoomde`) ;
  ~75 % des paires seraient dérivables mais choix user = statique (scan des
  nouvelles chaînes à la demande). Caches `~/.cache/gablue-tv/` : `epg.xml.gz`
  (brut, écriture atomique .part+rename) + `epg_parsed.tsv` (TTL 12 h, format
  identique au cache Android → reprise instantanée + maj en fond). Titre
  « en cours » sous les cartes (cyan `#00bcd4`, elidé, tooltip now/next) : maj
  **incrémentale** via `_card_epg_state` (seules les cartes dont le programme a
  changé — l'appel était dans la boucle de `_rebuild_grid` → O(n²), corrigé).
  Chaînes sans id xmltvfr (Italie, Autriche, une partie DE/UK/Est) = pas de
  guide, compteur `N/M` dans le statut.
- **Guide TV — rendu virtualisé** (`guide.py`, 10/2026) : le guide complet =
  ~90 widgets/chaîne × 79 ≈ 7000 widgets → transitions 1,2 s (« rien ne se
  passe » au [Start]). Depuis : **virtualisation** — métadonnées
  (`_guide_data`) séparées des widgets (`_guide_row_widgets`), seules les
  lignes visibles ±4 (`GUIDE_BUFFER_ROWS`) sont construites, géométrie manuelle
  (plus de QScrollArea par ligne : clip + blocs repositionnés par
  `_guide_set_offset`, molette horizontale/Maj = décale les blocs, molette
  verticale = défile + suit la sélection) → ouverture ~35 ms, 11 lignes
  construites sur 79. **Dirty flags** `_grid_dirty`/`_guide_dirty` +
  `_rebuild_visible_page` : une seule page reconstruite, jamais au toggle si
  rien n'a changé (mêmes objets carte préservés) ; `_sync_pills_for_view` ne
  refiltre que si le pays effectif change (`_active_country`). Flip de
  programme : rebuild seulement si le guide est visible, ligne restaurée par
  ch_id **et bloc par position** (le focus reste sur le programme choisi).
  Logos multi-cibles + **cache mémoire** `_logo_pixmem` (plus de relecture PNG
  à chaque rebuild) ; polish QSS seulement si la propriété change. Navigation :
  [A]/Entrée/clic = lancer la chaîne depuis **n'importe quel bloc** (en cours
  ou à venir — une live diffuse toujours son programme présent), [B] = retour à
  la vue d'origine (`_return_view` : lancé du guide → retour au guide, position
  + bloc conservés, `_epg_tick` au retour), D-pad via `_nav_step` (accélération
  conservée ; haut/bas → programme en cours de la nouvelle ligne, gauche/droite
  conserve le décalage), Home/End. Pastilles contextuelles : en guide, seuls
  les pays avec ≥ 1 chaîne EPG (sélection sans guide stashée/restaurée, cycle
  L1/R1 borné à la liste active). OSD contextuel grille/guide via `_show_view`
  centralisé (fix : OSD restauré au retour vidéo).

## Tests

Sonde hors écran dans un `HOME` de bac à sable (le cache `~/.cache/gablue-tv`
et les logos ne doivent jamais atterrir dans le vrai HOME) :

```bash
cd <dépôt>
HOME=$(mktemp -d) QT_QPA_PLATFORM=offscreen PYTHONPATH=src/tvqt python3 - <<'EOF'
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QTimer
app = QApplication([])
from gablue_tv.window import TVWindow
w = TVWindow()                          # grille + guide + widget vidéo
assert w._stack.count() == 3
assert len(w._buttons) == len(w._filtered)
w._toggle_guide()                       # vue guide (virtualisée)
w._show_view(0)
QTimer.singleShot(100, w.close)
QTimer.singleShot(200, app.quit)
app.exec()
EOF
```

Aucun affichage, aucune manette requise (0 manette = OSD masqué). Vérifier
aussi `src/tvqt/tvqt` depuis le dépôt (résolution du `sys.path`).
