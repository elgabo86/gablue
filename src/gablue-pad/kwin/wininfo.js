// wininfo.js - pousse la fenêtre active vers le panneau.
//
// __BUS__ = nom de SERVICE de l'overlay appelant, remplacé au chargement
// (org.gablue.Win.p<pid>) ; l'INTERFACE, elle, reste org.gablue.Win.
//
// Chargé par l'overlay à l'ouverture (pluginName « gablue-wininfo ») et
// déchargé à la fermeture : le QTimer disparaît avec le script, rien ne reste
// dans KWin. KWin n'expose aucun getter « fenêtre active » sur D-Bus, d'où ce
// script ; ses `console.info` vont dans le journal du compositeur.
var gablueBus = "__BUS__";

function gabluePush() {
    // try/catch : un appel D-Bus raté (service pas encore prêt) ne doit pas
    // tuer le script avant la création du QTimer
    try {
    var w = workspace.activeWindow;
    if (!w) {
        callDBus(gablueBus, "/win", "org.gablue.Win", "Info",
                 "", "", 0, "", false, "");
        return;
    }
    // internalId : c'est l'ANCRE du cyclage L2/R2 (l'overlay la renvoie au
    // script KWin) ; `workspace.activeWindow` ne suffit pas, le panneau
    // lui-même peut être la fenêtre active
    callDBus(gablueBus, "/win", "org.gablue.Win", "Info",
             String(w.caption || ""),
             String(w.resourceClass || ""),
             Number(w.pid || 0),
             String(w.desktopFileName || ""),
             Boolean(w.fullScreen),
             String(w.internalId || ""));
    } catch (e) {
    }
}

console.info("WININFO chargement, bus=" + gablueBus);
gabluePush();
console.info("WININFO push initial fait");
workspace.windowActivated.connect(gabluePush);

// 300 ms suffisent : le panneau relit l'état une fois par seconde.
var gablueTimer = new QTimer();
gablueTimer.interval = 300;
gablueTimer.timeout.connect(function() { console.info("WININFO tick"); gabluePush(); });
gablueTimer.start();
