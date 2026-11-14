"""Fenêtres de tâche hors écran : la progression alimentée par les marqueurs.

PySide6 est requis (`QT_QPA_PLATFORM=offscreen`, `HOME` de bac à sable) : le
module est **sauté** s'il manque, les autres tests restant sans dépendance.

    PYTHONPATH=src/gablue-manager python3 -m unittest discover \
        -s src/gablue-manager/tests -v
"""

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

try:
    from PySide6.QtWidgets import QApplication
except ImportError:                       # image sans PySide6, test sauté
    QApplication = None

HOST = {"id": "h", "label": "sucina", "user": "gab", "host": "sucina",
        "port": 22}


def _event(done: int, total: int) -> str:
    return "::gablue:progress " + json.dumps({
        "type": "ProgressBytes", "bytes": done, "bytesTotal": total,
        "task": "pulling",
    }) + "\n"


@unittest.skipIf(QApplication is None, "PySide6 absent")
class RemoteTaskDialogTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.home = tempfile.TemporaryDirectory()
        os.environ["HOME"] = cls.home.name
        cls.app = QApplication.instance() or QApplication([])

    @classmethod
    def tearDownClass(cls):
        cls.home.cleanup()

    def _dialog(self):
        from gablue_manager import tasks

        return tasks.RemoteTaskDialog(
            None, host=HOST, title="Mise à jour", command="true",
            progress_mode="busy",
        )

    def test_percent_only_chunks_move_the_bar(self):
        # Un téléchargement d'image n'émet **que** des `percent` : la barre
        # doit avancer sans qu'une ligne humaine arrive (bug du 14/11/2026).
        dialog = self._dialog()
        dialog._append("::gablue:stage 1 5 Image Gablue\n")
        dialog._append(_event(250, 1000))
        self.assertAlmostEqual(dialog.progress_bar.value(), 50.0, places=0)
        self.assertIn("Téléchargement", dialog.progress_label.text())
        dialog._append(_event(900, 1000))
        self.assertAlmostEqual(dialog.progress_bar.value(), 180.0, places=0)

    def test_protocol_lines_stay_out_of_the_journal(self):
        dialog = self._dialog()
        dialog._append("::gablue:stage 2 5 Flatpaks\n")
        dialog._append("Updating 1/2…\n")
        self.assertEqual("".join(dialog._output), "Updating 1/2…\n")

    def test_finished_reaches_full(self):
        dialog = self._dialog()
        dialog._append("::gablue:stage 5 5 Homebrew\n")
        dialog._finished(0)
        self.assertEqual(dialog.progress_bar.value(), 100)
        self.assertIn("Terminé", dialog.progress_label.text())


if __name__ == "__main__":
    unittest.main()
