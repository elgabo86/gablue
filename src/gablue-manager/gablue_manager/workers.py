"""Threads de fond (vérification, clé, commande distante, avahi).
"""

import os
import select
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed

from PySide6.QtCore import QThread, Signal

from . import apps, journal, remote_hardware, remote_updates, updates
from .constants import SSH
from .hosts import avahi_scan, remote_addresses
from .local import (hwinfo, memory_modules_from, nvm_smart,
                   pending_reboot_reasons, sata_temperatures,
                   vulkan_gpu_name)
from .ssh import _kill_group, check_host, install_key, key_status
from .util import _ssh_opts, display_name, ssh_target

class CheckWorker(QThread):
    """Vérifie tous les postes en parallèle, un signal par résultat."""

    result = Signal(str, str, str, float, str)   # id, état, détail, ms, adresse
    finished_all = Signal()

    def __init__(self, hosts: list[dict], parent=None):
        super().__init__(parent)
        self._hosts = [dict(host) for host in hosts]
        self._stop = threading.Event()

    def stop(self) -> None:
        self._stop.set()

    def run(self) -> None:
        if not self._hosts:
            self.finished_all.emit()
            return
        workers = max(1, min(6, len(self._hosts)))
        with ThreadPoolExecutor(max_workers=workers) as pool:
            futures = {
                pool.submit(check_host, host, self._stop): host["id"]
                for host in self._hosts
            }
            for future in as_completed(futures):
                host_id = futures[future]
                try:
                    state, detail, latency, address = future.result()
                except Exception as exc:  # jamais laisser un poste sans état
                    state, detail, latency, address = "error", str(exc), 0.0, ""
                if self._stop.is_set():
                    continue
                self.result.emit(host_id, state, detail, latency, address)
        self.finished_all.emit()

class KeyInstallWorker(QThread):
    """Copie la clé dans un thread : la fenêtre reste réactive."""

    log = Signal(str)
    confirm_host_key = Signal(str, object)   # nom affiché, [empreintes]
    done = Signal(bool, str)

    def __init__(self, host: dict, password: str, parent=None,
                 prescan: dict | None = None):
        super().__init__(parent)
        self._host = dict(host)
        self._password = password
        # Résultat de la sonde du dialogue (une connexion) : il porte les clés
        # annoncées et le fait que la clé est refusée, plus besoin de rescanner.
        self._prescan = prescan
        self.cancel = threading.Event()
        self.trust_event = threading.Event()
        self.trust_answer = False

    def _ask_trust(self, fingerprints: list[str]) -> bool:
        """Pose la question dans la GUI puis attend sa réponse."""
        self.trust_answer = False
        self.trust_event.clear()
        self.confirm_host_key.emit(display_name(self._host), fingerprints)
        while not self.trust_event.wait(0.2):
            if self.cancel.is_set():
                return False
        return self.trust_answer

    def run(self) -> None:
        try:
            ok, message = install_key(
                self._host, self._password, self.log.emit, self.cancel,
                self._ask_trust, self._prescan,
            )
        except Exception as exc:
            ok, message = False, str(exc)
        finally:
            self._password = ""
        self.done.emit(ok, message)

class KeyProbeWorker(QThread):
    """Une seule connexion : empreinte de l'hôte ET clé déjà acceptée ?

    Elle sert aussi de pré-scan à l'installation (`prescan`) : sans ça, la
    copie repayerait une connexion refusée, et deux échecs rapprochés suffisent
    à faire bloquer le client par `PerSourcePenalties` (5 s par échec, seuil
    15 s).
    """

    done = Signal(object)      # key_status()

    def __init__(self, host: dict, parent=None):
        super().__init__(parent)
        self._host = dict(host)

    def run(self) -> None:
        try:
            info = key_status(self._host)
        except Exception as exc:      # le dialogue ne doit jamais rester muet
            info = {
                "authenticated": False, "state": "error", "raw": "",
                "fingerprints": [], "stored": [], "error": str(exc),
            }
        self.done.emit(info)

class AddressWorker(QThread):
    """Adresses d'un poste (LAN + Tailscale), lues chez lui en une connexion."""

    done = Signal(str, object)      # id du poste, [{address, kind}]

    def __init__(self, host: dict, parent=None):
        super().__init__(parent)
        self._host = dict(host)

    def run(self) -> None:
        try:
            found = remote_addresses(self._host)
        except Exception:            # une sonde d'agrément ne doit rien casser
            found = []
        self.done.emit(self._host["id"], found)

