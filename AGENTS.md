# Instructions pour les agents - Gablue

## Vue d'ensemble du projet

Gablue est une distribution immuable personnalisée basée sur **Fedora Kinoite** (KDE Plasma), construite avec des Containerfiles et utilisant le processus de build **Universal Blue (uBlue)**. Le projet utilise buildah/podman pour la construction d'images container et rpm-ostree pour le déploiement immuable.

### Caractéristiques principales

- **Base** : Fedora Kinoite 43 (KDE Plasma)
- **Kernel** : Bazzite kernel (optimisé pour le gaming)
- **Mesa** : Terra Mesa (version plus récente pour meilleures performances)
- **NVIDIA** : Support des pilotes NVIDIA closed et open-source
- **Virtualisation** : Mode DX avec Docker, Libvirt, QEMU
- **Gaming** : Optimisations poussées (Gamescope, MangoHud, schedulers)

## Variantes d'images

Le projet construit 6 variantes distinctes :

| Variante | Description | Kernel | Trigger tag |
|----------|-------------|--------|-------------|
| `gablue-main` | Image standard sans NVIDIA | Bazzite (stable) | `[main]`, `[all]` |
| `gablue-nvidia` | Pilotes NVIDIA closed-source | Bazzite (stable) | `[nvidia]`, `[all]` |
| `gablue-nvidia-open` | Pilotes NVIDIA open-source | Bazzite (stable) | `[nvidia]`, `[all]` |
| `gablue-main-dx` | Mode développement (DX) avec virtualisation | Bazzite (stable) | `[dx]`, `[all]` |
| `gablue-main-test` | Image de test avec OpenGamepadUI | OGC (unstable) | `[test]`, `[all]` |
| `gablue-nvidia-open-test` | Test NVIDIA Open avec OpenGamepadUI | OGC (unstable) | `[test]`, `[nvidia]`, `[all]` |

### Différences entre variantes

**Main vs NVIDIA** :
- NVIDIA installe les pilotes depuis `ghcr.io/bazzite-org/nvidia-drivers`
- Kernel spécifique avec modules NVIDIA (`kernel-nvidia` ou `kernel-nvidia-closed-lts`)
- Paquets additionnels : `supergfxctl`, `supergfxctl-plasmoid` (Kinoite) ou `gnome-shell-extension-supergfxctl-gex` (Silverblue)

**Main vs DX** :
- DX inclut Docker CE, Libvirt, QEMU, virt-manager
- Activation automatique des services Docker et libvirt
- Groupes utilisateurs supplémentaires configurés

**Stable vs Test** :
- **Kernel** : Stable utilise Bazzite (stable), Test utilise OGC (unstable)
- **OpenGamepadUI** : Interface gaming expérimentale style Steam Deck (test uniquement)
- **Scripts spécifiques** : `kernel-test`, `nvidia-test`, `copr-test`, `rpm-test`, `post-install-test`, `systemd-test`
- **Containerfiles** : `Containerfile-gablue-test` (main-test), `Containerfile-gablue-nvidia-open-test` (nvidia-open-test)
- **Paquets additionnels** : `opengamepadui`, `gamescope-session-opengamepadui`, `powerstation`, `inputplumber`, `amdsmi`
- **Akmods complets** : v4l2loopback, xone, xpadneo, openrazer, zenergy, evdi, etc.

## Structure du projet

