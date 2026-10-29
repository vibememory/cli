#!/usr/bin/env python3
"""Compatibility launcher for the versioned Vibe Memory CLI."""

import hashlib
import json
import os
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "bin" / "vibe-memory"


def config_path():
    base = os.environ.get("APPDATA") if os.name == "nt" else os.environ.get("XDG_CONFIG_HOME")
    return (Path(base) if base else Path.home() / ".config") / "vibe-memory" / "client.json"


def origin():
    saved = {}
    path = config_path()
    if path.is_file():
        saved = json.loads(path.read_text(encoding="utf-8"))
    base = (os.environ.get("MEMORY_URL") or saved.get("url") or "https://vibememory.io").rstrip("/")
    parsed = urlsplit(base)
    if (
        parsed.scheme not in ("http", "https")
        or not parsed.hostname
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
        or parsed.path
    ):
        raise ValueError("Invalid Vibe Memory service origin")
    if parsed.scheme == "http" and parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("HTTPS is required except on localhost")
    return base


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def fetch(url, limit):
    request = urllib.request.Request(
        url, headers={"Cache-Control": "no-cache", "User-Agent": "VibeMemoryLauncher/2.0"}
    )
    with urllib.request.build_opener(NoRedirect).open(request, timeout=20) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError("CLI distribution exceeds size limit")
    return data


def ensure_cli():
    base = origin()
    manifest = json.loads(fetch(base + "/skills/vibe-memory/manifest.json", 16384))
    cli = manifest.get("cli", {})
    digest = cli.get("sha256", "")
    url = cli.get("url", "")
    if (
        cli.get("path") != "bin/vibe-memory"
        or len(digest) != 64
        or any(char not in "0123456789abcdef" for char in digest)
        or url != "/skills/vibe-memory/cli/" + digest + "/vibe-memory"
    ):
        raise ValueError("Unsupported CLI manifest")
    if TARGET.is_file() and hashlib.sha256(TARGET.read_bytes()).hexdigest() == digest:
        return TARGET
    data = fetch(base + url, 1024 * 1024)
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("CLI checksum mismatch; existing CLI preserved")
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=TARGET.parent, delete=False) as temporary:
        temporary.write(data)
        temporary_path = Path(temporary.name)
    try:
        temporary_path.chmod(0o755)
        temporary_path.replace(TARGET)
    finally:
        temporary_path.unlink(missing_ok=True)
    return TARGET


def main():
    cli = ensure_cli()
    os.execv(sys.executable, [sys.executable, str(cli), *sys.argv[1:]])


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, urllib.error.URLError, json.JSONDecodeError) as error:
        print(f"Vibe Memory: {error}", file=sys.stderr)
        sys.exit(1)
