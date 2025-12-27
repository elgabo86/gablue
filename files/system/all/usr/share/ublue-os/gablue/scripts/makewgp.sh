#!/bin/bash

# Vérifier qu'un dossier est fourni en argument
if [ $# -eq 0 ]; then
    echo "Usage: $0 <dossier_du_jeu>"
    exit 1
fi

GAME_DIR="$(realpath "$1")"

# Vérifier que le dossier existe
if [ ! -d "$GAME_DIR" ]; then
    echo "Erreur: le dossier '$GAME_DIR' n'existe pas"
    exit 1
fi

# Nom du paquet (basé sur le nom du dossier)
GAME_NAME="$(basename "$GAME_DIR")"
WGPACK_NAME="$(dirname "$GAME_DIR")/${GAME_NAME}.wgp"

echo "=== Création du paquet pour: $GAME_NAME ==="
echo "Dossier source: $GAME_DIR"

# Nettoyer les dossiers temporaires d'une exécution précédente
echo ""
echo "Nettoyage des dossiers temporaires..."
rm -rf "$GAME_DIR/.save" "$GAME_DIR/.extra"
rm -f "$GAME_DIR/.savepath" "$GAME_DIR/.extrapath"
echo ""

# Demander le niveau de compression zstd
DEFAULT_LEVEL=15
if command -v kdialog &> /dev/null; then
    INPUT=$(kdialog --inputbox "Niveau de compression zstd (1-19):\n1 = le plus rapide à lire\n19 = la plus petite taille\n0 = pas de compression" "$DEFAULT_LEVEL")
else
    echo "kdialog non disponible, utilisation du mode console"
    echo "Niveau de compression zstd (1-19):"
    echo "  1 = le plus rapide à lire"
    echo "  19 = la plus petite taille"
    echo "  0 = pas de compression"
    read -p "Niveau [$DEFAULT_LEVEL]: " INPUT
    INPUT=${INPUT:-$DEFAULT_LEVEL}
fi

case "$INPUT" in
    "0"|"none"|"non")
        COMPRESS_CMD=""
        SUFFIX=""
        ;;
    [1-9]|1[0-9])
        COMPRESS_CMD="-comp zstd -Xcompression-level $INPUT"
        SUFFIX="_zstd$INPUT"
        ;;
    *)
        echo "Choix invalide"
        exit 1
        ;;
esac

echo ""
echo "=== Recherche des exécutables (.exe et .bat) ==="

# Scanner le dossier pour trouver les .exe et .bat
EXE_LIST=$(find "$GAME_DIR" -type f \( -iname "*.exe" -o -iname "*.bat" \) 2>/dev/null)

if [ -z "$EXE_LIST" ]; then
    echo "Erreur: aucun fichier .exe ou .bat trouvé dans $GAME_DIR"
    exit 1
fi

# Préparer la liste pour kdialog ou sélection console
COUNT=0
EXE_ARRAY=()
while IFS= read -r exe; do
    REL_PATH="${exe#$GAME_DIR/}"
    EXE_ARRAY+=("$exe")
    EXE_ARRAY+=("$REL_PATH")
    if [ "$COUNT" -eq 0 ]; then
        EXE_ARRAY+=("on") # Premier sélectionné par défaut
    else
        EXE_ARRAY+=("off")
    fi
    COUNT=$((COUNT + 1))
done <<< "$EXE_LIST"

if command -v kdialog &> /dev/null; then
    SELECTED=$(kdialog --radiolist "Sélectionnez l'exécutable principal:" "${EXE_ARRAY[@]}")
else
    echo "Exécutables trouvés:"
    i=0
    while IFS= read -r exe; do
        echo "  $((i+1)). ${exe#$GAME_DIR/}"
        i=$((i + 1))
    done <<< "$EXE_LIST"
    read -p "Entrez le numéro de l'exécutable: " SELECTED_NUM
    SELECTED_NUM=$((SELECTED_NUM - 1))
    SELECTED="${EXE_ARRAY[$((SELECTED_NUM * 3))]}"
fi

if [ ! -f "$SELECTED" ]; then
    echo "Erreur: exécutable non valide"
    exit 1
fi

# Chemin relatif de l'exécutable par rapport au dossier du jeu
EXE_REL_PATH="${SELECTED#$GAME_DIR/}"

echo ""
echo "Exécutable sélectionné: $EXE_REL_PATH"

# Créer le fichier .launch dans le dossier source
LAUNCH_FILE="$GAME_DIR/.launch"
echo "$EXE_REL_PATH" > "$LAUNCH_FILE"
echo "Fichier .launch créé: $LAUNCH_FILE"

# Demander si l'utilisateur veut ajouter des arguments de lancement
BOTTLE_ARGS=""
if command -v kdialog &> /dev/null; then
    ADD_ARGS=$(kdialog --yesno "Voulez-vous ajouter des arguments de lancement ?\\n(ex: --dx12, --window, --no-borders, etc.)" --yes-label "Oui" --no-label "Non")
    if [ $? -eq 0 ]; then
        BOTTLE_ARGS=$(kdialog --inputbox "Entrez les arguments de lancement (sans le --dx12 précédent)")
    fi
