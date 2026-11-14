"""Application des mises à jour locales : la séquence, pas les sondes.

`updates.py` dit **ce qui** est en retard ; ce module dit **comment l'appliquer**
— une étape par domaine, dans l'ordre des dépendances réelles — et il
n'importe pas Qt : la fenêtre principale et le mode CLI `--update` lancent
exactement la même liste, un domaine à la fois ou tous d'un trait.

Une étape est un dict `{label, argv, root?, fifo?, hint?, parser?, env?, busy?,
progress_text?}` : `tasks.SequentialTaskDialog` en lit une partie, le mode CLI
d'un seul domaine passe le reste à `tasks.LocalTaskDialog`. Aucun outil absent
n'est une erreur : son étape est simplement omise, sinon « tout mettre à jour »
échouerait sur chaque poste sans Homebrew ou sans flatpak.

L'ordre est celui de `updates.DOMAINS` : l'image d'abord (elle peut apporter un
nouveau gwine), puis Flatpak, gwine et applications, qui ne dépendent pas de
l'image mais prennent du temps ; Homebrew en dernier, parce que c'est l'étape la
plus longue et la plus fragile — un échec ne doit pas laisser les AppImages en
retard.
"""

import shutil
from pathlib import Path

from . import apps, local, updates
from .constants import UPDATE_HELPER, UPDATE_PROGRESS_FIFO, _which

# Domaines acceptés par `--update` : les cinq de `updates.DOMAINS` plus « tout ».
DOMAIN_CHOICES = ("all", *updates.DOMAINS)

# Libellés d'étape (la fenêtre de séquence les affiche « Étape n/N — … ») et
# en-têtes de la fenêtre d'un domaine seul, partagés avec la fenêtre principale.
STEP_LABELS = {
    "image": "Gablue (image système)",
    "flatpak": "Flatpaks",
    "gwine": "gwine (composants)",
    "apps": "Applications",
    "brew": "Homebrew",
}

TITLES = {
    "image": "Mise à jour de Gablue",
    "flatpak": "Mise à jour des Flatpaks",
    "gwine": "Mise à jour de gwine",
    "apps": "Mise à jour des applications",
    "brew": "Mise à jour de Homebrew",
    "all": "Tout mettre à jour",
}

SUBTITLES = {
    "image": "La nouvelle image s'applique au prochain redémarrage.",
    "flatpak": "Applications Flatpak système.",
    "gwine": "Runner Wine et composants (DXVK, VKD3D, D7VK).",
    "brew": "Formules installées par brew.",
}

def image_step() -> dict | None:
    """Image système (bootc), appliquée au prochain démarrage.

    Le helper dédié publie la progression JSONL de bootc dans un FIFO (sudo
    ferme les fd >= 3, l'app ne peut pas lui en passer un) ; sans lui, `bootc`
    est muet et la barre balaie. Il exige root dans les deux cas.
    """
    text = "Téléchargement de l'image système…"
    if Path(UPDATE_HELPER).is_file():
        return {
            "label": STEP_LABELS["image"],
            "argv": [UPDATE_HELPER],
            "root": True,
            "fifo": UPDATE_PROGRESS_FIFO,
            "hint": local.gablue_update_hint,
            "progress_text": text,
        }
    bootc = shutil.which("bootc")
    if not bootc:
        return None
    return {
        "label": STEP_LABELS["image"],
        "argv": [bootc, "upgrade"],
        "root": True,
        "hint": local.gablue_update_hint,
        "busy": True,          # sans helper, bootc n'annonce rien
        "progress_text": text,
    }

def flatpak_step() -> dict | None:
    """Flatpaks `--system` : hors terminal, flatpak écrit un pourcentage."""
    if not shutil.which("flatpak"):
        return None
    return {
        "label": STEP_LABELS["flatpak"],
        "argv": [_which("bash"), "-c", local.flatpak_update_script()],
        "hint": local.flatpak_update_hint,
        "parser": local.parse_flatpak_progress,
    }

