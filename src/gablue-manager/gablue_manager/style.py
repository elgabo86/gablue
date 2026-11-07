"""Feuille de style QSS adaptée au thème Plasma.
"""

from PySide6.QtGui import QColor, QPalette
from PySide6.QtWidgets import QApplication

from .constants import COLOR_AUTH, COLOR_ONLINE

def _rgba(color: QColor, alpha: int) -> str:
    return f"rgba({color.red()},{color.green()},{color.blue()},{alpha})"

QSS_TEMPLATE = """
QLabel { background: transparent; }
QLabel#title { font-size: 22px; font-weight: 700; }
QLabel#subtitle { color: @dim@; font-size: 12px; }
QLabel#footer { color: @dim@; font-size: 12px; }
QLabel#hostName { font-size: 15px; font-weight: 600; }
QLabel#hostDetail { color: @dim@; font-size: 12px; }
QLabel#hostStatus { font-size: 12px; font-weight: 500; }
QLabel#groupName { color: @dim@; font-size: 12px; font-weight: 600; }
QCheckBox { font-size: 14px; font-weight: 600; spacing: 8px; }
QLabel#emptyTitle { font-size: 18px; font-weight: 600; }
QLabel#emptyText { color: @dim@; font-size: 13px; }
QLabel#error { color: #e5484d; font-size: 12px; }
QLabel#ok { color: @online@; font-size: 12px; font-weight: 600; }
QLabel#warn { color: @auth@; font-size: 12px; font-weight: 600; }
QLabel#hint { color: @dim@; font-size: 11px; }
QLabel#recommend {
    background: @onlineSoft@;
    color: @online@;
    border: 1px solid @onlineBorder@;
    border-radius: 9px;
    padding: 1px 8px;
    font-size: 11px;
    font-weight: 600;
}
QLabel#recommendWarn {
    background: @authSoft@;
    color: @auth@;
    border: 1px solid @authBorder@;
    border-radius: 9px;
    padding: 1px 8px;
    font-size: 11px;
    font-weight: 600;
}

QScrollArea { background: transparent; border: none; }
QScrollArea > QWidget > QWidget { background: transparent; }

QTabWidget::pane {
    border: 1px solid @border@;
    border-radius: 12px;
    top: -1px;
}
QTabBar::tab {
    background: transparent;
    color: @dim@;
    border: 1px solid transparent;
    border-top-left-radius: 9px;
    border-top-right-radius: 9px;
    padding: 7px 14px;
    margin-right: 4px;
    font-size: 12px;
}
QTabBar::tab:selected {
    background: @card@;
    color: @text@;
    border-color: @border@;
}
QTabBar::tab:hover { color: @text@; }

/* Croix de fermeture de la page d'un poste, seule au-dessus de la carte :
   plus grande que la croix d'onglet d'autrefois, c'est la sortie vers la
   liste. */
QToolButton#pageClose {
    background: transparent;
    border: none;
    border-radius: 10px;
    color: @dim@;
    font-size: 17px;
}
QToolButton#pageClose:hover { background: @cardHover@; color: @text@; }

QFrame#hostCard {
    background: @card@;
    border: 1px solid @border@;
    border-radius: 14px;
}
QFrame#hostCard[hover="true"] {
    background: @cardHover@;
    border-color: @accent@;
}

QFrame#localCard {
    background: @card@;
    border: 1px solid @accentSoft@;
    border-radius: 14px;
}

/* Panneaux du résumé (matériel, accès) : même famille que les cartes, sans
   l'accent de la carte du poste local. */
QFrame#panel {
    background: @card@;
    border: 1px solid @border@;
    border-radius: 14px;
}
QLabel#panelTitle { font-size: 13px; font-weight: 600; }
QLabel#specLabel { color: @dim@; font-size: 11px; }
QLabel#specValue { font-size: 13px; }
QLabel#updateLine { font-size: 12px; }

/* Bandeau actionnable (redémarrage requis) : orange, mais dans le ton du
   thème — il alerte sans crier. */
QFrame#notice {
    background: @authSoft@;
    border: 1px solid @authBorder@;
    border-radius: 12px;
}
QLabel#noticeText { color: @auth@; font-size: 12px; font-weight: 600; }

/* Onglets racine : le contenu n'est pas encadré, seuls les onglets portent le
   contour — la fenêtre reste une page, pas un formulaire. */
QTabWidget#mainTabs::pane { border: none; }
QTabWidget#mainTabs QTabBar::tab { padding: 8px 16px; font-size: 13px; }

/* Onglets de la fenêtre : boutons d'un en-tête maison, centrés (la barre
   native de QTabWidget ne sait pas se centrer sous QSS). */
QPushButton#tabButton {
    background: transparent;
    color: @dim@;
    border: 1px solid transparent;
    border-top-left-radius: 9px;
    border-top-right-radius: 9px;
    padding: 8px 16px;
    font-size: 13px;
}
QPushButton#tabButton:checked {
    background: @card@;
    color: @text@;
    border-color: @border@;
}
QPushButton#tabButton:hover { color: @text@; }

QPushButton {
    background: @card@;
    color: @text@;
    border: 1px solid @border@;
    border-radius: 9px;
    padding: 7px 14px;
    font-size: 12px;
}
QPushButton:hover { background: @cardHover@; border-color: @accentSoft@; }
QPushButton:pressed { background: @pressed@; }
QPushButton:disabled { color: @dim@; }

QPushButton#accent {
    background: @accent@;
    color: @onAccent@;
    border: 1px solid transparent;
    font-weight: 600;
    padding: 7px 16px;
}
QPushButton#accent:hover { background: @accentHover@; }
QPushButton#accent:disabled { background: @card@; color: @dim@; }

QPushButton#ghost {
    background: transparent;
    border: 1px solid @border@;
}

/* Vérifié à jour (Applications) : vert et inerte, pas grisé comme un bouton
   qui aurait perdu sa fonction. */
QPushButton#ok {
    background: @onlineSoft@;
    color: @online@;
    border: 1px solid @onlineBorder@;
    font-weight: 600;
}
QPushButton#ok:disabled {
    background: @onlineSoft@;
    color: @online@;
    border-color: @onlineBorder@;
}

QToolButton#more {
    background: transparent;
    color: @text@;
    border: 1px solid transparent;
    border-radius: 9px;
    padding: 4px 10px;
    font-size: 16px;
    font-weight: 700;
}
QToolButton#more:hover { background: @cardHover@; border-color: @border@; }
QToolButton#more::menu-indicator { image: none; }

QLineEdit, QSpinBox, QComboBox {
    background: @field@;
    color: @text@;
    border: 1px solid @border@;
    border-radius: 8px;
    padding: 7px 10px;
    selection-background-color: @accent@;
    selection-color: @onAccent@;
}
QLineEdit:focus, QSpinBox:focus, QComboBox:focus { border-color: @accent@; }
QComboBox::drop-down { border: none; width: 22px; }
QComboBox QAbstractItemView {
    background: @menu@;
    color: @text@;
    border: 1px solid @border@;
    border-radius: 8px;
    selection-background-color: @accent@;
    selection-color: @onAccent@;
}

QPlainTextEdit {
    background: @field@;
    color: @text@;
    border: 1px solid @border@;
    border-radius: 8px;
    font-family: monospace;
    font-size: 11px;
}

QProgressBar#busy {
    background: transparent;
    border: none;
    max-height: 3px;
}
QProgressBar#taskProgress {
    background: @barTrack@;
    border: none;
    border-radius: 5px;
    min-height: 22px;
    max-height: 22px;
    color: @text@;
    font-size: 11px;
    text-align: center;
}
QProgressBar#taskProgress::chunk {
    background: @accent@;
    border-radius: 5px;
}

QMenu {
    background: @menu@;
    color: @text@;
    border: 1px solid @border@;
    border-radius: 10px;
    padding: 6px;
}
QMenu::item { padding: 7px 22px 7px 14px; border-radius: 6px; }
QMenu::item:selected { background: @accent@; color: @onAccent@; }
QMenu::separator { height: 1px; background: @border@; margin: 5px 8px; }
"""

