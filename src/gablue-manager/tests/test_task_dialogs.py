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

    def test_header_summarises_a_multi_line_script(self):
        from gablue_manager import tasks

        dialog = tasks.RemoteTaskDialog(
            None, host=HOST, title="Mise à jour",
            command="rc=0\nprintf 'a\\n'\nexit $rc\n",
            progress_mode="busy",
        )
        header = dialog.log_view.toPlainText()
        self.assertIn("<script — 3 lignes>", header)
        self.assertNotIn("printf", header)
        # Le script complet reste consultable, une seule fois.
        dialog.log_toggle.setChecked(True)
        self.assertIn("printf", dialog.log_view.toPlainText())
        dialog._toggle_log(False)
        dialog._toggle_log(True)
        self.assertEqual(
            dialog.log_view.toPlainText().count("== Script =="), 1
        )

    def test_short_command_passes_a_single_line_through(self):
        from gablue_manager import tasks

        self.assertEqual(tasks._short_command("echo hi"), "echo hi")
        self.assertEqual(
            tasks._short_command("a\nb"), "<script — 2 lignes>"
        )


class _FakeSignal:
    """Suffisant pour `connect` : les tests pilotent les signaux à la main."""

    def connect(self, *_args, **_kwargs) -> None:
        return None


class _FakeWorker:
    """`SshTaskWorker` sans ssh : on inspecte la file et les états du lot."""

    started: list = []

    def __init__(self, host, command, parent=None, password=""):
        import threading

        self.host = dict(host)
        self.command = command
        self.cancel = threading.Event()
        self.log = _FakeSignal()
        self.done = _FakeSignal()
        self.ran = False
        _FakeWorker.started.append(self)

    def start(self) -> None:
        self.ran = True

    def wait(self, _ms=None) -> bool:
        return True


@unittest.skipIf(QApplication is None, "PySide6 absent")
class RemoteTrayPokeWorkerTest(unittest.TestCase):
    """Réveil du tray d'un poste : une seule commande ssh, best-effort."""

    def test_runs_the_poke_script_over_ssh(self):
        from unittest import mock

        from gablue_manager import remote_updates, workers

        worker = workers.RemoteTrayPokeWorker(HOST)
        with mock.patch.object(
            workers, "ssh_capture", return_value=(0, "")
        ) as sent:
            worker.run()
        sent.assert_called_once()
        args, kwargs = sent.call_args
        self.assertEqual(args[0], HOST)
        self.assertEqual(args[1], remote_updates.TRAY_POKE_SCRIPT)
        self.assertGreaterEqual(kwargs.get("timeout", 0), 10)

    def test_a_dead_host_is_silent(self):
        from unittest import mock

        from gablue_manager import workers

        worker = workers.RemoteTrayPokeWorker(HOST)
        with mock.patch.object(
            workers, "ssh_capture", return_value=(255, "")
        ) as sent, mock.patch.object(
            workers.journal, "log_failure"
        ) as logged:
            worker.run()
        sent.assert_called_once()
        self.assertFalse(logged.called)


