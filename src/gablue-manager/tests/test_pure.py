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
    apps, cli, hosts, journal, local, prefs, remote_hardware, remote_updates,
    ssh, updates, util,
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

    def test_parse_auto_update_status(self):
        text = (
            "image=enabled\nflatpak=disabled\nbrew=enabled\n"
            "image_frequency=daily\nflatpak_frequency=weekly\n"
            "brew_frequency=daily\n"
        )
        self.assertEqual(
            local.parse_auto_update_status(text),
            {"image": True, "flatpak": False, "brew": True,
             "frequencies": {"image": "daily", "flatpak": "weekly",
                             "brew": "daily"}},
        )
        # Sortie vide : tout désactivé, fréquence de l'image (hebdomadaire).
        self.assertEqual(
            local.parse_auto_update_status(""),
            {"image": False, "flatpak": False, "brew": False,
             "frequencies": {"image": "weekly", "flatpak": "weekly",
                             "brew": "weekly"}},
        )
        self.assertEqual(
            local.parse_auto_update_status(
                "image_frequency=horaire"
            )["frequencies"]["image"],
            "weekly",
        )

    def test_auto_update_script(self):
        script = local.auto_update_script(
            {"image": True, "brew": False}, {"image": "weekly"},
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update image on", script
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update brew off", script
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update schedule image weekly",
            script,
        )
        # Flatpaks inchangés : aucune bascule.
        self.assertNotIn("gablue-auto-update flatpak", script)
        self.assertNotIn("systemctl --user", script)
        self.assertIn("'::gablue:step 3 3 Fréquence — image Gablue'", script)

        enabled = local.auto_update_script(
            {"flatpak": True}, {"flatpak": "daily"},
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update flatpak on", enabled
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update schedule flatpak daily",
            enabled,
        )
        # Gablue ne met à jour que les Flatpaks système : plus de timer du
        # compte à écrire ou à basculer.
        self.assertNotIn("systemctl --user", enabled)
        self.assertIn("'::gablue:step 2 2 Fréquence — Flatpaks'", enabled)

        # Fréquence seule : aucun domaine basculé, seul le calendrier change.
        frequency_only = local.auto_update_script({}, {"brew": "daily"})
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update schedule brew daily",
            frequency_only,
        )
        self.assertNotIn(" systemctl --user", frequency_only)

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
        # `stepsCached` = déjà présent cumulé, pas un avancement de CE
        # téléchargement : 1 étape sur 4, pas 2 (vécu 08/11/2026).
        self.assertEqual(steps["percent"], 25.0)

        # Ligne réelle de bootc 1.17.1 (backend ostree, 08/11/2026) : le cache
        # d'une exécution précédente vaut 4,3 Gio pour 772 Mio à télécharger.
        # L'additionner collait la barre à 100 % dès le premier événement.
        real = local.parse_progress_event(
            '{"bytes":8193,"bytesCached":4296727642,'
            '"bytesTotal":772729517,"description":"Pulling Image: sha256:cdf",'
            '"id":"sha256:cdf","steps":0,"stepsCached":118,"stepsTotal":10,'
            '"subtasks":[],"task":"pulling","type":"ProgressBytes"}'
        )
        self.assertLess(real["percent"], 0.01)
        self.assertEqual((real["bytes"], real["bytes_total"]), (8193, 772729517))
        self.assertEqual(real["label"], "Téléchargement de l'image")

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
        # Une seule installation (système) : « Nothing to do. » veut dire
        # qu'il n'y avait rien à faire.
        self.assertEqual(
            local.flatpak_update_hint(
                "Looking for updates\u2026\nNothing to do.\n"
            ),
            "Aucune mise à jour disponible.",
        )
        self.assertEqual(
            local.flatpak_update_hint("Updating 1/2\u2026          45%\n"),
            "",
        )
        self.assertEqual(local.flatpak_update_hint(""), "")

    def test_flatpak_update_script_is_system_only(self):
        script = local.flatpak_update_script()
        self.assertIn("flatpak update --system -y", script)
        self.assertNotIn("--user", script)

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
        self.assertIn('"$brew" autoremove', script)
        self.assertIn('"$brew" cleanup', script)
        # Cinq caches régénérables : un rm -rf par domaine.
        self.assertIn(f"rm -rf -- {cache}", script)
        self.assertEqual(script.count("rm -rf --"), 5)
        self.assertIn("'::gablue:step 8 8 Homebrew'", script)

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

    def test_boot_options(self):
        text = (
            "0001\tFedora\tsdd · Samsung Portable SSD T5 EVO · 1.8T\n"
            "esp:/dev/sdd1\tWindows\tsdb · Disque externe · 512G\n"
            "0001\tDoublon\tignoré\n"
            "minuit\tX\ty\n"
            "0003\tSans détail\n"
            "0004\t\tlibellé vide\n"
        )
        options = local.parse_boot_options(text)
        self.assertEqual(
            [option["id"] for option in options],
            ["0001", "esp:/dev/sdd1", "0003"],
        )
        self.assertEqual(options[0]["label"], "Fedora")
        self.assertEqual(options[1]["detail"], "sdb · Disque externe · 512G")
        self.assertEqual(options[2]["detail"], "")
        self.assertEqual(local.parse_boot_options(""), [])
        self.assertEqual(local.parse_boot_options("pas de tabulation"), [])

        script = local.boot_select_script("0001")
        self.assertIn(
            "sudo -n /usr/libexec/gablue-bootnext boot 0001", script
        )
        self.assertIn("'::gablue:step 1 1 Démarrage unique'", script)
        self.assertIn(
            "/usr/libexec/gablue-bootnext boot esp:/dev/sdd1",
            local.boot_select_script("esp:/dev/sdd1"),
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

    def test_ryzen_limit(self):
        """Limite Tctl : état JSON tolérant et script d'un seul geste."""
        status = local.parse_ryzenadj_status(json.dumps({
            "supported": True,
            "limit": 85,
            "service_enabled": True,
            "service_active": True,
        }))
        self.assertEqual(
            status,
            {"supported": True, "limit": 85, "service_enabled": True,
             "service_active": True},
        )
        self.assertEqual(
            local.parse_ryzenadj_status(json.dumps({
                "supported": True, "limit": None,
                "service_enabled": False, "service_active": False,
            })),
            {"supported": True, "limit": None, "service_enabled": False,
             "service_active": False},
        )
        # Illisible : pas de JSON, pas de « supported » booléen, hors bornes.
        self.assertEqual(local.parse_ryzenadj_status("pas du json"), {})
        self.assertEqual(
            local.parse_ryzenadj_status('{"limit": 85}'), {}
        )
        self.assertEqual(
            local.parse_ryzenadj_status(
                '{"supported": true, "limit": 200}'
            ),
            {},
        )
        self.assertEqual(
            local.parse_ryzenadj_status(
                '{"supported": true, "limit": "chaud"}'
            ),
            {},
        )

        script = local.ryzenadj_script(85)
        self.assertIn("sudo -n /usr/libexec/gablue-ryzenadj set 85", script)
        self.assertIn("85 °C", script)
        # Hors bornes : jamais transmis tel quel au helper.
        self.assertIn(
            "sudo -n /usr/libexec/gablue-ryzenadj set 110",
            local.ryzenadj_script(999),
        )
        off = local.ryzenadj_script(None)
        self.assertIn("sudo -n /usr/libexec/gablue-ryzenadj off", off)
        self.assertNotIn(" set ", off)

    def test_ryzen_helper_config(self):
        """Le helper relit sa configuration et refuse une valeur hors bornes."""
        import importlib.machinery
        import importlib.util
        import tempfile

        path = (ROOT.parents[1] / "files/system/all/usr/libexec/"
                "gablue-ryzenadj")
        loader = importlib.machinery.SourceFileLoader(
            "gablue_ryzenadj_helper", str(path)
        )
        spec = importlib.util.spec_from_loader(loader.name, loader)
        module = importlib.util.module_from_spec(spec)
        loader.exec_module(module)

        folder = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(folder, True))
        module.CONF = folder / "ryzenadj.conf"
        self.assertIsNone(module.read_limit())
        module.CONF.write_text("TCTL_TEMP=85\n", encoding="utf-8")
        self.assertEqual(module.read_limit(), 85)
        module.CONF.write_text("TCTL_TEMP=120\n", encoding="utf-8")
        self.assertIsNone(module.read_limit())
        module.CONF.write_text("TDP=35\n", encoding="utf-8")
        self.assertIsNone(module.read_limit())
        self.assertEqual((module.MIN_TEMP, module.MAX_TEMP), (45, 110))

    def test_remote_clean_state(self):
        """Sonde SSH : disponibilités, tailles, helper système séparé."""
        text = "\n".join((
            "DIR .cache/gwine/temp 2048",
            "PODMAN",
            'PODMAN_DF=[{"RawReclaimable": 100}]',
            "FLATPAK",
            "CLEAN_HELPER",
            "BREW",
        ))
        state = local.parse_remote_clean_state(text)
        self.assertTrue(state["available"]["podman"])
        self.assertTrue(state["available"]["flatpak"])
        self.assertTrue(state["available"]["brew"])
        self.assertTrue(state["available"]["gwine_temp"])
        self.assertTrue(state["available"]["clean_helper"])
        self.assertFalse(state["available"]["lgp"])
        self.assertEqual(state["sizes"]["podman"], 100)
        self.assertEqual(state["sizes"]["gwine_temp"], 2048)
        # Le script système des Flatpaks dépend du helper, pas de l'outil.
        with_helper = local.remote_clean_script("flatpak", state["available"])
        self.assertIn("flatpak uninstall --unused --user -y", with_helper)
        self.assertIn("gablue-clean flatpak", with_helper)
        without = local.remote_clean_script(
            "flatpak", {"flatpak": True, "clean_helper": False}
        )
        self.assertNotIn("gablue-clean", without)

    def test_remote_btrfs_state(self):
        """JSON findmnt multi-ligne + une ligne COMP par montage."""
        text = "\n".join((
            "HELPER",
            '{"filesystems": [{"target": "/var", "uuid": "u1", '
            '"label": "root", "children": [{"target": "/var/home", '
            '"uuid": "u1", "label": ""}]}, '
            '{"target": "/etc", "uuid": "u1", "label": ""}]}',
            "COMP\t/var\tzstd",
            "COMP\t/var/home\t",
        ))
        state = local.parse_btrfs_remote_state(text)
        self.assertTrue(state["ok"])
        self.assertEqual(
            [(row["target"], row["compression"])
             for row in state["filesystems"]],
            [("/var", "zstd"), ("/var/home", "")],
        )
        without = local.parse_btrfs_remote_state("NO_HELPER\n{}")
        self.assertFalse(without["ok"])
        self.assertEqual(without["filesystems"], [])

    def test_remote_nvme_and_scripts(self):
        """Sonde NVMe et commandes distantes sans marqueurs de progression."""
        payload = json.dumps({"devices": [
            {"device": "/dev/nvme0", "hctm": True, "enabled": True},
        ]})
        state = local.parse_remote_nvme_thermal_state("HELPER\n" + payload)
        self.assertTrue(state["ok"])
        self.assertEqual(state["devices"][0]["device"], "/dev/nvme0")
        # Sans le marqueur du helper, les seuils restent illisibles.
        self.assertFalse(
            local.parse_remote_nvme_thermal_state(payload)["ok"]
        )
        self.assertEqual(
            local.parse_remote_nvme_thermal_state("NO_HELPER\n")["devices"],
            [],
        )

        self.assertEqual(
            local.btrfs_remote_script({"/var": True}),
            "sudo -n /usr/libexec/gablue-btrfs-compress zstd /var\n",
        )
        script = local.nvme_thermal_remote_script({
            "/dev/nvme0": (70, 75),
            "/dev/nvme1": None,
        })
        self.assertIn(
            "gablue-nvme-thermal set /dev/nvme0 70 75", script
        )
        self.assertIn("gablue-nvme-thermal off /dev/nvme1", script)
        self.assertNotIn("::gablue:", script)

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


class HardwareTest(unittest.TestCase):
    """Résumé matériel : analyses pures (aucune lecture root)."""

    def test_parse_cpu_info(self):
        text = (
            "processor\t: 0\n"
            "model name\t: AMD Ryzen 7 7800X3D 8-Core Processor\n"
            "cpu cores\t: 8\n"
            "siblings\t: 16\n\n"
            "processor\t: 1\n"
            "model name\t: AMD Ryzen 7 7800X3D 8-Core Processor\n"
        )
        info = local.parse_cpu_info(text)
        self.assertEqual(
            info["model"], "AMD Ryzen 7 7800X3D 8-Core Processor"
        )
        self.assertEqual(info["cores"], 8)
        self.assertEqual(info["threads"], 16)
        self.assertEqual(
            local.cpu_label(text),
            "AMD Ryzen 7 7800X3D 8-Core Processor · 8 cœurs / 16 threads",
        )

    def test_temperature_measures(self):
        # Chaque ligne de `format_temperatures` se recompose en mesures
        # (nom, valeur) : deux corps de texte, comme les lignes de disques.
        text = ("CPU\u00a048\u00a0°C\n"
                "SSD\u00a01\u00a037\u00a0°C\u00a0✓  SSD\u00a02\u00a032\u00a0°C\u00a0✓\n"
                "HDD\u00a01\u00a0(hdd1)\u00a036\u00a0°C  HDD\u00a02\u00a0(hdd2)\u00a037\u00a0°C\u00a0✗\n"
                "RAM\u00a01\u00a033\u00a0°C")
        self.assertEqual(local.temperature_measures(text), [
            [("CPU", "48 °C")],
            [("SSD 1", "37 °C ✓"), ("SSD 2", "32 °C ✓")],
            [("HDD 1 (hdd1)", "36 °C"), ("HDD 2 (hdd2)", "37 °C ✗")],
            [("RAM 1", "33 °C")],
        ])
        # Le nom peut lui-même porter une insécable : couper au premier
        # séparateur donnerait « SSD » + « 1 37 °C ». Lignes vides ignorées.
        self.assertEqual(local.temperature_measures("\n\n"), [])
        self.assertEqual(local.temperature_measures(""),
                         [])
        # Une mesure inattendue n'est jamais perdue : tout passe en nom.
        self.assertEqual(local.temperature_measures("Capteur muet"),
                         [[("Capteur muet", "")]])

    def test_temperatures(self):
        # Capteurs réels : k10temp + amdgpu (gen1), coretemp seul (sucina,
        # iGPU Intel sans capteur). Un seul capteur par nature, avec
        # l'étiquette significative.
        gen1 = [
            {"chip": "acpitz", "temps": [("temp1", 20.0)]},
            {"chip": "amdgpu", "temps": [("edge", 30.0), ("junction", 45.0)]},
            {"chip": "k10temp", "temps": [("Tctl", 30.875),
                                          ("Tccd1", 30.0)]},
            {"chip": "nvme", "temps": [("Composite", 36.85)]},
        ]
        values = local.parse_hwmon(gen1)
        self.assertEqual(values["cpu"], 30.875)
        self.assertEqual(values["gpu"], 30.0)      # edge, pas junction
        self.assertEqual(
            local.format_temperatures(values),
            "CPU\u00a031\u00a0°C\nGPU\u00a030\u00a0°C\nSSD\u00a037\u00a0°C",
        )
        sucina = [
            {"chip": "coretemp", "temps": [("Package id 0", 45.0),
                                           ("Core 0", 60.0)]},
        ]
        values = local.parse_hwmon(sucina)
        self.assertEqual(values["cpu"], 45.0)      # package, pas le max
        self.assertIsNone(values["gpu"])
        self.assertEqual(local.format_temperatures(values), "CPU\u00a045\u00a0°C")
        # Pilote sans étiquette connue : le maximum reste une mesure.
        self.assertEqual(
            local.parse_hwmon([{"chip": "amdgpu",
                                "temps": [("junction", 41.0)]}])["gpu"],
            41.0,
        )
        # Disques : pilote `nvme` → « SSD », numéroté dès qu'il y en a
        # plusieurs (deux instances hwmon distinctes, pas une seule).
        one = [{"chip": "nvme", "device": "nvme0",
                "temps": [("Composite", 36.85), ("Sensor 1", 40.0)]}]
        values = local.parse_hwmon(one)
        self.assertEqual(values["ssd"], [("SSD", 36.85, None)])
        two = one + [{"chip": "nvme", "device": "nvme1",
                      "temps": [("Composite", 32.0)]}]
        self.assertEqual(local.parse_hwmon(two)["ssd"],
                         [("SSD 1", 36.85, None), ("SSD 2", 32.0, None)])
        # Verdict SMART d'un NVMe : rattaché par le nom du périphérique hwmon.
        self.assertEqual(
            local.parse_hwmon(two, {"nvme1": True})["ssd"],
            [("SSD 1", 36.85, None), ("SSD 2", 32.0, True)],
        )
        self.assertEqual(
            local.format_temperatures(local.parse_hwmon(two, {"nvme1": True})),
            "SSD\u00a01\u00a037\u00a0°C  SSD\u00a02\u00a032\u00a0°C\u00a0✓",
        )
        self.assertEqual(
            local.format_temperatures(local.parse_hwmon(two)),
            "SSD\u00a01\u00a037\u00a0°C  SSD\u00a02\u00a032\u00a0°C",
        )
        # Une ligne par capteur et par périphérique (script SSH) : les deux
        # NVMe ne doivent pas fusionner en un seul disque à six sondes.
        lines = ("nvme|nvme0|Composite|36850\n"
                 "nvme|nvme0|Sensor 1|40000\n"
                 "nvme|nvme1|Composite|32000\n"
                 "k10temp|0000:00:18.3|Tctl|43500\n")
        values = local.parse_hwmon(local.parse_hwmon_lines(lines))
        self.assertEqual(values["cpu"], 43.5)
        self.assertEqual(values["ssd"],
                         [("SSD 1", 36.85, None), ("SSD 2", 32.0, None)])
        self.assertEqual(local.parse_hwmon([]),
                         {"cpu": None, "gpu": None, "ssd": [], "ram": []})
        self.assertEqual(local.format_temperatures({}), "")

    def test_sata_temperatures(self):
        # Le helper rend tous les disques SATA, y compris ceux en veille :
        # `celsius: null` = endormi (jamais réveillé), donc rien à afficher.
        payload = {"modules": [], "disks": [
            {"device": "sda", "mount": "/var/hdd3", "celsius": 36.0},
            {"device": "sdb", "mount": "/var/hdd2", "celsius": None},
            {"device": "sdc", "mount": "", "celsius": 25.0},
        ]}
        self.assertEqual(local.sata_temperatures(payload),
                         [("hdd3", 36.0, None), ("sdc", 25.0, None)])
        # Disque non monté : nom du périphérique, jamais un « hddN » qui
        # pourrait tomber sur le nom de montage d'un autre disque.
        payload["disks"] = [
            {"device": "sda", "mount": "/var/hdd2", "celsius": 30.0},
            {"device": "sdb", "mount": "", "celsius": 31.0},
        ]
        self.assertEqual(local.sata_temperatures(payload),
                         [("hdd2", 30.0, None), ("sdb", 31.0, None)])
        self.assertEqual(local.sata_temperatures({}), [])
        self.assertEqual(local.sata_temperatures({"disks": "nope"}), [])
        # Fusion avec les capteurs hwmon, dans l'ordre CPU · GPU · SSD · hdd.
        devices = [{"chip": "k10temp", "device": "0000:00:18.3",
                    "temps": [("Tctl", 30.0)]}]
        self.assertEqual(
            local.temperature_summary(devices, sata=[("hdd1", 36.0)]),
            "CPU\u00a030\u00a0°C\nHDD\u00a0(hdd1)\u00a036\u00a0°C",
        )
        # Une ligne par famille : les disques d'une même famille se partagent
        # leur ligne, les barrettes passent en dernier.
        self.assertEqual(
            local.temperature_summary(
                [{"chip": "spd5118", "temps": [("temp1", 33.0)]},
                 {"chip": "spd5118", "temps": [("temp1", 35.0)]}],
                sata=[("hdd1", 36.0), ("hdd2", 37.0)],
            ),
            "HDD\u00a01\u00a0(hdd1)\u00a036\u00a0°C  HDD\u00a02\u00a0(hdd2)\u00a037\u00a0°C\n"
            "RAM\u00a01\u00a033\u00a0°C  RAM\u00a02\u00a035\u00a0°C",
        )
        # Six disques SATA : deux lignes de trois, la case n'a pas la place
        # pour six mesures d'affilée.
        six = [("hdd1", 33.0), ("hdd2", 34.0), ("hdd3", 36.0),
               ("hdd4", 37.0), ("hdd5", 38.0), ("hdd6", 39.0)]
        self.assertEqual(
            local.format_temperatures({}, sata=six),
            "HDD\u00a01\u00a0(hdd1)\u00a033\u00a0°C  HDD\u00a02\u00a0(hdd2)\u00a034\u00a0°C  HDD\u00a03\u00a0(hdd3)\u00a036\u00a0°C\n"
            "HDD\u00a04\u00a0(hdd4)\u00a037\u00a0°C  HDD\u00a05\u00a0(hdd5)\u00a038\u00a0°C  HDD\u00a06\u00a0(hdd6)\u00a039\u00a0°C",
        )
        # Ordre d'affichage : le helper rend ses disques dans son propre
        # ordre (« 3 2 1 » à l'écran, vécu 08/11/2026), on trie par nom.
        self.assertEqual(
            local.format_temperatures(
                {}, sata=[("hdd3", 36.0), ("hdd1", 33.0),
                          ("hdd10", 40.0), ("hdd2", 34.0)],
            ),
            "HDD\u00a01\u00a0(hdd1)\u00a033\u00a0°C  HDD\u00a02\u00a0(hdd2)\u00a034\u00a0°C  HDD\u00a03\u00a0(hdd3)\u00a036\u00a0°C\n"
            "HDD\u00a04\u00a0(hdd10)\u00a040\u00a0°C",
        )
        # Les montages ne portent pas les mêmes noms partout : le disque est
        # numéroté comme les SSD, le montage passe entre parenthèses.
        self.assertEqual(
            local.format_temperatures(
                {}, sata=[("films", 31.0), ("data2", 33.0, True)],
            ),
            "HDD\u00a01\u00a0(data2)\u00a033\u00a0°C\u00a0✓  HDD\u00a02\u00a0(films)\u00a031\u00a0°C",
        )
        # Tri naturel : « hdd10 » après « hdd2 », jamais avant.
        self.assertEqual(
            sorted(["hdd10", "hdd2", "hdd1"], key=local.natural_key),
            ["hdd1", "hdd2", "hdd10"],
        )
        # Quel disque est « SSD 2 » ? L'étiquette de la page « Disques » le
        # dit, et le disque du système s'appelle « Système » (08/11/2026).
        self.assertEqual(local.disk_key("nvme0n1p3"), "nvme0")
        self.assertEqual(local.disk_key("sdc1"), "sdc")
        self.assertEqual(local.disk_key("mmcblk0p2"), "mmcblk0")
        labels = local.disk_labels([
            {"source": "/dev/nvme0n1p3", "label": "Dossier personnel",
             "system": True},
            {"source": "/dev/nvme1n1p1", "label": "ssdtemp", "system": False},
            {"source": "/dev/sdc1", "label": "hdd1", "system": False},
        ])
        self.assertEqual(
            labels, {"nvme0": "Système", "nvme1": "ssdtemp", "sdc": "hdd1"},
        )
        two_disks = [
            {"chip": "nvme", "device": "nvme0",
             "temps": [("Composite", 36.85)]},
            {"chip": "nvme", "device": "nvme1",
             "temps": [("Composite", 30.0)]},
        ]
        # Le périphérique hwmon porte l'étiquette ; sans elle, le nom reste nu.
        self.assertEqual(
            [entry[0] for entry in local.parse_hwmon(
                two_disks, labels={"nvme0": "Système", "nvme1": "ssdtemp"},
            )["ssd"]],
            ["SSD 1 (Système)", "SSD 2 (ssdtemp)"],
        )
        self.assertEqual(
            [entry[0] for entry in local.parse_hwmon(two_disks)["ssd"]],
            ["SSD 1", "SSD 2"],
        )
        self.assertEqual(
            local.temperature_summary(
                two_disks,
                disks=[{"source": "/dev/nvme0n1p3", "system": True,
                        "label": "Dossier personnel"},
                       {"source": "/dev/nvme1n1p1", "label": "ssdtemp",
                        "system": False}],
            ),
            "SSD\u00a01\u00a0(Système)\u00a037\u00a0°C  "
            "SSD\u00a02\u00a0(ssdtemp)\u00a030\u00a0°C",
        )
        # Verdict SMART : ✓ quand `smartctl -H` passe, ✗ quand il échoue,
        # rien quand le disque ne le publie pas (pont USB, disque muet).
        payload["disks"] = [
            {"device": "sda", "mount": "/var/hdd1", "celsius": 33.0,
             "smart": True},
            {"device": "sdb", "mount": "/var/hdd2", "celsius": 34.0,
             "smart": False},
            {"device": "sdc", "mount": "/var/hdd3", "celsius": 36.0},
        ]
        rows = local.sata_temperatures(payload)
        self.assertEqual(rows, [("hdd1", 33.0, True), ("hdd2", 34.0, False),
                                ("hdd3", 36.0, None)])
        self.assertEqual(
            local.format_temperatures({}, sata=rows),
            "HDD\u00a01\u00a0(hdd1)\u00a033\u00a0°C\u00a0✓  HDD\u00a02\u00a0(hdd2)\u00a034\u00a0°C\u00a0✗  "
            "HDD\u00a03\u00a0(hdd3)\u00a036\u00a0°C",
        )
        self.assertEqual(local.parse_hwinfo("pas du json"), {})
        self.assertEqual(local.parse_hwinfo('{"disks": []}'), {"disks": []})

    def test_parse_cpu_without_cores_field(self):
        # ARM et quelques VMs n'exposent pas « cpu cores » : les blocs
        # « processor » restent la seule mesure.
        info = local.parse_cpu_info("processor\t: 0\nprocessor\t: 1\n")
        self.assertEqual(info["threads"], 2)
        self.assertEqual(info["cores"], 0)

    def test_parse_lspci_display(self):
        amd = (
            '03:00.0 "VGA compatible controller [0300]" '
            '"Advanced Micro Devices, Inc. [AMD/ATI] [1002]" '
            '"Navi 44 [Radeon RX 9050 / 9060 XT] [7590]" -rc0 -p00 '
            '"Sapphire Technology Limited [1da2]" "Device [a493]"'
        )
        self.assertEqual(
            local.parse_lspci_display(amd),
            "Navi 44 [Radeon RX 9050 / 9060 XT]",
        )
        intel = (
            '00:02.0 "VGA compatible controller [0300]" '
            '"Intel Corporation [8086]" '
            '"Raptor Lake-S GT1 [UHD Graphics 770] [a780]" -r04 -p00 '
            '"ASUSTeK Computer Inc. [1043]" "Device [8882]"'
        )
        # La marque n'apparaît pas dans le nom du périphérique : elle est
        # ajoutée une seule fois, sinon « UHD Graphics » ne dit rien.
        self.assertEqual(
            local.parse_lspci_display(intel),
            "Intel Raptor Lake-S GT1 [UHD Graphics 770]",
        )
        self.assertEqual(local.parse_lspci_display(""), "")

    def test_format_capacity(self):
        self.assertEqual(local.format_capacity(16 * 1024 ** 3), "16 Gio")
        self.assertEqual(local.format_capacity(2 * 1024 ** 4), "2 Tio")
        self.assertEqual(local.format_capacity(0), "0 Gio")

    def test_format_uptime(self):
        self.assertEqual(local.format_uptime(0), "0 min")
        self.assertEqual(local.format_uptime(3725), "1 h 02")
        self.assertEqual(
            local.format_uptime(5 * 86400 + 4 * 3600), "5 j 4 h"
        )

    def test_memory_total(self):
        self.assertAlmostEqual(
            local.memory_total_gib("MemTotal:       32490004 kB\n"),
            30.98, places=2,
        )
        self.assertIsNone(local.memory_total_gib("rien"))

    def test_memory_usage(self):
        # « Utilisée » = MemTotal − MemAvailable : le cache de fichiers ne
        # doit pas compter comme de la mémoire occupée.
        text = ("MemTotal:       32490004 kB\n"
                "MemFree:         1055000 kB\n"
                "MemAvailable:   26444996 kB\n")
        self.assertEqual(local.format_memory(text),
                         "5,8 Gio utilisés sur 31 Gio · 19 %")
        # Sans MemAvailable : le total seul, jamais un pourcentage inventé.
        self.assertEqual(local.format_memory("MemTotal: 32490004 kB"),
                         "31 Gio")
        self.assertEqual(local.format_memory(""), "")
        self.assertIsNone(local.memory_used_gib("MemTotal: 1 kB"))

    def test_memory_installed_note(self):
        # Un iGPU réserve une tranche : la somme des barrettes est rappelée
        # entre parenthèses, sinon « Barrettes » semble contredire le total.
        text = ("MemTotal:       15100000 kB\n"
                "MemAvailable:    9000000 kB\n")
        modules = [{"size_gib": 8}, {"size_gib": 8}]
        self.assertEqual(
            local.format_memory(text, modules),
            "5,8 Gio utilisés sur 14,4 Gio (16 Gio installés) · 40 %",
        )
        # Mêmes arrondis : aucune parenthèse, ce serait un doublon.
        text = ("MemTotal:       16700000 kB\n"
                "MemAvailable:   10000000 kB\n")
        self.assertEqual(
            local.format_memory(text, [{"size_gib": 16}]),
            "6,4 Gio utilisés sur 15,9 Gio · 40 %",
        )
        self.assertEqual(local.memory_installed_gib([{"size_gib": 16}]), 16)
        self.assertEqual(local.memory_installed_gib([]), 0)

    def test_vram(self):
        # Carte dédiée : amdgpu publie le fabricant des puces (samsung) ;
        # NVIDIA passe par /proc. Les deux disent « VRAM ».
        dedicated = "total=17095983104\nvendor=samsung\n"
        self.assertEqual(local.parse_vram(dedicated), (15.921875, True))
        self.assertEqual(local.parse_vram("Video Memory:   8192 MiB"),
                         (8.0, True))
        # APU : pas de mémoire propre, le fichier de fabricant est absent —
        # la valeur est une tranche de RAM réservée par le BIOS.
        self.assertEqual(local.parse_vram("total=8589934592\nvendor=\n"),
                         (8.0, False))
        self.assertEqual(local.parse_vram(""), (None, False))
        self.assertEqual(local.parse_vram("Carte inconnue"), (None, False))
        # Libellés : « VRAM » pour du dédié, « RAM allouée » pour un APU ;
        # 15,9 Gio annoncés 16, mais 0,5 reste 0,5.
        self.assertEqual(local.format_vram(15.921875, True), "16 Gio VRAM")
        self.assertEqual(local.format_vram(8.0, True), "8 Gio VRAM")
        self.assertEqual(local.format_vram(0.5, True), "0,5 Gio VRAM")
        self.assertEqual(local.format_vram(8.0, False),
                         "8 Gio (RAM allouée)")

    def test_memory_modules(self):
        data = json.dumps({"modules": [
            {
                "size_gib": 16, "type": "DDR5", "speed": 4800,
                "configured_speed": 6000,
            },
            {
                "size_gib": 16, "type": "DDR5", "speed": 4800,
                "configured_speed": 6000,
            },
            {"size_gib": 0, "type": "DDR5"},
        ]})
        modules = local.parse_memory_modules(data)
        self.assertEqual(len(modules), 2)
        # La vitesse effective (Configured Memory Speed) prime sur celle du
        # profil JEDEC : c'est celle à laquelle la machine tourne.
        self.assertEqual(modules[0]["speed"], 6000)
        self.assertEqual(
            local.format_memory_modules(modules),
            "2 × 16 Gio · DDR5 · 6000 MT/s",
        )

    def test_memory_modules_single(self):
        modules = [
            {"size_gib": 16, "type": "DDR4", "speed": 2133,
             "manufacturer": "Kingston"},
        ]
        # Une barrette seule garde le « 1 × » : sans lui, « 16 Gio » se lisait
        # comme le total installé alors que la ligne « Mémoire » donne la RAM
        # utilisable (vécu 13/11/2026, fednuc).
        self.assertEqual(
            local.format_memory_modules(modules),
            "1 × 16 Gio · Kingston · DDR4 · 2133 MT/s",
        )
        self.assertEqual(local.memory_modules_label(modules), "Barrette")

    def test_memory_modules_label_plural(self):
        self.assertEqual(
            local.memory_modules_label([{"size_gib": 8}, {"size_gib": 8}]),
            "Barrettes",
        )
        # Une taille nulle n'est pas une barrette : le libellé suit.
        self.assertEqual(
            local.memory_modules_label(
                [{"size_gib": 8}, {"size_gib": 0}]
            ),
            "Barrette",
        )
        self.assertEqual(local.memory_modules_label([]), "Barrettes")

    def test_memory_modules_mixed(self):
        modules = [
            {"size_gib": 16, "type": "DDR5", "speed": 5600},
            {"size_gib": 32, "type": "DDR5", "speed": 5600},
        ]
        self.assertEqual(
            local.format_memory_modules(modules),
            "1 × 16 Gio + 1 × 32 Gio · DDR5 · 5600 MT/s",
        )

    def test_memory_modules_garbage(self):
        self.assertEqual(local.parse_memory_modules("pas du json"), [])
        self.assertEqual(local.parse_memory_modules("{}"), [])
        self.assertEqual(local.format_memory_modules([]), "")


