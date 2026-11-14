"""Client HTTP du protocole Gablue Share.

Aucune dépendance Qt ici : les fonctions sont appelées depuis des threads de
travail et lèvent `ApiError` avec un message lisible.
"""

import json
import socket
import urllib.error
import urllib.parse
import urllib.request

from .constants import LIST_TIMEOUT, PROBE_TIMEOUT


class ApiError(Exception):
    """Échec réseau ou protocole, message destiné à l'utilisateur."""


def peer_url(address: str, port: int) -> str:
    host = address
    if ":" in host and not host.startswith("["):
        host = f"[{host}]"
    return f"http://{host}:{port}"


def _segments(path: str) -> str:
    return "/".join(urllib.parse.quote(part, safe="") for part in path.split("/") if part)


def file_url(base: str, share_id: str, path: str) -> str:
    quoted_share = urllib.parse.quote(share_id, safe="")
    suffix = _segments(path)
    return f"{base}/f/{quoted_share}/{suffix}" if suffix else f"{base}/f/{quoted_share}"


def tar_url(base: str, share_id: str, path: str) -> str:
    quoted_share = urllib.parse.quote(share_id, safe="")
    suffix = _segments(path)
    return f"{base}/a/{quoted_share}/{suffix}" if suffix else f"{base}/a/{quoted_share}"


def list_url(base: str, share_id: str, path: str, limit: int = 2000) -> str:
    query = urllib.parse.urlencode({"path": path, "limit": limit})
    return f"{base}/api/list/{urllib.parse.quote(share_id, safe='')}?{query}"


def _request(url: str, timeout: float, headers: dict | None = None):
    request = urllib.request.Request(url, headers=headers or {})
    request.add_header("User-Agent", "gablue-share/0.1")
    try:
        return urllib.request.urlopen(request, timeout=timeout)  # noqa: S310 (http local)
    except urllib.error.HTTPError as error:
        raise ApiError(f"réponse HTTP {error.code}") from error
    except (urllib.error.URLError, socket.timeout, OSError) as error:
        raise ApiError("poste injoignable") from error


def info(base: str, timeout: float = PROBE_TIMEOUT) -> dict:
    with _request(f"{base}/api/info", timeout) as response:
        try:
            return json.loads(response.read().decode("utf-8"))
        except (ValueError, OSError) as error:
            raise ApiError("réponse illisible") from error


def list_dir(base: str, share_id: str, path: str = "") -> dict:
    with _request(list_url(base, share_id, path), LIST_TIMEOUT) as response:
        return json.loads(response.read().decode("utf-8"))


def head(base: str, share_id: str, path: str, timeout: float = LIST_TIMEOUT) -> dict:
    """En-têtes d'un fichier (ETag, taille, Last-Modified), en minuscules."""
    request = urllib.request.Request(file_url(base, share_id, path), method="HEAD")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
            return {key.lower(): value for key, value in response.headers.items()}
    except urllib.error.HTTPError as error:
        raise ApiError(f"réponse HTTP {error.code}") from error
    except (urllib.error.URLError, socket.timeout, OSError) as error:
        raise ApiError("poste injoignable") from error


def open_file(
    base: str, share_id: str, path: str, start: int = 0, etag: str = ""
):
    """Flux d'un fichier, éventuellement repris à `start` (Range + If-Range).

    Retourne `(response, offset)` : `offset` est le premier octet réellement
    servi (0 si le serveur a ignoré la reprise).
    """
    headers = {}
    if start > 0:
        headers["Range"] = f"bytes={start}-"
        if etag:
            headers["If-Range"] = etag
    response = _request(file_url(base, share_id, path), LIST_TIMEOUT, headers)
    if response.status == 206:
        offset = start
    else:
        offset = 0
    return response, offset


def open_tar(base: str, share_id: str, path: str):
    return _request(tar_url(base, share_id, path), LIST_TIMEOUT)
