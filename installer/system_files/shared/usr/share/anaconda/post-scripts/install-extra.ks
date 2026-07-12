%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-extra.log
# Déploiement du contenu /extra (cache gwine, cores, etc.) vers le système installé.
#
# /extra est intégré au squashfs du live par build.sh. Ce post-script le déploie
# item par item dans le système cible (home utilisateur pour données user,
# chemins système pour artefacts globaux).
#
# Ajouter un nouvel item : déposer son contenu dans /extra/<nom> côté build,
# puis ajouter une section ci-dessous qui lit /extra/<nom> et déploie à la
# destination voulue.
set -euo pipefail

# =============================================================================
# RÉSOLUTION DU DÉPLOIEMENT OSTREE
# =============================================================================

# En système ostree, /mnt/sysimage/etc et /mnt/sysimage/usr ne sont pas
# peuplés directement : le système réel vit dans le déploiement ostree.
# /mnt/sysimage n'est donc PAS chrootable. On résout le chemin du déploiement
# pour lire son /etc/passwd et chrooter dedans (restorecon, chown).
deployment="$(ostree rev-parse --repo=/mnt/sysimage/ostree/repo ostree/0/1/0)"
DEPLOY_ROOT="/mnt/sysimage/ostree/deploy/default/deploy/${deployment}.0"
DEPLOY_PASSWD="${DEPLOY_ROOT}/etc/passwd"

# =============================================================================
# UTILITAIRES
# =============================================================================

# Résout le home d'un utilisateur depuis le passwd du déploiement.
# Usage: gablue_homeuser <username>
gablue_homeuser() {
    local user="$1"
    awk -F: -v u="$user" '$1 == u {print $6}' "$DEPLOY_PASSWD"
}

# Trouve le premier utilisateur non-système (UID >= 1000) créé par Anaconda.
# Usage: gablue_first_created_user
gablue_first_created_user() {
    awk -F: '$1 != "root" && $3 >= 1000 && $3 < 65534 {print $1; exit}' "$DEPLOY_PASSWD"
}

# Applique chown et restorecon sur un répertoire de la cible.
# Le chroot se fait dans le déploiement ostree (chrootable), pas dans
# /mnt/sysimage. Le home (/home -> var/home) y est accessible.
# Usage: gablue_fixup_perms <dir> <user>
gablue_fixup_perms() {
    local dir="$1"
    local user="$2"
    chroot "$DEPLOY_ROOT" chown -R "${user}:" "$dir"
    chroot "$DEPLOY_ROOT" restorecon -R "$dir" 2>/dev/null || :
}

# =============================================================================
# DÉTECTION DE L'UTILISATEUR CRÉÉ
# =============================================================================

created_user="$(gablue_first_created_user)"

if [ -z "$created_user" ]; then
    echo "Aucun utilisateur non-système trouvé dans ${DEPLOY_PASSWD}"
    echo "Le déploiement ~/.cache/gwine est ignoré (utilisateur créé au premier boot, /etc/skel à prévoir)"
    touch /tmp/anaconda_custom_logs/install-extra-skipped
    exit 0
fi

created_home="$(gablue_homeuser "$created_user")"
if [ -z "$created_home" ] || [ ! -d "/mnt/sysimage${created_home}" ]; then
    echo "Home introuvable pour l'utilisateur $created_user: $created_home"
    exit 1
fi

echo "Utilisateur créé : $created_user (home: $created_home)"
export created_home created_user

# =============================================================================
# CACHE GWINE (~/.cache/gwine)
# =============================================================================

gwine_extra="/extra/gwine-cache-installer"
gwine_target="/mnt/sysimage${created_home}/.cache/gwine"

if [ -d "$gwine_extra" ] && [ -f "$gwine_extra/gwine-cache.tar.xz" ]; then
    echo "Déploiement du cache gwine vers ${created_home}/.cache/gwine..."

    mkdir -p "$gwine_target"

    echo "  - Extraction de l'archive cache (gwine-cache.tar.xz)..."
    tar -xJf "$gwine_extra/gwine-cache.tar.xz" -C "$gwine_target"

    # chown/restorecon sur .cache (et non seulement .cache/gwine) : le
    # mkdir -p ci-dessus crée .cache en root:root 0700 s'il n'existe pas
    # (le %post tourne en root), rendant tout ~/.cache inaccessible à
    # l'utilisateur (KDE, navigateurs, etc. cassent). On rétablit donc
    # récursivement le propriétaire et le contexte SELinux depuis .cache.
    gablue_fixup_perms "${created_home}/.cache" "$created_user"

    # Le runner proton n'est PAS extrait ici : gwine l'installera automatiquement
    # depuis le cache quand l'utilisateur lancera `gwine --init --offline`
    # (grâce aux modifications de fallback cache dans init-main.sh et runner.sh).
    # Cela évite de dupliquer l'espace disque tant que l'utilisateur n'a pas
    # explicitement initié le préfixe.

    echo "  ✓ Cache gwine déployé"
else
    echo "  ⚠ Pack cache gwine absent ($gwine_extra), ignoré"
fi

# =============================================================================
# NOUVEL ITEM : à ajouter ici (cette section est prévue pour extension)
# =============================================================================
# Exemple :
# retroarch_extra="/extra/retroarch-cores"
# retroarch_target="/mnt/sysimage${created_home}/.config/retroarch/cores"
# if [ -d "$retroarch_extra" ]; then
#     mkdir -p "$retroarch_target"
#     cp -a "$retroarch_extra"/. "$retroarch_target/"
#     gablue_fixup_perms "${created_home}/.config/retroarch" "$created_user"
#     echo "  ✓ Cores RetroArch déployés"
# fi

# =============================================================================
# /ETC/SKEL POUR LES FUTURS UTILISATEURS (optionnel)
# =============================================================================
# Si l'utilisateur est créé au premier boot (spoke sauté), /etc/skel garantit
# que le cache sera copié. On ne le fait que si l'utilisateur existe déjà pour
# ne pas dupliquer. Décommentez pour activer :
#
# skel_gwine="/mnt/sysimage/etc/skel/.cache/gwine"
# if [ ! -d "$skel_gwine" ] && [ -f "$gwine_extra/gwine-cache.tar.xz" ]; then
#     mkdir -p "$skel_gwine"
#     tar -xJf "$gwine_extra/gwine-cache.tar.xz" -C "$skel_gwine"
#     echo "  ✓ Cache gwine déployé dans /etc/skel (futurs utilisateurs)"
# fi

echo "Déploiement /extra terminé."
%end