else
    read -p "Voulez-vous ajouter des arguments de lancement ? (o/N): " ADD_ARGS
    if [[ "$ADD_ARGS" =~ ^[oOyY]$ ]]; then
        read -p "Entrez les arguments de lancement (ex: dx12, window, etc.): " BOTTLE_ARGS
    fi
fi

# Créer le fichier .args si des arguments ont été fournis
if [ -n "$BOTTLE_ARGS" ]; then
    ARGS_FILE="$GAME_DIR/.args"
    echo "$BOTTLE_ARGS" > "$ARGS_FILE"
    echo "Fichier .args créé: $ARGS_FILE (arguments: $BOTTLE_ARGS)"
fi

# Demander si l'utilisateur veut activer le fix manette
FIX_ENABLED=false
if command -v kdialog &> /dev/null; then
    kdialog --yesno "Voulez-vous activer le fix manette pour ce jeu ?\\n\\nCela modifie une clé de registre Wine (DisableHidraw)\\npour résoudre des problèmes de compatibilité manette." --yes-label "Oui" --no-label "Non"
    if [ $? -eq 0 ]; then
        FIX_ENABLED=true
    fi
else
    read -p "Activer le fix manette ? (o/N): " FIX_INPUT
    if [[ "$FIX_INPUT" =~ ^[oOyY]$ ]]; then
        FIX_ENABLED=true
    fi
fi

# Créer le fichier .fix si activé
if [ "$FIX_ENABLED" = true ]; then
    FIX_FILE="$GAME_DIR/.fix"
    touch "$FIX_FILE"
    echo "Fichier .fix créé: $FIX_FILE"
fi

echo ""
echo "=== Gestion des sauvegardes ==="

# Créer le dossier .savepath dans le wgp avec la structure complète
SAVE_WGP_DIR=""
SAVE_FILE=""

