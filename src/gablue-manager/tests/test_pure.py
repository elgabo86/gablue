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
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from gablue_manager import apps, hosts, local, ssh, util  # noqa: E402


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
            "sudo -n /usr/libexec/gablue-service displaylink.service on\n"
            "sudo -n /usr/libexec/gablue-service docker.service off\n"
            "sudo -n /usr/libexec/gablue-low-latency on\n"
            "sudo -n /usr/libexec/gablue-broadcom-wl off\n"
            "/usr/bin/gablue-atuin on\n",
        )

    def test_system_script_partial(self):
        self.assertEqual(
            local.system_apply_script({"low_latency": False}),
            "sudo -n /usr/libexec/gablue-low-latency off\n",
        )
        # atuin ne passe jamais par sudo : fichiers de l'utilisateur.
        self.assertNotIn(
            "sudo", local.system_apply_script({"atuin": False})
        )

    def test_auto_update_script(self):
        script = local.system_apply_script({"auto_updates": True})
        self.assertEqual(
            script,
            "sudo -n /usr/libexec/gablue-auto-update on\n"
            "systemctl --user enable --now flatpak-user-update.timer "
            "|| echo \"Timer Flatpak utilisateur inchangé (pas de session).\"\n",
        )
        self.assertIn(
            "sudo -n /usr/libexec/gablue-auto-update off",
            local.system_apply_script({"auto_updates": False}),
        )
        self.assertIn(
            "systemctl --user disable --now flatpak-user-update.timer",
            local.system_apply_script({"auto_updates": False}),
        )

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


class GwineTest(unittest.TestCase):
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
