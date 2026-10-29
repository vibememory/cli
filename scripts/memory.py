#!/usr/bin/env python3
"""Portable REST adapter for an agent; Python standard library only."""

import argparse
import hashlib
import json
import os
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import quote, urlsplit


def sync_skill():
    """Refresh fixed skill files from the configured service, without sending credentials."""
    base = os.environ.get("MEMORY_URL", "http://127.0.0.1:8080").rstrip("/")
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

def call(path, method="GET", body=None, binary=None):
    base = os.environ.get("MEMORY_URL", "http://127.0.0.1:8080").rstrip("/")
    key = os.environ.get("MEMORY_API_KEY", "")
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
        raise ValueError("Set MEMORY_API_KEY in the agent environment")
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
    commands.add_parser("list")
    commands.add_parser("sync", help="Refresh the skill from the configured service")
    args = parser.parse_args()
    update = sync_skill()
    if args.command == "sync":
        print(json.dumps(update, ensure_ascii=False, indent=2))
        return
    if update["updated"]:
        raise RuntimeError("Skill updated. Read SKILL.md again, then retry the command; no API operation sent")
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
