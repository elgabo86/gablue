# gablue-pad — overlay manette façon Steam Deck

**But** : remplacer `gamepadshortcuts` et ses scripts par un overlay unique,
piloté à la manette, qui coupe réellement les manettes pendant le menu. Le
service tourne en permanence (lecture evdev, sans grabbe), l'overlay ne s'ouvre
qu'à la demande et le gate BPF n'attache son hook que pendant le menu.

**Deux binaires visibles** : `gpad` (l'overlay, Rust + layer-shell) et
`gpad-trigger` (le déclencheur, C). Tout le reste vit dans
`/usr/libexec/gablue-pad/` ou `/usr/bin/gpad-*`.

## Composants

| Chemin | Rôle | Installé en |
|---|---|---|
| `gpad/` | overlay (Rust, wayland-client brut + layer-shell + shm) : panneau latéral droit, menu système, réglages intégrés, deux habillages RETRO / MODERNE | `/usr/bin/gpad` |
| `gpad/assets/` | embarqués dans le binaire : `NotoSans-Regular.ttf`, `glyphes/{ps,xbox}/` | — (dans le binaire) |
| `trigger/` | déclencheur : Home → overlay, combos, menu système, rescan des manettes, inhibition d'écran | `/usr/bin/gpad-trigger` |
| `gate/` | démon root + programme BPF LSM qui coupe les lectures `hidraw`/`js` pendant le menu, et `padnodes.sh` (inventaire js/hidraw par périphérique physique) | `/usr/libexec/gablue-pad/gpad-gate` (+ `.bpf.o`, `gate/padnodes.sh`) |
| `kbd/` | clavier virtuel KWin (Home+carré) | `/usr/bin/gpad-kbd` |
| `mouse/` | mode souris/clavier (Home+R3) | `/usr/bin/gpad-mouse` |
| `inhibit/` | inhibition de l'écran tant qu'une manette est connectée | `/usr/bin/gpad-inhibit` |
| `rumble/` | impulsion FF_RUMBLE (confirmation de sauvegarde de replay) | `/usr/bin/gpad-rumble` |
| `kwin/` | scripts KWin poussés à chaud (`wininfo.js`, `lister.js`, `fullscreen.js` ; le cyclage L2/R2 est **généré**) | `/usr/libexec/gablue-pad/kwin` |
| `scripts/` | lancements du menu Applications (`toggle-es`, `toggle-yt`, `toggle-tv`, `toggle-jellyfin`) + `fermer-applis`, `openes` | `/usr/libexec/gablue-pad/scripts` |
| `sounds/` | `beep.wav` (départ enregistrement/replay), `shot.wav` (capture), `clic.wav`/`noclic.wav` (mode souris) | idem |
| `recorder.sh`, `replay.sh`, `screenshot.sh`, `vrr.sh`, `mangohud.sh`, `booster.sh` | outils appelés par l'overlay et le déclencheur (`booster.sh` : « optimiser au premier plan », ligne `PRIORITE ACTIVE` des réglages ; `mangohud.sh` : **unique** réglage MangoHud du poste, partagé avec le dialogue « MangoHud… » de Gablue Manager) | idem |
| `evgrab/` | bancs : `vpad` (manette virtuelle + signaux), `rdr`, `hidrev` | **non installé** |

`killthemall` n'est **pas** ici : il est partagé (bouton `KillEmu.desktop`) et vit
dans `/usr/share/ublue-os/gablue/scripts/killthemall`, exporté au service par
`KILLTHEMALL`.

## Services

- `gablue-pad-gate.service` (**système**, root) : charge le programme BPF une
  fois, socket `/run/gablue-pad/gate.sock`, `--timeout 0` (démon de session).
  Le hook n'est attaché qu'entre `open` et `close` : hors menu, coût nul.
- `gablue-pad.service` (**utilisateur**, session) : `gpad-trigger /usr/bin/gpad`,
  environne tout le reste (chemins, sons, gate, kwin). Activé pour tous les
  comptes par l'image (`systemctl --global enable`).

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

## Build

- Binaires C : `make` dans ce dossier (`gpad-trigger`, `gpad-kbd`, `gpad-mouse`,
  `gpad-inhibit`, `gpad-rumble`) ; `make install DESTDIR=…` pose aussi
  l'arborescence de données dans `/usr/libexec/gablue-pad` (les scripts y
  résolvent `sounds/`, `scripts/` et `kwin/` par rapport à leur propre dossier).
- Overlay Rust : étape `gpad-build` du `Containerfile-gablue`
  (`files/scripts/build-gpad`), binaire récupéré par `COPY --from`.
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

## Documentation par domaine

| Domaine | Document |
|---|---|
| Overlay : pages, réglages, rendu/styles, fenêtre active, CLI, fade | `gpad/AGENTS.md` |
| Déclencheur : Home, combos, menu système, BT, options, gate | `trigger/AGENTS.md` |
| Gate BPF : hook LSM, protocole du démon, cibles, pistes écartées | `gate/AGENTS.md` |
| Retour haptique | `rumble/AGENTS.md` |
| Bancs et invariants de test | `evgrab/AGENTS.md` |
