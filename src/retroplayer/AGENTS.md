# RetroPlayer — contrat du sous-projet

Navigateur et lecteur de bandes-son de jeux rétro (joshw.info), GUI egui.
Le code vit **ici** (`gablue/src/retroplayer/`) ; le dépôt standalone et le TUI Go
d'origine ont été abandonnés (20/11), il n'en reste rien.

Montage « B » : `zxtune123` décode en WAV, `rodio` joue, la boucle « comme dans le
jeu » est tenue par notre source audio (pas par zxtune).

## Installation dans l'image

- `Containerfile-gablue`, étape `retroplayer-build` (toolchain jetée) :
  `COPY src/retroplayer/{Cargo.toml,Cargo.lock,crates,assets}` puis
  `files/scripts/build-retroplayer` (`alsa-lib-devel` + `pkg-config` pour `cpal`)
  → `/out/retroplayer`, récupéré par
  `COPY --from=retroplayer-build /out/retroplayer /usr/bin/retroplayer`.
  **`assets/` est obligatoire** : l'icône est embarquée par `include_bytes!`.
- `.desktop` : `files/system/all/usr/share/applications/retroplayer.desktop`
  (`StartupWMClass=retroplayer`) ; icône vectorielle :
  `files/system/all/usr/share/icons/hicolor/scalable/apps/retroplayer.svg`.
  L'app_id Wayland est fixé dans `main.rs` (`ViewportBuilder::with_app_id`) : sans
  lui KWin ne rattache pas la fenêtre au `.desktop` et affiche l'icône générique.
- Icône : `assets/make-icon.py` (PIL, supersampling 4×) écrit `assets/icon.rgba`
  (128×128, embarquée), `assets/icon-512.png` et `assets/icon.svg`, **plus la copie
  installée** dans `files/system/.../scalable/apps/` — une seule source, jamais
  deux dessins à garder synchro. Relancer le script après toute retouche.
- Runtime : `zxtune` (sonde de durée) **et** `zxtune123` (décodage) sont installés
  par `files/scripts/post-install` ; ne pas ré-exclure `zxtune123` du tarball comme
  le faisait l'ancienne version Go.

## Layout

- `crates/core` — bibliothèque : base SQLite (`db`), scraping joshw.info (`joshw`),
  téléchargement/extraction 7z + `.m3u` (`archive`, `m3u`), décodage (`decode`),
  lecteur (`player`), export MP3/FLAC + tags (`convert`).
- `crates/gui` — binaire `retroplayer`, egui/eframe.
- `bin/zxtune123` n'est pas versionné : cherché dans `$ZXTUNE`,
  `~/.cache/retroplayer/bin`, `bin/`, puis `PATH`.

## Build / test

```
./build.sh                      # dev local : génère .build/alsa/alsa.pc (en-têtes de brew) puis cargo build --release
cargo test -p retroplayer-core  # loopdetect, m3u, liens joshw (16 tests)
cargo run -p retroplayer-core --example loopcheck -- /chemin/piste.psf   # fenêtre + tours
cargo run -p retroplayer-core --example probe -- /chemin/piste.spc        # durée sans rendu
cargo run -p retroplayer-core --example inspect -- /chemin/fichier.vgz
./run.sh
```

Dans l'image, le build ne passe pas par `build.sh` : `build-retroplayer` installe
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
- Cache : `~/.cache/retroplayer/{retroplayer.db,archives/<id>,decoded,bin}` — compatible
  avec la base de l'ancienne version (mêmes tables `files`).
- `archives/<id>/.complete` contient `CACHE_VERSION` : l'incrémenter quand l'analyse des
  pistes change, sinon les caches existants gardent un index périmé **sans retélécharger**
  (vécu 19/11 : titres « Title Theme,0:01:55.087 » figés dans la base).
