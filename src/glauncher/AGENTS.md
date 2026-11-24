# glauncher

Launcher de jeux pour Gablue : liste les paquets `.wgp` (Windows → `gwine`) et
`.lgp` (Linux → `lgplaunch`) des dossiers configurés, avec l'icône du jeu,
pilotable au clavier et à la manette, plein écran.

Le code vit **ici** (`gablue/src/glauncher/`) : c'est la référence, le dépôt
standalone d'origine n'est plus mis à jour. Binaire autonome, aucune dépendance
système au-delà de `unsquashfs` (icône dans un `.wgp`), `qdbus6`/`qdbus` (plein
écran KWin) et `gwine`/`lgplaunch` pour lancer les paquets.

## Installation dans l'image

- `Containerfile-gablue`, étape `glauncher-build` (toolchain jetée) :
  `COPY src/glauncher/{Cargo.toml,Cargo.lock,src,assets}` puis
  `files/scripts/build-glauncher` → `/out/glauncher`, récupéré par
  `COPY --from=glauncher-build /out/glauncher /usr/bin/glauncher`.
  **`assets/` est obligatoire** : les visuels sont embarqués par
  `include_bytes!`, sans le dossier la compilation échoue.
- `.desktop` : `files/system/all/usr/share/applications/glauncher.desktop`
  (catégorie `Game`, `StartupWMClass=glauncher`) ; icône :
  `files/system/all/usr/share/icons/hicolor/scalable/apps/glauncher.svg`
  (copie de `assets/glauncher.svg`).
- **Fenêtre** : `window::Settings` dans `main.rs` porte
  `platform_specific.application_id = "glauncher"` (app_id Wayland = nom du
  `.desktop` : sans lui KWin ne rattache pas la fenêtre et affiche l'icône
  générique Wayland, constaté 20/11) et `decorations: true` **en permanence**.
  Ne jamais conditionner les décorations au plein écran : sous Wayland
  (xdg-decoration) elles sont négociées **à la création** — winit demande du
  client-side quand `decorations` est faux, et `window::toggle_decorations` ne
  peut plus rien négocier ensuite → sortir du plein écran rendait une fenêtre
  sans barre de titre (20/11). En plein écran KWin n'en dessine pas, donc le
  drapeau fixe convient aux deux modes ; `Message::ToggleFullscreen` ne fait
  que changer de `window::Mode`.
- Réglages : le launcher lit `~/.config/glauncher/config.toml` et ne l'écrit
  **le launcher, lui, n'écrit QUE trois clés** — `game_dirs`, `music` et
  `music_volume` —, chacune en préservant commentaires, ordre et clés inconnues
  (`save_game_dirs`, `save_music`), le même contrat que l'écrivain de Gablue
  Manager (`dialogs/glauncher.py`) : les deux sont interchangeables. Le reste
  (plein écran, SteamGridDB) appartient au manager. Vue, tri et taille des
  vignettes se règlent dans le launcher et vivent dans son cache
  (`state.txt`).
- Menu de l'overlay manette (`gablue-pad`) : entrée `GLAUNCHER`, au-dessus
  d'EmulationStation, via `scripts/toggle-glauncher`.
- Compilation locale : le `target-dir` vient de `~/.cargo/config.toml`
  (`~/.cache/opencode/cargo-target`), **doublé** par le `.cargo/config.toml` du
  dépôt (même valeur, versionnée : un poste où la config globale manque garde
  la sortie hors de `~/Syncthing`). **Jamais** de `target/` dans le dépôt
  (2,6 Go synchronisés constatés le 20/11 — garde-fou `.stignore` à la racine
  du dossier Syncthing). `test.sh` demande le chemin à `cargo metadata`.

## Commandes

```bash
cargo run -- --windowed --simulate 5 --dir demo-games   # test rapide (idem ./test.sh)
./test.sh                   # TES jeux, plein écran, lancement réel
./test.sh windowed          # TES jeux, en fenêtre
./test.sh pads              # manettes détectées + axes et bornes (diagnostic)
./test.sh simulate          # TES jeux, lancement simulé (fondu + écran noir)
./test.sh list              # liste numérotée (pour play)
./test.sh play 12           # lance vraiment le jeu n° 12 (« play <nom> » accepte un motif)
./test.sh index cold        # chrono du scan à froid puis à chaud
cargo test                  # tests unitaires (slug, extensions, troncature)
cargo build --release
glauncher --art "$HOME/Windows/Games/Jeu.wgp"   # logo + icône d'un paquet (overlay manette)
glauncher --shrink          # allège le cache existant (une passe après coup)
```

`--simulate` remplace la commande de lancement par un `sleep N` : on teste
l'écran noir, le retour au launcher et le code de sortie sans `gwine`.
`demo-games/` contient des paquets **factices** (fichiers texte renommés) et
leurs icônes, pour l'interface.

## Structure

- `src/main.rs` — arguments, `window::Settings`, démarrage iced
- `src/config.rs` — config TOML, recherche du fichier, options CLI
- `src/library.rs` — parcours parallèle des dossiers, icône/cover, `slugify`
- `src/index.rs` — index SQLite des paquets et des dossiers (mtime)
- `src/covers.rs` — worker SteamGridDB (API publique ou clé v2), identification
  des jeux (`identities.txt`), années (`years.txt`)
- `src/dialog.rs` — sélecteur de fichiers du système pour les images perso
  (`kdialog`, puis `zenity`), lancé hors de l'UI
- `src/thumbs.rs` — miniatures 320 px de la grille (deux voies : `now`
  puis `sweep`), invalidation par `.settings` et mtime
- `src/updates.rs` — mise à jour de l'image : commande en tâche de fond, lue
  sur le protocole `::gablue:` (le même que les dialogues de Gablue Manager)
- `src/disks.rs` — disques externes : points de montage lus dans
  `/proc/self/mountinfo`, découverte **bornée** des dossiers porteurs de
  paquets, mémoire des racines trouvées (`<cache>/disks.txt`) et guetteur des
  branchements
- `src/watch.rs` — surveillance inotify : rafale → chemins touchés ; sa
  `Veille` accepte une **nouvelle liste de dossiers à chaud**
- `src/folders.rs` — choix des dossiers de jeux : listage trié (dossiers
  seuls, cachés écartés, « .. » en tête), chemins en `~/…`, ajout sans doublon
  (comparaison sur le chemin résolu) et retrait qui ne touche **jamais** au
  disque
- `src/recorder.rs` — dossiers de sortie dans `~/.config/gablue/recorder.conf`
  (clés `dossier` et `dossier_screenshot`) : lecture, écriture conservatrice,
  « remettre par défaut » = retirer la clé (aucun chemin en dur)
- `src/gallery.rs` — galerie : scan des captures et vidéos, vignettes 320 px
  (JPEG) sur un fil de fond, cache `<cache>/gallery/`, date locale
  (`localtime_r`), minuteur de lecture
- `src/play.rs` — lecture d'un enregistrement : `mpv` plein écran piloté par
  socket IPC (pause, ±5 s, sortie), `PR_SET_PDEATHSIG` contre les films
  orphelins
- `src/gamepad.rs` — manettes via `evdev`, boutons normalisés (`BTN_SOUTH`…)
- `src/launch.rs` — construit la commande, lance le jeu, attend sa fin
- `src/playtime.rs` — temps de jeu cumulé, dans les dossiers de sauvegardes
  (`lgp-saves/glauncher/`, `LocalSavesWGP/glauncher/`), migration de l'ancien
  fichier du cache
- `src/music.rs` — musique de fond : `mpv` en tâche de fond, fondu par IPC
- `src/sons.rs` — autres flux audio du poste (`pactl`), pour taire la musique
  quand quelqu'un d'autre joue du son
- `src/sparkles.rs` — poussière d'étoiles du fond (`canvas`, minuteur 20 Hz)
- `src/pose.rs` — `Pose` : échelle + décalage d'un élément au rendu (plongée de
  la page, retour des vignettes, respiration de la sélection), sans surcouche
  ni mise en page
- `src/marquee.rs` — `Marquee` : titre recadré qui défile quand il ne tient pas
  (largeur naturelle, pause/aller/pause/retour)
- `src/spinner.rs` — roue de chargement du coin haut droit, dessinée au `canvas`,
  visible seulement pendant les téléchargements (`covers::busy`)
- `src/cursor.rs` — `Idle` : cache le pointeur quand il n'a pas bougé (enveloppe
  d'interaction au-dessus de toute la vue) + relève qui donne l'heure au repos
- `src/app.rs` — état, messages, bus interne, vue iced (grille + écran noir)
- `src/mode.rs` — mode de session : console (session nue) ou bureau, déduit de
  `XDG_SESSION_DESKTOP`/`XDG_CURRENT_DESKTOP` (`GLAUNCHER_MODE` pour le banc)
- `src/state.rs` — type de vue, tri **et portée** (tous / dossiers choisis /
  disques scannés) retenus (`<cache>/state.txt`)
- `src/copy.rs` — copie d'un paquet vers un autre dossier, en tâche de fond :
  destination (téléchargements XDG ou dossier de jeux), place libre
  (`statvfs`), règle de date, écriture `.part` + renommage, progression par
  morceaux de 8 Mio
- `src/status.rs` — indicateurs du coin haut gauche : manettes connectées
  (icône par famille, dessinée au canvas) et état du réseau lu dans
  `/sys/class/net`
- `assets/glyphs/{xbox,playstation}/*.png` — glyphes Xelu (CC0), embarqués
  avec `include_bytes!`
- `assets/glauncher.svg` + `.png` (512) — icône de l'application, à la charte
  Gablue (tuile dégradée `#4a86d8` → `#24549a`, coins arrondis, marque blanche) :
  un **G** et une épée à 45° détachée par un liseré du fond. Le SVG est la source,
  le PNG sert aux `.desktop` et aux barres des tâches.
