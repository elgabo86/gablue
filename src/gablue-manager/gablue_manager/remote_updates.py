"""Vérification des mises à jour d'un poste distant, **à la demande**.

Le poste local est sondé par l'app elle-même (`updates.check_all`) ; un poste
distant ne l'est que si l'utilisateur clique « Vérifier les mises à jour » sur
sa carte — aucune sonde périodique ne part sur le réseau. Une seule connexion
SSH exécute un script qui imprime des sections balisées (`::gablue:<nom>`),
l'analyse est faite ici avec les mêmes règles que pour le poste local.

Les domaines vérifiés sont ceux qu'on peut mettre à jour à distance depuis le
manager : image Gablue, Flatpaks, Homebrew, composants gwine et applications
(AppImages). Les états d'AppImages voyagent dans la même connexion (sections
`##nom` du dialogue « Applications », précédées de `::gablue:apps`) ; les flux
de releases, eux, se lisent **d'ici** : le poste n'a besoin d'aucun accès
réseau sortant pour être déclaré à jour.
"""

import re
import shlex
import time

from . import apps, updates
from .ssh import ssh_capture

# Une seule connexion : chaque section est balisée, son code de retour est
# écrit en clair (`rc=`) pour ne pas créer une fausse section. Les marqueurs
# sont émis par `printf '\n…'` : `rpm-ostree status --json` ne finit **pas**
# par un saut de ligne, et un `echo` collait `::gablue:image-check` au `}` du
# JSON — la section disparaissait (image « vérification impossible » sur tous
# les postes, et `json.loads` raté donc plus de version ni d'âge, vécu
# 09/11/2026), le `rc=` suivant collé pareil.
CHECK_SCRIPT = r"""
printf '\n::gablue:image\n'
rpm-ostree status --json 2>/dev/null || echo '{}'
printf '\n::gablue:image-check\n'
sudo -n /usr/bin/bootc upgrade --check 2>&1
printf '\nrc=%s\n' "$?"
printf '\n::gablue:flatpak\n'
flatpak remote-ls --updates --columns=application 2>/dev/null || true
printf '\n::gablue:brew\n'
if command -v brew >/dev/null 2>&1; then
    HOMEBREW_NO_AUTO_UPDATE=1 brew outdated --quiet 2>/dev/null || true
else
    echo 'nobrew'
fi
printf '\n::gablue:gwine\n'
if command -v gwine >/dev/null 2>&1; then
    timeout 180 gwine --init --check 2>&1
    printf '\nrc=%s\n' "$?"
else
    echo 'rc=127'
fi
"""

def _echo(text: str) -> str:
    """Ligne lisible du journal distant (`echo` sans risque de quoting)."""
    return "echo " + shlex.quote(text)


# Les applications ne peuvent pas être décidées par le poste : leur liste vient
# de `probe_apps` (flux de releases lus d'ici) et leurs commandes sont celles
# du poste, lues par `apps.remote_states`. Aucun marqueur `::gablue:` ici : un
# `RemoteTaskDialog` n'a pas d'analyseur, la fenêtre montre le journal.
def apps_section(pending_apps=(), states=None) -> str:
    """Lignes bash qui mettent à jour les applications d'un poste distant.

    Une application dont le script manque, ou dont le script ne connaît pas
    l'option attendue (`--yes` sur une image antérieure : il ouvrirait ses
    boîtes kdialog et la tâche attendrait indéfiniment), est ignorée avec un
    message dans le journal — jamais lancée en interactif.
    """
    states = states or {}
    wanted = set(pending_apps or ())
    lines: list[str] = []
    for app in apps.APPS:
        if app.key not in wanted:
            continue
        current = states.get(app.key) or {}
        if not current.get("script"):
            lines.append(_echo(f"{app.name} : script manquant, ignoré"))
            continue
        options = current.get("options") or ()
        if app.install_args and app.install_args[0] not in options:
            lines.append(_echo(
                f"{app.name} : image du poste antérieure à "
                f"{app.install_args[0]}, ignoré"
            ))
            continue
        lines.append(_echo(f"Mise à jour de {app.name}…"))
        lines.append(apps.install_command(app, current) + " || rc=1")
    return "\n".join(lines)


def update_chain(pending_apps=(), states=None) -> str:
    """Script « tout mettre à jour » d'un poste distant.

    L'image d'abord (elle peut apporter un nouveau gwine), puis Flatpaks,
    gwine, applications et Homebrew — le même ordre que sur le poste local.
    Chaque étape tente de s'exécuter même si la précédente échoue (`sudo -n`
    manquant sur un poste hors image) et le code de sortie reste non nul : la
    fenêtre du lot affiche l'échec du poste sans interrompre les autres.

    `pending_apps` et `states` viennent de `probe_apps` : vide, aucune section
    applications (le poste est à jour sur ce domaine).
    """
    section = apps_section(pending_apps, states)
    apps_block = f'echo "== Applications =="\n{section}\n' if section else ""
    return (
        "rc=0\n"
        "sudo -n /usr/bin/bootc upgrade || rc=1\n"
        "sudo -n /usr/bin/flatpak update --system -y || rc=1\n"
        "if command -v gwine >/dev/null 2>&1; then\n"
        "    gwine --init || rc=1\n"
        "fi\n"
        + apps_block +
        "if command -v brew >/dev/null 2>&1; then\n"
        "    HOMEBREW_NO_AUTO_UPDATE=1 brew update || rc=1\n"
        "    brew upgrade || rc=1\n"
        "fi\n"
        "exit $rc\n"
    )


