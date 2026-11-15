"""Accès aux partages protégés : qui entre, et jusqu'à quand.

Deux durées de vie, demandées le 15/11/2026 :

- **la session** : un mot de passe saisi vit en mémoire et meurt avec la
  fenêtre — rouvrir Gablue Share le redemande ;
- **mémorisé** : si l'utilisateur a coché « Se souvenir de ce mot de passe »,
  l'empreinte est écrite dans `~/.local/state/gablue-share/access.json` (0600) et
  le partage se déverrouille tout seul dès qu'il est découvert.

Rien de tout cela ne vit dans le démon : il ne connaît que l'empreinte attendue
et vérifie chaque requête. Le jeton de version (`revision`, publié avec le sel)
change avec le mot de passe : un accès mémorisé devient alors périmé, la fenêtre
le dit et redemande la saisie — sinon on naviguerait dans le vide avec un 401
pour seule explication.
"""

import json
import os
import tempfile

from PySide6.QtCore import QObject, Signal

from .constants import STATE_DIR

ACCESS_PATH = STATE_DIR / "access.json"


class Access(QObject):
    """Registre des déverrouillages, en mémoire et sur disque."""

    #: Un accès a été ajouté, retiré ou périmé : les listes se rafraîchissent.
    changed = Signal()
    #: Le mot de passe d'un partage a changé (nom du poste, nom du partage).
    password_changed = Signal(str, str)

    def __init__(self, parent=None):
        super().__init__(parent)
        #: Déverrouillé pour cette session seulement.
        self._session: dict[tuple[str, str], dict] = {}
        #: Déverrouillé de façon permanente (recopié en session à la découverte).
        self._saved: dict[tuple[str, str], dict] = {}
        #: Partages dont le mot de passe a changé et qu'on n'a pas réouvert.
        self._stale: set[tuple[str, str]] = set()
        #: Annonces déjà faites, par (poste, partage, version) : une seule fois.
        self._announced: set[tuple[str, str, str]] = set()
        #: Noms lisibles, retenus de la dernière annonce (pour les messages).
        self._names: dict[tuple[str, str], tuple[str, str]] = {}
        self.load()

    # --- lecture --------------------------------------------------------
    @staticmethod
    def _id(peer_id, share_id) -> tuple[str, str]:
        return (str(peer_id or ""), str(share_id or ""))

    def key(self, peer_id, share_id) -> str:
        """Empreinte à envoyer au démon, ou chaîne vide si rien n'est ouvert."""
        return str(self._session.get(self._id(peer_id, share_id), {}).get("key") or "")

    def has(self, peer_id, share_id) -> bool:
        return bool(self.key(peer_id, share_id))

    def stale(self, peer_id, share_id) -> bool:
        """Le mot de passe a changé et n'a pas encore été ressaisi."""
        return self._id(peer_id, share_id) in self._stale

    def remembered(self, peer_id, share_id) -> bool:
        return self._id(peer_id, share_id) in self._saved

    def names(self, peer_id, share_id) -> tuple[str, str]:
        return self._names.get(self._id(peer_id, share_id), ("un poste", "un partage"))

    # --- écriture -------------------------------------------------------
    def unlock(
        self,
        peer_id,
        share_id,
        key: str,
        revision: str,
        remember: bool = False,
    ) -> None:
        """Retient une empreinte (session, et sur disque si `remember`)."""
        identifiant = self._id(peer_id, share_id)
        entry = {"key": str(key), "revision": str(revision or "")}
        self._session[identifiant] = entry
        self._stale.discard(identifiant)
        self._announced = {
            marque for marque in self._announced if marque[:2] != identifiant
        }
        if remember:
            self._saved[identifiant] = dict(entry)
        else:
            self._saved.pop(identifiant, None)
        self.save()
        self.changed.emit()

    def forget(self, peer_id, share_id, announce: bool = False) -> None:
        """Oublie un accès (session **et** disque).

        `announce` : le démon vient de refuser l'empreinte (401) — inutile
        d'attendre l'annonce suivante pour dire que le mot de passe a changé.
        """
        identifiant = self._id(peer_id, share_id)
        if identifiant not in self._stale and announce:
            self._stale.add(identifiant)
            peer_name, share_name = self.names(*identifiant)
            marque = (identifiant[0], identifiant[1], "")
            if marque not in self._announced:
                self._announced.add(marque)
                self.password_changed.emit(peer_name, share_name)
        changed = False
        if self._session.pop(identifiant, None) is not None:
            changed = True
        if self._saved.pop(identifiant, None) is not None:
            changed = True
        if changed:
            self.save()
            self.changed.emit()

    # --- annonces de découverte ----------------------------------------
    def sync(self, peers) -> None:
        """Confronte les accès à ce que les postes annoncent maintenant.

        Trois décisions, dans cet ordre : ce qui a été ressaisi pour une autre
        version est oublié ; un partage redevenu ouvert n'a plus besoin de rien ;
        un accès mémorisé dont la version correspond rentre en session
        (déverrouillage automatique à la découverte).
        """
        locked: dict[tuple[str, str], str] = {}
        open_now: set[tuple[str, str]] = set()
        for peer in peers or []:
            for share in getattr(peer, "shares", []) or []:
                identifiant = self._id(peer.peer_id, share.get("id", ""))
                self._names[identifiant] = (
                    peer.name or "un poste",
                    str(share.get("name") or share.get("id") or "un partage"),
                )
                if share.get("locked"):
                    locked[identifiant] = str(share.get("revision") or "")
                else:
                    open_now.add(identifiant)

        changed = False
        perimes: list[tuple[str, str]] = []
        for identifiant, entry in list(self._session.items()):
            revision = locked.get(identifiant)
            if revision is None:
                continue  # ouvert ou disparu : rien à invalider
            if str(entry.get("revision") or "") != revision:
                del self._session[identifiant]
                changed = True
                perimes.append(identifiant)
        for identifiant in open_now:
            if self._session.pop(identifiant, None) is not None:
                changed = True
            if self._saved.pop(identifiant, None) is not None:
                changed = True
        for identifiant, revision in locked.items():
            entry = self._saved.get(identifiant)
            if entry is None:
                continue
            if str(entry.get("revision") or "") != revision:
                del self._saved[identifiant]
                changed = True
                perimes.append(identifiant)
                continue
            if self._session.get(identifiant, {}).get("key") != entry.get("key"):
                self._session[identifiant] = dict(entry)
                changed = True

        for identifiant in perimes:
            self._stale.add(identifiant)
            marque = (identifiant[0], identifiant[1], locked.get(identifiant, ""))
            if marque in self._announced:
                continue
            self._announced.add(marque)
            peer_name, share_name = self.names(*identifiant)
            self.password_changed.emit(peer_name, share_name)
        if changed:
            self.save()
            self.changed.emit()

    # --- persistance ----------------------------------------------------
    def load(self) -> None:
        try:
            raw = ACCESS_PATH.read_text(encoding="utf-8")
        except OSError:
            return
        try:
            document = json.loads(raw)
        except ValueError:
            return  # fichier illisible : il sera réécrit au prochain accès
        items = document.get("items", []) if isinstance(document, dict) else []
        for item in items:
            if not isinstance(item, dict):
                continue
            key = str(item.get("key") or "")
            peer_id = str(item.get("peer_id") or "")
            share_id = str(item.get("share_id") or "")
            if not key or not peer_id or not share_id:
                continue
            self._saved[(peer_id, share_id)] = {
                "key": key,
                "revision": str(item.get("revision") or ""),
            }

    def save(self) -> None:
        """Écriture atomique, 0600 : le fichier porte des empreintes."""
        STATE_DIR.mkdir(parents=True, exist_ok=True, mode=0o700)
        items = [
            {
                "peer_id": peer_id,
                "share_id": share_id,
                "revision": str(entry.get("revision") or ""),
                "key": str(entry.get("key") or ""),
            }
            for (peer_id, share_id), entry in self._saved.items()
        ]
        payload = json.dumps({"version": 1, "items": items}, ensure_ascii=False, indent=2)
        temporary = None
        try:
            handle, temporary = tempfile.mkstemp(
                dir=str(STATE_DIR), prefix=".access-", suffix=".json"
            )
            with os.fdopen(handle, "w", encoding="utf-8") as stream:
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
            os.chmod(temporary, 0o600)
            os.replace(temporary, ACCESS_PATH)
        except OSError:
            if temporary:
                try:
                    os.unlink(temporary)
                except OSError:
                    pass