- `assets/star.svg` + `star.png` (128) — étoile des favoris, embarquée.
- `assets/sparkle.png` (128) — étoile de la poussière du fond : halo, cœur et
  quatre branches, blanc, uniquement de l'alpha (la teinte vient de l'opacité).
  Générée une fois (gaussiennes + branches, cf. l'historique du 20/11) ; la
  refaire n'a d'intérêt que pour changer la forme du scintillement.

## Page Réglages (hors jeu)

Le launcher porte ses propres réglages, **hors partie** (les réglages qu'on
veut pendant une partie vivent dans `gpad`). Aujourd'hui : les **dossiers de
jeux**, les **dossiers de sortie** (enregistrements, captures d'écran),
l'**écran du jeu** et l'**entretien** (scan des disques externes, oubli des
jeux disparus). Quatre sections titrées (Dossiers de jeux / Sortie /
Affichage / Entretien), les en-têtes n'étant pas sélectionnables ; le compteur
« position / total » et l'aide contextuelle suivent la ligne. Ouverture : `F2`
au clavier, ou le menu des vues (triangle) → « Réglages… » — aucun bouton
manette n'était libre.

- **Liste** : « Ajouter un dossier… » puis les dossiers de `config.game_dirs`.
  `Croix`/Entrée choisit, `Carré` **arme** le retrait, `Rond`/Échap ferme.
- **Retrait confirmé** : le premier appui sur Carré affiche « retirer ? » et la
  ligne passe en rouge ; `Croix` retire, `Rond` désarme. Formulation à l'écran
  explicite : le dossier **et son contenu ne sont pas touchés**, seule la liste
  change. Changer de ligne désarme aussi (on ne confirme que ce qu'on voit).
- **Navigateur** (redessiné le 22/11, il était une simple liste de noms) :
  même présentation que la page Réglages — titre, compteur « ligne / total »,
  **fil d'Ariane** (trois derniers niveaux, le courant en accent), puis
  « ✓ Choisir ce dossier » en tête (l'action pour laquelle on est venu),
  « ↰ Remonter d'un niveau » et les dossiers préfixés « ▸ » (cachés écartés,
  tri alphabétique). Dossier sans sous-dossier : une ligne le dit. `Croix`
  entre (ou choisit la ligne 0), `Rond` remonte d'un niveau et **referme à la
  racine** (jamais de cul-de-sac). En remontant, le curseur se pose sur le
  dossier quitté.
- **Écriture** : `config::save_game_dirs` ne réécrit **que la clé
  `game_dirs`** — commentaires, ordre et clés inconnues du fichier sont
  conservés, comme l'écrivain de Gablue Manager : deux écrivains sur le fichier
  d'un compte doivent être interchangeables. Écriture atomique (temporaire +
  renommage), le manager lit le même fichier.
- **Effet immédiat** : `config.game_dirs` mis à jour, `Veille::definir` pour la
  surveillance (ajout et retrait à chaud), puis rescan complet — un dossier
  retiré doit faire disparaître ses jeux.
- **Dossiers de sortie** : deux lignes après les dossiers de jeux — *Dossier
  des enregistrements* (`dossier` de `~/.config/gablue/recorder.conf`, partagé
  avec `gpad-rec`, `grec` et Gablue Manager) et *Dossier des captures d'écran*
  (`dossier_screenshot`). `Croix` ouvre le **même navigateur** à partir du
  dossier courant (ou du défaut) ; `Carré` **retire la clé** : le dossier
  redevient le défaut du démon et de `screenshot.sh`, sans écrire un chemin en
  dur (le défaut peut changer avec l'image). L'écriture ne touche qu'à sa clé,
  commentaires et clés inconnues préservés (`src/recorder.rs`, même contrat que
  `save_game_dirs`). Un `~` littéral n'est jamais écrit : les chemins viennent
  du navigateur, donc absolus.
- **Son** : deux lignes entre la sortie et l'affichage. *Musique de fond* —
  `Croix` coupe ou relance, appliqué **tout de suite** (couper envoie `Stop` au
  fil de lecture, qui garde sa position : relancer reprend la piste où elle
  était) puis écrit `music = true|false`. *Volume de la musique* — au
  **gauche/droite**, par pas de 5 % (0 à 100), appliqué à chaud par
  `music::Music::volume` (la rampe de fondu suit, `mpv` ne redémarre pas) et
  écrit dans `music_volume`. Le retour de partie ne relance la musique que si
  `music` est vrai et le volume non nul : une musique coupée dans les Réglages
  ne revient pas après un jeu.
- **Écran du jeu** : dernière ligne de la page, réglée au **gauche/droite** —
  `Comme Plasma` (défaut : KWin applique la disposition enregistrée, étendue ou
  clonée), `Cloner` (réplique de l'écran principal), `Principal seul` (les
  autres sorties éteintes). La préférence n'est pas appliquée tout de suite :
  elle vit dans `~/.config/gablue/gaming.conf` et `gablue-gaming-launcher`
  l'applique au démarrage de la session console (`kscreen-doctor`, une seule
  invocation atomique). Le launcher ne parle jamais à KWin directement pour ça :
  il appelle `gablue-display` (`status`/`set`), seule vérité du format et de la
  validation (`GLAUNCHER_DISPLAY_HELPER` pour le banc).
- **Session console : pas d'image perso.** Le menu d'images n'affiche pas
  « Choisir un fichier… » (ni la touche `F`) : le sélecteur du bureau
  (`kdialog`/`zenity`) n'existe pas dans une session KWin nue, et une image
  perso se choisit sous Plasma. `Message::PickerFile` est inerte par sécurité
  (le clavier virtuel pourrait taper `f`).
- **Session console : clavier virtuel automatique.** Les prompts de saisie
  (identification SteamGridDB, date de sortie) n'ont pas de clavier physique :
  `IdentifyOpen`/`DateOpen` lancent `gpad-clavier`, qui ouvre le clavier de
  l'overlay **résident** (frappe uinput dans le champ qui a le focus).
  Surcharge : `GLAUNCHER_KBD_CMD` (banc). Au bureau, rien n'est lancé.
- **Banc** : `GLAUNCHER_SETTINGS_DEMO=1` ouvre la page, `=browser` ouvre en
  plus le navigateur sur `$HOME`, `=son` pose le curseur sur la ligne de la
  musique (vérifié au banc : `Croix` → `music = false` écrit, commentaires
  conservés ; volume → `music_volume = 65`) ; la fenêtre glissante (`fenetre()`) remplace
  une zone défilante à état, donc rien à reconstruire d'un rendu à l'autre.
  `GLAUNCHER_RECORDER_CONF` redirige `recorder.conf` vers un bac à sable.

## Galerie (captures, enregistrements, replays)

Section **séparée** (menus des vues → « Galerie… », triangle/T), plein écran
en session console. Deux dossiers lus dans `recorder.conf` : `screenshot_*.png`
(captures) et `Video_*.mp4` / `Replay_*.mp4` (enregistrements, replays), triés
du plus récent au plus ancien. Un seul flux, chaque tuile affiche **deux
lignes** : le nom du jeu quand la capture le connaît (métadonnée), puis
`genre · date · poids` ; sans nom de jeu, `genre · poids` puis la date
**locale au propre** (« 22 nov. 2026 · 22:31 ») — **jamais le nom du fichier**
(`Video_20261120_101500.mp4` ne dit rien à personne). Le fond garde la
**poussière d'étoiles** du launcher (`sparkles`), posée au-dessus de la
grille : un fond noir opaque l'aurait effacée.

- **Vue par jeu** : le bouton **`Galerie`** de la page d'un jeu (touche `G`)
  n'ouvre que **ses** captures et vidéos (`Galerie.jeu`, `GalleryOpen(Some(nom))` ;
  `None` pour la galerie complète des menus de vues). Le filtre est appliqué
  **dès l'ouverture** (`gallery_apply_filter`), donc les captures des autres
  jeux ne s'affichent pas même une image ; en-tête « Galerie <jeu> », légende
  et compteurs recalculés sur ce périmètre, et le Triangle continue de filtrer
  captures/vidéos **du jeu**. La clé de rapprochement est le **nom affiché du
  jeu = la tige du `.wgp`/`.lgp`** (`row.name`) : c'est la même chaîne que
  `nom=` de `running.txt` et donc du sidecar `<capture>.jeu`, donc ça marche
  aussi pour un jeu **non identifié sur SteamGridDB** (son seul nom est celui
  du fichier). Un paquet **renommé** perd ses anciennes captures dans cette
  vue (elles restent dans la galerie complète).
- **Comptage en fond** : le bouton n'apparaît que si le jeu a quelque chose à
  montrer, or la page ne doit pas parcourir les dossiers de sortie (des
  milliers de fichiers) à chaque ouverture. `spawn_comptes_galerie` compte par
  jeu (`gallery::scanne` → `Message::GalerieComptes`), au démarrage, à
  l'ouverture d'une page (au plus toutes les 20 s) et à la fermeture de la
  galerie (une capture mise à la corbeille fait disparaître le bouton).

- **Nom du jeu et logo** : `screenshot.sh`, `recorder.sh` et `replay.sh`
  écrivent un sidecar `<fichier>.jeu` (`nom=`, `logo=`) en lisant
  `running.txt` (launcher d'abord, puis `gwine`/`lgplaunch`), avec les PID
  comme garde (un fichier qui traîne après un plantage n'étiquette rien).
  Les captures `gpad-shot --titre=` portent en plus un `iTXt Title` dans le
  PNG ; la galerie lit le sidecar, puis cette métadonnée pour les anciennes
  captures. Le nom est **dans la galerie**, jamais gravé dans les pixels.
- **Filtre** : `Triangle` cycle tout → captures → vidéos (`GalleryFilter`,
  aussi `Tab`). C'est lui qui rend les captures accessibles quand une
  bibliothèque de milliers de vidéos les enterre en fin de tri (constaté le
  23/11). **Taille fixe** : pas de zoom dans la galerie — un cran de zoom sur
  une grille plein écran n'apportait rien (retiré le 23/11, `galerie_geometrie`
  ne prend plus de paramètre).
- **Confirmation d'effacement** : `Carré` ouvre un **panneau centré**
  (`gallery_confirm_view`) — nom/date/poids et deux glyphes, Croix = corbeille,
  Rond = annuler. Tant qu'il est ouvert, aucun autre bouton n'agit ; la tuile
  armée porte un liseré rouge. Pas de texte « effacer ? » sous la tuile (il
  décalait la grille).
- **Corbeille, jamais de suppression** : `gallery::mettre_a_la_corbeille`
  range le fichier dans la **corbeille XDG** (`~/.local/share/Trash/files` +
  `.trashinfo` URL-encodé, date locale, suffixes `.1`, `.2` en cas de
  collision) ; le sidecar `.jeu` suit. Échec = fichier laissé en place.
- **Copie partageable** : dans l'aperçu, `Carré` (ou `C`) écrit
  `<original>-logo.png` à côté : le screenshot avec le **logo du jeu** incrusté
  en bas à droite (18 % de la largeur, marge 2,5 %, jamais plus de 30 % de la
  hauteur), écriture atomique, sidecar recopié. L'original n'est pas touché ;
  sans logo connu (capture hors jeu), le bouton n'est pas proposé.

- **Vignettes** (`src/gallery.rs`) : fabriquées sur un fil de fond, une par une
  (`gallery::spawn`), pas sur le fil de l'interface — iced décode en plus sur un
  seul thread, et une capture 4K pèse 5 à 15 Mo. Réduction à 320 px en JPEG
  qualité 80 ; pour une vidéo, `ffmpeg` extrait une image à 0,5 s (déjà présent
  pour le son de capture). Cache `<cache>/gallery/<hash>.jpg`, clé = chemin +
  mtime (un fichier remplacé est refait). **À la demande** : seules les
  vignettes de la fenêtre visible (+ un rang de marge) sont demandées
  (`gallery_prepare`, `prepared`), sinon ouvrir une galerie de milliers de
  vidéos lancerait autant de `ffmpeg`. Rien n'anime la grille : pas de
  respiration, cadre de sélection fixe.
- **Géométrie** : `galerie_geometrie(largeur)` (pure, testée) choisit le
  nombre de colonnes pour une tuile visée de ~300 px (2 à 10) et **étale les
  tuiles sur toute la largeur**. Avant, 6 colonnes de 240 px figées : sur un
  écran 2K la galerie s'arrêtait « à la zone 1080 », ~1500 px au milieu de
  2560 (constaté le 22/11). À 1080p : 6 colonnes ; à 2K : 8.
  **Rangées** : `lignes_vues` se calcule sur la hauteur réellement libre
  (`hauteur - GALERIE_CHROME`, 173 px mesurés : padding 2×20, titre,
  sous-titre, compteur, aide et leurs interlignes), pas sur une constante
  pessimiste. `pas` = `tuile_h + 77` (vignette + interligne + légende + marge
  + 10 px entre rangées) : le `+86` d'avant, avec un chrome de 290, ne
  laissait que **4 rangées en 2K alors que la 5ᵉ tenait** (constaté le 23/11).
  Résultat : 5 rangées à 2560×1440, 3 à 1080p.
- **Ouverture** : `Croix` sur une capture → aperçu plein écran dans le launcher
  (image **pleine**, pas la vignette) : l'image est là **tout de suite**, seule
  une entrée de 0,16 s (opacité 0,70 → 1 et échelle 1,03 → 1) l'accompagne,
  pilotée à **60 Hz** par `tick` (`Apercu.entree`, dans `animating`). Avant,
  elle suivait le minuteur de la galerie (8 Hz) : il fallait une seconde
  entière pour la voir — jugé trop long (23/11). Gauche/droite passe à la capture
  précédente/suivante. Une vidéo → **voile noir séparé** (`video_veil`, sans
  l'« hyperespace » des étoiles qu'on voyait filer ~1 s), `mpv` lancé tout de
  suite, et le voile ne se lève que 400 ms après (`video_lift_at`) : il couvre
  l'animation d'ouverture de la fenêtre de mpv, qui sinon « venait de bas à
  droite » (constaté le 23/11).
- **Lecture vidéo** (`src/play.rs`) : `mpv --no-config --fullscreen --osc=no`
  avec une socket IPC. Le launcher garde la main : **Croix = pause**, **gauche/
  droite = ±5 s** (`seek … relative`), **Rond = sortie** (puis délai de grâce
  de 3 s avant `SIGKILL`). La manette est lue en evdev direct, donc elle marche
  même quand la fenêtre de `mpv` a le focus (la garde de focus est court-
  circuitée quand un lecteur tourne). **Fin de vidéo : la fenêtre reste**
  (`--keep-open=yes`) — le dernier plan est affiché à l'arrêt, c'est `Rond` qui
  referme (puis `try_wait` relève la sortie, le voile se lève et le launcher
  reprend le focus) ; avant, la fenêtre disparaissait au moment où l'on
  regarde la fin. **Progression** : `--osd-level=3` fait afficher à `mpv`
  « ▶ 00:01:12 / 01:44:20 (69 %) » à chaque saut (`--osd-duration=2500`,
  police 44) — mesuré au banc : le niveau 1 n'affiche rien et le niveau 2 la
  position seule, d'où l'impression d'avancer à l'aveugle.
- **Effacement confirmé** : `Carré` arme (« effacer ? Croix = oui »), `Croix`
  efface pour de bon, `Rond` annule, **tout déplacement désarme**. Le fichier
  est supprimé du disque (`remove_file`) : c'est la seule action destructive du
  launcher, d'où le double geste.
- **Banc** : `GLAUNCHER_GALLERY_DEMO=1` ouvre la grille au démarrage,
  `=apercu` ouvre aussi la première capture, `=video` la première vidéo
  (`GLAUNCHER_RECORDER_CONF` redirige le fichier des dossiers).

## Contrats

- **Les aides de commande restent en glyphes en session console** : en console
  il n'y a pas de clavier physique, mais un mouvement de souris (gpad-mouse,
  pointeur du jeu) basculait l'état d'entrée en « clavier » et les pages de jeu
  affichaient des aides en texte, sans glyphes (signalé le 23/11/2026). Le
  critère `pad_hints()` accepte donc l'état souris quand `mode.is_console()`.
- **Page de jeu : logo borné, bloc centré, aide en bas** : le logo suivait
  `ech` sans plafond et poussait les pastilles puis la ligne d'aide hors du bas
  de page — sa hauteur est plafonnée à 150 px. Le bloc (cover, textes,
  pastilles) est **centré dans la place libre au-dessus de l'aide**, bande
  héros comprise : posé en bas (23/11), il descendait trop et tout l'espace
  gagné avec une bannière partait sous lui. L'aide de commande reste la
  dernière ligne, à 8 px du bord (demandé le 23/11). Implémentation : une
  colonne `[milieu (Fill, center_y), pied]`, et non plus un `align_y(End)`
  conditionnel.
- **Recherche par nom (`/`)** : le champ s'ouvre au clavier, filtre à la
  frappe (`normalise` : minuscules, sans accents ni ligatures — « epee »
  trouve « Épée », « coeur » trouve « Cœur »), Échap efface et referme, Entrée
  ouvre le jeu sélectionné. La barre reste affichée tant que le filtre est
  actif, pour qu'une liste courte ne passe pas pour une bibliothèque vide.
  **Clavier uniquement** : la manette ne tape pas, et le launcher n'a pas de
  clavier virtuel — un filtre par roue de lettres serait une autre fonction.
- **Ordre « Derniers lancés »** : `recent.txt` dans le cache du launcher
  (`slug<TAB>epoch`, atomique, 24 entrées), écrit à chaque lancement. C'est un
  ordre de tri à part entière, **distinct de `Récents`** (date du paquet) ; il
  survit aux sessions (état `tri=derniers`). Le menu Tri porte une ligne
  **« Effacer les derniers lancés »**, présente seulement si l'historique
  n'est pas vide ; elle vide l'historique, **jamais** `playtime.txt`.
- **Barre de défilement pilotée par le périphérique** : `barre` démarre
  **éteinte** — un launcher ouvert à la manette n'a pas de barre à montrer
  (signalé le 23/11 : elle était là sans qu'on ait touché à la souris). Elle
  s'allume sur un **vrai** geste de souris (mouvement réel, clic, molette) ou
  une touche, et s'éteint au premier bouton de manette. Le geste est publié par
  l'enveloppe `cursor::Idle` (`.on_geste(Message::Souris)`), la seule à savoir
  distinguer un mouvement du `CursorMoved` synthétique que KWin envoie quand
  une surface apparaît sous le pointeur ; `.muet(self.barre)` coupe le message
  quand la barre est déjà là (sinon chaque mouvement du pointeur
  reconstruirait la vue). Style `barre_style` = rail et curseur transparents,
  marges conservées : la disparition ne décale pas le contenu d'un pixel.
- **La vue LISTE a sa propre présentation** : lignes en cartes (pochette,
  titre, année et temps de jeu alignés à droite, étoile), filet d'accent sur la
  ligne choisie, et **trois crans de taille** (`liste_taille` dans
  `state.txt`, 38/56/78 px d'art) pilotés par L2/R2 et `+`/`−` — le zoom
  modifiait les colonnes de la grille, jamais la liste (signalé le
  23/11/2026). Le panneau de droite montre la **bannière héros** (repli :
  pochette plafonnée) dans une carte qui prend sa hauteur et se centre :
  la cover pleine hauteur écrasait la liste.
- **Aides au clavier en pastilles de touche** (`key_hint`) : « Entrée »,
  « Échap », « F », « ←↑→↓ »… sont dessinées comme de petits capuchons
  arrondis, suivis de l'action — le pendant exact des glyphes de manette
  (`hint`). Avant, le clavier avait de longues phrases en texte nu à côté des
  icônes de boutons : deux langages visuels pour le même écran (signalé le
  23/11). Partout où `pad_hints()` choisit les glyphes, la branche clavier
  passe par `key_hint` : pied de page, page d'un jeu, dialogues (remplacer,
  identifier, date, alias, choix d'image) et prompts de saisie.
- **« Oublier les jeux disparus »** (Réglages ▸ Entretien, dernière ligne) :
  efface les fiches des jeux qui ne sont plus dans la liste — favoris,
  historique (`recent.txt`), temps de jeu (les trois fichiers), années,
  identifications, réglages de lancement (`games.txt`) et lignes d'index
  (`Index::purge_missing`, chemins absents). **Les images ne sont jamais
  touchées** (`assets/`, cache des vignettes) : un jeu rebranché retrouve ses
  visuels, mais repart sans favori ni temps de jeu. Confirmation par défaut sur
  « Annuler ». Nom choisi pour être lisible sans jargon (« vider la base »).
- **Le menu de choix d'image défile avec la sélection** : la grille
  (`PICKER_COLUMNS` = 5) a un `scrollable` identifié (`PICKER_SCROLL`) et
  `PickerNav` demande `operation::snap_to` avec un décalage relatif calculé sur
  la rangée sélectionnée. Sans ça, la manette descendait la sélection sous le
  panneau : les dernières rangées étaient inatteignables (signalé le
  23/11/2026).

- **Mode console ou bureau, jamais une option** : `src/mode.rs` lit
  `XDG_SESSION_DESKTOP` (nom posé par `DesktopNames` de
  `files/system/all/usr/share/wayland-sessions/gablue-gaming.desktop`, session
  KWin nue **sans plasmashell**) et `XDG_CURRENT_DESKTOP` en repli.
  `GLAUNCHER_MODE=console|desktop` ne sert qu'au banc d'essai. Aucun drapeau
  de ligne de commande à écrire deux fois : le même binaire lancé depuis le
  bureau garde son comportement historique.
- **En session console, le launcher ne se ferme pas** : « retour » (manette) et
  `Échap`/`q` sont **inertes** — le launcher EST la session, donc le fermer
  laisserait un écran noir. La sortie passe par le **menu système de `gpad`**
  (Home maintenu : Mode bureau, Redémarrer, Éteindre), qui est joignable même
  quand le launcher a le focus (déclencheur evdev). L'aide du pied de page n'y
  affiche **aucune** mention de sortie (« Quitter (2×) », « Échap quitter »
  disparaissent) : proposer une sortie qui n'existe pas serait un mensonge.
  En mode bureau, rien ne change (double appui = fermer la fenêtre).
- **Sortie de session = code 0** : `/usr/libexec/gablue-gaming-launcher` relance
  le launcher tant qu'il sort en erreur (plantage = relance, une console ne
  laisse pas un écran noir) et rend la main à KWin (`--exit-with-session`) dès
  que le code est 0. Ne pas toucher à ce contrat sans mettre à jour
  `/usr/libexec/gablue-gaming-session`, qui en dépend.

- **Nom affiché** : le nom du fichier sans extension, jamais `.wgp`/`.lgp`, et
  aucun badge de format sur la vignette.
- **Affichage** : `view = "grid"` (vignettes + nom), `"list"` (lignes, icône à
  gauche, grand aperçu à droite) ou `"mosaic"` (covers collées, sans nom, façon
  Switch Online) ; bascule V/Tab ou `BTN_NORTH` (Y/Triangle) sans relancer,
  dans l'ordre grille → liste → mosaïque. En liste, haut/bas avancent de 1,
  dans les deux grilles d'une rangée (`metrics().2`).
- **Remplissage** : la grille occupe toute la largeur de la fenêtre.
  `columns` = nombre MAXIMAL de colonnes, `cell_size` = largeur MINIMALE d'une
  vignette ; le reste est étalé (agrandir la fenêtre ne laisse pas de bande
  noire). **7 colonnes par défaut** (6 jusqu'au 23/11 : covers ~20 % plus
  petites demandées, 250 → 205 px de large sur 1920 px, une de plus par
  rangée ; 5 avant le 22/11, les covers mangeaient l'écran et le fond ne se
  voyait plus). Le nombre de colonnes est ENTIER : 7 donne −18 % de largeur
  (8 donnerait −31 %), c'est le cran le plus proche de la demande.
  En mosaïque, `columns` ne s'applique pas : `cell_size × 0.55`
  règle la densité. `+`/`−` (ou L2/R2) : en mosaïque ils changent `cell_size`
  en direct (bornes 120 à 520 px), ailleurs ils changent le **nombre de
  colonnes** (un cran = une colonne, bornes 3 à 12) et `cell_size` est
  recalculée À L'ENVERS (`cell_for_columns`) pour produire exactement ce nombre
  de colonnes.
  Pourquoi : `cell_size` n'est qu'une largeur MINIMALE — la fixer sur la largeur
  affichée plafonnait `fitted` au nombre de colonnes courant, donc le zoom
  arrière ne pouvait plus jamais en rajouter (constaté 18/11 : colonnes 5 → 6 en
  mémoire, 4 à l'écran, « je ne peux toujours pas dézoomer »).
- **Changement de taille ANIMÉ** (`Zoom`, `ZOOM_SECONDS` 0,16 s) : la nouvelle
  disposition est posée tout de suite, et chaque vignette **glisse** de
  l'ancienne case vers la nouvelle — position **et** taille — via `Pose`
  (`App::zoom_pose`). Le calcul se fait sur la **cover**, pas sur la case
  (`zoom_transform`) : à l'instant 0 la cover occupe exactement son ancien
  rectangle, l'écart se referme ensuite (échelle → 1, décalage → 0). Un cran
  pendant l'animation repart de la position **visuelle** courante
  (`layout_blend`) : appuyer sur R2 plusieurs fois ne fait aucun saut. Le cadre
  du canvas suit la même transformation (`sparkles::Breath::offset`) et la
  respiration se multiplie à l'échelle de la glissade. La liste n'anime rien
  (le zoom n'y change pas la taille des vignettes). `GLAUNCHER_ZOOM_MS` change
  la durée sans recompiler (40 à 2000 ms).
- **Marge de droite et taille des boîtes** : `usable_width()` retire la marge
  gauche et une marge droite plus large (`SCROLLBAR_GUTTER`), parce que la
  barre de défilement d'iced est une surimpression qui ne réserve aucune place —
  sans elle, la colonne de droite passait dessous (constaté 20/11). `CARD_INSET`
  (9 px) est une marge **transparente** entre la case et la boîte visible : la
  vignette rétrécit et l'air entre les boîtes augmente sans changer le nombre de
  colonnes. Tout ce qui situe une vignette utilise le pas de rangée
  `slot_height()` (boîte + ses deux marges) et non `row_height()`.
- **Sélection très visible** : cadre d'accent de 4 px (`FOCUS_BORDER`) **plus**
  fond teinté, sur les vignettes de la grille comme sur les lignes de liste — le
  fond seul se perdait parmi 666 vignettes (retour 18/11 « la sélection n'est
  pas assez visible »). En mosaïque (noms masqués, la sélection se lit au
  cadre) : cadre d'accent de 6 px **plus** un voile d'accent à 22 % posés **au-dessus** de la
  cover, aux dimensions explicites. Un cadre dessiné par le conteneur passait
  derrière l'image, et un `Fill` dans un `stack` se réduisait à zéro (retour
  18/11 : « on ne voit pas ce qui est sélectionné »).
- **Sans cover** : sample portrait (600×900, `assets/cover-sample.png`) à la
  taille exacte d'une cover, avec l'**icône du paquet** en petit (ratio
  conservé) et le **nom du jeu** par-dessus. Le sample ne porte **aucun
  texte** : le mot « glauncher » qu'il affichait en bas faisait doublon avec le
  nom du jeu et salissait toutes les vignettes sans cover (retiré le 22/11) —
  seul compte le dégradé, la tuile et le rond qui accueille l'emblème.
  Jamais d'image étirée.
- **Icône** : `<nom du paquet>.png` à côté du paquet, puis `icons/<slug>.png`,
  puis `<slug>.png` dans les `icon_dirs`, puis **`.icon.png` extrait du paquet**
  (`unsquashfs -no-progress -cat <pack> .icon.png`, ~15 ms) — même source que
  `create-game-shortcut.sh` de Gablue (squashfuse + repli icône d'exe là-bas).
  L'extraction est paresseuse (jeux affichés seulement, worker de fond sans
  délai de politesse) et le PNG atterrit dans `<cache>/icons/<slug>.png`. En
  liste c'est l'icône du paquet qui s'affiche, cover en repli.
- **Disparition** : un paquet absent (ou un dossier injoignable) sort de la
  liste au rescan. Le dossier, lui, reste dans la config — un disque externe
  débranché revient avec ses jeux dès qu'il est remonté (le veilleur re-teste
  les dossiers toutes les 2 s). **Rien ne le signale dans l'interface**
  (demandé le 20/11 : un « dossier introuvable » en pied de page, c'était
  moche) : la liste est simplement plus courte, sans message ni trou.
  Le veilleur mémorise **device + inode** de chaque dossier surveillé : un
  disque monté sur un point de montage qui existait déjà (dossier vide) garde
  le même chemin mais change d'inode, et la watch inotify de l'ancien inode ne
  reçoit plus rien. Un inode différent = watch reposée et dossier réindexé
  (constaté 20/11 : « si je branche le disque, rien ne se recharge »).
- **Assets** : `~/.local/share/glauncher/assets/` contient les visuels
  SteamGridDB (`grids/`, `heroes/`, `logos/`, `icons/`) **et trois fichiers
  texte volontairement partagés** : `years.txt` (années de sortie),
  `identities.txt` (jeu SteamGridDB de chaque paquet) et `absents.txt`
  (recherches négatives **datées**, voir *Identification d'un jeu*) : c'est le
  dossier **partageable** (Syncthing entre machines et entre amis), donc les
  visuels, les dates, l'identification et les « introuvables » suivent.
- **Nom des visuels = le JEU, pas le paquet** (`covers::key`) :
  `<id SteamGridDB>-<nom slugifié>` (`8103-doom.jpg`), ou `<id>` seul si le nom
  manque ; jeu jamais identifié → slug du paquet (visuels locaux). Pourquoi :
  deux `.wgp`/`.lgp` du même jeu nommés différemment (postes, amis via
  Syncthing) doivent partager les **mêmes** fichiers — le slug du paquet, lui,
  change avec le nom de fichier. L'id en tête évite que « Doom » 1993 et 2016
  se marchent dessus. `covers::key_of` (table) et `gpad::cle_visuels` (relecture
  de `identities.txt`) appliquent la MÊME règle : les deux doivent rester
  d'accord. `covers::cached_for` accepte aussi le fichier `<id>` seul (écrit
  avant que le nom soit connu). Miniatures, marqueurs `.miss`/`.choice` et
  `pack-icons/` restent par paquet (propres à la machine).
- **Apprentissage automatique** : le premier résultat de l'autocomplete (ou la
  fiche d'un id) est retenu dans `assets/identities.txt` par le worker
  (`CoverEvent::Identified`) et ses visuels déjà là sont **renommés** vers la
  clé (`rename_to_key`). Sans ça le redémarrage — qui ne fait aucune requête
  réseau — ne saurait plus sous quel nom les fichiers sont rangés.
  `covers::migrate_names` fait le même renommage au démarrage pour les
  installations antérieures (et re-clé `years.txt`), **sans jamais écraser**
  (une clé déjà présente gagne : c'est l'image arrivée de l'autre poste).
- **Le reste est hors du dossier partagé** : sorti au démarrage par
  `Config::migrate_cache` — miniatures
  (`<cache>/thumbs/`), icônes de paquets (`<cache>/pack-icons/`) et marqueurs
  (`<cache>/marks/<type>/` pour `.miss` et `.choice`). Pourquoi : les
  miniatures se régénèrent en boucle, les icônes de paquets viennent des
  paquets de la machine, et un `.miss` synchronisé masquerait un visuel 30
  jours chez l'autre. `migrate_cache` range aussi l'ancienne disposition
  (`covers/`, `icons/` à la racine du cache) — sans ça les covers déjà choisies
  seraient perdues. Rien n'est jamais supprimé : un visuel survit à la
  disparition du jeu comme du dossier (seul `covers::forget`, après une
  réidentification, retire les visuels d'un jeu).
- **Écriture atomique** : `covers::save` écrit `<fichier>.part` puis renomme
  (`write_atomic`), et les marqueurs aussi. Un dossier `assets/` synchronisé
  ne doit jamais propager une image à moitié écrite.
- **Copier un paquet** (`C` sur la page, ou `Options ▸ Copier le jeu…`) :
  panneau des destinations — **Téléchargements** (`XDG_DOWNLOAD_DIR` de
  `~/.config/user-dirs.dirs`, repli `~/Téléchargements` puis `~/Downloads`,
  jamais `xdg-user-dir` qui n'existe pas en session console) puis chaque
  dossier de `game_dirs` — avec la **place libre** de chacun (`statvfs`,
  `f_bavail` : ce que l'utilisateur peut réellement écrire) et la taille du
  paquet en titre. L'original n'est **jamais** déplacé ni modifié.
  - **Règle de date** : destination vide → copie ; fichier présent et **plus
    ancien** que la source → question « Mettre à jour ? » (« Annuler » par
    défaut) ; fichier présent à jour (même date ou plus récent) → « déjà
    présent, rien à copier », aucune écriture.
  - **Place insuffisante** dite sur la ligne (rouge) et au moment de valider,
    en comptant ce qui reste à écrire des copies déjà en file : deux jeux
    lancés d'affilée ne doivent pas dépasser ensemble l'espace libre.
  - **Écriture en `.part` puis renommage** : un fichier à moitié copié porte
    le nom d'un paquet qu'il n'est pas (la bibliothèque l'indexerait et le
    launcher tenterait de le lancer) ; la taille est vérifiée avant le
    renommage, un `.part` de mauvaise taille est retiré.
  - **File et progression cumulée** : une seule bande de **20 px** tout en haut
    de la fenêtre (`copies`, `BANDE_COPIE`), « copie de <jeu> · 42 % ·
    2,9 Gio sur 6,0 Gio ». Le pourcentage porte sur **toutes** les copies
    demandées (total = somme des tailles, `fait` avance à chaque copie
    terminée), d'où une barre qui va de 0 à 100 % au lieu de trois barres
    successives. Deux demandes vers la **même destination** ne se doublent pas
    (`en_cours`), sinon deux fils écriraient le même `.part`. Une copie
    cassée arrête la file et le dit ; une file vide termine par « N jeux
    copiés · poids ».
  - La bande existe **toujours** dans la pile racine (transparente au repos) et
    son enveloppe est en `Fill` + `align_y(Start)` : un enfant de hauteur fixe
    posé directement dans une `stack` n'est pas collé au bord.
  - Un paquet copié dans un `game_dirs` apparaît tout seul (le veilleur le
    voit, le fondu de réindexation le pose).
  - **Banc** : `GLAUNCHER_COPY_DEMO=1` ouvre le panneau, `=go` copie le jeu
    sélectionné vers la première destination, `=file` en fait deux (deux jeux,
    pour la progression cumulée).
- **Indicateurs d'état, coin bas droit** (`src/status.rs`, `App::coin_bas_view`) :
  l'icône du **réseau** puis une **icône par manette** connectée (quatre au plus,
  un `+N` au-delà), **suivies des deux roues** (Scan puis Visuels) — le pied de
  page laisse cette zone libre, le tri et le compte sont partis à gauche. La
  couche est posée **avant le voile** du lancement : les icônes s'assombrissent
  avec le reste de l'écran au lieu de rester en pleine couleur sur le noir
  (signalé le 24/11). Aucune image embarquée : tout est dessiné au canvas (arcs
  du Wi-Fi, prise du filaire, arcs barrés hors ligne ; pour la manette un
  **tracé unique** — coque bombée, deux poignées qui descendent, creux central —
  puis croix à gauche et deux boutons à droite ; les trois formes de la
  première version — corps + deux ronds — se lisaient comme une tache,
  signalé le 24/11), teinté par famille (`gamepad::PadKind` : vert Xbox, bleu
  PlayStation, gris pour une manette non reconnue).
  - Réseau lu dans `/sys/class/net` : une interface **physique** (dossier
    `device`) et `operstate == up` — les interfaces virtuelles (`docker0`,
    `virbr0`, `tailscale0`, `veth…`) sont actives sans câble et feraient
    croire à un réseau en ligne. Un guetteur relève l'état toutes les 3 s et
    n'envoie un message qu'au changement (`status::surveiller`) : `/sys` est
    gratuit, aucune sonde réseau, aucun NetworkManager par DBus.
  - Les manettes viennent des évènements `evdev` existants (`pads`), donc rien
    à sonder ; bleuté/grisé selon la famille, `MUTED` quand tout va bien côté
    réseau n'était pas lisible — l'icône passe en `ACCENT` dès qu'un lien est
    actif.
- **Disques externes (`scan_disks`, décoché par défaut)** : coché, les
  **points de montage** sous `/run/media/<utilisateur>/` et
  `/media/<utilisateur>/` sont ajoutés aux dossiers scannés —
  `disks::racines` lit `/proc/self/mountinfo` (aucune commande externe) puis
  cherche, par disque, les dossiers porteurs de `.lgp`/`.wgp` : profondeur 4,
  [`disks::BUDGET`] = 4000 dossiers visités au plus, `node_modules`, `.git`,
  `lost+found`, `$RECYCLE.BIN`… sautés. **La découverte est mémorisée**
  (`<cache>/disks.txt`, `<montage>	<racine>`) y compris pour les disques
  débranchés : un rebranchement ne coûte qu'un `stat` par racine connue, pas un
  parcours. Un dossier de jeux **créé depuis le dernier branchement** est
  trouvé par un second passage borné (`disks::nouveaux`), lancé **après** le
  rafraîchissement (`spawn_refresh`, phase 2, seulement au branchement) —
  les jeux connus sont déjà à l'écran quand il commence.
  **Décochée, seuls les `game_dirs` comptent** : l'option vidée, un scan
  complet (`replace_all`) retire de l'index les paquets des disques externes,
  et les racines ne sont plus surveillées — c'est la clé de l'index
  (`game_dirs` seulement, `Config::index_path`) qui reste stable, elle ne
  bouge pas au branchement (sinon chaque disque créerait une base neuve).
  Deux dynamismes : le **guetteur** `disks::surveiller` (lecture de
  `mountinfo` toutes les 2 s, rien à entretenir) envoie `Message::Disks` quand
  l'ensemble des montages change → `refresh_disques` (découverte + scan), et
  la surveillance inotify couvre `scan_dirs()` (dossiers choisis **et** racines
  trouvées) donc un paquet ajouté sur un disque déjà vu arrive tout seul.
  `--reindex` (CLI) fait la même découverte, pour mesurer sans fenêtre
  (banc : 68 ms la première fois, 2 ms ensuite sur le T5, 646 paquets).
  La découverte tourne dans le fil d'index, jamais sur le fil de l'interface ;
  au démarrage elle est lancée tout de suite seulement si l'option est
  cochée (`Message::Disks` au boot), sinon aucun accès aux disques avant la
  première image.
- **Aucun dossier de jeux par défaut** : `Config::default()` laisse
  `game_dirs` **vide** (idem `GLAUNCHER_DEFAULTS` côté Gablue Manager) — le
  launcher ne scanne que les chemins écrits dans `config.toml`, jamais
  `~/Games` ou `~/Windows/Games` de sa propre initiative. Liste vide =
  bibliothèque vide (demandé, 19/11/2026).
- **Dossiers de jeux** : `game_dirs` vit dans `config.toml`. Le choix se fait
  à la manette dans **Réglages ▸ Ajouter un dossier…** (navigateur, voir la
  section Réglages) : `config::save_game_dirs` ne réécrit que cette clé, en
  préservant commentaires et clés inconnues (même contrat que Gablue Manager,
  qui peut aussi l'écrire). Le veilleur inotify, lui, reste ici.
- **Miniatures (grille)** : `assets/thumbs/<slug>.jpg` — 320 px de large, JPEG
  qualité 80, générées par `src/thumbs.rs`. La grille, la mosaïque et les
  vignettes de liste affichent la **miniature** tant que la largeur demandée
  tient dans 320 px, la **cover pleine** au-delà (zoom max : aucune image
  agrandie). Pourquoi : `iced_wgpu` décode sur **un seul thread**, une fois par
  image et par session, au premier affichage — mesuré en release 3,18 ms pour
  une 600×900 contre 0,55 ms pour une 320×480 (×5,8), et 3× moins d'octets à
  téléverser. Deux voies comme les covers : `now` (jeux à l'écran) devant
  `sweep` (bibliothèque entière, pause de 40 ms entre deux images, remplacée à
  chaque réindexation) ; après quelques dizaines de secondes tout le cache est
  prêt. Invalidation : `.settings` (`w320-q80`) purge le cache quand on change
  largeur ou qualité, et une cover plus récente que sa miniature la refait
  (mtime). Sonde : `cargo test --release bench_decodage -- --ignored
  --nocapture`.
- **Options de lancement par jeu** (22/11) : trois lignes de plus dans
  `Options ▸`, dessinées comme dans l'overlay (interrupteur ou valeur, pas de
  texte « Croix = … ») — **Lancer dans gamescope**, **Résolution forcée**
  (cycle Croix : celle du jeu → 1080 → 900 → 720 → 480 → retour) et, **pour un
  `.wgp` seulement**, **Manettes en mode Xbox** (`gwine --xbox`, filtre
  `all` = toutes les manettes ; `lgplaunch` n'a pas d'équivalent, la ligne
  n'existe donc pas pour un `.lgp`). Les bascules **ne referment pas le menu** :
  l'état se voit changer sous les yeux. Elles sont écrites tout de suite dans
  `~/.local/share/glauncher/games.txt` (`src/pergame.rs`, une ligne par jeu
  `<slug>\tgamescope=1\tres=1080\txbox=1`), **jamais dans `assets/`** : ce
  dossier est partagé par Syncthing, alors qu'une TV 2K et un portable
  n'appellent pas les mêmes réglages. Fusion à l'écriture (une ligne d'une
  version future ou d'une autre session est conservée), ligne supprimée quand
  tout revient au défaut. `launch::build` ajoute les options **devant le
  paquet** (`--gamescope --resolution 1080 --xbox`), contrat commun à `gwine`
  et `lgplaunch` (ce dernier a reçu `--gamescope`/`--resolution` pour ça).
- **Lancement** : `launch_wgp`/`launch_lgp` + chemin du paquet en argument.
  `GLAUNCHER_LAUNCH_CMD` remplace toute la commande (tests).
- **Temps de jeu** (`src/playtime.rs`) : écrit **avec les sauvegardes**, pas
  dans le cache du launcher — `~/.local/share/lgp-saves/glauncher/playtime.txt`
  pour les `.lgp` (même racine que `lgplaunch`, ici un lien Syncthing) et
  `~/Windows/UserData/<user>/LocalSavesWGP/glauncher/playtime.txt` pour les
  `.wgp` (même racine que `gwine`). Pourquoi un dossier dédié et non le dossier
  du jeu : la clé de sauvegarde d'un paquet est son `GAMENAME` interne (skill
  `pack`), que le launcher ne connaît pas sans ouvrir le squashfs. Une ligne
  `slug<TAB>secondes` par jeu, écriture atomique ; le temps suit donc les
  sauvegardes au lieu de disparaître avec le cache. Un jeu n'écrit que dans le
  fichier de son format. L'ancien `<cache>/playtime.txt` est **migré au
  démarrage** (le format de chaque slug est alors connu) ; les jeux absents de
  la liste y restent et seront rangés quand ils reviendront, rien n'est perdu.
  À la lecture, le plus grand total gagne entre les fichiers (format changé,
  fichier recopié) : additionner compterait deux fois une partie. Un lancement
  de moins de 5 s ne compte pas.
- **Mise à jour de l'image** (`src/updates.rs`) : `M` (clavier seul tant que
  le menu réglages n'existe pas) lance `sudo -n bootc upgrade` — autorisé sans
  mot de passe par `sudoers.d/nopasswd` — et lit sa sortie sur le protocole
  `::gablue:` (`step n total libellé`, `busy`, `percent`, `done`), **le même
  que les dialogues de Gablue Manager** (`parse_gablue_progress`) : une seule
  vérité, deux rendus, une commande branchable sur l'un ou l'autre sans
  réécriture. `step` compte en `(n-1)/total` (100 % seulement au `done`),
  `busy` = barre indéterminée (35 %), `percent` = mesuré. Fin : code 0 →
  « prête — s'appliquera au redémarrage » (bootc **stage**, jamais appliqué à
  chaud), « no changes » → « déjà à jour » ; code ≠ 0 → barre pleine en rouge
  et la **dernière ligne de journal** (`detail`) plutôt qu'un code de sortie.
  La barre vit au-dessus de l'aide du pied de page, **16 px de haut même au
  repos** (le pied de page ne saute pas), son libellé **à côté d'elle** : dans
  le message d'état il passait sous l'aide centrée (constaté au banc le 22/11).
  Le remplissage ne change qu'un `FillPortion`, sur message — jamais une
  horloge, jamais une mise en page par image. `GLAUNCHER_UPDATE_CMD` remplace
  la commande et `GLAUNCHER_UPDATE_DEMO=1` la lance au démarrage (banc, faux
  émetteur) ; `PR_SET_PDEATHSIG` évite qu'un launcher tué laisse une commande
  root orpheline.
- **Musique de fond** (`music = true`, défaut) : piste **embarquée**
  (`assets/music/menu.opus`) — 22 Mo de WAV source → **1,6 Mo** en Opus 96 kb/s,
  préparée une fois par
  `ffmpeg -t 115.289 -af "afade=t=out:st=115.281:d=0.008" -i src.wav
  -c:a libopus -b:a 96k -vbr on -compression_level 10 -application audio
  -map_metadata -1` (coupe la queue morte de l'export Suno, pose la garde
  anti-clic, jette le commentaire « made with suno studio »). Recopiée dans
  `<cache>/music/` (mpv a besoin d'un fichier *cherchable* pour boucler) puis
  lue par `mpv --no-config --no-video --ab-loop-a=… --ab-loop-b=…
  --gapless-audio=yes`, volume piloté par sa socket IPC (`set_property volume`) :
  c'est le seul moyen d'avoir un vrai fondu, une `--volume` en ligne de commande
  étant figée. **Volume** : `music_volume` (0 à 100, défaut 60 —
  `music::DEFAULT_VOLUME`), réglé par le curseur « Volume » de Gablue Manager →
  Lancement de jeux ; `0` ne lance pas `mpv` du tout.
  Fondu d'entrée 1,2 s, sortie 0,9 s **pendant le voile noir** du lancement ;
  `mpv` est **arrêté** (pas mis en pause) pendant une partie et relancé au
  retour. **La reprise repart de la position quittée** : juste avant de rendre
  la main, la position est demandée à `mpv` par IPC (`get_property time-pos`,
  budget 120 ms — elle répond en ~1 ms, et une réponse qui traîne ferait jouer
  la musique par-dessus le jeu) et le `mpv` suivant reçoit **`--start=<position>`
  dans sa ligne de commande**, donc il démarre directement là, volume encore à
  0 ; fondu d'entrée court (`FADE_IN_RESUME_MS`, 0,5 s) pour ne pas brusquer son
  arrivée (demandé le 20/11). Le morceau ne repart donc jamais de son intro à
  chaque retour. **Deux pièges mesurés le 20/11** (symptôme : « la musique
  rejoue depuis le début ») : (1) lire un nombre FIXE de lignes IPC ne suffit
  pas — chaque `set_property volume` d'un fondu reçoit sa réponse
  (`request_id:0`), un fondu de sortie en laisse ~30 devant celle du `time-pos`,
  donc il faut lire **jusqu'à la réponse** dans la limite du budget ; (2) un
  `seek` par IPC juste après la connexion est **refusé** (`error running
  command` : le fichier n'est pas encore chargé, `file-loaded` arrive après),
  d'où la position passée en argument de `mpv` plutôt qu'en `seek`.
  Mesuré : ~2 à 3 % d'un cœur, ~85 Mo (`pw-play` ferait 1 % sans fondu).
  **Un autre son coupe la musique** (`src/sons.rs`) : dès qu'un flux audible
  n'est pas le nôtre (vidéo à côté, navigateur, app compagne qui continue
  après le jeu), la musique descend à 0 et **reprend** quand le silence
  revient — elle est un fond sonore, pas une deuxième bande-son. Notre propre
  `mpv` est exclu par son **PID**, jamais par son nom ; sous PipeWire ce PID
  se lit sur le **client** (`pactl list clients`, le flux `mpv` ne publie pas
  `application.process.id` — constaté le 22/11, sans quoi la musique se taisait
  toute seule). Contrôle toutes les 2 s (`AUDIO_CHECK`), `LC_ALL=C` pour
  `pactl`, `GLAUNCHER_MUSIC_DEBUG=1` trace `veut`/`bloque`.
  **Boucle** : `--ab-loop-a=0.133 --ab-loop-b=115.289`, soit la première note →
  fin de la **48ᵉ mesure** (le morceau est calé sur 100 BPM, mesure de 2,4 s, et
  ses 48 mesures font 115,2 s) : intro jouée en entier au démarrage, puis chaque
  tour repart sur la première note, **sans mélange ni décalage**. Écarté le
  20/11, chiffres à l'appui : replier la fin sur le début (2 s) faisait démarrer
  la musique sur son dénouement ; boucler le fichier tel quel collait **170 ms
  de silence** par tour (fin à −67 dB + 128 ms de silence avant l'attaque) —
  c'est la « petite coupure » signalée. Le fichier embarqué est la piste d'origine
  dont on a seulement retiré la queue morte de l'export (51 ms) avec 8 ms de
  fondu de garde : saut de bouclage 173/32768, aucune plage silencieuse aux
  jonctions sur une capture de la sortie réelle. `--gapless-audio=yes` reste
  indispensable (sinon 31 ms de blanc par tour, réouverture de la sortie audio).
  `libc` + `PR_SET_PDEATHSIG` (dans `pre_exec`) : un launcher qui meurt, même
  par plantage, ne laisse pas une musique orpheline. Le réglage s'écrit dans
  Gablue Manager → Lanceur de jeux (case « Jouer une musique de fond »).
- **Plein écran du jeu (KWin)** : `kwin::watch` suit les PID du jeu et force
  l'attribut `fullScreen` de KWin sur leurs fenêtres **normales** via un script
  KWin chargé par D-Bus (`org.kde.kwin.Scripting`, `qdbus6` puis `qdbus`) :
  beaucoup de jeux `gwine`/`lgplaunch` s'ouvrent sinon en 1280×720 au milieu du
  bureau. Le script est rechargé quand la liste de PID change (forks de wine,
  script lgplaunch) et **déchargé à la fin du jeu** ; dialogues et launcher ne
  sont pas touchés.
- **Wine daemonise : les PID du jeu ne sont pas tous des descendants.** Le
  processus qui porte la fenêtre est reparenté à `systemd --user` (constaté
  18/11 : `Antonblast.exe`, et `notepad.exe` en test — aucun plein écran alors
  que le script tournait, alors que l'overlay manette qui vise `activeWindow`
  marchait). `kwin::game_pids` ajoute donc aux descendants les processus qui
  partagent **leur session** (`bwrap --new-session` de gwine laisse une session
  propre au jeu ; `/proc/<pid>/stat` champ 6). La session du launcher est
  exclue : un jeu lancé sans sandbox hériterait de la session graphique et tout
  le bureau passerait en plein écran. Validé sur `notepad.exe` via gwine, à
  froid **et** wineserver déjà chaud : la fenêtre du jeu passe `fs=true`, un
  autre notepad lancé hors launcher reste intact, et `unloadScript` répond
  `false` après la sortie (script bien déchargé).
- **Barre Plasma entre l'écran noir et le jeu** : Plasma ne cache ses panneaux
  que lorsque la **fenêtre active** est plein écran (mesuré 18/11, reconfirmé le
  22/11 sur Plasma 6.8.3 : une fenêtre plein écran *inactive* laisse la barre
  devant). Deux mesures, deux protections dans le script KWin :
  1. la fenêtre du jeu est **activée avant** que son plein écran ne soit
     effectif (le client X11/wine met quelques centaines de ms à obéir) : tant
     qu'elle n'est pas réellement plein écran, le script **réactive l'écran noir
     du launcher** (qui l'est) puis rend la main dès que `fullScreen` passe à
     vrai (relance toutes les 30 ms, plafond 2 s). Contrepartie assumée : le jeu
     perd le focus quelques centaines de ms au démarrage ;
  2. la barre se voyait quand même **1 à 2 images** au moment où la fenêtre du
     jeu arrive (2 images sur 599 à 20 i/s, ~100 ms, bande du bas mesurée à
     20 Hz) : KWin ne réempile le plein écran qu'à l'image suivante. Le script
     pose donc `keepAbove` sur l'écran noir puis sur la fenêtre du jeu — la
     couche « Above » passe devant les panneaux, **y compris une barre
     `hiding=none` toujours visible** — et le retire `GRACE` (400 ms) après que
     le jeu est devenu actif plein écran, avec un garde-fou `MAX_AU_DESSUS`
     (8 s) pour ne jamais laisser une fenêtre au-dessus de la barre.
  **Aucun fichier de config Plasma n'est écrit** : une bascule de `panel.hiding`
  par `evaluateScript` réécrirait `plasma-org.kde.plasma.desktop-appletsrc`
  (écarté le 22/11). Vérifié par capture de la bande du bas pendant le
  lancement : 0 image de barre sur 561.
- **Filet `uncurtain` en fin de partie** : si le jeu meurt avant que le script
  n'ait rendu la main, `kwin::release` charge un dernier script (nom unique par
  relève : KWin renvoie `-1` pour un nom déjà pris) qui retire `keepAbove` de la
  fenêtre du launcher, puis le décharge. **L'ordre compte** : le script de
  surveillance est déchargé *avant* le nettoyage, sinon son minuteur (20 ms)
  reposait `keepAbove` juste après et le launcher restait au-dessus de tout
  jusqu'à la partie suivante. Les noms uniques datent du 22/11 : « KWin a refusé
  le script de nettoyage » à chaque partie après la première.
- **Un plein écran retiré reste retiré** : le script ne force chaque fenêtre que
  pendant ses 3 premières secondes (`JEUNE`, clé = `internalId`). Ensuite la
  fenêtre appartient à l'utilisateur : `F`/F11 ou l'overlay manette retirent le
  plein écran et un balayage toutes les 200 ms ne le remet pas (vérifié :
  `fs=false` toujours 4 s après la demande). Ne jamais réintroduire un forçage
  permanent : c'était la régression qui empêchait de sortir du plein écran en
  cours de partie.
- **Relève du script sans trou, et rarement** : le nouveau script est chargé
  avant que l'ancien soit déchargé (noms `glauncher-fullscreen-<pid>-<n>`), et
  le thread relit `game_pids` toutes les 100 ms les 6 premières secondes (puis
  1 s, puis 5 s). Il ne **recharge** que si un nouveau PID **a chargé
  `libX11`/`libwayland-client`** (`owns_display`, vérifié : `notepad.exe` oui,
  `wineserver` non) — seul un client d'affichage peut ouvrir une fenêtre. Wine
  forke en permanence : recharger à chaque fork repartait d'un état vierge et
  réimposait le plein écran retiré par l'utilisateur (constaté 18/11, 2 relèves
  au lieu de 8, `fs=false` toujours 40 s après la demande).
- **Nom de script unique par partie** : un launcher tué en pleine partie laisse
  son script dans KWin et KWin refuse un nom déjà pris (`loadScript` → `-1`,
  constaté 18/11 : plus aucun plein écran). Le nom porte le PID du launcher, le
  nom courant est noté dans `<cache>/kwin-fullscreen.name`, et `drop_stale_script`
  décharge celui de la session précédente.
- **Bascule de liste (vue, tri, filtre d'année)** : même principe que la page,
  en plus court — voile monté à **92 %** (`SWAP_PEAK`, `SWAP_RISE` 0,12 s,
  `SWAP_FALL` 0,18 s), le contenu est réordonné **au sommet** (`begin_swap` /
  `pending_swap` / `apply_swap`) quand il n'est plus lisible, puis le voile
  redescend. Jamais de noir complet : on ne change pas d'écran, on réordonne
  le même. Tri et filtre remettent la sélection et le défilement au début
  (`apply_swap`), la vue conserve la vignette courante. Tout passe par lui —
  `V`, menu « Type de liste », menu « Tri », menu « Année » —, sinon le
  changement apparaîtrait d'un coup. **La réindexation aussi** :
  `Message::Refreshed` compare les chemins de la nouvelle liste à ceux à
  l'écran (`meme_liste`) ; si elle change (paquet ajouté, disque branché ou
  retiré), le résultat attend le sommet du même voile (`Swap::Reload`,
  `pending_reload`) au lieu de se réordonner sous les yeux. La sélection et le
  défilement ne bougent pas (on ne remonte pas en haut parce qu'un disque est
  apparu), et un fondu déjà en cours n'en reçoit pas un second (le résultat
  est appliqué directement).
- **Fondu de la page d'un jeu** : fondu **enchaîné par le fond**, pas un
  passage au noir (jugé « trop agressif » le 20/11 — ça sautait au noir puis
  ressortait, sans montée). Deux rampes : montée `PAGE_FADE_RISE` (0,12 s),
  bascule au sommet, descente `PAGE_FADE_FALL` (0,20 s). La page n'entre ou ne
  sort de l'arbre **qu'au sommet** (`pending_page: PageStep::Open/Close`), quand
  le fond est opaque : aucun changement d'écran ne se voit. Même voile que le
  fondu du lancement (`veil_opacity = max(fade, page_fade)`), donc aucun enfant
  de plus dans la pile. Le minuteur des particules reste coupé (page ouverte),
  c'est le fondu lui-même qui demande les images (`animating`).
- **Menus et entrée en galerie** : fondu d'apparition par un voile noir
  **au-dessus** de la couche `menus` (`overlay_fade`, `FONDU_MENUS` 0,18 s à
  60 Hz), relancé quand `overlay_signature` change : ouverture d'un menu ou
  d'un prompt, entrée en galerie, menu qui en remplace un autre. iced 0.14 n'a
  pas d'opacité de sous-arbre : ce voile est le seul fondu global possible
  sans repeindre chaque couleur de panneau. Il existe **toujours** dans la
  pile (opacité nulle au repos), comme le voile de lancement. L'aperçu d'une
  photo et le panneau d'effacement en sont exclus : ils ont leur propre
  transition et un noir plein écran clignoterait pour rien.
- **Visuels arrivés en retard sur une page ouverte** (héros, logo, cover) : un
  compteur par visuel (`page_hero`, `page_logo`, `page_cover`, 0,30 s —
  `PAGE_ASSET_SECONDS`), remis à 1 à l'ouverture si l'image est déjà là
  (`reset_page_assets`), sinon il monte quand elle arrive (`advance_asset`).
  Le héros fait grandir la bande de 0 à sa hauteur **et** apparaît en fondu ;
  logo et cover n'ont pas de place à réserver, seule leur opacité monte
  (`ease_reveal`, décélération quadratique). La place réservée suit la même
  progression, donc les tailles (`dispo`/`cover_h`/`ech`) se compriment
  doucement — mise en page par image assumée, 0,30 s, une seule page.
- **Fondu** : la fenêtre s'ouvre noire et apparaît en fondu (`fade = 1` au
  démarrage, cible 0). Au lancement, un voile noir monte en 0,9 s **par-dessus
  le launcher** (ne jamais court-circuiter la vue sur `running`, sinon le
  passage au noir est brutal) ; le jeu démarre quand l'écran est noir. Au
  retour du processus : la page du jeu se referme (on revient à la **liste**),
  fondu inverse, launcher et `gain_focus`.
- **`test.sh` = binaire release** : `target/debug` décodait chaque image ~90×
  plus lentement (83 ms pour une cover, 47 ms pour une miniature, mesuré) — la
  grille mettait 8 s à se remplir alors que la liste était déjà dessinée. Les
  caisses de décodage sont en plus optimisées en debug
  (`[profile.dev.package] image/png/zune-jpeg/fdeflate/miniz_oxide/flate2/
  image-webp = opt-level 3`), donc `cargo run` reste utilisable.
- **Démarrage (aucun accès aux dossiers de jeux avant la première image)** :
  `library::from_rows` ne lit que le **cache local** (`icon_cached`), jamais le
  voisinage d'un paquet : les 666 paquets × 6 `stat` réveillaient un disque
  externe endormi et repoussaient l'affichage de plusieurs secondes (8 s
  constaté). Le voisinage est lu par `assets::spawn` (thread « maintenant »
  pour les jeux à l'écran, balayage 40 ms ensuite), et `missing_assets`
  n'extrait l'icône du squashfs ni ne demande l'icône SteamGridDB qu'une fois
  ce slug scanné (`icon_scanned`) — sinon on téléchargeait des icônes pour des
  jeux qui en ont une à côté du paquet. Le veilleur inotify est posé **5 s
  après** (`watch::START_DELAY`) : sa pose récursive est un parcours complet du
  disque (269 000 accès mesurés), elle attendait la première image pour rien.
  Mesuré (release, `strace`) : 0 accès aux dossiers de jeux avant 300 ms ;
  repères dans le journal : « préparation en 56 ms », « premier rendu en
  104 ms ».
- **Scan rapide** : index SQLite (`<cache>/index.sqlite`), parcours parallèle,
  `scan_depth` (défaut 3) et `scan_skip` (préfixes Wine, `Content`…). Un dossier
  dont la mtime n'a pas bougé n'est ni relu ni `stat`é : ses paquets viennent de
  l'index. Mesuré 18/11 sur le disque externe t5 (646 paquets, 170 000 fichiers
  au total) : **285 s sans limite de profondeur → 37 ms froid / 16 ms chaud**.
  `--reindex` reconstruit l'index sans ouvrir de fenêtre.
- **Covers** : seuls les jeux **affichés** sont demandés à SteamGridDB (worker
  throttlé 250 ms) ; les visuels déjà en cache ne sont jamais redemandés et un
  échec réseau n'est pas réessayé dans la session (`cover_failed`). La vue
  courante ne demande que **cover et icône** (`DEUX_TYPES`) : héros et logo ne
  servent qu'à la page, et les demander partout saturait le worker throttlé.
  La **page** d'un jeu demande les quatre types en **urgent** et ignore la file
  de fond. Les demandes **abandonnées** par un remplacement de la voie de fond
  sont libérées (`art_requested`) : sans ça, un jeu traversé trop vite restait
  marqué « demandé » pour la session et ne recevait **plus jamais** ses images
  en revenant dessus (constaté 18/11, « les images de Cat Quest ne se
  téléchargent pas quand je suis devant eux »).
- **« Introuvable » 30 jours (`.miss`)** : écrit seulement quand le jeu est
  inconnu de SteamGridDB (autocomplétion vide). Un jeu **connu** dont l'API ne
  rend aucune image remonte une **erreur** (« aucune cover », session visiteur
  périmée derrière Cloudflare) : l'appelant reprend une session, et le jeu
  n'est pas condamné pour 30 jours (marqueurs posés à tort observés 18/11 sur
  des jeux qui ont bien une cover). L'API publique rend parfois une
  **page vide** (session visiteur périmée derrière Cloudflare) : `candidates_public`
  en reprend une et réessaie une fois, avec `Referer` — sinon un jeu, et son
  menu d'image, restait sans résultat. Le cache de visuels (`Artwork`) est mis
  à jour à chaque visuel reçu : il alimente les réindexations, donc sans ça un
  rescan repartait des fichiers lus au démarrage et faisait disparaître ce qui
  venait d'être téléchargé. La plage demandée est calculée sur la
  **position de défilement** (et non sur la page de la sélection), avec une
  rangée de marge : une rangée à moitié visible charge aussi ses covers.
- **Priorités du worker** (`covers::Queue`, `Mutex` + `Condvar`, deux voies) :
  la voie `urgent` (menu d'image, puis extraction locale d'icône) passe
  toujours devant ; la voie `visible` (covers de fond) est **remplacée** à
  chaque changement de vue — aller loin dans la liste télécharge ce qui est
  là-bas, pas la file accumulée depuis le début. Ouvrir le menu d'image
  **suspend** les covers de fond (la voie est vidée) ; à la fermeture (Échap,
  retour, ou image choisie) `resume_covers` relance la couverture de la vue
  courante. Un seul worker suffit donc, la seconde file dédiée au menu a
  disparu.
- **Année de sortie** : SteamGridDB la donne dans l'autocomplete
  (`data[0].release_date`, secondes Unix) des deux API. Requête **dédiée**
  (`Query::Year`, file `urgent`) à l'ouverture de la page quand la clé du jeu
  est absente de la table : un jeu dont les visuels sont déjà en cache ne fait sinon
  aucune requête, donc n'aurait jamais son année. Avec une identification, la
  date vient de la **fiche du jeu choisi** (`/api/v2/games/id/<id>` ou
  `/api/public/game/<id>`), pas du premier résultat.
  **Mémoire : `assets/years.txt`** — le dossier partagé (Syncthing), donc les
  années suivent les visuels chez un autre poste. `<clé>=1998` par ligne (la
  clé du jeu, comme les visuels : `8103-doom=2016`), trié ;
  `<clé>=` note « regardé, SGDB n'en donne pas », et **rien n'est jamais
  redemandé** (ni par l'interface, ni par `--update`), sauf demande explicite
  « Demander à SteamGridDB » du prompt de date. Écriture atomique en
  **fusion** (`covers::save_years` relit le disque avant d'écrire) : le mode
  `--update` et l'interface peuvent écrire en même temps. Une erreur réseau,
  elle, ne note rien (la question pourra être reposée).
  **Date à la main** : `Options ▸` → `Date de sortie…` ouvre un prompt
  pré-rempli (année connue), Entrée enregistre (1950-2100), vide efface la
  date, et « Demander à SteamGridDB » relance la requête. C'est le recours
  quand le jeu n'est pas dans SteamGridDB du tout (le cas de « Marsupilami 2 »).
  `--update` apprend les années de tous les jeux qui en manquent (une requête
  + 250 ms chacun, comptées dans `UpdateReport::years`) ; `--date` (avec
  `--update` ou seul) ne fait que ça, sans visuel (`covers::update_years`).
  Affichage : « Sorti en 1998 » **sous le titre** (information du jeu), le
  temps de jeu restant en dessous (information du joueur) — les mettre sur la
  même ligne faisait bizarre (demandé le 20/11).
  La conversion horodatage → année est une petite boucle depuis 1970
  (`year_from_unix`, testée aux réveillons et au 29 février) ; une date `0`
  vaut « inconnue », pas 1970.
- **Filtre d'année** : dernière ligne du menu « Tri » (`T`/Carré), qui ouvre le
  menu « Année » — une ligne par année **connue**, avec son nombre de jeux, plus
  « Toutes les années ». Le filtre se combine avec le tri et les favoris
  (`apply_filter`), se retient dans `state.txt` (`annee=1998`) et s'affiche
  dans la pastille de tri (« TRI A→Z · 1998 »). Il ne peut porter que sur les
  jeux dont l'année est connue : leur nombre grandit avec les pages ouvertes et
  les `--update`.
- **Portée de la bibliothèque** (ligne « Bibliothèque » du même menu « Tri ») :
  `Tous les jeux` → `Dossiers choisis` → `Disques scannés` → retour, à chaque
  `Croix` **sans refermer le menu** (on voit la valeur changer). Le filtre
  porte sur le **chemin** du paquet : `Dossiers choisis` = sous un
  `game_dirs`, `Disques scannés` = sous une racine de `extra_dirs` **et pas**
  sous un dossier choisi (un dossier de `config.toml` posé sur un disque
  externe reste « choisi »). Répond à « où est ce jeu ? » quand un disque
  branché ajoute 600 paquets à trois. Se retient dans `state.txt`
  (`portee=disques`) et se lit dans la pastille (« TRI A→Z · disques »).
  Couper `scan_disks` ramène la portée à `Tous` : « disques scannés » sans
  disque scanné n'a plus de sens.
- **Menu « Tri » construit sur une liste de lignes** (`SortMenuEntry` :
  `Sort`, `Year`, `Scope`, `Clear`, `sort_menu_entries`) et non sur des indices
  calculés : l'ancienne arithmétique (`SortBy::ALL.len()` + 1 pour
  « Effacer les derniers lancés ») se décalait dès qu'une ligne conditionnelle
  apparaissait.
- **Page d'un jeu : moitié basse dimensionnée par l'écran.** La cover prend
  **72 % de la hauteur restante** sous la bande héros (bornée 300 à 520 px), et
  tout le bloc (logo, nom, temps de jeu, pastilles, marges, aide) suit
  le même facteur (`ech = cover_h / 308`, 1 à 1,7). Avant, ces dimensions
  étaient des pixels figés (205×308) : « encore trop petit » en 1080p (retour
  19/11). Sans héros, la bande ne réserve rien et le bloc se centre sur toute la
  page.
- **Jeu en cours publié pour l'overlay manette** : tant qu'un jeu tourne,
  `<cache>/running.txt` porte `slug=`, `nom=` (nom affiché du paquet), `pids=`,
  puis `logo=` et `icone=` — chemins absolus, **seulement s'ils existent**
  (`assets/logos/<clé>.png` ; icône : `assets/icons/<clé>.png` d'abord —
  c'est là qu'arrive une icône choisie, et `assets/` est partagé par Syncthing
  entre postes —, puis l'icône extraite du paquet, en PNG). C'est ce que lit `gpad` pour afficher le logo du jeu
  au-dessus de l'en-tête et remplacer le nom/icône de la fenêtre : le lien est
  le **PID** de la fenêtre active, jamais sa classe (détail →
  `src/gablue-pad/gpad/AGENTS.md`). Le fichier est réécrit quand la liste des
  PID change (Wine forke), en écriture atomique, et retiré à la fin du jeu s'il
  est encore le nôtre.
- **Poids du cache `assets/` : tout visuel est réécrit à une taille cible**
  (`covers::cible`, appliquée par `save` et par `--art`). SteamGridDB sert les
  héros en **3840×1240** (jusqu'à 12 Mo) et les grilles en 1024×1536 : cibles =
  héros **1920 px de large en JPEG q88**, grilles **600×900 en JPEG**, logos
  **1000 px en PNG**, icônes **256 px en PNG** (PNG obligatoire pour les deux
  derniers : l'overlay `gpad` ne décode que le PNG, et il faut l'alpha). Mesuré
  le 19/11 sur un cache réel : **438 Mo → 110 Mo** (411 fichiers réécrits,
  405 Mo → 77 Mo), en 15 s. Un fichier déjà au format et à la taille cibles
  n'est **jamais** réécrit (un JPEG ré-encodé perdrait de la qualité à chaque
  passe) — les dimensions se lisent dans l'en-tête (`ImageReader::into_dimensions`),
  pas en décodant l'image. `--shrink` applique la même règle au cache existant
  et supprime la vignette des grilles converties (elle est refaite).
- **`--art <paquet>` : les visuels d'un SEUL paquet, pour gwine/lgplaunch.**
  Lancé en fond par les deux lanceurs quand un `.wgp`/`.lgp` n'est pas passé par
  le launcher, il écrit dans le cache le logo et l'icône **manquants**
  (`covers::fetch_game`) : icône du paquet (`.icon.png` du squashfs) avant celle
  de SteamGridDB, un visuel déjà là n'est jamais redemandé, un `.miss` récent
  n'est pas réessayé, et tout est **ré-encodé en PNG** — `gpad` ne décode que le
  PNG, un logo en JPEG du cache serait invisible. Pas de fenêtre, pas d'index,
  pas de scan : deux requêtes au plus. `gpad` relit le cache une fois par
  seconde, donc le logo apparaît tout seul après le téléchargement. Comme
  `--update`, il **retient l'identification** qu'il résout (fichier partagé,
  visuels déjà là renommés vers la clé du jeu) : c'est ce qui fait converger un
  cache existant, lancé depuis `gwine`/`lgplaunch` sans passer par le launcher.
- **Page d'un jeu** : un clic sur une vignette déjà sélectionnée, ou Entrée/A,
  l'ouvre : héros en bande (largeur **explicite**, `ContentFit::Cover`), logo,
  cover, nom, **année de sortie** (sous le titre) et temps de jeu, puis trois
  pastilles — `Jouer`, `☆ Favori`, `Options ▸` (renommée le 20/11 : le menu ne
  contient plus seulement des images) — auxquelles s'ajoute **`Galerie`**
  quand le jeu a au moins une capture ou une vidéo (voir *Galerie*). Les
  actions sont **dynamiques** (`App::page_actions`) : `PageNav`, `PageChoose`
  et les flèches bornent sur cette liste, jamais sur les trois actions de
  base — sinon la manette n'atteignait pas « Galerie » (signalé le 24/11). **Ni chemin du paquet, ni état des visuels** (`Grid ✓ · Héros …`) :
  retiré le 20/11, la page doit rester une affiche, pas une fiche technique.
  Échap/B referme et rend la liste avec la
  sélection et le défilement intacts. **Sans héros, aucune bande n'est
  réservée** : le bloc du bas se centre sur la page (une bande grise vide
  faisait pauvre, retour 18/11). La page vient **par-dessus la liste** dans une
  couche de profondeur **constante** (voir « Couche d'affichage »).
- **Options réunies** : `Options ▸` ouvre un menu de **huit lignes** — Grid,
  Héros, Logo, Icône (`✓` = déjà là), `Identifier le jeu…` (ou `Revérifier sur
  SteamGridDB` quand la marque « introuvable » est fraîche), `Reprendre les
  visuels d'un jeu…`, `Date de sortie…` (année connue en bout de ligne) et
  `Copier le jeu…` (voir *Copier un paquet*) — **suivies des options de
  lancement du jeu** (voir ci-dessous). Une ligne mène au menu d'images du
  type choisi. **Aucun raccourci** n'ouvre ce menu depuis la liste, ni depuis la
  page (retiré le 20/11 : `C` sur la liste, puis `I`/`G`/`H`/`L`/`C`/`R`/`D` et
  les boutons manette Y/X sur la page) : le bouton de la page est le seul
  chemin. Le menu d'image
  propose les candidats SGDB **dans l'ordre du top**, vignettes aux proportions
  du type (grid 148×222, héros/logo 208×117, icône 120×120) ; Entrée/A
  enregistre **et referme le menu tout de suite** (le téléchargement continue en
  fond, pied de page « application de l'image… » ; `picked` garde la trace du
  choix pour le remplacement d'icône). Un **clic** sur une vignette vaut ce
  choix immédiatement (`Message::PickerChoose` → `PickerPick`) : avant, il ne
  faisait que mettre la case en évidence et le menu restait ouvert, alors que le
  reste de l'interface valide au premier clic (signalé 19/11). Le menu s'ouvre **tout de suite** : la
  liste des propositions arrive vide de vignettes (`CoverEvent::Choices`) et une
  case de la bonne taille est dessinée pour chacune ; chaque vignette
  téléchargée la remplit aussitôt (`CoverEvent::Choice`, identifiée par son
  `url`). Avant, le worker téléchargeait les douze vignettes **avant** d'émettre
  quoi que ce soit : plusieurs secondes devant un menu vide (signalé 19/11).
  Les vignettes sont décodées **une seule fois** (`Picker::thumbs`, un
  `Option<Handle>` par proposition) : un `Handle::from_bytes` construit dans la
  vue faisait redécoder les 24 PNG à chaque rendu — menu poussif et images qui
  clignotent (constaté 18/11). **Grilles : portrait seulement** (`candidates(assets, kind)` écarte
  les carrés et les formats larges) — le type `grid` de l'API publique mélange
  600×900, 512×512, 1024×1024 et 920×430, d'où l'impression de « pas que des
  grilles » dans le menu (constaté 18/11) ; si rien ne reste en portrait, tout
  est rendu plutôt que d'afficher un menu vide. L'image choisie est écrite dans `assets/<type>/<clé>`, un
  `<slug>.choice` garde l'URL ; comme un visuel présent n'est plus redemandé,
  **le choix est permanent**. Une image choisie et le marqueur vivent dans deux
  dossiers différents : le **fichier** dans `assets/` (partagé entre postes, donc
  un choix fait ailleurs gagne, y compris pour les icônes puisque
  `assets/icons/` passe avant `pack-icons/`), le marqueur `.choice` dans
  `marks/` (propre à la machine : `.miss` ne doit jamais être synchronisé). Pour les icônes, l'icône du paquet est proposée
  **en tête** du menu (`file://`), donc sélectionnable même après un choix
  distant.
- **Image perso** : « Choisir un fichier… » (bouton du menu d'images, ou `F`)
  ouvre le sélecteur du système (`src/dialog.rs` : `kdialog` puis `zenity`,
  lancé dans un fil à part — le dialogue reste ouvert tant qu'on navigue, l'UI
  ne gèle pas) et prend une image locale à la place de SteamGridDB. Elle est
  **adaptée au type** avant écriture (`covers::adapter`, pas `normaliser`) :
  recadrée au centre et remplie exactement — grid 600×900, héros 1920×620,
  icône 256×256 — parce qu'une photo 16:9 posée en cover laisserait deux bandes
  dans la vignette. Le **logo** n'est jamais recadré (un logo rogné perd son
  texte) : seulement réduit à 1000 px de large, ratio conservé. L'image écrite
  reçoit un marqueur `.choice` avec son `file://` : elle fait foi et n'est
  jamais remplacée automatiquement. Formats acceptés : ceux que le décodeur
  connaît (png, jpg, webp) ; une image illisible est annoncée en pied de page
  et ne touche pas au visuel en place.
- **Identification d'un jeu** : « Identifier le jeu… » (menu `Options ▸`, ou
  le bouton du même nom dans le menu d'images) ouvre un prompt pré-rempli
  **et présélectionné** avec l'identification courante ou le nom du paquet.
  Entrée cherche, puis une **liste** des jeux que SteamGridDB propose (nom et
  année) s'ouvre : Entrée/A retient le jeu, `R` corrige le nom, Échap annule.
  C'est la réponse au jeu mal reconnu : « Marsupilami 2 Salsa Palombia » tombait
  sur « Marsupilami: Hoobadventure », d'où une cover, une icône et une date
  d'un autre jeu (constaté 20/11). Le choix est écrit dans
  **`assets/identities.txt`** (`slug=<id>TAB<nom>`, fusion à l'écriture,
  temporaire + renommage) : c'est le dossier **partagé par Syncthing**, donc
  les copains et les autres postes téléchargent les visuels du MÊME jeu — et
  c'est cette identification qui **nomme** les fichiers (`<id>-<nom>`, voir
  *Nom des visuels*). Les
  requêtes `Auto`/`Choices`/`Year` portent alors l'`id` et court-circuitent la
  recherche par nom (y compris `--update`, `--date` et `--art`). Choisir une
  identification **oublie les visuels de l'ancien jeu** (`covers::forget` :
  fichiers des quatre types **sous l'ancienne clé**, `.choice` et `.miss` de
  la machine, handles en mémoire, miniature) puis redemande les quatre en fond
  sous la nouvelle clé ; l'icône du paquet,
  correcte par définition, est relue du cache. **Mais si le paquet a déjà des
  visuels, on demande d'abord** (`covers::has_art` → question « Remplacer les
  visuels ? », « Non, garder » sélectionné par défaut) : les écraser d'office
  jetait des images choisies à la main (demandé le 20/11). « Oui » appelle
  `replace_art` (l'oubli + la redemande ci-dessus) ; « Non » garde tout et
  l'identification est quand même retenue. Clavier seulement pour
  l'instant : à la manette, il faudra un clavier virtuel (la manette ne peut
  qu'annuler le prompt).
  **Jeu absent de SteamGridDB** : une recherche qui ne rend aucun jeu écrit une
  marque **datée** dans `assets/absents.txt`
  (`slug<TAB>verifie=AAAA-MM-JJ<TAB>requete=nom`). Ce n'est pas une affirmation
  sur le monde mais sur le dernier essai : la marque **expire au bout de 30
  jours** (`absent_frais`, date **locale** du poste : comparer à UTC faisait
  expirer une marque du jour entre 23 h et minuit, constaté au banc le 23/11),
  se lève dès qu'une identification, une recherche fructueuse ou une image
  choisie à la main la contredit, et s'affiche dans
  `Options` (« Introuvable sur SteamGridDB · vérifié le … »). `Croix` sur la
  ligne (alors libellée **Revérifier sur SteamGridDB**) lève la marque, oublie
  les `.miss` du jeu et relance la recherche **immédiatement** ; une marque
  fraîche évite simplement de redemander à chaque démarrage. Le filet de
  sécurité pour ces jeux est l'**alias visuel** (« Reprendre les visuels d'un
  jeu… ») : la liste des jeux qui ont déjà des images, `alias_visuels` copie
  les quatre types sous la clé du jeu cible, marque `.choice` (une recherche
  automatique ne les remplacera pas), lève la marque « introuvable » et pose
  `<slug>.alias` dans `<cache>/marks` : **aucune résolution automatique du nom**
  ensuite, sinon la recherche auto renommerait les fichiers copiés sous la clé
  d'un jeu qui n'est peut-être pas le bon (constaté au banc le 23/11). Les
  fichiers restent dans `assets/`, donc l'alias suit les autres postes ;
  `covers::forget` (identification, remplacement) retire le marqueur.
- **Favoris** : `F` sur la page (ou View/Share) bascule l'étoile. Les slugs sont
  écrits dans `<cache>/favorites.txt` (trié, un par ligne, écriture
  temporaire + rename) — c'est la seule mémoire, un jeu momentanément absent
  garde son étoile. La vignette porte une étoile (PNG embarqué
  `assets/star.png`, source `assets/star.svg` : le caractère `★` tombait sur un
  glyphe de repli minuscule). C'est une étoile **jaune façon Mario**, deux yeux
  noirs, contour sombre pour rester lisible sur une cover claire — dessinée pour
  être affichée en **14 à 16 px**, donc formes épaisses : les yeux se réduisent à
  une tache sombre à cette taille, c'est le contour et le jaune qui portent la
  lecture. Les favoris sont un **tri** (`T`/Carré, menu « Tri » → Favoris) :
  la même grille, filtrée, en-tête « N favoris » et liste vide expliquée.
  Ce n'est plus une vue (retirée du menu des vues le 20/11).
- **Animations** : fondu au noir de 0,9 s avant le lancement (le jeu démarre
  quand l'écran est noir), retour en fondu à la fin du jeu ; défilement animé
  vers la sélection. Tout est piloté au temps (dt), pas par image, et
  l'abonnement `window::frames()` n'existe que pendant une animation.
- **Titre trop long = défilement continu** : la vignette sélectionnée affiche
  le nom **entier** et le fait défiler quand il ne tient pas. Une pause de
  1,2 s (on lit le début), puis défilement **sans fin** vers la gauche à
  34 px/s : la fin sort, un blanc de `GAP` (48 px) passe, et le **début revient
  par la droite** — le texte ne remonte jamais en arrière (demandé le 22/11 ;
  l'ancien aller-retour avec pause à chaque bout est abandonné). Deux
  exemplaires du nom sont dessinés, espacés de `largeur + GAP` (`travel` donne
  l'avance modulo cette période ; le second n'est dessiné que s'il touche la
  bande). Les autres vignettes restent tronquées avec une ellipse (`short`) :
  animer toute la grille coûterait une mise en forme de texte par vignette et
  par image. Le widget `Marquee` (`src/marquee.rs`) met le texte en page à sa
  largeur **naturelle** (limite infinie) puis le recadre (`with_layer`) : un
  titre long ne peut ni élargir la vignette ni empiéter sur la voisine.
  **Piège mesuré le 22/11** : `with_layer(bande)` + `with_transformation(dx)`
  applique `dx` une **seconde** fois au cadrage du texte (iced multiplie le
  `viewport` reçu par la transformation courante, `iced_wgpu::text`) — la bande
  rétrécissait à mesure que le texte défilait, d'où « du vide à la place de la
  fin du nom ». On passe donc au contenu une bande décalée de `−dx` : le
  cadrage effectif reste la bande. L'horloge (`title_elapsed`) vient de
  `App::tick` et repart à zéro quand la sélection change ; elle ne tourne que si
  le nom dépasse le budget de caractères de la vignette (`App::title_budget`,
  volontairement pessimiste), donc aucune image demandée au repos.
- **Deux roues de chargement libellées** : `src/spinner.rs`, neuf points sur
  un cercle dans le **coin bas droit**, à gauche des icônes d'état (aucun
  contrôle n'y vit, grille comme page ; elles étaient en haut à droite jusqu'au
  24/11, où le coin bas droit a été demandé pour tout regrouper). Deux
  indicateurs côte à côte, chacun avec son libellé à droite de sa roue :
  **« Visuels »** pendant un téléchargement SteamGridDB
  (`covers::busy()`), **« Scan »** pendant une réindexation (`refreshing`,
  scan des dossiers ou d'un disque branché). Éteint, un indicateur ne dessine
  rien mais garde sa place (forme d'arbre constante) ; les deux tournent sur
  la même horloge (`App::spin`, un demi-tour de décalage pour la seconde).
  Trois pièges, tous constatés le 20/11 :
  - **emplacement permanent** dans la pile racine de `view` : un enfant
    conditionnel y recrée le sous-arbre, donc la zone défilante, et la liste
    remonte en haut à chaque téléchargement. Le canvas est toujours là,
    `visible` ne décide que du dessin ;
  - **coordonnées RELATIVES** dans un `canvas` : `Canvas::draw` translate déjà
    le renderer de `bounds.x/y` avant d'appeler `Program::draw`. Dessiner à
    `bounds.center()` (position absolue) place la figure deux fois trop loin,
    donc hors de l'écran — la roue ne s'affichait jamais pour cette raison
    (constaté 20/11). Utiliser `bounds.width/height * 0.5`, comme
    `sparkles` ;
  - **réveil explicite** : le worker de visuels appelle `covers::wake_with` au
    DÉBUT de chaque requête (`Message::Activity`), sinon l'interface ne se
    redessine qu'à la fin du téléchargement et la roue n'apparaît jamais —
    `covers::busy()` (compteur de requêtes + fenêtre de 700 ms après la
    dernière) ne suffit pas à lui seul, faute de message pour relancer la vue.
  `animating()` la prend en compte : sans ça, plus aucune image ne serait
  demandée et la roue resterait figée. La roue ne reflète que ce qu'on regarde :
  **la file de fond est en pause tant qu'une page est ouverte** (`OpenPage` →
  `queue.pause(true)`, fermeture → `resume_covers`), sinon elle tournait pour
  les covers de la liste qu'on ne voyait plus (constaté 20/11). Les visuels de
  la page passent par la file `urgent`, que la pause ne touche pas.
  `resume_covers` rend demandables les clés jetées par la pause (`visible_covers`
  → `art_requested`), sinon ces jeux restaient marqués « demandés » pour toute
  la session (le même piège existait en refermant le menu d'images).
- **Poussière d'étoiles** (`sparkles = true`, défaut) : `src/sparkles.rs`, un
  `canvas` transparent posé **au-dessus de la liste**. La page d'un jeu est
  opaque : elle a sa **propre copie** des particules (`page_dust`, au-dessus de
  la page et sous le voile/menus, demandé le 20/11 — les étoiles bougent sur la
  page comme dans la liste). Une seule des deux copies est dessinée à la fois.
  Les particules sont des **fonctions pures du temps** (graine par numéro, aucun
  état stocké, aucune allocation par image) : poussière fine, étoiles
  moyennes, grands halos pâles, dérive + scintillement `sin³`, parallaxe au
  défilement (10-18 %). **Coût = la fréquence** : dans iced tout message
  provoque une reconstruction de la vue *et* une image complète (pas de
  redessin partiel), donc un minuteur propre à **20 Hz** (`FRAME_MS`) envoyé
  par le bus, et **coupé** (`sparkles::set_running`, recalculé à chaque
  `subscription()`) dès que la scène ne peut pas être vue : fenêtre sans
  focus, menu, prompt, écran noir (`fade >= 0.999`), et pendant un
  lancement (`to_launch`). La page ouverte ne coupe plus le minuteur (c'est un
  écran regardé). Ne jamais le brancher sur `window::frames()` :
  60 Hz inutiles **pour la poussière** (la respiration, elle, en a besoin :
  voir sa puce — une seule horloge tourne à la fois). Réglages sans recompiler :
  `GLAUNCHER_FX_MS` (16-200, intervalle) et `GLAUNCHER_FX_DEBUG=1` (ms par
  image) ; `sparkles = false` coupe tout (aucun fil lancé).
  **Sonde de fluidité** `GLAUNCHER_ANIM_DEBUG=1` : `App::tick` journalise
  l'intervalle RÉEL entre deux images pendant un fondu (`image 8 ms (fade …)`)
  — c'est ce qui distingue « l'animation rame » de « le compositeur/l'échelle
  fait scintiller le texte » (22/11 : 8-9 ms constants à 120 Hz, donc rien à
  corriger dans la boucle).
- **Hyperespace au lancement** : pendant le voile noir (0,9 s), chaque
  particule est poussée vers l'extérieur depuis le centre (`fade²`, traînée
  `95 px`, opacité `1 + 2.8 × fade`) et le même calcul joué à l'envers les
  ramène au retour du jeu. Il est **piloté par `fade` seul**, donc il avance à
  la cadence du fondu (`window::frames`, 60 Hz) sans le minuteur des
  particules. On lance les jeux depuis leur **page** (« Jouer ») : la page est
  opaque au-dessus de la poussière, d'où la couche `page_dust` — une seconde
  copie des particules **au-dessus de la page et sous le voile**, qui porte
  aussi les étoiles au repos et les fait filer pendant le fondu. Sans elle
  l'effet ne se voyait jamais (constaté 20/11).
- **Plongée de la page au lancement** : en plus de l'hyperespace, la page
  ouverte **grossit** de 6 % (`PAGE_DIVE`) pendant que le voile monte, comme si
  on entrait dans l'écran. `Pose` (`src/pose.rs`) applique une vraie
  `Transformation` au rendu (texte ré-rasterisé à la taille voulue), sans
  toucher à la mise en page ; à l'échelle 1 le widget se dessine normalement.
  Un **agrandissement** et non un recul : un contenu qui rétrécit laissait voir
  son bord au milieu du noir (jugé moche le 20/11). Échelle **continue** :
  une quantification à 0,5 % donnait 12 paliers, donc des à-coups de quelques
  pixels (jugé saccadé le 20/11). Le texte y est re-rasterisé à chaque taille
  (la cadence, elle, est régulière : 8-9 ms par image, mesuré à 120 Hz) — un
  léger tremblement assumé le 22/11, gardé tel quel ; la respiration, elle, ne
  transforme plus son titre (voir sa puce). Ne pas l'étendre à d'autres
  éléments.
- **Retour d'un jeu** : chaque vignette revient **de son côté de l'écran**,
  en dézoomant, titre compris (`App::comeback` + `Pose` par carte).
  `RETURN_SPREAD` règle la distance parcourue (55 % de la largeur),
  `RETURN_ZOOM` le grossissement de départ (10 %), `RETURN_STAGGER` le retard
  des vignettes du centre (45 %) : les bords partent de plus loin et se posent
  les derniers. `RETURN_END` (15 %) les fait se poser **avant** la fin du fondu :
  sans ça, l'ultime image où la carte cesse d'être transformée sautait de
  quelques pixels. Armé par `Message::Launched`, désarmé à `fade = 0` — jamais au
  démarrage du launcher. Pendant le retour, le canvas **s'efface**
  (`breathing()` rend `None`) : la carte revient déjà transformée, un cadre
  resté à la position de repos suivrait mal.
- **Forme de l'arbre constante à la racine** : ne JAMAIS ajouter ni retirer un
  enfant conditionnel de la pile racine (`view`) pendant l'usage. Un changement
  de forme (même un `stack![…, extra]` ajouté par-dessus) fait RECRÉER le
  sous-arbre, donc la zone défilante : la liste remonte tout en haut, et la
  première interaction restaure la position (`scroll_to`), ce qui se lit comme
  un saut puis un retour (constaté 20/11 avec une roue de chargement affichée
  pendant les téléchargements — retirée pour ça). La page, elle, entre et sort
  de l'arbre à dessein, mais **seulement au sommet du fondu** (`pending_page`),
  quand aucun état de défilement ne compte plus.
- **`Pose`, pas `iced::widget::float`** : `float` transforme son contenu mais le
  dessine en **surcouche**, donc au-dessus de toute la pile — voile noir
  compris : les vignettes seraient visibles pendant l'écran noir et la page
  resterait affichée par-dessus le fondu (constaté à la lecture d'`iced_runtime`,
  20/11). `Pose` est le même enrobage (mêmes dimensions, même `layout`) mais
  applique la transformation **dans l'arbre**, avec `Renderer::with_transformation` :
  ordre des couches conservé et découpage de la zone défilante respecté.
  `Pose::pivot(Point)` choisit le point fixe en coordonnées locales (par défaut
  le centre du contenu) : deux `Pose` qui doivent grossir autour du même point
  de l'écran le reçoivent chacun dans les siennes.
- **Respiration de la sélection** : le **fond** de la vignette (case teintée,
  bordure, ombre) et son **visuel** (cover, tranche, étoile) sont grossis de 0 à
  **5 %** (période 4 s) via `Pose`, autour du **centre du titre** — et le titre,
  lui, n'est **jamais transformé** (mesuré 22/11, retour « le texte zoomé n'est
  pas top ») : iced re-rasterise un texte à chaque changement de taille
  (`text.size × transformation.scale_factor()`, `iced_wgpu::layer::draw_text`),
  donc un titre qui suit l'échelle par pas de 1 px scintille et saute pendant
  que la cover glisse en sub-pixel (constaté : lignes du texte qui
  apparaissent/disparaissent). Le texte est donc posé **hors de toute `Pose`**
  et la vignette monte au-dessus de son nom : 300 images de mesure, titre
  immobile au pixel. La carte est une pile `[fond, contenu]` — le fond
  (case+ombre) est transformé, le contenu porte la cover transformée **et** le
  titre intact. `Pose::pivot` donne le point fixe en coordonnées locales : le
  fond et la cover le reçoivent chacun dans les siennes, pour un même point de
  l'écran (le centre du titre, sous la cover : `caption_middle`).
  La glissade d'un changement de taille passe par le même pivot
  (`zoom_transform(old, card, art, pivot, e)`) : la cover arrive exactement
  dans sa case, l'ancienne géométrie (pivot = centre de la case) restant un cas
  particulier. Le canvas (`sparkles::draw_breath`) ne peint que le **cadre
  d'accent** autour de la cover, à la même échelle, autour du même
  `Breath::pivot` et avec le même facteur (`App::breath_factor`, calculé une
  fois par image, partagé par la carte et le canvas) — c'est ce qui garde le
  cadre au-dessus de l'image et collé à elle. Le fond restait figé tant que
  seul le canvas bougeait (constaté 20/11), et la cover réapparaissait d'un coup
  au minimum quand deux peintres se relayaient.
  Conséquences : la carte peint **tout** son visuel (plus de `owns_art`, plus
  de cover/tranche/étoile rejouées par le canvas, plus de vignette sans cover à
  traiter à part) et le canvas n'a plus qu'**une géométrie** — le cadre étant
  un tracé, il passe sous les particules (images), ordre voulu. Le cadre
  entoure la cover seule, pas le titre (demandé le 20/11). La respiration
  s'arrête pour une vignette qui mord sur le pied de page ou sous le voile du
  lancement (`VEIL_CLEAR`).
  **Cadence : 60 Hz, pas le minuteur de particules** (retour du 22/11 : « pas
  assez fluide »). `App::breath_pulses` alimente `animating`, donc
  `window::frames` redessine à la fréquence de l'écran pendant que la sélection
  respire, et le minuteur à 20 Hz s'arrête (`sparkles::set_running(false)`) :
  une seule horloge à la fois. C'est `App::tick` qui avance le temps du pas
  RÉEL (`Sparkles::advance_by(dt)`) quand le minuteur est arrêté ; quand il
  tourne, `Message::Sparkle` avance d'un pas fixe comme avant — jamais les
  deux, sinon le temps courrait 1,3× trop vite. Le temps n'avance que pendant
  les images : une pause (menu, page, fenêtre sans focus) fige la phase et la
  respiration reprend où elle s'était arrêtée.
  **Pas de gerbe d'étincelles** au changement de jeu : essayée puis retirée le
  20/11 (jugée « nulle » à l'usage) — ne pas la réintroduire sans demande.
- **Coins arrondis de la cover** : `ART_RADIUS` (5 px), appliqué à l'image
  (`border_radius`) et aux coins gauches de la tranche (sinon son angle carré
  dépasserait du visuel) : la cover se lit comme une boîte posée.
- **Ordre des tracés et des images dans le canvas** : iced dessine, **dans
  une même géométrie, tous les tracés (fills, strokes) avant toutes les
  images**. Le canvas ne porte que le cadre (tracé) et les particules
  (images) : les particules passent donc devant le cadre, ce qui est l'ordre
  voulu. Un élément à poser **sur** une image du canvas doit rester en tracé.
  (Quand le canvas redessinait la cover, il fallait deux géométries ; ce n'est
  plus le cas depuis que la carte respire elle-même.)
- **Tranche et ombre du boîtier** : toute vignette de grille (favoris compris)
  porte une bande sombre au bord gauche de sa cover (`sparkles::SPINE_FILL`,
  `spine_width`) plus une arête claire, et une ombre portée (`card_style`) :
  la cover se lit comme un boîtier posé, pas comme une image plate. Superposées
  en `stack`, donc aucune mise en page modifiée, et respirent avec la carte
  (elles sont dedans). Mosaïque exclue (covers collées) et liste exclue
  (vignette trop petite).
- **Espacement** : `GAP` (20 px) règle à la fois l'écart entre colonnes et
  entre rangées ; en mosaïque il vaut 0.
- **Idées d'ambiance** (halo, aurore, parallaxe, teintes, ce qui est écarté
  et pourquoi) : `src/IDEES.md`.
- **Manette** : lecture directe de `/dev/input/event*` (permissions uaccess de
  logind, comme Steam), jamais de grab — le jeu garde sa manette. Lecture
  non bloquante (8 ms) pour pouvoir RÉPÉTER pendant qu'un stick est maintenu.
  Croix ET stick gauche naviguent : seuil ~40 % avec hystérésis (relâchement
  25 %), 1er pas puis répétition toutes les 130 ms. **Le seuil est une fraction
  de la course réelle lue dans `absinfo`** : selon le pilote la manette expose
  `-32768..32767` ou `0..255` centré sur 128 (DualSense en Bluetooth, constaté
  sur 054c:09cc) — un seuil en valeur absolue ne se déclenche jamais dans le
  second cas. `--pads` affiche axes et bornes, `GLAUNCHER_PAD_DEBUG=1` trace
  chaque axe lu. L2/R2 (`BTN_TL2`/`BTN_TR2`) règlent la taille des vignettes.
  `BTN_SOUTH` lance, `BTN_EAST` (B/rond) **quitte au DEUXIÈME appui** dans les
  1,5 s (`QUIT_WINDOW`) : le premier affiche « appuie encore sur retour pour
  quitter » et n'agit pas, une autre action annule l'invite — pas d'éjection
  sur une pression malheureuse en jouant ; dans le menu d'image il annule.
  **`BTN_START` bascule grille → liste → mosaïque** et `BTN_NORTH` (Y/Triangle)
  ouvre le menu du tri — Start et Triangle ont été **échangés** le 22/11
  (Start est le geste fréquent, et son glyphe est explicite). `BTN_WEST`
  (X/Carré) ouvre aussi le tri. **Le bouton qui a ouvert un menu le referme**
  (Start sur les vues, Triangle/Carré sur le tri, `T` au clavier) : appuyer
  deux fois ne laisse pas un menu ouvert. **Aucun raccourci n'ouvre les visuels depuis la
  liste** : ils se changent uniquement depuis la page d'un jeu, et sur la page
  seuls la croix gauche/droite, A et B agissent (« Options ▸ » est une
  pastille comme les autres). Dans les menus qui suivent (images, identification,
  « remplacer les visuels ? »), A valide le choix courant et B l'annule —
  « Choisir un fichier… » reste au clavier/souris, un sélecteur de fichiers
  n'a pas de sens à la croix. Glyphes Xbox ou
  PlayStation selon le vendor (`0x045e` / `0x054c`).
- **Roulette de la souris** : tout défilement reçu devient la **cible**
  (`scroll_target = offset`) et arrête l'animation. Nos propres défilements
  n'entrent pas dans cette branche : `tick()` pose `self.scroll` avant de
  demander `scroll_to`, donc l'offset reçu vaut déjà `self.scroll` — un écart
  est forcément un geste de l'utilisateur. Comparer les SENS (pour « laisser
  l'animation continuer si elle va dans le même sens ») mangeait des crans de
  molette et produisait des remontées saccadées (constaté 18/11).
- **Défilement borné par iced — placer avec `scroll_drawn()`** : le
  `scrollable` d'iced borne l'offset demandé à `content - viewport`, donc viser
  au-delà ne dessine rien de plus. `Message::Scrolled` porte les deux valeurs
  (`offset` dessiné et `max` autorisé) ; `scroll_max` les retient et `tick()`
  borne `scroll`/`scroll_target` dessus. Toute position à l'écran (vignettes,
  cadre d'accent du canvas, plage de covers visibles, parallaxe des particules)
  se calcule depuis **`scroll_drawn()`** (le `scroll` cible borné), jamais
  depuis `scroll` seul : le centrage de la **dernière rangée** vise au-delà du
  maximum, et le cadre tombait alors **une demi-rangée au-dessus de la
  vignette** — visible au retour d'un jeu, quand `returning` cesse de masquer
  le cadre (constaté 20/11 : « la sélection est entre deux jeux »). Le défaut
  durait parce qu'iced ne réémet `Scrolled` que si l'offset change : borné au
  maximum avant ET après le lancement, il n'émettait rien et `scroll` restait
  en avance pour toute la session.
- **Couche d'affichage de profondeur constante** : `stack![liste, poussière,
  page, hyperespace, voile, menus]` **toujours** six enfants (un `Row` vide
  tient la place d'une couche fermée ou inutile). iced apparie les états des
  enfants d'un `stack` par
  position : ajouter la page au milieu de la pile recréait la zone défilante,
  et le retour depuis une page ramenait la grille tout en haut, le moindre
  déplacement la re-situant sur le jeu sélectionné (mesuré 18/11 : offset 0 à
  l'ouverture de la page). La page est enveloppée d'un `mouse_area` qui avale
  clic et molette, sinon ils atteignent les vignettes restées dessous.
- **Vue et tri retenus** : le dernier type de vue et le dernier ordre de tri
  sont écrits dans `<cache>/state.txt` (`vue=grille|liste|mosaique`,
  `tri=nom|recents|favoris`, `.part` + renommage) à chaque changement — `V`,
  menu « Type de liste » et menu « Tri » compris. `view` de `config.toml` ne sert plus que de
  valeur par défaut au tout premier lancement. Le launcher n'écrit dans
  `config.toml` que `game_dirs`, `music` et `music_volume` (ses trois réglages) ;
  tout le reste du fichier appartient à `gablue-manager`.
- **Rien à rafraîchir à la main** : le veilleur inotify met la liste à jour,
  donc plus de touche `R` ni de bouton Start/Menu « Rafraîchir » (retour 18/11).
- **Pied de page** : l'aide des touches est **centrée**, et **à gauche la
  pastille de tri puis le total de la bibliothèque** (le nombre de favoris
  quand le tri est Favoris : c'est la liste affichée) — le compte a été
  rapproché du tri le 24/11, les deux se lisant d'un coup ; le **message
  d'état** suit, et la place libre à droite revient au **coin bas droit**
  (réseau, manettes, roues). Le nom du jeu
  n'y est **pas** répété (la vignette sélectionnée le porte déjà ; demandé le
  20/11). Le tri est une **pastille** (bordure d'accent, « TRI » + libellé
  en 15 px) parce qu'une ligne grise se perdait dans le pied de page — et
  **changer de tri remet en début de liste** (sélection 0, défilement 0 posé et
  non animé) : sans ça on triait sans jamais voir le nouvel ordre, la liste
  restant là où elle était. `T` (ou Carré/X) ouvre le **menu « Tri »** — A→Z,
  Récents, Favoris — construit comme le menu des vues (`sort_menu_view`, tri
  courant coché). Deux couches superposées (`stack![côtés, aides centrées]`)
  parce qu'une `Row` ne sait pas centrer dans la fenêtre quand les blocs des
  côtés n'ont pas la même largeur ; un `Stack` prenant sa hauteur sur son
  PREMIER enfant, c'est la rangée des côtés (pastille de tri, plus haute que le
  texte d'aide au clavier) qui la donne — l'aide est centrée verticalement
  (`center_y(Fill)`). Dans l'autre ordre, la pastille dépassait sous sa bordure
  en mode clavier (constaté 20/11). La **seule** statistique est le total de la
  bibliothèque (demandé le 20/11 en bas à droite) ; la progression des covers
  (« covers 12/540 ») était du bruit, retirée le 20/11. L'aide est en
  **Lato embarquée** (`app::HELP_FONT`, `assets/fonts/Lato-Regular.ttf`, OFL) :
  la sans-serif système change d'un poste à l'autre. Le titre « glauncher » a
  disparu de son côté (l'en-tête n'existe plus). Le libellé de tri est
  « **Récents** » (majuscule, demandé le 20/11).
- **Touches MAINTENUES ignorées au clavier** : le noyau répète `KeyPressed`
  (auto-repeat X11/Wayland) ; sans garde, un `T` un peu long basculait le tri
  deux fois (constaté 20/11). La navigation clavier n'a jamais été répétée, donc
  `repeat: true` est simplement ignoré (`on_key`).
- **Manette : répétition des boutons** maintenus (croix et L1/R1) — délai
  350 ms puis toutes les 130 ms, exactement comme le stick (`repeats`). Sans
  ça, maintenir R1 ne faisait qu'une page. Valider/retour/menu ne se répètent
  jamais (le double appui de « retour » se déclencherait seul).
- **Fenêtre active** : clavier ET manette sont ignorés quand la fenêtre n'a pas
  le focus (`window::Event::Focused/Unfocused`). La manette est lue sur
  `/dev/input` directement : sans cette garde, un appui pendant qu'on joue ou
  qu'on travaille ailleurs pilotait le launcher.
- **Pointeur caché au repos** (demandé le 20/11) : `src/cursor.rs` enveloppe
  toute la vue (`cursor::Idle`, au-dessus du `mouse_area` du clic droit, donc
  au-dessus de toutes les couches) et rend `mouse::Interaction::Hidden` quand la
  souris n'a pas servi depuis `IDLE` (2 s) — c'est le **seul** chemin de iced
  pour cacher le pointeur : `iced_winit::Window::update_mouse` traduit `Hidden`
  en `set_cursor_visible(false)` et **rétablit le curseur du thème** (avec la
  bonne forme : main sur une pastille, etc.) dès qu'une autre interaction est
  rendue. Le pointeur est **caché dès l'ouverture** (`last: Option<Instant>`,
  `None` au départ) : une fenêtre qui apparaît sous un pointeur immobile ne doit
  pas montrer de flèche, d'où `CursorEntered` volontairement **ignoré** —
  seuls un déplacement (position différente de la dernière vue), un clic ou un
  cran de molette le réveillent. Trois pièges (constatés à la lecture puis
  mesurés le 20/11) :
  (1) l'interaction n'est calculée que pendant une **passe de mise à jour**,
  jamais au repos : sans message l'horloge ne tourne pas, d'où la relève
  `cursor::TICK` (250 ms, fil endormi + `Message::Nothing`, **aucune image**
  demandée) armée par `App::subscription` seulement si la fenêtre a le focus et
  que ni animation ni poussière d'étoiles ne redessinent déjà ;
  (2) `set_cursor_visible` n'est appelé que quand l'interaction **change** :
  l'horodatage doit repasser par une interaction visible au moindre mouvement,
  sinon le pointeur resterait caché après une sortie/ré-entrée de la souris
  (winit réapplique l'état caché à l'entrée, `reload_cursor_style`) ;
  (3) **KWin envoie un mouvement synthétique** quand une surface apparaît sous
  le pointeur (`CursorMoved` à +66 ms à l'ouverture d'un plein écran, mesuré
  par la trace) : le compter comme un geste rallumait la flèche 2 s à chaque
  ouverture — « je vois le curseur 2 secondes avant qu'il disparaisse ». La
  **première** position vue depuis une entrée n'est donc qu'une référence,
  seul un **changement** de position (ou un clic/molette) réveille.
  `GLAUNCHER_CURSOR_DEBUG=1` trace chaque évènement et chaque bascule,
  horodatés : c'est ainsi que le piège (3) a été trouvé, et le moyen de
  revérifier ce que le compositeur envoie sans deviner.
  Vérifié à l'écran (captures `include_cursor`) : pas de flèche à l'ouverture
  sous le pointeur (ni pendant les 2 s qui suivent), flèche de retour au
  premier vrai déplacement, disparue ~2 s après.
- **Langage visuel commun avec l'overlay** : le launcher et `gpad` sont un
  **écosystème**, pas deux applications. Les options (`Réglages`, navigateur de
  dossiers, galerie) utilisent les **mêmes contrôles** que l'overlay — ligne
  sélectionnée en pastille d'accent, **interrupteur** (pilule + bouton) pour un
  état on/off, **barre de réglage** (piste arrondie + remplissage d'accent) pour
  un pourcentage, aide du bas en **glyphes de manette** — et non du texte
  (« Croix = … ») quand c'est la manette qui pilote (`App::aide_menu`).
  **Casse alignée** : le libellé est capitalisé par `majuscule()` (« naviguer »
  → « Naviguer »), comme les glyphes de manette (« Ouvrir », « Tri ») — les
  deux aides commencent en minuscule/majuscule différemment sinon, et se lisent
  comme deux applications (signalé le 24/11). La capitalisation ne touche que
  la **première lettre** (accents préservés) et s'applique à tous les appels de
  `key_hint`, donc partout d'un coup.
  L'interrupteur et la barre s'inversent sur la ligne sélectionnée (piste
  sombre, remplissage noir) pour rester lisibles sur l'accent.
- **Aucun message d'état pour un réglage** qui se voit déjà sur sa ligne
  (musique, volume) : « musique de fond : 60 % » dans le pied de page était du
  bruit (demandé le 22/11). Seule une **erreur d'écriture** s'affiche.
- **Aides dynamiques** : les glyphes du pied de page et des menus suivent le
  **dernier périphérique utilisé** (`Input::Keyboard` / `Input::Pad`), pas la
  simple présence d'une manette : reprendre le clavier fait réapparaître les
  touches, un appui manette (ou son branchement) remet les glyphes.
- **Réseau** : toutes les requêtes SteamGridDB passent par un `ureq::Agent`
  partagé avec délais (30 s global, 10 s de connexion). Sans timeout, une
  connexion qui cale bloquait le worker indéfiniment : menu d'image figé sur
  « recherche… » et covers de fond suspendues (constaté 18/11).
- **Veilleur** : inotify récursif ; si la pose récursive échoue (préfixe Wine
  avec lien cassé, `z:` → `/boot`) on se rabat sur le 1er niveau et on le dit
  dans le journal — sinon le dossier n'était pas surveillé du tout.
- **Lancement réel testable** : `GLAUNCHER_LAUNCH_CMD` remplace la commande
  (utile pour vérifier le chemin de lancement sans démarrer un jeu).

## Décisions techniques

- **iced 0.14 + wgpu**, pas GTK4 : Kinoite n'a pas les en-têtes gtk4 et on ne
  peut pas les installer ; iced est 100 % Rust, Wayland natif, et se distribue
  en un seul binaire.
- **evdev**, pas gilrs/SDL : gilrs tire `libudev-sys` (en-tête absent sur
  Kinoite). evdev lit les évènements sans dépendance système.
- **Bus interne** (`UnboundedSender<Message>`) plutôt que `Task::perform` pour
  les évènements longs : la manette et l'attente de fin de jeu arrivent par
  `post()`, l'UI ne bloque jamais.

## À peaufiner (ordre pressenti)

1. ~~Extraire l'icône du paquet lui-même~~ → fait (`.icon.png` via `unsquashfs
   -cat`) ; reste le repli icône d'`.exe` pour les `.wgp` sans `.icon.png`
   (squashfuse + `exeiconextract.py`, comme `create-game-shortcut.sh`).
2. Filtrer/rechercher, dernier jeu lancé (les favoris sont faits).
3. Empêcher la mise en veille pendant un jeu (le pointeur, lui, est masqué au
   repos dans le launcher ; pendant une partie c'est le jeu qui gère le sien).
