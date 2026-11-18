// lister.js - pousse la liste des fenêtres vers le panneau, une entrée par
// fenêtre puis un « Fin ». Une entrée par appel : un tableau de structures
// D-Bus est pénible à marshaler depuis QJSEngine.
//
// Filtre = celui d'une barre des tâches : fenêtre normale ET pas « skipTaskbar ».
// Sans lui, le bureau (plasmashell), le pont Wayland->X11 et le panneau
// lui-même (resourceClass gpad) apparaissent dans la liste.
//
// __BUS__ = nom de SERVICE de l'overlay appelant, remplacé au chargement
// (org.gablue.Win.p<pid>) : deux instances peuvent coexister (gpad-trigger + banc).
// L'INTERFACE reste org.gablue.Win (la remplacer casse l'appel).
var gablueBus = "__BUS__";
var ws = workspace.stackingOrder;
var act = workspace.activeWindow;
var n = 0;
for (var i = 0; i < ws.length; i++) {
    var w = ws[i];
    if (!w.normalWindow || w.skipTaskbar || !w.resourceClass) {
        continue;
    }
    callDBus(gablueBus, "/win", "org.gablue.Win", "Entree",
             n,
             String(w.caption || w.resourceClass),
             String(w.resourceClass),
             Number(w.pid || 0),
             String(w.internalId),
             String(w.desktopFileName || ""),
             Boolean(w === act));
    n++;
}
callDBus(gablueBus, "/win", "org.gablue.Win", "Fin", n);
