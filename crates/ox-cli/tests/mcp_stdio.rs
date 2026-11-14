//! Real stdio clients attach to the same authoritative HTTP project cache as the browser.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write}, net::TcpListener, path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio}, sync::{atomic::{AtomicU64, Ordering}, mpsc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn work(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!("mcp-{label}-{}-{stamp}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&path).unwrap(); path.canonicalize().unwrap()
}
struct Server { child: Child, url: String, token: String, data: PathBuf }
impl Drop for Server { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }
impl Server {
    fn start(root: &Path) -> Self {
        static START: Mutex<()> = Mutex::new(());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let _start = START.lock().unwrap_or_else(|e| e.into_inner());
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port(); drop(socket);
        let token = format!("mcp-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let data = root.join("store");
        let child = Command::new(env!("CARGO_BIN_EXE_ox")).args(["serve", "--port", &port.to_string(), "--token", &token, "--data"])
            .arg(&data).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut server = Self { child, url: format!("http://127.0.0.1:{port}"), token, data };
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(server.child.try_wait().unwrap().is_none(), "owned HTTP child exited");
            if let Ok((200, value)) = server.try_request("test-ready", None, "server_info", json!({})) {
                if value["data"]["dataRoot"].as_str().is_some_and(|s| Path::new(s) == server.data.canonicalize().unwrap()) { break; }
            }
            assert!(Instant::now() < until, "owned HTTP child did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        server
    }
    fn try_request(&self, owner: &str, context: Option<&Value>, command: &str, args: Value) -> Result<(u16, Value), String> {
        let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(10))).build());
        let mut request = agent.post(format!("{}/api/{command}", self.url)).header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.token));
        if !owner.is_empty() { request = request.header("X-Ox-Session", owner); }
        if let Some(context) = context {
            request = request.header("X-Ox-Project", context["project"].as_str().unwrap()).header("X-Ox-Revision", context["revision"].as_str().unwrap());
        }
        let mut response = request.send(args.to_string().as_bytes()).map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok((status, serde_json::from_str(&body).unwrap()))
    }
    fn request(&self, context: Option<&Value>, command: &str, args: Value) -> (u16, Value) {
        self.try_request("browser-test", context, command, args).unwrap()
    }
}