class WifiPowersaveTest(unittest.TestCase):
    def test_parse_off(self):
        state = local.parse_wifi_powersave_state(
            "config=off\niface=wlp4s0:off\niface=wlp11s0:off\n"
        )
        self.assertEqual(state["config"], "off")
        self.assertTrue(state["disabled"])
        self.assertTrue(state["present"])
        self.assertEqual(
            state["ifaces"], {"wlp4s0": "off", "wlp11s0": "off"}
        )

    def test_parse_default_and_empty(self):
        state = local.parse_wifi_powersave_state("config=default\n")
        self.assertEqual(state["config"], "default")
        self.assertFalse(state["disabled"])
        self.assertFalse(state["present"])
        self.assertEqual(local.parse_wifi_powersave_state("")["config"],
                         "default")

    def test_parse_ignores_garbage(self):
        state = local.parse_wifi_powersave_state(
            "config=bizarre\niface=wlp4s0:\niface=:on\niface=wlp4s0:peut-être\n"
        )
        self.assertEqual(state["config"], "default")
        self.assertEqual(state["ifaces"], {})

    def test_apply_script(self):
        self.assertEqual(
            local.wifi_powersave_apply_script(True),
            "sudo -n /usr/libexec/gablue-wifi-powersave off\n",
        )
        self.assertEqual(
            local.wifi_powersave_apply_script(False),
            "sudo -n /usr/libexec/gablue-wifi-powersave on\n",
        )


