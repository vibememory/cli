use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use reqwest::blocking::{Body, Client, RequestBuilder};
use reqwest::header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use url::Url;

const USER_AGENT: &str = concat!("VibeMemoryCLI/", env!("CARGO_PKG_VERSION"));
const DEFAULT_ORIGIN: &str = "https://vibememory.io";
const MAX_DISTRIBUTION_BYTES: u64 = 1024 * 1024;

#[derive(Parser, Debug)]
#[command(
    name = "vibe-memory",
    version,
    about = "Native CLI for agent-authored memory, files, graph, feedback, and live events"
)]
struct Cli {
    #[arg(long, global = true, help = "Indent JSON output (default is compact)")]
    pretty: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Search agent-extracted keywords.
    Recall {
        #[arg(required = true)]
        keywords: Vec<String>,
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: u16,
        #[arg(long, default_value = "all", value_parser = ["all", "memories", "documents", "entities"])]
        scope: String,
    },
    /// Store an agent-authored durable memory.
    Remember {
        content: String,
        #[arg(short = 'k', long = "keyword", required = true)]
        keywords: Vec<String>,
        #[arg(long, default_value = "fact", value_parser = ["fact", "preference", "event", "instruction"])]
        kind: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long, default_value = "{}", help = "JSON object or @file")]
        metadata: String,
    },
    /// Store an agent-resolved entity.
    Entity {
        name: String,
        #[arg(long = "type")]
        entity_type: Option<String>,
        #[arg(short = 'k', long = "keyword")]
        keywords: Vec<String>,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long, default_value = "{}", help = "JSON object or @file")]
        metadata: String,
    },
    /// Store agent-prepared document metadata.
    Document {
        title: String,
        #[arg(short = 'k', long = "keyword", required = true)]
        keywords: Vec<String>,
        #[arg(long, default_value = "")]
        file_id: String,
        #[arg(long, default_value = "")]
        content: String,
        #[arg(long)]
        content_file: Option<PathBuf>,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long, default_value = "{}", help = "JSON object or @file")]
        metadata: String,
    },
    /// Stream an opaque file as raw bytes.
    Upload { file: PathBuf },
    /// Download an original file.
    Download { id: String, output: PathBuf },
    /// Store an agent-inferred relationship.
    Relate {
        source_id: String,
        target_id: String,
        #[arg(long)]
        relation: String,
        #[arg(long)]
        reason: String,
    },
    /// Retract or restore a relationship.
    RelationStatus {
        id: String,
        #[arg(long, value_parser = ["active", "retracted"])]
        status: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        version: u64,
    },
    /// Replace a record keyword index.
    Keywords {
        id: String,
        #[arg(required = true)]
        keywords: Vec<String>,
    },
    /// Read one record.
    Get { id: String },
    /// Delete one record.
    Forget { id: String },
    /// Read a record's relationship graph.
    Graph { id: String },
    /// List records.
    List {
        #[arg(long, value_parser = ["memory", "document", "entity", "file"])]
        kind: Option<String>,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: u16,
        #[arg(long, default_value_t = 0)]
        offset: u64,
    },
    /// Submit a feature request.
    Feedback {
        title: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        agent_name: String,
        #[arg(long, default_value = "")]
        use_case: String,
        #[arg(long, default_value = "medium", value_parser = ["low", "medium", "high"])]
        priority: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long = "retry-id", default_value = "")]
        client_request_id: String,
    },
    /// List submitted feature requests.
    FeedbackList,
    /// Read a submitted feature request.
    FeedbackGet { id: String },
    /// Submit a fully prepared JSON object.
    FromJson {
        #[arg(value_parser = ["memory", "entity", "document", "feedback"])]
        kind: String,
        payload: PathBuf,
    },
    /// Print a compact payload template.
    Template {
        #[arg(value_parser = ["memory", "entity", "document", "feedback"])]
        kind: String,
    },
    /// Print live changes as compact JSON lines.
    Watch {
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
        seconds: u64,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long)]
        cursor_file: Option<PathBuf>,
    },
    /// Refresh installed Skill instructions.
    Sync,
    /// Connect through browser approval.
    Login {
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "Connected agent")]
        name: String,
        #[arg(long)]
        no_browser: bool,
    },
    /// Fallback: save an existing API key interactively.
    Configure,
    /// Verify authentication without printing credentials.
    Status,
    /// Remove locally saved credentials.
    Logout,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct Config {
    #[serde(default)]
    url: String,
    #[serde(default)]
    api_key: String,
}

