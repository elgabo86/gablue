"""Découverte des postes : mDNS sur le LAN, sonde sur le tailnet.

Aucune saisie utilisateur : un poste apparaît dès qu'il est joignable **et**
qu'il partage au moins un dossier. Un poste qui ne partage rien n'annonce
rien ; s'il répond quand même (sonde Tailscale), sa liste `shares` vide le
fait écarter.

Identité = `peer_id` (machine-id), jamais le nom d'hôte ni l'adresse : le même
poste vu en LAN et en Tailscale est une seule entrée, LAN préféré.
"""

import json
import shutil
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor

from PySide6.QtCore import QObject, QTimer, Signal

from . import api, config
from .constants import EXPIRE_CYCLES, PORT, PROBE_INTERVAL, PROBE_TIMEOUT, SERVICE


class Peer:
    """Un poste distant, identifié par son `peer_id`."""

    def __init__(self, peer_id: str, name: str, port: int):
        self.peer_id = peer_id
        self.name = name
        self.port = port
        #: Vrai pour le pseudo-poste qui désigne **ce** poste : ses partages
        #: apparaissent dans la même liste que ceux des autres, servis par le
        #: démon local. Sert au libellé (« Ce poste ») et au tri.
        self.local = False
        self.shares: list[dict] = []
        self.addrs: dict[str, str] = {}  # hôte -> "lan" | "tailscale"
        #: Adresse qui a réellement répondu à la dernière sonde. Le LAN reste
        #: prioritaire (plus rapide qu'un détour par le tailnet), mais si le LAN
        #: ne répond plus, on ne s'obstine pas : c'est le tailnet qui sert.
        self.active_addr = ""
        self.last_ok = 0.0
        self.failures = 0

    def ordered_addrs(self) -> list[str]:
        """LAN d'abord, puis le reste : l'ordre qui coûte le moins."""
        lan = [addr for addr, source in self.addrs.items() if source == "lan"]
        other = [addr for addr, source in self.addrs.items() if source != "lan"]
        return lan + other

    def base_url(self) -> str:
        addrs = self.ordered_addrs()
        if not addrs:
            return ""
        host = self.active_addr if self.active_addr in addrs else addrs[0]
        return api.peer_url(host, self.port)

    def label(self) -> str:
        sources = set(self.addrs.values())
        if "lan" in sources:
            return "Réseau local"
        if "tailscale" in sources:
            return "Tailscale"
        return "Réseau"

    def describe(self) -> str:
        if not self.shares:
            return "Aucun partage"
        if len(self.shares) == 1:
            return str(self.shares[0].get("name") or "1 partage")
        return f"{len(self.shares)} partages"


def split_fields(line: str) -> list[str]:
    """Découpe une ligne Avahi sur les `;` non échappés."""
    fields: list[str] = []
    current: list[str] = []
    escaped = False
    for char in line:
        if escaped:
            current.append(char)
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == ";":
            fields.append("".join(current))
            current = []
        else:
            current.append(char)
    fields.append("".join(current))
    return fields


def parse_txt(raw: str) -> dict:
    values = {}
    for token in raw.split(" "):
        token = token.strip().strip('"')
        if "=" in token:
            key, value = token.split("=", 1)
            values[key] = value
    return values


