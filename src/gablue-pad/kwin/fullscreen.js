// fullscreen.js - bascule le plein écran de la fenêtre ACTIVE (chargé à la
// demande par l'overlay, exécuté, puis déchargé). KWin est seul maître des
// propriétés de fenêtre sur Wayland : passer par un script est le seul moyen
// de les modifier.
var w = workspace.activeWindow;
if (w) {
    w.fullScreen = !w.fullScreen;
}
