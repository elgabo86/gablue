"""État de l'icône de barre d'état, calculé hors GUI (`--tray-state`).

L'icône elle-même est un binaire Rust (`gablue-manager-tray`, StatusNotifierItem
par DBus, ~5 Mo) : elle ne charge ni Qt ni ce paquet. Tout ce qui demande de la
logique — raisons de redémarrage, sondes de mises à jour, phrases de
l'infobulle, du menu et de la notification — est calculé **ici**, dans un
processus Python éphémère, et rendu en JSON :

- l'icône appelle `--tray-state` toutes les 5 min (aucune sonde réseau, juste
  l'état déjà connu) et `--tray-state --check` à la fréquence des préférences
  (2 h par défaut, 24 h pour l'image), ou `--force` quand l'utilisateur clique
  sur « Vérifier les mises à jour » ;
- l'état complet (`state`) sort dans le JSON : c'est l'icône qui le range dans
  `tray-state.json`, un seul écrivain, donc pas de course entre deux processus ;
- une `notification` éventuelle est rendue par `updates.notification_text`,
  donc l'icône n'a aucune règle de politesse à réimplémenter.

Aucun import Qt : ce module doit rester appelable sans affichage et sortir en
quelques centaines de millisecondes.
"""

import json
import sys
import time
from pathlib import Path

from . import prefs, updates
from .local import pending_reboot_reasons

USAGE = """Usage : gablue-manager --tray-state --state FICHIER [--check] [--force] [--offline]

  --state FICHIER  état précédent, tenu par l'icône (lecture seule ici)
  --check          sonde les mises à jour (réseau) avant de rendre l'état
  --force          sonde même si la surveillance est coupée ou pas due
  --offline        pas de réseau : aucune sonde, l'état connu est conservé

Sortie : un objet JSON sur la sortie standard (voir `traystate.build`).
"""


# --- présentation de l'état ------------------------------------------------
#
# L'infobulle et le menu montrent les mêmes informations, mais l'infobulle
# n'accepte que du texte : lignes courtes, une par élément. Les raisons de
# redémarrage arrivent en minuscules depuis les sondes ; on les met en phrase
# et on supprime le doublon avec la ligne « Image Gablue » (l'image staged y
# est déjà décrite avec sa version).

def _sentence(text: str) -> str:
    text = (text or "").strip()
    return text[:1].upper() + text[1:] if text else ""


def tray_lines(events: list[str], state: dict | None,
               offline: bool = False) -> tuple[list[str], list[str]]:
    """(raisons de redémarrage, lignes de mises à jour) pour l'icône."""
    staged = updates.staged_image(state)
    version = str(staged.get("staged_version") or staged.get("version") or "")
    alerts: list[str] = []
    if staged:
        text = "Image Gablue"
        if version:
            text += f" — version {version}"
        alerts.append(text + " téléchargée")
    for reason in events:
        text = _sentence(reason)
        if not text:
            continue
        if staged and text.lower().startswith("nouvelle image"):
            continue
        alerts.append(text)
    domains: list[str] = []
    for line in updates.summary_lines(state):
        if staged and line.startswith("Image Gablue"):
            continue
        domains.append(line.replace(" : ", " — ", 1))
    if offline and not any("hors ligne" in line.lower() for line in domains):
        domains.append("Hors ligne — vérifications en pause")
    if not domains and state is not None:
        text = updates.compact_summary(state)
        if text:
            domains.append(text)
    return alerts, domains


# --- état tenu par l'icône --------------------------------------------------

def read_state(path: str | None) -> dict | None:
    """État précédent, écrit par l'icône ; None s'il manque ou est illisible."""
    if not path:
        return None
    try:
        document = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    if not isinstance(document, dict) or not isinstance(document.get("domains"), dict):
        return None
    return document


def _deferred(values: dict, previous: dict | None) -> set:
    """L'image a sa propre cadence (24 h) : inutile de sonder le registre à
    chaque passe des autres domaines."""
    if not previous:
        return set()
    last = float(previous.get("image_checked_at") or 0)
    if last and time.time() - last < prefs.image_interval_seconds(values):
        return {"image"}
    return set()


def _notification(state: dict | None, previous: dict | None,
                  manual: bool) -> dict | None:
    message = updates.notification_text(state, previous, manual)
    if not message:
        return None
    title, body = message
    return {"title": title, "body": body}


def probe(values: dict, previous: dict | None, force: bool,
          offline: bool) -> tuple[dict | None, dict | None]:
    """Sonde les mises à jour ; (état, notification). Aucun réseau si `offline`."""
    if not force and not prefs.checks_enabled(values):
        return previous, None
    if offline:
        # Rien n'est sondé hors ligne. On garde l'état connu — une image déjà
        # téléchargée reste à redémarrer — et on ne le date pas : le retour du
        # réseau re-sonde tout de suite.
        state = dict(previous or {"online": False, "domains": {}}, online=False)
        return state, _notification(state, previous, manual=force)
    domains = prefs.check_domains(values) if force \
        else prefs.auto_check_domains(values)
    deferred = set() if force else _deferred(values, previous)
    fresh = updates.check_all(domains, True, deferred=deferred)
    state = updates.merge_states(previous, fresh) or fresh
    if "image" in deferred and previous:
        if previous.get("image_checked_at"):
            state["image_checked_at"] = previous["image_checked_at"]
    else:
        state["image_checked_at"] = state.get("checked_at") or time.time()
    return state, _notification(state, previous, manual=force)


def build(state_path: str | None = None, check: bool = False,
          force: bool = False, offline: bool = False) -> dict:
    """Document JSON complet : ce que l'icône affiche, plus l'état à ranger."""
    values = prefs.load_prefs()
    previous = read_state(state_path)
    state = previous
    notification = None
    if check:
        state, notification = probe(values, previous, force, offline)
    # Surveillance coupée : les anciennes lignes ne doivent plus s'afficher
    # (une mise à jour trouvée il y a deux jours n'est plus « disponible »).
    shown = state if (force or prefs.checks_enabled(values)) else None
    alerts, domains = tray_lines(pending_reboot_reasons(), shown, offline)
    tooltip = ["Gablue Manager"]
    if alerts:
        tooltip.append("Redémarrage requis")
        tooltip.extend("• " + text for text in alerts)
    tooltip.extend("• " + text for text in domains)
    return {
        "tray_enabled": bool(values["tray"]),
        "checks_enabled": bool(prefs.checks_enabled(values)),
        "interval": prefs.check_interval_seconds(values)
        if prefs.checks_enabled(values) else 0,
        "checked_at": float((state or {}).get("checked_at") or 0),
        "offline": bool(offline),
        "alert": bool(alerts) or updates.has_updates(shown),
        "alerts": alerts,
        "domains": domains,
        "tooltip": tooltip,
        "state": state if check else None,
        "notification": notification,
    }


def main(argv: list[str]) -> int:
    state_path = None
    check = force = offline = False
    index = 0
    while index < len(argv):
        argument = argv[index]
        if argument == "--state" and index + 1 < len(argv):
            state_path = argv[index + 1]
            index += 2
            continue
        if argument == "--check":
            check = True
        elif argument == "--force":
            check = force = True
        elif argument == "--offline":
            offline = True
        elif argument in ("-h", "--help"):
            print(USAGE)
            return 0
        else:
            print(f"gablue-manager : option inconnue « {argument} »", file=sys.stderr)
            print(USAGE, file=sys.stderr)
            return 2
        index += 1
    document = build(state_path, check=check, force=force, offline=offline)
    print(json.dumps(document, ensure_ascii=False))
    return 0
