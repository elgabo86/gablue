# Instructions pour les agents - Gablue

## Vue d'ensemble du projet

Gablue est une distribution immuable personnalisée basée sur **Fedora Kinoite** (KDE Plasma), construite avec des Containerfiles et utilisant le processus de build **Universal Blue (uBlue)**. Le projet utilise buildah/podman pour la construction d'images container et rpm-ostree pour le déploiement immuable.

### Caractéristiques principales

- **Base** : Fedora Kinoite 44 (KDE Plasma)
- **Kernel** : OGC kernel depuis ublue-os/akmods (optimisé pour le gaming)
- **Mesa** : Terra Mesa (version plus récente pour meilleures performances, multilib fc44)
- **NVIDIA** : Support des pilotes NVIDIA closed et open-source via akmods
- **Virtualisation** : Mode DX avec Docker, Libvirt, QEMU
- **Gaming** : Optimisations poussées (Gamescope, MangoHud, schedulers)
- **gremote** : remote desktop Wayland éphémère entre postes Gablue (waypipe + KWin imbriqué) — pont en attendant krdp, pas un remplacement (détail → `files/system/AGENTS.md`)

## Variantes d'images

Le projet construit 6 variantes distinctes :

| Variante | Description | Kernel | NVIDIA | Trigger tag |
|----------|-------------|--------|--------|-------------|
| `gablue-main` | Image standard sans NVIDIA | OGC | - | `[main]`, `[all]` |
| `gablue-nvidia` | Pilotes NVIDIA closed-source | OGC LTS | nvidia-lts | `[nvidia]`, `[all]` |
| `gablue-nvidia-open` | Pilotes NVIDIA open-source | OGC | nvidia-open | `[nvidia]`, `[all]` |
| `gablue-main-dx` | Mode développement (DX) avec virtualisation + ROCm | OGC | - | `[dx]`, `[all]` |
| `gablue-nvidia-open-dx` | Mode DX NVIDIA Open (virtualisation + GPU NVIDIA) | OGC | nvidia-open | `[dx]`, `[nvidia]`, `[all]` |
| ~~`gablue-main-test`~~ | Image de test avec OpenGamepadUI (fc44) — **désactivé** | OGC | - | `[test]`, `[all]` |
| ~~`gablue-nvidia-open-test`~~ | Test NVIDIA Open avec OpenGamepadUI (fc44) — **désactivé** | OGC | nvidia-open | `[test]`, `[nvidia]`, `[all]` |

### Différences entre variantes

**Main vs NVIDIA** :
- NVIDIA installe les pilotes depuis `ghcr.io/ublue-os/akmods-${NVIDIA_FLAVOR}`
- `NVIDIA_FLAVOR=nvidia-lts` pour les pilotes closed, `NVIDIA_FLAVOR=nvidia-open` pour les open
- Installation via `nvidia-install.sh` de ublue-os (gère driver, kmod, container-toolkit, supergfxctl, SELinux, dracut)
- Paquets additionnels gérés par nvidia-install.sh : `supergfxctl`, `supergfxctl-plasmoid` (Kinoite)

**Main vs DX** :
- DX inclut Docker CE, Libvirt, QEMU, virt-manager
- Activation automatique des services Docker et libvirt
- Groupes utilisateurs supplémentaires configurés

**Images de test (branche `test`)** :
- Les anciennes variantes `-test` (gablue-main-test, gablue-nvidia-open-test) avec OpenGamepadUI sont **obsolètes** — les Containerfiles et scripts `-test` restent dans le dépôt pour référence mais ne sont plus buildés
- La nouvelle approche utilise une **branche `test`** : pousser des modifs sur `refs/heads/test` déclenche le build de `gablue-main-test` et `gablue-nvidia-open-test` (packages GHCR séparés, tag `latest`) — les modifs sont testées sur les packages `-test` sans toucher aux packages stables
- Le schedule quotidien ne build que `main` → `latest` n'est jamais pollué par les modifs de test
- `update-readme` est exclu sur la branche `test` (pour ne pas écraser le README avec des versions de test)
- Seules 2 variantes sont buildées sur `test` : `gablue-main-test` et `gablue-nvidia-open-test` (les autres jobs restreignent leur condition à `github.ref == 'refs/heads/main'`)
- Côté client : `rpm-ostree rebase ostree-unverified-registry:ghcr.io/elgabo86/gablue-main-test:latest` pour tester, puis `gablue-main:latest` pour revenir
- Quand les modifs sont validées, merger `test` dans `main` (le schedule propage sur `latest`)

