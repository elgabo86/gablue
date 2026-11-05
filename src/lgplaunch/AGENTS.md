# lgplaunch — paquet modulaire (src/lgplaunch)

Lanceur de jeux/applications Linux : paquets `.lgp` (squashfs via squashfuse)
et exécutables directs (ELF, `.sh`, `.py`, `.AppImage`). Le binaire installé
reste `/usr/bin/lgplaunch` ; les `.desktop`, `LGP.desktop`, ES-DE et
`killthemall` ne changent pas.

## Paquet

Sources = ce dossier (`src/lgplaunch/`), assemblées par `build.sh` (même
principe que `src/gwine-launcher/`) :

| Fichier | Rôle |
|---|---|
| `lgplaunch` | lanceur de développement : `SCRIPT_DIR` + `source lib/*.sh` dans l'ordre, puis `main()` |
| `build.sh` | concatène `lib/*.sh` + le code principal en `lgplaunch-standalone.sh` (bannières `# Module:`) |
| `lib/core.sh` | variables globales (HOME réel, chemins `/tmp/lgp-*`, `--exe`), `error_exit` |
| `lib/overlay.sh` | overlayfs kernel via `unshare -U -m` : vérification, enregistrement différé, montage/démontage, nettoyage des marqueurs |
| `lib/args.sh` | `parse_arguments` (`--args`, `--exe`, `--exe=<rel>`, fichier) |
| `lib/lgp.sh` | montage squashfuse, démontage (instance déjà lancée, orphelins), `.launch`/`.args`, nom interne `.gamename` |
| `lib/exe.sh` | `--exe` : validation (relatif, sans `..`), scan des exécutables, menu |
| `lib/saves.sh` | amorçage des sauvegardes depuis `.save` vers `~/.local/share/lgp-saves` |
| `lib/copy.sh` | copies récursives avec réécriture des symlinks (internes/externes au pack) |
| `lib/extras.sh` | amorçage des extras depuis `.extra` vers `~/.cache/lgp-extra` |
| `lib/temp.sh` | overlay `.temp` (par dossier) et overlay complet (`*` dans `.temppath`) |
| `lib/symlinks.sh` | symlinks `/tmp/lgp-saves|extra|temp/$UID/<jeu>` (multi-jeux en parallèle) |
| `lib/env.sh` | `.env` de la racine et du dossier de l'exe |
| `lib/launch.sh` | `.script.sh` de pré-lancement, `launch_game` (ELF/.sh/.py/.AppImage) |
| `lib/modes.sh` | `run_lgp_mode` (unshare + heredoc `LGP_NAMESPACE_EOF`), `run_classic_mode` |

**Règle** : l'ordre de `LIB_FILES` dans `build.sh` est celui du fichier
d'origine — le préserver, l'assemblage est un simple concat (aucune
réécriture). Le code principal (`main`) est extrait du lanceur par
`sed -n '/^main() {$/,$p'`. Ne pas réintroduire de fichier unique :
`lgplaunch-standalone.sh` est généré et gitignoré.

## Build

- `Containerfile-gablue` : `COPY src/lgplaunch /src/lgplaunch`, puis
  `RUN /ctx/build-lgplaunch` juste après `build-apk-installer`.
- `files/scripts/build-lgplaunch` : `bash build.sh` puis
  `install -Dm755 lgplaunch-standalone.sh /usr/bin/lgplaunch`.
- Développement : `src/lgplaunch/lgplaunch` source les modules en place.
- **Toute modif de `lib/` exige un rebuild d'image** (le binaire lit
  `/usr/bin/lgplaunch`, pas le dossier du dépôt).

## Comportement

- **`.lgp`** : squashfuse (pas de sudo) sur `/tmp/lgpackmount-$UID/<nom>`, nom
  lu dans `.gamename` (repli nom du fichier). Instance déjà montée : si
  personne ne l'utilise (`lsof`), montage orphelin nettoyé ; si elle tourne,
  confirmation kdialog puis kill/relance, sinon échec explicite.