#[derive(Debug, Clone)]
struct Connection {
    base: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct DeviceStart {
    device_code: String,
    user_code: String,
    expires_in: u64,
    #[serde(default = "default_poll_interval")]
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct DevicePoll {
    state: String,
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u64,
    revision: String,
    files: BTreeMap<String, String>,
}

fn default_poll_interval() -> u64 {
    3
}

fn config_path() -> Result<PathBuf> {
    if cfg!(windows) {
        let base = env::var_os("APPDATA").context("APPDATA is not set")?;
        return Ok(PathBuf::from(base).join("vibe-memory/client.json"));
    }
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .context("HOME and XDG_CONFIG_HOME are not set")?;
    Ok(base.join("vibe-memory/client.json"))
}

fn validate_origin(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let parsed =
        Url::parse(trimmed).context("Use a service origin such as https://vibememory.io")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        bail!("Use a service origin such as https://vibememory.io");
    }
    let host = parsed.host_str().unwrap_or_default();
    if parsed.scheme() == "http" && !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        bail!("HTTPS is required except on localhost");
    }
    Ok(trimmed.to_owned())
}

fn read_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&path)?.permissions().mode() & 0o077 != 0 {
            bail!(
                "Client config permissions are too broad; run chmod 600 on {}",
                path.display()
            );
        }
    }
    serde_json::from_slice(&fs::read(path)?).context("Invalid client configuration")
}

fn connection() -> Result<Connection> {
    let saved = read_config()?;
    let requested = env::var("MEMORY_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if saved.url.is_empty() {
                DEFAULT_ORIGIN.to_owned()
            } else {
                saved.url.clone()
            }
        });
    let base = validate_origin(&requested)?;
    let key = env::var("MEMORY_API_KEY").unwrap_or_else(|_| {
        if saved.url == base {
            saved.api_key
        } else {
            String::new()
        }
    });
    Ok(Connection { base, key })
}

fn atomic_write(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .context("Destination has no parent directory")?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(data)?;
    temporary.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    temporary
        .persist(path)
        .map_err(|error| anyhow!("Cannot replace {}: {}", path.display(), error.error))?;
    Ok(())
}

fn save_credentials(base: &str, key: &str) -> Result<()> {
    let data = serde_json::to_vec(&Config {
        url: base.to_owned(),
        api_key: key.to_owned(),
    })?;
    atomic_write(&config_path()?, &data, 0o600)
}

fn http_client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .context("Cannot initialize HTTPS client")
}

fn authenticated_request(
    connection: &Connection,
    method: Method,
    path: &str,
    timeout: Duration,
) -> Result<RequestBuilder> {
    if connection.key.is_empty() {
        bail!("Run login, configure, or set MEMORY_API_KEY");
    }
    Ok(http_client(timeout)?
        .request(method, format!("{}{}", connection.base, path))
        .header(AUTHORIZATION, format!("Bearer {}", connection.key)))
}

fn call_with(
    credentials: &Connection,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let mut request = authenticated_request(credentials, method, path, Duration::from_secs(180))?;
    if let Some(payload) = body {
        request = request.json(payload);
    }
    let mut response = request.send().context("API request failed")?;
    if !response.status().is_success() {
        bail!("API returned HTTP {}", response.status().as_u16());
    }
    let mut bytes = Vec::new();
    response.read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        Ok(json!({"deleted": true}))
    } else {
        serde_json::from_slice(&bytes).context("API returned invalid JSON")
    }
}

