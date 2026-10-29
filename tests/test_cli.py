import contextlib
import hashlib
import http.server
import importlib.machinery
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from functools import partial
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load_cli(path, name):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


class CLITest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cli = load_cli(ROOT / "bin/vibe-memory", "vibe_memory_cli_test")

    def test_origin_validation(self):
        self.assertEqual(
            self.cli.validate_origin("https://vibememory.io/"), "https://vibememory.io"
        )
        self.assertEqual(
            self.cli.validate_origin("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080",
        )
        for invalid in (
            "http://memory.example.com",
            "https://user:secret@example.com",
            "https://example.com/path",
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                self.cli.validate_origin(invalid)

    def test_compact_output_and_templates(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.cli.emit(self.cli.templates("entity"))
        line = output.getvalue()
        self.assertEqual(len(line.splitlines()), 1)
        self.assertEqual(json.loads(line)["entity_type"], "Person")

    def test_legacy_commands_translate_without_appearing_in_help(self):
        self.assertEqual(
            self.cli.normalize_legacy_arguments(["ingest", "document.json"]),
            ["from-json", "document", "document.json"],
        )
        self.assertEqual(
            self.cli.normalize_legacy_arguments(["--pretty", "feature-request", "request-id"]),
            ["--pretty", "feedback-get", "request-id"],
        )
        result = subprocess.run(
            [sys.executable, str(ROOT / "bin/vibe-memory"), "--help"],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("recall", result.stdout)
        self.assertNotIn("request-feature", result.stdout)

    def test_compatibility_launcher_bootstraps_verified_cli(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            public = root / "public"
            installed = root / "installed"
            cli_bytes = (ROOT / "bin/vibe-memory").read_bytes()
            digest = hashlib.sha256(cli_bytes).hexdigest()
            url = "/skills/vibe-memory/cli/" + digest + "/vibe-memory"
            manifest = {
                "schema_version": 1,
                "revision": "not-used-by-launcher",
                "files": {},
                "cli": {"path": "bin/vibe-memory", "sha256": digest, "url": url},
            }
            manifest_path = public / "skills/vibe-memory/manifest.json"
            manifest_path.parent.mkdir(parents=True)
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            cli_path = public / url.removeprefix("/")
            cli_path.parent.mkdir(parents=True)
            cli_path.write_bytes(cli_bytes)
            (installed / "scripts").mkdir(parents=True)
            shutil.copyfile(ROOT / "scripts/memory.py", installed / "scripts/memory.py")

            server = http.server.ThreadingHTTPServer(
                ("127.0.0.1", 0),
                partial(http.server.SimpleHTTPRequestHandler, directory=str(public)),
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                env = {
                    **os.environ,
                    "MEMORY_URL": f"http://127.0.0.1:{server.server_port}",
                }
                result = subprocess.run(
                    [sys.executable, str(installed / "scripts/memory.py"), "--help"],
                    env=env,
                    check=True,
                    capture_output=True,
                    text=True,
                    timeout=10,
                )
                self.assertIn("Vibe Memory CLI", result.stdout)
                downloaded = installed / "bin/vibe-memory"
                self.assertEqual(hashlib.sha256(downloaded.read_bytes()).hexdigest(), digest)
                if os.name != "nt":
                    self.assertTrue(downloaded.stat().st_mode & 0o100)
            finally:
                server.shutdown()
                server.server_close()
                thread.join()


if __name__ == "__main__":
    unittest.main()
