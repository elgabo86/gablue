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

## Variantes d'images

Le projet construit 6 variantes distinctes :

| Variante | Description | Kernel | NVIDIA | Trigger tag |
|----------|-------------|--------|--------|-------------|
| `gablue-main` | Image standard sans NVIDIA | OGC | - | `[main]`, `[all]` |
| `gablue-nvidia` | Pilotes NVIDIA closed-source | OGC | nvidia-lts | `[nvidia]`, `[all]` |
| `gablue-nvidia-open` | Pilotes NVIDIA open-source | OGC | nvidia-open | `[nvidia]`, `[all]` |
| `gablue-main-dx` | Mode développement (DX) avec virtualisation + ROCm | OGC | - | `[dx]`, `[all]` |
| `gablue-main-test` | Image de test avec OpenGamepadUI (fc44) | OGC | - | `[test]`, `[all]` |
| `gablue-nvidia-open-test` | Test NVIDIA Open avec OpenGamepadUI (fc44) | OGC | nvidia-open | `[test]`, `[nvidia]`, `[all]` |
| `gablue-nvidia-open-dx` | Mode DX NVIDIA Open (virtualisation + GPU NVIDIA) | OGC | nvidia-open | `[dx]`, `[nvidia]`, `[all]` |

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

**Stable vs Test** :
- **Containerfile** : Stable utilise `Containerfile-gablue` (unique pour toutes les variantes stables), Test utilise `Containerfile-gablue-test` et `Containerfile-gablue-nvidia-open-test`
- **Scripts** : Stable utilise les scripts sans suffixe (`kernel`, `copr`, `mesa`, etc.), Test utilise les scripts `-test`
- **OpenGamepadUI** : Interface gaming expérimentale style Steam Deck (test uniquement)
- **Paquets OGUI** : `opengamepadui`, `gamescope-session-opengamepadui`, `powerstation`, `inputplumber` (test uniquement)
- **Fichiers système test** : les scripts `-test` ajoutent leurs spécificités directement (sans dossier `files/system/test/` dédié)

## Structure du projet

```
.
├── Containerfile-gablue                   # Containerfile principal (toutes variantes stables)
├── Containerfile-gablue-test              # Containerfile pour main-test
├── Containerfile-gablue-nvidia-open-test  # Containerfile pour nvidia-open-test
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
│   │   └── Makefile                       # Compilation
│   ├── gablue-isomount/                    # Sources C du monteur d'images disque
│   │   ├── gablue-isomount.c              # Programme principal (UDisks2 DBus, Dolphin)
│   │   └── Makefile                       # Compilation
│   └── gwine-launcher/                     # Sources du lanceur gwine (Bash modulaire)
│       ├── gwine                           # Script point d'entrée
│       ├── build.sh                        # Assemblage du fichier standalone
│       ├── completions/                    # Completions bash et zsh
│       └── lib/                            # Bibliothèques modulaires (~60 fichiers)
├── files/
│   ├── scripts/                           # Scripts d'installation bash
│   │   ├── brew                           # Installation Homebrew
│   │   ├── build-c                       # Compilation sources C
│   │   ├── build-gwine                    # Assemblage script gwine standalone
│   │   ├── cleanup                        # Nettoyage intermédiaire
│   │   ├── copr                           # Configuration dépôts COPR
 │   │   ├── copr-test                      # Configuration dépôts COPR (test)
 │   │   ├── cpuid-fault                    # Compilation module kernel CPUID faulting
 │   │   ├── finalize                       # Finalisation de l'image
│   │   ├── initramfs                      # Génération initramfs
│   │   ├── install-kmods                 # Helper installation kmods (vérification existence RPMs)
│   │   ├── kernel                        # Installation kernel OGC + akmods
│   │   ├── kernel-test                    # Installation kernel OGC (test)
│   │   ├── mesa                           # Installation Mesa Terra (multilib fc44)
│   │   ├── mesa-test                      # Installation Mesa Terra (test)
│   │   ├── nvidia                         # Installation pilotes NVIDIA via akmods
│   │   ├── nvidia-test                    # Installation pilotes NVIDIA (test, wrapper)
│   │   ├── post-install                   # Post-installation principale
│   │   ├── post-install-test              # Post-installation test (wrapper)
│   │   ├── rpm                            # Paquets RPM (avec libs 32-bit Wine/Proton)
│   │   ├── rpm-test                       # Paquets RPM (test)
│   │   ├── systemd                        # Activation services systemd
│   │   └── systemd-test                   # Activation services (test)
│   └── system/                            # Fichiers système à copier
│       ├── all/                           # Fichiers communs à toutes les variantes
│       │   ├── etc/xdg/                   # Configs XDG système (kwinrulesrc VRR, autostart, blacklist)
│       │   └── usr/                       # Binaires, scripts, configurations, services
│       ├── main/                          # Réservé variante main (actuellement vide)
│       └── nvidia-common/                 # Fichiers communs nvidia + nvidia-open (modprobe, SELinux, CDI, distrobox)
├── .github/
│   ├── actions/                           # Composite actions locales
│   │   └── mount-btrfs-storage/           # Montage loopback BTRFS compressé sur "/"
│   │       └── action.yml
│   ├── workflows/                         # Workflows GitHub Actions
│   │   ├── gablue-builds.yml              # Workflow principal de build
│   │   ├── reusable-gablue-image.yml      # Workflow réutilisable
│   │   ├── build-gablue-live-isos.yml     # Build des ISOs live (Titanoboa)
│   │   └── clean-gablue-images.yml        # Nettoyage anciennes images
│   └── dependabot.yml                     # Configuration Dependabot
└── README.md
```

## Commandes de build

### Build local complet

```bash
# Build de l'image principale (main)
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --tag gablue-main .

# Build de l'image NVIDIA (closed)
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="nvidia" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --build-arg NVIDIA_FLAVOR="nvidia-lts" \
  --tag gablue-nvidia .

# Build de l'image NVIDIA Open
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="nvidia-open" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --build-arg NVIDIA_FLAVOR="nvidia-open" \
  --tag gablue-nvidia-open .

# Build de l'image DX (développement)
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --build-arg DX_MODE="true" \
  --tag gablue-main-dx .

# Build de l'image test (main-test)
sudo buildah build \
  --file Containerfile-gablue-test \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --tag gablue-main-test .

# Build de l'image nvidia-open-test
sudo buildah build \
  --file Containerfile-gablue-nvidia-open-test \
  --format "docker" \
  --build-arg VARIANT="nvidia-open" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --tag gablue-nvidia-open-test .
```

### Vérification de l'image construite

```bash
# Lister les images
podman images | grep gablue

# Tester l'image interactivement
podman run -it gablue-main /bin/bash

# Vérifier le contenu
podman run gablue-main cat /usr/lib/os-release
```

## Conventions de code

### Scripts Bash (files/scripts/)

Tous les scripts doivent suivre ces règles strictes :

**En-tête obligatoire** :
```bash
#!/usr/bin/bash

# Description du script en français
# Ce script effectue [description détaillée de la fonction]

set -eoux pipefail
```

**Options strictes** :
- `set -e` : Arrêt immédiat sur erreur
- `set -o` : Mode strict pour variables non définies
- `set -u` : Erreur sur variable non définie
- `set -x` : Mode debug (affichage des commandes)
- `pipefail` : Échec si une commande du pipeline échoue

**Style de code** :
- **Indentation** : 4 espaces (pas de tabulations)
- **Variables** : UPPER_CASE pour les variables d'environnement, snake_case pour les locales
- **Guillemets** : Toujours doubler les variables : `"$VARIABLE"`
- **Commentaires** : En français, avec sections délimitées

**Structure recommandée** :
```bash
#!/usr/bin/bash

# Description du script
# Objectif et détails du fonctionnement

set -eoux pipefail

# =============================================================================
# SECTION 1 : PRÉPARATION
# =============================================================================

# Code ici

# =============================================================================
# SECTION 2 : INSTALLATION
# =============================================================================

# Code ici
```

### Containerfiles

**Principes généraux** :
- Une instruction par ligne
- Commentaires explicatifs pour chaque étape
- Ordre optimal pour le cache Docker (du moins changeant au plus changeant)
- Multi-stage pour les dépendances externes

