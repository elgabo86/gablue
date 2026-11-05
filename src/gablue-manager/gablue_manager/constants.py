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

AVAHI_SERVICE_XML = """<?xml version="1.0" standalone="no"?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">%h</name>
  <service>
    <type>_ssh._tcp</type>
    <port>22</port>
  </service>
</service-group>
"""

REFRESH_MS = 30_000     # vérification automatique des postes

CHECK_DEADLINE = 8.0    # secondes par poste (ssh ConnectTimeout = 4)

PULSE_MS = 60           # cadence de l'animation des pastilles

WOL_PORTS = (9, 7)      # ports UDP du paquet magique (9 = standard)

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
    "nokey": COLOR_KEY,
    "refused": COLOR_OFFLINE,
    "timeout": COLOR_OFFLINE,
    "noroute": COLOR_OFFLINE,
    "dns": COLOR_OFFLINE,
    "error": COLOR_OFFLINE,
}

# États pour lesquels l'action utile est de (re)copier la clé
KEY_STATES = ("auth", "hostkey", "nokey")

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

SSH_KEYSCAN = _which("ssh-keyscan")

AVAHI_BROWSE = _which("avahi-browse")

IP = _which("ip")

# -- machine courante : mises à jour et options avancées ----------------------
OS_RELEASE = Path("/etc/os-release")

CMDLINE = Path("/proc/cmdline")

KARG_MITIGATIONS_OFF = "mitigations=off"

KARG_CLEARCPUID = "clearcpuid=514"      # ex-`ujust cpuid-fix-on`

KARG_BT_ERTM = "bluetooth.disable_ertm=1"

# Helpers privilégiés (NOPASSWD dans /etc/sudoers.d/nopasswd) : ils n'acceptent
# que des options connues, jamais une ligne de commande noyau arbitraire.
KARG_HELPER = "/usr/libexec/gablue-kargs"

CPUID_EMU_HELPER = "/usr/libexec/gablue-cpuid-emu"

TAILSCALE_HELPER = "/usr/libexec/gablue-tailscale"

CPUID_EMU_CONF = Path("/etc/modules-load.d/gablue-cpuid-emu.conf")
