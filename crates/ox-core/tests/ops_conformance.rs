//! Every case in tests/ops/fixtures.json must leave the table exactly as OpenRefine 3.10.1 did:
//! same columns, same rows in the same order, same typed cells, stars and flags.
//! Cases listed in tests/ops/known_failures.txt may differ (and must, so the list only shrinks).
//! Fetch-URL cases call a local server that answers like gen.mjs's; cross() cases look into the
//! "lookup" project, offered under that name.
use ox_core::{grel::Value, ops::Op, Project};
use serde_json::{json, Value as J};
use std::{
    collections::{BTreeSet, HashMap},
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

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
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The fetch-URL cases' server (SERVER in tests/ops/cases.mjs). Returns its address.
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let counts: Arc<Mutex<HashMap<String, u32>>> = Arc::default();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let mut parts = path.splitn(3, '/').skip(1);
            let (kind, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let (status, body) = match kind {
                "echo" => (200, format!("echo:{}", unescape(rest))),
                "status" => (rest.parse().unwrap_or(500), format!("status {rest}")),
                "header" => (200, req.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("x-ox")).map(|(_, v)| v.trim().to_string())).unwrap_or_default()),
                "count" => {
                    let mut c = counts.lock().unwrap();
                    let n = c.entry(rest.to_string()).or_default();
                    *n += 1;
                    (200, n.to_string())
                }
                _ => (404, String::new()),
            };
            let head = format!("HTTP/1.1 {status} X\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes()));
        }
    });
    base
}

/// Cell as OpenRefine's get-rows shows it: null | {"v": x} | {"v": iso, "t": "date"} | {"e": msg}.
fn cell(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Str(s) => json!({ "v": s }),
        Value::Int(i) => json!({ "v": i }),
        // like get-rows, a whole Double is written as an integer
        Value::Num(n) if n.is_finite() && *n == n.trunc() => json!({ "v": *n as i64 }),
        Value::Num(n) => json!({ "v": n }),
        Value::Bool(b) => json!({ "v": b }),
        Value::Date(d) => json!({ "v": ox_core::grel::value::fmt_date(d), "t": "date" }),
        Value::Error(e) => json!({ "e": e }),
        other => json!({ "v": other.to_java_string() }),
    }
}

/// OpenRefine writes a cell whose value was set to blank as {}; for us both are null.
/// Error messages are compared only as "an error".
fn norm(c: &J) -> J {
    match c {
        J::Object(m) if m.is_empty() => J::Null,
        J::Object(m) if m.contains_key("e") => json!({ "e": true }),
        other => other.clone(),
    }
}

#[test]
fn operations_match_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ops/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let dir = std::env::temp_dir().join("oxrefine-ops-conformance");
    std::fs::create_dir_all(&dir).unwrap();
    let corpus = dir.join("corpus.csv");
    std::fs::write(&corpus, fx["corpus"].as_str().unwrap()).unwrap();

    let lookup_csv = dir.join("lookup.csv");
    std::fs::write(&lookup_csv, fx["lookup"].as_str().unwrap()).unwrap();
    let lookup = Project::open(&lookup_csv).unwrap().table().clone();
    let base = serve();

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut p = Project::open(&corpus).unwrap();
        let mut why = None;
        let ops: J = serde_json::from_str(&case["ops"].to_string().replace("{BASE}", &base)).unwrap();
        let lookup = lookup.clone();
        ox_core::cross::scope(move |n| if n == "lookup" { vec![lookup.clone()] } else { vec![] }, || {
            for (i, op) in ops.as_array().unwrap().iter().enumerate() {
                let parsed: Result<Op, _> = serde_json::from_value(op.clone());
                if let Err(e) = parsed.map_err(|e| e.to_string()).and_then(|o| p.apply(o)) {
                    why = Some(format!("step {} ({}) failed: {e}", i + 1, op["op"]));
                    break;
                }
            }
        });
        if why.is_none() && case["ops"].as_array().unwrap().iter().any(|o| o["op"].as_str().unwrap().starts_with("ox/")) {
            // single edits: same history wording, and left out of the extracted operations like OpenRefine
            let steps: Vec<&str> = p.steps().iter().map(|o| o.description()).collect();
            if serde_json::to_value(&steps).unwrap() != case["history"] {
                why = Some(format!("history: expected {}, got {steps:?}", case["history"]));
            } else if json!(p.recipe().len()) != case["operations"] {
                why = Some(format!("extracted operations: expected {}, got {}", case["operations"], p.recipe().len()));
            }
        }
        if why.is_none() {
            let t = p.table();
            let cols: Vec<&str> = case["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
            let exp_rows = case["rows"].as_array().unwrap();
            if t.names != cols {
                why = Some(format!("columns: expected {cols:?}, got {:?}", t.names));
            } else if t.rows != exp_rows.len() {
                why = Some(format!("row count: expected {}, got {}", exp_rows.len(), t.rows));
            } else {
                'rows: for (r, er) in exp_rows.iter().enumerate() {
                    if er["starred"] != json!(t.starred[r]) || er["flagged"] != json!(t.flagged[r]) {
                        why = Some(format!("row {r}: star/flag differ"));
                        break;
                    }
                    for (c, ec) in er["cells"].as_array().unwrap().iter().enumerate() {
                        let got = cell(t.columns[c].get(r));
                        if norm(ec) != norm(&got) {
                            why = Some(format!("row {r} column '{}': expected {ec}, got {got}", cols[c]));
                            break 'rows;
                        }
                    }
                }
            }
        }
        match why {
            Some(w) => {
                failing.insert(name.to_string());
                if !known.contains(name) {
                    report.push(format!("NEW FAILURE  {name}\n    {w}"));
                }
            }
            None if known.contains(name) => report.push(format!("NOW PASSES (remove from known_failures.txt)  {name}")),
            None => {}
        }
    }
    println!("Operation conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