class RebootWorker(QThread):
    """Ce qui attend un redémarrage (image staged, kargs, modules)."""

    done = Signal(object)           # liste de raisons lisibles

    def run(self) -> None:
        try:
            reasons = pending_reboot_reasons()
        except Exception:            # une sonde d'agrément ne doit rien casser
            reasons = []
        self.done.emit(reasons)

class HardwareWorker(QThread):
    """Matériel lu hors thread GUI : barrettes (root) et nom Vulkan du GPU.

    `dmidecode` exige root : le helper NOPASSWD `gablue-hwinfo` rend un JSON.
    Absent (image antérieure), la liste reste vide — la mémoire totale vient de
    /proc/meminfo et ne dépend pas de ce worker. Vulkan charge les pilotes
    pour donner le nom commercial du GPU : quelques dizaines de ms qui n'ont
    rien à faire sur le thread de l'interface.
    """

    done = Signal(object)           # {"modules": [...], "gpu": "...", "disks": [...]}

    def run(self) -> None:
        result: dict = {"modules": [], "gpu": "", "disks": [], "nvm": {}}
        try:
            payload = hwinfo()       # un seul appel root : barrettes + disques
            result["modules"] = memory_modules_from(payload)
            result["disks"] = sata_temperatures(payload)
            result["nvm"] = nvm_smart(payload)
        except Exception:            # une sonde d'agrément ne doit rien casser
            pass
        try:
            result["gpu"] = vulkan_gpu_name()
        except Exception:
            pass
        self.done.emit(result)

class UpdateCheckWorker(QThread):
    """Sonde les mises à jour disponibles (image, Flatpaks, brew, gwine).

    `enabled` vient des préférences, `online` de la barre d'état
    (`QNetworkInformation`) : hors ligne, seules les sondes locales tournent.
    """

    done = Signal(object)           # état renvoyé par updates.check_all

    def __init__(self, enabled: dict[str, bool] | None = None,
                 online: bool = True, parent=None):
        super().__init__(parent)
        self._enabled = dict(enabled) if enabled is not None else None
        self._online = bool(online)
        self._stop = threading.Event()

    def stop(self) -> None:
        """Fermeture de la fenêtre : tuer les commandes en cours.

        Sans cela, un `brew update` de deux minutes retient le processus
        après la fermeture, et un QThread détruit en vol fait tomber
        l'application (vécu 07/11/2026).
        """
        self._stop.set()
        updates.cancel_running()

    def run(self) -> None:
        try:
            state = updates.check_all(self._enabled, self._online)
        except Exception as error:   # une sonde d'agrément ne doit rien casser
            journal.log_failure(
                "Vérification des mises à jour interrompue",
                detail=repr(error),
            )
            state = {
                "checked_at": time.time(),
                "online": self._online,
                "domains": {},
            }
        self.done.emit(state)

class AppsUpdateWorker(QThread):
    """Applications AppImage installées qui ont une mise à jour.

    La sonde interroge un flux de releases par application : elle reste hors
    du fil graphique, comme les autres vérifications réseau.
    """

    done = Signal(object)           # (clés à mettre à jour, doute, installées)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._stop = threading.Event()

    def stop(self) -> None:
        self._stop.set()

    def run(self) -> None:
        try:
            result = apps.pending_updates(stop=self._stop)
        except Exception as error:   # une sonde ne doit rien casser
            journal.log_failure(
                "Vérification des applications interrompue",
                detail=repr(error),
            )
            result = ([], True, 0)
        self.done.emit(result)

class RemoteAppsWorker(QThread):
    """AppImages en retard sur un poste distant (une connexion SSH + les flux).

    Sonde **à la demande** : elle ne part que d'un clic (mise à jour de ce
    poste, lot sur les postes en ligne), jamais d'un timer. Les flux de
    releases se lisent d'ici, le poste n'a rien à télécharger pour être jugé.
    """

    done = Signal(str, object)      # id, (à mettre à jour, doute, installées, états)

    def __init__(self, host: dict, parent=None):
        super().__init__(parent)
        self._id = host["id"]
        self._host = dict(host)

    def run(self) -> None:
        try:
            result = remote_updates.probe_apps(self._host)
        except Exception as error:   # une sonde ne doit rien casser
            journal.log_failure(
                f"Applications de {display_name(self._host)}",
                detail=repr(error),
            )
            result = ([], True, 0, {})
        self.done.emit(self._id, result)

