// KWin script: maximise et enlève les décorations
var windows = workspace.stackingOrder;
for (var i = 0; i < windows.length; i++) {
    var win = windows[i];
    if (win.normalWindow || win.dialog) {
        var area = workspace.clientArea(KWin.MaximizeArea, win.output, workspace.currentDesktop);
        win.frameGeometry = area;
        win.noBorder = true;
        win.minimized = true;
    }
}