class TailscaleTest(unittest.TestCase):
    def test_parse(self):
        state = local.parse_tailscale_state(
            "enabled=enabled\nactive=inactive\n"
            "server=https://head.example\ninstalled=yes\n"
            'status={"BackendState": "Running", "Self": {"DNSName": "x.y."}}\n'
        )
        self.assertEqual(state["server"], "https://head.example")
        self.assertTrue(state["installed"])
        self.assertTrue(state["enabled"])
        self.assertFalse(state["active"])
        self.assertEqual(state["status"]["backend"], "Running")
        self.assertEqual(state["status"]["node"], "x.y")

    def test_parse_status(self):
        status = local.parse_tailscale_status(
            '{"BackendState": "Running", '
            '"Self": {"DNSName": "gigafed.tail.io.", "HostName": "gigafed", '
            '"TailscaleIPs": ["100.64.0.45", "fd7a::2"]}, '
            '"CurrentTailnet": {"Name": "head.example", '
            '"MagicDNSSuffix": "tail.io"}}'
        )
        self.assertEqual(status["backend"], "Running")
        self.assertEqual(status["network"], "tail.io")
        self.assertEqual(status["node"], "gigafed")
        self.assertEqual(status["address"], "100.64.0.45")
        self.assertEqual(local.parse_tailscale_status(""), {})

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
                    prefs.check_interval_seconds(), 120 * 60
                )
                self.assertEqual(
                    prefs.image_interval_seconds(), 1440 * 60
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
            {"image": True, "flatpak": True, "brew": False, "gwine": True,
             "apps": True},
        )
        # L'interrupteur général coupe la surveillance automatique, sans
        # effacer les domaines choisis (le bouton « Vérifier » les réutilise).
        values["check_auto"] = False
        self.assertEqual(
            prefs.auto_check_domains(values),
            {"image": False, "flatpak": False, "brew": False, "gwine": False,
             "apps": False},
        )
        self.assertFalse(prefs.checks_enabled(values))
        self.assertTrue(prefs.check_domains(values)["image"])
        self.assertEqual(
            prefs.check_interval_seconds({**values, "check_interval": 2}),
            5 * 60,
        )


