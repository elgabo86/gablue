# trigger — `gpad-trigger` : déclencheur Home, combos, menu système

Petit démon C : lit les manettes evdev **sans grabbe**, ouvre l'overlay sur Home,
transforme Home+direction en combo, rescanne les manettes (2 s) et retire le
panneau si la manette disparaît. Il tient aussi l'inhibition d'écran. Lance
l'overlay avec `--console` pour le menu système.

## Home : le jeu le reçoit aussi, et c'est assumé

`gpad-trigger` lit la manette **sans grabbe**, et evdev livre chaque événement à
**tous** les lecteurs en même temps : quand Home ouvre le panneau, le jeu a déjà
reçu l'appui. Rien ne permet de le retirer. Comportement actuel, voulu :
**Home ouvre le panneau ET le jeu le voit**. De même Home referme le panneau
(`CLOSE (Home)`).

Un « double appui = Home pour le jeu, pas de menu » a été implémenté puis
**retiré** (16/11/2026) : le menu doit s'ouvrir à chaque appui.

Les mesures qui ferment le sujet « que le jeu ne voie rien sur un appui
simple » (16/11/2026, à revérifier si le noyau évolue) :

- **le grab est par périphérique** (`EVIOCGRAB`) : aucun moyen de retenir un seul
  bouton ; c'est pour ça que le panneau peut couper *toutes* les touches, mais
  pas seulement Home ;
- **réémettre dans le même nœud est hors de prix** : un cycle grab/ungrab coûte
  **14 ms** (`synchronize_rcu()` de `evdev_ungrab`) → 176 % de CPU à 125 Hz.
  L'injection dans le nœud n'est possible que grab relâché ;
- rester grabbé en permanence impose donc un **périphérique virtuel** (clone
  UHID, cf. Steam Input / inputtino) — écarté pour l'instant : pont
  bidirectionnel fragile (rapport BT `0x31` + CRC32, gâchettes adaptatives,
  vibration) ;
- `vpad` du banc d'essai **ne sait pas** vibrer et un nœud uinput naît en
  `root:root 0600` : ne pas compter dessus pour tester l'injection ou la manette.

