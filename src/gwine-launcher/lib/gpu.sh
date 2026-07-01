#!/bin/bash

################################################################################
# gpu.sh - Détection GPU et configuration Vulkan
################################################################################

# Détecte le fabricant du GPU (nvidia, amd, intel)
# Retourne le vendor en minuscules ou vide si non détecté
detect_gpu_vendor() {
    local gpu_vendor=""
    
    # Méthode 1: Détection via lspci
    if command -v lspci &>/dev/null; then
        local gpu_info
        gpu_info=$(lspci | grep -E "VGA|3D|Display" | head -1)
        
        if echo "$gpu_info" | grep -qi "nvidia"; then
            gpu_vendor="nvidia"
        elif echo "$gpu_info" | grep -qi "amd\|ati\b\|radeon"; then
            gpu_vendor="amd"
        elif echo "$gpu_info" | grep -qi "intel\|iris"; then
            gpu_vendor="intel"
        fi
    fi
    
    # Méthode 2: Détection via /sys/class/drm si non trouvé
    if [ -z "$gpu_vendor" ] && [ -d "/sys/class/drm" ]; then
        for card in /sys/class/drm/card*/device/vendor; do
            if [ -f "$card" ]; then
                local vendor
                vendor=$(cat "$card" 2>/dev/null)
                case "$vendor" in
                    0x10de) gpu_vendor="nvidia" ;;
                    0x1002) gpu_vendor="amd" ;;
                    0x8086) gpu_vendor="intel" ;;
                esac
                [ -n "$gpu_vendor" ] && break
            fi
        done
    fi
    
    # Méthode 3: Détection via pilotes Vulkan installés
    if [ -z "$gpu_vendor" ]; then
        if [ -f "/usr/share/vulkan/icd.d/nvidia_icd.x86_64.json" ] || [ -f "/etc/vulkan/icd.d/nvidia_icd.x86_64.json" ]; then
            gpu_vendor="nvidia"
        elif [ -f "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json" ] || [ -f "/etc/vulkan/icd.d/radeon_icd.x86_64.json" ]; then
            gpu_vendor="amd"
        elif [ -f "/usr/share/vulkan/icd.d/intel_icd.x86_64.json" ] || [ -f "/etc/vulkan/icd.d/intel_icd.x86_64.json" ]; then
            gpu_vendor="intel"
        fi
    fi
    
    # Méthode 4: nvidia-smi pour NVIDIA
    if [ -z "$gpu_vendor" ] && command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null; then
        gpu_vendor="nvidia"
    fi
    
    echo "$gpu_vendor"
}

# Vérifie si le système utilise un GPU spécifique
# Usage: is_gpu_vendor <vendor>
# Vendors supportés: nvidia, amd, intel
is_gpu_vendor() {
    local target_vendor="${1,,}"  # Convertir en minuscules
    local detected_vendor
    detected_vendor=$(detect_gpu_vendor)
    
    [ "$detected_vendor" = "$target_vendor" ]
}

# Wrapper pour compatibilité descendante
is_nvidia_gpu() {
    is_gpu_vendor "nvidia"
}

