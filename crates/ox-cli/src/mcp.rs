//! A deliberately small, tools-only MCP 2025-11-25 stdio adapter for the authoritative HTTP host.
//! No second project cache and no generic dispatch/file tool: every exposed command is listed here.
use ox_core::store::Store;
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex},
    time::Duration,
};

const VERSION: &str = "2025-11-25";
const MAX_INPUT: usize = 1024 * 1024;
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_HTTP: u64 = 8 * 1024 * 1024;
const MUTATION_NOTE: &str = "A failed or interrupted mutation may already have applied changes. Do not retry it automatically. Use open_project to inspect the current revision/history and undo if needed.";

#[derive(Clone)]
struct Backend { url: String, token: Option<String>, agent: ureq::Agent }
impl Backend {
    fn call(&self, owner: &str, cmd: &str, args: &Value, context: Option<(&str, &str)>) -> Result<Value, String> {
        let mut request = self.agent.post(format!("{}/api/{cmd}", self.url)).header("Content-Type", "application/json").header("X-Ox-Session", owner);
        if let Some(token) = &self.token { request = request.header("Authorization", format!("Bearer {token}")); }
        if let Some((project, revision)) = context {
            request = request.header("X-Ox-Project", project).header("X-Ox-Revision", revision);
        }
        // Never follow redirects, use environment proxies, or retry a mutation after a lost reply.
        let mut response = request.send(args.to_string().as_bytes()).map_err(|_| "Could not complete the local OxRefine request (connection, timeout or invalid HTTP response).".to_string())?;
        let status = response.status().as_u16();
        let body = response.body_mut().with_config().limit(MAX_HTTP).read_to_string().map_err(|_| "Could not read the OxRefine reply (it may exceed the 8 MiB limit). Narrow the query or export to an allowed file.".to_string())?;
        let value: Value = serde_json::from_str(&body).map_err(|_| "OxRefine returned invalid JSON.".to_string())?;
        if status != 200 { return Err(format!("OxRefine HTTP {status}: {value}")); }
        Ok(value)
    }

    fn roots(&self, data: &Path) -> Result<Vec<PathBuf>, String> {
        let value = self.call("mcp-info", "server_info", &json!({}), None)?;
        let root = |key: &str| value["data"][key].as_str().map(PathBuf::from).ok_or_else(|| "The server does not support MCP data-folder verification. Update ox serve.".to_string());
        if root("dataRoot")? != data { return Err("--data does not match this ox serve data folder. Start the server and adapter with the same --data DIR.".into()); }
        Ok(vec![data.join("projects"), data.join("staging"), root("projectsRoot")?, root("stagingRoot")?])
    }

    fn cancel(&self, owner: &str) {
        let fast = Self { agent: agent(Duration::from_secs(2)), ..self.clone() };
        let _ = fast.call(owner, "cancel_process", &json!({}), None);
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).proxy(None)
        .timeout_connect(Some(Duration::from_secs(2))).timeout_global(Some(timeout)).build())
}

