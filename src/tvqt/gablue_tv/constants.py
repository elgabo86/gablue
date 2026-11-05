"""Constantes de Gablue TV (tvqt).

Chemins de cache, flux HLS, géométrie de l'interface et durées partagées par
les modules. Aucune dépendance Qt ni evdev : valeurs seules.
"""

import os

# =============================================================================
# CONSTANTES RÉSEAU ET STOCKAGE
# =============================================================================

BASE = "https://viamotionhsi.netplus.ch/live/eds"
SUFFIX = "/browser-HLS8"

CONFIG_DIR = os.path.expanduser("~/.cache/gablue-tv")
CONFIG_FILE = os.path.join(CONFIG_DIR, "settings.json")
LOGO_DIR = os.path.join(CONFIG_DIR, "logos")
LOGO_BASE = "https://picserve.netplus.ch/channels"

# =============================================================================
# DIMENSIONS ET CONSTANTES D'INTERFACE
# =============================================================================

CARD_W = 168
CARD_H = 172

COOLDOWN_MS = 180
GRID_SPACING = 14
GRID_MARGIN = 20
CARD_RADIUS = 16
CURSOR_HIDE_MS = 2500  # délai de masquage du curseur en lecture

# --- Guide TV ---
GUIDE_CHAN_W = 150    # largeur de la cellule chaîne (logo + nom)
GUIDE_ROW_H = 96      # hauteur d'une ligne du guide
GUIDE_BLOCK_H = 84    # hauteur d'un bloc programme
GUIDE_MAX_PROGS = 8   # programmes affichés par chaîne (en cours + suivants)
GUIDE_ROW_PITCH = GUIDE_ROW_H + 6      # pas vertical d'une ligne (h + espacement)
GUIDE_BUFFER_ROWS = 4                  # lignes construites au-delà du visible
