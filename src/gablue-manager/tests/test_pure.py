"""Tests unitaires des fonctions pures de Gablue Manager.

Aucun affichage, aucun privilège : uniquement les calculs (scripts de mise à
jour, analyse des fichiers d'options, formats ssh). Lancer depuis la racine :

    PYTHONPATH=src/gablue-manager python3 -m unittest discover \
        -s src/gablue-manager/tests -v
"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from gablue_manager import local, util  # noqa: E402


class KargsTest(unittest.TestCase):
    def test_choices(self):
        choices = local.kargs_choices({"mitigations=off", "clearcpuid=514"}, True)
        self.assertFalse(choices["mitigations"])
        self.assertTrue(choices["cpuid"])
        self.assertFalse(choices["ertm"])
        self.assertTrue(choices["cpuid_emu"])

    def test_script_groups_kargs(self):
        script = local.kernel_apply_script({"mitigations": False, "ertm": True})
        self.assertEqual(
            script,
            "sudo -n /usr/libexec/gablue-kargs --mitigations off --ertm on\n",
        )

    def test_script_cpuid_alone(self):
        script = local.kernel_apply_script({"cpuid_emu": True})
        self.assertEqual(script, "sudo -n /usr/libexec/gablue-cpuid-emu on\n")

    def test_script_both(self):
        script = local.kernel_apply_script({"cpuid": True, "cpuid_emu": False})
        self.assertIn("gablue-kargs --cpuid on", script)
        self.assertIn("gablue-cpuid-emu off", script)


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

    def test_normalize_mac(self):
        self.assertEqual(util.normalize_mac("aa-bb-cc-dd-ee-ff"),
                         "AA:BB:CC:DD:EE:FF")
        self.assertEqual(util.normalize_mac("pas une mac"), "")

    def test_ssh_opts(self):
        opts = util._ssh_opts(self.host)
        self.assertIn("BatchMode=yes", opts)
        self.assertIn("IdentitiesOnly=yes", opts)
        self.assertIn(str(Path.home() / ".ssh/id_ed25519"), opts)


if __name__ == "__main__":
    unittest.main()
