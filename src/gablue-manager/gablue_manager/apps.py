"""Applications AppImage gérées par GearLever (émulateurs et outils).

Chaque application a déjà son script d'installation dans `/usr/bin` (dernière
release, intégration GearLever, correction du `.desktop`, copie des fichiers
Gablue pour ES-DE). Le manager ne réimplémente pas cette logique : il lance le
script et affiche son journal. ES-DE expose `--config-only` pour ne recopier que
la configuration Gablue (`custom_systems` + lanceurs Tools).

La suppression est faite ici, ou par un script dédié quand elle touche plus que
l'AppImage (`opencode2-remove` : CLI npm, entrées Dolphin, MCP Lightpanda).
Dans tous les cas la configuration, les sauvegardes et les jeux sont conservés.
"""

import fnmatch
import os
import re
import shlex
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .constants import _which

GEARLEVER_ID = "it.mijorus.gearlever"
ESDE_SHARE = Path("/usr/share/ublue-os/gablue/esde")


@dataclass(frozen=True)
class App:
    """Une application installée en AppImage dans `~/AppImages`."""

    key: str
    name: str
    description: str
    group: str          # "emulateur" | "outil" (titres de section du dialogue)
    pattern: str        # motif sur le nom de fichier, insensible à la casse
    script: str         # script d'installation dans /usr/bin
    desktop: str        # préfixe des .desktop et icônes à retirer
    install_args: tuple[str, ...] = ()
    cli: str = ""       # commande dont on lit `--version` (installation mixte)
    remover: str = ""   # script de suppression dédié (sinon suppression générique)


APPS: tuple[App, ...] = (
    App(
        "esde", "ES-DE",
        "Frontend manette ; copie aussi les systèmes et lanceurs Gablue.",
        "emulateur",
        "esde*.appimage", "esde-install", "esde",
        # Sans boîte de dialogue : le journal du manager fait office de retour.
        install_args=("--yes",),
    ),
    App(
        "azahar", "Azahar",
        "Émulateur Nintendo 3DS.",
        "emulateur",
        "azahar*.appimage", "azahar-install", "azahar",
    ),
    App(
        "eden", "Eden",
        "Émulateur Nintendo Switch.",
        "emulateur",
        "*eden*.appimage", "eden-install", "eden",
    ),
    App(
        "xenia", "Xenia Edge",
        "Émulateur Xbox 360.",
        "emulateur",
        "xenia*.appimage", "xenia-install", "xenia",
    ),
    App(
        "shadps4", "ShadPS4",
        "Émulateur PlayStation 4.",
        "emulateur",
        "shadps4*.appimage", "shadps4-install", "shadps4",
    ),
    App(
        "opencode", "OpenCode 2",
        "Éditeur et agent de code.",
        "outil",
        "opencode*.appimage", "opencode2-install", "opencode",
        cli="opencode", remover="opencode2-remove",
    ),
)

GROUPS: tuple[tuple[str, str], ...] = (
    ("emulateur", "Émulateurs"),
    ("outil", "Outils"),
)


def appimages_dir() -> Path:
    return Path.home() / "AppImages"


def match_appimage(app: App, names) -> str:
    """Premier nom de fichier qui correspond au motif de l'application."""
    for name in sorted(names):
        if fnmatch.fnmatch(name.lower(), app.pattern):
            return name
    return ""


def find_appimage(app: App, directory: Path | None = None) -> Path | None:
    """AppImage installée pour cette application, ou None."""
    directory = directory if directory is not None else appimages_dir()
    try:
        files = [entry.name for entry in directory.iterdir() if entry.is_file()]
    except OSError:
        return None
    name = match_appimage(app, files)
    return directory / name if name else None


