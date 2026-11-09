"""Applications AppImage gérées par GearLever (émulateurs et outils).

Le catalogue (noms, motifs, dépôts, règles de version) n'est **pas** ici : il
vit dans `/usr/share/ublue-os/gablue/apps.toml`, la même source que le moteur
d'installation `/usr/libexec/gablue-apps`. Le manager ne réimplémente donc ni la
liste des applications, ni la résolution de la dernière version : il lance le
script de l'image (`-install`, coquille du moteur pour les applications
migrées) et demande au moteur la version publiée (`gablue-apps probe --json`).

ES-DE expose `--config-only` pour ne recopier que la configuration Gablue
(`custom_systems` + lanceurs Tools). La suppression est faite ici, ou par un
script dédié quand elle touche plus que l'AppImage (`opencode2-remove` : CLI
npm, entrées Dolphin, MCP Lightpanda). Dans tous les cas la configuration, les
sauvegardes et les jeux sont conservés.
"""

import fnmatch
import json
import os
import re
import shlex
import shutil
import subprocess
import tomllib
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

from .constants import APPS_ENGINE, _which

GEARLEVER_ID = "it.mijorus.gearlever"
ESDE_SHARE = Path("/usr/share/ublue-os/gablue/esde")

# Catalogue partagé avec le moteur : même fichier dans l'image, et le même
# chemin relatif depuis le dépôt (lecture de développement et tests).
CATALOG_IMAGE = Path("/usr/share/ublue-os/gablue/apps.toml")
CATALOG_REPO = Path("files/system/all/usr/share/ublue-os/gablue/apps.toml")


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


def catalog_file() -> Path | None:
    """Catalogue partagé avec le moteur, ou None s'il est absent.

    L'image le livre à `/usr/share/ublue-os/gablue/apps.toml` ; dans le dépôt
    (développement, tests) il est sous `files/system/all/usr/…`. Un catalogue
    manquant n'est pas une erreur fatale : le dialogue se vide au lieu de
    faire tomber le manager.
    """
    override = os.environ.get("GABLUE_APPS_TOML")
    if override:
        return Path(override)
    if CATALOG_IMAGE.is_file():
        return CATALOG_IMAGE
    repo = Path(__file__).resolve().parents[3] / CATALOG_REPO
    return repo if repo.is_file() else None


def load_catalog(path: Path | None = None
                 ) -> tuple[tuple[App, ...], tuple[tuple[str, str], ...]]:
    """(applications, groupes) lus du catalogue TOML, dans l'ordre du fichier."""
    target = Path(path) if path is not None else catalog_file()
    if target is None:
        return (), ()
    try:
        with target.open("rb") as handle:
            data = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return (), ()
    groups = tuple(
        (str(group.get("key") or ""), str(group.get("label") or ""))
        for group in data.get("group") or []
        if group.get("key")
    )
    apps = tuple(
        App(
            key=str(row["key"]),
            name=str(row.get("name") or row["key"]),
            description=str(row.get("description") or ""),
            group=str(row.get("group") or ""),
            pattern=str(row.get("pattern") or ""),
            script=str(row.get("script") or ""),
            desktop=str(row.get("desktop") or ""),
            install_args=tuple(
                str(argument) for argument in row.get("install_args") or ()
            ),
            cli=str(row.get("cli") or ""),
            remover=str(row.get("remover") or ""),
        )
        for row in data.get("app") or []
        if row.get("key")
    )
    return apps, groups


