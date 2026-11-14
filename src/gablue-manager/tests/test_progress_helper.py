"""Progression des scripts de l'image (helpers bash, moteur `download`).

Deux choses se vérifient sans root et sans réseau réel :

- `/usr/libexec/gablue-progress` écrit les marqueurs `::gablue:` quand sa
  sortie est un tube et **rien** sur un terminal (une jauge machine n'y
  apprend rien à l'humain) ;
- les scripts restés en bash (`esde-install`, `opencode2-install`,
  `gablue-fetch`) et le helper sont syntaxiquement valides.

    PYTHONPATH=src/gablue-manager python3 -m unittest discover \
        -s src/gablue-manager/tests -v
"""

import os
import pty
import select
import shutil
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SYSTEM = ROOT / "files/system/all"
HELPER = SYSTEM / "usr/libexec/gablue-progress"
SCRIPTS = (
    SYSTEM / "usr/libexec/gablue-progress",
    SYSTEM / "usr/libexec/gablue-fetch",
    SYSTEM / "usr/bin/esde-install",
    SYSTEM / "usr/bin/opencode2-install",
)


class GablueProgressTest(unittest.TestCase):
    def _run(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["bash", str(HELPER), *args],
            capture_output=True, text=True, check=False,
        )

    def test_emits_markers_on_a_pipe(self):
        result = self._run("step", "2", "5", "Applications")
        self.assertEqual(result.stdout, "::gablue:step 2 5 Applications\n")
        self.assertEqual(self._run("busy", "Intégration").stdout,
                         "::gablue:busy Intégration\n")
        self.assertEqual(self._run("percent", "42.5", "Transfert").stdout,
                         "::gablue:percent 42.5 Transfert\n")
        self.assertEqual(self._run("done").stdout, "::gablue:done \n")

    def test_silent_on_a_terminal(self):
        master, slave = pty.openpty()
        try:
            proc = subprocess.run(
                ["bash", str(HELPER), "step", "1", "2", "x"],
                stdout=slave, stderr=subprocess.DEVNULL, check=False,
            )
            os.close(slave)
            slave = -1
            readable, _, _ = select.select([master], [], [], 0.5)
            output = b""
            if readable:
                try:
                    output = os.read(master, 4096)
                except OSError:
                    output = b""      # esclave déjà fermé : rien n'a été écrit
        finally:
            if slave >= 0:
                os.close(slave)
            os.close(master)
        self.assertEqual(proc.returncode, 0)
        self.assertEqual(output, b"")

    def test_unknown_verb_is_an_error(self):
        result = self._run("bogus")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Usage", result.stderr)

    @unittest.skipUnless(shutil.which("bash"), "bash absent")
    def test_scripts_are_valid_bash(self):
        for script in SCRIPTS:
            with self.subTest(script=script.name):
                result = subprocess.run(
                    ["bash", "-n", str(script)],
                    capture_output=True, text=True, check=False,
                )
                self.assertEqual(
                    result.returncode, 0, result.stderr
                )

    def test_installers_publish_progress(self):
        esde = (SYSTEM / "usr/bin/esde-install").read_text()
        opencode = (SYSTEM / "usr/bin/opencode2-install").read_text()
        # Le téléchargement vient du moteur (`gablue-fetch`), les phases des
        # scripts : pas de fenêtre muette pendant 245 Mio.
        self.assertIn("/usr/libexec/gablue-fetch", esde)
        self.assertIn('gprogress busy "Intégration d\'ES-DE', esde)
        self.assertIn("Étape 4/5 — OpenCode Desktop", opencode)
        self.assertIn("gprogress busy ", opencode)


if __name__ == "__main__":
    unittest.main()
