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

# Chemins pour la restauration
WINDOWS_HOME="$HOME/Windows/UserData"
SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
SAVES_DIR="$SAVES_BASE/$GAME_NAME"

# Fonction de restauration des sauvegardes et options
restore_game_files() {
    echo "Restauration des fichiers dans le dossier de jeu..."

    # Restaurer les sauvegardes depuis UserData
    if [ -f "$GAME_DIR/.savepath" ]; then
        while IFS= read -r SAVE_REL_PATH; do
            if [ -n "$SAVE_REL_PATH" ]; then
                SAVE_ITEM="$GAME_DIR/$SAVE_REL_PATH"
                FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

                # Supprimer le symlink ou le dossier existant
                if [ -L "$SAVE_ITEM" ] || [ -d "$SAVE_ITEM" ]; then
                    echo "Restauration des sauvegardes: $SAVE_REL_PATH"
                    rm -rf "$SAVE_ITEM"

                    # Restaurer depuis UserData
                    if [ -d "$FINAL_SAVE_ITEM" ]; then
                        mkdir -p "$(dirname "$SAVE_ITEM")"
                        cp -r "$FINAL_SAVE_ITEM/"* "$SAVE_ITEM/" 2>/dev/null
                    elif [ -f "$FINAL_SAVE_ITEM" ]; then
                        mkdir -p "$(dirname "$SAVE_ITEM")"
                        cp "$FINAL_SAVE_ITEM" "$SAVE_ITEM"
                    fi
                elif [ -d "$FINAL_SAVE_ITEM" ] && [ ! -e "$SAVE_ITEM" ]; then
                    # Dossier n'existe pas encore, restaurer depuis UserData
                    echo "Restauration des sauvegardes: $SAVE_REL_PATH"
                    mkdir -p "$SAVE_ITEM"
                    cp -r "$FINAL_SAVE_ITEM/"* "$SAVE_ITEM/" 2>/dev/null
                fi
            fi
        done < "$GAME_DIR/.savepath"
    fi

    # Restaurer les options depuis UserData
    if [ -f "$GAME_DIR/.keeppath" ]; then
        while IFS= read -r KEEP_REL_PATH; do
            if [ -n "$KEEP_REL_PATH" ]; then
                KEEP_ITEM="$GAME_DIR/$KEEP_REL_PATH"
                FINAL_KEEP_ITEM="$SAVES_DIR/$KEEP_REL_PATH"

                # Supprimer le symlink ou le dossier existant
                if [ -L "$KEEP_ITEM" ] || [ -d "$KEEP_ITEM" ]; then
                    echo "Restauration des options: $KEEP_REL_PATH"
                    rm -rf "$KEEP_ITEM"

                    # Restaurer depuis UserData
                    if [ -d "$FINAL_KEEP_ITEM" ]; then
                        mkdir -p "$(dirname "$KEEP_ITEM")"
                        cp -r "$FINAL_KEEP_ITEM/"* "$KEEP_ITEM/" 2>/dev/null
                    elif [ -f "$FINAL_KEEP_ITEM" ]; then
                        mkdir -p "$(dirname "$KEEP_ITEM")"
                        cp "$FINAL_KEEP_ITEM" "$KEEP_ITEM"
                    fi
                elif [ -d "$FINAL_KEEP_ITEM" ] && [ ! -e "$KEEP_ITEM" ]; then
                    # Dossier n'existe pas encore, restaurer depuis UserData
                    echo "Restauration des options: $KEEP_REL_PATH"
                    mkdir -p "$KEEP_ITEM"
                    cp -r "$FINAL_KEEP_ITEM/"* "$KEEP_ITEM/" 2>/dev/null
                fi
            fi
        done < "$GAME_DIR/.keeppath"
    fi

    echo "Restauration terminée."
}

# Cleanup en cas d'interruption (Ctrl+C)
trap 'echo ""; echo "Interruption détectée, restauration en cours..."; restore_game_files; exit 1' INT

echo "=== Création du paquet pour: $GAME_NAME ==="
echo "Dossier source: $GAME_DIR"