# Boucle pour ajouter plusieurs sauvegardes
SAVE_LOOP=true
while [ "$SAVE_LOOP" = true ]; do
    # Demander si le jeu utilise des saves dans le dossier du jeu
    SAVE_ENABLED=false
    if command -v kdialog &> /dev/null; then
        kdialog --yesno "Dossier/fichier de sauvegarde à gérer ?\\n\\nSauvegardes persistantes stockées dans UserData.\\nUne copie reste dans le paquet pour la portabilité." --yes-label "Oui" --no-label "Non"
        if [ $? -eq 0 ]; then
            SAVE_ENABLED=true
        fi
    else
        read -p "Y a-t-il un dossier ou fichier de sauvegarde à gérer ? (o/N): " SAVE_INPUT
        if [[ "$SAVE_INPUT" =~ ^[oOyY]$ ]]; then
            SAVE_ENABLED=true
        fi
    fi

    if [ "$SAVE_ENABLED" = false ]; then
        SAVE_LOOP=false
        break
    fi

    # Demander le type (dossier ou fichier)
    SAVE_TYPE=""
    if command -v kdialog &> /dev/null; then
        SAVE_TYPE=$(kdialog --radiolist "Que voulez-vous conserver ?" "dir" "Dossier" "on" "file" "Fichier" "off")
    else
        read -p "Type à conserver [d]ossier ou [f]ichier ? (D/f): " TYPE_INPUT
        if [[ "$TYPE_INPUT" =~ ^[fF]$ ]]; then
            SAVE_TYPE="file"
        else
            SAVE_TYPE="dir"
        fi
    fi

    # Utiliser le sélecteur de fichiers KDE pour dossier ou fichier
    if command -v kdialog &> /dev/null; then
        if [ "$SAVE_TYPE" = "dir" ]; then
            SELECTED_ITEM=$(kdialog --getexistingdirectory "$GAME_DIR")
        else
            SELECTED_ITEM=$(kdialog --getopenfilename "$GAME_DIR" "Tous les fichiers (*)")
        fi
    else
        if [ "$SAVE_TYPE" = "dir" ]; then
            echo "Dossiers disponibles dans $GAME_DIR:"
            DIR_LIST=$(find "$GAME_DIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort)
            i=0
            while IFS= read -r dir; do
                echo "  $((i+1)). $(basename "$dir")"
                i=$((i + 1))
            done <<< "$DIR_LIST"
            read -p "Entrez le numéro du dossier (0 pour annuler): " SELECTED_NUM
            SELECTED_NUM=$((SELECTED_NUM - 1))
            if [ $SELECTED_NUM -ge 0 ]; then
                SELECTED_ITEM=$(echo "$DIR_LIST" | sed -n "${SELECTED_NUM}p")
            else
                SELECTED_ITEM=""
            fi
        else
            echo "Entrez le chemin relatif du fichier de sauvegardes (depuis $GAME_DIR):"
            read -r REL_INPUT
            if [ -n "$REL_INPUT" ]; then
                SELECTED_ITEM="$GAME_DIR/$REL_INPUT"
            else
                SELECTED_ITEM=""
            fi
        fi
    fi

    if [ -n "$SELECTED_ITEM" ]; then
        if [ "$SAVE_TYPE" = "dir" ]; then
            if [ ! -d "$SELECTED_ITEM" ]; then
                echo "Erreur: le dossier n'existe pas"
                SAVE_LOOP=false
                continue
            fi
            SAVE_ITEM_NAME=$(basename "$SELECTED_ITEM")
            SAVE_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
            SAVE_ITEM_ABSOLUTE="$SELECTED_ITEM"

            # Vérifier que c'est bien dans le dossier du jeu
            if [[ "$SELECTED_ITEM" == "$GAME_DIR/"* ]]; then
                echo ""
                echo "Dossier de sauvegardes sélectionné: $SAVE_REL_PATH"

                # Chemins externes pour les saves
                WINDOWS_HOME="$HOME/Windows/UserData"
                SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"

                # Créer le dossier .save dans le wgp avec la structure complète
                SAVE_WGP_DIR="$GAME_DIR/.save/$SAVE_REL_PATH"
                mkdir -p "$(dirname "$SAVE_WGP_DIR")"

                # Copier le dossier vers .save (pour la portabilité du WGP)
                echo "Copie du dossier vers .save pour portabilité..."
                cp -a "$SAVE_ITEM_ABSOLUTE"/. "$SAVE_WGP_DIR/"

                # Copier vers UserData (pour le stockage local)
                mkdir -p "$SAVES_DIR/$SAVE_REL_PATH"
                cp -a "$SAVE_ITEM_ABSOLUTE"/. "$SAVES_DIR/$SAVE_REL_PATH/"

                # Créer un symlink à l'emplacement original pointant vers UserData
                echo "Création du symlink vers UserData..."
                rm -rf "$SAVE_ITEM_ABSOLUTE"
                ln -s "$SAVES_DIR/$SAVE_REL_PATH" "$SAVE_ITEM_ABSOLUTE"

                # Créer/Mettre à jour le fichier .savepath (ajouter une ligne par dossier)
                SAVE_FILE="$GAME_DIR/.savepath"
                echo "$SAVE_REL_PATH" >> "$SAVE_FILE"
                echo "Fichier .savepath mis à jour: $SAVE_FILE"
            else
                echo "Erreur: le dossier doit être dans le dossier du jeu: $GAME_DIR"
            fi
        else
            if [ ! -f "$SELECTED_ITEM" ]; then
                echo "Erreur: le fichier n'existe pas"
                SAVE_LOOP=false
                continue
            fi
            SAVE_FILE_NAME=$(basename "$SELECTED_ITEM")
            SAVE_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
            SAVE_FILE_ABSOLUTE="$SELECTED_ITEM"

            # Vérifier que c'est bien dans le dossier du jeu
            if [[ "$SELECTED_ITEM" == "$GAME_DIR/"* ]]; then
                echo ""
                echo "Fichier de sauvegardes sélectionné: $SAVE_REL_PATH"

                # Chemins externes pour les saves
                WINDOWS_HOME="$HOME/Windows/UserData"
                SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"

                # Créer le dossier .save dans le wgp avec la structure complète
                SAVE_WGP_DIR="$GAME_DIR/.save/$SAVE_REL_PATH"
                mkdir -p "$(dirname "$SAVE_WGP_DIR")"

                # Copier le fichier vers .save (pour la portabilité du WGP)
                echo "Copie du fichier vers .save pour portabilité..."
                cp "$SAVE_FILE_ABSOLUTE" "$SAVE_WGP_DIR"

                # Copier vers UserData (pour le stockage local)
                mkdir -p "$(dirname "$SAVES_DIR/$SAVE_REL_PATH")"
                cp "$SAVE_FILE_ABSOLUTE" "$SAVES_DIR/$SAVE_REL_PATH"

                # Créer un symlink à l'emplacement original pointant vers UserData
                echo "Création du symlink vers UserData..."
                rm -f "$SAVE_FILE_ABSOLUTE"
                ln -s "$SAVES_DIR/$SAVE_REL_PATH" "$SAVE_FILE_ABSOLUTE"

                # Créer/Mettre à jour le fichier .savepath (ajouter une ligne par fichier)
                SAVE_FILE="$GAME_DIR/.savepath"
                echo "$SAVE_REL_PATH" >> "$SAVE_FILE"
                echo "Fichier .savepath mis à jour: $SAVE_FILE"
            else
                echo "Erreur: le fichier doit être dans le dossier du jeu: $GAME_DIR"
            fi
        fi
    else
        echo "Aucun élément sélectionné."
        SAVE_LOOP=false
    fi
done

echo ""
echo "=== Gestion des fichiers d'extra ==="

EXTRA_REL_PATH=""
EXTRA_FILE_ABSOLUTE=""
EXTRA_FILE_NAME=""

# Créer le dossier .extra dans le wgp
EXTRA_WGP_DIR=""
EXTRAPATH_FILE=""

# Boucle pour ajouter plusieurs fichiers d'extra
EXTRA_LOOP=true
while [ "$EXTRA_LOOP" = true ]; do
    # Demander si le jeu a un fichier ou dossier d'extra à conserver
    EXTRA_ENABLED=false
    if command -v kdialog &> /dev/null; then
        kdialog --yesno "Fichier/dossier d'extra à conserver ?\\n\\nLes extras seront stockés dans /tmp/wgp-extra\\n\\nCe sont des fichiers temporaires (config, cache...)\\nperdus après la fermeture du jeu.\\n\\nLe WGP utilisera un symlink vers /tmp." --yes-label "Oui" --no-label "Non"
        if [ $? -eq 0 ]; then
            EXTRA_ENABLED=true
        fi
    else
        read -p "Y a-t-il un fichier ou dossier d'extra à conserver ? (o/N): " EXTRA_INPUT
        if [[ "$EXTRA_INPUT" =~ ^[oOyY]$ ]]; then
            EXTRA_ENABLED=true
        fi
    fi

    if [ "$EXTRA_ENABLED" = false ]; then
        EXTRA_LOOP=false
        break
    fi

    # Demander le type (fichier ou dossier)
    EXTRA_TYPE=""
    if command -v kdialog &> /dev/null; then
        EXTRA_TYPE=$(kdialog --radiolist "Que voulez-vous conserver ?" "file" "Fichier" "on" "dir" "Dossier" "off")
    else
        read -p "Type à conserver [f]ichier ou [d]ossier ? (f/D): " TYPE_INPUT
        if [[ "$TYPE_INPUT" =~ ^[fF]$ ]]; then
            EXTRA_TYPE="file"
        else
            EXTRA_TYPE="dir"
        fi
    fi

    # Utiliser le sélecteur de fichiers KDE pour fichier ou dossier
    if command -v kdialog &> /dev/null; then
        if [ "$EXTRA_TYPE" = "dir" ]; then
            SELECTED_ITEM=$(kdialog --getexistingdirectory "$GAME_DIR")
        else
            SELECTED_ITEM=$(kdialog --getopenfilename "$GAME_DIR" "Tous les fichiers (*)")
        fi
    else
        if [ "$EXTRA_TYPE" = "dir" ]; then
            echo "Dossiers disponibles dans $GAME_DIR:"
            DIR_LIST=$(find "$GAME_DIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort)
            i=0
            while IFS= read -r dir; do
                echo "  $((i+1)). $(basename "$dir")"
                i=$((i + 1))
            done <<< "$DIR_LIST"
            read -p "Entrez le numéro du dossier (0 pour annuler): " SELECTED_NUM
            SELECTED_NUM=$((SELECTED_NUM - 1))
            if [ $SELECTED_NUM -ge 0 ]; then
                SELECTED_ITEM=$(echo "$DIR_LIST" | sed -n "${SELECTED_NUM}p")
            else
                SELECTED_ITEM=""
            fi
        else
            echo "Entrez le chemin relatif du fichier d'extra (depuis $GAME_DIR):"
            read -r REL_INPUT
            if [ -n "$REL_INPUT" ]; then
                SELECTED_ITEM="$GAME_DIR/$REL_INPUT"
            else
                SELECTED_ITEM=""
            fi
        fi
    fi

    if [ -n "$SELECTED_ITEM" ]; then
        if [ "$EXTRA_TYPE" = "dir" ]; then
            if [ ! -d "$SELECTED_ITEM" ]; then
                echo "Erreur: le dossier n'existe pas"
                EXTRA_LOOP=false
                continue
            fi
            EXTRA_ITEM_NAME=$(basename "$SELECTED_ITEM")
            EXTRA_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
            EXTRA_ITEM_ABSOLUTE="$SELECTED_ITEM"

            # Vérifier que c'est bien dans le dossier du jeu
            if [[ "$SELECTED_ITEM" == "$GAME_DIR/"* ]]; then
                echo ""
                echo "Dossier d'extra sélectionné: $EXTRA_REL_PATH"

                # Chemin externe pour les extras
                EXTRA_BASE="/tmp/wgp-extra"
                EXTRA_DIR="$EXTRA_BASE/$GAME_NAME"

                # Créer le dossier .extra dans le wgp avec la structure complète
                EXTRA_WGP_DIR="$GAME_DIR/.extra/$EXTRA_REL_PATH"
                mkdir -p "$(dirname "$EXTRA_WGP_DIR")"

                # Copier le dossier vers .extra (pour la portabilité du WGP)
                echo "Copie du dossier vers .extra pour portabilité..."
                cp -a "$EXTRA_ITEM_ABSOLUTE"/. "$EXTRA_WGP_DIR/"

                # Créer un symlink à l'emplacement original pointant vers /tmp/wgp-extra
                echo "Création du symlink vers /tmp/wgp-extra..."
                rm -rf "$EXTRA_ITEM_ABSOLUTE"
                ln -s "$EXTRA_DIR/$EXTRA_REL_PATH" "$EXTRA_ITEM_ABSOLUTE"

                # Créer/Mettre à jour le fichier .extrapath (ajouter une ligne par dossier)
                EXTRAPATH_FILE="$GAME_DIR/.extrapath"
                echo "$EXTRA_REL_PATH" >> "$EXTRAPATH_FILE"
                echo "Fichier .extrapath mis à jour: $EXTRAPATH_FILE"
            else
                echo "Erreur: le dossier doit être dans le dossier du jeu: $GAME_DIR"
            fi
        else
            if [ ! -f "$SELECTED_ITEM" ]; then
                echo "Erreur: le fichier n'existe pas"
                EXTRA_LOOP=false
                continue
            fi
            EXTRA_FILE_NAME=$(basename "$SELECTED_ITEM")
            EXTRA_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
            EXTRA_FILE_ABSOLUTE="$SELECTED_ITEM"

            # Vérifier que c'est bien dans le dossier du jeu
            if [[ "$SELECTED_ITEM" == "$GAME_DIR/"* ]]; then
                echo ""
                echo "Fichier d'extra sélectionné: $EXTRA_REL_PATH"

                # Chemin externe pour les extras
                EXTRA_BASE="/tmp/wgp-extra"
                EXTRA_DIR="$EXTRA_BASE/$GAME_NAME"

                # Créer le dossier .extra dans le wgp avec la structure complète
                EXTRA_WGP_DIR="$GAME_DIR/.extra/$EXTRA_REL_PATH"
                mkdir -p "$(dirname "$EXTRA_WGP_DIR")"

                # Copier le fichier vers .extra (pour la portabilité du WGP)
                echo "Copie du fichier vers .extra pour portabilité..."
                cp "$EXTRA_FILE_ABSOLUTE" "$EXTRA_WGP_DIR"

                # Créer un symlink à l'emplacement original pointant vers /tmp/wgp-extra
                echo "Création du symlink vers /tmp/wgp-extra..."
                rm -f "$EXTRA_FILE_ABSOLUTE"
                ln -s "$EXTRA_DIR/$EXTRA_REL_PATH" "$EXTRA_FILE_ABSOLUTE"

                # Créer/Mettre à jour le fichier .extrapath (ajouter une ligne par fichier)
                EXTRAPATH_FILE="$GAME_DIR/.extrapath"
                echo "$EXTRA_REL_PATH" >> "$EXTRAPATH_FILE"
                echo "Fichier .extrapath mis à jour: $EXTRAPATH_FILE"
            else
                echo "Erreur: le fichier doit être dans le dossier du jeu: $GAME_DIR"
            fi
        fi
    else
        echo "Aucun élément sélectionné."
        EXTRA_LOOP=false
    fi
done

echo ""
echo "=== Création du squashfs ==="

# Vérifier si le fichier .wgp existe déjà
if [ -f "$WGPACK_NAME" ]; then
    if command -v kdialog &> /dev/null; then
        kdialog --warningyesno "Le fichier $WGPACK_NAME existe déjà.\n\nVoulez-vous l'écraser ?"
        OVERWRITE=$?
    else
        echo "Attention: le fichier $WGPACK_NAME existe déjà."
        read -p "Voulez-vous l'écraser ? (o/N): " CONFIRM
        [[ "$CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE=0 || OVERWRITE=1
    fi

    if [ $OVERWRITE -ne 0 ]; then
        echo "Opération annulée."
        # Restituer les fichiers depuis UserData vers l'emplacement original
        WINDOWS_HOME="$HOME/Windows/UserData"
        SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
        SAVES_DIR="$SAVES_BASE/$GAME_NAME"

        if [ -f "$GAME_DIR/.savepath" ]; then
            while IFS= read -r SAVE_REL_PATH; do
                [ -n "$SAVE_REL_PATH" ] || continue
                SAVE_ORIGINAL="$GAME_DIR/$SAVE_REL_PATH"
                SAVE_EXTERNAL="$SAVES_DIR/$SAVE_REL_PATH"
                # Supprimer le symlink, le fichier existe déjà dans UserData
                rm -rf "$SAVE_ORIGINAL" 2>/dev/null
                if [ -d "$SAVE_EXTERNAL" ]; then
                    cp -a "$SAVE_EXTERNAL"/. "$SAVE_ORIGINAL/" 2>/dev/null
                elif [ -f "$SAVE_EXTERNAL" ]; then
                    cp "$SAVE_EXTERNAL" "$SAVE_ORIGINAL" 2>/dev/null
                fi
                # Supprimer la copie de UserData
                rm -rf "$SAVE_EXTERNAL" 2>/dev/null
            done < "$GAME_DIR/.savepath"
        fi
        if [ -f "$GAME_DIR/.extrapath" ]; then
            while IFS= read -r EXTRA_REL_PATH; do
                [ -n "$EXTRA_REL_PATH" ] || continue
                EXTRA_ORIGINAL="$GAME_DIR/$EXTRA_REL_PATH"
                EXTRA_WGP_ITEM="$GAME_DIR/.extra/$EXTRA_REL_PATH"
                # Restaurer depuis .extra (les extra sont uniquement dans le WGP)
                if [ -d "$EXTRA_WGP_ITEM" ]; then
                    rm -rf "$EXTRA_ORIGINAL" 2>/dev/null
                    cp -a "$EXTRA_WGP_ITEM"/. "$EXTRA_ORIGINAL/" 2>/dev/null
                elif [ -f "$EXTRA_WGP_ITEM" ]; then
                    rm -f "$EXTRA_ORIGINAL" 2>/dev/null
                    cp "$EXTRA_WGP_ITEM" "$EXTRA_ORIGINAL" 2>/dev/null
                fi
            done < "$GAME_DIR/.extrapath"
        fi
        # Nettoyer les fichiers temporaires
        rm -f "$LAUNCH_FILE" 2>/dev/null
        [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE" 2>/dev/null
        [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE" 2>/dev/null
        [ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath" 2>/dev/null
        [ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save" 2>/dev/null
        [ -f "$GAME_DIR/.extrapath" ] && rm -f "$GAME_DIR/.extrapath" 2>/dev/null
        [ -d "$GAME_DIR/.extra" ] && rm -rf "$GAME_DIR/.extra" 2>/dev/null
        exit 0
    fi

    echo "Suppression de l'ancien fichier: $WGPACK_NAME"
    rm -f "$WGPACK_NAME"
fi

if command -v kdialog &> /dev/null; then
    # Fenêtre informative avec bouton Annuler personnalisé
    kdialog --msgbox "Compression en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    KDIALOG_PID=$!

    # Lancer mksquashfs (avec sortie pour voir la progression)
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -all-root &
    MKSQUASH_PID=$!

    # Surveiller tant que mksquashfs tourne
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Si kdialog fermé = annulation
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$WGPACK_NAME"
            echo ""
            echo "Compression annulée."

            # Restituer les fichiers depuis UserData/.extra avant nettoyage
            WINDOWS_HOME="$HOME/Windows/UserData"
            SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
            SAVES_DIR="$SAVES_BASE/$GAME_NAME"

            if [ -f "$GAME_DIR/.savepath" ]; then
                while IFS= read -r SAVE_REL_PATH; do
                    [ -n "$SAVE_REL_PATH" ] || continue
                    SAVE_ORIGINAL="$GAME_DIR/$SAVE_REL_PATH"
                    SAVE_EXTERNAL="$SAVES_DIR/$SAVE_REL_PATH"
                    # Supprimer le symlink et copier depuis UserData
                    rm -rf "$SAVE_ORIGINAL" 2>/dev/null
                    if [ -d "$SAVE_EXTERNAL" ]; then
                        cp -a "$SAVE_EXTERNAL"/. "$SAVE_ORIGINAL/" 2>/dev/null
                    elif [ -f "$SAVE_EXTERNAL" ]; then
                        cp "$SAVE_EXTERNAL" "$SAVE_ORIGINAL" 2>/dev/null
                    fi
                    # Supprimer la copie de UserData
                    rm -rf "$SAVE_EXTERNAL" 2>/dev/null
                done < "$GAME_DIR/.savepath"
            fi
            if [ -f "$GAME_DIR/.extrapath" ]; then
                while IFS= read -r EXTRA_REL_PATH; do
                    [ -n "$EXTRA_REL_PATH" ] || continue
                    EXTRA_ORIGINAL="$GAME_DIR/$EXTRA_REL_PATH"
                    EXTRA_WGP_ITEM="$GAME_DIR/.extra/$EXTRA_REL_PATH"
                    # Restaurer depuis .extra (les extra sont uniquement dans le WGP)
                    if [ -d "$EXTRA_WGP_ITEM" ]; then
                        rm -rf "$EXTRA_ORIGINAL" 2>/dev/null
                        cp -a "$EXTRA_WGP_ITEM"/. "$EXTRA_ORIGINAL/" 2>/dev/null
                    elif [ -f "$EXTRA_WGP_ITEM" ]; then
                        rm -f "$EXTRA_ORIGINAL" 2>/dev/null
                        cp "$EXTRA_WGP_ITEM" "$EXTRA_ORIGINAL" 2>/dev/null
                    fi
                done < "$GAME_DIR/.extrapath"
            fi

            # Nettoyer les fichiers temporaires
            rm -f "$LAUNCH_FILE"
            [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE"
            [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE"
            [ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath"
            [ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save"
            [ -f "$GAME_DIR/.extrapath" ] && rm -f "$GAME_DIR/.extrapath"
            [ -d "$GAME_DIR/.extra" ] && rm -rf "$GAME_DIR/.extra"
            exit 0
        fi
        sleep 0.2
    done

    # Fermer kdialog si encore ouvert
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $MKSQUASH_PID
    EXIT_CODE=$?

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        # Restituer les fichiers avant de quitter
        WINDOWS_HOME="$HOME/Windows/UserData"
        SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
        SAVES_DIR="$SAVES_BASE/$GAME_NAME"

        if [ -f "$GAME_DIR/.savepath" ]; then
            while IFS= read -r SAVE_REL_PATH; do
                [ -n "$SAVE_REL_PATH" ] || continue
                SAVE_ORIGINAL="$GAME_DIR/$SAVE_REL_PATH"
                SAVE_EXTERNAL="$SAVES_DIR/$SAVE_REL_PATH"
                # Supprimer le symlink et copier depuis UserData
                rm -rf "$SAVE_ORIGINAL" 2>/dev/null
                if [ -d "$SAVE_EXTERNAL" ]; then
                    cp -a "$SAVE_EXTERNAL"/. "$SAVE_ORIGINAL/" 2>/dev/null
                elif [ -f "$SAVE_EXTERNAL" ]; then
                    cp "$SAVE_EXTERNAL" "$SAVE_ORIGINAL" 2>/dev/null
                fi
                # Supprimer la copie de UserData
                rm -rf "$SAVE_EXTERNAL" 2>/dev/null
            done < "$GAME_DIR/.savepath"
        fi
        if [ -f "$GAME_DIR/.extrapath" ]; then
            while IFS= read -r EXTRA_REL_PATH; do
                [ -n "$EXTRA_REL_PATH" ] || continue
                EXTRA_ORIGINAL="$GAME_DIR/$EXTRA_REL_PATH"
                EXTRA_WGP_ITEM="$GAME_DIR/.extra/$EXTRA_REL_PATH"
                # Restaurer depuis .extra (les extra sont uniquement dans le WGP)
                if [ -d "$EXTRA_WGP_ITEM" ]; then
                    rm -rf "$EXTRA_ORIGINAL" 2>/dev/null
                    cp -a "$EXTRA_WGP_ITEM"/. "$EXTRA_ORIGINAL/" 2>/dev/null
                elif [ -f "$EXTRA_WGP_ITEM" ]; then
                    rm -f "$EXTRA_ORIGINAL" 2>/dev/null
                    cp "$EXTRA_WGP_ITEM" "$EXTRA_ORIGINAL" 2>/dev/null
                fi
            done < "$GAME_DIR/.extrapath"
        fi
        rm -f "$LAUNCH_FILE" 2>/dev/null
        [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE" 2>/dev/null
        [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE" 2>/dev/null
        [ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath" 2>/dev/null
        [ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save" 2>/dev/null
        [ -f "$GAME_DIR/.extrapath" ] && rm -f "$GAME_DIR/.extrapath" 2>/dev/null
        [ -d "$GAME_DIR/.extra" ] && rm -rf "$GAME_DIR/.extra" 2>/dev/null
        exit 1
    fi
else
    # Mode console: simplement lancer mksquashfs
    echo "Création de $WGPACK_NAME en cours..."
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -nopad

    if [ $? -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        # Restituer les fichiers avant de quitter
        WINDOWS_HOME="$HOME/Windows/UserData"
        SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
        SAVES_DIR="$SAVES_BASE/$GAME_NAME"

        if [ -f "$GAME_DIR/.savepath" ]; then
            while IFS= read -r SAVE_REL_PATH; do
                [ -n "$SAVE_REL_PATH" ] || continue
                SAVE_ORIGINAL="$GAME_DIR/$SAVE_REL_PATH"
                SAVE_EXTERNAL="$SAVES_DIR/$SAVE_REL_PATH"
                # Supprimer le symlink et copier depuis UserData
                rm -rf "$SAVE_ORIGINAL" 2>/dev/null
                if [ -d "$SAVE_EXTERNAL" ]; then
                    cp -a "$SAVE_EXTERNAL"/. "$SAVE_ORIGINAL/" 2>/dev/null
                elif [ -f "$SAVE_EXTERNAL" ]; then
                    cp "$SAVE_EXTERNAL" "$SAVE_ORIGINAL" 2>/dev/null
                fi
                # Supprimer la copie de UserData
                rm -rf "$SAVE_EXTERNAL" 2>/dev/null
            done < "$GAME_DIR/.savepath"
        fi
        if [ -f "$GAME_DIR/.extrapath" ]; then
            while IFS= read -r EXTRA_REL_PATH; do
                [ -n "$EXTRA_REL_PATH" ] || continue
                EXTRA_ORIGINAL="$GAME_DIR/$EXTRA_REL_PATH"
                EXTRA_WGP_ITEM="$GAME_DIR/.extra/$EXTRA_REL_PATH"
                # Restaurer depuis .extra (les extra sont uniquement dans le WGP)
                if [ -d "$EXTRA_WGP_ITEM" ]; then
                    rm -rf "$EXTRA_ORIGINAL" 2>/dev/null
                    cp -a "$EXTRA_WGP_ITEM"/. "$EXTRA_ORIGINAL/" 2>/dev/null
                elif [ -f "$EXTRA_WGP_ITEM" ]; then
                    rm -f "$EXTRA_ORIGINAL" 2>/dev/null
                    cp "$EXTRA_WGP_ITEM" "$EXTRA_ORIGINAL" 2>/dev/null
                fi
            done < "$GAME_DIR/.extrapath"
        fi
        rm -f "$LAUNCH_FILE" 2>/dev/null
        [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE" 2>/dev/null
        [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE" 2>/dev/null
        [ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath" 2>/dev/null
        [ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save" 2>/dev/null
        [ -f "$GAME_DIR/.extrapath" ] && rm -f "$GAME_DIR/.extrapath" 2>/dev/null
        [ -d "$GAME_DIR/.extra" ] && rm -rf "$GAME_DIR/.extra" 2>/dev/null
        exit 1
    fi
fi

# Calcul des tailles
SIZE_BEFORE=$(du -s "$GAME_DIR" | cut -f1)
SIZE_BEFORE_GB=$(echo "scale=2; $SIZE_BEFORE / 1024 / 1024" | bc)
SIZE_AFTER=$(du -s "$WGPACK_NAME" | cut -f1)
SIZE_AFTER_GB=$(echo "scale=2; $SIZE_AFTER / 1024 / 1024" | bc)
COMPRESSION_RATIO=$(echo "scale=1; (1 - $SIZE_AFTER / $SIZE_BEFORE) * 100" | bc)

# Restituer les fichiers originaux depuis UserData en supprimant les symlinks
echo ""
echo "=== Restitution des fichiers originaux ==="

WINDOWS_HOME="$HOME/Windows/UserData"
SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
SAVES_DIR="$SAVES_BASE/$GAME_NAME"

# Restitution des saves (depuis UserData)
if [ -f "$GAME_DIR/.savepath" ]; then
    while IFS= read -r SAVE_REL_PATH; do
        if [ -n "$SAVE_REL_PATH" ]; then
            SAVE_ORIGINAL="$GAME_DIR/$SAVE_REL_PATH"
            SAVE_EXTERNAL="$SAVES_DIR/$SAVE_REL_PATH"

            # Supprimer le symlink
            rm -rf "$SAVE_ORIGINAL"

            if [ -d "$SAVE_EXTERNAL" ]; then
                # Dossier : copier depuis UserData
                cp -a "$SAVE_EXTERNAL"/. "$SAVE_ORIGINAL/"
                echo "Save restaurée: $SAVE_REL_PATH"
            elif [ -f "$SAVE_EXTERNAL" ]; then
                # Fichier : copier depuis UserData
                cp "$SAVE_EXTERNAL" "$SAVE_ORIGINAL"
                echo "Save restaurée: $SAVE_REL_PATH"
            fi

            # Supprimer la copie de UserData (elle reste dans le WGP pour la portabilité)
            rm -rf "$SAVE_EXTERNAL"
        fi
    done < "$GAME_DIR/.savepath"
fi

# Restitution des extras
if [ -f "$GAME_DIR/.extrapath" ]; then
    while IFS= read -r EXTRA_REL_PATH; do
        if [ -n "$EXTRA_REL_PATH" ]; then
            EXTRA_ORIGINAL="$GAME_DIR/$EXTRA_REL_PATH"
            EXTRA_WGP_ITEM="$GAME_DIR/.extra/$EXTRA_REL_PATH"

            if [ -d "$EXTRA_WGP_ITEM" ]; then
                # Dossier : déplacer le contenu vers l'emplacement original
                rm -rf "$EXTRA_ORIGINAL"
                cp -a "$EXTRA_WGP_ITEM"/. "$EXTRA_ORIGINAL/"
                echo "Extra restauré: $EXTRA_REL_PATH"
            elif [ -f "$EXTRA_WGP_ITEM" ]; then
                # Fichier : déplacer vers l'emplacement original
                rm -f "$EXTRA_ORIGINAL"
                cp "$EXTRA_WGP_ITEM" "$EXTRA_ORIGINAL"
                echo "Extra restauré: $EXTRA_REL_PATH"
            fi
        fi
    done < "$GAME_DIR/.extrapath"
fi

# Supprimer les fichiers temporaires du dossier source
rm -f "$LAUNCH_FILE"
[ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE"
[ -f "$FIX_FILE" ] && rm -f "$FIX_FILE"
[ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath"
[ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save"
[ -f "$GAME_DIR/.extrapath" ] && rm -f "$GAME_DIR/.extrapath"
[ -d "$GAME_DIR/.extra" ] && rm -rf "$GAME_DIR/.extra"

echo ""
echo "=== Paquet créé avec succès ==="
echo "Fichier: $WGPACK_NAME"
echo "Taille avant: ${SIZE_BEFORE_GB} Go"
echo "Taille après: ${SIZE_AFTER_GB} Go"
echo "Gain: ${COMPRESSION_RATIO}%"
echo "Exécutable: $EXE_REL_PATH"
[ -n "$BOTTLE_ARGS" ] && echo "Arguments: $BOTTLE_ARGS"

# Fenêtre de succès
if command -v kdialog &> /dev/null; then
    if [ -n "$BOTTLE_ARGS" ]; then
        kdialog --title "Succès" --msgbox "Paquet créé avec succès !\n\nFichier: $WGPACK_NAME\n\nTaille avant: ${SIZE_BEFORE_GB} Go\nTaille après: ${SIZE_AFTER_GB} Go\nGain: ${COMPRESSION_RATIO}%\n\nExécutable: $EXE_REL_PATH\nArguments: $BOTTLE_ARGS"
    else
        kdialog --title "Succès" --msgbox "Paquet créé avec succès !\n\nFichier: $WGPACK_NAME\n\nTaille avant: ${SIZE_BEFORE_GB} Go\nTaille après: ${SIZE_AFTER_GB} Go\nGain: ${COMPRESSION_RATIO}%\n\nExécutable: $EXE_REL_PATH"
    fi
fi
