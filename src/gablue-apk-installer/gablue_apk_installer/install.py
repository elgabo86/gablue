"""Installation des APK (fichier simple ou bundle) sur un appareil."""

import glob as globmod
import os
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

from PySide6.QtCore import QThread, Signal

from .constants import BUNDLE_EXTENSIONS

# =============================================================================
# THREAD D'INSTALLATION
# =============================================================================

class InstallThread(QThread):
    """Installe les APK sur l'appareil sélectionné dans un thread séparé."""

    progress_update = Signal(str)
    install_finished = Signal(bool, str)
    file_progress = Signal(int, int)

    def __init__(self, serial, files):
        super().__init__()
        self.serial = serial
        self.files = files
        self._cancelled = False

    def cancel(self):
        self._cancelled = True

    def run(self):
        total = len(self.files)

        for idx, filepath in enumerate(self.files):
            if self._cancelled:
                self.install_finished.emit(False, "Installation annulée.")
                return

            filename = os.path.basename(filepath)
            ext = Path(filepath).suffix.lower()

            if not os.path.isfile(filepath):
                self.install_finished.emit(False, f"Fichier introuvable : {filename}")
                return

            self.file_progress.emit(idx, total)

            try:
                if ext in BUNDLE_EXTENSIONS:
                    self._install_bundle(filepath)
                else:
                    self._install_single(filepath)
            except Exception as e:
                self.install_finished.emit(False, str(e))
                return

        self.file_progress.emit(total, total)
        self.install_finished.emit(True, f"{total} fichier(s) installé(s) avec succès.")

    def _run_adb(self, args, timeout=120):
        """Exécute une commande adb et retourne stdout."""
        full_args = ['adb', '-s', self.serial] + args
        try:
            result = subprocess.run(
                full_args, capture_output=True, text=True, timeout=timeout
            )
        except subprocess.TimeoutExpired:
            raise RuntimeError("La commande adb a expiré (délai dépassé).")
        except FileNotFoundError:
            raise RuntimeError("adb n'est pas installé ou introuvable sur le système.")

        if result.returncode != 0:
            error_msg = result.stderr.strip() or result.stdout.strip() or "Erreur inconnue."
            raise RuntimeError(error_msg)

        return result.stdout.strip()

    def _install_single(self, filepath):
        """Installe un APK simple avec remplacement et downgrade automatiques."""
        filename = os.path.basename(filepath)
        self.progress_update.emit(f"Installation de {filename}...")
        output = self._run_adb(['install', '-r', '-d', filepath])
        self.progress_update.emit(f"{filename} : {output}")

    def _install_bundle(self, filepath):
        """Extrait un bundle et installe les APK via install-multiple."""
        filename = os.path.basename(filepath)
        self.progress_update.emit(f"Extraction de {filename}...")

        temp_dir = tempfile.mkdtemp(prefix='gablue-apk-')
        try:
            with zipfile.ZipFile(filepath, 'r') as zf:
                zf.extractall(temp_dir)

            apk_files = sorted(globmod.glob(
                os.path.join(temp_dir, '**', '*.apk'), recursive=True
            ))

            if not apk_files:
                raise RuntimeError(f"Aucun APK trouvé dans le bundle {filename}.")

            self.progress_update.emit(
                f"Installation de {len(apk_files)} APK depuis {filename}..."
            )

            install_args = ['install-multiple', '-r', '-d'] + apk_files
            output = self._run_adb(install_args)
            self.progress_update.emit(f"{filename} : {output}")

        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)