## Structure du projet

```
.
├── Containerfile-gablue                   # Containerfile principal (toutes variantes, stable et test)
├── cosign.pub                             # Clé publique pour signature
├── src/
│   ├── composefs-fix/                      # Correction espace libre Dolphin sur composefs
│   │   ├── composefs-fix.c                 # Hook LD_PRELOAD (intercepte statfs/statfs64)
│   │   └── Makefile                        # Compilation (.so)
│   ├── cpuid-fault/                         # Module kernel CPUID faulting (AMD)
│   │   ├── inc/                            # En-têtes (vmcb_layout.h, host_state.h)
│   │   ├── src/                            # Sources assembleur + C
│   │   └── Makefile                        # Compilation kernel (Kbuild)
│   ├── ds2xbox/                           # Sources C du convertisseur DualSense → Xbox
│   │   ├── ds2xbox.c                      # Programme principal (evdev, uinput)
│   │   └── Makefile                       # Compilation
│   ├── gamepadshortcuts/                  # Sources C du gestionnaire de raccourcis manette
│   │   ├── gamepadshortcuts.c             # Programme principal (inotify VT, evdev)
│   │   ├── mouse.c                        # Emulation souris/clavier via manette (evdev, uinput)
│   │   ├── kbdnav.c                       # Pont manette -> clavier virtuel (evdev grab, uinput, D-Bus KWin)
│   │   ├── AGENTS.md                      # Architecture interne (souris + kbdnav)
│   │   └── Makefile                       # Compilation (3 binaires)
│   ├── gablue-apk-installer/               # Installateur APK (Python modulaire)
│   │   ├── AGENTS.md                        # Paquet, appareils, installation
│   │   ├── gablue-apk-installer            # Lanceur /usr/bin (sys.path + main)
│   │   └── gablue_apk_installer/           # Paquet : app, window, device, install, dialogs/
│   ├── gablue-isomount/                    # Sources C du monteur d'images disque
│   │   ├── gablue-isomount.c              # Programme principal (UDisks2 DBus, Dolphin)
│   │   └── Makefile                       # Compilation
│   ├── gablue-manager/                     # Sources de Gablue Manager (Python modulaire)
│   │   ├── AGENTS.md                       # Paquet, dialogues, privilèges
│   │   ├── gablue-manager                  # Lanceur /usr/bin (sys.path + main)
│   │   └── gablue_manager/                 # Paquet : app, window, dialogs/, local, ssh…
│   ├── gwine-launcher/                     # Sources du lanceur gwine (Bash modulaire)
│   │   ├── AGENTS.md                        # Architecture interne du lanceur
│   │   ├── gwine                           # Script point d'entrée
│   │   ├── build.sh                        # Assemblage du fichier standalone
│   │   ├── completions/                    # Completions bash et zsh
│   │   └── lib/                            # Bibliothèques modulaires (~60 fichiers)
│   ├── lgplaunch/                          # Sources de lgplaunch (Bash modulaire)
│   │   ├── AGENTS.md                        # Modules, montage LGP, overlays
│   │   ├── lgplaunch                       # Lanceur de développement (source lib/)
│   │   ├── build.sh                        # Assemblage lib/*.sh → standalone
│   │   └── lib/                            # Modules (core, lgp, saves, exe, modes…)
│   └── tvqt/                               # Sources de Gablue TV (Python modulaire)
│       ├── AGENTS.md                        # Paquet, vues, manettes, EPG
│       ├── tvqt                            # Lanceur /usr/bin (sys.path + main)
│       └── gablue_tv/                      # Paquet : app, window, grid, guide…
├── files/
│   ├── scripts/                           # Scripts d'installation bash
│   │   ├── AGENTS.md                      # Détail des scripts de build (copr → cpuid-fault)
│   │   ├── build-c                       # Compilation sources C
│   │   ├── build-gwine                    # Assemblage script gwine standalone
│   │   ├── cleanup                        # Nettoyage intermédiaire
│   │   ├── copr                           # Configuration dépôts COPR
│   │   ├── cpuid-fault                    # Compilation module kernel CPUID faulting
│   │   ├── finalize                       # Finalisation de l'image
│   │   ├── initramfs                      # Génération initramfs
│   │   ├── install-kmods                 # Helper installation kmods (vérification existence RPMs)
│   │   ├── kernel                        # Installation kernel OGC + akmods
│   │   ├── mesa                           # Installation Mesa Terra (multilib fc44)
│   │   ├── nvidia                         # Installation pilotes NVIDIA via akmods
│   │   ├── post-install                   # Post-installation principale
│   │   ├── rpm                            # Paquets RPM (avec libs 32-bit Wine/Proton)
│   │   └── systemd                        # Activation services systemd
│   └── system/                            # Fichiers système à copier
│       ├── AGENTS.md                      # Docs des fichiers système (/etc, usr/bin, ujust…)
│       ├── all/                           # Fichiers communs à toutes les variantes
│       │   ├── etc/xdg/                   # Configs XDG système (kwinrulesrc VRR, autostart, blacklist)
│       │   └── usr/                       # Binaires, scripts, configurations, services
│       ├── main/                          # Réservé variante main (actuellement vide)
│       └── nvidia-common/                 # Fichiers communs nvidia + nvidia-open (modprobe, udev PM, SELinux, CDI, distrobox)
├── installer/                             # Build ISO live (Containerfile payload, build.sh, hooks Titanoboa)
│   └── AGENTS.md                          # Fonctionnement du live + build local
├── local-build/                           # Build ISO local (build-iso.sh)
├── .github/
│   ├── AGENTS.md                          # Docs workflows CI
│   ├── actions/                           # Composite actions locales
│   │   └── mount-btrfs-storage/           # Montage loopback BTRFS compressé sur "/"
│   │       └── action.yml
│   ├── workflows/                         # Workflows GitHub Actions
│   │   ├── gablue-builds.yml              # Workflow principal de build
│   │   ├── reusable-gablue-image.yml      # Workflow réutilisable
│   │   ├── build-gablue-live-isos.yml     # Build des ISOs live (Titanoboa)
│   │   ├── clean-gablue-images.yml        # Nettoyage anciennes images
│   │   └── bump-image-digests.yml         # Bump hebdo digests images pinnées (PR auto)
│   └── dependabot.yml                     # Configuration Dependabot
└── README.md
```