struct Grant { path: PathBuf, directory: bool }
struct Scratch { directory: PathBuf, file: PathBuf }
impl Scratch {
    fn create(target: &Path) -> Result<Self, String> {
        let directory = target.parent().ok_or("Export needs a parent directory")?.join(format!(".ox-mcp-{}", super::new_token()?));
        std::fs::create_dir(&directory).map_err(|e| format!("Could not create export scratch directory: {e}"))?;
        let file = directory.join("export");
        Ok(Self { directory, file })
    }
    fn publish(&self, target: &Path) -> Result<(), String> {
        // An atomic new directory entry, unlike exists()+rename()/copy()/File::create().
        // Unsupported filesystems fail closed; there is no overwrite fallback.
        std::fs::hard_link(&self.file, target).map_err(|e| format!("Could not publish export without overwriting: {e}. The destination must not exist and its filesystem must support hard links."))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
struct Adapter { backend: Backend, data: PathBuf, grants: Vec<Grant> }
impl Adapter {
    fn from_args(args: &[&str]) -> Result<Self, String> {
        let mut url = "http://127.0.0.1:3030".to_string();
        let mut data = Store::default_root();
        let mut allowed = vec![];
        let mut token = std::env::var("OXREFINE_TOKEN").ok().filter(|s| !s.is_empty());
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let value = it.next().ok_or_else(|| format!("{arg} requires a value"))?;
            match *arg {
                "--url" => url = value.trim_end_matches('/').to_string(),
                "--data" => data = PathBuf::from(value),
                "--allow" => allowed.push(PathBuf::from(value)),
                "--token" => token = Some(value.to_string()),
                _ => return Err(format!("unknown MCP option {arg}")),
            }
        }
        let uri: ureq::http::Uri = url.parse().map_err(|_| "--url must be http://127.0.0.1:PORT or http://localhost:PORT")?;
        if uri.scheme_str() != Some("http") || !matches!(uri.host(), Some("127.0.0.1" | "localhost"))
            || uri.port_u16().is_none_or(|p| p == 0) || uri.path() != "/" || uri.query().is_some()
            || uri.authority().is_some_and(|a| a.as_str().contains('@')) {
            return Err("--url must be an HTTP loopback address with a port and no credentials, path or query".into());
        }
        let data = data.canonicalize().map_err(|_| "--data must be an existing data folder used by the running ox serve")?;
        if !data.is_dir() { return Err("--data must be a directory".into()); }
        allowed.push(data.clone());
        let grants = allowed.into_iter().map(|path| {
            let path = path.canonicalize().map_err(|e| format!("--allow {}: {e}", path.display()))?;
            let directory = path.is_dir();
            if !directory && !path.is_file() { return Err("--allow must name an existing regular file or directory".into()); }
            Ok(Grant { path, directory })
        }).collect::<Result<_, String>>()?;
        let backend = Backend { url, token, agent: agent(Duration::from_secs(600)) };
        backend.roots(&data)?;
        Ok(Self { backend, data, grants })
    }

    fn file(&self, path: &str, write: bool, blocked: &[PathBuf]) -> Result<String, String> {
        let path = Path::new(path);
        let resolved = if write {
            // Existing outputs are refused, including symlinks and hard links. No accidental overwrite.
            if std::fs::symlink_metadata(path).is_ok() { return Err("Export destination already exists; choose a new filename.".into()); }
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
            let name = path.file_name().ok_or("Export needs a filename")?;
            parent.canonicalize().map_err(|_| "Export parent directory must already exist")?.join(name)
        } else {
            let resolved = path.canonicalize().map_err(|_| "Import file does not exist or cannot be resolved")?;
            if !resolved.is_file() { return Err("Import requires a regular file; folder imports are not exposed over MCP.".into()); }
            resolved
        };
        if blocked.iter().any(|root| resolved.starts_with(root)) {
            return Err("Direct access to managed projects/staging is not allowed. Use project tools and export to a separate directory.".into());
        }
        if !self.grants.iter().any(|grant| resolved == grant.path || grant.directory && resolved.starts_with(&grant.path)) {
            return Err("File is outside the MCP grants. Restart ox mcp with --allow FILE or --allow DIRECTORY for this location.".into());
        }
        resolved.to_str().map(str::to_owned).ok_or_else(|| "File path must be valid UTF-8".into())
    }

    fn tool(&self, name: &str, args: &Map<String, Value>, owner: &str, cancelled: &AtomicBool) -> Result<Value, String> {
        let blocked = self.backend.roots(&self.data)?;
        if cancelled.load(Ordering::Relaxed) { return Err("Request cancelled before execution.".into()); }
        let mut input = Value::Object(args.clone());
        let context = args.get("project").and_then(Value::as_str).zip(args.get("revision").and_then(Value::as_str));
        let mut export = None;
        let cmd = match name {
            "list_projects" => {
                input["params"] = json!({"paginate":true});
                if !args.contains_key("start") { input["start"] = json!(0); }
                if !args.contains_key("limit") { input["limit"] = json!(100); }
                "projects"
            }
            "create_project" => {
                input["path"] = json!(self.file(args["path"].as_str().unwrap(), false, &blocked)?);
                if args.get("archive").and_then(Value::as_bool).unwrap_or(false) { "import_project" } else { "open" }
            }
            "open_project" => { input["id"] = args["project"].clone(); "open_project" }
            "profile_columns" => "profile",
            "read_rows" => { if !args.contains_key("limit") { input["limit"] = json!(100); } "rows" }
            "facet_counts" => "facets",
            "cluster_column" => { if !args.contains_key("method") { input["method"] = json!("fingerprint"); } "cluster" }
            "preview_expression" => { if !args.contains_key("limit") { input["limit"] = json!(10); } "preview" }
            "apply_operation" => "apply",
            "apply_recipe" => "apply_recipe",
            "undo" => "goto",
            "export" => {
                let target = self.file(args["path"].as_str().unwrap(), true, &blocked)?;
                let scratch = Scratch::create(Path::new(&target))?;
                input["path"] = json!(scratch.file);
                export = Some((scratch, target));
                match args["format"].as_str().unwrap() {
                    "project" => "export_project", "original-project" => "export_original_project",
                    "recipe" => { input["params"]["fileReply"] = json!(true); "export_recipe" }
                    _ => "export",
                }
            }
            _ => return Err("unknown tool".into()),
        };
        let mut value = self.backend.call(owner, cmd, &input, context)?;
        if let Some((scratch, target)) = export {
            scratch.publish(Path::new(&target))?;
            if matches!(cmd, "export_project" | "export_original_project" | "export_recipe") { value["data"] = json!(target); }
        }
        if value.to_string().len() > MAX_OUTPUT / 2 {
            return Err("Tool result exceeds the 2 MiB data limit. Narrow the query or export to an allowed file.".into());
        }
        Ok(value)
    }
}

fn tools() -> Vec<Value> {
    let string = json!({"type":"string", "minLength":1, "maxLength":4096});
    let object = json!({"type":"object"});
    let integer = |max| json!({"type":"integer", "minimum":0, "maximum":max});
    let scoped = |extra: Value| {
        let mut fields = json!({"project":string, "revision":string}).as_object().unwrap().clone();
        fields.extend(extra.as_object().unwrap().clone()); Value::Object(fields)
    };
    let define = |name: &str, description: &str, properties: Value, required: &[&str], readonly: bool, network: bool| json!({
        "name":name, "description":description,
        "inputSchema":{"type":"object", "properties":properties, "required":required, "additionalProperties":false},
        "annotations":{"readOnlyHint":readonly, "destructiveHint":!readonly, "idempotentHint":readonly, "openWorldHint":network}
    });
    vec![
        define("list_projects", "List saved projects, metadata and revisions. Paginated; use open_project to inspect the current editable view.", json!({"start":integer(1_000_000), "limit":{"type":"integer","minimum":1,"maximum":500}}), &[], true, false),
        define("create_project", "Import an allowed file into a new saved project. Set archive=true for an OpenRefine project archive, including history. Returns data plus context.project/revision. File imports only.", json!({"path":string, "name":string, "format":string, "options":object, "archive":{"type":"boolean"}}), &["path"], false, false),
        define("open_project", "Inspect/refresh a saved project, including column names, history and cursor. Returns the current context.project/revision; use them explicitly in subsequent tools. Does not switch another browser's project.", json!({"project":string}), &["project"], true, false),
        define("profile_columns", "Profile all columns (types, blanks, errors and statistics) across the whole project. Requires the inspected revision.", scoped(json!({})), &["project","revision"], true, false),
        define("read_rows", "Read a page of rows plus matching total. Rows are [index,cells,starred,flagged,types,record?,reconciliation?]. engine is OpenRefine engineConfig (mode/facets); omitted means all rows.", scoped(json!({"engine":object,"start":integer(1_000_000_000),"limit":integer(1000)})), &["project","revision"], true, false),
        define("facet_counts", "Compute OpenRefine facet counts for engine.facets and mode. Honors the server's facet choice limit; response includes facet metadata.", scoped(json!({"engine":object})), &["project","revision","engine"], true, false),
        define("cluster_column", "Suggest clusters without changing data. method is an OxRefine keyer (default fingerprint), levenshtein or ppm; optional n, radius, block. engine restricts matching rows.", scoped(json!({"column":string,"engine":object,"method":string,"n":integer(100),"radius":{"type":"number","minimum":0,"maximum":100},"block":integer(100)})), &["project","revision","column"], true, false),
        define("preview_expression", "Preview GREL before/after pairs without changing history, over up to 100 matching rows. Supply column, expression and optional OpenRefine engineConfig.", scoped(json!({"column":string,"expression":string,"engine":object,"limit":integer(100)})), &["project","revision","column","expression"], true, false),
        define("apply_operation", "Apply one OpenRefine operation object as a saved undoable history step. A failure may leave changes: never retry automatically; open_project to inspect history/revision first. URL/reconciliation operations may access the network.", scoped(json!({"op":object})), &["project","revision","op"], false, true),
        define("apply_recipe", "Apply up to 1000 OpenRefine operation objects through normal saved history. All are parsed before running; a runtime failure can leave earlier steps applied. Never retry automatically; inspect open_project and undo explicitly. Network operations are supported.", scoped(json!({"ops":{"type":"array","items":object,"maxItems":1000}})), &["project","revision","ops"], false, true),
        define("undo", "Move saved history to an explicit cursor index (0 = original table). Can undo or redo; inspect open_project history/cursor first. Returns the new revision.", scoped(json!({"index":integer(1_000_000)})), &["project","revision","index"], false, true),
        define("export", "Export matching rows to a NEW file inside an allowed directory (existing files are refused). format=project exports archive/history; original-project copies original archive; recipe exports operation history. Other formats use OpenRefine exporter params. Does not change history.", scoped(json!({"path":string,"format":{"type":"string","enum":["csv","tsv","*sv","html","xls","xlsx","ods","sql","template","project","original-project","recipe"]},"engine":object,"params":object})), &["project","revision","path","format"], false, true),
    ]
}

// Validate exactly the advertised outer contract. Engine/options/operation objects are parsed by
// the shared engine; they never become arbitrary HTTP command names or file paths.
fn validate(schema: &Value, args: &Value) -> Result<Map<String, Value>, String> {
    let args = args.as_object().ok_or("arguments must be an object")?;
    let properties = schema["properties"].as_object().unwrap();
    for required in schema["required"].as_array().unwrap() {
        let key = required.as_str().unwrap();
        if !args.contains_key(key) { return Err(format!("missing argument {key}")); }
    }
    for (key, value) in args {
        let rule = properties.get(key).ok_or_else(|| format!("unknown argument {key}"))?;
        let valid_type = match rule["type"].as_str().unwrap() {
            "string" => value.is_string(), "object" => value.is_object(), "boolean" => value.is_boolean(),
            "integer" => value.as_u64().is_some(), "number" => value.is_number(), "array" => value.is_array(), _ => false,
        };
        if !valid_type { return Err(format!("invalid type for {key}")); }
        if let Some(s) = value.as_str() {
            let len = s.chars().count() as u64;
            if rule["minLength"].as_u64().is_some_and(|min| len < min) || rule["maxLength"].as_u64().is_some_and(|max| len > max)
                || rule["enum"].as_array().is_some_and(|values| !values.contains(value)) { return Err(format!("invalid value for {key}")); }
        }
        if let Some(n) = value.as_f64() {
            if rule["minimum"].as_f64().is_some_and(|min| n < min) || rule["maximum"].as_f64().is_some_and(|max| n > max) { return Err(format!("{key} is outside its allowed range")); }
        }
        if let Some(items) = value.as_array() {
            if rule["maxItems"].as_u64().is_some_and(|max| items.len() as u64 > max) || items.iter().any(|v| !v.is_object()) { return Err(format!("invalid recipe array {key}")); }
        }
    }
    Ok(args.clone())
}

type Output = Arc<Mutex<io::Stdout>>;
fn write(output: &Output, value: Value) -> Result<(), String> {
    let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
    serde_json::to_writer(&mut *output, &value).map_err(|e| e.to_string())?;
    output.write_all(b"\n").and_then(|_| output.flush()).map_err(|e| e.to_string())
}
fn error(output: &Output, id: Value, code: i64, message: &str) -> Result<(), String> {
    write(output, json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
}
fn reply(output: &Output, id: Value, result: Value) -> Result<(), String> {
    write(output, json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn tool_error(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}

struct Pending { owner: String, cancelled: AtomicBool, finished: AtomicBool }
struct Work { id: Value, name: String, args: Map<String, Value>, pending: Arc<Pending> }
type Requests = Arc<Mutex<HashMap<String, Arc<Pending>>>>;

fn cancel(backend: &Backend, pending: Arc<Pending>) {
    if pending.cancelled.swap(true, Ordering::Relaxed) { return; }
    let backend = backend.clone();
    std::thread::spawn(move || {
        // A notification can beat the HTTP job registration. Keep cancellation scoped to this
        // request until completion, never to a shared client or a later queued operation.
        while !pending.finished.load(Ordering::Relaxed) {
            backend.cancel(&pending.owner);
            std::thread::sleep(Duration::from_millis(30));
        }
    });
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let adapter = Adapter::from_args(args)?;
    let backend = adapter.backend.clone();
    let output = Arc::new(Mutex::new(io::stdout()));
    let requests: Requests = Arc::new(Mutex::new(HashMap::new()));
    let (sender, receiver) = mpsc::sync_channel::<Work>(4);
    let (worker_output, worker_requests) = (output.clone(), requests.clone());
    let worker = std::thread::spawn(move || {
        while let Ok(work) = receiver.recv() {
            let mut result = match adapter.tool(&work.name, &work.args, &work.pending.owner, &work.pending.cancelled) {
                Ok(value) => json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
                Err(message) => {
                    let note = if matches!(work.name.as_str(), "create_project" | "apply_operation" | "apply_recipe" | "undo" | "export") { format!(" {MUTATION_NOTE}") } else { String::new() };
                    tool_error(&format!("{message}{note}"))
                }
            };
            if result.to_string().len() > MAX_OUTPUT {
                result = json!({"content":[{"type":"text","text":format!("Tool reply exceeds 4 MiB. {MUTATION_NOTE}")}],"isError":true});
            }
            work.pending.finished.store(true, Ordering::Relaxed);
            worker_requests.lock().unwrap_or_else(|e| e.into_inner()).remove(&work.id.to_string());
            if reply(&worker_output, work.id, result).is_err() { break; }
        }
    });
    let stdin = io::stdin(); let mut reader = stdin.lock();
    let definitions = tools();
    let (mut initialized, mut ready, mut serial) = (false, false, 0u64);
    let session = super::new_token()?;
    let result = (|| -> Result<(), String> {
        loop {
            let mut frame = vec![];
            let n = reader.by_ref().take((MAX_INPUT + 1) as u64).read_until(b'\n', &mut frame).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            if n > MAX_INPUT { error(&output, Value::Null, -32700, "MCP message exceeds 1 MiB; closing transport")?; break; }
            if !frame.ends_with(b"\n") { error(&output, Value::Null, -32700, "MCP messages must end with a newline")?; break; }
            let message: Value = match serde_json::from_slice(&frame) {
                Ok(value) => value,
                Err(_) => { error(&output, Value::Null, -32700, "Invalid JSON")?; continue; }
            };
            let id = message.get("id").cloned();
            if !message.is_object() || message["jsonrpc"] != "2.0" || !message["method"].is_string()
                || id.as_ref().is_some_and(|id| !id.is_string() && id.as_i64().is_none()) {
                error(&output, Value::Null, -32600, "Invalid JSON-RPC request")?; continue;
            }
            let method = message["method"].as_str().unwrap();
            let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
            if id.is_none() {
                match method {
                    "notifications/initialized" if initialized => ready = true,
                    "notifications/cancelled" => {
                        if let Some(id) = params.get("requestId") {
                            let pending = requests.lock().unwrap_or_else(|e| e.into_inner()).get(&id.to_string()).cloned();
                            if let Some(pending) = pending { cancel(&backend, pending); }
                        }
                    }
                    _ => {}, // Notifications never get a response, including unknown notifications.
                }
                continue;
            }
            let id = id.unwrap();
            if requests.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id.to_string()) {
                error(&output, id, -32600, "Duplicate active request id")?; continue;
            }
            if !params.is_object() { error(&output, id, -32602, "params must be an object")?; continue; }
            match method {
                "initialize" if !initialized => {
                    if !params["protocolVersion"].is_string() || !params["capabilities"].is_object() || !params["clientInfo"].is_object()
                        || !params["clientInfo"]["name"].is_string() || !params["clientInfo"]["version"].is_string() {
                        error(&output, id, -32602, "initialize requires protocolVersion, capabilities and clientInfo name/version")?; continue;
                    }
                    initialized = true;
                    reply(&output, id, json!({"protocolVersion":VERSION,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"oxrefine","version":env!("CARGO_PKG_VERSION")},"instructions":"Tools attach to a running local ox serve. Inspect open_project before using explicit project/revision arguments. Edits use saved undo history. A stale revision is rejected; do not silently retry mutations. File tools are limited to invocation grants."}))?;
                }
                "initialize" => error(&output, id, -32600, "Already initialized")?,
                "ping" => reply(&output, id, json!({}))?,
                "tools/list" | "tools/call" if !ready => error(&output, id, -32000, "Initialize and send notifications/initialized before calling tools")?,
                "tools/list" => {
                    if params.as_object().unwrap().keys().any(|key| key != "_meta") || params.get("_meta").is_some_and(|value| !value.is_object()) {
                        error(&output, id, -32602, "tools/list accepts only optional _meta; all tools fit in one page")?;
                    }
                    else { reply(&output, id, json!({"tools":definitions}))?; }
                }
                "tools/call" => {
                    if params.as_object().unwrap().keys().any(|key| !matches!(key.as_str(), "name" | "arguments" | "_meta")) || params.get("_meta").is_some_and(|value| !value.is_object()) {
                        error(&output, id, -32602, "Unknown tools/call parameter")?; continue;
                    }
                    let Some(definition) = definitions.iter().find(|tool| tool["name"] == params["name"] && params["name"].is_string()) else {
                        error(&output, id, -32602, "Unknown tool name")?; continue;
                    };
                    if params.get("arguments").is_some_and(|value| !value.is_object()) {
                        error(&output, id, -32602, "arguments must be an object")?; continue;
                    }
                    let args = match validate(&definition["inputSchema"], params.get("arguments").unwrap_or(&json!({}))) {
                        Ok(args) => args,
                        Err(message) => { reply(&output, id, tool_error(&message))?; continue; }
                    };
                    serial += 1;
                    let pending = Arc::new(Pending { owner: format!("mcp-{session}-{serial}"), cancelled: AtomicBool::new(false), finished: AtomicBool::new(false) });
                    requests.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), pending.clone());
                    let work = Work { id: id.clone(), name: params["name"].as_str().unwrap().to_string(), args, pending: pending.clone() };
                    if sender.try_send(work).is_err() {
                        pending.finished.store(true, Ordering::Relaxed);
                        requests.lock().unwrap_or_else(|e| e.into_inner()).remove(&id.to_string());
                        error(&output, id, -32000, "MCP request queue is full; wait for outstanding tools")?;
                    }
                }
                _ => error(&output, id, -32601, "Method not found")?,
            }
        }
        Ok(())
    })();
    for pending in requests.lock().unwrap_or_else(|e| e.into_inner()).values() { cancel(&backend, pending.clone()); }
    drop(sender);
    let _ = worker.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_never_clobbers_a_destination_created_after_validation() {
        let root = std::env::temp_dir().join(format!("ox-mcp-publish-{}", crate::new_token().unwrap()));
        std::fs::create_dir(&root).unwrap();
        let target = root.join("result.csv");
        let scratch = Scratch::create(&target).unwrap();
        let directory = scratch.directory.clone();
        assert!(!target.exists()); // The earlier file grant/existence check has passed.
        std::fs::write(&scratch.file, b"new export").unwrap();
        std::fs::write(&target, b"another writer's data").unwrap();
        assert!(scratch.publish(&target).unwrap_err().contains("without overwriting"));
        assert_eq!(std::fs::read(&target).unwrap(), b"another writer's data");
        drop(scratch);
        assert!(!directory.exists(), "only the owned scratch file/directory should be cleaned");
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