fn call(method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
    call_with(&connection()?, method, path, body)
}

fn post_public<T: for<'de> Deserialize<'de>>(base: &str, path: &str, body: &Value) -> Result<T> {
    let response = http_client(Duration::from_secs(20))?
        .post(format!("{base}{path}"))
        .json(body)
        .send()
        .context("Connection request failed")?;
    if !response.status().is_success() {
        let message = match response.status().as_u16() {
            403 => "Connection denied",
            410 => "Connection expired; run login again",
            422 => "Cannot create a key; check the console key limit",
            429 => "Too many requests; wait before retrying",
            code => return Err(anyhow!("Login returned HTTP {code}")),
        };
        bail!(message);
    }
    response.json().context("Login returned invalid JSON")
}

fn browser_login(base: &str, name: &str, no_browser: bool) -> Result<()> {
    let base = validate_origin(base)?;
    let pending: DeviceStart = post_public(&base, "/auth/device/start", &json!({"name": name}))?;
    if pending.user_code.len() != 10
        || !pending
            .user_code
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_lowercase())
    {
        bail!("Invalid verification code");
    }
    let approval_url = format!("{base}/connect?code={}", pending.user_code);
    println!("Approve this agent in your browser: {approval_url}");
    println!("Confirm code: {} (expires in 5 minutes)", pending.user_code);
    io::stdout().flush()?;
    if !no_browser {
        let _ = webbrowser::open(&approval_url);
    }
    let deadline = Instant::now() + Duration::from_secs(pending.expires_in.min(300));
    let interval = pending.interval.clamp(3, 10);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_secs(interval));
        let result: DevicePoll = post_public(
            &base,
            "/auth/device/poll",
            &json!({"device_code": pending.device_code}),
        )?;
        match result.state.as_str() {
            "connected" => {
                let key = result
                    .key
                    .context("Connected response did not include a key")?;
                let credentials = Connection {
                    base: base.clone(),
                    key: key.clone(),
                };
                call_with(&credentials, Method::GET, "/v1/me", None)?;
                save_credentials(&base, &key)?;
                println!("Connected. Credentials saved locally; no API key was displayed.");
                return Ok(());
            }
            "pending" => {}
            _ => bail!("Unexpected login state"),
        }
    }
    bail!("Connection expired; run login again")
}

fn configure() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        bail!("Run configure in an interactive terminal");
    }
    let saved = read_config()?;
    let default = if saved.url.is_empty() {
        DEFAULT_ORIGIN
    } else {
        &saved.url
    };
    print!("Server URL [{default}]: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let base = validate_origin(if value.trim().is_empty() {
        default
    } else {
        value.trim()
    })?;
    println!("Create an agent key at {base}/console; paste it below (hidden).");
    let key = rpassword::prompt_password("API key: ")?.trim().to_owned();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        bail!("Enter a nonempty API key without whitespace");
    }
    let credentials = Connection {
        base: base.clone(),
        key: key.clone(),
    };
    call_with(&credentials, Method::GET, "/v1/me", None)?;
    save_credentials(&base, &key)?;
    println!(
        "Connected. Credentials saved locally to {}",
        config_path()?.display()
    );
    Ok(())
}

fn public_download(base: &str, path: &str, limit: u64) -> Result<Vec<u8>> {
    let response = http_client(Duration::from_secs(20))?
        .get(format!("{base}{path}"))
        .header(CACHE_CONTROL, "no-cache")
        .send()
        .context("Skill update request failed")?;
    if !response.status().is_success() {
        bail!("Skill update returned HTTP {}", response.status().as_u16());
    }
    let mut data = Vec::new();
    response.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        bail!("Skill distribution exceeds size limit");
    }
    Ok(data)
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn skill_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("VIBE_MEMORY_SKILL_DIR").map(PathBuf::from) {
        return Some(path);
    }
    if let Ok(executable) = env::current_exe()
        && let Some(root) = executable.parent().and_then(Path::parent)
        && root.join("SKILL.md").is_file()
    {
        return Some(root.to_owned());
    }
    env::current_dir()
        .ok()
        .filter(|path| path.join("SKILL.md").is_file())
}

