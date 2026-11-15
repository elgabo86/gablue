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


class AuthRequired(ApiError):
    """Le partage est protégé et l'empreinte fournie ne l'ouvre pas (401).

    Distinct d'une erreur réseau : c'est une invitation à saisir le mot de
    passe, pas une panne.
    """


def auth_headers(key: str) -> dict:
    """En-tête du protocole pour un partage protégé (vide = partage ouvert)."""
    return {"X-Gablue-Key": key} if key else {}


def peer_url(address: str, port: int) -> str:
    host = address
    if ":" in host:
        if not host.startswith("["):
            host = f"[{host}]"
        # Zone IPv6 lien-local (`fe80::1%eno1` renvoyé par Avahi) : le `%` doit
        # être échappé dans une URL, sinon la requête est refusée avant d'être
        # envoyée.
        host = host.replace("%", "%25")
    return f"http://{host}:{port}"


def _segments(path: str) -> str:
    return "/".join(urllib.parse.quote(part, safe="") for part in path.split("/") if part)


def file_url(base: str, share_id: str, path: str) -> str:
    quoted_share = urllib.parse.quote(share_id, safe="")
    suffix = _segments(path)
    return f"{base}/f/{quoted_share}/{suffix}" if suffix else f"{base}/f/{quoted_share}"


def tar_url(base: str, share_id: str, path: str, as_name: str = "") -> str:
    """Archive d'un dossier ; `as_name` impose le nom du dossier dans le tar."""
    quoted_share = urllib.parse.quote(share_id, safe="")
    suffix = _segments(path)
    url = f"{base}/a/{quoted_share}/{suffix}" if suffix else f"{base}/a/{quoted_share}"
    if as_name:
        url += "?" + urllib.parse.urlencode({"as": as_name})
    return url


def list_url(
    base: str,
    share_id: str,
    path: str,
    limit: int = 2000,
    offset: int = 0,
    sort: str = "",
    descending: bool = False,
) -> str:
    params: dict[str, object] = {"path": path, "limit": limit}
    if offset:
        params["offset"] = offset
    if sort:
        params["sort"] = sort
    if descending:
        params["desc"] = 1
    query = urllib.parse.urlencode(params)
    return f"{base}/api/list/{urllib.parse.quote(share_id, safe='')}?{query}"


def find_url(base: str, share_id: str, query_text: str, path: str, limit: int = 200) -> str:
    params = urllib.parse.urlencode({"q": query_text, "path": path, "limit": limit})
    return f"{base}/api/find/{urllib.parse.quote(share_id, safe='')}?{params}"


def size_url(base: str, share_id: str, path: str) -> str:
    params = urllib.parse.urlencode({"path": path})
    return f"{base}/api/size/{urllib.parse.quote(share_id, safe='')}?{params}"


def auth_url(base: str, share_id: str) -> str:
    return f"{base}/api/auth/{urllib.parse.quote(share_id, safe='')}"


def _request(url: str, timeout: float, headers: dict | None = None):
    request = urllib.request.Request(url, headers=headers or {})
    request.add_header("User-Agent", "gablue-share/0.1")
    try:
        return urllib.request.urlopen(request, timeout=timeout)  # noqa: S310 (http local)
    except urllib.error.HTTPError as error:
        if error.code == 401:
            raise AuthRequired("mot de passe requis") from error
        raise ApiError(f"réponse HTTP {error.code}") from error
    except (urllib.error.URLError, socket.timeout, OSError) as error:
        raise ApiError("poste injoignable") from error


def verify(base: str, share_id: str, key: str, timeout: float = PROBE_TIMEOUT) -> bool:
    """L'empreinte ouvre-t-elle ce partage ? (`/api/auth`, rien n'est lu)

    Faux = mot de passe incorrect (401) ; une panne réseau, elle, lève.
    """
    request = urllib.request.Request(auth_url(base, share_id), headers=auth_headers(key))
    request.add_header("User-Agent", "gablue-share/0.1")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
            return response.status == 200
    except urllib.error.HTTPError as error:
        if error.code == 401:
            return False
        raise ApiError(f"réponse HTTP {error.code}") from error
    except (urllib.error.URLError, socket.timeout, OSError) as error:
        raise ApiError("poste injoignable") from error


