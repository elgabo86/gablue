# GTracker (GUI) — contrat

Onglet **GTracker** de GTracker : plusieurs sources, découpe en tranches,
timeline multitrack, tempo maître, tonalité, export. Le moteur est décrit dans
`../../../core/AGENTS.md` ; ici, uniquement l'interface et son intégration.

## Nom

- L'application et le mode s'appellent **GTracker** : binaire `gtracker`, app_id
  Wayland `gtracker`, paquets `gtracker`/`gtracker-core`, projet `.gtrack`.
- Le renommage est fait **dans la copie de travail** ; le dépôt git de gablue
  n'est pas touché pendant le chantier (voir `../../../../AGENTS.md`, section
  « Nom et périmètre »). Les renommages de packaging (`.desktop`, icône,
  Containerfile) sont listés là et se font **ensemble** au merge.

## Place dans l'application — coquille unique

- **Plus de vues plein écran ni d'onglets** : l'application est une seule
  coquille (`crates/gui/src/shell/`) avec
  - une **bibliothèque latérale** : Catalogue et Téléchargés sont deux **filtres**
    (recherche, consoles, tri, mise à jour) ;
  - un **espace de travail central** : `Workspace::Arrange` = arrangement
    multitrack **avec** l'éditeur compact toujours visible en bas ;
    `Workspace::Edit` = l'éditeur de source complet (Découpe : poignées de
    boucle, badge BPM, pads) ;
  - un **inspecteur** : playlist de l'archive ouverte + détails de la sélection ;
  - **un seul transport à l'écran** : celui du projet en Arrangement, celui de
    la pré-écoute en Éditeur.
- **Un document = un projet** : une source ajoutée explicitement (double-clic
  dans la playlist, « ajouter au projet », dépôt de fichier) devient une source
  du projet et crée une piste si elle n'en a pas ; un rechargement de projet ou
  une simple écoute n'en crée pas. L'éditeur complet montre la même source, et
  les deux espaces partagent le même projet, la même sélection et le même undo.
- **Un seul propriétaire du son** : basculer d'espace coupe le flux de l'autre
  (projet ↔ pré-écoute) ; aucun mixage accidentel de deux transports.
- Le GTracker reçoit ses sources par : double-clic sur une piste de la playlist,
  bouton **« ajouter au projet »**, dépôt de fichier sur la fenêtre, ou source
  déjà présente dans le projet.
- **Écouter avant de poser** : un **clic** sur une tuile de l'éditeur de source
  joue la tranche une fois (`Cmd::Preview` → `preview` → `Player::play_preview`),
  sans la poser et sans rien changer au projet ; un **clic** sur un clip de la
  timeline sélectionne et joue **depuis son début**. Le glisser reste le geste
  de composition (tuile → piste).
