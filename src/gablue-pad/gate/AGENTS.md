# gate — couper réellement les manettes (BPF LSM)

`gate/` contient le démon **root** et le programme **BPF LSM** qui refuse les
lectures `hidraw`/`js` pendant que le panneau est ouvert — le trou que
`EVIOCGRAB` ne ferme pas (evdev seulement). L'overlay ouvre la gate à
l'affichage et la referme à la fermeture (`--gate <sock>`) ; le démon tourne
depuis le boot (`gablue-pad-gate.service`, `--timeout 0`) et `padnodes.sh` donne
à `gpad-trigger` les nodes js/hidraw de chaque manette physique.

## Protocole du démon (socket unix, une commande par connexion)

| Commande | Effet |
|---|---|
| `target <node>` | résout le node en `dev_t` noyau et l'ajoute aux cibles — **idempotent** |
| `clear` | vide la liste des cibles : l'overlay l'envoie à chaque ouverture, sinon un démon de session épuise ses 8 emplacements au fil des reconnexions |
| `exempt <pid>` | autorise le panneau à continuer à lire |
| `open` / `close` | attache / détache le hook (coût nul tant qu'il est détaché) |
| `status` / `quit` | état (cibles, exemptions, refus) / sortie |

**Aucune réponse sur la socket** : les comptes rendus (`STATUS`, `ADD`, `GATE
open`…) partent sur la sortie du démon, donc dans journald. Un client qui
attend un `recv()` attend indéfiniment — c'est un protocole de commande, pas
d'appel.

**Plusieurs clients à la fois** (`MAX_CLIENTS` 8, `CLIENT_IDLE` 2 s) : la
première version servait un seul client et ne relisait la socket d'écoute que
lorsqu'il n'y en avait aucun. Un client qui se connectait sans envoyer sa
commande (ou qui restait ouvert) bloquait donc le démon, la file du noyau se
remplissait et les clients **suivants** restaient plantés dans `connect()` :
l'overlay se figeait, `EVIOCGRAB` en main, et Home ne répondait plus (vécu
18/11/2026, diagnostiqué en connectant un client muet sur le démon en service).
L'écoute est désormais relue à **chaque tour**, une connexion est fermée après
sa commande et une connexion muette est libérée au bout de 2 s.

Le socket est en 0666 : sur un poste personnel, tout compte de la session doit
pouvoir ouvrir/fermer la gate (aucune escalade de privilège, un autre compte
local pourrait seulement couper les lectures d'une manette).

## Écarté, et pourquoi (ne pas y revenir sans preuve contraire)

| Piste | Pourquoi c'est non |
|---|---|
| `EVIOCGRAB` seul | couvre evdev seulement (390 évt pour le grabbeur, 0 pour les autres) ; `joydev` et `hidraw` passent encore |
| `chmod`/`MODE 000`, `HIDIOCREVOKE`, `EVIOCREVOKE` | ne coupent pas un fd **déjà** ouvert (mesuré) ; `revoke()` BSD n'existe pas sous Linux |
| `unbind` du pilote `playstation` | coupe tout le monde, **y compris l'overlay** |
| Sandbox `bwrap` (essai écarté, cf. banc) | marche vraiment (22 évt menu fermé → **0** → 40 après) mais impose de lancer chaque jeu par le sandbox : refusé (solution par jeu) |
| Proxy / pad virtuel pour les jeux (`padgated`) | latence ajoutée et perte du touchpad/gyro/barrette : refusé |
| **Gate BPF LSM** | retenu : coupe un fd déjà ouvert, sans proxy, sans rebind, coût nul hors menu |

## Mesuré (gate et evdev)

| Mesure | Valeur |
|---|---|
| lecteur ayant déjà le fd (hidraw) | refusé (`EAGAIN`), OK après `close` |
| jeu lisant en continu | `+250/s` → **0** pendant le panneau → `+250` |
| coût hors menu du gate | **nul** (programme non attaché) ; menu ouvert : +23 ns/read |
| démon | 3 Mo RSS, 0 tick CPU au repos |