def accent_color() -> QColor:
    """Couleur d'accent effective — celle du QSS, repli compris.

    Certains thèmes Plasma donnent une couleur de surbrillance aussi claire que
    le texte : invisible en fond de barre. `make_qss` bascule alors sur le bleu
    Gablue, et la peinture (barre indéterminée) doit utiliser la même.
    """
    palette = QApplication.palette()
    accent = palette.color(QPalette.Highlight)
    if accent.lightness() == palette.color(QPalette.WindowText).lightness():
        accent = QColor("#3daee9")
    return accent

def make_qss() -> str:
    palette = QApplication.palette()
    dark = palette.color(QPalette.Window).lightness() < 128
    text = palette.color(QPalette.WindowText)
    accent = accent_color()
    on_accent = palette.color(QPalette.HighlightedText)
    base = palette.color(QPalette.Base)
    white = QColor(255, 255, 255)
    black = QColor(0, 0, 0)
    tint = white if dark else black
    values = {
        "@card@": _rgba(tint, 20 if dark else 9),
        "@cardHover@": _rgba(tint, 34 if dark else 16),
        "@pressed@": _rgba(tint, 46 if dark else 24),
        "@border@": _rgba(tint, 32 if dark else 26),
        "@barTrack@": _rgba(tint, 26 if dark else 22),
        "@accent@": accent.name(),
        "@accentSoft@": _rgba(accent, 170),
        "@accentHover@": (accent.lighter(112) if dark else accent.darker(108)).name(),
        "@onAccent@": on_accent.name(),
        "@text@": text.name(),
        "@dim@": _rgba(text, 160),
        "@field@": base.name(),
        "@menu@": base.name(),
        "@online@": COLOR_ONLINE,
        "@onlineSoft@": _rgba(QColor(COLOR_ONLINE), 34),
        "@onlineBorder@": _rgba(QColor(COLOR_ONLINE), 110),
        "@auth@": COLOR_AUTH,
        "@authSoft@": _rgba(QColor(COLOR_AUTH), 34),
        "@authBorder@": _rgba(QColor(COLOR_AUTH), 120),
    }
    qss = QSS_TEMPLATE
    for token, value in values.items():
        qss = qss.replace(token, value)
    return qss
