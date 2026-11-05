"""Feuille de style QSS globale de l'interface."""

# =============================================================================
# FEUILLE DE STYLE GLOBALE
# =============================================================================

APP_STYLESHEET = """
/* --- Fenêtre --- */
QMainWindow {
    background-color: #080818;
}

/* --- Zone de défilement --- */
QScrollArea {
    background-color: transparent;
    border: none;
}
QScrollBar:vertical {
    background: #101028;
    width: 8px;
    border-radius: 4px;
    margin: 2px;
}
QScrollBar::handle:vertical {
    background: #2a2a50;
    border-radius: 4px;
    min-height: 40px;
}
QScrollBar::handle:vertical:hover {
    background: #3a3a60;
}
QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
    height: 0px;
}
QScrollBar:horizontal {
    background: #101028;
    height: 8px;
    border-radius: 4px;
    margin: 2px;
}
QScrollBar::handle:horizontal {
    background: #2a2a50;
    border-radius: 4px;
    min-width: 40px;
}
QScrollBar::handle:horizontal:hover {
    background: #3a3a60;
}
QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal {
    width: 0px;
}

/* --- Cartes de chaînes --- */
QFrame#channelCard {
    background-color: #111130;
    border: 2px solid #1c1c40;
    border-radius: 16px;
}
QFrame#channelCard:hover {
    background-color: #181840;
    border-color: #b39000;
}
QFrame#channelCard[active="true"] {
    background-color: #181d45;
    border: 3px solid #e6b800;
}

/* --- Pastilles de pays --- */
QPushButton#countryPill {
    background-color: #151530;
    border: 1px solid #282848;
    border-radius: 20px;
    color: #d0d0e0;
    font-size: 12px;
    font-weight: 500;
    padding: 4px 16px;
    min-height: 28px;
    max-height: 32px;
}
QPushButton#countryPill:hover {
    background-color: #1e1e40;
    color: #c0c0e0;
    border-color: #404068;
}
QPushButton#countryPill[active="true"] {
    background-color: #e6b800;
    border-color: #e6b800;
    color: #0a0a16;
    font-weight: 700;
}

/* --- Barre d'en-tête --- */
QFrame#topBar {
    background-color: #0c0c20;
    border-bottom: 1px solid #1c1c38;
}
QLabel#titleLabel {
    color: #e6b800;
    font-size: 21px;
    font-weight: 800;
    letter-spacing: 2px;
}
QLabel#countLabel {
    color: #8888a0;
    font-size: 13px;
    font-weight: 500;
}

/* --- Barre OSD manette --- */
QFrame#osdBar {
    background-color: #0c0c1e;
    border-top: 1px solid #1c1c38;
}
QLabel#osdLabel {
    color: #8888a0;
    font-size: 11px;
    font-weight: 500;
}

/* --- Overlay de chargement --- */
QWidget#loadingOverlay {
    background-color: rgba(4, 4, 16, 0.94);
}
QLabel#loadingTitle {
    color: #e6b800;
    font-size: 26px;
    font-weight: 800;
    background: transparent;
}
QLabel#loadingSubtitle {
    color: #7878a0;
    font-size: 15px;
    background: transparent;
}

/* --- Bouton bascule Chaînes / Guide --- */
QPushButton#viewToggle {
    background-color: #1a1a3a;
    border: 1px solid #3a3a60;
    border-radius: 6px;
    color: #e6b800;
    font-size: 13px;
    font-weight: 700;
    padding: 5px 14px;
    min-height: 26px;
    max-height: 30px;
}
QPushButton#viewToggle:hover {
    background-color: #26264e;
    border-color: #e6b800;
}

/* --- Ligne « en cours » sous les cartes de chaînes --- */
QLabel#epgNow {
    color: #00bcd4;
    font-size: 11px;
    font-weight: 600;
    background: transparent;
}

/* --- Guide TV --- */
QLabel#guideStatus {
    color: #8888a0;
    font-size: 13px;
    font-weight: 600;
    padding: 4px 6px;
    background: transparent;
}
QLabel#guideStatus[failed="true"] {
    color: #e05555;
}
QFrame#guideRow {
    background-color: #10102c;
    border: 2px solid #1c1c40;
    border-radius: 10px;
}
QFrame#guideRow[active="true"] {
    background-color: #14143a;
    border: 2px solid #e6b800;
}
QLabel#guideChanName {
    color: #e8e8f5;
    font-size: 12px;
    font-weight: 600;
    background: transparent;
}
QFrame#progBlock {
    background-color: #1a1a3e;
    border: 1px solid #26264c;
    border-radius: 8px;
}
QFrame#progBlock[now="true"] {
    background-color: #0e323c;
    border: 1px solid #00bcd4;
}
QFrame#progBlock[sel="true"] {
    background-color: #20204a;
    border: 2px solid #e6b800;
}
QFrame#progBlock[now="true"][sel="true"] {
    border: 2px solid #e6b800;
}
QLabel#progTime {
    color: #8a8aa8;
    font-size: 10px;
    background: transparent;
}
QFrame#progBlock[now="true"] QLabel#progTime {
    color: #6fd8e8;
}
QLabel#progTitle {
    color: #f0f0fa;
    font-size: 12px;
    font-weight: 600;
    background: transparent;
}
QLabel#progCat {
    color: #8888a0;
    font-size: 10px;
    background: transparent;
}
QProgressBar#progBar {
    background: #0a2a30;
    border: none;
    border-radius: 2px;
    min-height: 4px;
    max-height: 4px;
}
QProgressBar#progBar::chunk {
    background-color: #00bcd4;
    border-radius: 2px;
}
"""