```
.
├── Containerfile-gablue                   # Containerfile principal
├── Containerfile-gablue-test              # Containerfile pour main-test
├── Containerfile-gablue-nvidia-open-test  # Containerfile pour nvidia-open-test
├── cosign.pub                             # Clé publique pour signature
├── files/
│   ├── scripts/                           # Scripts d'installation bash
│   │   ├── brew                           # Installation Homebrew
│   │   ├── cleanup                        # Nettoyage intermédiaire
│   │   ├── copr                           # Configuration dépôts COPR (stable)
│   │   ├── copr-test                      # Configuration dépôts COPR (test)
│   │   ├── finalize                       # Finalisation de l'image
│   │   ├── initramfs                      # Génération initramfs
│   │   ├── kernel                        # Installation kernel Bazzite (stable)
│   │   ├── kernel-test                    # Installation kernel OGC (test)
│   │   ├── mesa                           # Installation Mesa Terra
│   │   ├── nvidia                         # Installation pilotes NVIDIA (stable)
│   │   ├── nvidia-test                    # Installation pilotes NVIDIA (test)
│   │   ├── post-install                   # Post-installation principale
│   │   ├── post-install-test              # Post-installation test
│   │   ├── rpm                            # Paquets RPM (stable)
│   │   ├── rpm-test                       # Paquets RPM (test)
│   │   ├── systemd                        # Activation services systemd
│   │   └── systemd-test                   # Activation services (test)
│   └── system/                            # Fichiers système à copier
│       ├── all/                           # Fichiers communs à toutes les variantes
│       │   ├── etc/                       # Configurations système (/etc)
│       │   └── usr/                       # Fichiers utilisateur (/usr)
│       ├── kinoite/                       # Fichiers spécifiques Kinoite
│       ├── main/                          # Fichiers spécifiques variante main
│       ├── nvidia/                        # Fichiers spécifiques NVIDIA
│       ├── nvidia-open/                   # Fichiers spécifiques NVIDIA Open
│       └── test/                          # Fichiers spécifiques images test
├── .github/
│   ├── workflows/                         # Workflows GitHub Actions
│   │   ├── gablue-builds.yml              # Workflow principal de build
│   │   ├── reusable-gablue-image.yml      # Workflow réutilisable
│   │   ├── build-gablue-isos.yml          # Build des ISOs d'installation
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
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
  --build-arg KERNEL_TYPE="bazzite" \
  --build-arg NVIDIA_VERSION="latest" \
  --tag gablue-main .

# Build de l'image NVIDIA
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="nvidia" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
  --build-arg KERNEL_TYPE="bazzite" \
  --build-arg NVIDIA_VERSION="latest" \
  --tag gablue-nvidia .

# Build de l'image NVIDIA Open
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="nvidia-open" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
  --build-arg KERNEL_TYPE="bazzite" \
  --build-arg NVIDIA_VERSION="latest" \
  --tag gablue-nvidia-open .

# Build de l'image DX (développement)
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
  --build-arg KERNEL_TYPE="bazzite" \
  --build-arg NVIDIA_VERSION="latest" \
  --build-arg DX_MODE="true" \
  --tag gablue-main-dx .

# Build de l'image test (main-test)
sudo buildah build \
  --file Containerfile-gablue-test \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
  --build-arg KERNEL_TYPE="ogc" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="6.19.11-ogc1.1.fc43.x86_64" \
  --tag gablue-main-test .

# Build de l'image nvidia-open-test
sudo buildah build \
  --file Containerfile-gablue-nvidia-open-test \
  --format "docker" \
  --build-arg VARIANT="nvidia-open" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
  --build-arg KERNEL_TYPE="ogc" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="6.19.11-ogc1.1.fc43.x86_64" \
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

**Pattern standard** :
```dockerfile
# Arguments de build
ARG VARIANT
ARG SOURCE_IMAGE
ARG FEDORA_VERSION

# Étape intermédiaire : récupération du kernel
FROM ghcr.io/bazzite-org/kernel-bazzite:latest-f${FEDORA_VERSION}-x86_64 AS kernel

# Image de base
FROM ghcr.io/ublue-os/${SOURCE_IMAGE}${SOURCE_SUFFIX}:${FEDORA_VERSION}

# Redéfinition des arguments après FROM
ARG VARIANT
ARG SOURCE_IMAGE

# Copie des fichiers
COPY files/scripts /ctx/
COPY files/system/all /
COPY files/system/${SOURCE_IMAGE} /
COPY files/system/${VARIANT} /

# Variables d'environnement
ENV VARIANT=${VARIANT}
ENV SOURCE_IMAGE=${SOURCE_IMAGE}

# Installation avec cache
RUN --mount=type=cache,dst=/var/cache \
    --mount=type=cache,dst=/var/log \
    --mount=type=bind,from=kernel,src=/,dst=/rpms/kernel \
    sh /ctx/script && \
    sh /ctx/cleanup
