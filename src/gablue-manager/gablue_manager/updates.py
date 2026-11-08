"""Mises à jour disponibles : image Gablue, Flatpaks, Homebrew, gwine.

Aucune de ces sondes n'installe quoi que ce soit : elles lisent un état et
laissent les commandes de mise à jour (`local.*_update_script`) faire le
travail. Trois d'entre elles touchent le réseau (image, Flatpaks, gwine) :
l'appelant annonce `online=False` quand la machine n'a pas de connexion, et
elles sont alors simplement sautées — la barre d'état ne réveille rien pour
rien.

Le module n'importe pas Qt : tout est appelable depuis un thread de fond
(`workers.UpdateCheckWorker`) et testable sans affichage.
"""

import json
import os
import re
import shutil
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor

from . import apps, journal, local
from .constants import BREW_BINARY

DOMAINS = ("image", "flatpak", "brew", "gwine", "apps")

DOMAIN_LABELS = {
    "image": "Image Gablue",
    "flatpak": "Flatpaks",
    "brew": "Homebrew",
    "gwine": "Composants gwine",
    "apps": "Applications",
}

GWINE_LABELS = {
    "runner": "Runner gwine",
    "dxvk": "DXVK",
    "vkd3d": "VKD3D-Proton",
    "d7vk": "D7VK",
    "nvapi": "DXVK-NVAPI",
    "cache": "Composants manquants",
}

# Commandes en cours : à la fermeture de la fenêtre on les tue, sinon un
# `brew update` de deux minutes retient le processus après la fermeture
# (vécu 07/11/2026) — le pool de threads attend ses tâches à la sortie.
_ACTIVE: set = set()
_ACTIVE_LOCK = threading.Lock()


def cancel_running() -> None:
    """Tue les sondes externes en cours (fermeture de la fenêtre)."""
    with _ACTIVE_LOCK:
        processes = list(_ACTIVE)
    for process in processes:
        try:
            process.kill()
        except OSError:
            pass


# Un domaine dont la sonde échoue n'est journalisé qu'une fois par processus :
# la passe revient toutes les deux heures, une entrée à chaque fois noierait
# le journal alors que la cause est la même (outil cassé, réseau, privilège).
_FAILED: dict[str, str] = {}
_FAILED_LOCK = threading.Lock()


def _note_failure(domain: str, detail: str) -> None:
    """Sonde de domaine en échec → journald, une fois par processus.

    Le panneau et le tray disent « vérification impossible » ; sans cette
    entrée, cette panne-là ne laissait aucune trace et se lisait comme un
    « à jour » de plus (vécu 08/11/2026).
    """
    with _FAILED_LOCK:
        if domain in _FAILED:
            return
        _FAILED[domain] = detail
    journal.log_failure(
        "Sonde de mises à jour impossible — "
        + DOMAIN_LABELS.get(domain, domain),
        detail=detail,
    )


def _run(argv: list[str], timeout: int,
         env: dict | None = None) -> tuple[int, str]:
    """Lance une sonde ; code 127 et sortie vide si elle n'existe pas."""
    try:
        process = subprocess.Popen(
            argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, env=env,
        )
    except OSError:
        return 127, ""
    with _ACTIVE_LOCK:
        _ACTIVE.add(process)
    try:
        output, error = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        process.kill()
        output, error = process.communicate()
        return 127, (output or "") + (error or "")
    finally:
        with _ACTIVE_LOCK:
            _ACTIVE.discard(process)
    return process.returncode, (output or "") + (error or "")

# -- image Gablue -----------------------------------------------------------