struct Mcp { child: Child, stdin: Option<ChildStdin>, lines: mpsc::Receiver<String>, id: u64 }
impl Drop for Mcp { fn drop(&mut self) { self.stdin.take(); let _ = self.child.kill(); let _ = self.child.wait(); } }
impl Mcp {
    fn start(server: &Server, grants: &[&Path]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ox"));
        command.args(["mcp", "--url", &server.url, "--data"]).arg(&server.data).env("OXREFINE_TOKEN", &server.token);
        for grant in grants { command.arg("--allow").arg(grant); }
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take(); let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() { match line { Ok(line) => if sender.send(line).is_err() { break; }, Err(_) => break } }
        });
        Self { child, stdin, lines, id: 0 }
    }
    fn raw(&mut self, raw: &str) { let stdin = self.stdin.as_mut().unwrap(); stdin.write_all(raw.as_bytes()).unwrap(); stdin.flush().unwrap(); }
    fn send(&mut self, value: Value) { self.raw(&format!("{value}\n")); }
    fn receive(&self) -> Value {
        let line = self.lines.recv_timeout(Duration::from_secs(25)).expect("MCP response deadline/stdout closed");
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("non-JSON MCP stdout: {line}"))
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1; let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let value = self.receive(); assert_eq!(value["id"], id); value
    }
    fn initialize(&mut self) {
        let reply = self.rpc("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"integration","version":"1"}}));
        assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(reply["result"]["capabilities"]["tools"]["listChanged"], false);
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
    fn tool(&mut self, name: &str, args: Value) -> Value { self.rpc("tools/call", json!({"name":name,"arguments":args})) }
    fn ok(&mut self, name: &str, args: Value) -> Value {
        let reply = self.tool(name, args); assert_eq!(reply["result"]["isError"], false, "{name}: {reply}");
        let data = reply["result"]["structuredContent"].clone();
        assert_eq!(serde_json::from_str::<Value>(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap(), data); data
    }
    fn failure(&mut self, name: &str, args: Value, contains: &str) {
        let reply = self.tool(name, args); assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains(contains), "{reply}");
    }
    fn close(&mut self) {
        self.stdin.take(); let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() { assert!(status.success()); break; }
            assert!(Instant::now() < until, "MCP did not exit on EOF");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn scoped(context: &Value, extra: Value) -> Value {
    let mut result = context.as_object().unwrap().clone(); result.extend(extra.as_object().unwrap().clone()); Value::Object(result)
}
fn rename(old: &str, new: &str) -> Value { json!({"op":"core/column-rename","oldColumnName":old,"newColumnName":new}) }
fn fixture(root: &Path) -> PathBuf {
    let path = root.join("data.csv"); std::fs::write(&path, "name,group\nAlice,A\nalice,A\nBob,B\n").unwrap(); path
}

#[test]
fn lifecycle_newline_framing_negotiation_and_closed_schemas() {
    let root = work("protocol"); let server = Server::start(&root); let mut mcp = Mcp::start(&server, &[]);
    assert_eq!(mcp.rpc("server/discover", json!({}))["error"]["code"], -32601);
    assert_eq!(mcp.rpc("tools/list", json!({}))["error"]["code"], -32000);
    mcp.raw("invalid-json\n"); assert_eq!(mcp.receive()["error"]["code"], -32700);
    mcp.raw("[]\n"); assert_eq!(mcp.receive()["error"]["code"], -32600);
    assert_eq!(mcp.rpc("initialize", json!({}))["error"]["code"], -32602);
    let negotiated = mcp.rpc("initialize", json!({"protocolVersion":"future-version","capabilities":{},"clientInfo":{"name":"test","version":"1"}}));
    assert_eq!(negotiated["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(mcp.rpc("tools/list", json!({}))["error"]["code"], -32000);
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let list = mcp.rpc("tools/list", json!({})); let tools = list["result"]["tools"].as_array().unwrap(); assert_eq!(tools.len(), 12);
    assert_eq!(mcp.rpc("tools/list", json!({"_meta":{"progressToken":"list-1"}}))["result"]["tools"],list["result"]["tools"]);
    assert_eq!(mcp.rpc("tools/list", json!({"_meta":true}))["error"]["code"],-32602);
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object"); assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert!(tool["annotations"]["readOnlyHint"].is_boolean());
    }
    assert_eq!(mcp.tool("dispatch", json!({"command":"open","path":"anywhere"}))["error"]["code"], -32602);
    assert_eq!(mcp.tool("list_projects", Value::Null)["error"]["code"], -32602, "malformed CallToolRequest is a protocol error");
    mcp.failure("list_projects", json!({"path":"anywhere"}), "unknown argument path");
    mcp.failure("list_projects", json!({"limit":0}), "allowed range");
    mcp.failure("read_rows", json!({"project":"x","revision":"r","limit":1001}), "allowed range");
    mcp.failure("read_rows", json!({"project":"x","revision":"r","start":-1}), "invalid type");
    mcp.failure("read_rows", json!({}), "missing argument");
    mcp.failure("apply_recipe", json!({"project":"x","revision":"r","ops":[null]}), "invalid recipe array");
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/unknown"}));
    assert_eq!(mcp.rpc("ping", json!({}))["result"], json!({}));
    mcp.close();
    let mut empty = Mcp::start(&server, &[]); empty.close();
    let mut partial = Mcp::start(&server, &[]); partial.raw("{\"jsonrpc\":"); partial.stdin.take();
    assert_eq!(partial.receive()["error"]["code"], -32700); partial.close();
    let mut oversized = Mcp::start(&server, &[]); oversized.raw(&" ".repeat(1024*1024+1));
    assert_eq!(oversized.receive()["error"]["code"], -32700); oversized.close();
}

#[test]
fn tools_share_browser_revisions_saved_history_and_exports() {
    let root = work("shared"); let source = fixture(&root); let server = Server::start(&root); let mut mcp = Mcp::start(&server, &[&root]); mcp.initialize();
    let created = mcp.ok("create_project", json!({"path":source,"name":"MCP project"})); let context = created["context"].clone(); let project = context["project"].clone();
    assert_eq!(created["data"]["rows"], 3); assert_eq!(created["data"]["cursor"], 0);
    let projects = mcp.ok("list_projects", json!({"limit":1})); assert_eq!(projects["data"]["total"], 1); assert_eq!(projects["data"]["projects"][0]["id"], project);
    assert_eq!(mcp.ok("list_projects", json!({"start":1}))["data"]["projects"], json!([]));
    assert!(mcp.ok("profile_columns", context.clone())["data"].is_array());
    let engine = json!({"facets":[{"type":"list","name":"group","columnName":"group","expression":"value","selection":[{"v":{"v":"A","l":"A"}}]}]});
    let rows = mcp.ok("read_rows", scoped(&context,json!({"engine":engine,"limit":1}))); assert_eq!(rows["data"]["total"], 2); assert_eq!(rows["data"]["rows"].as_array().unwrap().len(), 1);
    assert!(mcp.ok("facet_counts", scoped(&context,json!({"engine":engine})))["data"].is_array());
    assert!(mcp.ok("cluster_column", scoped(&context,json!({"column":"name"})))["data"].is_array());
    let preview = mcp.ok("preview_expression", scoped(&context,json!({"column":"name","expression":"value.toUppercase()","limit":1})));
    assert_eq!(preview["data"][0], json!(["Alice","ALICE"]));
    assert_eq!(mcp.ok("open_project",json!({"project":project}))["data"]["cursor"],0, "read tools must not add history");
    let browser = server.request(None,"open_project",json!({"id":project})); assert_eq!(browser.0,200); let browser_context = browser.1["context"].clone();
    let recipe = json!([rename("name","person"),{"op":"core/text-transform","columnName":"person","expression":"value.toUppercase()"}]);
    let changed = mcp.ok("apply_recipe",scoped(&context,json!({"ops":recipe}))); let changed_context=changed["context"].clone(); assert_eq!(changed["data"]["cursor"],2);
    assert_eq!(server.request(Some(&browser_context),"rows",json!({"limit":1})).0,409);
    let browser = server.request(Some(&browser_context),"summary",json!({})); assert_eq!(browser.1["data"]["columns"],json!(["person","group"]));
    let export = root.join("result.csv"); mcp.ok("export",scoped(&changed_context,json!({"path":export,"format":"csv"})));
    assert_eq!(std::fs::read_to_string(&export).unwrap(), "person,group\r\nALICE,A\r\nALICE,A\r\nBOB,B\r\n");
    mcp.failure("export",scoped(&changed_context,json!({"path":export,"format":"csv"})),"already exists");
    let recipe_path = root.join("recipe.json");
    let recipe_export = mcp.ok("export",scoped(&changed_context,json!({"path":recipe_path,"format":"recipe"})));
    assert_eq!(recipe_export["data"],json!(recipe_path));
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&recipe_path).unwrap()).unwrap().as_array().unwrap().len(),2);
    let archive = root.join("project.openrefine.tar.gz"); mcp.ok("export",scoped(&changed_context,json!({"path":archive,"format":"project"})));
    assert!(!std::fs::read_dir(&root).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".ox-mcp-")), "completed exports must clean their owned scratch files");
    let imported=mcp.ok("create_project",json!({"path":archive,"archive":true})); assert_eq!(imported["data"]["cursor"],2); assert_ne!(imported["context"]["project"],project);
    let undone=server.request(Some(&browser.1["context"]),"goto",json!({"index":1})); assert_eq!(undone.0,200);
    mcp.failure("apply_operation",scoped(&changed_context,json!({"op":rename("person","stale")})),"stale_project");
    let refreshed=mcp.ok("open_project",json!({"project":project})); assert_eq!(refreshed["data"]["cursor"],1);
    let reverted=mcp.ok("undo",scoped(&refreshed["context"],json!({"index":0}))); assert_eq!(reverted["data"]["columns"],json!(["name","group"]));
    assert_eq!(server.request(Some(&undone.1["context"]),"rows",json!({"limit":1})).0,409);
    mcp.close();
    assert_eq!(server.request(None,"projects",json!({})).0,200,"MCP EOF must leave attached HTTP server alive");
    let data=server.data.clone(); drop(server);
    let store=ox_core::store::Store::open(data).unwrap(); let (_,_,history)=store.load(project.as_str().unwrap()).unwrap(); assert_eq!(history.cursor,0); assert_eq!(history.ops.len(),2);
}

