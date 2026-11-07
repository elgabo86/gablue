"""Tests unitaires des fonctions pures de Gablue Manager.

Aucun affichage, aucun privilège : uniquement les calculs (scripts de mise à
jour, analyse des fichiers d'options, formats ssh). Lancer depuis la racine :

    PYTHONPATH=src/gablue-manager python3 -m unittest discover \
        -s src/gablue-manager/tests -v
"""

import ipaddress
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from gablue_manager import (  # noqa: E402
    apps, cli, hosts, journal, local, prefs, ssh, updates, util,
)


class KargsTest(unittest.TestCase):
    def test_choices(self):
        choices = local.kargs_choices(
            {"mitigations=off", "clearcpuid=514", "amdgpu.dcfeaturemask=0x402"},
            True,
        )
        self.assertFalse(choices["mitigations"])
        self.assertTrue(choices["cpuid"])
        self.assertFalse(choices["ertm"])
        self.assertTrue(choices["hdmi21"])
        self.assertTrue(choices["cpuid_emu"])

    def test_script_groups_kargs(self):
        script = local.kernel_apply_script({"mitigations": False, "ertm": True})
        self.assertEqual(
            script,
            "sudo -n /usr/libexec/gablue-kargs --mitigations off --ertm on\n",
        )

    def test_script_hdmi21_goes_through_kargs(self):
        script = local.kernel_apply_script({"hdmi21": True})
        self.assertEqual(
            script, "sudo -n /usr/libexec/gablue-kargs --hdmi21 on\n"
        )
        script = local.kernel_apply_script({"hdmi21": False, "ertm": True})
        self.assertEqual(
            script,
            "sudo -n /usr/libexec/gablue-kargs --ertm on --hdmi21 off\n",
        )

    def test_script_cpuid_alone(self):
        script = local.kernel_apply_script({"cpuid_emu": True})
        self.assertEqual(script, "sudo -n /usr/libexec/gablue-cpuid-emu on\n")

    def test_script_both(self):
        script = local.kernel_apply_script({"cpuid": True, "cpuid_emu": False})
        self.assertIn("gablue-kargs --cpuid on", script)
        self.assertIn("gablue-cpuid-emu off", script)

    def test_avahi_args(self):
        # Helper à liste fermée : aucun chemin temporaire en argument, sinon la
        # règle sudoers exacte ne matcherait pas (invite PolicyKit).
        self.assertEqual(
            local.avahi_apply_args(True),
            ["/usr/libexec/gablue-avahi", "on"],
        )
        self.assertEqual(
            local.avahi_apply_args(False),
            ["/usr/libexec/gablue-avahi", "off"],
        )


