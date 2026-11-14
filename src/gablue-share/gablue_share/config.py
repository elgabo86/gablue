"""La liste des dossiers partagés, sur disque.

`~/.config/gablue/share.json` est la **seule source de vérité** : elle survit à
la fermeture de l'app, au redémarrage et au mode permanent, et c'est elle que
le démon relit à chaud (mtime) sans redémarrer ni couper un transfert.
"""

import json
import os
import tempfile
import time
import uuid
from pathlib import Path

from .constants import CONFIG_DIR, CONFIG_PATH, PORT


def machine_id() -> str:
    """Identité stable du poste : jamais le nom d'hôte ni l'adresse."""
    try:
        value = Path("/etc/machine-id").read_text(encoding="utf-8").strip()
        if value:
            return value
    except OSError:
        pass
    return uuid.uuid4().hex


def hostname() -> str:
    try:
        value = Path("/etc/hostname").read_text(encoding="utf-8").strip()
        if value:
            return value
    except OSError:
        pass
    return os.environ.get("HOSTNAME") or "gablue"


def default_document() -> dict:
    return {
        "version": 1,
        "peer_id": machine_id(),
        "name": hostname(),
        "port": PORT,
        "shares": [],
    }


def load() -> dict:
    """Charge la configuration, la crée au premier lancement.

    Fichier illisible ou invalide : renommé `share.json.corrupt-<horodatage>`
    puis recréé — on n'écrase jamais en silence ce que l'utilisateur avait.
    """
    CONFIG_DIR.mkdir(parents=True, exist_ok=True, mode=0o700)
    if not CONFIG_PATH.exists():
        document = default_document()
        save(document)
        return document
    try:
        raw = CONFIG_PATH.read_text(encoding="utf-8")
        document = json.loads(raw)
        if not isinstance(document, dict):
            raise ValueError("objet JSON attendu")
    except (OSError, ValueError):
        backup = CONFIG_PATH.with_name(f"share.json.corrupt-{int(time.time())}")
        try:
            CONFIG_PATH.replace(backup)
        except OSError:
            pass
        document = default_document()
        save(document)
        return document

    document.setdefault("version", 1)
    document.setdefault("peer_id", machine_id())
    document.setdefault("name", hostname())
    document.setdefault("port", PORT)
    if not isinstance(document.get("shares"), list):
        document["shares"] = []
    cleaned = []
    for entry in document["shares"]:
        if not isinstance(entry, dict):
            continue
        path = str(entry.get("path", "")).strip()
        if not path:
            continue
        cleaned.append(
            {
                "id": str(entry.get("id") or f"s{uuid.uuid4().hex[:8]}"),
                "name": str(entry.get("name") or Path(path).name or "Partage"),
                "path": path,
            }
        )
    document["shares"] = cleaned
    return document


def save(document: dict) -> None:
    """Écriture atomique : temporaire + `os.replace`, dossier 0700, fichier 0600."""
    CONFIG_DIR.mkdir(parents=True, exist_ok=True, mode=0o700)
    payload = json.dumps(document, ensure_ascii=False, indent=2) + "\n"
    handle, temporary = tempfile.mkstemp(dir=str(CONFIG_DIR), prefix=".share-", suffix=".json")
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, CONFIG_PATH)
    except OSError:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise


def share_key(entry: dict) -> str:
    """Chemin réel : deux écritures différentes du même dossier = un seul partage."""
    return str(Path(entry.get("path", "")).expanduser().resolve())


def add_share(document: dict, path: str, name: str = "") -> tuple[bool, str]:
    """Ajoute un dossier à la liste permanente. Retourne (ok, message)."""
    candidate = Path(path).expanduser()
    try:
        resolved = candidate.resolve()
    except OSError:
        return False, "Chemin illisible."
    if not resolved.is_dir():
        return False, "Ce n'est pas un dossier."
    known = {share_key(entry) for entry in document["shares"]}
    if str(resolved) in known:
        return False, "Ce dossier est déjà partagé."
    document["shares"].append(
        {
            "id": f"s{uuid.uuid4().hex[:8]}",
            "name": name.strip() or resolved.name or "Partage",
            "path": str(resolved),
        }
    )
    try:
        save(document)
    except OSError as error:
        document["shares"].pop()
        return False, f"Écriture impossible : {error}"
    return True, ""


def remove_share(document: dict, share_id: str) -> bool:
    """Retire un dossier de la liste permanente (le dossier lui-même est intact)."""
    keep = [entry for entry in document["shares"] if entry.get("id") != share_id]
    if len(keep) == len(document["shares"]):
        return False
    removed = document["shares"]
    document["shares"] = keep
    try:
        save(document)
    except OSError:
        document["shares"] = removed
        return False
    return True