# Nettoyer les dossiers temporaires d'une exécution précédente
echo ""
echo "Nettoyage des dossiers temporaires..."
rm -rf "$GAME_DIR/.save" "$GAME_DIR/.keep"
rm -f "$GAME_DIR/.savepath" "$GAME_DIR/.keeppath"
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
        kdialog --yesno "Y a-t-il un dossier ou fichier de sauvegarde à gérer ?\\n\\nIl sera déplacé dans le dossier utilisateur\\net remplacé par un lien symbolique dans le paquet.\\n\\nUne copie sera conservée dans le dossier .save\\npour permettre la restauration sur un autre ordi." --yes-label "Oui" --no-label "Non"
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
                # Chemin vers le dossier de saves externe
                WINDOWS_HOME="$HOME/Windows/UserData"
                SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"
                FINAL_SAVE_DIR="$SAVES_DIR/$SAVE_REL_PATH"

                # Créer les dossiers parents
                mkdir -p "$(dirname "$FINAL_SAVE_DIR")"

                echo ""
                echo "Dossier de sauvegardes sélectionné: $SAVE_REL_PATH"
                echo "Destination: $FINAL_SAVE_DIR"

                # Vérifier si des saves existent déjà
                if [ -d "$FINAL_SAVE_DIR" ] && [ "$(ls -A "$FINAL_SAVE_DIR" 2>/dev/null)" ]; then
                    echo ""
                    echo "Attention: le dossier de sauvegardes existe déjà et contient des fichiers."
                    echo "Dossier: $FINAL_SAVE_DIR"

                    OVERWRITE_SAVES=1
                    if command -v kdialog &> /dev/null; then
                        kdialog --warningyesno "Le dossier de sauvegardes existe déjà:\\n\\n$FINAL_SAVE_DIR\\n\\nVoulez-vous l'écraser ?" --yes-label "Oui" --no-label "Non"
                        OVERWRITE_SAVES=$?
                    else
                        read -p "Voulez-vous l'écraser ? (o/N): " SAVE_CONFIRM
                        [[ "$SAVE_CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE_SAVES=0 || OVERWRITE_SAVES=1
                    fi

                    if [ $OVERWRITE_SAVES -ne 0 ]; then
                        echo "Conserve les sauvegardes existantes (les anciennes seront remplacées)"
                    else
                        echo "Remplacement des sauvegardes existantes..."
                        rm -rf "$FINAL_SAVE_DIR"
                    fi
                fi

                # Créer le dossier final si nécessaire (sans créer la destination de mv)
                mkdir -p "$(dirname "$FINAL_SAVE_DIR")"

                # Créer le dossier .save dans le wgp avec la structure complète
                SAVE_WGP_DIR="$GAME_DIR/.save/$SAVE_REL_PATH"
                mkdir -p "$(dirname "$SAVE_WGP_DIR")"

                # Copier le contenu du dossier vers le dossier .save (sauvegarde pour restauration)
                echo "Copie du contenu du dossier dans .save..."
                mkdir -p "$SAVE_WGP_DIR"
                cp -r "$SAVE_ITEM_ABSOLUTE/."* "$SAVE_WGP_DIR/" 2>/dev/null

                # Déplacer le dossier vers UserData pour le WGP (création du symlink temporaire)
                echo "Déplacement du dossier vers $FINAL_SAVE_DIR..."
                mv "$SAVE_ITEM_ABSOLUTE" "$FINAL_SAVE_DIR"
                ln -s "$FINAL_SAVE_DIR" "$SAVE_ITEM_ABSOLUTE"

                echo "Dossier de sauvegardes déplacé et lié (temporaire)."

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
                # Chemin vers le dossier de saves externe
                WINDOWS_HOME="$HOME/Windows/UserData"
                SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"
                FINAL_SAVE_FILE="$SAVES_DIR/$SAVE_REL_PATH"

                # Créer les dossiers parents
                mkdir -p "$(dirname "$FINAL_SAVE_FILE")"

                echo ""
                echo "Fichier de sauvegardes sélectionné: $SAVE_REL_PATH"
                echo "Destination: $FINAL_SAVE_FILE"

                # Vérifier si le fichier existe déjà
                if [ -f "$FINAL_SAVE_FILE" ]; then
                    echo ""
                    echo "Attention: le fichier de sauvegardes existe déjà."
                    echo "Fichier: $FINAL_SAVE_FILE"

                    OVERWRITE_SAVE=1
                    if command -v kdialog &> /dev/null; then
                        kdialog --warningyesno "Le fichier de sauvegardes existe déjà:\\n\\n$FINAL_SAVE_FILE\\n\\nVoulez-vous l'écraser ?" --yes-label "Oui" --no-label "Non"
                        OVERWRITE_SAVE=$?
                    else
                        read -p "Voulez-vous l'écraser ? (o/N): " SAVE_CONFIRM
                        [[ "$SAVE_CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE_SAVE=0 || OVERWRITE_SAVE=1
                    fi

                    if [ $OVERWRITE_SAVE -ne 0 ]; then
                        echo "Conserve le fichier existant"
                    else
                        echo "Remplacement du fichier existant..."
                        rm -f "$FINAL_SAVE_FILE"
                    fi
                fi

                # Créer le dossier .save dans le wgp avec la structure complète
                SAVE_WGP_DIR="$GAME_DIR/.save/$SAVE_REL_PATH"
                mkdir -p "$(dirname "$SAVE_WGP_DIR")"

                # Copier le fichier vers le dossier .save (sauvegarde pour restauration)
                echo "Copie du fichier dans .save..."
                cp "$SAVE_FILE_ABSOLUTE" "$SAVE_WGP_DIR"

                # Déplacer le fichier vers UserData pour le WGP (création du symlink temporaire)
                echo "Déplacement du fichier vers $FINAL_SAVE_FILE..."
                mv "$SAVE_FILE_ABSOLUTE" "$FINAL_SAVE_FILE"
                ln -s "$FINAL_SAVE_FILE" "$SAVE_FILE_ABSOLUTE"

                echo "Fichier de sauvegardes déplacé et lié (temporaire)."

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
echo "=== Gestion des fichiers d'options ==="

KEEP_REL_PATH=""
KEEP_FILE_ABSOLUTE=""
KEEP_FILE_NAME=""

# Créer le dossier .keep dans le wgp
KEEP_WGP_DIR=""
KEEPPATH_FILE=""

# Boucle pour ajouter plusieurs fichiers d'options
KEEP_LOOP=true
while [ "$KEEP_LOOP" = true ]; do
    # Demander si le jeu a un fichier ou dossier d'options à conserver
    KEEP_ENABLED=false
    if command -v kdialog &> /dev/null; then
        kdialog --yesno "Y a-t-il un fichier ou dossier d'options de configuration à conserver ?\\n\\nIl sera déplacé dans le dossier utilisateur\\net remplacé par un lien symbolique dans le paquet.\\n\\nUne copie sera conservée dans le dossier .keep\\npour permettre la restauration sur un autre ordi." --yes-label "Oui" --no-label "Non"
        if [ $? -eq 0 ]; then
            KEEP_ENABLED=true
        fi
    else
        read -p "Y a-t-il un fichier ou dossier d'options à conserver ? (o/N): " KEEP_INPUT
        if [[ "$KEEP_INPUT" =~ ^[oOyY]$ ]]; then
            KEEP_ENABLED=true
        fi
    fi

    if [ "$KEEP_ENABLED" = false ]; then
        KEEP_LOOP=false
        break
    fi

    # Demander le type (fichier ou dossier)
    KEEP_TYPE=""
    if command -v kdialog &> /dev/null; then
        KEEP_TYPE=$(kdialog --radiolist "Que voulez-vous conserver ?" "file" "Fichier" "on" "dir" "Dossier" "off")
    else
        read -p "Type à conserver [f]ichier ou [d]ossier ? (f/D): " TYPE_INPUT
        if [[ "$TYPE_INPUT" =~ ^[fF]$ ]]; then
            KEEP_TYPE="file"
        else
            KEEP_TYPE="dir"
        fi
    fi

    # Utiliser le sélecteur de fichiers KDE pour fichier ou dossier
    if command -v kdialog &> /dev/null; then
        if [ "$KEEP_TYPE" = "dir" ]; then
            SELECTED_ITEM=$(kdialog --getexistingdirectory "$GAME_DIR")
        else
            SELECTED_ITEM=$(kdialog --getopenfilename "$GAME_DIR" "Tous les fichiers (*)")
        fi
    else
        if [ "$KEEP_TYPE" = "dir" ]; then
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
            echo "Entrez le chemin relatif du fichier d'options (depuis $GAME_DIR):"
            read -r REL_INPUT
            if [ -n "$REL_INPUT" ]; then
                SELECTED_ITEM="$GAME_DIR/$REL_INPUT"
            else
                SELECTED_ITEM=""
            fi
        fi
    fi

    if [ -n "$SELECTED_ITEM" ]; then
        if [ "$KEEP_TYPE" = "dir" ]; then
            if [ ! -d "$SELECTED_ITEM" ]; then
                echo "Erreur: le dossier n'existe pas"
                KEEP_LOOP=false
                continue
            fi
            KEEP_ITEM_NAME=$(basename "$SELECTED_ITEM")
            KEEP_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
            KEEP_ITEM_ABSOLUTE="$SELECTED_ITEM"

            # Vérifier que c'est bien dans le dossier du jeu
            if [[ "$SELECTED_ITEM" == "$GAME_DIR/"* ]]; then
                # Chemin vers le dossier de saves externe
                WINDOWS_HOME="$HOME/Windows/UserData"
                SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"
                FINAL_KEEP_DIR="$SAVES_DIR/$KEEP_REL_PATH"

                # Créer les dossiers parents
                mkdir -p "$(dirname "$FINAL_KEEP_DIR")"

                echo ""
                echo "Dossier d'options sélectionné: $KEEP_REL_PATH"
                echo "Destination: $FINAL_KEEP_DIR"

                # Vérifier si le dossier existe déjà
                if [ -d "$FINAL_KEEP_DIR" ]; then
                    echo ""
                    echo "Attention: le dossier d'options existe déjà."
                    echo "Dossier: $FINAL_KEEP_DIR"

                    OVERWRITE_KEEP=1
                    if command -v kdialog &> /dev/null; then
                        kdialog --warningyesno "Le dossier d'options existe déjà:\\n\\n$FINAL_KEEP_DIR\\n\\nVoulez-vous l'écraser ?" --yes-label "Oui" --no-label "Non"
                        OVERWRITE_KEEP=$?
                    else
                        read -p "Voulez-vous l'écraser ? (o/N): " KEEP_CONFIRM
                        [[ "$KEEP_CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE_KEEP=0 || OVERWRITE_KEEP=1
                    fi

                    if [ $OVERWRITE_KEEP -ne 0 ]; then
                        echo "Conserve le dossier existant (les anciens fichiers seront remplacés)"
                    else
                        echo "Remplacement du dossier existant..."
                        rm -rf "$FINAL_KEEP_DIR"
                    fi
                fi

                # Créer le dossier .keep dans le wgp avec la structure complète
                KEEP_WGP_DIR="$GAME_DIR/.keep/$KEEP_REL_PATH"
                mkdir -p "$(dirname "$KEEP_WGP_DIR")"

                # Copier le contenu du dossier vers le dossier .keep (sauvegarde pour restauration)
                echo "Copie du contenu du dossier dans .keep..."
                mkdir -p "$KEEP_WGP_DIR"
                cp -r "$KEEP_ITEM_ABSOLUTE/."* "$KEEP_WGP_DIR/" 2>/dev/null

                # Déplacer le dossier vers UserData pour le WGP (création du symlink temporaire)
                echo "Déplacement du dossier vers $FINAL_KEEP_DIR..."
                mv "$KEEP_ITEM_ABSOLUTE" "$FINAL_KEEP_DIR"
                ln -s "$FINAL_KEEP_DIR" "$KEEP_ITEM_ABSOLUTE"

                echo "Dossier d'options déplacé et lié (temporaire)."

                # Créer/Mettre à jour le fichier .keeppath (ajouter une ligne par dossier)
                KEEPPATH_FILE="$GAME_DIR/.keeppath"
                echo "$KEEP_REL_PATH" >> "$KEEPPATH_FILE"
                echo "Fichier .keeppath mis à jour: $KEEPPATH_FILE"
            else
                echo "Erreur: le dossier doit être dans le dossier du jeu: $GAME_DIR"
            fi
        else
            if [ ! -f "$SELECTED_ITEM" ]; then
                echo "Erreur: le fichier n'existe pas"
                KEEP_LOOP=false
                continue
            fi
            KEEP_FILE_NAME=$(basename "$SELECTED_ITEM")
            KEEP_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
            KEEP_FILE_ABSOLUTE="$SELECTED_ITEM"

            # Vérifier que c'est bien dans le dossier du jeu
            if [[ "$SELECTED_ITEM" == "$GAME_DIR/"* ]]; then
                # Chemin vers le dossier de saves externe
                WINDOWS_HOME="$HOME/Windows/UserData"
                SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"
                FINAL_KEEP_FILE="$SAVES_DIR/$KEEP_REL_PATH"

                # Créer les dossiers parents
                mkdir -p "$(dirname "$FINAL_KEEP_FILE")"

                echo ""
                echo "Fichier d'options sélectionné: $KEEP_REL_PATH"
                echo "Destination: $FINAL_KEEP_FILE"

                # Vérifier si le fichier existe déjà
                if [ -f "$FINAL_KEEP_FILE" ]; then
                    echo ""
                    echo "Attention: le fichier d'options existe déjà."
                    echo "Fichier: $FINAL_KEEP_FILE"

                    OVERWRITE_KEEP=1
                    if command -v kdialog &> /dev/null; then
                        kdialog --warningyesno "Le fichier d'options existe déjà:\\n\\n$FINAL_KEEP_FILE\\n\\nVoulez-vous l'écraser ?" --yes-label "Oui" --no-label "Non"
                        OVERWRITE_KEEP=$?
                    else
                        read -p "Voulez-vous l'écraser ? (o/N): " KEEP_CONFIRM
                        [[ "$KEEP_CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE_KEEP=0 || OVERWRITE_KEEP=1
                    fi

                    if [ $OVERWRITE_KEEP -ne 0 ]; then
                        echo "Conserve le fichier existant"
                    else
                        echo "Remplacement du fichier existant..."
                        rm -f "$FINAL_KEEP_FILE"
                    fi
                fi

                # Créer le dossier .keep dans le wgp avec la structure complète
                KEEP_WGP_DIR="$GAME_DIR/.keep/$KEEP_REL_PATH"
                mkdir -p "$(dirname "$KEEP_WGP_DIR")"

                # Copier le fichier vers le dossier .keep (sauvegarde pour restauration)
                echo "Copie du fichier dans .keep..."
                cp "$KEEP_FILE_ABSOLUTE" "$KEEP_WGP_DIR"

                # Déplacer le fichier vers UserData pour le WGP (création du symlink temporaire)
                echo "Déplacement du fichier vers $FINAL_KEEP_FILE..."
                mv "$KEEP_FILE_ABSOLUTE" "$FINAL_KEEP_FILE"
                ln -s "$FINAL_KEEP_FILE" "$KEEP_FILE_ABSOLUTE"

                echo "Fichier d'options déplacé et lié (temporaire)."

                # Créer/Mettre à jour le fichier .keeppath (ajouter une ligne par fichier)
                KEEPPATH_FILE="$GAME_DIR/.keeppath"
                echo "$KEEP_REL_PATH" >> "$KEEPPATH_FILE"
                echo "Fichier .keeppath mis à jour: $KEEPPATH_FILE"
            else
                echo "Erreur: le fichier doit être dans le dossier du jeu: $GAME_DIR"
            fi
        fi
    else
        echo "Aucun élément sélectionné."
        KEEP_LOOP=false
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
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -nopad &
    MKSQUASH_PID=$!

    # Surveiller tant que mksquashfs tourne
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Si kdialog fermé = annulation
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$WGPACK_NAME"
            echo ""
            echo "Compression annulée, restauration en cours..."
            restore_game_files
            # Nettoyer les fichiers temporaires
            rm -f "$LAUNCH_FILE"
            [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE"
            [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE"
            [ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath"
            [ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save"
            [ -f "$GAME_DIR/.keeppath" ] && rm -f "$GAME_DIR/.keeppath"
            [ -d "$GAME_DIR/.keep" ] && rm -rf "$GAME_DIR/.keep"
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
        exit 1
    fi
else
    # Mode console: simplement lancer mksquashfs
    echo "Création de $WGPACK_NAME en cours..."
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -nopad

    if [ $? -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        exit 1
    fi
fi

# Calcul des tailles
SIZE_BEFORE=$(du -s "$GAME_DIR" | cut -f1)
SIZE_BEFORE_GB=$(echo "scale=2; $SIZE_BEFORE / 1024 / 1024" | bc)
SIZE_AFTER=$(du -s "$WGPACK_NAME" | cut -f1)
SIZE_AFTER_GB=$(echo "scale=2; $SIZE_AFTER / 1024 / 1024" | bc)
COMPRESSION_RATIO=$(echo "scale=1; (1 - $SIZE_AFTER / $SIZE_BEFORE) * 100" | bc)

echo ""
echo "=== Restauration du dossier source ==="

# Appeler la fonction de restauration
restore_game_files

# Supprimer les fichiers temporaires du dossier source
rm -f "$LAUNCH_FILE"
[ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE"
[ -f "$FIX_FILE" ] && rm -f "$FIX_FILE"
[ -f "$GAME_DIR/.savepath" ] && rm -f "$GAME_DIR/.savepath"
[ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save"
[ -f "$GAME_DIR/.keeppath" ] && rm -f "$GAME_DIR/.keeppath"
[ -d "$GAME_DIR/.keep" ] && rm -rf "$GAME_DIR/.keep"

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