class SystemTest(unittest.TestCase):
    def test_parse_variant(self):
        status = {
            "deployments": [
                {"booted": False, "container-image-reference":
                    "ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest"},
                {"booted": True, "container-image-reference":
                    "ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main-dx:latest"},
            ]
        }
        self.assertEqual(local.parse_variant(json.dumps(status)), "main-dx")
        self.assertEqual(local.parse_variant("pas du json"), "")
        self.assertEqual(local.parse_variant(json.dumps({"deployments": []})), "")

    def test_docker_only_on_dx(self):
        self.assertTrue(local.docker_available("main-dx"))
        self.assertTrue(local.docker_available("nvidia-open-dx"))
        self.assertFalse(local.docker_available("main"))
        self.assertFalse(local.docker_available("nvidia-open"))
        self.assertFalse(local.docker_available(""))

    def test_system_script(self):
        script = local.system_apply_script({
            "displaylink": True,
            "docker": False,
            "low_latency": True,
            "broadcom_wl": False,
            "atuin": True,
        })
        self.assertEqual(
            script,
            "printf '%s\\n' DisplayLink\n"
            "printf '%s\\n' '::gablue:step 1 5 DisplayLink'\n"
            "sudo -n /usr/libexec/gablue-service displaylink.service on\n"
            "printf '%s\\n' Docker\n"
            "printf '%s\\n' '::gablue:step 2 5 Docker'\n"
            "sudo -n /usr/libexec/gablue-service docker.service off\n"
            "printf '%s\\n' 'Vulkan low-latency'\n"
            "printf '%s\\n' '::gablue:step 3 5 Vulkan low-latency'\n"
            "sudo -n /usr/libexec/gablue-low-latency on\n"
            "printf '%s\\n' 'Pilote Wi-Fi Broadcom'\n"
            "printf '%s\\n' '::gablue:step 4 5 Pilote Wi-Fi Broadcom'\n"
            "sudo -n /usr/libexec/gablue-broadcom-wl off\n"
            "printf '%s\\n' 'Historique atuin'\n"
            "printf '%s\\n' '::gablue:step 5 5 Historique atuin'\n"
            "/usr/bin/gablue-atuin on\n"
            "printf '%s\\n' ::gablue:done\n",
        )

    def test_system_script_partial(self):
        self.assertEqual(
            local.system_apply_script({"low_latency": False}),
            "printf '%s\\n' 'Vulkan low-latency'\n"
            "printf '%s\\n' '::gablue:step 1 1 Vulkan low-latency'\n"
            "sudo -n /usr/libexec/gablue-low-latency off\n"
            "printf '%s\\n' ::gablue:done\n",
        )
        # atuin ne passe jamais par sudo : fichiers de l'utilisateur.
        self.assertNotIn(
            "sudo", local.system_apply_script({"atuin": False})
        )

    def test_auto_update_schedule(self):
        self.assertEqual(local.auto_update_calendar("daily"), "*-*-* 04:00:00")
        self.assertEqual(
            local.auto_update_calendar("weekly"), "Sat *-*-* 04:00:00"
        )
        dropin = local.auto_update_schedule_dropin("daily")
        # Les déclencheurs de l'image sont remis à zéro : sans ça le timer
        # cumulerait son OnBootSec/OnUnitInactiveSec et deux calendriers.
        self.assertIn("OnBootSec=\n", dropin)
        self.assertIn("OnUnitInactiveSec=\n", dropin)
        self.assertIn("OnCalendar=\n", dropin)
        self.assertIn("OnCalendar=*-*-* 04:00:00\n", dropin)
        self.assertIn("Persistent=true", dropin)
        self.assertIn("RandomizedDelaySec=10m", dropin)

    def test_parse_auto_update_status(self):
        text = (
            "image=enabled\nflatpak=disabled\nbrew=enabled\n"
            "frequency=daily\n"
        )
        self.assertEqual(
            local.parse_auto_update_status(text),
            {"image": True, "flatpak": False, "brew": True,
             "frequency": "daily"},
        )
        # Sortie vide : tout désactivé, rythme de l'image (hebdomadaire).
        self.assertEqual(
            local.parse_auto_update_status(""),
            {"image": False, "flatpak": False, "brew": False,
             "frequency": "weekly"},
        )
        self.assertEqual(
            local.parse_auto_update_status("frequency=hourly")["frequency"],
            "weekly",
        )

    def test_auto_update_script(self):
        script = local.auto_update_script(
            {"image": True, "brew": False},
            {"image": True, "flatpak": False, "brew": False},
            "weekly",
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update image on", script
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update brew off", script
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update schedule weekly", script
        )
        # Le timer Flatpak du compte suit `desired`, sans privilège.
        self.assertIn(
            "systemctl --user disable --now flatpak-user-update.timer",
            script,
        )
        self.assertNotIn(
            "gablue-auto-update flatpak", script
        )
        # 2 domaines + rythme + timer du compte.
        self.assertIn("'::gablue:step 4 4 Flatpaks du compte'", script)

        enabled = local.auto_update_script(
            {}, {"image": False, "flatpak": True, "brew": False}, "daily"
        )
        self.assertIn(
            "systemctl --user enable --now flatpak-user-update.timer",
            enabled,
        )
        self.assertIn("OnCalendar=*-*-* 04:00:00", enabled)
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update schedule daily", enabled
        )

    def test_system_script_ignores_auto_updates(self):
        # Les mises à jour automatiques ont leur propre dialogue : la clé
        # héritée ne doit plus rien produire dans « Services et pilotes ».
        self.assertEqual(
            local.system_apply_script({"auto_updates": True}).strip(), ""
        )

    def test_brew_update_script(self):
        script = local.brew_update_script()
        # Même ordre que les deux timers de l'image, et pas de second update
        # implicite pendant l'upgrade.
        self.assertIn('"$brew" update || rc=$?', script)
        self.assertIn('"$brew" upgrade || rc=$?', script)
        self.assertIn("HOMEBREW_NO_AUTO_UPDATE=1", script)
        # Chemin absolu : le PATH d'une session GUI n'a pas brew.
        self.assertIn("/home/linuxbrew/.linuxbrew/bin/brew", script)
        # Préfixe littéral, jamais résolu : /home est un lien vers /var/home
        # sur ostree, et brew compare ses chemins à ceux de son installation.
        self.assertIn('prefix="${brew%/bin/brew}"', script)
        self.assertNotIn("pwd -P", script)
        # Aucun privilège : brew reste dans son préfixe et le cache du compte.
        self.assertNotIn("sudo", script)
        self.assertIn("exit 127", script)   # brew absent → échec explicite

    def test_gablue_update_hint(self):
        up_to_date = (
            "No changes in ostree-image-signed:docker://ghcr.io/x/gablue-main"
            ":latest => sha256:abc\n"
        )
        self.assertIn("déjà à jour", local.gablue_update_hint(up_to_date))
        self.assertNotIn("Redémarrage", local.gablue_update_hint(up_to_date))

        # Déploiement déjà en attente : rien de neuf, mais le redémarrage
        # reste nécessaire — et le message doit le dire sans parler de
        # « nouvelle image ».
        already_staged = up_to_date + "Staged update present, not changed\n"
        hint = local.gablue_update_hint(already_staged)
        self.assertIn("déjà en attente", hint)
        self.assertNotIn("Redémarrage nécessaire", hint)

        new_image = "Queued for next boot: ostree-image-signed:docker://…\n"
        self.assertIn("Redémarrage nécessaire", local.gablue_update_hint(new_image))
        self.assertEqual(local.gablue_update_hint(""), "")

    def test_reboot_pending_helpers(self):
        # BOOT_IMAGE n'existe que dans /proc/cmdline : il ne doit pas faire
        # croire à un changement de kargs.
        self.assertFalse(local.kargs_pending_change(
            {"quiet", "BOOT_IMAGE=/vmlinuz"}, {"quiet"},
        ))
        self.assertTrue(local.kargs_pending_change(
            {"quiet", "mitigations=off"}, {"quiet"},
        ))
        # Déploiement staged = image téléchargée en attente de redémarrage.
        self.assertTrue(local.parse_staged_deployment(json.dumps(
            {"deployments": [{"staged": False}, {"staged": True}]}
        )))
        self.assertFalse(local.parse_staged_deployment(json.dumps(
            {"deployments": [{"staged": False}]}
        )))
        self.assertFalse(local.parse_staged_deployment("pas du json"))
        # Fichier écrit après le démarrage = bascule à appliquer ; une config
        # livrée par l'image (date antérieure) ne compte pas.
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "conf"
            path.write_text("x\n", encoding="utf-8")
            self.assertTrue(local.written_since(path, 1.0))
            self.assertFalse(local.written_since(path, time.time() + 60))
            self.assertFalse(local.written_since(Path(tmp) / "absent", 1.0))

    def test_progress_event(self):
        info = local.parse_progress_event(json.dumps({
            "type": "ProgressBytes", "task": "pulling",
            "description": "Pulling layers", "id": "x",
            "bytesCached": 0, "bytes": 11, "bytesTotal": 42,
            "stepsCached": 0, "steps": 0, "stepsTotal": 3, "subtasks": [],
        }))
        self.assertAlmostEqual(info["percent"], 26.19, places=1)
        self.assertEqual(info["label"], "Téléchargement de l'image")
        self.assertEqual(info["bytes_total"], 42)

        steps = local.parse_progress_event(json.dumps({
            "type": "ProgressSteps", "task": "importing",
            "description": "Layers", "id": "x",
            "stepsCached": 1, "steps": 1, "stepsTotal": 4, "subtasks": [],
        }))
        self.assertEqual(steps["percent"], 50.0)

        # Total inconnu → barre indéterminée.
        unknown = local.parse_progress_event(json.dumps({
            "type": "ProgressBytes", "task": "pulling",
            "bytes": 5, "bytesTotal": 0,
        }))
        self.assertIsNone(unknown["percent"])

        # Start, JSON tronqué, ligne vide : rien à afficher.
        self.assertEqual(local.parse_progress_event(
            '{"type":"Start","version":"0.1.0"}'), {})
        self.assertEqual(local.parse_progress_event("pas du json"), {})
        self.assertEqual(local.parse_progress_event(""), {})

    def test_flatpak_progress(self):
        # Ouverture d'opération : « Updating 3/7… » (le 3e sur 7).
        opening = local.parse_flatpak_progress("Updating 3/7\u2026")
        self.assertEqual(opening["action"], "Updating")
        self.assertEqual(opening["label"], "Mise à jour des Flatpaks 3/7")
        self.assertEqual((opening["op"], opening["total"]), (3, 7))
        self.assertAlmostEqual(opening["percent"], 100 * 2 / 7, places=2)

        # Même ligne réémise avec le pourcentage et le débit (barre en espaces
        # hors terminal, taille localisée).
        line = "Updating 3/7\u2026" + " " * 20 + " 45% 1,2 Mo/s"
        info = local.parse_flatpak_progress(line)
        self.assertAlmostEqual(info["percent"], 100 * 2.45 / 7, places=2)

        # Une seule opération : pas de compteur dans la ligne.
        alone = local.parse_flatpak_progress("Updating\u2026")
        self.assertEqual((alone["op"], alone["total"]), (1, 1))
        self.assertEqual(alone["percent"], 0.0)
        self.assertEqual(
            local.parse_flatpak_progress("Updating\u2026   67%")["percent"],
            67.0,
        )

        # Install / uninstall et bornage du pourcentage.
        self.assertEqual(
            local.parse_flatpak_progress("Installing 2/5\u2026 10%")["percent"],
            100 * 1.1 / 5,
        )
        self.assertEqual(
            local.parse_flatpak_progress("Updating 1/1\u2026 120%")["percent"],
            100.0,
        )

        # Tout le reste va au journal, sans toucher la barre.
        for text in (
            "Looking for updates\u2026",
            "Nothing to do.",
            # Ne pas confondre avec l'ouverture d'opération : pas d'ellipse
            # juste après le verbe.
            "Updating to rebased version",
            "",
        ):
            self.assertEqual(local.parse_flatpak_progress(text), {}, text)

    def test_flatpak_update_hint(self):
        # Deux phases (compte puis système) : « Nothing to do. » de la seconde
        # ne doit pas faire croire que rien n'a été mis à jour.
        self.assertEqual(
            local.flatpak_update_hint(
                "Looking for updates\u2026\nNothing to do.\n"
            ),
            "Aucune mise à jour disponible.",
        )
        self.assertEqual(
            local.flatpak_update_hint(
                "Updating 1/2\u2026\nNothing to do.\n"
            ),
            "",
        )
        self.assertEqual(local.flatpak_update_hint(""), "")

    def test_brew_progress(self):
        parse = local.brew_progress_parser()

        # Tant que le résumé n'est pas là, rien à afficher (l'étiquette
        # initiale « Recherche de mises à jour… » couvre le brew update).
        self.assertEqual(parse("==> Updating Homebrew..."), {})
        self.assertEqual(parse("Already up-to-date."), {})

        # Téléchargements en cours : la liste de l'en-tête donne déjà le
        # nombre de formules, mais aucun pourcentage → barre indéterminée.
        fetch = parse("==> Fetching downloads for: htop, wget")
        self.assertIsNone(fetch["percent"])
        self.assertEqual(fetch["label"], "Téléchargement de 2 formules")
        self.assertEqual(
            parse("==> Fetching downloads for: a, b and c")["label"],
            "Téléchargement de 3 formules",
        )
        self.assertEqual(
            parse("==> Fetching downloads for: seul")["label"],
            "Téléchargement de la formule…",
        )

        total = parse("==> Upgrading 3 outdated packages:")
        self.assertEqual(total["percent"], 0.0)
        self.assertEqual(total["label"], "Mise à jour de 3 formules")

        step = parse("==> Upgrading htop")
        self.assertAlmostEqual(step["percent"], 100 / 3, places=2)
        self.assertEqual(step["label"], "Mise à jour de htop (1/3)")

        # Une dépendance n'est pas dans le total : elle ne doit pas avancer la
        # barre (Homebrew écrit « Upgrading <formule> dependency: <dep> … »).
        self.assertEqual(
            parse("==> Upgrading wget dependency: openssl@3 (3.0 -> 3.1)"), {}
        )
        self.assertAlmostEqual(
            parse("==> Upgrading wget")["percent"], 200 / 3, places=2
        )
        self.assertEqual(parse("==> Pouring htop--3.3.0.bottle.tar.gz"), {})
        self.assertEqual(
            parse("🍺  /home/linuxbrew/Cellar/htop/3.3.0: 20 files"), {}
        )

        # Une seule formule : libellé sans compteur, 100 %.
        solo = local.brew_progress_parser()
        solo("==> Upgrading 1 outdated package:")
        info = solo("==> Upgrading nasm")
        self.assertEqual(info["label"], "Mise à jour de nasm")
        self.assertEqual(info["percent"], 100.0)

        # Dépendants hors total : on borne à 100 % au lieu de dépasser.
        self.assertEqual(parse("==> Upgrading extra")["percent"], 100.0)

    def test_brew_update_hint(self):
        # Rien en retard : brew upgrade n'écrit rien, brew update peut se
        # contenter de « Already up-to-date. ».
        self.assertEqual(
            local.brew_update_hint(
                "==> Updating Homebrew...\nAlready up-to-date.\n"
            ),
            "Aucune formule à mettre à jour.",
        )
        self.assertEqual(
            local.brew_update_hint("==> Upgrading 2 outdated packages:\n"), ""
        )
        self.assertEqual(local.brew_update_hint("==> Upgrading foo\n"), "")
        self.assertEqual(local.brew_update_hint(""), "Aucune formule à mettre à jour.")

    def test_rebase_script(self):
        self.assertEqual(
            local.rebase_script("nvidia-open-dx"),
            "sudo -n /usr/libexec/gablue-rebase nvidia-open-dx\n",
        )

    def test_atuin_active_reads_bashrc(self):
        with tempfile.TemporaryDirectory() as home:
            bashrc = Path(home) / ".bashrc"
            previous = os.environ.get("HOME")
            os.environ["HOME"] = home
            try:
                self.assertFalse(local.atuin_active())      # fichier absent
                bashrc.write_text("export PATH=~/bin:$PATH\n", encoding="utf-8")
                self.assertFalse(local.atuin_active())
                bashrc.write_text(
                    'eval "$(atuin init bash --disable-up-arrow)"\n',
                    encoding="utf-8",
                )
                self.assertTrue(local.atuin_active())
            finally:
                if previous is None:
                    os.environ.pop("HOME", None)
                else:
                    os.environ["HOME"] = previous

    def test_gamepad_script(self):
        script = local.system_apply_script({"gamepad": False})
        self.assertIn("/usr/bin/gablue-gamepad off", script)
        # Autostart de session : fichiers de l'utilisateur, jamais de sudo.
        self.assertNotIn("sudo", script)

    def test_clean_script_all_domains(self):
        import tempfile
        import unittest.mock as mock
        cache = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(cache, True))
        with mock.patch.object(
            local, "_which", lambda name: f"/usr/bin/{name}"
        ), mock.patch.object(
            local, "brew_available", lambda: True
        ), mock.patch.object(
            local, "clean_available", lambda: True
        ), mock.patch.object(
            local, "clean_cache_path", lambda _domain: cache
        ):
            script = local.clean_system_script()
        # Podman : le geste complet demandé (volumes compris).
        self.assertIn("podman system prune -a --volumes -f", script)
        self.assertIn("flatpak uninstall --unused --user -y", script)
        self.assertIn("sudo -n /usr/libexec/gablue-clean flatpak", script)
        self.assertIn("sudo -n /usr/libexec/gablue-clean rpm-ostree", script)
        self.assertIn('"$brew" autoremove', script)
        self.assertIn('"$brew" cleanup', script)
        # Six caches régénérables : un rm -rf par domaine.
        self.assertIn(f"rm -rf -- {cache}", script)
        self.assertEqual(script.count("rm -rf --"), 6)
        self.assertIn("'::gablue:step 10 10 Homebrew'", script)

    def test_clean_script_selection(self):
        import unittest.mock as mock
        with mock.patch.object(
            local, "_which", lambda name: f"/usr/bin/{name}"
        ), mock.patch.object(
            local, "brew_available", lambda: True
        ), mock.patch.object(
            local, "clean_available", lambda: True
        ):
            script = local.clean_system_script(["podman"])
        self.assertIn("podman system prune", script)
        self.assertNotIn("flatpak", script)
        self.assertNotIn("rpm-ostree", script)
        # Un domaine absent de la sélection ne compte pas dans le total.
        self.assertIn("'::gablue:step 1 1 Podman'", script)

    def test_btrfs_mounts(self):
        text = json.dumps({"filesystems": [
            {"target": "/etc", "uuid": "u", "label": "l"},
            {"target": "/sysroot", "uuid": "u", "label": "l",
             "children": [{"target": "/sysroot/ostree/var", "uuid": "u"}]},
            {"target": "/run/bees/home", "uuid": "u", "label": "l"},
            {"target": "/var", "uuid": "u", "label": "l",
             "children": [{"target": "/var/home", "uuid": "u", "label": "l"}]},
            {"target": "/var", "uuid": "u", "label": "l"},
        ]})
        rows = local.parse_btrfs_mounts(text)
        # Un sous-volume n'est pas fusionné avec son voisin : /var et /var/home
        # portent chacun leur drapeau.
        self.assertEqual(
            [row["target"] for row in rows], ["/var", "/var/home"]
        )
        self.assertEqual(local.parse_btrfs_mounts("pas du json"), [])

    def test_btrfs_script(self):
        script = local.btrfs_apply_script({"/var": True, "/var/home": False})
        self.assertIn(
            "sudo -n /usr/libexec/gablue-btrfs-compress zstd /var", script
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-btrfs-compress none /var/home",
            script,
        )
        self.assertIn("'::gablue:step 2 2", script)

    def test_grub_script(self):
        self.assertIn(
            "sudo -n /usr/libexec/gablue-grub set 2",
            local.grub_apply_script("2"),
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-grub unset",
            local.grub_apply_script("unset"),
        )

    def test_secureboot(self):
        script = local.secureboot_enroll_script()
        self.assertIn("sudo -n /usr/libexec/gablue-secureboot enroll", script)
        # Une seule étape : l'effet est au reboot, pas ici.
        self.assertIn("'::gablue:step 1 1 Secure Boot'", script)

        status = local.parse_secureboot_status(json.dumps({
            "efi": True,
            "secure_boot": "enabled",
            "password": "gablue",
            "keys": [
                {"path": "/a.der", "label": "A", "state": "enrolled"},
                {"path": "/b.der", "label": "B", "state": "not-enrolled"},
                # Entrées incomplètes : ignorées, pas d'exception.
                {"path": "", "label": "C", "state": "enrolled"},
                {"path": "/c.der", "label": "C", "state": "bizarre"},
            ],
        }))
        self.assertEqual(status["secure_boot"], "enabled")
        self.assertEqual(status["password"], "gablue")
        self.assertEqual(
            [(key["path"], key["state"]) for key in status["keys"]],
            [("/a.der", "enrolled"), ("/b.der", "not-enrolled")],
        )
        self.assertEqual(local.parse_secureboot_status("pas du json"), {})
        self.assertEqual(
            local.parse_secureboot_status('{"secure_boot": "enabled"}'), {}
        )
        self.assertEqual(
            local.parse_secureboot_status(
                '{"secure_boot": "peut-être", "keys": '
                '[{"path": "/a.der", "state": "enrolled"}]}'
            ),
            {},
        )
        self.assertEqual(
            local.secure_boot_label("enabled"), "Secure Boot activé"
        )
        self.assertEqual(
            local.secure_boot_label("disabled"), "Secure Boot désactivé"
        )
        self.assertEqual(local.secure_boot_label("unknown"), "")

    def test_secureboot_helper_key_states(self):
        """`mokutil --test-key` sort 0 quand la clé N'EST PAS enrôlée."""
        import importlib.machinery
        import importlib.util

        path = (ROOT.parents[1] / "files/system/all/usr/libexec/"
                "gablue-secureboot")
        # Le helper n'a pas d'extension `.py` : loader explicite.
        loader = importlib.machinery.SourceFileLoader(
            "gablue_secureboot_helper", str(path)
        )
        spec = importlib.util.spec_from_loader(loader.name, loader)
        module = importlib.util.module_from_spec(spec)
        loader.exec_module(module)
        self.assertEqual(
            module.parse_test_key(0, "k.der is not enrolled"), "not-enrolled"
        )
        self.assertEqual(
            module.parse_test_key(1, "k.der is already enrolled"), "enrolled"
        )
        self.assertEqual(
            module.parse_test_key(
                1, "k.der is already in the enrollment request"
            ),
            "pending",
        )
        self.assertEqual(
            module.parse_test_key(1, "k.der is blocked in dbx"), "blocked"
        )
        self.assertEqual(module.parse_test_key(127, ""), "unknown")
        self.assertEqual(module.key_state("/inexistant.der"), "missing")

    def test_format_size(self):
        self.assertEqual(local.format_size(None), "")
        self.assertEqual(local.format_size(0), "")
        self.assertEqual(local.format_size(900), "900 o")
        self.assertEqual(local.format_size(1536), "2 Kio")
        self.assertEqual(local.format_size(2 * 1024 ** 3), "2.0 Gio")

    def test_podman_reclaimable(self):
        text = json.dumps([
            {"Type": "Images", "RawReclaimable": 100},
            {"Type": "Containers", "RawReclaimable": 25},
            {"Type": "Local Volumes", "RawReclaimable": 0},
        ])
        self.assertEqual(local.parse_podman_reclaimable(text), 125)
        self.assertIsNone(local.parse_podman_reclaimable("pas du json"))

    def test_syncthing_versions(self):
        import tempfile
        home = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(home, True))
        config_dir = home / ".local/state/syncthing"
        config_dir.mkdir(parents=True)
        synced = home / "Sync/docs"
        (synced / ".stversions").mkdir(parents=True)
        custom = home / "CustomVersions"
        custom.mkdir()
        (config_dir / "config.xml").write_text(
            "<configuration>"
            f'<folder id="a" label="Docs" path="{synced}">'
            '<versioning type="trashcan"/></folder>'
            f'<folder id="b" label="Autre" path="{home}/Other">'
            f"<versionsPath>{custom}</versionsPath></folder>"
            "</configuration>",
            encoding="utf-8",
        )
        dirs = local.syncthing_version_dirs(home)
        self.assertEqual(
            [(item["label"], item["versions"]) for item in dirs],
            [("Docs", str(synced / ".stversions")),
             ("Autre", str(custom))],
        )
        # Chemin par défaut : .stversions DANS le dossier synchronisé.
        parsed = local.parse_syncthing_folders(
            f'<configuration><folder label="X" path="{home}/X"/></configuration>',
            home,
        )
        self.assertEqual(
            parsed[0]["versions"], str(home / "X/.stversions")
        )
        # `~` du config.xml se développe avec le home fourni.
        parsed = local.parse_syncthing_folders(
            '<configuration><folder label="Y" path="~/Sync/Y"/></configuration>',
            home,
        )
        self.assertEqual(
            parsed[0]["versions"], str(home / "Sync/Y/.stversions")
        )

    def test_nvme_script(self):
        script = local.nvme_thermal_script({
            "/dev/nvme0": (70, 75),
            "/dev/nvme1": None,
        })
        self.assertIn(
            "sudo -n /usr/libexec/gablue-nvme-thermal set /dev/nvme0 70 75",
            script,
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-nvme-thermal off /dev/nvme1", script
        )
        self.assertIn("'::gablue:step 2 2", script)


