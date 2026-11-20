# gpad — l'overlay (Rust, wayland-client brut + layer-shell + shm)

Surface `zwlr_layer_shell_v1` (layer overlay, région d'entrée vide : la souris
traverse), manette grabée en evdev pendant l'ouverture, rendu 100 % main dans un
buffer shm (aucun toolkit). Deux habillages : **RETRO** (police bitmap 5×7
embarquée) et **MODERNE** (Noto Sans TTF + glyphes de manette Xelu, embarqués
dans `assets/`). Contrat des gestes et liste des composants : `AGENTS.md` racine.

## Réglages (page REGLAGES)

Le menu **est** le panneau : aucune sous-page, aucun panneau secondaire. Les
réglages vivent sur la page `REGLAGES`, groupés par sections (titre + filet,
**jamais sélectionnables** — la navigation les saute). On navigue (haut/bas,
D-pad **ou** stick gauche) et chaque ligne de réglage se manipule **au
gauche/droite**, appliquée **immédiatement** — rien à valider.

| Section / ligne | Curseur | Effet |
|---|---|---|
| `SON` / `VOLUME` | 21 crans, 0..100 % | ±5 % par cran (`pactl`, sink par défaut) ; **A = muet / non muet** (le niveau est conservé) |
| `AFFICHAGE` / `MANGOHUD` | 3 positions, `MASQUE` **à gauche** → `PAR DEFAUT` → `HORIZONTAL` | `mangohud.sh set-vue` — **même source que le dialogue « MangoHud… » de Gablue Manager** (`~/.config/MangoHud/MangoHud.conf`), voir plus bas |
| `AFFICHAGE` / `FPS` | 7 positions, `30` → `48` → `60` → `90` → `120` → `165` → `INF` | `mangohud.sh set-fps` ; une limite **hors crans** (MangoJuice, dialogue « MangoHud… » de Gablue Manager) est affichée **telle quelle** et le curseur se pose sur le cran le plus proche — le premier gauche/droite repart de la vraie valeur (`refresh_mangohud` → `fps_reel`) |
| `AFFICHAGE` / `VRR` | **interrupteur** (A) | VRR **automatique** (Adaptive Sync de KWin) sur toutes les sorties capables ↔ désactivé : `vrr.sh toggle` → `kscreen-doctor output.<nom>.vrrpolicy.automatic\|never` |
| `REPLAY` / `DUREE REPLAY` | 5 positions, `15` → **`30`** → `60` → `120` → `300` s | passé en `REPLAY_SEC` au lancement du buffer |
| `PERFORMANCE` / `PRIORITE ACTIVE` | **interrupteur** (A) | « optimiser au premier plan » : le booster cgroup de Plasma donne la priorité CPU/IO à la fenêtre active ↔ désactivé : `booster.sh toggle` → `autostart` dans `~/.config/kcgroupsrc` + `systemctl --user restart\|stop plasma-foreground-booster.service` |
| `INTERFACE` / `STYLE RETRO` / `STYLE MODERNE` | **interrupteur** (A) | le libellé porte le style courant (OFF = retro, ON = moderne), appliqué **à chaud** + écrit dans `~/.config/gablue/gpad-style` |

Plus d'entrée `TOUT FERMER` ici : elle est passée dans le **menu système**
(Home maintenu), avec les autres actions qui ne se réparent pas.

- La ligne s'appelle **`PRIORITE ACTIVE`**, pas « optimiser au premier plan » :
  à l'échelle 3 la police bitmap avance de 6 × échelle, donc **18 caractères**
  tiennent dans les 340 px utiles et `text_fit` **tronque** (il ne réduit pas) —
  le libellé exact demandé (24 caractères) finissait en « OPTIMISER AU PREMI.. ».
  La section `PERFORMANCE` dit le domaine, la ligne dit l'effet.
- **MangoHud n'a qu'une source** : `~/.config/MangoHud/MangoHud.conf`, réglée
  ici ou par le dialogue « MangoHud… » de Gablue Manager (mêmes trois vues,
  mêmes clés `no_display` / `horizontal` / `fps_limit`). Les lanceurs gwine et
  LGP ne posent plus leurs propres clés (`mangohud`, `mangohud_layout`,
  `fps_limit` dans leurs fichiers d'options) : en `MANGOHUD_CONFIG` elles
  gagnaient clé par clé et masquaient donc ce réglage au lancement. Toute
  évolution de la sémantique se fait **des deux côtés** (`mangohud.sh` ↔
  `local.write_mangohud_conf`).
- L'état affiché est **relu** dans `~/.config/kcgroupsrc` (`booster.sh get`,
  au démarrage et après chaque bascule), jamais un cache : la clé peut avoir
  changé depuis Gablue Manager, qui écrit la même. **Clé absente = activé**
  (défaut de Plasma, et la lecture de Gablue Manager dit pareil).

- Chaque ligne de réglage est plus haute (86 px au lieu de 46) : nom à gauche,
  valeur à droite, **curseur** dessous. Toutes les valeurs sont au **même corps**
  (échelle 2) : `60 %`, `30`, `30 s`, `Masqué`, `Moderne` — à l'échelle 3 le
  volume et les FPS écrasaient les autres lignes (constaté).
