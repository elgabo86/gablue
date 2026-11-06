"""Ajustements d'affichage partagés (style des libellés, hauteur, icône).
"""

from PySide6.QtCore import QEvent, QObject, Qt, QTimer
from PySide6.QtGui import QIcon
from PySide6.QtWidgets import QDialog, QLabel

def app_icon() -> QIcon:
    """Icône de Gablue Manager, avec repli sur l'ancienne icône réseau.

    L'icône `gablue-manager` vient de l'image (`hicolor/scalable/apps`) ; hors
    image (lancement depuis le dépôt), le thème ne la connaît pas encore et on
    garde `network-server` plutôt qu'une fenêtre sans icône.
    """
    icon = QIcon.fromTheme("gablue-manager")
    return icon if not icon.isNull() else QIcon.fromTheme("network-server")

class _FitOnShow(QObject):
    """Réduit un dialogue à la hauteur de son contenu, une fois affiché."""

    done = False    # attribut de classe : eventFilter peut passer avant l'init

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

def recommendation_badge(parent) -> QLabel:
    """Pastille de conseil, posée à côté du libellé d'un réglage.

    Elle porte le sens dans son texte (jamais dans la seule couleur) : le
    libellé en 12 px risquait de passer pour une note de bas de page.
    """
    badge = QLabel(parent)
    badge.setObjectName("recommend")
    badge.setAlignment(Qt.AlignCenter)
    badge.setToolTip(
        "Réglage conseillé par Gablue. Le choix reste libre : votre valeur "
        "est conservée telle quelle."
    )
    return badge

def set_recommendation(badge: QLabel, hint: str, differs: bool,
                       known: bool = True, advice: str = "") -> None:
    """Met la pastille à jour pour le choix courant.

    `hint` dit en clair **le geste** qui rejoint le conseil (« à activer »,
    « à désactiver », « choisir X11 ») ; il n'apparaît que quand le choix s'en
    écarte.
    `advice` dit **l'état conseillé** (« activé », « désactivé », « X11 ») et
    s'affiche quand le choix le suit : un « ✓ Conseillé » seul, posé à côté
    d'une case décochée, se lisait comme « il est conseillé de cocher » —
    exactement l'inverse du conseil. Sans état lisible, la pastille disparaît.
    """
    badge.setVisible(known)
    if not known:
        return
    if differs:
        badge.setText(f"Conseillé : {hint}")
        restyle_label(badge, "recommendWarn")
    else:
        badge.setText(f"✓ Conseillé : {advice}" if advice else "✓ Conseillé")
        restyle_label(badge, "recommend")
