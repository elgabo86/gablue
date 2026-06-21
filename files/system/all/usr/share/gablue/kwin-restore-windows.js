// Test: demaximiser et restaurer décorations
var windows = workspace.stackingOrder;
for (var i = 0; i < windows.length; i++) {
    var win = windows[i];
    if (win.normalWindow || win.dialog) {
        win.noBorder = false;
        // Tenter de dé-maximiser en réduisant la géométrie
        var area = workspace.clientArea(KWin.MaximizeArea, win.output, workspace.currentDesktop);
        // Réduire à 80% de la zone maximale, centré
        var w = Math.round(area.width * 0.8);
        var h = Math.round(area.height * 0.8);
        var x = area.x + Math.round((area.width - w) / 2);
        var y = area.y + Math.round((area.height - h) / 2);
        win.frameGeometry = { x: x, y: y, width: w, height: h };
    }
}