fn sync_skill() -> Result<Value> {
    let base = connection()?.base;
    let manifest: Manifest = serde_json::from_slice(&public_download(
        &base,
        "/skills/vibe-memory/manifest.json",
        16_384,
    )?)
    .context("Invalid Skill manifest")?;
    let allowed = ["SKILL.md", "scripts/memory.py"];
    if manifest.schema_version != 1
        || manifest.files.len() != allowed.len()
        || allowed
            .iter()
            .any(|name| !manifest.files.contains_key(*name))
    {
        bail!("Unsupported Skill manifest");
    }
    let revision = sha256(&serde_json::to_vec(&manifest.files)?);
    if manifest.revision != revision {
        bail!("Invalid Skill revision");
    }
    let Some(root) = skill_root() else {
        return Ok(json!({
            "revision": revision,
            "updated": false,
            "instruction": "Standalone CLI is current; no installed Skill directory detected"
        }));
    };
    let mut pending = Vec::new();
    for name in allowed {
        let expected = &manifest.files[name];
        let target = root.join(name);
        if target.is_file() && sha256(&fs::read(&target)?) == *expected {
            continue;
        }
        let data = public_download(
            &base,
            &format!("/skills/vibe-memory/releases/{revision}/{name}"),
            MAX_DISTRIBUTION_BYTES,
        )?;
        if sha256(&data) != *expected {
            bail!("Skill checksum mismatch; no files updated");
        }
        pending.push((target, data));
    }
    for (target, data) in &pending {
        atomic_write(target, data, 0o644)?;
    }
    Ok(json!({
        "revision": revision,
        "updated": !pending.is_empty(),
        "instruction": if pending.is_empty() { "Skill is current" } else { "Read SKILL.md again before continuing" }
    }))
}

fn json_object(value: &str) -> Result<Value> {
    let text = if let Some(path) = value.strip_prefix('@') {
        fs::read_to_string(path).with_context(|| format!("Cannot read metadata file {path}"))?
    } else {
        value.to_owned()
    };
    let result: Value = serde_json::from_str(&text).context("Metadata is not valid JSON")?;
    if !result.is_object() {
        bail!("Metadata must be a JSON object");
    }
    Ok(result)
}

fn file_json(path: &Path) -> Result<Value> {
    let result: Value = serde_json::from_slice(&fs::read(path)?)?;
    if !result.is_object() {
        bail!("Payload must be a JSON object");
    }
    Ok(result)
}

fn upload_file(path: &Path) -> Result<Value> {
    let credentials = connection()?;
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Filename must be valid Unicode")?;
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("filename", filename);
    let endpoint = format!("/v1/files?{}", serializer.finish());
    let response = authenticated_request(
        &credentials,
        Method::POST,
        &endpoint,
        Duration::from_secs(180),
    )?
    .header(CONTENT_TYPE, "application/octet-stream")
    .header(CONTENT_LENGTH, size)
    .body(Body::new(file))
    .send()?;
    if !response.status().is_success() {
        bail!("API returned HTTP {}", response.status().as_u16());
    }
    response.json().context("API returned invalid JSON")
}

