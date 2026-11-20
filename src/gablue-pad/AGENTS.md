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
| `mouse/` | mode souris/clavier (Home+R3) | `/usr/bin/gpad-mouse` |
| `login/` | écran de connexion : pointeur manette (`gpad-login`) et pont du greeter (`login-bridge.sh` : thème sombre, clavier visuel affiché seulement si une manette est présente) | `/usr/bin/gpad-login`, `/usr/libexec/gablue-pad/login-bridge.sh` |
| `inhibit/` | inhibition de l'écran tant qu'une manette est connectée | `/usr/bin/gpad-inhibit` |
| `rumble/` | impulsion FF_RUMBLE (confirmation de sauvegarde de replay) | `/usr/bin/gpad-rumble` |
| `shot/` | capture d'écran par appel direct à KWin (`ScreenShot2`, sans pointeur, ~40 ms) | `/usr/bin/gpad-shot` |
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
  visuel que **tant qu'une manette est présente** : fermeture franche
  `mode 0 → 1` (`kwinrc` persiste le mode, ne jamais le laisser à `2`),
  ouverture `mode 2` + `forceActivate` dès qu'un evdev `ID_INPUT_JOYSTICK=1`
  apparaît, refermeture quand elle part. Aucun privilège : tout se passe dans la
  session du greeter, ce qui évite les franchissements SELinux. Un verrou
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
(journal : `Finished gablue-login-theme` puis `Starting …kwin_wayland`), le
clavier visuel s'ouvre et se referme au branchement/débranchement de la manette
(`mode i 2 … visible=true`), `VirtualKeyboardMode` reste à `1`, aucun AVC
SELinux, aucun processus laissé derrière au logout. À revérifier après une
montée de version de plasmalogin (le mécanisme `Before=` + `kdeglobals` n'est
pas un contrat public).

Le démon ne garde aucune cible périmée : l'overlay envoie `clear` puis `target`
pour chaque node js/hidraw à chaque ouverture (les `dev_t` changent à la
reconnexion d'une manette, et un démon de session épuiserait ses 8 emplacements).

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
| Home maintenu ~1,2 s | **menu système** centré (Tout fermer / Déconnecter / Redémarrer / Éteindre) |
| Home + gauche / droite / haut / bas | replay ou enregistrement / capture / bascule replay / panneau sur la ligne FPS |
| Home + SELECT / START / carré / R3 | `killthemall` / `openes` / clavier virtuel / mode souris |
| **L2 / R2** | fenêtre active précédente / suivante (ordre d'écran, ancré sur l'UUID) |
| **L1 / R1** | page précédente / suivante (contenu qui glisse, en-tête fixe) |

Pas de Home+triangle (retiré) : YouTube s'ouvre par la page Applications.
Détail des gestes et des pages : `gpad/AGENTS.md`.

## Enregistreur et replay

L'overlay est un **client de la socket** de `gpad-rec` (`capture::etat()`) : il
n'imagine jamais l'état, il lit `recording`/`replay`/`pending` au moment de
dessiner. Trois lignes d'action (page ACTIONS) et une section de réglages :

| Page | Ligne | Effet |
|---|---|---|
| ACTIONS | `ENREGISTRER UNE VIDEO` | `recorder.sh start`/`stop` (bascule, même ligne quand ça tourne) |
| ACTIONS | `REPLAY` | `replay.sh start`/`save`/`stop` selon l'état |
| RÉGLAGES | `CAPTURE` | Qualité, Codec, Résolution, FPS REC, Curseur |
| RÉGLAGES | `REPLAY AUTO` | interrupteur `auto_replay` (défaut **off**), écrit `recorder.conf` |

- **Défauts choisis pour que le fichier s'ouvre partout** : H.264 (compatibilité
  seule), résolution `SOURCE` (natif), qualité 3, 60 fps, curseur masqué —
  même si H.265 pèse 28 % de moins. `720P`/`1080P`/`1440P` sont des crans
  explicites pour un envoi léger (détail et barème →
  `../gablue-rec/AGENTS.md`).
- **Pas de bouton « Partager »** : le geste existant (Home+gauche, ou la ligne
  du menu quand le buffer tourne) sauve le replay. Pas de notification à la
  sauvegarde (elle entrerait dans le clip suivant), le rumble suffit.
- **Pas de bip** quand le replay démarre tout seul (`GPAD_SANS_BIP=1` posé par
  `grec`/le lancement automatique) : le son ne confirme qu'un geste manette.
- **Lignes compactes + défilement** : la page RÉGLAGES a maintenant sept lignes
  hautes ; `hauteur()` les resserre et `reg_top` fait défiler avec une barre de
  position quand ça dépasse (~sous 1200 px de haut, vu avec
  `GPAD_TEST_BUDGET`). La sélection reste visible en défilant.

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

- Binaires C : `make` dans ce dossier (`gpad-trigger`, `gpad-mouse`,
  `gpad-inhibit`, `gpad-rumble`, `gpad-login`, `gpad-shot`) ; `make install DESTDIR=…` pose aussi
  l'arborescence de données dans `/usr/libexec/gablue-pad` (les scripts y
  résolvent `sounds/`, `scripts/` et `kwin/` par rapport à leur propre dossier).
- Overlay Rust : étape `gpad-build` du `Containerfile-gablue`
  (`files/scripts/build-gpad`, qui installe `xkbcommon-devel` pour la crate
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