@unittest.skipIf(QApplication is None, "PySide6 absent")
class RemoteBatchDialogTest(unittest.TestCase):
    """Lot de postes : attente des sondes, relance d'un seul poste, watchdog.

    Aucun ssh : `SshTaskWorker` est remplacé par `_FakeWorker`, et les timers
    Qt ne tournent pas (pas de boucle d'événements), donc rien ne part tout
    seul en arrière-plan.
    """

    HOSTS = [
        {"id": "a", "label": "sucina", "user": "gab", "host": "sucina",
         "port": 22},
        {"id": "b", "label": "gen1", "user": "gab", "host": "gen1", "port": 22},
    ]

    @classmethod
    def setUpClass(cls):
        from gablue_manager import tasks

        cls.home = tempfile.TemporaryDirectory()
        os.environ["HOME"] = cls.home.name
        cls.app = QApplication.instance() or QApplication([])
        cls.tasks = tasks
        cls._real_worker = tasks.SshTaskWorker
        tasks.SshTaskWorker = _FakeWorker

    @classmethod
    def tearDownClass(cls):
        cls.tasks.SshTaskWorker = cls._real_worker
        cls.home.cleanup()

    def setUp(self):
        _FakeWorker.started = []

    def _dialog(self, **kwargs):
        options = {
            "hosts": self.HOSTS, "commands": {}, "await_commands": True,
        }
        options.update(kwargs)
        return self.tasks.RemoteBatchDialog(None, **options)

    def test_waits_for_each_probe_before_starting(self):
        dialog = self._dialog()
        self.assertEqual(dialog._awaiting, {0, 1})
        self.assertEqual(dialog._queue, [])
        self.assertEqual(dialog._rows[0].text(), "Préparation…")

        dialog.set_command(0, "echo 0")
        self.assertNotIn(0, dialog._awaiting)
        self.assertEqual(len(dialog._workers), 1)
        self.assertEqual(dialog._workers[0].command, "echo 0")
        self.assertEqual(dialog._rows[1].text(), "Préparation…")

        dialog.set_command(1, "echo 1")
        self.assertFalse(dialog._awaiting)

    def test_empty_chain_finishes_without_ssh(self):
        dialog = self._dialog()
        dialog.set_command(0, "")
        self.assertEqual(dialog._results[0], 0)
        self.assertEqual(dialog._rows[0].text(), "À jour")
        self.assertEqual(_FakeWorker.started, [])

    def test_a_failure_is_logged_and_retryable(self):
        from unittest import mock

        dialog = self._dialog()
        dialog.set_command(0, "echo 0")
        dialog.set_command(1, "")        # poste déjà à jour : aucun ssh
        dialog._append(0, "Échec du miroir\n")
        with mock.patch.object(
            self.tasks.journal, "log_failure"
        ) as logged:
            dialog._host_done(0, 3)
        logged.assert_called_once()
        args, kwargs = logged.call_args
        self.assertIn("sucina", args[0])
        self.assertIn("Échec du miroir", kwargs["detail"])
        self.assertEqual(kwargs["code"], 3)
        self.assertFalse(dialog._retry[0].isHidden())
        self.assertTrue(dialog._finished)

        # Relancer ce poste ne rejoue pas le lot et rouvre la fenêtre.
        dialog._retry_host(0)
        self.assertFalse(dialog._finished)
        self.assertNotIn(0, dialog._results)
        self.assertEqual(len(dialog._workers), 1)
        self.assertTrue(dialog._retry[0].isHidden())

    def test_watchdog_warns_then_abandons_one_host(self):
        import time

        dialog = self._dialog()
        dialog.set_command(0, "echo 0")
        dialog.STALL_WARN = 1
        dialog.STALL_ABORT = 1000
        now = time.monotonic()
        dialog._last_activity[0] = now - 90
        dialog._watch(0, now)
        self.assertIn("muet depuis 1 min", dialog._details[0].text())
        self.assertEqual(dialog._stalled, {})

        dialog.STALL_ABORT = 2
        dialog._last_activity[0] = now - 200
        dialog._watch(0, now)
        self.assertIn(0, dialog._stalled)
        self.assertTrue(dialog._workers[0].cancel.is_set())
        self.assertIn("abandonné", dialog._logs[0])

        # Le worker sort en code non nul : la ligne dit « Abandonné ».
        from unittest import mock

        with mock.patch.object(self.tasks.journal, "log_failure"):
            dialog._host_done(0, 143)
        self.assertEqual(dialog._rows[0].text(), "Abandonné")
        self.assertIn("Aucun progrès", dialog._details[0].text())

    def test_abort_clears_hosts_still_being_probed(self):
        dialog = self._dialog()
        dialog.abort()
        self.assertEqual(dialog._awaiting, set())
        self.assertEqual(dialog._rows[0].text(), "Annulé")
        self.assertEqual(dialog._rows[1].text(), "Annulé")
        # `set_command` refuse après annulation : plus personne n'attend.
        dialog.set_command(0, "echo 0")
        self.assertEqual(dialog._workers, {})


@unittest.skipIf(QApplication is None, "PySide6 absent")
class RebootProposalTest(unittest.TestCase):
    """Fin de mise à jour : proposer le redémarrage dans la même fenêtre."""

    @classmethod
    def setUpClass(cls):
        cls.home = tempfile.TemporaryDirectory()
        os.environ["HOME"] = cls.home.name
        cls.app = QApplication.instance() or QApplication([])

    @classmethod
    def tearDownClass(cls):
        cls.home.cleanup()

    def _sequential(self, **kwargs):
        from gablue_manager import tasks

        return tasks.SequentialTaskDialog(
            None, title="Tout mettre à jour",
            steps=[{"label": "Test", "argv": ["true"]}], **kwargs,
        )

    def _single(self, **kwargs):
        from gablue_manager import tasks

        return tasks.LocalTaskDialog(None, title="Test", argv=["true"], **kwargs)

    def test_reasons_show_the_proposal(self):
        from gablue_manager import tasks

        dialog = self._sequential(offer_reboot=True)
        self.assertTrue(dialog.reboot_row.isHidden())
        tasks._reboot_verdict(dialog, ["nouvelle image Gablue téléchargée"])
        self.assertTrue(dialog.reboot_row.isVisibleTo(dialog))
        self.assertIn("nouvelle image Gablue",
                      dialog.reboot_label.text())

    def test_no_reason_closes_a_success(self):
        from gablue_manager import tasks

        dialog = self._sequential(offer_reboot=True, close_on_success=True)
        dialog.show()
        tasks._reboot_verdict(dialog, [])
        self.app.processEvents()
        self.assertFalse(dialog.isVisible())

    def test_a_reason_keeps_the_window_open(self):
        from gablue_manager import tasks

        dialog = self._sequential(offer_reboot=True, close_on_success=True)
        dialog.show()
        tasks._reboot_verdict(dialog, ["paramètres du noyau modifiés"])
        self.app.processEvents()
        self.assertTrue(dialog.isVisible())

    def test_single_task_offers_it_too(self):
        from gablue_manager import tasks

        self.assertFalse(tasks._offer_reboot(self._single()))
        dialog = self._single(offer_reboot=True)
        tasks._reboot_verdict(dialog, ["émulation CPUID fault"])
        self.assertTrue(dialog.reboot_row.isVisibleTo(dialog))