class Discovery(QObject):
    """Registre des postes + les deux sources (mDNS, Tailscale)."""

    peers_changed = Signal(list)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._lock = threading.Lock()
        self._peers: dict[str, Peer] = {}
        self._dirty = False
        self._stop = threading.Event()
        self._cycle_lock = threading.Lock()
        self._avahi: subprocess.Popen | None = None
        self.active = False
        self._local_id = config.machine_id()
        self._ticker = QTimer(self)
        self._ticker.setInterval(600)
        self._ticker.timeout.connect(self._emit_if_dirty)

    # --- cycle de vie ---------------------------------------------------
    def start(self) -> None:
        self._stop.clear()
        self.active = True
        threading.Thread(target=self._browse_mdns, name="gshare-mdns", daemon=True).start()
        threading.Thread(target=self._probe_loop, name="gshare-probe", daemon=True).start()
        self._ticker.start()

    def stop(self) -> None:
        self.active = False
        self._stop.set()
        self._ticker.stop()
        process = self._avahi
        if process and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()

    # --- registre -------------------------------------------------------
    def _upsert(self, peer_id: str, name: str, host: str, port: int, source: str) -> Peer:
        with self._lock:
            peer = self._peers.get(peer_id)
            if peer is None:
                peer = Peer(peer_id, name or host, port)
                self._peers[peer_id] = peer
            if name:
                peer.name = name
            peer.port = port or peer.port
            peer.addrs[host] = source
            self._dirty = True
            return peer

    def _drop(self, peer_id: str) -> None:
        with self._lock:
            if self._peers.pop(peer_id, None) is not None:
                self._dirty = True

    def _snapshot(self) -> list[Peer]:
        with self._lock:
            peers = [
                peer
                for peer in self._peers.values()
                if peer.peer_id != self._local_id and peer.shares
            ]
        peers.sort(key=lambda peer: peer.name.lower())
        return peers

    def _apply_info(self, peer_id: str, payload: dict) -> bool:
        """Enregistre une réponse `/api/info`. Faux = le poste n'a rien à partager."""
        shares = payload.get("shares") or []
        with self._lock:
            peer = self._peers.get(peer_id)
            if peer is None:
                return False
            peer.last_ok = time.time()
            peer.failures = 0
            if peer.shares != shares:
                peer.shares = shares
                self._dirty = True
        if not shares:
            self._drop(peer_id)
            return False
        return True

    def _emit_if_dirty(self) -> None:
        with self._lock:
            dirty = self._dirty
            self._dirty = False
        if dirty:
            self.peers_changed.emit(self._snapshot())

    # --- mDNS -----------------------------------------------------------
    def _browse_mdns(self) -> None:
        while not self._stop.is_set():
            try:
                process = subprocess.Popen(
                    ["avahi-browse", "-pt", SERVICE],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.DEVNULL,
                    text=True,
                    bufsize=1,
                )
            except OSError:
                return  # avahi absent : le tailnet reste utilisable
            self._avahi = process
            assert process.stdout is not None
            for line in process.stdout:
                if self._stop.is_set():
                    break
                try:
                    self._handle_mdns(line.strip())
                except (IndexError, ValueError):
                    continue
            process.wait()
            self._avahi = None
            if self._stop.is_set():
                return
            self._stop.wait(3)  # avahi-daemon relancé : on réessaie

    def _handle_mdns(self, line: str) -> None:
        if not line:
            return
        marker = line[0]
        fields = split_fields(line)
        if marker == "=" and len(fields) >= 10:
            name, host = fields[3], fields[7]
            port = int(fields[8] or PORT)
            peer_id = parse_txt(fields[9]).get("peer_id", "")
            if not peer_id or not host or host.startswith("127.") or host == "::1":
                return
            source = "tailscale" if host.startswith(("100.", "fd7a:115c:a1e0")) else "lan"
            peer = self._upsert(peer_id, name, host, port, source)
            if peer.shares:
                return  # déjà connu et sondé : rien à ajouter
            threading.Thread(
                target=self._refresh_peer,
                args=(peer.peer_id,),
                name="gshare-refresh",
                daemon=True,
            ).start()
        elif marker == "-" and len(fields) >= 5:
            name = fields[3]
            with self._lock:
                candidates = [peer.peer_id for peer in self._peers.values() if peer.name == name]
            for peer_id in candidates:
                threading.Thread(
                    target=self._verify_peer,
                    args=(peer_id,),
                    name="gshare-verify",
                    daemon=True,
                ).start()

    # --- sondes ---------------------------------------------------------
    def _probe_addresses(self, peer_id: str, addresses: list[str], port: int) -> bool:
        for host in addresses:
            try:
                payload = api.info(api.peer_url(host, port), timeout=PROBE_TIMEOUT)
            except api.ApiError:
                continue
            if str(payload.get("peer_id") or "") not in ("", peer_id):
                continue
            with self._lock:
                peer = self._peers.get(peer_id)
                if peer is not None:
                    # L'adresse gagnante est retenue : c'est celle que la
                    # navigation et les transferts utiliseront ensuite.
                    peer.active_addr = host
            return self._apply_info(peer_id, payload)
        return False

    def _refresh_peer(self, peer_id: str) -> None:
        with self._lock:
            peer = self._peers.get(peer_id)
            addresses = peer.ordered_addrs() if peer else []
            port = peer.port if peer else PORT
        if not self._probe_addresses(peer_id, addresses, port):
            self._drop(peer_id)

    def _verify_peer(self, peer_id: str) -> None:
        """Après un `-` mDNS : on confirme avant de faire disparaître le poste."""
        with self._lock:
            peer = self._peers.get(peer_id)
            addresses = peer.ordered_addrs() if peer else []
            port = peer.port if peer else PORT
        if not addresses or not self._probe_addresses(peer_id, addresses, port):
            self._drop(peer_id)

    def _probe_loop(self) -> None:
        while not self._stop.is_set():
            self._cycle()
            self._stop.wait(PROBE_INTERVAL)

    def _cycle(self) -> None:
        """Une passe de découverte — jamais deux à la fois.

        Le rescan rapide (liste vide, toutes les 5 s) peut demander une passe
        pendant que celle de 30 s tourne encore : sans ce verrou, deux vagues de
        sondes partaient en parallèle sur les mêmes adresses.
        """
        if not self._cycle_lock.acquire(blocking=False):
            return
        try:
            self._probe_cycle()
            self._expire()
        finally:
            self._cycle_lock.release()

    def _probe_cycle(self) -> None:
        # 1. Postes déjà connus (LAN et tailnet) : vivants ? partages à jour ?
        with self._lock:
            known = [
                ("known", peer.peer_id, peer.ordered_addrs(), peer.port)
                for peer in self._peers.values()
            ]
        # 2. Postes présents sur le tailnet mais pas encore connus : l'identité
        #    vient de leur réponse, pas de `tailscale status`.
        discovered = [
            ("new", name, [address], port)
            for _id, name, address, port in self._tailscale_candidates()
        ]
        tasks = known + discovered
        with ThreadPoolExecutor(max_workers=16) as pool:
            futures = [
                pool.submit(self._probe_candidate, kind, key, addresses, port)
                for kind, key, addresses, port in tasks
            ]
            for future in futures:
                future.result()

    def _probe_candidate(self, kind: str, key: str, addresses: list[str], port: int) -> None:
        if kind == "known":
            self._probe_addresses(key, addresses, port)
            return
        for host in addresses:
            try:
                payload = api.info(api.peer_url(host, port), timeout=PROBE_TIMEOUT)
            except api.ApiError:
                continue
            remote_id = str(payload.get("peer_id") or "")
            if not remote_id or remote_id == self._local_id:
                return
            if not payload.get("shares"):
                return  # ne partage rien : n'apparaît pas
            self._upsert(
                remote_id,
                str(payload.get("name") or host),
                host,
                int(payload.get("port") or port),
                "tailscale",
            )
            self._apply_info(remote_id, payload)
            return

    def _tailscale_candidates(self) -> list[tuple[str, str, str, int]]:
        """`[(peer_id inconnu, nom, adresse, port)]` d'après `tailscale status`."""
        try:
            result = subprocess.run(
                ["tailscale", "status", "--json"],
                capture_output=True,
                text=True,
                timeout=5,
                check=False,
            )
        except (OSError, subprocess.SubprocessError):
            return []
        if result.returncode != 0:
            return []
        try:
            payload = json.loads(result.stdout)
        except ValueError:
            return []
        found = []
        for peer in (payload.get("Peer") or {}).values():
            if peer.get("Online") is False:
                continue
            addresses = [addr for addr in peer.get("TailscaleIPs") or [] if ":" not in addr]
            if not addresses:
                continue
            found.append(
                (
                    str(peer.get("ID") or ""),
                    str(peer.get("HostName") or peer.get("DNSName") or "poste"),
                    addresses[0],
                    PORT,
                )
            )
        return found

    def _expire(self) -> None:
        now = time.time()
        with self._lock:
            for peer_id, peer in list(self._peers.items()):
                if peer.last_ok and now - peer.last_ok > PROBE_INTERVAL * EXPIRE_CYCLES:
                    peer.failures += 1
                    if peer.failures >= EXPIRE_CYCLES:
                        del self._peers[peer_id]
                        self._dirty = True

    def refresh_now(self) -> None:
        """Bouton « Actualiser » (et rescan rapide) : une passe sans attendre."""
        threading.Thread(target=self._refresh_cycle, daemon=True).start()

    def _refresh_cycle(self) -> None:
        self._cycle()
        self._emit_if_dirty()

    def peers(self) -> list[Peer]:
        return self._snapshot()


def discovery_available() -> bool:
    """Au moins une source utilisable (sinon la GUI affiche une explication)."""
    return bool(shutil.which("avahi-browse") or shutil.which("tailscale"))