class UpdatesTest(unittest.TestCase):
    def test_deferred_domains_keep_previous_result(self):
        """Deux cadences : l'image reportée garde son état, le reste le perd."""
        values = dict(prefs.DEFAULTS)
        self.assertEqual(prefs.check_interval_seconds(values), 120 * 60)
        self.assertEqual(prefs.image_interval_seconds(values), 1440 * 60)
        # Une valeur aberrante est ramenée dans les bornes, pas ignorée.
        self.assertEqual(
            prefs.image_interval_seconds({"check_image_interval": 999999}),
            prefs.INTERVAL_RANGE[1] * 60,
        )
        enabled = {"image": True, "flatpak": True, "gwine": False,
                   "apps": False, "brew": False}
        state = updates.check_all(enabled, online=False, deferred={"image"})
        # Le domaine reporté n'est pas sondé et le dit.
        self.assertEqual(state["domains"]["image"], {"state": "deferred"})
        self.assertEqual(state["deferred"], ["image"])
        previous = {
            "checked_at": 1.0, "online": True, "deferred": [],
            "domains": {"image": {"state": "available", "days": 3}},
        }
        merged = updates.merge_states(previous, state)
        # Résultat précédent conservé pour l'image…
        self.assertEqual(merged["domains"]["image"]["state"], "available")
        # …et le domaine décoché reste « skipped », jamais l'ancien résultat.
        self.assertEqual(merged["domains"]["gwine"]["state"], "skipped")
        # Le premier état (aucun précédent) reste tel quel.
        self.assertEqual(updates.merge_states(None, state), state)

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

    def test_bootc_target_version(self):
        # Sortie réelle de `bootc upgrade --check` (bootc 1.17.1, 08/11/2026).
        text = (
            "Update available for: "
            "ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main-dx:latest\n"
            "  Version: 20261108-00h52\n"
            "  Digest: sha256:6cabe743e3e5\n"
            "Total new layers: 128   Size: 5.1 GB\n"
        )
        self.assertEqual(updates.parse_bootc_version(text), "20261108-00h52")
        self.assertEqual(updates.parse_bootc_version("No update available."), "")
        self.assertEqual(updates.parse_bootc_version(""), "")
        # Le résumé nomme la cible, pas seulement l'image qui tourne.
        image = {"state": "available", "staged": False,
                 "version": "20261107-23h18",
                 "available_version": "20261108-00h52"}
        line = "\n".join(updates.summary_lines({"domains": {"image": image}}))
        self.assertIn("20261107-23h18 → 20261108-00h52", line)
        # Bootc muet sur la version : on ne prétend pas connaître la cible.
        silent = {**image, "available_version": ""}
        line = "\n".join(updates.summary_lines({"domains": {"image": silent}}))
        self.assertIn("actuelle 20261107-23h18", line)
        self.assertNotIn("→", line)

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

    def test_check_gwine_logs_unknown(self):
        # « vérification impossible » sans trace ne se diagnostiquait pas :
        # le code retour et la fin de sortie partent dans journald.
        import unittest.mock as mock
        with mock.patch.object(
            updates, "_run",
            return_value=(1, "Erreur: Option invalide '--check'\n"),
        ), mock.patch.object(
            updates, "_FAILED", {}
        ), mock.patch.object(
            updates.journal, "log_failure"
        ) as log:
            result = updates.check_gwine()
        self.assertEqual(result["state"], "unknown")
        log.assert_called_once()
        detail = log.call_args.kwargs.get("detail", "")
        self.assertIn("code 1", detail)
        self.assertIn("Option invalide", detail)
        # Un verdict concluant ne journalise rien (pas de bruit).
        with mock.patch.object(
            updates, "_run", return_value=(0, "gwine-check: uptodate\n"),
        ), mock.patch.object(updates, "_FAILED", {}), mock.patch.object(
            updates.journal, "log_failure"
        ) as log:
            self.assertEqual(updates.check_gwine()["state"], "uptodate")
        log.assert_not_called()

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
        # L'âge du déploiement n'est pas une « mise à jour » : il vit dans
        # « Matériel et système » et dans le statut, jamais dans la liste.
        self.assertNotIn("Dernière mise à jour de Gablue", text)
        self.assertIn(
            "il y a 12 jours", updates.image_detail(state["domains"]["image"])
        )
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
        # Image déjà téléchargée : ce n'est PLUS une mise à jour « disponible »
        # (rien à télécharger, seule reste un redémarrage), donc pas de +1 —
        # et le résumé le dit au lieu d'annoncer « aucune mise à jour »
        # (vécu 13/11/2026 : « 1 mise à jour » puis un « Mettre à jour » sans
        # rien à faire).
        offline_staged = {"online": False, "domains": {
            "image": {"state": "available", "staged": True, "days": 2},
            "flatpak": {"state": "offline"},
            "brew": {"state": "offline"},
            "gwine": {"state": "offline"},
        }}
        self.assertEqual(updates.total_updates(offline_staged), 0)
        self.assertIn("redémarrage requis",
                      updates.compact_summary(offline_staged))
        staged = {"online": True, "domains": {
            "image": {"state": "available", "staged": True, "days": 2,
                      "version": "20261109-11h34",
                      "staged_version": "20261112-08h54"},
            "flatpak": {"state": "updates", "apps": ["org.kde.krita"]},
        }}
        self.assertEqual(updates.total_updates(staged), 1)
        self.assertEqual(
            updates.staged_text(staged),
            "Image 20261112-08h54 téléchargée — redémarrage requis",
        )
        self.assertIn(
            "Image Gablue : mise à jour téléchargée",
            "\n".join(updates.summary_lines(staged)),
        )
        self.assertIn("1 mise à jour", updates.compact_summary(staged))


    def test_notification_text(self):
        # Auto : parle seulement quand le compteur monte (pas de doublon
        # toutes les deux heures) ; manuel : répond toujours, même « rien ».
        upd = {"checked_at": 1.0, "online": True, "domains": {
            "image": {"state": "available", "staged": False,
                      "version": "20261107-23h18",
                      "available_version": "20261108-00h52"},
            "flatpak": {"state": "updates", "apps": ["org.kde.krita"]},
        }}
        title, body = updates.notification_text(upd)
        self.assertEqual(title, "2 mises à jour disponibles")
        self.assertIn("20261107-23h18 → 20261108-00h52", body)
        self.assertIn("org.kde.krita", body)
        self.assertIsNone(updates.notification_text(upd, upd))
        self.assertEqual(updates.notification_text(upd, upd, manual=True)[0],
                         "2 mises à jour disponibles")
        # Rien à signaler, mais l'utilisateur a demandé : il reçoit sa réponse.
        none = {"online": True, "domains": {
            "image": {"state": "uptodate", "days": 3},
            "flatpak": {"state": "uptodate", "apps": []},
        }}
        self.assertEqual(updates.notification_text(none),
                         None)
        self.assertEqual(updates.notification_text(none, manual=True),
                         ("Aucune mise à jour", "Gablue mis à jour il y a 3 jours"))
        self.assertEqual(
            updates.notification_text({"online": False, "domains": {}},
                                      manual=True),
            ("Vérification des mises à jour",
             "Hors ligne — vérification impossible"),
        )
        self.assertIsNone(updates.notification_text(None, manual=True))

    def test_unverified_domains(self):
        # Un domaine illisible n'autorise ni « À jour » ni « aucune mise à
        # jour » : le résumé doit nommer le doute (vécu 08/11/2026).
        state = {"online": True, "domains": {
            "image": {"state": "uptodate", "days": 1},
            "flatpak": {"state": "unknown", "apps": []},
            "brew": {"state": "unavailable", "formulas": []},
            "gwine": {"state": "uptodate", "components": []},
            "apps": {"state": "uptodate", "apps": []},
        }}
        self.assertEqual(updates.unverified_domains(state), ["flatpak"])
        self.assertEqual(updates.unverified_domains(None), [])
        self.assertEqual(
            updates.compact_summary(state), "1 domaine non vérifié"
        )
        self.assertEqual(
            updates.compact_summary({**state, "domains": {
                **state["domains"],
                "brew": {"state": "unknown"},
                "apps": {"state": "unknown"},
            }}),
            "3 domaines non vérifiés",
        )

    def test_state_message(self):
        # La fenêtre pousse son état au tray (`state:<json>`) : même données
        # des deux côtés, aucune sonde à refaire.
        state = {"online": True, "checked_at": 1_800_000_000, "domains": {
            "image": {"state": "uptodate"},
            "apps": {"state": "updates", "apps": ["ES-DE", "Éden"]},
        }}
        message = updates.state_message(state)
        self.assertTrue(message.startswith(b"state:"))
        self.assertEqual(updates.parse_state_message(message), state)
        # Un message d'une autre nature, tronqué ou d'une autre version n'est
        # pas un état : le tray reprend ses messages connus, il ne casse pas.
        self.assertIsNone(updates.parse_state_message(b"updates"))
        self.assertIsNone(updates.parse_state_message(b"reload"))
        self.assertIsNone(updates.parse_state_message(b"state:"))
        self.assertIsNone(updates.parse_state_message(b"state:{pas du json"))
        self.assertIsNone(updates.parse_state_message(b"state:[1, 2]"))
        self.assertIsNone(updates.parse_state_message(b"state:{}"))
        self.assertIsNone(updates.parse_state_message(b""))
        # L'état d'une machine hors ligne reste un état valide (domaines vides).
        offline = {"online": False, "domains": {}}
        self.assertEqual(
            updates.parse_state_message(updates.state_message(offline)),
            offline,
        )

    def test_apps_domain(self):
        # La même fabrique sert le poste local (`pending_updates`) et un poste
        # distant (`pending_for` sur les états reçus par SSH).
        self.assertEqual(
            updates.apps_domain(["esde"], False, 3),
            {"state": "updates", "apps": ["ES-DE"]},
        )
        self.assertEqual(
            updates.apps_domain([], False, 3), {"state": "uptodate", "apps": []}
        )
        self.assertEqual(
            updates.apps_domain([], True, 2), {"state": "unknown", "apps": []}
        )
        self.assertEqual(
            updates.apps_domain([], True, 0),
            {"state": "unavailable", "apps": []},
        )
        state = {"online": True, "domains": {
            "apps": {"state": "updates", "apps": ["Eden", "ES-DE"]},
            "image": {"state": "uptodate"},
        }}
        self.assertEqual(
            updates.domain_count("apps", state["domains"]["apps"]), 2
        )
        self.assertIn(
            "Applications : Eden, ES-DE",
            "\n".join(updates.summary_lines(state)),
        )
        self.assertIn("2 mises à jour", updates.compact_summary(state))
        # Aucune application installée : domaine indisponible, pas « à jour ».
        unavailable = {"state": "unavailable", "apps": []}
        self.assertEqual(updates.domain_count("apps", unavailable), 0)
        self.assertIn(
            "Applications : absent de ce poste",
            "\n".join(updates.summary_lines(
                {"online": True, "domains": {"apps": unavailable}}
            )),
        )


