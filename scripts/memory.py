#!/usr/bin/env python3
"""Portable REST adapter for an agent; Python standard library only."""

import argparse
import getpass
import hashlib
import json
import os
import sys
import tempfile
import time
import urllib.error
import urllib.request
import webbrowser
from pathlib import Path
from urllib.parse import quote, urlsplit


def config_path():
    base = os.environ.get("APPDATA") if os.name == "nt" else os.environ.get("XDG_CONFIG_HOME")
    return (Path(base) if base else Path.home() / ".config") / "vibe-memory" / "client.json"


def validate_origin(base):
    base = base.strip().rstrip("/")
    parsed = urlsplit(base)
    if (parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username
            or parsed.password or parsed.query or parsed.fragment or parsed.path):
        raise ValueError("Use a service origin such as https://vibememory.io, without a path or credentials")
    if parsed.scheme == "http" and parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("HTTPS is required except on localhost")
    return base


def read_config():
    path = config_path()
    if not path.exists():
        return {}
    if os.name != "nt" and path.stat().st_mode & 0o077:
        raise ValueError("Client config permissions are too broad; run chmod 600 on " + str(path))
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("Invalid client configuration")
    return data


def connection():
    saved = read_config()
    base = validate_origin(os.environ.get("MEMORY_URL") or saved.get("url") or "https://vibememory.io")
    key = os.environ.get("MEMORY_API_KEY")
    if key is None:
        # Never send a saved key to an origin selected by an unrelated environment override.
        key = saved.get("api_key", "") if saved.get("url") == base else ""
    return base, key



def save_credentials(base, key):
    path = config_path()
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                     delete=False) as temporary:
        json.dump({"url": base, "api_key": key}, temporary)
        temporary_path = Path(temporary.name)
    try:
        temporary_path.replace(path)
    finally:
        temporary_path.unlink(missing_ok=True)


def browser_login(base, name, no_browser=False):
    base = validate_origin(base)

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    opener = urllib.request.build_opener(NoRedirect)

    def post(path, payload):
        request = urllib.request.Request(
            base + path, data=json.dumps(payload).encode(), method="POST",
            headers={"Content-Type": "application/json", "User-Agent": "VibeMemorySkill/1.0"},
        )
        try:
            with opener.open(request, timeout=20) as response:
                return json.loads(response.read(16384))
        except urllib.error.HTTPError as error:
            messages = {403: "Connection denied", 410: "Connection expired; run login again",
                        422: "Cannot create a key; check the console key limit",
                        429: "Too many requests; wait before retrying"}
            raise RuntimeError(messages.get(error.code, f"Login returned HTTP {error.code}")) from None

    pending = post("/auth/device/start", {"name": name})
    code = pending["user_code"]
    if len(code) != 10 or any(char not in "ABCDEF0123456789" for char in code):
        raise ValueError("Invalid verification code")
    url = base + "/connect?code=" + code
    print("Approve this agent in your browser: " + url, flush=True)
    print("Confirm code: " + code + " (expires in 5 minutes)", flush=True)
    if not no_browser:
        webbrowser.open(url)
    deadline = time.monotonic() + min(300, pending["expires_in"])
    interval = max(3, min(10, pending.get("interval", 3)))
    while time.monotonic() < deadline:
        time.sleep(interval)
        result = post("/auth/device/poll", {"device_code": pending["device_code"]})
        if result["state"] == "connected":
            call("/v1/me", credentials=(base, result["key"]))
            save_credentials(base, result["key"])
            print("Connected. Credentials saved locally; no API key needs to be copied.", flush=True)
            if os.environ.get("MEMORY_URL") or os.environ.get("MEMORY_API_KEY"):
                print("Note: environment credentials override saved settings.", flush=True)
            return
        if result["state"] != "pending":
            raise RuntimeError("Unexpected login state")
    raise RuntimeError("Connection expired; run login again")

