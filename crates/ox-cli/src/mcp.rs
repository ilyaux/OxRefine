//! A deliberately small, tools-only MCP 2025-11-25 server, over stdio (`ox mcp`) and Streamable
//! HTTP (`/mcp` of `ox serve`). Both call the one authoritative host of a data folder; there is no
//! second project cache and no generic dispatch/file tool: every exposed command is listed here.
use crate::web;
use ox_core::store::Store;
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

const VERSION: &str = "2025-11-25";
pub const MAX_INPUT: usize = 1024 * 1024;
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_HTTP: u64 = 8 * 1024 * 1024;
const MAX_SESSIONS: usize = 64;
const MUTATION_NOTE: &str = "A failed or interrupted mutation may already have applied changes. Do not retry it automatically. Use open_project to inspect the current revision/history and undo if needed.";
const MUTATING: [&str; 7] = ["create_project", "rename_project", "delete_project", "apply_operation", "apply_recipe", "undo", "export"];
const GREL_DOCS: &str = include_str!("../../../ui/src/grel-docs.json");

/// The host the tools call: a server over loopback HTTP, or the one in this process.
#[derive(Clone)]
enum Backend {
    Http { url: String, token: Option<String>, agent: ureq::Agent },
    Local(web::Host),
}
impl Backend {
    fn http(url: String, token: Option<String>) -> Self {
        Self::Http { url, token, agent: agent(Duration::from_secs(600)) }
    }

    fn call(&self, owner: &str, cmd: &str, args: &Value, context: Option<(&str, &str)>) -> Result<Value, String> {
        let (status, value) = match self {
            Self::Http { url, token, agent } => {
                let mut request = agent.post(format!("{url}/api/{cmd}")).header("Content-Type", "application/json").header("X-Ox-Session", owner);
                if let Some(token) = token { request = request.header("Authorization", format!("Bearer {token}")); }
                if let Some((project, revision)) = context {
                    request = request.header("X-Ox-Project", project).header("X-Ox-Revision", revision);
                }
                // Never follow redirects, use environment proxies, or retry a mutation after a lost reply.
                let mut response = request.send(args.to_string().as_bytes()).map_err(|_| "Could not complete the local OxRefine request (connection, timeout or invalid HTTP response). If the OxRefine server stopped, restart this MCP server.".to_string())?;
                let status = response.status().as_u16();
                let body = response.body_mut().with_config().limit(MAX_HTTP).read_to_string().map_err(|_| "Could not read the OxRefine reply (it may exceed the 8 MiB limit). Narrow the query or export to an allowed file.".to_string())?;
                (status, serde_json::from_str(&body).map_err(|_| "OxRefine returned invalid JSON.".to_string())?)
            }
            Self::Local(host) => {
                let scope = web::Scope::new(Some(owner.into()), context.map(|c| c.0.into()), context.map(|c| c.1.into()))?;
                let web::Reply(status, value) = host.api(cmd, args.clone(), &scope);
                (status, value)
            }
        };
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
        let fast = match self {
            Self::Http { url, token, .. } => Self::Http { url: url.clone(), token: token.clone(), agent: agent(Duration::from_secs(2)) },
            local => local.clone(),
        };
        let _ = fast.call(owner, "cancel_process", &json!({}), None);
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).proxy(None)
        .timeout_connect(Some(Duration::from_secs(2))).timeout_global(Some(timeout)).build())
}