- **La bibliothèque reprend le son après un passage dans l'arrangement** : le
  Player ne sert qu'un flux à la fois et `play_source` oublie le PCM de la piste
  (vécu 21/11 : « je peux plus jouer » au retour dans l'éditeur). L'app garde
  `LibAudio` (même `Arc`, position incluse) et le réinstalle via
  `LibAudio::restore` quand la piste manque ; `play_preview` ne l'oublie jamais.
- Deux musiques côte à côte = deux sources, deux pistes, clips au même temps :
  le plan a deux voix et le rendu est leur somme exacte (test
  `deux_musiques_jouent_ensemble`).

## Panneaux

```
┌ barre haute : GTracker · Arrangement|Éditeur · projet (nom, nouveau, ouvrir,
│               enregistrer, exporter) · annuler/refaire · source · panneaux ┐
├ bibliothèque      │ pistes │ timeline (règle, lanes, clips, tête)          │ inspecteur
│ (Catalogue /      │        │                                              │ (playlist +
│  Téléchargés)     │        │                                              │  détails)
├───────────────────┴────────┴──────────────────────────────────────────────┴───────────┤
│ transport du projet (Arrangement) ou de la pré-écoute (Éditeur)                        │
├ barre d'état : position, sélection, message ──────────────────────────────────────────┤
└ éditeur compact (Arrangement) : source, forme d'onde, grille, tuiles ─────────────────┘
```

- Colonne de pistes : nom, source, badge `maître`/`suit`, BPM effectif, tonalité,
  repitch/stretch, gain, mute/solo, couleur. Un clic sur le badge `maître`
  **switche** la piste maîtresse (et repose le BPM du transport, voir moteur).
- Timeline : règle cliquable (seek), grille 1/16 avec mesures accentuées, tête de
  lecture, clips. Molette = défilement horizontal, ctrl+molette = zoom centré sur
  le curseur, comme la Découpe.
- Éditeur de source : source courante, BPM/tonalité (badges), forme d'onde avec
  frontières de coupe, tuiles de tranches déplaçables.
- **Menu projet** : nom (le nom EST le fichier), nouveau, enregistrer (ctrl+S),
  ouvrir un projet du dossier `projets/` (les sources sont rechargées et
  ré-analysées par des jobs), export MP3/FLAC.
- **Barre d'état** : position musicale (mesure/temps), description de la
  sélection, message. Le point coloré signale un projet modifié non enregistré.

## Gestes et sémantique (à respecter à la lettre)

- **Glisser une tuile** de l'éditeur vers une lane = créer un clip à la position
  déposée, aimanté à la grille. La tuile porte sa longueur musicale :
  `length_beats = durée_tranche / beat_maître`. Le clip reçoit la **tonalité de
  la tranche** (`key::analyse` sur sa plage), pas celle de la source : c'est
  elle que « même gamme » transpose.
- **Corps d'un clip** : déplacer (`at_beat`), longueur inchangée. Aimant 1/16 par
  défaut, `alt` = libre.
- **Bord droit** : rogner le contenu (`src_end`), la longueur musicale suit
  (`length_beats = (src_end − src_start) / trames_par_beat`). On coupe de l'audio,
  on ne l'étire pas par accident.
- **Bord gauche** : rogner `src_start` **et** déplacer `at_beat` d'autant, pour que
  le contenu restant ne glisse pas dans le temps.
- **Étirer à la grille** = action explicite (menu contextuel : « ajuster à la
  mesure »), pas un geste de bord.
- **Clic droit sur un clip** : dupliquer, couper à la tête de lecture, supprimer,
  repitch/stretch, transposer ±1/±12, accorder à la maîtresse, boucler pour
  remplir.
- **Chevauchement autorisé** sur une même piste : mixé, sans fondu croisé
  automatique. Interdire le chevauchement obligerait à écraser des clips au
  glisser.
- **Toute mutation passe par `edit(label, …)`** : instantané avant, applique,
  pousse dans l'historique. Un glisser = **un** instantané (pris au début,
  validé au relâchement), jamais un par image.
- **Un clip rogné à zéro disparaît** : pas de clip de longueur nulle invisible.

## Formes d'onde (rendu réel, pas des blocs)

- Le clip dessine un min/max **par colonne de pixel** (pas un rectangle coloré),
  calculé depuis la pyramide `peaks::Wave` sur la plage exacte du clip.
- Texture par clip à largeur fixe (1024 px), réutilisée et étirée à l'affichage :
  régénérée seulement si (source, `src_start`, `src_end`, teinte) change. Une
  régénération par image pendant un zoom rendrait l'application inutilisable.
- Zoom profond : quand une colonne couvre moins de quelques trames, dessiner le
  PCM (trait fin) au lieu d'une enveloppe aplatie.
- Cache borné (LRU, ~64 textures) : un projet de cent clips ne doit pas manger la
  mémoire GPU.
- Culling : un clip hors de la zone visible n'est ni texturé ni dessiné.

## Visuel

- Thème sombre, une couleur d'accent **par piste** (palette fixe lisible), clip =
  rectangle arrondi avec la forme d'onde, nom + longueur en temps, fades en
  triangles, contour 2 px en sélection, fantôme pendant le glisser, ligne
  d'aimantation quand le clip se colle à la grille.
- Le clip **qui sonne** est surligné (une seule source de vérité : la position de
  lecture), pas « tous les clips de cette tranche ».
