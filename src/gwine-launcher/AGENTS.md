# Gwine Launcher — contrat du lanceur (src/gwine-launcher)

> Détail des modules `lib/` (architecture, composants, shims, pièges) →
> `lib/AGENTS.md`.

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
│   └── AGENTS.md            # Architecture, modules, points techniques
│   ├── core.sh              # Variables globales et fonctions utilitaires
│   ├── parse-args.sh        # Analyse des arguments CLI et aide
│   ├── xbox.sh              # Gestion ds2xbox (émulation Xbox, compteur références)
│   ├── runner.sh            # Gestion des runners Wine/Proton
│   ├── dxvk-mode.sh         # Gestion du mode DXVK standard/async
│   ├── display-mode.sh      # Gestion du mode d'affichage (Wayland vs X11)
│   ├── ui.sh                # Interface utilisateur (Gablue Manager, GameMode)
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
│   │   ├── d7vk.sh              # Gestion D7VK (Direct3D 7/6/5/3 → Vulkan, proxy ddraw)
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
- gablue-manager (interface graphique optionnelle) : progression, confirmation et choix passent par ses modes CLI (`--progress`, `--confirm`, `--choose`) au lieu de kdialog. Sans session graphique ou sans le binaire, tout retombe sur le terminal (l'init fonctionne quand même, avec un journal lisible)
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
./gwine --init                  # Init/MAJ : seulement si un composant a changé
./gwine --init --force          # Réinitialisation complète forcée
./gwine --init --offline        # Mode offline
./gwine --init --dxvk-async     # Init avec DXVK-GPLAsync
./gwine --update                # Mettre à jour les composants
./gwine --download-components   # Télécharger tous les composants
./gwine --cachepack             # Créer un pack cache pour offline
```

`--init` est la voie de mise à jour (gablue-manager, local et distant) : il ne
réinitialise le préfixe que si un composant manque ou a une nouvelle version
(runner gwine, DXVK/GPLAsync, VKD3D, D7VK, NVAPI, Mono/Gecko, wincomponents) ;
sinon il rend la main sans rien toucher. `--force` réinitialise même à jour.

Premier lancement (`.exe`/`.wgp` sans préfixe) : `check_prefix_or_offer_init()`
lance l'init comme sous-processus dans la **fenêtre de progression de Gablue
Manager** (`gablue-manager --progress … -- gwine --init`, fermée d'elle-même en
cas de succès), puis enchaîne sur le jeu. Cache offline complet → init
`--offline` directe ; sinon `gablue-manager --confirm` demande avant de
télécharger. Sans manager, confirmation au terminal et init en direct. Aucune
fenêtre kdialog.

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
./gwine --gamescope ~/jeu.wgp   # Lancer dans gamescope avec mangoapp (désactive MangoHud injecté)
./gwine --720 ~/jeu.wgp        # Gamescope 1280x720 interne (raccourci)
./gwine --900 ~/jeu.wgp        # Gamescope 1600x900 interne (raccourci)
./gwine --1080 ~/jeu.wgp       # Gamescope 1920x1080 interne (raccourci)
./gwine --gamescope-args "-w 1920 -h 1080 -f --mangoapp" ~/jeu.wgp  # Args gamescope personnalisés
# Fichier .gamescope : gamescope s'active automatiquement au lancement
#   emplacements : racine du pack .wgp, ou à côté du fichier lancé (.exe/.bat)
#   contenu = args (ex: "-f -w 1280 -h 720 --adaptive-sync --mangoapp")
#   vide = défaut gwine, "off" = désactivé pour ce jeu
```

### Configuration gamescope par défaut
```bash
./gwine --720                   # Définir 720p par défaut pour les futurs lancements
./gwine --1080                  # Définir 1080p par défaut
./gwine --no-gamescope          # Désactiver gamescope par défaut
```

### Configuration xbox par défaut
```bash
./gwine --xbox-on               # Activer le mode xbox par défaut (tous les lancements)
./gwine --xbox-on --xbox-ds4    # Activer le mode xbox par défaut (DualShock 4 uniquement)
./gwine --xbox-off              # Désactiver le mode xbox par défaut
```

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

## Notes pour les agents

1. Toujours vérifier l'existence du préfixe Wine avant les opérations
2. Respecter le système d'overlay pour les modifications
3. Tester les modes offline et online
4. Prendre en compte les différents types de GPU
5. Gérer correctement les erreurs de montage/démontage des overlays kernel (namespaces)
6. Les fichiers `.env` dans les dossiers de jeux sont sourcés automatiquement
7. Les fichiers `.reg` sont auto-détectés et installés
8. Ne jamais committer le fichier `gwine-standalone.sh` généré