class RemoteUpdatesTest(unittest.TestCase):
    """Sonde d'un poste distant : script balisé et analyse côté manager."""

    def sample(self, staged_version: str = "") -> str:
        deployments = [
            {"booted": True, "version": "20261106", "timestamp": 0},
        ]
        if staged_version:
            deployments.append(
                {"booted": False, "staged": True, "version": staged_version}
            )
        return "\n".join([
            "::gablue:image",
            json.dumps({"deployments": deployments}),
            "::gablue:image-check",
            "Update available for: ghcr.io/elgabo86/gablue-main:latest",
            "rc=0",
            "::gablue:flatpak",
            "org.kde.krita",
            "::gablue:brew",
            "nobrew",
            "::gablue:gwine",
            "component|runner|1.2|1.3",
            "gwine-check: updates",
            "rc=10",
        ])

    def test_parse_check(self):
        state = remote_updates.parse_check(self.sample(), now=100.0)
        self.assertEqual(state["checked_at"], 100.0)
        self.assertTrue(state["online"])
        domains = state["domains"]
        self.assertEqual(domains["image"]["state"], "available")
        self.assertFalse(domains["image"]["staged"])
        self.assertEqual(
            domains["flatpak"],
            {"state": "updates", "apps": ["org.kde.krita"]},
        )
        # Homebrew absent du poste : « indisponible », pas « à jour ».
        self.assertEqual(domains["brew"]["state"], "unavailable")
        self.assertEqual(domains["gwine"]["state"], "updates")
        self.assertEqual(updates.total_updates(state), 3)

    def test_staged_image_wins(self):
        state = remote_updates.parse_check(
            self.sample("20261107"), now=1.0
        )
        image = state["domains"]["image"]
        self.assertEqual(image["state"], "available")
        self.assertTrue(image["staged"])
        self.assertEqual(image["staged_version"], "20261107")

    def test_update_chain(self):
        chain = remote_updates.update_chain()
        for command in (
            "bootc upgrade", "flatpak update --system -y",
            "gwine --init", "brew upgrade",
        ):
            self.assertIn(command, chain)
        # Gablue n'installe qu'en système : aucune mise à jour du compte.
        self.assertNotIn("--user", chain)
        # Sans application en retard, aucune section applications.
        self.assertNotIn("Applications", chain)

    def test_update_chain_apps(self):
        states = {
            "esde": {"script": "/usr/bin/esde-install",
                     "options": {"--yes", "--config-only"}},
            "eden": {"script": "/usr/bin/eden-install", "options": set()},
            "xenia": {"script": "", "options": set()},
        }
        chain = remote_updates.update_chain(["esde", "eden", "xenia"], states)
        self.assertIn("== Applications ==", chain)
        self.assertIn("/usr/bin/esde-install --yes || rc=1", chain)
        self.assertIn("/usr/bin/eden-install || rc=1", chain)
        self.assertIn("Xenia Edge : script manquant, ignoré", chain)
        # 3 étapes + 2 applications + `brew update` + `brew upgrade`.
        self.assertEqual(chain.count("|| rc=1"), 7)
        # Un script qui ne connaît pas `--yes` ouvrirait ses boîtes kdialog :
        # ce n'est pas une mise à jour, c'est une attente infinie.
        old = {"esde": {"script": "/usr/bin/esde-install", "options": set()}}
        chain = remote_updates.update_chain(["esde"], old)
        self.assertIn("antérieure à --yes, ignoré", chain)
        self.assertNotIn("esde-install --yes", chain)

    def test_apps_chain_alone(self):
        states = {"esde": {"script": "/usr/bin/esde-install",
                           "options": {"--yes"}}}
        chain = remote_updates.apps_chain(["esde"], states)
        self.assertTrue(chain.startswith("rc=0\n"))
        self.assertIn("esde-install --yes || rc=1", chain)
        self.assertIn("exit $rc", chain)
        self.assertNotIn("bootc", chain)
        self.assertEqual(remote_updates.apps_chain([], {}), "")

    def test_check_script_keeps_apps_sections_apart(self):
        # Les sections `##` des AppImages ne doivent pas tomber dans la
        # dernière section `::gablue:` (le gwine), sinon son analyse est fausse.
        text = (
            "::gablue:gwine\n"
            "rc=0\n"
            "::gablue:apps\n"
            "##gearlever\n"
            "yes\n"
            "##images\n"
            "esde.appimage\n"
            "##scripts\n"
            "esde-install\t/usr/bin/esde-install\t --yes\n"
        )
        parsed = remote_updates.sections(text)
        self.assertEqual(parsed["gwine"], ["rc=0"])
        states, gearlever = apps.remote_states(text)
        self.assertTrue(gearlever)
        self.assertTrue(states["esde"]["installed"])
        self.assertEqual(states["esde"]["options"], {"--yes"})


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

        # Avancement mesuré (téléchargement du moteur d'AppImages) : le
        # libellé suit la valeur, la barre est déterministe.
        measured = local.parse_gablue_progress(
            "::gablue:percent 42.3 Téléchargement de lichii — 56,7 / 134,2 Mio"
        )
        self.assertTrue(measured["hide"])
        self.assertAlmostEqual(measured["percent"], 42.3)
        self.assertEqual(
            measured["label"],
            "Téléchargement de lichii — 56,7 / 134,2 Mio",
        )
        # Borné à 100, jamais d'exception sur une valeur illisible.
        self.assertEqual(
            local.parse_gablue_progress("::gablue:percent 250 x")["percent"],
            100.0,
        )
        self.assertEqual(local.parse_gablue_progress("::gablue:percent ?"), {})
        self.assertEqual(
            local.parse_gablue_progress("::gablue:percent 10")["label"],
            "Téléchargement…",
        )

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

    def test_gwine_binary_prefers_image(self):
        # Le binaire de l'image gagne sur un gwine personnel du PATH : un
        # ~/.local/bin/gwine d'une version antérieure ignore `--check` et
        # faisait répondre « vérification impossible » (vécu 09/11/2026).
        import unittest.mock as mock
        with mock.patch.object(Path, "exists", return_value=True), \
                mock.patch.object(local.shutil, "which",
                                  return_value="/home/x/.local/bin/gwine"):
            self.assertEqual(local.gwine_binary(), "/usr/bin/gwine")
        # Hors image : le PATH, puis le nom nu (résolution par le shell).
        with mock.patch.object(Path, "exists", return_value=False), \
                mock.patch.object(local.shutil, "which",
                                  return_value="/home/x/.local/bin/gwine"):
            self.assertEqual(
                local.gwine_binary(), "/home/x/.local/bin/gwine"
            )
        with mock.patch.object(Path, "exists", return_value=False), \
                mock.patch.object(local.shutil, "which", return_value=None):
            self.assertEqual(local.gwine_binary(), "gwine")


