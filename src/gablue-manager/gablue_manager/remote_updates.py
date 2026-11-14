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
from .local import (
    GABLUE_PROGRESS_PREFIX, brew_progress_parser, parse_flatpak_progress,
    parse_gablue_progress, parse_progress_event,
)
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

    Progression : `::gablue:step i/N` compte les applications **réellement
    lancées** (les ignorées ne font pas avancer la barre pour rien), et le
    moteur d'AppImages publie ensuite son propre `percent` — même protocole
    qu'en local (`apps.update_script`), donc rien de neuf côté poste.
    """
    states = states or {}
    wanted = set(pending_apps or ())
    skipped: list[str] = []
    runnable: list[tuple] = []
    for app in apps.APPS:
        if app.key not in wanted:
            continue
        current = states.get(app.key) or {}
        if not current.get("script"):
            skipped.append(_echo(f"{app.name} : script manquant, ignoré"))
            continue
        options = current.get("options") or ()
        if app.install_args and app.install_args[0] not in options:
            skipped.append(_echo(
                f"{app.name} : image du poste antérieure à "
                f"{app.install_args[0]}, ignoré"
            ))
            continue
        runnable.append((app, current))

    lines: list[str] = list(skipped)
    total = len(runnable)
    for index, (app, current) in enumerate(runnable, start=1):
        lines.append(_marker(f"::gablue:step {index} {total} {app.name}"))
        lines.append(_marker(
            f"::gablue:busy Mise à jour de {app.name} ({index}/{total})…"
        ))
        lines.append(_echo(f"Mise à jour de {app.name}…"))
        lines.append(apps.install_command(app, current) + " || rc=1")
    return "\n".join(lines)


# Étapes de la chaîne, dans l'ordre où le poste les exécute. Le nombre est fixe
# (une étape sautée s'annonce quand même) pour que la barre d'un poste sache où
# elle en est sans deviner ce que le poste a d'installé.
STAGES = (
    "Image Gablue",
    "Flatpaks",
    "gwine",
    "Applications",
    "Homebrew",
)

STEP_MARKER = "::gablue:stage "
PROGRESS_MARKER = "::gablue:progress "

def _marker(text: str) -> str:
    """Ligne de protocole sur la sortie du poste.

    `printf '\\n%s\\n'` avec un argument `shlex.quote` : le saut de ligne
    d'ouverture empêche le collage à une sortie qui n'a pas fini par `\\n`, et
    le format à trous évite qu'un libellé (nom d'application) casse le quoting.
    """
    return "printf '\\n%s\\n' " + shlex.quote(text)

def _stage(index: int, total: int, label: str) -> str:
    """Annonce d'une étape de la chaîne.

    Verbe `stage` et non `step` : les scripts d'installation d'AppImages
    publient déjà `::gablue:step n total` pour leur **propre** progression, et
    deux numérotations sur un même flux se marcheraient dessus — la
    sous-progression d'une application écraserait le compte des étapes du poste.
    """
    return _marker(f"::gablue:stage {index} {total} {label}")


IMAGE_UPGRADE = (
    # Progression réelle de l'étape qui dure : `bootc upgrade --progress-fd`
    # refuse les fd 1 et 2, et sudo ferme les fd ≥ 3 par défaut (`-C 99` serait
    # refusé sans `closefrom_override`, vérifié le 14/11/2026). On réutilise donc
    # le helper de l'image, déjà NOPASSWD et sans argument : il publie le JSONL
    # de bootc dans un FIFO, et un lecteur de la **même** session ssh l'étiquette
    # en `::gablue:progress`. Bonus : ses traps tuent bootc quand le ssh tombe
    # (annulation du lot, lien coupé), ce que `sudo bootc` seul ne fait pas.
    # Sans le helper (image antérieure), repli muet mais fonctionnel.
    "if [ -x /usr/libexec/gablue-update ]; then\n"
    "    sudo -n /usr/libexec/gablue-update &\n"
    "    helper=$!\n"
    "    while [ ! -p /run/gablue/update-progress.fifo ]"
    " && kill -0 \"$helper\" 2>/dev/null; do sleep 0.1; done\n"
    "    while IFS= read -r line; do"
    " printf '::gablue:progress %s\\n' \"$line\";"
    " done 2>/dev/null < /run/gablue/update-progress.fifo &\n"
    "    reader=$!\n"
    "    wait \"$helper\"\n"
    "    helper_rc=$?\n"
    "    kill \"$reader\" 2>/dev/null || true\n"
    "    case \"$helper_rc\" in 0) ;; *) rc=1 ;; esac\n"
    "else\n"
    "    sudo -n /usr/bin/bootc upgrade || rc=1\n"
    "fi"
)

FLATPAK_UPGRADE = "sudo -n /usr/bin/flatpak update --system -y || rc=1"

GWINE_UPGRADE = (
    # `GABLUE_PROGRESS=1` : gwine publie alors ses `::gablue:step|busy|done`
    # en plus de ses lignes lisibles (lib/ui.sh), les mêmes marqueurs qu'en
    # local. Effet de bord utile : `run_with_progress` n'ouvre plus de fenêtre sur
    # le poste distant, il exécute directement.
    "if command -v gwine >/dev/null 2>&1; then\n"
    "    GABLUE_PROGRESS=1 gwine --init || rc=1\n"
    "fi"
)

BREW_UPGRADE = (
    "if command -v brew >/dev/null 2>&1; then\n"
    "    HOMEBREW_NO_AUTO_UPDATE=1 brew update || rc=1\n"
    "    brew upgrade || rc=1\n"
    "fi"
)

def update_chain(pending_apps=(), states=None) -> str:
    """Script « tout mettre à jour » d'un poste distant.

    L'image d'abord (elle peut apporter un nouveau gwine), puis Flatpaks,
    gwine, applications et Homebrew — le même ordre que sur le poste local.
    Chaque étape tente de s'exécuter même si la précédente échoue (`sudo -n`
    manquant sur un poste hors image) et le code de sortie reste non nul : la
    fenêtre du lot affiche l'échec du poste sans interrompre les autres.

    **Progression** : chaque étape s'annonce (`::gablue:stage n 5 libellé`),
    même celles qui seront sautées (`command -v`), pour que le nombre d'étapes
    soit fixe et que la barre du poste ne mente pas. L'image, elle, publie en
    plus ses événements JSONL (`::gablue:progress {…}`), seule mesure réelle
    d'une étape qui dure un quart d'heure — voir `IMAGE_UPGRADE`. Ces lignes
    sont **lues et retirées du journal** par `HostProgress`.

    `pending_apps` et `states` viennent de `probe_apps` : vide, aucune section
    applications (le poste est à jour sur ce domaine).
    """
    section = apps_section(pending_apps, states)
    total = len(STAGES)
    apps_block = (
        f"echo '== Applications =='\n{section}"
        if section else "echo 'Applications : à jour'"
    )
    lines = ["rc=0", _stage(1, total, STAGES[0]), IMAGE_UPGRADE]
    lines += [_stage(2, total, STAGES[1]), FLATPAK_UPGRADE]
    lines += [_stage(3, total, STAGES[2]), GWINE_UPGRADE]
    lines += [_stage(4, total, STAGES[3]), apps_block]
    lines += [_stage(5, total, STAGES[4]), BREW_UPGRADE, "exit $rc"]
    return "\n".join(lines) + "\n"


def apps_chain(pending_apps=(), states=None) -> str:
    """Script des applications seules (entrée « Applications… » d'un poste)."""
    section = apps_section(pending_apps, states)
    if not section:
        return ""
    return "rc=0\n" + section + "\nexit $rc\n"


def image_chain() -> str:
    """Étape image seule : action « Mettre à jour Gablue » d'un poste.

    Même bloc que dans `update_chain` (progression JSONL comprise), suivi de
    `bootc status` : `gablue_update_hint` lit le compte rendu d'upgrade, et la
    version amorcée n'apparaît que dans le status. Sans marqueur d'étape, la
    fenêtre n'avait ni barre ni libellé pendant le tirage de l'image.
    """
    return "\n".join([
        _stage(1, 1, STAGES[0]),
        "rc=0",
        IMAGE_UPGRADE,
        "echo '---STATUS---'",
        "sudo -n /usr/bin/bootc status 2>/dev/null || true",
        "exit $rc",
    ]) + "\n"


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


class HostProgress:
    """Progression d'un poste du lot, découpée depuis son flux ssh.

    Le poste annonce ses étapes (`::gablue:stage n total libellé`) et, selon
    l'étape, mesure ce qu'il fait :

    - **image** : JSONL de `bootc --progress-fd`, étiqueté `::gablue:progress` ;
    - **gwine** : ses propres `::gablue:step|busy|done` (`GABLUE_PROGRESS=1`) ;
    - **applications** : `::gablue:step|busy` de la section, puis le `percent`
      du moteur d'AppImages ;
    - **Flatpaks** et **Homebrew** : aucune marque ni effort côté poste, mais
      leur sortie lisible porte de quoi mesurer — `local.parse_flatpak_progress`
      (`Updating 3/7… 45 %`) et `local.brew_progress_parser` (`Upgrading x` sur
      le total annoncé), les mêmes analyseurs qu'en local.

    Tout le reste repart au journal, et les lignes de protocole en sont
    retirées : le journal reste du texte pour l'humain. Une ligne incomplète est
    gardée pour le morceau suivant — le ssh découpe ses lectures n'importe où.

    `percent` ne rend **rien** tant que l'étape courante n'a pas de mesure
    (téléchargement d'une AppImage, `brew` qui ne publie aucun pourcentage) :
    la fenêtre montre alors une barre qui balaie au lieu d'un pourcentage figé.
    `fraction`, elle, est toujours un nombre — c'est ce que consomme la barre
    **globale** du lot.
    """

    # Étapes dont la sortie lisible suffit à mesurer (aucun marqueur requis).
    LINE_STAGES = (STAGES[1], STAGES[4])

    def __init__(self) -> None:
        self._buffer = ""
        self._brew = brew_progress_parser()      # état par poste
        self.step = 0
        self.total = 0
        self.stage = ""
        self.detail = ""
        self.intra: float | None = None
        self.finished = False

    # -- flux ---------------------------------------------------------------
    def feed(self, text: str) -> str:
        """Consomme un morceau de sortie ; rend le texte à écrire au journal."""
        self._buffer += text or ""
        kept: list[str] = []
        while "\n" in self._buffer:
            line, _, self._buffer = self._buffer.partition("\n")
            if self._consume(line):
                continue
            self._measure(line)
            kept.append(line + "\n")
        return "".join(kept)

    def flush(self) -> str:
        """Reste de la dernière ligne (commande finie sans saut de ligne)."""
        line, self._buffer = self._buffer, ""
        if self._consume(line):
            return ""
        self._measure(line)
        return line

    def _consume(self, line: str) -> bool:
        """Absorbe une ligne de protocole ; faux si elle va au journal."""
        text = (line or "").strip()
        if not text.startswith(GABLUE_PROGRESS_PREFIX):
            return False
        if text.startswith(STEP_MARKER):
            parts = text[len(STEP_MARKER):].split(" ", 2)
            if len(parts) == 3:
                try:
                    self.step = max(1, int(parts[0]))
                    self.total = max(1, int(parts[1]))
                except ValueError:
                    return True
                self.stage = parts[2].strip()
                self.detail = ""
                self.intra = None
            return True
        if text.startswith(PROGRESS_MARKER):
            event = parse_progress_event(text[len(PROGRESS_MARKER):])
            if event:
                self.intra = event.get("percent")
                self.detail = str(event.get("label") or "")
            return True
        # Sous-progression d'une étape (moteur d'AppImages, gwine) : mêmes
        # verbes que le protocole local, donc le même analyseur.
        info = parse_gablue_progress(text)
        if info:
            self.intra = info.get("percent")
            self.detail = str(info.get("label") or "")
        return True

    def _measure(self, line: str) -> None:
        """Mesure une ligne lisible de l'étape courante (elle reste au journal)."""
        if self.stage not in self.LINE_STAGES:
            return
        parser = (
            parse_flatpak_progress if self.stage == STAGES[1] else self._brew
        )
        info = parser((line or "").rstrip())
        if not info:
            return
        self.intra = info.get("percent")
        self.detail = str(info.get("label") or "")

    # -- affichage ----------------------------------------------------------
    @property
    def percent(self) -> float | None:
        """Avancement global mesuré (0…100), None : barre indéterminée."""
        if self.finished:
            return 100.0
        if self.intra is None or not self.total or not self.step:
            return None
        return 100.0 * self.fraction

    @property
    def fraction(self) -> float:
        """Avancement 0…1 sur la chaîne entière, même sans mesure d'étape."""
        if self.finished:
            return 1.0
        if not self.total or not self.step:
            return 0.0
        part = 0.0
        if self.intra is not None:
            part = max(0.0, min(1.0, self.intra / 100.0))
        return max(0.0, min(1.0, (self.step - 1 + part) / self.total))

    def label(self) -> str:
        """« Image Gablue · Téléchargement de l'image » (vide avant l'étape 1)."""
        if not self.stage:
            return ""
        return " · ".join(part for part in (self.stage, self.detail) if part)