def configure():
    if not sys.stdin.isatty() or not sys.stderr.isatty():
        raise ValueError("Run configure in your own interactive terminal; API keys must not be pasted into agent chat")
    saved = read_config()
    default = saved.get("url") or "https://vibememory.io"
    base = validate_origin(input(f"Server URL [{default}]: ").strip() or default)
    print("Create an agent API key at " + base + "/console; paste it below (hidden).")
    key = getpass.getpass("API key: ").strip()
    if not key or any(char.isspace() for char in key):
        raise ValueError("Enter a nonempty API key without whitespace")
    # Verify before saving, using only the URL and key entered in this terminal.
    call("/v1/me", credentials=(base, key))
    save_credentials(base, key)
    path = config_path()
    print("Connected. Credentials saved locally to " + str(path))
    if os.environ.get("MEMORY_URL") or os.environ.get("MEMORY_API_KEY"):
        print("Note: MEMORY_URL / MEMORY_API_KEY environment variables override saved settings.")

def sync_skill():
    """Refresh fixed skill files from the configured service, without sending credentials."""
    base, _ = connection()
    parsed = urlsplit(base)
    if (parsed.scheme not in ("http", "https") or not parsed.hostname
            or parsed.username or parsed.password or parsed.query or parsed.fragment):
        raise ValueError("MEMORY_URL must be a service URL without credentials, query or fragment")
    if parsed.scheme == "http" and parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("Skill updates require HTTPS except on localhost")

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    opener = urllib.request.build_opener(NoRedirect)

    def download(path, limit):
        request = urllib.request.Request(base + path, headers={"Cache-Control": "no-cache", "User-Agent": "VibeMemorySkill/1.0"})
        with opener.open(request, timeout=15) as response:
            data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError("Skill distribution exceeds size limit")
        return data

    manifest = json.loads(download("/skills/vibe-memory/manifest.json", 16384))
    allowed = {"SKILL.md", "scripts/memory.py"}
    files = manifest.get("files", {})
    if manifest.get("schema_version") != 1 or set(files) != allowed:
        raise ValueError("Unsupported skill manifest")
    revision = hashlib.sha256(
        json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    if manifest.get("revision") != revision:
        raise ValueError("Invalid skill revision")
    root = Path(__file__).resolve().parents[1]
    pending = []
    for name in sorted(allowed):
        target = root / name
        if target.is_file() and hashlib.sha256(target.read_bytes()).hexdigest() == files[name]:
            continue
        # Content-addressed paths keep a release consistent during concurrent web deployments.
        data = download("/skills/vibe-memory/releases/" + revision + "/" + name, 1048576)
        if hashlib.sha256(data).hexdigest() != files[name]:
            raise ValueError("Skill checksum mismatch; no files updated")
        pending.append((target, data))
    # Validate every download before replacing any file. Each replacement is atomic.
    for target, data in pending:
        target.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temporary:
            temporary.write(data)
            temporary_path = Path(temporary.name)
        try:
            temporary_path.replace(target)
        finally:
            temporary_path.unlink(missing_ok=True)
    return {"revision": revision, "updated": bool(pending),
            "instruction": "Read SKILL.md again before continuing" if pending else "Skill is current"}

def call(path, method="GET", body=None, binary=None, credentials=None):
    base, key = credentials if credentials is not None else connection()
    base = validate_origin(base)
    parsed = urlsplit(base)
    if (
        parsed.scheme not in ("http", "https")
        or not parsed.hostname
        or parsed.username
        or parsed.password
    ):
        raise ValueError("MEMORY_URL must be an HTTP(S) service URL without embedded credentials")
    if parsed.scheme == "http" and parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("Use HTTPS when sending an API key to a remote service")
    if not key:
        raise ValueError("Run configure in your terminal, or set MEMORY_API_KEY in the agent environment")
    headers = {"Authorization": "Bearer " + key, "Content-Type": "application/json",
               "User-Agent": "VibeMemorySkill/1.0"}
    if binary is not None:
        headers.update(
            {
                "Content-Type": "application/octet-stream",
                "Content-Length": str(os.fstat(binary.fileno()).st_size),
            }
        )
    request = urllib.request.Request(
        base + path,
        method=method,
        headers=headers,
        data=binary
        if binary is not None
        else (None if body is None else json.dumps(body, ensure_ascii=False).encode()),
    )

    # Do not forward credentials if the service redirects to another host.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    try:
        response = urllib.request.build_opener(NoRedirect).open(request, timeout=180)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"API returned HTTP {error.code}") from None
    with response:
        data = response.read()
        return json.loads(data) if data else {"deleted": True}



