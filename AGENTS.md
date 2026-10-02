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
│   ├── gablue-isomount/                    # Sources C du monteur d'images disque
│   │   ├── gablue-isomount.c              # Programme principal (UDisks2 DBus, Dolphin)
│   │   └── Makefile                       # Compilation
│   └── gwine-launcher/                     # Sources du lanceur gwine (Bash modulaire)
│       ├── AGENTS.md                        # Architecture interne du lanceur
│       ├── gwine                           # Script point d'entrée
│       ├── build.sh                        # Assemblage du fichier standalone
│       ├── completions/                    # Completions bash et zsh
│       └── lib/                            # Bibliothèques modulaires (~60 fichiers)
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
│   │   └── clean-gablue-images.yml        # Nettoyage anciennes images
│   └── dependabot.yml                     # Configuration Dependabot
└── README.md
```

## Commandes de build

### Build d'une variante

```bash
# Exemple : gablue-main — adapter VARIANT / NVIDIA_FLAVOR / DX_MODE selon la variante
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="44" \
  --build-arg KERNEL_FLAVOR="ogc" \
  --build-arg KERNEL_VERSION="<version>" \
  --tag gablue-main .
```

| Variante | VARIANT | Build-args additionnels |
|----------|---------|-------------------------|
| gablue-main | main | — |
| gablue-nvidia | nvidia | `NVIDIA_FLAVOR="nvidia-lts"` |
| gablue-nvidia-open | nvidia-open | `NVIDIA_FLAVOR="nvidia-open"` |
| gablue-main-dx | main | `DX_MODE="true"` |
| gablue-nvidia-open-dx | nvidia-open | `NVIDIA_FLAVOR="nvidia-open"` + `DX_MODE="true"` |

### Vérification de l'image construite

```bash
# Lister les images
podman images | grep gablue

# Tester l'image interactivement
podman run -it gablue-main /bin/bash

# Vérifier le contenu
podman run gablue-main cat /usr/lib/os-release

# Vérifier les paquets installés
podman run gablue-main rpm -qa | grep -E "(nvidia|kernel|mesa)"

# Vérifier les services
podman run gablue-main systemctl list-unit-files --state=enabled

# Vérifier la taille
podman images gablue-main
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

# Étape intermédiaire : image Homebrew pré-construite (aligné Bazzite d0c9330, digest épinglé)
FROM ghcr.io/ublue-os/brew:latest@sha256:<digest> AS brew

# Image de base
FROM quay.io/fedora-ostree-desktops/${SOURCE_IMAGE}:${FEDORA_VERSION}

# Redéfinition des arguments après FROM
ARG VARIANT
ARG SOURCE_IMAGE
ARG DX_MODE
ARG KERNEL_FLAVOR
ARG KERNEL_VERSION

# Copie des fichiers Homebrew pré-construits (avant files/system/all pour le cache)
COPY --from=brew /system_files/ /

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

Le build de test et les vérifications post-build utilisent les commandes de la section « Commandes de build » ci-dessus (tag `test-build` ou nom de variante au choix).

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
- **Actions GitHub épinglées par SHA** : toutes les actions tierces sont pinnées par SHA (avec commentaire `# vX.Y.Z`) pour la sécurité supply-chain, mises à jour par Dependabot (daily)

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
| Scripts de build (copr → cpuid-fault) | `files/scripts/AGENTS.md` | modifier `files/scripts/` ou une étape RUN |
| Fichiers système (/etc, /usr/bin, binfmt, ujust, plasmoid) | `files/system/AGENTS.md` | modifier `files/system/` |
| Workflows CI | `.github/AGENTS.md` | modifier `.github/` ou un tag de commit |
| ISO live / installateur | `installer/AGENTS.md` | modifier `installer/` ou `local-build/` |
| Lanceur gwine | `src/gwine-launcher/AGENTS.md` | modifier `src/gwine-launcher/` |
| Manette (gamepadshortcuts, mouse, kbdnav) | `src/gamepadshortcuts/AGENTS.md` | modifier `src/gamepadshortcuts/` |

Changements suivant la racine :
- Ajout d'une nouvelle variante d'image
- Modification de la structure du projet ou des conventions transverses
- Changement des dépôts ou sources, ajout de nouvelles conventions
- Nouveau/déplacement d'un AGENTS.md de sous-dossier (index ci-dessus)