APPS, GROUPS = load_catalog()


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
    try:
        mtime = image.stat().st_mtime if image is not None else 0.0
    except OSError:
        mtime = 0.0
    return {
        "installed": image is not None,
        "name": image.name if image is not None else "",
        "path": image,
        "mtime": mtime,
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


# -- Mises à jour des AppImages --------------------------------------------
#
# Une seule résolution dans toute l'image : le moteur `/usr/libexec/gablue-apps`
# (`probe <clé> --json`), celui qui téléchargera. Le manager ne recalcule donc
# plus les versions lui-même — il avait sa propre lecture du flux Atom, qui ne
# distingue pas les pré-releases et proposait une « mise à jour » (Lichii
# build-130) que le moteur refusait d'installer (`/releases/latest` → 125).

PROBE_TIMEOUT = 25.0        # le moteur borne lui-même chaque requête à 20 s


def latest_info(app: App) -> dict:
    """Dernière version publiée, telle que la voit le moteur d'installation.

    `{}` quand le moteur est absent (image antérieure) ou que le réseau ne
    répond pas : le verdict reste indéterminé et le bouton reste actif — on ne
    dit jamais « à jour » à tort.
    """
    if not Path(APPS_ENGINE).is_file():
        return {}
    try:
        result = subprocess.run(
            [APPS_ENGINE, "probe", app.key, "--json"],
            capture_output=True, text=True, timeout=PROBE_TIMEOUT,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}
    if result.returncode != 0:
        return {}
    try:
        data = json.loads(result.stdout or "")
    except ValueError:
        return {}
    if not isinstance(data, dict):
        return {}
    return {
        "version": str(data.get("version") or ""),
        "published": data.get("published"),
    }


def version_tuple(text: str) -> tuple[int, ...]:
    """Nombres d'une version, comparables (« v0.2.1 » → (0, 2, 1))."""
    match = re.search(r"\d+(?:\.\d+)*", text or "")
    if match is None:
        return ()
    return tuple(int(part) for part in match.group(0).split("."))


def update_available(current: dict, info: dict) -> bool | None:
    """Mise à jour disponible ? `None` = impossible de conclure.

    Comparaison de version quand les deux côtés en donnent une (certains tags
    GitHub sont des hachages) ; sinon date de publication de la release contre
    date du fichier installé. En cas de doute on ne dit jamais « à jour » :
    le bouton reste utilisable.
    """
    if not info:
        return None
    latest = version_tuple(info.get("version") or "")
    installed = version_tuple(
        current.get("version") or current.get("cli") or ""
    )
    if latest and installed:
        return latest > installed
    published = info.get("published")
    mtime = current.get("mtime") or 0
    if published and mtime:
        # Petite marge : le fichier est écrit juste après la publication.
        return published > float(mtime) + 60
    return None


def app_update_verdicts(states: dict[str, dict],
                        infos: dict[str, dict]) -> tuple[list[str], bool]:
    """Clés des applications installées à mettre à jour, et doute éventuel.

    `infos` porte la dernière version publiée par application ({} = flux muet
    ou version non comparable) : le verdict vaut alors `None`, qui n'est pas
    une erreur mais interdit de conclure « tout est à jour ».
    """
    pending: list[str] = []
    unknown = False
    for app in APPS:
        current = states.get(app.key)
        if not current or not installed_any(app, current):
            continue
        verdict = update_available(current, infos.get(app.key) or {})
        if verdict is True:
            pending.append(app.key)
        elif verdict is None:
            unknown = True
    return pending, unknown


def pending_for(states: dict[str, dict],
                stop=None) -> tuple[list[str], bool, int]:
    """(clés à mettre à jour, doute, nombre d'applications installées).

    Mêmes verdicts que `pending_updates`, sur des états **déjà lus** : le poste
    local lit les siens (`all_states`), un poste distant les reçoit par SSH
    (`remote_states`). Les flux de releases, eux, se lisent toujours d'ici —
    aucun poste n'a besoin d'un accès réseau sortant pour être à jour.

    `{}` côté version publiée laisse le verdict indéterminé au lieu de mentir.
    `stop` (threading.Event) vient de la fermeture de la fenêtre : on cesse
    d'attendre les applications restantes, sans bloquer la sortie. Les sondes
    déjà parties expirent d'elles-mêmes (`PROBE_TIMEOUT`).
    """
    installed = [app for app in APPS if installed_any(app, states[app.key])]
    if not installed:
        return [], False, 0
    infos: dict[str, dict] = {}
    pool = ThreadPoolExecutor(max_workers=4)
    try:
        try:
            futures = {
                app.key: pool.submit(latest_info, app) for app in installed
            }
        except RuntimeError:
            # Interpréteur en cours d'arrêt (`cannot schedule new futures
            # after interpreter shutdown`) : plus personne n'attend de
            # verdict, on rend « indéterminé » au lieu de lever (vécu
            # 09/11/2026, sonde survivante à la fermeture).
            return [], True, len(installed)
        for app in installed:
            future = futures.get(app.key)
            if future is None or (stop is not None and stop.is_set()):
                infos[app.key] = {}
                continue
            try:
                infos[app.key] = future.result(
                    timeout=PROBE_TIMEOUT + 2
                )
            except Exception:
                infos[app.key] = {}
    finally:
        pool.shutdown(wait=False, cancel_futures=True)
    pending, unknown = app_update_verdicts(states, infos)
    return pending, unknown, len(installed)


def pending_updates(stop=None) -> tuple[list[str], bool, int]:
    """Applications installées sur CE poste qui ont une mise à jour."""
    return pending_for(all_states(), stop)


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


def names_for(keys) -> list[str]:
    """Noms affichés des applications correspondant à ces clés.

    L'ordre est celui du catalogue (`APPS`), donc stable d'un affichage à
    l'autre : le dialogue et la confirmation listent la même chose.
    """
    wanted = set(keys)
    return [app.name for app in APPS if app.key in wanted]


def _marker(text: str) -> str:
    """Ligne de protocole `::gablue:` écrite par le script sur sa sortie.

    `printf '%s\\n'` plutôt qu'un format à trous : un nom d'application avec
    une apostrophe casserait un `printf '<texte>'` en single quotes.
    """
    return "printf '%s\\n' " + shlex.quote(text)


def update_script(keys) -> str:
    """Script bash : mettre à jour les applications listées, une par une.

    Le script d'installation de chaque application télécharge la dernière
    release et réintègre l'AppImage dans le menu (GearLever) ; `--yes` coupe
    ses questions.

    Progression : `::gablue:step i/N` pose le plancher de la barre (i-1
    applications derrière), puis `::gablue:busy` la fait **balayer** pendant le
    téléchargement de l'application en cours — un AppImage arrive d'un bloc
    (cURL sans pourcentage exploitable), donc le libellé dit quelle application
    travaille et le balayage montre que ça avance. Sans ce marqueur, la barre
    restait figée sur `(i-1)/N` pendant toute la durée du transfert : c'était
    un « 0 % » pendant une minute pour une application unique (vécu 08/11/2026).

    `set -e` : une application en échec arrête la suite, le code de sortie est
    celui de la première qui a échoué — sinon la dernière qui réussit rendait
    « Terminé » alors qu'une autre n'avait pas été mise à jour.
    """
    wanted = set(keys)
    selected = [app for app in APPS if app.key in wanted]
    if not selected:
        return ""
    total = len(selected)
    lines: list[str] = ["set -e"]
    for index, app in enumerate(selected, start=1):
        lines.append(_marker(f"::gablue:step {index} {total} {app.name}"))
        lines.append(
            _marker(f"::gablue:busy Mise à jour de {app.name} "
                    f"({index}/{total})…")
        )
        lines.append(shlex.join(install_argv(app)))
    return "\n".join(lines) + "\n"


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
printf '\n##gearlever\n'
if flatpak info it.mijorus.gearlever >/dev/null 2>&1; then
    echo yes
    flatpak run it.mijorus.gearlever --list-installed 2>/dev/null
else
    echo no
fi
printf '\n##images\n'
ls -1 "$HOME/AppImages" 2>/dev/null
printf '\n##mtimes\n'
for f in "$HOME/AppImages"/*; do
    [ -f "$f" ] || continue
    printf '%s\t%s\n' "$(basename "$f")" \
        "$(stat -c %Y "$f" 2>/dev/null || echo 0)"
done
printf '\n##cli\n'
if command -v opencode >/dev/null 2>&1; then opencode --version 2>/dev/null; fi
printf '\n##scripts\n'
for script in esde-install azahar-install eden-install xenia-install \
              lichii-install opencode2-install opencode2-remove; do
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
    mtimes: dict[str, float] = {}
    for line in sections.get("mtimes", []):
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        try:
            mtimes[parts[0].strip()] = float(parts[1])
        except ValueError:
            continue
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
        "mtimes": mtimes,
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
            "mtime": parsed["mtimes"].get(name, 0.0) if name else 0.0,
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