fn loopback(url: &str) -> Result<String, String> {
    let uri: ureq::http::Uri = url.parse().map_err(|_| "--url must be http://127.0.0.1:PORT or http://localhost:PORT")?;
    if uri.scheme_str() != Some("http") || !matches!(uri.host(), Some("127.0.0.1" | "localhost"))
        || uri.port_u16().is_none_or(|p| p == 0) || uri.path() != "/" || uri.query().is_some()
        || uri.authority().is_some_and(|a| a.as_str().contains('@')) {
        return Err("--url must be an HTTP loopback address with a port and no credentials, path or query".into());
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// Without --url: the server of this data folder (an `ox serve`, or another `ox mcp` hosting it),
/// else a host in this process. One data folder never gets two hosts.
fn connect(data: &Path, token: Option<String>) -> Result<Backend, String> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        match web::claim(data) {
            web::Claim::Held(lock) => return Ok(Backend::Local(super::embedded(data, Some(lock))?)),
            web::Claim::Unsupported => return Ok(Backend::Local(super::embedded(data, None)?)),
            // the holder may still be starting: wait for its address and answer
            web::Claim::Busy => {
                if let Some((url, advertised)) = web::advertised(data) {
                    let backend = Backend::http(loopback(&url)?, advertised.or_else(|| token.clone()));
                    if backend.roots(data).is_ok() { return Ok(backend); }
                }
            }
        }
        if Instant::now() > until {
            return Err(format!("Another OxRefine holds {} but does not answer. Close it, or start ox serve and give ox mcp its --url.", data.display()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
    /// `data` is canonical; it is granted besides `allowed`.
    fn new(backend: Backend, data: PathBuf, allowed: &[PathBuf]) -> Result<Self, String> {
        let grants = allowed.iter().chain([&data]).map(|path| {
            let path = path.canonicalize().map_err(|e| format!("--allow {}: {e}", path.display()))?;
            let directory = path.is_dir();
            if !directory && !path.is_file() { return Err("--allow must name an existing regular file or directory".into()); }
            Ok(Grant { path, directory })
        }).collect::<Result<_, String>>()?;
        backend.roots(&data)?;
        Ok(Self { backend, data, grants })
    }

    fn from_args(args: &[&str]) -> Result<Self, String> {
        let mut url = None;
        let mut data = Store::default_root();
        let mut allowed = vec![];
        let mut token = std::env::var("OXREFINE_TOKEN").ok().filter(|s| !s.is_empty());
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let value = it.next().ok_or_else(|| format!("{arg} requires a value"))?;
            match *arg {
                "--url" => url = Some(loopback(value)?),
                "--data" => data = PathBuf::from(value),
                "--allow" => allowed.push(PathBuf::from(value)),
                "--token" => token = Some(value.to_string()),
                _ => return Err(format!("unknown MCP option {arg}")),
            }
        }
        // a standalone host creates its data folder, as ox serve does
        if url.is_none() { std::fs::create_dir_all(&data).map_err(|e| format!("--data {}: {e}", data.display()))?; }
        let data = data.canonicalize().map_err(|_| "--data must be an existing data folder used by the running ox serve")?;
        if !data.is_dir() { return Err("--data must be a directory".into()); }
        let backend = match url {
            Some(url) => Backend::http(url, token),
            None => connect(&data, token)?,
        };
        Self::new(backend, data, &allowed)
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
        let mut detected = None;
        let cmd = match name {
            "list_projects" => {
                input["params"] = json!({"paginate":true});
                if !args.contains_key("start") { input["start"] = json!(0); }
                if !args.contains_key("limit") { input["limit"] = json!(100); }
                "projects"
            }
            "create_project" => {
                let archive = args.get("archive").and_then(Value::as_bool).unwrap_or(false);
                match ["path", "url", "text"].map(|key| args.get(key).and_then(Value::as_str)) {
                    [Some(path), None, None] => input["path"] = json!(self.file(path, false, &blocked)?),
                    // a web address or pasted text is staged as a file first, as in the browser
                    [None, url, text] if url.is_some() != text.is_some() && !archive => {
                        let staged = match url {
                            Some(url) => self.backend.call(owner, "fetch_url", &json!({"url":url}), None)?,
                            None => self.backend.call(owner, "paste", &json!({"text":text}), None)?,
                        };
                        input["path"] = staged["data"]["path"].clone();
                        input["origin"] = staged["data"]["origin"].clone();
                        if !args.contains_key("name") { input["name"] = staged["data"]["name"].clone(); }
                    }
                    _ => return Err("Give exactly one of path, url or text; archive=true needs a path.".into()),
                }
                if archive { "import_project" } else { "open" }
            }
            "open_project" => { input["id"] = args["project"].clone(); "open_project" }
            "rename_project" | "delete_project" => { input["id"] = args["project"].clone(); name }
            "preview_import" => {
                input["path"] = json!(self.file(args["path"].as_str().unwrap(), false, &blocked)?);
                if !args.contains_key("format") || !args.contains_key("options") {
                    let guess = self.backend.call(owner, "inspect", &json!({"path":input["path"]}), None)?["data"].take();
                    if guess["format"] == "openrefine-project" { return Ok(json!({"data":{"detected":guess}})); }
                    if !args.contains_key("format") { input["format"] = guess["format"].clone(); }
                    if !args.contains_key("options") { input["options"] = guess["options"].clone(); }
                    detected = Some(guess);
                }
                if !args.contains_key("limit") { input["limit"] = json!(20); }
                "preview_import"
            }
            "profile_columns" => "profile",
            "read_rows" => { if !args.contains_key("limit") { input["limit"] = json!(100); } "rows" }
            "facet_counts" => "facets",
            "cluster_column" => { if !args.contains_key("method") { input["method"] = json!("fingerprint"); } "cluster" }
            "preview_expression" => { if !args.contains_key("limit") { input["limit"] = json!(10); } "preview" }
            "guess_reconcile_types" => { input["url"] = args["service"].clone(); "recon_guess_types" }
            "list_functions" => return Ok(json!({"data": functions(args.get("query").and_then(Value::as_str))?})),
            "list_extensions" => "extensions",
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
        if let Some(guess) = detected { value["data"]["detected"] = guess; }
        if value.to_string().len() > MAX_OUTPUT / 2 {
            return Err("Tool result exceeds the 2 MiB data limit. Narrow the query or export to an allowed file.".into());
        }
        Ok(value)
    }
}

/// OpenRefine's GREL reference (the expression editor's), optionally only entries mentioning `query`.
fn functions(query: Option<&str>) -> Result<Value, String> {
    let docs: Value = serde_json::from_str(GREL_DOCS).map_err(|e| e.to_string())?;
    let query = query.map(str::to_lowercase);
    let pick = |group: &str| -> Map<String, Value> {
        docs[group].as_object().into_iter().flatten()
            .filter(|(name, doc)| query.as_ref().is_none_or(|q| name.to_lowercase().contains(q) || doc.to_string().to_lowercase().contains(q)))
            .map(|(name, doc)| (name.clone(), doc.clone())).collect()
    };
    Ok(json!({"functions":pick("functions"), "controls":pick("controls"), "source":docs["source"],
        "note":"Extension functions are listed by list_extensions. Prefix an expression with python: or clojure: for those languages."}))
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
        define("preview_import", "Preview how an allowed file imports, without creating a project: the guessed format and OpenRefine import options (detected) and the first rows. Pass format/options to try others, then give the same to create_project.", json!({"path":string, "format":string, "options":object, "limit":{"type":"integer","minimum":1,"maximum":100}}), &["path"], true, false),
        define("create_project", "Create a saved project from exactly one of: path (an allowed file), url (a web address or Google Sheets link, downloaded first) or text (pasted CSV/TSV/JSON or other data). Set archive=true for an OpenRefine project archive at path, including history. Returns data plus context.project/revision.", json!({"path":string, "url":string, "text":{"type":"string","minLength":1,"maxLength":1_000_000}, "name":string, "format":string, "options":object, "archive":{"type":"boolean"}}), &[], false, true),
        define("open_project", "Inspect/refresh a saved project, including column names, history and cursor. Returns the current context.project/revision; use them explicitly in subsequent tools. Does not switch another browser's project.", json!({"project":string}), &["project"], true, false),
        define("rename_project", "Rename a saved project. Returns its metadata and new revision.", scoped(json!({"name":string})), &["project","revision","name"], false, false),
        define("delete_project", "Permanently delete a saved project and its history. This cannot be undone: confirm with the user first.", scoped(json!({})), &["project","revision"], false, false),
        define("profile_columns", "Profile all columns (types, blanks, errors and statistics) across the whole project. Requires the inspected revision.", scoped(json!({})), &["project","revision"], true, false),
        define("read_rows", "Read a page of rows plus matching total. Rows are [index,cells,starred,flagged,types,record?,reconciliation?]. engine is OpenRefine engineConfig (mode/facets); omitted means all rows.", scoped(json!({"engine":object,"start":integer(1_000_000_000),"limit":integer(1000)})), &["project","revision"], true, false),
        define("facet_counts", "Compute OpenRefine facet counts for engine.facets and mode. Honors the server's facet choice limit; response includes facet metadata.", scoped(json!({"engine":object})), &["project","revision","engine"], true, false),
        define("cluster_column", "Suggest clusters without changing data. method is an OxRefine keyer (default fingerprint), levenshtein or ppm; optional n, radius, block. engine restricts matching rows.", scoped(json!({"column":string,"engine":object,"method":string,"n":integer(100),"radius":{"type":"number","minimum":0,"maximum":100},"block":integer(100)})), &["project","revision","column"], true, false),
        define("preview_expression", "Preview before/after pairs of an expression without changing history, over up to 100 matching rows. GREL by default; prefix python: (or jython:) or clojure: for those languages. Supply column, expression and optional OpenRefine engineConfig.", scoped(json!({"column":string,"expression":string,"engine":object,"limit":integer(100)})), &["project","revision","column","expression"], true, false),
        define("list_functions", "OpenRefine's GREL function and control reference (parameters, return type, description). query filters by name or text, e.g. \"date\".", json!({"query":string}), &[], true, false),
        define("list_extensions", "Installed OxRefine extensions with their GREL functions, importers, exporters and operations (applied as ext/operation steps with apply_operation), and any load errors.", json!({}), &[], true, false),
        define("guess_reconcile_types", "Ask a reconciliation service (its endpoint URL in service) which types fit a column's values, best first. Use a type in a core/recon operation with apply_operation.", scoped(json!({"column":string,"service":string})), &["project","revision","column","service"], true, true),
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

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn tool_error(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}

struct Pending { owner: String, cancelled: AtomicBool, finished: AtomicBool }
struct Call { id: Value, name: String, args: Map<String, Value>, pending: Arc<Pending> }
enum Step { Reply(Value), Call(Call), Nothing }

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

/// One client: the stdio peer, or one Mcp-Session-Id over HTTP.
struct Peer {
    session: String,
    /// (initialize answered, notifications/initialized received)
    state: Mutex<(bool, bool)>,
    requests: Mutex<HashMap<String, Arc<Pending>>>,
    serial: AtomicU64,
    created: Instant,
}

impl Peer {
    fn new() -> Result<Self, String> {
        Ok(Self { session: super::new_token()?, state: Mutex::new((false, false)), requests: Mutex::default(), serial: AtomicU64::new(0), created: Instant::now() })
    }

    fn requests(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Pending>>> {
        self.requests.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Answer one JSON-RPC message. A tool call comes back registered, for the caller to run and `finish`.
    fn step(&self, message: &Value, definitions: &[Value], backend: &Backend) -> Step {
        let id = message.get("id").cloned();
        if !message.is_object() || message["jsonrpc"] != "2.0" || !message["method"].is_string()
            || id.as_ref().is_some_and(|id| !id.is_string() && id.as_i64().is_none()) {
            return Step::Reply(rpc_error(Value::Null, -32600, "Invalid JSON-RPC request"));
        }
        let method = message["method"].as_str().unwrap();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(id) = id else {
            match method {
                "notifications/initialized" if state.0 => state.1 = true,
                "notifications/cancelled" => {
                    if let Some(id) = params.get("requestId") {
                        let pending = self.requests().get(&id.to_string()).cloned();
                        if let Some(pending) = pending { cancel(backend, pending); }
                    }
                }
                _ => {}, // Notifications never get a response, including unknown notifications.
            }
            return Step::Nothing;
        };
        let fail = |code, message: &str| Step::Reply(rpc_error(id.clone(), code, message));
        if self.requests().contains_key(&id.to_string()) { return fail(-32600, "Duplicate active request id"); }
        if !params.is_object() { return fail(-32602, "params must be an object"); }
        match method {
            "initialize" if !state.0 => {
                if !params["protocolVersion"].is_string() || !params["capabilities"].is_object() || !params["clientInfo"].is_object()
                    || !params["clientInfo"]["name"].is_string() || !params["clientInfo"]["version"].is_string() {
                    return fail(-32602, "initialize requires protocolVersion, capabilities and clientInfo name/version");
                }
                state.0 = true;
                Step::Reply(rpc_result(id, json!({"protocolVersion":VERSION,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"oxrefine","version":env!("CARGO_PKG_VERSION")},"instructions":"Tools work on the projects of one OxRefine data folder, shared with its browser view. Inspect open_project before using explicit project/revision arguments. Edits use saved undo history. A stale revision is rejected; do not silently retry mutations. File tools are limited to the granted folders."})))
            }
            "initialize" => fail(-32600, "Already initialized"),
            "ping" => Step::Reply(rpc_result(id, json!({}))),
            "tools/list" | "tools/call" if !state.1 => fail(-32000, "Initialize and send notifications/initialized before calling tools"),
            "tools/list" => {
                if params.as_object().unwrap().keys().any(|key| key != "_meta") || params.get("_meta").is_some_and(|value| !value.is_object()) {
                    return fail(-32602, "tools/list accepts only optional _meta; all tools fit in one page");
                }
                Step::Reply(rpc_result(id, json!({"tools":definitions})))
            }
            "tools/call" => {
                if params.as_object().unwrap().keys().any(|key| !matches!(key.as_str(), "name" | "arguments" | "_meta")) || params.get("_meta").is_some_and(|value| !value.is_object()) {
                    return fail(-32602, "Unknown tools/call parameter");
                }
                let Some(definition) = definitions.iter().find(|tool| tool["name"] == params["name"] && params["name"].is_string()) else {
                    return fail(-32602, "Unknown tool name");
                };
                if params.get("arguments").is_some_and(|value| !value.is_object()) {
                    return fail(-32602, "arguments must be an object");
                }
                let args = match validate(&definition["inputSchema"], params.get("arguments").unwrap_or(&json!({}))) {
                    Ok(args) => args,
                    Err(message) => return Step::Reply(rpc_result(id, tool_error(&message))),
                };
                let serial = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
                let pending = Arc::new(Pending { owner: format!("mcp-{}-{serial}", self.session), cancelled: AtomicBool::new(false), finished: AtomicBool::new(false) });
                self.requests().insert(id.to_string(), pending.clone());
                Step::Call(Call { id, name: params["name"].as_str().unwrap().to_string(), args, pending })
            }
            _ => fail(-32601, "Method not found"),
        }
    }

    fn finish(&self, call: &Call) {
        call.pending.finished.store(true, Ordering::Relaxed);
        self.requests().remove(&call.id.to_string());
    }

    fn cancel_all(&self, backend: &Backend) {
        for pending in self.requests().values() { cancel(backend, pending.clone()); }
    }
}

/// Run a registered tool call; the JSON-RPC response.
fn execute(adapter: &Adapter, call: &Call) -> Value {
    let mut result = match adapter.tool(&call.name, &call.args, &call.pending.owner, &call.pending.cancelled) {
        Ok(value) => json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}),
        Err(message) => {
            let note = if MUTATING.contains(&call.name.as_str()) { format!(" {MUTATION_NOTE}") } else { String::new() };
            tool_error(&format!("{message}{note}"))
        }
    };
    if result.to_string().len() > MAX_OUTPUT {
        result = json!({"content":[{"type":"text","text":format!("Tool reply exceeds 4 MiB. {MUTATION_NOTE}")}],"isError":true});
    }
    rpc_result(call.id.clone(), result)
}

type Output = Arc<Mutex<io::Stdout>>;
fn write(output: &Output, value: Value) -> Result<(), String> {
    let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
    serde_json::to_writer(&mut *output, &value).map_err(|e| e.to_string())?;
    output.write_all(b"\n").and_then(|_| output.flush()).map_err(|e| e.to_string())
}

/// `ox mcp`: newline-delimited JSON-RPC on stdin/stdout; tools run one at a time on a worker.
pub fn run(args: &[&str]) -> Result<(), String> {
    let adapter = Arc::new(Adapter::from_args(args)?);
    let peer = Arc::new(Peer::new()?);
    let output = Arc::new(Mutex::new(io::stdout()));
    let (sender, receiver) = mpsc::sync_channel::<Call>(4);
    let (worker_adapter, worker_peer, worker_output) = (adapter.clone(), peer.clone(), output.clone());
    // a large stack, as for the main thread: a standalone host evaluates Python expressions here
    let worker = std::thread::Builder::new().name("mcp".into()).stack_size(16 << 20).spawn(move || {
        while let Ok(call) = receiver.recv() {
            let reply = execute(&worker_adapter, &call);
            worker_peer.finish(&call);
            if write(&worker_output, reply).is_err() { break; }
        }
        ox_core::grel::release_thread();
    }).map_err(|e| e.to_string())?;
    let stdin = io::stdin(); let mut reader = stdin.lock();
    let definitions = tools();
    let result = (|| -> Result<(), String> {
        loop {
            let mut frame = vec![];
            let n = reader.by_ref().take((MAX_INPUT + 1) as u64).read_until(b'\n', &mut frame).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            if n > MAX_INPUT { write(&output, rpc_error(Value::Null, -32700, "MCP message exceeds 1 MiB; closing transport"))?; break; }
            if !frame.ends_with(b"\n") { write(&output, rpc_error(Value::Null, -32700, "MCP messages must end with a newline"))?; break; }
            let Ok(message) = serde_json::from_slice::<Value>(&frame) else {
                write(&output, rpc_error(Value::Null, -32700, "Invalid JSON"))?; continue;
            };
            match peer.step(&message, &definitions, &adapter.backend) {
                Step::Reply(reply) => write(&output, reply)?,
                Step::Nothing => {}
                Step::Call(call) => {
                    if let Err(mpsc::TrySendError::Full(call) | mpsc::TrySendError::Disconnected(call)) = sender.try_send(call) {
                        peer.finish(&call);
                        write(&output, rpc_error(call.id, -32000, "MCP request queue is full; wait for outstanding tools"))?;
                    }
                }
            }
        }
        Ok(())
    })();
    peer.cancel_all(&adapter.backend);
    drop(sender);
    let _ = worker.join();
    result
}

/// `/mcp` of `ox serve`: MCP Streamable HTTP. Each POST gets one JSON reply (no event streams);
/// sessions are named by Mcp-Session-Id. Files: the data folder and `ox serve --allow` paths.
pub struct Http { adapter: Adapter, definitions: Vec<Value>, peers: Mutex<HashMap<String, Arc<Peer>>> }

impl Http {
    pub fn new(host: web::Host, data: &Path, allowed: &[PathBuf]) -> Result<Self, String> {
        let data = data.canonicalize().map_err(|e| format!("{}: {e}", data.display()))?;
        Ok(Self { adapter: Adapter::new(Backend::Local(host), data, allowed)?, definitions: tools(), peers: Mutex::default() })
    }

    /// (status, Mcp-Session-Id of a new session, body)
    pub fn handle(&self, method: &str, session: Option<&str>, version: Option<&str>, body: &[u8]) -> (u16, Option<String>, Option<Value>) {
        let backend = &self.adapter.backend;
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        let peer = session.and_then(|id| peers.get(id).cloned());
        match method {
            "POST" => {}
            "DELETE" => {
                let Some(peer) = peer else { return (404, None, None) };
                peers.remove(&peer.session);
                peer.cancel_all(backend);
                return (200, None, None);
            }
            _ => return (405, None, None),
        }
        if body.len() > MAX_INPUT { return (413, None, Some(rpc_error(Value::Null, -32700, "MCP message exceeds 1 MiB"))); }
        let Ok(message) = serde_json::from_slice::<Value>(body) else { return (400, None, Some(rpc_error(Value::Null, -32700, "Invalid JSON"))) };
        // replies to server requests: this server sends none, so there is nothing to match
        if message.get("method").is_none() && (message.get("result").is_some() || message.get("error").is_some()) { return (202, None, None); }
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let initialize = message["method"] == "initialize";
        let peer = match peer {
            _ if initialize => match Peer::new() {
                Ok(peer) => Arc::new(peer),
                Err(e) => return (500, None, Some(rpc_error(id, -32603, &e))),
            },
            Some(peer) => peer,
            None if session.is_some() => return (404, None, Some(rpc_error(id, -32001, "Unknown or ended MCP session; initialize again"))),
            None => return (400, None, Some(rpc_error(id, -32600, "Missing Mcp-Session-Id header; initialize first"))),
        };
        if !initialize && version.is_some_and(|v| v != VERSION) { return (400, None, Some(rpc_error(id, -32600, "Unsupported MCP-Protocol-Version"))); }
        drop(peers);
        match peer.step(&message, &self.definitions, backend) {
            Step::Reply(reply) if initialize && reply.get("result").is_some() => {
                let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
                // ponytail: at most MAX_SESSIONS; the oldest idle one goes, clients that keep it get 404 and initialize again
                if peers.len() >= MAX_SESSIONS {
                    let oldest = peers.values().filter(|p| p.requests().is_empty()).min_by_key(|p| p.created).map(|p| p.session.clone());
                    if let Some(oldest) = oldest { peers.remove(&oldest); }
                }
                peers.insert(peer.session.clone(), peer.clone());
                (200, Some(peer.session.clone()), Some(reply))
            }
            Step::Reply(reply) => (200, None, Some(reply)),
            Step::Nothing => (202, None, None),
            Step::Call(call) => {
                let reply = execute(&self.adapter, &call);
                peer.finish(&call);
                (200, None, Some(reply))
            }
        }
    }
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

    #[test]
    fn function_reference_filters_by_name_or_text() {
        let all = functions(None).unwrap();
        assert!(all["functions"].as_object().unwrap().len() > 100);
        let dates = functions(Some("DATE")).unwrap();
        assert!(dates["functions"]["toDate"].is_object() && !dates["functions"].as_object().unwrap().contains_key("abs"));
    }
}
