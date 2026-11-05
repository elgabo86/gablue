"""Découverte et connexion des appareils ADB (USB et WiFi)."""

import re
import subprocess

from PySide6.QtCore import QThread, Signal

from .constants import ADB_DEFAULT_PORT

# =============================================================================
# SCANNER DE PÉRIPHÉRIQUES ADB
# =============================================================================

class DeviceScanner(QThread):
    """Scanne les appareils connectés via ADB dans un thread séparé."""

    devices_found = Signal(list)  # [(serial, state, model, product, is_network), ...]

    def run(self):
        try:
            result = subprocess.run(
                ['adb', 'devices', '-l'],
                capture_output=True, text=True, timeout=10
            )
            output = result.stdout
        except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
            self.devices_found.emit([])
            return

        devices = []
        for line in output.strip().split('\n')[1:]:
            line = line.strip()
            if not line:
                continue

            parts = line.split()
            if len(parts) < 2:
                continue

            serial = parts[0]
            state = parts[1]
            is_network = ':' in serial  # IP:port → réseau

            props = {}
            for part in parts[2:]:
                if ':' in part:
                    key, value = part.split(':', 1)
                    props[key] = value

            model = props.get('model', serial)
            product = props.get('product', '')
            devices.append((serial, state, model, product, is_network))

        self.devices_found.emit(devices)

# =============================================================================
# THREAD DE CONNEXION RÉSEAU (USB → WiFi)
# =============================================================================

class NetworkConnectThread(QThread):
    """Active le mode TCP/IP sur le téléphone et s'y connecte en WiFi."""

    finished = Signal(bool, str)  # succès, message

    def __init__(self, serial=None, is_already_network=False,
                 pair_addr=None, pair_code=None, connect_addr=None):
        super().__init__()
        self.serial = serial
        self.is_already_network = is_already_network
        self.pair_addr = pair_addr
        self.pair_code = pair_code
        self.connect_addr = connect_addr

    def run(self):
        try:
            if self.pair_addr and self.pair_code and self.connect_addr:
                # Mode appairage sans fil (Android 11+, pas d'USB)
                self._run_adb(None, ['pair', self.pair_addr, self.pair_code])
                self._run_adb(None, ['connect', self.connect_addr])
                self.finished.emit(True,
                    f"Appairé et connecté : {self.connect_addr}\n"
                    f"L'appareil apparaîtra dans la liste."
                )
                return

            ip = None

            if self.is_already_network:
                ip = self.serial
            else:
                self._run_adb(self.serial, ['tcpip', str(ADB_DEFAULT_PORT)])
                ip = self._get_device_ip(self.serial)

            if not ip:
                self.finished.emit(False,
                    "Impossible de détecter l'adresse IP du téléphone.\n"
                    "Vérifiez que le WiFi est activé sur le téléphone."
                )
                return

            self._run_adb(None, ['connect', ip])
            self.finished.emit(True,
                f"Connecté en WiFi : {ip}\n"
                f"Vous pouvez débrancher le câble USB."
            )

        except Exception as e:
            self.finished.emit(False, str(e))

    def _get_device_ip(self, serial):
        """Récupère l'adresse IP WiFi du téléphone."""
        # Méthode 1 : ip addr show wlan0
        try:
            result = subprocess.run(
                ['adb', '-s', serial, 'shell', 'ip', '-4', 'addr', 'show', 'wlan0'],
                capture_output=True, text=True, timeout=10
            )
            match = re.search(r'inet\s+(\d+\.\d+\.\d+\.\d+)', result.stdout)
            if match:
                ip = match.group(1)
                return f"{ip}:{ADB_DEFAULT_PORT}"
        except Exception:
            pass

        # Méthode 2 : ip route (fallback)
        try:
            result = subprocess.run(
                ['adb', '-s', serial, 'shell', 'ip', 'route'],
                capture_output=True, text=True, timeout=10
            )
            for line in result.stdout.split('\n'):
                if 'wlan0' in line:
                    match = re.search(r'src\s+(\d+\.\d+\.\d+\.\d+)', line)
                    if match:
                        ip = match.group(1)
                        return f"{ip}:{ADB_DEFAULT_PORT}"
        except Exception:
            pass

        # Méthode 3 : getprop (dernier recours)
        try:
            result = subprocess.run(
                ['adb', '-s', serial, 'shell', 'getprop', 'dhcp.wlan0.ipaddress'],
                capture_output=True, text=True, timeout=10
            )
            ip = result.stdout.strip()
            if ip and re.match(r'\d+\.\d+\.\d+\.\d+', ip):
                return f"{ip}:{ADB_DEFAULT_PORT}"
        except Exception:
            pass

        return None

    def _run_adb(self, serial, args, timeout=15):
        """Exécute une commande adb."""
        cmd = ['adb']
        if serial:
            cmd += ['-s', serial]
        cmd += args

        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)

        if result.returncode != 0:
            error = result.stderr.strip() or result.stdout.strip() or "Erreur inconnue."
            raise RuntimeError(error)

        return result.stdout.strip()