```

**Bonnes pratiques RUN** :
- Utiliser `--mount=type=cache` pour `/var/cache` et `/var/log`
- Utiliser `--mount=type=bind,from=stage` pour accéder aux étapes intermédiaires
- Chaîner les commandes avec `&&` pour réduire les layers
- Terminer par `sh /ctx/cleanup` pour nettoyer

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
    dnf5 -y install waydroid
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
- **COPR** : bazzite-org/bazzite, ublue-os/staging, ublue-os/packages, che/nerd-fonts, hikariknight/looking-glass-kvmfr, lizardbyte/beta
- **Tiers** : Tailscale, Negativo17
- **RPMFusion** : free et nonfree
- **Terra (FyraLabs)** : terra-release, terra-release-extras, terra-release-mesa

Exclusions importantes :
- Mesa et kernel des dépôts Fedora (fournis par Terra)
- Priorité Terra = 3 (haute), RPMFusion = 5 (basse)

### 2. kernel / kernel-test - Installation du kernel

**kernel (stable)** : Installation du kernel Bazzite
- Récupération depuis l'étape intermédiaire `kernel`
- Installation des paquets kernel standards
- Ajout des paquets NVIDIA si variante NVIDIA
- Versionlock pour verrouiller les versions
- Installation de scx-scheds depuis COPR bieszczaders/kernel-cachyos-addons

**kernel-test (test)** : Installation du kernel OGC (Open Game Kernel)
- Récupération depuis `ghcr.io/ublue-os/akmods:ogc-*`
- Installation du kernel instable avec akmods complets
- Akmods inclus : v4l2loopback, xone, xpadneo, openrazer, zenergy, evdi, gcadapter, new-lg4ff, etc.
- Versionlock pour verrouiller les versions

### 3. mesa - Installation Mesa Terra

Swap Mesa vers la version Terra optimisée :
- Swap de `mesa-filesystem` vers terra-mesa
- Installation des pilotes Mesa principaux
- Paquets i686 pour variantes NVIDIA (compatibilité jeux)
- Versionlock des paquets Mesa

### 4. nvidia / nvidia-test - Installation pilotes NVIDIA

**nvidia (stable)** : Installation depuis l'étape intermédiaire `nvidia` ou `nvidia-open`
- Librairies NVIDIA (libnvidia-*)
- Pilotes et utilitaires (nvidia-driver, nvidia-settings)
- Container toolkit pour Docker
- Configuration SELinux pour les conteneurs NVIDIA
- Modification dracut pour forcer le chargement des pilotes

**nvidia-test (test)** : Installation depuis les akmods ublue-os
- Librairies EGL Wayland (32 et 64 bits)
- Installation via script ublue-os depuis `/tmp/rpms/nvidia-open` ou `/tmp/rpms/nvidia`
- Support multilib activé (MULTILIB=1)
- Configuration post-installation : liens symboliques, suppression ICD Nouveau

### 5. copr / copr-test - Configuration des dépôts

**copr (stable)** : Configuration des dépôts COPR
- COPR : bazzite-org/bazzite, bazzite-org/bazzite-multilib, ublue-os/staging, ublue-os/packages, che/nerd-fonts, hikariknight/looking-glass-kvmfr, lizardbyte/beta
- Tiers : Tailscale, Negativo17
- RPMFusion : free et nonfree
- Terra (FyraLabs) : terra-release, terra-release-extras, terra-release-mesa

**copr-test (test)** : Configuration avec exclusions supplémentaires
- Mêmes dépôts que stable
- Exclusion supplémentaire : `gamescope-session` (conflit avec OpenGamepadUI)

Exclusions importantes :
- Mesa et kernel des dépôts Fedora (fournis par Terra)
- Priorité Terra = 3 (haute), RPMFusion = 5 (basse)

### 6. rpm / rpm-test - Paquets RPM

Installation extensive de paquets organisée par catégories :
- **CLI** : fswatch, btop, fastfetch, git, atuin, tldr, etc.
- **Réseau** : tailscale, rar
- **Multimédia** : yt-dlp
- **Virtualisation (DX uniquement)** : docker-ce, libvirt, qemu, virt-manager
- **Terminal fun** : asciiquarium, cmatrix
- **Gaming** : sunshine, mangohud, gamescope
- **KDE** : okular, gwenview, kcalc, yakuake (Kinoite uniquement)
- **Polices** : nerd-fonts
- **Développement** : gcc, python3-pip

**rpm-test** ajoute :
- `amdsmi` (monitoring AMD)
- `opengamepadui`, `gamescope-session-opengamepadui`, `powerstation`, `inputplumber` (OpenGamepadUI)

**rpm** inclut en plus :
- Outils SELinux : `checkpolicy`, `selinux-policy-devel`

Paquets supprimés :
- firefox, firefox-langpacks, htop
- plasma-discover-rpm-ostree (Kinoite)

### 7. post-install / post-install-test

Configuration post-installation étendue :
- Permissions des exécutables (chmod +x)
- Capacités système (setcap pour ksysguard, gamescope)
- Binaires externes (zxtune)
- Branding Gablue (os-release)
- Configuration système (tuned, bluetooth, timers)
- Désactivation des dépôts
- Nettoyage des fichiers .desktop
- Configuration DX (iptables, NetworkManager)

### 8. systemd / systemd-test

Activation/désactivation des services systemd :
- **Activés** : system-flatpak-setup, earlyoom, rpm-ostreed-automatic, flatpak-update
- **Désactivés** : scx_loader, tailscaled, displaylink
- **Conditionnels** : libvirt (DX), waydroid (main)

**systemd-test** ajoute :
- Services OpenGamepadUI désactivés : inputplumber, powerstation
- Note: opengamepadui-session.service n'est PAS activé par défaut

### 9. initramfs

Génération de l'initramfs avec dracut :
- Détection de la version kernel installée
- Génération avec options ostree et fido2
- Permissions sécurisées (0600)

### 9. cleanup / finalize

Nettoyage des fichiers temporaires et commit OSTree :
- `dnf5 clean all`
- Suppression des fichiers temporaires
- `ostree container commit`

## Workflows GitHub Actions

### gablue-builds.yml

Workflow principal déclenché par :
- Push sur main (avec tags spécifiques)
- Pull requests
- Schedule quotidien (06:00 UTC)
- Workflow_dispatch (manuel)

**Jobs** :
- `build-main` : Build gablue-main
- `build-nvidia` : Build gablue-nvidia
- `build-nvidia-open` : Build gablue-nvidia-open
- `build-dx` : Build gablue-main-dx
- `build-test` : Build gablue-main-test

### reusable-gablue-image.yml

Workflow réutilisable pour le build d'une image :

**Étapes** :
1. Récupération de la version kernel via `skopeo inspect` (stable) ou `skopeo list-tags` (test)
2. Checkout du dépôt
3. Maximisation de l'espace de build
4. Mount BTRFS pour podman storage
5. **Cache DNF** : Restoration du cache des paquets (basé sur kde/gnome)
6. Génération des tags (timestamp, SHA, latest)
7. Build de l'image avec buildah
8. **Sauvegarde du cache DNF** (uniquement sur branche main)
9. Application des labels OCI
10. Rechunk avec rpm-ostree
11. Tag et push vers GHCR
12. Signature avec Cosign

**Détection dynamique du kernel** :
- **Stable** : Version extraite via `skopeo inspect ghcr.io/bazzite-org/kernel-bazzite:latest-*`
- **Test** : Dernier tag OGC via `skopeo list-tags ghcr.io/ublue-os/akmods` → filtre `ogc-{FEDORA_VERSION}-*`
- **Manuel** : Spécifier `kernel_version` dans le workflow pour forcer une version

**Cache DNF** :
- Restauration avant le build pour accélérer l'installation des paquets
- Sauvegarde après le build (uniquement sur `main`, pas sur les PRs)
- Cache séparé par environnement : `Linux-buildah-kde` ou `Linux-buildah-gnome`

### build-gablue-isos.yml

Build des ISOs d'installation (tous les 5 jours) :
- Matrix de build pour chaque variante
- Upload vers VikingFile
- Création de release GitHub avec liens de téléchargement
- Génération des checksums SHA256

### clean-gablue-images.yml

Nettoyage automatique (tous les dimanches) :
- Suppression des images > 90 jours
- Conservation des 7 dernières images taggées
- Conservation des 7 dernières images non-taggées

## Messages de commit et tags

**Les messages de commit doivent être rédigés en anglais.**

Les tags dans les messages de commit déclenchent les builds :

| Tag | Images déclenchées |
|-----|-------------------|
| `[all]` | Toutes les images |
| `[main]` | gablue-main |
| `[nvidia]` | gablue-nvidia, gablue-nvidia-open, gablue-nvidia-open-test |
| `[dx]` | gablue-main-dx |
| `[test]` | gablue-main-test, gablue-nvidia-open-test |

**Exemples** :
```bash
git commit -m "[main] Update KDE packages"
git commit -m "[nvidia] Update NVIDIA drivers to 550"
git commit -m "[all] Update Bazzite kernel"
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
  --build-arg SOURCE_SUFFIX="-main" \
  --build-arg FEDORA_VERSION="43" \
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

