# evgrab — bancs reproductibles (manette virtuelle, aucun matériel)

`vpad` (manette virtuelle uinput pilotée par signaux), `rdr` (lecteur evdev court,
option `-g` pour tester `EVIOCGRAB`), `hidrev` (lecture `hidraw`), `hidloop`.
Tous les bancs qui touchent la manette passent par ici : voir les invariants
ci-dessous **avant** le premier lancement.

## `vpad` : signaux

Manette virtuelle pilotée par signaux (aucun matériel requis) :
`evgrab/vpad` + signaux : `SIGUSR1`=A, `SIGUSR2`=bas, `SIGHUP`=Home,
`SIGALRM`=B, `SIGRTMIN+1`=Home+gauche, `+2`=Home+droite, `+3/+4/+5/+6`=stick
gauche/droite/haut/bas, `+7/+8`=Home+haut/bas, `+14`=L1, `+15`=R1,
`+20/+21`=Home appui/relâche (pour tester un **maintien**), `+22`=**un seul pas
vers le haut** (`BTN_NORTH`), `+23`=**Home+L3** (`BTN_THUMBL`). Le stick (`+5`)
émet deux pas d'un coup, ce qui
interdit de se poser sur l'en-tête : utiliser `+22` pour la navigation fine. `VPAD_NOM=<nom>` change le nom du
périphérique : indispensable quand le `gpad-trigger` de l'utilisateur tourne, son
scan filtrant les noms contenant « gamepad »/« controller » — un nom neutre
(`bench net test`) le rend aveugle à la manette de test, sinon il lance **ses**
overlays sur les mêmes appuis et la manette est grabée par une autre instance.

## Invariants (violés une fois = machine gelée ou fichier perdu)

- **Une seule instance** de `gpad-trigger` / overlay / démon de gate ; `timeout`
  et `trap` de nettoyage dans tout script de test ; vérifier `pgrep` à la fin.
- L'enregistreur et le buffer (une instance, arrêt par `killall -SIGINT`) :
  `AGENTS.md` racine.
- **Jamais `pgrep -f gpu-screen-recorder`** : le motif matche le shell appelant
  (il s'est tué lui-même). Utiliser `pgrep -f '^gpu-screen-recorder '` ou
  `killall`.
- **Ne jamais gater evdev** : `gamepadshortcuts` ne teste pas les erreurs de
  `read()` → 98 % d'un cœur et 13,5 M refus en 3 s (mesuré). evdev = `EVIOCGRAB`.
- **`inode->i_rdev` est un `dev_t` noyau** (`major<<20 | minor`), pas
  `st_rdev` : sans conversion, aucune comparaison ne matche (13:80 vs 0xD00050).
- Pendant un test, l'arbre de manettes virtuelle n'est lisible qu'après l'ACL
  udev : attendre `[[ -r $node ]]` avant de grabber, sinon `Permission denied`.
- **Un `gpad-trigger` par scénario** : il survit à son overlay (il attend le Home
  suivant) — sans le tuer, deux scénarios écrivent dans le même journal et
  plusieurs overlays se disputent le grab (`EBUSY`).
- **Tester souris avec un stub** (`MOUSE`), jamais le binaire réel :
  `gpad-mouse` grabbe la manette et déplace le pointeur. Le `comm` du stub doit
  être le nom attendu (binaire, pas script) pour que la détection d'état
  fonctionne.
- **Le clavier virtuel ne se teste PAS en direct chez l'utilisateur** : il
  grabbe une manette et affiche une surface plein écran sur son bureau. Passer
  `--clavier <node de vpad>` (jamais `--grab`, qui prend la première manette
  venue — donc la vraie), et jamais pendant qu'il travaille.
- **Restaurer le son après un test de volume/muet** : relire `pactl` avant,
  remettre volume **et** mute à l'état initial (un test a déjà laissé la sortie
  muette en silence).
- **Banc `vpad` : chaque axe ET chaque touche utilisés doivent être déclarés**
  (`UI_ABS_SETUP`, `UI_SET_KEYBIT`). `ABS_Y`, `BTN_SELECT` et `BTN_NORTH`
  manquaient : le noyau jetait les émissions **sans erreur**, donc le stick
  gauche/droite marchait, le haut/bas et les combos étaient muets au banc —
  même classe de panne que `ABS_X` non défini en Rust (binding silencieux).
- **Tester les combos avec des stubs** : `RECORDER`/`SCREENSHOT`/`KILLTHEMALL`/
  `SCRIPTS` pointés vers des scripts qui journalisent, `PAD_SONY=1` pour
  l'affichage.
- **Jamais lancer les vraies applications depuis un test** (`flatpak run`,
  `tvqt`, `esde.appimage`, `killthemall`) : `SCRIPTS` vers des stubs, et
  `TVQT`/`ESDE` surchargés. Un stub de `pgrep` doit **échouer** (exit 1) quand
  il ne trouve rien — un stub toujours « OK » fait prendre la branche fermeture
  et donne un faux positif (payé une fois).
- **Ne jamais lancer les vrais `killthemall`/`openes`/`launchyt` en test** :
  `killthemall` tue tous les processus `.exe`/`.bat` de l'utilisateur. Pour
  tester les combos, copier `gpad-trigger` dans un bac à sable
  (`/tmp/opencode/pw/bin/gpad-trigger` + `../scripts/` stubs) : il déduit ses
  chemins de son propre exécutable.
- **Aucun test ne déconnecte une vraie manette** : `PAD_BT_MAC` +
  `BLUETOOTHCTL` stub prouvent la détection et l'appel (`bluetoothctl
  disconnect a4:ae:…`) sans toucher au matériel de l'utilisateur.