fn download_file(id: &str, output: &Path) -> Result<Value> {
    let credentials = connection()?;
    let endpoint = format!("/v1/files/{}", quote(id));
    let mut response = authenticated_request(
        &credentials,
        Method::GET,
        &endpoint,
        Duration::from_secs(180),
    )?
    .send()?;
    if !response.status().is_success() {
        bail!("API returned HTTP {}", response.status().as_u16());
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut destination = File::create(output)?;
    let size = io::copy(&mut response, &mut destination)?;
    Ok(json!({"downloaded": true, "path": output, "size_bytes": size}))
}

fn quote(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

fn save_cursor(path: &Path, value: u64) -> Result<()> {
    atomic_write(path, value.to_string().as_bytes(), 0o600)
}

fn watch_events(
    seconds: u64,
    mut after: u64,
    cursor_file: Option<&Path>,
    pretty: bool,
) -> Result<()> {
    let credentials = connection()?;
    if let Some(path) = cursor_file
        && path.exists()
    {
        after = fs::read_to_string(path)?.trim().parse()?;
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
        let duration = remaining.clamp(1, 25);
        let request = authenticated_request(
            &credentials,
            Method::GET,
            &format!("/v1/events?seconds={duration}"),
            Duration::from_secs(duration + 15),
        )?
        .header(ACCEPT, "text/event-stream")
        .header("Last-Event-ID", after.to_string());
        let response = match request.send() {
            Ok(response) => response,
            Err(error) => {
                eprintln!("Vibe Memory stream interrupted ({error}); reconnecting");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        if !response.status().is_success() {
            bail!("Event stream returned HTTP {}", response.status().as_u16());
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type.starts_with("text/event-stream") {
            bail!("Expected an SSE event stream");
        }
        let mut event_id: Option<u64> = None;
        let mut event_name: Option<String> = None;
        let mut event_data: Option<String> = None;
        for line in BufReader::new(response).lines() {
            let line = line?;
            if line.is_empty() {
                if let (Some(id), Some(name), Some(data)) =
                    (event_id.take(), event_name.take(), event_data.take())
                    && id > after
                {
                    emit(
                        &json!({"id": id, "event": name, "data": serde_json::from_str::<Value>(&data)?}),
                        pretty,
                    )?;
                    after = id;
                    if let Some(path) = cursor_file {
                        save_cursor(path, after)?;
                    }
                }
                continue;
            }
            if let Some(value) = line.strip_prefix("id:") {
                event_id = Some(value.trim().parse::<u64>()?);
            } else if let Some(value) = line.strip_prefix("event:") {
                event_name = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("data:") {
                event_data = Some(value.trim_start().to_owned());
            }
        }
        if Instant::now() < deadline {
            thread::sleep(Duration::from_millis(250));
        }
    }
    Ok(())
}

fn template(kind: &str) -> Value {
    match kind {
        "memory" => json!({
            "content": "durable fact or preference",
            "kind": "fact",
            "keywords": ["topic", "alias"],
            "source": "agent-session",
            "tags": [],
            "metadata": {"evidence": "user-stated"}
        }),
        "entity" => json!({
            "name": "Entity name",
            "entity_type": "Person",
            "description": "verified description",
            "keywords": ["name", "alias"],
            "source": "agent-session",
            "tags": [],
            "metadata": {}
        }),
        "document" => json!({
            "title": "document title",
            "keywords": ["topic", "project"],
            "file_id": "optional uploaded file ID",
            "content": "optional opaque text",
            "source": "file or URL",
            "tags": [],
            "metadata": {"summary": "agent-authored summary", "mime_type": "text/plain"}
        }),
        _ => json!({
            "title": "Feature title",
            "description": "Concrete missing capability",
            "agent_name": "agent name",
            "use_case": "Why it matters",
            "priority": "medium",
            "source": "agent-session",
            "client_request_id": "stable-retry-id"
        }),
    }
}

fn emit(value: &Value, pretty: bool) -> Result<()> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}

fn normalize_legacy_arguments(arguments: Vec<OsString>) -> Vec<OsString> {
    let mut values = arguments;
    let index = if values.get(1).and_then(|value| value.to_str()) == Some("--pretty") {
        2
    } else {
        1
    };
    let Some(command) = values.get(index).and_then(|value| value.to_str()) else {
        return values;
    };
    let replacement: &[&str] = match command {
        "ingest" => &["from-json", "document"],
        "entity-json" => &["from-json", "entity"],
        "request-feature" => &["from-json", "feedback"],
        "feature-request" => &["feedback-get"],
        "feature-requests" => &["feedback-list"],
        _ => return values,
    };
    values.splice(
        index..=index,
        replacement.iter().map(|value| OsString::from(*value)),
    );
    values
}

fn ensure_current_skill() -> Result<()> {
    let update = sync_skill()?;
    if update
        .get("updated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        bail!("Skill updated. Read SKILL.md again, then retry; no data operation was sent");
    }
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse_from(normalize_legacy_arguments(env::args_os().collect()));
    match &cli.command {
        Command::Login {
            url,
            name,
            no_browser,
        } => {
            let base = url.clone().unwrap_or(connection()?.base);
            return browser_login(&base, name, *no_browser);
        }
        Command::Configure => return configure(),
        Command::Status => {
            let me = call(Method::GET, "/v1/me", None)?;
            return emit(
                &json!({
                    "connected": true,
                    "url": connection()?.base,
                    "user_id": me.get("user_id").cloned().unwrap_or(Value::Null)
                }),
                cli.pretty,
            );
        }
        Command::Logout => {
            let path = config_path()?;
            if path.exists() {
                fs::remove_file(path)?;
            }
            return emit(&json!({"local_credentials_removed": true}), cli.pretty);
        }
        Command::Template { kind } => return emit(&template(kind), cli.pretty),
        _ => {}
    }

    if matches!(cli.command, Command::Sync) {
        return emit(&sync_skill()?, cli.pretty);
    }
    ensure_current_skill()?;
    if let Command::Watch {
        seconds,
        after,
        cursor_file,
    } = &cli.command
    {
        return watch_events(*seconds, *after, cursor_file.as_deref(), cli.pretty);
    }

    let result = match cli.command {
        Command::Recall {
            keywords,
            limit,
            scope,
        } => call(
            Method::POST,
            "/v1/search",
            Some(&json!({"keywords": keywords, "limit": limit, "scope": scope})),
        )?,
        Command::Remember {
            content,
            keywords,
            kind,
            source,
            tags,
            metadata,
        } => call(
            Method::POST,
            "/v1/memories",
            Some(&json!({
                "content": content,
                "kind": kind,
                "source": source,
                "tags": tags,
                "keywords": keywords,
                "metadata": json_object(&metadata)?
            })),
        )?,
        Command::Entity {
            name,
            entity_type,
            keywords,
            description,
            source,
            tags,
            metadata,
        } => {
            if entity_type.is_none() || keywords.is_empty() {
                let path = PathBuf::from(&name);
                if !path.is_file() {
                    bail!("entity requires --type and at least one --keyword");
                }
                call(Method::POST, "/v1/entities", Some(&file_json(&path)?))?
            } else {
                call(
                    Method::POST,
                    "/v1/entities",
                    Some(&json!({
                        "name": name,
                        "entity_type": entity_type,
                        "description": description,
                        "source": source,
                        "tags": tags,
                        "keywords": keywords,
                        "metadata": json_object(&metadata)?
                    })),
                )?
            }
        }
        Command::Document {
            title,
            keywords,
            file_id,
            content,
            content_file,
            source,
            tags,
            metadata,
        } => {
            let content = if let Some(path) = content_file {
                fs::read_to_string(path)?
            } else {
                content
            };
            call(
                Method::POST,
                "/v1/documents",
                Some(&json!({
                    "title": title,
                    "file_id": if file_id.is_empty() { Value::Null } else { Value::String(file_id) },
                    "content": content,
                    "source": source,
                    "tags": tags,
                    "keywords": keywords,
                    "metadata": json_object(&metadata)?
                })),
            )?
        }
        Command::Upload { file } => upload_file(&file)?,
        Command::Download { id, output } => download_file(&id, &output)?,
        Command::Relate {
            source_id,
            target_id,
            relation,
            reason,
        } => call(
            Method::POST,
            "/v1/relations",
            Some(&json!({
                "source_id": source_id,
                "target_id": target_id,
                "relation": relation,
                "reason": reason
            })),
        )?,
        Command::RelationStatus {
            id,
            status,
            reason,
            version,
        } => call(
            Method::PATCH,
            &format!("/v1/relations/{}", quote(&id)),
            Some(&json!({
                "status": status,
                "reason": reason,
                "expected_version": version
            })),
        )?,
        Command::Keywords { id, keywords } => call(
            Method::PUT,
            &format!("/v1/records/{}/keywords", quote(&id)),
            Some(&json!({"keywords": keywords})),
        )?,
        Command::Feedback {
            title,
            description,
            agent_name,
            use_case,
            priority,
            source,
            client_request_id,
        } => call(
            Method::POST,
            "/v1/feature-requests",
            Some(&json!({
                "title": title,
                "description": description,
                "agent_name": agent_name,
                "use_case": use_case,
                "priority": priority,
                "source": source,
                "client_request_id": if client_request_id.is_empty() { Value::Null } else { Value::String(client_request_id) }
            })),
        )?,
        Command::FeedbackList => call(Method::GET, "/v1/feature-requests", None)?,
        Command::FeedbackGet { id } => call(
            Method::GET,
            &format!("/v1/feature-requests/{}", quote(&id)),
            None,
        )?,
        Command::FromJson { kind, payload } => {
            let path = match kind.as_str() {
                "memory" => "/v1/memories",
                "entity" => "/v1/entities",
                "document" => "/v1/documents",
                _ => "/v1/feature-requests",
            };
            call(Method::POST, path, Some(&file_json(&payload)?))?
        }
        Command::List {
            kind,
            limit,
            offset,
        } => {
            let mut endpoint = format!("/v1/records?limit={limit}&offset={offset}");
            if let Some(kind) = kind {
                endpoint.push_str("&kind=");
                endpoint.push_str(&quote(&kind));
            }
            call(Method::GET, &endpoint, None)?
        }
        Command::Get { id } => call(Method::GET, &format!("/v1/records/{}", quote(&id)), None)?,
        Command::Forget { id } => {
            call(Method::DELETE, &format!("/v1/records/{}", quote(&id)), None)?
        }
        Command::Graph { id } => call(Method::GET, &format!("/v1/graph/{}", quote(&id)), None)?,
        Command::Watch { .. }
        | Command::Sync
        | Command::Login { .. }
        | Command::Configure
        | Command::Status
        | Command::Logout
        | Command::Template { .. } => unreachable!(),
    };
    emit(&result, cli.pretty)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Vibe Memory: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_origins() {
        assert_eq!(
            validate_origin("https://vibememory.io/").unwrap(),
            "https://vibememory.io"
        );
        assert!(validate_origin("http://remote.example").is_err());
        assert!(validate_origin("https://user:secret@example.com").is_err());
        assert!(validate_origin("https://example.com/path").is_err());
        assert!(validate_origin("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn translates_legacy_commands() {
        let values = vec!["vibe-memory", "ingest", "payload.json"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let normalized = normalize_legacy_arguments(values);
        assert_eq!(
            normalized,
            ["vibe-memory", "from-json", "document", "payload.json"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn templates_are_objects() {
        assert_eq!(template("entity")["entity_type"], "Person");
        assert!(template("document")["metadata"].is_object());
    }

    #[test]
    fn hashes_match_known_value() {
        assert_eq!(
            sha256(b"vibe-memory"),
            "8704e001764e69d1797ffba8fe559da86c06c97b889dc44b22f363f1e3509483"
        );
    }
}
