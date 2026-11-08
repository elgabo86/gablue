"""État et commandes de la machine courante (sshd, kargs, gwine,
Tailscale).
"""

import configparser
import json
import os
import re
import shlex
import shutil
import subprocess
import time
from pathlib import Path

from .constants import (
    ATUIN_HELPER, AUTO_UPDATE_HELPER, AVAHI_HELPER,
    AVAHI_SERVICE_FILE, BOOTSELECT_HELPER, BREW_BINARY, BROADCOM_HELPER,
    BROADCOM_WL_FILE, BTRFS_HELPER, CLEAN_HELPER, CMDLINE,
    CPUID_EMU_BLACKLIST,
    CPUID_EMU_CONF, CPUID_EMU_HELPER, GAMEPAD_HELPER, GRUB_HELPER,
    HWINFO_HELPER,
    NVME_THERMAL_HELPER, RYZENADJ_BINARY, RYZENADJ_HELPER, RYZENADJ_SERVICE,
    SECUREBOOT_HELPER,
    KARG_BT_ERTM, KARG_CLEARCPUID,
    KARG_HDMI21, KARG_HELPER, KARG_MITIGATIONS_OFF, LOW_LATENCY_FILE,
    LOW_LATENCY_HELPER, OS_RELEASE, REBASE_HELPER, SERVICE_HELPER,
    TAILSCALE_HELPER, _which,
)
from .ssh import ssh_capture