class RemoteUpdatesWorker(QThread):
    """Mises à jour d'un poste distant, sondées à la demande.

    Une seule connexion SSH (`remote_updates.CHECK_SCRIPT`) et l'analyse ici ;
    aucun lancement périodique : la carte d'un poste ne sait rien tant que
    l'utilisateur n'a pas demandé.
    """

    done = Signal(str, object)      # id du poste, état au format « local »

    def __init__(self, host: dict, parent=None):
        super().__init__(parent)
        self._id = host["id"]
        self._host = dict(host)

    def run(self) -> None:
        try:
            state = remote_updates.check(self._host)
        except Exception as error:   # une sonde ne doit rien casser
            journal.log_failure(
                f"Vérification des mises à jour de {self._host.get('name', '')}",
                detail=repr(error),
            )
            state = {"online": False, "domains": {}}
        self.done.emit(self._id, state)

class RemoteHardwareWorker(QThread):
    """Résumé matériel d'un poste distant, en une seule connexion SSH.

    Chargé seulement quand l'onglet du poste s'ouvre (ou au premier
    rafraîchissement de cet onglet) : aucun balayage périodique du réseau.
    """

    done = Signal(str, object)      # id du poste, valeurs pour InfoPanel

    def __init__(self, host: dict, parent=None):
        super().__init__(parent)
        self._id = host["id"]
        self._host = dict(host)

    def run(self) -> None:
        try:
            values = remote_hardware.check(self._host)
        except Exception as error:   # une sonde d'agrément ne doit rien casser
            journal.log_failure(
                f"Matériel de {display_name(self._host)}",
                detail=repr(error),
            )
            values = {}
        self.done.emit(self._id, values)

class SshTaskWorker(QThread):
    """Exécute une commande distante en streaming (arrêt, redémarrage, mise à jour)."""

    log = Signal(str)
    done = Signal(int)

    def __init__(self, host: dict, command: str, parent=None,
                 password: str = ""):
        super().__init__(parent)
        self._host = dict(host)
        self._command = command
        # Mot de passe sudo éventuel : écrit sur stdin (jamais en argv, donc
        # invisible dans `ps`), utilisé une seule fois par `sudo -S -v`.
        self._password = password
        self.cancel = threading.Event()

    def run(self) -> None:
        argv = [
            SSH, *_ssh_opts(self._host), ssh_target(self._host), self._command,
        ]
        try:
            proc = subprocess.Popen(
                argv,
                stdin=subprocess.PIPE if self._password else subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, start_new_session=True,
            )
        except OSError as exc:
            self.log.emit(f"ssh introuvable ({exc})\n")
            self.done.emit(127)
            return
        if self._password and proc.stdin is not None:
            try:
                proc.stdin.write((self._password + "\n").encode("utf-8"))
                proc.stdin.flush()
                proc.stdin.close()
            except OSError:
                pass
        assert proc.stdout is not None
        fd = proc.stdout.fileno()
        try:
            while True:
                if self.cancel.is_set():
                    _kill_group(proc)
                    self.log.emit("\n[annulé]\n")
                    break
                try:
                    ready, _, _ = select.select([fd], [], [], 0.2)
                except (OSError, ValueError):
                    break
                if ready:
                    try:
                        chunk = os.read(fd, 4096)
                    except OSError:
                        chunk = b""
                    if not chunk:
                        break
                    self.log.emit(chunk.decode("utf-8", "replace"))
                elif proc.poll() is not None:
                    break
        finally:
            proc.stdout.close()
        try:
            code = proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            _kill_group(proc)
            code = proc.returncode if proc.returncode is not None else 1
        self.done.emit(code)

class AvahiWorker(QThread):
    """Recherche mDNS des services SSH du réseau local."""

    found = Signal(object)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.stop = threading.Event()

    def run(self) -> None:
        try:
            self.found.emit(avahi_scan(stop=self.stop))
        except Exception:
            self.found.emit([])