- Un dossier `archives/<id>` sans `.complete` est une extraction interrompue : `ensure`
  le supprime avant de recommencer, sinon `scan` indexe un contenu partiel.
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
- `tracks.loop` / `tracks.loop_start` : `NULL` = jamais mesuré, `0` = mesuré sans
  boucle distincte, `> 0` = fenêtre. C'est ce qui évite de relancer la sonde à chaque
  ouverture. `PRAGMA user_version` invalide les mesures d'une version précédente
  (v2 : fenêtres PSF fausses → `UPDATE tracks SET loop = NULL, loop_start = NULL`).
- Boucle activable à tout moment, y compris après la fin du morceau : rodio a
  consommé la source, donc `Player` garde les PCM et `queue_from()` réinjecte une
  `LoopSource` (même chose pour seek/lecture après la fin). Le mode est global
  (piste suivante incluse).
- En boucle, l'UI affiche la **durée du cycle** (la fenêtre) ou, à défaut, la durée
  de la piste : zxtune rend 3 min fixes pour un NSF de 1:55, et la barre suit le cycle
  pour que le curseur warp au point de reprise. Le libellé rappelle la durée totale de
  la piste quand le rendu contient plusieurs tours.
- Cache audio (`~/.cache/retroplayer/decoded`) : un WAV par piste jouée, plafonné à
  1,5 Go (`RETROPLAYER_CACHE_MB`), index `decoded/index` (`clé<TAB>dernière lecture`) et
  éviction des pistes les moins récemment jouées (WAV + `.loop`). Sans plafond :
  1,7 Go pour 47 pistes (vécu 20/11). Le WAV avec fondu d'un PSF est supprimé dès que
  la variante `-nofade` existe (inatteignable).
- Formats que zxtune ne lit pas (`.txtp`, `.vgmstream`, `.ss2`…) : repli sur
  `vgmstream-cli` (`vgmstream.rs`) — métadonnées `-I` (durée, points de boucle exacts),
  décodage `-l 1 -f 0` (un tour, sans fondu). Binaire : `VGMSTREAM`, cache `bin/`, PATH,
  brew.
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
  de fois (`total / P > 8`) est un motif, pas la boucle, et est écarté. Cache de
  l'analyse : `decoded/<clé>.loop` versionné (`LOOP_CACHE_VERSION`, à incrémenter dès
  que le détecteur change).
- Fondu de sortie en lecture simple : le rendu n'a plus de fondu (retiré pour la
  boucle), donc `Player` applique un fondu linéaire de `clamp(total*0,1 ; 1 s ; 3 s)`
  sur les derniers échantillons quand la boucle est OFF (`LoopSource.fade`).
- Fondu croisé de couture en boucle : **2 ms**, rampe en cosinus (`LoopSource.xfade`).
  Plus long (10 ms) = on mélange la fin de phrase et l'attaque suivante → flam audible
  (« quelques ms de décalage », vécu 20/11) ; plus court = la marche de la couture
  s'entend comme un clic (vécu 20/11, « mini coupe » sans fondu).
- Boucle : OFF au démarrage (demande utilisateur, 19/11). Fenêtre par défaut = A/B en base,
  sinon boucle du format (VGM), sinon **longueur du `.m3u`** (zxtune rend 3 min fixes pour
  NSF/SPC/GBS), sinon fichier entier. `LoopSource::next` enroule **avant** le test de fin de
  buffer : en mode boucle, la source ne rend jamais `None`.
- egui 0.37 : `weak_text_alpha` vaut 0,6 par défaut, illisible sur les libellés 11,5 px en
  thème sombre → passé à 0,78 dans `style()` (constaté 19/11, à revérifier à la prochaine
  version).

## État

Fait : base + recherche (par nom/console/année/tags), tri (nom, console, date), filtre par
console, mise à jour joshw.info, téléchargement/extraction avec cache listé et supprimable
(onglet « Téléchargés »), pistes `.m3u`, lecture (pause/seek/position), boucle format + A/B
en base, détection de la boucle PSF dans le rendu, fondu de sortie en lecture simple, export
MP3/FLAC taggé, repli `vgmstream-cli` (.txtp/.vgmstream), icône.

À faire : export « intro + N boucles + fondu », lecture des `.psf2`/`.ss2` non couverts,
repli quand `zxtune123` est absent (message clair au lieu d'une piste muette).
