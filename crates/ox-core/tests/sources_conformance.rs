//! Every case in tests/sources/fixtures.json must come out of the clipboard / web-address import as
//! OpenRefine 3.10.1's wizard made it: same guessed format, same default options, same file name and
//! the same table. The web addresses are served by a small local server, like gen.mjs does.
use base64::Engine as _;
use ox_core::{
    grel::Value,
    import::{download, import, inspect, stage_text, ImportOptions},
};
use serde_json::{json, Value as J};
use std::{collections::BTreeSet, io::{Read, Write}, net::TcpListener};

fn cell(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Str(s) => json!({ "v": s }),
        Value::Int(i) => json!({ "v": i }),
        Value::Num(n) if n.is_finite() && *n == n.trunc() => json!({ "v": *n as i64 }),
        Value::Num(n) => json!({ "v": n }),
        Value::Bool(b) => json!({ "v": b }),
        Value::Date(d) => json!({ "v": ox_core::grel::value::fmt_date(d), "t": "date" }),
        Value::Error(_) => json!({ "e": true }),
        other => json!({ "v": other.to_java_string() }),
    }
}

fn norm(c: &J) -> J {
    match c {
        J::Object(m) if m.is_empty() => J::Null,
        other => other.clone(),
    }
}

/// OpenRefine's format ids for ours.
fn openrefine_format(f: &str) -> &str {
    match f {
        "csv" => "text/line-based/*sv",
        "lines" => "text/line-based",
        "json" => "text/json",
        "jsonlines" => "text/json",
        "excel" => "binary/text/xml/xls/xlsx",
        "ods" => "text/xml/ods",
        "xml" => "text/xml",
        other => other,
    }
}

/// Serve `files` ({path: [content type, body, encoding?]}) on a free local port.
fn serve(files: J) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, mime, body) = match files.get(&path) {
                Some(f) => {
                    let body = f[1].as_str().unwrap();
                    let bytes = if f[2] == "base64" { base64::engine::general_purpose::STANDARD.decode(body).unwrap() } else { body.as_bytes().to_vec() };
                    ("200 OK", f[0].as_str().unwrap().to_string(), bytes)
                }
                None => ("404 Not Found", "text/plain".into(), b"not found".to_vec()),
            };
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&body));
        }
    });
    port
}

#[test]
fn sources_match_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/sources/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let port = serve(fx["files"].clone());
    let dir = std::env::temp_dir().join("oxrefine-sources-conformance");

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let why = (|| -> Result<(), String> {
            let path = if let Some(text) = case["clipboard"].as_str() {
                stage_text(&dir, text)?
            } else {
                let (path, file) = download(&dir, &format!("http://127.0.0.1:{port}{}", case["url"].as_str().unwrap()))?;
                // OpenRefine's first file: the download, or the first file inside an archive
                let first = inspect(&path)?["files"][0]["name"].as_str().map_or(file, String::from);
                if first != case["fileName"] {
                    return Err(format!("file name: expected {}, got {first:?}", case["fileName"]));
                }
                path
            };
            let info = inspect(&path)?;
            let format = openrefine_format(info["format"].as_str().unwrap());
            if format != case["format"] {
                return Err(format!("format: expected {}, got {format}", case["format"]));
            }
            // the wizard defaults we model must match OpenRefine's
            let (ours, theirs) = (&info["options"], &case["options"]);
            for (k, v) in theirs.as_object().unwrap() {
                let want = match (k.as_str(), v) {
                    ("separator", J::String(s)) if format == "text/line-based" => continue,
                    ("separator", J::String(s)) => json!(s.replace("\\t", "\t")),
                    ("encoding" | "storeBlankColumns" | "forceText", _) => continue,
                    ("sheetRecords", J::Array(a)) => {
                        let picked = ours["sheets"].as_array().map_or(0, |s| s.iter().filter(|x| x["selected"] == true).count());
                        let want = a.iter().filter(|x| x["selected"] == true).count();
                        if picked != want {
                            return Err(format!("sheets selected: expected {want}, got {picked}"));
                        }
                        continue;
                    }
                    _ => v.clone(),
                };
                if ours[k] != want {
                    return Err(format!("default option {k}: expected {want}, got {}", ours[k]));
                }
            }
            let mut opts = ours.clone();
            if let Some(extra) = case["choose"].as_object() {
                opts.as_object_mut().unwrap().extend(extra.clone());
            }
            let o: ImportOptions = serde_json::from_value(opts).map_err(|e| e.to_string())?;
            let t = import(&path, info["format"].as_str(), &o)?;
            if t.names != case["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect::<Vec<_>>() {
                return Err(format!("columns: expected {}, got {:?}", case["columns"], t.names));
            }
            let rows = case["rows"].as_array().unwrap();
            if t.rows != rows.len() {
                return Err(format!("row count: expected {}, got {}", rows.len(), t.rows));
            }
            for (r, er) in rows.iter().enumerate() {
                for (c, ec) in er.as_array().unwrap().iter().enumerate() {
                    let got = cell(t.columns[c].get(r));
                    if norm(ec) != norm(&got) {
                        return Err(format!("row {r} column '{}': expected {ec}, got {got}", t.names[c]));
                    }
                }
            }
            Ok(())
        })()
        .err();
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
    // a missing page is an error, like OpenRefine's "Error uploading data"
    assert!(download(&dir, &format!("http://127.0.0.1:{port}/missing.csv")).is_err());
    assert!(download(&dir, "file:///etc/passwd").is_err(), "only http(s)");
    println!("Source conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
