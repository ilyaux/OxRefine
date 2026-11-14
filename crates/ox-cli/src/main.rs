use ox_core::{ops::parse_recipe, store::Store, Project, Session};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
mod web;
mod mcp;

const USAGE: &str = "usage:
  ox apply <history.json> <input> -o <output>          replay an OpenRefine operation history headlessly
                                                        (input or output may be an .openrefine.tar.gz project)
  ox serve [file] [--port 3030] [--data DIR] [--host ADDR] [--token T]
                                                        the app in a browser, at http://127.0.0.1:3030/; projects are saved
                                                        in DIR. --host 0.0.0.0 serves other machines too, behind a token
  ox mcp [--url http://127.0.0.1:3030] [--data DIR] [--allow PATH] [--token T]
                                                        MCP over stdio; attach to a running local ox serve using the SAME
                                                        --data DIR. Repeat --allow for additional files/folders; token may
                                                        also be set in OXREFINE_TOKEN. Logs go to stderr";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["apply", recipe, input, "-o", out] => apply(recipe, input, out),
        ["serve", rest @ ..] => serve(rest),
        ["mcp", rest @ ..] => mcp::run(rest),
        _ => Err(USAGE.into()),
    }
}

fn apply(recipe: &str, input: &str, out: &str) -> Result<(), String> {
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
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--port" => port = it.next().and_then(|p| p.parse().ok()).ok_or("--port needs a number")?,
            "--data" => data = it.next().ok_or("--data needs a folder")?.into(),
            "--host" => host = it.next().ok_or("--host needs an address, like 0.0.0.0")?.to_string(),
            "--token" => token = Some(it.next().ok_or("--token needs a value")?.to_string()),
            f => file = Some(f),
        }
    }
    let local = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1");
    // beyond this machine every request needs the token
    if !local && token.is_none() {
        token = Some(new_token()?);
    }
    let store = Store::open(&data)?;
    let staging = ox_core::import::staging_dir(Some(store.root()));
    let mut session = Session::with_store(store);
    // a file named on the command line opens without being added to the project library
    session.project = file.map(|f| Project::open(f.as_ref())).transpose()?;
    let slot = Arc::new(Mutex::new(web::Projects::new(session, new_token()?)));
    eprintln!("projects are saved in {}", data.display());
    let server = tiny_http::Server::http((host.as_str(), port)).map_err(|e| format!("{host}:{port}: {e}"))?;
    let shown = if host == "0.0.0.0" || host == "::" { "<this machine's address>".to_string() } else { host.clone() };
    match &token {
        Some(t) => eprintln!("OxRefine on http://{shown}:{port}/?token={t}\n(anyone with this link can read and write files as this user)"),
        None => eprintln!("OxRefine on http://127.0.0.1:{port}/"),
    }
    if ui::ASSETS.is_empty() {
        eprintln!("the web UI is not built into this binary: run `npm run build` in ui/, then build ox again");
    }
    let host_ok = Arc::new([format!("127.0.0.1:{port}"), format!("localhost:{port}")]);
    let token = Arc::new(token);
    // a thread per request: `processes` and `cancel_process` answer while a long command holds the session
    for mut req in server.incoming_requests() {
        let (slot, host_ok, token, staging) = (slot.clone(), host_ok.clone(), token.clone(), staging.clone());
        std::thread::spawn(move || {
            let header = |name: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_owned());
            let url = req.url().to_owned();
            let path = url.split('?').next().unwrap_or("/").to_owned();
            let json = |code: u16, v: Value| {
                let ct = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
                tiny_http::Response::from_string(v.to_string()).with_status_code(code).with_header(ct).boxed()
            };
            // On this machine: refuse other Host headers (DNS rebinding). With a token: it decides.
            let authorized = match token.as_ref() {
                Some(t) => header("Authorization").is_some_and(|h| h == format!("Bearer {t}")) || query(&url, "token").is_some_and(|q| &q == t),
                None => header("Host").is_some_and(|h| host_ok.contains(&h)),
            };
            let response = if *req.method() == tiny_http::Method::Get && !path.starts_with("/api/") && path != "/download" {
                // the web UI; unknown paths get the page itself
                let asset = ui::ASSETS.iter().find(|(p, _)| *p == path).or_else(|| ui::ASSETS.iter().find(|(p, _)| *p == "/index.html"));
                match asset {
                    Some((p, bytes)) => {
                        let ct = tiny_http::Header::from_bytes("Content-Type", mime(p)).unwrap();
                        tiny_http::Response::from_data(*bytes).with_header(ct).boxed()
                    }
                    None => tiny_http::Response::from_string("OxRefine's web UI is not built into this binary: run `npm run build` in ui/, then build ox again.").boxed(),
                }
            } else if !authorized {
                json(403, json!("forbidden"))
            } else if path == "/upload" {
                // a file from the browser, staged for the import wizard; the custom header keeps cross-site forms out
                match header("X-File-Name").map(|n| unescape(&n)) {
                    Some(name) => match ox_core::import::stage_path(&staging, &name).and_then(|dest| {
                        let mut f = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
                        std::io::copy(req.as_reader(), &mut f).map_err(|e| e.to_string())?;
                        Ok(dest)
                    }) {
                        Ok(dest) => json(200, json!({ "data": { "path": dest, "name": name }, "ms": 0 })),
                        Err(e) => json(400, json!(e)),
                    },
                    None => json(403, json!("forbidden")),
                }
            } else if path == "/download" {
                // a file a command wrote into the staging folder, sent once and then removed
                // only files inside the staging folder, `..` and links resolved first
                let inside = |p: &std::path::Path| p.canonicalize().ok().zip(staging.canonicalize().ok()).is_some_and(|(p, s)| p.starts_with(s));
                let file = query(&url, "path").map(std::path::PathBuf::from).filter(|p| p.is_file() && inside(p));
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
                json(403, json!("forbidden"))
            } else if path == "/stage" {
                // where a command should write a file the browser then downloads
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).ok();
                let name = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["name"].as_str().map(String::from)).unwrap_or_else(|| "download".into());
                match ox_core::import::stage_path(&staging, &name) {
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
                        let web::Reply(code, value) = web::free(&cmd, &args, &scope).unwrap_or_else(|| slot.lock().unwrap_or_else(|e| e.into_inner()).call(&cmd, args, &scope));
                        json(code, value)
                    }
                    Err(error) => json(400, Value::String(error)),
                }
            };
            req.respond(response).ok();
        });
    }
    Ok(())
}