- **Aucun glyphe d'icône** (polices egui incomplètes, le bouton devient un carré
  vide) : icônes dessinées, le reste en mots. Règle déjà en vigueur dans la
  Découpe.
- Repaint 30 ms pendant la lecture, 60 ms au repos.

## Raccourcis

| Touche | Action |
|---|---|
| `espace` | lecture / pause |
| `début` | retour à 0 |
| `suppr` | supprimer le clip sélectionné |
| `ctrl+D` | dupliquer |
| `ctrl+Z` / `ctrl+Y` | annuler / refaire |
| `ctrl+S` | enregistrer le projet |
| `S` | couper le clip sélectionné à la tête de lecture |
| `M` | mute de la piste sélectionnée |
| `1`–`9` | sélection de piste |
| flèches | déplacer le clip sélectionné d'un pas de grille (maj = une mesure) ; haut/bas = piste voisine |
| `ctrl+S` | enregistrer |
| `M` | mute de la piste sélectionnée |

Aucune touche ne doit agir quand un champ de saisie a le focus.

## Intégration avec `main.rs`

- Jobs ajoutés : `GtrackerLoad { track }`, `GtrackerTempo { track }` (rythme
  **et** tonalité en une passe), `GtrackerExport { project, audio, format, name }`.
  Messages correspondants : `GtrackerLoaded(track, decoded)`,
  `GtrackerTempo(path, tempo, key)`, `GtrackerExported(path)`. La réponse est
  rapprochée par **chemin de fichier** (`Source.file`), pas par index : une
  réponse tardive ne doit jamais s'appliquer à une autre source (même leçon que
  `Msg::Tracks`).
- Décodage et analyses sur le fil **local** existant ; le fil réseau ne voit
  jamais un projet.
- Le GTracker ne touche pas à la base du catalogue : un projet n'écrit rien en
  SQLite, rien sous `~/.cache`.
- `App` garde son état de bibliothèque ; le GTracker vit dans son propre module avec
  son `Project`. Ouvrir une piste de bibliothèque ne remplace pas la composition.

## Dossiers

- Projets : `util::music_dir()/GTracker/projets/`.
- Export du projet : `util::music_dir()/GTracker/` (même règle que l'export
  existant, `xdg-user-dir MUSIC` — jamais un nom codé en dur).

## Pièges (vécu du projet, à ne pas réintroduire)

- Un widget egui qui se superpose à un autre prend le survol et rend la molette
  muette (vécu 21/11) : un seul widget par geste.
- Un libellé long pousse une rangée sur deux lignes et casse la mise en page :
  badges courts, infobulles pour le détail.
- La capture d'écran ne prouve rien sur l'audio : la vérification passe par les
  tests du moteur et `mixcheck`, jamais par « ça a l'air bon ».
- **Les tuiles et les repères suivent la grille ajustée, pas le BPM détecté**
  (vécu 26/11 : « les séquences se décalent de plus en plus, plus elles sont loin
  du départ du son »). `slices_for` et `App::grid` passent par
  `tempo::fitted` / `App::fit` (rempli par `refresh_fit` dans
  `recut`/`recut_clips`) : `Tempo::beat × pas` dérive de 0,1 % par tranche, soit
  280 ms en fin de piste sur un USF N64, alors que la découpe de l'éditeur était
  juste. Ne jamais recalculer une période à la main dans l'interface.
- **Une tête de piste courte appartient à la première tuile** (vécu 26/11 :
  « il manque un petit bout de musique au tout début »). `slices_for` découpe
  par `tempo::cut_from` comme l'éditeur : tête courte absorbée par la tuile 1,
  ancre ajustée **négative** (grille qui commence avant 0) → la tête est une
  première tuile plus courte qui commence à 0, vraie intro en tuile à part.
  Démarrer la pile à l'ancre faisait disparaître les premières dizaines de ms
  de la piste — invisibles dans l'éditeur, absentes du montage et de l'export.
- **Grille posée à la main = rigide** : `cut_anchor` (origine glissée) ou
  `bpm_manual` vide `App::fit` et `cut_now` repasse par `tempo::cut` ; sinon un
  BPM réglé à 120,00 redevenait 120,31 au recalcul suivant.
