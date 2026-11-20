# RetroPlayer (Rust + egui)

Navigateur de bandes-son rétro (joshw.info) avec lecture réelle et boucle « comme
dans le jeu ».

Vit dans l'image Gablue : sources ici (`gablue/src/retroplayer/`), binaire compilé
par l'étape `retroplayer-build` du `Containerfile-gablue` et installé en
`/usr/bin/retroplayer` avec son `.desktop` et son icône.

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
- export MP3 (backend LAME de zxtune) ou FLAC, tagué (titre / compositeur / album).

## Dépendances

- `zxtune123` (17 Mo, pas de Qt) : `~/.cache/retroplayer/bin/zxtune123` ou `$ZXTUNE`.
- `vgmstream-cli` (fourni par l'image) pour les conteneurs que zxtune ne lit pas :
  `.txtp`, `.fsb`, `.bfstm`, `.btsnd`, `.hca`, `.at3`… (`$VGMSTREAM` pour en imposer un).
- Pour compiler : `cargo` + les en-têtes ALSA (fournis par `brew install alsa-lib`,
  utilisés seulement à la compilation ; la libALSA du système est liée).

## Utilisation

```
./build.sh
./run.sh
```

- double-clic sur une archive → téléchargement, extraction, liste des pistes ;
- les durées absentes des `.m3u` (PSF, XA, sets sans playlist) sont mesurées en
  tâche de fond par une sonde zxtune (~30 ms/piste) puis stockées en base ;
- clic sur une piste → décodage (0,1–0,5 s) puis lecture ;
- export : dossier cible + MP3/FLAC + « Exporter la piste ».

## Cache

`~/.cache/retroplayer/`

- `retroplayer.db` — base (partagée avec l'ancienne version) + tables `tracks` et `loops` ;
- `archives/<id>/x` — archives extraites (conservées : rejouer ne re-télécharge pas) ;
- rien pour l'audio : les pistes décodées restent en mémoire (LRU, 512 Mo), un WAV
  temporaire sert de tampon dans `/dev/shm` puis disparaît ;
- `bin/zxtune123` — décodeur.

## Notes techniques

- `zxtune123` ne peut pas être piloté de l'extérieur (pas d'IPC, touches seulement
  sur son propre TTY) : c'est pour ça que la lecture est faite par rodio.
- les échantillons sont identiques au direct (md5 PCM WAV == FLAC, mesuré) ; le MP3,
  lui, est avec perte et réservé à l'export.
- boucle : `fichier?#N` + fenêtre `total − loop` de l'en-tête VGM, au sample près.
  Pour les formats sans métadonnée (NSF/SPC/GBS), repli sur la longueur du `.m3u`,
  puis fichier entier ; le mode boucle enroule toujours, la lecture ne s'arrête pas.
- zxtune annonce `Time` (durée rendue) et `Loop duration` sur sa sortie standard ;
  `stdbuf -oL` est nécessaire pour lire l'en-tête sans attendre la fin du rendu.
