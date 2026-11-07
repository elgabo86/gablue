"""Constantes, chemins et binaires du poste.
"""

import os
import re
import shutil
from pathlib import Path

CONFIG_DIR = Path(
    os.environ.get("XDG_CONFIG_HOME") or (Path.home() / ".config")
) / "gablue"

HOSTS_FILE = CONFIG_DIR / "ssh-hosts.json"

SSH_DIR = Path.home() / ".ssh"

PRIVATE_KEY = SSH_DIR / "id_ed25519"

PUBLIC_KEY = SSH_DIR / "id_ed25519.pub"

KNOWN_HOSTS = SSH_DIR / "known_hosts"

AVAHI_SERVICE_FILE = Path("/etc/avahi/services/ssh.service")

# Helper d'annonce mDNS (NOPASSWD) : il écrit le fichier lui-même, aucun chemin
# libre en argument — `install` sur un temporaire ne serait pas blanchissable.
AVAHI_HELPER = "/usr/libexec/gablue-avahi"

REFRESH_MS = 30_000     # vérification automatique des postes

CHECK_DEADLINE = 8.0    # secondes par poste (ssh ConnectTimeout = 4)

PULSE_MS = 60           # cadence de l'animation des pastilles

# États où la sonde automatique ne sert à rien et coûte cher : une connexion
# fermée avant authentification compte comme une offense pour
# `PerSourcePenalties` (OpenSSH 9.8+), et sonder toutes les 30 s un poste qui
# refuse la clé finit par faire bannir cette machine par son sshd.
NO_AUTO_PROBE_STATES = ("auth", "hostkey", "penalty")

# Une clé refusée par le poste ne se répare pas en attendant : l'action utile
# est de (re)copier la clé.
KEY_STATES = ("auth", "hostkey", "nokey")

# Couleurs des états (lisibles en thème clair comme en thème sombre)
COLOR_ONLINE = "#37c978"

COLOR_AUTH = "#e8a33d"

COLOR_KEY = "#a97cf0"

COLOR_OFFLINE = "#8b909a"

COLOR_CHECKING = "#4a9eff"

STATE_TEXT = {
    "unknown": "Non vérifié",
    "checking": "Vérification…",
    "online": "En ligne · clé SSH acceptée",
    "auth": "Joignable · clé SSH refusée",
    "hostkey": "Clé d'hôte inconnue",
    "penalty": "Poste bloquant temporairement cette machine",
    "nokey": "Aucune clé locale",
    "refused": "Hors ligne · connexion refusée",
    "timeout": "Hors ligne · délai dépassé",
    "noroute": "Hors ligne · réseau injoignable",
    "dns": "Hôte introuvable",
    "error": "Erreur de connexion",
}

STATE_COLOR = {
    "unknown": COLOR_OFFLINE,
    "checking": COLOR_CHECKING,
    "online": COLOR_ONLINE,
    "auth": COLOR_AUTH,
    "hostkey": COLOR_KEY,
    "penalty": COLOR_AUTH,
    "nokey": COLOR_KEY,
    "refused": COLOR_OFFLINE,
    "timeout": COLOR_OFFLINE,
    "noroute": COLOR_OFFLINE,
    "dns": COLOR_OFFLINE,
    "error": COLOR_OFFLINE,
}

PASSWORD_RE = re.compile(
    r"(?i)(?:password|mot de passe|passphrase)[^\n]*:[ \t]*$", re.MULTILINE
)

def _which(name: str) -> str:
    """Chemin absolu d'un binaire, ou son nom si introuvable."""
    found = shutil.which(name)
    if found:
        return found
    for candidate in (f"/usr/bin/{name}", f"/usr/local/bin/{name}"):
        if os.access(candidate, os.X_OK):
            return candidate
    return name

SSH = _which("ssh")

SSH_COPY_ID = _which("ssh-copy-id")

SSH_KEYGEN = _which("ssh-keygen")

AVAHI_BROWSE = _which("avahi-browse")

IP = _which("ip")

# -- machine courante : mises à jour et options avancées ----------------------
OS_RELEASE = Path("/etc/os-release")

CMDLINE = Path("/proc/cmdline")

KARG_MITIGATIONS_OFF = "mitigations=off"

KARG_CLEARCPUID = "clearcpuid=514"      # ex-`ujust cpuid-fix-on`