## Build et conventions de code

Conventions transverses : scripts Bash en français (`#!/usr/bin/bash`,
`set -eoux pipefail`, indentation 4 espaces, variables entre guillemets),
Containerfile une instruction par ligne (mounts cache/bind/tmpfs, `/ctx/cleanup`
en fin de RUN), appels aux scripts via `/ctx/<script>`.

Commandes de build, vérification de l'image, conventions détaillées
(Bash/Containerfile), gestion des erreurs, tests et dépannage →
`files/scripts/AGENTS.md`.

## Messages de commit et tags

**Les messages de commit doivent être rédigés en anglais.**

Les tags dans les messages de commit déclenchent les builds :

| Tag | Images déclenchées |
|-----|-------------------|
| `[iso]` | gablue-main, gablue-main-dx, gablue-nvidia, gablue-nvidia-open, gablue-nvidia-open-dx (ISOs live) |
| `[all]` | Toutes les images |
| `[all-iso]` | Toutes les images **puis** les ISOs live automatiquement (chaînage via `workflow_run` une fois les images publiées) |
| `[main]` | gablue-main |
| `[nvidia]` | gablue-nvidia, gablue-nvidia-open |
| `[dx]` | gablue-main-dx |
| ~~`[test]`~~ | ~~gablue-main-test, gablue-nvidia-open-test~~ (désactivé — utiliser la branche `test`, voir ci-dessous) |

**Branche `test`** : un push sur `refs/heads/test` déclenche `build-main` et `build-nvidia-open` qui construisent les packages séparés `gablue-main-test` et `gablue-nvidia-open-test` (tag `latest`). Aucun tag de commit nécessaire — n'importe quel push sur la branche `test` déclenche le build. Le schedule ne build que `main` → `latest` n'est jamais pollué.

**Exemples** :
```bash
# Branche main (avec tags de commit)
git commit -m "[main] Update KDE packages"
git commit -m "[nvidia] Update NVIDIA drivers to 550"
git commit -m "[iso] Trigger live ISO build"
git commit -m "[all] Migrate to fc44 and OGC kernel"

# Branche test (pas de tag nécessaire)
git checkout test
git commit -m "test: experimental feature"
git push origin test
```

## Sécurité

### Clés et signatures

