"""Vérification des mises à jour d'un poste distant, **à la demande**.

Le poste local est sondé par l'app elle-même (`updates.check_all`) ; un poste
distant ne l'est que si l'utilisateur clique « Vérifier les mises à jour » sur
sa carte — aucune sonde périodique ne part sur le réseau. Une seule connexion
SSH exécute un script qui imprime des sections balisées (`::gablue:<nom>`),
l'analyse est faite ici avec les mêmes règles que pour le poste local.

Les domaines vérifiés sont ceux qu'on peut mettre à jour à distance depuis le
manager : image Gablue, Flatpaks, Homebrew, composants gwine. Les AppImages du
poste distant ne sont pas sondées (pas d'entrée de mise à jour pour elles).
"""

import re
import time

from . import updates
from .ssh import ssh_capture

# Une seule connexion : chaque section est balisée, son code de retour est
# écrit en clair (`rc=`) pour ne pas créer une fausse section.
CHECK_SCRIPT = r"""
echo '::gablue:image'
rpm-ostree status --json 2>/dev/null || echo '{}'
echo '::gablue:image-check'
sudo -n /usr/bin/bootc upgrade --check 2>&1
echo "rc=$?"
echo '::gablue:flatpak'
flatpak remote-ls --updates --columns=application 2>/dev/null || true
echo '::gablue:brew'
if command -v brew >/dev/null 2>&1; then
    HOMEBREW_NO_AUTO_UPDATE=1 brew outdated --quiet 2>/dev/null || true
else
    echo 'nobrew'
fi
echo '::gablue:gwine'
if command -v gwine >/dev/null 2>&1; then
    timeout 180 gwine --init --check 2>&1
    echo "rc=$?"
else
    echo 'rc=127'
fi
"""

# « Tout mettre à jour » sur un poste distant : l'image d'abord (elle peut
# apporter un nouveau gwine), puis Flatpaks, gwine et Homebrew — le même ordre
# que sur le poste local. Chaque étape tente de s'exécuter même si la
# précédente échoue (`sudo -n` manquant sur un poste hors image), et le code de
# sortie reste non nul : la fenêtre du lot affiche l'échec du poste sans
# interrompre les autres.
UPDATE_CHAIN = r"""
rc=0
sudo -n /usr/bin/bootc upgrade || rc=1
sudo -n /usr/bin/flatpak update --system -y || rc=1
if command -v gwine >/dev/null 2>&1; then
    gwine --init || rc=1
fi
if command -v brew >/dev/null 2>&1; then
    HOMEBREW_NO_AUTO_UPDATE=1 brew update || rc=1
    brew upgrade || rc=1
fi
exit $rc
"""

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
    code, text = ssh_capture(host, CHECK_SCRIPT, timeout=timeout)
    if code != 0 or MARKER not in (text or ""):
        return {"checked_at": time.time(), "online": False, "domains": {}}
    return parse_check(text)
