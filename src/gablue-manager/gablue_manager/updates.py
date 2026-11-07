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
import time
from concurrent.futures import ThreadPoolExecutor

from . import local
from .constants import BREW_BINARY

DOMAINS = ("image", "flatpak", "brew", "gwine")

DOMAIN_LABELS = {
    "image": "Image Gablue",
    "flatpak": "Flatpaks",
    "brew": "Homebrew",
    "gwine": "Composants gwine",
}

GWINE_LABELS = {
    "runner": "Runner gwine",
    "dxvk": "DXVK",
    "vkd3d": "VKD3D-Proton",
    "d7vk": "D7VK",
    "nvapi": "DXVK-NVAPI",
    "cache": "Composants manquants",
}

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
    """Applications Flatpak en retard, installation système et compte."""
    apps: list[str] = []
    for extra in ((), ("--user",)):
        returncode, text = _run(
            ["flatpak", "remote-ls", *extra, "--updates",
             "--columns=application"],
            60,
        )
        if returncode == 0:
            apps.extend(parse_flatpak_updates(text))
    unique = sorted(set(apps))
    return {"state": "updates" if unique else "uptodate", "apps": unique}

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
        for domain, future in jobs.items():
            try:
                results[domain] = future.result()
            except Exception:              # une sonde d'agrément ne casse rien
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

def _days_text(days: int | None) -> str:
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
        version = image.get("staged_version") or image.get("version") or ""
        if status == "available":
            if image.get("staged"):
                detail = "mise à jour téléchargée, redémarrage requis"
            else:
                detail = "mise à jour disponible"
            if version:
                detail += f" — version {version}"
            lines.append(f"Image Gablue : {detail}")
        elif status == "uptodate":
            lines.append("Image Gablue : à jour")
        elif status == "offline":
            lines.append("Image Gablue : non vérifiée (hors ligne)")
        elif status != "skipped":
            lines.append("Image Gablue : vérification impossible")
        if image.get("days") is not None:
            lines.append(
                "Dernière mise à jour de Gablue : "
                + _days_text(image.get("days"))
            )

    for domain in ("flatpak", "brew", "gwine"):
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
    age = (domains.get("image") or {}).get("days")
    text = "Aucune mise à jour disponible"
    if age is not None:
        text += f" · Gablue mis à jour {_days_text(age)}"
    return text
