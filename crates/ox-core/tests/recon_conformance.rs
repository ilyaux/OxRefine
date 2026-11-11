//! Every case in tests/recon/fixtures.json must leave the table as OpenRefine 3.10.1 did: same
//! cells, same Recons (as get-rows shows them), same column reconConfig and reconStats, same
//! history wording. A local service answers each query with what tests/recon/service.mjs told
//! OpenRefine; a query OpenRefine never sent gets no answer (and fails the case).
//! Cases listed in tests/recon/known_failures.txt may differ (and must, so the list only shrinks).
use ox_core::{grel::Value, ops::Op, Project};
use serde_json::{json, Value as J};
use std::{
    collections::{BTreeSet, HashMap},
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::Arc,
};

/// The query with its object keys sorted, as gen.mjs keys the answers.
fn sorted(v: &J) -> J {
    match v {
        J::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            J::Object(keys.into_iter().map(|k| (k.clone(), sorted(&m[k]))).collect())
        }
        J::Array(a) => J::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&').find_map(|kv| kv.strip_prefix(&format!("{key}="))).map(|v| url::form_urlencoded::parse(format!("x={v}").as_bytes()).next().unwrap().1.into_owned())
}

/// The service: recorded answers by query; /svc500 fails; /svcnull answers null.
fn serve(answers: Arc<HashMap<String, J>>, extensions: Arc<HashMap<String, J>>, manifest: J) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap_or(0);
            let path = line.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap().to_string();
            let mut len = 0;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).ok();
            let body = String::from_utf8_lossy(&body);
            let (status, out) = match path.as_str() {
                "/svc500" => (500, json!({ "error": "down" }).to_string()),
                "/svcnull" => (200, "null".into()),
                "/svc" if form_value(&body, "extend").is_some() => {
                    // data extension: rows per item and property list, meta per property
                    let q: J = serde_json::from_str(&form_value(&body, "extend").unwrap()).unwrap();
                    let pids: Vec<J> = q["properties"].as_array().unwrap().iter().map(|p| p["id"].clone()).collect();
                    let meta: Vec<J> = pids.iter().map(|p| extensions.get(&json!(["meta", p]).to_string()).cloned().unwrap_or(J::Null)).collect();
                    let rows: serde_json::Map<String, J> = q["ids"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|id| extensions.get(&json!([id, pids]).to_string()).filter(|r| !r.is_null()).map(|r| (id.as_str().unwrap().to_string(), r.clone())))
                        .collect();
                    (200, json!({ "meta": meta, "rows": rows }).to_string())
                }
                "/svc" => match form_value(&body, "queries") {
                    None => (200, manifest.to_string()),
                    Some(q) => {
                        let queries: serde_json::Map<String, J> = serde_json::from_str(&q).unwrap();
                        let out: serde_json::Map<String, J> = queries
                            .iter()
                            .filter_map(|(k, q)| answers.get(&sorted(q).to_string()).map(|a| (k.clone(), a.clone())))
                            .collect();
                        (200, J::Object(out).to_string())
                    }
                },
                _ => (404, "{}".into()),
            };
            let reason = if status == 500 { "Internal Server Error" } else { "OK" };
            let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", out.len());
            let mut s = stream;
            let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(out.as_bytes()));
        }
    });
    base
}

/// Numbers compared as numbers (100 == 100.0), the service's address as {BASE}.
fn norm(v: &J) -> J {
    match v {
        J::Number(n) => json!(n.as_f64()),
        J::String(s) if s.starts_with("http://127.0.0.1:") => {
            let rest = &s["http://127.0.0.1:".len()..];
            J::String(format!("{{BASE}}{}", &rest[rest.find('/').unwrap_or(rest.len())..]))
        }
        J::Object(m) if m.is_empty() => J::Null,
        J::Object(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        J::Array(a) => J::Array(a.iter().map(norm).collect()),
        other => other.clone(),
    }
}

fn cell(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Str(s) => json!({ "v": s }),
        Value::Int(i) => json!({ "v": i }),
        Value::Num(n) => json!({ "v": n }),
        Value::Bool(b) => json!({ "v": b }),
        Value::Date(d) => json!({ "v": ox_core::grel::value::fmt_date(d), "t": "date" }),
        Value::Error(e) => json!({ "e": e }),
        other => json!({ "v": other.to_java_string() }),
    }
}

