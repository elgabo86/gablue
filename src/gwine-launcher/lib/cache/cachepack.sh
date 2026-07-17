#!/bin/bash

################################################################################
# cachepack.sh - Création de packs cache pour installation offline
################################################################################

create_cachepack() {
    local OUTPUT_DIR="$(pwd)/gwine-cache-installer"
    local CACHE_SOURCE="$HOME/.cache/gwine"
    local COMPONENTS_SOURCE="$CACHE_SOURCE/components"
    
    echo "Création du pack cache pour installation offline..."
    echo ""
    
    if [ ! -d "$CACHE_SOURCE" ]; then
        error_exit "Cache gwine non trouvé dans $CACHE_SOURCE\nLancez d'abord 'gwine --download-components' pour télécharger les composants."
    fi
    
    if [ -d "$OUTPUT_DIR" ]; then
        echo "Le dossier $OUTPUT_DIR existe déjà, suppression..."
        rm -rf "$OUTPUT_DIR"
    fi
    
    mkdir -p "$OUTPUT_DIR"
    echo "Dossier de sortie: $OUTPUT_DIR"
    echo ""
    
    echo "Vérification des composants..."
    local missing_components=false
    
    local GWINE_ARCHIVE_DIR="$COMPONENTS_SOURCE/gwine"
    if [ -d "$GWINE_ARCHIVE_DIR" ] && [ -n "$(ls -A "$GWINE_ARCHIVE_DIR"/*.tar.xz 2>/dev/null)" ]; then
        echo "  ✓ gwine présent dans le cache"
    else
        echo "  ⚠️  gwine n'est pas dans le cache"
        missing_components=true
    fi
    
    if [ ! -d "$COMPONENTS_SOURCE" ]; then
        echo "  ✗ Composants non trouvés"
        missing_components=true
    else
        echo "  ✓ Composants présents"
    fi
    
    local has_dxvk=false
    if [ -d "$DXVK_CACHE_DIR" ]; then
        find "$DXVK_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -qv "gplasync\|nvapi" && has_dxvk=true
    fi
    if [ "$has_dxvk" = false ]; then
        echo "  ⚠️  DXVK manquant"
        missing_components=true
    else
        echo "  ✓ DXVK standard présent"
    fi
    
    local has_dxvk_async=false
    if [ -d "$DXVK_ASYNC_CACHE_DIR" ]; then
        find "$DXVK_ASYNC_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk*" 2>/dev/null | grep -q . && has_dxvk_async=true
    fi
    if [ "$has_dxvk_async" = false ]; then
        echo "  ⚠️  DXVK-GPLAsync manquant"
        missing_components=true
    else
        echo "  ✓ DXVK-GPLAsync présent"
    fi
    
    local has_vkd3d=false
    if [ -d "$VKD3D_CACHE_DIR" ]; then
        find "$VKD3D_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null | grep -q . && has_vkd3d=true
    fi
    if [ "$has_vkd3d" = false ]; then
        echo "  ⚠️  VKD3D-Proton manquant"
        missing_components=true
    else
        echo "  ✓ VKD3D-Proton présent"
    fi
    
    local has_nvapi=false
    if [ -d "$DXVK_NVAPI_CACHE_DIR" ]; then
        find "$DXVK_NVAPI_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk-nvapi-*" 2>/dev/null | grep -q . && has_nvapi=true
    fi
    if [ "$has_nvapi" = false ]; then
        echo "  ⚠️  DXVK-NVAPI manquant"
        missing_components=true
    else
        echo "  ✓ DXVK-NVAPI présent"
    fi
    
    local wine_cache="$COMPONENTS_SOURCE/wine-cache"
    if [ ! -f "$wine_cache/wine-mono-11.2.0-x86.msi" ] || \
       [ ! -f "$wine_cache/wine-gecko-2.47.4-x86_64.msi" ] || \
       [ ! -f "$wine_cache/wine-gecko-2.47.4-x86.msi" ]; then
        echo "  ⚠️  Wine Mono/Gecko manquants"
        missing_components=true
    else
        echo "  ✓ Wine Mono/Gecko présents"
    fi
    
    if [ ! -d "$CACHE_SOURCE/wincomponents" ]; then
        echo "  ⚠️  Composants Windows (wincomponents) manquants"
        missing_components=true
    else
        if command -v check_wincomponents_cache &>/dev/null; then
            if ! check_wincomponents_cache 2>/dev/null; then
                echo "  ⚠️  Certains composants Windows sont manquants"
                missing_components=true
            else
                echo "  ✓ Composants Windows complets"
            fi
        else
            echo "  ✓ Composants Windows présents"
        fi
    fi
    
    if [ "$missing_components" = true ]; then
        echo ""
        echo "=========================================="
        echo "Certains composants sont manquants."
        echo ""
        echo "Pour télécharger tous les composants:"
        echo "  gwine --download-components"
        echo ""
        echo "Le pack cache ne peut pas être créé."
        echo "=========================================="
        rm -rf "$OUTPUT_DIR"
        exit 1
    fi
    
    echo ""
    echo "Copie du cache..."
    local TEMP_CACHE="$OUTPUT_DIR/.temp_cache"
    mkdir -p "$TEMP_CACHE"
    
    echo "  - Copie des composants..."
    cp -r "$COMPONENTS_SOURCE" "$TEMP_CACHE/"
    
    local WINCOMPONENTS_SOURCE="$CACHE_SOURCE/wincomponents"
    if [ -d "$WINCOMPONENTS_SOURCE" ]; then
        echo "  - Copie des composants Windows (wincomponents)..."
        cp -r "$WINCOMPONENTS_SOURCE" "$TEMP_CACHE/"
    else
        echo "  ⚠️  ATTENTION: wincomponents non trouvé !"
        echo "      Le mode offline ne fonctionnera pas correctement."
        echo "      Lancez d'abord: gwine --download-components"
        rm -rf "$OUTPUT_DIR"
        exit 1
    fi
    
    echo ""
    echo "Création de l'archive..."
    local ARCHIVE_NAME="gwine-cache.tar.xz"
    local ARCHIVE_PATH="$OUTPUT_DIR/$ARCHIVE_NAME"
    
    cd "$OUTPUT_DIR"
    if tar -cJf "$ARCHIVE_NAME" -C "$TEMP_CACHE" .; then
        echo "  ✓ Archive créée: $ARCHIVE_NAME"
    else
        echo "  ✗ Échec de la création de l'archive"
        rm -rf "$OUTPUT_DIR"
        exit 1
    fi
    
    rm -rf "$TEMP_CACHE"
    
    echo ""
    echo "Création du script de déploiement..."
    
    cat > "$OUTPUT_DIR/install-cache.sh" << 'DEPLOY_SCRIPT'
#!/bin/bash

# Script de déploiement du cache Gwine
# Ce script déploie le cache sur le système et propose de lancer l'initialisation

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARCHIVE_FILE="$SCRIPT_DIR/gwine-cache.tar.xz"
CACHE_DIR="$HOME/.cache/gwine"
GWINE_DIR="$HOME/.local/share/gwine"

# Couleurs pour les messages
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "========================================"
echo "  Déploiement du cache Gwine"
echo "========================================"
echo ""

# Vérifier que l'archive existe
if [ ! -f "$ARCHIVE_FILE" ]; then
    echo -e "${RED}Erreur: Archive non trouvée: $ARCHIVE_FILE${NC}"
    exit 1
fi

# Vérifier que gwine est disponible (dans le PATH)
if ! command -v gwine &> /dev/null; then
    echo -e "${RED}Erreur: Gwine n'est pas installé sur ce système.${NC}"
    echo "Veuillez installer gwine avant de déployer le cache."
    exit 1
fi

# Demander confirmation
if command -v kdialog &> /dev/null; then
    if ! kdialog --title "Déploiement du cache" --yesno "Voulez-vous déployer le cache Gwine sur ce système ?"; then
        echo "Déploiement annulé."
        exit 0
    fi
else
    echo "Ce script va déployer le cache Gwine dans:"
    echo "  - $CACHE_DIR"
    echo "  - $GWINE_DIR"
    echo ""
    read -p "Voulez-vous déployer le cache ? [O/n]: " -r
    if [[ ! "$REPLY" =~ ^[OoYy]$ ]] && [ -n "$REPLY" ]; then
        echo "Déploiement annulé."
        exit 0
    fi
fi

echo ""
echo "Déploiement en cours..."

# Nettoyer l'ancien cache en préservant le shader cache
echo "  - Nettoyage de l'ancien cache..."
if [ -d "$CACHE_DIR" ]; then
    # Supprimer tout sauf shader-cache
    find "$CACHE_DIR" -mindepth 1 -maxdepth 1 ! -name "shader-cache" -exec rm -rf {} + 2>/dev/null || true
fi

# Supprimer l'ancien gwine
if [ -d "$GWINE_DIR" ]; then
    echo "  - Suppression de l'ancienne installation..."
    rm -rf "$GWINE_DIR"
fi

# Créer les nouveaux répertoires
mkdir -p "$CACHE_DIR"
mkdir -p "$GWINE_DIR"

# Extraire l'archive
echo "  - Extraction de l'archive..."
if tar -xJf "$ARCHIVE_FILE" -C "$CACHE_DIR"; then
    echo -e "${GREEN}  ✓ Cache déployé avec succès${NC}"
else
    echo -e "${RED}  ✗ Échec du déploiement${NC}"
    # Restaurer les backups en cas d'échec
    rm -rf "$CACHE_DIR" "$GWINE_DIR"
    exit 1
fi

# Extraire et installer gwine depuis components/gwine/
if [ -d "$CACHE_DIR/components/gwine" ]; then
    echo "  - Installation de gwine..."
    mkdir -p "$GWINE_DIR/runner"
    archive_file=$(ls "$CACHE_DIR/components/gwine"/*.tar.xz 2>/dev/null | head -1)
    if [ -n "$archive_file" ]; then
        temp_extract="/tmp/gwine-extract-$$"
        mkdir -p "$temp_extract"
        if tar -xJf "$archive_file" -C "$temp_extract"; then
            extracted_dir=$(find "$temp_extract" -maxdepth 1 -type d -name "*gwine*" | head -1)
            [ -z "$extracted_dir" ] && extracted_dir=$(find "$temp_extract" -maxdepth 1 -type d | head -1)
            if [ -n "$extracted_dir" ]; then
                cp -r "$extracted_dir"/* "$GWINE_DIR/runner/"
                echo -e "${GREEN}  ✓ gwine installé${NC}"
            fi
        fi
        rm -rf "$temp_extract"
    fi
fi

echo ""
echo -e "${GREEN}✓ Déploiement terminé avec succès !${NC}"
echo ""

# Demander si l'utilisateur veut lancer --init --offline
echo "Voulez-vous maintenant initialiser le préfixe Wine ?"
echo "Cela lancera: gwine --init --offline --kdialog"
echo ""

INIT_CONFIRM=false
if command -v kdialog &> /dev/null; then
    if kdialog --title "Initialisation" --yesno "Voulez-vous initialiser le préfixe Wine maintenant ?\n\nCela configurera Wine avec tous les composants nécessaires."; then
        INIT_CONFIRM=true
    fi
else
    read -p "Lancer l'initialisation ? [O/n]: " -r
    if [[ "$REPLY" =~ ^[OoYy]$ ]] || [ -z "$REPLY" ]; then
        INIT_CONFIRM=true
    fi
fi

if [ "$INIT_CONFIRM" = true ]; then
    echo ""
    echo "Lancement de l'initialisation..."
    echo "========================================"
    gwine --init --offline --kdialog
else
    echo ""
    echo "Pour initialiser plus tard, lancez:"
    echo "  gwine --init --offline"
fi

echo ""
echo "========================================"
DEPLOY_SCRIPT

    chmod +x "$OUTPUT_DIR/install-cache.sh"
    echo "  ✓ Script de déploiement créé: install-cache.sh"
    
    cat > "$OUTPUT_DIR/README.txt" << 'README'
Gwine Cache Installer
=====================

Ce dossier contient tous les composants nécessaires pour installer Gwine
en mode offline.

Contenu:
  - gwine-cache.tar.xz    : Archive contenant le cache et gwine
  - install-cache.sh      : Script de déploiement automatique
  - README.txt            : Ce fichier

Instructions:
=============

1. Copiez ce dossier sur la machine cible (sans connexion internet)

2. Exécutez le script de déploiement:
     ./install-cache.sh

   Le script va:
   - Déployer le cache dans ~/.cache/gwine/
   - Installer gwine dans ~/.local/share/gwine/runner/
   - Proposer d'initialiser le préfixe Wine (avec kdialog si disponible)

3. Alternative manuelle:
   - Extraire gwine-cache.tar.xz dans ~/.cache/gwine/
   - Extraire les archives de components/gwine/ vers ~/.local/share/gwine/runner/
   - Lancer: gwine --init --offline

Notes:
======
- Assurez-vous que les dépendances système sont installées
  (wine, squashfuse, bubblewrap, etc.)
- Le script détecte automatiquement si kdialog est disponible
- Le préfixe Wine sera créé dans ~/Windows/Prefix/
- Le runner gwine est inclus dans le pack
- Deux versions DXVK sont incluses : standard et GPLAsync
- Pour utiliser DXVK-GPLAsync: gwine --init --dxvk-async
  (DXVK_ASYNC=1 sera automatiquement défini)

Pour plus d'informations: https://github.com/elgabo86/gwine
README

    echo "  ✓ README créé"
    
    echo ""
    echo "========================================"
    echo "  Pack cache créé avec succès !"
    echo "========================================"
    echo ""
    echo "Emplacement: $OUTPUT_DIR"
    echo ""
    echo "Contenu:"
    ls -lh "$OUTPUT_DIR" | tail -n +2
    echo ""
    echo "Pour déployer sur une autre machine:"
    echo "  1. Copiez le dossier '$OUTPUT_DIR'"
    echo "  2. Exécutez: ./install-cache.sh"
    echo ""
    echo "Le cache est prêt pour le mode offline."
}