def gwine_step() -> dict | None:
    """Runner Wine et composants (`gwine --init`).

    Le binaire vient de `local.gwine_binary()` : celui de l'image d'abord. Un
    gwine personnel antérieur dans `~/.local/bin` précède `/usr/bin` dans le
    PATH d'une session graphique, or c'est le composant de l'image que la mise
    à jour doit remplacer (même règle que la sonde `--init --check`).
    """
    binary = local.gwine_binary()
    if not (Path(binary).is_file() or shutil.which(binary)):
        return None
    return {
        "label": STEP_LABELS["gwine"],
        "argv": [binary, "--init"],
        "parser": local.parse_gablue_progress,
        "env": {"GABLUE_PROGRESS": "1"},
    }

def apps_step(pending_apps) -> dict | None:
    """AppImages en retard, une par une.

    La liste vient du réseau (`apps.pending_updates`, un flux de releases par
    application installée) : sans elle, il n'y a rien à faire. Un AppImage se
    télécharge d'un bloc — la barre balaie et le libellé nomme l'application en
    cours (i/N) plutôt que d'afficher une fraction inventée.
    """
    keys = [str(key) for key in (pending_apps or [])]
    if not keys:
        return None
    names = ", ".join(apps.names_for(keys))
    return {
        "label": f"{STEP_LABELS['apps']} ({names})",
        "argv": [_which("bash"), "-c", apps.update_script(keys)],
        "parser": local.parse_gablue_progress,
        "busy": True,
        "progress_text": "Mise à jour des applications…",
    }

def brew_step() -> dict | None:
    """Homebrew : `brew update` puis `brew upgrade`, sans privilège."""
    if not local.brew_available():
        return None
    return {
        "label": STEP_LABELS["brew"],
        "argv": [_which("bash"), "-c", local.brew_update_script()],
        "hint": local.brew_update_hint,
        "parser": local.brew_progress_parser(),
    }

_BUILDERS = {
    "image": image_step,
    "flatpak": flatpak_step,
    "gwine": gwine_step,
    "brew": brew_step,
}

def step_for(domain: str, pending_apps=()) -> dict | None:
    """Étape d'un domaine, ou None si l'outil est absent / rien à faire."""
    if domain == "apps":
        return apps_step(pending_apps)
    builder = _BUILDERS.get(domain)
    return builder() if builder is not None else None

def steps(pending_apps=None) -> list[dict]:
    """Toutes les étapes possibles sur ce poste, dans l'ordre des dépendances."""
    built: list[dict] = []
    for domain in updates.DOMAINS:
        step = step_for(domain, pending_apps)
        if step is not None:
            built.append(step)
    return built

def needs_apps_probe(domain: str) -> bool:
    """Ce domaine exige-t-il la sonde réseau des AppImages avant de partir ?

    Les autres domaines n'ont besoin d'aucune liste : leurs étapes ne dépendent
    que des outils présents. L'état `applications` des préférences ne garde que
    des **noms**, pas les clés que `apps.update_script` attend — d'où la sonde.
    """
    return domain in ("all", "apps")

def local_task_kwargs(step: dict) -> dict:
    """Étape → arguments de `tasks.LocalTaskDialog` (un seul domaine).

    Une étape porte ce que la séquence sait lire ; la fenêtre d'un domaine seul
    attend les mêmes informations sous d'autres noms (`hint_from_output`,
    `progress_fifo`, `env_extra`) plus un mode de barre. `busy` devient
    « balayage » : une étape sans mesure ne doit pas afficher 0 % figé.
    """
    return {
        "argv": list(step["argv"]),
        "root": bool(step.get("root")),
        "hint_from_output": step.get("hint"),
        "progress_fifo": step.get("fifo") or "",
        "progress_parser": step.get("parser"),
        "env_extra": step.get("env"),
        "progress_mode": "busy" if step.get("busy") else "auto",
        "progress_text": step.get("progress_text") or "",
    }
