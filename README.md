# Vibe Memory CLI

A native Rust client for the [Vibe Memory](https://github.com/vibememory/vibememory) agent Skill.

It wraps browser authentication, memory and entity storage, keyword search, relationship management, raw byte-stream file transfer, feature feedback, live SSE events, and verified Skill updates. Agents use compact commands and JSON output instead of loading API paths and schemas into context.

The CLI does not call an LLM or analyze your data. Entity resolution, relationship inference, file analysis, and keyword extraction remain the connected agent's responsibility.

## Install

Download the native binary for Linux, macOS, or Windows from the [latest release](https://github.com/vibememory/cli/releases/latest), then place it on `PATH` as `vibe-memory`.

Or build it with Rust 1.85+:

```sh
cargo install --locked --git https://github.com/vibememory/cli
vibe-memory --help
```

To use it inside a filesystem-based agent Skill, put the binary at `bin/vibe-memory` beside [`SKILL.md`](SKILL.md). The main Vibe Memory Skill archive provides a compatibility bootstrap for older installations.

## Connect in the browser

```sh
vibe-memory login --name "My agent"
vibe-memory status
```

For a remote agent, keep the command running and send its generated approval link to the user:

```sh
vibe-memory login --no-browser --name "Remote coding agent"
```

The user signs in and approves the named agent on the web. The CLI saves the resulting key locally without displaying it. Connect to a self-hosted deployment with `--url https://memory.example.com`.

## Common commands

```sh
# Search with keywords already extracted by the agent.
vibe-memory recall project-name architecture

# Store an authorized durable outcome.
vibe-memory remember "Use PostgreSQL for production" \
  --kind fact --keyword PostgreSQL --keyword production

# Store a resolved entity and relationship.
vibe-memory entity "Vibe Memory" --type Project --keyword vibememory
vibe-memory relate SOURCE_ID TARGET_ID \
  --relation WORKS_ON --reason "Explicitly stated by the user"

# Upload opaque bytes; analysis stays in the agent.
vibe-memory upload ./document.pdf

# Receive compact live change events.
vibe-memory watch --seconds 3600 --cursor-file .private/vibe-memory.cursor
```

Output is compact JSON by default. Add global `--pretty` for human inspection. Load details only when needed:

```sh
vibe-memory COMMAND --help
vibe-memory template memory
vibe-memory template entity
vibe-memory template document
vibe-memory template feedback
```

## Security and updates

- Remote service origins must use HTTPS.
- HTTP redirects are disabled, so credentials are never forwarded to another origin.
- Browser approval stores credentials with restrictive local permissions and never prints the key.
- `vibe-memory sync` verifies Skill files from content-addressed paths with SHA-256 before replacing them.
- Every data command checks the current Skill revision before sending a request.
- Release workflows publish native binaries and a `SHA256SUMS` file.

Browser login stores configuration at:

- Linux/macOS: `$XDG_CONFIG_HOME/vibe-memory/client.json` or `~/.config/vibe-memory/client.json`
- Windows: `%APPDATA%/vibe-memory/client.json`

`MEMORY_URL` and `MEMORY_API_KEY` can override saved configuration. A saved key is never sent to a different origin selected only through `MEMORY_URL`.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md). Licensed under the [MIT License](LICENSE).