def watch_events(seconds=60, after=0, cursor_file=None):
    """Print authenticated SSE changes as JSON lines; reconnect using the last emitted ID."""
    base, key = connection()
    if not key:
        raise ValueError("Run configure in your terminal, or set MEMORY_API_KEY in the agent environment")
    if cursor_file and cursor_file.exists():
        after = int(cursor_file.read_text().strip())
    if after < 0 or after > 9223372036854775807:
        raise ValueError("Invalid event cursor")

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    opener = urllib.request.build_opener(NoRedirect)
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        duration = max(1, min(25, int(deadline - time.monotonic())))
        request = urllib.request.Request(
            base + "/v1/events?seconds=" + str(duration),
            headers={"Authorization": "Bearer " + key, "Accept": "text/event-stream",
                     "Last-Event-ID": str(after), "User-Agent": "VibeMemorySkill/1.0"},
        )
        try:
            with opener.open(request, timeout=duration + 15) as response:
                if response.headers.get_content_type() != "text/event-stream":
                    raise ValueError("Expected an SSE event stream")
                event = {}
                for raw in response:
                    line = raw.decode("utf-8").rstrip("\r\n")
                    if not line:
                        if {"id", "event", "data"} <= event.keys():
                            event_id = int(event["id"])
                            if event_id > after:
                                print(json.dumps({"id": event_id, "event": event["event"],
                                                  "data": json.loads(event["data"])},
                                                 ensure_ascii=False), flush=True)
                                after = event_id
                                if cursor_file:
                                    cursor_file.parent.mkdir(parents=True, exist_ok=True)
                                    with tempfile.NamedTemporaryFile(
                                        mode="w", dir=cursor_file.parent, delete=False
                                    ) as saved:
                                        saved.write(str(after))
                                        saved_path = Path(saved.name)
                                    saved_path.replace(cursor_file)
                        event = {}
                    elif line.startswith(("id:", "event:", "data:")):
                        name, value = line.split(":", 1)
                        event[name] = value.lstrip(" ")
        except urllib.error.HTTPError as error:
            raise RuntimeError(f"Event stream returned HTTP {error.code}") from None
        except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
            print(f"Vibe Memory event stream interrupted ({type(error).__name__}); reconnecting",
                  file=sys.stderr)
        if time.monotonic() < deadline:
            time.sleep(min(1, max(0, deadline - time.monotonic())))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    recall = commands.add_parser(
        "recall", help="Search with keywords already extracted by the agent"
    )
    recall.add_argument("keywords", nargs="+")
    recall.add_argument("--limit", type=int, default=8)
    recall.add_argument(
        "--scope", choices=["all", "memories", "documents", "entities"], default="all"
    )
    remember = commands.add_parser("remember")
    remember.add_argument("content")
    remember.add_argument("--keyword", action="append", required=True)
    remember.add_argument(
        "--kind", choices=["fact", "preference", "event", "instruction"], default="fact"
    )
    remember.add_argument("--source", default="")
    remember.add_argument("--tag", action="append", default=[])
    upload = commands.add_parser(
        "upload", help="Stream an original file as raw bytes, without analyzing it"
    )
    upload.add_argument("file", type=Path)
    for name in ("ingest", "entity", "request-feature"):
        command = commands.add_parser(
            name, help="POST a JSON payload already prepared by the agent"
        )
        command.add_argument("payload", type=Path)
    relate = commands.add_parser("relate")
    relate.add_argument("source_id")
    relate.add_argument("target_id")
    relate.add_argument("--relation", required=True)
    relate.add_argument("--reason", required=True)
    reindex = commands.add_parser("keywords")
    reindex.add_argument("id")
    reindex.add_argument("keywords", nargs="+")
    for name in ("get", "forget", "graph"):
        command = commands.add_parser(name)
        command.add_argument("id")
    feature = commands.add_parser("feature-request", help="Read one submitted feature request")
    feature.add_argument("id")
    commands.add_parser("feature-requests", help="List this user's feature requests")
    watch = commands.add_parser("watch", help="Receive live memory changes as JSON lines")
    watch.add_argument("--seconds", type=int, choices=range(1, 3601), default=60,
                       metavar="1..3600")
    watch.add_argument("--after", type=int, default=0)
    watch.add_argument("--cursor-file", type=Path)
    commands.add_parser("list")
    commands.add_parser("sync", help="Refresh the skill from the configured service")
    login = commands.add_parser("login", help="Sign in and approve this agent in the browser")
    login.add_argument("--url", help="Service origin (defaults to saved configuration or hosted service)")
    login.add_argument("--name", default="Connected agent")
    login.add_argument("--no-browser", action="store_true", help="Print approval link for a remote agent")
    commands.add_parser("configure", help="Interactively verify and save URL and API key (hidden input)")
    commands.add_parser("status", help="Check authentication without displaying the API key")
    commands.add_parser("logout", help="Remove locally saved credentials; does not revoke server keys")
    args = parser.parse_args()
    if args.command == "login":
        browser_login(args.url or connection()[0], args.name, args.no_browser)
        return
    if args.command == "configure":
        configure()
        return
    if args.command == "status":
        call("/v1/me")
        print(json.dumps({"connected": True, "url": connection()[0]}, indent=2))
        return
    if args.command == "logout":
        config_path().unlink(missing_ok=True)
        print("Local credentials removed. Environment credentials are unchanged; revoke keys in the console if needed.")
        return
    update = sync_skill()
    if args.command == "sync":
        print(json.dumps(update, ensure_ascii=False, indent=2))
        return
    if update["updated"]:
        raise RuntimeError("Skill updated. Read SKILL.md again, then retry the command; no API operation sent")
    if args.command == "watch":
        watch_events(args.seconds, args.after, args.cursor_file)
        return
    if args.command == "recall":
        result = call(
            "/v1/search", "POST", dict(keywords=args.keywords, limit=args.limit, scope=args.scope)
        )
    elif args.command == "remember":
        result = call(
            "/v1/memories",
            "POST",
            dict(
                content=args.content,
                kind=args.kind,
                source=args.source,
                tags=args.tag,
                keywords=args.keyword,
            ),
        )
    elif args.command == "upload":
        with args.file.open("rb") as source:
            result = call(
                "/v1/files?filename=" + quote(args.file.name, safe=""), "POST", binary=source
            )
    elif args.command in ("ingest", "entity", "request-feature"):
        payload = json.loads(args.payload.read_text(encoding="utf-8"))
        path = {
            "ingest": "/v1/documents",
            "entity": "/v1/entities",
            "request-feature": "/v1/feature-requests",
        }[args.command]
        result = call(path, "POST", payload)
    elif args.command == "relate":
        result = call(
            "/v1/relations",
            "POST",
            dict(
                source_id=args.source_id,
                target_id=args.target_id,
                relation=args.relation,
                reason=args.reason,
            ),
        )
    elif args.command == "keywords":
        result = call(
            "/v1/records/" + quote(args.id, safe="") + "/keywords",
            "PUT",
            {"keywords": args.keywords},
        )
    elif args.command == "feature-requests":
        result = call("/v1/feature-requests")
    elif args.command == "feature-request":
        result = call("/v1/feature-requests/" + quote(args.id, safe=""))
    elif args.command == "list":
        result = call("/v1/records")
    else:
        id = quote(args.id, safe="")
        path = "/v1/graph/" if args.command == "graph" else "/v1/records/"
        result = call(path + id, "DELETE" if args.command == "forget" else "GET")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, RuntimeError) as error:
        print(f"Vibe Memory: {error}", file=sys.stderr)
        sys.exit(1)
