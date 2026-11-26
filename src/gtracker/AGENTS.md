# GTracker — contrat du sous-projet

Navigateur et lecteur de bandes-son de jeux rétro (joshw.info), GUI egui.
Le code vit **ici** (`gablue/src/gtracker/`) ; le dépôt standalone et le TUI Go
d'origine ont été abandonnés (20/11), il n'en reste rien.

Montage « B » : `zxtune123` décode en WAV, `rodio` joue, la boucle « comme dans le
jeu » est tenue par notre source audio (pas par zxtune).

## Nom et périmètre du chantier

L'application s'appelle **GTracker** (ex-RetroPlayer), et le mode multitrack porte
le même nom : paquets `gtracker` / `gtracker-core`, binaire `gtracker`, app_id
Wayland `gtracker`, cache `~/.cache/gtracker`, variables `GTRACKER_*`.

Le renommage est **fait dans le dépôt** (22/11/2026, un seul commit : jamais de
renommage partiel, l'image pointerait sur un binaire absent) : `src/gtracker/`,
`files/scripts/build-gtracker` + étape `gtracker-build` du Containerfile,
`gtracker.desktop` (`Name`/`Exec`/`StartupWMClass`) et `gtracker.svg` — le dessin
de l'icône est celui de l'ancien RetroPlayer, inchangé (`assets/icon.*`).

Pendant le chantier, le travail vivait dans une copie hors dépôt
(`~/.cache/opencode/rp-studio`) : le dépôt est partagé par plusieurs sessions,
une sortie de build n'a rien à y faire, et un commit y est public (il déclenche
un build d'images). **Toute modification en cours repasse par cette copie** avant
d'être portée au dépôt — jamais d'édition directe sur `src/gtracker/` partagé
sans `git status` préalable.

Les données déjà sur le poste sont reprises, pas perdues : `util::cache_root`
renomme `~/.cache/retroplayer` → `~/.cache/gtracker` au premier lancement (et
`retroplayer.db` → `gtracker.db`), et `RETROPLAYER_CACHE`/`RETROPLAYER_RAM_MB`
restent acceptés en repli. Les exports déjà présents dans
`~/Musique/RetroPlayer/` ne bougent pas ; les nouveaux vont dans
`~/Musique/GTracker/`.

## Installation dans l'image

- `Containerfile-gablue`, étape `gtracker-build` (toolchain jetée) :
  `COPY src/gtracker/{Cargo.toml,Cargo.lock,crates,assets}` puis
  `files/scripts/build-gtracker` (`alsa-lib-devel` + `pkg-config` pour `cpal`)
  → `/out/gtracker`, récupéré par
  `COPY --from=gtracker-build /out/gtracker /usr/bin/gtracker`.
  **`assets/` est obligatoire** : l'icône est embarquée par `include_bytes!`.
- `.desktop` : `files/system/all/usr/share/applications/gtracker.desktop`
  (`StartupWMClass=gtracker`) ; icône vectorielle :
  `files/system/all/usr/share/icons/hicolor/scalable/apps/gtracker.svg`.
  L'app_id Wayland est fixé dans `main.rs` (`ViewportBuilder::with_app_id`) : sans
  lui KWin ne rattache pas la fenêtre au `.desktop` et affiche l'icône générique.
- Ouverture **maximisée** : `ViewportBuilder::with_maximized(true)` ne suffit pas
  — l'app renvoie `ViewportCommand::Maximized(true)` une fois, première trame
  affichée (`App::maximized_once`, laissé à `true` ensuite pour ne pas
  re-maximiser si l'utilisateur réduit). Le drapeau du builder était seul
  inopérant en client XWayland (constaté eframe 0.37 / KWin, 22/11 : fenêtre
  restée à 1240×780) ; depuis le passage en Wayland natif la double ceinture ne
  coûte rien et couvre les compositeurs qui l'ignorent.
- Icône : `assets/make-icon.py` (PIL, supersampling 4×) écrit `assets/icon.rgba`
  (128×128, embarquée), `assets/icon-512.png` et `assets/icon.svg`, **plus la copie
  installée** dans `files/system/.../scalable/apps/` — une seule source, jamais
  deux dessins à garder synchro. Relancer le script après toute retouche.
- Décodeur de secours : étape `vgmstream-build` →
  `COPY --from=vgmstream-build /out/vgmstream-cli /usr/bin/vgmstream-cli`
  (`vgmstream.rs` le trouve par PATH).
- Runtime : `zxtune123` (décodage WAV de GTracker) est installé par
  `files/scripts/post-install` ; le tarball ZXTune apporte aussi `zxtune-qt`
  (GUI Qt) et son entrée de bureau, **exclus** du démontage tar — ne pas
  ré-exclure `zxtune123` comme le faisait l'ancienne version Go.

## Layout

- `crates/core` — bibliothèque : base SQLite (`db`), scraping joshw.info (`joshw`),
  téléchargement/extraction 7z + `.m3u` (`archive`, `m3u`), décodage (`decode`),
  lecteur (`player`), tempo/grille de découpe (`tempo`), export MP3/FLAC + tags (`convert`).
  GTracker : `mixer` (transport/voix/rendu), `stretch`, `key`, `project`, `peaks` —
  contrat dans `crates/core/AGENTS.md`.
- `crates/gui` — binaire `gtracker`, egui/eframe **Wayland natif** (`eframe`
  avec la seule fonctionnalité `wayland` : la règle de la maison interdit X11, et
  un client XWayland paye deux fois — KWin ignore `with_maximized`, l'app_id
  n'est qu'un `WM_CLASS`). Coquille unique dans
  `crates/gui/src/shell/` (bibliothèque, espace de travail, inspecteur) ;
  arrangement/éditeur dans `crates/gui/src/gtracker/` — contrat dans
  `crates/gui/src/gtracker/AGENTS.md`.
- `bin/zxtune123` n'est pas versionné : cherché dans `$ZXTUNE`,
  `~/.cache/gtracker/bin`, `bin/`, puis `PATH`.

## Coquille unique (objectif 2)

- **Une seule fenêtre, plus d'onglets plein écran ni d'enum `View`** :
  bibliothèque latérale (Catalogue et Téléchargés = **filtres**), espace de
  travail central (`Workspace::Arrange` = pistes + timeline + éditeur compact en
  bas ; `Workspace::Edit` = Découpe complète), inspecteur (playlist + détails),
  **un seul transport visible** (projet en Arrangement, pré-écoute en Éditeur).
- Barre haute = **document** : nom, nouveau, enregistrer (ctrl+S), ouvrir,
  exporter, undo/redo, source courante, repli des panneaux. Les actions du
  projet ne disparaissent donc jamais en changeant d'espace.
- **Un document = un projet** : double-clic dans la playlist ou dépôt de fichier
  ajoute une source et crée une piste ; clic droit « éditer » ajoute la source
  sans piste ; un simple clic écoute. Le rechargement d'un projet ne crée rien.
- Bascule d'espace = coupe le flux de l'autre (projet ↔ pré-écoute), jamais deux
  transports qui sonnent ensemble. Le retour dans l'éditeur **réinstalle la
  piste** (`LibAudio::restore`, même PCM, position gardée) : `stop_source`
  laissait une sortie vide (« je peux plus jouer », vécu 21/11).
- **Écoute** : clic sur une tuile = la tranche joue une fois ; clic sur un clip
  de la timeline = lecture depuis son début. Le glisser reste le geste qui
  compose (tuile → piste) ; plusieurs sources sur plusieurs pistes = mixage.
- **Panneaux** : boutons `Bibliothèque`/`Inspecteur` dans la barre haute,
  `F8`/`F7`, et quand un panneau est fermé un **rail cliquable** sur le bord
  porte son nom à la verticale — il ne peut plus « disparaître » sans laisser de
  porte de retour. Pas de croix dans l'en-tête de l'inspecteur (un clic au
  hasard dans le coin ne doit pas le replier, vécu 21/11).
- **Thème unique** (`mod theme` dans `main.rs`) : couleurs, densité, typographie ;
  la coquille, la bibliothèque et l'arrangement n'ont plus chacun leurs gris.
  Les icônes sont dessinées (triangle/carré du transport) : les polices
  embarquées n'ont pas ▶/■/▸ (carrés vides, vécu 21/11).
- La **dernière piste** peut être supprimée ; déposer une tranche recrée la piste
  de sa source.
- **Arrêter le son** : bouton `Arrêter` dans l'en-tête de l'inspecteur (visible
  dès qu'un flux sonne), clic sur la piste en cours dans la playlist, ou
  `stop_sound()` (`p.halt()`, pas `stop_source` : le PCM reste chargé et
  « Lecture » repart de la position). `gtracker.silence()` coupe le projet sans
  remettre la tête à zéro.
- **Export** : deux boutons explicites (`Exporter en MP3`, `Exporter en FLAC`)
  dans l'inspecteur et dans la barre de l'éditeur (pas de pastille de format à
  cocher puis un bouton unique).
- **Défaut de découpe : 1 mesure** (`clip_beats = 4.0`, 4/4) dans l'éditeur
  comme dans l'arrangement (le passage à 2 mesures du 21/11 a été annulé, demande
  user 22/11).
- **Timeline** : molette = zoom autour du pointeur (`zoom` 8–400 px/temps),
  Maj+molette = défilement ; la tête est **suivie** pendant la lecture
  (`follow_scroll` : la vue ne bouge que si la tête sort de la fenêtre, puis la
  replace à 35 %). Un zoom ou un défilement manuel coupe le suivi, réarmé par
  « Écouter ».
- **Éditeur de sources empilé** : l'arrangement montre **toutes** les sources
  l'une sous l'autre (`source_panel` → `source_row`), chacune avec son nom, son
  tempo, sa **bande d'onde** et toutes ses tranches ; plus de sélecteur de
  source. Le clic sur le nom désigne la source de l'inspecteur, le clic sur une
  tranche l'écoute, le glisser la pose. Une rangée par source, défilement
  vertical.
- **La boucle se corrige dans l'arrangement** (`source_wave`) : les deux bords
  de la bande d'onde se tirent au pixel (Maj = calage sur le pas de découpe),
  exactement comme la poignée du mode Découpe — plus d'aller-retour d'espace.
  La boucle vit dans le **projet** (`Source.loop_start/loop_end`,
  `serde(default)`), initialisée depuis `Decoded.window` à l'ajout de la source,
  et `slices_for` découpe **de `loop_start` à `loop_end`** (une intro ne
  fabrique donc plus de tranche). Les deux sens sont synchronisés : correction
  dans l'éditeur → `set_source_loop` (l'arrangement redécoupe) ; correction dans
  l'arrangement → `Cmd::SourceLoop` (la coquille recale `loop_win`, la découpe
  de l'éditeur et le lecteur si c'est la même piste, et écrit la base
  `tracks.loop_*` seulement si la source vient de l'archive ouverte).
- **Méta des listes** : la ligne console/date de la liste des téléchargés est en
  `theme::FAINT` 10,5 (elle se lisait comme un second titre, retour user 22/11).
- **Ce qui sonne se voit** : le clip sous la tête de lecture est allumé dans la
  timeline (bord accent + jauge de progression) et la tuile du segment en cours
  s'allume dans l'éditeur — y compris en lecture simple de la piste
  (`clip_at_player`), pas seulement pendant un montage.
- Preuve : `./check-ui.sh` (aucun `enum View`, tests `shell` avec marqueur
  `SHELL OK`, puis `./check.sh`). Dernière ligne attendue : `UI OK`.

## Build / test

```
./build.sh                      # dev local : génère .build/alsa/alsa.pc (en-têtes de brew) puis cargo build --release
cargo test -p gtracker-core  # loopdetect, tempo, m3u, liens et noms joshw
cargo test -p gtracker       # pads, longueurs de découpe, dépôt dans le montage,
                             # coquille (shell) et gestes simulés sans écran
./check.sh                   # moteur + GTracker, doit finir par OK
./check-ui.sh                # critère de l'interface unifiée, doit finir par UI OK
cargo run -p gtracker-core --example loopcheck -- /chemin/piste.psf   # fenêtre + tours
cargo run -p gtracker-core --example probe -- /chemin/piste.spc        # durée sans rendu
cargo run -p gtracker-core --example inspect -- /chemin/fichier.vgz
./run.sh
```

Dans l'image, le build ne passe pas par `build.sh` : `build-gtracker` installe
`alsa-lib-devel`/`pkg-config` et appelle `cargo build --release --locked` (le
bricolage `alsa.pc` ne sert qu'à Kinoite, où les en-têtes manquent).
Vérification headless préférée à la GUI : `loopcheck` imprime la fenêtre détectée
et le nombre de tours en quelques centaines de ms.

## Règles et pièges (vécu)

- `cpal` exige les en-têtes ALSA absents de Kinoite : headers de brew + `libasound.so.2`
  du système lié via un `alsa.pc` généré. Ne pas remplacer par la libasound de brew
  (les plugins PipeWire sont ceux du système).
- `zxtune123 --wav filename=…` **n'écrit rien** (sortie 0, silence) si l'extension de
  sortie n'est pas audio → toujours écrire `.part.wav` puis renommer (vécu 19/11).
- Compteurs VGM en échantillons 44,1 kHz à partir de la v1.50 ; en ticks avant.
- `--null` joue à vitesse maximale : réservé aux tests de touches.
- L'API egui 0.37 est centrée `Ui` : `App::ui(&mut self, ui, frame)`, `Panel::top(..).show(ui, ..)`,
  `CentralPanel::show(ui, ..)`.
- Le dossier `folder` de la base est la source de l'URL (`a`…`z`, `0-9`, `zzz_*`) ;
  recalculer la première lettre casse les entrées `zzz_*` (bug de la version Go).
- **Mise à jour du catalogue : un dossier raté cachait des jeux entiers.** `joshw::update`
  lançait un thread par site (24 requêtes d'un coup, timeout 15 s) et avalait tout
  `Err` (`if let Ok(…)`) : un passage laissait des **sites entiers** absents (GBS, SMD,
  Wii…) et des dossiers tronqués, sans un mot — « il ne trouve pas Yoshi's Island sur
  SNES » (vécu 24/11 : base à 25 329 lignes au lieu de 39 646, dossier `spc/s` à 87
  entrées sur 410). Règles : un **404** est un dossier absent du site (tous n'ont pas
  `zzz_prototypes`) → `Ok("")`, pas un raté ; toute autre erreur **remet le dossier en
  fin de file** (5 tentatives, pause croissante, 4 requêtes à la fois — l'IPv6 de
  joshw.info est injoignable d'ici, `ureq` sort parfois « Network is unreachable »,
  constaté 24/11) ; `UpdateReport` porte `inserted` **et** `failed`, l'UI affiche
  « N dossiers non récupérés, relancer la mise à jour » et les URL partent sur stderr.
  Mesure après correction : 0 raté sur 696 dossiers, 128 s. L'update est **additif**
  (`INSERT OR IGNORE`, rien n'est supprimé) : le relancer ne coûte que le réseau.
- Cache : `~/.cache/gtracker/{gtracker.db,archives/<id>,bin}` — compatible avec la
  base de l'ancienne version (mêmes tables `files`). Les échantillons décodés ne vont
  pas sur le disque : cache mémoire (`GTRACKER_RAM_MB`, 512 Mo par défaut).
- `archives/<id>/.complete` contient `CACHE_VERSION` : l'incrémenter quand l'analyse des
  pistes change, sinon les caches existants gardent un index périmé **sans retélécharger**
  (vécu 19/11 : titres « Title Theme,0:01:55.087 » figés dans la base).
- Un dossier `archives/<id>` sans `.complete` **n'est plus supprimé** : une analyse de
  pistes qui échoue (aucune piste reconnue) laissait le dossier sans marque, et la
  tentative suivante effaçait des centaines de Mo déjà extraits avant de les
  re-télécharger (vécu 21/11, Super Mario Galaxy : 732 Mo). L'extraction 7z réécrit
  par-dessus, elle est idempotente. Conséquence : `CACHE_VERSION` doit être incrémenté
  dès que `scan` change (passé à `5` quand `walk` a accepté les formats vgmstream).
- **Index des `.m3u` : vérifier la base avant de s'y fier.** Les sets GBS écrivent
  `0,1,…,N-1` (le premier morceau porte l'index 0) alors que zxtune compte à partir
  de 1 (`?#0` est invalide) : sans décalage, chaque piste jouait le morceau
  *précédent* — titre, durée et boucle tombaient sur un autre morceau (vécu 21/11,
  Wario Land GB : « Title » jouait un thème de niveau). `m3u::parse` décale tout de
  +1 dès qu'une ligne porte l'index 0 (NSF, indexés à 1, ne bougent pas ; `$00`
  hexa est déjà converti en 1). Une réindexation vide les fenêtres A/B de l'entrée
  (`loops`) : elles désigneraient un autre morceau.
- **Durée `.m3u` = arrondi à la seconde, donc trop courte.** Boucler dessus perd un
  bout à chaque tour (vécu 21/11, Kirby's Adventure NES « Vegetable Valley » : 38 s
  annoncés, boucle réelle **38,318 s** — mesuré sur l'enveloppe, pic net, et le
  signal n'est *pas* reproductible à l'échantillon, seul le RMS l'est). Le rendu
  contient plusieurs tours : `loopdetect::refine_announced` cherche la meilleure
  médiane de corrélation à ±0,8 s (enveloppe 5 ms, fenêtres 6 s, toutes les
  positions) et n'accepte que ≥ 0,90. Sous ce seuil seulement, `detect` peut
  proposer une période dans ±10 % de la durée annoncée. Sonde : 300–400 ms par piste
  (mise en cache mémoire).
- **Le BPM est recalé sur la fin de boucle** (`Tempo::across`) : un BPM estimé à
  0,2 % près dérive d'un demi-temps sur 40 s et la dernière coupe tombe au milieu
  d'une attaque (« il manque un mini bout »). On vise le nombre entier de temps le
  plus proche de la fenêtre, et on ne touche au BPM que si l'écart reste < 3 % (sinon
  c'est la durée qui n'est pas un multiple de temps). La phase absolue est conservée.
- Durées `.m3u` : `88` (s), `M:SS`, `M:SS.mmm`, `H:MM:SS` — `parse_len` accepte les quatre
  et traite `0:00:00` comme une valeur valide (champ de boucle vide) ; filtrer les zéros à
  l'usage, pas au parsing.
- Durées manquantes (PSF/XA, sets sans `.m3u`) : `decode::probe` lance zxtune avec
  `stdbuf -oL --wav` et coupe le processus dès la ligne `Time: … Loop duration: …`
  (~30 ms/piste, aucun rendu gardé). zxtune met ses métadonnées sur **stdout**, et
  sans `stdbuf` elles restent dans le tampon jusqu'à la fin du rendu.
- Fondu PSF : zxtune l'applique **dans** la durée rendue en lecture unique (`fade`
  des tags `[TAG]`), mais **pas** en mode boucle — vérifié : `--loop --wav` rend des
  centaines de secondes à niveau constant (aucun fondu) et répète toutes les `length`.
  Donc la piste boucle sur `length` entière, pas sur `length − fade` (piège : couper
  au début du fondu ampute 8 s de musique, vécu Ape Escape 19/11).
  Le rendu de lecture est fait depuis une copie `.[stem].__nofade.psf` (tag `fade=0`,
  même dossier pour que `_lib=` résolve), supprimée juste après ; `walk()` ignore les
  fichiers cachés pour qu'elle ne devienne jamais une piste.
- **Boucle officielle des `.spc` (SNES)** : le tag ID666 texte la porte —
  « seconds until loop » (3 chiffres à 0xA9) + fondu (`0xAC`, ms) — et zxtune la
  respecte (595 s de boucle + 10 s de fondu = WAV de 605 s). `spc::loop_window`
  la lit et `decode`/`probe` la préfèrent au `.m3u` : `Some((0, 595))`, une passe.
  78 % des rips joshw mesurés portent le champ (300/386) ; les rippers le bourrent
  de **NUL** ou d'espaces, les deux se retirent avant `parse` (vécu 21/11 : `5\0\0`
  ignoré, la piste tombait sur la détection d'enveloppe). Pas de champ équivalent
  ailleurs : NSF/GBS/HES/SID n'ont pas de point de boucle (le driver boucle à
  l'exécution), PSF/USF donnent `length`/`fade`, VGM et GENH leur en-tête.
- `tracks.loop` / `tracks.loop_start` : `NULL` = jamais mesuré, `0` = mesuré sans
  boucle distincte, `> 0` = fenêtre. C'est ce qui évite de relancer la sonde à chaque
  ouverture. `PRAGMA user_version` invalide les mesures d'une version précédente
  (v2 : fenêtres PSF fausses ; v3 : boucle `.spc` non lue ; v4 : blanc de tête
  du rendu retiré, la timeline a bougé de 28 à 75 ms → `UPDATE tracks SET
  loop = NULL, loop_start = NULL`).
- Boucle activable à tout moment, y compris après la fin du morceau : rodio a
  consommé la source, donc `Player` garde les PCM et `queue_from()` réinjecte une
  `LoopSource` (même chose pour seek/lecture après la fin). Le mode est global
  (piste suivante incluse).
- En boucle, l'UI affiche la **durée du cycle** (la fenêtre) ou, à défaut, la durée
  de la piste : zxtune rend 3 min fixes pour un NSF de 1:55, et la barre suit le cycle
  pour que le curseur warp au point de reprise. Le libellé rappelle la durée totale de
  la piste quand le rendu contient plusieurs tours.
- Cache audio **en mémoire seulement** (`decode.rs`) : le rendu passe par un WAV
  temporaire dans `/dev/shm` aussitôt relu puis supprimé (fichier *et* dossier), et les
  échantillons restent en RAM pour les relectures — LRU plafonné à 512 Mo de PCM
  (`GTRACKER_RAM_MB`), analyse de boucle comprise dans l'entrée. Aucun WAV ne
  persiste : l'ancien cache disque montait à 1,7 Go pour 47 pistes (vécu 20/11) pour un
  gain nul. Supprimer une archive vide le cache mémoire (`decode::clear_ram_cache`).
- **La liste des pistes et le décodeur doivent accepter les mêmes extensions** :
  `walk` prend `is_audio(...) || vgmstream::handles(...)` (plus les fichiers non
  vides), sinon une archive dont tout l'audio passe par vgmstream apparaît **vide**
  (« aucune piste reconnue »). Vécu 21/11 : Super Mario Galaxy = 74 `.ast` +
  105 `.bms` de séquence, zéro piste — `.ast` manquait dans `vgmstream::EXTENSIONS`
  et dans `AUDIO_EXTS`. Les `.txtp` de 0 octet (marqueurs de canaux d'un rip
  multi-piste) sont ignorés ; les `.bms` Nintendo ne sont lus par **aucun** des deux
  décodeurs (zxtune sort en erreur) → ne pas les ajouter à `AUDIO_EXTS`.
  Diagnostic : `cargo run --release -p gtracker-core --example scancheck -- <id>`.
- Formats que zxtune ne lit pas (`.txtp`, `.fsb`, `.bfstm`, `.btsnd`, `.hca`, `.at3`,
  `.at9`, `.xma`…) : repli sur `vgmstream-cli` (`vgmstream.rs`) — métadonnées `-I`
  (durée, points de boucle du format), décodage `-l 1 -f 0` (un tour, sans fondu),
  sous-piste `-s N`. `.vag` et `.mp3` restent à zxtune (reconnus, mesuré sur un set PS3
  `psf3.joshw.info`). Si vgmstream échoue (codec absent, conteneur exotique), `decode`
  **retombe sur zxtune** au lieu d'afficher une erreur.
- `loopingInfo` de `-I` est un sous-objet `{"start":N,"end":M}` (pas `loopStart`) : le
  lire à part, sinon la boucle du fichier est perdue (vécu 20/11, `.btsnd` Wii U).
- Build image : `files/scripts/build-vgmstream` (étape `vgmstream-build`), version
  r2168 épinglée + sha256. `USE_VORBIS` et `USE_MPEG` **indispensables** : les banques
  FSB5 des sets Switch/Wii U sont du Vorbis (39/207 fichiers lisibles sans, 207/207
  avec). `USE_FFMPEG=OFF` : les libs de l'image viennent de negativo17, `ffmpeg-devel`
  de Fedora donnerait un autre soname → binaire qui ne charge pas. Prix : Opus dans un
  FSB5 ne passe pas.
- PSF : `length` n'est **pas** la période de boucle, c'est N tours de la séquence
  (`loopdetect` : autocorrélation de l'enveloppe RMS, candidats `total/N`, affinage
  audio). Boucler sur `length` rejouait la même partie deux fois (vécu 20/11, Ape
  Escape « Dark Ruins » : 6:02 = 2 × 3:00.73). Résultat mémorisé dans
  `decoded/<clé>.loop`, invalidé avec le WAV.
- Le premier tour peut être précédé d'un silence que les tours suivants n'ont pas
  (zxtune rejoue à partir du point de boucle) : `leading_silence()` démarre la boucle
  après lui, sinon on entend un blanc à chaque tour (vécu 20/11, « Stage Select » :
  218 ms). Une fenêtre « piste entière » en base (`loops`) cède à la période détectée :
  ce n'est pas un choix A/B.
- Une intro **non silencieuse** est sautée seulement si la transition est franche
  (`find_start` : 8 s de contenu répété après 8 s qui ne le sont pas) — vécu
  « Western Land » : intro 52 s puis boucle de 64 s, pour un `length` de 163 s (2,55
  tours, donc pas de « nombre entier de tours » à chercher). Un rendu qui varie à
  l'échantillon près (état de réverbération) ne donne aucune frontière fiable : on ne
  coupe rien.
- Période : `refine` garde le **maximum** de 4 fenêtres d'1 s (2 moitiés du rendu),
  pas la moyenne — mesuré 20/11 sur « Stage Select » : moyenne 0,43 partout (pic noyé
  par la réverbération, détection perdue), maximum 0,96. Le prix est qu'une coïncidence
  peut faire gagner quelques ms. D'où l'étape suivante.
- Recalage de couture (`detect` étape 5) : la période globale reste à quelques ms près,
  donc on recale la **fin** de boucle sur le début en corrélant une fenêtre courte
  (0,25 s) sur ±30 ms — court exprès, sinon la réverbération noie la comparaison.
  Mesuré : « Stage Select » passe de 5,9000 s (période de la grille) à 5,8835 s, ce que
  confirme une corrélation à 2 s sur le rendu (±0,1 ms).
- Le score d'un décalage est la **fraction** des positions où le contenu se répète
  (enveloppe RMS, fenêtres 2 s, balayage à 50 ms — une grille de 0,25 s rate le pic
  d'une période courte, vécu « Stage Select »). Un décalage qui revient des dizaines
  de fois (`total / P > 8`) est un motif, pas la boucle, et est écarté. L'analyse est
  refaite à chaque décodage (elle vit dans l'entrée du cache mémoire) : pas de fichier
  annexe à versionner.
- **Formats sans aucune longueur (USF/miniusf N64, SPC sans `.m3u`) : la boucle
  reste le fichier entier**, et c'est mesuré, pas un oubli. Le rendu d'un tour ne
  contient aucune répétition exploitable (corrélation max 0,28 sur « World Map »),
  et `--loop` de zxtune relance le module toutes les `length` au lieu de révéler le
  point de boucle (relevé 21/11 sur « World Map » : audio continu, mais la
  corrélation tombe à 0 aux redémarrages du module). Le rendu en mode boucle ne
  peut pas être récupéré
  en flux : `filename=/dev/stdout` ne rend que l'en-tête (156 octets), le backend
  WAV écrit un en-tête provisoire et a besoin d'un fichier cherchable. La piste
  boucle donc sur `total`, ce qui inclut l'intro éventuelle — le manuel A/B reste
  la solution pour ces sets.
- **Blanc numérique de tête de rendu : retiré du PCM** (vécu 21/11 sur FF8
  « Force Your Way », 36 ms ; étendu le 26/11). zxtune écrit **des zéros exacts**
  au début de chaque WAV rendu — la montée en régime de son moteur : mesuré
  74,94 ms à l'identique sur les USF N64 (Banjo-Tooie) et 28 à 39 ms sur les PSF
  (FF8), les tranches suivantes n'ayant aucun blanc. Au départ, seul
  `LoopSource.head` (`head_silence`) l'enjambait à la lecture : la timeline le
  gardait, la grille se calait dessus et la **première tranche démarrait
  quelques ms après 0** — le début de la piste n'appartenait à aucune tranche
  (« il manque un mini bout au début de chaque musique, du coup ça fausse la
  découpe », retour user 26/11, N64 **et** PSX). `decode_uncached` le retire
  donc du PCM (`head_zeros`, zéros exacts seulement, cap 200 ms) et décale
  d'autant les durées et fenêtres **absolues** (en-tête VGM/SPC, longueur
  `.m3u`) avec `shift_window` : sans ça la boucle désignerait un autre instant.
  Mesure après : `silencecheck` donne `0.00 ms` de blanc enjambé et la première
  tranche à `0.000` (Banjo-Tooie « Cauldron Keep », FF8 « Blue Fields »).
  `head_silence`/`LoopSource.head` restent en filet (PCM vgmstream ou piste
  déjà décodée), **rendus en échantillons entrelacés, pas en trames** — tout
  `pos` de `LoopSource` est un index d'échantillon. Version trames = index
  impair dès que la tête compte un nombre impair de trames (mesuré 1661 trames,
  impair, sur FF8) → **canaux inversés** : « une fois ça joue plus à gauche, une
  fois plus à droite » (vécu 21/11). Test : `le_blanc_de_tete_ne_decale_pas_les_canaux`,
  plus `blanc_numerique_de_tete_retire` (rognage + décalage de fenêtre). Les
  fenêtres mesurées en base sont oubliées (`user_version` 4, comme v2/v3)
  puisque la timeline a bougé de ces quelques dizaines de ms.
  L'**export d'un montage** (`convert::segment_ranges`) applique la même règle :
  blanc retiré quand l'export commence à 0 (une tranche entièrement dans le
  blanc disparaît), intact au milieu d'un montage. L'export d'une piste entière
  passe par zxtune et garde son lead-in (normal pour un rip, et le refaire
  imposerait un décodage + réencodage ffmpeg).
- **Une tête de piste plus longue qu'un artefact de moteur suit la règle de
  découpe** : au-delà des zéros exacts (une intro douce, « Banjo's House Blues »
  0,6 s), `cut_on` la garde en tuile « intro » si elle vaut ≥ 35 % d'une tranche
  et la jette sinon — c'est la demande du 21/11 (« ils doivent être tous du même
  temps »). Ne pas confondre avec le blanc numérique ci-dessus, qui n'est pas de
  la musique : lui n'a aucune raison d'exister dans la timeline.
- **Blanc de tête/queue d'un fichier audio courant : retiré du PCM**
  (`decode::trim_edge_silence`, demande user 21/11 : « certains sons ont du blanc
  au début et ça fausse tout »). Un `.wav`/`.mp3`/`.flac`/`.ogg` déposé sur la
  fenêtre peut commencer par une ou deux secondes de blanc : la grille s'y cale
  (temps 1 dans le silence) et toutes les coupes tombent à côté — mesuré sur
  `04_The Dungeons.wav` : 1,13 s de blanc en tête, ancre à 0,068 s, découpe
  décalée de ~1,6 s ; après trim, l'ancre tombe sur la musique et la première
  tranche s'ouvre sur une attaque (pic 0,09). Retiré **du PCM** (pas seulement
  à la lecture comme `head_silence`) pour que découpe, tempo, forme d'onde et
  export voient le même morceau ; seuil 1e-4 (le bruit de fond d'un mp3 n'est
  pas du blanc), cap 10 s de chaque côté, rien si le reste ferait moins d'une
  seconde. **Réservé à `util::is_common_audio`** : sur un chiptune, points de
  boucle et durée du `.m3u` sont absolus et ne survivraient pas au décalage.
  Test : `blanc_de_tete_et_queue_retire`. L'export d'une piste entière repasse
  par zxtune et garde donc son lead-in : c'est le montage (PCM) qui est trimé.
- **Pas** de fondu de sortie : une piste se termine net (demande utilisateur, 20/11). Le
  rendu de lecture n'a pas de fondu (retiré pour la boucle), donc la fin est sèche et
  c'est voulu — ne pas réintroduire de `fade` de sortie dans `LoopSource`. (Un
  raccord *entre deux tranches* est fondu, lui : voir la couture de suite.)
- Fondu croisé de couture en boucle : **2 ms**, rampe en cosinus (`LoopSource.xfade`).
  Plus long (10 ms) = on mélange la fin de phrase et l'attaque suivante → flam audible
  (« quelques ms de décalage », vécu 20/11) ; plus court = la marche de la couture
  s'entend comme un clic (vécu 20/11, « mini coupe » sans fondu).
- Boucle : OFF au démarrage (demande utilisateur, 19/11). Fenêtre par défaut = A/B en base,
  sinon boucle du format (VGM), sinon **longueur du `.m3u`** (zxtune rend 3 min fixes pour
  NSF/SPC/GBS), sinon fichier entier. `LoopSource::next` enroule **avant** le test de fin de
  buffer : en mode boucle, la source ne rend jamais `None`.
- Piste entière : un A/B « appliquer » est écrit dans `loops` et revient **à chaque
  lecture** de la piste ; sans le bouton « piste entière » (qui supprime la ligne
  `loops`), le morceau complet n'est plus accessible (demande user, 21/11).
- Tempo (`tempo.rs`) : enveloppe d'attaque = diff demi-redressée du log de l'énergie,
  trame 20 ms / saut 5 ms (une trame de 46 ms décalait la phase de ~25 ms) ; tempo par
  autocorrélation + peigne, puis phase par grille, puis **régression linéaire** des
  attaques recollées (un tempo de tracker est constant, la grille doit tenir 3 min) ;
  rejet si le pouls (flux moyen sur la grille / flux moyen) < 1,6 — sinon du bruit
  blanc sort « 120 BPM » (vécu 21/11). 60 s analysées, ~8 ms en release ; résultat
  dans l'entrée du cache mémoire (`Decoded.tempo`).
- Découpe (`tempo::cut`) : segments de N temps sur la grille (1t/2t/1m/2m/4m, 4/4
  supposé), tête avant le premier temps et queue incomplète conservées pour que la
  montage reconstruise la piste. La découpe est **générée à la lecture** (dès que le
  tempo est connu) et recalculée au changement de longueur ; le bouton « aimant »
  colle A/B au temps le plus proche.
- **Fenêtre A/B en base : toujours la valider.** Une ligne `loops` périmée
  masque la boucle du format : constaté le 21/11, rendu PSF de 269 s et ligne
  `(0, 362 s)` en base → la boucle zxtune (20 → 192,6 s) n'était **jamais**
  jouée, l'intro et le silence après la boucle revenaient à chaque tour (« la
  boucle ne se fait pas alors que zxtune y arrivait »). Rejet si la fin dépasse
  `decoded.total`, ou si la fenêtre englobe la boucle détectée ; une A/B
  *intérieure* à la boucle reste valable.
