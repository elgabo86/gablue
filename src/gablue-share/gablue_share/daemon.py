"""Cycle de vie du démon : lancé par la GUI, il **survit** à sa fenêtre.

Fermer Gablue Share ne coupe plus le partage : le démon est détaché et tourne
jusqu'à l'extinction du poste, la déconnexion, ou la disparition du dernier
dossier partagé. Un démon déjà présent (le nôtre, ou l'unité systemd) est
adopté tel quel — mais seulement s'il est bien **au même compte**.
"""

import os
import pwd
import subprocess
import time

from . import api, config, service
from .constants import CONFIG_PATH, PORT, daemon_binary, launcher

LOCAL_URL = f"http://127.0.0.1:{PORT}"

# Qui répond sur le port.
MINE = "mine"
ABSENT = "absent"
OTHER_ACCOUNT = "other"
FOREIGN = "foreign"


class Daemon:
    def __init__(self) -> None:
        self._process: subprocess.Popen | None = None
        self.external = False
        # Résultat du dernier `probe()` : `window.py` s'en sert pour empêcher
        # de « partager » quand ce poste est déjà pris par un autre compte.
        self.kind = ABSENT
        self.message = ""
        #: L'icône est portée par le démon ; les tests la coupent (`--no-tray`).
        self.tray = True
        #: Dernière réponse du démon (l'icône de notification y lit le nombre de
        #: partages sans refaire de requête).
        self.payload: dict | None = None

    # --- lecture --------------------------------------------------------
    def info(self) -> dict | None:
        try:
            return api.info(LOCAL_URL, timeout=0.5)
        except api.ApiError:
            return None

    def running(self) -> bool:
        return self.info() is not None

    def probe(self) -> tuple[str, str]:
        """Qui écoute : le mien, un autre compte, un étranger, personne.

        Le port est fixe et `peer_id` vaut le machine-id : deux comptes d'un même
        poste se ressemblent trait pour trait, seul `uid` les sépare. Sans ce
        test, la GUI du second compte adoptait le démon de l'autre (même
        `peer_id`) et croyait partager — alors qu'aucun de ses dossiers n'était
        servi, et sans le moindre message (vécu 15/11/2026).
        """
        payload = self.info()
        self.payload = payload
        if payload is None:
            return ABSENT, ""
        if str(payload.get("peer_id")) != config.machine_id():
            return FOREIGN, f"Le port {PORT} est occupé par un autre programme."
        owner = payload.get("uid")
        # Démon d'une version antérieure (pas de `uid`) : on ne peut pas
        # trancher, on l'adopte comme avant plutôt que de bloquer à tort.
        if owner is not None and int(owner) != os.getuid():
            return OTHER_ACCOUNT, self._other_message(int(owner))
        return MINE, ""

    @staticmethod
    def _other_message(uid: int) -> str:
        try:
            nom = pwd.getpwuid(uid).pw_name
        except KeyError:
            nom = f"uid {uid}"
        return (
            f"Le compte « {nom} » partage déjà sur ce poste : vos dossiers ne "
            "peuvent pas être partagés tant que ce partage est actif."
        )

    # --- démarrage ------------------------------------------------------
    def start(self) -> tuple[bool, str]:
        """Démarre le démon si nécessaire. (ok, message d'erreur)"""
        self.kind, self.message = self.probe()
        if self.kind == MINE:
            self.external = True  # permanence ou autre instance : on ne possède pas
            return True, ""
        if self.kind != ABSENT:
            return False, self.message

        # Permanence activée mais service arrêté : c'est lui qui doit servir,
        # pas un enfant de la GUI.
        if service.enabled():
            ok, message = service.start()
            if not ok:
                self.message = message
                return False, message
            deadline = time.monotonic() + 5.0
            while time.monotonic() < deadline:
                self.kind, self.message = self.probe()
                if self.kind == MINE:
                    self.external = True
                    return True, ""
                if self.kind != ABSENT:
                    return False, self.message
                time.sleep(0.1)
            return False, "Le service permanent n'a pas répondu."

        binary = daemon_binary()
        if not binary:
            return False, "Démon introuvable : gablue-shared n'est pas installé."

        arguments = [binary, "--config", str(CONFIG_PATH)]
        programme = launcher()
        if programme:
            arguments += ["--launcher", programme]
        if not self.tray:
            arguments.append("--no-tray")
        try:
            self._process = subprocess.Popen(
                arguments,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                # Détaché : le partage survit à la fermeture de la fenêtre,
                # jusqu'à l'extinction du poste ou la déconnexion.
                start_new_session=True,
            )
        except OSError as error:
            return False, f"Démarrage impossible : {error}"

        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            self.kind, self.message = self.probe()
            if self.kind == MINE:
                return True, ""
            if self.kind != ABSENT:
                return False, self.message
            if self._process.poll() is not None:
                return False, "Le démon s'est arrêté au démarrage (configuration ?)."
            time.sleep(0.1)
        return False, "Le démon n'a pas répondu."

    # --- arrêt ----------------------------------------------------------
    def stop(self) -> None:
        process = self._process
        self._process = None
        if not process or process.poll() is not None:
            return
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
