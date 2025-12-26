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

SAVE_REL_PATH=""
SAVE_DIR_ABSOLUTE=""

# Demander si le jeu utilise des saves dans le dossier du jeu
SAVE_ENABLED=false
if command -v kdialog &> /dev/null; then
    kdialog --yesno "Ce jeu utilise-t-il ses sauvegardes dans le dossier du même jeu ?\\\\n\\\\nSi oui, le dossier de save sera déplacé vers\\\\n~/Windows/$USER/AppData/Local/LocalSaves/$GAME_NAME/\\\\net remplacé par un lien symbolique dans le paquet." --yes-label "Oui" --no-label "Non"
    if [ $? -eq 0 ]; then
        SAVE_ENABLED=true
    fi
else
    read -p "Le jeu utilise-t-il des sauvegardes dans le dossier du jeu ? (o/N): " SAVE_INPUT
    if [[ "$SAVE_INPUT" =~ ^[oOyY]$ ]]; then
        SAVE_ENABLED=true
    fi
fi

if [ "$SAVE_ENABLED" = true ]; then
    # Lister les dossiers dans le dossier du jeu (jusqu'à 3 niveaux de profondeur)
    DIR_LIST=$(find "$GAME_DIR" -mindepth 1 -maxdepth 3 -type d 2>/dev/null | sort)

    if [ -z "$DIR_LIST" ]; then
        echo "Aucun sous-dossier trouvé dans $GAME_DIR"
    else
        # Préparer la liste pour kdialog ou sélection console
        COUNT=0
        DIR_ARRAY=()
        while IFS= read -r dir; do
            REL_PATH="${dir#$GAME_DIR/}"
            DIR_ARRAY+=("$dir")
            DIR_ARRAY+=("$REL_PATH")
            DIR_ARRAY+=("off")
            COUNT=$((COUNT + 1))
        done <<< "$DIR_LIST"

        if [ $COUNT -gt 0 ]; then
            if command -v kdialog &> /dev/null; then
                SELECTED_DIR=$(kdialog --radiolist "Sélectionnez le dossier des sauvegardes:" "${DIR_ARRAY[@]}")
            else
                echo "Dossiers trouvés:"
                i=0
                while IFS= read -r dir; do
                    echo "  $((i+1)). ${dir#$GAME_DIR/}"
                    i=$((i + 1))
                done <<< "$DIR_LIST"
                read -p "Entrez le numéro du dossier de sauvegardes (0 pour annuler): " SELECTED_NUM
                SELECTED_NUM=$((SELECTED_NUM - 1))
                if [ $SELECTED_NUM -ge 0 ] && [ $SELECTED_NUM -lt $COUNT ]; then
                    SELECTED_DIR="${DIR_ARRAY[$((SELECTED_NUM * 3))]}"
                else
                    SELECTED_DIR=""
                fi
            fi

            if [ -n "$SELECTED_DIR" ] && [ -d "$SELECTED_DIR" ]; then
                SAVE_DIR_NAME=$(basename "$SELECTED_DIR")
                SAVE_REL_PATH="$SAVE_DIR_NAME"
                SAVE_DIR_ABSOLUTE="$SELECTED_DIR"

                # Chemin vers le dossier de saves externe
                WINDOWS_HOME="$HOME/Windows"
                SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
                SAVES_DIR="$SAVES_BASE/$GAME_NAME"
                FINAL_SAVE_DIR="$SAVES_DIR/$SAVE_DIR_NAME"

                # Créer les dossiers parents
                mkdir -p "$SAVES_DIR"

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
                        kdialog --warningyesno "Le dossier de sauvegardes existe déjà et contient des fichiers:\\n\\n$FINAL_SAVE_DIR\\n\\nVoulez-vous les écraser ?" --yes-label "Oui" --no-label "Non"
                        OVERWRITE_SAVES=$?
                    else
                        read -p "Voulez-vous les écraser ? (o/N): " SAVE_CONFIRM
                        [[ "$SAVE_CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE_SAVES=0 || OVERWRITE_SAVES=1
                    fi

                    if [ $OVERWRITE_SAVES -ne 0 ]; then
                        echo "Conserve les sauvegardes existantes (les anciennes seront remplacées)"
                    else
                        echo "Remplacement des sauvegardes existantes..."
                        rm -rf "$FINAL_SAVE_DIR"
                    fi
                fi

                # Créer le dossier final si nécessaire
                mkdir -p "$FINAL_SAVE_DIR"

                # Déplacer le contenu (récursivement)
                echo "Déplacement des sauvegardes vers $FINAL_SAVE_DIR..."
                cp -r "$SAVE_DIR_ABSOLUTE"/. "$FINAL_SAVE_DIR/"

                # Supprimer le dossier original
                rm -rf "$SAVE_DIR_ABSOLUTE"

                # Créer un lien symbolique
                echo "Création du lien symbolique dans le paquet..."
                ln -s "$FINAL_SAVE_DIR" "$SAVE_DIR_ABSOLUTE"

                echo "Sauvegardes déplacées et liées avec succès."

                # Créer le fichier .savepath
                SAVE_FILE="$GAME_DIR/.savepath"
                echo "$SAVE_REL_PATH" > "$SAVE_FILE"
                echo "Fichier .savepath créé: $SAVE_FILE"
            fi
        fi
    fi
fi

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
    # Demander si le jeu a un fichier d'options à conserver
    KEEP_ENABLED=false
    if command -v kdialog &> /dev/null; then
        kdialog --yesno "Y a-t-il un fichier d'options de configuration à conserver ?\\\\n\\\\nLe fichier sera déplacé vers\\\\n~/Windows/$USER/AppData/Local/LocalSaves/$GAME_NAME/\\\\net remplacé par un lien symbolique dans le paquet.\\\\n\\\\nUne copie sera conservée dans un dossier .keep\\\\npour permettre la restauration sur un autre ordi." --yes-label "Oui" --no-label "Non"
        if [ $? -eq 0 ]; then
            KEEP_ENABLED=true
        fi
    else
        read -p "Y a-t-il un fichier d'options à conserver ? (o/N): " KEEP_INPUT
        if [[ "$KEEP_INPUT" =~ ^[oOyY]$ ]]; then
            KEEP_ENABLED=true
        fi
    fi

    if [ "$KEEP_ENABLED" = false ]; then
        KEEP_LOOP=false
        break
    fi

    # Utiliser le sélecteur de fichiers KDE
    if command -v kdialog &> /dev/null; then
        SELECTED_FILE=$(kdialog --getopenfilename "$GAME_DIR" "Tous les fichiers (*)")
    else
        echo "Entrez le chemin relatif du fichier d'options (depuis $GAME_DIR):"
        read -r REL_INPUT
        if [ -n "$REL_INPUT" ]; then
            SELECTED_FILE="$GAME_DIR/$REL_INPUT"
        else
            SELECTED_FILE=""
        fi
    fi

    if [ -n "$SELECTED_FILE" ] && [ -f "$SELECTED_FILE" ]; then
        KEEP_FILE_NAME=$(basename "$SELECTED_FILE")
        KEEP_REL_PATH="${SELECTED_FILE#$GAME_DIR/}"
        KEEP_FILE_ABSOLUTE="$SELECTED_FILE"

        # Vérifier que c'est bien dans le dossier du jeu
        if [[ "$SELECTED_FILE" == "$GAME_DIR/"* ]]; then
            # Chemin vers le dossier de saves externe
            WINDOWS_HOME="$HOME/Windows"
            SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
            SAVES_DIR="$SAVES_BASE/$GAME_NAME"
            # Remplacer les / par _ pour éviter les collisions
            KEEP_STORED_NAME="${KEEP_REL_PATH//\//_}"
            FINAL_KEEP_FILE="$SAVES_DIR/$KEEP_STORED_NAME"

            # Créer les dossiers parents
            mkdir -p "$SAVES_DIR"

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

            # Créer le dossier .keep dans le wgp
            KEEP_WGP_DIR="$GAME_DIR/.keep"
            mkdir -p "$KEEP_WGP_DIR"

            # Copier le fichier vers le dossier .keep (sauvegarde pour restauration)
            echo "Copie du fichier dans .keep..."
            cp "$KEEP_FILE_ABSOLUTE" "$KEEP_WGP_DIR/$KEEP_FILE_NAME"

            # Déplacer le fichier vers le dossier de sauvegardes externe
            echo "Déplacement du fichier vers $FINAL_KEEP_FILE..."
            mv "$KEEP_FILE_ABSOLUTE" "$FINAL_KEEP_FILE"

            # Créer un lien symbolique
            echo "Création du lien symbolique dans le paquet..."
            ln -s "$FINAL_KEEP_FILE" "$KEEP_FILE_ABSOLUTE"

            echo "Fichier d'options déplacé et lié avec succès."

            # Créer/Mettre à jour le fichier .keeppath (ajouter une ligne par fichier)
            KEEPPATH_FILE="$GAME_DIR/.keeppath"
            echo "$KEEP_REL_PATH" >> "$KEEPPATH_FILE"
            echo "Fichier .keeppath mis à jour: $KEEPPATH_FILE"
        else
            echo "Erreur: le fichier doit être dans le dossier du jeu: $GAME_DIR"
        fi
    else
        echo "Aucun fichier sélectionné."
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
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -all-root &
    MKSQUASH_PID=$!

    # Surveiller tant que mksquashfs tourne
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Si kdialog fermé = annulation
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$WGPACK_NAME"
            rm -f "$LAUNCH_FILE"
            [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE"
            [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE"
            [ -f "$SAVE_FILE" ] && rm -f "$SAVE_FILE"
            [ -f "$KEEPPATH_FILE" ] && rm -f "$KEEPPATH_FILE"
            [ -d "$KEEP_WGP_DIR" ] && rm -rf "$KEEP_WGP_DIR"
            echo ""
            echo "Compression annulée"
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
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -all-root

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

# Supprimer les fichiers temporaires du dossier source
rm -f "$LAUNCH_FILE"
[ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE"
[ -f "$FIX_FILE" ] && rm -f "$FIX_FILE"
[ -f "$SAVE_FILE" ] && rm -f "$SAVE_FILE"
[ -f "$KEEPPATH_FILE" ] && rm -f "$KEEPPATH_FILE"
[ -d "$KEEP_WGP_DIR" ] && rm -rf "$KEEP_WGP_DIR"

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
