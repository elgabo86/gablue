# gablue-rec — enregistreur / replay maison (remplace gpu-screen-recorder)

**But** : capture de l'écran par KMS/DRM + encodage matériel VAAPI, dans **un
seul processus** qui sert à la fois l'enregistrement et le buffer de replay, avec
une **socket d'état** pour l'overlay. Il remplace le flatpak
`com.dec05eba.gpu_screen_recorder` et ses conséquences : `pgrep`/`killall` pour
deviner l'état, `SIGINT` sur le mauvais PID, interdiction d'écrire dans `/tmp`,
deux binaires exclusifs à arbitrer à la main.

**Statut** (20/11/2026) : **v1 complète, non commitée**. Cœur validé au banc —
enregistrement à froid et à chaud, replay (anneau + sauvegarde), H.264/H.265/10
bits, audio du monitor, 120/144/240 fps, socket complète, sortie propre, fichiers
décodés de bout en bout (vidéo **et** audio) sans une erreur. Branché : étape
d'image `gablue-rec-build` (`files/scripts/build-rec`), `setcap` posé par
`post-install`, service utilisateur `gpad-rec.service` activé globalement,
réglages dans l'overlay (`CAPTURE`, `REPLAY AUTO`) et dans Gablue Manager, icône
rouge d'enregistrement dans le tray, commande `grec` + `.desktop`. Reste : le
curseur affiché (v2), le portail fenêtre (v2), les variantes NVIDIA.

## Pourquoi pas ffmpeg + `kmsgrab`

Mesuré le 20/11/2026 sur ce poste (ffmpeg 9.0.2 negativo17, Navi 44) :

- le scanout de KWin est en **`AR30`** (`DRM_FORMAT_ARGB2101010`, 10 bits, même
  HDR désactivé) ; `kmsgrab` **refuse** ce fourcc (`Framebuffer pixel format
  30335241 is not a known supported format`), et son option `-format` n'est
  qu'une **assertion**, pas un contournement (lu dans `libavdevice/kmsgrab.c`) ;
- `libavutil/hwcontext_vaapi.c` a la même lacune : `vaapi_drm_format_map` ne
  contient que `XRGB2101010`, pas `ARGB2101010`.

**Contournement retenu, bit-exact** : un scanout est **opaque**, `ARGB2101010` et
`XRGB2101010` ont le même layout mémoire (les 2 bits hauts sont alpha `0b11` dans
l'un, padding dans l'autre). On déclare donc `XR30` au descripteur dmabuf ; la
table VAAPI l'accepte et le GPU reçoit une surface `RGB32_10`. Vérifié par A/B :
PSNR direct **26,1 dB** contre **16,1 dB** avec rouge/bleu permutés → couleurs
justes.

Conséquence : la capture est **la nôtre** (`drmModeGetFB2` + `drmPrimeHandleToFD`
→ `AVDRMFrameDescriptor`), pas celle de ffmpeg. Le reste (conversion, encodage,
muxage) reste sur **libav**, donc aucun nouveau paquet runtime : `libdrm`,
`libav*`, `libva` sont déjà dans l'image.

## Privilèges

`drmModeGetFB2` exige `CAP_SYS_ADMIN` sur un FB qu'on n'a pas créé. Motif
**Sunshine** (`src/platform/linux/kmsgrab.cpp`), repris tel quel :

- file cap `cap_sys_admin+p` **sur ce binaire seulement** (jamais sur `ffmpeg`,
  jamais root, jamais de démon privilégié) ;
- le drapeau est monté en **EFFECTIVE** juste autour de l'appel, puis retiré ;
  les capacités étant **par thread**, l'implémentation définitive isole les
  appels dans un thread dédié — un reste du processus ne peut pas hériter du
  privilège ;
- `drmPrimeHandleToFD`, l'import VAAPI et l'encodage ne demandent **aucune**
  capacité.

`/usr/bin/gpad-rec` avec une file cap : à vérifier côté SELinux en image
(`user_home_t` en banc passe sans alerte, `bin_t` non testé).

## Architecture

