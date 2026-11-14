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

    def test_no_stage_marker_is_concatenated(self):
        # Un marqueur collé à une sortie sans saut de ligne final a déjà coûté
        # une section entière (09/11/2026) : chaque `printf` ouvre ET ferme.
        script = remote_updates.update_chain()
        for line in script.splitlines():
            if "::gablue:stage" in line:
                self.assertTrue(line.startswith("printf '\\n%s\\n' "))
                self.assertTrue(line.rstrip().endswith("'"))


class ImageChainTest(unittest.TestCase):
    """Action « Mettre à jour Gablue » d'un poste seul.

    Elle lançait `bootc upgrade` nu : aucune étape annoncée, donc ni barre ni
    libellé pendant tout le tirage de l'image (même trou que le lot, corrigé).
    """

    def test_single_stage_is_announced(self):
        script = remote_updates.image_chain()
        self.assertIn(
            f"::gablue:stage 1 1 {remote_updates.STAGES[0]}", script
        )

    def test_uses_the_same_image_block_then_status(self):
        script = remote_updates.image_chain()
        self.assertIn(remote_updates.IMAGE_UPGRADE, script)
        self.assertIn("/usr/libexec/gablue-update", script)
        self.assertIn("---STATUS---", script)
        self.assertIn("bootc status", script)
        self.assertIn("\nrc=0\n", script)
        self.assertIn("exit $rc", script)

    def test_chain_is_a_single_step(self):
        # Le dialogue mono-poste n'affiche pas d'étapes : un seul marqueur
        # suffit, et deux compteurs feraient reculer la barre.
        script = remote_updates.image_chain()
        self.assertEqual(script.count("::gablue:stage"), 1)


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

    def test_flatpak_stage_is_measured_from_its_output(self):
        # Aucun marqueur côté poste : la sortie de `flatpak update` suffit.
        self.state.feed("::gablue:stage 2 5 Flatpaks\n")
        kept = self.state.feed("Updating 2/4…\n")
        self.assertEqual(kept, "Updating 2/4…\n")      # reste au journal
        # « 2/4 » sans pourcentage : la phase est à (2-1)/4 = 25 %, soit 5 %
        # d'une étape sur cinq, donc 25 % de la chaîne (étape 2 : base 20 %).
        self.assertAlmostEqual(self.state.percent, 25.0)
        self.state.feed("Updating 2/4…                    50%  1,2 Mo/s\n")
        self.assertAlmostEqual(self.state.percent, 27.5)   # (1 + 0,375) / 5
        self.assertIn("Flatpaks", self.state.label())

    def test_brew_stage_is_measured_per_formula(self):
        self.state.feed("::gablue:stage 5 5 Homebrew\n")
        self.state.feed("==> Upgrading 2 outdated packages:\n")
        self.assertAlmostEqual(self.state.percent, 80.0)    # 4/5, aucune formule
        self.state.feed("==> Upgrading llhttp\n")
        self.assertAlmostEqual(self.state.percent, 90.0)
        self.state.feed("==> Upgrading osslsigncode\n")
        self.assertAlmostEqual(self.state.percent, 100.0)
        self.assertIn("osslsigncode", self.state.label())

    def test_brew_fetch_leaves_the_bar_indeterminate(self):
        # Homebrew ne publie aucun pourcentage (curl --silent hors terminal) :
        # balayage, jamais un chiffre inventé.
        self.state.feed("::gablue:stage 5 5 Homebrew\n")
        self.state.feed("==> Fetching downloads for: llhttp and osslsigncode\n")
        self.assertIsNone(self.state.percent)
        self.assertIn("formules", self.state.label())

    def test_gwine_runs_with_progress_enabled(self):
        script = remote_updates.update_chain()
        self.assertIn("GABLUE_PROGRESS=1 gwine --init", script)

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


class TrayPokeTest(unittest.TestCase):
    """Réveil de la pastille d'un poste après une mise à jour.

    Le badge est calculé dans **la session du poste**, à l'ouverture de session
    puis à sa cadence (2 h) : sans ce message, il gardait l'état d'avant la
    mise à jour jusqu'à la sonde suivante (vécu 14/11/2026, gen1 restait rouge
    après un lot pourtant réussi). Le script est testé pour de vrai : une
    socket locale joue le tray.
    """

    def _run(self, env):
        import subprocess

        return subprocess.run(
            ["bash", "-c", remote_updates.TRAY_POKE_SCRIPT],
            env=env, capture_output=True, text=True, timeout=20,
            check=False,
        )

    def test_script_sends_updates_on_the_instance_socket(self):
        import os
        import socket
        import tempfile
        import threading

        with tempfile.TemporaryDirectory() as base:
            path = os.path.join(
                base, f"gablue-manager-tray-{os.getuid()}"
            )
            server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            server.bind(path)
            server.listen(1)
            received: list[bytes] = []

            def accept() -> None:
                conn, _address = server.accept()
                conn.settimeout(5)
                received.append(conn.recv(64))
                conn.close()

            thread = threading.Thread(target=accept, daemon=True)
            thread.start()
            # `QLocalServer` met ses sockets dans le dossier temporaire de Qt
            # (`/tmp` en pratique), pas dans `$XDG_RUNTIME_DIR` : c'est ce que
            # le script doit viser en premier.
            env = dict(os.environ, TMPDIR=base)
            env.pop("XDG_RUNTIME_DIR", None)
            result = self._run(env)
            thread.join(5)
            server.close()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(received, [b"updates"])

    def test_without_a_session_it_is_not_an_error(self):
        # Un poste sans session graphique n'a personne à réveiller : le
        # script sort 0 et ne laisse rien dans le journal de la tâche.
        import os
        import tempfile

        with tempfile.TemporaryDirectory() as base:
            env = dict(os.environ, TMPDIR=base)
            env.pop("XDG_RUNTIME_DIR", None)
            result = self._run(env)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stderr.strip(), "")

    def test_script_tries_qt_temp_then_runtime_dirs(self):
        # La socket réelle est `/tmp/gablue-manager-tray-<uid>` (QLocalServer
        # utilise `QDir::tempPath()`, vérifié sur gen1 le 14/11/2026 : rien
        # dans `/run/user/1000`). Les chemins systemd restent en secours.
        script = remote_updates.TRAY_POKE_SCRIPT
        self.assertIn("tempfile.gettempdir()", script)
        self.assertIn("XDG_RUNTIME_DIR", script)
        self.assertIn("/run/user/%d", script)
        self.assertIn("gablue-manager-tray-%d", script)

    def test_script_is_valid_bash(self):
        import subprocess

        result = subprocess.run(
            ["bash", "-n"], input=remote_updates.TRAY_POKE_SCRIPT,
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


class KeepaliveTest(unittest.TestCase):
    def test_long_tasks_get_a_keepalive(self):
        opts = util._ssh_opts(
            {"port": 22, "user": "gab", "host": "192.168.1.110"}
        )
        self.assertIn("ServerAliveInterval=15", opts)
        self.assertIn("ServerAliveCountMax=4", opts)


if __name__ == "__main__":
    unittest.main()
