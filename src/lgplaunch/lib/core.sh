
#======================================
# Variables globales
#======================================
args=""
fullpath=""
# Mode sélection d'exécutable (--exe) : choisir un binaire dans le pack au lancement
# --exe           → menu des exécutables du pack
# --exe=<rel>     → lance directement ce chemin (relatif à la racine du pack)
SELECT_EXE_MODE=false
SELECT_EXE_PATH=
# Normaliser $HOME vers /var/home (chemin réel sur Silverblue/Kinoite)
HOME_REAL="$(realpath "$HOME")"
SAVES_SYMLINK="/tmp/lgp-saves-$UID"
SAVES_REAL="$HOME_REAL/.local/share/lgp-saves"
EXTRA_SYMLINK="/tmp/lgp-extra-$UID"
EXTRA_REAL="$HOME_REAL/.cache/lgp-extra"
TEMP_SYMLINK="/tmp/lgp-temp-$UID"
TEMP_REAL="/tmp/lgp-temp-$UID"

# Variables LGP
LGPACK_NAME=""
GAME_INTERNAL_NAME=""
MOUNT_DIR=""

#======================================
# Fonctions d'affichage et utilitaires
#======================================

# Affiche un message d'erreur et quitte
error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

