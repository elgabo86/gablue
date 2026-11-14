"""Feuille de style, dérivée de la palette Plasma (thème clair ou sombre)."""

from PySide6.QtGui import QPalette


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

    QFrame#panel {{ background: {base}; border: 1px solid {border}; border-radius: 10px; }}
    QFrame#card {{ background: {base}; border: 1px solid {border}; border-radius: 10px; }}
    QFrame#card:hover {{ border: 1px solid {accent}; }}
    QFrame#row {{ background: transparent; border: none; border-bottom: 1px solid {border}; }}
    QFrame#toast {{ background: {base}; border: 1px solid {border}; border-radius: 8px; }}

    QPushButton {{
        background: {base}; border: 1px solid {border}; border-radius: 8px;
        padding: 7px 14px;
    }}
    QPushButton:hover {{ border: 1px solid {accent}; }}
    QPushButton:disabled {{ color: {muted}; border-color: {border}; }}
    QPushButton#primary {{
        background: {accent}; color: {palette.color(QPalette.HighlightedText).name()};
        border: 1px solid {accent}; font-weight: 600;
    }}
    QPushButton#flat {{ background: transparent; border: none; padding: 4px 8px; }}
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
