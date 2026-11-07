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
from pathlib import Path

from .constants import (
    ATUIN_HELPER, AUTO_UPDATE_HELPER, AUTO_UPDATE_USER_TIMER, AVAHI_HELPER,
    AVAHI_SERVICE_FILE, BREW_BINARY, BROADCOM_HELPER, BROADCOM_WL_FILE,
    BTRFS_HELPER, CLEAN_HELPER, CMDLINE, CPUID_EMU_BLACKLIST,
    CPUID_EMU_CONF, CPUID_EMU_HELPER, GAMEPAD_HELPER, GRUB_HELPER,
    NVME_THERMAL_HELPER, SECUREBOOT_HELPER,
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

    Le script lance deux mises à jour successives (compte puis système) : les
    compteurs repartent de zéro à la seconde, la barre peut donc redescendre une
    fois entre les deux phases.
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
    """Phrase de fin de journal pour `flatpak update`.

    Le script enchaîne deux mises à jour (compte puis système) : la seconde
    répond presque toujours « Nothing to do. » même quand la première a mis des
    applications à jour. On ne conclut donc « rien à faire » que si aucune
    opération n'a démarré — sinon le compte rendu mentirait.
    """
    if any(_FLATPAK_OP_RE.match(line) for line in (text or "").splitlines()):
        return ""
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


AUTO_UPDATE_TARGETS = ("image", "flatpak", "brew")

def auto_update_calendar(frequency: str) -> str:
    """Calendrier systemd du rythme choisi (04:00, comme l'image)."""
    return "*-*-* 04:00:00" if frequency == "daily" else "Sat *-*-* 04:00:00"

def auto_update_schedule_dropin(frequency: str) -> str:
    """Drop-in `[Timer]` commun aux timers système et au timer du compte.

    Les trois déclencheurs sont remis à zéro avant le nôtre : `rpm-ostreed`
    porte `OnBootSec`/`OnUnitInactiveSec` et l'image un `OnCalendar` samedi —
    sans les vider, le timer cumulerait deux calendriers.
    """
    return (
        "[Timer]\n"
        "OnBootSec=\n"
        "OnUnitInactiveSec=\n"
        "OnCalendar=\n"
        f"OnCalendar={auto_update_calendar(frequency)}\n"
        "Persistent=true\n"
        "RandomizedDelaySec=10m\n"
    )

def parse_auto_update_status(text: str) -> dict:
    """Lignes `clé=valeur` du helper → domaines actifs et rythme."""
    values: dict[str, str] = {}
    for line in (text or "").splitlines():
        key, _, value = line.partition("=")
        if key.strip() in ("image", "flatpak", "brew", "frequency"):
            values[key.strip()] = value.strip()
    frequency = values.get("frequency", "weekly")
    return {
        "image": values.get("image") == "enabled",
        "flatpak": values.get("flatpak") == "enabled",
        "brew": values.get("brew") == "enabled",
        "frequency": frequency if frequency in ("daily", "weekly") else "weekly",
    }

def auto_update_state() -> dict:
    """État complet : timers système (helper) + timer Flatpak de la session.

    `flatpak` n'est vrai que si les deux moitiés tournent : une image mise à
    jour sans les applications du compte ne doit pas s'afficher « activé ».
    """
    state = {
        "ok": False, "image": False, "flatpak": False, "brew": False,
        "user_flatpak": user_service_enabled(AUTO_UPDATE_USER_TIMER),
        "frequency": "weekly",
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
    state["flatpak"] = state["flatpak"] and state["user_flatpak"]
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

def auto_update_script(changes: dict[str, bool], desired: dict[str, bool],
                       frequency: str) -> str:
    """Script d'application : domaines modifiés, rythme, timer du compte.

    Le rythme est écrit même inchangé (idempotent) : c'est lui qui crée les
    drop-ins `/etc` absents sur une image fraîche. Le timer Flatpak de la
    session est géré ici, sans privilège, parce que son drop-in vit dans
    `~/.config/systemd/user`.
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
    steps.append(("Rythme", [
        f"sudo -n {AUTO_UPDATE_HELPER} schedule {frequency}"
    ]))

    dropin = auto_update_schedule_dropin(frequency)
    user = [
        'dir="$HOME/.config/systemd/user/'
        f'{AUTO_UPDATE_USER_TIMER}.d"',
        'mkdir -p "$dir"',
        'cat > "$dir/50-gablue-schedule.conf" <<\'EOF\'',
        *dropin.splitlines(),
        "EOF",
        "systemctl --user daemon-reload || true",
    ]
    if desired.get("flatpak"):
        user += [
            f"systemctl --user enable --now {AUTO_UPDATE_USER_TIMER} "
            '|| echo "Timer Flatpak utilisateur inchangé (pas de session)."',
            f"systemctl --user restart {AUTO_UPDATE_USER_TIMER} || true",
        ]
    else:
        user.append(
            f"systemctl --user disable --now {AUTO_UPDATE_USER_TIMER} "
            '|| echo "Timer Flatpak utilisateur inchangé (pas de session)."'
        )
    steps.append(("Flatpaks du compte", user))
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


# Domaines de nettoyage, dans l'ordre où ils s'appliquent.
CLEAN_DOMAINS = (
    "podman", "flatpak", "ostree", "brew",
    "gwine_temp", "gwine_shaders", "gwine", "wgp", "lgp", "opencode",
)

# Caches purement régénérables : composants et shaders gwine, amorçage des
# extras WGP/LGP, paquets npm des plugins OpenCode. Jamais les sauvegardes
# (~/.local/share/lgp-saves, ~/.local/share/opencode) ni la configuration.
CLEAN_CACHE_SUFFIXES = {
    "gwine_temp": ".cache/gwine/temp",
    "gwine_shaders": ".cache/gwine/shader-cache",
    "gwine": ".cache/gwine",
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
    if domain == "ostree":
        return clean_available()
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
    les Flatpaks **système** et `rpm-ostree cleanup` passent par le helper
    privilégié (sous-commandes exactes du sudoers), magasins root obligent.
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
    if "ostree" in selected and clean_available():
        steps.append(("rpm-ostree", [
            f"sudo -n {CLEAN_HELPER} rpm-ostree",
        ]))
    for domain, label in (
        ("gwine_temp", "Cache gwine — temp"),
        ("gwine_shaders", "Cache gwine — shaders"),
        ("gwine", "Cache gwine"),
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
        # Même préfixe et mêmes exports que `brew_update_script` : le PATH
        # d'une session GUI ne contient pas `/home/linuxbrew/.linuxbrew/bin`.
        steps.append(("Homebrew", [
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
        ]))
    return _steps_script(steps)


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
    try:
        data = json.loads(result.stdout)
    except ValueError:
        return []
    devices = data.get("devices") if isinstance(data, dict) else None
    if not isinstance(devices, list):
        return []
    return [item for item in devices if isinstance(item, dict)]


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
    if "SecureBoot enabled" in text:
        return "enabled"
    if "SecureBoot disabled" in text:
        return "disabled"
    return "unknown"


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
