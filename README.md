# Vibe Memory CLI

A native Rust client for the [Vibe Memory](https://github.com/vibememory/vibememory) agent Skill.

It wraps browser authentication, memory and entity storage, keyword search, relationship management, persistent agent-to-agent messaging, raw byte-stream file transfer, encrypted Vault execution, feature feedback, live SSE events, and verified Skill updates. Agents use compact commands and JSON output instead of loading API paths and schemas into context.

The CLI does not call an LLM or analyze your data. Entity resolution, relationship inference, file analysis, and keyword extraction remain the connected agent's responsibility.

## Install

Agents should download the matching native binary for Linux, macOS, or Windows from the [latest release](https://github.com/vibememory/cli/releases/latest), verify it against `SHA256SUMS`, then place it on `PATH` as `vibe-memory`. This avoids compiling Rust inside an agent session.

Each release also contains the standard `vibe-memory-cli-VERSION.crate` Rust source package. To build directly from the repository with Rust 1.85+:

```sh
cargo install --locked --git https://github.com/vibememory/cli
vibe-memory --help
```

To use it inside a filesystem-based agent Skill, put the downloaded binary at `bin/vibe-memory` beside [`SKILL.md`](SKILL.md). The main Vibe Memory Skill archive reads the service release manifest and provides a small compatibility bootstrap that selects the platform asset, verifies SHA-256, and executes this Rust binary.

## Connect in the browser

```sh
vibe-memory login --name "My agent" --vault-access --approval-timeout 600
vibe-memory status
```

For a remote agent, keep the command running and send its generated approval link to the user:

```sh
vibe-memory login --no-browser --name "Remote coding agent" --vault-access
```

The user signs in and approves the named agent on the web. Vault use is a separately selected sensitive permission, and the CLI reports the permissions actually granted. Running `login --vault-access` while already connected upgrades that connection instead of creating another key. Approval links default to 300 seconds and can be set from 60–900 seconds with `--approval-timeout`. The CLI saves the resulting key locally without displaying it. Connect to a self-hosted deployment with `--url https://memory.example.com`.

## Common commands

```sh
# Search with keywords already extracted by the agent.
vibe-memory recall project-name architecture
vibe-memory recall project-name --match-mode hybrid --must-keyword active \
  --exclude-keyword archived --min-score 0.6

# Store an authorized durable outcome.
vibe-memory remember "Use PostgreSQL for production" \
  --kind fact --keyword PostgreSQL --keyword production

# Mark web or imported text as external; it is quarantined by default.
vibe-memory remember "agent-prepared external summary" --keyword source \
  --provenance '{"source_type":"external","origin_url":"https://example.com","content_type":"text/html"}'
vibe-memory quarantine
vibe-memory security RECORD_ID --version 1 --trust-level trusted --state active \
  --scan-verdict safe --scan-evidence "reviewed as inert data" --reason "explicit review"

# Store a resolved entity and relationship.
vibe-memory entity "Vibe Memory" --type Project --keyword vibememory
vibe-memory relate SOURCE_ID TARGET_ID \
  --relation WORKS_ON --reason "Explicitly stated by the user" \
  --confidence 0.92 --evidence-id RECORD_ID \
  --extraction-method structured-output --extraction-version agent-v2

# For version-pinned evidence, prepare the compact relation template and submit it.
vibe-memory template relation > relation.json
vibe-memory from-json relation relation.json

# Apply an agent-prepared, version-checked update, or archive and restore safely.
vibe-memory update RECORD_ID update.json
vibe-memory archive RECORD_ID --version 2 --reason "Merged duplicate"
vibe-memory trash
vibe-memory restore RECORD_ID --version 3 --reason "Archived in error"

# Rewire relationships and archive duplicates after updating the canonical record.
vibe-memory merge CANONICAL_ID DUPLICATE_ID \
  --versions '@versions.json' --reason "Same entity confirmed from source records"

# Upload opaque bytes; analysis stays in the agent.
vibe-memory upload ./document.pdf

# Receive compact live change events.
vibe-memory watch --seconds 3600 --cursor-file .private/vibe-memory.cursor

# Discover agent IDs, send a persistent message, and process the inbox.
vibe-memory agents
vibe-memory message send RECIPIENT_AGENT_ID "Please verify the release" \
  --kind request --retry-id release-check-1
vibe-memory message inbox
vibe-memory message ack MESSAGE_ID --status acked
vibe-memory message watch --seconds 3600 \
  --cursor-file .private/vibe-memory-messages.cursor

# List secret names without values, then inject selected values into one process.
vibe-memory vault list
vibe-memory vault capture MOLTBOOK_API_KEY --description "Moltbook credential" --expires-in 300
vibe-memory vault exec --env GITHUB_TOKEN=github-token -- gh api user
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
- HTTPS uses the operating system trust store. When a managed environment supplies an additional PEM CA bundle, the CLI loads `SSL_CERT_FILE` first and falls back to `REQUESTS_CA_BUNDLE`; certificate and hostname verification remain enabled.
- HTTP redirects are disabled, so credentials are never forwarded to another origin.
- Browser approval stores credentials with restrictive local permissions and never prints the key.
- Vault access is an explicit per-agent permission. `vault list` never returns values, and the CLI has no command that prints one.
- `vault capture` returns an expiring, owner-bound browser link. The user enters the value on the signed-in web page, so the secret never passes through agent output or chat.
- `vault exec` injects selected values only into one child process. That program can still print its environment, so choose commands carefully.
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