- **Boucle = zone de découpe.** `App::loop_win` (fenêtre réellement jouée)
  borne la découpe (`cut_whole` = fin de boucle) et **ancre la grille sur son
  début** (`App::grid`) : un PSF a une intro, les mesures de la boucle ne sont
  pas alignées sur le début du fichier. L'intro devient une seule tuile
  « intro », exclue par défaut du montage, de ▶ et de l'export
  (`base_seq`, case « sans l'intro »). `Tempo.phase` peut donc sortir de
  `[0, beat)` : `cut`/`cut_on` la prennent comme **origine absolue**, sans
  modulo (seul le cas négatif avance d'une tranche).
- **Durées égales → grille ajustée globalement** (`tempo::cut_on`) : une grille
  rigide posée sur un BPM estimé 0,1 % faux décale les dernières mesures de ~0,4 s
  sur 3 min (vécu 21/11 : « petit à petit ça se décale »). Le recollage
  frontière par frontière (±¼ de temps, séquentiel) corrigeait cette dérive mais
  donnait des tranches de durées différentes — mesuré sur FF8 « Force Your Way » :
  2,764 / 2,744 / … / 2,679 s pour une même mesure (demande user 21/11 : « les
  parts n'ont pas le même temps »). `fit_grid` suit donc les attaques **de proche
  en proche** (indispensable quand le BPM dérive : une recherche autour de la
  grille théorique sort de la fenêtre au bout de quelques mesures), puis ajuste
  **une** ancre et **une** période par moindres carrés (refus des points à résidu
  > 3 écarts médians), avec garde-fous : ±2 % sur la période, ancre ±60 ms (au-delà
  c'est l'appariement des premières attaques qui est douteux : SPC « Lockjaw's
  Saga » donnait +191 ms, soit une fausse intro de 0,93 s). `cut_on` découpe
  ensuite **strictement périodique** : mesuré après, 2,737 s pour *toutes* les
  tranches de FF8 (`cutjitter`, `RP_CUT_DEBUG=1` affichent la grille ajustée).
  Un ajustement **robuste** (Theil–Sen) et un garde-fou élargi à ±½ temps ont
  été essayés le 26/11 puis **annulés** : ils déplaçaient l'ancre d'un demi-temps
  sur plusieurs pistes et « tout était faux » à l'écoute (retour user). Le
  moindre-carré borné à ±60 ms est le comportement retenu ; l'écart résiduel de
  phase vient de l'enveloppe (pic de flux ~20 ms après l'attaque), pas d'un
  réglage à élargir.
  **Phase négative** (bouton « − mesure ») : `fit_grid` ramenait la phase sur
  la première ligne ≥ 0 (`phase -= floor(phase/len0) · len0`), comme `cut` ;
  sans ça la fenêtre de l'ancre était `clamp(0.0, phase + 60 ms)` avec une borne
  haute négative → panique Rust « min > max » et le programme **quittait** au
  clic sur « − mesure » (vécu 21/11). Test : `decalage_negatif_ne_plante_pas`.
  **Ancre ajustée négative** (la grille commence avant 0) : `cut_from` avance
  d'une tranche et garde la tête comme **première tranche** — sinon les
  premières secondes de la piste n'appartenaient à aucune tranche (« il manque
  un petit bout de musique au tout début », vécu 26/11).
  Tête : une vraie intro (ancre ≥ 35 % d'une tranche) reste une tranche à part ;
  une tête **courte est absorbée par la première tranche** (demande user 26/11) :
  la jeter faisait commencer le montage quelques dizaines de ms après le début
  de la piste (« il manque un mini bout au début de chaque musique »). Cette tête
  est de la musique (anacrouse), pas un artefact de moteur, et le démarrage de
  la grille est désormais recollé sur les attaques (`tempo::pulse`), donc plus
  sur une phase décalée qui gonflait la première tranche. Conséquence : la
  **première** tranche peut être un peu plus longue que les autres (de la tête),
  et ses voisines restent rigoureusement égales. Le 21/11, la tête courte était
  jetée pour que toutes les tranches durent pareil (GoldenEye « Bunker 1 » :
  4,084 s contre 4,004) — c'est ce compromis qui est abandonné, pas la règle de
  durée égale.
  Seule la **dernière** tranche peut différer : la fin de boucle n'est pas
  forcément un multiple de la tranche (FF8 boucle sur 107,996 s ≈ 39,4 tranches
  de 2 mesures → 1,23 s ; Lockjaw's Saga → 7,03 s). Mesure de contrôle :
  `cutcheck <fichier> 1 85 8` doit afficher la même durée pour la tranche 0 et
  la 1 (Bunker 1 : 4,004 s partout).
  L'enveloppe d'attaque du rendu entier vit dans `Decoded.envelope` (calculée une
  fois avec le tempo), pas dans `Tempo` (qui reste `Copy`).
- **La grille ajustée est la seule vérité de découpe** (vécu 26/11 : « les
  séquences se décalent de plus en plus, plus elles sont loin du départ du son »).
  `tempo::fitted(env, tempo, pas, fin)` expose l'ancre et la période que
  `cut_on` a réellement retenues, et `tempo::cut_from(fin, ancre, période)` la
  découpe strictement périodique extraite de `cut_on`. Tout ce qui **dessine ou
  découpe** une source passe par ces deux fonctions : `App::fit`
  (`refresh_fit` dans `recut`/`recut_clips`) alimente `cut_now`, les repères de
  la forme d'onde, les battements et l'aimant via `App::grid`, et
  `slices_for` (tuiles de l'arrangement) refait le même ajustement. Utiliser
  `Tempo::beat × pas` à la place fait dériver de 0,1 % — mesuré le 26/11 avant
  le recalage du pouls sur Banjo-Tooie « Cauldron Keep: Gun Chamber » :
  115,13 BPM détectés contre 2,0867 s réels (repli de l'enveloppe sur la
  période : contraste 1,81 contre 0,42), soit **2,2 ms par mesure et 280 ms en
  fin de piste**, alors que la découpe de l'éditeur, elle, était juste. Après le
  recalage (`tempo::pulse`), l'écart résiduel entre période ajustée et théorique
  reste ≤ 0,05 % (Cloud Race +0,04 %) — c'est justement ce que la découpe doit
  suivre. `grid_base` est l'entrée de
  l'ajustement : ne jamais y réinjecter le résultat (chaque passage cumulerait
  jusqu'à 60 ms de raffinement d'ancre). Grille **posée à la main** (`cut_anchor`
  ou `bpm_manual`) = rigide : `fit` est vidé et `cut_now` repasse par
  `tempo::cut`, un BPM réglé à 120,00 ne doit pas redevenir 120,31 en douce.
  Tests : `grille_ajustee_ne_derive_pas_en_fin_de_piste` (moteur) et
  `tuiles_de_larrangement_ne_derivent_pas` (GUI).
- **Choix du tempo en deux étages** (vécu 21/11, « la coupe n'est pas au bon
  endroit » sur les PSF d'Ape Escape) :
  1. le peigne d'harmoniques favorise les sous-multiples ; dans la bande ≥ 85 % du
     meilleur, on retient le candidat dont l'autocorrélation **à son propre temps**
     est la meilleure (≥ 90 % du max), à qualité comparable le plus rapide ;
  2. si ce temps n'est **pas nettement périodique** (`ac < 0,30` : rendu réverbéré),
     un balayage 60–200 BPM par pas de 1 mesure la **fraction de temps à ±30 ms
     d'une attaque franche** et garde le meilleur (à régularité comparable, le
     temps le plus long — un temps trop lent laisse les tranches alignées sur des
     multiples de mesure, un temps trop rapide les coupe au milieu). C'est la seule
     mesure qui regarde le morceau au lieu du modèle.
  Mesuré : Dark Ruins 113 → **170 BPM** (61 % des temps sur une attaque, frontières
  à 2 ms), Specter's Factory 70 → **176 BPM** (59 %, 4 ms), FF8 66 (moitié de 132,
  tranches de 2 mesures), SMB 200 (89 %), Kirby NES 100 %, Wario GB 91 %. Un seuil
  de rejet plus bas quand le balayage a parlé (`conf < 0,15` **et** régularité
  < 0,40) : sinon SMB retombait sur « aucun tempo ».
- **Ape Escape « Specter's Factory » découpe mal et ce n'est pas un bug de
  découpe** : l'enveloppe d'attaque n'a aucun pic net (autocorrélation ≤ 0,15,
  familles 66/88/110/132/176 BPM en 4:3), le tempo retenu est donc une hypothèse.
  `cut_on` ajuste une période unique : les longueurs restent égales, mais une
  attaque hors temps ne peut pas être suivie — c'est le prix de la régularité, et
  c'est voulu (une mesure doit durer une mesure). Le tempo reste une hypothèse sur
  ce morceau : ne pas « améliorer » la découpe là-dessus, c'est la détection du
  rythme qu'il faudrait revoir.
- **La boucle détectée se corrige à la souris dans la forme d'onde** (demande
  user 21/11 : « bouger la boucle qu'il détecte si il se trompe ») : les deux
  bords (« boucle » = début, « fin ») sont des poignées à 8 px, glissées au
  bouton primaire ; le glisser loin d'elles déplace toujours la vue. Glisser le
  **début** ré-ancre la grille (`grid()` prend `loop_win.start` comme phase) et
  la **fin** borne la découpe (`cut_whole`) : les tranches, la découpe et — si
  elle boucle — la lecture suivent image par image (`recut_clips`, sans toucher à
  l'arrangement). Au relâchement seulement : `recut()` complet, mémorisation
  (`db.set_loop_window`, donc « boucle A/B enregistrée » à la réouverture) et
  message. Le bord se cale **toujours sur le temps** de la grille détectée
  (`Tempo::snap`, demande user 21/11 : « je ne veux pas pouvoir la placer en
  décalé trop fin ») — pas de rayon d'aimant, pas d'échappement `alt` : un départ
  posé entre deux temps décale toutes les tranches suivantes. Si la correction
  manuelle ne convient plus, le bouton **« auto »** (à côté de la case « boucle
  auto », visible seulement quand la fenêtre courante diffère de la détectée)
  remet la boucle du fichier et **efface l'A/B en base** (`clear_loop_window`) —
  sans ça, la correction reviendrait à la réouverture. Cocher « boucle auto »
  efface aussi l'A/B : « auto » veut dire « la boucle détectée ». Invariants
  testés (`bord_de_boucle_borne`) : 0,2 s de marge entre les bords, rien hors
  piste. Un bord hors de la fenêtre zoomée n'a pas de poignée (sinon il volerait
  le glisser de vue au bord de l'écran).
- **Forme d'onde dans le mode Découpe** (`wave_strip`) : temps fins, mesures plus
  marquées, traits = frontières de coupe, bords de boucle « boucle » / « fin »,
  tête de lecture, temps sous le curseur. Raison : sur un PSF réverbéré, plusieurs
  tempos sont également « plausibles » (vécu 21/11, Dark Ruins 113 au lieu de 170)
  et un tempo faux ne s'entend qu'après coup — à l'œil, la grille à côté des
  attaques se voit tout de suite. Les min/max viennent de `Decoded.peaks`
  (`PEAK_BUCKETS` = 2400 paires ≈ 19 Ko quel que soit le morceau, calculés une
  fois au décodage) ; `App::peaks` est vidé à la sélection d'une nouvelle piste
  pour ne jamais afficher l'onde d'un morceau sous la grille d'un autre.
- **Grille automatique par défaut, rattrapage manuel minimal** : la batterie de
  boutons ÷2/×2/±0,5, la ligne « Grille » (1/16, ±temps, ±mesure, « recaler ») et
  le glisser de grille ont été supprimés (demande user 21/11 : « je veux plus
  tous ces boutons pour changer le BPM et peaufiner »). Il reste **deux** filets
  de sécurité, et ils ne doivent pas regrossir : le tempo (`bpm_manual`, réglé
  **sur le badge BPM** — glisser vertical = ±0,1 BPM/px (maj ×0,2, utile au pavé
  tactile), molette = ±0,5, alt+molette = ×2/÷2 — avec un bouton « auto » qui
  l'efface) et le départ en temps (`shift_beats`). Le glisser est le chemin
  principal : une souris sans roue ne doit pas être un cul-de-sac (demande user
  21/11). **Un seul widget porte les deux gestes** : ajouter un `ui.interact`
  par-dessus le badge lui prend le survol (egui ne donne `hovered()` qu'au
  widget du dessus) et rend la molette muette — régression constatée le 21/11,
  couverte par le test `un_seul_widget_par_geste`. La forme
  d'onde ne sert qu'à **vérifier** (molette = zoom, glisser = déplacer la vue ;
  la préhension des bords de boucle est **limitée à la poignée du haut**, 24 px,
  sinon tout glisser attrapait un bord). Les **traits de coupe** se saisissent
  aussi (±6 px, hors poignée) : les glisser déplace **toute la grille** (les
  durées restent égales) et la grille **reste où on la lâche**, au pixel.
  Même règle pour la **poignée de boucle** : le bord suit le pointeur au pixel
  (`loop_edge_time`) ; le calage sur le pas du mode (1/2 temps, 1/2/4 mesures)
  ne se fait que **avec Maj** — le calage permanent sautait de 2 mesures à
  chaque geste et rendait la correction inutilisable (retour user 22/11,
  il annulait explicitement la demande du 21/11). Mécanique : `cut_anchor` (origine manuelle, normalisée dans
  `[0, pas)`) + `cut_now` qui découpe **rigide** (`tempo::cut`, pas de
  `fit_grid` — sinon les traits bougeraient sous la souris) et **jette** la
  tuile d'intro partielle, sinon une grille déplacée fabriquerait une tranche de
  longueur différente. Le bouton **« par défaut »** (visible dès qu'un décalage
  ou une grille manuelle existe) remet `shift_beats = 0` et `cut_anchor = None`. Le zoom est **proportionnel au delta**
  de molette (~6 % par cran de 50 pt, `exp(-dy · 0,0013)`, borné à 0,75–1,33 par
  image) : les facteurs fixes 0,8 / 1,25 par événement étaient bien trop rapides,
  et avec un trackpad qui envoie trente petits deltas ils zoomaient trente fois
  (demande user 21/11 : « le zoom dézoom est beaucoup trop rapide avec la
  roulette »). `smooth_scroll_delta` est déjà lissé : proportionnel, le total
  reste juste quelle que soit la découpe en images.
  `bpm_manual` ne touche qu'à la période (`beat = 60/bpm`), **jamais** à la phase :
  sinon une correction de 0,5 BPM déplaçait le départ des mesures. Remis à `None`
  à chaque nouvelle piste et à chaque nouvelle grille.
- **Battement visible** (demande user 21/11 : « jolis battements des mesures/tempo
  et aussi sur le morceau joué ») : `beat_at(t, phase, beat, per_unit)` donne
  `(bloc, temps 0-based, fraction)`, `per_unit` = **pas du mode** (`clip_beats` :
  4 = 1 mesure, 8 = 2 mesures, 2 = 2 temps, 1, 16) et non la mesure fixe —
  demandé le 21/11 : « le beat visible ne doit pas être toujours à 1 mesure, si
  je mets 2 mesures ou 2 temps il doit s'adapter ». Trois affichages suivent ce
  pas : `beat_lights` (un point par temps du bloc, le courant allumé avec un
  halo qui s'éteint, écart/rayon resserrés au-delà de 4 points, libellé
  `pulse_label` : « mesure 5 » pour 4 temps, « mesures 3–4 » pour 8,
  « segment 7 » pour 1/2 temps, « intro » avant la grille), présent dans
  **l'en-tête du mode Découpe et dans la barre du lecteur** (donc visible dans
  toutes les vues) ; la **bande du bloc en cours** sous la forme d'onde
  (`bar_len = beat × clip_beats`, sinon une tuile de 2 mesures n'était
  surlignée qu'à moitié) ; et les **temps du segment sur la tuile qui sonne**
  (haut de la tuile, entre la touche et le « + », même pas que l'en-tête). Les mesures de la grille
  sont teintées de l'accent (1,5 px) avec un numéro quand la place le permet
  (toutes les 1, 2 ou 4 mesures selon le zoom) ; les temps restent fins. Hors
  lecture (pause, fin) les points restent éteints et affichent « mesure — » : pas
  de saut de mise en page. Repaint à 30 ms pendant la lecture (60 ms au repos),
  sinon le halo avance par sauts. Mesure 1 = première mesure après la phase ;
  avant la phase (intro) la mesure vaut 0, ce qui fait retomber la bande au bon
  endroit.
- **Vocabulaire** : ce que l'interface appelle **« montage »** (ex-« suite »,
  demande user 21/11) s'appelle `seq`/`arrange` dans le code — ne pas renommer
  les identifiants à chaque changement de mot. Le fichier exporté prend
  `<titre> (montage).<ext>`, et le nom par défaut d'un export sans titre est
  `montage`.
- **Chaîne de détection** (`decode`), dans cet ordre, le premier qui tient gagne :
  1. `beats::detect` — réseau **Beat This!** (ISMIR 2024) via le portage Rust
     `beat-this` (backend `rten`, Rust pur : ni Python ni ONNX Runtime système).
     Les modèles (~83 Mo, `mel_spectrogram.onnx` + `beat_this.onnx`) sont
     téléchargés une fois dans `~/.cache/gtracker/models/` (SHA-256 vérifiée,
     `.part` puis renommage atomique) ; sans réseau la fonction renvoie `None`.
     On ajuste une droite sur ses temps (les temps sautés sont absorbés par un
     arrondi entier), puis on **rejette** si le résidu médian > 45 ms, si la
     période ne tient pas sur les deux moitiés du morceau (`consistent`), ou si
     la piste fait moins de 16 temps.
  2. `tempo::detect` — enveloppe d'attaque + autocorrélation/balayage (historique,
     voir plus haut).
  3. `tempo::detect_weak` — la même chose **sans la porte de rejet** : mieux vaut
     une grille imparfaite que pas de découpe.
  4. `Tempo::fallback()` — 120 BPM, phase 0, `confidence = 0` : le mode Découpe
     reste utilisable partout, et l'interface affiche « grille régulière ».
  Puis `decode::analyse_tempo` passe la grille retenue par **`tempo::pulse`**
  (sauf quand le format déclare son point de boucle : `window.is_some()`, ce
  point est le bon ancrage par construction) : la phase est recherchée sur
  **±½ temps** avec une fenêtre étroite (±8 ms) qui maximise le flux des temps,
  et l'octave est doublée si la grille détectée tombe à plus de 0,15 temps des
  attaques **et** que le double couvre ≥ 0,90 des temps. Mesuré le 26/11 sur
  Banjo-Tooie « Cauldron Keep: Gun Chamber » : détection à 115,13 BPM phase
  0,0017, soit **0,27 temps (144 ms) avant** les attaques → chaque frontière
  tombait *après* l'attaque (« toutes les séquences ont un bout en trop à la
  fin ») ; après recalage, 230,27 BPM (4 temps = 1,043 s, couverture 0,96) et
  frontières ~10 ms avant l'attaque PCM. Les quatre autres pistes mesurées
  (Banjo's House Blues 84,97, Cloud Race 180,01, FF8 « Blue Fields » 160,00,
  « CAVERN » 73,05) ont une erreur de phase < 0,05 temps : elles ne bougent pas.
  **La force ne peut pas décider de l'octave** : les temps ajoutés par un
  doublement sont systématiquement moins forts (rapport 0,44 à 0,75), et les
  doubler à tort aurait cassé les deux pistes lentes. Le **doublement est la
  partie ambiguë** : le repli d'enveloppe de « Cauldron Keep » donne une période
  de 2,0866 s (= 4 temps à 115,13), donc doubler fait des tuiles d'une
  demi-mesure. L'essai de le retirer (26/11) a donné un « tout est faux » à
  l'écoute et a été **annulé** — ne pas retoucher l'octave dans la détection,
  c'est le réglage du badge (alt+molette ×2/÷2) qui tranche.
  Une phase ramenée près de 0 temps est préférée **juste avant 0** (`origin_phase`,
  ±¾ de temps) : à 0,98 temps la première tranche absorberait presque une mesure
  de plus.
  Mesures (21/11) : réseau exact sur synthétique 120 BPM (résidu 2 ms) et sur un
  chiptune 170 BPM réverbéré (7 ms, y compris le petit modèle) ; sur les pistes
  où il est cohérent il tombe d'accord avec l'enveloppe à 0,1 BPM près (7 pistes
  de DKC2, `RP_GRID_DEBUG=1` le montre) ; sur Dark Ruins il est **rejeté** (sa
  période ne tient pas d'une moitié à l'autre) et l'enveloppe garde 170 BPM.
- **Le point de boucle déclaré par le fichier n'est pas toujours musical** et
  fabrique alors une « intro » qui n'existe pas (vécu 21/11, Super Mario Galaxy :
  `SMG_galaxy12_strm.ast` déclare `loop start` = 852 986 échantillons à 32 kHz =
  26,656 s — champ `0x18` de l'en-tête, lu correctement par vgmstream ; 70 des
  74 `.ast` de cette archive ont un marqueur non nul ; sur les pistes mesurées il
  tombe pile sur la grille (72, 72, 8 et 4 temps). Décision user (21/11) : la
  boucle détectée n'est **pas** utilisée par défaut — une découpe se fait sur la
  piste entière, sans intro fabriquée. La case **« boucle auto »** (mode Découpe,
  à côté de « sans l'intro »), **décochée par défaut**, l'active pour la piste :
  `loop_win = detected_loop()`, `anchor = son début`, découpe bornée à sa fin.
  Le choix est mémorisé par piste (`fmt_loop_on` en base, table vidée à la
  réindexation ; l'ancienne `fmt_loop_off`, de sens inverse, est supprimée à
  l'init). Ne pas décider à la place de l'utilisateur : on ne sait pas trancher
  entre un vrai marqueur de jeu et un marqueur technique, seule l'écoute le dit —
  donc le défaut est « pas de boucle ».
- **Pas de boucle affichée sans boucle demandée** (`shows_loop` = `auto_loop ||
  loop_manual`) : `loop_win` porte aussi la **fin de découpe** quand elle vient
  de la longueur du `.m3u` (NSF rendu en 3 min pour une piste de 1:55) — ce
  n'est pas une boucle. Sans la garde, la forme d'onde dessinait les poignées
  « boucle »/« fin » et on pouvait les saisir sur une piste où rien n'avait été
  demandé (vécu 21/11 : « il affiche la boucle alors que je n'ai pas cliqué sur
  poser une boucle »). La découpe, elle, garde cette fin (`cut_whole`).
- **Boucle posée à la main, et retirable** (demande user 21/11 : « comment je
  fais pour en mettre manuellement ? et les enlever ensuite ? ») : le bouton
  **« poser une boucle »** (mode Découpe, même ligne ; il apparaît quand aucune
  boucle manuelle n'existe, y compris sur une piste sans boucle détectée) pose
  **4 mesures** à partir de la mesure la plus proche de la tête de lecture
  (`manual_loop`, pure et testée), puis `set_loop_window` en base : les bords se
  glissent ensuite dans la forme d'onde. **« retirer la boucle »** remet
  `loop_win = None` et `anchor = 0` (découpe sur la piste entière) et efface
  l'A/B en base. Le drapeau `loop_manual` distingue un choix de l'utilisateur
  (pose, bords glissés, A/B en base) de la **longueur du `.m3u`**, qui est aussi
  une `loop_win` mais n'a rien à voir avec une boucle : sans ce drapeau,
  « retirer la boucle » aurait supprimé une longueur de `.m3u` et fait découper
  les 3 min du rendu. Cocher « boucle auto » ou cliquer « auto » remet
  `loop_manual = false`.
- **Aucune piste n'est indécoupable** (vécu 21/11 : « Donkey Kong Country 2, y en
  a plein que je peux pas découper ») : la porte de rejet fermait le mode Découpe
  sur 41 des 109 pistes de DKC2. Après la chaîne ci-dessus : 0. Les 34 grilles de
  secours restantes sont des jingles de 3–5 s (Lose/Win/Player Down), où aucun
  tempo n'a de sens — c'est le comportement voulu.
- **L'analyse du rythme n'est pas dans le décodage** (demande user 21/11 : « la
  découpe prend du temps, on ne la fait pas quand on joue un son »). `decode`
  renvoie `Decoded.tempo = None` ; `decode::analyse_tempo` est appelée par le
  fil de travail depuis `Job::Tempo`, déclenché par `dj_panel` → `dj_waiting` au
  premier affichage du mode Découpe (une fois par piste ; `tempo_req` empêche
  les doublons, `tempo_failed` les boucles d'erreur). Le résultat est remis dans
  l'entrée mémoire (`decode::remember_tempo`), donc revenir sur l'onglet est
  instantané. `Msg::Tempo` applique la grille et appelle `recut()`. Pendant
  l'attente, `dj_waiting` affiche une **roue de chargement** (`egui::Spinner`,
  30 ms de repaint pour qu'elle tourne, demande user 21/11) — pas de spinner
  quand l'analyse a échoué : le message reste seul.
- **Le montage boucle** (`seq` dans le code, demande user 21/11) : `start_seq(seq, at, looped)` +
  `seq_next(len, from, looped, i)` (fonction libre : `player` emprunte déjà
  `self.player`). Depuis le transport (« écouter ») et depuis un clic sur une
  ligne du montage, `looped = true` : le dernier segment programme le premier,
  sans trou ; si la source s'arrête quand même (`exhausted`), on relance depuis
  `seq_from`. Un **enchaînement de clics sur les tuiles reste « une fois »**
  (`looped = false`) : c'est le comportement demandé pour les pads souris.
- **Deux modes de clic sur les tuiles** (`ClickMode`, sélecteur au-dessus des
  tuiles, demande user 21/11) : **dernier seul** (défaut, demande user 21/11 :
  « clic dernier seul par défaut ») ne garde que le dernier clic, **en file**
  empile chaque clic — tout ce qui est cliqué est joué dans l'ordre.
  `click_plan` tronque la file après le segment en cours et programme le
  nouveau à la couture, donc si A joue, B cliqué puis C cliqué, on entend
  **A puis C** (B sauté). Le segment en cours n'est jamais coupé, même en
  « dernier seul » : la bascule se fait à la couture, par la source
  (`queue_segment`), pas par un seek qui ferait un trou. Test :
  `clics_en_file_ou_dernier_seul`.
- **Tuiles en attente colorées** : `pad_grid` passe le rang (`pending`, 1 = joue
  juste après) à `pad_tile`, qui teinte la tuile avec `warn_fg_color` et affiche
  « +n ». Trois états distincts : accent plein = sonne, teinte d'alerte = en
  file, accent pâle = présent dans le montage.
- **Clic droit = arrêt** (demande user 21/11, après une première lecture erronée
  de sa remarque) : `App::stop_all` coupe la musique et les patterns en cours
  (montage de lecture `seq`, pads) — `arrange` est **conservé** (composition, pas
  lecture). Le transport « stop » appelle la même fonction. Le clic droit est lu
  en début de trame et appliqué en fin : une ligne du montage le **consomme**
  (`right_consumed`) pour retirer son occurrence sans arrêter la lecture. La
  barre de temps et la forme d'onde ne réagissent qu'au bouton **primaire**
  (`clicked_by`/`dragged_by`), sinon un clic droit y déplaçait la lecture.
- **Départ des segments en temps** (`shift_beats`) : deux mesures se retrouvent
  parfois appariées de travers, ou le départ tombe un temps trop tôt. Six boutons
  décalent l'origine de la grille (phase += `shift_beats × beat`) sans toucher au
  tempo, puis `recut()` : « − mesure » (−4 temps), « − 2 temps », « − 1 temps »,
  « + 1 temps », « + 2 temps », « + mesure » (+4) — demande user 21/11 : « ±1 et
  ±2 temps aussi ? ». Le décalage est **cumulé** (on peut enchaîner les clics) et
  en **temps**, pas en mesures : `shift_label` reste **court** (« − 6 temps »,
  « + 1 mesure », signe porté par le total, sans parenthèse) parce qu'une valeur
  longue poussait la rangée sur deux lignes (vécu 21/11). C'est la **seule**
  commande de grille restante : elle change l'origine, pas le BPM. Remis à 0 à
  chaque nouvelle piste/grille, et le retour en arrière est le bouton **« départ
  auto »** (un vrai bouton, pas un « 0 » : demande user 21/11).

  **Présentation** (vécu 21/11 : « départ audio pas très clair… ça fait deux
  lignes et c'est illisible à cause des marques bleues ») : trois rangées
  distinctes au lieu d'une seule qui débordait — « Segment » (5 tailles),
  « Départ des segments » (état + flèches) et « Boucle ». Au sein du départ,
  l'état est un **badge neutre** (`Frame` fond `noninteractive.bg_fill`, texte
  `strong`) et non du texte en couleur d'accent, qui se lisait comme un bouton
  sélectionné de plus ; son infobulle donne le **premier segment** en secondes
  (`grid().phase` normalisée dans `[0, pas)`) et dit si c'est « auto » (point de
  boucle/première attaque) ou « posé à la souris ». Les six flèches sont sur
  leur propre `horizontal_wrapped` : elles passent sur une deuxième ligne si la
  largeur manque (« fais deux lignes de bouton si besoin », demande user 21/11). Une phase négative est gérée par `fit_grid` (voir
  plus haut) — le plantage du bouton « − mesure » venait de là.
- Diagnostic hors UI : `cargo run --release -p gtracker-core --example
  gridcheck -- <fichier>…` affiche durée, fenêtre de boucle et tempo retenu ;
  `RP_GRID_DEBUG=1` dit quand c'est le réseau qui a gagné (BPM, nombre de temps,
  résidu). `loopcheck` reste l'outil de la boucle ; `cutcheck` montre les
  tranches ; `cutjitter` mesure l'écart des attaques à la grille et l'ajustement
  ancre/période (c'est lui qui a montré les 2,764 / 2,744 / 2,679 s de FF8) ;
  `silencecheck` mesure le blanc de tête du rendu (profil 2 ms/colonne et silence
  au début de chaque tranche) — c'est lui qui a établi les 36 ms de
  FF8 « Force Your Way ».
- Segments joués hors boucle : `Player::play_segment` pose un marqueur `stop`
  (échantillon de fin) dans `Shared` ; le segment suivant est programmé par
  `queue_segment` et la source y **saute** à la fin du précédent (`jumps` sert
  d'horloge à la GUI). Aucun trou d'une frame entre deux tranches (vécu 21/11 :
  « y a une coupure entre les deux »). Le montage (arrangement) peut répéter un
  segment ; `exhausted()` ne sert plus qu'à la fin du montage.
- **Couture de suite fondue sur 2 ms** (`LoopSource::seam`) : sur les derniers
  échantillons du segment, la source mélange sa fin avec le début du segment
  suivant **déjà programmé**, puis reprend après ce qui vient d'être entendu
  (`seam_skip`). Sans elle, le raccord est une marche — fin de tranche au milieu
  de la musique + attaque nette — entendue comme un « léger grésillement
  saccadé » à chaque enchaînement (vécu 26/11). Même durée et même rampe en
  cosinus que la boucle ; le fondu **remplace** ces échantillons au lieu de
  s'ajouter, donc la suite perd 2 ms par raccord (inaudible) et n'en rejoue
  aucun. Il ne s'applique que si un suivant est en file : le dernier segment
  s'arrête net. Test : `couture_de_segments_fondue_sans_clic`.
- Éditeur de source (espace « Éditeur » de la coquille, ex-mode Découpe) :
  en-tête (piste, BPM, longueur, grille), grille de tuiles à gauche, montage à droite,
  transport en bas. **Clic sur une tuile = jouer le segment une fois** ; un clic
  pendant la lecture **l'enchaîne à la couture** (`hit_tile` pousse dans `seq` et
  programme le suivant avec `queue_segment`, donc en tempo et sans trou), la chaîne
  s'arrête après le dernier clic. **Glisser une tuile vers le montage = composer** ;
  glisser une pastille du montage la déplace ; **clic droit = retirer cette
  occurrence**. Une pastille = **une** occurrence (pas de groupe « seg 4 ×3 ») :
  avec un groupe, toutes les copies du même segment s'allumaient ensemble pendant
  la lecture et un clic droit ne savait pas laquelle retirer (vécu 21/11). La
  surbrillance compare la **position** (`seq_pos`) et le segment, pas le seul
  numéro de segment. Suite vide = écouter/exporter toute la découpe. Le format
  MP3/FLAC est dans la barre du bas (même champ que l'export).
  Souris et clavier ne se mélangent pas : un clic coupe un pad clavier
  (`stop_keyboard`) et une touche coupe la chaîne de clics (le pad reprend la main) ;
  la tuile tenue à la souris ne boucle plus (demande user, 21/11 : « je veux que
  quand je clique sur un il se joue une fois et ça s'arrête »).
- **Aucun glyphe d'icône dans les boutons** (▶, ✂, ⤓…) : les polices d'egui ne les
  contiennent pas toutes, le bouton devient un carré vide (vécu 21/11 : « les images
  des boutons ne sont pas visibles »). Les icônes du mode Découpe sont **dessinées**
  (`paint_icon`/`djb`), le reste est en mots.
- Pads live : une touche = un segment, maintenir boucle ; maintenir 2+ touches fait
  alterner les segments **à chaque couture**, dans l'ordre d'appui (`next_pad`). La
  bascule est programmée dans `Shared` (`next_start`/`next_end`) et appliquée par
  `LoopSource` au tour suivant, donc à l'échantillon près — pas au prochain
  rafraîchissement de la GUI. Relâcher coupe la boucle : le tour en cours finit, puis
  la piste continue. Touches : 1–9, 0, puis les rangées de lettres sauf R (début) et
  L (boucle) ; aucune ne sonne quand un champ de saisie a le focus. Les pads
  fonctionnent **quand rien ne joue** (`Player::resume` relance la source si la file
  est vide) et la tuile tenue à la souris compte comme un pad tenu (`mouse_pad`, mêlé
  aux touches pour l'alternance).
- **La préférence de boucle ne se touche pas ailleurs.** `self.looping` est le
  choix de l'utilisateur (défaut ON : la musique de jeu boucle) ; les pads, le
  montage, « stop » et un glissement annulé ne doivent **pas** l'éteindre — seul le
  bouton « boucle » le fait. Vécu 21/11 : après une jam, la piste suivante
  (« Don't Be Afraid » de FF8, boucle 0 → 72,7 s détectée) jouait ses 160 s d'une
  traite, la boucle ayant été coupée par un relâchement de pad. Le bouton lit la
  préférence, la barre de temps lit l'état réel de la source (`player.looping()`),
  et le statut affiche la fenêtre retenue (« boucle détectée (2 tours) · 0:00 –
  1:12.7 ») pour que ce soit vérifiable à l'œil.
- **Deux qualités MP3, au choix** (demande user 21/11) : `Format::Mp3Vbr`
  (`-V 0`, ≈ 245 kb/s, défaut) et `Format::Mp3Cbr` (320 kb/s), à côté de `Flac`.
  L'export d'une piste passe par le backend de zxtune, qui veut ses options en
  **une seule map séparée par des virgules** (`--mp3=mode=cbr,bitrate=320,
  filename=…`) : un titre contenant une virgule couperait le nom (`a,b.mp3` →
  `a.mp3`, mesuré 21/11). On encode donc **toujours** dans un fichier temporaire
  au nom sûr (`.gtracker-<pid>-<n>.<ext>`), puis `rename` atomique dans le
  dossier d'export. La détection d'écrasement (`… (2).mp3`) se fait sur le nom
  final. Vérifié : VBR 247 kb/s, CBR 320 kb/s sur une piste SPC de 112 s.
- **`--flac` n'existe pas dans zxtune** (0.7.x : « unrecognised option »,
  constaté 21/11) : l'export FLAC rendait donc un fichier vide en silence. Il
  passe maintenant par un WAV temporaire (`--wav`) puis `ffmpeg -c:a flac
  -compression_level 8`, le WAV étant supprimé ensuite. L'export de montage
  utilise déjà ffmpeg sur le PCM en mémoire.
- Export d'un montage (`convert::export_segments`) : ffmpeg reçoit le PCM f32 LE sur
  stdin (tranches dans l'ordre, répétitions comprises), `-map_metadata -1`, puis tags
  lofty. ffmpeg est dans l'image (`files/scripts/rpm`) ; les bornes sont alignées sur
  des trames, sinon les canaux se décalent.
- **Deux fils de travail** (`spawn_workers`) : `goes_to_net` envoie le
  téléchargement (`Open` d'une archive absente) et `Update` sur le fil **réseau** ;
  lecture, découpe, export et suppression restent sur le fil **local**. Un seul
  fil faisait attendre l'écoute d'une archive déjà présente derrière un
  téléchargement de plusieurs minutes (demande user 21/11 : « pourquoi je ne peux
  pas écouter ce que j'ai déjà dl pendant ce temps »). Corollaires : la base est
  en WAL avec `busy_timeout = 5000` (deux écrivains) ; `Msg::Tracks`/`Msg::Playing`
  portent l'**identifiant de l'archive** et sont ignorés s'il ne correspond plus à
  l'archive ouverte (une réponse tardive ne doit pas changer la liste sous les
  yeux) ; ouvrir une archive appelle `clear_cut()` (tempo, découpe, boucle,
  arrangement, forme d'onde) — sans ça l'écran Découpe montrait la boucle et les
  tranches du morceau précédent (vécu 21/11 : un PSX sans boucle « héritait » de
  celle du titre d'avant).
- **Fichier audio déposé sur la fenêtre** (demande user 21/11 : « glisser
  n'importe quelle mp3/flac/wav/ogg et ça me la découpe ») : `raw.dropped_files`
  lu en tête de trame, extension reconnue par `util::is_playable_file` (audio
  courant **ou** format de jeu), premier fichier retenu. Le fichier devient une
  entrée **synthétique** : `site = LOCAL_SITE` (`"fichiers"`), `path` absolu,
  identifiant **négatif** dérivé du chemin seul (`util::local_entry_id`), donc
  jamais confondu avec un rowid et stable entre sessions (la boucle A/B et
  « boucle auto » mémorisées survivent). `archive::ensure` a une branche
  `LOCAL_SITE` qui rend **une** piste sans réseau ni extraction ; tout le reste
  (lecture, découpe, export, tempo) marche sans modification. `goes_to_net` doit
  renvoyer **false** pour ces entrées : sans ça le fichier local partait sur le
  fil réseau. L'ouverture (`open_file`) vide l'état de piste (`clear_cut`),
  bascule en mode Découpe et envoie `Job::Load` : le rendu est **chargé mais
  muet** (`Player::load(..., autoplay = false)` → sink en pause, tête à 0), la
  lecture ne part qu'au clic sur « écouter » (demande user 21/11 : « ne fais pas
  jouer automatiquement quand je glisse un son »). La grille arrive ensuite. Rien
  n'est écrit en base, le fichier n'apparaît pas au catalogue. Le décodage passe
  par zxtune, qui lit mp3/flac/wav/ogg/m4a (vérifié 21/11 : 112 s de FLAC rendu
  en 44,1 kHz stéréo).
- **Dossier d'export = dossier Musique XDG** (`util::music_dir`) : `xdg-user-dir
  MUSIC`, puis `XDG_MUSIC_DIR` de `user-dirs.dirs`, puis `$HOME/Musique`. Ne
  jamais coder un nom en dur : sur ce poste c'est `~/Musique`, **sans « s »**
  (`~/Musiques` existe aussi mais n'est pas le dossier déclaré), vécu 21/11.
  Sous-dossier `GTracker`.
- Liste « Téléchargés » : triée **A → Z sur le titre, toutes consoles mêlées**
  (demande user 21/11). Elle était triée par console puis par nom, ce qui
  éparpillait une même série ; la console ne départage plus que deux titres
  identiques (`cached_sort_key`, test `telecharges_tries_a_z_toutes_consoles`).
  Le catalogue, lui, se trie déjà par nom (`SortBy::Name`, défaut) en mélangeant
  les consoles — ne pas « re-grouper » par console.
- Filtre « Téléchargés » : ouvrir une archive déjà extraite **ne change pas
  d'espace de travail** (la playlist s'affiche dans l'inspecteur) ; revenir au
  catalogue à chaque clic faisait perdre la liste en cours (demande user, 21/11).
- **Clic droit sur une piste = ouvrir dans la Découpe** (demande user 21/11 :
  « au lieu de lire la chanson ») : `cut_track` bascule la vue en Découpe et
  charge la piste avec `Job::Load` (muet, prête à découper) ; le clic gauche
  continue de lire (`play_track`). Le clic droit de la ligne est **consommé**
  (`right_consumed`), sinon l'arrêt global du clic droit coupait tout.
- **Catalogue et « Téléchargés » ont chacun leur sélection et leur playlist.**
  `App::selected` (index du catalogue) et `App::cached_sel` (id) sont indépendants ;
  cliquer une archive téléchargée ne réécrit plus la ligne sélectionnée dans le
  catalogue (vécu 21/11 : les deux listes se mélangeaient). Le jeu réellement
  chargé vit dans `App::open_entry` : c'est lui qui fait foi pour jouer, exporter,
  sonder les durées et enregistrer une boucle A/B — jamais un index de liste. La
  La playlist appartient **à l'onglet qui l'a ouverte** (`list_belongs` :
  `open_from_cached == view est Téléchargés`, Découpe toujours, fichier déposé
  jamais) — ouvrir depuis « Téléchargés » puis revenir au catalogue n'affiche
  rien, et inversement (demande user 21/11 : « si je retourne dans catalogue
  elle ne devrait pas se voir »). `open_from_cached` était **inversé** dans
  `open_archive` (il valait `true` pour une ouverture depuis le catalogue) :
  c'est ce qui faisait apparaître la playlist de l'autre onglet ; une simple
  sélection dans l'autre onglet ne suffit pas non plus, seul Découpe suit
  toujours le jeu ouvert. Les métadonnées de l'archive ouverte (compositeur,
  album) ne s'affichent dans Détails que si l'entrée sélectionnée **est** celle
  ouverte, sinon le panneau montrait celles de l'autre onglet. Une archive
  téléchargée absente du catalogue s'ouvre par son identifiant (`entry_by_id`,
  repli sur la base).
- Tri initial : `refilter()` doit être appelé à la construction de l'app. La liste
  part de `all_entries()` (`ORDER BY site, folder, file_name`) ; sans ce premier
  tri, l'UI affiche les sites alphabétiques d'abord (NDS avant NSF…) au lieu de
  tout le catalogue A→Z (vécu 21/11, demande user).
- egui 0.37 : `weak_text_alpha` vaut 0,6 par défaut, illisible sur les libellés 11,5 px en
  thème sombre → passé à 0,78 dans `style()` (constaté 19/11, à revérifier à la prochaine
  version).

## État

Fait : coquille unique (bibliothèque, arrangement + éditeur, inspecteur, transport),
base + recherche (par nom/console/année/tags), tri (nom, console, date), filtre par
console, mise à jour joshw.info, téléchargement/extraction avec cache listé et supprimable
(filtre « Téléchargés »), pistes `.m3u`, lecture (pause/seek/position), boucle format + A/B
en base, détection de la boucle PSF dans le rendu, cache audio en RAM, export MP3/FLAC
taggé, repli `vgmstream-cli` (.txtp/.fsb/.bfstm/.btsnd/.at3…), icône, tempo + découpe
rythmique (temps/mesures), pads live (boucle tenue, alternance), montages joués et exportés.

À faire : export « intro + N boucles + fondu », mètre 3/4 (le découpage suppose 4/4),
lecture des `.psf2`/`.ss2` non couverts, repli quand `zxtune123` est absent (message clair
au lieu d'une piste muette).
