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
import termios
import threading
import time

from PySide6.QtWidgets import QInputDialog, QLineEdit

from .constants import (
    AUTO_UPDATE_HELPER, CHECK_DEADLINE, KNOWN_HOSTS, PASSWORD_RE, PRIVATE_KEY,
    PUBLIC_KEY, SSH, SSH_COPY_ID, SSH_DIR, SSH_KEYGEN, SSH_KEYSCAN,
)
from .hosts import detect_mac, detect_mac_remote
from .util import _ssh_opts, address_text, display_name, ssh_target

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

def probe_key(host: dict, accept_new: bool = False) -> tuple[bool, str]:
    """La connexion sans mot de passe avec notre clé fonctionne-t-elle ?

    `accept_new` n'est utilisé que par l'installation : l'utilisateur demande
    explicitement à déposer la clé, on peut donc accepter l'empreinte d'hôte au
    premier contact (TOFU), comme le fait ssh-copy-id. La vérification
    périodique, elle, reste stricte et n'écrit jamais dans known_hosts.
    """
    if not (PRIVATE_KEY.exists() and PUBLIC_KEY.exists()):
        return False, "aucune clé locale"
    try:
        result = subprocess.run(
            [SSH, *_ssh_opts(host, accept_new=accept_new),
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
    """Empreintes SHA256 lisibles à partir d'une sortie ssh-keyscan/-F."""
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

def host_key_info(host: dict) -> tuple[str, list[str], list[str], str]:
    """Empreintes de l'hôte : « known », « unknown », « changed » ou « unreachable ».

    `ssh-keyscan` ne s'authentifie pas et n'écrit rien : il annonce simplement
    les clés publiques de la machine. On les compare à `~/.ssh/known_hosts`
    pour distinguer un premier contact (à faire valider) d'une empreinte qui a
    CHANGÉ (possible attaque de l'homme du milieu : on refuse). Le texte brut
    est renvoyé pour pouvoir ÉPINGLER exactement les clés montrées à
    l'utilisateur, sans repasser par le réseau.
    """
    names = [host["host"]]
    if host["port"] != 22:
        names = [f'[{host["host"]}]:{host["port"]}']
    else:
        names.append(f'[{host["host"]}]:22')

    stored: list[str] = []
    for name in names:
        try:
            result = subprocess.run(
                [SSH_KEYGEN, "-F", name, "-f", str(KNOWN_HOSTS)],
                capture_output=True, text=True, timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        if result.returncode == 0:
            stored.extend(_fingerprints(result.stdout))
    stored = sorted(set(stored))

    raw = ""
    scanned: list[str] = []
    try:
        result = subprocess.run(
            [SSH_KEYSCAN, "-T", "5", "-p", str(host["port"]), host["host"]],
            capture_output=True, text=True, timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired):
        result = None
    if result is not None and result.returncode == 0:
        raw = "\n".join(
            line for line in result.stdout.splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        )
        scanned = sorted(set(_fingerprints(raw)))

    if stored and scanned:
        if set(stored) & set(scanned):
            return "known", scanned, stored, raw
        return "changed", scanned, stored, raw
    if stored:
        return "known", stored, stored, ""
    if scanned:
        return "unknown", scanned, stored, raw
    return "unreachable", [], stored, ""

def trust_host_key(raw_keys: str) -> bool:
    """Épingle dans known_hosts les lignes ssh-keyscan acceptées par l'utilisateur.

    On écrit les clés EXACTES qui ont été affichées (pas celles d'une nouvelle
    connexion) : si une autre clé se présente ensuite, ssh la refuse. C'est ce
    qui évite le `accept-new` silencieux et sa fenêtre d'attaque.
    """
    lines = [
        line.strip() for line in raw_keys.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    if not lines:
        return False
    try:
        SSH_DIR.mkdir(parents=True, exist_ok=True)
        os.chmod(SSH_DIR, 0o700)
        mode = "a" if KNOWN_HOSTS.exists() else "w"
        with open(KNOWN_HOSTS, mode, encoding="utf-8") as handle:
            if mode == "a" and KNOWN_HOSTS.stat().st_size:
                handle.write("\n")
            handle.write("\n".join(lines) + "\n")
        os.chmod(KNOWN_HOSTS, 0o600)
    except OSError:
        return False
    return True

def check_host(host: dict, stop: threading.Event) -> tuple[str, str, float, str]:
    """État d'un poste, sans bloquer la GUI.

    Retourne (état, détail, latence ms, MAC détectée). La latence est le temps
    du handshake SSH complet (connexion + authentification) : c'est la seule
    mesure sans ouvrir une connexion TCP supplémentaire à chaque vérification.
    """
    if not (PRIVATE_KEY.exists() and PUBLIC_KEY.exists()):
        return "nokey", "la clé locale sera créée à l'installation", 0.0, ""

    command = [
        SSH, *_ssh_opts(host),
        ssh_target(host), "true",
    ]
    started = time.monotonic()
    try:
        proc = subprocess.Popen(
            command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
    except OSError as exc:
        return "error", f"ssh introuvable ({exc})", 0.0, ""

    deadline = time.monotonic() + CHECK_DEADLINE
    while proc.poll() is None:
        if stop.is_set():
            proc.kill()
            proc.wait()
            return "unknown", "", 0.0, ""
        if time.monotonic() > deadline:
            proc.kill()
            try:
                proc.communicate(timeout=2)
            except subprocess.TimeoutExpired:
                pass
            return "timeout", "délai dépassé", 0.0, ""
        time.sleep(0.08)

    try:
        _, err = proc.communicate(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
        _, err = proc.communicate()
    latency = (time.monotonic() - started) * 1000.0
    if proc.returncode == 0:
        mac = ""
        if not host.get("mac"):
            mac = detect_mac(host) or detect_mac_remote(host)
        return "online", "", latency, mac

    text = (err or "").strip()
    low = text.lower()
    if "host key verification failed" in low:
        return "hostkey", "empreinte inconnue ou changée", 0.0, ""
    if "permission denied" in low:
        return "auth", "clé refusée", 0.0, ""
    if "connection refused" in low:
        return "refused", "connexion refusée", 0.0, ""
    if "no route to host" in low:
        return "noroute", "réseau injoignable", 0.0, ""
    if "could not resolve" in low or "name or service not known" in low:
        return "dns", "nom introuvable", 0.0, ""
    if "timed out" in low or "timeout" in low:
        return "timeout", "délai dépassé", 0.0, ""
    first = text.splitlines()[0] if text else f"code {proc.returncode}"
    return "error", first, 0.0, ""

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
                ask_trust=None) -> tuple[bool, str]:
    """Crée la clé si besoin, la copie avec ssh-copy-id, puis vérifie.

    `ask_trust(empreintes)` est appelé au premier contact : la GUI affiche les
    empreintes et attend l'accord de l'utilisateur. Une empreinte qui a CHANGÉ
    n'est jamais acceptée ici, même si l'appelant dirait oui : c'est le signe
    classique d'une attaque de l'homme du milieu.
    """
    ok, error = ensure_local_key()
    if not ok:
        return False, error
    log(f"Clé locale : {PRIVATE_KEY}\n")

    state, scanned, stored, raw = host_key_info(host)
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
        if not trust_host_key(raw):
            return False, "impossible d'enregistrer l'empreinte dans ~/.ssh/known_hosts"
        log("Empreinte acceptée et enregistrée dans known_hosts.\n")
    elif state == "unreachable":
        log("Empreinte d'hôte indisponible (ssh-keyscan).\n")
        return False, ("empreinte d'hôte indisponible — poste injoignable, "
                       "réessayez dans un instant")

    ok, _ = probe_key(host)
    if ok:
        log("Cette clé est déjà acceptée par le poste.\n")
        return True, "clé déjà installée"

    if not password:
        return False, "mot de passe requis pour copier la clé"

    log(f"Copie de {PUBLIC_KEY.name} vers {address_text(host)}…\n")
    command = [
        SSH_COPY_ID,
        "-i", str(PUBLIC_KEY),
        "-p", str(host["port"]),
        "-o", "StrictHostKeyChecking=yes",
        "-o", "ConnectTimeout=10",
        "-o", "NumberOfPasswordPrompts=2",
        ssh_target(host),
    ]
    code, _ = _run_with_password(command, password, log, cancel)

    ok, detail = probe_key(host)
    if ok:
        return True, "clé installée, connexion sans mot de passe vérifiée"
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

    On sonde un helper autorisé en NOPASSWD (`gablue-auto-update status`), PAS
    `sudo -n true` : aucune règle sudoers ne vise `true`, même sur Gablue, donc
    la sonde échouait toujours et TOUTE action root distante (redémarrage,
    extinction, `bootc upgrade` pourtant autorisé) réclamait le mot de passe.
    Même piège et même remède que `privileged_prefix()` en local.
    """
    code, _ = ssh_capture(
        host, f"sudo -n {AUTO_UPDATE_HELPER} status", timeout=10
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
