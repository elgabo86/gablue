"""Registre des postes, découverte mDNS, import ssh_config, WoL.
"""

import getpass
import glob
import ipaddress
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from .constants import AVAHI_BROWSE, CONFIG_DIR, HOSTS_FILE, IP, SSH, SSH_DIR, WOL_PORTS
from .util import _ssh_opts, normalize_mac, normalize_record, ssh_target

def _broadcast_for(address: str) -> str:
    """Adresse de diffusion du sous-réseau qui route vers `address`."""
    try:
        route = subprocess.run(
            [IP, "route", "get", address], capture_output=True, text=True,
            timeout=4,
        )
        match = re.search(r"\bdev\s+(\S+)", route.stdout)
        if not match:
            return ""
        addr = subprocess.run(
            [IP, "-o", "-4", "addr", "show", "dev", match.group(1)],
            capture_output=True, text=True, timeout=4,
        )
        found = re.search(r"\binet\s+(\d+\.\d+\.\d+\.\d+/\d+)", addr.stdout)
        if not found:
            return ""
        return str(ipaddress.ip_interface(found.group(1)).network.broadcast_address)
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return ""

def wake_on_lan(host: dict) -> tuple[bool, str]:
    """Envoie le paquet magique (broadcast global + diffusion du sous-réseau)."""
    mac = normalize_mac(host.get("mac", ""))
    if not mac:
        return False, "adresse MAC inconnue : renseignez-la dans « Modifier »"
    payload = b"\xff" * 6 + bytes.fromhex(mac.replace(":", "")) * 16
    targets = ["255.255.255.255"]
    subnet = _broadcast_for(host["host"])
    if subnet and subnet not in targets:
        targets.append(subnet)
    sent = 0
    last_error = ""
    for address in targets:
        for port in WOL_PORTS:
            try:
                with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
                    sock.sendto(payload, (address, port))
                sent += 1
            except OSError as exc:
                last_error = str(exc)
    if sent:
        return True, ""
    return False, last_error or "envoi impossible"

def detect_mac(host: dict) -> str:
    """MAC dans la table ARP locale (gratuit, mais seulement si le poste a parlé)."""
    try:
        result = subprocess.run(
            [IP, "neigh", "show", "to", host["host"]],
            capture_output=True, text=True, timeout=4,
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    match = re.search(r"lladdr\s+([0-9a-fA-F:]{17})", result.stdout)
    return normalize_mac(match.group(1)) if match else ""

def detect_mac_remote(host: dict) -> str:
    """MAC de l'interface par défaut, lue par SSH (poste en ligne, ARP vide)."""
    command = (
        "cat /sys/class/net/$(ip route show default 2>/dev/null"
        " | awk '{print $5; exit}')/address 2>/dev/null"
    )
    try:
        result = subprocess.run(
            [SSH, *_ssh_opts(host), ssh_target(host), command],
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return normalize_mac(result.stdout.strip()) if result.returncode == 0 else ""

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

def _ssh_config_sources(path: Path, depth: int = 0, seen=None):
    """Lignes (clé, valeur) de ~/.ssh/config, Include compris, dans l'ordre."""
    seen = seen if seen is not None else set()
    try:
        resolved = path.resolve()
    except OSError:
        return
    if depth > 3 or resolved in seen or not resolved.is_file():
        return
    seen.add(resolved)
    try:
        text = resolved.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        match = re.match(r"(?i)^include\s+(.+)$", line)
        if match:
            for pattern in match.group(1).split():
                target = Path(os.path.expanduser(pattern))
                if not target.is_absolute():
                    target = SSH_DIR / pattern
                for item in sorted(glob.glob(str(target))):
                    yield from _ssh_config_sources(Path(item), depth + 1, seen)
            continue
        if "=" in line:
            key, _, value = line.partition("=")
        else:
            key, _, value = line.partition(" ")
        yield key.strip().lower(), value.strip()

def parse_ssh_config() -> list[dict]:
    """Hôtes concrets de ~/.ssh/config ; les motifs (`*`, `?`, `!`) sont ignorés."""
    blocks = []
    defaults = {}
    current = None
    for key, value in _ssh_config_sources(SSH_DIR / "config"):
        if key == "host":
            current = {"patterns": value.split(), "opts": {}}
            blocks.append(current)
        elif key == "match":
            current = None
        elif value:
            if current is None:
                defaults.setdefault(key, value)
            else:
                current["opts"].setdefault(key, value)

    hosts = []
    seen = set()
    for block in blocks:
        for pattern in block["patterns"]:
            if not pattern or any(ch in pattern for ch in "*?!"):
                continue
            options = block["opts"]
            address = options.get("hostname", pattern)
            user = options.get("user", defaults.get("user", getpass.getuser()))
            try:
                port = int(options.get("port", defaults.get("port", 22)))
            except ValueError:
                port = 22
            if not (1 <= port <= 65535) or not address or "%" in address:
                continue
            if any(ch in address for ch in "@/") or "@" in user:
                continue
            key = (user, address, port)
            if key in seen:
                continue
            seen.add(key)
            hosts.append({
                "label": pattern, "user": user, "host": address, "port": port,
            })
    return hosts

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
