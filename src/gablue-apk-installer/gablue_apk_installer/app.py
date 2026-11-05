"""Point d'entrée de l'installateur APK Gablue."""

import sys

from PySide6.QtWidgets import QApplication

from .window import ApkInstallerWindow


def main():
    app = QApplication(sys.argv)
    app.setApplicationName("Installateur APK Gablue")

    window = ApkInstallerWindow()
    window.show()

    return app.exec()
