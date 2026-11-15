"""La liste des dossiers partagés, sur disque.

`~/.config/gablue/share.json` est la **seule source de vérité** : elle survit à
la fermeture de l'app, au redémarrage et au mode permanent, et c'est elle que
le démon relit à chaud (mtime) sans redémarrer ni couper un transfert.
"""

import hashlib
import json
import os
import stat
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
    normalised = False
    for entry in document["shares"]:
        if not isinstance(entry, dict):
            continue
        path = str(entry.get("path", "")).strip()
        if not path:
            continue
        identifier = share_id(path)
        if str(entry.get("id") or "") != identifier:
            normalised = True
        cleaned.append(
            {
                "id": identifier,
                "name": str(entry.get("name") or Path(path).name or "Partage"),
                "path": path,
                "hidden": bool(entry.get("hidden", False)),
                "recursive": bool(entry.get("recursive", True)),
            }
        )
    document["shares"] = cleaned
    document["visibility"] = visibility(document)
    if normalised:
        # Identifiants d'une version antérieure : on les réaligne une fois,
        # pour que le même dossier garde toujours le même identifiant.
        save(document)
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


def share_id(path: str) -> str:
    """Identifiant **stable et invisible** d'un partage.

    Dérivé du chemin réel, jamais du nom affiché : renommer un partage ne casse
    rien, et retirer puis re-ajouter le même dossier redonne le même identifiant
    — c'est ce qui permet à la file de téléchargement de se rattacher.
    """
    digest = hashlib.sha256(share_key({"path": path}).encode("utf-8")).hexdigest()
    return f"s{digest[:12]}"


def share_key(entry: dict) -> str:
    """Chemin réel : deux écritures différentes du même dossier = un seul partage."""
    return str(Path(entry.get("path", "")).expanduser().resolve())


#: Qui peut voir ce poste. Le démon applique la règle ; ici on ne fait que
#: l'écrire et l'afficher.
VISIBILITIES = {
    "both": "Réseau local et Tailscale",
    "lan": "Réseau local seulement",
    "tailscale": "Tailscale seulement",
}


def visibility(document: dict) -> str:
    """Visibilité courante, toujours une des clés de `VISIBILITIES`."""
    value = str(document.get("visibility") or "both").strip().lower()
    return value if value in VISIBILITIES else "both"


def set_visibility(document: dict, value: str) -> tuple[bool, str]:
    """Change la visibilité et l'écrit. (ok, message)"""
    if value not in VISIBILITIES:
        return False, "Visibilité inconnue."
    previous = document.get("visibility")
    document["visibility"] = value
    try:
        save(document)
    except OSError as error:
        if previous is None:
            document.pop("visibility", None)
        else:
            document["visibility"] = previous
        return False, f"Écriture impossible : {error}"
    return True, ""


#: Racines où un dossier peut être un point de montage de disque externe.
MOUNT_ROOTS = ("/mnt", "/media", "/run/media")


def share_available(path: str) -> bool:
    """Le dossier d'un partage est-il là *maintenant* ?

    Miroir de `config::is_available` côté démon, qui fait foi pour le réseau :
    un disque externe débranché n'existe plus (udisks retire son point de
    montage), et un point de montage manuel laissé vide se reconnaît à son
    périphérique, identique à celui de son parent. Sert seulement à l'affichage
    du panneau — la décision de servir ou d'annoncer appartient au démon.
    """
    candidate = Path(path)
    try:
        status = candidate.stat()
    except OSError:
        return False
    if not stat.S_ISDIR(status.st_mode):
        return False
    # Comparaison par composants (`/mntfoo` n'est pas sous `/mnt`), comme
    # `Path::starts_with` côté démon.
    if any(candidate.is_relative_to(root) for root in MOUNT_ROOTS):
        try:
            parent = candidate.parent.stat()
        except OSError:
            return True
        if parent.st_dev == status.st_dev:
            return False  # rien n'est monté sur ce dossier vide
    return True


#: Options de vue d'un partage. Le démon les applique ; la GUI les écrit.
#: `hidden` : afficher les noms qui commencent par un point (défaut : non).
#: `recursive` : exposer les sous-dossiers (défaut : oui).
SHARE_OPTIONS = ("hidden", "recursive")


def share_option(entry: dict, key: str) -> bool:
    """Valeur courante d'une option, avec le défaut du démon."""
    return bool(entry.get(key, key == "recursive"))


def set_share_option(
    document: dict, identifier: str, key: str, value: bool
) -> tuple[bool, str]:
    """Écrit une option de vue, en laissant le reste intact. (ok, message)"""
    if key not in SHARE_OPTIONS:
        return False, "Option inconnue."
    for entry in document.get("shares", []):
        if entry.get("id") != identifier:
            continue
        if share_option(entry, key) == bool(value) and key in entry:
            return True, ""
        previous = entry.get(key, None)
        existed = key in entry
        entry[key] = bool(value)
        try:
            save(document)
        except OSError as error:
            if existed:
                entry[key] = previous
            else:
                entry.pop(key, None)
            return False, f"Écriture impossible : {error}"
        return True, ""
    return False, "Partage introuvable."


def rename_share(document: dict, identifier: str, name: str) -> tuple[bool, str]:
    """Renomme un partage. Nom vide → nom du dossier (règle de l'app)."""
    for entry in document.get("shares", []):
        if entry.get("id") != identifier:
            continue
        cleaned = name.strip() or Path(entry.get("path", "")).name or "Partage"
        previous = entry.get("name")
        if cleaned == previous:
            return True, ""
        entry["name"] = cleaned
        try:
            save(document)
        except OSError as error:
            entry["name"] = previous
            return False, f"Écriture impossible : {error}"
        return True, ""
    return False, "Partage introuvable."


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
            "id": share_id(str(resolved)),
            "name": name.strip() or resolved.name or "Partage",
            "path": str(resolved),
            "hidden": False,  # cachés masqués par défaut
            "recursive": True,  # sous-dossiers inclus par défaut
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