#[test]
fn reconciliation_matches_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/recon/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let dir = std::env::temp_dir().join("oxrefine-recon-conformance");
    std::fs::create_dir_all(&dir).unwrap();
    let corpus = dir.join("corpus.csv");
    std::fs::write(&corpus, fx["corpus"].as_str().unwrap()).unwrap();
    let answers: HashMap<String, J> = fx["answers"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let extensions: HashMap<String, J> = fx["extensions"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let base = serve(Arc::new(answers), Arc::new(extensions), fx["manifest"].clone());

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut p = Project::open(&corpus).unwrap();
        let mut why = None;
        let ops: J = serde_json::from_str(&case["ops"].to_string().replace("{BASE}", &base)).unwrap();
        for (i, op) in ops.as_array().unwrap().iter().enumerate() {
            let parsed: Result<Op, _> = serde_json::from_value(op.clone());
            if let Err(e) = parsed.map_err(|e| e.to_string()).and_then(|o| p.apply(o)) {
                why = Some(format!("step {} ({}) failed: {e}", i + 1, op["op"]));
                break;
            }
        }
        let t = p.table();
        if why.is_none() {
            let steps: Vec<&str> = p.steps().iter().map(|o| o.description()).collect();
            // reconciling and single-cell judging: other history wording (with row counts) isn't ours
            let ours = |h: &str| h.starts_with("Reconcile") || h.contains(" single cell on row ");
            let expected: Vec<String> = case["history"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).filter(|h| ours(h)).collect();
            let steps: Vec<String> = steps.iter().map(|s| s.to_string()).filter(|h| ours(h)).collect();
            if steps != expected {
                why = Some(format!("history: expected {expected:?}, got {steps:?}"));
            }
        }
        if why.is_none() {
            let cols: Vec<&str> = case["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
            if t.names != cols {
                why = Some(format!("columns: expected {cols:?}, got {:?}", t.names));
            }
        }
        if why.is_none() {
            // recon ids in order of first appearance, as gen.mjs numbers them
            let mut ids: HashMap<i64, usize> = HashMap::new();
            'rows: for (r, er) in case["rows"].as_array().unwrap().iter().enumerate() {
                for (c, ec) in er["cells"].as_array().unwrap().iter().enumerate() {
                    let col = &t.columns[c];
                    let mut got = cell(col.get(r));
                    if let Some(recon) = col.recon(r) {
                        let n = ids.len() + 1;
                        let id = *ids.entry(recon.id).or_insert(n);
                        let mut j = recon.to_json(false);
                        j["id"] = json!(id);
                        got["r"] = j;
                    }
                    if norm(ec) != norm(&got) {
                        why = Some(format!("row {r} column '{}':\n      expected {}\n      got      {}", t.names[c], norm(ec), norm(&got)));
                        break 'rows;
                    }
                }
            }
        }
        if why.is_none() {
            for ec in case["columnRecon"].as_array().unwrap() {
                let c = &t.columns[t.col(ec["name"].as_str().unwrap()).unwrap()];
                let rc = c.recon.as_ref();
                let got = json!({
                    "name": ec["name"],
                    "reconConfig": rc.and_then(|rc| rc.config.clone()),
                    "reconStats": rc.and_then(|rc| rc.stats.clone()),
                });
                if norm(ec) != norm(&got) {
                    why = Some(format!("column {}:\n      expected {}\n      got      {}", ec["name"], norm(ec), norm(&got)));
                    break;
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
    println!("Reconciliation conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
