"""Point d'entrée de Gablue TV (format OpenGL, QApplication, fenêtre)."""

import sys

from PySide6.QtGui import QFont, QSurfaceFormat
from PySide6.QtWidgets import QApplication

from .style import APP_STYLESHEET
from .window import TVWindow

# =============================================================================
# POINT D'ENTRÉE
# =============================================================================

def main():
    # Forcer OpenGL desktop 3.3 core (pas GLES) pour compatibilité mpv
    fmt = QSurfaceFormat()
    fmt.setRenderableType(QSurfaceFormat.OpenGL)
    fmt.setVersion(3, 3)
    fmt.setProfile(QSurfaceFormat.CoreProfile)
    QSurfaceFormat.setDefaultFormat(fmt)

    app = QApplication(sys.argv)
    app.setApplicationName("Gablue TV")
    app.setDesktopFileName("gablue-tv")
    app.setOrganizationName("gablue")
    app.setStyle("Fusion")

    # Police légèrement agrandie pour usage TV
    font = QFont("Noto Sans", 11)
    font.setStyleStrategy(QFont.PreferAntialias)
    app.setFont(font)
    app.setStyleSheet(APP_STYLESHEET)

    win = TVWindow()
    if "--fullscreen" in sys.argv or "-f" in sys.argv:
        win.showFullScreen()
    else:
        win.show()
    sys.exit(app.exec())