def parse_gearlever_listing(text: str) -> dict[str, str]:
    """Versions indexées par chemin, depuis `gearlever --list-installed`.

        ES-DE   [3.4.0]   [UpdatesNotAvailable]   /home/gab/AppImages/esde.appimage

    Le nom peut contenir des espaces, le chemin est le dernier champ.
    """
    versions: dict[str, str] = {}
    for line in text.splitlines():
        match = re.match(
            r"^\s*(?P<name>.+?)\s+\[(?P<version>[^\]]*)\]\s+"
            r"\[[^\]]*\]\s+(?P<path>/\S+)\s*$",
            line,
        )
        if not match:
            continue
        version = match.group("version").strip()
        if version.lower() in ("", "unknown", "not specified", "n/a"):
            version = ""
        versions[str(Path(match.group("path")))] = version
    return versions


def integrated_versions() -> dict[str, str]:
    """Versions connues de GearLever, indexées par chemin d'AppImage."""
    try:
        result = subprocess.run(
            ["flatpak", "run", GEARLEVER_ID, "--list-installed"],
            capture_output=True, text=True, timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}
    if result.returncode != 0:
        return {}
    return parse_gearlever_listing(result.stdout)


def gearlever_available() -> bool:
    """GearLever est-il installé (sans lui, ni installation ni mise à jour) ?"""
    try:
        result = subprocess.run(
            ["flatpak", "info", GEARLEVER_ID],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def parse_version(text: str) -> str:
    """Premier numéro de version d'une sortie `--version` (vide sinon)."""
    match = re.search(r"\d+\.\d+(?:\.\d+)*", text or "")
    return match.group(0) if match else ""


def cli_version(command: str) -> str:
    """Version de la commande, ou vide si elle n'est pas installée."""
    if not command:
        return ""
    binary = _which(command)
    if not binary:
        # Session graphique : ~/.local/bin n'est pas toujours dans le PATH.
        candidate = Path.home() / ".local/bin" / command
        if candidate.is_file() and os.access(candidate, os.X_OK):
            binary = str(candidate)
    if not binary:
        return ""
    try:
        result = subprocess.run(
            [binary, "--version"], capture_output=True, text=True, timeout=8,
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return parse_version(result.stdout or result.stderr)


def script_options(script: str) -> set[str]:
    """Options connues du script (image plus ancienne = ensemble vide)."""
    return {
        option for option in ("--yes", "--config-only")
        if script_has_option(script, option)
    }


def state(app: App, versions: dict[str, str] | None = None,
          directory: Path | None = None) -> dict:
    """État d'une application : AppImage, version GearLever, script, CLI."""
    image = find_appimage(app, directory)
    if versions is None:
        versions = integrated_versions()
    version = versions.get(str(image), "") if image is not None else ""
    script = _which(app.script)
    # `_which` rend le nom nu quand le binaire manque : pour le script de
    # suppression, on veut savoir s'il EXISTE (sinon retrait à moitié).
    remover = _which(app.remover) if app.remover else ""
    if remover and not Path(remover).is_file():
        remover = ""
    return {
        "installed": image is not None,
        "name": image.name if image is not None else "",
        "path": image,
        "version": version,
        "cli": cli_version(app.cli),
        "script": script,
        "script_ok": Path(script).is_file() and os.access(script, os.X_OK),
        "options": script_options(script),
        "remover": remover,
    }


def all_states() -> dict[str, dict]:
    """État de toutes les applications, avec une seule interrogation de GearLever."""
    versions = integrated_versions()
    return {app.key: state(app, versions) for app in APPS}


def installed_any(app: App, current: dict) -> bool:
    """Installation partielle comprise (CLI seule, AppImage seule)."""
    return bool(current["installed"] or current["cli"])


def describe(app: App, current: dict) -> str:
    """Texte d'état d'une ligne du dialogue."""
    if not installed_any(app, current):
        return "Non installé"
    parts: list[str] = []
    if current["cli"]:
        parts.append(f"CLI {current['cli']}")
    if current["installed"]:
        name = current.get("name") or (
            current["path"].name if current.get("path") else ""
        )
        if current["version"]:
            name += f" · version {current['version']}"
        parts.append(name)
    return "Installé — " + " · ".join(parts)


def install_argv(app: App) -> list[str]:
    """Commande d'installation / mise à jour (le script fait les deux)."""
    return [_which(app.script), *app.install_args]


def config_argv() -> list[str]:
    """Recopie uniquement la configuration Gablue d'ES-DE."""
    return [_which("esde-install"), "--config-only"]


def script_has_option(script: str, option: str) -> bool:
    """Le script de l'image connaît-il cette option ?

    On LIT le fichier au lieu de le sonder en l'exécutant : un ancien
    `esde-install` ignore les arguments inconnus et lance l'installation
    interactive (constaté — la fenêtre « ES-DE semble déjà installé » surgit
    au milieu d'une simple mise à jour de configuration).
    """
    try:
        text = Path(script).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return False
    return option in text


def esde_rom_path(home: Path | None = None) -> Path:
    """`ROMDirectory` d'es_settings.xml, ou ~/Roms (même règle que le script)."""
    home = home if home is not None else Path.home()
    fallback = home / "Roms"
    settings = home / "ES-DE/settings/es_settings.xml"
    try:
        text = settings.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return fallback
    match = re.search(r'ROMDirectory"\s+value="([^"]*)"', text)
    if not match or not match.group(1):
        return fallback
    return Path(match.group(1))


def copy_esde_config(home: Path | None = None,
                     share: Path | None = None) -> list[str]:
    """Équivalent de `esde-install --config-only`, sans shell ni dialogue.

    Utilisé quand le script de l'image ne connaît pas l'option (image plus
    ancienne que le paquet) : `custom_systems` est recopié dans `~/ES-DE/` et
    les lanceurs Tools dans `<ROMDirectory>/tools/`, sans écraser un lanceur
    personnalisé déjà présent.
    """
    home = home if home is not None else Path.home()
    share = share if share is not None else ESDE_SHARE
    copied: list[str] = []

    systems = share / "custom_systems"
    if systems.is_dir():
        target = home / "ES-DE"
        target.mkdir(parents=True, exist_ok=True)
        shutil.copytree(systems, target / "custom_systems",
                        dirs_exist_ok=True)
        copied.append("custom_systems")

    tools = share / "tools"
    if tools.is_dir():
        directory = esde_rom_path(home) / "tools"
        directory.mkdir(parents=True, exist_ok=True)
        for launcher in sorted(tools.glob("*.sh")):
            destination = directory / launcher.name
            if destination.exists():
                continue
            shutil.copyfile(launcher, destination)
            destination.chmod(0o755)
            copied.append(launcher.name)
    return copied


def remove_argv(app: App) -> list[str]:
    """Commande de suppression : script dédié, sinon retrait générique."""
    if app.remover:
        return [_which(app.remover)]
    return [_which("bash"), "-c", remove_script(app)]


def remove_script(app: App) -> str:
    """Script de suppression générique : retrait GearLever puis fichiers.

    La configuration et les sauvegardes ne sont PAS touchées : seul l'AppImage,
    son entrée de menu et ses icônes disparaissent (réinstallation propre).
    """
    return (
        "exec < /dev/null\n"
        "shopt -s nullglob nocaseglob\n"
        f'for image in "$HOME/AppImages"/{app.pattern}; do\n'
        '    echo "Suppression de $image"\n'
        '    yes | flatpak run it.mijorus.gearlever --remove "$image" '
        "2>/dev/null || true\n"
        '    rm -f "$image"\n'
        "done\n"
        f'rm -f "$HOME/.local/share/applications"/{app.desktop}*.desktop\n'
        f'rm -f "$HOME/.local/share/icons"/hicolor/*/apps/{app.desktop}*\n'
        f'rm -f "$HOME/.local/share/icons"/{app.desktop}*\n'
        f'rm -f "$HOME/AppImages/.icons"/{app.desktop}*\n'
        "kbuildsycoca6 >/dev/null 2>&1 || "
        'update-desktop-database "$HOME/.local/share/applications" '
        "2>/dev/null || true\n"
    )


# =============================================================================
# POSTES DISTANTS (une seule connexion SSH pour tout l'état)
# =============================================================================

REMOTE_STATE_SCRIPT = r"""
echo "##gearlever"
if flatpak info it.mijorus.gearlever >/dev/null 2>&1; then
    echo yes
    flatpak run it.mijorus.gearlever --list-installed 2>/dev/null
else
    echo no
fi
echo "##images"
ls -1 "$HOME/AppImages" 2>/dev/null
echo "##cli"
if command -v opencode >/dev/null 2>&1; then opencode --version 2>/dev/null; fi
echo "##scripts"
for script in esde-install azahar-install eden-install xenia-install \
              shadps4-install opencode2-install opencode2-remove; do
    path="$(command -v "$script" 2>/dev/null || true)"
    if [ -z "$path" ]; then
        printf '%s\t%s\t\n' "$script" "-"
        continue
    fi
    options=""
    grep -q -- '--yes' "$path" 2>/dev/null && options="$options --yes"
    grep -q -- '--config-only' "$path" 2>/dev/null && options="$options --config-only"
    printf '%s\t%s\t%s\n' "$script" "$path" "$options"
done
""".strip()


def parse_remote_state(text: str) -> dict:
    """Découpe la sortie de `REMOTE_STATE_SCRIPT` (sections `##nom`)."""
    sections: dict[str, list[str]] = {}
    current = ""
    for raw in text.splitlines():
        line = raw.rstrip()
        if line.startswith("##"):
            current = line[2:].strip()
            sections[current] = []
        elif current:
            sections[current].append(line)

    gearlever_lines = sections.get("gearlever", [])
    gearlever = bool(gearlever_lines) and gearlever_lines[0].strip() == "yes"
    versions = (
        parse_gearlever_listing("\n".join(gearlever_lines[1:]))
        if gearlever else {}
    )
    scripts: dict[str, dict] = {}
    for line in sections.get("scripts", []):
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        path = "" if parts[1] == "-" else parts[1].strip()
        options = set(parts[2].split()) if len(parts) > 2 else set()
        scripts[parts[0]] = {"path": path, "options": options}
    return {
        "gearlever": gearlever,
        "versions": versions,
        "images": [line for line in sections.get("images", []) if line.strip()],
        "cli": parse_version("\n".join(sections.get("cli", []))),
        "scripts": scripts,
    }


def remote_states(text: str) -> tuple[dict[str, dict], bool]:
    """État des applications d'un poste, depuis la sortie du script distant."""
    parsed = parse_remote_state(text)
    states: dict[str, dict] = {}
    for app in APPS:
        name = match_appimage(app, parsed["images"])
        version = ""
        if name:
            for path, value in parsed["versions"].items():
                if Path(path).name == name:
                    version = value
                    break
        info = parsed["scripts"].get(app.script, {})
        remover = parsed["scripts"].get(app.remover, {}) if app.remover else {}
        states[app.key] = {
            "installed": bool(name),
            "name": name,
            "path": None,
            "version": version,
            "cli": parsed["cli"] if app.key == "opencode" else "",
            "script": info.get("path", ""),
            "script_ok": bool(info.get("path")),
            "options": info.get("options", set()),
            "remover": remover.get("path", ""),
        }
    return states, parsed["gearlever"]


def install_command(app: App, current: dict) -> str:
    """Commande d'installation / mise à jour sur un poste distant."""
    args = list(app.install_args)
    if args and args[0] not in current["options"]:
        args = []          # option inconnue du script : ne pas l'envoyer
    return shlex.join([current["script"], *args])


def config_command(current: dict) -> str:
    """Recopie de la configuration ES-DE à distance ("" si non supportée)."""
    if "--config-only" not in current["options"]:
        return ""
    return shlex.join([current["script"], "--config-only"])


def remove_command(app: App, current: dict) -> str:
    """Suppression à distance : script dédié, sinon retrait générique."""
    if app.remover:
        if current["remover"]:
            return shlex.join([current["remover"]])
        return ""          # script dédié absent du poste : on ne fait pas à moitié
    return shlex.join(["bash", "-c", remove_script(app)])