### Scripts utilisateur (/usr/bin)

Scripts personnalisés Gablue :
- `gablue-update` : Mise à jour du système (interface PySide6)
- `sync-gamepads` : Synchronisation des manettes
- `system-flatpak-setup` : Configuration Flatpak système
- Scripts gaming : `a13`, `chdman`, `citron-install`, `eden-install`, etc.
- Scripts utilitaires : `ytdl`, `dlcover`, `genimg`, `raroms`, etc.

### Scripts gamepadshortcuts (/usr/share/ublue-os/gablue/scripts/gamepadshortcuts)

Scripts de contrôle par manette :
- `gamepadshortcuts.py` : Gestionnaire principal des raccourcis (evdev)
- `menuvsr.py` : Menu VR pour actions système (PySide6 + evdev, glassmorphism)
- `mouse.py` : Contrôle souris via manette
- `decoblue` : Déconnexion Bluetooth
- `launchyt` : Lancement YouTube
- `openes` : Overture EmulationStation

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
- **Gaming** : `scx-enable/disable`, `cpuid-fix-on/off`
- **Virtualisation** : `docker-enable/disable`, `dx-group`
- **Maintenance** : `gablue-update`, `brew-reset`, `pyenv-setup`
- **Rebase** : `gablue-rebase-*` pour changer de variante

