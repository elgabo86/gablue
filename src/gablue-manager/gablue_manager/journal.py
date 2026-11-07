"""Journal des échecs vers journald — aucun fichier, aucune rotation.

Gablue Manager ne tient pas de journal persistant : les fenêtres de tâche
gardent leur sortie **en mémoire** (bornée par Qt) et le seul log qui survit à
la session est celui de systemd. On y écrit donc **les échecs et les
exceptions**, pas les sondes périodiques : quelques entrées par mois, sans
bruit, et c'est journald qui gère la taille (`SystemMaxUse`, expiration,
`RateLimitBurst`) — aucun quota à maintenir ici.

    journalctl --user -t gablue-manager -b
    journalctl -t gablue-manager --since -1d

Le protocole natif (`/run/systemd/journal/socket`) est utilisé en priorité :
il accepte des champs structurés et ne dépend d'aucun démon. Repli sur
`/dev/log` (syslog) si le socket natif manque. Toute erreur d'envoi est
silencieuse : journaliser ne doit jamais casser une action.
"""

import os
import socket
import sys
import time
import traceback

IDENTIFIER = "gablue-manager"
NATIVE_SOCKET = "/run/systemd/journal/socket"
SYSLOG_SOCKET = "/dev/log"

PRIORITY_ERR = 3
PRIORITY_WARNING = 4
PRIORITY_INFO = 6

# Un champ trop gros ferait un datagramme inutile : journald refuserait ou
# gonflerait le disque pour rien. 8 Kio suffisent très largement (une pile
# d'exception ou la fin d'un journal de tâche).
MAX_VALUE = 8 * 1024

# Anti-spam, côté application : journald a son propre rate-limit, mais il
# laisserait passer un bug en boucle. Deux garde-fous, volontairement simples :
# une même erreur (même première ligne, même priorité) n'est écrite qu'une fois
# par fenêtre de 10 minutes, et un processus n'écrit jamais plus de 200 entrées
# (une dernière ligne annonce la coupure). Les sondes périodiques ne journalisent
# rien : seuls les échecs et exceptions empruntent ce module.
DEDUP_SECONDS = 600
MAX_ENTRIES = 200

_seen: dict[str, float] = {}
_sent = 0

def _clip(text: str, limit: int = MAX_VALUE) -> str:
    text = str(text)
    if len(text) <= limit:
        return text
    return text[:limit] + "\n[… tronqué]"

def encode(fields: dict) -> bytes:
    """Datagramme du protocole natif : `CLÉ=valeur` par ligne.

    Une valeur ne peut pas contenir de saut de ligne brut : il s'écrit `\\n`
    (séquence littérale), que journald redécode à la lecture.
    """
    lines = []
    for key, value in fields.items():
        lines.append(f"{key}=" + _clip(value).replace("\n", "\\n"))
    # Le dernier champ doit lui aussi être terminé par un saut de ligne :
    # journald ignore un champ final non terminé (constaté 07/11/2026 —
    # GABLUE_CODE disparaissait de l'entrée).
    return ("\n".join(lines) + "\n").encode("utf-8", "replace")

def _send(data: bytes) -> bool:
    try:
        if os.path.exists(NATIVE_SOCKET):
            with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as sock:
                sock.connect(NATIVE_SOCKET)
                sock.send(data)
            return True
        if os.path.exists(SYSLOG_SOCKET):
            # Repli syslog classique : priorité + identifiant + message.
            with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as sock:
                sock.connect(SYSLOG_SOCKET)
                sock.send(b"<3>" + IDENTIFIER.encode() + b": " + data)
            return True
    except OSError:
        return False
    return False

def log(message: str, priority: int = PRIORITY_INFO, **fields) -> bool:
    """Écrit une entrée, sous réserve de l'anti-spam ; jamais d'erreur.

    False si l'entrée est écartée (doublon récent, plafond atteint) ou si
    journald n'est pas joignable.
    """
    global _sent
    if _sent >= MAX_ENTRIES:
        return False
    signature = f"{int(priority)}|" + str(message).splitlines()[0][:200]
    now = time.monotonic()
    last = _seen.get(signature)
    if last is not None and now - last < DEDUP_SECONDS:
        return False
    _seen[signature] = now
    _sent += 1

    payload = {
        "MESSAGE": message,
        "PRIORITY": str(int(priority)),
        "SYSLOG_IDENTIFIER": IDENTIFIER,
    }
    for field, value in fields.items():
        if value is None:
            continue
        payload["GABLUE_" + str(field).upper()] = value
    sent = _send(encode(payload))
    if _sent == MAX_ENTRIES:
        # Une seule ligne pour dire que la suite est coupée : sans elle, un
        # journal qui s'arrête net se lit comme un plantage.
        _send(encode({
            "MESSAGE": f"Limite de {MAX_ENTRIES} entrées atteinte : journal "
                       "suspendu pour cette session.",
            "PRIORITY": str(PRIORITY_WARNING),
            "SYSLOG_IDENTIFIER": IDENTIFIER,
        }))
    return sent

def log_failure(title: str, detail: str = "", code: int | None = None,
                **fields) -> bool:
    """Échec d'action : titre, code retour et fin du journal de la tâche."""
    message = title
    if code is not None:
        message += f" — échec (code {code})"
    if detail:
        message += "\n" + detail.strip()[-MAX_VALUE:]
    if code is not None:
        fields.setdefault("code", code)
    return log(message, PRIORITY_ERR, **fields)

def install_excepthook() -> None:
    """Journalise les exceptions non attrapées, puis laisse le comportement Qt.

    Appelé une fois au démarrage : une exception dans un slot ou un thread
    laisse une trace dans journald, même si la fenêtre a été lancée depuis le
    menu et que sa stderr n'existe plus.
    """
    previous = sys.excepthook

    def hook(exc_type, exc, tb):
        try:
            log_failure(
                f"Exception non gérée : {exc_type.__name__}",
                detail="".join(traceback.format_exception(exc_type, exc, tb)),
            )
        except Exception:
            pass
        previous(exc_type, exc, tb)

    sys.excepthook = hook
