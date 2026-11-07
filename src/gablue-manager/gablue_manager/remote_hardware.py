"""Résumé matériel d'un poste distant, en **une seule** connexion SSH.

La page d'un poste reprend celle du poste local : un script imprimé par
sections balisées (`::gablue:<nom>`) ramène les fichiers bruts (/proc, DMI,
`df`, `rpm-ostree status`, `mokutil`, Vulkan) et l'analyse locale réutilise
les fonctions de `local` — une seule façon de formatter un processeur, une
barrette ou un disque.

Aucun privilège obligatoire : les barrettes passent par le helper NOPASSWD
`gablue-hwinfo` quand il existe (image récente), sinon la ligne reste vide ;
le reste est lisible par l'utilisateur du SSH.
"""

import time

from . import local, updates
from .remote_updates import MARKER, sections, ssh_capture

# `vulkaninfo` vient avec KInfoCenter (dépendance de Plasma) et donne le nom
# commercial exact du GPU ; `lspci` sert de repli. Tout est borné par timeout :
# un poste lent ne doit pas bloquer la page.
HARDWARE_SCRIPT = r"""
echo '::gablue:os'
. /etc/os-release 2>/dev/null
echo "pretty=$PRETTY_NAME"
echo "home=$HOME"
echo '::gablue:efi'
if [ -d /sys/firmware/efi ]; then echo 'efi=1'; else echo 'efi=0'; fi
mokutil --sb-state 2>/dev/null || true
echo '::gablue:dmi'
for f in sys_vendor product_name board_vendor board_name bios_version bios_date; do
    echo "$f=$(cat /sys/class/dmi/id/$f 2>/dev/null)"
done
echo '::gablue:cpu'
grep -m1 '^model name' /proc/cpuinfo 2>/dev/null || true
awk -F: '/^cpu cores/{gsub(/ /,"",$2); print "cores="$2; exit}' /proc/cpuinfo 2>/dev/null
echo "threads=$(grep -c '^processor' /proc/cpuinfo 2>/dev/null)"
echo '::gablue:mem'
grep '^MemTotal' /proc/meminfo 2>/dev/null || true
echo '::gablue:modules'
sudo -n /usr/libexec/gablue-hwinfo 2>/dev/null || echo '{}'
echo '::gablue:gpu'
lspci -mm -nn -d ::0300 2>/dev/null || true
echo '::gablue:vulkan'
timeout 8 vulkaninfo --summary 2>/dev/null | sed -n 's/^[[:space:]]*deviceName[[:space:]]*=[[:space:]]*//p' | head -2 || true
echo '::gablue:df'
df -P -B1 2>/dev/null | grep '^/dev/' || true
echo '::gablue:kernel'
uname -r 2>/dev/null || true
echo '::gablue:boot'
grep '^btime ' /proc/stat 2>/dev/null || true
echo '::gablue:image'
rpm-ostree status --json 2>/dev/null || echo '{}'
"""


def _key_values(lines: list[str]) -> dict[str, str]:
    """Lignes `clé=valeur` (la première occurrence gagne)."""
    values: dict[str, str] = {}
    for line in lines or []:
        key, sep, value = line.partition("=")
        if not sep:
            continue
        key = key.strip()
        if key and key not in values:
            values[key] = value.strip()
    return values


def _int(value: str) -> int:
    try:
        return int(str(value).strip())
    except (TypeError, ValueError):
        return 0


def parse_hardware(text: str, now: float | None = None) -> dict:
    """Sortie de `HARDWARE_SCRIPT` → valeurs pour `InfoPanel` (mêmes clés)."""
    split = sections(text)
    values: dict[str, str] = {}

    os_info = _key_values(split.get("os", []))
    values["system"] = os_info.get("pretty", "")
    home = os_info.get("home", "") or "/home"

    dmi = _key_values(split.get("dmi", []))
    values.update(local.identity_from_dmi(dmi))

    cpu_lines = split.get("cpu", [])
    model = ""
    cores = 0
    threads = 0
    for line in cpu_lines:
        if line.startswith("model name"):
            model = line.partition(":")[2].strip()
        elif line.startswith("cores="):
            cores = _int(line.partition("=")[2])
        elif line.startswith("threads="):
            threads = _int(line.partition("=")[2])
    values["cpu"] = local.cpu_label_parts(model, cores, threads)

    total = local.memory_total_gib("\n".join(split.get("mem", [])))
    values["memory"] = local.format_gib(total) if total else ""

    modules = local.parse_memory_modules("\n".join(split.get("modules", [])))
    values["modules"] = local.format_memory_modules(modules)

    gpu = local.parse_lspci_display("\n".join(split.get("gpu", [])))
    for line in split.get("vulkan", []):
        name = local.parse_vulkan_name(line)
        if name and not any(
            word in name.casefold()
            for word in ("llvmpipe", "lavapipe", "swiftshader")
        ):
            gpu = name
            break
    values["gpu"] = gpu

    df_text = "\n".join(split.get("df", []))
    disk_rows = local.disk_rows_from_df(df_text, home)
    values["disks"] = local.format_disks(disk_rows, home)
    # La page d'un poste distant affiche les mêmes lignes de disques que le
    # local (sans bouton Filelight, qui ne lit que la machine courante).
    values["disk_rows"] = disk_rows

    kernel = next((line.strip() for line in split.get("kernel", [])
                   if line.strip()), "")
    values["kernel"] = kernel

    # `/proc/stat` écrit `btime <secondes>` (espace, pas `=`) : l'ancien
    # `_key_values` ne trouvait aucun séparateur et laissait « Démarrage »
    # vide sur tous les postes distants (vécu 07/11/2026).
    btime = 0
    for line in split.get("boot", []):
        parts = line.split()
        if len(parts) >= 2 and parts[0] == "btime":
            btime = _int(parts[1])
            break
    if btime > 0:
        values["uptime"] = "depuis " + local.format_uptime(
            (now if now is not None else time.time()) - btime
        )

    deployment = updates.parse_deployment_age(
        "\n".join(split.get("image", [])), now
    )
    values["image"] = updates.image_detail({
        **deployment,
        "state": "available" if deployment.get("staged") else "uptodate",
    })

    efi_text = "\n".join(split.get("efi", []))
    efi = "efi=1" in efi_text
    state = local.parse_secure_boot(efi_text, efi)
    values["secure_boot"] = local.secure_boot_label(state)

    return {key: value for key, value in values.items() if value}


def check(host: dict, timeout: float = 40.0) -> dict:
    """Sonde un poste ; injoignable ou script muet → dictionnaire vide."""
    code, text = ssh_capture(host, HARDWARE_SCRIPT, timeout=timeout)
    if code != 0 or MARKER not in (text or ""):
        return {}
    return parse_hardware(text)