class AppsTest(unittest.TestCase):
    by_key = {app.key: app for app in apps.APPS}

    def test_find_appimage(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            (directory / "eden.appimage").touch()
            (directory / "esde.appimage_3.4.0.OLD").touch()
            (directory / "xenia_edge.appimage").touch()
            (directory / "lichii.appimage").touch()
            self.assertEqual(
                apps.find_appimage(self.by_key["eden"], directory).name,
                "eden.appimage",
            )
            self.assertEqual(
                apps.find_appimage(self.by_key["lichii"], directory).name,
                "lichii.appimage",
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

    def test_app_update_verdicts(self):
        states = {
            "esde": {"installed": True, "mtime": 100.0, "version": "3.4.0",
                     "cli": ""},
            "eden": {"installed": True, "mtime": 100.0, "version": "0.0.1",
                     "cli": ""},
            "xenia": {"installed": True, "mtime": 100.0, "version": "",
                      "cli": ""},
            "azahar": {"installed": False, "mtime": 0.0, "version": "",
                       "cli": ""},
        }
        infos = {
            "esde": {"version": "3.4.0"},
            "eden": {"version": "0.0.2"},
            "xenia": {},                 # flux muet : verdict indéterminé
        }
        pending, unknown = apps.app_update_verdicts(states, infos)
        self.assertEqual(pending, ["eden"])
        self.assertTrue(unknown)
        # Rien d'installé : ni mise à jour, ni doute.
        self.assertEqual(
            apps.app_update_verdicts(
                {"azahar": {"installed": False, "mtime": 0.0, "cli": ""}}, {}
            ),
            ([], False),
        )

    def test_update_script(self):
        script = apps.update_script(["eden", "esde"])
        lines = script.strip().splitlines()
        # `set -e` : une application en échec arrête la suite.
        self.assertEqual(lines[0], "set -e")
        self.assertIn("::gablue:step 1 2 ES-DE", lines[1])
        # Le marqueur `busy` fait balayer la barre pendant le téléchargement,
        # qui n'a aucun pourcentage à montrer.
        self.assertIn("::gablue:busy", lines[2])
        self.assertIn("(1/2)", lines[2])
        self.assertTrue(lines[3].endswith("esde-install --yes"))
        self.assertIn("::gablue:step 2 2 Eden", lines[4])
        self.assertIn("(2/2)", lines[5])
        self.assertTrue(lines[6].endswith("eden-install"))
        self.assertEqual(apps.update_script([]), "")

    def test_names_for(self):
        # Ordre du catalogue, pas celui des clés reçues : la confirmation et
        # la fenêtre de tâche listent toujours la même chose.
        self.assertEqual(
            apps.names_for({"eden", "esde"}), ["ES-DE", "Eden"],
        )
        self.assertEqual(apps.names_for(("inconnue",)), [])

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

    def test_catalog(self):
        """Catalogue TOML partagé avec le moteur : ordre, champs, groupes."""
        self.assertEqual(
            [app.key for app in apps.APPS],
            ["esde", "azahar", "eden", "xenia", "lichii", "opencode"],
        )
        self.assertEqual(
            apps.GROUPS,
            (("emulateur", "Émulateurs"), ("outil", "Outils")),
        )
        self.assertEqual(self.by_key["esde"].install_args, ("--yes",))
        self.assertEqual(self.by_key["opencode"].cli, "opencode")
        self.assertEqual(self.by_key["opencode"].remover, "opencode2-remove")
        self.assertEqual(self.by_key["eden"].pattern, "*eden*.appimage")
        self.assertEqual(self.by_key["lichii"].script, "lichii-install")

    def test_latest_info_asks_engine(self):
        """La dernière version vient du moteur, pas d'un flux relu ici.

        Deux résolutions concurrentes se contredisaient : le flux Atom du
        manager ignore les pré-releases et proposait une mise à jour que le
        moteur (`/releases/latest`) refusait d'installer.
        """
        from unittest import mock

        payload = (
            '{"key": "lichii", "tag": "build-125", "version": "build-125", '
            '"name": "x", "url": "y", "published": 1794254796.0}'
        )
        done = mock.Mock(returncode=0, stdout=payload)
        with mock.patch.object(apps, "APPS_ENGINE", __file__), \
                mock.patch.object(
                    apps.subprocess, "run", return_value=done
                ) as run:
            info = apps.latest_info(self.by_key["lichii"])
        self.assertEqual(info["version"], "build-125")
        self.assertEqual(info["published"], 1794254796.0)
        self.assertEqual(
            run.call_args[0][0][1:], ["probe", "lichii", "--json"],
        )

    def test_latest_info_without_engine(self):
        """Moteur absent (image antérieure) : verdict indéterminé, pas d'erreur."""
        from unittest import mock

        with mock.patch.object(
            apps, "APPS_ENGINE", "/absent/gablue-apps"
        ):
            self.assertEqual(apps.latest_info(self.by_key["eden"]), {})

    def test_latest_info_engine_failure(self):
        """Moteur en échec ou sortie illisible : {} (jamais « à jour »)."""
        from unittest import mock

        for done in (
            mock.Mock(returncode=1, stdout=""),
            mock.Mock(returncode=0, stdout="pas du JSON"),
            mock.Mock(returncode=0, stdout="[]"),
        ):
            with mock.patch.object(apps, "APPS_ENGINE", __file__), \
                    mock.patch.object(apps.subprocess, "run", return_value=done):
                self.assertEqual(apps.latest_info(self.by_key["eden"]), {})

    def test_version_tuple(self):
        self.assertEqual(apps.version_tuple("v0.2.1"), (0, 2, 1))
        self.assertEqual(apps.version_tuple("azahar 2126.2-A"), (2126, 2))
        self.assertEqual(apps.version_tuple(""), ())
        # GearLever rapporte le X-AppImage-Version complet : même build.
        self.assertFalse(apps.update_available(
            {"version": "build-117-89440b2"}, {"version": "build-117"},
        ))
        self.assertTrue(apps.update_available(
            {"version": "build-116-92f0212"}, {"version": "build-117"},
        ))

    def test_update_available(self):
        """Version si comparable, sinon date ; jamais « à jour » au doute."""
        self.assertTrue(apps.update_available(
            {"version": "2.1.4"}, {"version": "2.2.6"},
        ))
        self.assertFalse(apps.update_available(
            {"version": "2.2.6"}, {"version": "2.2.6"},
        ))
        # OpenCode sans version AppImage : la CLI fait foi.
        self.assertFalse(apps.update_available(
            {"version": "", "cli": "2.2.6"}, {"version": "2.2.6"},
        ))
        # Tag non numérique (hachage) → date de publication.
        self.assertTrue(apps.update_available(
            {"version": "", "mtime": 1000.0},
            {"version": "9809e6f", "published": 2000.0},
        ))
        self.assertFalse(apps.update_available(
            {"version": "", "mtime": 1000.0},
            {"version": "9809e6f", "published": 500.0},
        ))
        self.assertIsNone(apps.update_available({"version": "1.0"}, {}))
        self.assertIsNone(apps.update_available(
            {"version": "", "mtime": 0}, {"version": "", "published": 0},
        ))

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
            "##mtimes\n"
            "esde.appimage\t1767225600\n"
            "eden.appimage\t1767225600\n"
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
        self.assertEqual(states["esde"]["mtime"], 1767225600.0)
        self.assertEqual(states["xenia"]["mtime"], 0.0)
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


class KeyReadyTest(unittest.TestCase):
    """« Clé installée » exige la clé acceptée ET l'empreinte épinglée.

    Le pré-scan travaille sur un known_hosts jetable : `authenticated` seul
    laissait ajouter un poste « clé installée » sans jamais épingler
    l'empreinte, ce qui affichait « Clé d'hôte inconnue » (vécu 08/11/2026).
    """

    def test_accepted_and_pinned(self):
        self.assertTrue(ssh.key_ready({"authenticated": True, "state": "known"}))

    def test_accepted_but_unpinned(self):
        self.assertFalse(ssh.key_ready({"authenticated": True, "state": "unknown"}))

    def test_refused(self):
        self.assertFalse(ssh.key_ready({"authenticated": False, "state": "known"}))
        self.assertFalse(ssh.key_ready({}))
        self.assertFalse(ssh.key_ready(None))


class InstallKeyOrderTest(unittest.TestCase):
    """L'ordre clé/empreinte : l'épinglage passe AVANT le verdict de succès."""

    host = {"user": "gab", "host": "192.168.1.113", "port": 22,
            "addresses": [], "active": ""}

    def _install(self, info, password="", trust=True):
        calls = {"pin": 0, "copy": 0, "probe": 0}
        saved = (ssh.key_status, ssh.ensure_local_key, ssh.trust_host_key,
                 ssh.probe_key, ssh._run_with_password)
        ssh.key_status = lambda _host: info
        ssh.ensure_local_key = lambda: (True, "")
        ssh.trust_host_key = lambda _raw, _host: (calls.__setitem__(
            "pin", calls["pin"] + 1) or True)
        ssh.probe_key = lambda _host: (calls.__setitem__(
            "probe", calls["probe"] + 1) or (True, ""))
        ssh._run_with_password = lambda *_args, **_kwargs: (
            calls.__setitem__("copy", calls["copy"] + 1) or (0, ""))
        try:
            ok, message = ssh.install_key(
                self.host, password, lambda *_a: None, None,
                lambda _prints: trust, info,
            )
        finally:
            (ssh.key_status, ssh.ensure_local_key, ssh.trust_host_key,
             ssh.probe_key, ssh._run_with_password) = saved
        return ok, message, calls

    def test_pinned_and_accepted_needs_nothing(self):
        ok, _message, calls = self._install(
            {"authenticated": True, "state": "known", "raw": "",
             "fingerprints": [], "stored": [], "error": ""}
        )
        self.assertTrue(ok)
        self.assertEqual(calls, {"pin": 0, "copy": 0, "probe": 0})

    def test_accepted_but_unpinned_pins_without_password(self):
        ok, _message, calls = self._install(
            {"authenticated": True, "state": "unknown", "raw": "",
             "fingerprints": [], "stored": [], "error": ""}
        )
        self.assertTrue(ok)
        self.assertEqual(calls["pin"], 1)
        self.assertEqual(calls["copy"], 0)

    def test_accepted_but_unpinned_needs_trust(self):
        ok, message, calls = self._install(
            {"authenticated": True, "state": "unknown", "raw": "",
             "fingerprints": [], "stored": [], "error": ""},
            trust=False,
        )
        self.assertFalse(ok)
        self.assertIn("empreinte refusée", message)
        self.assertEqual(calls["pin"], 0)
        self.assertEqual(calls["copy"], 0)

    def test_changed_fingerprint_is_refused(self):
        ok, message, calls = self._install(
            {"authenticated": True, "state": "changed", "raw": "",
             "fingerprints": [], "stored": [], "error": ""}
        )
        self.assertFalse(ok)
        self.assertIn("changé", message)
        self.assertEqual(calls["pin"], 0)
        self.assertEqual(calls["copy"], 0)

    def test_refused_key_copies_with_password(self):
        ok, _message, calls = self._install(
            {"authenticated": False, "state": "unknown", "raw": "",
             "fingerprints": [], "stored": [], "error": ""},
            password="secret",
        )
        self.assertTrue(ok)
        self.assertEqual(calls["pin"], 1)
        self.assertEqual(calls["copy"], 1)
        self.assertEqual(calls["probe"], 1)


class AdoptAddressKeyTest(unittest.TestCase):
    """Une adresse de secours jamais épinglée hérite de la clé déjà connue.

    `sucina` n'avait son empreinte que sous son nom : dès que le mDNS ne
    résolvait plus, la sonde tombait sur `100.64.0.2`, que
    `StrictHostKeyChecking=yes` refusait — carte « Clé d'hôte inconnue » pour
    une machine parfaitement connue (vécu 13/11/2026).
    """

    host = {
        "user": "gab", "host": "sucina", "port": 22, "active": "",
        "addresses": [{"address": "100.64.0.2", "kind": "tailscale"}],
    }

    def _adopt(self, stored, raw, scanned_stored=None):
        calls = {"scan": 0, "pin": 0}
        saved = (ssh.scan_host_keys, ssh._stored_fingerprints,
                 ssh.host_key_state, ssh.trust_host_key)
        ssh.scan_host_keys = lambda _host, _address="": (
            calls.__setitem__("scan", calls["scan"] + 1) or (raw, True, ""))
        ssh._stored_fingerprints = lambda _address, _port: list(stored)
        ssh.host_key_state = lambda _host, _raw: (
            "known" if scanned_stored is None else scanned_stored[0],
            scanned_stored[1] if scanned_stored else ["k"],
            scanned_stored[2] if scanned_stored else ["k"],
        )
        ssh.trust_host_key = lambda _raw, _host: (
            calls.__setitem__("pin", calls["pin"] + 1) or True)
        try:
            ok = ssh.adopt_address_key(self.host, "100.64.0.2")
        finally:
            (ssh.scan_host_keys, ssh._stored_fingerprints,
             ssh.host_key_state, ssh.trust_host_key) = saved
        return ok, calls

    def test_without_known_key_nothing_is_scanned(self):
        ok, calls = self._adopt([], "100.64.0.2 ssh-ed25519 AAAA\n")
        self.assertFalse(ok)
        self.assertEqual(calls, {"scan": 0, "pin": 0})

    def test_same_key_is_pinned(self):
        ok, calls = self._adopt(
            ["ssh-ed25519 AAAA"], "100.64.0.2 ssh-ed25519 AAAA\n")
        self.assertTrue(ok)
        self.assertEqual(calls, {"scan": 1, "pin": 1})

    def test_different_key_is_never_written(self):
        ok, calls = self._adopt(
            ["ssh-ed25519 AAAA"], "100.64.0.2 ssh-ed25519 BBBB\n",
            scanned_stored=("changed", ["ssh-ed25519 BBBB"],
                            ["ssh-ed25519 AAAA"]),
        )
        self.assertFalse(ok)
        self.assertEqual(calls, {"scan": 1, "pin": 0})

    def test_mute_scan_changes_nothing(self):
        ok, calls = self._adopt(["ssh-ed25519 AAAA"], "")
        self.assertFalse(ok)
        self.assertEqual(calls, {"scan": 1, "pin": 0})


class CheckHostAdoptionTest(unittest.TestCase):
    """La sonde épingle l'adresse de secours et resonde au lieu d'échouer."""

    host = {
        "user": "gab", "host": "sucina", "port": 22, "active": "",
        "addresses": [{"address": "100.64.0.2", "kind": "tailscale"}],
    }

    def test_hostkey_on_fallback_address_ends_online(self):
        probed = []

        def fake_probe(_host, address, _stop):
            probed.append(address)
            if address == "sucina":
                return "dns", "nom introuvable", 0.0
            if probed.count("100.64.0.2") == 1:
                return "hostkey", "empreinte inconnue ou changée", 0.0
            return "online", "", 12.0

        saved = (ssh._probe_address, ssh.adopt_address_key,
                 ssh.PRIVATE_KEY, ssh.PUBLIC_KEY)
        ssh._probe_address = fake_probe
        ssh.adopt_address_key = lambda _host, _address: True
        ssh.PRIVATE_KEY = ROOT  # n'importe quel chemin existant
        ssh.PUBLIC_KEY = ROOT
        try:
            state, _detail, latency, address = ssh.check_host(
                self.host, __import__("threading").Event())
        finally:
            ssh._probe_address, ssh.adopt_address_key = saved[0], saved[1]
            ssh.PRIVATE_KEY, ssh.PUBLIC_KEY = saved[2], saved[3]
        self.assertEqual(state, "online")
        self.assertEqual(address, "100.64.0.2")
        self.assertEqual(probed, ["sucina", "100.64.0.2", "100.64.0.2"])

    def test_unadoptable_key_still_reports_hostkey(self):
        saved = (ssh._probe_address, ssh.adopt_address_key,
                 ssh.PRIVATE_KEY, ssh.PUBLIC_KEY)
        ssh._probe_address = lambda _host, _address, _stop: (
            "hostkey", "empreinte inconnue ou changée", 0.0)
        ssh.adopt_address_key = lambda _host, _address: False
        ssh.PRIVATE_KEY = ROOT
        ssh.PUBLIC_KEY = ROOT
        try:
            state, _detail, _latency, address = ssh.check_host(
                self.host, __import__("threading").Event())
        finally:
            ssh._probe_address, ssh.adopt_address_key = saved[0], saved[1]
            ssh.PRIVATE_KEY, ssh.PUBLIC_KEY = saved[2], saved[3]
        self.assertEqual(state, "hostkey")
        self.assertEqual(address, "sucina")


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


class RemoteHardwareTest(unittest.TestCase):
    DF = (
        "/dev/nvme0n1p3 1000000000 500000000 400000000 60% /var/home\n"
        "/dev/nvme0n1p3 1000000000 500000000 400000000 60% /var\n"
        "/dev/nvme0n1p2 200000000 100000000 80000000 60% /boot\n"
        "/dev/nvme0n1p1 100000000 50000000 40000000 60% /boot/efi\n"
        "/dev/sda1 2000000000 1000000000 900000000 50% /var/hdd1"
    )

    def test_df_skips_boot_partitions(self):
        rows = local.parse_df(self.DF, "/var/home/gab")
        mounts = [row["mount"] for row in rows]
        self.assertEqual(mounts, ["/var/home", "/var/hdd1"])
        labelled = local.disk_rows_from_df(self.DF, "/var/home/gab")
        self.assertEqual(labelled[0]["label"], "Dossier personnel")

    def test_disk_totals(self):
        rows = [
            {"total": 4 * 1024 ** 4, "free": 1024 ** 4},        # 4 Tio, 1 libre
            {"total": 2 * 1024 ** 4, "free": 512 * 1024 ** 3},  # 2 Tio, 0,5 libre
        ]
        self.assertEqual(local.format_disk_totals(rows),
                         "Total : 1,5 Tio libres sur 6 Tio   75 % utilisé")
        # Un seul disque : sa ligne dit déjà tout, pas de synthèse.
        self.assertEqual(local.format_disk_totals(rows[:1]), "")
        self.assertEqual(local.format_disk_totals([]), "")
        self.assertEqual(local.format_disk_totals([{"total": 0}]), "")

    def test_single_module_label(self):
        text = (
            "::gablue:mem\n"
            "MemTotal:       15300000 kB\n"
            "MemAvailable:    6100000 kB\n"
            "::gablue:modules\n"
            '{"modules": [{"size_gib": 16, "type": "DDR4", "speed": 2133,'
            ' "manufacturer": "Kingston"}], "disks": [], "nvm": {}}\n'
        )
        values = remote_hardware.parse_hardware(text, now=1762503600)
        self.assertEqual(
            values["modules"], "1 × 16 Gio · Kingston · DDR4 · 2133 MT/s"
        )
        self.assertEqual(values["modules_label"], "Barrette")
        # La mémoire installée accompagne la mémoire utilisable, y compris
        # sur un poste distant (même analyse que le poste local).
        self.assertEqual(
            values["memory"],
            "8,8 Gio utilisés sur 14,6 Gio (16 Gio installés) · 60 %",
        )

    def test_boot_time_from_proc_stat(self):
        text = (
            "::gablue:os\npretty=Gablue 45\nhome=/var/home/gab\n"
            "::gablue:df\n" + self.DF + "\n"
            "::gablue:boot\nbtime 1762500000\n"
        )
        values = remote_hardware.parse_hardware(text, now=1762503600)
        self.assertEqual(values["uptime"], "depuis 1 h 00")
        self.assertNotIn("efi", values["disks"])
        self.assertNotIn("boot :", values["disks"])




class ImageDetailTest(unittest.TestCase):
    def test_staged_shows_running_and_pending(self):
        text = updates.image_detail({
            "version": "20261106-10h51",
            "staged": True,
            "staged_version": "20261107-16h59",
            "days": 2,
        })
        self.assertEqual(
            text, "actuelle 20261106-10h51 · en attente 20261107-16h59"
        )

    def test_running_only_shows_age(self):
        text = updates.image_detail({"version": "20261106-10h51", "days": 0})
        self.assertEqual(text, "actuelle 20261106-10h51 · déployée aujourd'hui")

    def test_staged_without_version(self):
        text = updates.image_detail({"staged": True, "version": "20261106"})
        self.assertEqual(text, "actuelle 20261106 · mise à jour en attente")



if __name__ == "__main__":
    unittest.main()
