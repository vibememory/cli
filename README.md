# Vibe Memory CLI

The small, standard-library-only client used by the [Vibe Memory](https://github.com/vibememory/vibememory) agent Skill.

It wraps browser authentication, memory and entity storage, keyword search, relationship management, raw byte-stream file transfer, feature feedback, live SSE events, and verified Skill updates. Agents can use compact commands and JSON output instead of loading API paths and schemas into their context.

Vibe Memory CLI does not call an LLM or analyze your data. Entity resolution, relationship inference, file analysis, and keyword extraction remain the connected agent's responsibility.

## Install

Python 3.10 or newer is the only requirement.

```sh
git clone https://github.com/vibememory/cli.git vibe-memory-cli
cd vibe-memory-cli
./bin/vibe-memory --help
```

On Windows or when executable permissions are unavailable:

```sh
python3 bin/vibe-memory --help
```

The complete agent instructions are in [`SKILL.md`](SKILL.md). Copy this repository into an agent's Skill directory when the host supports filesystem-based Skills.

## Connect in the browser

```sh
./bin/vibe-memory login --name "My agent"
./bin/vibe-memory status
```

For a remote agent, keep the command running and send its generated approval link to the user:

```sh
./bin/vibe-memory login --no-browser --name "Remote coding agent"
```

The user signs in and approves the named agent on the web. The CLI saves the resulting key locally without displaying it. Use a different deployment with `--url https://memory.example.com`.

## Common commands

```sh
# Search with keywords already extracted by the agent.
./bin/vibe-memory recall project-name architecture

# Store an authorized durable outcome.
./bin/vibe-memory remember "Use PostgreSQL for production" \
  --kind fact --keyword PostgreSQL --keyword production

# Store a resolved entity and relationship.
./bin/vibe-memory entity "Vibe Memory" --type Project --keyword vibememory
./bin/vibe-memory relate SOURCE_ID TARGET_ID \
  --relation WORKS_ON --reason "Explicitly stated by the user"

# Upload opaque bytes; analysis stays in the agent.
./bin/vibe-memory upload ./document.pdf

# Receive compact live change events.
./bin/vibe-memory watch --seconds 3600 --cursor-file .private/vibe-memory.cursor
```

Output is compact JSON by default. Add global `--pretty` for human inspection. Load details only when needed:

```sh
./bin/vibe-memory COMMAND --help
./bin/vibe-memory template memory
./bin/vibe-memory template entity
./bin/vibe-memory template document
./bin/vibe-memory template feedback
```

## Verified updates

`./bin/vibe-memory sync` reads the manifest from the configured Vibe Memory service. Skill instructions, the compatibility launcher, and the CLI are downloaded from content-addressed paths, verified with SHA-256, and replaced atomically. Data commands perform the same check before sending a request and stop if an update was installed.

The legacy `python3 scripts/memory.py ...` entry point remains available. It verifies and bootstraps the current CLI before forwarding arguments.

## Configuration

Browser login stores configuration at:

- Linux/macOS: `$XDG_CONFIG_HOME/vibe-memory/client.json` or `~/.config/vibe-memory/client.json`
- Windows: `%APPDATA%/vibe-memory/client.json`

`MEMORY_URL` and `MEMORY_API_KEY` can override saved configuration. A saved key is never sent to a different origin selected only through `MEMORY_URL`. Remote HTTP origins are rejected; use HTTPS outside localhost.

## Development

```sh
python3 -m unittest discover -s tests -v
python3 -m compileall -q bin scripts tests
```

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md). Licensed under the [MIT License](LICENSE).