class TailscaleTest(unittest.TestCase):
    def test_parse(self):
        state = local.parse_tailscale_state(
            "enabled=enabled\nactive=inactive\n"
            "server=https://head.example\ninstalled=yes\n"
        )
        self.assertEqual(state["server"], "https://head.example")
        self.assertTrue(state["installed"])
        self.assertTrue(state["enabled"])
        self.assertFalse(state["active"])

    def test_valid_server(self):
        self.assertTrue(local.valid_tailscale_server("https://head.example"))
        self.assertTrue(local.valid_tailscale_server("http://10.0.0.1:8080"))
        self.assertFalse(local.valid_tailscale_server("ftp://head.example"))
        self.assertFalse(local.valid_tailscale_server("https://a b"))
        self.assertFalse(local.valid_tailscale_server(""))

    def test_apply_script(self):
        self.assertEqual(
            local.tailscale_apply_script(True, "https://head.example"),
            "sudo -n /usr/libexec/gablue-tailscale on --login-server "
            "https://head.example\n",
        )
        self.assertEqual(
            local.tailscale_apply_script(False),
            "sudo -n /usr/libexec/gablue-tailscale off\n",
        )

    def test_find_auth_url(self):
        output = (
            "Connexion au serveur https://head.example…\n"
            "Une page de validation peut s'ouvrir dans le navigateur.\n"
            "To authenticate, visit:\n\n"
            "\thttps://head.example/register/abc123\n\n"
            "Success.\n"
        )
        # Le serveur de connexion est écarté, le lien d'enregistrement (le
        # dernier, quel que soit le chemin du serveur) est retenu.
        self.assertEqual(
            local.find_auth_url(output, ("https://head.example",)),
            "https://head.example/register/abc123",
        )
        self.assertEqual(
            local.find_auth_url("https://login.tailscale.com/a/xyz."),
            "https://login.tailscale.com/a/xyz",
        )
        # Seul le serveur est connu pour l'instant : rien à copier.
        self.assertEqual(
            local.find_auth_url(
                "Connexion au serveur https://head.example…\n",
                ("https://head.example",),
            ),
            "",
        )
        self.assertEqual(local.find_auth_url(""), "")

    def test_registered(self):
        self.assertTrue(local.tailscale_registered(
            '{"BackendState": "Running", "Self": {}}'
        ))
        self.assertFalse(local.tailscale_registered(
            '{"BackendState": "NeedsLogin"}'
        ))
        # Avertissement avant le JSON : on part de la première accolade.
        self.assertTrue(local.tailscale_registered(
            'warning: something\n{"BackendState": "Running"}'
        ))
        self.assertFalse(local.tailscale_registered("pas du json"))
        self.assertFalse(local.tailscale_registered(""))