def info(base: str, timeout: float = PROBE_TIMEOUT) -> dict:
    with _request(f"{base}/api/info", timeout) as response:
        try:
            return json.loads(response.read().decode("utf-8"))
        except (ValueError, OSError) as error:
            raise ApiError("réponse illisible") from error


def list_dir(
    base: str,
    share_id: str,
    path: str = "",
    offset: int = 0,
    limit: int = 500,
    sort: str = "",
    descending: bool = False,
    key: str = "",
) -> tuple[dict, str]:
    """Une page de listing + l'empreinte (`ETag`) du dossier complet."""
    url = list_url(base, share_id, path, limit=limit, offset=offset, sort=sort, descending=descending)
    with _request(url, LIST_TIMEOUT, auth_headers(key)) as response:
        payload = json.loads(response.read().decode("utf-8"))
        return payload, response.headers.get("ETag", "")


def find(
    base: str, share_id: str, query: str, path: str = "", limit: int = 200, key: str = ""
) -> dict:
    """Recherche récursive par nom, bornée côté serveur (~400 ms).

    `limit` borne le nombre de résultats **par partage** : la recherche globale
    interroge plusieurs partages à la fois, elle n'a pas besoin de tout.
    """
    with _request(find_url(base, share_id, query, path, limit), LIST_TIMEOUT, auth_headers(key)) as response:
        return json.loads(response.read().decode("utf-8"))


def folder_size(base: str, share_id: str, path: str, key: str = "") -> dict:
    """Taille d'un dossier : `complete: false` = minorant (calcul borné)."""
    with _request(size_url(base, share_id, path), LIST_TIMEOUT, auth_headers(key)) as response:
        return json.loads(response.read().decode("utf-8"))


def list_tag(base: str, share_id: str, path: str, etag: str, key: str = "") -> str:
    """Empreinte du dossier, en `HEAD` conditionnel.

    Retourne `""` quand le dossier n'a pas bougé (304) : c'est le cas normal, et
    la réponse ne transporte aucun corps — quelques centaines d'octets toutes
    les quelques secondes, jamais le listing complet.
    """
    headers = {"If-None-Match": etag} if etag else {}
    headers.update(auth_headers(key))
    request = urllib.request.Request(
        list_url(base, share_id, path, limit=1), headers=headers, method="HEAD"
    )
    try:
        with urllib.request.urlopen(request, timeout=LIST_TIMEOUT) as response:  # noqa: S310
            return response.headers.get("ETag", "")
    except urllib.error.HTTPError as error:
        if error.code == 304:
            return ""
        if error.code == 401:
            raise AuthRequired("mot de passe requis") from error
        raise ApiError(f"réponse HTTP {error.code}") from error
    except (urllib.error.URLError, socket.timeout, OSError) as error:
        raise ApiError("poste injoignable") from error


def head(
    base: str, share_id: str, path: str, timeout: float = LIST_TIMEOUT, key: str = ""
) -> dict:
    """En-têtes d'un fichier (ETag, taille, Last-Modified), en minuscules."""
    request = urllib.request.Request(
        file_url(base, share_id, path), headers=auth_headers(key), method="HEAD"
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
            return {name.lower(): value for name, value in response.headers.items()}
    except urllib.error.HTTPError as error:
        if error.code == 401:
            raise AuthRequired("mot de passe requis") from error
        raise ApiError(f"réponse HTTP {error.code}") from error
    except (urllib.error.URLError, socket.timeout, OSError) as error:
        raise ApiError("poste injoignable") from error


def open_file(
    base: str, share_id: str, path: str, start: int = 0, etag: str = "", key: str = ""
):
    """Flux d'un fichier, éventuellement repris à `start` (Range + If-Range).

    Retourne `(response, offset)` : `offset` est le premier octet réellement
    servi (0 si le serveur a ignoré la reprise).
    """
    headers = auth_headers(key)
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


def open_tar(base: str, share_id: str, path: str, as_name: str = "", key: str = ""):
    return _request(tar_url(base, share_id, path, as_name), LIST_TIMEOUT, auth_headers(key))
