"""Registre des postes et recherche sur le réseau local.
"""

import getpass
import ipaddress
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from .constants import AVAHI_BROWSE, CONFIG_DIR, HOSTS_FILE, IP, SSH
from .util import _ssh_opts, normalize_record, ssh_target

TAILSCALE_NETWORK = ipaddress.IPv4Network("100.64.0.0/10")


def parse_local_ipv4(text: str) -> tuple[set[str], list]:
    """Adresses et sous-réseaux IPv4 de cette machine (`ip -o -4 addr show`).

    Sert à distinguer les adresses du poste distant qui sont réellement sur
    notre lien (LAN) de celles qui ne sont que des interfaces internes de
    cette machine : boucle locale, pont docker/podman, virbr0 de libvirt…
    """
    own: set[str] = set()
    networks: list = []
    for match in re.finditer(r"inet (\d+\.\d+\.\d+\.\d+)/(\d+)", text):
        try:
            interface = ipaddress.ip_interface(f"{match.group(1)}/{match.group(2)}")
        except ValueError:
            continue
        own.add(str(interface.ip))
        networks.append(interface.network)
    return own, networks


def local_ipv4() -> tuple[set[str], list]:
    """Interfaces IPv4 du poste (`ip` absent ou muet → ensemble vide)."""
    try:
        result = subprocess.run(
            [IP, "-o", "-4", "addr", "show"],
            capture_output=True, text=True, timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return set(), []
    if result.returncode != 0:
        return set(), []
    return parse_local_ipv4(result.stdout)


def address_kind(address: str, own: set[str], networks: list) -> str:
    """« lan », « tailscale » ou « » (adresse inutilisable depuis ce poste)."""
    try:
        ip = ipaddress.IPv4Address(address)
    except ValueError:
        return ""
    if ip.is_loopback or ip.is_link_local or str(ip) in own:
        return ""
    if ip in TAILSCALE_NETWORK:
        return "tailscale"
    if not networks:
        # Pas de table d'interfaces : rester permissif plutôt que de tout
        # masquer, on n'a alors que le filtre boucle locale / nos adresses.
        return "lan"
    return "lan" if any(ip in network for network in networks) else ""


# Une seule adresse LAN (celle de la route par défaut, pas docker0) et
# l'adresse Tailscale si le service y répond.
REMOTE_ADDRESS_SCRIPT = (
    "ip -4 route get 1.1.1.1 2>/dev/null"
    " | sed -n 's/.* src \\([0-9.]*\\).*/\\1/p';"
    " tailscale ip -4 2>/dev/null; true"
)


def remote_address_kind(address: str, own: set[str] | None = None) -> str:
    """« lan », « tailscale » ou « » pour une adresse rapportée par le poste.

    Contrairement à `address_kind` (qui juge ce qui est joignable d'ICI), on
    garde ici l'adresse LAN même hors de nos sous-réseaux directs : elle reste
    la bonne adresse une fois rentré à la maison, et le filtrage par
    sous-réseau local ne concerne que les annonces mDNS.
    """
    if own is None:
        own, _networks = local_ipv4()
    try:
        ip = ipaddress.IPv4Address(address)
    except ValueError:
        return ""
    if (ip.is_loopback or ip.is_link_local or ip.is_multicast
            or ip.is_unspecified or str(ip) in own):
        return ""
    if ip in TAILSCALE_NETWORK:
        return "tailscale"
    return "lan" if ip.is_private else ""


def parse_remote_addresses(text: str, main: str = "",
                           own: set[str] | None = None) -> list[dict]:
    """Adresses IPv4 d'un poste, lues chez lui (`ip route` + `tailscale ip`).

    Avahi n'annonce pas l'interface `tailscale0` par défaut : l'adresse
    Tailscale d'un poste ne peut pas venir de la découverte mDNS. La seule
    façon fiable de la connaître, c'est de demander au poste lui-même — ce que
    fait cette sonde après une connexion réussie. L'adresse LAN vient de la
    route par défaut (pas des interfaces : docker0 et consorts n'ont rien à
    faire dans le registre).
    """
    if own is None:
        own, _networks = local_ipv4()
    found: list[dict] = []
    seen = {main} if main else set()
    for token in re.findall(r"\b\d+\.\d+\.\d+\.\d+\b", text or ""):
        if token in seen:
            continue
        kind = remote_address_kind(token, own)
        if not kind:
            continue
        seen.add(token)
        found.append({"address": token, "kind": kind})
    found.sort(key=lambda item: ("lan", "tailscale").index(item["kind"]))
    return found


def remote_addresses(host: dict) -> list[dict]:
    """Adresses du poste lues par SSH (une connexion, silencieux si échec)."""
    try:
        result = subprocess.run(
            [SSH, *_ssh_opts(host), ssh_target(host), REMOTE_ADDRESS_SCRIPT],
            capture_output=True, text=True, timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    if result.returncode != 0:
        return []
    return parse_remote_addresses(result.stdout, host["host"])


def group_candidates(entries: list[dict], own: set[str] | None = None,
                     networks: list | None = None) -> list[dict]:
    """Une entrée par machine : adresses inutiles écartées, LAN puis Tailscale.

    Avahi publie une adresse par interface du poste (boucle locale, docker0,
    tailscale0, LAN…). On ne garde que celles joignables d'ici et on les
    regroupe sous le nom mDNS de la machine.
    """
    if own is None or networks is None:
        own, networks = local_ipv4()
    kinds = ("lan", "tailscale")
    grouped: dict[tuple, dict] = {}
    for entry in entries:
        kind = address_kind(str(entry.get("host", "")), own, networks)
        if not kind:
            continue
        key = (
            str(entry.get("hostname") or entry.get("label") or "").casefold(),
            str(entry.get("user", "")),
            int(entry.get("port", 22)),
        )
        record = grouped.get(key)
        if record is None:
            record = {
                "label": entry.get("label") or entry.get("hostname") or "",
                "user": entry.get("user", ""),
                "port": int(entry.get("port", 22)),
                "hosts": [],
            }
            grouped[key] = record
        if entry["host"] not in [item[0] for item in record["hosts"]]:
            record["hosts"].append((str(entry["host"]), kind))
    found = []
    for record in grouped.values():
        record["hosts"].sort(key=lambda item: kinds.index(item[1]))
        record["host"] = record["hosts"][0][0]
        found.append(record)
    found.sort(key=lambda item: item["label"].casefold())
    return found


def known_targets(hosts: list[dict] | None) -> set[tuple[str, str, int]]:
    """Triplets (utilisateur, adresse, port) déjà enregistrés.

    **Toutes** les adresses d'un poste comptent, pas seulement la principale :
    un poste enregistré en LAN puis revu par Tailscale est la même machine, et
    inversement. Sans ça, relancer une découverte reproposait un poste déjà
    ajouté dès que l'adresse annoncée n'était pas celle du registre.
    """
    targets: set[tuple[str, str, int]] = set()
    for host in hosts or []:
        user = str(host.get("user") or "")
        try:
            port = int(host.get("port", 22) or 22)
        except (TypeError, ValueError):
            continue
        entries = [{"address": host.get("host")}, *(host.get("addresses") or [])]
        for entry in entries:
            address = str((entry or {}).get("address") or "")
            if user and address:
                targets.add((user, address, port))
    return targets


def candidate_known(candidate: dict,
                    targets: set[tuple[str, str, int]]) -> bool:
    """Ce poste proposé est-il déjà dans le registre (une de ses adresses) ?"""
    user = str(candidate.get("user") or "")
    try:
        port = int(candidate.get("port", 22) or 22)
    except (TypeError, ValueError):
        return False
    addresses = [
        address for address, _kind in candidate.get("hosts") or []
    ] or [candidate.get("host")]
    return any(
        (user, str(address), port) in targets
        for address in addresses if address
    )


def avahi_scan(timeout: float = 8.0,
               stop: threading.Event | None = None) -> list[dict]:
    """Postes SSH annoncés en mDNS sur le réseau local, une entrée par machine."""
    if AVAHI_BROWSE == "avahi-browse" and not shutil.which("avahi-browse"):
        return []
    try:
        proc = subprocess.Popen(
            [AVAHI_BROWSE, "-rtp", "_ssh._tcp"],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
    except OSError:
        return []
    deadline = time.monotonic() + timeout
    while proc.poll() is None:
        if ((stop is not None and stop.is_set())
                or time.monotonic() > deadline):
            proc.kill()
            proc.wait()
            return []
        time.sleep(0.1)
    output = proc.stdout.read() if proc.stdout is not None else ""
    entries = []
    for line in output.splitlines():
        if not line.startswith("="):
            continue
        fields = line.split(";")
        if len(fields) < 10:
            continue
        _, _iface, proto, name, service, _domain, hostname, address, port, _txt = \
            fields[:10]
        if proto != "IPv4" or service != "_ssh._tcp":
            continue
        if not address or "%" in address:
            continue
        try:
            number = int(port or 22)
        except ValueError:
            number = 22
        label = re.sub(r"\s*\[[^]]*\]\s*$", "", name).strip() or hostname
        entries.append({
            "label": label,
            "hostname": hostname or label,
            "user": getpass.getuser(),
            "host": address,
            "port": number,
        })
    return group_candidates(entries)

def load_hosts() -> list[dict]:
    """Lit le registre ; un fichier illisible est écarté, pas écrasé."""
    try:
        raw = HOSTS_FILE.read_text(encoding="utf-8")
    except FileNotFoundError:
        return []
    except OSError as exc:
        print(f"gablue-manager : {HOSTS_FILE} illisible ({exc})", file=sys.stderr)
        return []
    try:
        data = json.loads(raw)
    except ValueError:
        backup = HOSTS_FILE.with_name(
            f"ssh-hosts.corrupt-{int(time.time())}.json"
        )
        try:
            HOSTS_FILE.replace(backup)
            print(f"gablue-manager : registre illisible, sauvegardé dans {backup}",
                  file=sys.stderr)
        except OSError:
            pass
        return []
    hosts = data.get("hosts") if isinstance(data, dict) else data
    if not isinstance(hosts, list):
        return []
    cleaned = []
    seen = set()
    for item in hosts:
        host = normalize_record(item)
        if host and host["id"] not in seen:
            seen.add(host["id"])
            cleaned.append(host)
    return cleaned

def save_hosts(hosts: list[dict]) -> None:
    """Écriture atomique et privée (le registre contient des adresses)."""
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(CONFIG_DIR, 0o700)
    except OSError:
        pass
    payload = json.dumps(
        {"version": 1, "hosts": hosts}, ensure_ascii=False, indent=2
    ) + "\n"
    fd, tmp = tempfile.mkstemp(dir=str(CONFIG_DIR), prefix=".ssh-hosts-")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(payload)
        os.chmod(tmp, 0o600)
        os.replace(tmp, HOSTS_FILE)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise
