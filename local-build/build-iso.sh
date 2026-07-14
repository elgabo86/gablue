#!/usr/bin/bash
# Script de build local de l'ISO live Gablue
# Usage: ./build-iso.sh [variante] [options]
#
# Variantes : main, main-dx, nvidia, nvidia-open, nvidia-open-dx (défaut: main)
#
# Options :
#   --run            Lancer la VM QEMU après le build
#   --pull           Forcer le pull de l'image de base avant le build
#   --skip-flatpaks  Ignorer l'installation des flatpaks (build de test rapide)
#
# Exemples :
#   ./build-iso.sh main                    # Build complet
#   ./build-iso.sh main-dx --run           # Build + test dans QEMU
#   ./build-iso.sh nvidia --pull           # Pull forcé + build
#   ./build-iso.sh main --skip-flatpaks    # Build rapide sans flatpaks

set -eoux pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SCRIPT_DIR/output"
REPO_OWNER="elgabo86"
TITANOBOA_REPO="https://github.com/ublue-os/titanoboa.git"
TITANOBOA_IMAGE="localhost/titanoboa:latest"

# =============================================================================
# ARGUMENTS
# =============================================================================

VARIANT="${1:-main}"
shift 2>/dev/null || true

RUN_VM=false
FORCE_PULL=false
SKIP_FLATPAKS=false

for arg in "$@"; do
    case "$arg" in
        --run) RUN_VM=true ;;
        --pull) FORCE_PULL=true ;;
        --skip-flatpaks) SKIP_FLATPAKS=true ;;
        *) echo "Option inconnue: $arg"; exit 1 ;;
    esac
done

IMAGE_NAME="gablue-${VARIANT}"
BASE_IMAGE="ghcr.io/${REPO_OWNER}/${IMAGE_NAME}:latest"
PAYLOAD_IMAGE="localhost/gablue-iso-payload:latest"
ISO_NAME="${IMAGE_NAME}-live-test.iso"

echo "=============================================="
echo " Build ISO Gablue - Variante : $VARIANT"
echo " Image base : $BASE_IMAGE"
echo "=============================================="
echo ""

# Rafraîchir le timestamp sudo une seule fois pour toute la session
sudo -v

# =============================================================================
# PULL IMAGE DE BASE
# =============================================================================

if [ "$FORCE_PULL" = true ]; then
    echo ">>> Pull de l'image de base..."
    sudo podman pull "$BASE_IMAGE"
    echo ""
fi

# =============================================================================
# BUILD DU PAYLOAD
# =============================================================================

echo ">>> Build du payload ISO..."
cd "$PROJECT_DIR"
sudo podman build \
    --cap-add sys_admin \
    --security-opt label=disable \
    --network=host \
    --build-arg BASE_IMAGE="$BASE_IMAGE" \
    --build-arg INSTALL_IMAGE_PAYLOAD="$BASE_IMAGE" \
    --build-arg SKIP_FLATPAKS="${SKIP_FLATPAKS:-}" \
    -t "$PAYLOAD_IMAGE" installer/

# =============================================================================
# BUILD TITANOBOA (si pas déjà fait)
# =============================================================================

if ! sudo podman images --format '{{.Repository}}:{{.Tag}}' | grep -q "^${TITANOBOA_IMAGE}$" 2>/dev/null; then
    echo ">>> Build de Titanoboa..."
    TITANOBOA_DIR="$(mktemp -d)"
    git clone --depth 1 "$TITANOBOA_REPO" "$TITANOBOA_DIR"
    sudo podman build -t "$TITANOBOA_IMAGE" "$TITANOBOA_DIR"
    rm -rf "$TITANOBOA_DIR"
    echo ""
fi

# =============================================================================
# GÉNÉRATION DE L'ISO
# =============================================================================

echo ""
echo ">>> Génération de l'ISO via Titanoboa..."
mkdir -p "$OUTPUT_DIR"

# Re-rafraîchir sudo (le build du payload a pu prendre > 5 min)
sudo -v

sudo podman run --rm \
    --security-opt label=disable \
    -v "$OUTPUT_DIR:/output:Z" \
    -v /var/lib/containers/storage:/usr/lib/containers/storage:ro,Z \
    -v "$PROJECT_DIR/installer/titanoboa_build_iso.sh:/app/bin/build_iso.sh:ro,Z" \
    --mount type=image,source="$PAYLOAD_IMAGE",dst=/rootfs \
    "$TITANOBOA_IMAGE"

# Titanoboa nomme l'ISO selon le label dans iso.yaml → GABLUE_LIVE.iso
if [ -f "$OUTPUT_DIR/GABLUE_LIVE.iso" ]; then
    mv "$OUTPUT_DIR/GABLUE_LIVE.iso" "$OUTPUT_DIR/$ISO_NAME"
    sudo chown "$(id -u):$(id -g)" "$OUTPUT_DIR/$ISO_NAME"
    echo ""
    echo "=============================================="
    echo " ISO créée : $OUTPUT_DIR/$ISO_NAME"
    ls -lh "$OUTPUT_DIR/$ISO_NAME"
    echo "=============================================="
else
    echo "ERREUR : ISO non trouvée dans $OUTPUT_DIR/"
    ls -la "$OUTPUT_DIR/" 2>/dev/null || true
    exit 1
fi

# =============================================================================
# TEST QEMU (optionnel)
# =============================================================================

if [ "$RUN_VM" = true ]; then
    echo ""
    echo ">>> Lancement de QEMU..."

    if ! command -v qemu-system-x86_64 &>/dev/null; then
        echo "ERREUR : qemu-system-x86_64 non trouvé."
        echo "Sur Kinoite : utilise un container ou passe en variante DX."
        exit 1
    fi

    qemu-system-x86_64 \
        -enable-kvm \
        -M q35 \
        -cpu host \
        -smp 4 \
        -m 8G \
        -net nic,model=virtio \
        -net user,hostfwd=tcp::2222-:22 \
        -display gtk,show-cursor=on \
        -boot d \
        -cdrom "$OUTPUT_DIR/$ISO_NAME" &
else
    echo ""
    echo "Pour tester dans QEMU : $0 $VARIANT --run"
fi

# Révoquer le timestamp sudo
sudo -k
