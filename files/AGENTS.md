# Build et conventions (files/) — gablue

> Sous-document du AGENTS.md racine. Conventions transverses d'écriture (Bash,
> Containerfile), commandes de build, vérification de l'image, gestion des
> erreurs, tests et dépannage. Le détail de chaque script appelé par un RUN →
> `files/scripts/AGENTS.md`. Fichiers système posés dans l'image →
> `files/system/AGENTS.md`.
> Même règle que la racine : à jour avant tout commit touchant le
> Containerfile, une étape RUN ou un script de `files/scripts/`.

## Commandes de build

### Build d'une variante

```bash
# Exemple : gablue-main — adapter VARIANT / NVIDIA_FLAVOR / DX_MODE selon la variante
sudo buildah build \
  --file Containerfile-gablue \
  --format "docker" \
  --build-arg VARIANT="main" \
  --build-arg SOURCE_IMAGE="kinoite" \
  --build-arg FEDORA_VERSION="45" \
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


## Conventions d'écriture

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

# Étape intermédiaire : image Homebrew pré-construite (aligné Bazzite ea2770f, digest épinglé)
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

