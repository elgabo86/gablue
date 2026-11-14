"""Téléchargements : un fichier repris (`Range` + ETag) ou un dossier en tar.

Le `.part` reste dans le dossier de destination, donc fermer l'app ne perd
rien : au prochain téléchargement du même fichier, la reprise repart du
dernier octet confirmé si l'ETag n'a pas changé.
"""

import os
import tarfile
import time
from pathlib import Path

from PySide6.QtCore import QObject, QThread, Signal

from . import api

CHUNK = 256 * 1024


class Cancelled(Exception):
    """Interruption demandée par l'utilisateur ou par la fermeture de l'app."""


class Job:
    """Un téléchargement demandé par l'utilisateur."""

    def __init__(self, peer, share_id: str, share_name: str, path: str, name: str, is_dir: bool, dest: Path):
        self.peer = peer
        self.share_id = share_id
        self.share_name = share_name
        self.path = path
        self.name = name
        self.is_dir = is_dir
        self.dest = dest
        self.state = "en attente"
        self.done = 0
        self.total = 0
        self.error = ""
        self.started_at = 0.0
        self.finished_at = 0.0

    # --- affichage ------------------------------------------------------
    def title(self) -> str:
        return self.name + (" (dossier)" if self.is_dir else "")

    def subtitle(self) -> str:
        source = f"{self.peer.name} · {self.share_name}"
        if self.state == "échec":
            return f"{source} — {self.error}"
        return source

    def fraction(self) -> float:
        if self.total <= 0:
            return 0.0
        return min(1.0, self.done / self.total)


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

    def __init__(self, job: Job, parent=None):
        super().__init__(parent)
        self.job = job

    def run(self) -> None:
        self.job.state = "en cours"
        self.job.started_at = time.time()
        try:
            if self.job.is_dir:
                self._download_folder()
            else:
                self._download_file()
            self.job.state = "terminé"
            self.job.finished_at = time.time()
            self.completed.emit(self.job)
        except Cancelled:
            self.job.state = "annulé"
            self.job.finished_at = time.time()
        except api.ApiError as error:
            self._fail(str(error))
        except (OSError, tarfile.TarError) as error:
            self._fail(f"{type(error).__name__} : {error}")

    def _fail(self, message: str) -> None:
        self.job.state = "échec"
        self.job.error = message
        self.job.finished_at = time.time()
        self.failed.emit(self.job, message)

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

        destination = self.job.dest
        destination.parent.mkdir(parents=True, exist_ok=True)
        part = part_path(destination, etag)
        start = part.stat().st_size if part.exists() else 0
        if start and (size and start >= size):
            if size and start == size:
                os.replace(part, destination)  # déjà complet
                self.job.done = size
                self.progress.emit(self.job)
                return
            start = 0

        if start:
            self.job.done = start
            self.progress.emit(self.job)

        response, offset = api.open_file(base, self.job.share_id, self.job.path, start, etag)
        with response:
            mode = "ab" if offset else "wb"
            if offset == 0:
                self.job.done = 0
                start = 0
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
            self.progress.emit(self.job)

        response = api.open_tar(base, self.job.share_id, self.job.path)
        with response:
            stream = _Counter(response, account, self.isInterruptionRequested)
            # filter="data" refuse les chemins absolus et les sorties d'arbre.
            with tarfile.open(fileobj=stream, mode="r|") as archive:
                archive.extractall(path=destination.parent, filter="data")
        self.job.total = received
        self.progress.emit(self.job)


class Downloads(QObject):
    """File simple : un transfert à la fois, dans l'ordre des demandes."""

    changed = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.jobs: list[Job] = []
        self._worker: DownloadWorker | None = None

    def enqueue(self, job: Job) -> None:
        self.jobs.append(job)
        self.changed.emit()
        self._pump()

    def pending(self) -> int:
        return sum(1 for job in self.jobs if job.state in ("en attente", "en cours"))

    def cancel(self, job: Job) -> None:
        if job.state in ("en attente", "en cours"):
            job.state = "annulé"
            worker = self._worker
            if worker is not None and worker.job is job:
                worker.requestInterruption()
            self.changed.emit()
            self._pump()

    def shutdown(self) -> None:
        """Arrêt propre à la fermeture : aucun transfert ne survit à la fenêtre."""
        for job in self.jobs:
            if job.state in ("en attente", "en cours"):
                job.state = "annulé"
        worker = self._worker
        if worker is None:
            return
        worker.requestInterruption()
        if not worker.wait(3000):
            worker.terminate()
            worker.wait(1000)
        self._worker = None

    def _pump(self) -> None:
        if self._worker is not None:
            return
        for job in self.jobs:
            if job.state == "en attente":
                self._start(job)
                return

    def _start(self, job: Job) -> None:
        worker = DownloadWorker(job, self)
        worker.progress.connect(self._on_progress)
        worker.completed.connect(self._on_completed)
        worker.failed.connect(self._on_failed)
        worker.finished.connect(self._on_worker_finished)
        self._worker = worker
        worker.start()

    def _on_progress(self, job: Job) -> None:
        self.changed.emit()

    def _on_completed(self, job: Job) -> None:
        self.changed.emit()

    def _on_failed(self, job: Job, message: str) -> None:
        self.changed.emit()

    def _on_worker_finished(self) -> None:
        self._worker = None
        self._pump()