#[test]
fn partial_recipe_error_explains_saved_steps_and_undo_recovers() {
    let root=work("partial"); let source=fixture(&root); let server=Server::start(&root); let mut mcp=Mcp::start(&server,&[&root]); mcp.initialize();
    let initial=mcp.ok("create_project",json!({"path":source})); let project=initial["context"]["project"].clone();
    mcp.failure("apply_recipe",scoped(&initial["context"],json!({"ops":[rename("name","person"),rename("missing","other")]})),"1 of 2 recipe step(s) remain applied");
    let partial=mcp.ok("open_project",json!({"project":project})); assert_eq!(partial["data"]["cursor"],1); assert_eq!(partial["data"]["columns"],json!(["person","group"]));
    mcp.failure("apply_recipe",scoped(&partial["context"],json!({"ops":[rename("person","new"),{"op":"nonexistent/operation"}]})),"unknown");
    let parsed=mcp.ok("open_project",json!({"project":project})); assert_eq!(parsed["data"]["cursor"],1); assert_eq!(parsed["data"]["columns"],json!(["person","group"]));
    let undone=mcp.ok("undo",scoped(&parsed["context"],json!({"index":0}))); assert_eq!(undone["data"]["columns"],json!(["name","group"]));
}

#[test]
fn file_capabilities_reject_traversal_internal_paths_and_symlinks() {
    let root=work("paths"); let source=fixture(&root); let server=Server::start(&root); let mut mcp=Mcp::start(&server,&[]); mcp.initialize();
    mcp.failure("create_project",json!({"path":source}),"outside the MCP grants");
    let permitted=server.data.join("input.csv"); std::fs::copy(&source,&permitted).unwrap();
    let created=mcp.ok("create_project",json!({"path":permitted})); let context=created["context"].clone();
    mcp.failure("create_project",json!({"path":server.data.join("../data.csv")}),"outside the MCP grants");
    mcp.failure("export",scoped(&context,json!({"path":server.data.join("../escaped.csv"),"format":"csv"})),"outside the MCP grants");
    assert!(!root.join("escaped.csv").exists());
    let managed=server.data.join("projects").join(context["project"].as_str().unwrap());
    mcp.failure("create_project",json!({"path":managed.join("meta.json")}),"managed projects/staging");
    mcp.failure("export",scoped(&context,json!({"path":managed.join("injected.csv"),"format":"csv"})),"managed projects/staging");
    std::fs::create_dir_all(server.data.join("staging")).unwrap();
    mcp.failure("export",scoped(&context,json!({"path":server.data.join("staging/out.csv"),"format":"csv"})),"managed projects/staging");
    let link=server.data.join("external-link");
    #[cfg(unix)] let linked=std::os::unix::fs::symlink(&root,&link).is_ok();
    #[cfg(windows)] let linked=std::os::windows::fs::symlink_dir(&root,&link).is_ok();
    if linked {
        mcp.failure("create_project",json!({"path":link.join("data.csv")}),"outside the MCP grants");
        mcp.failure("export",scoped(&context,json!({"path":link.join("linked-escape.csv"),"format":"csv"})),"outside the MCP grants");
        assert!(!root.join("linked-escape.csv").exists());
    }
    let output=server.data.join("allowed.csv"); mcp.ok("export",scoped(&context,json!({"path":output,"format":"csv"}))); assert!(output.exists());
    // An exact file grant allows that file, never its siblings or a replacement symlink.
    let mut exact=Mcp::start(&server,&[&source]); exact.initialize(); exact.ok("create_project",json!({"path":source}));
    let other=root.join("sibling.csv"); std::fs::write(&other,"x\nsecret\n").unwrap(); exact.failure("create_project",json!({"path":other}),"outside the MCP grants");
}

