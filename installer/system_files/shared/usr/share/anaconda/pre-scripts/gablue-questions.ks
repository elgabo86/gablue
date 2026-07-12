%pre-install --log=/tmp/anaconda_custom_logs/gablue-questions.log
# Questions interactives Gablue posées AVANT le déploiement de l'image.
#
# Ce script %pre-install s'exécute après le formatage du disque mais avant
# que Gablue soit déployé. Il regroupe toutes les interactions utilisateur
# (yad) au même endroit pour que l'installation ne soit plus interrompue
# ensuite. Les choix sont :
#   1. Compression BTRFS zstd : appliquée immédiatement via btrfs property
#      set sur les subvolumes montés (héritée par tout ce qui est écrit
#      ensuite : déploiement ostree, /var, flatpaks).
#   2. Sélection des flatpaks optionnels : écrite dans un fichier /tmp lu
#      par install-flatpaks.ks (%post).
#   3. Cache gwine (exécution d'applications Windows) : choix écrit dans un
#      fichier /tmp lu par install-extra.ks (%post).
#
# %pre-install et %post --nochroot tournent tous deux dans l'environnement
# de l'installateur : /tmp est partagé entre eux (comme anaconda_custom_logs).
set -oux pipefail

FLATPAK_OPTIONAL="/usr/share/gablue/flatpaks-optional"
SELECTION_FILE="/tmp/gablue-selected-flatpaks"
GWINE_CHOICE_FILE="/tmp/gablue-install-gwine-cache"

uid=$(id -u liveuser)
yad_user() {
    run0 --user=liveuser env XDG_RUNTIME_DIR=/run/user/"$uid" yad "$@"
}

# =============================================================================
# 1. COMPRESSION BTRFS ZSTD (oui par défaut) - appliquée immédiatement
# =============================================================================

# La compression est posée ici (après formatage, avant déploiement) car avec
# composefs le compress=zstd du fstab généré par Anaconda est sans effet
# (la racine est un overlay, pas un montage btrfs direct). La propriété
# btrfs est héritée par tous les nouveaux fichiers.
if yad_user --question --on-top --center --skip-taskbar \
        --title="Compression du disque" \
        --width=450 \
        --text="Activer la compression BTRFS (zstd) sur le disque ?\n\nRecommandé : gain d'espace notable pour un impact minimal sur les performances." \
        --button="Sans compression:1" \
        --button="Activer la compression:0"; then
    echo "Compression zstd demandée, application sur les subvolumes cibles..."
    # Selon l'environnement Anaconda/Titanoboa, la cible peut être montée
    # sous /mnt/sysroot, /mnt/sysimage ou /var/mnt/sys* : on matche tous les
    # cas. On pose la propriété sur chaque point de montage btrfs de la cible
    # (root, /var, /home) ; elle est héritée par tous les nouveaux fichiers.
    applied=0
    while read -r mp; do
        [ -z "$mp" ] && continue
        echo "→ btrfs property set $mp compression zstd"
        if btrfs property set "$mp" compression zstd; then
            applied=1
        else
            echo "  échec sur $mp"
        fi
    done < <(findmnt -n -o TARGET -t btrfs --list | grep -E '/mnt/sys(root|image)(/|$)')
    if [ "$applied" -eq 0 ]; then
        echo "ATTENTION : aucun subvolume cible trouvé, compression non appliquée."
        findmnt -t btrfs --list -o TARGET,SOURCE || :
    fi
else
    echo "Compression non activée par l'utilisateur."
fi

# =============================================================================
# 2. SÉLECTION DES FLATPAKS OPTIONNELS
# =============================================================================

# Écrit la liste des refs à CONSERVER dans SELECTION_FILE (une par ligne).
# install-flatpaks.ks copie tous les flatpaks puis désinstalle ceux qui ne
# sont pas dans ce fichier. Fichier absent => tous les optionnels retirés.
if [ -f "$FLATPAK_OPTIONAL" ]; then
    YAD_ARGS=(--list --checklist
        --width=700 --height=400
        --on-top --center --skip-taskbar
        --title="Sélection des Flatpaks"
        --text="Choisissez les flatpaks supplémentaires à conserver.\nLes autres seront désinstallés :"
        --column="Garder" --column="Ref" --column="Application"
        --print-column=2 --hide-column=2)

    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        name=$(flatpak info --system --show-name "$ref" 2>/dev/null || echo "$ref")
        YAD_ARGS+=(FALSE "$ref" "$name")
    done < "$FLATPAK_OPTIONAL"

    if TO_KEEP=$(yad_user "${YAD_ARGS[@]}" 2>/dev/null); then
        echo "$TO_KEEP" > "$SELECTION_FILE"
    else
        echo "Dialogue flatpaks annulé, aucun optionnel conservé."
        : > "$SELECTION_FILE"
    fi
fi

# =============================================================================
# 3. CACHE GWINE - EXÉCUTION D'APPLICATIONS WINDOWS (oui par défaut)
# =============================================================================

# Écrit "yes"/"no" dans GWINE_CHOICE_FILE, lu par install-extra.ks.
# Le cache accélère la première exécution d'applications Windows via gwine ;
# il reste installable en ligne plus tard (gwine le télécharge à la demande).
if yad_user --question --on-top --center --skip-taskbar \
        --title="Support des applications Windows" \
        --width=480 \
        --text="Installer le cache pour l'exécution d'applications Windows (gwine) ?\n\nRecommandé : accélère la première utilisation.\nCe cache peut aussi être téléchargé en ligne plus tard." \
        --button="Non:1" \
        --button="Oui:0"; then
    echo "yes" > "$GWINE_CHOICE_FILE"
    echo "Cache gwine : demandé."
else
    echo "no" > "$GWINE_CHOICE_FILE"
    echo "Cache gwine : refusé par l'utilisateur."
fi

echo "Questions Gablue terminées."
%end
