"""État et commandes de la machine courante (sshd, kargs, gwine,
Tailscale).
"""

import configparser
import json
import re
import shlex
import shutil
import subprocess
from pathlib import Path

from .constants import (
    ATUIN_HELPER, AUTO_UPDATE_HELPER, AUTO_UPDATE_USER_TIMER, AVAHI_HELPER,
    AVAHI_SERVICE_FILE, BREW_BINARY, BROADCOM_HELPER, BROADCOM_WL_FILE,
    CMDLINE, CPUID_EMU_BLACKLIST, CPUID_EMU_CONF, CPUID_EMU_HELPER,
    KARG_BT_ERTM, KARG_CLEARCPUID,
    KARG_HDMI21, KARG_HELPER, KARG_MITIGATIONS_OFF, LOW_LATENCY_FILE,
    LOW_LATENCY_HELPER, OS_RELEASE, REBASE_HELPER, SERVICE_HELPER,
    TAILSCALE_HELPER, _which,
)
from .ssh import ssh_capture

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
    image ostree staged, kargs du prochain démarrage différents, configs noyau
    écrites depuis le démarrage (Wi-Fi Broadcom, émulation CPUID), et module
    d'émulation encore chargé alors que sa persistance a été retirée.
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
    """Mise à jour des Flatpaks : compte, puis système sans mot de passe.

    Sans option, `flatpak update` ne vise que l'installation système (défaut de
    flatpak) et PolicyKit demande le mot de passe ; l'image autorise
    `/usr/bin/flatpak update --system -y` dans `/etc/sudoers.d/nopasswd`
    (arguments exacts, aucun joker), donc `sudo -n` passe sans dialogue. Hors
    image Gablue la règle manque : l'appel direct reprend la main et PolicyKit
    demande l'autorisation comme avant.
    """
    return (
        "rc=0\n"
        "if [ -n \"$(flatpak --user list --columns=application 2>/dev/null)\" ]; "
        "then flatpak update --user -y || rc=$?; fi\n"
        "sudo -n flatpak update --system -y || "
        "flatpak update --system -y || rc=$?\n"
        "exit $rc\n"
    )

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
"""

def parse_tailscale_state(output: str) -> dict:
    """État Tailscale à partir des lignes `clé=valeur` du script ci-dessus."""
    values: dict[str, str] = {}
    for line in output.splitlines():
        key, _, value = line.partition("=")
        if key.strip() in ("enabled", "active", "server", "installed"):
            values[key.strip()] = value.strip()
    return {
        "installed": values.get("installed") == "yes",
        "enabled": values.get("enabled") == "enabled",
        "active": values.get("active") == "active",
        "server": values.get("server", ""),
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


def user_service_enabled(name: str) -> bool:
    """Le service *utilisateur* est-il activé (timer de la session) ?"""
    try:
        result = subprocess.run(
            ["systemctl", "--user", "is-enabled", name],
            capture_output=True, text=True, timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def auto_update_available() -> bool:
    """Le helper des mises à jour automatiques est-il dans l'image ?"""
    return Path(AUTO_UPDATE_HELPER).is_file()


def auto_updates_enabled() -> bool:
    """Image Gablue, Flatpaks et Homebrew sont-ils mis à jour automatiquement ?

    Les timers système sont lus par le helper (même liste fermée que pour la
    bascule, brew compris) ; le timer Flatpak de la session se lit sans
    privilège. Le tout est traité comme un seul réglage : une moitié éteinte
    ne doit pas s'afficher « activé ».
    """
    try:
        result = subprocess.run(
            ["sudo", "-n", AUTO_UPDATE_HELPER, "status"],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0 and user_service_enabled(
        AUTO_UPDATE_USER_TIMER
    )


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


def services_helpers_available() -> bool:
    """Les helpers du dialogue « Services et pilotes » sont-ils installés ?"""
    return all(
        Path(path).is_file()
        for path in (SERVICE_HELPER, LOW_LATENCY_HELPER, BROADCOM_HELPER,
                     ATUIN_HELPER)
    )


def rebase_helper_available() -> bool:
    """Le helper de bascule de variante est-il installé ?"""
    return Path(REBASE_HELPER).is_file()


def services_choices() -> dict:
    """État des bascules : services, couche Vulkan, Wi-Fi, atuin, màj auto."""
    return {
        "displaylink": service_enabled("displaylink.service"),
        "docker": service_enabled("docker.service"),
        "low_latency": LOW_LATENCY_FILE.is_file(),
        "broadcom_wl": BROADCOM_WL_FILE.is_file(),
        "atuin": atuin_active(),
        "auto_updates": auto_update_available() and auto_updates_enabled(),
    }


def system_apply_script(changes: dict[str, bool]) -> str:
    """Script d'application : un helper par domaine, en un seul passage."""
    lines: list[str] = []
    for key, unit in (
        ("displaylink", "displaylink.service"),
        ("docker", "docker.service"),
    ):
        if key in changes:
            lines.append(
                f"sudo -n {SERVICE_HELPER} {unit} "
                + ("on" if changes[key] else "off")
            )
    if "low_latency" in changes:
        lines.append(
            f"sudo -n {LOW_LATENCY_HELPER} "
            + ("on" if changes["low_latency"] else "off")
        )
    if "broadcom_wl" in changes:
        lines.append(
            f"sudo -n {BROADCOM_HELPER} "
            + ("on" if changes["broadcom_wl"] else "off")
        )
    if "atuin" in changes:
        # Intégration du shell : fichiers de l'utilisateur, aucun privilège.
        lines.append(
            f"{ATUIN_HELPER} " + ("on" if changes["atuin"] else "off")
        )
    if "auto_updates" in changes:
        enabled = changes["auto_updates"]
        # Timers système par le helper (règle sudoers NOPASSWD, liste fermée),
        # puis le timer Flatpak de la session, sans privilège. `--user` échoue
        # sans bus de session : on le dit au lieu de faire échouer une moitié
        # déjà appliquée.
        lines.append(
            f"sudo -n {AUTO_UPDATE_HELPER} " + ("on" if enabled else "off")
        )
        lines.append(
            f"systemctl --user {'enable' if enabled else 'disable'} --now "
            f"{AUTO_UPDATE_USER_TIMER} "
            "|| echo \"Timer Flatpak utilisateur inchangé (pas de session).\""
        )
    return "\n".join(lines) + "\n"


def rebase_script(variant: str) -> str:
    """Bascule vers une autre variante d'image (redémarrage ensuite)."""
    return f"sudo -n {REBASE_HELPER} {variant}\n"

# Tâches de bootc (champ machine `task` de la progression JSONL) → libellé.
PROGRESS_TASKS = {
    "pulling": "Téléchargement de l'image",
    "ostree_chunk": "Téléchargement des objets OSTree",
    "ostree_derived": "Construction de la couche dérivée",
    "importing": "Import de l'image",
    "composing": "Écriture du déploiement",
}

def parse_progress_event(line: str) -> dict:
    """Événement JSONL de `bootc upgrade --progress-fd` → progression lisible.

    Chaque ligne est un objet `{type: ProgressBytes|ProgressSteps, …}` (voir
    `progress_jsonl.rs` de bootc). `{}` pour tout le reste (message Start, JSON
    tronqué, ligne vide). Le pourcentage prend les octets quand le total est
    connu (images), sinon les étapes (couches), sinon None → barre indéterminée.
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
    done = number("bytes") + number("bytesCached")
    steps_total = number("stepsTotal")
    steps = number("steps") + number("stepsCached")
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