#[test]
fn startup_refuses_wrong_data_or_remote_backend_without_exposing_tokens() {
    let root=work("mismatch"); let server=Server::start(&root); let other=root.join("wrong-store"); std::fs::create_dir(&other).unwrap();
    let output=Command::new(env!("CARGO_BIN_EXE_ox")).args(["mcp","--url",&server.url,"--data"]).arg(&other)
        .env("OXREFINE_TOKEN",&server.token).stdin(Stdio::null()).output().unwrap();
    assert!(!output.status.success()); assert!(output.stdout.is_empty()); let error=String::from_utf8(output.stderr).unwrap(); assert!(error.contains("does not match")); assert!(!error.contains(&server.token));
    let output=Command::new(env!("CARGO_BIN_EXE_ox")).args(["mcp","--url","http://example.com:3030","--data"]).arg(&server.data).stdin(Stdio::null()).output().unwrap();
    assert!(!output.status.success()); assert!(String::from_utf8(output.stderr).unwrap().contains("loopback"));
    let output=Command::new(env!("CARGO_BIN_EXE_ox")).args(["mcp","--url","http://[::1]:3030","--data"]).arg(&server.data).stdin(Stdio::null()).output().unwrap();
    assert!(!output.status.success()); assert!(String::from_utf8(output.stderr).unwrap().contains("loopback"));
}