```
poll plane.fb_id (500 µs)                       ← pas de minuteur : la vérité est le flip
  └─ fb_id changé ?  → GetFB2 (cap) → dmabuf → AVFrame(DRM_PRIME, XR30)
       └─ hwmap(derive vaapi) → scale_vaapi (RGB10→NV12/P010 + resize, GPU)
            └─ hevc_vaapi / h264_vaapi
                 ├─ enregistrement : mp4 (+ audio AAC du monitor du sink)
                 └─ replay : anneau de segments MPEG-TS de ~1 s, sauvegarde = remux `-c copy`
```

- **Un seul pipeline**, deux sorties. Enregistrement et replay ne sont **plus
  exclusifs** : l'anneau tourne et le fichier s'ouvre/ferme à côté.
- **Les fps sont le taux réel des flips** (VFR). Le réglage `fps` n'est qu'un
  **plafond** d'émission (on jette les flips au-delà) : jamais de doublon
  fabriqué, jamais de beat contre le 165 Hz de l'écran.
- Mesuré (1440p natif, HEVC) : **120,1 / 144,1 / 240,1 fps** tenus, CPU 3-5 % ;
  le plafond n'est ni le GPU ni le CPU.
- **Le curseur n'est pas dans le scanout** (plane séparée) : masqué par défaut,
  exactement comme le `-cursor no` actuel. « Afficher » = lire la plane curseur
  et la mélanger sur GPU (`overlay_vaapi`), ce n'est **pas** fait en v1.