KARG_BT_ERTM = "bluetooth.disable_ertm=1"

# HDMI 2.1 sur GPU AMD (ex-`ujust configure-amd-hdmi21`) : FreeSync élargi,
# au prix de scintillements possibles sur certains écrans.
KARG_HDMI21 = "amdgpu.dcfeaturemask=0x402"

# Helpers privilégiés (NOPASSWD dans /etc/sudoers.d/nopasswd) : ils n'acceptent
# que des options connues, jamais une ligne de commande noyau arbitraire.
KARG_HELPER = "/usr/libexec/gablue-kargs"

CPUID_EMU_HELPER = "/usr/libexec/gablue-cpuid-emu"

TAILSCALE_HELPER = "/usr/libexec/gablue-tailscale"

CPUID_EMU_CONF = Path("/etc/modules-load.d/gablue-cpuid-emu.conf")

# Blacklist de kvm_amd posée avec l'émulation : sa date dit si le module doit
# être (dé)chargé au prochain démarrage.
CPUID_EMU_BLACKLIST = Path("/etc/modprobe.d/gablue-cpuid-emu.conf")

# Services, couche Vulkan et pilote Wi-Fi : ex-recettes ujust, mêmes helpers.
SERVICE_HELPER = "/usr/libexec/gablue-service"

LOW_LATENCY_HELPER = "/usr/libexec/gablue-low-latency"

BROADCOM_HELPER = "/usr/libexec/gablue-broadcom-wl"

REBASE_HELPER = "/usr/libexec/gablue-rebase"

ATUIN_HELPER = "/usr/bin/gablue-atuin"

GAMEPAD_HELPER = "/usr/bin/gablue-gamepad"

CLEAN_HELPER = "/usr/libexec/gablue-clean"

BTRFS_HELPER = "/usr/libexec/gablue-btrfs-compress"

NVME_THERMAL_HELPER = "/usr/libexec/gablue-nvme-thermal"

GRUB_HELPER = "/usr/libexec/gablue-grub"

BOOTSELECT_HELPER = "/usr/libexec/gablue-bootnext"

SECUREBOOT_HELPER = "/usr/libexec/gablue-secureboot"

RYZENADJ_BINARY = "/usr/bin/ryzenadj"

RYZENADJ_HELPER = "/usr/libexec/gablue-ryzenadj"

RYZENADJ_SERVICE = "gablue-ryzenadj.service"

LOW_LATENCY_FILE = Path("/etc/profile.d/vulkan-low-latency.sh")

BROADCOM_WL_FILE = Path("/etc/modprobe.d/wl-enable.conf")

# Homebrew de l'image (préfixe uBlue). Le PATH d'une session GUI ne le contient
# pas forcément (`/etc/profile.d/gablue-brew.sh` n'est lu que par les shells),
# d'où le chemin absolu pour la mise à jour manuelle.
BREW_BINARY = Path("/home/linuxbrew/.linuxbrew/bin/brew")

# Mises à jour automatiques : timers système fournis par l'image (image OSTree),
# par flatpak et par brew. Le timer utilisateur vit dans la session, il n'est
# donc pas concerné par la règle sudoers ; périmètre plus large que
# `ujust toggle-updates` upstream (qui laisse brew de côté), comme
# `toggle-updates-all`.
AUTO_UPDATE_HELPER = "/usr/libexec/gablue-auto-update"

# Mise à jour de l'image : helper root qui lance `bootc upgrade` et publie sa
# progression JSONL (`--progress-fd`) dans un FIFO à chemin fixe. Un descripteur
# passé par l'app ne survivrait pas à sudo (il ferme les fd >= 3), d'où le FIFO
# que le manager lit sans privilège.
UPDATE_HELPER = "/usr/libexec/gablue-update"

UPDATE_PROGRESS_FIFO = "/run/gablue/update-progress.fifo"

AUTO_UPDATE_USER_TIMER = "flatpak-user-update.timer"

# Variantes publiées par Gablue (le rebase bascule d'une image à l'autre).
REBASE_VARIANTS = (
    ("main", "main — standard"),
    ("main-dx", "main-dx — développement"),
    ("nvidia", "nvidia — NVIDIA propriétaire"),
    ("nvidia-open", "nvidia-open — NVIDIA open source"),
    ("nvidia-open-dx", "nvidia-open-dx — NVIDIA open + DX"),
)