@unittest.skipIf(QApplication is None, "PySide6 absent")
class TrayUpdateMenuTest(unittest.TestCase):
    """Sous-menu « Mettre à jour » : un domaine par action, grisé sans attente."""

    @classmethod
    def setUpClass(cls):
        cls.home = tempfile.TemporaryDirectory()
        os.environ["HOME"] = cls.home.name
        cls.app = QApplication.instance() or QApplication([])

    @classmethod
    def tearDownClass(cls):
        cls.home.cleanup()

    def _menu(self, state):
        from gablue_manager import tray

        icon = tray.Tray(self.app)
        icon.updates_state = state
        icon._build_menu()
        for action in icon.menu.actions():
            if action.text() == "Mettre à jour":
                return icon, action.menu()
        raise AssertionError("sous-menu « Mettre à jour » absent")

    def test_domains_greyed_without_state(self):
        _icon, menu = self._menu(None)
        texts = [a.text() for a in menu.actions() if not a.isSeparator()]
        self.assertEqual(texts[0], "Tout mettre à jour")
        self.assertTrue(menu.actions()[0].isEnabled())
        for action in menu.actions():
            if action.isSeparator() or action.text() == "Tout mettre à jour":
                continue
            self.assertFalse(action.isEnabled(), action.text())

    def test_counts_and_enabling(self):
        state = {"checked_at": 1.0, "domains": {
            "image": {"state": "available", "version": "20261115"},
            "flatpak": {"state": "updates", "apps": ["a", "b"]},
            "gwine": {"state": "uptodate", "components": []},
            "apps": {"state": "updates", "apps": ["ES-DE"]},
            "brew": {"state": "unknown", "formulas": []},
        }}
        _icon, menu = self._menu(state)
        by_text = {a.text(): a for a in menu.actions() if not a.isSeparator()}
        self.assertIn("Tout mettre à jour (4)", by_text)
        self.assertTrue(by_text["Image Gablue (1)"].isEnabled())
        self.assertTrue(by_text["Flatpaks (2)"].isEnabled())
        self.assertTrue(by_text["Applications (1)"].isEnabled())
        self.assertFalse(by_text["Composants gwine"].isEnabled())
        self.assertFalse(by_text["Homebrew"].isEnabled())

    def test_a_running_update_locks_the_menu(self):
        icon, _menu = self._menu({"checked_at": 1.0, "domains": {
            "flatpak": {"state": "updates", "apps": ["a"]},
        }})
        icon._job = object()
        icon._job_domain = "flatpak"
        icon._build_menu()
        menu = next(a.menu() for a in icon.menu.actions()
                    if a.text() == "Mettre à jour")
        actions = [a for a in menu.actions() if not a.isSeparator()]
        self.assertEqual(len(actions), 1)
        self.assertIn("en cours", actions[0].text())
        self.assertFalse(actions[0].isEnabled())

    def test_start_update_tracks_its_subprocess(self):
        from unittest import mock

        from gablue_manager import instance

        icon, _menu = self._menu(None)
        with mock.patch.object(
            instance, "manager_argv",
            return_value=["/usr/bin/bash", "-c", "exit 0"],
        ) as argv:
            icon.start_update("flatpak")
            self.assertTrue(argv.called)
            self.assertIsNotNone(icon._job)
            # Un second clic ne lance rien : le poste est réservé.
            running = icon._job
            icon.start_update("brew")
            self.assertIs(icon._job, running)
            self.assertTrue(icon._job.waitForFinished(5000))
        self.app.processEvents()            # livraison du signal `finished`
        self.assertIsNone(icon._job)
        self.assertEqual(icon._job_domain, "")


@unittest.skipIf(QApplication is None, "PySide6 absent")
class UpdateLockTest(unittest.TestCase):
    """`instance.update_socket` : une mise à jour à la fois sur ce poste."""

    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_update_channel_is_per_user(self):
        from gablue_manager import instance

        self.assertTrue(instance.update_socket().endswith(f"-{os.getuid()}"))
        self.assertNotEqual(instance.update_socket(), instance.tray_socket())

    def test_manager_argv_keeps_module_invocation(self):
        from gablue_manager import instance

        argv = instance.manager_argv("--update", "brew")
        self.assertEqual(argv[-2:], ["--update", "brew"])
        self.assertIn("gablue_manager", argv)

    def test_release_frees_the_channel(self):
        from gablue_manager import instance

        name = f"gablue-manager-test-lock-{os.getpid()}"
        first = instance.claim(name)
        self.assertIsNotNone(first)
        self.assertIsNone(instance.claim(name))     # déjà pris
        instance.release(first)
        again = instance.claim(name)                # rendu, donc reprenable
        self.assertIsNotNone(again)
        instance.release(again)


if __name__ == "__main__":
    unittest.main()
