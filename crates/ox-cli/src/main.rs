use ox_core::{ops::parse_recipe, store::Store, Project, Session};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};
mod web;
mod mcp;

const USAGE: &str = "usage:
  ox apply <history.json> <input> -o <output>          replay an OpenRefine operation history headlessly
                                                        (input or output may be an .openrefine.tar.gz project)
  ox serve [file] [--port 3030] [--data DIR] [--host ADDR] [--token T] [--allow-host NAME] [--allow PATH] [--log]
                                                        the app in a browser, at http://127.0.0.1:3030/; projects are saved
                                                        in DIR. --host 0.0.0.0 serves other machines too, behind a token
                                                        (--token, or OXREFINE_TOKEN). Without one, only requests addressed
                                                        to 127.0.0.1 or localhost are answered; --allow-host adds a name a
                                                        reverse proxy passes on. MCP clients connect to /mcp; repeat --allow
                                                        for files they may use. --log prints each request to stderr
  ox mcp [--data DIR] [--allow PATH] [--url http://127.0.0.1:3030] [--token T]
                                                        MCP over stdio for the projects in DIR: uses the ox serve of DIR if
                                                        one runs, else serves DIR itself. Repeat --allow for files/folders;
                                                        --url/--token (or OXREFINE_TOKEN) name a server. Logs go to stderr";

fn main() {
    // Work on a thread with a stack of known size. The embedded Python measures how much stack it
    // has left, and under musl (the static Linux build) the main thread's size isn't reported
    // correctly, which made every Python expression fail with RecursionError.
    let worker = std::thread::Builder::new().name("ox".into()).stack_size(16 << 20).spawn(run);
    match worker.map_err(|e| e.to_string()).and_then(|w| w.join().map_err(|_| "ox stopped after an internal error".to_string())) {
        Ok(Ok(())) => {}
        Ok(Err(e)) | Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let budget = ox_core::storage::from_env(&std::env::temp_dir().join(format!("oxrefine-spill-{}", std::process::id())))?;
    ox_core::storage::with_budget(budget, run_scoped)
}

fn run_scoped() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // the usual flags service scripts and package managers expect: help and version on stdout, exit 0
    if args.iter().any(|a| a == "--help" || a == "-h") || args.first().is_some_and(|a| a == "help") {
        let extra = "  ox --version                                          print the version\n  ox --help                                             print this help";
        println!("ox {} (OxRefine)\n\n{USAGE}\n{extra}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if matches!(args.first().map(String::as_str), Some("--version" | "-V" | "version")) {
        println!("ox {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["apply", recipe, input, "-o", out] => apply(recipe, input, out),
        ["serve", rest @ ..] => serve(rest),
        ["mcp", rest @ ..] => mcp::run(rest),
        _ => Err(USAGE.into()),
    }
}

fn apply(recipe: &str, input: &str, out: &str) -> Result<(), String> {
    // recipes may use functions and operations from installed extensions
    for e in ox_core::extensions::load(&ox_core::extensions::folders(&Store::default_root())) {
        if let Some(error) = e.error {
            eprintln!("warning: extension {} didn't load: {error}", e.name);
        }
    }
    let value = serde_json::from_str(&std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?)
        .map_err(|e| format!("{recipe}: {e}"))?;
    let ops = parse_recipe(value).map_err(|e| format!("{recipe}: {e}"))?;
    let t = Instant::now();
    let mut p = Project::open(input.as_ref())?;
    if let Some(warning) = p.archive_warning() {
        eprintln!("warning: {warning}");
    }
    let (rows, cols) = (p.table().rows, p.table().names.len());
    eprintln!("import  {:>9.1} ms   {rows} rows x {cols} columns", ms(t));
    let t = Instant::now();
    for (i, op) in ops.into_iter().enumerate() {
        p.apply(op).map_err(|e| format!("step {}: {e}; output was not written", i + 1))?;
    }
    eprintln!("apply   {:>9.1} ms   {} operations", ms(t), p.recipe().len());
    let t = Instant::now();
    let output_name = out.to_ascii_lowercase();
    if output_name.ends_with(".tar.gz") || output_name.ends_with(".tgz") {
        ox_core::orproject::write(&p, out.as_ref())?;
        eprintln!("export  {:>9.1} ms   {out}", ms(t));
        return Ok(());
    }
    let t2 = p.table();
    ox_core::export::export_file(t2, &(0..t2.rows as u32).collect::<Vec<_>>(), ox_core::export::format_for(out), &serde_json::Value::Null, "data", out.as_ref())?;
    eprintln!("export  {:>9.1} ms   {out}", ms(t));
    Ok(())
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

mod ui {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

/// A short page in place of the app: why it isn't shown, and what to do.
fn note(text: &str) -> String {
    let text = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    format!("<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>OxRefine</title>\
<p style=\"font:16px system-ui,sans-serif;margin:3em auto;max-width:34em;padding:0 1em\">{text}</p>")
}

/// The host name in a Host header or an Origin, lowercased and without its port.
fn host_name(h: &str) -> String {
    let h = h.trim().trim_start_matches("http://").trim_start_matches("https://").to_ascii_lowercase();
    if let Some(v6) = h.strip_prefix('[') {
        return v6.split(']').next().unwrap_or_default().to_string();
    }
    match h.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name.to_string(),
        _ => h,
    }
}

/// Why a request is refused. Each says what to do about it: the browser and API clients get the
/// first text, whoever runs `ox serve` the second, once, on stderr.
enum Refusal {
    /// a server with a token, and a request without it
    Token,
    /// a server without a token, and a request addressed to another host name (DNS rebinding)
    Host(String),
}

impl Refusal {
    fn text(&self) -> String {
        match self {
            Refusal::Token => "This OxRefine needs its access link, the address with ?token=… at the end. Ask whoever runs it for the link.".into(),
            Refusal::Host(name) => format!(
                "This OxRefine answers only requests addressed to 127.0.0.1 or localhost, and this one was addressed to {name}. \
Behind a reverse proxy that passes on the visitor's host name, whoever runs it can add --allow-host {name}, or start it with --token."
            ),
        }
    }
    fn log(&self) -> String {
        match self {
            Refusal::Token => "refused a request without the access token: open the link with ?token=… printed above".into(),
            Refusal::Host(name) => format!(
                "refused requests addressed to {name}: without a token ox serve answers only 127.0.0.1 and localhost; \
behind a reverse proxy that passes on the visitor's host name, add --allow-host {name}, or use --token"
            ),
        }
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// A random token for `--host` beyond this machine (like Jupyter's): the API reads and writes files.
fn new_token() -> Result<String, String> {
    let mut b = [0u8; 18];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn query(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.strip_prefix(&format!("{key}=")).map(unescape))
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match (b[i], s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(x)) => {
                out.push(x);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn serve(args: &[&str]) -> Result<(), String> {
    let (mut file, mut port, mut data, mut host, mut token) = (None, 3030u16, Store::default_root(), "127.0.0.1".to_string(), None);
    let (mut allow, mut hosts, mut log) = (vec![], vec![], false);
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--allow-host" => hosts.push(host_name(it.next().ok_or("--allow-host needs a host name, like intranet.example.com")?)),
            "--log" => log = true,
            "--port" => port = it.next().and_then(|p| p.parse().ok()).ok_or("--port needs a number")?,
            "--data" => data = it.next().ok_or("--data needs a folder")?.into(),
            "--host" => host = it.next().ok_or("--host needs an address, like 0.0.0.0")?.to_string(),
            "--token" => token = Some(it.next().ok_or("--token needs a value")?.to_string()),
            "--allow" => allow.push(PathBuf::from(it.next().ok_or("--allow needs a file or folder")?)),
            f => file = Some(f),
        }
    }
    // the token may come from the environment, out of other users' sight in the process list
    token = token.or_else(|| std::env::var("OXREFINE_TOKEN").ok().filter(|t| !t.trim().is_empty()));
    let local = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1");
    // beyond this machine every request needs the token
    if !local && token.is_none() {
        token = Some(new_token()?);
    }
    let store = Store::open(&data)?;
    let lock = match web::claim(&data) {
        web::Claim::Busy => {
            let at = web::advertised(&data).map(|(url, _)| format!(" at {url}")).unwrap_or_default();
            return Err(format!("another OxRefine already serves {}{at}; one data folder has one server", data.display()));
        }
        web::Claim::Held(lock) => Some(lock),
        web::Claim::Unsupported => None,
    };
    let session = Session::with_store(store);
    let server = tiny_http::Server::http((host.as_str(), port)).map_err(|e| format!("{host}:{port}: {e}"))?;
    let address = server.server_addr().to_ip().ok_or("not an IP address")?;
    let port = address.port();
    let mut listener = Listener::new(session, file, token, &data, &allow)?;
    (listener.hosts, listener.log) = (hosts, log);
    eprintln!("projects are saved in {}", data.display());
    let shown = if host == "0.0.0.0" || host == "::" { "<this machine's address>".to_string() } else { host.clone() };
    match &listener.token {
        Some(t) => eprintln!("OxRefine on http://{shown}:{port}/?token={t}\n(anyone with this link can read and write files as this user)"),
        None => eprintln!("OxRefine on http://127.0.0.1:{port}/"),
    }
    if listener.token.is_none() && !listener.hosts.is_empty() {
        eprintln!("also answering requests addressed to {}, as a reverse proxy passes them on", listener.hosts.join(", "));
    }
    if ui::ASSETS.is_empty() {
        eprintln!("the web UI is not built into this binary: run `npm run build` in ui/, then build ox again");
    }
    // where `ox mcp` finds this server: only an address on this machine
    let ip = address.ip();
    if lock.is_some() && (ip.is_loopback() || ip.is_unspecified()) {
        let loopback = if ip.is_ipv4() { format!("127.0.0.1:{port}") } else { format!("[::1]:{port}") };
        if let Err(e) = web::advertise(&data, &format!("http://{loopback}"), listener.token.as_deref()) {
            eprintln!("warning: ox mcp won't find this server by itself ({e}); give it --url");
        }
    }
    listen(server, listener);
    drop(lock);
    Ok(())
}

/// A standalone `ox mcp` (no server for its data folder) hosts the folder itself on a free loopback
/// port: the browser can watch the projects there, and other `ox mcp` processes attach to it.
fn embedded(data: &Path, lock: Option<std::fs::File>) -> Result<web::Host, String> {
    let session = Session::with_store(Store::open(data)?);
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = server.server_addr().to_ip().ok_or("not an IP address")?.port();
    let token = new_token()?;
    let listener = Listener::new(session, None, Some(token.clone()), data, &[])?;
    let host = listener.host.clone();
    if lock.is_some() {
        if let Err(e) = web::advertise(data, &format!("http://127.0.0.1:{port}"), Some(&token)) {
            eprintln!("warning: other ox mcp processes won't find this host ({e})");
        }
    }
    eprintln!("OxRefine for this assistant: projects in {}; watch them at http://127.0.0.1:{port}/?token={token}", data.display());
    std::thread::spawn(move || {
        listen(server, listener);
        drop(lock);
    });
    Ok(host)
}

struct Listener {
    host: web::Host,
    token: Option<String>,
    /// host names answered without a token besides this machine's (--allow-host)
    hosts: Vec<String>,
    /// print each request to stderr (--log)
    log: bool,
    /// refusals already reported on stderr
    warned: Mutex<HashSet<String>>,
    staging: PathBuf,
    mcp: mcp::Http,
}

impl Listener {
    fn new(mut session: Session, file: Option<&str>, token: Option<String>, data: &Path, allow: &[PathBuf]) -> Result<Self, String> {
        let store = session.store.as_ref().ok_or("no project store")?;
        let (budget, staging) = (store.code_budget(), ox_core::import::staging_dir(Some(store.root())));
        // a file named on the command line opens without being added to the project library
        session.project = ox_core::storage::with_budget(budget.clone(), || file.map(|f| Project::open(f.as_ref())).transpose())?;
        let host = web::Host::new(session, budget)?;
        let mcp = mcp::Http::new(host.clone(), data, allow)?;
        Ok(Self { host, token, hosts: vec![], log: false, warned: Mutex::default(), staging, mcp })
    }

    /// A host name answered without a token: this machine's, or one named with --allow-host.
    fn answers(&self, name: &str) -> bool {
        matches!(name, "127.0.0.1" | "localhost" | "::1") || self.hosts.iter().any(|h| h == name)
    }

    /// The one access rule. With a token, the token decides: an Authorization header or ?token=.
    /// Without one, the Host header must name this machine or an --allow-host name, so a web page
    /// can't reach this server through a DNS name it controls.
    fn access(&self, host: Option<&str>, authorization: Option<&str>, url: &str) -> Result<(), Refusal> {
        match &self.token {
            Some(t) if authorization == Some(&format!("Bearer {t}")) || query(url, "token").as_ref() == Some(t) => Ok(()),
            Some(_) => Err(Refusal::Token),
            None => {
                let name = host.map(host_name).unwrap_or_default();
                if self.answers(&name) { Ok(()) } else { Err(Refusal::Host(name)) }
            }
        }
    }

    /// Tell whoever runs the server about a kind of refusal, once.
    fn warn(&self, r: &Refusal) {
        let mut warned = self.warned.lock().unwrap_or_else(|e| e.into_inner());
        // ponytail: Host names come from clients, so only the first few distinct ones are reported
        if warned.len() < 16 && warned.insert(r.log()) {
            eprintln!("{}", r.log());
        }
    }
}

fn listen(server: tiny_http::Server, listener: Listener) {
    let listener = Arc::new(listener);
    // a thread per request: `processes` and `cancel_process` answer while a long command holds the session
    for mut req in server.incoming_requests() {
        let l = listener.clone();
        // as large a stack as the main thread's: Python expressions run here
        let _ = std::thread::Builder::new().stack_size(16 << 20).spawn(move || {
            let header = |name: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_owned());
            let url = req.url().to_owned();
            // repeated slashes count as one: Apache's `ProxyPass /refine http://host:port/` sends //assets/…
            let mut path = String::new();
            for c in url.split('?').next().unwrap_or("/").chars() {
                if !(c == '/' && path.ends_with('/')) {
                    path.push(c);
                }
            }
            let json = |code: u16, v: Value| {
                let ct = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
                tiny_http::Response::from_string(v.to_string()).with_status_code(code).with_header(ct).boxed()
            };
            let started = Instant::now();
            let access = l.access(header("Host").as_deref(), header("Authorization").as_deref(), &url);
            let refuse = |r: &Refusal| {
                l.warn(r);
                json(403, json!(r.text()))
            };
            let read = matches!(req.method(), tiny_http::Method::Get | tiny_http::Method::Head);
            let mut response = if path == "/healthz" {
                // for load balancers and container health checks: it says nothing but that the server runs
                tiny_http::Response::from_string("ok").boxed()
            } else if path == "/mcp" {
                // MCP over HTTP; without a token a web page may call it only from an address this server answers
                let origin_ok = l.token.is_some() || header("Origin").is_none_or(|o| l.answers(&host_name(&o)));
                if let Err(r) = &access {
                    refuse(r)
                } else if !origin_ok {
                    json(403, json!("MCP from a web page is answered only from an address this server answers (127.0.0.1, localhost or --allow-host)"))
                } else {
                    let (session, version) = (header("Mcp-Session-Id"), header("MCP-Protocol-Version"));
                    let mut body = vec![];
                    let read = req.as_reader().take(mcp::MAX_INPUT as u64 + 1).read_to_end(&mut body);
                    let (code, created, value) = match read {
                        Ok(_) => l.mcp.handle(req.method().as_str(), session.as_deref(), version.as_deref(), &body),
                        Err(_) => (400, None, None),
                    };
                    let mut response = match value {
                        Some(value) => json(code, value),
                        None => tiny_http::Response::empty(code).boxed(),
                    };
                    if let Some(id) = created {
                        response.add_header(tiny_http::Header::from_bytes("Mcp-Session-Id", id).unwrap());
                    }
                    if code == 405 {
                        response.add_header(tiny_http::Header::from_bytes("Allow", "POST, DELETE").unwrap());
                    }
                    response
                }
            } else if read && !path.starts_with("/api/") && path != "/download" {
                // the web UI; other paths get the page itself, but a missing file is a 404: the page
                // in place of a script leaves the browser a blank screen
                let file = path.rsplit('/').next().is_some_and(|name| name.contains('.'));
                let asset = ui::ASSETS.iter().find(|(p, _)| *p == path).or_else(|| ui::ASSETS.iter().find(|(p, _)| !file && *p == "/index.html"));
                // the page itself follows the access rule, so nobody refused sees the app, and learns
                // why instead; its scripts and styles are the same published build for everyone and stay open
                let page = path == "/index.html" || asset.map_or(!file, |(p, _)| *p == "/index.html");
                match (asset, &access) {
                    (_, Err(r)) if page => {
                        l.warn(r);
                        let ct = tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap();
                        tiny_http::Response::from_string(note(&r.text())).with_status_code(403).with_header(ct).boxed()
                    }
                    (Some((p, bytes)), _) => {
                        let ct = tiny_http::Header::from_bytes("Content-Type", mime(p)).unwrap();
                        tiny_http::Response::from_data(*bytes).with_header(ct).boxed()
                    }
                    (None, _) if ui::ASSETS.is_empty() => tiny_http::Response::from_string("OxRefine's web UI is not built into this binary: run `npm run build` in ui/, then build ox again.").boxed(),
                    (None, _) => tiny_http::Response::from_string(format!("not found: {path}")).with_status_code(404).boxed(),
                }
            } else if let Err(r) = &access {
                refuse(r)
            } else if path == "/upload" {
                // a file from the browser, staged for the import wizard; the custom header keeps cross-site forms out
                match header("X-File-Name").map(|n| unescape(&n)) {
                    Some(name) => match ox_core::import::stage_path(&l.staging, &name).and_then(|dest| {
                        let mut f = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
                        std::io::copy(req.as_reader(), &mut f).map_err(|e| e.to_string())?;
                        Ok(dest)
                    }) {
                        Ok(dest) => json(200, json!({ "data": { "path": dest, "name": name }, "ms": 0 })),
                        Err(e) => json(400, json!(e)),
                    },
                    None => json(403, json!("an upload needs its X-File-Name header")),
                }
            } else if path == "/download" {
                // a file a command wrote into the staging folder, sent once and then removed
                // only files inside the staging folder, `..` and links resolved first
                let inside = |p: &Path| p.canonicalize().ok().zip(l.staging.canonicalize().ok()).is_some_and(|(p, s)| p.starts_with(s));
                let file = query(&url, "path").map(PathBuf::from).filter(|p| p.is_file() && inside(p));
                match file.and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                    Some((p, bytes)) => {
                        let name = p.file_name().map(|n| n.to_string_lossy().replace('"', "")).unwrap_or_default();
                        let _ = std::fs::remove_dir_all(p.parent().unwrap_or(&p));
                        let cd = tiny_http::Header::from_bytes("Content-Disposition", format!("attachment; filename=\"{name}\"")).unwrap();
                        let ct = tiny_http::Header::from_bytes("Content-Type", mime(&name)).unwrap();
                        tiny_http::Response::from_data(bytes).with_header(cd).with_header(ct).boxed()
                    }
                    None => json(404, json!("no such download")),
                }
            } else if !header("Content-Type").is_some_and(|c| c.starts_with("application/json")) {
                // cross-site form posts can't send JSON without a preflight we never answer
                json(403, json!("API requests send JSON: Content-Type: application/json"))
            } else if path == "/stage" {
                // where a command should write a file the browser then downloads
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).ok();
                let name = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["name"].as_str().map(String::from)).unwrap_or_else(|| "download".into());
                match ox_core::import::stage_path(&l.staging, &name) {
                    Ok(p) => json(200, json!({ "data": { "path": p }, "ms": 0 })),
                    Err(e) => json(400, json!(e)),
                }
            } else {
                let cmd = path.trim_start_matches("/api/").to_owned();
                let scope = web::Scope::new(header("X-Ox-Session"), header("X-Ox-Project"), header("X-Ox-Revision"));
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).ok();
                let args = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
                match scope {
                    Ok(scope) => {
                        let web::Reply(code, value) = l.host.api(&cmd, args, &scope);
                        json(code, value)
                    }
                    Err(error) => json(400, Value::String(error)),
                }
            };
            // addresses carry the token: never pass them on to other sites, or into a proxy's log as the referrer
            response.add_header(tiny_http::Header::from_bytes("Referrer-Policy", "no-referrer").unwrap());
            if l.log {
                // the path only: the query may hold the token
                eprintln!("{} {path} {} {:.1} ms", req.method(), response.status_code().0, started.elapsed().as_secs_f64() * 1000.0);
            }
            req.respond(response).ok();
            // this thread ends here: free its Python interpreter while RustPython can still be torn down
            ox_core::grel::release_thread();
        });
    }
}