**Pattern standard (stable)** :
```dockerfile
# Arguments de build
ARG VARIANT
ARG SOURCE_IMAGE
ARG FEDORA_VERSION
ARG KERNEL_FLAVOR
ARG KERNEL_VERSION
ARG NVIDIA_FLAVOR="nvidia-open"
ARG DX_MODE

# Étape intermédiaire : scripts de build (bind-mountés, jamais dans l'image finale)
FROM scratch AS ctx
COPY files/scripts /

# Étapes intermédiaires : akmods
FROM ghcr.io/ublue-os/akmods:${KERNEL_FLAVOR}-${FEDORA_VERSION}-${KERNEL_VERSION} AS akmods
FROM ghcr.io/ublue-os/akmods-extra:${KERNEL_FLAVOR}-${FEDORA_VERSION}-${KERNEL_VERSION} AS akmods-extra
FROM ghcr.io/ublue-os/akmods-${NVIDIA_FLAVOR}:${KERNEL_FLAVOR}-${FEDORA_VERSION}-${KERNEL_VERSION} AS akmods-nvidia

# Étape intermédiaire : fichiers NVIDIA communs (bind-mountés dans le RUN nvidia)
FROM scratch AS nvidia-common-files
COPY files/system/nvidia-common /

# Image de base
FROM quay.io/fedora-ostree-desktops/${SOURCE_IMAGE}:${FEDORA_VERSION}

# Redéfinition des arguments après FROM
ARG VARIANT
ARG SOURCE_IMAGE
ARG DX_MODE
ARG KERNEL_FLAVOR
ARG KERNEL_VERSION

# Copie des fichiers système communs (les scripts sont bind-mountés, pas copiés)
COPY files/system/all /

# Variables d'environnement
ENV VARIANT=${VARIANT}
ENV SOURCE_IMAGE=${SOURCE_IMAGE}
ENV DX_MODE=${DX_MODE}
ENV KERNEL_FLAVOR=${KERNEL_FLAVOR}
ENV KERNEL_VERSION=${KERNEL_VERSION}

# Configuration des dépôts (avant kernel pour les dépendances des kmods)
RUN --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=tmpfs,dst=/tmp \
    /ctx/copr && \
    /ctx/cleanup

# Installation du kernel avec akmods
RUN --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=bind,from=akmods,src=/kernel-rpms,dst=/tmp/kernel-rpms \
    --mount=type=bind,from=akmods,src=/rpms/common,dst=/tmp/rpms/common \
    --mount=type=bind,from=akmods,src=/rpms/kmods,dst=/tmp/rpms/kmods \
    --mount=type=bind,from=akmods,src=/rpms/ublue-os,dst=/tmp/rpms/ublue-os \
    --mount=type=bind,from=akmods-extra,src=/rpms/extra,dst=/tmp/rpms/extra \
    --mount=type=bind,from=akmods-extra,src=/rpms/kmods,dst=/tmp/rpms/kmods-extra \
    --mount=type=tmpfs,dst=/tmp \
    /ctx/kernel && \
    /ctx/cleanup

# Installation NVIDIA (conditionnel)
RUN --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=bind,from=akmods-nvidia,src=/rpms,dst=/tmp/rpms/nvidia \
    --mount=type=bind,from=nvidia-common-files,src=/,dst=/tmp/nvidia-files \
    --mount=type=tmpfs,dst=/tmp \
    if [ "$VARIANT" = "nvidia" ] || [ "$VARIANT" = "nvidia-open" ]; then \
        cp -r /tmp/nvidia-files/* / && \
        /ctx/nvidia; \
    fi && \
    /ctx/cleanup
```

**Bonnes pratiques RUN** :
- Utiliser `--mount=type=cache` pour `/var/cache` et `/var/log` (cache DNF persistant entre builds)
- Utiliser `--mount=type=bind,from=ctx,source=/,target=/ctx` pour accéder aux scripts sans les inclure dans l'image
- Utiliser `--mount=type=bind,from=stage` pour accéder aux étapes intermédiaires (akmods)
- Utiliser `--mount=type=tmpfs,dst=/tmp` pour éviter que les fichiers temporaires ne touchent le layer
- Chaîner les commandes avec `&&` pour réduire les layers
- Terminer par `/ctx/cleanup` pour nettoyer
- Appeler les scripts directement (`/ctx/script`) sans `sh`

### Justfile (60-custom.just)

**Format** :
```just
# Description de la commande
command-name:
    #!/usr/bin/bash
    echo "Hello World"
```

**Conventions** :
- Nommage en kebab-case
- Shebang obligatoire
- Description sur une ligne avant la commande
- Indentation avec 4 espaces

**Gestion des subvolumes BTRFS** :
- `btrfs filesystem defrag -r` ne traverse pas les limites de subvolumes — utiliser `findmnt -t btrfs` filtré par UUID pour lister les points de montage individuels de chaque subvolume
- En fallback (disques externes où les subvolumes ne sont pas montés séparément), utiliser `sudo btrfs subvolume list` et reconstruire les chemins
- Exclusions communes aux deux commandes : `.beeshome` (metadata BEES), `root*` (ostree système, reflinks), `*.snapshots` (snapper, reflinks)
- `btrfs-compress-defrag` exclut en plus `/var` et `var*` (risque reflinks Docker/Podman) — `btrfs-compress` (property set, safe) ne les exclut pas
- Affichage des filesystems par label (`findmnt -o TARGET,UUID,LABEL`) quand disponible
- Parsing de la propriété compression via `cut -d= -f2` (car `btrfs property get` renvoie `compression=valeur`)
- Utiliser `mapfile -t` pour lire les subvolumes dans un tableau

