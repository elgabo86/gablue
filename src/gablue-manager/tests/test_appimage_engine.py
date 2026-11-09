"""Tests unitaires du moteur d'installation des AppImages (`gablue-apps`).

Le moteur n'est pas un fichier `.py` (c'est `/usr/libexec/gablue-apps`), donc il
est chargé par chemin. Rien ici ne touche au réseau ni au vrai `~/AppImages` :
les flux et l'API Gitea sont simulés, `HOME` est un bac à sable.

    PYTHONPATH=src/gablue-manager python3 -m unittest discover \
        -s src/gablue-manager/tests -v
"""

import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
import platform
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
ENGINE_PATH = ROOT / "files/system/all/usr/libexec/gablue-apps"


def _load_engine():
    loader = importlib.machinery.SourceFileLoader(
        "gablue_apps", str(ENGINE_PATH)
    )
    spec = importlib.util.spec_from_loader("gablue_apps", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


engine = _load_engine()


ATOM_AZAHAR = (
    "<feed><title>Release notes</title>"
    "<entry><title>AzaharPlus 2126.2-A</title>"
    "<updated>2026-11-01T12:00:00Z</updated>"
    '<link rel="alternate" type="text/html" '
    'href="https://github.com/AzaharPlus/AzaharPlus/releases/tag/'
    'AZAHAR_PLUS_2126_2_A"/></entry></feed>'
)
ATOM_MIXED = (
    "<feed>"
    "<entry><title>Eglantine nx2 build 65</title>"
    '<link rel="alternate" href="https://github.com/zortos293/Lichii/'
    'releases/tag/build-65"/></entry>'
    "<entry><title>Lichii nx2 build 125</title>"
    '<link rel="alternate" href="https://github.com/zortos293/Lichii/'
    'releases/tag/build-125"/></entry></feed>'
)
ATOM_GITLAB = (
    "<feed><entry><title>ES-DE 3.5.0</title>"
    "<updated>2026-11-02T00:00:00Z</updated>"
    '<link href="https://gitlab.com/es-de/emulationstation-de/'
    '-/releases/v3.5.0"/></entry></feed>'
)
# Page telle que GitHub la sert : AppImage ET zsync du même nom, le motif
# ancré doit choisir l'AppImage (sinon on télécharge 8 octets de zsync).
EXPANDED_EDEN = (
    '<a href="/eden-emu/eden/releases/download/v0.2.1/'
    'Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage.zsync">zsync</a>'
    '<a href="/eden-emu/eden/releases/download/v0.2.1/'
    'Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage">AppImage</a>'
    '<a href="/eden-emu/eden/releases/download/v0.2.1/'
    'Eden-Android-v0.2.1-standard.apk">apk</a>'
)


@contextlib.contextmanager
def sandbox(home: Path):
    """Bac à sable complet : `Path.home()` ET `$HOME` des sous-processus.

    Patcher `Path.home()` seul laissait un faux `flatpak` écrire dans le
    vrai `~/AppImages` (vécu 09/11/2026 : une AppImage de 36 octets et un
    `.desktop` écrasé dans le dossier personnel).
    """
    with mock.patch.object(engine.Path, "home", return_value=home), \
            mock.patch.dict(os.environ, {"HOME": str(home)}):
        yield


class QuietTestCase(unittest.TestCase):
    """Capture la sortie du moteur : les marqueurs `::gablue:` sont du bruit ici."""

    def setUp(self):
        patcher = mock.patch.object(sys, "stdout", io.StringIO())
        patcher.start()
        self.addCleanup(patcher.stop)


class FakeResponse:
    """Réponse minimale pour `_pump` (en-têtes, code, corps découpé)."""

    def __init__(self, body: bytes, headers=None, status=200, chunk=7):
        self._body = body
        self.headers = dict(headers or {})
        self._status = status
        self._chunk = chunk

    def __enter__(self):
        return self

    def __exit__(self, *exception):
        return False

    def getcode(self):
        return self._status

    def read(self, size=-1):
        piece, self._body = self._body[:self._chunk], self._body[self._chunk:]
        return piece


class CatalogTest(unittest.TestCase):
    def test_catalog_path_and_apps(self):
        catalog = engine.load_catalog()
        self.assertEqual(
            [app["key"] for app in catalog["apps"]],
            ["esde", "azahar", "eden", "xenia", "lichii", "opencode"],
        )
        self.assertEqual(
            catalog["groups"],
            [("emulateur", "Émulateurs"), ("outil", "Outils")],
        )

    def test_lichii_rules(self):
        catalog = engine.load_catalog()
        app = engine.find_app(catalog, "lichii")
        # Le flux du dépôt porte des releases sans préfixe : le tag vient de la
        # redirection de /releases/latest, pas du flux.
        self.assertEqual(app["release"]["kind"], "github-latest")
        self.assertEqual(
            app["release"]["asset"], "lichii-linux-{arch}-pgo.AppImage"
        )
        self.assertEqual(app["desktop_name"], "Lichii")

    def test_unknown_app(self):
        catalog = engine.load_catalog()
        with self.assertRaises(engine.EngineError) as caught:
            engine.find_app(catalog, "inconnue")
        self.assertIn("application inconnue", str(caught.exception))

    def test_missing_catalog(self):
        with self.assertRaises(engine.EngineError):
            engine.load_catalog(Path("/absent/apps.toml"))


class ArchTest(unittest.TestCase):
    def test_mapping(self):
        machine = platform.machine()
        # Eden publie des assets `amd64`, pas `x86_64`.
        self.assertEqual(
            engine._arch_token({"arch": {machine: "amd64"}}), "amd64"
        )
        # Sans table, le nom de la machine fait foi.
        self.assertEqual(engine._arch_token({}), machine)


class AtomTest(unittest.TestCase):
    def test_github_tag(self):
        with mock.patch.object(engine, "fetch_text", return_value=ATOM_AZAHAR):
            self.assertEqual(
                engine.tag_from_atom("AzaharPlus/AzaharPlus"),
                "AZAHAR_PLUS_2126_2_A",
            )

    def test_version_prefers_dotted_title(self):
        """Le titre porte la version lisible (« 2126.2 »), le tag est bricolé."""
        with mock.patch.object(engine, "fetch_text", return_value=ATOM_AZAHAR):
            info = engine.atom_latest(
                "https://github.com/AzaharPlus/AzaharPlus/releases.atom"
            )
        self.assertEqual(info["version"], "2126.2")
        self.assertAlmostEqual(info["published"], 1793534400.0, delta=1.0)

    def test_title_filter(self):
        with mock.patch.object(engine, "fetch_text", return_value=ATOM_MIXED):
            self.assertEqual(
                engine.tag_from_atom("zortos293/Lichii", r"^Lichii\b"),
                "build-125",
            )
            # Sans filtre, l'entrée Eglantine du flux gagnerait.
            self.assertEqual(
                engine.tag_from_atom("zortos293/Lichii"), "build-65"
            )

    def test_gitlab_tag(self):
        # Même analyse pour un flux GitLab (`/-/releases/<tag>`).
        with mock.patch.object(engine, "fetch_text", return_value=ATOM_GITLAB):
            catalog = engine.load_catalog()
            info = engine.resolve(engine.find_app(catalog, "esde"))
        self.assertEqual(info["tag"], "v3.5.0")
        self.assertEqual(info["version"], "3.5.0")

    def test_empty_feed(self):
        with mock.patch.object(engine, "fetch_text", return_value="<feed/>"):
            self.assertEqual(engine.tag_from_atom("x/y"), "")


class LatestRedirectTest(unittest.TestCase):
    def test_tag_from_redirect(self):
        response = mock.Mock()
        response.geturl.return_value = (
            "https://github.com/zortos293/Lichii/releases/tag/build-125"
        )
        response.__enter__ = lambda self: self
        response.__exit__ = lambda *args: False
        with mock.patch.object(
            engine.urllib.request, "urlopen", return_value=response
        ) as opened:
            tag = engine.tag_from_latest("zortos293/Lichii")
        self.assertEqual(tag, "build-125")
        # `no-store` : sans lui GitHub servait un 504 mis en cache (09/11/2026).
        headers = opened.call_args[0][0].headers
        self.assertEqual(headers.get("Cache-control"), "no-store")
        self.assertEqual(opened.call_args[0][0].method, "HEAD")

    def test_redirect_without_tag(self):
        response = mock.Mock()
        response.geturl.return_value = "https://github.com/x/y/releases"
        response.__enter__ = lambda self: self
        response.__exit__ = lambda *args: False
        with mock.patch.object(
            engine.urllib.request, "urlopen", return_value=response
        ):
            self.assertEqual(engine.tag_from_latest("x/y"), "")


class AssetTest(unittest.TestCase):
    def test_expanded_assets_skip_zsync(self):
        with mock.patch.object(
            engine, "fetch_text", return_value=EXPANDED_EDEN
        ):
            name, url = engine.asset_from_expanded(
                "eden-emu/eden", "v0.2.1",
                r"Eden-Linux-.*-amd64-clang-pgo\.AppImage$",
            )
        self.assertEqual(name, "Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage")
        self.assertTrue(url.startswith("https://github.com/eden-emu/eden/"))

    def test_expanded_assets_no_match(self):
        with mock.patch.object(
            engine, "fetch_text", return_value=EXPANDED_EDEN
        ):
            self.assertEqual(
                engine.asset_from_expanded("e/e", "t", r"absent\.AppImage$"),
                ("", ""),
            )


class ResolveTest(unittest.TestCase):
    """Le catalogue porte les règles : aucune ne doit être fausse."""

    def setUp(self):
        self.catalog = engine.load_catalog()

    def test_azahar(self):
        page = (
            '<a href="/AzaharPlus/AzaharPlus/releases/download/'
            'AZAHAR_PLUS_2126_2_A/azaharplus-2126.2-A-android_coexists.apk">a</a>'
            '<a href="/AzaharPlus/AzaharPlus/releases/download/'
            'AZAHAR_PLUS_2126_2_A/azaharplus-2126.2-A-linux.AppImage">b</a>'
        )
        with mock.patch.object(
            engine, "fetch_text",
            side_effect=lambda url, timeout=0: (
                ATOM_AZAHAR if url.endswith("releases.atom") else page
            ),
        ):
            info = engine.resolve(engine.find_app(self.catalog, "azahar"))
        self.assertEqual(info["tag"], "AZAHAR_PLUS_2126_2_A")
        # Version comparable à ce que GearLever rapporte (2126.2-A).
        self.assertEqual(info["version"], "2126.2")
        self.assertEqual(
            info["name"], "azaharplus-2126.2-A-linux.AppImage"
        )
        self.assertEqual(
            info["url"],
            "https://github.com/AzaharPlus/AzaharPlus/releases/download/"
            "AZAHAR_PLUS_2126_2_A/azaharplus-2126.2-A-linux.AppImage",
        )

    def test_xenia_commit_tag(self):
        feed = (
            "<feed><entry><title>xenia edge</title>"
            "<updated>2026-11-09T12:00:00Z</updated>"
            '<link href="https://github.com/has207/xenia-edge/releases/tag/'
            '32264e9"/></entry></feed>'
        )
        page = (
            '<a href="/has207/xenia-edge/releases/download/32264e9/'
            'xenia_edge_linux.AppImage">x</a>'
            '<a href="/has207/xenia-edge/releases/download/32264e9/'
            'xenia_edge_windows.zip">z</a>'
        )
        with mock.patch.object(
            engine, "fetch_text",
            side_effect=lambda url, timeout=0: (
                feed if url.endswith("releases.atom") else page
            ),
        ):
            info = engine.resolve(engine.find_app(self.catalog, "xenia"))
        self.assertEqual(info["tag"], "32264e9")
        # Tag = hachage : seule la date permet au manager de conclure.
        self.assertEqual(info["version"], "32264e9")
        self.assertIsNotNone(info["published"])
        self.assertEqual(info["name"], "xenia_edge_linux.AppImage")

    def test_eden_gitea(self):
        payload = {
            "tag_name": "v0.2.1",
            "published_at": "2026-11-01T00:00:00Z",
            "assets": [
                {"name": "Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage.zsync",
                 "browser_download_url": "https://e/zsync"},
                {"name": "Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage",
                 "browser_download_url": "https://e/appimage"},
                {"name": "Eden-Android-v0.2.1-standard.apk",
                 "browser_download_url": "https://e/apk"},
            ],
        }
        with mock.patch.object(
            engine, "fetch_text", return_value=json.dumps(payload)
        ):
            info = engine.resolve(engine.find_app(self.catalog, "eden"))
        self.assertEqual(info["version"], "v0.2.1")
        self.assertEqual(
            info["name"], "Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage"
        )
        self.assertEqual(info["url"], "https://e/appimage")
        self.assertAlmostEqual(info["published"], 1793491200.0, delta=1.0)

    def test_eden_without_matching_asset(self):
        payload = {"tag_name": "v1", "assets": [{"name": "autre.zip"}]}
        with mock.patch.object(
            engine, "fetch_text", return_value=json.dumps(payload)
        ):
            with self.assertRaises(engine.EngineError):
                engine.resolve(engine.find_app(self.catalog, "eden"))

    def test_npm(self):
        with mock.patch.object(
            engine, "fetch_text", return_value='{"version": "2.2.6"}'
        ):
            info = engine.resolve(engine.find_app(self.catalog, "opencode"))
        self.assertEqual(info["version"], "2.2.6")
        self.assertEqual(info["url"], "")

    def test_network_failure(self):
        with mock.patch.object(
            engine, "fetch_text",
            side_effect=engine.urllib.error.URLError("hors ligne"),
        ):
            with self.assertRaises(engine.EngineError) as caught:
                engine.resolve(engine.find_app(self.catalog, "azahar"))
        self.assertIn("introuvable", str(caught.exception))


class DownloadTest(QuietTestCase):
    def test_expected_total(self):
        self.assertEqual(
            engine._expected_total(FakeResponse(b"", {"Content-Length": "10"}), 0),
            10,
        )
        self.assertEqual(
            engine._expected_total(
                FakeResponse(b"", {"Content-Range": "bytes 5-9/10"}), 5
            ),
            10,
        )
        self.assertEqual(engine._expected_total(FakeResponse(b""), 0), 0)

    def test_pump_from_scratch(self):
        with tempfile.TemporaryDirectory() as directory:
            part = Path(directory) / "x.AppImage.part"
            reporter = engine.Reporter("test")
            body = b"\x7fELF" + b"a" * 20
            size = engine._pump(
                FakeResponse(body, {"Content-Length": str(len(body))}),
                part, 0, reporter,
            )
            self.assertEqual(size, len(body))
            self.assertEqual(part.read_bytes(), body)

    def test_pump_resume_appends(self):
        with tempfile.TemporaryDirectory() as directory:
            part = Path(directory) / "x.AppImage.part"
            part.write_bytes(b"\x7fELF")
            reporter = engine.Reporter("test")
            size = engine._pump(
                FakeResponse(
                    b"reste",
                    {"Content-Range": f"bytes 4-8/9", "Content-Length": "5"},
                    status=206,
                ),
                part, 4, reporter,
            )
            self.assertEqual(size, 9)
            self.assertEqual(part.read_bytes(), b"\x7fELFreste")

    def test_pump_ignored_range_restarts(self):
        """Serveur qui ignore `Range` (200) : on repart de zéro, sans coller."""
        with tempfile.TemporaryDirectory() as directory:
            part = Path(directory) / "x.AppImage.part"
            part.write_bytes(b"vieux contenu")
            reporter = engine.Reporter("test")
            size = engine._pump(
                FakeResponse(b"neuf", {"Content-Length": "4"}), part, 13,
                reporter,
            )
            self.assertEqual(size, 4)
            self.assertEqual(part.read_bytes(), b"neuf")

    def test_pump_short_transfer(self):
        with tempfile.TemporaryDirectory() as directory:
            part = Path(directory) / "x.AppImage.part"
            with self.assertRaises(engine.DownloadError):
                engine._pump(
                    FakeResponse(b"court", {"Content-Length": "100"}),
                    part, 0, engine.Reporter("test"),
                )

    def test_pump_slow_watchdog(self):
        """Une connexion qui traîne est abandonnée, le partiel est gardé."""
        with tempfile.TemporaryDirectory() as directory:
            part = Path(directory) / "x.AppImage.part"
            calls = {"n": 0}

            class Slow(FakeResponse):
                def read(self, size=-1):
                    calls["n"] += 1
                    return b"x" if calls["n"] < 3 else b""

            clock = iter([0.0, 0.5, 40.0, 41.0, 42.0, 43.0])
            with mock.patch.object(
                engine.time, "monotonic", side_effect=lambda: next(clock)
            ), mock.patch.object(engine, "SLOW_SECONDS", 30.0), \
                    mock.patch.object(engine, "MIN_SPEED", 200_000):
                with self.assertRaises(engine.DownloadError) as caught:
                    engine._pump(Slow(b"", {"Content-Length": "1000"}),
                                 part, 0, engine.Reporter("test"))
            self.assertIn("trop lente", str(caught.exception))
            self.assertTrue(part.exists())

    def test_download_retries_then_succeeds(self):
        with tempfile.TemporaryDirectory() as directory:
            dest = Path(directory) / "app.AppImage"
            body = b"\x7fELF" + b"z" * 10
            calls = {"n": 0}

            def fake_open(url, offset):
                calls["n"] += 1
                if calls["n"] == 1:
                    raise engine.DownloadError("réseau : coupé")
                return FakeResponse(body, {"Content-Length": str(len(body))})

            with mock.patch.object(engine, "_open", side_effect=fake_open), \
                    mock.patch.object(engine.time, "sleep", lambda *_: None):
                size = engine.download("file://x", dest, engine.Reporter("t"))
            self.assertEqual(size, len(body))
            self.assertEqual(dest.read_bytes(), body)
            self.assertFalse(dest.with_name("app.AppImage.part").exists())

    def test_download_gives_up(self):
        with tempfile.TemporaryDirectory() as directory:
            dest = Path(directory) / "app.AppImage"
            with mock.patch.object(
                engine, "_open", side_effect=engine.DownloadError("mort")
            ), mock.patch.object(engine.time, "sleep", lambda *_: None):
                with self.assertRaises(engine.DownloadError) as caught:
                    engine.download("file://x", dest, engine.Reporter("t"))
            self.assertIn("abandon après", str(caught.exception))

    def test_download_reuses_complete_cache(self):
        """Release déjà téléchargée dans son dossier de tag : zéro requête."""
        with tempfile.TemporaryDirectory() as directory:
            dest = Path(directory) / "app.AppImage"
            dest.write_bytes(b"\x7fELF" + b"q" * 12)
            with mock.patch.object(engine, "_open") as opened:
                size = engine.download(
                    "file://x", dest, engine.Reporter("t")
                )
            self.assertEqual(size, 16)
            opened.assert_not_called()


class VerifyTest(unittest.TestCase):
    def test_elf_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ok.AppImage"
            path.write_bytes(b"\x7fELF" + b"0" * 10)
            self.assertEqual(engine.verify_appimage(path), 14)
            self.assertTrue(path.stat().st_mode & 0o100)

    def test_html_rejected_and_removed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "erreur.AppImage"
            path.write_bytes(b"<html>504</html>")
            with self.assertRaises(engine.DownloadError):
                engine.verify_appimage(path)
            self.assertFalse(path.exists())


class ReportTest(unittest.TestCase):
    def test_percent_marker(self):
        stream = io.StringIO()
        with mock.patch.object(sys, "stdout", stream):
            reporter = engine.Reporter("Téléchargement de x")
            reporter.progress(50, 100)
            reporter.progress(100, 100)
        lines = [line for line in stream.getvalue().splitlines() if line]
        self.assertTrue(
            lines[0].startswith(
                "::gablue:percent 50.0 Téléchargement de x — "
            )
        )
        self.assertIn("0,0 / 0,0 Mio", lines[0])
        self.assertTrue(lines[-1].startswith("::gablue:percent 100.0"))

    def test_no_marker_on_terminal(self):
        stream = io.StringIO()

        class Tty(io.StringIO):
            def isatty(self):
                return True

        tty = Tty()
        with mock.patch.object(sys, "stdout", tty):
            reporter = engine.Reporter("gros fichier")
            reporter.busy()
            reporter.progress(10, 100)
            reporter.finish()
        self.assertNotIn("::gablue:", tty.getvalue())
        self.assertIn("gros fichier", tty.getvalue())


class InstallDryRunTest(QuietTestCase):
    def test_dry_run_downloads_and_verifies(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            source = home / "source.AppImage"
            source.write_bytes(b"\x7fELF" + b"b" * 64)
            info = {
                "tag": "build-125",
                "version": "build-125",
                "name": "lichii-linux-x86_64-pgo.AppImage",
                "url": source.as_uri(),
                "published": None,
            }
            catalog = engine.load_catalog()
            app = engine.find_app(catalog, "lichii")
            with mock.patch.object(engine.Path, "home", return_value=home), \
                    mock.patch.object(engine, "resolve", return_value=info), \
                    mock.patch.object(engine, "gearlever_available") as gear:
                code = engine.install(app, dry_run=True)
            self.assertEqual(code, 0)
            # Un dossier de cache par release : une reprise ne peut viser que
            # la même build.
            cached = (home / ".cache/lichii/build-125"
                      / "lichii-linux-x86_64-pgo.AppImage")
            self.assertTrue(cached.is_file())
            self.assertEqual(cached.read_bytes(), source.read_bytes())
            gear.assert_not_called()
            self.assertFalse((home / "AppImages").exists())

    def test_dry_run_completes_partial(self):
        """Un partiel plus court est complété, pas doublé."""
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            source = home / "source.AppImage"
            body = b"\x7fELF" + b"c" * 40
            source.write_bytes(body)
            target = (home / ".cache/lichii/t"
                      / "lichii-linux-x86_64-pgo.AppImage")
            target.parent.mkdir(parents=True)
            target.with_name(target.name + ".part").write_bytes(body[:9])
            info = {"tag": "t", "version": "t", "name": target.name,
                    "url": source.as_uri(), "published": None}
            catalog = engine.load_catalog()
            with mock.patch.object(engine.Path, "home", return_value=home), \
                    mock.patch.object(engine, "resolve", return_value=info):
                engine.install(engine.find_app(catalog, "lichii"),
                               dry_run=True)
            self.assertEqual(target.read_bytes(), body)


class InstallFlowTest(QuietTestCase):
    """Installation complète, avec un `flatpak` factice (GearLever simulé)."""

    SHIM = """#!/bin/bash
# Garde-fou : ce faux flatpak ne doit jamais écrire dans un vrai $HOME.
case "$HOME" in
  /tmp/*|*"/opencode/"*) ;;
  *) echo "shim hors bac à sable : $HOME" >&2; exit 9 ;;
esac
case "$1" in
  info) exit 0 ;;
  run)
    case "$3" in
      --list-installed) exit 0 ;;
      --remove) rm -f "$4"; exit 0 ;;
      --integrate)
        mkdir -p "$HOME/AppImages" "$HOME/.local/share/applications"
        cp "$4" "$HOME/AppImages/$(basename "$4")"
        printf '[Desktop Entry]\\nName=Lichii (build-125)\\nExec=x\\n' \\
            > "$HOME/.local/share/applications/lichii.desktop"
        exit 0 ;;
    esac ;;
esac
exit 1
"""

    def test_install_replaces_old_and_patches_menu(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            bindir = home / "bin"
            shim = bindir / "flatpak"
            shim.parent.mkdir()
            shim.write_text(self.SHIM, encoding="utf-8")
            shim.chmod(0o755)
            # Ancienne version installée : elle doit disparaître.
            (home / "AppImages").mkdir()
            old = home / "AppImages/lichii-old.AppImage"
            old.write_bytes(b"\x7fELF old")
            source = home / "source.AppImage"
            body = b"\x7fELF" + b"n" * 32
            source.write_bytes(body)
            info = {"tag": "build-125", "version": "build-125",
                    "name": "lichii-linux-x86_64-pgo.AppImage",
                    "url": source.as_uri(), "published": None}
            catalog = engine.load_catalog()
            app = engine.find_app(catalog, "lichii")
            with sandbox(home), \
                    mock.patch.object(engine, "resolve", return_value=info), \
                    mock.patch.object(engine, "refresh_menu"), \
                    mock.patch.dict("os.environ",
                                    {"PATH": f"{bindir}:/usr/bin:/bin"}):
                self.assertEqual(engine.install(app), 0)
            self.assertFalse(old.exists())
            installed = home / "AppImages/lichii-linux-x86_64-pgo.AppImage"
            self.assertEqual(installed.read_bytes(), body)
            desktop = home / ".local/share/applications/lichii.desktop"
            self.assertIn("Name=Lichii\n", desktop.read_text(encoding="utf-8"))
            # Le cache ne survit pas à une intégration réussie.
            self.assertFalse((home / ".cache/lichii").exists())

    def test_install_without_gearlever(self):
        """GearLever absent : erreur claire, rien n'est intégré."""
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            bindir = home / "bin"
            bindir.mkdir()
            missing = bindir / "flatpak"
            missing.write_text("#!/bin/bash\nexit 1\n", encoding="utf-8")
            missing.chmod(0o755)
            source = home / "source.AppImage"
            source.write_bytes(b"\x7fELF" + b"n" * 8)
            info = {"tag": "t", "version": "t", "name": "lichii-x.AppImage",
                    "url": source.as_uri(), "published": None}
            catalog = engine.load_catalog()
            with sandbox(home), \
                    mock.patch.object(engine, "resolve", return_value=info), \
                    mock.patch.dict("os.environ",
                                    {"PATH": f"{bindir}:/usr/bin:/bin"}):
                with self.assertRaises(engine.EngineError) as caught:
                    engine.install(engine.find_app(catalog, "lichii"))
            self.assertIn("GearLever", str(caught.exception))
            self.assertFalse((home / "AppImages").exists())


class MenuTest(unittest.TestCase):
    def test_installed_images_pattern(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / "Lichii.AppImage").write_bytes(b"x")
            (folder / "autre.bin").write_bytes(b"x")
            catalog = engine.load_catalog()
            app = engine.find_app(catalog, "lichii")
            with mock.patch.object(engine, "appimages_dir", return_value=folder):
                found = engine.installed_images(app)
            self.assertEqual([path.name for path in found], ["Lichii.AppImage"])

    def test_patch_desktop(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            entry = home / ".local/share/applications/lichii.desktop"
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "[Desktop Entry]\nName=Lichii (build-125)\nExec=x\n",
                encoding="utf-8",
            )
            catalog = engine.load_catalog()
            with mock.patch.object(engine.Path, "home", return_value=home):
                engine.patch_desktop(engine.find_app(catalog, "lichii"))
            self.assertIn(
                "Name=Lichii\n", entry.read_text(encoding="utf-8")
            )

    def test_patch_desktop_drops_version_key(self):
        """Azahar : « AzaharPlus (2126.2-A) » → « Azahar », sans clé de version."""
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            entry = (home / ".local/share/applications"
                     / "azaharplus-2126.2-A.desktop")
            entry.parent.mkdir(parents=True)
            entry.write_text(
                "[Desktop Entry]\nName=AzaharPlus (2126.2-A)\n"
                "X-AppImage-Version=2126.2\nExec=x\n",
                encoding="utf-8",
            )
            catalog = engine.load_catalog()
            with mock.patch.object(engine.Path, "home", return_value=home):
                engine.patch_desktop(engine.find_app(catalog, "azahar"))
            text = entry.read_text(encoding="utf-8")
            self.assertIn("Name=Azahar\n", text)
            self.assertNotIn("X-AppImage-Version", text)
            self.assertIn("Exec=x", text)

    def test_patch_desktop_renames_for_eden(self):
        """Eden : le .desktop nommé d'après la release devient `eden.desktop`."""
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            folder = home / ".local/share/applications"
            folder.mkdir(parents=True)
            entry = folder / "Eden-v0.2.1-amd64.desktop"
            entry.write_text("Name=Eden (v0.2.1)\n", encoding="utf-8")
            catalog = engine.load_catalog()
            with mock.patch.object(engine.Path, "home", return_value=home):
                engine.patch_desktop(engine.find_app(catalog, "eden"))
            self.assertFalse(entry.exists())
            self.assertEqual(
                (folder / "eden.desktop").read_text(encoding="utf-8"),
                "Name=Eden\n",
            )

    def test_patch_desktop_without_entry(self):
        """Pas de .desktop (intégration ratée) : aucune erreur, rien de créé."""
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home / ".local/share/applications").mkdir(parents=True)
            catalog = engine.load_catalog()
            with mock.patch.object(engine.Path, "home", return_value=home):
                engine.patch_desktop(engine.find_app(catalog, "lichii"))
            self.assertEqual(
                list((home / ".local/share/applications").iterdir()), []
            )


class CliTest(unittest.TestCase):
    def test_list(self):
        stream = io.StringIO()
        with mock.patch.object(sys, "stdout", stream):
            self.assertEqual(engine.main(["list"]), 0)
        lines = stream.getvalue().strip().splitlines()
        self.assertEqual(len(lines), 6)
        self.assertTrue(lines[0].startswith("esde\t"))

    def test_probe_json(self):
        info = {"tag": "v1", "version": "v1", "name": "a.AppImage",
                "url": "https://x/a.AppImage", "published": None}
        stream = io.StringIO()
        with mock.patch.object(engine, "resolve", return_value=info), \
                mock.patch.object(sys, "stdout", stream):
            self.assertEqual(engine.main(["probe", "eden", "--json"]), 0)
        data = json.loads(stream.getvalue())
        self.assertEqual(data["key"], "eden")
        self.assertEqual(data["url"], "https://x/a.AppImage")

    def test_install_refused_for_legacy_script(self):
        error = io.StringIO()
        with mock.patch.object(sys, "stderr", error):
            self.assertEqual(engine.main(["install", "esde"]), 1)
        self.assertIn("script de l'image", error.getvalue())

    def test_unknown_key(self):
        error = io.StringIO()
        with mock.patch.object(sys, "stderr", error):
            self.assertEqual(engine.main(["probe", "inexistante"]), 1)
        self.assertIn("application inconnue", error.getvalue())


if __name__ == "__main__":
    unittest.main()
