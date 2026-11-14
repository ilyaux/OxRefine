//! Real HTTP clients keep independent selections and cannot overwrite another client's newer view.
use serde_json::{json, Value};
use std::{
    io::{Read, Write}, net::{SocketAddr, TcpListener, TcpStream}, path::{Path, PathBuf},
    process::{Child, Command, Stdio}, sync::{mpsc, Mutex, atomic::{AtomicU64, Ordering}}, time::{Duration, Instant},
};

#[derive(Clone, Default)]
struct Context { project: Option<String>, revision: Option<String> }
impl Context {
    fn from(reply: &Value) -> Self {
        Self { project: reply["context"]["project"].as_str().map(str::to_owned), revision: reply["context"]["revision"].as_str().map(str::to_owned) }
    }
}
struct Client { owner: &'static str, context: Option<Context> }
impl Client {
    fn new(owner: &'static str) -> Self { Self { owner, context: None } }
    fn request(&self, server: &Server, cmd: &str, args: Value) -> (u16, Value) {
        server.request(Some(self.owner), self.context.as_ref(), cmd, args)
    }
    fn call(&mut self, server: &Server, cmd: &str, args: Value) -> Value {
        let (status, reply) = self.request(server, cmd, args);
        assert_eq!(status, 200, "{cmd}: {reply}");
        if matches!(cmd, "open" | "import_project" | "open_project" | "summary" | "close" | "apply" | "apply_recipe" | "goto" | "rename_project" | "set_project_info" | "delete_project") {
            self.context = Some(Context::from(&reply));
        }
        reply["data"].clone()
    }
}
struct Server { child: Child, addr: SocketAddr, token: Option<String> }
impl Drop for Server { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }
impl Server {
    fn start(data: &Path, token: Option<&str>) -> Self {
        // bind(0)/drop/spawn is not an atomic reservation, especially with parallel Windows tests.
        static START: Mutex<()> = Mutex::new(());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let _start = START.lock().unwrap_or_else(|error| error.into_inner());
        let token = format!("{}-{}-{}", token.unwrap_or("browser-test"), std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap(); drop(listener);
        let mut command = Command::new(env!("CARGO_BIN_EXE_ox"));
        command.args(["serve", "--port", &addr.port().to_string(), "--data"]).arg(data);
        command.args(["--token", &token]);
        let child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut server = Self { child, addr, token: Some(token) };
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(server.child.try_wait().unwrap().is_none(), "owned server exited during startup on {addr}");
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                let (status, reply) = server.request(None, None, "projects", json!({}));
                if status == 200 && reply["data"].is_array() {
                    // A random service (or another test's server) must not satisfy readiness.
                    let token = server.token.take();
                    let unauthorized = server.request(None, None, "projects", json!({}));
                    server.token = token;
                    assert_eq!(unauthorized.0, 403, "endpoint on {addr} did not enforce this server's private token");
                    assert!(server.child.try_wait().unwrap().is_none(), "owned server exited after readiness on {addr}");
                    break;
                }
            }
            assert!(Instant::now() < until, "owned server did not become ready on {addr}");
            std::thread::sleep(Duration::from_millis(20));
        }
        server
    }
    fn request(&self, owner: Option<&str>, context: Option<&Context>, cmd: &str, args: Value) -> (u16, Value) {
        let mut headers = String::new();
        if let Some(owner) = owner { headers += &format!("X-Ox-Session: {owner}\r\n"); }
        if let Some(context) = context {
            headers += &format!("X-Ox-Project: {}\r\n", context.project.as_deref().unwrap_or("none"));
            if let Some(revision) = &context.revision { headers += &format!("X-Ox-Revision: {revision}\r\n"); }
        }
        if let Some(token) = &self.token { headers += &format!("Authorization: Bearer {token}\r\n"); }
        let mut stream = TcpStream::connect_timeout(&self.addr, Duration::from_secs(3)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let body = args.to_string();
        write!(stream, "POST /api/{cmd} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}", self.addr, body.len()).unwrap();
        let mut reply = String::new(); stream.read_to_string(&mut reply).unwrap();
        let (headers, body) = reply.split_once("\r\n\r\n").unwrap();
        (headers.split_whitespace().nth(1).unwrap().parse().unwrap(), serde_json::from_str(body).unwrap())
    }
}
fn work(label: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(format!("target/browser-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap(); root
}
fn csv(root: &Path, name: &str) -> PathBuf {
    let path = root.join(format!("{name}.csv")); std::fs::write(&path, format!("name\n{name}\n")).unwrap(); path
}
fn rename(old: &str, new: &str) -> Value { json!({ "op": "core/column-rename", "oldColumnName": old, "newColumnName": new }) }

#[test]
fn selections_stale_writes_and_reads_are_isolated_with_shared_authoritative_data() {
    let root = work("selection"); let server = Server::start(&root.join("store"), Some("test-token"));
    let mut a = Client::new("browser-a"); let mut b = Client::new("browser-b");
    let alpha = a.call(&server, "open", json!({ "path": csv(&root, "alpha") }))["id"].as_str().unwrap().to_owned();
    let beta = b.call(&server, "open", json!({ "path": csv(&root, "beta") }))["id"].as_str().unwrap().to_owned();
    assert_eq!(a.call(&server, "rows", json!({ "limit": 10 }))["rows"][0][1][0], "alpha");
    assert_eq!(b.call(&server, "rows", json!({ "limit": 10 }))["rows"][0][1][0], "beta");
    assert_ne!(alpha, beta);
    // A scoped page without an addressed project cannot inherit the legacy client's active selection.
    let legacy = server.request(None, None, "open_project", json!({ "id": beta })); assert_eq!(legacy.0, 200);
    assert_eq!(server.request(Some("new-page"), None, "rows", json!({ "limit": 1 })).0, 400);
    b.call(&server, "open_project", json!({ "id": alpha }));
    a.call(&server, "apply", json!({ "op": rename("name", "person") }));
    let (status, error) = b.request(&server, "apply", json!({ "op": rename("name", "stale") }));
    assert_eq!(status, 409); assert_eq!(error["code"], "stale_project");
    assert_eq!(b.request(&server, "rows", json!({ "limit": 1 })).0, 409, "reads must not silently bless stale schema");
    let refreshed = b.call(&server, "summary", json!({})); assert_eq!(refreshed["columns"], json!(["person"]));
    b.call(&server, "apply", json!({ "op": { "op": "core/text-transform", "columnName": "person", "expression": "grel:value.toUppercase()" } }));
    assert_eq!(a.request(&server, "goto", json!({ "index": 0 })).0, 409);
    a.call(&server, "summary", json!({}));
    assert_eq!(a.call(&server, "rows", json!({ "limit": 1 }))["rows"][0][1][0], "ALPHA");
    // Closing one page does not close a colleague's addressed project.
    a.call(&server, "close", json!({}));
    assert_eq!(a.request(&server, "summary", json!({})).0, 400);
    assert_eq!(b.call(&server, "summary", json!({}))["cursor"], 2);
}

#[test]
fn commons_previews_need_no_project_context_and_failed_creation_keeps_the_selection() {
    let root = work("commons-routing"); let server = Server::start(&root.join("store"), None);
    let mut a = Client::new("browser-a"); let mut b = Client::new("browser-b");
    let id = a.call(&server, "open", json!({ "path": csv(&root, "original") }))["id"].as_str().unwrap().to_owned();
    b.call(&server, "open_project", json!({ "id": id }));
    a.call(&server, "apply", json!({ "op": rename("name", "person") }));
    assert_eq!(b.request(&server, "rows", json!({})).0, 409);

    let wiki = root.join("tables.wiki");
    std::fs::write(&wiki, "{|\n! Name !! Value\n|-\n| Alpha || 42\n|}\n").unwrap();
    let metadata = b.call(&server, "wikitext_tables", json!({ "path": wiki }));
    assert_eq!(metadata["tables"][0]["index"], 0);
    assert_eq!(server.request(Some("fresh-wiki-page"), None, "wikitext_tables", json!({ "path": wiki })).0, 200);
    assert_eq!(b.request(&server, "rows", json!({})).0, 409, "import inspection does not bless stale editor context");

    // Invalid options are rejected locally: this regression makes no Commons requests.
    let (status, error) = b.request(&server, "commons_prepare", json!({ "params": { "categories": [], "requestId": "routing-test" } }));
    assert_eq!(status, 400); assert!(error.as_str().unwrap().contains("categories"));
    assert_eq!(server.request(Some("fresh-page"), None, "commons_preview", json!({ "id": "expired" })).0, 400);
    let (status, error) = b.request(&server, "commons_create", json!({ "id": "expired" }));
    assert_eq!(status, 400); assert!(error.as_str().unwrap().contains("expired"));
    assert_eq!(b.request(&server, "rows", json!({})).0, 409, "failed creation does not bless an old editor revision");
    assert_eq!(b.call(&server, "summary", json!({}))["id"], id);
    assert_eq!(b.call(&server, "rows", json!({ "limit": 1 }))["rows"][0][1][0], "original");
    assert_eq!(a.call(&server, "projects", json!({})).as_array().unwrap().len(), 1);
}

#[test]
fn database_profiles_and_previews_are_global_without_changing_project_revisions() {
    let root = work("database-routing"); let server = Server::start(&root.join("store"), None);
    let mut a = Client::new("database-a"); let mut b = Client::new("database-b");
    let id = a.call(&server, "open", json!({ "path": csv(&root, "original") }))["id"].as_str().unwrap().to_owned();
    b.call(&server, "open_project", json!({ "id": id }));
    a.call(&server, "apply", json!({ "op": rename("name", "person") }));
    assert_eq!(b.request(&server, "rows", json!({})).0, 409);
    let saved = b.call(&server, "database_save_profile", json!({ "params": { "profile": {
        "name": "Synthetic SQLite", "driver": "sqlite", "sqlitePath": root.join("fixture.sqlite")
    } } }))["profile"].clone();
    assert_eq!(b.call(&server, "database_profiles", json!({}))["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(server.request(Some("fresh-page"), None, "storage_stats", json!({})).0, 200);
    assert_eq!(server.request(Some("fresh-page"), None, "database_preview", json!({ "id": "expired" })).0, 400);
    let conflict = b.request(&server, "database_delete_profile", json!({ "id": saved["id"], "revision": "stale" }));
    assert_eq!(conflict.0, 409); assert_eq!(conflict.1["code"], "database_profile_conflict");
    assert_eq!(b.request(&server, "database_create", json!({ "id": "expired" })).0, 400);
    assert_eq!(b.request(&server, "rows", json!({})).0, 409, "profile edits and failed imports cannot refresh stale editor context");
    assert_eq!(b.call(&server, "summary", json!({}))["id"], id);
    b.call(&server, "database_delete_profile", json!({ "id": saved["id"], "revision": saved["revision"] }));
    assert_eq!(a.call(&server, "projects", json!({})).as_array().unwrap().len(), 1);
}

#[test]
fn partial_failures_metadata_eviction_and_restart_keep_revision_guards() {
    let root = work("restart"); let data = root.join("store"); let mut server = Server::start(&data, None);
    let mut a = Client::new("browser-a"); let mut b = Client::new("browser-b");
    let id = a.call(&server, "open", json!({ "path": csv(&root, "alpha") }))["id"].as_str().unwrap().to_owned();
    b.call(&server, "open_project", json!({ "id": id }));
    let failed = a.request(&server, "apply_recipe", json!({ "ops": [rename("name", "person"), rename("missing", "bad")] }));
    assert_eq!(failed.0, 400); assert!(failed.1.as_str().unwrap().contains("1 of 2"));
    let output = root.join("existing.csv"); std::fs::write(&output, "keep").unwrap();
    assert_eq!(b.request(&server, "export", json!({ "path": output })).0, 409);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "keep");
    assert_eq!(b.call(&server, "summary", json!({}))["cursor"], 1);
    b.call(&server, "goto", json!({ "index": 0 }));
    a.call(&server, "summary", json!({}));
    let before_metadata = a.context.clone();
    b.call(&server, "rename_project", json!({ "id": id, "name": "renamed" }));
    assert_eq!(a.request(&server, "apply", json!({ "op": rename("name", "wrong") })).0, 409);
    // Force saved project eviction; its revision still rejects the original view.
    for i in 0..6 { b.call(&server, "open", json!({ "path": csv(&root, &format!("other{i}")) })); }
    a.context = before_metadata;
    assert_eq!(a.request(&server, "rows", json!({ "limit": 1 })).0, 409);
    let refreshed = a.call(&server, "summary", json!({}));
    assert_eq!(refreshed["name"], "renamed"); assert_eq!(refreshed["cursor"], 0); assert_eq!(refreshed["history"].as_array().unwrap().len(), 1);
    let old = a.context.clone(); drop(server); server = Server::start(&data, None);
    assert_eq!(a.request(&server, "apply", json!({ "op": rename("name", "wrong") })).0, 409, "restart epoch invalidates old editor context");
    a.call(&server, "summary", json!({})); assert_ne!(a.context.as_ref().unwrap().revision, old.as_ref().unwrap().revision);
    a.call(&server, "goto", json!({ "index": 1 }));
    assert_eq!(a.call(&server, "summary", json!({}))["columns"], json!(["person"]));
    b.call(&server, "open_project", json!({ "id": id }));
    b.call(&server, "delete_project", json!({ "id": id }));
    let deleted = a.request(&server, "summary", json!({})); assert_eq!(deleted.0, 404); assert_eq!(deleted.1["code"], "project_gone");
}

#[test]
fn cancellation_and_jobs_belong_to_the_requesting_page() {
    let root = work("cancel"); let server = Server::start(&root.join("store"), None);
    let mut a = Client::new("browser-a"); let b = Client::new("browser-b");
    a.call(&server, "open", json!({ "path": csv(&root, "alpha") }));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/slow", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = mpsc::channel(); let (release_tx, release_rx) = mpsc::channel();
    let endpoint = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap(); let mut request = [0; 2048]; socket.read(&mut request).unwrap(); ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(15)).unwrap(); let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    });
    let context = a.context.clone(); let addr = server.addr; let token = server.token.clone().unwrap();
    let worker = std::thread::spawn(move || {
        // This child handle does not own the server process; the minimal request below borrows its address.
        let mut stream = TcpStream::connect(addr).unwrap(); stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let context = context.unwrap();
        let body = json!({ "ops": [rename("name", "person"), { "op": "core/column-addition-by-fetching-urls", "baseColumnName": "person", "newColumnName": "fetched", "columnInsertIndex": 1, "urlExpression": format!("grel:'{url}'"), "delay": 0, "onError": "store-error", "cacheResponses": true, "httpHeadersJson": [] }] }).to_string();
        write!(stream, "POST /api/apply_recipe HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nAuthorization: Bearer {token}\r\nX-Ox-Session: browser-a\r\nX-Ox-Project: {}\r\nX-Ox-Revision: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", context.project.unwrap(), context.revision.unwrap(), body.len()).unwrap();
        let mut reply = String::new(); stream.read_to_string(&mut reply).unwrap(); reply
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let own = a.request(&server, "processes", json!({})).1["data"].clone();
    let job = own.as_array().unwrap().iter().find(|job| job["state"] == "running").unwrap()["id"].as_u64().unwrap();
    assert_eq!(b.request(&server, "processes", json!({})).1["data"], json!([]));
    assert_eq!(b.request(&server, "cancel_process", json!({ "id": job })).0, 403);
    assert_eq!(b.request(&server, "cancel_process", json!({})).0, 200);
    assert_eq!(a.request(&server, "cancel_process", json!({ "id": job })).0, 200);
    release_tx.send(()).unwrap(); endpoint.join().unwrap();
    let reply = worker.join().unwrap(); assert!(reply.starts_with("HTTP/1.1 400")); assert!(reply.contains("Cancelled"));
    let jobs = a.request(&server, "processes", json!({})).1["data"].clone();
    assert_eq!(jobs.as_array().unwrap().iter().find(|item| item["id"] == job).unwrap()["state"], "cancelled");
    assert_eq!(a.call(&server, "summary", json!({}))["cursor"], 1);
}

#[test]
fn failed_save_cannot_be_evicted_or_replaced_by_opening_the_same_project() {
    let root = work("failed-save"); let data = root.join("store"); let server = Server::start(&data, None);
    let mut a = Client::new("browser-a"); let mut b = Client::new("browser-b");
    let id = a.call(&server, "open", json!({ "path": csv(&root, "alpha") }))["id"].as_str().unwrap().to_owned();
    let history = data.join("projects").join(&id).join("history.json");
    let original = std::fs::read(&history).unwrap();
    std::fs::remove_file(&history).unwrap(); std::fs::create_dir(&history).unwrap();
    assert_eq!(a.request(&server, "apply", json!({ "op": rename("name", "person") })).0, 400);
    // Opening the same project must use the authoritative in-memory value, not replay damaged disk state.
    assert_eq!(b.call(&server, "open_project", json!({ "id": id }))["columns"], json!(["person"]));
    for i in 0..6 { b.call(&server, "open", json!({ "path": csv(&root, &format!("other{i}")) })); }
    let preserved = a.call(&server, "summary", json!({}));
    assert_eq!(preserved["columns"], json!(["person"])); assert_eq!(preserved["cursor"], 1);
    std::fs::remove_dir(&history).unwrap(); std::fs::write(&history, original).unwrap();
    a.call(&server, "goto", json!({ "index": 1 }));
    drop(server);
    let server = Server::start(&data, None);
    assert_eq!(a.call(&server, "summary", json!({}))["columns"], json!(["person"]));
}

#[test]
fn scoped_archive_import_and_exports_keep_preserved_extension_data_after_restart() {
    let root = work("extensions"); let data = root.join("store"); let mut server = Server::start(&data, None);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/orproject/extensions/wikibase.openrefine.tar.gz");
    let mut a = Client::new("browser-a"); let mut b = Client::new("browser-b");
    let id = a.call(&server, "import_project", json!({ "path": source }))["id"].as_str().unwrap().to_owned();
    b.call(&server, "open_project", json!({ "id": id }));
    a.call(&server, "apply", json!({ "op": rename("name", "person") }));
    let original = root.join("original.openrefine.tar.gz");
    assert_eq!(b.request(&server, "export_original_project", json!({ "path": original })).0, 409);
    assert!(!original.exists());
    b.call(&server, "summary", json!({}));
    b.call(&server, "export_original_project", json!({ "path": original }));
    assert_eq!(std::fs::read(&original).unwrap(), std::fs::read(&source).unwrap());
    drop(server); server = Server::start(&data, None);
    assert!(b.call(&server, "summary", json!({}))["archiveWarning"].is_string());
    let restored = root.join("restored.openrefine.tar.gz");
    b.call(&server, "export_original_project", json!({ "path": restored }));
    assert_eq!(std::fs::read(restored).unwrap(), std::fs::read(source).unwrap());
    let edited = root.join("edited.csv"); b.call(&server, "export", json!({ "path": edited }));
    assert!(std::fs::read_to_string(edited).unwrap().starts_with("person,qty"));
    assert_eq!(b.request(&server, "export_project", json!({ "path": root.join("unsafe.openrefine.tar.gz") })).0, 400);
}
