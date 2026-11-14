"""Cycle de vie du démon : lancé par la GUI, il meurt avec elle.

Un démon déjà présent (mode permanent, unité systemd) est adopté tel quel :
la GUI ne le tue pas, elle s'y rattache.
"""

import os
import subprocess
import time

from . import api, config, service
from .constants import CONFIG_PATH, PORT, daemon_binary

LOCAL_URL = f"http://127.0.0.1:{PORT}"


class Daemon:
    def __init__(self) -> None:
        self._process: subprocess.Popen | None = None
        self.external = False

    # --- lecture --------------------------------------------------------
    def info(self) -> dict | None:
        try:
            return api.info(LOCAL_URL, timeout=0.5)
        except api.ApiError:
            return None

    def running(self) -> bool:
        return self.info() is not None

    # --- démarrage ------------------------------------------------------
    def start(self) -> tuple[bool, str]:
        """Démarre le démon si nécessaire. (ok, message d'erreur)"""
        payload = self.info()
        if payload is not None:
            if str(payload.get("peer_id")) == config.machine_id():
                self.external = True  # permanence ou autre instance : on ne possède pas
                return True, ""
            return False, f"Le port {PORT} est occupé par un autre programme."

        # Permanence activée mais service arrêté : c'est lui qui doit servir,
        # pas un enfant de la GUI.
        if service.enabled():
            ok, message = service.start()
            if not ok:
                return False, message
            deadline = time.monotonic() + 5.0
            while time.monotonic() < deadline:
                if self.info() is not None:
                    self.external = True
                    return True, ""
                time.sleep(0.1)
            return False, "Le service permanent n'a pas répondu."

        binary = daemon_binary()
        if not binary:
            return False, "Démon introuvable : gablue-shared n'est pas installé."

        try:
            self._process = subprocess.Popen(
                [binary, "--config", str(CONFIG_PATH), "--parent-pid", str(os.getpid())],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        except OSError as error:
            return False, f"Démarrage impossible : {error}"

        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            if self.info() is not None:
                return True, ""
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
