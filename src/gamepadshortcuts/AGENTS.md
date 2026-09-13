# Instructions pour les agents - kbdnav

## Rôle

Pont manette → clavier virtuel Plasma Keyboard (binaire `/usr/bin/kbdnav`, ~25 Ko).
Permet de taper dans n'importe quel champ texte avec la manette de jeu :
navigation au D-pad/stick dans le clavier virtuel KDE officiel, validation
des touches, backspace/espace/Tab. Lancé par `gamepadshortcuts` via la
combo **Home+Carré** (l'instance est tracked via `check_kbdnav()`).

## Architecture générale

Le clavier virtuel KDE officiel est **plasma-keyboard** (input method
Wayland, successeur de Maliit depuis Plasma 6.5/6.6, défaut Fedora 44).
Son KCM propose une option **"Keyboard navigation"** (créée pour
Bigscreen/accessibilité) : le clavier virtuel se pilote alors aux
touches flèches + Entrée. Le pont exploite exactement ce mécanisme :

```
Manette (evdev)
    │  EVIOCGRAB (exclusif)
    ▼
kbdnav ──uinput──► touches flèches/Entrée/Echap/etc.
    │                            │
    │                            ▼
    │                     KWin (grab clavier de l'IM)
    │                            │
    │                            ▼
    └── D-Bus ──► plasma-keyboard (navigation OSK) ──► commit texte
         (active/visible/mode)
```

La manette est **grabée en evdev** (`EVIOCGRAB`) tant que le pont tourne :
aucun autre lecteur evdev ne reçoit le pad (jeux SDL sans hidapi, ds2xbox,
gamepadshortcuts lui-même — c'est voulu, la frappe a priorité).

### Cycle de vie (100 % auto-porté)

gamepadshortcuts lance juste le binaire. Le pont gère tout :

1. **Démarrage** : grab evdev + affichage du clavier
   (`mode AnyInput (2)` + `forceActivate` via D-Bus KWin)
2. **Frappe** : injection uinput
3. **Sortie** (Home+Carré, Home+R3 → mode souris, fermeture via l'UI du
   clavier, déconnexion manette, SIGTERM) : masquage du clavier
   (`setMode Never (0)` puis `setMode NonMouseInput (1)`, cf.
   ci-dessous) + **kill -9 de plasma-keyboard** + tout relâché

### Pourquoi la séquence de sortie Never (0) → NonMouseInput (1) et le kill

- En mode `NonMouseInput` (1, défaut), KWin refuse d'afficher le panneau
  au clavier/souris (`shouldShowOnActive()` n'accepte que touch/tablet) —
  `forceActivate` seul est donc inopérant. Le mode `AnyInput (2)` est la
  seule façon d'afficher, mais il fait popper le clavier à chaque champ
  texte. D'où le toggle **AnyInput ↔ repos** géré par le pont : rien ne
  poppe jamais spontanément pendant la session.
- **Le mode `Never (0)` n'est jamais l'état final** : c'est le mode
  « Désactivé » et l'applet OSK du system tray (`manage-inputmethod`)
  passe alors en `ActiveStatus` — son icône s'affiche en permanence dans
  la barre des tâches (comportement Plasma 6.5/6.6). En `NonMouseInput`
  (défaut Kinoite), l'applet reste cachée (`PassiveStatus`).
- `setMode` de KWin **persiste** le mode dans kwinrc
  (`[Wayland] VirtualKeyboardMode`) : le 1 final restaure la config par
  défaut au lieu de laisser un `Never` durable (qui tuerait aussi
  l'affichage tactile hors manette).
- Ordre obligatoire **0 puis 1** : seul le passage à `Never` déclenche
  `hide()` côté KWin, qui masque le panneau **et** reset
  `m_showRequested`/`m_forceShowRequested` — sans ce reset, le `show()`
  inconditionnel de `setPanel()` ré-afficherait le panneau re-mappé après
  le redémarrage de l'IM. Le `setMode(1)` n'affiche rien.
- Piège résiduel : une app peut ré-afficher un panneau **déjà mappé**
  (demande `text-input` explicite). Le `kill -9` fait repartir
  plasma-keyboard avec un panneau jamais mappé, incapable de
  réapparaître ; en `NonMouseInput`, ni clavier ni manette ne
  re-déclenchent le panneau. KWin relance l'IM automatiquement sur crash
  (`QProcess::CrashExit`, 5 relances max / fenêtre de 20 s).
- Vigilance : le `setMode(2)` du démarrage persiste aussi `AnyInput`
  dans kwinrc — si le pont meurt brutalement (SIGKILL, crash), l'état
  reste « popups spontanés » jusqu'au prochain cycle complet du pont.

### Quarantaine de frappe (poll D-Bus 250 ms)

Deux propriétés KWin (`org.kde.KWin /VirtualKeyboard`, interface
`org.kde.kwin.VirtualKeyboard`) lues en un appel :
`busctl --user get-property … active visible`

| active | visible | État |
|--------|---------|------|
| true   | true    | **Frappe autorisée** (IM active + panneau exposé : le grab clavier de l'IM capture les flèches, jamais re-transmises) |
| true   | false (0,5 s) | **L'utilisateur a fermé le clavier via son UI → sortie du pont** (sinon il resterait grabé à vie) |
| false  | false   | Focus hors champ texte → **quarantaine** : pad grabé muet, le pont reste, re-focus → tout reprend |

Point clé : quand le focus est sur une app **sans** champ texte
(VacuumTube, un jeu), KWin désactive l'IM → le grab clavier de
plasma-keyboard tombe → nos touches partiraient en direct dans l'app. La
quarantaine évite ça. Le panneau se masque aussi automatiquement quand
l'IM se désactive (Qt : `inputMethod()->setVisible(false)`).

### Mapping des touches

| Manette | Injecté (uinput) | Note |
|---------|------------------|------|
| D-pad / stick gauche | Flèches | Auto-repeat manuel (400 ms puis 60 ms), D-pad prioritaire |
| A (BTN_SOUTH) | Entrée | **Appui court** : valide la touche surlignée. **Maintien** (≥ 600 ms, `diacriticsHoldThresholdMs`) : popup d'accents (long-press) — une fois ouverte, flèches + Entrée sélectionnent, Échap ferme (`OverlayController` upstream redirige ces touches vers l'overlay) |
| B (BTN_EAST) | Échap | Ferme le clavier (→ sortie auto via détection UI) |
| Carré (BTN_WEST) | Backspace | Auto-repeat |
| Triangle (BTN_NORTH) | Espace | Auto-repeat |
| L1 / R1 | Tab | Changement de page du clavier (peu utile v1) |
| **Start** | — | Sortie du pont (raccourci de fermeture, marche même en quarantaine) |
| **Home + Carré** | — | Sortie du pont |
| **Home + R3** | — | Sortie + passage en mode souris (voir ci-dessous) |

Home et Start sont trackés même en quarantaine (les raccourcis de sortie
doivent toujours marcher). Entrée est pressée **sans auto-repeat** (ni côté
pont, le repeat KWin côté IM reste sans effet sur une touche déjà
pressée) : c'est le maintien qui déclenche le long-press des accents.

### Handoff souris (Home+R3) et aveuglement mutuel

Pendant le grab, **gamepadshortcuts ne voit plus rien** (le pad lui est
caché). Conséquences assumées et gérées :

- Les combos gamepadshortcuts (Home+Y, etc.) sont inactives pendant la
  frappe — voulu.
- **Home+R3** pendant la frappe ne peut être vu que par le pont : il
  s'arrête et envoie **SIGUSR1** à son parent si c'est bien
  `gamepadshortcuts` (vérification de `/proc/<ppid>/comm`, sinon no-op).
  Le handler côté gamepadshortcuts pose un flag (`pending_mouse_launch`,
  fork/exec pas async-safe) et sa boucle principale lance
  `gamepadshortcuts-mouse`. Le tracking du processus souris reste ainsi
  correct (c'est gamepadshortcuts qui l'a lancé).
- À la sortie du pont, `check_kbdnav()` fait un `reset_button_states()` :
  pendant le grab, les releases de Home/Carré sont passées sous le nez de
  gamepadshortcuts — sans reset, une combo résiduelle déclencherait une
  action parasite au premier événement reçu.
- Réciproquement, **Home+Carré pendant le mode souris** tue la souris
  (SIGTERM + waitpid) avant de lancer le pont — la souris ne grabbe pas,
  donc gamepadshortcuts voit ce combo lui-même.

### Multi-session (VT tracking)

Même mécanique que gamepadshortcuts : `XDG_VTNR` + inotify sur
`/sys/class/tty/tty0/active`. VT inactif → `EVIOCGRAB` **relâché** et
pause complète (sinon l'instance gamepadshortcuts de l'autre session
serait aveugle). VT actif → re-grab et reprise.

## Limitations connues (assumées v1)

- **Canal hidraw** : les apps lisant la manette via `/dev/hidraw` (SDL
  avec driver hidapi — défaut pour DualSense/Switch Pro — et Steam)
  **continuent de recevoir le pad pendant la frappe**. Aucun mécanisme
  noyau ne permet de les couper (`HIDIOCGRAB` n'a jamais existé ;
  `HIDIOCREVOKE` ne révoque que le fd appelant — vérifié dans
  `drivers/hid/hidraw.c`). Solution long terme : proxy complet à la
  Steam Input (pad virtuel uhid), hors périmètre v1.
- **Pas de re-lancement sur déconnexion** : manette débranchée → le pont
  sort. Refaire Home+Carré après rebranchement.
- **Layout fr_FR** en preset système (pas de layout fr_CH dans
  plasma-keyboard — repli alphabétique sur fr_CA sinon). Un layout
  fr_CH QML custom est la suite naturelle (plasma-keyboard supporte les
  layouts custom).
- Navigation aux flèches = déplacement touche par touche (pas de saut
  par zones comme le clavier Steam).

## Dépendances de configuration (image)

- `files/system/all/etc/xdg/plasmakeyboardrc` : presets **obligatoires**
  — `keyboardNavigationEnabled=true` (sinon les flèches ne naviguent pas),
  `enabledLocales=fr_FR` et `panelFillScreenWidth=false` (clavier centré).
- plasma-keyboard doit tourner (défaut Kinoite 44, lancé par KWin).
- `busctl`/`qdbus` dans le PATH session, `/dev/uinput` accessible
  (uaccess logind).

## Points de vigilance pour les modifications

- **Ne jamais injecter hors `keys_allowed`** : les touches arrivent alors
  dans l'app focus (re-transmission de l'IM quand le panneau n'est pas
  exposé).
- **Conserver le poll `active && !visible`** : c'est la seule détection
  de fermeture via l'UI ; sans lui le pont reste grabé à vie.
- **Conserver le `kill -9` de plasma-keyboard dans cleanup()** : sans lui,
  les apps ré-affichent le panneau fermé via leurs demandes text-input.
- **Conserver l'ordre `setMode(0)` puis `setMode(1)` dans cleanup()** :
  le 0 fournit le `hide()`/reset `m_showRequested` (le 1 seul laisse le
  panneau réapparaître après le kill), le 1 final évite l'applet OSK
  visible et le `Never` persistant dans kwinrc — ne pas inverser, ni
  fusionner en un seul appel.
- **Le SIGUSR1 ne doit partir qu'au parent `gamepadshortcuts`** : en test
  manuel (parent = shell), le kill ferait mourir le shell.
- Les `system()` (busctl/qdbus/pkill) sont volontaires (prototype-simple) ;
  une migration vers sd-bus est possible mais pas nécessaire.
- Après toute modification : `gcc -Wall -Wextra -O2 -std=c11
  -D_GNU_SOURCE -o /tmp/opencode/kbdnav kbdnav.c` puis test
  du cycle complet (affichage → frappe → Home+Carré → tout relâché).