def parse_deployment_age(text: str, now: float | None = None) -> dict:
    """Âge du déploiement amorcé, version et présence d'une image en attente.

    `timestamp` d'un déploiement = moment où il a été déployé localement,
    c'est-à-dire la dernière fois que Gablue a été mis à jour sur ce poste
    (ou l'installation). Lisible sans privilège.
    """
    try:
        data = json.loads(text)
    except ValueError:
        return {}
    deployments = data.get("deployments") if isinstance(data, dict) else None
    if not isinstance(deployments, list):
        return {}
    booted = next(
        (item for item in deployments
         if isinstance(item, dict) and item.get("booted")),
        None,
    )
    if booted is None:
        return {}
    days = None
    timestamp = booted.get("timestamp")
    if isinstance(timestamp, (int, float)) and timestamp > 0:
        seconds = max(0.0, (now or time.time()) - timestamp)
        days = int(seconds // 86400)
    staged = next(
        (item for item in deployments
         if isinstance(item, dict) and item.get("staged")),
        None,
    )
    return {
        "days": days,
        "version": str(booted.get("version") or ""),
        "staged": staged is not None,
        "staged_version": str((staged or {}).get("version") or ""),
    }

def parse_bootc_check(text: str, returncode: int = 0) -> str:
    """Sortie de `bootc upgrade --check` → available | uptodate | unknown."""
    lowered = (text or "").lower()
    if "update available for:" in lowered:
        return "available"
    if "no update available" in lowered or "no changes in" in lowered:
        return "uptodate"
    return "unknown"

_BOOTC_VERSION_RE = re.compile(r"^\s*Version:\s*(\S+)\s*$", re.MULTILINE)

def parse_bootc_version(text: str) -> str:
    """Version de l'image distante annoncée par `bootc upgrade --check`.

    bootc écrit le bloc `Update available for: …` puis `Version:` (et
    `Digest:`) — vérifié le 08/11/2026 avec bootc 1.17.1. Une version
    antérieure qui n'en donne pas laisse la ligne du résumé sans cible.
    """
    match = _BOOTC_VERSION_RE.search(text or "")
    return match.group(1) if match else ""

def check_image(online: bool = True) -> dict:
    """Disponibilité d'une image plus récente + âge de celle qui tourne.

    Une image déjà téléchargée (`staged`) se voit sans réseau et sans
    privilège ; sinon `bootc upgrade --check` interroge le registre (métadonnées
    seulement, quelques kilo-octets) via la règle sudoers existante.
    """
    _, text = _run(["rpm-ostree", "status", "--json"], 20)
    info = parse_deployment_age(text)
    result = {
        "state": "unknown",
        "days": info.get("days"),
        "version": info.get("version", ""),
        "staged": bool(info.get("staged")),
        "staged_version": info.get("staged_version", ""),
    }
    if result["staged"]:
        result["state"] = "available"
        return result
    if not online:
        result["state"] = "offline"
        return result
    returncode, output = _run(
        ["sudo", "-n", "bootc", "upgrade", "--check"], 120,
    )
    result["state"] = parse_bootc_check(output, returncode)
    # Version de l'image **distante** quand bootc l'annonce : sans elle, le
    # résumé ne pouvait dire que « actuelle X » et laissait croire qu'aucune
    # version n'arrivait (vécu 08/11/2026).
    result["available_version"] = (
        parse_bootc_version(output) if result["state"] == "available" else ""
    )
    return result

# -- Flatpaks ---------------------------------------------------------------

def parse_flatpak_updates(text: str) -> list[str]:
    """`flatpak remote-ls --updates` → identifiants applicatifs triés."""
    apps = {
        line.strip() for line in (text or "").splitlines()
        if line.strip() and "." in line
    }
    return sorted(apps)

def check_flatpaks() -> dict:
    """Applications Flatpak système en retard.

    Gablue n'installe rien en `--user` : la sonde ne regarde que l'installation
    système, sinon des applications du compte compteraient comme « à mettre à
    jour » alors que le bouton ne les mettrait pas à jour.
    """
    returncode, text = _run(
        ["flatpak", "remote-ls", "--updates", "--columns=application"], 60,
    )
    if returncode != 0:
        _note_failure(
            "flatpak",
            f"flatpak remote-ls --updates — code {returncode}\n{text[-400:]}",
        )
        return {"state": "unknown", "apps": []}
    apps = parse_flatpak_updates(text)
    return {"state": "updates" if apps else "uptodate", "apps": apps}

# -- Homebrew ---------------------------------------------------------------

def parse_brew_outdated(text: str) -> list[str]:
    """`brew outdated --quiet` → noms de formules triés.

    Les avertissements de Homebrew arrivent sur stderr (fusionné) : on ne
    garde que les lignes qui ont la forme d'un nom de formule ou de cask.
    """
    names = set()
    for line in (text or "").splitlines():
        name = line.strip()
        if name and re.match(r"^[A-Za-z0-9@+_./-]+$", name):
            names.add(name)
    return sorted(names)

def check_brew() -> dict:
    """Formules Homebrew en retard (aucun privilège, données API en cache)."""
    if not local.brew_available():
        return {"state": "unavailable", "formulas": []}
    binary = str(BREW_BINARY) if BREW_BINARY.exists() else (
        shutil.which("brew") or ""
    )
    if not binary:
        return {"state": "unavailable", "formulas": []}
    env = dict(os.environ)
    # Pas de `brew update` implicite : la sonde doit rester courte, et les
    # données API en cache suffisent à savoir ce qui est en retard.
    env["HOMEBREW_NO_AUTO_UPDATE"] = "1"
    env["HOMEBREW_NO_ENV_HINTS"] = "1"
    returncode, text = _run([binary, "outdated", "--quiet"], 120, env)
    if returncode != 0:
        _note_failure(
            "brew",
            f"brew outdated — code {returncode}\n{text[-400:]}",
        )
        return {"state": "unknown", "formulas": []}
    formulas = parse_brew_outdated(text)
    return {
        "state": "updates" if formulas else "uptodate",
        "formulas": formulas,
    }

# -- gwine ------------------------------------------------------------------

def parse_gwine_check(text: str, returncode: int = 0) -> dict:
    """Sortie de `gwine --init --check` → composants en retard.

    gwine émet une ligne `component|clé|actuelle|disponible` par composant et
    sort 0 (à jour), 10 (mises à jour) ou 3 (indéterminé : réseau, version).
    """
    components: list[dict] = []
    for line in (text or "").splitlines():
        if not line.startswith("component|"):
            continue
        fields = line.split("|")
        if len(fields) < 2:
            continue
        components.append({
            "key": fields[1].strip(),
            "current": fields[2].strip() if len(fields) > 2 else "",
            "latest": fields[3].strip() if len(fields) > 3 else "",
        })
    if components:
        state = "updates"
    elif returncode == 0:
        state = "uptodate"
    else:
        # 10 sans ligne ne devrait pas arriver ; tout le reste (3, 1 d'une
        # image antérieure sans --check) est indéterminé.
        state = "unknown"
    return {"state": state, "components": components}

def check_gwine() -> dict:
    """Composants gwine (runner, DXVK, VKD3D, D7VK, NVAPI) en retard."""
    returncode, text = _run(
        [local.gwine_binary(), "--init", "--check"], 120,
    )
    return parse_gwine_check(text, returncode)

def apps_domain(pending, unknown: bool, installed: int) -> dict:
    """État du domaine « applications » depuis une sonde (`pending_updates`).

    Partagé par les deux postes : le local sonde ses propres AppImages, un
    poste distant rend les mêmes trois valeurs par SSH (`remote_updates`).
    """
    if pending:
        return {"state": "updates", "apps": sorted(apps.names_for(pending))}
    if not installed:
        return {"state": "unavailable", "apps": []}
    return {"state": "unknown" if unknown else "uptodate", "apps": []}


def check_apps() -> dict:
    """Applications AppImage installées en retard.

    La sonde passe par les flux de releases (jamais l'API GitHub) et reste
    honnête : un flux muet laisse le domaine « indéterminé » plutôt que de
    conclure « à jour ». Aucune application installée → domaine indisponible,
    comme un outil absent du poste.
    """
    return apps_domain(*apps.pending_updates())

# -- agrégat ----------------------------------------------------------------

def check_all(enabled: dict[str, bool] | None = None,
              online: bool = True) -> dict:
    """Sonde les domaines demandés, en parallèle, sans jamais lever.

    `enabled` vient des préférences (`prefs.check_domains`) ; un domaine
    décoché n'apparaît pas dans le résultat. Hors ligne, seules les sondes
    locales tournent (image déjà téléchargée, âge du déploiement).
    """
    enabled = {domain: True for domain in DOMAINS} if enabled is None \
        else dict(enabled)
    results: dict[str, dict] = {}

    jobs = {}
    with ThreadPoolExecutor(max_workers=4) as pool:
        if enabled.get("image"):
            jobs["image"] = pool.submit(check_image, online)
        if online and enabled.get("flatpak"):
            jobs["flatpak"] = pool.submit(check_flatpaks)
        if online and enabled.get("brew"):
            jobs["brew"] = pool.submit(check_brew)
        if online and enabled.get("gwine"):
            jobs["gwine"] = pool.submit(check_gwine)
        if online and enabled.get("apps"):
            jobs["apps"] = pool.submit(check_apps)
        for domain, future in jobs.items():
            try:
                results[domain] = future.result()
            except Exception as error:      # une sonde d'agrément ne casse rien
                _note_failure(domain, repr(error))
                results[domain] = {"state": "unknown"}

    for domain in DOMAINS:
        if domain in results:
            continue
        if not enabled.get(domain):
            results[domain] = {"state": "skipped"}
        else:
            results[domain] = {"state": "offline"}

    return {
        "checked_at": time.time(),
        "online": bool(online),
        "domains": results,
    }

def domain_count(domain: str, result: dict) -> int:
    """Nombre d'éléments en retard pour un domaine (image = 0 ou 1)."""
    state = (result or {}).get("state")
    if state != "updates" and not (domain == "image" and state == "available"):
        return 0
    if domain == "image":
        return 1
    if domain == "flatpak":
        return len(result.get("apps") or [])
    if domain == "brew":
        return len(result.get("formulas") or [])
    if domain == "gwine":
        return len(result.get("components") or [])
    if domain == "apps":
        return len(result.get("apps") or [])
    return 0

def total_updates(state: dict | None) -> int:
    if not state:
        return 0
    return sum(
        domain_count(domain, result)
        for domain, result in (state.get("domains") or {}).items()
    )

def has_updates(state: dict | None) -> bool:
    return total_updates(state) > 0

def unverified_domains(state: dict | None) -> list[str]:
    """Domaines dont la sonde n'a pas pu conclure (état « unknown »).

    Un domaine illisible n'est pas un domaine à jour : le statut doit le
    dire, sinon « À jour » couvre une sonde qui a échoué (vécu 08/11/2026).
    `unavailable` (« absent de ce poste ») est une réponse, pas un doute.
    """
    domains = (state or {}).get("domains") or {}
    return [
        domain for domain in DOMAINS
        if (domains.get(domain) or {}).get("state") == "unknown"
    ]

def image_detail(image: dict) -> str:
    """Ligne « Image Gablue » : celle qui tourne, puis celle en attente.

    `version` est le déploiement amorcé (celui qui tourne) ; `staged_version`
    n'existe que si une image attend le redémarrage. On nomme les deux plutôt
    que d'afficher « nouvelle image en attente » sans dire laquelle — et la
    version tournante reste affichée même quand une autre est en attente
    (vécu 07/11/2026).
    """
    version = str(image.get("version") or "")
    staged = str(image.get("staged_version") or "")
    parts: list[str] = []
    if version:
        parts.append(f"actuelle {version}")
    if image.get("staged"):
        if staged:
            parts.append(f"en attente {staged}")
        else:
            parts.append("mise à jour en attente")
    elif version and image.get("days") is not None:
        parts.append("déployée " + days_text(image.get("days")))
    return " · ".join(parts)

def days_text(days: int | None) -> str:
    if days is None:
        return ""
    if days <= 0:
        return "aujourd'hui"
    return f"il y a {days} jour{'s' if days > 1 else ''}"

def _list_text(names: list[str], limit: int = 5) -> str:
    """Liste lisible, bornée : une infobulle ne doit pas couvrir l'écran."""
    names = [name for name in (names or []) if name]
    if len(names) <= limit:
        return ", ".join(names)
    return ", ".join(names[:limit]) + f" et {len(names) - limit} autres"

def summary_lines(state: dict | None) -> list[str]:
    """Une ligne lisible par domaine — infobulle et menu de la barre d'état."""
    if not state:
        return []
    domains = state.get("domains") or {}
    lines: list[str] = []

    image = domains.get("image")
    if image:
        status = image.get("state")
        version = image.get("version") or ""
        staged_version = image.get("staged_version") or ""
        if status == "available":
            if image.get("staged"):
                detail = "mise à jour téléchargée, redémarrage requis"
                if staged_version and version:
                    detail += f" — {version} → {staged_version}"
                elif staged_version:
                    detail += f" — version {staged_version}"
            else:
                # `bootc upgrade --check` annonce la version distante
                # (`Version:`) : on la nomme, précédée de celle qui tourne.
                # Une version de bootc qui ne la donne pas retombait sur
                # « actuelle X », qui ne disait pas ce qui arrive.
                detail = "mise à jour disponible"
                available = str(image.get("available_version") or "")
                if available and version:
                    detail += f" — {version} → {available}"
                elif available:
                    detail += f" — version {available}"
                elif version:
                    detail += f" — actuelle {version}"
            lines.append(f"Image Gablue : {detail}")
        elif status == "uptodate":
            lines.append("Image Gablue : à jour")
        elif status == "offline":
            lines.append("Image Gablue : non vérifiée (hors ligne)")
        elif status != "skipped":
            lines.append("Image Gablue : vérification impossible")

    for domain in ("flatpak", "brew", "gwine", "apps"):
        result = domains.get(domain) or {}
        status = result.get("state")
        label = DOMAIN_LABELS[domain]
        if status == "updates":
            if domain == "flatpak":
                lines.append(f"{label} : " + _list_text(result.get("apps")))
            elif domain == "brew":
                lines.append(
                    f"{label} : " + _list_text(result.get("formulas"))
                )
            elif domain == "apps":
                lines.append(f"{label} : " + _list_text(result.get("apps")))
            else:
                parts = []
                for component in result.get("components") or []:
                    name = GWINE_LABELS.get(
                        component["key"], component["key"]
                    )
                    current = component.get("current") or "absent"
                    latest = component.get("latest") or "à jour ?"
                    parts.append(f"{name} {current} → {latest}")
                lines.append(f"{label} : " + ", ".join(parts))
        elif status == "uptodate":
            lines.append(f"{label} : à jour")
        elif status == "offline":
            lines.append(f"{label} : non vérifié (hors ligne)")
        elif status == "unavailable":
            lines.append(f"{label} : absent de ce poste")
        elif status == "unknown":
            lines.append(f"{label} : vérification impossible")

    return lines

def compact_summary(state: dict | None) -> str:
    """Phrase courte pour la carte « Ce poste »."""
    if not state:
        return ""
    domains = state.get("domains") or {}
    if domains and all(
        (result or {}).get("state") == "skipped"
        for result in domains.values()
    ):
        return "Vérifications de mises à jour désactivées"
    total = total_updates(state)
    if total:
        # Une image déjà téléchargée compte même hors ligne : le redémarrage
        # reste à faire, la connexion n'y change rien.
        names = [
            DOMAIN_LABELS[domain].lower()
            for domain in DOMAINS
            if domain_count(domain, domains.get(domain) or {})
        ]
        count = f"{total} mise{'s' if total > 1 else ''} à jour"
        return count + " : " + ", ".join(names)
    if not state.get("online", True):
        return "Hors ligne — vérification impossible"
    unknown = [
        domain for domain in DOMAINS
        if (domains.get(domain) or {}).get("state") in ("unknown", "unavailable")
    ]
    if unknown and len(unknown) == len(DOMAINS):
        return "Vérification impossible"
    unverified = unverified_domains(state)
    if unverified:
        # Même règle que le statut du panneau : un domaine illisible n'autorise
        # pas à annoncer « aucune mise à jour ».
        if len(unverified) == 1:
            return "1 domaine non vérifié"
        return f"{len(unverified)} domaines non vérifiés"
    age = (domains.get("image") or {}).get("days")
    text = "Aucune mise à jour disponible"
    if age is not None:
        text += f" · Gablue mis à jour {days_text(age)}"
    return text


def notification_text(state: dict | None, previous: dict | None = None,
                      manual: bool = False) -> tuple[str, str] | None:
    """(titre, corps) de la notification du tray, ou None pour ne rien dire.

    Une passe **automatique** ne parle que si elle trouve du nouveau (le
    compteur monte) : répéter « 3 mises à jour disponibles » toutes les deux
    heures serait du bruit, la pastille rouge suffit. Une vérification
    **demandée** (menu du tray) répond toujours, y compris « aucune mise à
    jour », sinon le clic reste sans effet visible.
    """
    if not state:
        return None
    total = total_updates(state)
    if total:
        if not manual and total <= total_updates(previous):
            return None
        title = ("1 mise à jour disponible" if total == 1
                 else f"{total} mises à jour disponibles")
        body = "\n".join(
            line for line in summary_lines(state)
            if not line.endswith(" : à jour")
        )
        return title, body
    if not manual:
        return None
    detail = compact_summary(state)
    if detail.startswith("Aucune mise à jour disponible"):
        # Le titre le dit déjà ; le corps ne garde que l'âge de l'image.
        return "Aucune mise à jour", detail.partition(" · ")[2]
    # Hors ligne, domaine illisible, vérifications coupées : le titre dit le
    # doute, le corps dit pourquoi.
    return "Vérification des mises à jour", detail