def _run(argv: list[str], timeout: int,
         env: dict | None = None) -> tuple[int, str]:
    """Lance une sonde ; code 127 et sortie vide si elle n'existe pas."""
    try:
        result = subprocess.run(
            argv, capture_output=True, text=True, timeout=timeout, env=env,
        )
    except (OSError, subprocess.TimeoutExpired):
        return 127, ""
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def _sudo_run(argv: list[str]) -> tuple[bool, str]:
    """Exécute une commande privilégiée sans jamais demander de mot de passe."""
    try:
        result = subprocess.run(
            ["sudo", "-n", *argv], capture_output=True, text=True, timeout=25,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return False, str(exc)
    if result.returncode == 0:
        return True, ""
    lines = (result.stderr or result.stdout or "").strip().splitlines()
    return False, lines[-1] if lines else ""

def set_local_sshd_sudo(enabled: bool) -> tuple[bool, str]:
    """Tentative silencieuse d'activation/coupure du sshd local.

    `sudo -n` ne demande JAMAIS de mot de passe : sur une image Gablue
    (/etc/sudoers.d/nopasswd) c'est immédiat, ailleurs ça échoue en quelques
    millisecondes et l'appelant bascule sur `pkexec` (dialogue PolicyKit).
    """
    action = "enable" if enabled else "disable"
    return _sudo_run([_which("systemctl"), action, "sshd.service", "--now"])

def avahi_publish_state() -> tuple[bool, bool]:
    """(ce poste s'annonce en _ssh._tcp, avahi-daemon est actif)."""
    daemon = False
    try:
        result = subprocess.run(
            ["systemctl", "is-active", "avahi-daemon.service"],
            capture_output=True, text=True, timeout=5,
        )
        daemon = result.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        pass
    return AVAHI_SERVICE_FILE.exists(), daemon

def avahi_apply_args(enabled: bool) -> list[str]:
    """Commande privilégiée : publier ou retirer l'annonce mDNS.

    Le fichier de service est écrit par le helper, pas par l'appelant : un
    `install` avec un chemin temporaire en argument ne pourrait pas être
    blanchi aux arguments exacts dans sudoers (d'où une invite PolicyKit).
    """
    return [AVAHI_HELPER, "on" if enabled else "off"]

def local_sshd_state() -> tuple[bool, bool]:
    """(actif maintenant, activé au démarrage)."""
    def query(verb: str) -> bool:
        try:
            result = subprocess.run(
                ["systemctl", verb, "sshd.service"],
                capture_output=True, text=True, timeout=5,
            )
        except (OSError, subprocess.TimeoutExpired):
            return False
        return result.returncode == 0

    return query("is-active"), query("is-enabled")

def local_distro() -> str:
    """PRETTY_NAME de /etc/os-release, ou chaîne vide."""
    try:
        for line in OS_RELEASE.read_text(encoding="utf-8").splitlines():
            if line.startswith("PRETTY_NAME="):
                return line.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    return ""

def kernel_cmdline() -> set[str]:
    """Paramètres du noyau en cours (effet réel = après redémarrage)."""
    try:
        return set(CMDLINE.read_text(encoding="utf-8").split())
    except OSError:
        return set()

def privileged_prefix() -> list[str]:
    """`sudo -n` si l'image l'autorise (Gablue), sinon `pkexec` (PolicyKit).

    `sudo -n true` ne peut pas servir de sonde : aucune règle pour `true`, même
    sur une image Gablue, si bien que le test échouait toujours et que TOUTE
    action root passait par le dialogue PolicyKit malgré la règle sudoers. On
    sonde `gablue-cpuid-emu status`, autorisé en NOPASSWD dans l'image et sans
    effet ; hors image Gablue il échoue en quelques millisecondes.
    """
    try:
        probe = subprocess.run(
            ["sudo", "-n", CPUID_EMU_HELPER, "status"],
            capture_output=True, timeout=5,
        )
        if probe.returncode == 0:
            return ["sudo", "-n"]
    except (OSError, subprocess.TimeoutExpired):
        pass
    return ["pkexec"]

def kargs_pending() -> set[str]:
    """Paramètres du noyau du **prochain démarrage** (`rpm-ostree kargs`).

    `/proc/cmdline` ne décrit que le noyau en cours : après un changement il
    faudrait redémarrer pour voir le nouvel état, alors que `rpm-ostree kargs`
    affiche déjà la ligne de commande du déploiement à venir — en lecture, sans
    root. Repli sur `/proc/cmdline` hors système ostree.
    """
    try:
        result = subprocess.run(
            [_which("rpm-ostree"), "kargs"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode == 0 and result.stdout.strip():
            return set(result.stdout.split())
    except (OSError, subprocess.TimeoutExpired):
        pass
    return kernel_cmdline()

def cpuid_emu_active() -> bool:
    """L'émulation CPUID fault est-elle persistée sur ce poste ?"""
    return CPUID_EMU_CONF.exists()

def boot_time() -> float:
    """Horodatage du démarrage courant (`btime` de /proc/stat), 0 si illisible."""
    try:
        for line in Path("/proc/stat").read_text(encoding="utf-8").splitlines():
            if line.startswith("btime "):
                return float(line.split()[1])
    except (OSError, ValueError):
        pass
    return 0.0

def written_since(path: Path, since: float) -> bool:
    """Ce fichier a-t-il été écrit après ce moment ?

    Sert à distinguer une config livrée par l'image (date du build, bien avant
    le démarrage) d'une bascule faite depuis : seule la seconde attend un
    redémarrage, et le test se nettoie tout seul une fois le poste redémarré.
    """
    try:
        return path.is_file() and since > 0 and path.stat().st_mtime > since
    except OSError:
        return False

# -- matériel et système de ce poste ----------------------------------------

DMI_DIR = Path("/sys/class/dmi/id")

# Valeurs que les fabricants laissent quand le champ DMI n'est pas renseigné :
# les afficher donnerait « ASUS System Product Name » comme nom de machine.
GENERIC_DMI = {
    "", "system product name", "to be filled by o.e.m.", "default string",
    "not specified", "none", "unknown", "system version",
}

# Identifiants PCI des principaux fabricants de GPU, pour le repli sans lspci.
GPU_VENDORS = {
    "0x1002": "AMD", "0x10de": "NVIDIA", "0x8086": "Intel",
    "0x1af4": "Virtio", "0x13b5": "ARM",
}

# lspci est une sonde à part : son résultat ne change pas de la session, on le
# garde pour ne pas relancer un processus à chaque rafraîchissement.
_GPU_LABEL: str | None = None

def _read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace").strip()
    except OSError:
        return ""

def _gio_amount(value: float) -> str:
    """Nombre en Gio, sans décimale inutile (« 16 », « 31 », « 1,8 »)."""
    if abs(value - round(value)) < 0.05:
        return str(int(round(value)))
    return f"{value:.1f}".replace(".", ",")

def format_gib(value: float) -> str:
    """« 31 Gio » — mémoire totale, sans décimale inutile."""
    return f"{_gio_amount(value)} Gio"

def format_capacity(num_bytes: int) -> str:
    """Taille lisible en Gio/Tio (base 1024, unités françaises)."""
    gio = max(0, int(num_bytes)) / (1024 ** 3)
    if gio >= 1024:
        return f"{_gio_amount(gio / 1024)} Tio"
    return f"{_gio_amount(gio)} Gio"

def format_uptime(seconds: float) -> str:
    """Durée de démarrage, la plus courte lisible (« 3 h 12 », « 5 j 4 h »)."""
    total = max(0, int(seconds))
    days, rest = divmod(total, 86400)
    hours, rest = divmod(rest, 3600)
    minutes = rest // 60
    if days:
        return f"{days} j {hours} h"
    if hours:
        return f"{hours} h {minutes:02d}"
    return f"{minutes} min"

def parse_cpu_info(text: str) -> dict:
    """`/proc/cpuinfo` → modèle, cœurs physiques et threads.

    Les champs `cpu cores` et `siblings` ne sont pas présents partout (ARM) :
    on compte alors les blocs `processor`, qui donnent les threads.
    """
    model = ""
    cores = 0
    threads = 0
    siblings = 0
    for line in (text or "").splitlines():
        key, sep, value = line.partition(":")
        if not sep:
            continue
        key = key.strip()
        value = value.strip()
        if key == "model name" and not model:
            model = re.sub(r"\s+", " ", value)
        elif key == "cpu cores" and not cores and value.isdigit():
            cores = int(value)
        elif key == "siblings" and not siblings and value.isdigit():
            siblings = int(value)
        elif key == "processor":
            threads += 1
    return {
        "model": model,
        "cores": cores,
        "threads": max(threads, siblings),
    }

def cpu_label_parts(model: str, cores: int, threads: int) -> str:
    """Modèle du processeur et nombre de cœurs/threads, en une ligne."""
    if not model:
        return ""
    if cores and threads and threads != cores:
        return f"{model} · {cores} cœurs / {threads} threads"
    count = cores or threads
    return f"{model} · {count} cœurs" if count else model

def cpu_label(text: str | None = None) -> str:
    """Modèle du processeur et nombre de cœurs/threads, depuis /proc/cpuinfo."""
    if text is None:
        text = _read_text(Path("/proc/cpuinfo"))
    info = parse_cpu_info(text)
    return cpu_label_parts(info["model"], info["cores"], info["threads"])

def parse_lspci_display(text: str) -> str:
    """Sortie de `lspci -mm -nn -d ::0300` → nom commercial de la carte.

    `-mm` découpe chaque carte en champs entre guillemets : classe, fabricant,
    périphérique, sous-système. Le nom du périphérique porte déjà la marque
    (« Radeon », « GeForce »), on n'ajoute le fabricant que s'il manque
    (« Intel » pour un « UHD Graphics »).
    """
    for line in (text or "").splitlines():
        fields = re.findall(r'"([^"]*)"', line)
        if len(fields) < 3:
            continue
        vendor = re.sub(r"\s*\[[0-9A-Fa-f]{4}\]\s*$", "", fields[1]).strip()
        device = re.sub(r"\s*\[[0-9A-Fa-f]{4}\]\s*$", "", fields[2]).strip()
        if not device or device.casefold().startswith(("device", "unknown")):
            continue
        brand = _gpu_brand(vendor)
        named = any(
            word in device
            for word in ("Radeon", "GeForce", "Arc", "Intel", "AMD", "NVIDIA")
        )
        return device if named or not brand else f"{brand} {device}"
    return ""

def _gpu_brand(vendor: str) -> str:
    """Fabricant court : « Advanced Micro Devices [AMD/ATI] » → « AMD »."""
    match = re.search(r"\[([^\]]+)\]\s*$", vendor or "")
    if match:
        vendor = match.group(1).split("/")[0]
    lowered = (vendor or "").casefold()
    for needle, name in (
        ("advanced micro devices", "AMD"), ("nvidia", "NVIDIA"),
        ("intel", "Intel"),
    ):
        if needle in lowered:
            return name
    # « ATI » seul, jamais comme sous-chaîne : « corporation » la contient.
    if re.search(r"\bati\b", lowered):
        return "AMD"
    return (vendor or "").split(",")[0].strip()

def gpu_label() -> str:
    """Carte graphique de ce poste, ou chaîne vide si introuvable.

    `lspci` donne le nom commercial ; sans lui, `/sys/class/drm` ne fournit que
    le fabricant et le pilote, ce qui reste utile (« AMD (amdgpu) »).
    """
    global _GPU_LABEL
    if _GPU_LABEL is not None:
        return _GPU_LABEL
    label = ""
    if shutil.which("lspci"):
        code, text = _run(["lspci", "-mm", "-nn", "-d", "::0300"], 5)
        if code == 0:
            label = parse_lspci_display(text)
    _GPU_LABEL = label or _gpu_from_sysfs()
    return _GPU_LABEL

def parse_vulkan_name(name: str) -> str:
    """« AMD Radeon RX 9060 XT (RADV GFX1200) » → « AMD Radeon RX 9060 XT ».

    Mesa ajoute le pilote et le die entre parenthèses ; KInfoCenter affiche le
    nom commercial nu, on fait pareil. Seuls ces suffixes connus sont retirés
    (jamais le « (R) » d'Intel).
    """
    cleaned = re.sub(
        r"\s*\((?:RADV|LLVM|Mesa|AMD proprietary)[^)]*\)\s*$",
        "", (name or "").strip(), flags=re.IGNORECASE,
    )
    # Codes de die en majuscules (« (TGL GT2) », « (GFX1200) ») : ils
    # n'apprennent rien à l'utilisateur, le modèle commercial suffit.
    cleaned = re.sub(r"\s*\([A-Z][A-Z0-9 ]{1,}\)\s*$", "", cleaned)
    return cleaned.strip()

def vulkan_gpu_name() -> str:
    """Nom commercial du GPU par Vulkan, ou chaîne vide.

    C'est la source de KInfoCenter : `lspci` ne connaît que le nom du die
    (« Navi 44 [Radeon RX 9050 / 9060 XT] »), alors que le pilote Mesa lit la
    VBIOS et expose le modèle exact (« AMD Radeon RX 9060 XT »). L'instance
    Vulkan est minimale et détruite aussitôt ; l'appel se fait depuis un
    thread de fond (le chargement des pilotes prend quelques dizaines de ms).
    """
    try:
        import ctypes
    except ImportError:
        return ""

    class _ApplicationInfo(ctypes.Structure):
        _fields_ = [
            ("sType", ctypes.c_int32), ("pNext", ctypes.c_void_p),
            ("pApplicationName", ctypes.c_char_p),
            ("applicationVersion", ctypes.c_uint32),
            ("pEngineName", ctypes.c_char_p),
            ("engineVersion", ctypes.c_uint32),
            ("apiVersion", ctypes.c_uint32),
        ]

    # sType de VkInstanceCreateInfo = 1, celui de VkApplicationInfo = 0.
    class _InstanceCreateInfo(ctypes.Structure):
        _fields_ = [
            ("sType", ctypes.c_int32), ("pNext", ctypes.c_void_p),
            ("flags", ctypes.c_uint32),
            ("pApplicationInfo", ctypes.POINTER(_ApplicationInfo)),
            ("enabledLayerCount", ctypes.c_uint32),
            ("ppEnabledLayerNames", ctypes.c_void_p),
            ("enabledExtensionCount", ctypes.c_uint32),
            ("ppEnabledExtensionNames", ctypes.c_void_p),
        ]

    try:
        lib = ctypes.CDLL("libvulkan.so.1")
    except OSError:
        return ""
    try:
        # Signatures explicites : sans elles, ctypes tronque les pointeurs en
        # 64 bits et l'appel part en segfault.
        lib.vkCreateInstance.argtypes = [
            ctypes.POINTER(_InstanceCreateInfo), ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_void_p),
        ]
        lib.vkCreateInstance.restype = ctypes.c_int32
        lib.vkDestroyInstance.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        lib.vkEnumeratePhysicalDevices.argtypes = [
            ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32),
            ctypes.POINTER(ctypes.c_void_p),
        ]
        lib.vkEnumeratePhysicalDevices.restype = ctypes.c_int32
        lib.vkGetPhysicalDeviceProperties.argtypes = [
            ctypes.c_void_p, ctypes.c_void_p,
        ]

        app_info = _ApplicationInfo(
            0, None, b"gablue-manager", 1, b"gablue-manager", 1, 4194304,
        )
        create_info = _InstanceCreateInfo(
            1, None, 0, ctypes.pointer(app_info), 0, None, 0, None,
        )
        instance = ctypes.c_void_p()
        if lib.vkCreateInstance(
            ctypes.byref(create_info), None, ctypes.byref(instance)
        ) != 0:
            return ""
        try:
            count = ctypes.c_uint32(0)
            if lib.vkEnumeratePhysicalDevices(
                instance, ctypes.byref(count), None
            ) != 0 or count.value == 0:
                return ""
            devices = (ctypes.c_void_p * count.value)()
            if lib.vkEnumeratePhysicalDevices(
                instance, ctypes.byref(count), devices
            ) != 0:
                return ""
            for index in range(count.value):
                # VkPhysicalDeviceProperties : deviceName commence à l'octet
                # 20 (cinq uint32) et fait 256 octets.
                buffer = ctypes.create_string_buffer(1024)
                lib.vkGetPhysicalDeviceProperties(devices[index], buffer)
                raw = bytes(buffer)[20:20 + 256]
                name = parse_vulkan_name(
                    raw.split(b"\0", 1)[0].decode("utf-8", "replace")
                )
                if not name:
                    continue
                if any(
                    word in name.casefold()
                    for word in ("llvmpipe", "lavapipe", "swiftshader")
                ):
                    continue
                return name
            return ""
        finally:
            lib.vkDestroyInstance(instance, None)
    except Exception:
        return ""

def _gpu_from_sysfs() -> str:
    for device in sorted(Path("/sys/class/drm").glob("card*/device")):
        vendor = _read_text(device / "vendor").lower()
        if not vendor:
            continue
        driver = ""
        for line in _read_text(device / "uevent").splitlines():
            if line.startswith("DRIVER="):
                driver = line.split("=", 1)[1].strip()
        name = GPU_VENDORS.get(vendor, vendor)
        return f"{name} ({driver})" if driver else name
    return ""

def memory_total_gib(text: str) -> float | None:
    """`/proc/meminfo` → mémoire utilisable en Gio (None si illisible)."""
    match = re.search(r"^MemTotal:\s+(\d+)\s*kB", text or "", re.MULTILINE)
    if not match:
        return None
    return int(match.group(1)) / (1024 ** 2)

def memory_used_gib(text: str) -> float | None:
    """Mémoire réellement occupée : `MemTotal` − `MemAvailable`, en Gio.

    `MemAvailable` est l'estimation du noyau de ce qui reste utilisable sans
    swapper — cache de fichiers compris. C'est la seule mesure qui ne compte
    pas le cache comme « mémoire utilisée » : `MemFree` seul donnerait 25 Gio
    « occupés » sur un poste qui ne fait que lire des fichiers.
    """
    total = re.search(r"^MemTotal:\s+(\d+)\s*kB", text or "", re.MULTILINE)
    available = re.search(r"^MemAvailable:\s+(\d+)\s*kB", text or "",
                          re.MULTILINE)
    if not total or not available:
        return None
    used = int(total.group(1)) - int(available.group(1))
    return max(0.0, used / (1024 ** 2))

def format_memory(text: str) -> str:
    """« 12,4 Gio utilisés sur 31 Gio · 40 % » (vide si `/proc` illisible).

    Sans `MemAvailable` (vieux noyau, poste exotique), on retombe sur le seul
    total plutôt que d'inventer un pourcentage.
    """
    total = memory_total_gib(text)
    if not total:
        return ""
    used = memory_used_gib(text)
    if used is None:
        return format_gib(total)
    return (f"{format_gib(used)} utilisés sur {format_gib(total)}"
            f" · {round(used / total * 100)} %")

def parse_vram(text: str) -> tuple[float | None, bool]:
    """(taille en Gio, mémoire dédiée) depuis `/sys` (amdgpu) ou `/proc` (NVIDIA).

    Format attendu : des paires `total=<octets>` / `vendor=<fabricant>` par
    carte, ou les lignes « Video Memory: 8192 MiB » de `/proc/driver/nvidia`.
    Plusieurs cartes → la plus grosse.

    « Dédiée » veut dire mémoire **propre à la carte** : amdgpu ne publie
    `mem_info_vram_vendor` que dans ce cas. Sur un APU la VRAM est une tranche
    de RAM système réservée par le BIOS, et le fichier est absent — d'où le
    libellé « RAM allouée » plutôt que « VRAM », qui laisserait croire à de la
    mémoire en plus.
    """
    best: float | None = None
    dedicated = False
    for line in (text or "").splitlines():
        line = line.strip()
        if line.startswith("total="):
            digits = line[len("total="):].strip()
            if digits.isdigit():
                value = int(digits) / (1024 ** 3)
                best = value if best is None else max(best, value)
        elif line.startswith("vendor="):
            dedicated = dedicated or bool(line[len("vendor="):].strip())
        else:
            match = re.search(r"Video Memory:\s*(\d+)\s*MiB", line,
                              re.IGNORECASE)
            if match:
                value = int(match.group(1)) / 1024
                best = value if best is None else max(best, value)
                dedicated = True
    return best, dedicated

def format_vram(gib: float, dedicated: bool = True) -> str:
    """« 16 Gio VRAM », ou « 8 Gio (RAM allouée) » sur un APU.

    Une carte vendue 16 Go en annonce 15,9 une fois la mémoire réservée
    retirée par le pilote ; sous 4 Gio on garde la décimale, au-dessus
    l'arrondi à l'entier est ce que l'utilisateur connaît. Sur un APU, le GPU
    n'a pas de mémoire propre : la valeur est une tranche de RAM réservée par
    le BIOS, et « VRAM » ferait croire à de la mémoire supplémentaire.
    """
    value = f"{round(gib)}" if gib >= 4 else _gio_amount(gib)
    return f"{value} Gio VRAM" if dedicated else f"{value} Gio (RAM allouée)"

def gpu_vram() -> tuple[float | None, bool]:
    """VRAM de la machine courante, sans privilège (amdgpu puis NVIDIA)."""
    texts: list[str] = []
    for path in sorted(Path("/").glob(str(VRAM_SYSFS).lstrip("/"))):
        texts.append(f"total={_read_text(path).strip()}")
        vendor = _read_text(path.parent / VRAM_VENDOR.name).strip()
        texts.append(f"vendor={vendor}")
    for path in sorted(Path("/").glob(str(VRAM_NVIDIA).lstrip("/"))):
        texts.append(_read_text(path))
    return parse_vram("\n".join(texts))

def format_memory_modules(modules: list[dict]) -> str:
    """Barrettes → « 2 × 16 Gio · DDR5 · 6000 MT/s » (chaîne vide sans module)."""
    rows = [
        module for module in (modules or [])
        if isinstance(module, dict) and float(module.get("size_gib") or 0) > 0
    ]
    if not rows:
        return ""
    counts: dict[float, int] = {}
    for module in rows:
        size = round(float(module["size_gib"]), 1)
        counts[size] = counts.get(size, 0) + 1
    total = sum(size * count for size, count in counts.items())
    count = sum(counts.values())
    if len(counts) == 1:
        if count == 1:
            sizes = f"{_gio_amount(total)} Gio"
        else:
            sizes = f"{count} × {_gio_amount(total / count)} Gio"
    else:
        sizes = " + ".join(
            f"{n} × {_gio_amount(size)} Gio"
            for size, n in sorted(counts.items())
        )
    kinds = sorted({str(module.get("type") or "") for module in rows} - {""})
    brands = sorted({
        str(module.get("manufacturer") or "").strip() for module in rows
    } - {"", "Unknown", "Not Specified", "No Module Installed"})
    speeds = sorted({
        int(module.get("speed") or 0) for module in rows
        if int(module.get("speed") or 0) > 0
    })
    parts = [sizes]
    if brands:
        parts.append("/".join(brands))
    if kinds:
        parts.append("/".join(kinds))
    if speeds:
        parts.append("/".join(str(speed) for speed in speeds) + " MT/s")
    return " · ".join(parts)

def parse_memory_modules(text: str) -> list[dict]:
    """JSON du helper `gablue-hwinfo` → barrettes exploitables.

    Le helper sort `{"modules": [{size_gib, type, speed, configured_speed}]}` ;
    une sortie illisible (helper absent, dmidecode muet) donne une liste vide,
    jamais une exception — la mémoire totale, elle, vient de /proc/meminfo.
    """
    return memory_modules_from(parse_hwinfo(text))

def memory_modules_from(data: dict) -> list[dict]:
    """Partie « modules » du JSON du helper → barrettes (liste vide sinon)."""
    modules = data.get("modules") if isinstance(data, dict) else None
    rows: list[dict] = []
    for item in modules or []:
        if not isinstance(item, dict):
            continue
        try:
            size = float(item.get("size_gib") or 0)
        except (TypeError, ValueError):
            continue
        if size <= 0:
            continue
        speed = item.get("configured_speed") or item.get("speed") or 0
        try:
            speed = int(speed)
        except (TypeError, ValueError):
            speed = 0
        rows.append({
            "size_gib": round(size, 1),
            "type": str(item.get("type") or "").strip(),
            "speed": max(0, speed),
            "manufacturer": str(item.get("manufacturer") or "").strip(),
        })
    return rows

def hwinfo() -> dict:
    """JSON du helper NOPASSWD `gablue-hwinfo` ({} s'il est absent ou muet).

    Une seule invocation rend les barrettes **et** la température des disques
    SATA : chaque appel passe par sudo, on ne le paie pas deux fois.
    """
    if not Path(HWINFO_HELPER).is_file():
        return {}
    try:
        result = subprocess.run(
            ["sudo", "-n", HWINFO_HELPER],
            capture_output=True, text=True, timeout=60,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}
    if result.returncode != 0:
        return {}
    return parse_hwinfo(result.stdout)

def parse_hwinfo(text: str) -> dict:
    """Sortie du helper → dictionnaire (`{}` si elle est illisible)."""
    try:
        data = json.loads(text or "")
    except (TypeError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}

def memory_modules() -> list[dict]:
    """Barrettes mémoire lues par le helper NOPASSWD (liste vide sinon)."""
    return memory_modules_from(hwinfo())

def sata_temperatures(data: dict | None = None) -> list[tuple]:
    """Disques SATA du helper → [(« hdd1 », 36.0)] ; un disque en veille est écarté.

    `celsius` absent = disque endormi (ou sans sonde) : il ne s'affiche pas,
    et il n'a surtout pas été réveillé pour ça. Le nom vient du point de
    montage, celui que la page « Disques » affiche déjà (« hdd1 », « data2 ») ;
    un disque non monté garde son nom de périphérique (« sdc »), sans risque
    de collision avec un nom de montage. Le troisième champ est le verdict
    `smartctl -H` du disque (True/False), ou None s'il ne le publie pas.
    """
    payload = hwinfo() if data is None else data
    rows: list[tuple] = []
    for item in (payload or {}).get("disks") or []:
        if not isinstance(item, dict):
            continue
        celsius = item.get("celsius")
        if not isinstance(celsius, (int, float)):
            continue
        mount = str(item.get("mount") or "").rstrip("/")
        device = str(item.get("device") or "")
        name = os.path.basename(mount) or device
        if not name:
            continue
        smart = item.get("smart")
        rows.append((name, float(celsius),
                     smart if isinstance(smart, bool) else None))
    return rows

def identity_from_dmi(values: dict[str, str]) -> dict[str, str]:
    """Identité de la machine à partir des champs DMI.

    Même règle en local (fichiers `/sys/class/dmi/id`) et à distance (sonde
    SSH) : un produit générique laisse la place à la carte mère, seule
    information réellement utile sur une machine assemblée.
    """
    vendor = str(values.get("sys_vendor") or "").strip()
    product = str(values.get("product_name") or "").strip()
    if product and product.casefold() not in GENERIC_DMI:
        model = " ".join(part for part in (vendor, product) if part)
        label = "Modèle"
    else:
        model = " ".join(part for part in (
            str(values.get("board_vendor") or "").strip(),
            str(values.get("board_name") or "").strip(),
        ) if part)
        label = "Carte mère"
    bios = " · ".join(part for part in (
        str(values.get("bios_version") or "").strip(),
        str(values.get("bios_date") or "").strip(),
    ) if part)
    return {"model": model, "model_label": label, "bios": bios}

def machine_identity() -> dict[str, str]:
    """Modèle de la machine et version du BIOS, lus dans `/sys/class/dmi/id`."""
    return identity_from_dmi({
        name: _read_text(DMI_DIR / name)
        for name in (
            "sys_vendor", "product_name", "board_vendor", "board_name",
            "bios_version", "bios_date",
        )
    })

# Types de systèmes de fichiers sans disque réel (pseudo-fs, boucles, overlay)
# et points de montage système qui n'ont pas leur place sur le résumé.
MOUNT_SKIP_TYPES = {
    "autofs", "binfmt_misc", "bpf", "cgroup", "cgroup2", "composefs",
    "configfs", "debugfs", "devpts", "devtmpfs", "efivarfs", "fusectl",
    "hugetlbfs", "mqueue", "nsfs", "overlay", "proc", "pstore", "ramfs",
    "rpc_pipefs", "securityfs", "selinuxfs", "squashfs", "sysfs", "tmpfs",
    "tracefs",
}

MOUNT_SKIP_POINTS = {
    "/boot", "/boot/efi", "/dev", "/etc", "/proc", "/sys", "/sysroot",
}

# /proc/mounts échappe espaces et autres caractères en octal.
MOUNT_ESCAPES = ("\\040", "\\011", "\\012", "\\134")
MOUNT_CHARS = (" ", "\t", "\n", "\\")

def _mount_path(value: str) -> str:
    for escape, char in zip(MOUNT_ESCAPES, MOUNT_CHARS):
        value = value.replace(escape, char)
    return value

def _real_home(home: str | None = None) -> str:
    """Dossier personnel réel : sur ostree, `/home` est un lien vers /var/home."""
    return os.path.realpath(str(home or Path.home()))

def _home_mount(mount: str, home: str) -> bool:
    """Ce montage porte-t-il le dossier personnel ? (« / » est le système)."""
    if mount == "/":
        return False
    base = mount.rstrip("/")
    return mount == home or home.startswith(base + "/")

def _pick_mounts(entries: list[tuple], home: str) -> list[tuple]:
    """Un seul montage par périphérique, le plus parlant pour l'utilisateur.

    Un SSD Btrfs apparaît plusieurs fois (/var, /var/home, /sysroot…) : on
    garde le montage qui porte le dossier personnel, sinon le plus court.
    """
    by_source: dict[str, list[tuple]] = {}
    for entry in entries:
        by_source.setdefault(entry[0], []).append(entry)
    picked: list[tuple] = []
    for group in by_source.values():
        picked.append(sorted(group, key=lambda item: (
            0 if _home_mount(item[1], home) else 1,
            -len(item[1]) if _home_mount(item[1], home) else len(item[1]),
            item[1],
        ))[0])
    picked.sort(key=lambda item: (
        0 if _home_mount(item[1], home) else 1, len(item[1]), item[1]
    ))
    return picked

def parse_mounts(text: str, home: str | None = None) -> list[dict]:
    """`/proc/mounts` → un système de fichiers réel par périphérique."""
    home = _real_home(home)
    entries: list[tuple] = []
    for line in (text or "").splitlines():
        parts = line.split()
        if len(parts) < 3:
            continue
        source, mount, fstype = parts[0], _mount_path(parts[1]), parts[2]
        if not source.startswith("/dev/"):
            continue
        if fstype in MOUNT_SKIP_TYPES or mount in MOUNT_SKIP_POINTS:
            continue
        entries.append((source, mount, fstype))
    return [
        {"source": entry[0], "mount": entry[1], "fstype": entry[2]}
        for entry in _pick_mounts(entries, home)
    ]

def parse_df(text: str, home: str | None = None) -> list[dict]:
    """`df -P -B1` → disques réels, taille et espace libre en octets.

    Sert au poste distant : `shutil.disk_usage` ne voit que la machine locale.
    Les partitions d'amorçage (`/boot`, `/boot/efi`) et les pseudo-systèmes
    sont écartés comme pour `/proc/mounts`, sinon « efi » et « boot »
    apparaissent au milieu des disques de données.
    """
    home = _real_home(home)
    entries: list[tuple] = []
    for line in (text or "").splitlines():
        fields = line.split(None, 5)
        if len(fields) < 6 or not fields[0].startswith("/dev/"):
            continue
        mount = fields[5].strip()
        if mount in MOUNT_SKIP_POINTS:
            continue
        try:
            total = int(fields[1])
            free = int(fields[3])
        except ValueError:
            continue
        entries.append((fields[0], mount, total, free))
    return [
        {"source": entry[0], "mount": entry[1],
         "total": entry[2], "free": entry[3]}
        for entry in _pick_mounts(entries, home)
    ]

def mount_label(mount: str, home: str | None = None) -> str:
    """Nom court d'un montage : « Dossier personnel », « hdd1 », « /mnt/x »."""
    home = _real_home(home)
    if mount == home or (mount != "/" and home.startswith(mount.rstrip("/") + "/")):
        return "Dossier personnel"
    if mount == "/":
        return "Système"
    name = Path(mount).name
    return name or mount

def disk_row(source: str, mount: str, total, free,
             home: str | None = None) -> dict | None:
    """Un disque prêt à afficher : nom, tailles, pourcentage et cible Filelight.

    `path` est le dossier à ouvrir dans Filelight : le point de montage, sauf
    pour le disque du système — y scanner `/usr` en lecture seule et l'image
    entière n'apprend rien, le dossier personnel est ce que l'utilisateur
    cherche (choix explicite, 07/11/2026).
    """
    home = _real_home(home)
    try:
        total = int(total or 0)
        free = int(free or 0)
    except (TypeError, ValueError):
        return None
    if total <= 0:
        return None
    free = max(0, min(free, total))
    used = total - free
    system = mount == "/" or _home_mount(mount, home)
    return {
        "source": source,
        "mount": mount,
        "label": mount_label(mount, home),
        "total": total,
        "free": free,
        "percent": int(round(used * 100 / total)),
        "text": (
            f"{format_capacity(free)} libres sur {format_capacity(total)}"
        ),
        "path": home if system else mount,
        "system": system,
    }

def disk_rows(text: str | None = None, home: str | None = None) -> list[dict]:
    """Disques de la machine courante, lus dans `/proc/mounts`."""
    if text is None:
        text = _read_text(Path("/proc/mounts"))
    home = _real_home(home)
    rows: list[dict] = []
    for entry in parse_mounts(text, home):
        try:
            usage = shutil.disk_usage(entry["mount"])
        except OSError:
            continue
        row = disk_row(
            entry["source"], entry["mount"], usage.total, usage.free, home
        )
        if row is not None:
            rows.append(row)
    return rows

def format_disk_totals(rows: list[dict]) -> str:
    """« Total : 1,9 Tio libres sur 13 Tio   85 % utilisé » (vide si un seul).

    Même forme qu'une ligne de disque — espace libre d'abord, pourcentage
    utilisé ensuite — sinon la synthèse se lisait autrement que ce qu'elle
    résume. Addition des disques **affichés**, donc sans double compte : les
    points de montage d'un même périphérique sont déjà ramenés à un seul
    (`parse_mounts`). Rien à ajouter avec un disque unique : sa ligne dit
    déjà tout.
    """
    rows = [row for row in (rows or []) if row]
    if len(rows) < 2:
        return ""
    total = sum(int(row.get("total") or 0) for row in rows)
    free = sum(int(row.get("free") or 0) for row in rows)
    if total <= 0:
        return ""
    used = max(0, total - free)
    return (f"Total : {format_capacity(free)} libres sur "
            f"{format_capacity(total)}   "
            f"{round(used / total * 100)} % utilisé")

def disk_rows_from_df(text: str, home: str | None = None) -> list[dict]:
    """`df -P -B1` → disques prêts à afficher (poste distant)."""
    home = _real_home(home)
    rows: list[dict] = []
    for entry in parse_df(text, home):
        row = disk_row(
            entry["source"], entry["mount"], entry["total"], entry["free"],
            home,
        )
        if row is not None:
            rows.append(row)
    return rows

def format_disk_row(row: dict) -> str:
    """Une ligne de disque : « hdd1 : 1,3 Tio libres sur 1,8 Tio · 50 % utilisé »."""
    return (
        f"{row.get('label') or row.get('mount') or ''} : "
        f"{row.get('text') or ''} · {int(row.get('percent') or 0)} % utilisé"
    )

def format_disks(rows: list[dict], home: str | None = None) -> str:
    """Disques en texte, une ligne chacun (poste distant, infobulles)."""
    home = _real_home(home)
    lines: list[str] = []
    for row in rows or []:
        entry = row
        if "percent" not in entry:
            entry = disk_row(
                str(row.get("source") or ""), str(row.get("mount") or ""),
                row.get("total"), row.get("free"), home,
            )
        if entry:
            lines.append(format_disk_row(entry))
    return "\n".join(lines)

def disks_label(text: str | None = None) -> str:
    """Disques de la machine courante, une ligne chacun."""
    return "\n".join(format_disk_row(row) for row in disk_rows(text))

def parse_secure_boot(text: str, efi: bool) -> str:
    """`mokutil --sb-state` + présence d'EFI → enabled|disabled|unsupported|unknown."""
    if not efi:
        return "unsupported"
    if "SecureBoot enabled" in (text or ""):
        return "enabled"
    if "SecureBoot disabled" in (text or ""):
        return "disabled"
    return "unknown"

# -- températures -----------------------------------------------------------
#
# `/sys/class/hwmon` est la seule source sans privilège : chaque pilote y
# expose ses capteurs (k10temp/coretemp pour le CPU, amdgpu/nvidia pour le
# GPU, nvme/drivetemp pour les disques). Les valeurs sont en millidegrés.

HWMON_ROOT = Path("/sys/class/hwmon")
CPU_TEMP_CHIPS = ("k10temp", "coretemp", "zenpower", "cpu_thermal")
# Étiquettes utiles : « Tctl » (k10temp), « Package id 0 » (coretemp),
# « Tdie » (zenpower). À défaut, le maximum des capteurs du pilote.
CPU_TEMP_LABELS = ("Tctl", "Tdie", "Package id 0")
GPU_TEMP_CHIPS = ("amdgpu", "radeon", "nouveau", "i915", "xe", "nvidia")
GPU_TEMP_LABELS = ("edge", "temp1")
DISK_TEMP_CHIPS = ("nvme", "drivetemp")
DISK_TEMP_LABELS = ("Composite", "temp1")
# Barrettes : `spd5118` (DDR5) et `jc42` (DDR4) exposent `temp1` par barrette.
RAM_TEMP_CHIPS = ("spd5118", "jc42")
RAM_TEMP_LABELS = ("temp1",)
# VRAM : amdgpu l'expose dans /sys (octets), NVIDIA dans /proc (MiB).
VRAM_SYSFS = Path("/sys/class/drm/card*/device/mem_info_vram_total")
VRAM_VENDOR = Path("mem_info_vram_vendor")
VRAM_NVIDIA = Path("/proc/driver/nvidia/gpus/*/information")

# Espace insécable : « hdd5 38 °C » reste d'un bloc quand la case se replie.
NBSP = "\u00a0"
# Disques d'une même famille par ligne : au-delà, la case déborde.
TEMPS_PER_LINE = 3

def read_hwmon(root: Path | None = None) -> list[dict]:
    """Capteurs `/sys/class/hwmon` → [{chip, device, temps: [(label, °C)]}]."""
    base = HWMON_ROOT if root is None else root
    try:
        entries = sorted(base.glob("hwmon*"))
    except OSError:
        return []
    devices: list[dict] = []
    for entry in entries:
        chip = _read_text(entry / "name")
        if not chip:
            continue
        temps: list[tuple[str, float]] = []
        for path in sorted(entry.glob("temp*_input")):
            try:
                celsius = int(_read_text(path)) / 1000.0
            except ValueError:
                continue
            label = _read_text(
                Path(str(path)[: -len("_input")] + "_label")
            )
            temps.append((label or "temp1", celsius))
        if not temps:
            continue
        try:
            device = os.path.basename(os.readlink(entry / "device"))
        except OSError:
            device = ""
        devices.append({"chip": chip, "device": device, "temps": temps})
    return devices

def parse_hwmon_lines(text: str) -> list[dict]:
    """Lignes `capteur|périphérique|étiquette|millidegrés` → dicts de `read_hwmon`.

    Même lecture en local (`read_hwmon` sur /sys) qu'à distance (le script SSH
    imprime ces lignes) : une seule analyse, une seule sélection. Le
    périphérique sépare deux pilotes de même nom — deux NVMe sont deux
    instances `nvme`, pas un seul capteur à douze températures.
    """
    devices: dict[tuple[str, str], list[tuple[str, float]]] = {}
    for line in (text or "").splitlines():
        chip, sep, rest = line.partition("|")
        if not sep:
            continue
        device, sep, rest = rest.partition("|")
        if not sep:
            continue
        label, sep, raw = rest.partition("|")
        if not sep:
            continue
        try:
            celsius = int(raw.strip()) / 1000.0
        except ValueError:
            continue
        devices.setdefault((chip.strip(), device.strip()), []).append(
            (label.strip() or "temp1", celsius)
        )
    return [
        {"chip": chip, "device": device, "temps": temps}
        for (chip, device), temps in devices.items() if chip
    ]

def _pick_temperature(temps: list[tuple[str, float]],
                      preferred: tuple[str, ...]) -> float | None:
    for label, celsius in temps:
        if label in preferred:
            return celsius
    return max((value for _, value in temps), default=None)

def _numbered(values: list[float], label: str) -> list[tuple[str, float]]:
    """Températures d'une famille → [(« SSD », 31), (« SSD 2 », 33)…].

    Un seul élément garde le nom seul ; plusieurs sont numérotés dans l'ordre
    des capteurs, qui ne dépend que du matériel.
    """
    if len(values) == 1:
        return [(label, values[0])]
    return [(f"{label} {index}", value) for index, value in enumerate(values, 1)]

def nvm_smart(data: dict | None = None) -> dict[str, bool]:
    """Verdicts SMART des NVMe rendus par le helper : {« nvme0 »: True}."""
    if data is None:
        data = hwinfo()
    verdicts = (data or {}).get("nvm")
    if not isinstance(verdicts, dict):
        return {}
    return {str(device): bool(passed)
            for device, passed in verdicts.items()
            if isinstance(passed, bool)}

def disk_key(name: str) -> str:
    """Périphérique de base d'un nom de source : « nvme0n1p3 » → « nvme0 ».

    Une partition (`sdc1`, `nvme0n1p3`) ramène à son disque : c'est le disque
    qui a un capteur de température et un point de montage à nommer.
    """
    base = os.path.basename(str(name or "").strip())
    match = re.match(r"(nvme\d+|mmcblk\d+|md\d+|loop\d+)", base)
    if match:
        return match.group(1)
    match = re.match(r"([a-z]+)", base)
    return match.group(1) if match else base


def disk_labels(rows: list[dict] | None = None) -> dict[str, str]:
    """Périphérique → texte entre parenthèses dans le bloc de températures.

    « SSD 2 » ne dit pas quel disque c'est : on reprend l'étiquette de la page
    « Disques » (« hdd2 », « ssdtemp ») — et le disque qui porte `/` ou le
    dossier personnel s'appelle « Système » (demandé le 08/11/2026). Un disque
    à plusieurs points de montage garde « Système » s'il l'est, sinon le
    premier label rencontré.
    """
    labels: dict[str, str] = {}
    for row in rows or []:
        source = str(row.get("source") or "")
        key = disk_key(source)
        if not source or not key:
            continue
        if row.get("system"):
            labels[key] = "Système"
            continue
        labels.setdefault(key, str(row.get("label") or ""))
    return {key: text for key, text in labels.items() if text}


def parse_hwmon(devices: list[dict],
                nvm_smart_map: dict[str, bool] | None = None,
                labels: dict[str, str] | None = None) -> dict:
    """Capteurs hwmon → températures utiles (CPU, GPU, SSD, RAM).

    Un seul capteur par nature : le premier pilote connu qui en donne, avec
    l'étiquette significative quand elle existe (`edge` plutôt que
    `junction`, `Tctl` plutôt que les cœurs, `Composite` plutôt que les
    sondes internes). Un pilote absent — pas de capteur GPU sur un iGPU
    Intel, par exemple — ne s'invente pas : la valeur manque et rien ne
    s'affiche. Disques et barrettes sont numérotés : « SSD » puis « SSD 1 »,
    « SSD 2 », « RAM 1 », « RAM 2 ». `labels` (périphérique → étiquette,
    `disk_labels`) ajoute le disque concerné en clair : « SSD 1 (Système) ».
    """
    cpu: float | None = None
    gpu: float | None = None
    disks: list[tuple[str, float]] = []
    ram: list[float] = []
    for device in devices or []:
        chip = str(device.get("chip") or "")
        temps = list(device.get("temps") or [])
        if not temps:
            continue
        if cpu is None and chip in CPU_TEMP_CHIPS:
            cpu = _pick_temperature(temps, CPU_TEMP_LABELS)
        elif gpu is None and chip in GPU_TEMP_CHIPS:
            gpu = _pick_temperature(temps, GPU_TEMP_LABELS)
        elif chip in DISK_TEMP_CHIPS:
            value = _pick_temperature(temps, DISK_TEMP_LABELS)
            if value is not None:
                disks.append((str(device.get("device") or ""), value))
        elif chip in RAM_TEMP_CHIPS:
            value = _pick_temperature(temps, RAM_TEMP_LABELS)
            if value is not None:
                ram.append(value)
    # Le nom est numéroté à part, puis rattaché au périphérique hwmon : c'est
    # ce dernier qui porte le verdict SMART venu du helper (`nvme0`).
    names = _numbered([value for _, value in disks], "SSD")
    ssd = []
    for (device, value), (name, _) in zip(disks, names):
        # Le périphérique hwmon (« nvme1 ») donne l'étiquette du disque ; il
        # porte aussi le verdict SMART venu du helper.
        label = (labels or {}).get(disk_key(device))
        ssd.append((
            f"{name} ({label})" if label else name,
            value,
            (nvm_smart_map or {}).get(device),
        ))
    return {
        "cpu": cpu,
        "gpu": gpu,
        "ssd": ssd,
        "ram": _numbered(ram, "RAM"),
    }

# Verdict SMART d'un disque SATA, tel que `smartctl -H` le rend.
SMART_MARKS = {True: "✓", False: "✗"}

def _measure(name: str, value: float, smart: bool | None = None) -> str:
    """« hdd5 38 °C ✓ » : les espaces internes sont insécables.

    La case du tableau peut se replier ; une mesure ne doit jamais être
    coupée en deux entre son nom, son chiffre, son unité et son verdict. Le
    verdict n'apparaît que s'il est connu : un disque en veille, ou un pont
    USB qui ne publie rien, n'affiche ni ✓ ni ✗.
    """
    text = f"{str(name).replace(' ', NBSP)}{NBSP}{value:.0f}{NBSP}°C"
    mark = SMART_MARKS.get(smart)
    return f"{text}{NBSP}{mark}" if mark else text

def temperature_measures(text: str) -> list[list[tuple[str, str]]]:
    """« SSD 1 37 °C ✓  SSD 2 32 °C ✓ » → [[("SSD 1", "37 °C ✓"), …]].

    Une ligne de `format_temperatures` porte jusqu'à trois mesures séparées
    par deux espaces, chacune « nom <insécable> 38 <insécable> °C … ». Le nom
    peut lui-même contenir une insécable (« SSD 1 », « RAM 2 »), d'où la
    recherche du **dernier** séparateur devant le chiffre et non d'une simple
    coupure au premier : couper bêtement donnerait « SSD » + « 1 37 °C ».

    Sert à composer chaque ligne en deux corps de texte, comme les lignes de
    disques : le nom en clair, la mesure en gris. Les espaces insécables sont
    rendues comme des espaces normales — la composition n'en a plus besoin,
    chaque morceau est un libellé à part.
    """
    rows: list[list[tuple[str, str]]] = []
    for line in (text or "").splitlines():
        line = line.strip()
        if not line:
            continue
        measures: list[tuple[str, str]] = []
        for chunk in re.split(r" {2,}", line):
            match = re.match(r"^(.+?)\u00a0(\d+(?:[.,]\d+)?\u00a0°C.*)$", chunk)
            name, detail = (match.group(1), match.group(2)) if match else (chunk, "")
            measures.append((name.replace(NBSP, " "),
                             detail.replace(NBSP, " ").strip()))
        rows.append(measures)
    return rows

def natural_key(text: str) -> tuple[str, ...]:
    """Clé de tri « naturelle » : « hdd2 » avant « hdd10 », « hdd1 » avant « sdc ».

    Un tri texte classerait 10 avant 2, et l'ordre rendu par `hwinfo` suivait
    ses propres montages : la ligne de températures SATA s'affichait « 3 2 1 »,
    à l'envers de la numérotation (vécu 08/11/2026). Les nombres sont comparés
    comme des nombres, le reste comme du texte insensible à la casse.
    """
    parts = re.split(r"(\d+)", (text or "").casefold())
    return tuple(
        f"{int(part):08d}" if part.isdigit() else part for part in parts
    )


def format_temperatures(values: dict,
                        sata: list[tuple[str, float]] | None = None) -> str:
    """Une ligne par famille — « CPU », « GPU », « SSD 1  SSD 2 »…

    Les disques d'une même famille partagent leur ligne (un NVMe par SSD, un
    SATA par hdd) : sur un poste à six disques, une seule ligne pour tout
    serait illisible et une ligne par disque prendrait toute la hauteur. Rien
    n'est inventé : une famille sans capteur ne produit aucune ligne.
    """
    values = values or {}
    lines: list[str] = []
    for key, label in (("cpu", "CPU"), ("gpu", "GPU")):
        value = values.get(key)
        if value:
            lines.append(_measure(label, value))
    # Les disques SATA arrivent dans l'ordre du helper : on les trie par nom
    # (hdd1, hdd2, hdd10) — « 3 2 1 » se lisait à l'envers (vécu 08/11/2026).
    # Leur nom vient du point de montage, qui change d'un poste à l'autre
    # (« hdd2 », « data2 », « films ») : ils sont donc numérotés comme les SSD
    # (« HDD 1 ») et le montage passe entre parenthèses — sans lui, « DATA2 »
    # ne disait plus de quel disque il s'agissait (demandé le 08/11/2026).
    sata = sorted(sata or [], key=lambda entry: natural_key(str(entry[0])))
    sata = [
        (f"{label} ({entry[0]})", entry[1]) + tuple(entry[2:])
        for (label, _), entry in zip(
            _numbered([entry[1] for entry in sata], "HDD"), sata
        )
    ]
    for entries in (values.get("ssd"), sata, values.get("ram")):
        entries = [entry for entry in entries or [] if entry]
        for start in range(0, len(entries), TEMPS_PER_LINE):
            lines.append("  ".join(
                _measure(str(entry[0]), entry[1],
                         entry[2] if len(entry) > 2 else None)
                for entry in entries[start:start + TEMPS_PER_LINE]
            ))
    return "\n".join(lines)

def temperature_summary(devices: list[dict] | None = None,
                        sata: list[tuple] | None = None,
                        nvm: dict[str, bool] | None = None,
                        disks: list[dict] | None = None) -> str:
    """Températures du poste, prêtes à afficher (vide si aucun capteur).

    `sata` vient du helper `gablue-hwinfo` (`smartctl -n standby`) : les
    disques SATA n'ont pas de capteur hwmon tant que `drivetemp` n'est pas
    chargé, et ce pilote lit le SMART sans regarder le mode d'alimentation —
    il réveillerait les disques endormis, ce que la sonde évite.
    """
    values = parse_hwmon(
        read_hwmon() if devices is None else devices, nvm, disk_labels(disks),
    )
    return format_temperatures(values, sata)

def hardware_summary(sata: list[tuple] | None = None,
                     nvm: dict[str, bool] | None = None) -> dict[str, str]:
    """Résumé de la machine, en lectures locales uniquement.

    Pas de sous-processus coûteux : `lspci` est mis en cache, tout le reste
    vient de /proc, /sys et `disk_usage`. Les barrettes, qui demandent root,
    et le nom commercial du GPU (Vulkan) arrivent par `workers.HardwareWorker`.
    """
    values = machine_identity()
    values["system"] = local_distro()
    values["cpu"] = cpu_label()
    values["gpu"] = gpu_label()
    vram, dedicated = gpu_vram()
    if vram:
        # Gardé à part : la fenêtre remplace le nom du die par le nom
        # commercial (Vulkan) et doit pouvoir réattacher la VRAM.
        values["vram"] = format_vram(vram, dedicated)
    values["memory"] = format_memory(_read_text(Path("/proc/meminfo")))
    values["disks"] = disks_label()
    # Températures CPU/GPU/NVMe : /sys seulement, aucun privilège (vide si la
    # machine n'expose pas de capteur) ; `sata` vient du helper `gablue-hwinfo`
    # (smartctl sans réveil), mis en cache par la fenêtre.
    values["temperatures"] = temperature_summary(
        sata=sata, nvm=nvm, disks=disk_rows(),
    )
    try:
        values["kernel"] = os.uname().release
    except OSError:
        values["kernel"] = ""
    boot = boot_time()
    values["uptime"] = (
        "depuis " + format_uptime(time.time() - boot) if boot > 0 else ""
    )
    values["secure_boot"] = secure_boot_label(secure_boot_state())
    return {key: value for key, value in values.items() if value}



def loaded_modules() -> set[str]:
    """Modules noyau chargés (lecture sans privilège)."""
    try:
        text = Path("/proc/modules").read_text(encoding="utf-8")
    except OSError:
        return set()
    return {line.split()[0] for line in text.splitlines() if line.strip()}

def parse_staged_deployment(text: str) -> bool:
    """Une image téléchargée attend-elle un redémarrage ?

    `rpm-ostree status --json` marque ce déploiement `staged: true` ; les
    anciens déploiements (pinned, précédent) ne le sont pas.
    """
    try:
        data = json.loads(text)
    except (TypeError, ValueError):
        return False
    return any(
        bool(deployment.get("staged"))
        for deployment in data.get("deployments", [])
        if isinstance(deployment, dict)
    )

def kargs_pending_change(current: set[str], pending: set[str]) -> bool:
    """Les paramètres du noyau en cours diffèrent-ils de ceux du prochain boot ?

    `BOOT_IMAGE=` n'existe que dans `/proc/cmdline` et décrit le noyau en
    cours : sans l'écarter, tout poste semblerait en attente de redémarrage.
    """
    def stable(args: set[str]) -> set[str]:
        return {arg for arg in args if not arg.startswith("BOOT_IMAGE=")}
    return stable(current) != stable(pending)

def pending_reboot_reasons() -> list[str]:
    """Ce qui attend un redémarrage pour prendre effet (lecture sans root).

    Quatre sources, dans l'ordre où on les présente :
    image ostree staged, kargs du prochain démarrage différents, configs
    écrites depuis le démarrage (Wi-Fi Broadcom, couche Vulkan low-latency,
    émulation CPUID), et module d'émulation encore chargé alors que sa
    persistance a été retirée.

    La couche Vulkan low-latency ne demande qu'une **nouvelle session**, mais
    elle est signalée comme les autres : sans redémarrage ni reconnexion,
    le réglage reste sans effet et l'utilisateur n'a aucun autre signe.
    """
    reasons: list[str] = []
    try:
        result = subprocess.run(
            [_which("rpm-ostree"), "status", "--json"],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        result = None
    if (result is not None and result.returncode == 0
            and parse_staged_deployment(result.stdout)):
        reasons.append("nouvelle image Gablue téléchargée")

    if kargs_pending_change(kernel_cmdline(), kargs_pending()):
        reasons.append("paramètres du noyau modifiés")

    boot = boot_time()
    if written_since(BROADCOM_WL_FILE, boot):
        reasons.append("pilote Wi-Fi Broadcom wl")
    if written_since(LOW_LATENCY_FILE, boot):
        reasons.append("couche Vulkan low-latency (nouvelle session)")
    if (written_since(CPUID_EMU_CONF, boot)
            or written_since(CPUID_EMU_BLACKLIST, boot)
            or ("cpuid_fault_emulation" in loaded_modules()
                and not CPUID_EMU_CONF.exists())):
        reasons.append("émulation CPUID fault")
    return reasons

def kargs_choices(kargs: set[str], emu_active: bool = False) -> dict[str, bool]:
    """État des cases à cocher : coché = réglage Gablue appliqué."""
    return {
        "mitigations": KARG_MITIGATIONS_OFF not in kargs,
        "cpuid": KARG_CLEARCPUID in kargs,
        "ertm": KARG_BT_ERTM in kargs,
        "hdmi21": KARG_HDMI21 in kargs,
        "cpuid_emu": emu_active,
    }

def kernel_apply_script(changes: dict[str, bool]) -> str:
    """Script d'application : les helpers /usr/libexec, en un seul passage.

    Les trois paramètres du noyau partent dans un unique `rpm-ostree kargs`
    (un déploiement) ; l'émulation CPUID fault passe par son propre helper car
    elle ne touche pas la ligne de commande mais charge un module.
    """
    flags: list[str] = []
    for key in ("mitigations", "cpuid", "ertm", "hdmi21"):
        if key in changes:
            flags += [f"--{key}", "on" if changes[key] else "off"]
    lines: list[str] = []
    if flags:
        lines.append(f"sudo -n {KARG_HELPER} " + " ".join(flags))
    if "cpuid_emu" in changes:
        lines.append(
            f"sudo -n {CPUID_EMU_HELPER} "
            + ("on" if changes["cpuid_emu"] else "off")
        )
    return "\n".join(lines) + "\n"

def remote_helpers_nopasswd(host: dict) -> bool:
    """Les helpers `/usr/libexec` passent-ils sans mot de passe à distance ?

    `sudo -n true` ne dit rien (aucune règle pour `true`, même sur une image
    Gablue) et `sudo -l <cmd>` rend toujours 0. On sonde donc la sous-commande
    `status` du helper, sans effet : elle répond 0 quand la règle NOPASSWD
    existe, et sudo rend 1 quand il réclame le mot de passe.
    """
    code, _ = ssh_capture(host, f"sudo -n {CPUID_EMU_HELPER} status", timeout=10)
    return code == 0

def flatpak_update_script() -> str:
    """Mise à jour des Flatpaks **système** uniquement, sans mot de passe.

    Gablue n'installe rien en `--user` : aucune installation du compte n'est
    mise à jour ici. L'image autorise `/usr/bin/flatpak update --system -y`
    dans `/etc/sudoers.d/nopasswd` (arguments exacts, aucun joker), donc
    `sudo -n` passe sans dialogue. Hors image Gablue la règle manque : l'appel
    direct reprend la main et PolicyKit demande l'autorisation comme avant.
    """
    return (
        "sudo -n flatpak update --system -y || "
        "flatpak update --system -y\n"
    )

# Ligne de progression de flatpak hors terminal (sortie non « fancy ») :
# « Updating 3/7… » à l'ouverture de chaque opération, puis la même ligne suivie
# d'une barre en espaces et du pourcentage, réémise à chaque changement. Les
# msgid anglais ne sont pas traduits en français (vérifié 07/11/2026, flatpak
# 1.18.4 : « Updating %d/%d… » et « %3d%% » ont un msgstr vide dans po/fr.po).
_FLATPAK_OP_RE = re.compile(
    r"^\s*(Updating|Installing|Uninstalling)(?:\s+(\d+)/(\d+))?\u2026"
)
_FLATPAK_PERCENT_RE = re.compile(r"(\d{1,3})\s*%")

FLATPAK_ACTIONS = {
    "Updating": "Mise à jour des Flatpaks",
    "Installing": "Installation de Flatpaks",
    "Uninstalling": "Désinstallation de Flatpaks",
}

def parse_flatpak_progress(line: str) -> dict:
    """Ligne de `flatpak update` → progression (action, op/total, pourcentage).

    Hors terminal, flatpak ne dessine pas son tableau : il écrit l'ouverture de
    chaque opération (« Updating 3/7… ») puis réémet cette ligne avec le
    pourcentage (« Updating 3/7…      45% 1,2 Mo/s ») à chaque changement. Le
    pourcentage renvoyé est global à la phase en cours —
    `(op - 1 + pct / 100) / total` — pour que la barre avance d'une opération à
    l'autre ; sur la ligne d'ouverture (aucun pourcentage), on retient le début
    de l'opération. `{}` pour toute autre ligne (« Looking for updates… »,
    « Nothing to do. »), qui reste donc du journal.
    """
    match = _FLATPAK_OP_RE.match(line or "")
    if match is None:
        return {}
    action = match.group(1)
    op = int(match.group(2) or 1)
    total = max(1, int(match.group(3) or 1))
    percentages = _FLATPAK_PERCENT_RE.findall(line[match.end():])
    percent = None
    if percentages:
        percent = max(0.0, min(100.0, float(percentages[-1])))
    done = min(op, total) - 1 + (percent or 0.0) / 100.0
    label = FLATPAK_ACTIONS.get(action, "Mise à jour des Flatpaks")
    if total > 1:
        label += f" {op}/{total}"
    return {
        "action": action,
        "label": label,
        "op": op,
        "total": total,
        "percent": min(100.0, 100.0 * done / total),
    }

def flatpak_update_hint(text: str) -> str:
    """Phrase de fin de journal pour `flatpak update --system`.

    Une seule installation est mise à jour (le système) : « Nothing to do. »
    veut donc bien dire qu'il n'y avait rien à faire. L'ancien script enchaînait
    compte puis système, et la seconde phase aurait menti.
    """
    if "Nothing to do." in (text or ""):
        return "Aucune mise à jour disponible."
    return ""

def brew_available() -> bool:
    """Homebrew est-il installé sur ce poste ?"""
    return BREW_BINARY.exists() or shutil.which("brew") is not None

def brew_update_script() -> str:
    """Mise à jour de Homebrew : `brew update` puis `brew upgrade`.

    Même mécanique que les deux timers de l'image (même ordre, même préfixe) :
    le chemin est absolu parce que le PATH d'une session GUI ne contient pas
    `/home/linuxbrew/.linuxbrew/bin`, et `HOMEBREW_NO_AUTO_UPDATE` évite un
    second `brew update` implicite pendant l'upgrade. Le préfixe se déduit du
    binaire **sans le résoudre** (`/home` est un lien vers `/var/home` sur
    ostree, et brew compare ses chemins à ceux de son installation). Aucun
    privilège : brew travaille dans son préfixe et dans le cache de
    l'utilisateur.
    """
    return (
        "brew=/home/linuxbrew/.linuxbrew/bin/brew\n"
        "if [ ! -x \"$brew\" ]; then brew=\"$(command -v brew)\"; fi\n"
        "if [ -z \"$brew\" ]; then\n"
        "    echo \"Homebrew introuvable sur ce poste.\"\n"
        "    exit 127\n"
        "fi\n"
        "prefix=\"${brew%/bin/brew}\"\n"
        "export HOMEBREW_PREFIX=\"$prefix\"\n"
        "export HOMEBREW_CELLAR=\"$prefix/Cellar\"\n"
        "export HOMEBREW_REPOSITORY=\"$prefix/Homebrew\"\n"
        "export HOMEBREW_NO_AUTO_UPDATE=1\n"
        "rc=0\n"
        "\"$brew\" update || rc=$?\n"
        "\"$brew\" upgrade || rc=$?\n"
        "exit $rc\n"
    )

# En-têtes de `brew upgrade` (sortie non TTY, sans couleur) : liste des
# téléchargements, total éventuel, puis une ligne par formule. Homebrew 7 les
# écrit en clair, mais ne donne aucun pourcentage : voir `brew_progress_parser`.
_BREW_TOTAL_RE = re.compile(
    r"^==>\s+Upgrading\s+(\d+)\s+outdated\s+(?:packages?|casks?|formulae?)\s*:"
)
_BREW_STEP_RE = re.compile(r"^==>\s+Upgrading\s+(.+?)\s*$")
_BREW_FETCH_RE = re.compile(r"^==>\s+Fetching downloads for\s*:\s*(.+?)\s*$")

def brew_progress_parser():
    """Analyseur stateful de `brew upgrade` → progression par formule.

    Homebrew ne publie **aucun pourcentage** quand sa sortie n'est pas un
    terminal : `curl` est lancé avec `--silent` (`utils/curl.rb`, vérifié sur
    Homebrew 7.0.12 le 07/11/2026), donc ni jauge ni débit, et la file de
    téléchargements concurrente n'affiche que des lignes de fin. Ce qui reste
    fiable, ce sont ses en-têtes : « Fetching downloads for: a, b » donne la
    liste des formules à télécharger (Homebrew 7 ne résume qu'après), chaque
    « Upgrading <nom> » avance d'une formule, et un éventuel « Upgrading N
    outdated packages: » (versions antérieures) fixe le total. La barre
    progresse donc par paquets, pas en octets ; les lignes de dépendance
    (« Upgrading <formule> dependency: <dep> … ») ne comptent pas, elles ne font
    pas partie du total.

    Renvoie un appelable `(ligne) -> info` : `{"label", "percent"}` (percent
    `None` = barre indéterminée, pendant les téléchargements) ou `{}` pour les
    lignes sans intérêt. L'état (total, rang) vit dans la fermeture : chaque
    fenêtre de tâche a le sien.
    """
    state = {"total": 0, "op": 0}

    def info(label: str, percent: float | None) -> dict:
        return {"label": label, "percent": percent}

    def parse(line: str) -> dict:
        text = (line or "").rstrip()
        match = _BREW_TOTAL_RE.match(text)
        if match is not None:
            state["total"] = max(1, int(match.group(1)))
            state["op"] = 0
            count = state["total"]
            return info(
                f"Mise à jour de {count} "
                + ("formule" if count == 1 else "formules"),
                0.0,
            )
        match = _BREW_FETCH_RE.match(text)
        if match is not None:
            # Téléchargements en cours : le résumé « Upgrading N … » n'arrive
            # qu'après (Homebrew 7), mais l'en-tête de la file donne déjà la
            # liste des formules concernées. Aucun pourcentage n'est
            # disponible : la barre reste indéterminée.
            names = match.group(1)
            count = len([name for name in re.split(r",\s*|\s+and\s+", names)
                         if name.strip()])
            state["total"] = count
            state["op"] = 0
            label = ("Téléchargement de la formule…" if count <= 1
                     else f"Téléchargement de {count} formules")
            return info(label, None)
        match = _BREW_STEP_RE.match(text)
        if match is None:
            return {}
        name = match.group(1)
        if " dependency:" in name:      # dépendance tirée par une formule
            return {}
        state["op"] += 1
        total = state["total"]
        percent = None
        if total > 0:
            state["op"] = min(state["op"], total)
            percent = 100.0 * state["op"] / total
        label = f"Mise à jour de {name}"
        if total > 1:
            label += f" ({state['op']}/{total})"
        return info(label, percent)

    return parse

def brew_update_hint(text: str) -> str:
    """Phrase de fin de journal pour `brew update` + `brew upgrade`.

    `brew upgrade` n'écrit rien quand aucune formule n'est en retard (un
    `brew update` sans changement se contente de « Already up-to-date. ») :
    sans en-tête d'upgrade, le compte rendu doit le dire au lieu de laisser un
    simple « Terminé. ».
    """
    if any(_BREW_TOTAL_RE.match(line) or _BREW_STEP_RE.match(line)
           or _BREW_FETCH_RE.match(line)
           for line in (text or "").splitlines()):
        return ""
    return "Aucune formule à mettre à jour."

def gablue_update_hint(text: str) -> str:
    """Phrase de fin de journal pour `bootc upgrade`.

    bootc répond « No changes in … » quand rien n'est nouveau, et ajoute
    « Staged update present, not changed » quand un déploiement attend déjà un
    redémarrage. Chercher « staged » sans traiter ce cas annonçait « nouvelle
    image » après une mise à jour sans changement (vécu 07/11/2026) ; le
    message doit dire la vérité : rien de neuf, ou une image déjà en attente.
    """
    low = (text or "").lower()
    if "no changes in" in low or "staged update present" in low:
        if "staged update present" in low or "no changes in staged image" in low:
            return (
                "Rien de nouveau à télécharger ; une image est déjà en "
                "attente — redémarrage pour l'appliquer."
            )
        return "Aucune mise à jour disponible : le poste est déjà à jour."
    if any(mark in low for mark in (
        "queued for next boot", "staged but not queued", "upgraded", "staged",
    )):
        return "Redémarrage nécessaire pour appliquer la nouvelle image."
    return ""

def gwine_binary() -> str:
    """Chemin de gwine (repli /usr/bin, le PATH d'une session GUI est incomplet)."""
    found = shutil.which("gwine")
    if found:
        return found
    fallback = Path("/usr/bin/gwine")
    return str(fallback) if fallback.exists() else "gwine"

def gwine_options_path() -> Path:
    """Fichier d'options de gwine (`$GWINE_DIR/options`)."""
    return Path.home() / ".local/share/gwine/options"

def parse_gwine_options(text: str) -> dict:
    """Lit `dxvk_mode`, `display_mode` et `xbox_default`."""
    values: dict[str, str] = {}
    for line in text.splitlines():
        if "=" in line:
            key, _, value = line.partition("=")
            values[key.strip()] = value.strip()
    return {
        "dxvk_async": values.get("dxvk_mode") == "dxvk-async",
        "wayland": values.get("display_mode") == "wayland",
        "xbox": values.get("xbox_default") == "on",
    }

def parse_foreground_booster(text: str) -> bool:
    """Lit `[Foreground Booster] autostart` de kcgroupsrc (absent = activé)."""
    parser = configparser.RawConfigParser(strict=False)
    parser.optionxform = str
    try:
        if text.strip():
            parser.read_string(text)
        if parser.has_option("Foreground Booster", "autostart"):
            return parser.getboolean("Foreground Booster", "autostart")
    except (configparser.Error, ValueError):
        pass
    return True

def foreground_booster_path() -> Path:
    return Path.home() / ".config/kcgroupsrc"

def valid_tailscale_server(url: str) -> bool:
    """Lien de control server : http(s), sans espace ni guillemet."""
    return (
        url.startswith(("http://", "https://"))
        and len(url) > 9
        and not any(char.isspace() or char in "'\"" for char in url)
    )

AUTH_URL_RE = re.compile(r"https?://[^\s'\"<>]+")

def find_auth_url(text: str, exclude: tuple[str, ...] = ()) -> str:
    """Lien d'enregistrement dans la sortie de `tailscale up`.

    Le helper écrit d'abord « Connexion au serveur <control server>… », puis
    tailscale affiche le lien d'authentification, toujours en dernier. On
    prend donc le dernier lien qui n'est pas le serveur de connexion : le
    chemin varie selon le serveur (Tailscale `/a/…`, headscale
    `/register/…`), le coder en dur en casserait un des deux.
    """
    ignored = {url.rstrip("/") for url in exclude if url}
    urls = [
        match.group(0).rstrip(".,;:)]}…!")
        for match in AUTH_URL_RE.finditer(text or "")
    ]
    for url in reversed(urls):
        if url.rstrip("/") in ignored:
            continue
        return url
    return ""

def tailscale_registered(text: str) -> bool:
    """`tailscale status --json` : nœud authentifié et actif ?

    Lu sans privilège (l'appareil est confié à l'opérateur par
    `--operator`), comme sonde de validation : dès que le navigateur a
    confirmé, l'état passe à `Running` et le lien d'enregistrement n'a plus
    lieu d'être affiché.
    """
    start = (text or "").find("{")
    if start < 0:
        return False
    try:
        data = json.loads(text[start:])
    except (TypeError, ValueError):
        return False
    return isinstance(data, dict) and data.get("BackendState") == "Running"

def tailscale_apply_script(enabled: bool, server: str = "") -> str:
    """Commande du helper : `on --login-server URL` ou `off`."""
    if enabled:
        return (
            f"sudo -n {TAILSCALE_HELPER} on --login-server "
            f"{shlex.quote(server)}\n"
        )
    return f"sudo -n {TAILSCALE_HELPER} off\n"


TAILSCALE_STATE_SCRIPT = r"""
printf 'enabled=%s\n' "$(systemctl is-enabled tailscaled.service 2>/dev/null | head -n 1)"
printf 'active=%s\n' "$(systemctl is-active tailscaled.service 2>/dev/null | head -n 1)"
printf 'server=%s\n' "$(tailscale debug prefs 2>/dev/null | tr ',' '\n' | grep -m1 'ControlURL' | cut -d'"' -f4)"
printf 'installed=%s\n' "$(command -v tailscale >/dev/null 2>&1 && echo yes || echo no)"
printf 'status=%s\n' "$(tailscale status --json 2>/dev/null | tr -d '\n')"
"""

def parse_tailscale_status(text: str) -> dict:
    """`tailscale status --json` → réseau, nom du nœud et adresse.

    `CurrentTailnet.Name` est le serveur de contrôle chez Headscale (le domaine
    du tailnet chez Tailscale SaaS) ; `MagicDNSSuffix` donne le suffixe commun
    des noms. Le nom utilisable par les autres nœuds est `Self.DNSName`, dont on
    retire le point final et le suffixe MagicDNS (affiché à part).
    """
    start = (text or "").find("{")
    if start < 0:
        return {}
    try:
        data = json.loads(text[start:])
    except (TypeError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    self_info = data.get("Self") or {}
    tailnet = data.get("CurrentTailnet") or {}
    addresses = self_info.get("TailscaleIPs") or []
    suffix = str(tailnet.get("MagicDNSSuffix") or "")
    name = str(self_info.get("DNSName") or "").rstrip(".")
    if suffix and name.endswith("." + suffix):
        # Le nom court suffit dans l'interface : « gigafed », pas
        # « gigafed.tail.io » quand le réseau s'appelle déjà tail.io.
        name = name[: -(len(suffix) + 1)]
    return {
        "backend": str(data.get("BackendState") or ""),
        "network": suffix or str(tailnet.get("Name") or ""),
        "node": name or str(self_info.get("HostName") or ""),
        "address": str(addresses[0]) if addresses else "",
    }


def parse_tailscale_state(output: str) -> dict:
    """État Tailscale à partir des lignes `clé=valeur` du script ci-dessus."""
    values: dict[str, str] = {}
    for line in output.splitlines():
        key, _, value = line.partition("=")
        if key.strip() in ("enabled", "active", "server", "installed",
                           "status"):
            values[key.strip()] = value.strip()
    return {
        "installed": values.get("installed") == "yes",
        "enabled": values.get("enabled") == "enabled",
        "active": values.get("active") == "active",
        "server": values.get("server", ""),
        "status": parse_tailscale_status(values.get("status", "")),
    }


# =============================================================================
# SERVICES, COUCHE VULKAN ET VARIANTE (ex-recettes ujust)
# =============================================================================

def parse_variant(text: str) -> str:
    """Variante Gablue de l'image démarrée, depuis `rpm-ostree status --json`.

    Le déploiement `booted` porte sa référence d'image
    (`ostree-image-signed:docker://…/gablue-main-dx:latest`) : on en extrait le
    nom. Chaîne vide hors image Gablue ou si le format change.
    """
    try:
        data = json.loads(text)
    except (TypeError, ValueError):
        return ""
    for deployment in data.get("deployments", []):
        if not deployment.get("booted"):
            continue
        reference = str(deployment.get("container-image-reference") or "")
        match = re.search(r"gablue-([a-z0-9-]+)", reference)
        return match.group(1) if match else ""
    return ""


def local_variant() -> str:
    """Variante de l'image démarrée (lecture sans privilège)."""
    try:
        result = subprocess.run(
            ["rpm-ostree", "status", "--json"],
            capture_output=True, text=True, timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    if result.returncode != 0:
        return ""
    return parse_variant(result.stdout)


def service_enabled(name: str) -> bool:
    """Le service est-il activé au démarrage ?"""
    try:
        result = subprocess.run(
            ["systemctl", "is-enabled", name],
            capture_output=True, text=True, timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def auto_update_available() -> bool:
    """Le helper des mises à jour automatiques est-il dans l'image ?"""
    return Path(AUTO_UPDATE_HELPER).is_file()


AUTO_UPDATE_TARGETS = ("image", "flatpak", "brew")

def parse_auto_update_status(text: str) -> dict:
    """Lignes `clé=valeur` du helper → domaines actifs et fréquence de chacun."""
    values: dict[str, str] = {}
    for line in (text or "").splitlines():
        key, _, value = line.partition("=")
        key = key.strip()
        if key in (
            "image", "flatpak", "brew",
            "image_frequency", "flatpak_frequency", "brew_frequency",
        ):
            values[key] = value.strip()

    def frequency(domain: str) -> str:
        value = values.get(f"{domain}_frequency", "weekly")
        return value if value in ("daily", "weekly") else "weekly"

    return {
        "image": values.get("image") == "enabled",
        "flatpak": values.get("flatpak") == "enabled",
        "brew": values.get("brew") == "enabled",
        "frequencies": {
            domain: frequency(domain) for domain in AUTO_UPDATE_TARGETS
        },
    }

def auto_update_state() -> dict:
    """État complet des timers système, lu par le helper privilégié."""
    state = {
        "ok": False, "image": False, "flatpak": False, "brew": False,
        "frequencies": {domain: "weekly" for domain in AUTO_UPDATE_TARGETS},
    }
    try:
        result = subprocess.run(
            ["sudo", "-n", AUTO_UPDATE_HELPER, "status"],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return state
    if result.returncode != 0:
        return state
    state.update(parse_auto_update_status(result.stdout))
    state["ok"] = True
    return state

def _steps_script(steps: list[tuple[str, list[str]]]) -> str:
    """Assemble des étapes en un script qui publie sa progression.

    Chaque domaine annonce `::gablue:step n total libellé` (retiré du journal
    par `parse_gablue_progress`) juste avant ses commandes, et `done` conclut.
    Le total ne compte que les domaines réellement modifiés.
    """
    total = len(steps)
    lines: list[str] = []
    for index, (label, commands) in enumerate(steps, 1):
        lines.append(f"printf '%s\\n' " + shlex.quote(label))
        lines.append(f"printf '%s\\n' " + shlex.quote(
            f"::gablue:step {index} {total} {label}"
        ))
        lines.extend(commands)
    if lines:
        lines.append("printf '%s\\n' " + shlex.quote("::gablue:done"))
    return "\n".join(lines) + "\n"

def auto_update_script(changes: dict[str, bool],
                       frequencies: dict[str, str]) -> str:
    """Script d'application : domaines modifiés et fréquences.

    `frequencies` ne porte que les domaines dont la fréquence change : un
    domaine inchangé garde son drop-in, ou le défaut hebdomadaire de l'image.
    Tout passe par le helper système : Gablue ne met à jour que les Flatpaks
    `--system`, il n'y a plus de timer du compte à basculer.
    """
    steps: list[tuple[str, list[str]]] = []
    for key, label in (
        ("image", "Image Gablue"),
        ("flatpak", "Flatpaks système"),
        ("brew", "Homebrew"),
    ):
        if key in changes:
            steps.append((label, [
                f"sudo -n {AUTO_UPDATE_HELPER} {key} "
                + ("on" if changes[key] else "off")
            ]))
    for key, label in (
        ("image", "Fréquence — image Gablue"),
        ("flatpak", "Fréquence — Flatpaks"),
        ("brew", "Fréquence — Homebrew"),
    ):
        if key in frequencies:
            steps.append((label, [
                f"sudo -n {AUTO_UPDATE_HELPER} schedule {key} "
                f"{frequencies[key]}"
            ]))
    return _steps_script(steps)


def docker_available(variant: str) -> bool:
    """Le basculement Docker n'a de sens que sur une variante DX."""
    return variant.endswith("-dx")


def atuin_active() -> bool:
    """L'intégration atuin est-elle branchée dans le shell de l'utilisateur ?"""
    try:
        text = (Path.home() / ".bashrc").read_text(
            encoding="utf-8", errors="replace"
        )
    except OSError:
        return False
    return "atuin init bash" in text


def atuin_available() -> bool:
    """Le helper gablue-atuin est-il installé (présent dans l'image) ?"""
    return Path(ATUIN_HELPER).is_file()


def gamepad_available() -> bool:
    """Le helper des raccourcis manette est-il installé (image à jour) ?"""
    return Path(GAMEPAD_HELPER).is_file()


def gamepad_active() -> bool:
    """Les raccourcis manette Gablue sont-ils configurés pour démarrer ?"""
    try:
        result = subprocess.run(
            [GAMEPAD_HELPER, "status"], capture_output=True, text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def services_helpers_available() -> bool:
    """Les helpers du dialogue « Services et pilotes » sont-ils installés ?"""
    return all(
        Path(path).is_file()
        for path in (SERVICE_HELPER, LOW_LATENCY_HELPER, BROADCOM_HELPER,
                     ATUIN_HELPER)
    )


def clean_available() -> bool:
    """Le helper de nettoyage système est-il installé (image à jour) ?"""
    return Path(CLEAN_HELPER).is_file()


# Domaines de nettoyage, dans l'ordre affiché. Les caches régénérables
# d'abord (petits gestes fréquents), les grands outils ensuite — Podman,
# Flatpaks et Homebrew ferment la liste, leur nettoyage étant plus lourd.
CLEAN_DOMAINS = (
    "gwine_temp", "gwine_shaders", "wgp", "lgp", "opencode",
    "podman", "flatpak", "brew",
)

# Caches purement régénérables : composants et shaders gwine, amorçage des
# extras WGP/LGP, paquets npm des plugins OpenCode. Jamais les sauvegardes
# (~/.local/share/lgp-saves, ~/.local/share/opencode) ni la configuration.
CLEAN_CACHE_SUFFIXES = {
    "gwine_temp": ".cache/gwine/temp",
    "gwine_shaders": ".cache/gwine/shader-cache",
    "wgp": ".cache/wgp-extra",
    "lgp": ".cache/lgp-extra",
    "opencode": ".cache/opencode/npm",
}


def clean_cache_path(domain: str) -> Path | None:
    """Dossier de cache régénérable d'un domaine (None si ce n'est pas un cache)."""
    suffix = CLEAN_CACHE_SUFFIXES.get(domain)
    return Path.home() / suffix if suffix else None


def format_size(value: int | None) -> str:
    """Octets → texte lisible (« 3.4 Gio ») ; "" si la taille est inconnue."""
    if value is None or value <= 0:
        return ""
    size = float(value)
    for unit in ("o", "Kio", "Mio"):
        if size < 1024:
            return f"{size:.0f} {unit}"
        size /= 1024
    return f"{size:.1f} Gio"


def parse_podman_reclaimable(text: str) -> int | None:
    """`podman system df --format json` → octets récupérables (somme des types)."""
    try:
        data = json.loads(text)
    except ValueError:
        return None
    if not isinstance(data, list):
        return None
    total = 0
    found = False
    for item in data:
        if not isinstance(item, dict):
            continue
        try:
            total += int(item.get("RawReclaimable"))
        except (TypeError, ValueError):
            continue
        found = True
    return total if found else None


def podman_reclaimable() -> int | None:
    """Espace que `podman system prune -a --volumes` peut rendre."""
    code, text = _run(["podman", "system", "df", "--format", "json"], 30)
    if code != 0:
        return None
    return parse_podman_reclaimable(text)


def cache_size(domain: str) -> int | None:
    """Taille apparente d'un cache régénérable (octets), None si absent.

    `du -sb` compte les blocs réellement occupés d'après les tailles de
    fichiers : c'est l'ordre de grandeur du gain, pas une promesse à l'octet.
    """
    path = clean_cache_path(domain)
    if path is None or not path.exists():
        return None
    code, text = _run(["du", "-sb", str(path)], 120)
    if code != 0:
        return None
    first = (text.split() or [""])[0]
    try:
        return int(first)
    except (TypeError, ValueError):
        return None


def clean_domain_available(domain: str) -> bool:
    """Le domaine peut-il être nettoyé sur cette machine ?"""
    if domain == "podman":
        return bool(_which("podman"))          # variantes DX uniquement
    if domain == "flatpak":
        return bool(_which("flatpak")) or clean_available()
    if domain == "brew":
        return brew_available()
    path = clean_cache_path(domain)
    if path is not None:
        return path.exists()
    return False


def clean_system_script(
    domains: list[str] | tuple[str, ...] | None = None,
) -> str:
    """Nettoyage des restes inutilisés, pour les domaines demandés.

    Sans argument : tous les domaines disponibles (comportement de la recette
    `ujust clean-system`). Podman et Homebrew tournent en utilisateur : leurs
    images, volumes et cache sont les siens. Les Flatpaks du **compte** aussi ;
    les Flatpaks **système** passent par le helper privilégié (sous-commande
    exacte du sudoers), magasin root oblige.
    """
    selected = set(domains) if domains is not None else set(CLEAN_DOMAINS)
    steps: list[tuple[str, list[str]]] = []
    if "podman" in selected and _which("podman"):
        # `system prune -a --volumes` : images, conteneurs arrêtés, réseaux,
        # cache de build ET volumes non utilisés — le geste complet demandé.
        steps.append(("Podman", ["podman system prune -a --volumes -f"]))
    if "flatpak" in selected:
        commands: list[str] = []
        if _which("flatpak"):
            commands.append("flatpak uninstall --unused --user -y || true")
        if clean_available():
            commands.append(f"sudo -n {CLEAN_HELPER} flatpak")
        if commands:
            steps.append(("Flatpaks", commands))
    for domain, label in (
        ("gwine_temp", "Cache gwine — temp"),
        ("gwine_shaders", "Cache gwine — shaders"),
        ("wgp", "Cache WGP"),
        ("lgp", "Cache LGP"),
        ("opencode", "Cache OpenCode"),
    ):
        if domain not in selected:
            continue
        path = clean_cache_path(domain)
        if path is not None and path.exists():
            # `rm -rf` sur un chemin figé (jamais une variable de shell) :
            # le cache est régénéré au prochain lancement de l'outil.
            steps.append((label, [f"rm -rf -- {shlex.quote(str(path))}"]))
    if "brew" in selected and brew_available():
        steps.append(("Homebrew", list(BREW_CLEAN_COMMANDS)))
    return _steps_script(steps)


# Même préfixe et mêmes exports que `brew_update_script` : le PATH d'une
# session GUI ne contient pas `/home/linuxbrew/.linuxbrew/bin`. Partagé par le
# nettoyage local et distant (le poste distant a la même disposition).
BREW_CLEAN_COMMANDS = (
    "brew=/home/linuxbrew/.linuxbrew/bin/brew",
    'if [ ! -x "$brew" ]; then brew="$(command -v brew)"; fi',
    'if [ -n "$brew" ]; then',
    '    prefix="${brew%/bin/brew}"',
    '    export HOMEBREW_PREFIX="$prefix"',
    '    export HOMEBREW_CELLAR="$prefix/Cellar"',
    '    export HOMEBREW_REPOSITORY="$prefix/Homebrew"',
    '    export HOMEBREW_NO_AUTO_UPDATE=1',
    '    "$brew" autoremove || true',
    '    "$brew" cleanup || true',
    'fi',
)


# -- Nettoyage à distance ----------------------------------------------------

# Sonde unique (une seule connexion SSH) : présence des outils, caches et
# tailles, `podman system df` compacté sur une ligne pour être analysable.
REMOTE_CLEAN_PROBE = "\n".join((
    'for d in .cache/gwine/temp .cache/gwine/shader-cache .cache/wgp-extra '
    '.cache/lgp-extra .cache/opencode/npm; do',
    '    [ -d "$HOME/$d" ] && printf "DIR %s %s\\n" "$d" '
    '"$(du -sb "$HOME/$d" 2>/dev/null | cut -f1)"',
    'done',
    'if command -v podman >/dev/null 2>&1; then',
    '    echo PODMAN',
    '    printf "PODMAN_DF=%s\\n" '
    '"$(podman system df --format json 2>/dev/null | tr -d "\\n")"',
    'fi',
    'command -v flatpak >/dev/null 2>&1 && echo FLATPAK',
    '[ -f /usr/libexec/gablue-clean ] && echo CLEAN_HELPER',
    'if [ -x /home/linuxbrew/.linuxbrew/bin/brew ] '
    '|| command -v brew >/dev/null 2>&1; then echo BREW; fi',
)) + "\n"


def parse_remote_clean_state(text: str) -> dict:
    """Sortie de `REMOTE_CLEAN_PROBE` → disponibilités et tailles par domaine.

    « ? » (taille inconnue) vient de l'absence de mesure : Flatpaks et Homebrew
    ne savent pas la donner, exactement comme en local.
    """
    suffixes = {suffix: domain for domain, suffix in CLEAN_CACHE_SUFFIXES.items()}
    # Un Flatpak système peut exister sans commande `flatpak` côté utilisateur.
    available = {domain: False for domain in CLEAN_DOMAINS}
    sizes: dict[str, int | None] = {}
    podman_df = ""
    for line in (text or "").splitlines():
        line = line.strip()
        if line == "PODMAN":
            available["podman"] = True
        elif line == "FLATPAK":
            available["flatpak"] = True
        elif line == "CLEAN_HELPER":
            available["flatpak"] = True
            available["clean_helper"] = True
        elif line == "BREW":
            available["brew"] = True
        elif line.startswith("PODMAN_DF="):
            podman_df = line.split("=", 1)[1]
        elif line.startswith("DIR "):
            parts = line.split(" ", 2)
            if len(parts) != 3:
                continue
            domain = suffixes.get(parts[1])
            if domain is None:
                continue
            available[domain] = True
            try:
                sizes[domain] = int(parts[2])
            except ValueError:
                sizes[domain] = None
    if available["podman"]:
        sizes["podman"] = parse_podman_reclaimable(podman_df) if podman_df else None
    return {"available": available, "sizes": sizes}


def remote_clean_script(domain: str, available: dict) -> str:
    """Commandes de nettoyage d'UN domaine, exécutées sur le poste distant.

    Mêmes gestes qu'en local, mais sans rien supposer de l'image locale : la
    présence des outils et du helper vient de la sonde du poste.
    """
    if domain == "podman":
        return "podman system prune -a --volumes -f || true\n"
    if domain == "flatpak":
        commands: list[str] = []
        if available.get("flatpak"):
            commands.append("flatpak uninstall --unused --user -y || true")
        if available.get("clean_helper"):
            commands.append(f"sudo -n {CLEAN_HELPER} flatpak")
        return "\n".join(commands) + ("\n" if commands else "")
    if domain == "brew":
        return "\n".join(BREW_CLEAN_COMMANDS) + "\n"
    suffix = CLEAN_CACHE_SUFFIXES.get(domain)
    if suffix:
        return f'rm -rf -- "$HOME/{suffix}"\n'
    return ""


# -- Btrfs : compression par système de fichiers ----------------------------

# Montages techniques : /etc et /sysroot ne sont pas des cibles utiles (ils
# doublonnent le système), et /run/bees/* appartient au démon de déduplication.
BTRFS_EXCLUDED_TARGETS = ("/etc",)
BTRFS_EXCLUDED_PREFIXES = ("/sysroot", "/run/bees")


def parse_btrfs_mounts(text: str) -> list[dict]:
    """`findmnt -t btrfs -o TARGET,UUID,LABEL -J` → montages affichables.

    Un point de montage = une ligne : sur ostree, `/var` et `/var/home` sont
    deux sous-volumes distincts du même disque, avec chacun son drapeau de
    compression — les fusionner par UUID afficherait un état faux.
    """
    try:
        data = json.loads(text)
    except ValueError:
        return []
    filesystems = data.get("filesystems") if isinstance(data, dict) else None
    if not isinstance(filesystems, list):
        return []
    rows: list[dict] = []
    seen: set[str] = set()
    # `findmnt -J` imbrique les montages enfants (`/var/home` sous `/var`) :
    # les ignorer ferait disparaître un sous-volume de la liste.
    pending = list(filesystems)
    while pending:
        item = pending.pop(0)
        if not isinstance(item, dict):
            continue
        pending.extend(item.get("children") or [])
        target = str(item.get("target") or "")
        if not target or target in seen:
            continue
        if target in BTRFS_EXCLUDED_TARGETS:
            continue
        if target.startswith(BTRFS_EXCLUDED_PREFIXES):
            continue
        seen.add(target)
        rows.append({
            "target": target,
            "uuid": str(item.get("uuid") or ""),
            "label": str(item.get("label") or ""),
        })
    return sorted(rows, key=lambda row: row["target"])


def btrfs_compression(path: str) -> str:
    """Drapeau de compression d'un chemin (`zstd`, `lzo`… ; "" = aucun)."""
    try:
        result = subprocess.run(
            [_which("btrfs"), "property", "get", path, "compression"],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    for line in (result.stdout or "").splitlines():
        if line.startswith("compression="):
            return line.split("=", 1)[1].strip()
    return ""


def btrfs_filesystems() -> list[dict]:
    """Montages Btrfs présents, avec l'état de compression de chacun."""
    try:
        result = subprocess.run(
            ["findmnt", "-t", "btrfs", "-o", "TARGET,UUID,LABEL", "-J"],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    if result.returncode != 0:
        return []
    rows = parse_btrfs_mounts(result.stdout)
    for row in rows:
        row["compression"] = btrfs_compression(row["target"])
    return rows


def btrfs_helper_available() -> bool:
    """Le helper de compression Btrfs est-il installé (image à jour) ?"""
    return Path(BTRFS_HELPER).is_file()


def btrfs_apply_script(changes: dict[str, bool]) -> str:
    """Un `btrfs property set` par montage modifié (helper privilégié)."""
    steps: list[tuple[str, list[str]]] = []
    for target, enabled in changes.items():
        action = "zstd" if enabled else "none"
        steps.append((f"Compression {target}", [
            f"sudo -n {BTRFS_HELPER} {action} {shlex.quote(target)}"
        ]))
    return _steps_script(steps)


# -- NVMe : limite thermique (HCTM) -----------------------------------------

# Presets proposés par la recette ujust, du plus prudent au plus permissif :
# (TMT1 « throttle doux », TMT2 « throttle agressif ») en °C.
NVME_PRESETS = ((65, 70), (70, 75), (75, 80))


def nvme_thermal_helper_available() -> bool:
    """Le helper de limite thermique NVMe est-il installé ?"""
    return Path(NVME_THERMAL_HELPER).is_file()


def parse_nvme_thermal_devices(text: str) -> list[dict]:
    """JSON de `gablue-nvme-thermal status` → cartes de contrôleurs."""
    try:
        data = json.loads(text)
    except (TypeError, ValueError):
        return []
    devices = data.get("devices") if isinstance(data, dict) else None
    if not isinstance(devices, list):
        return []
    return [item for item in devices if isinstance(item, dict)]


def nvme_thermal_devices() -> list[dict]:
    """État HCTM de chaque contrôleur NVMe (lu par le helper privilégié).

    La lecture (`id-ctrl`, `get-feature`) exige root sur nvme-cli : le helper
    la fait en une passe et rend un JSON stable, que le dialogue consomme.
    """
    try:
        result = subprocess.run(
            ["sudo", "-n", NVME_THERMAL_HELPER, "status"],
            capture_output=True, text=True, timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    if result.returncode != 0:
        return []
    return parse_nvme_thermal_devices(result.stdout)


# -- Maintenance à distance : sondes et commandes SSH -----------------------

# Une seule connexion par outil. Le helper local n'existe pas forcément sur
# le poste (image antérieure) : la sonde le dit, le dialogue grise alors ce
# qui ne peut pas être appliqué.

REMOTE_NVME_THERMAL_PROBE = "\n".join((
    'if [ -x /usr/libexec/gablue-nvme-thermal ]; then',
    '    echo HELPER',
    '    sudo -n /usr/libexec/gablue-nvme-thermal status 2>/dev/null '
    '|| echo UNREADABLE',
    'else',
    '    echo NO_HELPER',
    'fi',
)) + "\n"


def parse_remote_nvme_thermal_state(text: str) -> dict:
    """Sortie de `REMOTE_NVME_THERMAL_PROBE` → `{"ok", "devices"}`."""
    markers = {"HELPER", "NO_HELPER", "UNREADABLE"}
    lines = (text or "").splitlines()
    helper = any(line.strip() == "HELPER" for line in lines)
    payload = "\n".join(
        line for line in lines if line.strip() not in markers
    )
    devices = parse_nvme_thermal_devices(payload)
    return {"ok": helper and bool(devices), "devices": devices}


REMOTE_BTRFS_PROBE = "\n".join((
    'if [ -x /usr/libexec/gablue-btrfs-compress ]; then',
    '    echo HELPER',
    'else',
    '    echo NO_HELPER',
    'fi',
    'findmnt -t btrfs -o TARGET,UUID,LABEL -J 2>/dev/null',
    'findmnt -t btrfs -o TARGET -n -r 2>/dev/null | while IFS= read -r t; do',
    '    c=$(btrfs property get "$t" compression 2>/dev/null '
    "| sed -n 's/^compression=//p')",
    '    printf "COMP\\t%s\\t%s\\n" "$t" "$c"',
    'done',
)) + "\n"


def parse_btrfs_remote_state(text: str) -> dict:
    """Sortie de `REMOTE_BTRFS_PROBE` → `{"ok", "filesystems"}`.

    Le JSON de `findmnt` occupe plusieurs lignes et précède les lignes
    `COMP` (un point de montage par ligne, compression lue sans privilège).
    """
    helper = False
    compressions: dict[str, str] = {}
    json_lines: list[str] = []
    for line in (text or "").splitlines():
        stripped = line.strip()
        if stripped == "HELPER":
            helper = True
            continue
        if stripped == "NO_HELPER":
            continue
        if line.startswith("COMP\t"):
            parts = line.split("\t")
            if len(parts) >= 2:
                compressions[parts[1]] = parts[2] if len(parts) > 2 else ""
            continue
        json_lines.append(line)
    filesystems = parse_btrfs_mounts("\n".join(json_lines))
    for row in filesystems:
        row["compression"] = compressions.get(row["target"], "")
    return {"ok": helper, "filesystems": filesystems}


def btrfs_remote_script(changes: dict[str, bool]) -> str:
    """Un `btrfs property set` par montage, sans marqueurs de progression.

    `RemoteTaskDialog` n'accepte pas `progress_parser` : les étapes
    `::gablue:` du script local ne seraient pas interprétées, on écrit donc
    les commandes nues.
    """
    lines: list[str] = []
    for target, enabled in changes.items():
        action = "zstd" if enabled else "none"
        lines.append(
            f"sudo -n {BTRFS_HELPER} {action} {shlex.quote(target)}"
        )
    return "\n".join(lines) + ("\n" if lines else "")


def nvme_thermal_remote_script(
    changes: dict[str, tuple[int, int] | None],
) -> str:
    """`set-feature` (ou remise au firmware) par contrôleur, commandes nues."""
    lines: list[str] = []
    for device, thresholds in changes.items():
        if thresholds is None:
            lines.append(
                f"sudo -n {NVME_THERMAL_HELPER} off {shlex.quote(device)}"
            )
            continue
        first, second = thresholds
        lines.append(
            f"sudo -n {NVME_THERMAL_HELPER} set {shlex.quote(device)} "
            f"{int(first)} {int(second)}"
        )
    return "\n".join(lines) + ("\n" if lines else "")


def nvme_thermal_script(
    changes: dict[str, tuple[int, int] | None],
) -> str:
    """Un `set-feature` (ou une remise au firmware) par SSD modifié."""
    steps: list[tuple[str, list[str]]] = []
    for device, thresholds in changes.items():
        if thresholds is None:
            steps.append((f"{device} : contrôle firmware", [
                f"sudo -n {NVME_THERMAL_HELPER} off {shlex.quote(device)}"
            ]))
            continue
        first, second = thresholds
        steps.append((f"{device} : {first}/{second} °C", [
            f"sudo -n {NVME_THERMAL_HELPER} set {shlex.quote(device)} "
            f"{int(first)} {int(second)}"
        ]))
    return _steps_script(steps)


# -- Ryzen : limite de température Tctl (ryzenadj -f) -----------------------

# Bornes du helper : sous 45 °C le CPU throttle presque en permanence,
# au-dessus de 110 °C la limite ne protège plus rien. `limitw --temp` accepte
# 50…105 ; le helper élargit d'un cran utile côté froid.
RYZENADJ_MIN_TEMP, RYZENADJ_MAX_TEMP = 45, 110


def ryzen_cpu() -> bool:
    """Le CPU est-il un AMD Ryzen (seul cas où ryzenadj agit) ?"""
    try:
        info = Path("/proc/cpuinfo").read_text(
            encoding="utf-8", errors="replace"
        )
    except OSError:
        return False
    return "AuthenticAMD" in info and "Ryzen" in info


def ryzenadj_available() -> bool:
    """Un CPU Ryzen et l'outil ryzenadj : la limite a un sens sur ce poste."""
    return Path(RYZENADJ_BINARY).is_file() and ryzen_cpu()


def ryzenadj_helper_available() -> bool:
    """Le helper de limite Tctl est-il installé par l'image ?"""
    return Path(RYZENADJ_HELPER).is_file()


def parse_ryzenadj_status(text: str) -> dict:
    """Sortie JSON du helper → état stable ; {} si illisible.

    Le helper est la seule source : la limite vit dans
    `/etc/gablue/ryzenadj.conf` (lisible par root seulement en pratique) et
    l'unité `gablue-ryzenadj.service` la réapplique à chaque démarrage.
    """
    try:
        data = json.loads(text)
    except (TypeError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    if not isinstance(data.get("supported"), bool):
        return {}
    limit = data.get("limit")
    if limit is not None:
        try:
            limit = int(limit)
        except (TypeError, ValueError):
            return {}
        if not RYZENADJ_MIN_TEMP <= limit <= RYZENADJ_MAX_TEMP:
            return {}
    return {
        "supported": data["supported"],
        "limit": limit,
        "service_enabled": bool(data.get("service_enabled")),
        "service_active": bool(data.get("service_active")),
    }


def ryzenadj_status() -> dict:
    """État lu en `sudo -n` (aucune invite) ; {} si le helper échoue."""
    code, text = _run(["sudo", "-n", RYZENADJ_HELPER, "status"], 15)
    if code != 0:
        return {}
    return parse_ryzenadj_status(text)


def ryzenadj_script(limit: int | None) -> str:
    """Un seul geste : appliquer la limite (et activer) ou la retirer."""
    if limit is None:
        return _steps_script([(
            "Retirer la limite Tctl",
            [f"sudo -n {RYZENADJ_HELPER} off"],
        )])
    value = max(RYZENADJ_MIN_TEMP, min(int(limit), RYZENADJ_MAX_TEMP))
    return _steps_script([(
        f"Limiter Tctl à {value} °C",
        [f"sudo -n {RYZENADJ_HELPER} set {value}"],
    )])


# -- Syncthing : dossiers de versions (.stversions) --------------------------

# Emplacements connus du config.xml de Syncthing, y compris le bac à sable
# Flatpak (SyncThingy range son état sous ~/.var/app/<id>/.local/state).
SYNCTHING_CONFIG_GLOBS = (
    ".local/state/syncthing/config.xml",
    ".config/syncthing/config.xml",
    ".var/app/*/.local/state/syncthing/config.xml",
    ".var/app/*/config/syncthing/config.xml",
    ".var/app/*/data/syncthing/config.xml",
)


def syncthing_running() -> bool:
    """Le démon Syncthing tourne-t-il (Flatpak compris) ?"""
    code, _ = _run(["pgrep", "-x", "syncthing"], 5)
    return code == 0


def syncthing_config_paths(home: Path | None = None) -> list[Path]:
    """Config.xml de Syncthing présents sur la machine (dédoublonnés)."""
    root = home or Path.home()
    paths: list[Path] = []
    seen: set[Path] = set()
    for pattern in SYNCTHING_CONFIG_GLOBS:
        for path in root.glob(pattern):
            if path.is_file() and path not in seen:
                seen.add(path)
                paths.append(path)
    return paths


def parse_syncthing_folders(text: str, home: Path | None = None) -> list[dict]:
    """`config.xml` → dossiers de versions (`.stversions`) déclarés.

    Syncthing crée `.stversions` dans le dossier synchronisé, sauf si
    `versionsPath` est renseigné : ce chemin-là fait foi. Les `~` sont
    développés avec le vrai home (le Flatpak voit le même /home).
    """
    import xml.etree.ElementTree as ElementTree

    try:
        root = ElementTree.fromstring(text)
    except ElementTree.ParseError:
        return []
    home_path = home or Path.home()
    rows: list[dict] = []
    seen: set[str] = set()
    for folder in root.iter("folder"):
        folder_path = (folder.get("path") or "").strip()
        if not folder_path:
            continue
        versions = ""
        for child in folder:
            if child.tag == "versionsPath":
                versions = (child.text or "").strip()
                break
        def expand(value: str) -> str:
            # Le home passé fait foi (`~` du config.xml) ; sinon ~ de l'utilisateur.
            if value == "~" or value.startswith("~/"):
                return str(home_path / value[2:]) if value != "~" else str(home_path)
            return os.path.expanduser(value)

        folder_path = expand(folder_path)
        versions = expand(versions) if versions else os.path.join(
            folder_path, ".stversions"
        )
        if versions in seen:
            continue
        seen.add(versions)
        rows.append({
            "label": (folder.get("label") or "").strip(),
            "path": folder_path,
            "versions": versions,
        })
    return rows


def syncthing_version_dirs(home: Path | None = None) -> list[dict]:
    """Dossiers `.stversions` existants, avec leur libellé de dossier.

    Config illisible ou absente : repli sur `~/Syncthing/*/.stversions`, la
    disposition créée par l'assistant Syncthing.
    """
    home_path = home or Path.home()
    rows: list[dict] = []
    for config in syncthing_config_paths(home_path):
        try:
            text = config.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        rows.extend(parse_syncthing_folders(text, home_path))
    if not rows:
        for path in sorted(home_path.glob("Syncthing/*/.stversions")):
            if path.is_dir():
                rows.append({
                    "label": path.parent.name,
                    "path": str(path.parent),
                    "versions": str(path),
                })
    existing = []
    seen: set[str] = set()
    for row in rows:
        if row["versions"] in seen or not Path(row["versions"]).is_dir():
            continue
        seen.add(row["versions"])
        existing.append(row)
    return existing


def versions_dir_size(path: str) -> int | None:
    """Taille apparente d'un dossier `.stversions` (octets), None si absente."""
    if not Path(path).is_dir():
        return None
    code, text = _run(["du", "-sb", path], 180)
    if code != 0:
        return None
    first = (text.split() or [""])[0]
    try:
        return int(first)
    except (TypeError, ValueError):
        return None


# -- GRUB : visibilité du menu (menu_auto_hide) ------------------------------

def grub_helper_available() -> bool:
    """Le helper du menu GRUB est-il installé (image à jour) ?"""
    return Path(GRUB_HELPER).is_file()


def grub_menu_state() -> str:
    """`menu_auto_hide` courant : « unset », « 0 », « 1 », « 2 » ou "" (illisible).

    `grubenv` est en 0700 root : la lecture passe par le helper en `sudo -n`
    (jamais d'invite de mot de passe, comme le reste de Gablue Manager).
    """
    code, text = _run(["sudo", "-n", GRUB_HELPER, "status"], 15)
    if code != 0:
        return ""
    value = (text.strip().splitlines() or [""])[0].strip()
    return value if value in ("unset", "0", "1", "2") else ""


def grub_apply_script(value: str) -> str:
    """Script d'application (une étape) pour la valeur choisie."""
    if value == "unset":
        command = f"sudo -n {GRUB_HELPER} unset"
    else:
        command = f"sudo -n {GRUB_HELPER} set {value}"
    return _steps_script([("Menu GRUB", [command])])


# -- Secure Boot : état et enrôlement des clés MOK ---------------------------

SECURE_BOOT_STATES = ("enabled", "disabled", "unsupported", "unknown")
SECURE_BOOT_KEY_STATES = (
    "missing", "enrolled", "pending", "not-enrolled", "blocked", "unknown",
)


def secure_boot_state() -> str:
    """État Secure Boot : « enabled », « disabled », « unsupported », « unknown ».

    `mokutil --sb-state` ne demande aucun privilège : la carte « Ce poste »
    peut l'afficher au démarrage, sans helper ni sudo.
    """
    if not Path("/sys/firmware/efi").is_dir():
        return "unsupported"
    code, text = _run(["mokutil", "--sb-state"], 15)
    if code == 127:
        return "unknown"
    return parse_secure_boot(text, True)


def secure_boot_label(state: str) -> str:
    """Libellé court pour la carte « Ce poste » ("" si l'état est inconnu)."""
    return {
        "enabled": "Secure Boot activé",
        "disabled": "Secure Boot désactivé",
        "unsupported": "démarrage BIOS",
    }.get(state, "")


def parse_secureboot_status(text: str) -> dict:
    """JSON du helper → dict validé ({} si illisible ou incomplet)."""
    try:
        data = json.loads(text)
    except (TypeError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    secure_boot = str(data.get("secure_boot") or "")
    if secure_boot not in SECURE_BOOT_STATES:
        return {}
    keys: list[dict] = []
    for item in data.get("keys") or []:
        if not isinstance(item, dict):
            continue
        path = str(item.get("path") or "")
        state = str(item.get("state") or "")
        if not path or state not in SECURE_BOOT_KEY_STATES:
            continue
        keys.append({
            "path": path,
            "label": str(item.get("label") or path),
            "state": state,
        })
    if not keys:
        return {}
    return {
        "efi": bool(data.get("efi")),
        "secure_boot": secure_boot,
        "password": str(data.get("password") or ""),
        "keys": keys,
    }


def secureboot_helper_available() -> bool:
    """Le helper d'enrôlement MOK est-il installé (image à jour) ?"""
    return Path(SECUREBOOT_HELPER).is_file()


def secureboot_status() -> dict:
    """État lu par le helper en `sudo -n` ; {} s'il est absent ou illisible.

    Les certificats vivent dans `/etc/pki/akmods/certs` (0700 root) : même la
    lecture passe par root, jamais par une invite de mot de passe.
    """
    if not secureboot_helper_available():
        return {}
    code, text = _run(["sudo", "-n", SECUREBOOT_HELPER, "status"], 90)
    if code != 0:
        return {}
    return parse_secureboot_status(text)


def secureboot_enroll_script() -> str:
    """Script d'enrôlement (une étape) : la demande prend effet au reboot."""
    return _steps_script(
        [("Secure Boot", [f"sudo -n {SECUREBOOT_HELPER} enroll"])]
    )


# -- Démarrage unique sur un autre système (BootNext) -----------------------

def parse_boot_options(text: str) -> list[dict]:
    """Sortie du helper `list` → options {id, label, detail}.

    Une option par ligne `id<TAB>libellé<TAB>détail` ; l'identifiant est un
    numéro EFI (4 hexadécimaux) ou `esp:/dev/sdX1`. Toute autre forme est
    écartée : le dialogue ne doit jamais envoyer au helper une valeur qu'il
    n'a pas lui-même proposée.
    """
    options: list[dict] = []
    seen: set[str] = set()
    for raw in (text or "").splitlines():
        parts = raw.split("\t")
        if len(parts) < 2:
            continue
        ident = parts[0].strip()
        label = parts[1].strip()
        detail = parts[2].strip() if len(parts) > 2 else ""
        if not ident or not label or ident in seen:
            continue
        if not re.fullmatch(r"[0-9A-Fa-f]{4}|esp:/dev/[A-Za-z0-9]+", ident):
            continue
        seen.add(ident)
        options.append({"id": ident, "label": label, "detail": detail})
    return options


def bootselect_helper_available() -> bool:
    """Le helper de démarrage ponctuel est-il installé (image à jour) ?"""
    return Path(BOOTSELECT_HELPER).is_file()


def boot_options() -> dict:
    """{state, options} — autres disques amorçables, système principal exclu.

    Le helper lit `efibootmgr`, résout les disques et monte les ESP en
    lecture seule : tout passe par `sudo -n` (règle sudoers exacte), jamais
    par une invite de mot de passe. `state` : « ok », « empty » (aucun autre
    disque), « unsupported » (poste non UEFI), « absent » (image antérieure)
    ou « error » (lecture impossible).
    """
    if not Path("/sys/firmware/efi").is_dir():
        return {"state": "unsupported", "options": []}
    if not bootselect_helper_available():
        return {"state": "absent", "options": []}
    code, text = _run(["sudo", "-n", BOOTSELECT_HELPER, "list"], 45)
    if code != 0:
        return {"state": "error", "options": []}
    options = parse_boot_options(text)
    return {"state": "ok" if options else "empty", "options": options}


def boot_select_script(ident: str) -> str:
    """Script d'une étape : arme BootNext puis redémarre (le helper fait tout)."""
    return _steps_script([(
        "Démarrage unique",
        [f"sudo -n {BOOTSELECT_HELPER} boot {shlex.quote(ident)}"],
    )])


def rebase_helper_available() -> bool:
    """Le helper de bascule de variante est-il installé ?"""
    return Path(REBASE_HELPER).is_file()


def services_choices() -> dict:
    """État des bascules : services, couche Vulkan, Wi-Fi, atuin."""
    return {
        "displaylink": service_enabled("displaylink.service"),
        "docker": service_enabled("docker.service"),
        "low_latency": LOW_LATENCY_FILE.is_file(),
        "broadcom_wl": BROADCOM_WL_FILE.is_file(),
        "atuin": atuin_active(),
        "gamepad": gamepad_active() if gamepad_available() else False,
    }


def system_apply_script(changes: dict[str, bool]) -> str:
    """Script d'application : un helper par domaine, en un seul passage.

    Un seul « Appliquer » peut toucher six domaines ; `_steps_script` publie
    leur avancement pour la barre du dialogue.
    """
    steps: list[tuple[str, list[str]]] = []
    for key, unit, label in (
        ("displaylink", "displaylink.service", "DisplayLink"),
        ("docker", "docker.service", "Docker"),
    ):
        if key in changes:
            steps.append((label, [
                f"sudo -n {SERVICE_HELPER} {unit} "
                + ("on" if changes[key] else "off")
            ]))
    if "low_latency" in changes:
        steps.append(("Vulkan low-latency", [
            f"sudo -n {LOW_LATENCY_HELPER} "
            + ("on" if changes["low_latency"] else "off")
        ]))
    if "broadcom_wl" in changes:
        steps.append(("Pilote Wi-Fi Broadcom", [
            f"sudo -n {BROADCOM_HELPER} "
            + ("on" if changes["broadcom_wl"] else "off")
        ]))
    if "atuin" in changes:
        # Intégration du shell : fichiers de l'utilisateur, aucun privilège.
        steps.append(("Historique atuin", [
            f"{ATUIN_HELPER} " + ("on" if changes["atuin"] else "off")
        ]))
    if "gamepad" in changes:
        # Autostart de session : fichiers de l'utilisateur, aucun privilège.
        steps.append(("Raccourcis manette", [
            f"{GAMEPAD_HELPER} " + ("on" if changes["gamepad"] else "off")
        ]))
    return _steps_script(steps)


def rebase_script(variant: str) -> str:
    """Bascule vers une autre variante d'image (redémarrage ensuite)."""
    return f"sudo -n {REBASE_HELPER} {variant}\n"

# Tâches de bootc (champ machine `task` de la progression JSONL) → libellé.
PROGRESS_TASKS = {
    "pulling": "Téléchargement de l'image",
    "ostree_chunk": "Téléchargement des objets OSTree",
    "ostree_derived": "Construction de la couche dérivée",
    "importing": "Import de l'image",
    "staging": "Préparation du déploiement",
    "composing": "Écriture du déploiement",
}

def parse_progress_event(line: str) -> dict:
    """Événement JSONL de `bootc upgrade --progress-fd` → progression lisible.

    Chaque ligne est un objet `{type: ProgressBytes|ProgressSteps, …}` (voir
    `progress_jsonl.rs` de bootc). `{}` pour tout le reste (message Start, JSON
    tronqué, ligne vide). Le pourcentage prend les octets quand le total est
    connu (images), sinon les étapes (couches), sinon None → barre indéterminée.

    Seuls `bytes` et `steps` comptent : `bytesCached`/`stepsCached` sont le
    **déjà présent cumulé**, pas un avancement de l'opération en cours. Les
    additionner donnait « 4,0 Gio / 772 Mio » et une barre à 100 % dès le
    premier événement (constaté le 08/11/2026, bootc 1.17.1, backend ostree :
    `bytesCached` 4 296 727 642 pour `bytesTotal` 772 729 517, et 118 étapes
    en cache pour 10 à faire).
    """
    try:
        event = json.loads(line)
    except (TypeError, ValueError):
        return {}
    if not isinstance(event, dict) or event.get("type") not in (
        "ProgressBytes", "ProgressSteps",
    ):
        return {}

    def number(key: str) -> int:
        try:
            return int(event.get(key) or 0)
        except (TypeError, ValueError):
            return 0

    total = number("bytesTotal")
    done = number("bytes")
    steps_total = number("stepsTotal")
    steps = number("steps")
    percent: float | None = None
    if total > 0:
        percent = 100.0 * done / total
    elif steps_total > 0:
        percent = 100.0 * steps / steps_total
    task = str(event.get("task") or "")
    return {
        "label": PROGRESS_TASKS.get(task) or str(event.get("description") or ""),
        "percent": None if percent is None else max(0.0, min(100.0, percent)),
        "bytes": done,
        "bytes_total": total,
        "steps": steps,
        "steps_total": steps_total,
    }

# Protocole de progression des outils externes (gwine) vers le dialogue de
# tâche. Une ligne = un événement, sur la sortie fusionnée de la commande :
#   ::gablue:step 4 12 Installation de Wine Mono et Gecko
#   ::gablue:busy Téléchargement des composants Windows
#   ::gablue:done Initialisation terminée
# Les lignes de protocole sont retirées du journal (`hide`) : l'outil écrit sa
# version lisible juste à côté, le journal reste donc du texte pour l'humain.
GABLUE_PROGRESS_PREFIX = "::gablue:"

def parse_gablue_progress(line: str) -> dict:
    """Ligne de protocole `::gablue:` → progression du dialogue.

    `step n total libellé` : une étape commence ; n-1 étapes sont derrière, donc
    la barre affiche `(n-1)/total` — elle n'atteint 100 % qu'au `done` final.
    `busy libellé` : travail sans total (téléchargement) → barre indéterminée.
    `done [libellé]` : 100 %. `{}` pour toute autre ligne (journal normal).
    """
    text = (line or "").rstrip()
    if not text.startswith(GABLUE_PROGRESS_PREFIX):
        return {}
    verb, _, rest = text[len(GABLUE_PROGRESS_PREFIX):].strip().partition(" ")
    rest = rest.strip()
    if verb == "step":
        parts = rest.split(" ", 2)
        if len(parts) < 3:
            return {}
        try:
            step = int(parts[0])
            total = max(1, int(parts[1]))
        except ValueError:
            return {}
        step = min(max(1, step), total)
        return {
            "hide": True,
            "label": f"Étape {step}/{total} — {parts[2]}",
            "percent": 100.0 * (step - 1) / total,
            "step": step,
            "total": total,
        }
    if verb == "busy":
        return {
            "hide": True,
            "label": rest or "Opération en cours…",
            "percent": None,
        }
    if verb == "done":
        return {
            "hide": True,
            "label": rest or "Terminé.",
            "percent": 100.0,
        }
    return {}
