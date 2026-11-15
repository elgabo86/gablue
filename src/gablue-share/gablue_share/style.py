"""Feuille de style, dérivée de la palette Plasma (thème clair ou sombre)."""

from PySide6.QtGui import QColor, QPalette


def _veil(color: str, alpha: float) -> str:
    """Le texte du thème, rendu translucide : un contour lisible partout.

    `QPalette.Mid` est calculé entre le bouton et le fond : dans un thème
    sombre, il se confond avec le fond du panneau et les boutons n'ont plus de
    contour (vécu 15/11/2026). Une teinte dérivée du texte marche dans les deux
    thèmes.
    """
    teinte = QColor(color)
    return f"rgba({teinte.red()}, {teinte.green()}, {teinte.blue()}, {alpha})"


def stylesheet(palette: QPalette) -> str:
    window = palette.color(QPalette.Window).name()
    base = palette.color(QPalette.Base).name()
    text = palette.color(QPalette.Text).name()
    muted = palette.color(QPalette.PlaceholderText).name()
    accent = palette.color(QPalette.Highlight).name()
    border = palette.color(QPalette.Mid).name()
    return f"""
    QWidget {{ color: {text}; }}
    QMainWindow, QDialog {{ background: {window}; }}

    QLabel#title {{ font-size: 16px; font-weight: 600; }}
    QLabel#section {{ font-size: 13px; font-weight: 600; }}
    QLabel#muted {{ color: {muted}; }}
    QLabel#hint {{ color: {muted}; font-size: 12px; }}
    /* Avertissement (poste déjà partagé par un autre compte) : un ambre
       translucide, lisible sur fond clair comme sombre — le texte reste celui
       de la palette. */
    QLabel#alert {{
        background: rgba(229, 165, 10, 0.14);
        border: 1px solid rgba(229, 165, 10, 0.55);
        border-radius: 8px; padding: 8px 10px;
    }}

    QFrame#panel {{ background: {base}; border: 1px solid {border}; border-radius: 10px; }}
    QFrame#card {{ background: {base}; border: 1px solid {border}; border-radius: 10px; }}
    QFrame#card:hover {{ border: 1px solid {accent}; }}
    QFrame#row {{ background: transparent; border: none; border-bottom: 1px solid {border}; }}
    QWidget#scrim {{ background: rgba(0, 0, 0, 110); }}
    QFrame#toast {{ background: {base}; border: 1px solid {border}; border-radius: 8px; }}

    QPushButton {{
        background: {base}; border: 1px solid {border}; border-radius: 8px;
        padding: 7px 14px;
    }}
    QPushButton:hover {{ border: 1px solid {accent}; }}
    QPushButton:disabled {{ color: {muted}; border-color: {border}; }}
    /* Sans ces deux règles, le style explicite écrase le grisé de Qt : un
       bouton désactivé gardait son bleu plein (constaté 15/11/2026). */
    QPushButton#primary:disabled {{
        background: {_veil(text, 0.10)}; color: {muted};
        border-color: {_veil(text, 0.14)};
    }}
    QComboBox:disabled {{ color: {muted}; border-color: {_veil(text, 0.14)}; }}
    QPushButton#primary {{
        background: {accent}; color: {palette.color(QPalette.HighlightedText).name()};
        border: 1px solid {accent}; font-weight: 600;
    }}
    QPushButton#flat {{ background: transparent; border: none; padding: 4px 8px; }}
    QPushButton#chip {{
        background: transparent; border: 1px solid {_veil(text, 0.30)};
        border-radius: 6px; padding: 4px 10px;
    }}
    QPushButton#chip:hover {{ border: 1px solid {accent}; color: {accent}; }}
    QPushButton#chip:disabled {{ color: {muted}; border-color: {_veil(text, 0.12)}; }}
    QPushButton#chevron {{
        background: transparent; border: none; padding: 0; font-size: 15px;
    }}
    QPushButton#chevron:hover {{ color: {accent}; }}

    /* Barres de défilement : une poignée arrondie sur piste transparente,
       sans flèches — sinon Qt peint la sienne, qui jure avec le reste. */
    QScrollBar:vertical {{ background: transparent; width: 10px; margin: 0; }}
    QScrollBar::handle:vertical {{
        background: {_veil(text, 0.22)}; border-radius: 5px; min-height: 28px;
    }}
    QScrollBar::handle:vertical:hover {{ background: {_veil(text, 0.40)}; }}
    QScrollBar:horizontal {{ background: transparent; height: 10px; margin: 0; }}
    QScrollBar::handle:horizontal {{
        background: {_veil(text, 0.22)}; border-radius: 5px; min-width: 28px;
    }}
    QScrollBar::handle:horizontal:hover {{ background: {_veil(text, 0.40)}; }}
    QScrollBar::add-line, QScrollBar::sub-line {{ width: 0; height: 0; }}
    QScrollBar::add-page, QScrollBar::sub-page {{ background: transparent; }}
    QPushButton#flat:hover {{ background: {window}; border-radius: 6px; }}

    QTableWidget {{
        background: {base}; border: 1px solid {border}; border-radius: 10px;
        gridline-color: transparent; selection-background-color: {accent};
        selection-color: {palette.color(QPalette.HighlightedText).name()};
    }}
    QHeaderView::section {{
        background: {base}; color: {muted}; border: none;
        border-bottom: 1px solid {border}; padding: 6px 8px;
    }}
    QTableWidget::item {{ padding: 4px 8px; }}

    QScrollArea {{ background: transparent; border: none; }}
    QScrollArea > QWidget > QWidget {{ background: transparent; }}
    QProgressBar {{
        background: {window}; border: none; border-radius: 4px; height: 6px;
        text-align: center;
    }}
    QProgressBar::chunk {{ background: {accent}; border-radius: 4px; }}
    QComboBox {{ background: {base}; border: 1px solid {border}; border-radius: 8px; padding: 6px 10px; }}
    QToolTip {{ background: {base}; color: {text}; border: 1px solid {border}; }}
    """
