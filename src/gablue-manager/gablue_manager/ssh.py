"""Connexions ssh : empreintes, copie de clé, exécution distante.
"""

import fcntl
import getpass
import os
import pty
import select
import signal
import socket
import subprocess
import tempfile
import termios
import threading
import time
from pathlib import Path

from PySide6.QtWidgets import QInputDialog, QLineEdit

from .constants import (
    CHECK_DEADLINE, CPUID_EMU_HELPER, KNOWN_HOSTS, PASSWORD_RE, PRIVATE_KEY,
    PUBLIC_KEY, SSH, SSH_COPY_ID, SSH_DIR, SSH_KEYGEN,
)
from .util import _ssh_opts, address_text, display_name, host_addresses, ssh_target

def ensure_local_key() -> tuple[bool, str]:
    """Crée ~/.ssh/id_ed25519 (sans passphrase) si elle manque."""
    if PRIVATE_KEY.exists() and PUBLIC_KEY.exists():
        return True, ""
    try:
        SSH_DIR.mkdir(parents=True, exist_ok=True)
        os.chmod(SSH_DIR, 0o700)
    except OSError as exc:
        return False, f"~/.ssh inaccessible ({exc})"
    try:
        result = subprocess.run(
            [SSH_KEYGEN, "-t", "ed25519", "-N", "", "-q",
             "-C", f"{getpass.getuser()}@{socket.gethostname()}",
             "-f", str(PRIVATE_KEY)],
            capture_output=True, text=True, timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return False, f"ssh-keygen a échoué ({exc})"
    if result.returncode != 0 or not PUBLIC_KEY.exists():
        detail = (result.stderr or result.stdout or "").strip()
        return False, f"création de la clé impossible : {detail}"
    return True, ""

def probe_key(host: dict) -> tuple[bool, str]:
    """La connexion sans mot de passe avec notre clé fonctionne-t-elle ?

    Toujours stricte (`StrictHostKeyChecking=yes`) : on ne teste que notre clé,
    et on n'écrit jamais dans known_hosts ici — l'épinglage est un geste
    explicite de l'utilisateur (`trust_host_key`).
    """
    if not (PRIVATE_KEY.exists() and PUBLIC_KEY.exists()):
        return False, "aucune clé locale"
    try:
        result = subprocess.run(
            [SSH, *_ssh_opts(host),
             ssh_target(host), "true"],
            capture_output=True, text=True, timeout=12,
        )
    except subprocess.TimeoutExpired:
        return False, "délai dépassé"
    except OSError as exc:
        return False, f"ssh introuvable ({exc})"
    if result.returncode == 0:
        return True, ""
    detail = (result.stderr or "").strip().splitlines()
    return False, detail[-1] if detail else f"ssh a retourné {result.returncode}"

def _fingerprints(keys_text: str) -> list[str]:
    """Empreintes SHA256 lisibles à partir de lignes known_hosts (-F)."""
    if not keys_text.strip():
        return []
    try:
        result = subprocess.run(
            [SSH_KEYGEN, "-lf", "-"], input=keys_text,
            capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    prints = []
    for line in result.stdout.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        kind = line.rsplit("(", 1)[1].rstrip(")") if "(" in line else "clé"
        prints.append(f"{kind:<10} {parts[1]}")
    return prints

PENALTY_MARKS = (
    "not allowed at this time",
    "connection reset by",
)

def looks_penalized(text: str) -> bool:
    """Le poste a-t-il bloqué temporairement cette machine ?

    OpenSSH 9.8+ (`PerSourcePenalties`, actif par défaut) compte chaque
    connexion fermée AVANT authentification et bloque la source : sshd répond
    alors « Not allowed at this time » ou coupe la connexion pendant le délai.
    Une rafale de sondes et de `ssh-keyscan` en fabrique assez pour se faire
    bannir soi-même — d'où ce diagnostic explicite, au lieu d'un
    « injoignable » qui laissait croire à un problème réseau.
    """
    low = (text or "").lower()
    return any(mark in low for mark in PENALTY_MARKS)

def scan_host_keys(host: dict) -> tuple[str, bool, str]:
    """Clés d'hôte de la machine, en UNE connexion (known_hosts jetable).

    `ssh-keyscan` ouvre une connexion PAR type de clé — six d'affilée — et
    chacune compte comme une offense pour `PerSourcePenalties` (authfail 5 s,
    blocage à partir de 15 s accumulées). Une seule connexion ssh avec un
    known_hosts temporaire donne les mêmes clés pour une seule offense ; le
    fichier temporaire reçoit exactement les clés annoncées, qu'on épingle
    ensuite (jamais `accept-new` sur le vrai fichier).

    La même connexion teste l'authentification : code 0 = notre clé est DÉJÀ
    acceptée, inutile de refaire une sonde (chaque échec de plus rapproche du
    blocage). Retourne (clés brutes, clé acceptée, erreur ssh).
    """
    handle, path = tempfile.mkstemp(prefix="gablue-hostkey-")
    os.close(handle)
    authenticated = False
    error = ""
    try:
        try:
            result = subprocess.run(
                [SSH,
                 "-T",
                 "-i", str(PRIVATE_KEY),
                 "-o", "IdentitiesOnly=yes",
                 "-o", "BatchMode=yes",
                 "-o", "StrictHostKeyChecking=no",
                 "-o", f"UserKnownHostsFile={path}",
                 "-o", "GlobalKnownHostsFile=/dev/null",
                 "-o", "UpdateHostKeys=no",
                 "-o", "ConnectTimeout=5",
                 "-p", str(host["port"]),
                 ssh_target(host), "true"],
                capture_output=True, text=True, timeout=20,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            return "", False, str(exc)
        authenticated = result.returncode == 0
        error = (result.stderr or "").strip()
        try:
            raw = Path(path).read_text(encoding="utf-8", errors="replace")
        except OSError:
            raw = ""
        return raw, authenticated, error
    finally:
        try:
            os.unlink(path)
        except OSError:
            pass

def _stored_fingerprints(address: str, port: int) -> list[str]:
    """Empreintes connues pour cette adresse (forme simple et `[hôte]:port`)."""
    names = [address] if port == 22 else [f"[{address}]:{port}"]
    if port == 22:
        names.append(f"[{address}]:22")
    found: list[str] = []
    for name in names:
        try:
            result = subprocess.run(
                [SSH_KEYGEN, "-F", name, "-f", str(KNOWN_HOSTS)],
                capture_output=True, text=True, timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        if result.returncode == 0:
            found.extend(_fingerprints(result.stdout))
    return found

def host_key_state(host: dict, raw_keys: str) -> tuple[str, list[str], list[str]]:
    """« known », « unknown », « changed » ou « unreachable » + empreintes.

    On compare les clés annoncées à `~/.ssh/known_hosts` — pour TOUTES les
    adresses du poste, LAN comme Tailscale — afin de distinguer un premier
    contact (à faire valider) d'une empreinte qui a CHANGÉ (possible attaque de
    l'homme du milieu : on refuse).
    """
    stored = sorted({
        item for address in host_addresses(host)
        for item in _stored_fingerprints(address, host["port"])
    })
    scanned = sorted(set(_fingerprints(raw_keys)))
    if stored and scanned:
        if set(stored) & set(scanned):
            return "known", scanned, stored
        return "changed", scanned, stored
    if stored:
        return "known", stored, stored
    if scanned:
        return "unknown", scanned, stored
    return "unreachable", [], stored

def key_status(host: dict) -> dict:
    """État de la clé en UNE connexion (empreinte + clé déjà acceptée ?).

    Sert de sonde au dialogue : le résultat est réutilisé par l'installation
    (`prescan`), pour ne pas payer une deuxième connexion fermée sans
    authentification — c'est ce qui déclenchait le blocage `PerSourcePenalties`.
    """
    raw, authenticated, error = scan_host_keys(host)
    state, scanned, stored = host_key_state(host, raw)
    return {
        "authenticated": authenticated,
        "state": state,
        "raw": raw,
        "fingerprints": scanned,
        "stored": stored,
        "error": error,
    }

def pinned_entries(raw_keys: str, addresses: list[str], port: int) -> list[str]:
    """Lignes known_hosts à écrire : les clés annoncées, sous chaque adresse.

    Fonction pure (testable) : on reprend le type et la clé tels qu'affichés,
    et on ne garde du champ hôte que ce qu'on maîtrise — un known_hosts haché
    (`HashKnownHosts yes`) est donc réécrit en clair pour nos adresses.
    """
    entries: list[str] = []
    seen: set[str] = set()
    for line in (raw_keys or "").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        keytype, keydata = parts[1], parts[2]
        for address in addresses:
            name = address if port == 22 else f"[{address}]:{port}"
            entry = f"{name} {keytype} {keydata}"
            if entry not in seen:
                seen.add(entry)
                entries.append(entry)
    return entries

def trust_host_key(raw_keys: str, host: dict) -> bool:
    """Épingle les clés EXACTES affichées, sous toutes les adresses du poste.

    On écrit les clés montrées à l'utilisateur (pas celles d'une nouvelle
    connexion) : si une autre clé se présente ensuite, ssh la refuse. C'est ce
    qui évite le `accept-new` silencieux et sa fenêtre d'attaque. Elles sont
    épinglées sous chaque adresse connue — même machine, même clé — pour que
    passer du LAN à Tailscale ne redéclenche pas une demande de confiance.
    """
    entries = pinned_entries(raw_keys, host_addresses(host), host["port"])
    if not entries:
        return False
    try:
        SSH_DIR.mkdir(parents=True, exist_ok=True)
        os.chmod(SSH_DIR, 0o700)
        mode = "a" if KNOWN_HOSTS.exists() else "w"
        with open(KNOWN_HOSTS, mode, encoding="utf-8") as handle:
            if mode == "a" and KNOWN_HOSTS.stat().st_size:
                handle.write("\n")
            handle.write("\n".join(entries) + "\n")
        os.chmod(KNOWN_HOSTS, 0o600)
    except OSError:
        return False
    return True

def _probe_address(host: dict, address: str,
                   stop: threading.Event) -> tuple[str, str, float]:
    """État du poste vu par une adresse précise (état, détail, latence ms)."""
    command = [SSH, *_ssh_opts(host), ssh_target(host, address), "true"]
    started = time.monotonic()
    try:
        proc = subprocess.Popen(
            command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
    except OSError as exc:
        return "error", f"ssh introuvable ({exc})", 0.0

    deadline = time.monotonic() + CHECK_DEADLINE
    while proc.poll() is None:
        if stop.is_set():
            proc.kill()
            proc.wait()
            return "unknown", "", 0.0
        if time.monotonic() > deadline:
            proc.kill()
            try:
                proc.communicate(timeout=2)
            except subprocess.TimeoutExpired:
                pass
            return "timeout", "délai dépassé", 0.0
        time.sleep(0.08)

    try:
        _, err = proc.communicate(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
        _, err = proc.communicate()
    latency = (time.monotonic() - started) * 1000.0
    if proc.returncode == 0:
        return "online", "", latency

    text = (err or "").strip()
    low = text.lower()
    if "host key verification failed" in low:
        return "hostkey", "empreinte inconnue ou changée", 0.0
    if looks_penalized(text):
        return "penalty", "le poste bloque temporairement cette machine", 0.0
    if "permission denied" in low:
        return "auth", "clé refusée", 0.0
    if "connection refused" in low:
        return "refused", "connexion refusée", 0.0
    if "no route to host" in low:
        return "noroute", "réseau injoignable", 0.0
    if "could not resolve" in low or "name or service not known" in low:
        return "dns", "nom introuvable", 0.0
    if "timed out" in low or "timeout" in low:
        return "timeout", "délai dépassé", 0.0
    first = text.splitlines()[0] if text else f"code {proc.returncode}"
    return "error", first, 0.0

# États qui prouvent qu'on a bien atteint le poste : inutile d'essayer son
# adresse de secours (le LAN répond « clé refusée », ce n'est pas une panne).
REACHED_STATES = ("online", "auth", "hostkey", "penalty")

def check_host(host: dict, stop: threading.Event) -> tuple[str, str, float, str]:
    """État d'un poste, sans bloquer la GUI.

    Retourne (état, détail, latence ms, adresse utilisée). Les adresses sont
    essayées dans l'ordre — principale puis secours — pour qu'un poste
    enregistré en LAN reste joignable par Tailscale en déplacement (et
    inversement). La latence est celle du handshake SSH complet : pas de
    connexion TCP nue en plus, sinon sshd verrait une sonde vide de plus.
    """
    if not (PRIVATE_KEY.exists() and PUBLIC_KEY.exists()):
        return "nokey", "la clé locale sera créée à l'installation", 0.0, ""

    addresses = host_addresses(host)
    last = ("timeout", "délai dépassé", 0.0, addresses[0])
    deadline = time.monotonic() + CHECK_DEADLINE * max(1, len(addresses))
    for address in addresses:
        if time.monotonic() > deadline:
            break
        state, detail, latency = _probe_address(host, address, stop)
        if state == "unknown":          # vérification annulée
            return "unknown", "", 0.0, ""
        last = (state, detail, latency, address)
        if state in REACHED_STATES:
            break
    return last

def _kill_group(proc: subprocess.Popen) -> None:
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    except OSError:
        return
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except OSError:
            pass

def _run_with_password(command: list[str], password: str,
                       log, cancel: threading.Event | None = None) -> tuple[int, str]:
    """Lance une commande interactive dans un pty et répond aux invites.

    ssh-copy-id demande le mot de passe sur le terminal : on lui donne un vrai
    pty (session + terminal de contrôle) et on écrit le secret dès qu'une invite
    « password: » apparaît. Mot de passe jamais dans argv, donc invisible dans ps.
    """
    master, slave = pty.openpty()

    def _attach_tty() -> None:
        os.setsid()
        try:
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        except OSError:
            pass

    try:
        proc = subprocess.Popen(
            command, stdin=slave, stdout=slave, stderr=slave,
            preexec_fn=_attach_tty, close_fds=True,
            env={**os.environ, "TERM": "dumb"},
        )
    except OSError as exc:
        os.close(master)
        os.close(slave)
        return 127, f"{command[0]} introuvable ({exc})"
    os.close(slave)

    output = ""
    answered_at = -1
    deadline = time.monotonic() + 60
    try:
        while True:
            if cancel is not None and cancel.is_set():
                _kill_group(proc)
                output += "\n[annulé]\n"
                break
            if time.monotonic() > deadline:
                _kill_group(proc)
                output += "\n[délai dépassé]\n"
                break
            ready, _, _ = select.select([master], [], [], 0.2)
            if ready:
                try:
                    chunk = os.read(master, 4096)
                except OSError:
                    chunk = b""
                if not chunk:
                    break
                text = chunk.decode("utf-8", "replace")
                output += text
                log(text)
                tail = output[-8192:]
                matches = list(PASSWORD_RE.finditer(tail))
                if matches:
                    end = len(output) - len(tail) + matches[-1].end()
                    if end > answered_at:
                        os.write(master, (password + "\n").encode())
                        answered_at = end
            elif proc.poll() is not None:
                while True:
                    more, _, _ = select.select([master], [], [], 0)
                    if not more:
                        break
                    try:
                        chunk = os.read(master, 4096)
                    except OSError:
                        chunk = b""
                    if not chunk:
                        break
                    text = chunk.decode("utf-8", "replace")
                    output += text
                    log(text)
                break
    finally:
        try:
            os.close(master)
        except OSError:
            pass

    try:
        code = proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        _kill_group(proc)
        code = proc.returncode if proc.returncode is not None else 1
    return code, output

def install_key(host: dict, password: str, log, cancel=None,
                ask_trust=None, prescan: dict | None = None) -> tuple[bool, str]:
    """Crée la clé si besoin, la copie avec ssh-copy-id, puis vérifie.

    `ask_trust(empreintes)` est appelé au premier contact : la GUI affiche les
    empreintes et attend l'accord de l'utilisateur. Une empreinte qui a CHANGÉ
    n'est jamais acceptée ici, même si l'appelant dirait oui : c'est le signe
    classique d'une attaque de l'homme du milieu.

    `prescan` est le résultat de `key_status()` déjà obtenu par le dialogue :
    il porte les clés brutes ET le fait que notre clé est refusée. Le réutiliser
    évite une seconde connexion sans authentification, chaque échec comptant
    pour `PerSourcePenalties` (blocage du client à partir de 15 s accumulées).
    """
    ok, error = ensure_local_key()
    if not ok:
        return False, error
    log(f"Clé locale : {PRIVATE_KEY}\n")

    info = prescan if prescan is not None else key_status(host)
    raw = info.get("raw", "")
    state = info.get("state", "unreachable")
    scanned = list(info.get("fingerprints") or [])
    stored = list(info.get("stored") or [])

    if info.get("authenticated"):
        log("Cette clé est déjà acceptée par le poste.\n")
        return True, "clé déjà installée"

    if raw:
        # L'empreinte a pu être épinglée depuis le pré-scan (nouvel essai après
        # « Réessayer ») : la comparaison est locale et gratuite, on la refait
        # pour ne pas redemander une confiance déjà donnée.
        state, scanned, stored = host_key_state(host, raw)

    if state == "changed":
        log("Empreinte d'hôte DIFFÉRENTE de celle enregistrée.\n")
        log("  connue   : " + (", ".join(stored) or "?") + "\n")
        log("  annoncée : " + (", ".join(scanned) or "?") + "\n")
        return False, ("l'empreinte de l'hôte a changé — possible attaque ; "
                       "vérifiez la machine avant de continuer")
    if state == "unknown":
        log("Premier contact : empreintes annoncées par l'hôte\n")
        for item in scanned:
            log(f"  {item}\n")
        if ask_trust is None:
            return False, "empreinte d'hôte inconnue : acceptation explicite requise"
        if not ask_trust(scanned):
            return False, "empreinte refusée"
        if not trust_host_key(raw, host):
            return False, "impossible d'enregistrer l'empreinte dans ~/.ssh/known_hosts"
        log("Empreinte acceptée et enregistrée dans known_hosts.\n")
    elif state == "unreachable":
        log("Empreinte d'hôte indisponible (aucune clé annoncée).\n")
        # Ne pas relancer une connexion de diagnostic : c'est justement la
        # rafale de connexions qui fait bloquer la source par sshd.
        if looks_penalized(info.get("error", "")):
            return False, ("le poste a temporairement bloqué cette machine "
                           "(limite sshd) — attendez une minute, puis réessayez")
        return False, ("aucune clé annoncée par le poste — injoignable, ou "
                       "connexions temporairement limitées (sshd) ; attendez "
                       "une minute puis réessayez")

    if not password:
        return False, "mot de passe requis pour copier la clé"

    log(f"Copie de {PUBLIC_KEY.name} vers {address_text(host)}…\n")
    command = [
        SSH_COPY_ID,
        # `-f` : sans lui, ssh-copy-id ouvre une connexion de plus pour tester
        # si la clé est déjà en place — or le scan vient de prouver qu'elle est
        # refusée, et chaque connexion en échec compte pour
        # `PerSourcePenalties` (blocage du client). Au pire, une clé présente
        # mais inopérante (mauvais droits) est réécrite une seconde fois.
        "-f",
        "-i", str(PUBLIC_KEY),
        "-p", str(host["port"]),
        "-o", "StrictHostKeyChecking=yes",
        "-o", "ConnectTimeout=10",
        "-o", "NumberOfPasswordPrompts=2",
        ssh_target(host),
    ]
    code, output = _run_with_password(command, password, log, cancel)

    low = (output or "").lower()
    if code != 0:
        if looks_penalized(output):
            # Le poste a coupé la connexion : ce n'est ni le mot de passe ni
            # la clé, inutile d'ajouter une sonde de plus.
            return False, ("le poste a temporairement bloqué cette machine "
                           "(limite sshd) — attendez une minute, puis réessayez")
        if ("permission denied" in low or "authentication failed" in low
                or "no supported authentication" in low):
            return False, "mot de passe refusé par le poste"

    # Le code retour de ssh-copy-id n'est pas fiable : seule la sonde muette
    # dit si la copie a réellement pris. Elle réussit dans le cas nominal,
    # donc elle n'ajoute aucune offense.
    ok, detail = probe_key(host)
    if ok:
        return True, "clé installée, connexion sans mot de passe vérifiée"
    if looks_penalized(detail):
        return False, ("le poste a temporairement bloqué cette machine (limite "
                       "sshd) — attendez une minute, puis réessayez")
    if detail:
        return False, f"la clé n'est pas acceptée après la copie ({detail})"
    return False, f"ssh-copy-id a échoué (code {code})"

def ssh_capture(host: dict, command: str,
                timeout: float = 12.0) -> tuple[int, str]:
    """Exécute une commande distante et rend (code, sortie)."""
    try:
        result = subprocess.run(
            [SSH, *_ssh_opts(host), ssh_target(host), command],
            capture_output=True, text=True, timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired):
        return 255, ""
    return result.returncode, result.stdout

def remote_sudo_nopasswd(host: dict) -> bool:
    """`sudo -n` passe-t-il sur ce poste (image Gablue) ?

    On sonde un helper autorisé en NOPASSWD (`gablue-cpuid-emu status`), PAS
    `sudo -n true` : aucune règle sudoers ne vise `true`, même sur Gablue, donc
    la sonde échouait toujours et TOUTE action root distante (redémarrage,
    extinction, `bootc upgrade` pourtant autorisé) réclamait le mot de passe.
    Même piège et même remède que `privileged_prefix()` en local. `status` de
    `gablue-cpuid-emu` se contente d'imprimer on/off et sort 0 ; celui de
    `gablue-auto-update` sort 1 dès qu'un timer est désactivé, ce qui faisait
    croire à une règle absente (donc mot de passe) sur un poste dont les mises
    à jour automatiques étaient coupées — vécu 07/11/2026.
    """
    code, _ = ssh_capture(
        host, f"sudo -n {CPUID_EMU_HELPER} status", timeout=10
    )
    return code == 0

def remote_sudo_prefix(password: str) -> str:
    """Ligne qui valide sudo une fois pour toutes (mot de passe sur stdin)."""
    if not password:
        return ""
    return "sudo -S -p '' -v || exit 1\n"

def ask_remote_password(parent, host: dict) -> str | None:
    """Mot de passe sudo du poste distant ; jamais enregistré, jamais en argv."""
    text, ok = QInputDialog.getText(
        parent, "Autorisation root",
        f"Mot de passe de « {host['user']} » sur {display_name(host)} :\n"
        "sudo le demande sur ce poste ; il n'est ni affiché ni enregistré.",
        QLineEdit.Password,
    )
    return text if ok else None