- Audio : **monitor du sink** seulement (pas d'audio par application), capturé
  par **`parec`** (un processus par session, tube lu avec `poll`) et non par le
  décodeur `pulse` d'FFmpeg — cf. pièges. Un flux de **lecture muet** maintient
  le sink réveillé pendant la session : sans lui le monitor ne délivre plus rien
  dès que rien ne joue et la piste s'arrête au milieu du clip.

## Défauts : pensés pour que le fichier s'ouvre chez les autres

**Deux défauts, deux raisons distinctes.** Le codec par défaut est **H.264**,
pas HEVC — choix de compatibilité, pas de poids : un PC Windows sans l'extension
« HEVC Video Extensions » affiche un écran noir avec du son, et le lecteur
embarqué de Discord (Chromium) ne prévisualise pas le HEVC de façon fiable.
Expliquer les codecs aux amis n'est pas une fonctionnalité. Le HEVC reste
**disponible** (≈28 % de moins), jamais par défaut.

La résolution par défaut est **celle de l'écran** (`source`), pas 1080p : le
downscale est **bilinéaire** (Mesa n'expose pas de lanczos) et le texte fin du
HUD y perd ; garder le natif ne coûte que du disque. `720p`, `1080p` et `1440p`
restent des crans explicites pour qui veut un fichier léger, ou un envoi qui
passe sous les 10 Mo de Discord gratuit.

Conséquence assumée : à 1440p natif, 30 s pèsent **~55 Mo** (6,4 Go/h en q3 à
60 fps) au lieu de ~31 Mo en 1080p. Le poids est le seul prix — la
compatibilité ne dépend que du codec.

**Un seul geste pour sauver un replay** — Home+haut quand le buffer tourne
(sinon il le démarre), ou la ligne `SAUVER REPLAY` du panneau. Pas de bouton « Partager », pas de
destination à choisir : la sauvegarde écrit un clip prêt à envoyer. Le mp4 est
écrit avec `+faststart` (moov en tête) pour que Discord/WhatsApp l'affichent
sans attendre la fin du téléchargement.

**Silencieux quand personne n'a appuyé sur un bouton.** Le replay automatique
(ouverture d'un `.lgp`/`.wgp`), `grec` et le `.desktop` ne font **aucun bruit**
(`GPAD_SANS_BIP=1`, posé par `grec` et par le lancement automatique) : le bip ne
confirme qu'un geste manette. Et **aucune notification à la sauvegarde** — elle
apparaîtrait dans le clip suivant. La confirmation est le rumble, et le fichier
dans le dossier.

**Replay automatique** (`auto_replay=1`, désactivé par défaut) : le démon lit
`$XDG_RUNTIME_DIR/gablue/running.txt` (écrit par glauncher, gwine et lgplaunch)
et n'agit que sur les **transitions** — `paquet=` finissant en `.lgp`/`.wgp`
démarre le replay, sa disparition l'arrête. Un `replay.stop` manuel coupe
l'automatisme pour la session en cours (`auto_lance`) : un replay arrêté pendant
la partie ne redémarre pas tout seul.

Les débits, eux, restent ce qu'ils sont — c'est juste une information :

| Destination | Cible | 30 s pèsent |
|---|---|---|
| Discord sans Nitro | ≤ 10 Mo → 2,5 Mbit/s (ou couper à 15 s) | ~9 Mo |
| Discord Nitro | 8-12 Mbit/s | 30-45 Mo |
| WhatsApp | 720p, ~4 Mbit/s (re-encodé côté serveur) | ~15 Mo |
| Telegram (document) | aucune limite pratique | — |
| YouTube (lien privé) | 12-16 Mbit/s, 1440p accepté (meilleure source pour son VP9/AV1) | ~50 Mo |

- Coût mesuré d'une conversion vers le partage sur cette machine (VAAPI,
  1440p HEVC → 1080p H.264, `+faststart`) : **0,73 s pour 5,9 s de clip**, soit
  ~8× le temps réel — 30 s se convertissent en ~4 s. C'est le prix d'un bouton
  « Partager » qui ne touche pas à l'archive.

## Pièges à ne pas réintroduire

- **`fb_id` identique = pas de nouvelle image** : en mode plafonné on n'émet
  rien (le lecteur tient l'image précédente) ; ne jamais ré-encoder la même
  frame pour « remplir » un débit fixe.
- **pts réels** (`av_gettime` au flip), jamais un index de frame : un horodatage
  inventé casse la synchronisation audio et le replay.
- **mp4 non finalisé = fichier inutilisable** : tout arrêt passe par un chemin
  propre (vidange encodeur + `av_write_trailer`). Le `SIGINT` sur un `flatpak
  run` ne finalisait rien — c'est le bug qu'on supprime.
- **Le buffer de replay vit en RAM** (`/dev/shm` ou memfd), jamais dans `/tmp`
  sur disque ni dans `$HOME` : 30 s à 25 Mbit/s ≈ 95 Mo, et rien ne doit toucher
  le disque tant que le joueur ne demande pas la sauvegarde.
- **Un seul écrivain par fichier** : sauvegarde en `.part` + `rename` atomique,
  sinon une sauvegarde interrompue laisse un mp4 tronqué à la place du nom final.
- **`AR30` → `XR30`** : ne pas « corriger » ce mapping, c'est ce qui rend la
  capture possible ; le vérifier si KWin change de format (le log dit le fourcc).
- Le format du scanout peut changer en cours de route (HDR activé, changement de
  mode) : sur `Plane framebuffer format changed`, **arrêter proprement** plutôt
  que d'encoder du garbage.

### Pièges payés au banc (20/11/2026, à ne pas réintroduire)

- **L'extradata HEVC/AVC n'existe pas à l'ouverture de l'encodeur VAAPI** : les
  VPS/SPS/PPS partent *in-band* avec le premier IDR. Un mp4 ouvert en cours de
  route écrit donc un `stsd` sans `hvcC` → `No start code is found`, fichier
  illisible. L'extradata est reconstruite depuis le premier IDR
  (`memoire_extradata`) puis attachée au `codecpar` avant l'entête.
- **`av_interleaved_write_frame` prend possession du paquet** : écrire le même
  `AVPacket` dans deux muxeurs donne `size 0` au second (TS corrompus,
  `startcode missing`). Il faut `av_packet_ref` par sortie (`ecrit_ref`).
- **Ouvrir une sortie ne suffit pas** : l'encodeur a quelques frames de retard,
  donc des paquets antérieurs s'y glissent et référencent des images absentes
  (`Could not find ref with POC N`). On force un IDR (`forced_idr=1` +
  `pict_type=I`) **et** on jette tout jusqu'au premier keyframe
  (`attendre_idr`).
- **La première frame doit être encodée avant d'ouvrir les sorties** : c'est
  elle qui produit l'extradata et l'IDR. Séquence : `send_frame` → vider dans
  `premiers[]` → ouvrir mp4/anneau → écrire `premiers[]`.
- **CSV du muxeur `segment`** : `nom,début,fin` (durées **cumulées**), chemins
  **relatifs** au motif. Un parseur `nom,durée` lit du vent (liste vide →
  sauvegarde impossible).
- **`ffmpeg -f null` n'est pas un validateur** : sur du VFR il invente des
  avertissements « non monotonically increasing dts ». Valider un fichier avec
  `ffmpeg -v error -i f -map 0:v -f rawvideo -y /dev/null` (et idem `-map 0:a`
  en `s16le`).

### Pièges audio (payés le 20/11/2026)

- **Un encodeur AAC ne se réutilise pas d'une session à l'autre.** Après
  `avcodec_send_frame(NULL)` (drain) puis `avcodec_flush_buffers`, l'état n'est
  pas garanti propre (ffmpeg-devel, « avcodec/nvenc: fix flushing for
  encoder-reuse ») : mesuré ici, la 2e session sortait des paquets à **pts
  invalide** puis `EOF` définitif → tous les clips après le premier étaient
  **muets**, avec 248 « écriture: échec » dans le journal. Correctif : un
  encodeur **neuf par session** (`audio_encodeur_neuf`), libéré au repos ; les
  paramètres du flux (extradata compris) sont copiés **une fois** dans
  `e->audio_par` pour les muxeurs. Coût : quelques ms par clip.
- **Le décodeur `pulse` d'FFmpeg bloque à vie** une fois le sink endormi :
  `libavdevice/pulse_audio_dec.c` attend dans `pa_threaded_mainloop_wait` sans
  aucun crochet d'interruption (lu dans la source). C'est pour ça que la capture
  passe par `parec` dans un tube lu avec `poll(100 ms)` : la boucle garde la main
  sur l'état de la session et peut la fermer à tout moment.
- **Le monitor ne délivre rien quand rien ne joue.** Un flux de **lecture muet**
  (`pa_simple`, 100 ms de zéros en boucle) garde le sink réveillé ; il est ouvert
  **par le thread de surveillance**, jamais par le thread audio — ce dernier est
  justement celui qui peut être en attente, le faire fermer serait un
  interblocage. Fermé après ~3 s de repos.
- **L'horodatage audio est par session** : l'origine du clip est `t0`, on jette
  le son capturé avant (marge 100 ms) et `audio_off` aligne le premier
  échantillon sur l'image. « maintenant − t0 » au moment de la lecture est un
  piège : le thread peut découvrir la session en retard, la piste commençait
  alors à la fin du clip.

## Configuration

`~/.config/gablue/recorder.conf`, `clé=valeur`, absente = défaut. Écrite par
`gpad` (réglages intégrés) ou `gablue-manager`.

**Le démon surveille le `mtime` du fichier** (poll 500 ms) : aucun écrivain n'a
besoin de connaître la socket ni d'envoyer `reload` — écrire le fichier suffit.
Le rechargement est **différé** tant qu'un mp4 est ouvert (changer de codec
finaliserait l'enregistrement en cours) ; `status` expose `pending` pour que
l'overlay le dise. `reload` et SIGHUP forcent la même relecture.

Les variables d'environnement (`RECORD_DIR`, `RECORD_RES`, `RECORD_FPS`,
`RECORD_AUDIO`, `REPLAY_SEC`) restent prioritaires pour le banc.

| Clé | Défaut | Effet |
|---|---|---|
| `codec` | `h264` | `h264` (High, compatible partout) ou `h265` (HEVC Main) |
| `qualite` | `3` | 1-5, voir table ci-dessous |
| `fps` | `60` | plafond d'émission : `60`, `120`, `144`, ou `flips` (aucun plafond, VFR réel) |
| `replay_sec` | `30` | profondeur du buffer |
| `curseur` | `masque` | `masque` ou `affiche` (affiche = v2, cf. plus haut) |
| `dix_bits` | `0` | `1` = HEVC Main10 (P010), inutile en H.264 |
| `resolution` | `source` | `source` (natif, barème sur la taille réelle), ou une taille en `LxH` : `1280x720`, `1920x1080`, `2560x1440` |
| `audio` | `default_output` | `default_output` ou `none` |
| `auto_replay` | `0` | `1` = replay automatique dès qu'un `.lgp`/`.wgp` tourne |
| `dossier` | `~/Vidéos/Captures` | sortie des fichiers |

Niveaux de qualité — base **1080p60 en H.264** (le cas par défaut), puis
`× pixels/1080p × fps/60`, le HEVC retirant 28 % :

| Niveau | Nom | VBR | maxrate | 30 s en 1080p60 H.264 |
|---|---|---|---|---|
| 1 | Économique | 3 Mbit/s | 5 | ~9 Mo (passe en Discord gratuit) |
| 2 | Normal | 6 Mbit/s | 10 | ~18 Mo |
| 3 | Haute (défaut) | 10 Mbit/s | 16 | **29 Mo mesurés** (8,1 Mbit/s) |
| 4 | Très haute | 16 Mbit/s | 26 | ~48 Mo |
| 5 | Maximale | CQP 20 (qualité constante, aucun plafond) | — | variable |

Mesuré en H.264 **1080p**, niveau 3, 30 s de motif très détaillé en mouvement :
**29,1 Mo, 8,1 Mbit/s, 53 img/s réelles → 3,4 Go/h** (au même niveau en `source`
1440p : ~1,78× le débit, ~55 Mo / 6,4 Go/h). Le même clip
en `fps=flips` (89,8 img/s réelles, écran 165 Hz) : **47,9 Mo → 5,6 Go/h** —
c'est le prix du mode sans plafond.


**En VFR, le débit suit la cadence DÉCLARÉE à l'encodeur.** Le contrôle de débit
calcule un budget *par image* : si on déclare 60 fps en émettant 120-165 flips,
le débit réel double (mesuré : 12,4 Mbit/s pour une cible à 10). D'où le défaut
`fps=60`, et en mode `flips` la cadence déclarée est le rafraîchissement réel de
l'écran, lu dans le mode DRM.


**La taille du fichier suit le débit, pas la résolution.** Réduire `resolution`
ne réduit le volume *que* parce que le barème est multiplié par
`pixels/1440p` (et `fps/60`) — un VBR à débit constant produirait le même
fichier en 1080p qu'en 1440p, avec juste moins de détail. Le downscale lui-même
est gratuit (VPP côté GPU), mais il est **bilinéaire** (Mesa n'expose pas de
lanczos) : le texte fin du HUD y perd.

Mesuré le 20/11/2026 sur du contenu en mouvement (fenêtre 1600x900 rejouée en
boucle, 3 s par prise) :

| Réglage | Taille mesurée | Débit | Ordre de grandeur (1 h) |
|---|---|---|---|
| q3 VBR, 1440p | 12,24 Mo | 34,3 Mbit/s (rafale, se stabilise vers 26) | ≈ 11,7 Go |
| q5 CQP 20, 1440p | 8,95 Mo | 25,1 Mbit/s | ≈ 11 Go (sans plafond) |
| q3 VBR, 1080p | 7,79 Mo | 21,9 Mbit/s | ≈ 6,6 Go |
| q5 CQP 20, 1080p | 5,15 Mo | 14,5 Mbit/s | ≈ 6,5 Go |

Le CQP (niveau 5) sort parfois **plus petit** que le VBR à qualité égale, mais il
n'a **aucun plafond** : sur du grain, des particules ou une scène très détaillée
il peut monter à plusieurs centaines de Mbit/s. C'est pour ça qu'il n'est pas le
défaut, malgré sa qualité.

## Protocole de la socket

`$XDG_RUNTIME_DIR/gablue/gpad-rec.sock`, une ligne JSON par message, réponse
synchrone. Remplace **tout** `pgrep`/`killall`/fichier d'état.

```jsonc
// client → serveur
{"cmd":"status"}            {"cmd":"record.start"}   {"cmd":"record.stop"}
{"cmd":"replay.start"}      {"cmd":"replay.save"}    {"cmd":"replay.stop"}
{"cmd":"reload"}            {"cmd":"quit"}

// serveur → client
{"ok":true,"recording":false,"replay":true,"replay_sec":30,"ring":18,
 "file":"…/Video_2026-11-20_22-31-04.mp4","t":12.3,"pending":false,
 "frames":40,"err":null}
```

Règles : `record.start`/`replay.start` sont **idempotents** (déjà actif = `ok`
avec l'état courant) ; `record.stop` est **synchrone** (répond après la
finalisation du mp4) ; `replay.save` répond avec le chemin **final** (après
rename) pour que l'overlay n'ait plus à lire un journal ; `pending` dit qu'un
`recorder.conf` modifié attend la fermeture du fichier courant ; `quit` arrête
proprement (finalise l'enregistrement, libère DRM et VAAPI).

`grec` est le client de cette socket pour le clavier et le `.desktop` :

- **`grec` seul = au premier plan** : il démarre (ou rejoint l'enregistrement en
  cours), affiche `REC 00:12 — Ctrl-C pour arrêter` quand la sortie est un
  terminal, et **Ctrl-C arrête en finalisant le mp4** (le `stop` de la socket ne
  rend la main qu'après le moov ; un second Ctrl-C ne coupe pas la
  finalisation). Code de sortie 130.
- `grec toggle` = démarrage/arrêt sans rester attaché : c'est ce que lance
  l'entrée de menu (`Exec=grec toggle`, sans terminal).
- `start`, `stop`, `status`, `save`, `replay start|stop|status`, `dir`.

Il est **silencieux** (`GPAD_SANS_BIP=1`) et suit `GPAD_REC_SOCK`/`GPAD_REC`
pour le banc.

## Build et banc

Local (Kinoite immuable = pas de `-devel` sur l'hôte) : conteneur `fedora:45`
avec le dépôt **fedora-multimedia** pour `ffmpeg-devel` 9.0.2, c'est-à-dire
**exactement** les sonames de l'image (`libavcodec.so.63`, `libavfilter.so.12`) :

```bash
podman run -d --name gablue-rec-build -v ~/.cache/opencode/gablue-rec-bench:/w:z \
    -w /w registry.fedoraproject.org/fedora:45 sleep infinity
podman exec gablue-rec-build dnf5 -y install gcc make pkgconf-pkg-config \
    libdrm-devel libcap-devel
podman exec gablue-rec-build dnf5 -y config-manager addrepo \
    --from-repofile=https://negativo17.org/repos/fedora-multimedia.repo
podman exec gablue-rec-build dnf5 -y install --enable-repo='*fedora-multimedia*' \
    --allowerasing ffmpeg-devel
# puis : make  (dans une copie montée dans /w)
```

En image : étape dédiée `gablue-rec-build` (`files/scripts/build-rec`),
`COPY --from` de `gpad-rec` et `grec` — l'étape finale n'a **pas** les `-devel`
ffmpeg/libdrm/libcap. La file cap est posée par `files/scripts/post-install`
(`setcap cap_sys_admin+p /usr/bin/gpad-rec`) — **vérifier SELinux** à ce
moment-là (`bin_t` + `cap_sys_admin` n'est pas testé en image).

Banc qualité à lancer avant de juger : `./test-qualite.sh` (4 profils,
sauvegarde/restauration de `recorder.conf`, mesure durée/débit/Go-h/décodage).
Audio compris : chaque clip doit avoir `durée audio ≈ durée vidéo` (`ffprobe
-show_entries stream=codec_type,start_time,duration`).

Banc : `pkexec setcap cap_sys_admin+p ./gpad-rec` une fois par compilation, puis
exécution en utilisateur normal. Scénario complet avec `banc/client.py` :

```bash
./gpad-rec --daemon --replay-sec 6 --socket /tmp/opencode/grec.sock \
    --tampon /dev/shm/grec &
python3 banc/client.py /tmp/opencode/grec.sock     # status, replay, record, quit
```

Mesures de référence à reproduire (1440p, DP-1 à 165 Hz) :

| Ce qui est mesuré | Valeur du banc |
|---|---|
| 60/120/144/240 fps tenus (HEVC, 1440p natif) | 60,1 / 120,1 / 144,1 / 240,1 |
| CPU (boucle complète, 1440p60) | 0,15 s par 5 s ≈ **3 %** |
| Couleurs (PSNR direct vs rouge/bleu permutés) | 26,1 dB contre 16,1 dB |
| Anneau 6 s (1080p HEVC ~6 Mbit/s) | 4,9 Mo en `/dev/shm` |
| Sauvegarde d'un replay de 6 s (remux copie) | ~0,2 s |


## Écarté (et pourquoi)

- **Portail ScreenCast** (capture d'une fenêtre, exclusion de l'overlay) :
  gardé pour la v2, mais le *restore token* est lié à une **fenêtre** — chaque
  relance de jeu rouvre le sélecteur, mur infranchissable à la manette.
- **`ScreenShot2` par frame** : mesuré ~40 ms par capture, compositeur
  verrouillé → 1080p30 au mieux, seulement en repli.
- **`hwdownload` + encodage CPU** : copie 8-33 Mo par frame, aucun intérêt.
- **Lier les fps à `fps_limit` de MangoHud** : c'est un plafond du jeu, pas ce
  que KWin présente, et ça n'existe pas sans MangoHud.
