use ox_core::{dispatch, dispatch_free, ops::Op, store::Store, Project, Session};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

const USAGE: &str = "usage:
  ox apply <history.json> <input.csv> -o <output.csv>   replay an OpenRefine operation history headlessly
  ox serve [file] [--port 3030] [--data DIR]             JSON API for the web UI (ui/: npm run dev); projects are saved in DIR";

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
        _ => Err(USAGE.into()),
    }
}

fn apply(recipe: &str, input: &str, out: &str) -> Result<(), String> {
    let ops: Vec<Op> = serde_json::from_str(&std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?)
        .map_err(|e| format!("{recipe}: {e}"))?;
    let t = Instant::now();
    let mut p = Project::open(input.as_ref())?;
    let (rows, cols) = (p.table().rows, p.table().names.len());
    eprintln!("import  {:>9.1} ms   {rows} rows x {cols} columns", ms(t));
    let t = Instant::now();
    for (i, op) in ops.into_iter().enumerate() {
        p.apply(op).map_err(|e| format!("step {}: {e}", i + 1))?;
    }
    eprintln!("apply   {:>9.1} ms   {} operations", ms(t), p.recipe().len());
    let t = Instant::now();
    let t2 = p.table();
    let bytes = ox_core::export::export(t2, &(0..t2.rows as u32).collect::<Vec<_>>(), ox_core::export::format_for(out), &serde_json::Value::Null, "data")?;
    std::fs::write(out, bytes).map_err(|e| format!("{out}: {e}"))?;
    eprintln!("export  {:>9.1} ms   {out}", ms(t));
    Ok(())
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn serve(args: &[&str]) -> Result<(), String> {
    let (mut file, mut port, mut data) = (None, 3030u16, Store::default_root());
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--port" => port = it.next().and_then(|p| p.parse().ok()).ok_or("--port needs a number")?,
            "--data" => data = it.next().ok_or("--data needs a folder")?.into(),
            f => file = Some(f),
        }
    }
    let mut session = Session::with_store(Store::open(&data)?);
    // a file named on the command line opens without being added to the project library
    session.project = file.map(|f| Project::open(f.as_ref())).transpose()?;
    let slot = Arc::new(Mutex::new(session));
    eprintln!("projects are saved in {}", data.display());
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let host_ok = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    eprintln!("OxRefine API on http://127.0.0.1:{port}/api/<command>");
    // a thread per request: `processes` and `cancel_process` answer while a long command holds the session
    for mut req in server.incoming_requests() {
        let slot = slot.clone();
        let host_ok = host_ok.clone();
        std::thread::spawn(move || {
        let header = |name: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_owned());
        // The API reads and writes local files: refuse other hosts (DNS rebinding) and
        // non-JSON bodies (cross-site form posts can't set this type without a preflight we never answer).
        let trusted = header("Host").is_some_and(|h| host_ok.contains(&h))
            && header("Content-Type").is_some_and(|c| c.starts_with("application/json"));
        let (code, body) = if !trusted {
            (403, json!("forbidden"))
        } else {
            let cmd = req.url().trim_start_matches("/api/").to_owned();
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body).ok();
            let args = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
            let result = dispatch_free(&cmd, &args).unwrap_or_else(|| dispatch(&mut slot.lock().unwrap_or_else(|e| e.into_inner()), &cmd, args));
            match result {
                Ok(v) => (200, v),
                Err(e) => (400, Value::String(e)),
            }
        };
        let ct = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
        req.respond(tiny_http::Response::from_string(body.to_string()).with_status_code(code).with_header(ct)).ok();
        });
    }
    Ok(())
}
