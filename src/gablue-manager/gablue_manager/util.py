"""Petites fonctions de présentation (cible ssh, nom, URL KIO).
"""

import re
import time
import uuid

from .constants import PRIVATE_KEY

def ssh_target(host: dict) -> str:
    """Cible ssh : utilisateur@hôte (l'hôte IPv6 garde ses deux-points)."""
    return f'{host["user"]}@{host["host"]}'

def address_text(host: dict) -> str:
    """Adresse lisible : utilisateur@hôte, avec :port si non standard."""
    text = ssh_target(host)
    if host["port"] != 22:
        text += f':{host["port"]}'
    return text

def display_name(host: dict) -> str:
    return (host.get("label") or "").strip() or host["host"]

def dolphin_url(host: dict) -> str:
    """URL KIO sftp:// (crochets obligatoires autour d'une IPv6)."""
    addr = host["host"]
    if ":" in addr and not addr.startswith("["):
        addr = f"[{addr}]"
    url = f'sftp://{host["user"]}@{addr}'
    if host["port"] != 22:
        url += f':{host["port"]}'
    return url + "/"

def normalize_mac(value: object) -> str:
    """MAC sous forme AA:BB:CC:DD:EE:FF, ou chaîne vide si invalide."""
    digits = re.sub(r"[^0-9A-Fa-f]", "", str(value or ""))
    if len(digits) != 12:
        return ""
    return ":".join(digits[i:i + 2] for i in range(0, 12, 2)).upper()

def normalize_record(item: object) -> dict | None:
    """Valide une entrée du registre ; None si elle est inutilisable."""
    if not isinstance(item, dict):
        return None
    try:
        user = str(item["user"]).strip()
        name = str(item["host"]).strip()
        port = int(item.get("port", 22))
    except (KeyError, TypeError, ValueError):
        return None
    if not user or not name or not (1 <= port <= 65535):
        return None
    return {
        "id": str(item.get("id") or uuid.uuid4().hex),
        "label": str(item.get("label") or "").strip(),
        "user": user,
        "host": name,
        "port": port,
        "mac": normalize_mac(item.get("mac", "")),
        "key_added": bool(item.get("key_added", False)),
        "added_at": int(item.get("added_at") or time.time()),
    }


def _ssh_opts(host: dict, batch: bool = True,
              accept_new: bool = False) -> list[str]:
    """Options communes : on teste exactement LA clé gérée par Gablue."""
    check = "accept-new" if accept_new else "yes"
    opts = [
        "-T",
        "-i", str(PRIVATE_KEY),
        "-o", "IdentitiesOnly=yes",
        "-o", "ConnectTimeout=4",
        "-o", "ConnectionAttempts=1",
        "-o", f"StrictHostKeyChecking={check}",
        "-p", str(host["port"]),
    ]
    if batch:
        opts += ["-o", "BatchMode=yes"]
    return opts