class PrefsTest(unittest.TestCase):
    def test_defaults_and_roundtrip(self):
        import unittest.mock as mock
        with tempfile.TemporaryDirectory() as home:
            config = Path(home) / "gablue"
            with mock.patch.object(prefs, "CONFIG_DIR", config), \
                    mock.patch.object(prefs, "PREFS_FILE",
                                      config / "preferences.json"):
                # Absent : barre d'état et vérifications actives par défaut.
                self.assertTrue(prefs.tray_enabled())
                self.assertTrue(prefs.checks_enabled())
                self.assertEqual(
                    prefs.check_interval_seconds(), 360 * 60
                )
                prefs.set_tray_enabled(False)
                self.assertFalse(prefs.tray_enabled())
                stored = json.loads(
                    prefs.PREFS_FILE.read_text(encoding="utf-8")
                )
                self.assertFalse(stored["tray"])
                # Toutes les clés connues sont écrites, aux défauts près.
                self.assertEqual(
                    set(stored), set(prefs.DEFAULTS)
                )
                self.assertEqual(
                    prefs.PREFS_FILE.stat().st_mode & 0o777, 0o600
                )
                # Fichier illisible : retomber sur le défaut, jamais planter.
                prefs.PREFS_FILE.write_text("{pas du json", encoding="utf-8")
                self.assertTrue(prefs.tray_enabled())
                prefs.set_tray_enabled(True)
                self.assertTrue(prefs.tray_enabled())
                # Clé inconnue, valeur non booléenne ou intervalle aberrant :
                # ignorés ou ramenés dans les bornes.
                prefs.PREFS_FILE.write_text(
                    '{"tray": "oui", "autre": 1, "check_interval": 999999}',
                    encoding="utf-8",
                )
                values = prefs.load_prefs()
                self.assertTrue(values["tray"])
                self.assertEqual(
                    values["check_interval"], prefs.INTERVAL_RANGE[1]
                )
                self.assertNotIn("autre", values)

    def test_check_domains(self):
        values = dict(prefs.DEFAULTS)
        values["check_brew"] = False
        self.assertEqual(
            prefs.check_domains(values),
            {"image": True, "flatpak": True, "brew": False, "gwine": True},
        )
        # L'interrupteur général coupe la surveillance automatique, sans
        # effacer les domaines choisis (le bouton « Vérifier » les réutilise).
        values["check_auto"] = False
        self.assertEqual(
            prefs.auto_check_domains(values),
            {"image": False, "flatpak": False, "brew": False, "gwine": False},
        )
        self.assertFalse(prefs.checks_enabled(values))
        self.assertTrue(prefs.check_domains(values)["image"])
        self.assertEqual(
            prefs.check_interval_seconds({**values, "check_interval": 2}),
            5 * 60,
        )


