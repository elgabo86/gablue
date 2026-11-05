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

QPushButton#pill {
    background: transparent;
    border: 1px solid @border@;
    border-radius: 11px;
    padding: 4px 11px;
    font-size: 11px;
}
QPushButton#pill:hover { background: @cardHover@; border-color: @accentSoft@; }

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
QProgressBar#busy::chunk { background: @accent@; border-radius: 1px; }

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

def make_qss() -> str:
    palette = QApplication.palette()
    dark = palette.color(QPalette.Window).lightness() < 128
    text = palette.color(QPalette.WindowText)
    accent = palette.color(QPalette.Highlight)
    on_accent = palette.color(QPalette.HighlightedText)
    if accent.lightness() == text.lightness():
        accent = QColor("#3daee9")
    base = palette.color(QPalette.Base)
    white = QColor(255, 255, 255)
    black = QColor(0, 0, 0)
    tint = white if dark else black
    values = {
        "@card@": _rgba(tint, 20 if dark else 9),
        "@cardHover@": _rgba(tint, 34 if dark else 16),
        "@pressed@": _rgba(tint, 46 if dark else 24),
        "@border@": _rgba(tint, 32 if dark else 26),
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
