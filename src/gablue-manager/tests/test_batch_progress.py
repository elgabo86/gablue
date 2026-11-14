"""Progression du lot de mises à jour distantes (fonctions pures).

Couvre les marqueurs que la chaîne d'un poste publie (`::gablue:stage`,
`::gablue:progress`) et leur lecture par `remote_updates.HostProgress`, sans
Qt ni ssh. Lancer depuis la racine :

    PYTHONPATH=src/gablue-manager python3 -m unittest discover \
        -s src/gablue-manager/tests -v
"""

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from gablue_manager import remote_updates, util  # noqa: E402


def _bytes_event(done: int, total: int) -> str:
    """Événement JSONL de bootc, tel que le helper le publie dans le FIFO."""
    return json.dumps({
        "type": "ProgressBytes",
        "bytes": done,
        "bytesTotal": total,
        "task": "pulling",
    })


class ChainMarkersTest(unittest.TestCase):
    def test_every_stage_is_announced(self):
        script = remote_updates.update_chain()
        total = len(remote_updates.STAGES)
        for index, label in enumerate(remote_updates.STAGES, start=1):
            self.assertIn(
                f"::gablue:stage {index} {total} {label}", script
            )

    def test_stage_order(self):
        script = remote_updates.update_chain()
        positions = [
            script.index(f"::gablue:stage {index} ")
            for index in range(1, len(remote_updates.STAGES) + 1)
        ]
        self.assertEqual(positions, sorted(positions))

    def test_image_progress_has_a_fallback(self):
        script = remote_updates.update_chain()
        # Progression par le helper (FIFO) puis repli sans helper.
        self.assertIn("/usr/libexec/gablue-update", script)
        self.assertIn("update-progress.fifo", script)
        self.assertIn("sudo -n /usr/bin/bootc upgrade || rc=1", script)

    def test_chain_exits_with_first_failure(self):
        script = remote_updates.update_chain()
        self.assertTrue(script.startswith("rc=0"))
        self.assertIn("exit $rc", script)

    def test_apps_section_only_when_pending(self):
        states = {
            "xenia": {"script": "/usr/bin/xenia-install", "options": ["--yes"]},
        }
        with_app = remote_updates.update_chain(["xenia"], states)
        without = remote_updates.update_chain()
        self.assertIn("/usr/bin/xenia-install", with_app)
        self.assertIn("Applications : à jour", without)
        self.assertNotIn("xenia-install", without)

    def test_no_step_marker_is_concatenated(self):
        # Un marqueur collé à une sortie sans saut de ligne final a déjà coûté
        # une section entière (09/11/2026) : chaque `printf` ouvre ET ferme.
        script = remote_updates.update_chain()
        for line in script.splitlines():
            if "::gablue:stage" in line:
                self.assertTrue(line.startswith("printf '"))
                self.assertTrue(line.endswith("\\n'"))