class UpdatesTest(unittest.TestCase):
    def test_deployment_age(self):
        deployments = {
            "deployments": [
                {"booted": False, "staged": True, "version": "20261107",
                 "timestamp": 0},
                {"booted": True, "staged": False, "version": "20261106",
                 "timestamp": 1_800_000_000},
            ]
        }
        info = updates.parse_deployment_age(
            json.dumps(deployments), now=1_800_000_000 + 3 * 86400
        )
        self.assertEqual(info["days"], 3)
        self.assertEqual(info["version"], "20261106")
        self.assertTrue(info["staged"])
        self.assertEqual(info["staged_version"], "20261107")
        self.assertEqual(updates.parse_deployment_age("pas du json"), {})
        self.assertEqual(updates.parse_deployment_age("{}"), {})

    def test_bootc_check(self):
        self.assertEqual(
            updates.parse_bootc_check("Update available for: img", 0),
            "available",
        )
        self.assertEqual(
            updates.parse_bootc_check("No update available.", 0), "uptodate"
        )
        self.assertEqual(
            updates.parse_bootc_check(
                "No changes in image => sha256:abc", 0
            ),
            "uptodate",
        )
        self.assertEqual(updates.parse_bootc_check("erreur réseau", 1),
                         "unknown")

    def test_gwine_check(self):
        text = (
            "component|runner|1.2.3|1.3.0\n"
            "component|cache||\n"
            "gwine-check: updates\n"
        )
        result = updates.parse_gwine_check(text, 10)
        self.assertEqual(result["state"], "updates")
        self.assertEqual(result["components"][0]["key"], "runner")
        self.assertEqual(result["components"][0]["latest"], "1.3.0")
        self.assertEqual(result["components"][1]["key"], "cache")
        self.assertEqual(
            updates.parse_gwine_check("gwine-check: uptodate", 0)["state"],
            "uptodate",
        )
        self.assertEqual(
            updates.parse_gwine_check("gwine-check: unknown", 3)["state"],
            "unknown",
        )

    def test_flatpak_and_brew_lists(self):
        self.assertEqual(
            updates.parse_flatpak_updates(
                "org.kde.krita\n\norg.gimp.GIMP\norg.kde.krita\n"
            ),
            ["org.gimp.GIMP", "org.kde.krita"],
        )
        self.assertEqual(
            updates.parse_brew_outdated("==> Outdated\nhello\nwget\n"),
            ["hello", "wget"],
        )

    def test_summary(self):
        state = {
            "checked_at": 1.0,
            "online": True,
            "domains": {
                "image": {"state": "available", "days": 12,
                          "version": "20261107", "staged": False},
                "flatpak": {"state": "updates",
                            "apps": ["org.kde.krita", "org.gimp.GIMP"]},
                "brew": {"state": "uptodate", "formulas": []},
                "gwine": {"state": "updates", "components": [
                    {"key": "runner", "current": "1.2.3", "latest": "1.3.0"},
                ]},
            },
        }
        self.assertEqual(updates.total_updates(state), 4)
        self.assertTrue(updates.has_updates(state))
        text = "\n".join(updates.summary_lines(state))
        self.assertIn("Image Gablue : mise à jour disponible", text)
        self.assertIn("il y a 12 jours", text)
        self.assertIn("Homebrew : à jour", text)
        self.assertIn("Runner gwine 1.2.3 → 1.3.0", text)
        self.assertIn("4 mises à jour", updates.compact_summary(state))
        # Tous les domaines sautés : le dire, ne pas annoncer « à jour ».
        skipped = {"online": True, "domains": {
            domain: {"state": "skipped"} for domain in updates.DOMAINS
        }}
        self.assertEqual(updates.compact_summary(skipped),
                         "Vérifications de mises à jour désactivées")
        self.assertEqual(
            updates.compact_summary({"online": False, "domains": {}}),
            "Hors ligne — vérification impossible",
        )
        # Hors ligne mais image déjà téléchargée : le redémarrage reste à
        # faire, on ne le cache pas derrière « hors ligne ».
        offline_staged = {"online": False, "domains": {
            "image": {"state": "available", "staged": True, "days": 2},
            "flatpak": {"state": "offline"},
            "brew": {"state": "offline"},
            "gwine": {"state": "offline"},
        }}
        self.assertIn("1 mise à jour", updates.compact_summary(offline_staged))


class JournalTest(unittest.TestCase):
    def setUp(self):
        import unittest.mock as mock
        self.mock = mock
        patcher = mock.patch.dict(journal._seen, clear=True)
        patcher.start()
        self.addCleanup(patcher.stop)
        journal._sent = 0

    def test_encode(self):
        self.assertEqual(
            journal.encode({"MESSAGE": "a\nb", "PRIORITY": "3"}),
            b"MESSAGE=a\\nb\nPRIORITY=3\n",
        )
        clipped = journal.encode(
            {"MESSAGE": "x" * (journal.MAX_VALUE + 10)}
        ).decode()
        self.assertTrue(clipped.rstrip("\n").endswith("[… tronqué]"))
        self.assertLess(len(clipped), journal.MAX_VALUE + 100)

    def test_failure_fields(self):
        sent = []
        with self.mock.patch.object(
            journal, "_send", lambda data: sent.append(data) or True
        ):
            self.assertTrue(
                journal.log_failure("Échec de la tâche", "détail\nsuite", code=7)
            )
        self.assertEqual(len(sent), 1)
        text = sent[0].decode()
        self.assertIn("SYSLOG_IDENTIFIER=gablue-manager", text)
        self.assertIn("PRIORITY=3", text)
        self.assertIn("GABLUE_CODE=7", text)
        self.assertIn("Échec de la tâche — échec (code 7)", text)
        self.assertIn("détail\\nsuite", text)

    def test_dedup_and_cap(self):
        sent = []
        with self.mock.patch.object(
            journal, "_send", lambda data: sent.append(data) or True
        ):
            # Même erreur (même première ligne, même priorité) : une seule
            # entrée par fenêtre de 10 minutes.
            self.assertTrue(journal.log_failure("Échec A", code=1))
            self.assertFalse(journal.log_failure("Échec A", code=1))
            self.assertTrue(journal.log_failure("Échec B", code=1))
            # Plafond : la dernière entrée autorisée est suivie d'un avis.
            journal._sent = journal.MAX_ENTRIES - 1
            self.assertTrue(journal.log_failure("Échec C", code=1))
            self.assertFalse(journal.log_failure("Échec D", code=1))
        self.assertEqual(len(sent), 4)    # A, B, C + avis de coupure
        self.assertIn(b"Limite de", sent[-1])

    def test_send_never_raises(self):
        with self.mock.patch.object(
            journal, "NATIVE_SOCKET", "/nonexistant",
        ), self.mock.patch.object(
            journal, "SYSLOG_SOCKET", "/nonexistant",
        ):
            self.assertFalse(journal.log("test"))


