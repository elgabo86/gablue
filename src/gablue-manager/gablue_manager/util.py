"""Petites fonctions de présentation (cible ssh, nom, URL KIO).
"""

import time
import uuid

from .constants import PRIVATE_KEY

def _clean_address(value: object) -> str:
    """Adresse utilisable en argument de ssh, ou chaîne vide."""
    text = str(value or "").strip()
    if not text or any(ch.isspace() for ch in text):
        return ""
    if any(ch in text for ch in "@/"):
        return ""
    return text

def active_address(host: dict) -> str:
    """Adresse à utiliser maintenant : la dernière joignable, sinon la principale.

    Un poste enregistré avec son adresse LAN reste joignable à distance par son
    adresse Tailscale : la dernière adresse qui a répondu est conservée
    (`active`) et sert aux actions, sans redemander à l'utilisateur laquelle
    choisir.
    """
    return str(host.get("active") or "") or host["host"]

def host_addresses(host: dict) -> list[str]:
    """Adresse principale puis les adresses de secours (LAN, Tailscale…)."""
    found = [host["host"]]
    for item in host.get("addresses") or []:
        address = str(item.get("address") or "")
        if address and address not in found:
            found.append(address)
    return found

def active_kind(host: dict) -> str:
    """Type de l'adresse active (« LAN », « Tailscale » ou vide)."""
    current = active_address(host)
    for item in host.get("addresses") or []:
        if item.get("address") == current:
            return {"lan": "LAN", "tailscale": "Tailscale"}.get(item.get("kind", ""), "")
    return ""

def ssh_target(host: dict, address: str = "") -> str:
    """Cible ssh : utilisateur@hôte (l'hôte IPv6 garde ses deux-points)."""
    return f'{host["user"]}@{address or active_address(host)}'

def address_text(host: dict, address: str = "") -> str:
    """Adresse lisible : utilisateur@hôte, avec :port si non standard."""
    text = ssh_target(host, address)
    if host["port"] != 22:
        text += f':{host["port"]}'
    return text

def display_name(host: dict) -> str:
    return (host.get("label") or "").strip() or host["host"]

def dolphin_url(host: dict) -> str:
    """URL KIO sftp:// (crochets obligatoires autour d'une IPv6)."""
    addr = active_address(host)
    if ":" in addr and not addr.startswith("["):
        addr = f"[{addr}]"
    url = f'sftp://{host["user"]}@{addr}'
    if host["port"] != 22:
        url += f':{host["port"]}'
    return url + "/"

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

    addresses: list[dict] = []
    raw_addresses = item.get("addresses")
    if isinstance(raw_addresses, list):
        for entry in raw_addresses:
            if isinstance(entry, dict):
                address = _clean_address(entry.get("address"))
                kind = str(entry.get("kind") or "").strip()
            else:
                address = _clean_address(entry)
                kind = ""
            if not address or address == name:
                continue
            if any(found["address"] == address for found in addresses):
                continue
            addresses.append({"address": address, "kind": kind})

    known = [name] + [entry["address"] for entry in addresses]
    active = _clean_address(item.get("active"))
    return {
        "id": str(item.get("id") or uuid.uuid4().hex),
        "label": str(item.get("label") or "").strip(),
        "user": user,
        "host": name,
        "port": port,
        "addresses": addresses,
        "active": active if active in known else "",
        "key_added": bool(item.get("key_added", False)),
        "added_at": int(item.get("added_at") or time.time()),
    }


def _ssh_opts(host: dict, batch: bool = True) -> list[str]:
    """Options communes : on teste exactement LA clé gérée par Gablue.

    `StrictHostKeyChecking=yes` partout (jamais `accept-new`) : une clé d'hôte
    inconnue doit être épinglée explicitement, pas acceptée en passant.
    """
    opts = [
        "-T",
        "-i", str(PRIVATE_KEY),
        "-o", "IdentitiesOnly=yes",
        "-o", "ConnectTimeout=4",
        "-o", "ConnectionAttempts=1",
        "-o", "StrictHostKeyChecking=yes",
        "-p", str(host["port"]),
    ]
    if batch:
        opts += ["-o", "BatchMode=yes"]
    return opts