## Sécurité

### Clés et signatures

- **cosign.pub** : Clé publique pour vérification des images
- Ne jamais commiter `cosign.key` ou `cosign.private`
- Les images sont signées automatiquement dans le workflow

### Bonnes pratiques

- Utiliser des variables d'environnement pour les secrets
- Vérifier les signatures des dépôts ajoutés
- Limiter les permissions des fichiers exécutables
- Désactiver les dépôts après installation
- Utiliser `|| true` pour les commandes optionnelles

### SELinux

- Module NVIDIA container installé dans le script nvidia
- Configuration pour les conteneurs avec accès GPU

## Langue et internationalisation

- **Documentation** : Français
- **Commentaires de code** : Français
- **Messages utilisateur** : Français (alias, scripts, etc.)
- **Variables** : Anglais ou français cohérent
- **Commits** : Français ou anglais (avec tags obligatoires)

## Dépannage courant

### Erreurs de build

**Problème** : Cache corrompu  
**Solution** : `sudo buildah rm -a && sudo podman system prune -a`

**Problème** : Kernel non trouvé  
**Solution** : Vérifier que l'étape intermédiaire kernel est bien montée

**Problème** : Conflits de paquets  
**Solution** : Vérifier les exclusions dans le script copr

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

## Mise à jour de ce document

Ce document doit être mis à jour lors des changements suivants :
- Ajout d'une nouvelle variante d'image
- Modification de la structure des scripts
- Changement des dépôts ou sources
- Ajout de nouvelles conventions
- Modification des workflows