class CliTest(unittest.TestCase):
    def test_progress(self):
        opts = cli.parse_args([
            "--progress", "--title", "Initialisation de gwine",
            "--subtitle", "test", "--auto-close-success", "--",
            "gwine", "--init", "--offline",
        ])
        self.assertEqual(opts["mode"], "progress")
        self.assertEqual(opts["title"], "Initialisation de gwine")
        self.assertTrue(opts["auto_close_success"])
        # Les options de la commande après « -- » ne sont pas à nous.
        self.assertEqual(opts["command"], ["gwine", "--init", "--offline"])

    def test_confirm_choose(self):
        opts = cli.parse_args([
            "--confirm", "--title", "T", "--text", "Question ?",
            "--yes", "Migrer", "--no", "Annuler",
        ])
        self.assertEqual((opts["yes"], opts["no"]), ("Migrer", "Annuler"))
        opts = cli.parse_args([
            "--choose", "--title", "T", "--text", "Choix",
            "--item", "a.exe", "--item", "b.exe\tB (64-bit)",
        ])
        self.assertEqual(opts["items"], ["a.exe", "b.exe\tB (64-bit)"])

    def test_erreurs(self):
        with self.assertRaises(cli.UsageError):
            cli.parse_args(["--inconnu"])
        with self.assertRaises(cli.UsageError):
            cli.parse_args(["--progress", "--title"])   # valeur manquante
        with self.assertRaises(cli.UsageError):
            cli.parse_args(["--progress", "--title", "T", "gwine", "--init"])  # sans --


class GwineTest(unittest.TestCase):
    def test_gablue_progress_protocol(self):
        # Étape n/total : n-1 étapes derrière, la barre n'atteint pas 100 %.
        info = local.parse_gablue_progress(
            "::gablue:step 4 12 Installation de Wine Mono et Gecko"
        )
        self.assertTrue(info["hide"])
        self.assertEqual(info["label"], "Étape 4/12 — Installation de Wine Mono et Gecko")
        self.assertAlmostEqual(info["percent"], 25.0)

        first = local.parse_gablue_progress("::gablue:step 1 4 Préparation")
        self.assertEqual(first["percent"], 0.0)

        # Total inconnu : barre indéterminée (balayage de gauche à droite).
        busy = local.parse_gablue_progress(
            "::gablue:busy Téléchargement des composants Windows"
        )
        self.assertIsNone(busy["percent"])
        self.assertTrue(busy["hide"])

        done = local.parse_gablue_progress("::gablue:done Préfixe Wine initialisé.")
        self.assertEqual(done["percent"], 100.0)
        self.assertEqual(done["label"], "Préfixe Wine initialisé.")

        # Lignes ordinaires : journal, rien à consommer.
        for line in ("", "Installation de Mono", "Error: ::gablue:step 1 2 x"):
            if line.startswith("::gablue:step"):
                continue
            self.assertEqual(local.parse_gablue_progress(line), {})
        # Ligne malformée : ignorée, pas d'exception.
        self.assertEqual(local.parse_gablue_progress("::gablue:step 2"), {})
        self.assertEqual(
            local.parse_gablue_progress("::gablue:step a b libellé"), {}
        )

    def test_options(self):
        state = local.parse_gwine_options(
            "dxvk_mode=dxvk-async\ndisplay_mode=wayland\nxbox_default=on\n"
        )
        self.assertEqual(
            state, {"dxvk_async": True, "wayland": True, "xbox": True}
        )

    def test_defaults(self):
        state = local.parse_gwine_options("")
        self.assertEqual(
            state, {"dxvk_async": False, "wayland": False, "xbox": False}
        )

    def test_booster(self):
        self.assertTrue(local.parse_foreground_booster(""))
        self.assertTrue(
            local.parse_foreground_booster(
                "[Foreground Booster]\nautostart=true\n"
            )
        )
        self.assertFalse(
            local.parse_foreground_booster(
                "[Foreground Booster]\nautostart=false\n"
            )
        )