- **Saves / extras / temp** : amorçage **par item** — absent copié du pack,
  présent intact, jamais écrasé (un pack re-classé se répare au lancement ;
  même logique que gwine `lib/wgp/symlinks.sh`). Symlinks par jeu
  (`/tmp/lgp-saves-$UID/<jeu>` → `~/.local/share/lgp-saves/<jeu>`,
  `.../lgp-extra-$UID` → `~/.cache/lgp-extra`, temp dans `/tmp`) : plusieurs
  packs tournent en parallèle.
- **Temp** : overlayfs kernel **dans le unshare du jeu** (montage différé
  listé par `_LGP_KERNEL_OVERLAY_MOUNTS`, exécuté par
  `_lgp_kernel_overlay_mount_script`). `*` dans `.temppath` = overlay complet
  du pack entier (`/tmp/lgp-full-overlay-$UID/lgp-<nom>` ; upper/work
  séparés). Constaté (test synthétique 05/11/2026, comportement conservé) :
  `prepare_temps` tourne **avant** `read_lgp_config`, donc `FULL_EXE_PATH` est
  encore vide au moment du calcul de `REL_EXE` — le chemin de l'exe redirigé
  est ensuite réécrit par `read_lgp_config` et le jeu s'exécute depuis le mont
  squashfuse ; l'overlay complet reste monté et utilisable par son chemin
  `/tmp/lgp-full-overlay/...`.
- **`--exe`** (action `EditLGP.desktop`) : `--exe` = menu des exécutables du
  pack, `--exe=<rel>` = chemin direct relatif à la racine (absolu et `..`
  refusés). Scan en deux passes — **racine du pack d'abord** (`.launch`,
  `game.sh`/`launch.sh`, binaires) puis le reste trié, plafond 40 ; libs
  (`*/lib/*`, `*.so*`, `*.a/.o/.dll`) et plomberie (`.rebuild`, `.save`)
  exclues — sans ça les 20 entrées d'un pack Dolphin étaient remplies par ses
  `.so` et son `launch.sh` n'apparaissait jamais (vécu F-ZERO GX 10/2026).
  Symlinks suivis manuellement (jamais `find -L`, qui sortirait du pack par
  `/tmp/lgp-saves/...`). Menu par `kdialog`, repli numéroté sur `/dev/tty`.
- **Classique** : exécutable direct, `.script.sh` du dossier puis `.env`,
  arguments `--args`.
- **Nettoyage** : `trap cleanup_lgp EXIT` (démontage FUSE, symlinks, overlays
  orphelins, marqueurs `/tmp/lgp-ko-*.pid`). `killthemall` cible les packs par
  leur mount `/tmp/lgpackmount` (pas par le nom du lanceur).
- Handlers MIME `LGP.desktop`/`LGPTerminal.desktop` (`application/x-lgp`) ;
  commandes ES-DE `desktop`/`switch`.

## Tests

Toujours en `HOME` de bac à sable (les saves/extras vont dans le sandbox, pas
dans ceux de l'utilisateur) et sans instance `lgplaunch` en cours :

1. **Assemblage** : `bash src/lgplaunch/build.sh`, puis `bash -n` sur le
   standalone, le lanceur et chaque `lib/*.sh`.
2. **Code identique** : comparer le standalone à l'ancien fichier d'origine en
   ignorant les lignes de commentaire et les lignes vides — l'ordre des lignes
   de code doit être identique (bannières de modules mises à part).
3. **Fonctions identiques** : sourcer les deux scripts sans la dernière ligne
   (`main "$@"`) et comparer `declare -f` fonction par fonction.
4. **Comportement** : lancer l'ancien, le standalone et le lanceur de dev sur
   les mêmes cas et comparer sortie + code retour — sans arguments, fichier
   absent, `--args` sur un script direct, et sur un vrai `.lgp` les refus
   `--exe=/abs`, `--exe=../x`, `--exe=absent`, `--exe=.launch` (non
   exécutable). Fin de chaque cas : aucun montage `/tmp/lgpackmount-$UID` ni
   `squashfuse` résiduel.
5. **Chemins complets** avec un pack synthétique (`mksquashfs` : `.gamename`,
   `.launch`, `.args`, `.savepath` + `.save/`, `.extrapath` + `.extra/`,
   `.temppath` + `.temp/`, `.env`, script de test) : amorçage saves/extras,
   `.env`, `.args`, overlay temp monté, `--exe=<script>` lancé, overlay
   complet (lecture lower + écriture upper).