setup_gpu_vulkan() {
    local VK_ICD_64=""
    local VK_ICD_32=""
    
    # Détecter le GPU via la fonction centralisée
    local GPU_VENDOR
    GPU_VENDOR=$(detect_gpu_vendor)
    
    # Sélectionner le driver selon le GPU détecté
    case "$GPU_VENDOR" in
        amd)
            if [ -f "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json" ]; then
                VK_ICD_64="/usr/share/vulkan/icd.d/radeon_icd.x86_64.json"
                [ -f "/usr/share/vulkan/icd.d/radeon_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/radeon_icd.i686.json"
            elif [ -f "/etc/vulkan/icd.d/radeon_icd.x86_64.json" ]; then
                VK_ICD_64="/etc/vulkan/icd.d/radeon_icd.x86_64.json"
                [ -f "/etc/vulkan/icd.d/radeon_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/radeon_icd.i686.json"
            fi
            ;;
        intel)
            # Essayer d'abord le driver ANV standard
            if [ -f "/usr/share/vulkan/icd.d/intel_icd.x86_64.json" ]; then
                VK_ICD_64="/usr/share/vulkan/icd.d/intel_icd.x86_64.json"
                [ -f "/usr/share/vulkan/icd.d/intel_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/intel_icd.i686.json"
            elif [ -f "/etc/vulkan/icd.d/intel_icd.x86_64.json" ]; then
                VK_ICD_64="/etc/vulkan/icd.d/intel_icd.x86_64.json"
                [ -f "/etc/vulkan/icd.d/intel_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/intel_icd.i686.json"
            # Sinon essayer le driver Haswell/Broadwell (intel_hasvk)
            elif [ -f "/usr/share/vulkan/icd.d/intel_hasvk_icd.x86_64.json" ]; then
                VK_ICD_64="/usr/share/vulkan/icd.d/intel_hasvk_icd.x86_64.json"
                [ -f "/usr/share/vulkan/icd.d/intel_hasvk_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/intel_hasvk_icd.i686.json"
            elif [ -f "/etc/vulkan/icd.d/intel_hasvk_icd.x86_64.json" ]; then
                VK_ICD_64="/etc/vulkan/icd.d/intel_hasvk_icd.x86_64.json"
                [ -f "/etc/vulkan/icd.d/intel_hasvk_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/intel_hasvk_icd.i686.json"
            fi
            ;;
        nvidia)
            if [ -f "/usr/share/vulkan/icd.d/nvidia_icd.x86_64.json" ]; then
                VK_ICD_64="/usr/share/vulkan/icd.d/nvidia_icd.x86_64.json"
                [ -f "/usr/share/vulkan/icd.d/nvidia_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/nvidia_icd.i686.json"
            elif [ -f "/etc/vulkan/icd.d/nvidia_icd.x86_64.json" ]; then
                VK_ICD_64="/etc/vulkan/icd.d/nvidia_icd.x86_64.json"
                [ -f "/etc/vulkan/icd.d/nvidia_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/nvidia_icd.i686.json"
            fi
            ;;
    esac
    
    # Fallback: si pas détecté, utiliser l'ancienne méthode
    if [ -z "$VK_ICD_64" ]; then
        echo "GPU non détecté, utilisation du driver par défaut..."
        # Chercher NVIDIA (priorité si présent)
        if [ -f "/usr/share/vulkan/icd.d/nvidia_icd.x86_64.json" ]; then
            VK_ICD_64="/usr/share/vulkan/icd.d/nvidia_icd.x86_64.json"
            [ -f "/usr/share/vulkan/icd.d/nvidia_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/nvidia_icd.i686.json"
        elif [ -f "/etc/vulkan/icd.d/nvidia_icd.x86_64.json" ]; then
            VK_ICD_64="/etc/vulkan/icd.d/nvidia_icd.x86_64.json"
            [ -f "/etc/vulkan/icd.d/nvidia_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/nvidia_icd.i686.json"
        # Chercher AMD RADV (radeon)
        elif [ -f "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json" ]; then
            VK_ICD_64="/usr/share/vulkan/icd.d/radeon_icd.x86_64.json"
            [ -f "/usr/share/vulkan/icd.d/radeon_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/radeon_icd.i686.json"
        elif [ -f "/etc/vulkan/icd.d/radeon_icd.x86_64.json" ]; then
            VK_ICD_64="/etc/vulkan/icd.d/radeon_icd.x86_64.json"
            [ -f "/etc/vulkan/icd.d/radeon_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/radeon_icd.i686.json"
        # Chercher Intel
        elif [ -f "/usr/share/vulkan/icd.d/intel_icd.x86_64.json" ]; then
            VK_ICD_64="/usr/share/vulkan/icd.d/intel_icd.x86_64.json"
            [ -f "/usr/share/vulkan/icd.d/intel_icd.i686.json" ] && VK_ICD_32="/usr/share/vulkan/icd.d/intel_icd.i686.json"
        elif [ -f "/etc/vulkan/icd.d/intel_icd.x86_64.json" ]; then
            VK_ICD_64="/etc/vulkan/icd.d/intel_icd.x86_64.json"
            [ -f "/etc/vulkan/icd.d/intel_icd.i686.json" ] && VK_ICD_32="/etc/vulkan/icd.d/intel_icd.i686.json"
        fi
    fi
    
    if [ -n "$VK_ICD_64" ]; then
        if [ -n "$VK_ICD_32" ]; then
            export VK_ICD_FILENAMES="$VK_ICD_64:$VK_ICD_32"
            echo "Driver Vulkan détecté: $VK_ICD_64 + $VK_ICD_32"
        else
            export VK_ICD_FILENAMES="$VK_ICD_64"
            echo "Driver Vulkan détecté: $VK_ICD_64"
        fi
    fi
}