**Complétion bash** :
- `files/system/all/usr/share/bash-completion/completions/ujust` : surcharge la complétion buggy du paquet `ublue-os-just` (qui n'enregistrait jamais la complétion pour `ujust`)
- Génère dynamiquement la liste des recettes via `ujust --summary`

## Gestion des erreurs

### Commandes critiques vs optionnelles

```bash
# Commande critique - doit réussir (arrêt si échec)
dnf5 -y install package

# Commande optionnelle - peut échouer sans bloquer
test -f /etc/fichier && rm /etc/fichier || true
```

### Patterns conditionnels

**Selon la variante** :
```bash
if [ "$VARIANT" == "nvidia" ]; then
    dnf5 -y install nvidia-driver
fi

if [ "$VARIANT" == "main" ]; then
    dnf5 -y install radeontop
fi
```

**Selon l'image source** :
```bash
if [ "$SOURCE_IMAGE" == "kinoite" ]; then
    dnf5 -y install okular gwenview kcalc
fi
```

**Selon le mode DX** :
```bash
if [ "${DX_MODE:-false}" == "true" ]; then
    dnf5 -y install docker-ce docker-ce-cli
fi
```

### Gestion des dépôts

**Activation** :
```bash
for copr in repo1 repo2 repo3; do
    dnf5 -y copr enable $copr
done && unset -v copr
```

**Désactivation après installation** :
```bash
for copr in repo1 repo2 repo3; do
    dnf5 -y copr disable $copr
done && unset -v copr
```

## Scripts de build détaillés

### 1. copr - Configuration des dépôts

Configure tous les dépôts tiers nécessaires :
- **keepcache=1** : Activé au début pour que le cache DNF persiste entre builds (désactivé dans finalize)
- **COPR** : bazzite-org/bazzite, bazzite-org/bazzite-multilib, ublue-os/staging, ublue-os/packages, che/nerd-fonts, hikariknight/looking-glass-kvmfr, lizardbyte/beta, bazzite-org/rom-properties
- **Tiers** : Tailscale, Negativo17
- **Terra (FyraLabs)** : terra-release, terra-release-extras, terra-release-mesa

Exclusions importantes :
- Mesa et kernel des dépôts Fedora (fournis par Terra)
- Exclusions bazzite : pipewire-*, bluez-*, xorg-x11-server-Xwayland, wireplumber-* (alignement i686/x86_64 fc44)
- Priorité Terra = 3 (haute)

### 2. kernel - Installation du kernel OGC + akmods

**kernel (stable)** : Installation du kernel OGC et des akmods depuis ublue-os
- Récupération depuis `ghcr.io/ublue-os/akmods` et `ghcr.io/ublue-os/akmods-extra`
- Installation du kernel depuis `/tmp/kernel-rpms/`
- Utilisation du helper `/ctx/install-kmods` qui vérifie l'existence de chaque RPM avant installation (évite les échecs si un module n'est plus présent dans l'image akmods)
- Kmods communs (via install-kmods) : framework-laptop, openrazer, v4l2loopback, xone, xpadneo
- Kmods extras (via install-kmods) : zenergy, gcadapter, evdi, new-lg4ff, t150-driver, hid-fanatecff, sc0710, system76
- Versionlock pour verrouiller les versions
- Installation de scx-scheds depuis COPR bieszczaders/kernel-cachyos-addons

**kernel-test (test)** : Identique au stable

### 3. mesa - Installation Mesa Terra (multilib fc44)

**mesa (stable)** : Swap Mesa vers la version Terra optimisée
- Swap de `mesa-filesystem` vers terra-mesa
- Installation x86_64 : dri-drivers, libEGL, libGL, libgbm, vulkan-drivers
- Installation i686 : dri-drivers, libEGL, libGL, libgbm, vulkan-drivers
- Terra fc44 : les fichiers `LICENSE.dependencies` sont nommés par arch (`.i386` / `.x86_64`), pas de conflit
- Versionlock des paquets Mesa

**mesa-test (test)** : Identique au stable

### 4. nvidia - Installation pilotes NVIDIA via akmods

**nvidia (stable)** : Installation via `nvidia-install.sh` de ublue-os
- Suppression de `nvidia-gpu-firmware` (conflit avec pilotes propriétaires)
- Activation terra-mesa pour egl-wayland + Mesa i686
- Installation EGL Wayland (32 et 64 bits)
- Appel de `nvidia-install.sh` avec `AKMODNV_PATH="/tmp/rpms/nvidia"`, `MULTILIB=1`, `IMAGE_NAME="$SOURCE_IMAGE"`
- nvidia-install.sh gère : driver, kmod, container-toolkit, supergfxctl, SELinux, dracut force_drivers, staging COPR
- Configuration post-installation : suppression ICD Nouveau, symlink libnvidia-ml, disable supergfxd
- **VK_hdr_layer** pour pilotes closed uniquement (pas nvidia-open) : extraction manuelle du RPM
- **nvidia-modeset.conf** : copie de `/etc/modprobe.d/` vers `/usr/lib/modprobe.d/` (workaround Dracut, avec vérification `[ -f ]`) pour pilotes closed, les pilotes open n'ont pas ce fichier
- Désactivation terra-mesa après installation

**nvidia-test (test)** : Wrapper appelant `sh /ctx/nvidia` (même pattern que `post-install-test` → `post-install`)

### 5. rpm - Paquets RPM

Installation extensive de paquets organisée par catégories :
- **CLI** : fswatch, btop, fastfetch, git, atuin, tldr, amdsmi, jq, zoxide, etc.
- **Réseau** : tailscale, rar
- **Multimédia** : yt-dlp
- **Virtualisation (toutes variantes)** : `qemu-guest-agent` (agent invité QEMU) — léger (~300 Ko), inoffensif sur bare metal (service lié au device virtio-serial `org.qemu.guest_agent.0`, ne démarre pas si absent), utile en VM (IP dans virt-manager, graceful shutdown, snapshots consistants). Installé sur toutes les variantes pour que les ISOs live fonctionnent dans libvirt. Activé par preset Fedora (`enable qemu-guest-agent.service`)
- **Virtualisation (DX uniquement)** : docker-ce, libvirt, virt-manager, virt-viewer, virt-install, swtpm, guestfs-tools, python3-libguestfs, qemu-kvm-core, qemu-system-ppc/m68k/arm/aarch64-core (émulation rétro), spice-server, et modules QEMU modulaires externes (display qxl/virtio-gpu/virtio-vga, audio spice/pipewire/pa/alsa, usb host/redirect/smartcard, ui spice/gtk/opengl/egl/dbus, char-spice) — les modules virtio/net/pci/vfio sont compilés statiquement dans qemu-system-x86-core (tiré par qemu-kvm-core), requis car `--setopt=install_weak_deps=False` empêche l'auto-install des modules externes en dépendances faibles. `python3-libguestfs` est requis explicitement (paquet Optional du groupe Fedora `Virtualization`) : virt-manager importe le module `guestfs` dans `inspection.py` pour l'inspection des VMs ; sans lui, l'import échoue en silencieux (try/except) et l'inspection/resize via libguestfs est désactivée
- **Terminal fun** : asciiquarium, cmatrix
- **Gaming** : sunshine, mangohud, gamescope
- **BTRFS** : snapper, btrfs-assistant (snapshots et maintenance, non activés par défaut)
- **KDE** : okular, gwenview, kcalc, yakuake (Kinoite uniquement)
- **Polices** : nerd-fonts
- **Runtime** : patch, bzip2, sqlite, uv
- **Python (scripts Gablue)** : python3-evdev, python3-uinput, python3-pyside6
- **SELinux** : checkpolicy, selinux-policy-devel
- **Libs 32-bit Wine/Proton complètes** : fontconfig, freetype, X11 (composite, cursor, damage, fix, i, inerama, randr, render, tst, v), Wayland (epoxy, decor, cursor, egl), core (gnutls, unwind, cups, openldap), audio (pulseaudio, pipewire upgrade + libs, FAudio, alsa, openal, ogg, vorbis, flac, sndfile), vulkan-loader (terra-mesa), vidéo (libva, libvdpau)

**Upgrade initial restreint** (toutes variantes) :
- `dnf5 -y upgrade --refresh --repo=fedora --repo=updates` **avant** toute installation
- L'image Kinoite de base peut avoir jusqu'à 48h de retard sur les mises à jour Fedora
- Restreint aux dépôts officiels uniquement : les exclusions `mesa-*` et `kernel-*` du copr protègent Mesa et kernel, NVIDIA vient des RPMs akmods externes
- Exécuté avant le versionlock de plasma-desktop pour que ce dernier soit déjà à jour avant d'être verrouillé

Paquets supprimés :
- firefox, firefox-langpacks, htop
- plasma-welcome-fedora, plasma-welcome
- plasma-discover-rpm-ostree (Kinoite)

**rpm-test** :
- Wrapper (identique à rpm, prévu pour ajouter des paquets spécifiques au test)

### 5a. pypi - Packages Python sans équivalent RPM

**pypi** (appelé après rpm) :
- Installe `terminaltexteffects` depuis PyPI via `uv` (pas d'équivalent RPM Fedora)
- **Conflit site-packages** : un RPM peut créer un fichier ou symlink à n'importe quel niveau du chemin `/usr/local/lib/python3.14/site-packages/` au lieu d'un répertoire (ex: transition majeure Python), ce qui bloque `uv pip install` (`File exists, os error 17`). Le script remonte la hiérarchie et supprime tout ce qui n'est pas un répertoire, puis crée l'arborescence avec `mkdir -p` avant d'installer

### 5b. build-c / build-gwine - Compilation des sources

**build-c** (appelé après pypi) :
- Compile les binaires C depuis `/src/gamepadshortcuts`, `/src/ds2xbox`, `/src/gablue-isomount`
- Installation via `make -C <dir> install DESTDIR=`
- Nettoie les sources après compilation (`rm -rf /src/<dir>`)
- Désinstalle `dbus-devel` après compilation (inutile dans l'image finale)

**build-gwine** (appelé après build-c) :
- Assemble le script gwine standalone depuis `/src/gwine-launcher/`
- Exécute `build.sh` qui concatène les ~60 fichiers modulaires `lib/` en un script unique
- Embarque les shims overlayfs (`composefs_statfs_shim.so` 32/64 bits) en base64 pour extraction au runtime
- Installe `/usr/bin/gwine` et les completions bash/zsh
- Nettoie les sources après assemblage
- **IMPORTANT** : toute modification des fichiers `lib/` (ex. `lib/modes/init-main.sh`, `lib/runner.sh`) nécessite un rebuild de l'image pour être effective. Le gwine assemblé est celui utilisé par `build.sh` pour générer le pack cache de l'ISO → l'image doit être reconstruite **avant** l'ISO
- Voir `src/gwine-launcher/AGENTS.md` pour l'architecture interne du lanceur

### 6. post-install / post-install-test

Configuration post-installation étendue :
- Permissions des exécutables (chmod +x)
- Capacités système (setcap pour gamescope)
- Compilation modules SELinux personnalisés (.te -> .pp)
- Binaires externes (retroplayer, zxtune)
- Branding Gablue (os-release)
- Configuration système (tuned, bluetooth, pipewire, timers)
- Désactivation des dépôts
- Nettoyage des fichiers .desktop
- Configuration DX (iptables, NetworkManager)
- MIME par défaut (Windows.desktop, Linux.desktop)
- **Mises à jour automatiques** : active `AutomaticUpdatePolicy=stage` dans `/etc/rpm-ostreed.conf` (copie depuis `/usr/share/ublue-os/update-services/etc/rpm-ostreed.conf` fourni par le RPM `ublue-os-update-services`) et reprogramme les timers flatpak + rpm-ostree le samedi à 04:00 avec `RandomizedDelaySec=10m`
- **Linuxbrew** : ajoute `/home/linuxbrew/.linuxbrew/bin` au `secure_path` de sudo

**post-install-test** ajoute :
- Permissions pour scripts OpenGamepadUI (steamos-session-select, gwine-plugin) — wrapper appelant d'abord `post-install`

**Correction composefs** (dans post-install, toutes variantes) :
- Compile un LD_PRELOAD minimal (`gablue-composefs-fix.so`, ~2.6 Ko) qui intercepte `statfs`/`statfs64`
- Corrige l'affichage de l'espace libre dans Dolphin sur les systèmes composefs (Fedora Kinoite 42+)
- L'overlay composefs en `/` rapporte 0 blocs libres, le hook redirige `/`, `/home` et `/home/*` vers `/var/home` (btrfs)
- Injection via `sed` dans le `.desktop` Dolphin (`Exec=env LD_PRELOAD=...`)
- Sources dans `src/composefs-fix/`

**Correction plasmalogin settle udev** (TEMPORAIRE, toutes variantes) :
- **Problème** : plasmalogin.service exécute `udevadm settle --timeout=10` avant de démarrer le greeter, ce qui provoque un écran noir de 10s sur certaines cartes mères (queue udev jamais vide)
- **Fix** : drop-in `plasmalogin.service.d/90-gablue-settle.conf` remplace le settle aveugle par `/usr/libexec/gablue-wait-devices`
- **Script** : attend que `/dev/dri/card*` et `/dev/input/event*` existent, puis 1s de délai fixe pour les docks/HID lents (timeout global 5s)
- **Impact** : passe de ~10.2s à ~1.1s sur les systèmes affectés (fixe upstream KDE `a8c752fe` trop conservateur)
- **À SUPPRIMER** quand Fedora/KDE réduit le timeout du settle natif ou adopte une approche plus ciblée
- Fichiers : `files/system/all/usr/libexec/gablue-wait-devices`, `files/system/all/usr/lib/systemd/system/plasmalogin.service.d/90-gablue-settle.conf`

**Wrapper swap-session Plasma Bigscreen** (toutes variantes) :
- Remplace `/usr/bin/plasma-bigscreen-swap-session` par `gablue-bigscreen-swap-session` (script C++ bigscreen appelle ce binaire directement via QProcess)
- **Aller (swap_to_bigscreen)** :
  - Sauvegarde l'environnement et les settings KWin (`BorderlessMaximizedWindows`, `Placement`, `NoPlugin`)
  - Source `plasma-bigscreen-common-env` pour charger les variables bigscreen
  - Écrit les settings KWin bigscreen dans `~/.config/kwinrc` (fenêtres maximisées, sans décorations, pas de plugin déco)
  - Détecte les écrans connectés et met tous les secondaires en miroir sur le principal via `kscreen-doctor output.X.mirror.Y` (bigscreen ne gère pas le multi-écran)
  - Lance l'inputhandler via `kioclient exec` (mécanisme KDE natif pour les permissions Wayland `X-KDE-Wayland-Interfaces`)
  - Remplace plasmashell (`plasmashell --replace`)
  - Après 2s (bigscreen chargé) : maximise toutes les fenêtres existantes et retire leurs décorations via un script KWin 6 (`workspace.stackingOrder`, `frameGeometry = MaximizeFullArea`, `noBorder = true`)
- **Retour (swap_to_default)** :
  - Tue l'inputhandler (SIGTERM puis SIGKILL)
  - Restaure les settings KWin originaux (supprime les clés si elles n'existaient pas avant)
  - Relance `plasma-xwaylandvideobridge.service` (tué par bigscreen dans `HomeScreen.qml` — hack qui cassait X11 au retour)
  - Restaure l'environnement sauvegardé (dont `XDG_CONFIG_DIRS`)
  - Reconfigure KWin (`qdbus reconfigure`) pour reprendre la config Plasma normale
  - Annule le mirroring écrans (`output.X.mirror.none`)
  - Remplace plasmashell
  - Après 2s : restaure les décorations et dé-maximise les fenêtres via un script KWin 6 (`noBorder = false`, `frameGeometry` à 80% centré)
- Détection automatique du mode : basée sur `pgrep -f plasma-bigscreen-inputhandler` (plus fiable que la variable d'env `PLASMA_BIGSCREEN_LAUNCH_REASON` qui n'est pas héritée via KLauncher)
- Fichiers : `files/system/all/usr/bin/gablue-bigscreen-swap-session`, `files/system/all/usr/share/gablue/kwin-maximize-all.js`, `files/system/all/usr/share/gablue/kwin-restore-windows.js`

**Initialisation session native Bigscreen** (toutes variantes) :
- Autostart KDE (`/etc/xdg/autostart/gablue-bigscreen-session.desktop`) qui exécute `gablue-bigscreen-session-init` au login (s'exécute dans toutes les sessions Plasma)
- Détection de la session bigscreen native via `PLASMA_PLATFORM=mediacenter` (variable sourcée par `plasma-bigscreen-common-env` au démarrage de la session native, avant l'autostart)
- **En session Plasma normale** (non bigscreen) : si un fichier `~/.cache/plasma-bigscreen/kscreen-mirrored.txt` existe (mirroring résiduel d'une session bigscreen précédente), annule le mirroring de chaque output listé via `kscreen-doctor output.X.mirror.none` puis supprime le fichier. Attend l'initialisation de KWin avant d'agir
- **En session bigscreen native** :
  - Création du symlink blacklist (`~/.config/applications-blacklistrc` -> `/etc/xdg/applications-blacklistrc`) pour cacher les entrées inutiles dans le menu bigscreen
  - Attente de l'initialisation KWin (boucle `kscreen-doctor --json`, max 10s)
  - Mirroring de tous les écrans secondaires sur le principal via `kscreen-doctor output.X.mirror.Y` (bigscreen ne gère pas le multi-écran)
  - Sauvegarde des noms des outputs mirrorés dans `kscreen-mirrored.txt` (fichier partagé avec `gablue-bigscreen-swap-session`) pour permettre la restauration au prochain login Plasma
- Note : les actions spécifiques au swap (minimiser/restaurer les fenêtres, sauvegarder/restaurer KWin, gérer l'inputhandler) restent uniquement dans `gablue-bigscreen-swap-session`
- Fichiers : `files/system/all/usr/libexec/gablue-bigscreen-session-init`, `files/system/all/etc/xdg/autostart/gablue-bigscreen-session.desktop`

### 7. systemd / systemd-test

Activation/désactivation des services systemd :
- **Activés (toutes variantes)** : rpm-ostreed-automatic, flatpak-update, cec-poweroff-tv, cec-active-source, dmemcg-booster
- **Désactivés** : scx_loader, tailscaled, displaylink
- **Masqués** : systemd-remount-fs, flatpak-add-fedora-repos (empêche la réactivation du remote Fedora Flatpak au premier boot ; ce service natif du paquet `flatpak` réajoute `fedora`/`fedora-testing` tant que `/var/lib/flatpak/.fedora-initialized` n'existe pas, annulant le `disable-fedora-flatpak.ks` du kickstart. On garde uniquement Flathub, fourni par `/etc/flatpak/remotes.d/flathub.flatpakrepo`)
- **Conditionnels (DX)** : ublue-os-libvirt-workarounds, gablue-dx-groups, incus-workaround

**systemd-test** ajoute :
- Services OpenGamepadUI désactivés : inputplumber, powerstation
- Note: opengamepadui-session.service n'est PAS activé par défaut

### 8. initramfs

Génération de l'initramfs avec dracut :
- Détection de la version kernel installée via dnf5 repoquery
- Génération avec options ostree et fido2
- Permissions sécurisées (0600)

### 9. cleanup / finalize

**cleanup** (appelé après chaque étape RUN) :
- Suppression de `/tmp/*`, `/var/log/dnf5.log`, `/boot/*`
- PAS de `dnf5 clean all` (le cache mount `/var/cache` n'entre pas dans l'image, le nettoyer détruirait le cache DNF persistant)
- PAS de `ostree container commit` (inutile avec bootc/rechunk)

**finalize** (appelé une seule fois à la fin) :
- `dnf5 config-manager setopt keepcache=0` (désactive le keepcache activé dans copr)
- Nettoyage de `/var/*` sauf le répertoire cache
- Migration des utilisateurs/groupes vers `/usr/lib/passwd` et `/usr/lib/group`
- Nettoyage des fichiers de verrou et de `/usr/etc`
- PAS de `ostree container commit` (le rechunk dans le workflow s'en occupe)

### 10. cpuid-fault - Module kernel CPUID faulting

Compile le module `cpuid_fault_emulation` (source dans `src/cpuid-fault/`).
Ce module émule le CPUID faulting sur les CPU AMD sans support natif (AM4, Steam Deck).
Sur les CPU avec support natif (Intel 4th gen+, AMD Ryzen 7000+), le module n'est pas
nécessaire — le kernel gère le CPUID faulting via `ARCH_SET_CPUID` nativement.

- Compilation via `make -C /usr/src/kernels/${KVER} M=/src/cpuid-fault modules`
- Le module est **AMD SVM uniquement** (instructions AMD-V), il sera ignoré sur Intel
- Signature Secure Boot via `/run/secrets/gablue-kmod-key` (monté depuis le secret CI `GABLUE_KMOD_KEY`)
- Si la clé n'est pas disponible (build local), le module est compilé mais non signé
- Le certificat public `gablue-kmod.der` est installé dans `/etc/pki/akmods/certs/`
- **Conflit avec KVM** : le module utilise AMD-V, KVM doit être déchargé avant chargement
- Géré par les commandes `ujust cpuid-emu-on` / `ujust cpuid-emu-off`

## Workflows GitHub Actions

### gablue-builds.yml

Workflow principal déclenché par :
- Push sur main (avec tags spécifiques : `[main]`, `[nvidia]`, `[dx]`, `[all]`, `[all-iso]`)
- Pull requests
- Schedule quotidien (02:00 UTC)
- Workflow_dispatch (manuel)

**Optimisations** :
- `paths-ignore` : les modifications de fichiers `.md` et `.txt` ne déclenchent pas de build (les changements dans `.github/**` déclenchent bien la build s'ils portent le bon tag, car les jobs sont filtrés par tag de commit)
- `concurrency` : annule les builds en cours si un nouveau push arrive sur la même branche
- **Attention** : un commit avec `[all]` (ou tout autre tag de build) annule la build en cours (concurrency `cancel-in-progress`). Pour committer un changement de workflow sans relancer/annuler une build, utiliser `[skip ci]` (reconnu nativement par GitHub Actions, aucun run déclenché)

**Chaînage ISO (`[all-iso]`)** : le tag `[all-iso]` déclenche les 5 variantes d'images (comme `[all]`). À la fin du workflow, le workflow ISO se déclenche automatiquement via `workflow_run` (voir `build-gablue-live-isos.yml`). Les sous-chaînes ne collisionnent pas : `contains('[all-iso]', '[all]')` et `contains('[all-iso]', '[iso]')` sont tous deux **faux**, donc `[all-iso]` ne déclenche pas l'ISO immédiatement au push.

**Jobs** :
- `build-main` : Build gablue-main (Containerfile-gablue, nvidia_flavor non défini)
- `build-nvidia` : Build gablue-nvidia (Containerfile-gablue, nvidia_flavor=nvidia-lts)
- `build-nvidia-open` : Build gablue-nvidia-open (Containerfile-gablue, nvidia_flavor=nvidia-open)
- `build-dx` : Build gablue-main-dx (Containerfile-gablue, nvidia_flavor non défini, DX_MODE=true)
- `build-nvidia-open-dx` : Build gablue-nvidia-open-dx (Containerfile-gablue, nvidia_flavor=nvidia-open, DX_MODE=true)
- `build-test` : Build gablue-main-test (Containerfile-gablue-test)
- `build-nvidia-open-test` : Build gablue-nvidia-open-test (Containerfile-gablue-nvidia-open-test)
- `update-readme` : Met à jour le tableau de versions du README depuis les artifacts `versions-*` (needs sur les 5 builds d'images, ignoré sur `pull_request`). Commit `[skip ci]` avec push résilient : boucle jusqu'à 5 tentatives avec `git pull --rebase --autostash origin main` entre chaque essai pour absorber les commits concurrents (ex. un push arrivé pendant les ~50 min de build)

### reusable-gablue-image.yml

Workflow réutilisable pour le build d'une image :

**Inputs** :
- `image_name`, `image_desc`, `image_variant` : Identification de l'image
- `source_image` : Image de base Fedora (kinoite)
- `fedora_version` : Version Fedora (44 pour toutes)
- `kernel_type` : Type de kernel (`ogc`), utilisé comme KERNEL_FLAVOR dans le build
- `kernel_version` : Version du kernel (défaut hardcodé dans `reusable-gablue-image.yml`, surcharge possible par job)
- `nvidia_flavor` : Flavor NVIDIA (`nvidia-lts` ou `nvidia-open`, optionnel pour variantes non-NVIDIA)
- `containerfile` : Containerfile explicite (optionnel, défaut Containerfile-gablue)

**Étapes** :
1. Récupération automatique de la version kernel via `skopeo list-tags` uniquement si `kernel_version` est vide
2. Checkout du dépôt
3. Maximisation de l'espace de build
4. Mount BTRFS pour podman storage via la composite action locale `./.github/actions/mount-btrfs-storage` (voir section dédiée) — le storage `/var/lib/containers` est placé sur un loopback BTRFS compressé zstd sur `/`, ce qui absorbe le pic d'espace du rechunk sur la variante DX (`raw-img` + `chunked-img` cohabitent)
5. Build de l'image avec buildah (KERNEL_FLAVOR passé via kernel_type, NVIDIA_FLAVOR si fourni) — **retry** via `nick-fields/retry@v4` avec `retry_on: error` et `timeout_minutes: 60` : le script shell détecte les erreurs réseau (EOF, TLS handshake timeout, connection refused/reset, DNS, Curl timeout, etc.) et sort avec le code 1 (retry), les erreurs de build (échec d'un script RUN) sortent avec le code 2 (échec immédiat). **`retry_on_exit_code` NE DOIT PAS être utilisé** car il désactive le retry sur timeout (bug connu [nick-fields/retry#145](https://github.com/nick-fields/retry/issues/145)). Nettoyage `buildah rmi raw-img` au début de chaque tentative. **`set +e -o pipefail` obligatoire** : `nick-fields/retry@v4` n'hérite pas du `pipefail` de GitHub Actions ; sans lui, `$?` capture le code de `tee` (0) au lieu de `buildah` à travers le pipe `| tee`, masquant tout échec de build (l'étape suivante tente alors `buildah from raw-img` sur une image inexistante → podman essaie de la pull depuis les registres → 404/denied)
6. Application des labels OCI (définis directement dans le step, sans docker/metadata-action)
7. Vérification SecureBoot (step "SecureBoot check") : vérifie la présence du certificat Gablue (`/etc/pki/akmods/certs/gablue-secure-boot.der`) et que les kmods du kernel sont bien signés via `modinfo | grep sig_id`. Échec → l'image ne bootera pas en SecureBoot. Le certificat est enrollable côté client via `ujust secureboot`
8. Collecte des métriques (step "Collect build metrics") : durée de build, espace disque, taille image décompressée (`raw-img`), nombre de RPMs, kernel, mesa, taille compressée (initialement "N/A" car le push n'a pas encore eu lieu) → JSON `metrics-<image>` (artifact, rétention 90 j) + step summary (en anglais). Les libellés affichés sont en anglais, seuls les commentaires YAML restent en français
9. Rechunk avec rpm-ostree
10. Tag et push vers GHCR — **retry bash natif** (pas d'action externe) : une simple boucle `for attempt in 1 2 3` avec `sleep 15` entre tentatives. Le push utilise `skopeo copy` depuis `containers-storage:` et `skopeo inspect containers-storage:` pour le digest. **Pourquoi pas nick-fields/retry@v4 ni wretry.action** : `nick-fields/retry@v4` utilise Node.js `spawn()` qui pipe stdout/stderr — avec une image chunkée (100+ layers), les 100+ lignes "Copying blob" sur stderr saturent le pipe Node.js, l'événement `exit` n'arrive jamais et le process reste bloqué indéfiniment. `wretry.action` (composite, bash natif) fonctionnait mais est déprécié (Node.js 20). Une boucle bash native dans un `run:` standard hérite du stdio du runner (pas de pipe) → pas de hang
11. Signature avec Cosign
12. Métriques post-push (step "Update compressed size") : inspecte le registre distant GHCR via `skopeo inspect --raw docker://$dest_image | jq` en sommant les tailles des layers et du config blob pour obtenir la taille compressée réelle, met à jour le JSON metrics et le step summary. Upload des métriques après cette étape (le fichier JSON final contient la taille compressée)

**Version du kernel** :
- **Par défaut** : Hardcodée dans `reusable-gablue-image.yml` (input `kernel_version`). Version choisie manuellement, actuellement `7.1.3-ogc3.4.fc44.x86_64` (ublue-os/bazzite@982d035)
- **Auto-détection** : Si `kernel_version` est vide, dernier tag OGC via `skopeo list-tags ghcr.io/ublue-os/akmods` → filtre `{KERNEL_FLAVOR}-{FEDORA_VERSION}-*`
- **Manuel** : Spécifier `kernel_version` dans un job pour surcharge ponctuelle

### build-gablue-live-isos.yml

Build des **ISOs live** avec environnement de bureau Plasma complet (tous les 5 jours) :
- Permet d'essayer Gablue avant installation (LiveCD complet, pas juste Anaconda)
- **Déclencheurs** : schedule (5 jours), `workflow_dispatch`, push avec `[iso]`, et **`workflow_run`** à la fin du workflow d'images. Le chaînage `workflow_run` ne construit l'ISO que si l'exécution amont a été déclenchée par un **push** (`workflow_run.event == 'push'`), a **réussi** (`conclusion == 'success'`) et que le message de commit contient **`[all-iso]`** — garantit que les images `:latest` sont publiées avant de builder les ISOs. Le checkout utilise `workflow_run.head_sha` pour rester sur le commit d'origine.
- **Concurrency ISO** : le groupe `build-gablue-live-isos-${{ github.run_id }}-iso` utilise `github.run_id` (et non `github.ref`) pour garantir que chaque run ISO est unique. Sans cela, un `workflow_run` déclenché par un échec de build d'images annulerait un `workflow_dispatch` ISO en cours (même groupe `main-iso`, `cancel-in-progress: true`), alors que ses jobs sont de toute façon `skipped` (conclusion != success).
- Utilise **Titanoboa**, un installateur bootc qui génère un squashfs live. Le binaire `build_iso.sh` est celui de l'image Titanoboa (`quay.io/fedora/fedora:latest`) patché via `installer/titanoboa_build_iso.sh` (bind-mounté, sans `-all-root` pour préserver l'UID 1000 du préfixe Wine)
- 5 variantes : gablue-main, gablue-main-dx, gablue-nvidia, gablue-nvidia-open, gablue-nvidia-open-dx
- **Processus en 2 étapes** :
  1. Build d'une image container payload via `installer/Containerfile` (basée sur l'image Gablue, flatpaks pré-cachés, swap kernel OGC→vanilla pour Secure Boot). Le stockage podman est sur le loopback BTRFS compressé (composite action `mount-btrfs-storage`) **avec `image_copy_tmp_dir` redirigé dans le loopback** via un drop-in `/etc/containers/containers.conf.d/` : par défaut podman copie le layer diff du commit (~30G non compressés) dans `/var/tmp` **sur le disque hôte**, ce qui saturait l'hôte, affamait le fichier loopback sparse et forçait BTRFS en read-only (les « corruptions » historiques du loopback — read-only fs, disk I/O error — n'étaient que ce mécanisme, pas une corruption intrinsèque). Chemin explicite `/var/lib/containers/image-copy-tmp` plutôt que la valeur spéciale `"storage"` (résolution buggy, podman#28211). La boucle de build (`for attempt in 1 2 3`) **ne retente pas** si le log contient `no space left on device`, `read-only file system` ou `disk I/O error` (erreurs non récupérables : chaque tentative reconstruit une image identique, et un loopback passé read-only est mort) et sort immédiatement.
  2. Génération de l'ISO via `podman run` direct (remplace l'action `Zeglius/titanoboa`) : le script patché est bind-mounté sur `/src/build_iso.sh`, le payload `localhost/payload:latest` est monté via `--mount type=image`, le répertoire de sortie ISO est bind-mounté en `/output`
- Signature Cosign + attestation de provenance sur chaque ISO
- Upload vers BuzzHeavier, release GitHub `latest-live-iso`
- `timeout-minutes: 180` (le live est plus long à construire)

#### Build local d'ISO (`local-build/`)

Script de build local pour tester les modifications d'installateur sans CI :

```bash
# Build de l'ISO main
./local-build/build-iso.sh main

# Build + test dans QEMU
./local-build/build-iso.sh main --run

# Pull forcé de l'image de base avant build
./local-build/build-iso.sh main --pull

# Build rapide sans flatpaks (test)
./local-build/build-iso.sh main --skip-flatpaks
```

Variantes disponibles : `main`, `main-dx`, `nvidia`, `nvidia-open`, `nvidia-open-dx`.
L'ISO est générée dans `local-build/output/` avec `chown` sur le fichier pour les permissions utilisateur. Le script utilise `sudo -v` au début (un seul mot de passe), et bind-mounte `installer/titanoboa_build_iso.sh` (patché sans `-all-root`) par-dessus celui de l'image Titanoboa.

#### Dossier `installer/`

```
installer/
├── Containerfile                    # Build payload (FROM image Gablue, bind-mount build.sh, SKIP_FLATPAKS arg)
├── build.sh                         # Assemblage : flatpaks (requis + optionnels + runtimes, skippables via SKIP_FLATPAKS), swap kernel, dracut-live, livesys, Anaconda, pack cache gwine → /extra, pré-initialisation préfixe Wine live
├── iso.yaml                         # Config GRUB (label GABLUE_LIVE, timeout 3s, entrées sans apostrophes, enforcing=0)
├── flatpaks                         # Liste des flatpaks obligatoires (format : ref flatpak)
├── flatpaks-optional                # Liste des flatpaks optionnels (checklist yad)
├── titanoboa_hook_preinitramfs.sh   # Swap kernel OGC → vanilla Fedora (Secure Boot)
├── titanoboa_hook_postrootfs.sh     # Anaconda + kickstart bootc + live tweaks (Xvfb, gparted, etc.)
├── titanoboa_build_iso.sh           # Patch Titanoboa : retire -all-root de mksquashfs (préserve les UID)
├── extra/                           # Contenu local arbitraire copié dans /extra du live (gitignore sauf .gitkeep)
└── system_files/shared/             # Config Anaconda (pre-scripts + post-scripts), autostart, localisation live (fr_CH)
```

#### Fonctionnement du live

1. **Swap kernel** : Le kernel OGC (non signé) est remplacé par le kernel vanilla Fedora (signé) pour Secure Boot
2. **Flatpaks** : 
   - Les listes `installer/flatpaks` (8 apps requises) et `installer/flatpaks-optional` (25 apps optionnelles) définissent quels flatpaks sont **pré-téléchargés** dans l'ISO
   - **MangoHud** (`org.freedesktop.Platform.VulkanLayer.MangoHud`) : runtime obligatoire, version freedesktop détectée dynamiquement (`flatpak remote-ls flathub --runtime | awk -F'\t'` pour extraire la dernière branche). Installé dans le live et ajouté à la liste requise post-install
   - **OBS VkCapture** (`org.freedesktop.Platform.VulkanLayer.OBSVkCapture`) : installé dans le live (même version freedesktop que MangoHud), **ne suit pas OBS Studio** — si OBS est décoché dans la checklist, OBS VkCapture est aussi désinstallé
   - **Proton-GE** (`com.valvesoftware.Steam.CompatibilityTool.Proton-GE`) : installé dans le live (branche `stable`), **suit Steam** — si Steam est décoché, Proton-GE est désinstallé
    - Pour les variantes NVIDIA, les runtimes `org.freedesktop.Platform.GL[32].nvidia-XXX` sont automatiquement ajoutés aux obligatoires (version détectée depuis `rpm -q nvidia-driver`)
    - **Questions interactives regroupées en `%pre-install`** (`pre-scripts/gablue-questions.ks`, `%include` avant `ostreecontainer`) : toutes les interactions utilisateur (yad) sont posées **après le formatage mais avant le déploiement de l'image**, pour ne plus interrompre l'installation ensuite. `%pre-install` et `%post --nochroot` tournent tous deux dans l'environnement de l'installateur → `/tmp` est partagé, on y écrit les choix lus ensuite par les `%post`. Trois questions :
      1. **Compression BTRFS zstd** (oui par défaut) : appliquée **immédiatement** via `btrfs property set <subvol> compression zstd` sur les montages `/mnt/sysroot*`. Posée ici car avec **composefs** le `compress=zstd` du fstab généré par Anaconda est **sans effet** (la racine est un overlay, pas un montage btrfs direct). La propriété btrfs est **héritée par tous les nouveaux fichiers** → déploiement ostree, `/var` et flatpaks compressés dès l'écriture. Niveau par défaut (zstd:3, comme `ujust btrfs-compress`)
      2. **Sélection des flatpaks optionnels** (checklist yad, tout décoché par défaut) : écrit la liste des refs à conserver dans `/tmp/gablue-selected-flatpaks`
      3. **Cache gwine (applications Windows)** (oui par défaut) : écrit `yes`/`no` dans `/tmp/gablue-install-gwine-cache` ; le texte précise que le cache est aussi téléchargeable en ligne plus tard
      - yad lancé via `run0 --user=liveuser env XDG_RUNTIME_DIR=... yad`, avec `--on-top --center --skip-taskbar` (sinon le dialogue s'ouvre derrière la fenêtre Anaconda plein écran et l'installation semble figée)
    - Ensuite, `install-flatpaks.ks` (`%post --nochroot`) **lit** `/tmp/gablue-selected-flatpaks` (fichier absent => aucun optionnel conservé) puis :
      1. **Copie `/var/lib/flatpak` (live) vers le déploiement ostree** via `rsync -aAXUHKP --open-noatime --filter="-x security.selinux"`. Le filtre `-x security.selinux` est **indispensable** : les fichiers du live sont étiquetés `unlabeled_t` et SELinux enforcing refuse le `lremovexattr`/`lsetxattr` du xattr SELinux sur la cible btrfs (`Permission denied` → rsync code 23 → échec du `%post` → crash Anaconda « Message recipient disconnected from message bus without replying »). Les autres xattrs (`user.ostree*`, critiques) restent copiés. Les labels SELinux sont posés au boot par ostree/`restorecon`
      2. **Désinstalle les optionnels non désirés DIRECTEMENT dans la cible ostree** (pas dans le live) : enregistre une installation flatpak `gtarget` (`/etc/flatpak/installations.d/gtarget.conf`) pointant sur `<deployment>/var/lib/flatpak`, puis `flatpak --installation=gtarget uninstall`. **Pourquoi pas dans le live** : le live monte `/var/lib/flatpak` en overlayfs (bind RO via `var-lib-flatpak.mount`), et `flatpak uninstall` y échoue en `Invalid cross-device link` (EXDEV) car les hardlinks entre `repo/objects` et les checkouts ne traversent pas les couches overlay. La cible ostree est sur btrfs (RW, monolithique) → pas d'EXDEV. En root avec `--installation=<nom>`, flatpak opère directement sur le dépôt sans le helper D-Bus système
      3. **Itère par ref complète** (pas par ID) via `awk -F/` sur `flatpak list --columns=ref` : MangoHud et OBS VkCapture ont **plusieurs branches installées** (ex. 24.08 + 25.08), un uninstall par ID échouerait avec « Multiple installed refs match … unable to proceed in non-interactive mode »
      4. **Dépendances conditionnelles** : Proton-GE désinstallé si Steam non coché, OBS VkCapture si OBS non coché
      5. **Nettoie les runtimes orphelins** via `flatpak --installation=gtarget uninstall --unused`
      - Le dépôt Flathub est déjà présent dans `/etc/flatpak/remotes.d/` (ajouté par `build.sh`), pas besoin de `flatpak remote-add` lors de l'install
3. **Création de compte utilisateur** : Aucun compte pré-rempli — le spoke utilisateur Anaconda est visible et l'utilisateur choisit librement son nom/mot de passe. KDE Plasma gère la création au premier démarrage si le spoke est skippé.
4. **Session live** : Bureau Plasma complet via `livesys-scripts`, l'installateur Anaconda n'est pas lancé automatiquement (l'utilisateur le lance via `liveinst` si besoin). Les flatpaks pré-cachés sont visibles dans le menu Plasma (XDG_DATA_DIRS configuré dans `/etc/environment.d/99-gablue-flatpak-live.conf`).
5. **Écran de bienvenue** : `plasma-welcome` est retiré du live (hook postrootfs) pour éviter le lancement automatique au boot
6. **Dossier Bureau** : `livesys-scripts` crée un dossier `Desktop` (anglais) avec `liveinst.desktop` avant `xdg-user-dirs-update`, l'empêchant d'être renommé. Le dossier reste en anglais (`Desktop`).
7. **Installation** : Kickstart Anaconda avec `ostreecontainer` (bootc), BTRFS par défaut, compression zstd:1
8. **Secure Boot** : Enrollment automatique de la clé MOK Gablue avec mot de passe `gablue`
9. **Post-install** : `bootc switch --mutate-in-place` pour activer la signature
10. **Services désactivés dans le live** : flatpak-update, cec-poweroff, dmemcg-booster, tailscaled, brew, greenboot...
11. **NVIDIA live** : Fix `GSK_RENDERER=gl`, réinstallation mesa-vulkan-drivers+nvidia-gpu-firmware (kernel vanilla = pas de drivers proprio, on utilise nouveau)
 12. **Localisation live** : La session live est configurée en français suisse (`fr_CH.UTF-8`) avec clavier QWERTZ suisse romand (`ch(fr)`). Les fichiers sont dans `system_files/shared/etc/` : `locale.conf` (LANG + LANGUAGE), `vconsole.conf` (KEYMAP=ch-fr), `X11/xorg.conf.d/00-keyboard.conf` (layout XKB). Ces fichiers ne sont copiés que dans le payload live (n'affectent pas l'image installée). Les langpacks (`langpacks-fr`, `glibc-all-langpacks`) proviennent de l'image Gablue de base. **Anaconda** est préconfiguré via le kickstart (`titanoboa_hook_postrootfs.sh`) avec `lang fr_CH.UTF-8` et `keyboard --vckeymap=ch-fr --xlayouts='ch (fr)'` : l'écran de langue/clavier de l'installateur est prérempli en français suisse (l'utilisateur peut toujours changer, le spoke reste visible).
13. **GRUB** : Les noms d'entrées ne doivent pas contenir d'apostrophes (Titanoboa génère `menuentry '...'` sans échapper les apostrophes internes, ce qui casse le parsing GRUB et ne montre qu'une seule entrée)
 14. **Dossier `/extra` (live uniquement → déployé à l'install)** : `build.sh` peuple `/extra` du rootfs live. **Le contenu est déployé sur le système installé** par le post-script `install-extra.ks` (voir ci-dessous), et n'est JAMAIS présent dans l'image container (l'installation redéploie l'image propre via `ostreecontainer` + `bootc switch`). Contenu :
      - **Pack cache gwine** : `build.sh` lance `gwine --download-components` puis `gwine --cachepack` et copie le dossier `gwine-cache-installer/` (gwine-cache.tar.xz + install-cache.sh + README) dans `/extra`. Utilise le `gwine` de l'image de base → l'image Gablue doit être reconstruite **avant** l'ISO pour embarquer le comportement à jour (dont le fallback cache offline, cf. `src/gwine-launcher/AGENTS.md`). **Fail-fast** : le build échoue (`exit 1`) si gwine est absent, si le téléchargement des composants échoue, ou si l'archive `gwine-cache.tar.xz` n'est pas produite — pour ne jamais générer d'ISO sans le pack cache. Le build local (`local-build/build-iso.sh`) passe `--network=host` au `podman build` du payload pour que `gwine --download-components` ait accès au réseau (sinon le réseau podman par défaut peut échouer sur Kinoite → build interrompu). Le cache est ré-extrait temporairement pour l'init du préfixe Wine (voir point 16), puis supprimé de l'ISO finale (seule l'archive `/extra` persiste)
      - **Contenu local** : le dossier `installer/extra/` (bind-monté sur `/src/extra`, gitignore sauf `.gitkeep`) est copié dans `/extra` pour les builds locaux — permet d'embarquer des fichiers/dossiers arbitraires. Absent/vide en CI → section ignorée
 15. **Post-script `install-extra.ks`** (`%post --nochroot`) : déployé dans le kickstart juste après `install-flatpaks.ks`. Lit `/extra` dans le live et déploie chaque item à sa destination dans le système installé :
      - **Résolution du déploiement ostree** : en système ostree, `/mnt/sysimage/etc` et `/mnt/sysimage/usr` ne sont **pas** peuplés directement (le système réel vit dans `<deployment>`), donc `/mnt/sysimage` n'est **pas chrootable**. Le script résout `deployment=$(ostree rev-parse --repo=/mnt/sysimage/ostree/repo ostree/0/1/0)` puis `DEPLOY_ROOT=/mnt/sysimage/ostree/deploy/default/deploy/${deployment}.0`. Le passwd est lu dans `${DEPLOY_ROOT}/etc/passwd` (et non `/mnt/sysimage/etc/passwd` qui n'existe pas → sinon `awk: cannot open file` → ScriptError). Les `chown`/`restorecon` se font via `chroot "$DEPLOY_ROOT"` (le home `/home -> var/home` y est accessible)
      - **Utilisateur** : détection dynamique du premier UID ≥ 1000 créé par Anaconda dans `${DEPLOY_ROOT}/etc/passwd`
      - **Cache gwine** : crée `~/.cache/gwine` avec `chattr +C` (nodatacow) **avant** extraction pour éviter le CoW btrfs. Extrait `gwine-cache.tar.xz`, applique `chown -R` + `restorecon` non récursif (seul `.cache` est labellisé, pas les 2 Go de gwine). Le runner gwine **n'est pas extrait** (gwine l'installe lui-même à la volée depuis le cache en mode offline — modification apportée dans `src/gwine-launcher/` pour éviter de dupliquer l'espace disque)
     - **Extensible** : chaque nouvel item (ex. cores RetroArch) s'ajoute comme une section dans le script, avec ses propres `chown`/`restorecon`
      - **Fallback** : si aucun utilisateur n'est trouvé (spoke sauté, création au premier boot), le script loggue et skip sans échec. `/etc/skel` est laissé en option (commenté) pour les futurs utilisateurs
 16. **Pré-initialisation du préfixe Wine dans le live** : après le pack cache, `build.sh` pré-initialise un préfixe Wine complet pour la session live. Le préfixe et le runner sont stockés dans `/usr/share/gablue/wine-home/` et `/usr/share/gablue/wine-runner/` (squashfs), avec un symlink `~/Windows → /usr/share/gablue/wine-home` pour que les chemins du registre pointent vers `/home/liveuser/...`. L'init est lancée via `xvfb-run` (Xvfb requis pour PhysX/OpenAL, installateurs `.exe` qui nécessitent `CreateWindow`). Le cache est extrait temporairement pour l'init puis supprimé (seule l'archive `/extra` persiste pour l'install système). `find ... -exec chown 1000:1000 {} +` (au lieu de `chown -R`, race condition sur les `.tmp` Wine) sur les cibles dans `/usr/share/gablue/` pour que liveuser (UID 1000) soit propriétaire au boot. **Livesys** crée l'utilisateur normalement. **Deux protections contre le crash Xvfb sur images NVIDIA** : (1) les variables GLVND (`__GLX_VENDOR_LIBRARY_NAME=mesa`, `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`, `LIBGL_ALWAYS_SOFTWARE=1`) protègent le côté client GL (Wine/wineboot), (2) un `mv` temporaire de `libglxserver_nvidia.so` et `libnvidia-egl-gbm.so.1` protège le côté serveur X (Xvfb charge son propre module GLX via le mécanisme Xorg, qui ignore les variables GLVND). Les .so sont restaurés immédiatement après `xvfb-run`.
 17. **Patch Titanoboa `-all-root`** : `installer/titanoboa_build_iso.sh` est une copie modifiée du `build_iso.sh` de Titanoboa, bind-mountée à la fois par `local-build/build-iso.sh` et par le workflow CI. La seule différence : le `-all-root` est retiré de l'appel `mksquashfs`, ce qui préserve les UID/GID du payload (UID 1000 pour les fichiers Wine). Sans ce patch, tout le squashfs est root:root et liveuser ne peut pas écrire dans le préfixe. Le CI utilise désormais le même script patché (bind-mounté via `-v` dans le `podman run` Titanoboa).

### clean-gablue-images.yml

Nettoyage automatique (tous les dimanches) :
- Suppression des images > 90 jours
- Conservation des 7 dernières images taggées
- Conservation des 7 dernières images non-taggées
- Packages nettoyés : gablue-main, gablue-nvidia, gablue-nvidia-open, gablue-main-dx, gablue-main-test, gablue-nvidia-open-test

### Composite action `mount-btrfs-storage`

**Fichier** : `.github/actions/mount-btrfs-storage/action.yml`

Action composite locale qui remplace `ublue-os/container-storage-action`. Elle crée un loopback BTRFS compressé (zstd:2) sur "/" et y monte le storage podman.

**Pourquoi** : les runners `ubuntu-24.04` ne montent plus de disque temporaire sur `/mnt`. L'action amont détectait l'absence de `/mnt` et sautait **silencieusement** le montage (simple `notice`, pas d'erreur), laissant le storage sur ext4 sans compression. Sur les builds à gros payload (ISO avec flatpaks, rechunk DX), cela causait `no space left on device` au commit/export de l'image.

**Inputs** :

| Input | Défaut | Description |
|-------|--------|-------------|
| `target-dir` | `/var/lib/containers` | Répertoire à placer sur le loopback BTRFS |
| `loopback-free` | `0.9` | Fraction de l'espace libre de "/" allouée au loopback (fichier sparse, occupation physique dépend du contenu compressé) |
| `mount-opts` | `compress-force=zstd:2` | Options de montage BTRFS |

**Contrainte** : action locale `uses: ./.github/actions/mount-btrfs-storage` — le dépôt doit être **checkout** avant l'appel.

**Utilisation dans les workflows** :
- `reusable-gablue-image.yml` : checkout → mount → build → rechunk → push
- `build-gablue-live-isos.yml` : libérer espace → checkout → mount → drop-in `image_copy_tmp_dir` → build payload → podman run Titanoboa (avec script patché)

## Messages de commit et tags

**Les messages de commit doivent être rédigés en anglais.**

Les tags dans les messages de commit déclenchent les builds :

| Tag | Images déclenchées |
|-----|-------------------|
| `[iso]` | gablue-main, gablue-main-dx, gablue-nvidia, gablue-nvidia-open, gablue-nvidia-open-dx (ISOs live) |
| `[all]` | Toutes les images |
| `[all-iso]` | Toutes les images **puis** les ISOs live automatiquement (chaînage via `workflow_run` une fois les images publiées) |
| `[main]` | gablue-main |
| `[nvidia]` | gablue-nvidia, gablue-nvidia-open, gablue-nvidia-open-test |
| `[dx]` | gablue-main-dx |
| `[test]` | gablue-main-test, gablue-nvidia-open-test |

**Exemples** :
```bash
git commit -m "[main] Update KDE packages"
git commit -m "[nvidia] Update NVIDIA drivers to 550"
git commit -m "[iso] Trigger live ISO build"
git commit -m "[all] Migrate to fc44 and OGC kernel"
```

## Tests et validation

### Analyse statique des scripts

```bash
# Vérifier tous les scripts
find files/scripts -type f -exec shellcheck {} \;

# Vérifier un script spécifique
shellcheck files/scripts/copr
shellcheck files/scripts/post-install

# Vérification syntaxique bash
bash -n files/scripts/nom_du_script
```

### Test de build local

```bash
# Build rapide pour test
sudo buildah build \
  --file Containerfile-gablue \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --tag test-build .

# Test interactif
podman run -it test-build /bin/bash
```

### Vérification post-build

```bash
# Vérifier les paquets installés
podman run test-build rpm -qa | grep -E "(nvidia|kernel|mesa)"

# Vérifier les services
podman run test-build systemctl list-unit-files --state=enabled

# Vérifier la taille
podman images test-build
```

## Fichiers système importants

### Configurations système (/etc)

- **distrobox/distrobox.conf** : Configuration Distrobox
- **firewalld/zones/nm-shared.xml** : Zone firewall partagée
- **profile.d/customperso.sh** : Alias et personnalisations shell
- **security/limits.d/memlock.conf** : Limites mémoire
- **skel/.config/gtk-4.0/** : Configuration GTK par défaut
- **sudoers.d/nopasswd** : Configuration sudo sans mot de passe
- **systemd/** : Timeouts et configuration systemd
- **yum.repos.d/docker-ce.repo** : Dépôt Docker

### Exécution des binaires Windows (binfmt_misc)

- **usr/lib/binfmt.d/gablue-windows.conf** : règle binfmt_misc (magic `MZ` → interpréteur `/usr/bin/gwine`), chargée au boot par `systemd-binfmt.service` (statique, activé par la présence du fichier)
- Permet `./jeu.exe` en terminal et rend fonctionnel le bouton « Exécuter » de Dolphin/KIO pour les `.exe` ayant le bit `+x` (copies depuis NTFS/exFAT, archives) : le kernel invoque gwine avec le chemin du `.exe`
- Complément du patch runner `no_exe_executable_bit.mypatch` (dépôt `elgabo86/gwine`) : le wineserver gwine ne pose plus `+x` sur les `.exe`/`.com` créés par les installateurs Windows. Sans `+x`, KIO ouvre nativement les `.exe` avec le handler par défaut (Windows.desktop → gwine), avec icônes et menu « Ouvrir avec » intacts — KIO ne traite un `.exe` comme binaire natif que s'il a le bit exécutable (comportement hardcodé, sans option de config)

### Scripts utilisateur (/usr/bin)

Scripts personnalisés Gablue :
- `gablue-update` : Mise à jour du système via bootc (script Python autonome dans `/usr/bin`, GUI PySide6 avec PTY pour progression temps réel, fallback CLI automatique si pas d'affichage graphique)
- `gablue-bigscreen-swap-session` : Wrapper swap-session Plasma Bigscreen
- `gablue-bigscreen-session-init` : Initialisation session native Bigscreen (autostart, blacklist + mirroring)
- Scripts gaming : `azahar-install`, `citron-install`, `eden-install`, `esde-install`, `hermes-install`, `qwen-install`, `shadps4-install`, `xenia-install`
- Scripts utilitaires : `dlv`, `dlcover`, `tv`, `tvqt`, `ventoy`, `wallpaper-import`, `clean-media`
- `dlv` : Téléchargeur YouTube unifié (bash) avec support playlist (`--mp3`, `--mp4`, `--mkv`, `--mkv-1080`, `--playlist`). Remplace les anciens alias `dlv-mp*` et le script `ytdl`. Completion bash dans `/usr/share/bash-completion/completions/dlv`.
- `retroplayer` : TUI Go pour explorer et écouter des musiques rétro (téléchargé depuis GitHub Releases pendant le build, dépôt séparé)
- Gestion Wine/Proton : `gwine` (assemblé depuis `src/gwine-launcher/`), `scrap-win`
- `tvqt` : Interface TV Gablue (PySide6 + mpv, navigation manette, ~170 chaînes)

### Binaire gamepadshortcuts (/usr/bin)

Gestionnaire principal des raccourcis manette en C natif (~500 Ko RAM) :
- `gamepadshortcuts` : Binaire C remplaçant l'ancien script Python (~28 Mo RAM)
- Détection automatique de manette via `/dev/input/event*` (evdev, ioctl)
- Support multi-session Wayland : suivi du VT actif via inotify sur `/sys/class/tty/tty0/active`
  - Une instance par session (autostart KDE)
  - Filtrage des événements quand le VT n'est pas actif (pas de conflit entre sessions)
  - Reprise automatique au retour sur le VT

### Binaire gablue-isomount (/usr/bin)

Monteur d'images disque en C natif (~2.7 Mo RAM) :
- `gablue-isomount` : Remplace le plugin dolphin-plugins mountisoaction (bug KDE #471487)
- Monte les fichiers ISO/IMG/EFI via l'API UDisks2 DBus (LoopSetup + Filesystem.Mount)
- Ouvre une nouvelle fenêtre Dolphin sur le point de montage (panneau Devices à jour)
- Démontage automatique quand toutes les instances Dolphin sont fermées
- Si le device est occupé (autre programme), attend sa libération avant démontage
- Si l'image est déjà montée, ouvre juste une nouvelle fenêtre sans remonter
- Service menu KDE : clic droit → "Monter" (remplace l'action native)
- Double-clic : défini comme application par défaut pour les types MIME ISO/IMG/EFI
- Log dans `/tmp/gablue-isomount.log`

### Interface tvqt (/usr/bin)

Interface de télévision Gablue en Python (PySide6 + libmpv) :
- `tvqt` : Interface TV optimisée manette de jeu (~170 chaînes, navigation D-pad)
- Lecture des flux HLS via `libmpv` embarqué + `QOpenGLWidget` (API `mpv_render_context` OpenGL)
- Téléchargement et cache des logos des chaînes
- Filtrage par pays avec pastilles (Suisse, France, Allemagne, Italie, etc.)
- Accélération progressive de la navigation au maintien du D-pad

**Gestion du focus Wayland** (ajout 2025) :
- **Problème** : evdev lit les événements manette même quand tvqt n'est pas au premier plan, provoquant des interférences avec les jeux
- **Solution** : suivi de l'état d'activation via `changeEvent(QEvent.ActivationChange)` — méthode Qt6 fiable sous Wayland car les événements viennent directement du compositor KWin
- **Comportement** :
  - Fenêtre tvqt active → manette fonctionne (navigation chaînes ET lecture vidéo intégrée)
  - Autre application au premier plan (jeu, etc.) → manette **ignorée**

**Lecteur vidéo intégré** (ajout 2025, refonte 2026) :
- mpv est embarqué dans la fenêtre tvqt via `libmpv` + `QOpenGLWidget` (API `mpv_render_context` OpenGL)
- **Python 3.14** : `c_void_p` retourne désormais un `int` Python → wrapper explicite `c_void_p(handle)` requis après `mpv_create()`, sinon ctypes passe le handle en 32-bit (segfault dans `render_context_create`)
- Plus de fenêtre mpv externe ni de sous-processus : le rendu vidéo est natif dans le widget Qt6, compatible Wayland
- La manette fonctionne uniquement quand tvqt a le focus Wayland (navigation + contrôle lecture)
- Bascule grille/vidéo transparente : [A] lance/stop, [B] retour grille, D-pad = volume/seek
- **Fullscreen** : automatique au lancement d'une chaîne, double-clic gauche = toggle, clic droit = retour grille
- **GUI masquée** en mode vidéo : barre supérieure et OSD cachés, seul le flux vidéo est visible

### Scripts gamepadshortcuts (/usr/share/ublue-os/gablue/scripts/gamepadshortcuts)

Scripts lancés par le binaire gamepadshortcuts :
- `launchgamepadshortcuts` : Lanceur avec lockfile par user
- `menuvsr.py` : Menu VR pour actions système (PySide6 + evdev, glassmorphism)
- `mouse.py` : Contrôle souris via manette
- `decoblue` : Déconnexion Bluetooth
- `launchyt` : Lancement YouTube
- `openes` : Overture EmulationStation
- `killthemall` : Tue tous les émulateurs de la session courante
- `takescreenshot`, `startstoprecord` : Capture d'écran / enregistrement
- `changefps`, `showhidemango` : Contrôle FPS / overlay MangoHud

### Configuration tuned (/usr/lib/tuned)

Profils optimisés Gablue :
- `balanced-gablue`
- `balanced-battery-gablue`
- `throughput-performance-gablue`
- `powersave-gablue`
- `powersave-battery-gablue`

### Just commands (/usr/share/ublue-os/just/)

Commandes ujust disponibles :
- **Système** : `configure-grub`, `kernel-setup`, `mitigations-on/off`
- **Réseau** : `tailscale-up`, `ssh-on/off`, `toggle-wol`
- **GPU** : `amd-corectrl-set-kargs`, `toggle-i915-sleep-fix`
- **Gaming** : `scx-enable/disable`, `cpuid-fix-on/off`, `cpuid-emu-on/off`
- **Virtualisation** : `docker-enable/disable`, `dx-group`, `setup-kvmfr`, `libvirt-reset-cache` (efface le cache capabilities libvirt, corrige l'erreur "video model 'virtio' unsupported" dans virt-manager)
- **Maintenance** : `gablue-update`, `brew-reset`, `pyenv-remove`, `snapshots-enable/disable`, `btrfs-compress`, `btrfs-compress-defrag`
- **Rebase** : `gablue-rebase-*` pour changer de variante

## Sécurité

### Clés et signatures

- **cosign.pub** : Clé publique pour vérification des images
- **gablue-kmod.der** : Certificat Secure Boot pour les modules kernel customs (signé par `GABLUE_KMOD_KEY`, enrollé via `ujust secureboot`)
- **gablue-secure-boot.der** : Certificat Secure Boot pour les kmods ublue-os (enrollé via `ujust secureboot`)
- Ne jamais commiter les clés privées (`cosign.key`, `gablue-kmod.key`)
- Les images sont signées automatiquement dans le workflow

### Bonnes pratiques

- Utiliser des variables d'environnement pour les secrets
- Vérifier les signatures des dépôts ajoutés
- Limiter les permissions des fichiers exécutables
- Désactiver les dépôts après installation
- Utiliser `|| true` pour les commandes optionnelles

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

## Dépannage courant

### Erreurs de build

**Problème** : Cache corrompu
**Solution** : `sudo buildah rm -a && sudo podman system prune -a`

**Problème** : Kernel non trouvé
**Solution** : Vérifier que les étapes intermédiaires akmods sont bien montées et que la version kernel existe dans les tags

**Problème** : Conflits de paquets
**Solution** : Vérifier les exclusions dans le script copr (pipewire/bluez/xwayland exclus de bazzite)

**Problème** : Conflit de fichier i686/x86_64 (fc44 multilib)
**Solution** : Terra fc44 nomme les fichiers LICENSE par arch (`.i386` / `.x86_64`), plus de conflit. Si conflit avec d'autres paquets, utiliser `rpm -i --nodeps --excludepath`

**Problème** : Version mismatch x86_64/i686 (fc44)
**Solution** : Upgrader les paquets x86_64 avant d'installer les i686 (ex: pipewire-libs)

### Problèmes d'images

**Problème** : Image trop grande
**Solution** : Vérifier le nettoyage dans cleanup/finalize

**Problème** : Services non démarrés
**Solution** : Vérifier le script systemd et les conditions

## Ressources et liens

- **Universal Blue** : https://universal-blue.org/
- **Bazzite** : https://github.com/ublue-os/bazzite
- **Fedora Kinoite** : https://fedoraproject.org/kinoite/
- **Terra** : https://github.com/terrapkg
- **Documentation uBlue** : https://docs.universal-blue.org/
- **RetroPlayer** : https://github.com/elgabo86/retroplayer

## Mise à jour de ce document

**RÈGLE** : Ce document DOIT être mis à jour avant chaque commit qui modifie la structure, les Containerfiles, les scripts ou les workflows. Ne jamais committer sans avoir vérifié que l'AGENTS.md reflète l'état exact du projet.

Ce document doit être mis à jour lors des changements suivants :
- Ajout d'une nouvelle variante d'image
- Modification de la structure des scripts ou fichiers système
- Changement des dépôts ou sources
- Ajout de nouvelles conventions
- Modification des workflows