class AppsTest(unittest.TestCase):
    by_key = {app.key: app for app in apps.APPS}

    def test_find_appimage(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            (directory / "eden.appimage").touch()
            (directory / "esde.appimage_3.4.0.OLD").touch()
            (directory / "xenia_edge.appimage").touch()
            self.assertEqual(
                apps.find_appimage(self.by_key["eden"], directory).name,
                "eden.appimage",
            )
            self.assertIsNone(
                apps.find_appimage(self.by_key["azahar"], directory)
            )
            self.assertTrue(
                apps.state(
                    self.by_key["xenia"], {str(directory / "xenia_edge.appimage"):
                                            "1.2"}, directory,
                )["installed"]
            )

    def test_install_argv(self):
        self.assertEqual(
            apps.install_argv(self.by_key["esde"])[-1], "--yes"
        )
        self.assertNotIn("--yes", apps.install_argv(self.by_key["eden"]))
        self.assertTrue(
            apps.install_argv(self.by_key["eden"])[0].endswith(
                "eden-install"
            )
        )

    def test_config_argv(self):
        argv = apps.config_argv()
        self.assertTrue(argv[0].endswith("esde-install"))
        self.assertEqual(argv[1], "--config-only")

    def test_remove_script_keeps_data(self):
        script = apps.remove_script(self.by_key["eden"])
        self.assertIn(
            f'"$HOME/AppImages"/{self.by_key["eden"].pattern}', script
        )
        self.assertIn("gearlever --remove", script)
        self.assertNotIn("$HOME/ES-DE", script)     # configuration conservée
        self.assertNotIn("Roms", script)

    def test_opencode_remove_uses_script(self):
        argv = apps.remove_argv(self.by_key["opencode"])
        self.assertTrue(argv[0].endswith("opencode2-remove"))
        self.assertEqual(len(argv), 1)

    def test_opencode_describe_partial(self):
        app = self.by_key["opencode"]
        self.assertEqual(
            apps.describe(app, {"installed": False, "path": None,
                                "version": "", "cli": "2.0.16"}),
            "Installé — CLI 2.0.16",
        )
        image = Path("/home/gab/AppImages/opencode.appimage")
        text = apps.describe(
            app,
            {"installed": True, "path": image, "version": "2.0.16",
             "cli": "2.0.16"},
        )
        self.assertEqual(
            text, "Installé — CLI 2.0.16 · opencode.appimage · version 2.0.16"
        )
        self.assertEqual(
            apps.describe(app, {"installed": False, "path": None,
                                "version": "", "cli": ""}),
            "Non installé",
        )
        self.assertFalse(
            apps.installed_any(app, {"installed": False, "path": None,
                                     "version": "", "cli": ""})
        )

    def test_script_has_option(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / "esde-install"
            script.write_text("#!/bin/bash\n", encoding="utf-8")
            self.assertFalse(
                apps.script_has_option(str(script), "--config-only")
            )
            script.write_text(
                "        --config-only) CONFIG_ONLY=1 ;;\n", encoding="utf-8"
            )
            self.assertTrue(
                apps.script_has_option(str(script), "--config-only")
            )
            self.assertFalse(
                apps.script_has_option("/absent/esde-install", "--config-only")
            )

    def test_esde_rom_path(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            self.assertEqual(apps.esde_rom_path(home), home / "Roms")
            settings = home / "ES-DE/settings"
            settings.mkdir(parents=True)
            (settings / "es_settings.xml").write_text(
                '<string name="ROMDirectory" value="/mnt/roms" />\n',
                encoding="utf-8",
            )
            self.assertEqual(apps.esde_rom_path(home), Path("/mnt/roms"))

    def test_copy_esde_config(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            home, share = root / "home", root / "share"
            (share / "custom_systems").mkdir(parents=True)
            (share / "custom_systems" / "es_systems.xml").write_text(
                "x", encoding="utf-8"
            )
            (share / "tools").mkdir()
            (share / "tools" / "TV.sh").write_text("tv", encoding="utf-8")
            (share / "tools" / "Jellyfin.sh").write_text("jf", encoding="utf-8")

            copied = apps.copy_esde_config(home, share)
            self.assertIn("custom_systems", copied)
            self.assertTrue(
                (home / "ES-DE/custom_systems/es_systems.xml").is_file()
            )
            launcher = home / "Roms/tools/TV.sh"
            self.assertTrue(os.access(launcher, os.X_OK))

            # Un lanceur personnalisé n'est jamais écrasé (ni recopié).
            launcher.write_text("perso", encoding="utf-8")
            (home / "Roms/tools/Jellyfin.sh").unlink()
            copied = apps.copy_esde_config(home, share)
            self.assertNotIn("TV.sh", copied)
            self.assertIn("Jellyfin.sh", copied)
            self.assertEqual(launcher.read_text(encoding="utf-8"), "perso")

    def test_parse_version(self):
        self.assertEqual(apps.parse_version("2.0.16"), "2.0.16")
        self.assertEqual(apps.parse_version("opencode 2.1.0\n"), "2.1.0")
        self.assertEqual(apps.parse_version("pas de version"), "")

    def test_remote_state(self):
        output = (
            "##gearlever\n"
            "yes\n"
            "ES-DE      [3.4.0]    [UpdatesNotAvailable]        "
            "/home/gab/AppImages/esde.appimage\n"
            "Eden       [v0.2.1]   [StaticFileUpdater|embedded] "
            "/home/gab/AppImages/eden.appimage\n"
            "##images\n"
            "esde.appimage\n"
            "eden.appimage\n"
            "xenia_edge.appimage\n"
            "##cli\n"
            "2.1.4\n"
            "##scripts\n"
            "esde-install\t/usr/bin/esde-install\t --yes --config-only\n"
            "azahar-install\t-\t\n"
            "eden-install\t/usr/bin/eden-install\t\n"
            "opencode2-install\t/usr/bin/opencode2-install\t\n"
            "opencode2-remove\t/usr/bin/opencode2-remove\t\n"
        )
        states, gearlever = apps.remote_states(output)
        self.assertTrue(gearlever)
        self.assertTrue(states["esde"]["installed"])
        self.assertEqual(states["esde"]["name"], "esde.appimage")
        self.assertEqual(states["esde"]["version"], "3.4.0")
        self.assertEqual(
            states["esde"]["options"], {"--yes", "--config-only"}
        )
        self.assertEqual(states["eden"]["version"], "v0.2.1")
        self.assertEqual(states["xenia"]["cli"], "")
        self.assertEqual(states["opencode"]["cli"], "2.1.4")
        self.assertTrue(states["opencode"]["remover"].endswith(
            "opencode2-remove"
        ))
        self.assertFalse(states["azahar"]["installed"])
        self.assertFalse(states["azahar"]["script_ok"])

    def test_remote_commands(self):
        output = (
            "##gearlever\nno\n##images\n"
            "esde.appimage\neden.appimage\nopencode.appimage\n"
            "##cli\n2.1.4\n##scripts\n"
            "esde-install\t/usr/bin/esde-install\t --yes --config-only\n"
            "eden-install\t/usr/bin/eden-install\t\n"
            "opencode2-remove\t/usr/bin/opencode2-remove\t\n"
        )
        states, gearlever = apps.remote_states(output)
        self.assertFalse(gearlever)
        self.assertEqual(
            apps.install_command(self.by_key["esde"], states["esde"]),
            "/usr/bin/esde-install --yes",
        )
        self.assertEqual(
            apps.install_command(self.by_key["eden"], states["eden"]),
            "/usr/bin/eden-install",
        )
        self.assertEqual(
            apps.config_command(states["esde"]),
            "/usr/bin/esde-install --config-only",
        )
        self.assertEqual(apps.config_command(states["eden"]), "")
        self.assertEqual(
            apps.remove_command(self.by_key["opencode"], states["opencode"]),
            "/usr/bin/opencode2-remove",
        )
        self.assertTrue(
            apps.remove_command(
                self.by_key["eden"], states["eden"]
            ).startswith("bash -c ")
        )

    def test_remote_without_gearlever_or_scripts(self):
        states, gearlever = apps.remote_states("##gearlever\nno\n")
        self.assertFalse(gearlever)
        self.assertFalse(apps.installed_any(
            self.by_key["eden"], states["eden"]
        ))
        self.assertEqual(apps.config_command(states["esde"]), "")
        self.assertEqual(
            apps.remove_command(self.by_key["opencode"], states["opencode"]),
            "",
        )

    def test_parse_gearlever_listing(self):
        versions = apps.parse_gearlever_listing(
            "Eden         [UNKNOWN]         [GithubUpdater|embedded]   "
            "/home/gab/AppImages/eden.appimage\n"
            "Xenia Edge   [Not specified]   [UpdatesNotAvailable]      "
            "/home/gab/AppImages/xenia_edge.appimage\n"
            "LM-Studio    [0.4.21+2]        [UpdatesNotAvailable]      "
            "/home/gab/AppImages/lmstudio.appimage\n"
        )
        self.assertEqual(versions["/home/gab/AppImages/eden.appimage"], "")
        self.assertEqual(
            versions["/home/gab/AppImages/xenia_edge.appimage"], ""
        )
        self.assertEqual(
            versions["/home/gab/AppImages/lmstudio.appimage"], "0.4.21+2"
        )


class DiscoveryTest(unittest.TestCase):
    own = {"127.0.0.1", "172.17.0.1", "192.168.1.50", "100.64.0.10"}
    networks = [
        ipaddress.ip_network("127.0.0.0/8"),
        ipaddress.ip_network("172.17.0.0/16"),
        ipaddress.ip_network("192.168.1.0/24"),
        ipaddress.ip_network("100.64.0.10/32"),
    ]

    def entry(self, address, hostname="gigafed.local", port=22):
        return {"label": hostname.split(".")[0], "hostname": hostname,
                "user": "gab", "host": address, "port": port}

    def test_parse_local_ipv4(self):
        own, networks = hosts.parse_local_ipv4(
            "2: enp5s0    inet 192.168.1.50/24 brd 192.168.1.255 scope global\n"
            "3: docker0    inet 172.17.0.1/16 brd 172.17.255.255 scope global\n"
            "4: lo         inet 127.0.0.1/8 scope host lo\n"
        )
        self.assertEqual(own, {"192.168.1.50", "172.17.0.1", "127.0.0.1"})
        self.assertEqual(
            [str(network) for network in networks],
            ["192.168.1.0/24", "172.17.0.0/16", "127.0.0.0/8"],
        )

    def test_address_kind(self):
        kind = lambda address: hosts.address_kind(
            address, self.own, self.networks
        )
        self.assertEqual(kind("192.168.1.109"), "lan")
        self.assertEqual(kind("100.64.0.45"), "tailscale")
        self.assertEqual(kind("127.0.0.1"), "")        # boucle locale
        self.assertEqual(kind("172.17.0.1"), "")       # notre docker0
        self.assertEqual(kind("172.18.0.1"), "")       # pont d'une autre machine
        self.assertEqual(kind("169.254.3.4"), "")      # lien local
        self.assertEqual(kind("8.8.8.8"), "")          # hors de notre lien

    def test_known_targets_cover_every_address(self):
        # Un poste revu par Tailscale alors qu'il est enregistré en LAN est la
        # même machine : toutes les adresses du registre comptent.
        existing = [{
            "user": "gab", "host": "192.168.1.109", "port": 22,
            "addresses": [{"address": "100.64.0.45", "kind": "tailscale"}],
        }]
        targets = hosts.known_targets(existing)
        self.assertIn(("gab", "192.168.1.109", 22), targets)
        self.assertIn(("gab", "100.64.0.45", 22), targets)

        tailscale_only = {
            "user": "gab", "host": "100.64.0.45", "port": 22,
            "hosts": [("100.64.0.45", "tailscale"),
                      ("192.168.1.109", "lan")],
        }
        self.assertTrue(hosts.candidate_known(tailscale_only, targets))

    def test_candidate_known_respects_user_and_port(self):
        targets = hosts.known_targets([{
            "user": "gab", "host": "192.168.1.109", "port": 22,
        }])
        self.assertFalse(hosts.candidate_known(
            {"user": "bdc", "host": "192.168.1.109", "port": 22}, targets
        ))
        self.assertFalse(hosts.candidate_known(
            {"user": "gab", "host": "192.168.1.109", "port": 2222}, targets
        ))
        self.assertFalse(hosts.candidate_known(
            {"user": "gab", "host": "192.168.1.110", "port": 22}, targets
        ))

    def test_group_one_machine(self):
        groups = hosts.group_candidates(
            [
                self.entry("127.0.0.1"),
                self.entry("172.17.0.1"),
                self.entry("100.64.0.45"),
                self.entry("192.168.1.109"),
            ],
            self.own, self.networks,
        )
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]["host"], "192.168.1.109")
        self.assertEqual(
            groups[0]["hosts"],
            [("192.168.1.109", "lan"), ("100.64.0.45", "tailscale")],
        )

    def test_group_tailscale_only(self):
        groups = hosts.group_candidates(
            [self.entry("127.0.0.1"), self.entry("100.64.0.45")],
            self.own, self.networks,
        )
        self.assertEqual([item["host"] for item in groups], ["100.64.0.45"])

    def test_group_separates_machines_and_ports(self):
        groups = hosts.group_candidates(
            [
                self.entry("192.168.1.109"),
                self.entry("192.168.1.20", hostname="sureau.local"),
                self.entry("192.168.1.109", port=2222),
            ],
            self.own, self.networks,
        )
        self.assertEqual(len(groups), 3)
        self.assertEqual(
            sorted(item["label"] for item in groups),
            ["gigafed", "gigafed", "sureau"],
        )

    def test_group_without_networks_is_permissive(self):
        groups = hosts.group_candidates(
            [self.entry("172.18.0.1"), self.entry("127.0.0.1")],
            self.own, [],
        )
        self.assertEqual([item["host"] for item in groups], ["172.18.0.1"])


class RemoteAddressTest(unittest.TestCase):
    own = {"192.168.1.109"}

    def test_parse_lan_and_tailscale(self):
        text = "192.168.1.110\n100.64.0.45\n"
        self.assertEqual(
            hosts.parse_remote_addresses(text, "192.168.1.110", self.own),
            [{"address": "100.64.0.45", "kind": "tailscale"}],
        )

    def test_parse_filters_noise(self):
        text = ("127.0.0.1\n169.254.1.2\n192.168.1.109\n"
                "1.1.1.1\n8.8.8.8\n10.0.0.7\n100.64.0.45\n")
        self.assertEqual(
            hosts.parse_remote_addresses(text, "", self.own),
            [
                {"address": "10.0.0.7", "kind": "lan"},
                {"address": "100.64.0.45", "kind": "tailscale"},
            ],
        )


class PinnedHostKeyTest(unittest.TestCase):
    def test_pins_every_address(self):
        raw = "[192.168.1.110]:2222 ssh-ed25519 AAAA\n# commentaire\n"
        self.assertEqual(
            ssh.pinned_entries(raw, ["192.168.1.110", "100.64.0.45"], 2222),
            [
                "[192.168.1.110]:2222 ssh-ed25519 AAAA",
                "[100.64.0.45]:2222 ssh-ed25519 AAAA",
            ],
        )

    def test_port_22_stays_plain(self):
        raw = "192.168.1.110 ssh-ed25519 AAAA\n"
        self.assertEqual(
            ssh.pinned_entries(raw, ["192.168.1.110"], 22),
            ["192.168.1.110 ssh-ed25519 AAAA"],
        )

    def test_hashed_known_hosts_is_rewritten(self):
        raw = "|1|abc=|def= ssh-ed25519 AAAA\n"
        self.assertEqual(
            ssh.pinned_entries(raw, ["gen1.local"], 22),
            ["gen1.local ssh-ed25519 AAAA"],
        )


class UtilTest(unittest.TestCase):
    host = {"user": "gab", "host": "sucina.local", "port": 22}

    def test_target_and_address(self):
        self.assertEqual(util.ssh_target(self.host), "gab@sucina.local")
        self.assertEqual(util.address_text(self.host), "gab@sucina.local")
        self.assertEqual(
            util.address_text({**self.host, "port": 2222}),
            "gab@sucina.local:2222",
        )

    def test_dolphin_url_ipv6(self):
        url = util.dolphin_url({"user": "gab", "host": "fd00::1", "port": 2222})
        self.assertEqual(url, "sftp://gab@[fd00::1]:2222/")

    def test_active_address_falls_back(self):
        host = {
            "user": "gab", "host": "192.168.1.110", "port": 22,
            "addresses": [{"address": "100.64.0.45", "kind": "tailscale"}],
            "active": "",
        }
        self.assertEqual(util.host_addresses(host),
                         ["192.168.1.110", "100.64.0.45"])
        self.assertEqual(util.ssh_target(host), "gab@192.168.1.110")
        self.assertEqual(util.active_kind(host), "")
        host["active"] = "100.64.0.45"
        self.assertEqual(util.ssh_target(host), "gab@100.64.0.45")
        self.assertEqual(util.address_text(host), "gab@100.64.0.45")
        self.assertEqual(util.active_kind(host), "Tailscale")
        self.assertEqual(util.dolphin_url(host), "sftp://gab@100.64.0.45/")

    def test_normalize_record_addresses(self):
        record = util.normalize_record({
            "user": "gab", "host": "192.168.1.110", "port": 22,
            "addresses": [
                {"address": "100.64.0.45", "kind": "tailscale"},
                {"address": "192.168.1.110", "kind": "lan"},   # doublon
                {"address": "100.64.0.45", "kind": "tailscale"},  # doublon
                "pas une adresse avec espaces",
                "10.0.0.7",
            ],
            "active": "100.64.0.45",
            "mac": "AA:BB:CC:DD:EE:FF",     # ancien champ, ignoré
        })
        self.assertEqual(record["addresses"], [
            {"address": "100.64.0.45", "kind": "tailscale"},
            {"address": "10.0.0.7", "kind": ""},
        ])
        self.assertEqual(record["active"], "100.64.0.45")
        self.assertNotIn("mac", record)

    def test_normalize_record_drops_unknown_active(self):
        record = util.normalize_record({
            "user": "gab", "host": "192.168.1.110", "active": "10.9.9.9",
        })
        self.assertEqual(record["active"], "")
        self.assertEqual(record["addresses"], [])

    def test_ssh_opts(self):
        opts = util._ssh_opts(self.host)
        self.assertIn("BatchMode=yes", opts)
        self.assertIn("IdentitiesOnly=yes", opts)
        self.assertIn(str(Path.home() / ".ssh/id_ed25519"), opts)


if __name__ == "__main__":
    unittest.main()