class HostProgressTest(unittest.TestCase):
    def setUp(self):
        self.state = remote_updates.HostProgress()

    def test_unknown_stage_is_indeterminate(self):
        self.assertIsNone(self.state.percent)
        self.assertEqual(self.state.fraction, 0.0)

    def test_steps_advance_the_chain(self):
        total = len(remote_updates.STAGES)
        for index, label in enumerate(remote_updates.STAGES, start=1):
            self.state.feed(f"::gablue:stage {index} {total} {label}\n")
            self.assertIsNone(self.state.percent)     # pas de mesure d'étape
            self.assertAlmostEqual(
                self.state.fraction, (index - 1) / total
            )
            self.assertEqual(self.state.label(), label)

    def test_bootc_bytes_drive_the_bar(self):
        self.state.feed("::gablue:stage 1 5 Image Gablue\n")
        self.state.feed(
            "::gablue:progress " + _bytes_event(50, 100) + "\n"
        )
        # 50 % de l'étape 1 sur 5 étapes → 10 % de la chaîne.
        self.assertAlmostEqual(self.state.percent, 10.0)
        self.assertTrue(self.state.label().startswith("Image Gablue · "))

    def test_progress_lines_never_reach_the_log(self):
        kept = self.state.feed(
            "::gablue:stage 1 5 Image Gablue\n"
            + "::gablue:progress " + _bytes_event(1, 2) + "\n"
            + "Queued for next boot\n"
        )
        self.assertEqual(kept, "Queued for next boot\n")

    def test_partial_line_is_buffered(self):
        self.assertEqual(self.state.feed("Queued for "), "")
        self.assertEqual(self.state.feed("next boot\n"), "Queued for next boot\n")

    def test_partial_protocol_line_is_not_logged(self):
        kept = self.state.feed("::gablue:stage 2 5 Fla")
        self.assertEqual(kept, "")
        self.assertEqual(self.state.feed("tpaks\n"), "")
        self.assertEqual(self.state.stage, "Flatpaks")

    def test_flush_returns_trailing_text(self):
        self.state.feed("Terminé sans saut de ligne")
        self.assertEqual(self.state.flush(), "Terminé sans saut de ligne")
        self.assertEqual(self.state.flush(), "")

    def test_finished_stage_reaches_full(self):
        self.state.feed("::gablue:stage 5 5 Homebrew\n")
        self.state.finished = True
        self.assertEqual(self.state.percent, 100.0)
        self.assertEqual(self.state.fraction, 1.0)

    def test_progress_without_stage_stays_indeterminate(self):
        # Un événement de bootc sans annonce d'étape ne doit pas fabriquer un
        # « 0 % » : la barre balaie, elle ne ment pas.
        self.state.feed("::gablue:progress " + _bytes_event(5, 10) + "\n")
        self.assertIsNone(self.state.percent)

    def test_app_markers_feed_the_current_stage(self):
        # Le moteur d'AppImages publie les verbes du protocole local : ils
        # mesurent l'étape en cours, ils ne renumérotent pas la chaîne.
        self.state.feed("::gablue:stage 4 5 Applications\n")
        self.state.feed("::gablue:busy Téléchargement de Xenia Edge\n")
        self.assertIsNone(self.state.percent)
        self.assertIn("Xenia Edge", self.state.label())
        self.state.feed("::gablue:percent 50.0 Téléchargement de Xenia Edge\n")
        self.assertAlmostEqual(self.state.percent, 70.0)   # (3 + 0,5) / 5
        self.state.feed("::gablue:step 2 4 Mise à jour suivante\n")
        self.assertAlmostEqual(self.state.percent, 65.0)   # (3 + 0,25) / 5
        self.assertEqual(self.state.stage, "Applications")
        self.state.feed("::gablue:done\n")
        self.assertAlmostEqual(self.state.percent, 80.0)

    def test_app_markers_never_reach_the_log(self):
        kept = self.state.feed(
            "::gablue:stage 4 5 Applications\n"
            "::gablue:busy Téléchargement de Xenia Edge\n"
            "::gablue:percent 12.5 Téléchargement…\n"
            "::gablue:done\n"
            "29,2 Mio vérifiés (AppImage ELF).\n"
        )
        self.assertEqual(kept, "29,2 Mio vérifiés (AppImage ELF).\n")

    def test_garbage_markers_are_ignored(self):
        self.assertEqual(
            self.state.feed("::gablue:progress pas du json\n"), ""
        )
        self.assertIsNone(self.state.percent)
        self.assertEqual(self.state.feed("::gablue:stage x y\n"), "")


class KeepaliveTest(unittest.TestCase):
    def test_long_tasks_get_a_keepalive(self):
        opts = util._ssh_opts(
            {"port": 22, "user": "gab", "host": "192.168.1.110"}
        )
        self.assertIn("ServerAliveInterval=15", opts)
        self.assertIn("ServerAliveCountMax=4", opts)


if __name__ == "__main__":
    unittest.main()
