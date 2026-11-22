# GTracker (Rust + egui)

Navigateur de bandes-son rétro (joshw.info) avec lecture réelle et boucle « comme
dans le jeu ».

Vit dans l'image Gablue : sources ici (`gablue/src/gtracker/`), binaire compilé
par l'étape `gtracker-build` du `Containerfile-gablue` et installé en
`/usr/bin/gtracker` avec son `.desktop` et son icône.

## Ce que fait cette version

- recherche dans 38 000+ archives (base SQLite existante réutilisée) ;
- téléchargement + extraction 7z (pur Rust) et lecture des `.m3u` de joshw.info,
  donc les **vrais titres de pistes** (au lieu du nom de l'archive) ;
- lecture par `rodio` sur des échantillons décodés par `zxtune123` (bit pour bit
  identiques au direct, FLAC/WAV sans perte) ;
- boucle sur la fenêtre du format (VGM, en-tête `.genh`), la période **détectée dans
  le rendu** pour les PSF (le `length` du tag n'est pas la période) ou une fenêtre A/B
  enregistrée en base ; les PSF sont rendus sans leur fondu final (copie `fade=0`),
  donc la boucle est complète et sans fondu à chaque tour ;
- repli sur `vgmstream-cli` pour les formats que zxtune ne lit pas (`.txtp`,
  `.vgmstream`, `.ss2`…), avec les points de boucle exacts du fichier ;
- export MP3 (backend LAME de zxtune) ou FLAC, tagué (titre / compositeur / album) ;
- **arrangement GTracker** (espace de travail) : timeline multitrack — plusieurs sources,
  tranches déposées sur des pistes, clips à forme d'onde réelle, tempo maître
  **switchable** (une seule piste donne le tempo, les autres se recalent),
  repitch par défaut / stretch WSOLA en option, « même gamme » (transposition
  en bloc vers la tonalité de la maîtresse, analysée **par tranche**), gain
  général, métronome, boucle de projet, undo/redo, raccourcis clavier,
  sauvegarde `.gtrack` et export MP3/FLAC ;
- **éditeur de source** (espace « Éditeur ») : tempo détecté **automatiquement** dans le
  rendu, découpe en temps/mesures recalée sur les attaques réelles (un BPM estimé
  au plus juste ne décale plus les dernières mesures). Trois détecteurs en
  cascade : un **réseau de neurones** (Beat This!, ISMIR 2024 — modèle téléchargé
  une fois dans le cache), puis l'enveloppe d'attaque, puis une grille régulière
  de secours : **toute piste est découpable**, aucune n'est refusée faute de
  rythme. Aucun réglage de grille à la main ; pads live **à la souris comme au
  clavier** (tenir boucle le segment, deux pads alternent), composition de la
  montage en glissant les tuiles, répétitions, **le montage boucle** quand on
  l'écoute (un enchaînement de clics sur les tuiles, lui, joue une fois), les
  tuiles en attente s'affichent en teinte d'alerte avec leur rang, et un
  « départ ± mesure » permet de changer le groupement des mesures sans toucher au
  tempo, et la **molette sur le badge BPM** corrige le tempo à la main (±0,5 BPM,
  maj pour 0,1, alt pour ×2/÷2) avec un bouton « auto » pour revenir au tempo
  détecté ; export MP3/FLAC (ffmpeg, sans trou entre les tranches) ; une **forme
  d'onde** montre la grille (temps, mesures) et
  les frontières de coupe — molette pour zoomer, glisser pour déplacer la vue —
  et sert à vérifier d'un coup d'œil que les traits tombent sur les attaques ;
  quand la piste a une intro avant sa boucle (PSF), la découpe est ancrée sur le
  point de boucle et l'intro devient une tuile à part, exclue par défaut.

## Interface

Une seule coquille, plus d'onglets plein écran : bibliothèque latérale
(Catalogue / Téléchargés = filtres), espace de travail central (Arrangement =
pistes, clips et éditeur compact en bas ; Éditeur = Découpe complète), inspecteur
(playlist + détails), un seul transport à l'écran, barre haute pour le document
(nom, nouveau, enregistrer, ouvrir, exporter) et l'historique. Un document = un
projet : double-clic sur une piste de la playlist l'ajoute comme piste prête à
arranger ; clic droit l'ouvre dans l'éditeur ; un clic l'écoute seulement.

## Dépendances

- `zxtune123` (17 Mo, pas de Qt) : `~/.cache/gtracker/bin/zxtune123` ou `$ZXTUNE`.
- `vgmstream-cli` (fourni par l'image) pour les conteneurs que zxtune ne lit pas :
  `.txtp`, `.fsb`, `.bfstm`, `.btsnd`, `.hca`, `.at3`, `.ast` (flux Wii, Super Mario
  Galaxy)… (`$VGMSTREAM` pour en imposer un) ; **toute extension que vgmstream sait
  ouvrir est listée comme piste** — une archive Wii/Switch n'apparaît plus vide.
- `ffmpeg` (fourni par l'image) pour encoder un montage de segments en un fichier.
- Pour compiler : `cargo` + les en-têtes ALSA (fournis par `brew install alsa-lib`,
  utilisés seulement à la compilation ; la libALSA du système est liée).

## Utilisation

```
./build.sh
./run.sh
```

- double-clic sur une archive → téléchargement, extraction, liste des pistes ;
- filtre « Téléchargés » de la bibliothèque (archives déjà extraites) : chaque
  filtre garde sa propre sélection, ouvrir un jeu téléchargé ne change ni la
  ligne sélectionnée dans le catalogue ni la playlist affichée de l'autre ;
- les durées absentes des `.m3u` (PSF, XA, sets sans playlist) sont mesurées en
  tâche de fond par une sonde zxtune (~30 ms/piste) puis stockées en base ;
- dans l'éditeur de source, la forme d'onde sert de vérification (zoom molette,
  glisser = déplacer la vue) **et** de correction : glisser la poignée « boucle »
  ou « fin » déplace le point de boucle détecté (aimant sur la grille, `alt`
  pour libérer), avec recoupe et mémorisation par piste ;
- **clic droit n'importe où → arrêt** (musique + patterns en cours ; le montage
  composée reste intacte) ;
- clic sur une piste → décodage (0,1–0,5 s) puis lecture — le rythme n'est
  **pas** analysé à ce moment-là : la détection (réseau) tourne à la première
  ouverture de l'éditeur de source, en tâche de fond, et reste en mémoire pour la suite ;
- export : dossier cible + MP3/FLAC + « Exporter la piste » ;
- dans le GTracker, dépose un fichier sur la fenêtre (ou « ajouter la piste
  ouverte ») : espace = lecture/pause, suppr = retirer un clip, ctrl+z/y =
  annuler/refaire, ctrl+d = dupliquer, s = couper à la tête, ctrl+molette =
  zoom, flèches = déplacer le clip d'un pas (maj = une mesure), 1–9 = piste,
  M = mute ; clic droit sur un clip = transposer, accorder à la maîtresse,
  fades, boucler, ajuster à la mesure. Projets :
  `~/Musique/GTracker/projets/<nom>.gtrack` (menu « projet »).

## Cache

`~/.cache/gtracker/`

- `gtracker.db` — base (partagée avec l'ancienne version) + tables `tracks` et `loops` ;
- `archives/<id>/x` — archives extraites (conservées : rejouer ne re-télécharge pas) ;
- rien pour l'audio : les pistes décodées restent en mémoire (LRU, 1024 Mo ;
  les sources du projet ouvert sont **épinglées**, jamais évincées), un WAV
  temporaire sert de tampon dans `/dev/shm` puis disparaît ;
- `bin/zxtune123` — décodeur.

## Notes techniques

- `zxtune123` ne peut pas être piloté de l'extérieur (pas d'IPC, touches seulement
  sur son propre TTY) : c'est pour ça que la lecture est faite par rodio.
- les échantillons sont identiques au direct (md5 PCM WAV == FLAC, mesuré) ; le MP3,
  lui, est avec perte et réservé à l'export.
- boucle : `fichier?#N` + fenêtre `total − loop` de l'en-tête VGM, au sample près.
  Pour les formats sans métadonnée (NSF/SPC/GBS), la longueur du `.m3u` est
  **recalée sur la répétition réelle du rendu** (les `.m3u` arrondissent à la
  seconde : Kirby NES annonce 38 s pour 38,318 s, et chaque tour perdait un temps),
  puis fichier entier ; le mode boucle enroule toujours, la lecture ne s'arrête pas.
- `.m3u` indexés à partir de 0 (GBS) : zxtune compte à partir de 1, sinon chaque
  piste joue le morceau précédent et le titre ne correspond plus au temps.
- zxtune annonce `Time` (durée rendue) et `Loop duration` sur sa sortie standard ;
  `stdbuf -oL` est nécessaire pour lire l'en-tête sans attendre la fin du rendu.
