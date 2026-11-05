"""Ajustements d'affichage partagés (style des libellés, hauteur).
"""

from PySide6.QtCore import QEvent, QObject, QTimer
from PySide6.QtWidgets import QDialog, QLabel

class _FitOnShow(QObject):
    """Réduit un dialogue à la hauteur de son contenu, une fois affiché."""

    def __init__(self, dialog: QDialog, width: int):
        super().__init__(dialog)
        self.dialog = dialog
        self.width = width
        self.done = False

    def eventFilter(self, obj, event) -> bool:
        if not self.done and event.type() == QEvent.Show:
            self.done = True
            QTimer.singleShot(0, self._apply)
        return False

    def _apply(self) -> None:
        try:
            layout = self.dialog.layout()
        except RuntimeError:      # dialogue déjà détruit avant le timer
            return
        if layout is None:
            return
        width = self.dialog.width() or self.width
        height = layout.heightForWidth(width)
        if height > 0:
            self.dialog.resize(width, height)

def fit_dialog_height(dialog: QDialog, width: int) -> None:
    """Donne au dialogue la hauteur de son contenu à sa largeur finale.

    Un QDialog se dimensionne pour sa largeur « naturelle », plus étroite :
    les textes repliés y comptent plus de lignes, et une fois élargi par
    `setMinimumWidth` le dialogue garde un vide sous les cartes. Le calcul
    n'est juste qu'une fois la fenêtre affichée (sa largeur est alors connue),
    d'où le filtre sur l'événement Show.
    """
    if width > 0:
        dialog._fit_on_show = _FitOnShow(dialog, width)
        dialog.installEventFilter(dialog._fit_on_show)

def restyle_label(widget: QLabel, name: str) -> None:
    """Change l'objectName d'un libellé et réapplique la feuille de style."""
    if widget.objectName() == name:
        return
    widget.setObjectName(name)
    widget.style().unpolish(widget)
    widget.style().polish(widget)