Piège (résolu par l'intégration) : un second démon qui lit la manette en
continue tenait le nœud et `EVIOCGRAB` renvoyait `EBUSY` — le panneau n'isolait
plus rien, sans autre trace que `grab: EVIOCGRAB … Busy`. Il n'y a plus qu'un
seul lecteur : `gablue-pad.service` (l'ancien service manette est supprimé).

## Chemins installés

Le banc résout tout par rapport au binaire (`../recorder.sh`,
`../gate/padnodes.sh`…). Dans l'image, l'unité utilisateur exporte des chemins
absolus et le déclencheur les prend en priorité (`chemins_env()`) :
`RECORDER`, `SCREENSHOT`, `SCRIPTS`, `INHIBIT`, `MOUSE`, `KBDNAV`, `REPLAY`,
`PADNODES`, `GPAD_GATE`, `KILLTHEMALL` (script partagé hors du dossier du
service).

## Options (Gablue Manager)

`~/.config/gablue/gpad-options`, une ligne `clé=valeur`, **absente = on** :
`menu` (Home ouvre le panneau et le menu système), `shortcuts` (combos),
`inhibit` (pas de verrouillage d'écran). Le helper `/usr/bin/gablue-pad` écrit
le fichier puis envoie **SIGHUP** : le déclencheur relit et applique sans
redémarrage. `menu=off` laisse Home au jeu ; `shortcuts=off` n'empêche pas
Home seul d'ouvrir le panneau (les deux fonctions sont indépendantes).

## Gate

Si le socket existe (`GPAD_GATE`, sinon `/run/gablue-pad/gate.sock`), le
déclencheur calcule les nodes js/hidraw avec `padnodes.sh` **au démarrage et à
chaque changement de manettes** (jamais à chaque appui : le script coûte des
dizaines de ms) et les passe à l'overlay en `--gate` / `--gate-target`. Démon
absent : l'overlay reste utilisable, il n'isole simplement pas la manette.

## Combos Home + D-pad

Tous ne partent que si **Home est tenu** et qu'aucun panneau n'est ouvert
(`child < 0`) : un combo posé sur un appui simple n'a pas de sens, et Home seul
ouvre le panneau **au relâchement**.

| Combo | Effet |
|---|---|
| Home + gauche | sauvegarde du buffer de replay s'il tourne, sinon bascule de l'**enregistrement** (`replay.sh status` fait foi, jamais un état local) |
| Home + droite | capture d'écran (`screenshot.sh`) |
| Home + haut | **bascule du mode replay** (`replay.sh toggle`, en arrière-plan : le bip de départ + 0,35 s de silence + démarrage prennent ~1,2 s, et `gpad-trigger` doit rester à l'écoute du retour de Home) |
| Home + bas | ouvre le panneau directement sur la ligne **FPS** |

`PAD_NODE` est transmis pour que la vibration parte sur la bonne manette.

## Menu système (Home maintenu ~1,2 s)

Comme une vraie console : garder **Home** appuyé ouvre un **petit panneau
centré** (520×360) qui ne contient que des actions définitives.

| Ligne | Effet (après confirmation) |
|---|---|
| `TOUT FERMER` | `scripts/killthemall` (jeux Windows/Wine, émulateurs, LGP/AppImage montés) **puis** `scripts/fermer-applis` (les applis du menu Applications : ES-DE, VacuumTube, Gablue TV, Jellyfin) — contrairement à Home+SELECT qui ne fait que `killthemall` |
| `DECONNECTER LA MANETTE` | seulement si la manette qui pilote est en **Bluetooth** |
| `REDEMARRER` | `systemctl reboot` |
| `ETEINDRE` | `systemctl poweroff` |

- **Toutes demandent deux appuis** (armement rouge + `?`, navigation désarme).
- **Le menu se ferme avant d'agir** (`combo_apres`), comme le replay : le
  panneau ne doit ni rester à l'écran pendant l'extinction, ni se filmer.
- **Maintien alors que le panneau latéral est OUVERT** : `gpad-trigger` ne peut pas
  le voir — c'est l'overlay qui tient `EVIOCGRAB`, donc lui seul lit la manette.
  L'overlay vérifie le maintien **à chaque tour de boucle** (`surveiller_home`,
  seuil `HOME_LONG_MS` = 1,2 s, identique à `gpad-trigger`) ; au seuil il envoie
  **SIGUSR1** au pid de `gpad-trigger` (reçu en `GPAD_TRIGGER_PID`), affiche
  `HOME MAINTENU -> menu systeme`, puis **se ferme**. `gpad-trigger` reçoit le
  signal, **attend sa sortie** (pad rendu) et lance `--console`. Mesuré :
  `HOME MAINTENU -> menu systeme` + `[gpad-trigger] demande de menu systeme
  (overlay)` + `SIGNAL -> fermeture` + `CONSOLE items=3`.
- **Bug de boucle corrigé (17/11/2026)** : `gpad-trigger` faisait
  `if (poll(...) <= 0) continue;`. Or `poll` renvoie **0** à chaque délai (50 ms)
  et **-1/EINTR** à chaque signal (SIGCHLD, SIGUSR1) : le bas de boucle — où
  vivent le maintien de Home et la demande de menu système — n'était donc
  jamais atteint sans événement manette. Le test est maintenant
  `if (r < 0 && errno != EINTR) continue;`.
- Bascule : **un maintien n'ouvre rien d'autre que le menu système**. Le panneau
  latéral n'est lancé qu'au **relâchement** d'un appui court ; à 1,2 s
  (`HOME_LONG_MS`) `gpad-trigger` lance directement `--console`. Sans ça, le
  panneau latéral apparaissait à 300 ms puis disparaissait quand le menu système
  prenait sa place (constaté à l'usage) — la « grâce de 300 ms » a été
  supprimée, un combo n'ayant plus de course avec le panneau.
- Si un panneau latéral est **déjà** ouvert (Home appuyé une seconde fois),
  `gpad-trigger` l'arrête (`SIGTERM`, fermeture propre : pad rendu + gate refermé),
  **attend sa sortie** puis lance le menu système. Une seule instance peut grabber la
  manette : relancer avant que l'ancienne ait rendu le pad donne `EBUSY`.
- Côté overlay, `--console` (drapeau interne, journaux `CONSOLE …`) = surface
  plein écran + cadre centré, **pas de KWin**, pas de nom de menu, pas d'en-tête de
  fenêtre, et **aucun titre** : trois ou quatre lignes et l'aide (**centrée**).
  Le panneau latéral non plus n'a plus de titre `OVERLAY MANETTE` : le **nom du
  menu courant** s'affiche en bas (`nom_menu`, centré), plus de `L1 < PAGE > R1`
  en haut.
- Banc : `POWEROFF` / `REBOOT` remplacent `systemctl` (le banc **ne doit jamais
  éteindre la machine**), `KILLTHEMALL` et `BLUETOOTHCTL` comme d'habitude.

Mesuré : Home tenu → `HOME maintenu -> menu systeme` +
`SIGNAL -> fermeture` + `CONSOLE items=4` ; `ARME TOUT FERMER` → `CONSOLE
killthemall` + `GPAD_DOWN` + stub appelé ; `ARME DECONNECTER LA MANETTE` →
`BT disconnect aa:bb:…` ; `ARME REDEMARRER` → `SYSTEME reboot` (stub).

## Manette Bluetooth : déconnexion depuis le menu système

L'entrée `DECONNECTER LA MANETTE` n'existe **que** dans le menu système, et
seulement si la manette qui pilote est en **Bluetooth** (jamais pour une manette
USB/filaire/virtuelle). Il n'y a **pas** d'entrée « quitter » : **B** ferme le
panneau, c'est tout.

- Détection : `/sys/class/input/eventN/device` (résolu) contient
  `/virtual/misc/uhid/` → Bluetooth ; l'adresse vient de `uniq`. Même règle que
  `padnodes.sh` (`transport=bt`). Vérifié sur la vraie DualSense :
  `BT 84:30:95:42:25:35`.
- Action : `bluetoothctl disconnect <mac>` (sans root). La manette part →
  `gpad-trigger` enlève le panneau → le gate se referme, donc l'overlay ne peut pas
  rester avec une manette coupée.
- Hooks de banc : `PAD_BT_MAC=a4:ae:…` force la détection (manette virtuelle),
  `BLUETOOTHCTL=/chemin/stub` remplace l'outil. **Ne jamais déconnecter la
  vraie manette en test** : vérifier la détection et le stub suffisent.

## Écran : pas de verrouillage tant qu'une manette est connectée

`gpad-trigger` tient une inhibition `org.freedesktop.ScreenSaver.Inhibit` **tant
qu'au moins une manette est connectée** (comportement de l'ancien
`gamepadshortcuts`). Le helper `gpad-inhibit` garde la connexion D-Bus
ouverte — un `gdbus call` ponctuel rendrait le cookie mort à la sortie du
processus — et relâche (`UnInhibit`) sur EOF de son stdin, que `gpad-trigger` ferme
quand plus aucune manette n'est vue.

Mesuré (`dbus-monitor`) : même connexion émettrice, `Inhibit` → cookie 14118,
puis `UnInhibit uint32 14118` à la disparition de la manette ; helper à 0
processus ensuite.

Deux pièges payés, tous les deux dans `gpad-trigger` :

- **`FD_CLOEXEC` sur le tube** : sans lui, l'overlay (et chaque script forké)
  hérite du tube ; fermer notre copie ne produit **aucun EOF** et l'écran reste
  inhibé à vie (mesuré : `INHIBIT` sans jamais d'`UNINHIBIT`).
- **`poll(0 fd)` + `continue`** : quand la dernière manette partait, `poll`
  rendait 0 immédiatement et le `continue` sautait l'évaluation de
  l'inhibition. Elle est maintenant évaluée **avant** le `poll`, et le délai
  passe à 1 s sans manette (fini le spam « aucune manette »).

## Mesuré

| Mesure | Valeur |
|---|---|
| détection BT | `BT 84:30:95:42:25:35` sur la vraie DualSense ; rien sur une manette virtuelle |
| deuxième manette | ne pilote pas (0 événement) ; manette partie → panneau enlevé, gate refermé |
| Home maintenu | `HOME maintenu -> menu systeme` + `SIGNAL -> fermeture` + `CONSOLE items=4` |