def apps_chain(pending_apps=(), states=None) -> str:
    """Script des applications seules (entrée « Applications… » d'un poste)."""
    section = apps_section(pending_apps, states)
    if not section:
        return ""
    return "rc=0\n" + section + "\nexit $rc\n"


def probe_apps(host: dict, timeout: float = 90.0) -> tuple:
    """AppImages d'un poste distant : une connexion SSH + les flux de releases.

    Rend `(clés à mettre à jour, doute, installées, états)` — les états servent
    à construire les commandes (`install_command` a besoin des options que le
    script du poste connaît). Poste injoignable → `([], True, 0, {})`, jamais
    « tout est à jour ».
    """
    code, text = ssh_capture(host, apps.REMOTE_STATE_SCRIPT, timeout=timeout)
    if code != 0 or "##gearlever" not in (text or ""):
        return [], True, 0, {}
    states, _gearlever = apps.remote_states(text)
    pending, unknown, installed = apps.pending_for(states)
    return pending, unknown, installed, states


# État des AppImages greffé sur la sonde : ses sections `##nom` sont précédées
# d'un marqueur `::gablue:apps`, sinon elles atterrissent dans la dernière
# section `::gablue:` (le gwine) et faussent son analyse.
APPS_STATE_SCRIPT = "echo '::gablue:apps'\n" + apps.REMOTE_STATE_SCRIPT

MARKER = "::gablue:"
_RC_RE = re.compile(r"^rc=(\d+)\s*$", re.MULTILINE)


def sections(text: str) -> dict[str, list[str]]:
    """Découpe `::gablue:<nom>` en sections ; les autres lignes suivent."""
    sections: dict[str, list[str]] = {}
    current = ""
    for line in (text or "").splitlines():
        if line.startswith(MARKER):
            current = line[len(MARKER):].strip()
            sections.setdefault(current, [])
            continue
        if current:
            sections[current].append(line)
    return sections


def _split_rc(lines: list[str]) -> tuple[int, str]:
    """Retire la dernière ligne `rc=N` d'une section → (code, reste)."""
    text = "\n".join(lines)
    matches = list(_RC_RE.finditer(text))
    if not matches:
        return 0, text
    last = matches[-1]
    return int(last.group(1)), (text[:last.start()] + text[last.end():]).strip()


def parse_check(text: str, now: float | None = None) -> dict:
    """Sortie du script → état au même format que le poste local."""
    split = sections(text)
    domains: dict[str, dict] = {}

    deployment = updates.parse_deployment_age(
        "\n".join(split.get("image", []))
    )
    image_rc, image_text = _split_rc(split.get("image-check", []))
    verdict = updates.parse_bootc_check(image_text, image_rc)
    if deployment.get("staged"):
        # Une image déjà téléchargée se voit sans réseau ni privilège : le
        # redémarrage reste à faire, rien à télécharger.
        domains["image"] = {
            "state": "available",
            "staged": True,
            "staged_version": deployment.get("staged_version", ""),
            "version": deployment.get("version", ""),
            "days": deployment.get("days"),
        }
    elif verdict == "available":
        domains["image"] = {
            "state": "available",
            "staged": False,
            "version": deployment.get("version", ""),
            "available_version": updates.parse_bootc_version(image_text),
            "days": deployment.get("days"),
        }
    elif verdict == "uptodate":
        domains["image"] = {
            "state": "uptodate",
            "days": deployment.get("days"),
        }
    else:
        domains["image"] = {"state": "unknown"}

    flatpaks = updates.parse_flatpak_updates(
        "\n".join(split.get("flatpak", []))
    )
    domains["flatpak"] = {
        "state": "updates" if flatpaks else "uptodate", "apps": flatpaks,
    }

    brew_lines = split.get("brew", [])
    if any(line.strip() == "nobrew" for line in brew_lines):
        domains["brew"] = {"state": "unavailable", "formulas": []}
    else:
        formulas = updates.parse_brew_outdated("\n".join(brew_lines))
        domains["brew"] = {
            "state": "updates" if formulas else "uptodate",
            "formulas": formulas,
        }

    gwine_rc, gwine_text = _split_rc(split.get("gwine", []))
    if gwine_rc == 127:
        domains["gwine"] = {"state": "unavailable", "components": []}
    else:
        parsed = updates.parse_gwine_check(gwine_text, gwine_rc)
        domains["gwine"] = parsed

    return {
        "checked_at": now if now is not None else time.time(),
        "online": True,
        "domains": domains,
    }


def check(host: dict, timeout: float = 240.0) -> dict:
    """Sonde un poste distant ; injoignable → état « hors ligne »."""
    code, text = ssh_capture(
        host, CHECK_SCRIPT + "\n" + APPS_STATE_SCRIPT, timeout=timeout,
    )
    if code != 0 or MARKER not in (text or ""):
        return {"checked_at": time.time(), "online": False, "domains": {}}
    state = parse_check(text)
    # Les applications demandent les flux de releases : lus d'ici, donc après
    # la connexion, sans rien exiger du réseau du poste.
    states, _gearlever = apps.remote_states(text)
    state["domains"]["apps"] = updates.apps_domain(
        *apps.pending_for(states)
    )
    return state