- **cosign.pub** : Clé publique pour vérification des images
- **gablue-kmod.der** : Certificat Secure Boot pour les modules kernel customs (signé par `GABLUE_KMOD_KEY`, enrollé via `ujust secureboot`)
- **gablue-secure-boot.der** : Certificat Secure Boot pour les kmods ublue-os (enrollé via `ujust secureboot`)
- Ne jamais commiter les clés privées (`cosign.key`, `gablue-kmod.key`)
- Les images sont signées automatiquement dans le workflow

### Bonnes pratiques

- Secrets par variables d'environnement, jamais en clair ; ne jamais commiter de clé privée
- Vérifier les signatures des dépôts ajoutés et désactiver chaque dépôt après installation ; `|| true` réservé aux commandes optionnelles
- Limiter les permissions des fichiers exécutables ; actions CI épinglées par SHA → `.github/AGENTS.md`

### SELinux

- Modules personnalisés compilés depuis `.te` dans post-install
- Module NVIDIA container installé par nvidia-install.sh (`nvidia-container.pp` dans `files/system/nvidia-common/`)
- Configuration pour les conteneurs avec accès GPU

## Langue et internationalisation

- **Documentation** : Français
- **Commentaires de code** : Français
- **Messages utilisateur** : Français (alias, scripts, etc.)
- **Variables** : Anglais ou français cohérent
- **Commits** : Anglais (avec tags obligatoires)

## Ressources et liens

- **Universal Blue** : https://universal-blue.org/
- **Bazzite** : https://github.com/ublue-os/bazzite
- **Fedora Kinoite** : https://fedoraproject.org/kinoite/
- **Terra** : https://github.com/terrapkg
- **Documentation uBlue** : https://docs.universal-blue.org/
- **RetroPlayer** : https://github.com/elgabo86/retroplayer

## Architecture documentaire (AGENTS.md)

OpenCode ne charge au démarrage que le AGENTS.md global + racine ; les
AGENTS.md des sous-dossiers sont chargés automatiquement quand l'agent
lit/travaille dans ces dossiers. La racine = contrat global ; le détail par
domaine vit dans le sous-dossier concerné. Zéro duplication entre docs.

**RÈGLE** : chaque AGENTS.md (racine et sous-dossiers) DOIT être mis à jour
avant chaque commit qui modifie ce qu'il couvre. Ne jamais committer sans
vérifier que la doc du domaine reflète l'état exact. Un domaine qui grossit →
son propre AGENTS.md de sous-dossier + pointeur dans l'index ci-dessous.

### Index des documents

| Domaine | Document | À lire AVANT de... |
|---------|----------|--------------------|
| Scripts de build + conventions (Bash, Containerfile), build/vérification/dépannage | `files/scripts/AGENTS.md` | modifier `files/scripts/`, le `Containerfile-gablue` ou une étape RUN |
| Fichiers système (/etc, /usr/bin, binfmt, ujust, plasmoid) | `files/system/AGENTS.md` | modifier `files/system/` |
| Workflows CI | `.github/AGENTS.md` | modifier `.github/` ou un tag de commit |
| ISO live / installateur | `installer/AGENTS.md` | modifier `installer/` ou `local-build/` |
| Lanceur gwine | `src/gwine-launcher/AGENTS.md` | modifier `src/gwine-launcher/` |
| Gablue Manager (GUI, paquet `gablue_manager`) | `src/gablue-manager/AGENTS.md` | modifier `src/gablue-manager/` ou un dialogue |
| TV Gablue (tvqt, paquet `gablue_tv`) | `src/tvqt/AGENTS.md` | modifier `src/tvqt/` ou une vue |
| Installateur APK (paquet `gablue_apk_installer`) | `src/gablue-apk-installer/AGENTS.md` | modifier `src/gablue-apk-installer/` |
| lgplaunch (modules Bash `lib/`) | `src/lgplaunch/AGENTS.md` | modifier `src/lgplaunch/` |
| Manette (gamepadshortcuts, mouse, kbdnav) | `src/gamepadshortcuts/AGENTS.md` | modifier `src/gamepadshortcuts/` |

Changements suivant la racine :
- Ajout d'une nouvelle variante d'image
- Modification de la structure du projet ou des conventions transverses
- Changement des dépôts ou sources, ajout de nouvelles conventions
- Nouveau/déplacement d'un AGENTS.md de sous-dossier (index ci-dessus)