- **Curseur façon Plasma** : sillon arrondi continu (6 px de haut, bouts ronds),
  rempli jusqu'à la poignée, poignée ronde de 16 px à liseré sombre. **Pas de
  graduations** (Plasma n'en a pas ; la valeur est écrite à droite).
- **Un seul jeu de centres** pour le sillon, le remplissage et la poignée
  (`centre(i) = x + 8 + i*pas`, pas = `(w-16)/(n-1)`) : les graduations
  partaient de `x + pas*i` et la poignée de `x + pas*i + 9`, elle flottait donc
  ~6 px à droite de son repère — flagrant sur les curseurs à 3 crans.
- Le sillon non rempli (`piste`) doit **se voir sur le panneau sombre** : à
  `0x1e2832` il se confondait avec le fond et l'on ne voyait que la poignée
  (constaté en RETRO) → `0x2c3846`.
- Sur la ligne **sélectionnée**, le curseur prend les variantes
  `curseur_*_sur` (poignée bleue à contour sombre sur la pastille claire) : une
  poignée blanche sur pastille blanche ne se voyait pas.
- **États à deux valeurs = interrupteur**, pas un mot : `REPLAY`,
  `MODE SOURIS`, `PLEIN ECRAN` (page FENETRE) affichent une
  pastille `interrupteur()` (rail `rail`, bouton clair, glissé à gauche/droite).
  Le rail doit être plus clair que le panneau, sinon seul le bouton se voit.
- Les valeurs **butent** aux extrémités : un curseur ne repart pas à zéro.
- Haut/bas **continuent de naviguer** même sur une ligne de réglage : on se
  pose, on règle, on repart.
- Abandonné en cours de route : `FPS 60/120` (libellé remplacé par `FPS` +
  valeur), sous-vues plein panneau, **panneau secondaire coulissant** (le
  demandé est « dans le menu même » : ne pas y revenir). Marge layer-shell :
  **positive honorée** (bordure mesurée à x=1600 pour `--test-margin 480`),
  **négative ignorée** par KWin → le panneau glisse par décalage du **dessin**.
- `FOCUS fps|mango|volume` : le panneau s'ouvre **sur la page `REGLAGES`**,
  curseur sur la ligne (c'est ce que passe le combo Home+bas). Mesuré :
  `FOCUS fps` → page `REGLAGES`, `FPS` sélectionné. Mesuré aussi : `FOCUS
  mango` puis stick droite → `vue=horizontal`.

Mesuré : `VOLUME` 50 → 60 → 55 % (`pactl`), `A` → `MUTE true` puis
`MUTE false` ; `MANGOHUD` gauche → `vue=masque` ; `FPS` gauche → `fps=165` ;
**D-pad** gauche/droite → même effet que le stick.

## Pages : ACTIONS, RÉGLAGES, APPLICATIONS (L1/R1)

Trois pages dans le même panneau, dans cet ordre : `ACTIONS` (défaut),
`REGLAGES`, `APPLICATIONS`. **R1 avance, L1 recule** (elles allaient dans le
même sens) ; la liste des fenêtres est un **aller-retour** : `B` y revient sur
la page qui l'a ouverte au lieu de fermer le panneau, et activer une fenêtre y
revient aussi (`fen_retour`). La sélection est propre à chaque page, la liste
affichée est celle de la page courante (`liste()`), et les séparateurs de
section ne sont jamais sélectionnés (`pas_selectable()` saute les
`Entree::Groupe`). Deux sous-pages hors cycle : **FENETRE** (actions de la
fenêtre active) et **FENETRES** (liste) — on y entre par l'en-tête ou par
`CHOISIR FENETRE`, on en sort par `B`.

**Changement de page animé** : le **contenu** glisse horizontalement (l'ancienne
page sort d'un côté, la nouvelle entre de l'autre, 180 ms) ; l'en-tête de
fenêtre et le pied ne bougent pas. Le dessin passe par `Canvas::ox` (décalage)
et `Canvas::clip` (découpe) : la liste est dessinée **deux fois** pendant
l'animation, avec `self.page` permuté entre les deux. Le `poll` descend à 8 ms
tant que `pdep` tourne.

**Titre de page** : sous le nom de la fenêtre (18 px plus bas, avec de l'air
avant la liste), le nom de la page **centré** en échelle 3 (`ACTIONS`,
`RÉGLAGES`, `APPLIS`, `FENÊTRE`, `FENÊTRES`) avec un **filet d'accent dégradé
de chaque côté** (`filet_fondu_inv` à gauche, `filet_fondu` à droite), **2 px
d'épaisseur** et alpha 0xBB : à 1 px et 0x66 le filet disparaissait sur le
papier peint (constaté « pas assez visible »). Pas de pastille dans le titre.

**Suivi de page** : trois **ronds** en bas du panneau (9 px, écart 9), la page
courante pleine en accent et grossie de 2 px, les autres en `rail`. Des carrés
à l'origine — le titre n'en veut plus, le suivi reste, en ronds (demandé). Les
sous-pages `FENÊTRE`/`FENÊTRES` n'ont pas de rond à elles : c'est la page qui
les a ouvertes qui reste allumée (`page_cycle()`).

**Page `ACTIONS`** — la fenêtre active et tout ce qui agit dessus, puis les
actions de session :

```
FENETRE ACTIVE (icône + légende + classe ; A = page FERMER / PLEIN ECRAN / CHOISIR FENETRE ; L2/R2 = cycle)
ENREGISTRER UNE VIDEO
CAPTURE ECRAN
REPLAY
MODE SOURIS
CLAVIER VIRTUEL
```

**Page `REGLAGES`** — uniquement des réglages, par sections :

```
SON        VOLUME
AFFICHAGE  MANGOHUD
           FPS
           VRR
REPLAY     DUREE REPLAY
PERFORMANCE PRIORITE ACTIVE
INTERFACE  STYLE RETRO / STYLE MODERNE
```

L'en-tête est le **premier élément de chaque page** (`Entree::Fenetre`) : il est
donc sélectionnable partout. **À l'ouverture, le curseur est sur la fenêtre
active** (`index_entete()`) — c'est ce qu'on regarde en premier, et `A` y ouvre
la page `FENÊTRE` (demandé). **Sans fenêtre active** (info KWin entièrement
vide : titre **et** classe), le panneau ouvre **`APPLICATIONS`** avec son
premier élément : la page ACTIONS n'aurait aucune cible (demandé). Une fenêtre
sans titre mais avec sa classe (Project Midas) reste une fenêtre : elle garde
ses actions, et la classe sert de nom. Une page atteinte par **L1/R1** s'ouvre,
elle, sur son **contenu** (`premier_action()`) ; `--focus` (Home+bas) garde la
ligne qu'il impose (`focus_impose`).

Le choix se fait **dans la boucle**, pas au démarrage : l'info de fenêtre arrive
par D-Bus ~100 ms après le lancement, et décider plus tôt ouvrait
`APPLICATIONS` alors qu'une fenêtre était active (constaté). `page_initiale`
passe à vrai au premier `Info` reçu (`recu`) ou après une grâce de **600 ms**
(garantit un panneau utilisable même sans KWin). En **résident**, pas de
grâce : le script pousse depuis le démarrage de la session, attendre ne
servirait qu'à montrer la page ACTIONS par défaut pendant que le panneau est
déjà visible, puis à basculer sèchement sur APPLIS (vécu gen1 20/11/2026,
script `wininfo` jamais chargé).
Ordre retenu : la fenêtre active d'abord, puis ses actions, puis la session
(manette en main) ; réglages en dernier (on les change une fois). Un séparateur
fait 52 px (titre 2×, filet de 1 px jusqu'au bord), il ne reçoit ni curseur `>`
ni surbrillance.

**Page `APPLICATIONS`** — voir ci-dessous.

| Ligne | Fenêtre reconnue (class/desktop) | Lancement (si absente) |
|---|---|---|
| `GLAUNCHER` | `glauncher` | `scripts/toggle-glauncher` → `/usr/bin/glauncher` (plein écran par défaut) |
| `EMULATIONSTATION` | `esde`, `es-de`, `emulationstation` | `scripts/toggle-es` → `~/AppImages/esde.appimage` (plein écran par nature) |
| `YOUTUBE` | `vacuumtube` | `scripts/toggle-yt` → `flatpak run … --fullscreen` |
| `GABLUE TV` | `tvqt`, `gablue_tv`, `gablue-tv` | `scripts/toggle-tv` → `/usr/bin/tvqt --fullscreen` |
| `JELLYFIN` | `jellyfin` | `scripts/toggle-jellyfin` → `flatpak run … --fullscreen` (ligne **ajoutée seulement si** `flatpak list --app` le contient) |

- **Un appui sur une appli DÉJÀ OUVERTE l'amène au premier plan, elle ne la
  ferme jamais** (l'ancienne bascule la tuait — pénible en usage manette). Le
  panneau lit les fenêtres (`lire_fenetres`, même `lister.js` que la page
  Fenêtres, sans changer de page), cherche une fenêtre dont la class ou le
  desktopFileName contient un motif de `motifs_appli`, puis active son UUID
  (`activer_par_uuid`, le script KWin de la page Fenêtres). Statut `YOUTUBE:
  ACTIVE`.
- **Le panneau se ferme dès qu'on a choisi une appli** (`request_close` après
  l'activation comme après le lancement) : c'est un menu de lancement, il n'a
  rien à rester par-dessus le jeu ou l'appli qu'on vient d'ouvrir. Une entrée
  sans script (`RIEN A FAIRE`) le laisse ouvert — rien n'a été lancé. Un appui
  ignoré (même appli dans les 2,5 s) ne le ferme pas non plus.
- Le script `toggle-*` n'est appelé que si **aucune** fenêtre ne correspond
  (appli pas encore démarrée, ou KWin injoignable) : détaché, le panneau ne se
  fige pas, statut `YOUTUBE: LANCEMENT`. Les scripts **ne tuent plus rien** :
  si l'appli tourne mais que sa fenêtre n'a pas été reconnue, ils répondent
  « deja ouverte » au lieu de fermer une seconde fois par surprise.
- **Doublon d'appui** : un second A sur la même appli dans les 2,5 s est ignoré
  (`DEJA EN COURS`) — l'appli n'est pas encore visible pour `pgrep`, le script
  la relancerait.
- **Plein écran** : `tvqt` ne passe en plein écran que sur `--fullscreen`
  (`gablue_tv/app.py`), `VacuumTube` et Jellyfin Desktop l'ont aussi (vérifié
  par `flatpak run org.jellyfin.JellyfinDesktop --help`). Mesuré : `tvqt
  --fullscreen` → `WIN … fs=true Gablue TV | gablue-tv`.
- Dossier des scripts fourni par `SCRIPTS` (exporté par `run-gpad.sh`) ;
  `TVQT` et `ESDE` surchargeables pour tester au banc sans lancer les vraies
  applis.
- Mesuré : `APPLIS 4` ici (client Jellyfin installé) ; `PAGE Applis` →
  `SELECT YOUTUBE` → `APPLI YOUTUBE active uuid=… ok=true` (fenêtre factice
  `vacuumtube` ramenée au premier plan) ; sans fenêtre → `APPLI …/toggle-tv` ;
  icônes : `ICONE appli … -> trouvee` pour les quatre.

## Clavier virtuel

Le clavier vit **dans le résident** (`src/clavier.rs`), pas dans un second
binaire : il garde le grab déjà tenu, donc aucun transfert de manette, aucun
`exec`, aucune animation KWin — et il se ferme sans quitter le processus. Il
s'ouvre par l'IPC (`kbd <node>`) ou par `--clavier <node>` (repli du
déclencheur : un seul usage, puis sortie, sinon chaque appui laisserait un
overlay orphelin).

- **Surface dédiée à la taille du panneau**, transparente,
  `keyboard_interactivity = None` + région d'entrée vide : le focus reste à la
  fenêtre active et la souris traverse. Elle est ancrée en haut à gauche et
  **déplacée par marges layer-shell** : KWin ne réagit qu'à `marginsChanged`
  (`scheduleRearrange`), sans configure et sans renvoyer de tampon.
- **Ne JAMAIS élargir cette surface à l'écran** en dessinant le panneau dedans
  (première version, à ne pas refaire) : à chaque nouveau tampon KWin fait
  `im.convertTo()` sur **toute l'image** (`GLTexture::update`) — 3,7 M pixels
  par image en 1440p, d'où un déplacement qui saccade. Le dessin du panneau
  coûte 0,31 ms (`--bench-clavier`), le coût était donc bien côté compositeur.
  Une surface pleine taille n'a qu'un intérêt : apprendre la taille **logique**
  de l'écran (sonde au premier `configure`, puis on rétrécit) — c'est ce qui
  évite de supposer un facteur d'échelle.
- **Frappe par clavier uinput** (`gablue-clavier`, créé au premier usage —
  350 ms d'enregistrement, gardé toute la session) : le noyau livre les touches
  à la fenêtre active comme un clavier USB. KWin n'implémente pas
  `zwp_virtual_keyboard_v1`, d'où ce chemin. Toujours envoyer le `SYN_REPORT`
  du **relâchement**, même sans modificateur : sans lui la touche reste
  enfoncée côté noyau et KWin la répète (10 Entrées pour un appui, constaté).
- **Table dérivée du keymap XKB** de la session (`xkbcommon`, `kxkbrc`, puis
  `GPAD_CLAVIER_LAYOUT`/`GPAD_CLAVIER_VARIANT` pour le banc) : 4 niveaux
  affichés, accents morts marqués, `code evdev = keycode XKB − 8`. Aucune table
  en dur, donc ch-DE marche sans code.
- **Tampon = panneau** (707×367) : on repeint entièrement à chaque image
  (~0,3 ms), et un déplacement ne redessine **rien** (seules les marges
  changent, le tampon n'est pas renvoyé).
- Position mémorisée dans `~/.config/gablue/gpad-clavier` (écrite à la
  fermeture), bornée à l'écran à chaque ouverture.

| Geste | Effet |
|---|---|
| D-pad / stick gauche | curseur entre les touches (400 ms puis 110 ms) |
| Stick droit | déplacer le clavier (courbe `gpad-mouse` : zone morte 0,08, 2→12 px/frame) |
| Croix | taper la touche (400 ms puis 80 ms) |
| Carré | retour arrière (répétition) |
| Triangle | espace (répétition) |
| Rond | fermer le clavier |
| L1 / R1 | curseur de texte ← / → (répétition tenue) |
| **L2** | **Maj maintenue** → L2+L1/R1 = **sélection** |
| R2 | Entrée |
| L3 | Maj collante ; R3 | Ctrl collant ; SELECT | AltGr collante |
| START | Tab |

Disposition calquée sur les OSK console (Steam Deck : `X` = backspace, `Y` =
espace, `L2` = Maj, `R2` = Entrée ; PS5 : croix tape, carré efface, triangle
espace, rond ferme, **L1/R1 = curseur de texte**, L2 = Maj). Aucune combinaison
ne demande de tenir un petit bouton : la sélection est **index gauche (L2) +
index droit (L1/R1)**, les pouces restent sur le D-pad et les boutons. Un chord
« Select tenu + D-pad » a été écarté : Select est sous le pouce déjà posé sur
le D-pad.

Mesuré (banc vpad, `--clavier` sur la manette virtuelle) : `KBD OPEN` →
`KBD ch/fr ecran=2560x1440 pos=(926,993)` → deux pas + Croix tapent `a` puis
`s` dans une fenêtre Qt, Rond → `HIDDEN` et le processus sort (code 0).

Piège mesuré : `norme_axe` **borne** à −1..1. Un périphérique qui annonce une
plage étroite (`0..1`) et envoie 32767 donnait `mag ≈ 32766`, la courbe
quadratique explosait et un seul événement produisait un déplacement de 2 Gpx
(`attempt to add with overflow`). Ne jamais retirer ces bornes.

## Mode souris

`MODE SOURIS` lance/arrête `gpad-mouse` **sans toucher au panneau** : il ne
grabbe pas, il lit le node en partage. Tant que le panneau est ouvert, notre
`EVIOCGRAB` le rend muet ; il reprend à la fermeture. La ligne affiche `ACTIF`
(vert) ou `ARRET`, relue une fois par seconde en scannant `/proc/<pid>/comm`
(aucun fork).

Mesuré : `SELECT MODE SOURIS` → `MOUSE START gpad-mouse`,
`gpad-mouse` = 1 processus, **aucun `GPAD_DOWN`** (le panneau reste) ; un
second A → `MOUSE STOP gpad-mouse (1)`, panneau toujours ouvert. Home+R3 et
Home+carré depuis `gpad-trigger` → `COMBO Home+R3` / `COMBO Home+carre`.

## Stick : un pas par poussée (hystérésis + délai)

Le stick gauche **et** le D-pad naviguent, mais seul le stick dérive : ses
`EV_ABS` arrivent en continu, et un stick qui flotte autour d'un seuil unique
arme/réarme à chaque échantillon → **sauts de deux ou trois lignes « parfois »**
(constaté). Deux verrous :

- **hystérésis** : armé en franchissant **1/3** de la demi-plage, réarmé
  seulement quand l'écart retombe **sous 1/6** (`stick_x_latched` /
  `stick_y_latched`) ;
- **`PAS_MIN` = 140 ms** entre deux pas au stick (`derniere_nav`), pour le stick
  qui traverse le seuil d'un coup. Le D-pad n'est pas concerné : un `HAT` ne
  produit qu'un événement par appui.

Mesuré au banc : deux impulsions de stick à 50 ms d'écart → **un** seul pas
(`NAV DOWN` une fois) ; à 400 ms → deux pas.

## Rendu : styles RETRO et MODERNE

Tout est dessiné à la main dans un buffer shm (pas de toolkit). **Aucune couleur
n'est une constante** : le dessin lit `self.th` (struct `Theme`), qui porte à la
fois la palette et la géométrie (rayons, barres d'accent, ombre).

| Style | Police | Allure |
|---|---|---|
| `RETRO` (défaut) | bitmap 5×7 embarquée, **capitales** | SteamOS : fond quasi noir (~91 %), accent bleu Steam `#1a9fff`, **surbrillance claire en pastille** à texte sombre, rayon 16 / sélection 20 |
| `MODERNE` | Noto Sans TTF (`fontdue`), casse de phrase accentuée | **même thème, même géométrie** (`theme_pour()` renvoie `theme_retro()` pour les deux) + **glyphes de manette Xelu** dans l'aide du bas, marqueur de sélection en trait d'accent |

- **Choix à chaud** : ligne `STYLE` sur `REGLAGES`, section `INTERFACE`. C'est un
  **interrupteur** (`A`, pas un curseur) dont le libellé dit le style courant —
  `Style retro` (OFF) / `Style moderne` (ON) — appliqué immédiatement et écrit
  dans `~/.config/gablue/gpad-style` (choix mémorisé).
- **Résolution au démarrage** : `--style <retro|moderne>` > `PAD_STYLE` (banc)
  > fichier > `retro`. Un nom inconnu retombe sur `retro` (`style_normalise`
  mappe `moderne|modern|ttf` → 1, tout le reste → 0, donc `deck`/`nu`/`breeze`
  des anciennes configs restent valides).
- **Une seule identité visuelle** : MODERNE ne change ni couleurs ni géométrie,
  seulement la police et les glyphes. Un futur curseur de palette changera les
  couleurs des deux d'un coup.
- **`Theme::sur()`** : la surbrillance est **claire**, les gris/verts/rouges de
  la ligne sélectionnée passent en version sombre (`sel_txt`, `sel_dim`,
  `sel_ok`, `sel_warn`), sinon illisibles.

Géométrie et chrome, communs aux deux styles :

- **Coins arrondis** (`coins(rayon)`) : on efface les pixels hors du quart de
  cercle — le buffer étant en alpha, ça donne un vrai arrondi sans anti-aliasing.
- **Fond en dégradé vertical** (`rect_v`) : le jeu se devine derrière. Mesuré sur
  fond blanc : `srgb(57,62,73)` au milieu du panneau (~86 % opaque).
- **Cadre** : liseré 1 px (`edge`) ; aucune barre d'accent (retirée, la pastille
  claire suffit).
- **Ombre portée** décalée (+6/+8) et arrondie comme le panneau (absente si
  `ombre == 0`), jamais sur un voile (translucide sur opaque = fenêtre sur le
  bureau).
- **Sélection** : dégradé **arrondi** (`rrect_v`) — jamais `rect_v` + `coins()` :
  `coins()` évide les coins à transparent, donc dès qu'une fenêtre claire passe
  derrière le menu système la pastille « ronde » redevient un **rectangle**
  (constaté). Même traitement pour l'en-tête de fenêtre active.
- **Marqueur de sélection** : `>` en RETRO (langage pixel art), **trait d'accent
  arrondi** en MODERNE (un chevron fait « terminal » à côté d'une police moderne).
- **Ombre portée** : **panneau latéral seulement**. Dans le menu système le
  cadre est posé sur un voile translucide : l'ombre y dessinait un liseré d'une
  autre couleur en bas et à droite (constaté, retiré).
- **`rrect()`** : rectangle arrondi qui **n'efface rien** — pour tout ce qui se
  pose *sur* le panneau (sillon, poignée, interrupteur). `coins()` sert au cadre,
  où il faut au contraire évider les coins.
- **`coins_fond()`** : `coins()` **efface** les pixels hors du quart de cercle
  (transparent). Sous un voile, cela perçait un **trou clair** aux quatre coins
  du cadre — visible autour du menu système. `coins_fond(…, fond)` y repose la
  couleur du dessous (voile en plein écran, 0 pour le panneau latéral dont la
  surface *est* le panneau).
- **Voile du menu système** : 85 % (0xd8) minimum — une photo claire derrière un
  modal reste trop lisible sinon.

### MODERNE : police et glyphes

- **Police** : `fontdue` 0.9.5, `assets/NotoSans-Regular.ttf` embarqué par
  `include_bytes!`, chargé **une fois** (`charger_police`), 9 px × échelle (la
  hauteur de capitale tombe sur les 7 × échelle du bitmap, donc toutes les mises
  en page existantes tiennent). Licence **Apache 2.0** →
  `assets/LICENSE-NotoSans.txt`.
- **Sous-ensemble embarqué** : l'asset n'est pas la Noto Sans complète mais
  **1580 glyphes** (112 Ko au lieu de 607) — latin + étendu, grec, cyrillique,
  ponctuation, devises, flèches/formes. `fontdue` parse **tout** le fichier au
  chargement, glyphes inutilisés compris : mesuré 20/11/2026, tas ~10 Mo pour
  la police complète contre ~2 Mo pour ce sous-ensemble, soit RSS du résident
  **13 Mo au lieu de 19** — rendu identique (A/B sur capture : 9 pixels
  différents sur 518 000, le bord du panneau). Les symboles `→ ↔ ✕ ○`
  employés dans les libellés sont **déjà absents** de la police complète
  (glyphes PNG Xelu pour les boutons) : rien perdu.
  Régénération (fonttools hors image, l'asset est commité ; sortie dans un
  fichier temporaire puis remplacement, jamais entrée = sortie) :
  `uvx --from fonttools pyftsubset assets/NotoSans-Regular.ttf --output-file=/tmp/NotoSans-subset.ttf --unicodes="U+0000-024F,U+02B0-02FF,U+0300-036F,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,U+2000-206F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2500-25FF,U+2713-2718" --layout-features='' --no-hinting --desubroutinize`
  puis `mv /tmp/NotoSans-subset.ttf assets/NotoSans-Regular.ttf`.
- **Casse de phrase** (`joli()`) : la source est en capitales ASCII ; en MODERNE
  chaque libellé devient « Choisir fenêtre » avec ses accents (`ACCENTS`), les
  sigles restent en capitales (`SIGLES` : FPS, TV, HUD, REC…). Ne touche **que**
  les chaînes entièrement en capitales : les légendes de fenêtres, les noms
  d'applis et les valeurs numériques gardent leur casse. RETRO garde ses
  capitales (la bitmap n'a pas de minuscules).
- **Glyphes** : `assets/glyphes/{ps,xbox}/` (pack **Xelu**, CC0 —
  `assets/glyphes/README-Xelu-CC0.txt`), 100×100 PNG, embarqués par
  `include_bytes!` et chargés au premier dessin (`charger_glyphes`). Famille
  choisie par `pad_sony` (croix/rond vs A/B). Ils sont blittés **dans leurs
  couleurs d'origine** : teintés d'une seule couleur, la lettre évidée du disque
  disparaît à 21 px (mesuré).
- **Aide du bas** (MODERNE uniquement) : deux lignes avec les glyphes
  (`aide_ligne`, **24 px** — à 21 px les lettres des boutons restaient
  illisibles) — `[dpad] Naviguer  [L1][R1] Menu  [L2][R2] Fenêtre` puis
  `[A] Valider  [B] Fermer`. Une paire de gâchettes s'écrit `l1|r1` (glyphes
  accolés, sans grand trou). En RETRO, la ligne texte d'origine (et les glyphes
  dessinés ✕/○ pour une manette Sony) est conservée.
- **Confirmation** : une ligne armée affiche `CONFIRMER` en échelle 2 avec le
  **glyphe du bouton qui confirme** (croix PS / A Xbox), centré sur la ligne,
  au lieu d'un `?`.
- **`B` sur une ligne armée ne fait que la désarmer** (journal
  `DESARME (FERMER) par B`, statut `PRET`) : il ne revient pas en arrière et ne
  ferme pas le panneau. L'armement est testé **avant** les `Cancel` de
  sous-page (`FENETRE`, `FENETRES`) et avant la fermeture — sinon le geste
  « annuler » faisait sortir du menu (constaté).

`argb_p()` (prémultiplié) est obligatoire pour tout pixel **translucide** : le
buffer est en `Argb8888` prémultiplié, une couleur pleine y ressort plus claire
que voulu (les aplats sombres du panneau, eux, ne bougent quasi pas).

## Fenêtre active : en-tête et page FENETRE

En haut du panneau : **icône + légende + classe** de la fenêtre active. Quand un
jeu publié est rattaché à la fenêtre, la légende est son **nom de paquet** et la
classe disparaît (elle n'apprend rien) ; une fenêtre **sans titre** affiche sa
classe (Project Midas). Cet en-tête est une entrée comme les autres
(`Entree::Fenetre`), présente en tête de
**chaque** page ; **aucun filet de séparation** sous lui (retiré : il coupait le
panneau en deux pour rien). Sa **classe** (nom d'appli) a sa propre ligne sous
le nom, le **titre de page** vient encore dessous, et la liste des choix commence
à **162 px** : sans cet air, le nom de la fenêtre se confond avec le premier
choix (constaté en MODERNE).

### Page FENETRE (fenêtre active)

A sur l'en-tête ouvre une **page**, exactement comme `CHOISIR FENETRE` ouvre la
liste des fenêtres — même présentation, même retour (une boîte flottante a été
essayée puis abandonnée : deux langages visuels pour le même genre de choix, et
un fond qu'il fallait assombrir/éclaircir sans fin).

| Entrée | Effet |
|---|---|
| `FERMER` | A **arme** (ligne et rappel `CONFIRMER` en rouge) ; un second A envoie **SIGKILL** au pid **et à tous ses descendants** |
| `PLEIN ECRAN` | A bascule le plein écran de la fenêtre active (interrupteur), immédiatement |
| `CHOISIR FENETRE` | ouvre la liste des fenêtres |

- `B` revient sur **la page d'origine** (`fen_page_retour`), le panneau reste
  ouvert, **le curseur sur le nom de la fenêtre** (`index_entete`) : c'est de là
  qu'on a ouvert la page, on doit repartir de là.
- La page n'est **pas** dans le cycle L1/R1 : on en sort par `B`.
- `CAPTURE ECRAN`, `REPLAY`… restent sur la page ACTIONS : cette page ne contient
  que les actions de la fenêtre (et le choix d'une autre).
- **Aucune fenêtre active** (info KWin entièrement vide, `a_fenetre()`) : A sur
  l'en-tête ouvre **directement la liste** (`SELECT FENETRE (aucune)`) — il n'y
  a rien à fermer. Un titre vide **avec** une classe n'est pas « aucune
  fenêtre » : la page FENETRE s'ouvre normalement (vécu Project Midas .lgp,
  20/11/2026).
- Dans le **menu système**, l'aide du bas est **centrée** dans le cadre (alignée
  à gauche sous un menu centré, ça se voyait) ; le panneau latéral, lui, montre
  le **titre de page** sous l'en-tête.

Bouger dans la liste (ou changer de page) **désarme** : on ne ferme jamais par
inadvertance après avoir navigué. Le panneau ne se ferme pas après le kill.

**L'arbre, pas le seul pid** (`descendants()` lit en profondeur les
`children` de **tous les threads** de chaque processus, `/proc/<pid>/task/*/children`) :
un `kill` du pid de la fenêtre laissait ses enfants **reparentés à systemd**,
donc vivants — un émulateur lancé par ES-DE (enfant du frontend), les
sous-processus d'une AppImage, le `QtWebEngineProcess` d'une appli flatpak. Le
thread principal seul ne suffit pas : WebEngine et Wine forment leurs enfants
depuis un thread de travail, invisibles dans `/proc/<pid>/task/<pid>/children`
(fallait le constater : le lien jeu ↔ fenêtre échouait sur le banc). Les enfants
sont tués **avant** le parent, sinon le parent meurt d'abord et les orphelins
échappent au kill. Mesuré :

| Cible | Avant | Après |
|---|---|---|
| arbre de test (bash → bash → python fenêtre → 2 `sleep`) | 4 processus | `KILL 34886 enfants=2` → **tout mort**, plus aucun `sleep 300` |
| **ES-DE** (AppImage : runtime + `squashfuse` + `es-de`) | pid fenêtre `es-de`, parent runtime, sibling `squashfuse` | `KILL 35137` → runtime et mount `/tmp/.mount_esde…` **partis seuls en ~2 s**, aucun `fusermount` nécessaire |
| **Jellyfin** (flatpak) | `jellyfin-desktop` + `QtWebEngineProcess` + sous-sandbox `bwrap --args 73` | `KILL 35505 enfants=1` → plus rien 2 s après, sous-sandbox comprise |

**Ce que le kill ne fait délibérément PAS** — la portée est la seule sûreté
possible ici :

- **aucun `pkill bwrap`** : les `bwrap` de **gwine**, de ZapZap, de SyncThingy…
  ne sont pas des descendants de la fenêtre tuée et doivent survivre ;
- **aucun `fusermount -uz`** : les montages **LGP** (`/tmp/lgpackmount-*`) et les
  AppImages en cours ne sont pas à nous ; les superviseurs (runtime AppImage,
  `bwrap` de flatpak) démontent et sortent seuls quand leur enfant meurt — c'est
  mesuré ci-dessus, et c'est la raison de ne rien balayer ;
- **`squashfuse` n'est jamais visé en premier** : il est frère/parent de la
  fenêtre et non descendant ; c'est le piège déjà documenté dans
  `scripts/killthemall` (tuer le démon d'abord laisse le jeu figé sur des
  lectures FUSE mortes) ;
- pas de scan global de `/proc` : uniquement l'arbre du pid de la fenêtre.

## Logo, nom et icône du jeu lancé (glauncher, gwine, lgplaunch)

Quand la fenêtre active est celle d'un jeu lancé, le **logo du jeu** (visuel
SteamGridDB) s'affiche **au-dessus de l'en-tête de fenêtre**, dans une bande
pleine largeur ; l'en-tête, le titre de page et la liste descendent d'autant
(`logo_taille` → `decalage_logo`, répercuté dans `fen_visibles`). L'en-tête
lui-même prend le **nom du paquet** et **son icône**.

- **Deux publications, même format** (`lire_jeux_actifs`) : le cache de
  glauncher (`<cache>/running.txt` : `slug=`, `nom=`, `pids=`, `logo=`,
  `icone=`) et le dossier d'exécution de la session
  (`$XDG_RUNTIME_DIR/gablue/running.txt`, repli `/tmp/gablue-<uid>`), où
  **gwine et lgplaunch** publient `nom=`, `paquet=` (chemin du `.wgp`/`.lgp`) et
  `pids=`. Les deux sont lues à chaque passe ; le jeu dont **aucun PID ne vit**
  est écarté (lanceur tué en pleine partie).
- **Le lien fenêtre ↔ jeu est le PID, jamais la classe** : `wininfo.js` pousse
  le PID de la fenêtre active. Quatre liens, du plus sûr au plus large : PID
  publié tel quel ; PID **descendant** d'un PID publié (jeu natif, ou jeu dans
  son `bwrap`) ; **environnement** du processus — `GWINE_WGP_FILE` (gwine) ou
  `LGPACK_FILE` (lgplaunch) nomme le paquet lancé, et ce chemin doit être celui
  publié (ou un argument de la cmdline d'un PID publié quand seul glauncher
  publie) ; même **session** qu'un descendant publié — Wine reparente ses
  processus à systemd, la fenêtre n'est plus dans l'arbre mais garde la session
  de `bwrap --new-session`. La session de gpad est **exclue** de ce quatrième
  lien : un paquet lancé depuis un terminal est dans la session du bureau, et
  toutes les fenêtres matcheraient (le lien par descendance suffit alors).
- **Les jeux à launcher interne (Steam, Unity) exigent le lien par
  environnement** : leurs processus sont reparentés à systemd **et** prennent
  chacun leur propre session (le `setsid` du wineserver), donc ni l'arbre ni la
  session ne les rattachent au PID publié. Vécu Sonic Superstars 19/11/2026 :
  `pids=30751,30498` (bwrap, gwine), fenêtre `pid=30813` (`SonicSuperstars.exe`,
  ppid=systemd, session 30813) → aucun lien, pas de logo ; l'environnement de
  `30813` portait `GWINE_WGP_FILE=/run/media/.../Sonic Superstars.wgp`. Un jeu
  ne peut donc pas être reconnu par la seule classe (`steam_proton`).
- **`paquet=` donne le slug**, donc les visuels du cache, résolus à chaque
  lecture (une fois par seconde) : `slugify` est la règle exacte du launcher
  (`library::slugify`, accents conservés). Logo :
  `assets/logos/<slug>.png` ; icône : `assets/icons/<slug>.png` d'abord (une
  icône choisie y arrive, et le dossier est partagé entre postes), puis
  `pack-icons/<slug>.png`. **PNG seulement** (l'overlay ne décode pas le
  JPEG) : `glauncher --art <paquet>`, lancé en fond par gwine/lgplaunch juste
  après la publication, convertit et écrit ce qui manque dans son cache. Le
  launcher normalise tout le cache (héros et grilles en JPEG 1920 / 600×900,
  logos 1000 px et icônes 256 px en PNG) : un logo du cache ne dépasse donc
  jamais ~1 Mo, et l'overlay n'a pas à redimensionner pour ses 96 px. Le
  visuel apparaît donc **tout seul**, une seconde après son téléchargement,
  sans que le lanceur republie quoi que ce soit.
- **L'overlay ne télécharge rien et n'invente aucun visuel** : chemin absent du
  fichier, visuel absent du cache, image illisible = pas de bandeau, pas
  d'icône substituée.
- Le **cache** est celui de glauncher : `GLAUNCHER_CACHE` (banc), sinon
  `cache_dir` de `~/.config/glauncher/config.toml`, sinon
  `~/.local/share/glauncher`.
- **Nom et icône de l'en-tête viennent du paquet**, pas de la fenêtre : une
  fenêtre de jeu `gwine`/`lgplaunch` porte le titre du binaire et l'icône de
  l'exécutable. Quand `nom=` est publié, il remplace la légende KWin et la
  **ligne de classe disparaît** (`steam_proton` n'apprend rien) ; `icone=`
  remplace la résolution d'icône habituelle. Sans ces clés (image antérieure),
  l'en-tête retombe sur la fenêtre, comme avant.
- **Relu** au changement de fenêtre et une fois par seconde (le visuel peut
  arriver dans le cache après le lancement) ; journal `LOGO <chemin> (<slug>)`
  ou `LOGO aucun`.
- Hauteur plafonnée à `LOGO_MAX_H` (96 px), largeur bornée au panneau, ratio
  conservé (blit `Canvas::image`, même moyenne de boîte que les icônes).
  **Air identique au-dessus et au-dessous** (`LOGO_MARGE`, 16 px) : l'en-tête
  part de `MARGE + logo + MARGE`, donc le décalage vaut
  `th + 2×MARGE − 50` — la marge fixe de 50 px de l'en-tête était la cause d'un
  écart de 10 px en haut contre 60 en bas (vécu 19/11). Sous 18 px de haut,
  l'image n'a plus d'air à elle : rien n'est affiché. En **menu système**
  (`--console`), jamais.
- **La bande n'est réservée que si la page tient** : `decalage_logo` compare la
  bande à la place restante (`hauteur_contenu`, mêmes hauteurs que le dessin).
  Une page pleine — RÉGLAGES et ses sept lignes hautes — la masque plutôt que
  de pousser sa dernière ligne hors du panneau ; la page FENÊTRES défile, elle
  garde le bandeau et s'ajuste (`fen_visibles`).
- Banc sans jeu : `../test-logo.sh` (fenêtre du terminal + un logo du cache ;
  `--jeu` suit la publication réelle — celle de glauncher **ou** celle de
  gwine/lgplaunch).

## Liste des fenêtres (menu `CHOISIR FENETRE`)

`CHOISIR FENETRE` (entrée de la page FENETRE, ou A sur l'en-tête quand aucune
fenêtre n'est active) ouvre une
page qui liste les fenêtres **activables** avec leur icône (titre + marqueur
`ACTIVE`). A active la fenêtre choisie par son UUID KWin (`internalId`), puis le
panneau revient **à la page d'origine** (`fen_retour`) ; `B` y revient aussi
sans activer. L2/R2 font la même chose
sans ouvrir la liste.

Filtre = celui d'une barre des tâches : `normalWindow && !skipTaskbar && resourceClass`.
Sans lui, `plasmashell`, `xwaylandvideobridge` et le panneau lui-même
(`resourceClass=gpad`) apparaissent dans la liste ; **le même filtre**
sert au cyclage L2/R2 (sinon « précédente » tombe sur le bureau) **et à la
fenêtre active de l'en-tête** (`wininfo.js`) : le pont vidéo XWayland
(`xwaylandvideobridge`, `normalWindow` mais `skipTaskbar`) prend le focus au
login et, sans ce filtre, le panneau s'ouvrait sur la page FENÊTRE du pont au
lieu de la page APPLICATIONS (vécu 18/11/2026).

**Cyclage L2/R2** (`cycler_fenetre`) : script KWin **généré** à chaque appui
(`CYCLE_JS`), ancré sur l'**UUID** de la fenêtre active (poussé par `wininfo.js`)
et parcourant un ordre **stable** (position à l'écran), jamais `stackingOrder` —
celui-ci change à chaque activation, ce qui faisait rebondir L2 sur les deux
dernières fenêtres au lieu de parcourir la pile (constaté à l'usage, 17/11/2026).
Détail du script et de la signature D-Bus : `kwin/AGENTS.md`.

Les icônes sont préchargées **à l'ouverture de la liste** (SVG rastérisé une
fois) : les résoudre pendant le dessin ferait sauter le panneau. Au-delà de la
hauteur visible, la page défile pour garder la sélection à l'écran.

Mesuré : `WIN pid=36418 fs=false VacuumTube | rocks.shy.VacuumTube` puis
`FULLSCREEN true` + `WIN … fs=true` ; `KILL ARME pid=…` puis `KILL <pid> rc=0`
et la fenêtre cible meurt (test sur `ffplay`, jamais sur une appli de l'user).
`PAD_ICONE_TEST=<nom>` permet de tester la résolution d'icône sans fenêtre.

## Pièges de la recherche d'icône

- **Deux dispositions de thème** : `hicolor/48x48/apps/x` mais **`breeze/apps/48/x`**
  (le dossier de taille est *après* la catégorie). Oublier la seconde fait
  échouer toutes les applis KDE.
- **Le nom d'icône n'est pas le `desktopFileName`** : Konsole déclare
  `Icon=utilities-terminal`. Il faut lire le `.desktop` (applications XDG) avant
  de chercher.
- **Breeze ne livre que des SVG** (aucun PNG) : rastérisés une fois par
  ImageMagick dans `~/.cache/opencode/icons/`.
- **La catégorie n'est pas toujours `apps`** : `video-display`, l'icône que
  déclare `gablue-tv.desktop`, n'existe que dans `Adwaita/scalable/devices/`
  et `AdwaitaLegacy/48x48/legacy/`. La recherche balaie donc `apps`, `devices`
  et `legacy` et le thème `AdwaitaLegacy` — sinon Gablue TV est la seule ligne
  du menu sans icône.
- **`Icon=` peut être un CHEMIN ABSOLU** : `esde.desktop` déclare
  `Icon=/home/gab/AppImages/.icons/esde`, un SVG **sans extension**. On prend le
  chemin tel quel (plus `.png`/`.svg` essayés) avant de chercher dans les thèmes,
  et on tente le décodage PNG avant de conclure au SVG (se fier à l'extension
  ratait ce fichier).
- Police du panneau : 5x7, **le `?` n'existait pas** (glyphe ajouté à `glyph()`).

Navigation : la liste **boucle** (bas depuis la dernière ligne revient à la
première) — pendant un test, un « bas » de trop a relancé le clavier virtuel au
lieu de tuer ; en cas de doute, relire `SELECT …` dans le journal avant de
conclure.

## Une seule ligne pour les deux modes exclusifs

- **Replay actif** → la ligne `ENREGISTRER UNE VIDEO` devient **`SAUVER REPLAY`** (en
  vert) : mêmes touches, même place, et l'enregistrement n'est pas proposé
  puisque les deux sont exclusifs.
- **Enregistrement en cours** → la ligne `REPLAY` est **grisée**
  (`BLOQUE (REC)`), et le libellé devient `STOP ENREGISTRER` avec un
  **compteur `MM:SS`** à droite (`H:MM:SS` au-delà de l'heure).
- **A sur `REPLAY` pendant une capture est refusé** (`REPLAY: BLOQUE (REC)`,
  journal `REPLAY refuse (enregistrement en cours)`). Sans ce garde, A
  activait le buffer, la ligne `ENREGISTRER UNE VIDEO` passait en `SAUVER REPLAY` alors
  qu'un enregistrement tournait encore : deux modes à la fois (constaté à
  l'usage, 17/11/2026). L'exclusivité est donc tenue dans **les deux sens**.
- **`ENREGISTRER UNE VIDEO` ferme le panneau AVANT de filmer** : au démarrage,
  `combo_apres = Apres::Record` + `request_close()`, le recorder n'est lancé
  qu'après `FENETRE RETIREE -> action differee` — sinon le panneau se filme
  lui-même (même règle que Home+gauche). À l'**arrêt**, le panneau reste
  (rien ne se filme plus, et l'état `STOP → ARRET` sert de retour visuel).
  Mesuré : `ENREGISTRER UNE VIDEO -> fermeture puis record` → `GPAD_DOWN` →
  `RECORDER toggle` ; en arrêt, `SELECT ENREGISTRER UNE VIDEO` → stub appelé, panneau
  toujours ouvert.
- Le compteur vient du `start=<epoch>` écrit par `recorder.sh` dans
  `/tmp/opencode/recorder.state` : il reste juste même si le panneau est fermé
  puis rouvert pendant l'enregistrement.
- **Home+gauche** suit la même règle : si le replay tourne, il sauvegarde
  (`gpad-trigger` teste `replay.sh status`), sinon il bascule l'enregistrement.
- **Home+haut** = bascule du mode replay, panneau fermé (`gpad-trigger`) **et**
  panneau ouvert (`Apres::ReplayToggle` → `FENETRE RETIREE -> action differee`
  → `replay.sh toggle`). Fermer d'abord évite de filmer le panneau, et le bip
  de départ part avant le buffer (voir ci-dessous).
- **A sur `REPLAY` ne se contredit pas pendant le démarrage** : `replay_active`
  est posé tout de suite, et la passe d'état d'une seconde (qui relit les
  processus) ne le corrige **pas** pendant les 6 s qui suivent la bascule
  (`replay_attente`). `replay.sh start` met ~1,2 s (bip + pause + flatpak) et
  `stop` attend la sortie : sans cette fenêtre, la ligne repassait « ARRET »
  une demi-seconde avant de revenir « ACTIF » (constaté 19/11). Passé le délai,
  ou dès que l'état réel concorde, c'est de nouveau lui qui fait foi.
- **Replay : bip de départ, puis silence.** `replay.sh start` joue
  `sounds/beep.wav` (le bip historique de l'enregistrement) **et attend**
  `BIP_PAUSE` (0,35 s) **avant** de lancer gpu-screen-recorder : le monitor du
  sink garde la queue du son quelques centaines de ms, un buffer lancé aussitôt
  l'enregistrerait. Mesuré : `time ./replay.sh start` ≈ 1,2 s (0,67 s de bip +
  0,35 s de pause + démarrage). L'**arrêt** et la **sauvegarde** restent muets :
  un son de fin partirait dans le buffer encore ouvert (l'arrêt joue après le
  `killall`, donc plus rien ne l'enregistre — silence conservé par choix, c'est
  la vibration seule qui confirme une sauvegarde).
- **L'enregistrement a le même bip de départ**, avec le même schéma :
  `recorder.sh start` joue `sounds/beep.wav`, puis attend `BIP_PAUSE` (0,35 s)
  avant de lancer l'encodeur (les bips avaient été supprimés justement parce
  que le bip partait dans le fichier — l'ordre règle ça). Vérifié sur un clip
  de test : `beep.wav` culmine à −0,9 dB, la première seconde du fichier à
  −7,8 dB → **le bip n'est pas dedans**. L'**arrêt** reste muet
  (`stop.wav` inutilisé) : la notification donne le chemin et le son de fin
  n'aurait plus rien à enregistrer, autant ne pas l'ajouter.

**`comm` est tronqué à 15 octets par le noyau** : comparer `comm` au nom entier
ne matche jamais un binaire plus long — `gpu-screen-recorder` devient
`gpu-screen-reco`, d'où un enregistrement jamais détecté. `processus_actif()`
compare donc le nom tronqué à 15 (constaté 16/11/2026).

Ligne `ENREGISTRER UNE VIDEO` : c'est le **processus** qui fait foi (état + `gpu-screen-recorder`
sans `-r`), pas le fichier d'état — un `killall` de test laissait sinon
`STOP ENREGISTREMENT` affiché alors que rien ne tournait.

## Combos depuis le panneau ouvert, et glyphes Sony

Deux manques comblés par la même modification du lecteur evdev :

- **Les combos marchent panneau ouvert.** C'est l'overlay qui tient le grab,
  donc `gpad-trigger` est aveugle : c'est lui qui doit voir Home + direction.
  `BTN_MODE` reste `home_down` ; la direction suivante devient un combo au lieu
  d'une navigation. Appui bref (< 600 ms) sans direction = fermeture.
- **L'action attend que la fenêtre soit retirée** : `combo_apres` est exécuté
  après `wl_surface.destroy()` + flush, juste avant `GPAD_DOWN`. Sinon le
  menu se filmerait dans l'enregistrement — c'est exactement ce qu'on veut
  éviter. Mesuré : 203 ms entre le combo et la fin, ordre du journal
  `COMBO Home+gauche` → `FENETRE RETIREE -> action differee` → `GPAD_DOWN` →
  `RECORDER_STUB toggle`.
- **Glyphes Sony (RETRO)** : si la manette est Sony (`id/vendor = 054c`), l'aide
  du bas affiche **✕ VALIDER ○ FERMER** au lieu de `A:`/`B:`. La police bitmap
  n'a ni ✕ ni ○ : ils sont **dessinés** (deux diagonales ; anneau de 40 points).
  En MODERNE, ce sont les vrais glyphes PS5 du pack Xelu. Vérifié sur la vraie DualSense :
  `PAD /dev/input/event14 sony=true bt=true`. La détection est faite **avant**
  le grab (un node illisible ne doit pas changer l'interface).

Le bas du panneau ne montre plus que deux lignes de commande
(`DPAD/STICK: NAVIGUER`, `A: VALIDER B: FERMER`) : le journal de diagnostic
(evdev/hidraw/PAD/GATE/REC) et le balayage des lecteurs toutes les secondes ont
été retirés — ils coûtaient un scan `/proc` inutile une fois le banc validé.

## Rafraîchissement : tout état relu doit salir le panneau

Le panneau ne se redessine que si `dirty` est vrai. Tout état lu **en dehors
d'un événement manette** (mode replay, mode souris, compteur, fenêtre active)
doit donc mettre `dirty = true` **quand il change** — sinon l'écran garde
l'ancienne valeur : le mode replay restait affiché `ARRET` pendant qu'il
tournait (constaté 16/11/2026).

## Menu système : fondu, pas de zoom

Le panneau latéral **glisse** (il a un bord par lequel sortir) ; le menu
système (`--console`, cadre centré sur un voile plein écran) **s'efface** :
`opacite` va de 0 à 1 en **700 ms** à l'ouverture, et de 1 à 0 en **320 ms** à
la fermeture (`fondu_ms + 380` à l'entrée, `fondu_ms` à la sortie, base
`PAD_FONDU_MS` = 320). L'entrée est volontairement plus lente : sur un fond
sombre la montée courte ne se voyait pas. Le fondu d'entrée part de la **première image** (bloc `configured &&
!slid_in`), jamais du début du processus : le démarrage — wayland, script KWin,
icônes — prend **138 ms** mesurés, donc un fondu lancé trop tôt était quasiment
terminé quand la surface paraissait (constaté : « ça ne fait pas la fondue »).

**Calibrage** (enregistrement 60 fps + moyenne d'un patch sur la pastille de la
première ligne) : à 160 ms, la montée ne durait que **~100 ms** à l'écran (5
images utiles) — invisible à l'usage, tout en étant correct au log. À 380/320 ms
la rampe perçue fait ~300/250 ms. Toute modification de ces durées se vérifie
**à l'écran**, jamais au log : une opacité qui progresse ne garantit pas qu'on la
voit. `appliquer_opacite()` multiplie les quatre canaux de **tout** le
tampon (format prémultiplié : l'alpha reste cohérent avec les couleurs), et
`running = false` attend la fin du fondu — la surface reste montée jusqu'à ce
qu'elle soit transparente.

Pourquoi : démonter la surface d'un coup laissait KWin animer la fermeture de
la fenêtre, donc un **dézoom** sur le menu qui part (constaté, « c'est moche »).
La fondue est la nôtre, KWin zoome une surface déjà invisible.

## Entrée du panneau : attendre KWin, puis glisser (pas de fondu)

KWin traite cette surface comme une **fenêtre normale** (`normalWindow=true`,
`dock=false` mesuré par script KWin) : son animation d'ouverture s'applique donc
**au-dessus** de notre glissement. L'effet actif par défaut est **`Scale`**
(`/usr/share/kwin/effects/scale/metadata.json`, `EnabledByDefault=true` ;
`Glide` et `Fade` sont désactivés), d'où l'impression de « zoom en arrivant de la
droite ». À la fermeture rien ne se voit : notre glissement de sortie précède
`wl_surface.destroy()`, KWin n'a plus de client à animer — l'asymétrie est
normale, pas un bug.

Séquence d'entrée d'un overlay **jetable** (exec) — en `--resident`, l'étape 1
n'existe pas : la surface est déjà mappée (voir plus bas) :

1. `opacite = 0` et attente `PAD_PANNEAU_DELAI_MS` (200 ms — la durée
   `animationTime(200)` de l'effet `Scale`, pas 250 : on restait invisible
   50 ms de trop, mesuré 19/11/2026), `entree: Option<Instant>` : KWin anime
   une surface **invisible**. Le contenu est de toute façon encore hors écran
   (`off = panel_w`), donc rien ne clignote même une fois opaque.
   Le décompte part du **premier `configure`**, qui est traité à l'instant où
   il est lu : sans le `dispatch_pending` frais après `g.read()`, le
   `configure` n'était vu qu'au tour suivant, après un `poll` de 100 ms au
   repos — mesuré à 57 ms d'arrivée, entrée traitée à 160 ms (vécu 19/11/2026).
   Latence mesurée depuis le `exec` : `configure` 53 ms, premier tampon
   (transparent) 57 ms, révélation 269 ms, panneau en place ~469 ms — contre
   415 / 615 ms avant ces deux corrections.
2. Au bout de l'attente : `opacite = 1` d'un coup, puis glissement de 200 ms
   (`off` `panel_w` → `0`, ease-out cubic).

**Pas de fondu pendant le glissement** : les deux easings étant « ease-out », le
panneau était à 87 % de sa course à 90 ms alors qu'il n'était qu'à moitié opaque
— on ne voyait que la fin du mouvement, d'où « il apparaît sur place » (constaté
le 18/11/2026, après une première version qui fondait pendant le glissement).
Le fondu ne sert qu'à l'attente, et là c'est `opacite = 0` qui le fait.

`appliquer_opacite()` s'applique donc **au panneau aussi** : la garde
`if !self.side` qui l'en excluait datait du temps où il ne fondait pas (le fondu
était alors un no-op silencieux — mesuré : pixel identique avec ou sans).

Mesuré (captures `spectacle`, attente 2500 ms + glissement 4000 ms pour avoir le
temps de capturer) : pendant l'attente, colonne du panneau = fond
`srgb(35,38,39)` (rien de visible) ; mi-glissement, `x=2100` fond, `x=2250` et
`x=2400` panneau **opaque** `srgb(23,29,38)` (aucune translucidité, et le bord
gauche progresse bien de droite à gauche).

## Mode résident (`--resident --ipc <socket>`)

Le service ne lance **plus** un overlay par appui : `gpad-trigger` démarre
`gpad` une fois, en résident, et lui envoie `open <node> <cibles…>`,
`kbd <node>` ou `close` sur un socket Unix (`lire_ipc`). Tout l'état d'ouverture est refait par
`ouvrir()` : manette du node grabée, cibles du gate reposées puis `open`, page
remise sur `ACCUEIL`, curseur en tête, `depart`/`derniere_activite` réinitialisés
(sinon la passe d'inactivité de 10 s refermait le panneau aussitôt ouvert), puis
glissement `panel_w → 0` sur 200 ms.

- **La surface reste mappée**, transparente (`opacite = 0`) et hors écran
  (`off = panel_w`) entre deux ouvertures : KWin n'anime l'ouverture **qu'au
  map**, donc plus d'attente de son effet `Scale` ni d'`exec` à payer. C'est le
  gain principal : mesuré **6 ms** entre la commande et le premier tampon
  (**11,5 ms** depuis le relâchement de Home, bout en bout au banc), panneau en
  place ~205 ms (contre ~470 ms par ouverture avec exec).
- **Fenêtre invisible mais jamais détruite** : la fenêtre layer-shell n'apparaît
  pas dans `workspace.windowList()` de KWin (vérifié le 19/11/2026) — ni barre
  des tâches, ni liste FENÊTRES.
- **Caché = presque mort** : pas de manette, gate refermé, aucun scan `/proc` ni
  lecture de `running.txt`, aucun dessin ; `poll` à **1 s** (pas `-1` :
  `signal()` installe `SA_RESTART`, un `poll(-1)` serait repris après SIGTERM et
  le processus ne sortirait jamais — mesuré). SIGTERM met `quitter = true` :
  sortie après la fermeture, même en résident (sinon systemd attendrait son
  `TimeoutStopSec`).
- **Le script KWin `wininfo.js` reste chargé** entre deux ouvertures (son
  QTimer pousse la fenêtre active toutes les 300 ms) : à l'ouverture, le nom,
  la page et le logo du jeu sont **déjà** justes, sans rechargement D-Bus ni
  attente du premier `Info`. Coût : 3 push/s, négligeable.
- **Relancé s'il est muet** : tant que le script ne pousse plus (aucun `Info`
  depuis **3 s** — il en envoie un toutes les 300 ms), le résident **caché**
  recharge `wininfo.js` une fois par minute (KWin redémarré, chargement initial
  raté). La tentative précédente est déchargée avant la suivante (pas
  d'empilement de scripts dans KWin). C'est la **fraîcheur** du dernier `Info`
  qui fait foi : `recu` reste vrai à vie. Sans ça le panneau restait aveugle
  jusqu'à la fin de la session : en-tête vide, page d'ouverture APPLICATIONS.
- En résident, `request_close()` ne quitte pas : il glisse hors écran puis
  `cacher()` rend la manette, ferme le gate et exécute l'action différée
  (`executer_apres`, replay/capture/clavier) — panneau déjà hors écran.
- `--resident` : aucune manette au démarrage (le node arrive avec `open`), gate
  et cibles posés à chaque ouverture, `--seconds`/SIGTERM restent des sorties.
  Le socket `--ipc` est créé au **premier `configure`** (`demarrer_ipc`) : son
  existence signifie « panneau prêt », et aucun `open` ne peut tomber sur une
  surface sans taille. Il est retiré à la sortie (pas de fichier mort à chaque
  redémarrage du service).
- La police TTF est chargée **après le premier `commit`** Wayland (elle ne sert
  qu'au premier dessin) : ~15 ms de parsing recouverts par la réponse de KWin.
  Les réglages (`pactl`, VRR, MangoHud, booster) sont relus **après la première
  image** (`a_rafraichir`), jamais avant.

## Fermeture automatique par inactivité (10 s)

Panneau latéral **et** menu système se ferment après **10 s** sans **entrée
voulue** (`inactivite`, `--idle N`, `0` = jamais) : bouton appuyé, direction de
D-pad, ou stick sorti de sa zone morte (`/5` de la demi-plage). Le bruit de
repos ne compte pas — rafraîchir sur *tout* événement lu laissait un panneau
ouvert **11 min** avec la manette grabée (un DualShock 4 émet `ABS_X` 124↔125 en
continu, mesuré ; le jeu ne recevait plus rien). `surveiller_inactivite()` est
appelée à chaque tour de boucle et passe par `request_close()` (glissement de
sortie, pas de disparition sèche) ; journal `CLOSE (inactivite 10s)`.

Mesuré : `--idle 4`, stick poussé à t=3,5 s → toujours ouvert à t=5,5 s
(rafraîchi), puis `CLOSE (inactivite 4s)` après 4 s de vrai silence.

Pourquoi : un panneau laissé ouvert tient `EVIOCGRAB` — le jeu ne reçoit plus
rien — et le menu système masque l'écran. Mesuré au banc : `--idle 5` →
`CLOSE (inactivite 5s)` puis sortie propre.

## Gate : commandes bornées

`gate_cmd()` ouvre **une connexion par commande** (`clear`, `target`, `open`,
`close`) et n'attend aucune réponse : le démon répond sur sa sortie, jamais sur
la socket. Le `connect` est **borné à 500 ms** (thread + `recv_timeout`) : sur
une file d'attente saturée, `UnixStream::connect` attend indéfiniment, et
l'overlay restait figé — surface déjà retirée, mais `EVIOCGRAB` en main — si
bien que Home ne répondait plus du tout (vécu 18/11/2026, démon bloqué par un
client muet). Une commande perdue vaut mieux que ça : le démon est idempotent
(`clear` + `target`s reposés à chaque ouverture).

Sans `close` (overlay tué au SIGKILL, OOM), le hook du gate reste attaché : la
manette ne parle plus aux jeux jusqu'à la prochaine ouverture du panneau, dont
la séquence `clear` → `target` → `open` → `close` rétablit l'état. Rien à
nettoyer à la main.

## Ligne de commande et variables d'environnement

| Drapeau | Effet |
|---|---|
| `--grab <node>` | manette à grabber (sinon détection auto) |
| `--no-grab` | aucun grab (test de rendu) |
| `--gate <sock>` / `--gate-target <node>` | socket du démon de gate et nodes à couper |
| `--seconds N` | auto-fermeture après N secondes (bancs) |
| `--idle N` | fermeture après N secondes **sans événement manette** (`0` = jamais ; 10 s par défaut) |
| `--width N` | largeur du panneau latéral (480 par défaut) |
| `--slide-ms N` | durée du glissement (0 = 180 ms entrée / 140 ms sortie) |
| `--console` | mode **menu système** : surface plein écran assombrie + cadre centré, sans KWin, sans nom de menu ni en-tête (voir `trigger/AGENTS.md`) |
| `--resident` | survit entre deux ouvertures (surface mappée transparente hors écran) ; `open`/`close` arrivent par `--ipc` |
| `--ipc <socket>` | socket de commande du déclencheur (`open <node> <cibles…>`, `kbd <node>`, `close`) |
| `--fullscreen` | surface plein écran (banc) |
| `--focus fps\|mango\|volume` | ouvre sur `REGLAGES`, curseur sur la ligne |
| `--style retro\|moderne` | style visuel (prioritaire sur `PAD_STYLE` et le fichier) |
| `--test-margin N` | marge layer-shell positive (banc) |
| `--no-kwin` | n'installe pas le script d'info de fenêtre |
| `--clavier [node]` | ouvre **directement le clavier virtuel**, puis sort à la fermeture (repli du déclencheur, banc). Sans node : la manette détectée ; sans aucune manette : sortie code 1 |
| `--bench-clavier` | mesure le coût d'une image du clavier (aucun Wayland, aucune fenêtre, aucun grab) : `BENCH panneau … ms` |

| Variable | Rôle |
|---|---|
| `RECORDER`, `SCREENSHOT`, `MANGOHUD`, `VRR`, `BOOSTER`, `REPLAY`, `RUMBLE` | outils remplacés par des stubs |
| `KILLTHEMALL`, `FERMERAPPLIS`, `SCRIPTS`, `MOUSE` | idem pour les actions et combos |
| `GPAD_CLAVIER_LAYOUT`, `GPAD_CLAVIER_VARIANT` | forcent la disposition du clavier (banc, sans toucher `kxkbrc`) |
| `REBOOT`, `POWEROFF`, `BLUETOOTHCTL` | commandes système remplacées (le banc n'éteint jamais la machine) |
| `PAD_NODE` | manette qui a déclenché (transmise au replay et à `rumble`) |
| `PAD_SONY`, `PAD_BT_MAC` | forcent famille / transport (banc, manette virtuelle) |
| `GPAD_TRIGGER_PID` | pid de `gpad-trigger` à prévenir sur Home maintenu |
| `PAD_STYLE` | style de démarrage (banc, sans écrire de fichier) |
| `PAD_FONDU_MS` | durée du fondu du menu système (défaut 140 ms ; banc : ralentir pour le mesurer) |
| `PAD_PANNEAU_DELAI_MS` | attente avant le glissement d'entrée du panneau (défaut 200 ms : `animationTime(200)` de l'effet `Scale` de KWin ; banc) |
| `PAD_NO_KWIN`, `PAD_ICONE_TEST` | banc : sans KWin, test de résolution d'icône |
| `KWIN_SCRIPTS` | dossier des scripts KWin (`kwin/`) |

## Mesuré (overlay)

| Mesure | Valeur |
|---|---|
| glissement de sortie | bordure mesurée x=2118 → 2383 (repart à droite) ; marges layer-shell ignorées par KWin → décalage du **dessin** |
| volume dans la liste | 50 → 60 → 55 %, confirmé par `pactl` |
| A sur VOLUME | `MUTE true` puis `MUTE false` (niveau conservé) |
| FPS dans la liste | cran gauche → `fps=165` écrit dans le fichier de test ; fichier à `fps_limit=144` (hors crans) → droite `FPS_IDX 5 (165)`, gauche `FPS_IDX 4 (120)` depuis la vraie valeur, jamais depuis un cran par défaut |
| MangoHud dans la liste | cran droite → `vue=horizontal`, cran gauche → `vue=masque` |
| style | `--style truc` → `STYLE RETRO (retro) police=bitmap` ; `A` sur `STYLE` → `STYLE MODERNE (moderne) police=ttf` + fichier + bascule à chaud |
| vrr | `A` sur `VRR` → `VRR desactive` puis `kscreen-doctor -o` montre `Vrr: Never` sur chaque sortie ; second `A` → `Vrr: Automatic` (mesuré, `vrr=auto`) |
| glyphes | `GLYPHES 11 (xbox)` au premier dessin MODERNE ; manette Sony → `(ps)` |
| booster | page REGLAGES, 5 « bas » depuis `VOLUME` (sections sautées) → `SELECT PRIORITE ACTIVE` ; `A` → `BOOSTER actif` puis `BOOSTER inactif`, avec `kwriteconfig6 … autostart true\|false` + `systemctl --user restart\|stop plasma-foreground-booster.service` (HOME et `systemctl` factices) |
