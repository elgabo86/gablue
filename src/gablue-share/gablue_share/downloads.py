"""File de téléchargement persistante.

Un élément est identifié par `(peer_id, share_id, chemin)` — **jamais par une
adresse** : au redémarrage, la découverte retrouve le poste (LAN ou Tailscale,
l'adresse a pu changer) et la reprise repart du `.part` laissé sur disque.

La file vit ici, pas dans le démon : fermer la GUI arrête les transferts, le
démon continue de servir si la permanence est active.
"""

import json
import os
import tarfile
import tempfile
import time
from pathlib import Path

from PySide6.QtCore import QObject, QThread, QTimer, Signal

from . import api
from .constants import STATE_DIR

QUEUE_PATH = STATE_DIR / "queue.json"
CHUNK = 256 * 1024
KEEP_FINISHED = 10

WAITING_PEER = "en attente du poste"
WAITING_SHARE = "en attente du partage"


class Cancelled(Exception):
    """Interruption demandée par l'utilisateur ou par la fermeture de l'app."""


class Job:
    """Un téléchargement, sérialisable dans la file."""

    def __init__(
        self,
        peer,
        share_id: str,
        share_name: str,
        path: str,
        name: str,
        is_dir: bool,
        dest: Path,
        state: str = "en attente",
        done: int = 0,
        total: int = 0,
        etag: str = "",
        error: str = "",
        identifier: str = "",
        created: float = 0.0,
    ):
        self.peer = peer
        self.peer_id = peer.peer_id if peer else ""
        self.peer_name = peer.name if peer else ""
        self.share_id = share_id
        self.share_name = share_name
        self.path = path
        self.name = name
        self.is_dir = is_dir
        self.dest = Path(dest)
        self.state = state
        self.done = done
        self.total = total
        self.etag = etag
        self.error = error
        self.identifier = identifier or f"j{int(time.time() * 1000):x}-{os.getpid():x}"
        self.created = created or time.time()
        # Transitoires (jamais écrits dans le fichier) : débit courant, nombre
        # de tentatives et date du prochain essai (reprise auto, avec attente
        # croissante pour ne pas marteler un poste éteint).
        self.speed = 0.0
        self.attempts = 0
        self.next_attempt = 0.0
        self._last_measure = (0.0, 0)

    # --- affichage ------------------------------------------------------
    def title(self) -> str:
        return self.name + (" (dossier)" if self.is_dir else "")

    def subtitle(self) -> str:
        source = f"{self.peer_name or 'poste inconnu'} · {self.share_name}"
        if self.state == "échec":
            return f"{source} — {self.error}"
        if self.state == WAITING_PEER:
            return f"{source} — poste hors ligne, reprise dès qu'il revient"
        if self.state == WAITING_SHARE:
            return f"{source} — partage retiré, reprise dès qu'il réapparaît"
        if self.state == "terminé":
            return f"{source} — {self.dest}"
        return source

    def fraction(self) -> float:
        if self.total <= 0:
            return 0.0
        return min(1.0, self.done / self.total)

    def waiting(self) -> bool:
        return self.state in (WAITING_PEER, WAITING_SHARE)

    def retryable(self) -> bool:
        return self.state in ("échec", WAITING_SHARE)

    # --- sérialisation --------------------------------------------------
    def to_dict(self) -> dict:
        return {
            "id": self.identifier,
            "peer_id": self.peer_id,
            "peer_name": self.peer_name,
            "share_id": self.share_id,
            "share_name": self.share_name,
            "path": self.path,
            "name": self.name,
            "is_dir": self.is_dir,
            "dest": str(self.dest),
            "state": self.state,
            "done": self.done,
            "total": self.total,
            "etag": self.etag,
            "error": self.error,
            "created": self.created,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "Job":
        job = cls(
            peer=None,
            share_id=str(data["share_id"]),
            share_name=str(data.get("share_name", "")),
            path=str(data["path"]),
            name=str(data["name"]),
            is_dir=bool(data.get("is_dir", False)),
            dest=Path(data["dest"]),
            state=str(data.get("state", WAITING_PEER)),
            done=int(data.get("done", 0)),
            total=int(data.get("total", 0)),
            etag=str(data.get("etag", "")),
            error=str(data.get("error", "")),
            identifier=str(data.get("id", "")),
            created=float(data.get("created", 0)),
        )
        job.peer_id = str(data.get("peer_id", ""))
        job.peer_name = str(data.get("peer_name", ""))
        return job


def unique_target(directory: Path, name: str, is_dir: bool) -> Path:
    """Jamais d'écrasement silencieux : `nom (1).ext` pour un fichier."""
    candidate = directory / name
    if is_dir or not candidate.exists():
        return candidate
    stem, suffix = os.path.splitext(name)
    index = 1
    while True:
        candidate = directory / f"{stem} ({index}){suffix}"
        if not candidate.exists():
            return candidate
        index += 1


def part_path(dest: Path, etag: str) -> Path:
    tag = "".join(character for character in etag if character.isalnum())[-8:] or "0"
    return dest.parent / f".{dest.name}.{tag}.part"


class _Counter:
    """Enveloppe de lecture qui compte les octets et respecte l'annulation."""

    def __init__(self, stream, callback, cancelled):
        self._stream = stream
        self._callback = callback
        self._cancelled = cancelled

    def read(self, size=-1):
        if self._cancelled():
            raise Cancelled()
        chunk = self._stream.read(size)
        if chunk:
            self._callback(len(chunk))
        return chunk


class DownloadWorker(QThread):
    progress = Signal(object)
    completed = Signal(object)
    failed = Signal(object, str)
    waiting = Signal(object, str)

    def __init__(self, job: Job, parent=None):
        super().__init__(parent)
        self.job = job
        #: Réponse HTTP en cours, si un transfert est actif (voir `abort()`).
        self._response = None

    def run(self) -> None:
        if self.job.peer is None:
            # File rechargée, poste pas encore retrouvé : rien à tenter.
            self.waiting.emit(self.job, "peer")
            return
        self.job.state = "en cours"
        self.job.speed = 0.0
        self.job._last_measure = (time.monotonic(), self.job.done)
        self.progress.emit(self.job)
        try:
            if self.job.is_dir:
                self._download_folder()
            else:
                self._download_file()
            if self.job.state in ("annulé", "en pause"):
                return
            self.job.state = "terminé"
            self.completed.emit(self.job)
        except Cancelled:
            # « en pause » est un arrêt voulu, pas un annulation.
            if self.job.state not in ("en pause", "annulé"):
                self.job.state = "annulé"
        except api.ApiError as error:
            message = str(error)
            if "injoignable" in message:
                self.waiting.emit(self.job, "peer")
            elif "404" in message:
                self.waiting.emit(self.job, "share")
            else:
                self.failed.emit(self.job, message)
        except (OSError, tarfile.TarError) as error:
            self.failed.emit(self.job, f"{type(error).__name__} : {error}")

    def abort(self) -> None:
        """Débloque une lecture en cours en fermant sa socket.

        `QThread.terminate()` est inutilisable ici : tuer un fil Python pendant
        une lecture système fait planter le processus (SIGSEGV constaté le
        15/11/2026). Fermer la réponse fait échouer `read()` tout de suite, et
        le fil sort proprement en voyant la demande d'interruption.
        """
        response = getattr(self, "_response", None)
        self._response = None
        if response is not None:
            try:
                response.close()
            except Exception:  # noqa: BLE001 (fermeture best-effort)
                pass

    def _measure(self) -> None:
        """Débit lissé sur la fenêtre écoulée (affichage, jamais décision)."""
        now = time.monotonic()
        previous_time, previous_done = self.job._last_measure
        elapsed = now - previous_time
        if elapsed >= 0.5:
            self.job.speed = max(0.0, (self.job.done - previous_done) / elapsed)
            self.job._last_measure = (now, self.job.done)

    # --- fichier --------------------------------------------------------
    def _download_file(self) -> None:
        base = self.job.peer.base_url()
        headers = api.head(base, self.job.share_id, self.job.path)
        etag = headers.get("etag", "")
        try:
            size = int(headers.get("content-length", "0"))
        except ValueError:
            size = 0
        self.job.total = size
        self.job.etag = etag

        destination = self.job.dest
        destination.parent.mkdir(parents=True, exist_ok=True)
        part = part_path(destination, etag)
        start = part.stat().st_size if part.exists() else 0
        if start and size and start >= size:
            if start == size:
                os.replace(part, destination)  # déjà complet
                self.job.done = size
                self.progress.emit(self.job)
                return
            start = 0

        if start:
            self.job.done = start
            self.progress.emit(self.job)

        response, offset = api.open_file(base, self.job.share_id, self.job.path, start, etag)
        self._response = response
        with response:
            mode = "ab" if offset else "wb"
            if offset == 0:
                self.job.done = 0
                part = part_path(destination, etag)
            with open(part, mode) as stream:
                last_emit = 0.0
                while True:
                    if self.isInterruptionRequested():
                        raise Cancelled()
                    chunk = response.read(CHUNK)
                    if not chunk:
                        break
                    stream.write(chunk)
                    self.job.done += len(chunk)
                    now = time.monotonic()
                    if now - last_emit > 0.15:
                        self._measure()
                        self.progress.emit(self.job)
                        last_emit = now
                stream.flush()
                os.fsync(stream.fileno())

        if size and part.stat().st_size != size:
            raise api.ApiError("taille reçue différente de la taille annoncée")
        os.replace(part, destination)
        self.progress.emit(self.job)

    # --- dossier --------------------------------------------------------
    def _download_folder(self) -> None:
        base = self.job.peer.base_url()
        destination = self.job.dest
        destination.parent.mkdir(parents=True, exist_ok=True)
        received = 0

        def account(count: int) -> None:
            nonlocal received
            received += count
            self.job.done = received
            self._measure()
            self.progress.emit(self.job)

        # `as=nom` : le dossier extrait porte le nom affiché dans l'app,
        # pas le nom physique du dossier côté source.
        response = api.open_tar(base, self.job.share_id, self.job.path, self.job.name)
        self._response = response
        with response:
            stream = _Counter(response, account, self.isInterruptionRequested)
            # filter="data" refuse les chemins absolus et les sorties d'arbre.
            with tarfile.open(fileobj=stream, mode="r|") as archive:
                archive.extractall(path=destination.parent, filter="data")
        self.job.total = received
        self.progress.emit(self.job)


class Downloads(QObject):
    """File ordonnée et persistante : un transfert à la fois."""

    changed = Signal()
    finished = Signal(object, bool)  # job, succès (notification côté fenêtre)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.jobs: list[Job] = []
        self._worker: DownloadWorker | None = None
        self._known_peers: dict[str, object] = {}
        self._save_timer = QTimer(self)
        self._save_timer.setSingleShot(True)
        self._save_timer.setInterval(800)
        self._save_timer.timeout.connect(self.save)
        # Reprise automatique : un élément « en attente du poste » n'attend pas
        # un événement de découverte pour réessayer (le poste peut rester
        # annoncé alors que son disque ou son application revient). Attente
        # croissante 5 s → 5 min : rien ne martèle un poste éteint.
        self._retry_timer = QTimer(self)
        self._retry_timer.setInterval(5000)
        self._retry_timer.timeout.connect(self._retry_waiting)
        self._retry_timer.start()
        self.load()

    # --- persistance ----------------------------------------------------
    def load(self) -> None:
        try:
            raw = QUEUE_PATH.read_text(encoding="utf-8")
        except OSError:
            return
        try:
            document = json.loads(raw)
        except ValueError:
            return  # fichier illisible : il sera réécrit au prochain changement
        items = document.get("items", []) if isinstance(document, dict) else []
        loaded: list[Job] = []
        for entry in items:
            try:
                job = Job.from_dict(entry)
            except (KeyError, TypeError, ValueError):
                continue
            if job.state in ("annulé", "terminé"):
                continue
            if job.state in ("en cours", "en attente"):
                job.state = WAITING_PEER
            # Point de reprise : la taille du .part s'il correspond encore.
            part = part_path(job.dest, job.etag)
            if part.exists():
                job.done = part.stat().st_size
            loaded.append(job)
        self.jobs = loaded

    def save(self) -> None:
        STATE_DIR.mkdir(parents=True, exist_ok=True, mode=0o700)
        items = [job.to_dict() for job in self.jobs if job.state != "annulé"]
        payload = json.dumps({"version": 1, "items": items}, ensure_ascii=False, indent=2)
        temporary = None
        try:
            handle, temporary = tempfile.mkstemp(dir=str(STATE_DIR), prefix=".queue-", suffix=".json")
            with os.fdopen(handle, "w", encoding="utf-8") as stream:
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
            os.chmod(temporary, 0o600)
            os.replace(temporary, QUEUE_PATH)
        except OSError:
            if temporary:
                try:
                    os.unlink(temporary)
                except OSError:
                    pass

    def _touch(self) -> None:
        self.changed.emit()
        if not self._save_timer.isActive():
            self._save_timer.start()

    # --- file -----------------------------------------------------------
    def enqueue(self, job: Job) -> None:
        job.state = "en attente" if job.peer else WAITING_PEER
        self.jobs.append(job)
        self._prune()
        self._touch()
        self._pump()

    def retry(self, job: Job) -> None:
        if job.state not in ("échec",) and not job.retryable() and job.state != WAITING_PEER:
            return
        job.error = ""
        job.attempts = 0
        job.next_attempt = 0.0
        job.state = "en attente" if job.peer else WAITING_PEER
        self._touch()
        self._pump()

    def pause(self, job: Job) -> None:
        """Suspend un transfert ; le `.part` garde la progression."""
        if job.state in ("terminé", "échec", "annulé", "en pause"):
            return
        job.state = "en pause"
        worker = self._worker
        if worker is not None and worker.job is job:
            worker.requestInterruption()
        self._touch()
        self._pump()

    def resume(self, job: Job) -> None:
        if job.state != "en pause":
            return
        job.attempts = 0
        job.next_attempt = 0.0
        job.state = "en attente" if job.peer else WAITING_PEER
        self._touch()
        self._pump()

    def cancel(self, job: Job) -> None:
        if job.state in ("en attente", "en cours", "en pause", WAITING_PEER, WAITING_SHARE):
            job.state = "annulé"
            worker = self._worker
            if worker is not None and worker.job is job:
                worker.requestInterruption()
            self._touch()
            self._pump()

    def shutdown(self, timeout_ms: int = 1500) -> None:
        """Arrête la file avant la fermeture de la fenêtre.

        Un `QThread` détruit pendant qu'il tourne fait **avorter le processus**
        (core dump) : on demande l'arrêt et on attend. Le `.part` reste sur
        disque et l'élément redevient « en attente », donc il repart tout seul
        au prochain lancement — même contrat que la reprise après réouverture.
        """
        actifs = [job for job in self.jobs if job.state == "en cours"]
        for job in actifs:
            # « en pause » d'abord : c'est ce qui empêche le worker d'interpréter
            # l'interruption comme une annulation.
            job.state = "en pause"
        self._retry_timer.stop()
        worker = self._worker
        if worker is not None:
            worker.requestInterruption()
            # Fermer la socket débloque une lecture en cours : sans ça il
            # faudrait attendre le timeout réseau (15 s) avant de rendre la main.
            worker.abort()
            # Attente courte : un transfert normal rend la main en quelques
            # millisecondes. Si la connexion est bloquée (poste muet), tant pis :
            # la file est écrite et la sortie du processus ne dépend plus de ce
            # fil (voir `app._leave`).
            worker.wait(timeout_ms)
            self._worker = None
        for job in actifs:
            job.state = "en attente" if job.peer else WAITING_PEER
            job.speed = 0.0
        self._save_timer.stop()
        self.save()

    def clear_finished(self) -> None:
        """Retire de la liste les transferts **réussis** (les fichiers restent).

        Un échec garde sa ligne : c'est justement celle qu'on veut relancer.
        Rien n'est effacé sur disque ici — le retrait d'un `.part` appartient à
        `remove()`.
        """
        keep = [job for job in self.jobs if job.state != "terminé"]
        if len(keep) != len(self.jobs):
            self.jobs = keep
            self._touch()

    def remove(self, job: Job) -> None:
        """Retire de la file **et** efface le `.part` laissé sur disque.

        Un élément retiré volontairement ne doit pas laisser un demi-fichier
        caché dans le dossier de destination pour toujours.
        """
        self.cancel(job)
        self._discard_parts(job)
        if job in self.jobs:
            self.jobs.remove(job)
            self._touch()

    @staticmethod
    def _discard_parts(job: Job) -> None:
        for candidate in job.dest.parent.glob(f".{job.dest.name}.*.part"):
            try:
                candidate.unlink()
            except OSError:
                pass

    def pending(self) -> int:
        return sum(1 for job in self.jobs if job.state in ("en attente", "en cours", WAITING_PEER, WAITING_SHARE))

    def sync_peers(self, peers) -> None:
        """La découverte vient de publier sa liste : on rattache les attentes."""
        self._known_peers = {peer.peer_id: peer for peer in peers}
        changed = False
        for job in self.jobs:
            if job.state in ("terminé", "échec", "annulé", "en cours"):
                continue
            peer = self._known_peers.get(job.peer_id)
            if peer is None:
                if job.state != WAITING_PEER or job.peer is not None:
                    job.peer = None
                    job.state = WAITING_PEER
                    changed = True
                continue
            job.peer = peer
            job.peer_name = peer.name
            job.attempts = 0
            job.next_attempt = 0.0
            if job.state == WAITING_PEER:
                job.state = "en attente"
                changed = True
            elif job.state == WAITING_SHARE and any(
                share.get("id") == job.share_id for share in peer.shares
            ):
                job.state = "en attente"
                changed = True
        if changed:
            self._touch()
            self._pump()

    def _prune(self) -> None:
        finished = [job for job in self.jobs if job.state in ("terminé", "annulé")]
        while len(finished) > KEEP_FINISHED:
            oldest = finished.pop(0)
            if oldest in self.jobs:
                self.jobs.remove(oldest)

    def _pump(self) -> None:
        if self._worker is not None:
            return
        for job in self.jobs:
            if job.state == "en attente" and job.peer is not None:
                self._start(job)
                return

    def _start(self, job: Job) -> None:
        worker = DownloadWorker(job, self)
        worker.progress.connect(self._on_progress)
        worker.completed.connect(self._on_completed)
        worker.failed.connect(self._on_failed)
        worker.waiting.connect(self._on_waiting)
        worker.finished.connect(self._on_worker_finished)
        self._worker = worker
        worker.start()

    # --- retours du worker ---------------------------------------------
    def _on_progress(self, job: Job) -> None:
        self._touch()

    def _on_completed(self, job: Job) -> None:
        job.done = job.total or job.done
        job.speed = 0.0
        job.attempts = 0
        self._touch()
        self.finished.emit(job, True)

    def _on_failed(self, job: Job, message: str) -> None:
        job.state = "échec"
        job.error = message
        job.speed = 0.0
        self._touch()
        self.finished.emit(job, False)

    def _on_waiting(self, job: Job, kind: str) -> None:
        job.peer = None
        job.attempts += 1
        # 5 s, 10, 20, 40, 80, 160 puis 300 s : on réessaie de plus en plus
        # espacé, sans jamais abandonner (un disque remonté doit repartir seul).
        delay = min(300, 5 * (2 ** (job.attempts - 1)))
        job.next_attempt = time.time() + delay
        if kind == "share":
            job.state = WAITING_SHARE
            job.error = "partage introuvable"
        else:
            job.state = WAITING_PEER
            job.error = "poste injoignable"
        self._touch()

    def _retry_waiting(self) -> None:
        """Relance périodique des éléments en attente (reprise transparente)."""
        now = time.time()
        changed = False
        for job in self.jobs:
            if job.state not in (WAITING_PEER, WAITING_SHARE):
                continue
            if job.next_attempt and job.next_attempt > now:
                continue
            peer = self._known_peers.get(job.peer_id)
            if peer is None:
                job.next_attempt = now + 60  # pas la peine d'essayer : poste absent
                continue
            job.peer = peer
            job.peer_name = peer.name
            job.state = "en attente"
            changed = True
        if changed:
            self._touch()
            self._pump()

    def summary(self) -> tuple[float, int, float]:
        """(débit cumulé, octets restants, secondes estimées) pour l'en-tête."""
        speed = sum(job.speed for job in self.jobs if job.state == "en cours")
        remaining = sum(
            max(0, job.total - job.done)
            for job in self.jobs
            if job.state in ("en attente", "en cours")
        )
        eta = remaining / speed if speed > 0 else 0.0
        return speed, remaining, eta

    def _on_worker_finished(self) -> None:
        self._worker = None
        self.save()
        self._pump()

