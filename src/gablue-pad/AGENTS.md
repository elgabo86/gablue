# gablue-pad — overlay manette façon Steam Deck

**But** : remplacer `gamepadshortcuts` et ses scripts par un overlay unique,
piloté à la manette, qui coupe réellement les manettes pendant le menu. Le
service tourne en permanence (lecture evdev, sans grabbe), l'overlay est
**lancé une fois puis résident** (`--resident --ipc`, ouverture par socket) et
le gate BPF n'attache son hook que pendant le menu.

Home est perçu **instantanément** parce que la surface reste mappée,
transparente et hors écran entre deux ouvertures : plus d'`exec`, plus
d'animation KWin à attendre. Mesuré : premier tampon 6 ms, panneau en place
~206 ms (contre ~470 ms par ouverture avec exec). Détail →
`gpad/AGENTS.md` (mode résident) et `trigger/AGENTS.md` (IPC).

**Deux binaires visibles** : `gpad` (l'overlay, Rust + layer-shell) et
`gpad-trigger` (le déclencheur, C). Tout le reste vit dans
`/usr/libexec/gablue-pad/` ou `/usr/bin/gpad-*`.

## Composants

| Chemin | Rôle | Installé en |
|---|---|---|
| `gpad/` | overlay (Rust, wayland-client brut + layer-shell + shm) : panneau latéral droit, menu système, réglages intégrés, **clavier virtuel** (Home+carré), deux habillages RETRO / MODERNE | `/usr/bin/gpad` |
| `gpad/assets/` | embarqués dans le binaire : `NotoSans-Regular.ttf`, `glyphes/{ps,xbox}/` | — (dans le binaire) |
| `trigger/` | déclencheur : Home → overlay, combos, menu système, rescan des manettes, inhibition d'écran | `/usr/bin/gpad-trigger` |
| `gate/` | démon root + programme BPF LSM qui coupe les lectures `hidraw`/`js` pendant le menu, et `padnodes.sh` (inventaire js/hidraw par périphérique physique) | `/usr/libexec/gablue-pad/gpad-gate` (+ `.bpf.o`, `gate/padnodes.sh`) |
| `mouse/` | mode souris/clavier (Home+R3) : **grabbe** le node evdev de la manette et **coupe** ses nodes js/hidraw par le gate, pour que le jeu ne la reçoive plus pendant qu'on pointe | `/usr/bin/gpad-mouse` |
| `login/` | écran de connexion : pointeur manette (`gpad-login`) et pont du greeter (`login-bridge.sh` : thème sombre, clavier visuel ouvert **au premier mouvement** d'une manette, jamais à son simple branchement) | `/usr/bin/gpad-login`, `/usr/libexec/gablue-pad/login-bridge.sh` |
| `inhibit/` | inhibition de l'écran tant qu'une manette est connectée ; en session console, tenue en permanence (`GPAD_INHIBIT_TOUJOURS=1`, lancée par `gablue-gaming-session`) | `/usr/bin/gpad-inhibit` |
| `rumble/` | impulsion FF_RUMBLE (confirmation de sauvegarde de replay) | `/usr/bin/gpad-rumble` |
| `shot/` | capture d'écran par appel direct à KWin (`ScreenShot2`, sans pointeur, ~40 ms) | `/usr/bin/gpad-shot` |
| `clavier/` | `gpad-clavier` : ouvre le clavier virtuel du **résident** à la demande (le prompt d'identification du launcher n'a pas de clavier en session console) | `/usr/bin/gpad-clavier` |
| `kwin/` | scripts KWin poussés à chaud (`wininfo.js`, `lister.js`, `fullscreen.js` ; le cyclage L2/R2 est **généré**) | `/usr/libexec/gablue-pad/kwin` |
| `scripts/` | lancements du menu Applications (`toggle-glauncher`, `toggle-es`, `toggle-yt`, `toggle-tv`, `toggle-jellyfin`) + `fermer-applis`, `openes` | `/usr/libexec/gablue-pad/scripts` |
| `sounds/` | `beep.wav` (départ enregistrement/replay, **sauté** avec `GPAD_SANS_BIP=1` : `grec`, `.desktop`, replay automatique), `shot.wav` (capture), `clic.wav`/`noclic.wav` (mode souris) | idem |
| `recorder.sh`, `replay.sh`, `screenshot.sh`, `vrr.sh`, `mangohud.sh`, `booster.sh` | outils appelés par l'overlay et le déclencheur (`recorder.sh`/`replay.sh` sont de purs **clients de la socket** de `gpad-rec` : plus aucun `pgrep`/`killall`/fichier d'état, l'état vient de `status`) (`screenshot.sh` : son d'abord puis `gpad-shot`, repli spectacle — voir « Capture d'écran » ; `booster.sh` : « optimiser au premier plan », ligne `PRIORITE ACTIVE` des réglages ; `mangohud.sh` : **unique** réglage MangoHud du poste, partagé avec le dialogue « MangoHud… » de Gablue Manager) | idem |
| `evgrab/` | bancs : `vpad` (manette virtuelle + signaux), `rdr`, `hidrev` | **non installé** |

`killthemall` n'est **pas** ici : il est partagé (bouton `KillEmu.desktop`) et vit
dans `/usr/share/ublue-os/gablue/scripts/killthemall`, exporté au service par
`KILLTHEMALL`.

## Services

- `gablue-pad-gate.service` (**système**, root) : charge le programme BPF une
  fois, socket `/run/gablue-pad/gate.sock`, `--timeout 0` (démon de session).
  Le hook n'est attaché qu'entre `open` et `close` : hors menu, coût nul.
- `gablue-pad.service` (**utilisateur**, session) : `gpad-trigger /usr/bin/gpad`,
  environne tout le reste (chemins, sons, gate, kwin). Le déclencheur lance
  l'overlay **une fois** en résident et l'ouvre par le socket
  `$XDG_RUNTIME_DIR/gablue/gpad-<pid>.sock`. Activé pour tous les comptes par
  l'image (`systemctl --global enable`).
- **Socket Wayland vérifié au démarrage** : `gpad-trigger` teste la connexion
  au `WAYLAND_DISPLAY` hérité ; s'il est mort, il adopte un `wayland-*` du
  dossier runtime qui accepte la connexion et l'exporte (overlay,
  `kscreen-doctor`, captures en héritent à l'`exec`). Indispensable parce que
  la session console écrit `gablue-gaming` dans le gestionnaire utilisateur
  **partagé** : un service relancé avant que Plasma ne réécrive cet
  environnement gardait le socket mort — overlay invisible et `kscreen-doctor`
  qui plante à chaque Home (constaté le 24/11/2026). Vérifié le même jour avec
  un `WAYLAND_DISPLAY=gablue-gaming` mort : « adopté wayland-1 », overlay
  résident mappé et menu visible ; avec un environnement valide, aucune
  adoption (rien n'est touché).
- **Écran du panneau** : en double écran, le résident vise l'écran **qui a le
  focus** (`org.kde.KWin.activeOutputName`) et ne recrée sa surface que si
  l'écran change — layer-shell fige l'écran à la création, donc un résident
  mappé une fois restait à côté du jeu (détail et mesures → `gpad/AGENTS.md`).
- `gablue-login-theme.service` (**utilisateur**, greeter, `Type=oneshot`) :
  `login-bridge.sh theme`, **`Before=plasma-login-kwin_wayland.service`** — le
  KWin du greeter lit le `kdeglobals` de `plasmalogin` à son démarrage, donc le
  thème doit être écrit avant lui (aucun `pkill plasma-keyboard` : rien ne
  garantit que KWin relance l'input method, la seule voie sûre est d'écrire
  avant). `WantedBy=plasma-login-wayland.target`, `RemainAfterExit=yes`.
- `gablue-login-greeter.service` (**utilisateur**, greeter, après KWin) :
  `login-bridge.sh pad`, `WantedBy=plasma-login-wayland.target`, donc les deux
  unités ne démarrent **jamais** dans une session utilisateur. Le pont lit
  `/etc/gablue/login.conf` (`pad=on|off`, `dark=on|off`) et n'ouvre le clavier
  visuel qu'**au premier vrai mouvement d'une manette** — jamais au simple
  branchement, qui affichait le clavier à l'arrivée sur l'écran de connexion
  (demande de l'utilisateur, 21/11/2026). Deux mécanismes distincts : la
  présence (evdev `ID_INPUT_JOYSTICK=1`, sondée 1×/s) ne sert qu'à lancer
  `gpad-login` ; le **mouvement** vient de `gpad-login` lui-même, seul lecteur
  des événements, qui envoie **SIGUSR1** au pont à chaque activité (appui de
  touche, stick hors zone morte, pointeur qui avance), avec un débit limité à
  2 signaux/s — un stick maintenu déplacerait sinon le pointeur sans nouvel
  événement evdev. Le pont exporte son PID (`GPAD_LOGIN_NOTIFY_PID`) et
  `gpad-login` ne signale que si ce PID est bien celui de son parent : lancé à
  la main, il ne signale personne, et un orphelin ne peut pas tuer un processus
  qui aurait récupéré le numéro. Signal plutôt que FIFO : aucun objet nouveau à
  créer dans le domaine SELinux du greeter (`mkfifo` = classe `fifo`, pas
  couverte par une règle `file`), et pas de sémantique EOF à gérer.
  **Clavier refermé après 10 s sans mouvement** (`OSK_IDLE`, compteur
  `$SECONDS`) et rouvert au geste suivant — sinon il restait affiché en
  permanence. Déclencheurs d'ouverture : appui de
  touche (`EV_KEY` valeur 1), stick **hors zone morte** (un état relu à
  l'ouverture, manette au repos, ne suffit donc pas), D-pad (`ABS_HAT0*` ≠ 0).
  Ouverture `mode 2` + `forceActivate`, fermeture **`1` puis `0`** quand la
  manette part. Les deux crans sont nécessaires, et l'ordre compte (mesuré sur
  KWin 6.8.3 le 24/11/2026) : `0` **seul ne referme pas** un panneau déjà
  mappé (le clavier restait affiché), `1` le referme, et **finir sur `0`**
  empêche KWin de le rouvrir tout seul sur un évènement tactile — le pavé
  tactile d'une DualSense branchée en USB en émet un à l'énumération, d'où le
  clavier qui s'affichait au simple branchement (signalé le 24/11/2026).
  L'ancien ordre (`0` puis `1`) refermait bien mais laissait le mode qui
  rouvre. Le `1` intermédiaire est immédiat : aucune fenêtre utile pour qu'un
  toucher le rouvre.
  `gpad-login` ignore en plus les 0,6 s qui suivent l'ouverture du node
  (`NOTIFY_SETTLE`) : certaines manettes y renvoient leur état complet (axes
  hors zone morte, pavé tactile, boutons), ce qui n'est pas un geste de
  l'utilisateur. Aucun privilège : tout se passe dans la session du greeter, ce
  qui évite les franchissements SELinux. Un verrou
  `flock` (dans `$XDG_RUNTIME_DIR`) empêche deux ponts de tourner ensemble —
  deux `gpad-login` donneraient deux pointeurs virtuels, donc un mouvement
  doublé.
- Pointeur (`gpad-login`) : stick gauche **ou** droit (le plus écarté gagne),
  **même courbe que `gpad-mouse`** (zone morte 0,08, 2 → 12, accumulateur
  sub-pixel, 120 Hz), X/Croix ou R1 = clic gauche, Carré = Retour arrière,
  Start = Entrée ; la manette n'est pas grabée et le périphérique virtuel est
  détruit à la sortie (`SIGTERM`/`SIGINT`).

Validé sur gen1 le 19/11/2026 (plasma-login-manager 6.8.3, Plasma 6.8) :
`theme` **finit** avant que `plasma-login-kwin_wayland.service` ne démarre
(journal : `Finished gablue-login-theme` puis `Starting …kwin_wayland`),
`VirtualKeyboardMode` finit à `0` (Never) après fermeture depuis le 24/11
(il restait à `1` auparavant, ce qui laissait la réouverture automatique),
aucun AVC SELinux, aucun
processus laissé derrière au logout. À revérifier après une montée de version
de plasmalogin (le mécanisme `Before=` + `kdeglobals` n'est pas un contrat
public).

Le déclenchement du clavier visuel par le **premier mouvement** (21/11/2026) a
d'abord été validé au banc — `gpad-login` et le pont réels, manette virtuelle
`vpad`, `busctl`/`udevadm` en stubs : rien au repos ni au branchement, SIGUSR1
au stick comme à l'appui de touche, aucun signal sans `GPAD_LOGIN_NOTIFY_PID`,
`mode i 2` + `forceActivate`, maintien pendant une activité régulière,
**fermeture à 10 s sans mouvement puis réouverture au geste suivant**,
`1` puis `0` au débranchement, `gpad-login` arrêté à la sortie. La fermeture a
été **mesurée sur un KWin réel** (banc uinput + `spectacle`, 24/11/2026) :
`0` seul laisse le panneau affiché, `1` le referme, `1` puis `0` le referme et
laisse le mode sûr. **Validé sur l'écran de connexion réel** (gen1,
24/11/2026, DualSense en USB), journal à l'appui : manette vue et rebranchée
sans aucun clavier, `mouvement manette → clavier visuel affiché` (`i 2`) avec
un appui réel, puis `10 s sans mouvement → clavier visuel fermé`. Mesure
complémentaire au banc : **0 signal** pendant l'énumération USB (l'état complet
relu à l'`open` est ignoré par `NOTIFY_SETTLE`), **1** au geste suivant — donc
l'ouverture ne dépend que d'un vrai mouvement.

Le démon ne garde aucune cible périmée : l'overlay envoie `clear` puis `target`
pour chaque node js/hidraw à chaque ouverture (les `dev_t` changent à la
reconnexion d'une manette, et un démon de session épuiserait ses 8 emplacements).

## Ordre des manettes : NON (décision utilisateur, 23/11/2026)

Attribuer un **numéro de joueur** (manette 1, 2…) n'est pas fait et ne sera pas
fait : il faudrait soit réécrire les événements vers des manettes **virtuelles**
(uinput), soit s'appuyer sur l'ordre d'énumération du noyau, qui n'est pas
stable d'un démarrage ni d'une reconnexion à l'autre. Les essais menés n'ont
rien donné de fiable et l'utilisateur refuse explicitement les manettes
virtuelles. Les manettes sont donc prises **telles que le noyau les présente**
(`nf` nodes evdev), sans tri : ne pas reproposer ce réglage sans un moyen
matériel ou noyau qui le rende déterministe.

## Options utilisateur (Gablue Manager)

`/usr/bin/gablue-pad` écrit `~/.config/gablue/gpad-options` (`clé=valeur`,
absente = on) et envoie SIGHUP au service, qui relit à chaud :

| Clé | Effet |
|---|---|
| `menu` | Home ouvre le panneau et le menu système (Home maintenu) |
| `shortcuts` | combos Home+bouton et Home+D-pad |
| `inhibit` | pas de verrouillage d'écran tant qu'une manette est connectée |

`gablue-pad off` reste le coupe-circuit complet (masque systemd utilisateur, il
l'emporte sur l'activation de l'image) ; un masque posé sur l'ancien service
`gablue-gamepadshortcuts` est migré une fois.

## Gestes

| Geste | Effet |
|---|---|
| Home (appui court) | ouvre le panneau **au relâchement** ; le jeu a reçu l'appui |
| Home maintenu ~1,2 s | **menu système** centré (Mise en veille / Mode bureau\|jeu / Tout fermer / Déconnecter la manette / Fermer la session / Redémarrer / Éteindre) |
| Home + gauche | bascule l'**enregistrement** (indépendant du replay) |
| Home + droite | capture d'écran |
| Home + haut | **replay** : démarre le buffer, ou **sauvegarde** s'il tourne |
| Home + bas | **arrête** le replay ; buffer arrêté → ouvre le panneau sur la ligne FPS |
| Home + L3 | **Home laissé au jeu** : ni panneau, ni menu système — pour les jeux dont le menu Home se superpose au nôtre |
| Home + SELECT / START / carré / R3 | `killthemall` / `openes` / clavier virtuel / mode souris |
| **L2 / R2** | fenêtre active précédente / suivante (ordre d'écran, ancré sur l'UUID) |
| **L1 / R1** | page précédente / suivante (contenu qui glisse, en-tête fixe) |

Pas de Home+triangle (retiré) : YouTube s'ouvre par la page Applications.
Détail des gestes et des pages : `gpad/AGENTS.md`.

**Mode souris (Home+R3)** : pendant le mode, la manette **n'appartient qu'au
pointeur**. Deux verrous, les mêmes que l'overlay quand son panneau est ouvert :
`EVIOCGRAB` sur le node evdev (les jeux SDL/Wine ne voient plus stick ni
boutons) et le **gate BPF** sur les nodes `js`/`hidraw` (le trou que le grab ne
ferme pas), que `gpad-trigger` transmet au binaire (`--gate <socket>
--gate-target <node>`, nodes de `padnodes.sh` recalculés à chaud). Sans eux, le
jeu continuait de recevoir la manette : déplacer le pointeur faisait aussi
marcher le personnage (signalé le 24/11/2026). Le binaire n'envoie **jamais**
`clear` au démon (il viderait les cibles de l'overlay) : `target` s'accumule,
le démon est idempotent, et un `open` attache le hook. À la sortie — Home+R3, `SIGTERM`/`SIGINT`/`SIGHUP`, ou manette débranchée — il
envoie **`close`** puis relâche le grab : sans ça la manette resterait coupée
pour tout le monde jusqu'au prochain `close`. Le grab est **retenté toutes les
0,5 s** tant qu'il ne l'a pas : lancé depuis le panneau, le premier essai
échoue (l'overlay le tient) et sans ce retry le mode souris restait sans grab
après la fermeture du panneau (vécu 25/11/2026 : Home ne revenait plus, le
déclencheur ne voyant plus la manette). **Home**, quand le pointeur tient le
grab, ne quitte pas le mode : il prête la manette (relâche le grab) et demande
`open` au résident via `--ipc`, puis la reprend à la fermeture du panneau. La boucle attend en `nanosleep` (interruptible par
un signal), pas en `usleep`, pour que l'arrêt soit immédiat.

**Changement de session** (`MODE JEU` sur le bureau, `MODE BUREAU` en console) :
la ligne vit dans SYSTEME, entre la veille et les sorties — elle passe par
`/usr/libexec/gablue-session-mode` (root via pkexec, règle polkit wheel), qui
arrête la session sortante, écrit l'autologin de plasmalogin puis redémarre le
gestionnaire de connexion. `FERMER LA SESSION` prend le même chemin **quand un
autologin est armé** (fichier `zz-gablue-session.conf` présent) : une simple
déconnexion relancerait la même session (plasmalogin garde `Session=` en
mémoire, bug KDE 522181). Détail et pièges → `files/system/AGENTS.md`.

**Bascule jeu ⇄ application compagne** (`BASCULER`, accueil et L2/R2 panneau
ouvert) :
elle ne se contente pas d'activer la fenêtre. Celle du jeu est **plein écran**,
donc dans une couche au-dessus des fenêtres normales — l'app restait derrière
et l'écran semblait revenir au jeu (constaté le 23/11/2026 avec Gablue TV et
Jellyfin). Le jeu est **minimisé** et son plein écran retiré en allant vers
l'app, les deux remis en revenant (`script_bascule`, testé — minimiser est
indispensable : un jeu Wine/XWayland reprenait le focus aussitôt et l'app
n'était vue qu'une fraction de seconde, 25/11/2026), `keepAbove` levé au
passage, et
l'activation part dans le **même** script KWin (deux chargements se
marcheraient dessus). Un seul jeu à la fois : c'est le modèle du launcher
(`running: Option<String>`), il n'y a donc jamais de choix entre deux jeux.

**Le son suit la fenêtre** : en allant vers l'app, les flux du jeu passent en
**sourdine** (`pactl set-sink-input-mute`, tous ceux dont le PID est celui de la
fenêtre ou un descendant) ; au retour ils sont rendus et les deux s'entendent.
Muet et pas volume à 0 : le jeu garde son réglage. Les applications compagnes
sont exclues du filtre par leur nom (`est_compagne`) — leur son ne doit jamais
être coupé par la bascule, même si leur flux est lancé par un enfant du jeu. Si
l'app est fermée alors que le jeu est en sourdine, `refresh_bascule` rend le son
sans attendre un geste : sans ça le jeu restait muet, le sélecteur n'existant
que tant qu'une app est ouverte.

**Home+L2 / Home+R2 hors panneau** (déclencheur → `bascule <sens>` sur l'IPC du
résident) : même bascule sans ouvrir le menu — R2 vers l'app, L2 vers le jeu.
Au bureau (aucun jeu publié) les deux cyclent **toutes** les fenêtres dans
l'ordre de la pile, R2 en avant, L2 en arrière (`CYCLE_JS`, même ancre que le
cyclage du panneau). Deux pièges trouvés en le testant le 23/11/2026 :
(1) pas de garde « déjà sur la cible » — une app lancée pendant une partie est
*active* sous la fenêtre plein écran, donc la garde refusait la bascule qui
sert justement à révéler l'app ; la direction s'applique toujours, rejouée elle
est idempotente ; (2) la fenêtre du jeu se cherche parmi **tous** les jeux
publiés (`running.txt`), pas via `jeu_actif()` qui ne désigne que le jeu de la
fenêtre active — app au premier plan = aucun jeu reconnu, et la bascule cyclait
les fenêtres au lieu de passer à l'app.

**Bascule en tête de l'accueil** : quand un jeu tourne **et** qu'une app
compagne est ouverte, `BASCULER` est le **premier geste** de l'accueil, juste
après le nom de la fenêtre et ses trois volumes au bureau ; en **console** il
suit `FERMER` et précède les volumes (25/11/2026) —
il n'y a plus de pages `JEU` / `FENETRES` : la ligne vit dans
`accueil_items(dispo)` et `refresh_bascule` est relu après la première image
(à l'ouverture), jamais avant : `lister.js` prend jusqu'à 400 ms. Sans jeu
mais avec une app compagne, la bascule va et vient entre l'app et
**glauncher** — pas de cyclage de fenêtres au hasard.

**Captures d'écran : format et qualité** (page CAPTURE, `FORMAT PHOTO` /
`QUALITE PHOTO`) sont écrits dans `~/.config/gablue/recorder.conf`
(`shot_format` = `jpeg|png`, `shot_qualite` = `basse|moyenne|haute`) et lus par
`gpad-shot` (C) : `jpeg` → 92/82/72 avec sous-échantillonnage 4:2:0, `png` →
compression 9/6/3. Défaut **JPEG / haute** : une capture PNG 1440p pesait
plusieurs Mo (signalé le 23/11/2026), le JPEG la ramène à quelques centaines de
Ko sans perte visible. Le même fichier alimente le dialogue de Gablue Manager,
aucune seconde source.

**Réveil par Home** : une manette Bluetooth éteinte avec la machine en veille
la rallume. C'est du système, pas de l'overlay — clé Bluetooth en réveil USB
plus `WakeAllowed` posé sur les manettes appariées par
`gablue-pad-wake.service` (détail et mesures → `files/system/AGENTS.md`). La
manette se reconnecte ensuite seule, sans redémarrage : l'overlay retrouve son
événement evdev au même titre qu'une manette rebranchée.

Le changement de session fait aussi **redémarrer** `gablue-pad.service` côté
console : l'unité vit dans le gestionnaire utilisateur, partagée entre
sessions ; un `start` sur une unité déjà `active` ne rafraîchit pas son
`WAYLAND_DISPLAY` et l'overlay resterait sur un compositeur mort (constaté le
23/11/2026).

## Enregistreur et replay

L'overlay est un **client de la socket** de `gpad-rec` (`capture::etat()`) : il
n'imagine jamais l'état, il lit `recording`/`replay`/`pending` au moment de
dessiner. Les gestes (accueil, donc à un appui d'ouverture) et les réglages
(catégorie `CAPTURE`) :

| Page | Ligne | Effet |
|---|---|---|
| ACCUEIL | `ENREGISTRER UNE VIDEO` | `recorder.sh start`/`stop` (bascule, même ligne quand ça tourne) |
| ACCUEIL | `REPLAY` | interrupteur du buffer (`replay.sh start`/`stop`), usable pendant une capture |
| ACCUEIL | `SAUVER REPLAY` | `replay.sh save` ; la ligne **n'existe pas** quand le buffer est arrêté (elle apparaît dès qu'il tourne) |
| CAPTURE | `QUALITE`, `CODEC`, `RESOLUTION`, `FPS REC`, `CURSEUR`, `FORMAT PHOTO`, `QUALITE PHOTO` | les réglages de l'enregistreur et des captures d'écran |
| CAPTURE | `DUREE REPLAY`, `REPLAY AUTO` | profondeur du buffer et son démarrage automatique |

- **Défauts choisis pour que le fichier s'ouvre partout** : H.264 (compatibilité
  seule), résolution `SOURCE` (natif), qualité 3, 60 fps, curseur masqué —
  même si H.265 pèse 28 % de moins. `720P`/`1080P`/`1440P` sont des crans
  explicites pour un envoi léger (détail et barème →
  `../gablue-rec/AGENTS.md`).
- **Pas de bouton « Partager »** : Home+haut sauve le replay quand le buffer
  tourne (sinon il le démarre), ou la ligne `SAUVER REPLAY` du panneau. Pas de
  notification à la sauvegarde (elle entrerait dans le clip suivant), le rumble
  suffit.
- **Pas de bip** quand le replay démarre tout seul (`GPAD_SANS_BIP=1` posé par
  `grec`/le lancement automatique) : le son ne confirme qu'un geste manette.
- **Lignes compactes + défilement** : la catégorie `CAPTURE` porte neuf lignes
  hautes (les actions sont sur l'accueil depuis le 24/11/2026) ; `hauteur()` les
  resserre et `reg_top` fait défiler avec une barre de position quand ça dépasse
  (~sous 1100 px de haut, vu avec `GPAD_TEST_BUDGET`). La sélection reste
  visible en défilant. L'accueil lui-même tient en 1080p (mesuré le 24/11/2026,
  1920×1080 : seize lignes, pied de page visible, aucun défilement ; quatorze
  depuis le retrait de `FERMER` et `PLEIN ECRAN`, le 25/11/2026).

## Clavier virtuel à la demande (`gpad-clavier`)

Le lanceur de jeux demande parfois une saisie (nom du jeu chez SteamGridDB,
date de sortie) ; en session console il n'y a pas de clavier physique. Le
launcher lance `gpad-clavier`, qui **n'ouvre pas un second overlay** : il envoie
`kbd <node>` au résident déjà lancé par `gpad-trigger` (socket
`$XDG_RUNTIME_DIR/gablue/gpad-<pid>.sock`, le plus récent). Le clavier tape par
uinput dans la fenêtre qui a le focus — donc dans le champ de saisie. Node
trouvé comme `padnodes.sh` (bloc de `/proc/bus/input/devices` avec un handler
`jsN`), `PAD_NODE` prioritaire. Aucun socket (service arrêté) : repli
`gpad --clavier <node>` du déclencheur. `connect` borné à 500 ms, comme le
déclencheur : un résident bloqué ne fige pas le launcher.

## Capture d'écran (`gpad-shot`)

Home + droite → `screenshot.sh` → `gpad-shot` : appel direct à
`org.kde.KWin.ScreenShot2` + écriture PNG, sans Spectacle. Mesuré sur cette
machine (2560×1440, Plasma 6.8.3) : **~40 ms** (`dbus 4-10`, transfert `4`,
PNG `18`) contre **~350 ms** pour `spectacle -f -b -n -o` (client Qt + son
unité systemd + deflate plus coûteux). Aucun paquet nouveau au runtime :
`libdbus` (déjà là pour `gpad-inhibit`) et `zlib` (`zlib-devel` au build).

- **Le pointeur n'est jamais dans l'image** : `include-cursor: false` est
  envoyé explicitement. Vérifié en A/B sur deux captures consécutives : la
  seule différence entre `true` et `false` est un blob de **14×21 px** au
  pointeur (1,6·10⁻⁵ des pixels), tout le reste est identique au bit près.
- **Un écran, pas le bureau** : `CaptureActiveScreen` (celui de la fenêtre
  active, donc du jeu), repli `CaptureWorkspace` quand il n'y a aucune fenêtre
  active. `native-resolution: true`, comme Spectacle — la résolution physique
  est conservée sur un écran HiDPI.
- **Parité mesurée** avec Spectacle sur captures quasi simultanées :
  **PSNR 55,6 dB** (écarts = horloge et animations), fichiers comparables
  (1,0-1,3 Mo contre 0,99-1,4 Mo : filtre 0 + deflate niveau 1, ~4× plus
  rapide à encoder pour ~30 % de volume en plus).
- **Le son part avant la capture** (`setsid ffplay … &` puis capture) : le
  déclencheur est entendu en ~60 ms. Avant, il arrivait après la capture et le
  démarrage de `ffplay` (~0,4 s en tout).
- **Repli** : `gpad-shot` absent ou en échec (compositeur non-KWin, interface
  disparue) → `spectacle -f -b -n -o`, vérifié au banc avec
  `GPAD_SHOT=/bin/false`. `GPAD_SHOT` est exporté par l'unité, prioritaire ;
  au banc le binaire est trouvé à côté du script.
- **Dossier de sortie** : `dossier_screenshot` de
  `~/.config/gablue/recorder.conf` (même fichier que l'enregistreur),
  réglable depuis les Réglages du launcher et Gablue Manager ; clé absente ou
  vide = `~/Images/Screenshots`. `SCREENSHOT_DIR` reste prioritaire (bancs).
- **Nom du jeu** : `screenshot.sh` passe `--titre=<nom>` (variable
  `GPAD_SHOT_TITRE` acceptée) → `gpad-shot` écrit un `iTXt Title` (UTF-8, pas
  `tEXt` Latin-1) **dans le PNG**, jamais dans les pixels, et pose un sidecar
  `<fichier>.jeu` (`nom=`, `logo=`). `recorder.sh` et `replay.sh` posent le
  même sidecar sur leurs clips finalisés. La galerie du launcher s'en sert
  pour la légende et la copie avec logo ; les PID de `running.txt` servent de
  garde (fichier orphelin après un plantage = aucune étiquette).

Trois pièges payés, à ne pas réintroduire :

- **interblocage du tube** : KWin écrit la frame **entière** dans le tube
  *avant* de répondre ; le tube fait 64 Kio, donc sans lecteur concurrent les
  deux côtés se bloquent (constaté : appel muet, aucun message) ;
- **l'EOF du tube n'est pas fiable** (la bibliothèque D-Bus garde une extrémité
  ouverte : sous Gio, `read` bloquait à vie après les 14,7 Mo) → on s'arrête sur
  `stride × height` annoncés dans la réponse, avec un délai de garde (5 s pour
  l'appel D-Bus, 10 s pour la frame) ;
- **`format` est un `QImage::Format`** : 16 = `RGBX8888` (octets R,V,B,X),
  4/5 = ARGB32/BGRA, 13 = RGB888. Traiter 16 comme du BGRA échange rouge et
  bleu — et une frame tronquée doit faire échouer la commande, jamais produire
  un PNG partiel (un prototype Python sans contrôle de taille l'a fait).

## Build

- **Tout répertoire compilé par le `Makefile` doit avoir son `COPY` dans le
  `Containerfile-gablue`** (copies explicites composant par composant : `gpad/`
  arrive par `COPY --from=gpad-build`, `evgrab/` ne doit jamais entrer).
  Deux builds ont cassé sur cet oubli : `kbd/` supprimé sans retirer son `COPY`
  (21/11/2026), puis `shot/` ajouté sans `COPY` — `make: *** No rule to make
  target 'shot/gpad-shot.c'`, cinq images rouges. Dans `build-c`, ce message
  signifie une source absente de la couche, jamais un défaut du `Makefile`.
- Binaires C : `make` dans ce dossier (`gpad-trigger`, `gpad-mouse`,
  `gpad-inhibit`, `gpad-rumble`, `gpad-login`, `gpad-shot`) ; `make install DESTDIR=…` pose aussi
  l'arborescence de données dans `/usr/libexec/gablue-pad` (les scripts y
  résolvent `sounds/`, `scripts/` et `kwin/` par rapport à leur propre dossier).
- Overlay Rust : étape `gpad-build` du `Containerfile-gablue`
  (`files/scripts/build-gpad`, qui installe `libxkbcommon-devel` pour la crate
  `xkbcommon` du clavier), binaire récupéré par `COPY --from`.
- Gate BPF : étape `gpad-gate-build` (`files/scripts/build-gpad-gate`),
  `vmlinux.h` **épinglé** dans `gate/` (dump BTF du noyau OGC : ne jamais le
  régénérer depuis la machine de build, le layout d'inode différerait).
- Côté image : `files/scripts/build-c` compile et installe, `files/scripts/systemd`
  active les deux unités, `files/scripts/post-install` pose les `+x`.

## Banc (hors image)

`./run-gpad.sh` monte la chaîne complète (gate en `pkexec`, `gpad-trigger`,
overlay) avec les mêmes variables d'environnement que le service ; `vpad` sert
de manette virtuelle (signaux : `evgrab/AGENTS.md`). Le banc garde les chemins
relatifs au binaire, l'image passe les chemins absolus par l'environnement.
`./test-logo.sh` est le banc du **logo du jeu** : il publie un `running.txt`
factice dans le cache de glauncher (fenêtre du terminal + un logo du cache, ou
`--pid` d'un vrai jeu), ou suit la publication réelle (`--jeu`) — celle de
glauncher **ou** celle de gwine/lgplaunch, qui écrivent `nom=` / `paquet=` /
`pids=` dans `$XDG_RUNTIME_DIR/gablue/running.txt`. Il ouvre l'overlay **sans
grab** pendant 12 s (`--duree`) avec
`KWIN_SCRIPTS` — sans cette variable `chemin_kwin` ne trouve pas `wininfo.js`
depuis le binaire du cache cargo, donc aucune fenêtre active — et remet le
fichier d'origine à la sortie, jamais celui d'un jeu en cours (`--force` seul
l'autorise). `--reel` compile les deux binaires et donne la marche à suivre
avec un vrai jeu.

## Documentation par domaine

| Domaine | Document |
|---|---|
| Overlay : pages, réglages, rendu/styles, fenêtre active, CLI, fade | `gpad/AGENTS.md` |
| Déclencheur : Home, combos, menu système, BT, options, gate | `trigger/AGENTS.md` |
| Gate BPF : hook LSM, protocole du démon, cibles, pistes écartées | `gate/AGENTS.md` |
| Retour haptique | `rumble/AGENTS.md` |
| Bancs et invariants de test | `evgrab/AGENTS.md` |