#[test]
fn cancellation_and_eof_cancel_only_the_addressed_request() {
    let root=work("cancel"); let source=fixture(&root); let server=Server::start(&root); let mut mcp=Mcp::start(&server,&[&root]); mcp.initialize();
    let created=mcp.ok("create_project",json!({"path":source})); let project=created["context"]["project"].clone();
    let fetch=tiny_http::Server::http("127.0.0.1:0").unwrap(); let url=format!("http://{}/",fetch.server_addr());
    let (started,seen)=mpsc::channel(); let (release,resume)=mpsc::channel();
    let fetcher=std::thread::spawn(move || {
        for index in 0..3 {
            let request=fetch.recv_timeout(Duration::from_secs(15)).unwrap().expect("fetch request deadline");
            started.send(index).unwrap(); resume.recv_timeout(Duration::from_secs(15)).unwrap();
            request.respond(tiny_http::Response::from_string("fetched")).unwrap();
        }
    });
    let operation=|name: &str| json!({"op":"core/column-addition-by-fetching-urls","baseColumnName":"name","urlExpression":format!("'{}'",url),"onError":"store-error","newColumnName":name,"columnInsertIndex":2,"delay":0,"cacheResponses":true,"httpHeadersJson":[]});
    let start=|mcp: &mut Mcp,context: &Value,name: &str| {
        mcp.id+=1; let id=mcp.id;
        mcp.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"apply_operation","arguments":scoped(context,json!({"op":operation(name)}))}})); id
    };
    let first=start(&mut mcp,&created["context"],"cancelled"); assert_eq!(seen.recv_timeout(Duration::from_secs(10)).unwrap(),0);
    // Resident memory is process-wide and must answer while this command owns the project lock.
    let (status,memory)=server.request(None,"memory",json!({})); assert_eq!(status,200);
    assert_eq!(memory["data"]["pid"],server.child.id());
    let current=memory["data"]["currentBytes"].as_u64().unwrap();
    assert!(current>0); assert!(memory["data"]["peakBytes"].as_u64().unwrap()>=current);
    assert_eq!(memory["data"]["peakSource"],"os");
    // An unrelated browser page cannot cancel the MCP-owned job.
    server.request(None,"cancel_process",json!({}));
    assert!(!server.try_request("",None,"processes",json!({})).unwrap().1["data"].as_array().unwrap().iter().any(|job| job["state"]=="running" && job["cancelRequested"]==true));
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":first,"reason":"test"}}));
    let until=Instant::now()+Duration::from_secs(5);
    loop {
        let jobs=server.try_request("",None,"processes",json!({})).unwrap().1;
        if jobs["data"].as_array().unwrap().iter().any(|job| job["state"]=="running" && job["cancelRequested"]==true) { break; }
        assert!(Instant::now()<until,"MCP cancellation did not reach its owned job"); std::thread::sleep(Duration::from_millis(20));
    }
    release.send(()).unwrap(); let cancelled=mcp.receive(); assert_eq!(cancelled["id"],first); assert_eq!(cancelled["result"]["isError"],true); assert!(cancelled.to_string().contains("Cancelled"));
    let current=mcp.ok("open_project",json!({"project":project})); assert_eq!(current["data"]["cursor"],0);
    let second=start(&mut mcp,&current["context"],"kept"); assert_eq!(seen.recv_timeout(Duration::from_secs(10)).unwrap(),1);
    // A delayed notification for the finished request must not cancel the following one.
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":first}}));
    assert_eq!(mcp.rpc("ping",json!({}))["result"],json!({}),"reader stays responsive during a running tool");
    release.send(()).unwrap(); let kept=mcp.receive(); assert_eq!(kept["id"],second); assert_eq!(kept["result"]["isError"],false,"{kept}");
    let context=kept["result"]["structuredContent"]["context"].clone();
    let third=start(&mut mcp,&context,"eof-cancelled"); assert_eq!(seen.recv_timeout(Duration::from_secs(10)).unwrap(),2);
    mcp.stdin.take();
    let until=Instant::now()+Duration::from_secs(5);
    loop {
        let jobs=server.try_request("",None,"processes",json!({})).unwrap().1;
        if jobs["data"].as_array().unwrap().iter().any(|job| job["state"]=="running" && job["cancelRequested"]==true) { break; }
        assert!(Instant::now()<until,"EOF did not cancel the active MCP job"); std::thread::sleep(Duration::from_millis(20));
    }
    release.send(()).unwrap(); let eof=mcp.receive(); assert_eq!(eof["id"],third); assert_eq!(eof["result"]["isError"],true); mcp.close(); fetcher.join().unwrap();
    let refreshed=server.request(None,"open_project",json!({"id":project})); assert_eq!(refreshed.0,200); assert_eq!(refreshed.1["data"]["cursor"],1); assert_eq!(refreshed.1["data"]["columns"],json!(["name","group","kept"]));
}
