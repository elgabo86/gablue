# AGENTS.md - Gwine Launcher

## Description du projet

**Gwine** est un lanceur de jeux Windows pour Linux utilisant son runner Wine intégré (basé sur l'arbre Valve Proton). C'est un outil Bash permettant de lancer des jeux Windows via Wine avec support des paquets WGP (format personnalisé) et des exécutables directs (.exe).

## Type de projet

- **Langage principal** : Bash scripting
- **Architecture** : Modulaire avec script principal + bibliothèques
- **Type** : Lanceur de jeux Windows sous Linux

## Structure du projet

```
src/gwine-launcher/
├── gwine                    # Script principal (point d'entrée)
├── ds2xbox                  # Binaire ds2xbox (émulation manette Xbox 360)
├── build.sh                 # Script d'assemblage du fichier standalone
├── lib/                     # Bibliothèques modulaires
│   ├── core.sh              # Variables globales et fonctions utilitaires
│   ├── parse-args.sh        # Analyse des arguments CLI et aide
│   ├── xbox.sh              # Gestion ds2xbox (émulation Xbox, compteur références)
│   ├── runner.sh            # Gestion des runners Wine/Proton
│   ├── dxvk-mode.sh         # Gestion du mode DXVK standard/async
│   ├── display-mode.sh      # Gestion du mode d'affichage (Wayland vs X11)
│   ├── ui.sh                # Interface utilisateur (kdialog, GameMode)
│   ├── gpu.sh               # Détection GPU et configuration Vulkan
│   ├── download.sh          # Téléchargement et gestion GitHub
│   ├── cache/               # Modules de gestion du cache
│   │   ├── gwine-runner.sh       # Téléchargement/installation runner gwine
│   │   ├── dxvk-vkd3d.sh        # DXVK, VKD3D, NVAPI + download_vkd3d()
│   │   ├── offline.sh           # Mode offline, préparation cache
│   │   └── cachepack.sh         # Création de packs cache offline
│   ├── component.sh         # Redirection vers modules components/
│   ├── components/          # Modules de gestion des composants
│   │   ├── utils.sh             # Utilitaires pour composants
│   │   ├── dxvk.sh              # Gestion DXVK/VKD3D
│   │   ├── nvapi.sh             # Gestion DXVK-NVAPI (NVIDIA uniquement)
│   │   ├── mono.sh              # Gestion Wine Mono/Gecko
│   │   └── winetricks.sh        # Intégration winetricks
│   ├── wincomponents.sh     # Redirection vers modules wincomponents/
│   ├── wincomponents/       # Composants Windows sans winetricks
│   │   ├── defs.sh              # URLs, checksums, liste composants
│   │   ├── utils.sh             # Téléchargement, vérification cache
│   │   ├── vcrun.sh             # Visual C++ Redistributables
│   │   ├── dotnet.sh            # .NET Desktop Runtime
│   │   ├── directx.sh           # DirectX, XACT, D3DCompiler
│   │   ├── fonts.sh             # Core Fonts et Tahoma
│   │   ├── misc.sh              # OpenAL, PhysX, MSLS31, VB6
│   │   ├── wmp9.sh              # Windows Media Player 9 + wsh57
│   │   └── main.sh              # install_all_wincomponents
│   ├── wineprefix.sh        # Gestion du préfixe Wine
│   ├── wineserver-manager.sh  # Redirection vers modules wineserver/
│   ├── wineserver/          # Modules de gestion wineserver
│   │   ├── init.sh              # Initialisation système de locks
│   │   ├── master.sh            # Gestion instance maître
│   │   ├── counter.sh           # Compteur d'instances
│   │   ├── server.sh            # Gestion wineserver
│   │   └── cleanup.sh           # Nettoyage locks orphelins
│   ├── wgp.sh               # Redirection vers modules wgp/
│   ├── wgp/                 # Modules de gestion des packs WGP
│   │   ├── core.sh              # Variables globales et utilitaires WGP
│   │   ├── mount.sh             # Montage/démontage squashfuse
│   │   ├── overlay.sh           # Overlays kernel overlayfs (via unshare)
│   │   ├── symlinks.sh          # Symlinks saves/extras
│   │   └── modes.sh             # Modes WGP (sélection exe)
│   ├── launcher.sh          # Redirection vers modules launcher/
│   ├── launcher-utils.sh    # Redirection vers modules launcher-utils/
│   ├── launcher-utils/      # Utilitaires de lancement
│   │   ├── env.sh               # Chargement fichiers .env
│   │   ├── registry.sh          # Installation fichiers .reg
│   │   ├── pds.sh               # Configuration ProgramData Saves
│   │   ├── paths.sh             # Utilitaires de chemins
│   │   ├── conflict.sh          # Vérification conflits d'instance
│   │   └── setup.sh             # Configuration environnement Wine
│   ├── launcher-main.sh     # Lancement effectif des jeux (launch_wine_game)
│   ├── sandbox.sh           # Sandboxing avec bubblewrap
│   ├── composefs_statfs_shim.c  # Source C du shim overlayfs statfs
│   ├── composefs_statfs_shim.so # Shim 64-bit compilé
│   ├── composefs_statfs_shim32.so # Shim 32-bit compilé
│   ├── mode-init.sh         # Redirection vers modules modes/
│   ├── modes/               # Modules d'initialisation
│   │   ├── init-main.sh         # init_prefix_only, init_wineprefix
│   │   └── init-ensure.sh       # ensure_wineprefix, ensure_wineprefix_full
│   ├── mode-update.sh       # Mode --update
│   ├── mode-tools.sh        # Outils Wine (--regedit, --winecfg, etc.)
│   └── dir-config.sh        # Configuration répertoires bind mounts
└── AGENTS.md                # Ce fichier
```

## Technologies et dépendances

### Dépendances système requises
- Wine 64-bit (gwine)
- squashfuse (montage des .wgp)
- bubblewrap (sandboxing)
- wget, curl (téléchargements)
- tar, 7z (extraction)
- kdialog, qdbus (interface graphique optionnelle). La commande qdbus est détectée dynamiquement par `_get_qdbus_cmd()` (ui.sh) qui teste `qdbus6`, `qdbus-qt6` (Plasma 6 / Kinoite fc44), `qdbus-qt5` puis `qdbus`. Sans qdbus, les barres de progression kdialog sont silencieusement désactivées (l'init/update fonctionne quand même, sans feedback visuel)
- gamemoded (GameMode pour performances)

### Support GPU
- NVIDIA (drivers propriétaires)
- AMD (RADV)
- Intel (ANV, Haswell/Broadwell)

## Commandes principales

### Lancement de jeux
```bash
./gwine ~/Jeux/monjeu.wgp       # Lancer un pack WGP
./gwine ~/Jeux/monjeu.exe       # Lancer un exécutable
```

### Initialisation et mise à jour
```bash
./gwine --init                  # Initialisation standard
./gwine --init --offline        # Mode offline
./gwine --init --kdialog        # Avec interface graphique
./gwine --init --dxvk-async     # Init avec DXVK-GPLAsync
./gwine --update                # Mettre à jour les composants
./gwine --download-components   # Télécharger tous les composants
./gwine --cachepack             # Créer un pack cache pour offline
```

### Mode DXVK
```bash
./gwine --dxvk                  # Utiliser DXVK standard
./gwine --dxvk-async            # Utiliser DXVK-GPLAsync
```

### Outils Wine
```bash
./gwine --regedit               # Editeur de registre
./gwine --reg fichier.reg       # Installer un fichier registre
./gwine --reg add 'HKCU\Software\Wine\DllOverrides' /v ddraw /d native,builtin /f  # Ajouter une valeur
./gwine --reg del 'HKCU\Software\Wine\DllOverrides' /v ddraw /f                    # Supprimer une valeur
./gwine --reg get 'HKCU\Software\Wine\DllOverrides' /v ddraw                        # Consulter une clé/valeur
./gwine --winecfg               # Configuration Wine
./gwine --winetricks            # Lancer winetricks
./gwine --cmd                   # Terminal Wine
./gwine --joytest               # Test de joypad
./gwine --kill                  # Forcer l'arrêt de tous les processus
```

#### Sous-commandes --reg
- `add <clé> /v <valeur> /d <donnée> [/f]` : Ajouter une valeur de registre
- `del <clé> [/v <valeur> | /va | /f]` : Supprimer une valeur ou clé de registre
- `get <clé> [/v <valeur>]` : Consulter une clé ou valeur de registre
- `<fichier.reg>` : Installer un fichier .reg (comportement historique)

### Options de lancement
```bash
./gwine --fix ~/jeu.wgp         # Mode fix manette
./gwine --xbox ~/jeu.wgp        # Émulation manettes Sony en Xbox 360 via ds2xbox
./gwine --xbox-ds4 ~/jeu.wgp   # Émulation DualShock 4 uniquement
./gwine --xbox-dualsense ~/jeu.wgp  # Émulation DualSense uniquement
./gwine --nofix ~/jeu.wgp       # Ignorer le fichier .fix d'un jeu
./gwine --exewgp ~/jeu.wgp      # Choisir l'exécutable dans un WGP
./gwine --nosandbox ~/jeu.exe   # Désactiver le sandboxing
./gwine --wayland               # Forcer Wine à utiliser Wayland
./gwine --args "--option" ~/jeu # Passer des arguments au jeu
./gwine --env VAR=VAL ~/jeu     # Passer une variable d'environnement
./gwine --dir add <chemin>      # Ajouter un répertoire aux bind mounts
./gwine --dir list              # Lister les répertoires bindés
```

### Configuration xbox par défaut
```bash
./gwine --xbox-on               # Activer le mode xbox par défaut (tous les lancements)
./gwine --xbox-on --xbox-ds4    # Activer le mode xbox par défaut (DualShock 4 uniquement)
./gwine --xbox-off              # Désactiver le mode xbox par défaut
```

## Architecture du code

### Flux d'exécution principal
1. `main()` dans `gwine` - point d'entrée
2. `parse_arguments()` - Analyse des arguments CLI
3. `run_wgp_mode()` ou `run_classic_mode()` selon le type de fichier
4. Montage du pack WGP (si applicable)
5. Initialisation du préfixe Wine
6. Configuration GPU (Vulkan)
7. Lancement avec sandboxing bubblewrap

### Ordre de chargement des modules (gwine)
```bash
lib/core.sh
lib/parse-args.sh
lib/xbox.sh
lib/runner.sh
lib/dxvk-mode.sh
lib/display-mode.sh
lib/download.sh
lib/gpu.sh
lib/ui.sh
lib/component.sh          # -> charge components/*
lib/wincomponents.sh      # -> charge wincomponents/*
lib/cache/gwine-runner.sh
lib/cache/dxvk-vkd3d.sh
lib/cache/offline.sh
lib/cache/cachepack.sh
lib/wineprefix.sh
lib/wineserver-manager.sh # -> charge wineserver/*
lib/sandbox.sh
lib/wgp.sh                # -> charge wgp/*
lib/launcher.sh           # -> charge launcher-utils/* + launcher-main.sh
lib/mode-init.sh          # -> charge modes/*
lib/mode-update.sh
lib/mode-tools.sh
lib/dir-config.sh
```

### Modules et responsabilités

- **core.sh** : Variables globales, fonctions utilitaires
- **parse-args.sh** : Analyse des arguments CLI, validation, aide (show_help)
- **xbox.sh** : Gestion ds2xbox (émulation Xbox 360, compteur références, config par défaut)
- **runner.sh** : Gestion des runners Wine/Proton
- **dxvk-mode.sh** : Configuration DXVK standard vs GPLAsync
- **display-mode.sh** : Gestion du mode d'affichage (Wayland natif vs X11/XWayland)
- **ui.sh** : Barres de progression kdialog, GameMode. `_get_qdbus_cmd()` détecte la commande qdbus disponible (`qdbus6`, `qdbus-qt6`, `qdbus-qt5`, `qdbus`) ; toutes les fonctions `progress_*` et les gardes kdialog (init-ensure.sh, mode-update.sh) l'utilisent au lieu de tester `qdbus` en dur
- **gpu.sh** : Détection automatique du GPU et configuration Vulkan
- **download.sh** : Téléchargement GitHub, extraction d'archives, récupération versions composants (get_component_version avec double source officiel+bottles pour DXVK/VKD3D)
- **cache/*** : Gestion du cache
  - gwine-runner.sh : Téléchargement, installation et mise à jour du runner gwine
  - dxvk-vkd3d.sh : Mise à jour DXVK, VKD3D, NVAPI + download_vkd3d() pour téléchargement VKD3D seul (utilisé quand le mode DXVK async gère DXVK séparément)
  - offline.sh : Préparation cache, mode offline, téléchargement Mono/Gecko avec vérification de version. `prepare_full_offline_cache()` (appelée par `--download-components`) pré-cache le runner gwine et télécharge **toujours** DXVK-NVAPI (cache portable, indépendant du GPU de la machine qui construit le pack)
  - cachepack.sh : Création de packs cache pour déploiement offline. Empaquette le runner gwine, vérifie DXVK standard + GPLAsync + VKD3D + DXVK-NVAPI + Mono/Gecko + wincomponents. Le `install-cache.sh` généré déploie uniquement gwine
- **component.sh** : Fichier de redirection vers components/*
- **components/*** : Gestion des composants individuels
  - utils.sh : Utilitaires (copy_dll_files, create_dll_overrides, get_wine_system_paths)
  - dxvk.sh : Installation DXVK/VKD3D
  - nvapi.sh : DXVK-NVAPI (NVIDIA uniquement)
  - mono.sh : Wine Mono/Gecko
  - winetricks.sh : Intégration winetricks
- **wincomponents.sh** : Fichier de redirection vers wincomponents/*
- **wincomponents/*** : Installation des composants Windows (remplace winetricks)
  - defs.sh : URLs, checksums, liste des composants requis
  - utils.sh : Téléchargement et vérification du cache
  - vcrun.sh : Visual C++ Redistributables (2010-2022, VCRUN6)
  - dotnet.sh : .NET Desktop Runtime (6, 7, 8)
  - directx.sh : DirectX, XACT Engine, D3DCompiler
  - fonts.sh : Core Fonts et Tahoma
  - misc.sh : OpenAL, PhysX, MSLS31, VB6 Runtime
  - wmp9.sh : Windows Media Player 9 + wsh57. setup_wm.exe installe les codecs de base (wmvcore, wmp, l3codeca.acm) en 32 et 64-bit. Le pack supplémentaire wm9codecs (WM9Codecs9x.exe) est ignoré silencieusement en 64-bit car superflu.
  - main.sh : Fonction principale install_all_wincomponents
- **wineprefix.sh** : Création/gestion du préfixe Wine, copie ICU 68 DLLs (gwine uniquement)
- **wineserver-manager.sh** : Fichier de redirection vers wineserver/*
- **wineserver/*** : Gestion du wineserver persistant
  - init.sh : Initialisation système de locks
  - master.sh : Gestion instance maître (namespaces partagés)
  - counter.sh : Compteur d'instances
  - server.sh : Gestion wineserver (wait, kill, wrappers)
  - cleanup.sh : Nettoyage locks orphelins
- **wgp.sh** : Fichier de redirection vers wgp/*
- **wgp/*** : Gestion des packs WGP
  - core.sh : Variables globales et utilitaires WGP
  - mount.sh : Montage/démontage squashfuse
  - overlay.sh : Overlays kernel overlayfs (via unshare, user namespaces)
  - symlinks.sh : Symlinks saves/extras
  - modes.sh : Modes WGP (sélection exe)
- **launcher.sh** : Fichier de redirection vers launcher-utils/* + launcher-main.sh
- **launcher-utils/*** : Utilitaires de lancement
  - env.sh : Chargement fichiers .env
  - registry.sh : Installation fichiers .reg
  - pds.sh : Configuration ProgramData Saves
  - paths.sh : Utilitaires de chemins (transliterate, create_temp_path)
  - conflict.sh : Vérification conflits d'instance
  - setup.sh : Configuration environnement Wine
- **launcher-main.sh** : Lancement effectif des jeux avec Wine (launch_wine_game)
- **sandbox.sh** : Configuration bubblewrap pour le sandboxing
- **mode-init.sh** : Fichier de redirection vers modes/*
- **modes/*** : Modules d'initialisation du préfixe
  - init-main.sh : init_prefix_only, init_wineprefix
  - init-ensure.sh : ensure_wineprefix, ensure_wineprefix_full
- **mode-update.sh** : Mode --update
- **mode-tools.sh** : Outils Wine (--regedit, --winecfg, --winetricks, --reg avec sous-commandes add/del/get)
- **dir-config.sh** : Configuration répertoires bind mounts

## Répertoires importants

- **Préfixe Wine** : `$HOME/Windows/Prefix`
- **Données Gwine** : `$HOME/.local/share/gwine`
- **Lib Gwine** : `$HOME/.local/share/gwine/lib` (shims, binaires)
- **Cache** : `$HOME/.cache/gwine`
- **Dossier Windows** : `$HOME/Windows/` (contient UserData, SteamData, Games, Applications)

## Conventions de code

- Le script principal `gwine` source les fichiers de redirection de `lib/`
- Les fichiers de redirection (component.sh, wincomponents.sh, etc.) chargent les sous-modules
- Variables globales définies dans `core.sh` et utilisées dans tous les modules
- Utilisation extensive de fonctions bash modulaires
- Gestion des erreurs avec `set -e` et vérifications explicites

## Points techniques importants

### Système d'overlay
- Utilisation du kernel overlayfs natif via `unshare` (user namespaces) pour les overlays WGP (.temp)
- Montage différé : les overlays sont montés dans le même unshare que bwrap au moment du lancement
- Le workdir doit être sur un chemin SÉPARÉ du upperdir (pas `.work/` dans upperdir)

### Format WGP
- Fichiers squashfs compressés avec extension `.wgp`
- Contiennent un fichier `.gamename` pour le nom interne
- Support des dossiers `saves/`, `extra/`, `temp/` pour persistance des données

### Sandboxing
- Utilisation de bubblewrap pour isoler l'exécution des jeux
- Restrictions sur les accès système pour la sécurité

### Variables d'environnement
- `WINEPREFIX`, `WINEARCH`, `WINEFSYNC`, `WINENTSYNC`
- `VK_ICD_FILENAMES` (sélection du driver Vulkan)
- `DISABLE_GAMEMODE=1` pour désactiver GameMode
- `WINE_LARGE_ADDRESS_AWARE=1` activé par défaut
- `LC_ALL=fr_FR` par défaut (surchargeable via `--env` ou variable d'environnement)
- `DXVK_ASYNC=1` en mode dxvk-async
- `SDL_GAMECONTROLLER_IGNORE_DEVICES` en mode xbox (IDs Sony masqués, voir xbox.sh)
- `GWINE_NO_STATFS_SHIM=1` pour désactiver le shim overlayfs statfs
- `GST_PLUGIN_SYSTEM_PATH_1_0` (plugins bundlés uniquement, pas de paths système)
- `GST_PLUGIN_FEATURE_RANK` (nvh264dec:0,nvh265dec:0 en mode proton)
- `GST_REGISTRY` / `GST_REGISTRY_1_0` (cache registry dédié gwine)
- `GST_REGISTRY_32` (cache registry i686 dédié gwine)
- `GST_PLUGIN_SCANNER` / `GST_PLUGIN_SCANNER_1_0` (scanner bundlé 64-bit)
- `GST_PLUGIN_SCANNER_32` (scanner bundlé 32-bit)

### Overlayfs statfs shim — Fix Kinoite/composefs

Sur Fedora Kinoite (ostree), le rootfs `/` est un overlayfs read-only avec `f_bavail=0`. Wine mappe Z: vers `/`, donc `GetDiskFreeSpaceEx(NULL)` retourne 0 MB → les jeux qui vérifient l'espace disque du CWD crashent (ex: AGS v3.6 "Unable to write in the savegame directory").

**Architecture du shim** :
- `composefs_statfs_shim.c` : source C qui intercepte `fstatfs`/`fstatfs64`/`statvfs`/`statvfs64`/`fstatvfs`/`fstatvfs64` via `LD_PRELOAD`
- Quand `f_type == OVERLAYFS_SUPER_MAGIC (0x794c7630)` et `f_bavail == 0`, le shim remplace les valeurs par des blocks factices (50000000 blocks = ~200 GB)
- Deux binaires : 64-bit (`lib64/`) et 32-bit (`lib/`) — Wine lance des processus mixtes 32/64-bit
- `LD_PRELOAD` utilise `$LIB` de glibc : `$GWINE_LIB_DIR/$LIB/composefs_statfs_shim.so` → le dynamic linker charge automatiquement le bon .so selon la classe ELF du process (pas de messages `wrong ELF class`)

**Installation runtime** :
- Mode standalone : les .so sont embarqués en base64 dans `gwine-standalone.sh`, extraits via `_gwine_extract_shims()` dans `$GWINE_LIB_DIR/lib/` et `$GWINE_LIB_DIR/lib64/`
- Mode développement : les .so sont copiés depuis `lib/` du repo vers `$GWINE_LIB_DIR/`

**Détection** : `stat -f / --printf="%a"` retourne `0` (f_bavail) sur Kinoite → shim activé automatiquement

**Recompilation** (si modification du source C) :
```bash
# 64-bit
gcc -shared -fPIC -o lib/composefs_statfs_shim.so lib/composefs_statfs_shim.c -ldl
# 32-bit (nécessite glibc-devel.i686, utiliser le container Fedora)
podman run --rm -v "$(pwd)/lib:/src:z" docker.io/library/fedora:43 bash -c \
  "dnf install -y gcc glibc-devel.i686 && gcc -m32 -shared -fPIC -o /src/composefs_statfs_shim32.so /src/composefs_statfs_shim.c -ldl"
```

### Gestion des composants
- DXVK : double source (officiel `doitsujin/dxvk` + `bottlesdevs/components`), prend la version la plus haute (priorité officiel en cas d'égalité)
- VKD3D-Proton : double source (officiel `HansKristian-Work/vkd3d-proton` + `bottlesdevs/components`), même règle
- DXVK-NVAPI : source unique (`bottlesdevs/components`, NVIDIA uniquement). **Installé** dans le préfixe seulement sur GPU NVIDIA (`install_dxvk_nvapi` garde son `is_nvidia_gpu`), mais **toujours téléchargé/empaqueté** dans le cache offline pour rester portable
- DXVK-GPLAsync : source unique (`gitlab.com/Ph42oN/dxvk-gplasync`, API GitLab v4), version extraite depuis le tag GitLab (format `vX.Y.Z-N`)
- `auto_update_components()` détecte le mode DXVK configuré (`dxvk` ou `dxvk-async`) et adapte la vérification/téléchargement en conséquence (async → `download_dxvk_async` + `download_vkd3d`, standard → `download_updated_dxvk_vkd3d`)
- La source choisie est stockée dans les globales `_DXVK_SOURCE` / `_VKD3D_SOURCE` ("official" ou "bottles")
- Téléchargement automatique depuis GitHub
- Système de vérification SHA256
- Gestion des versions avec backup automatique
- Support offline avec cache local
- Mode `--cachepack` pour créer des packs déployables sur machines sans internet
- **Installation du runner depuis le cache en mode offline** : le runner extrait vit dans `~/.local/share/gwine/runner`, tandis que le cache `~/.cache/gwine/components/gwine` ne contient que l'archive. Pour permettre de ne déployer QUE `~/.cache/gwine` (ex. déploiement offline pendant l'install Anaconda), deux points d'entrée installent le runner extrait à la volée depuis le cache si absent :
  - `init_prefix_only()` (modes/init-main.sh) : en mode `--init --offline`, si `$WINE_DIR/bin/wine` est absent, appelle `install_gwine_from_cache` puis `update_runner_paths` (au lieu de l'ancien `error_exit` immédiat). Échec uniquement si aucune archive n'est dans le cache
  - `ensure_runner_installed()` (runner.sh) : au lancement d'un jeu sans réseau, tente la même installation depuis le cache avant d'échouer (préserve le comportement online : si réseau dispo, télécharge la dernière version)
- **Détection automatique du mode offline (first-run)** : quand le préfixe n'existe pas et qu'un jeu est lancé pour la première fois, `check_prefix_or_offer_init()` (gwine) détecte automatiquement si le cache local est complet via `gablue_offline_cache_ready()` (offline.sh). Si le cache contient le runner (archive), Wine Mono/Gecko, et les wincomponents, l'init se fait en mode `--init --offline` avec barre de progression kdialog, **sans jamais solliciter le réseau**. Si le cache est incomplet, le comportement actuel est conservé (proposition d'init online). `gablue_offline_cache_ready()` vérifie :
  - Archive du runner présente (`components/gwine/gwine-*.tar.xz`)
  - Wine Mono/Gecko présents (`components/wine-cache/wine-mono-*.msi`, `wine-gecko-*.msi`)
  - Composants Windows présents (`wincomponents/`, validés par `check_wincomponents_cache`)

### Build standalone
- `build.sh` assemble tous les modules en un fichier `gwine-standalone.sh`
- Les shims overlayfs .so sont embarqués en base64 et extraits au runtime
- Utilise les fichiers de redirection pour charger les sous-modules
- Le fichier généré ne doit pas être commité

## Précautions et cas particuliers

- **Overlayfs shim** : Sur Kinoite, le rootfs a `f_bavail=0` → le shim corrige `GetDiskFreeSpaceEx(NULL)` automatiquement
- **Overlays kernel** : Nécessitent un workdir SÉPARÉ de l'upperdir (le kernel overlayfs via unshare échoue si workdir ⊆ upperdir)
- **Caractères non-ASCII** : Conversion en chemins temporaires pour Wine
- **Verrous de processus** : Évite les conflits sur les packs WGP
- **Relancement** : Support du relancement d'un jeu déjà en cours d'exécution
- **Mono/Gecko** : Les versions sont hardcodées dans le code (MONO_VER, GECKO_VER dans offline.sh). Le cache vérifie la présence des fichiers exacts (pas juste « dossier non vide »). Les anciennes versions sont supprimées avant téléchargement. En cas de mise à jour des versions hardcodées, penser à mettre à jour les 4 endroits : download_missing_components(), prepare_local_cache(), prepare_full_offline_cache() (offline.sh) **et la vérification dans create_cachepack() (cachepack.sh)** — un oubli sur ce dernier fait échouer `--cachepack` avec « Wine Mono/Gecko manquants ».
- **DXVK/VKD3D sources** : get_component_version compare officiel et bottlesdevs, stocke le choix dans `_DXVK_SOURCE` / `_VKD3D_SOURCE`. Les fonctions de téléchargement (dxvk-vkd3d.sh, offline.sh) lisent ces globales pour construire l'URL.

## Notes pour les agents

1. Toujours vérifier l'existence du préfixe Wine avant les opérations
2. Respecter le système d'overlay pour les modifications
3. Tester les modes offline et online
4. Prendre en compte les différents types de GPU
5. Gérer correctement les erreurs de montage/démontage des overlays kernel (namespaces)
6. Les fichiers `.env` dans les dossiers de jeux sont sourcés automatiquement
7. Les fichiers `.reg` sont auto-détectés et installés
8. Ne jamais committer le fichier `gwine-standalone.sh` généré
