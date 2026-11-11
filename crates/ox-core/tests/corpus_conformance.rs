//! The messy-data corpus (tests/corpus/files) must import as OpenRefine 3.10.1's wizard imports it:
//! same guessed format and encoding, same default options, and with those the same table.
//! Exceptions live in tests/corpus/known_failures.txt.
use ox_core::{
    grel::Value,
    import::{import, inspect, ImportOptions},
};
use serde_json::{json, Value as J};
use std::collections::BTreeSet;

fn cell(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Str(s) => json!({ "v": s }),
        Value::Int(i) => json!({ "v": i }),
        Value::Num(n) if n.is_finite() && *n == n.trunc() && n.abs() < 1e15 => json!({ "v": *n as i64 }),
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
        J::Object(m) if m.contains_key("e") => json!({ "e": true }),
        // numbers compare by value: 9223372036854775808.0 is written as an integer by one side, a float by the other
        J::Object(m) if m.get("v").is_some_and(J::is_number) && m.len() == 1 => json!({ "v": m["v"].as_f64() }),
        other => other.clone(),
    }
}

fn openrefine_format(f: &str) -> &str {
    match f {
        "csv" => "text/line-based/*sv",
        "lines" => "text/line-based",
        "json" | "jsonlines" => "text/json",
        "excel" => "binary/text/xml/xls/xlsx",
        "ods" => "text/xml/ods",
        other => other,
    }
}

/// OpenRefine names plain ASCII "US-ASCII" and UTF-8 with a byte order mark "UTF-8-BOM"; both read as UTF-8.
fn same_encoding(theirs: &str, ours: &str) -> bool {
    let canon = |e: &str| match e.to_ascii_uppercase().as_str() {
        "US-ASCII" | "UTF-8-BOM" | "UTF8" => "UTF-8".to_string(),
        other => other.to_string(),
    };
    canon(theirs) == canon(ours)
}

#[test]
fn corpus_matches_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/corpus/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let path = std::path::PathBuf::from(format!("{root}files/{}", case["file"].as_str().unwrap()));
        let why = (|| -> Result<(), String> {
            if let Some(e) = case["error"].as_str() {
                return Err(format!("OpenRefine could not import it ({e})"));
            }
            let info = inspect(&path)?;
            let format = openrefine_format(info["format"].as_str().unwrap());
            if format != case["format"] {
                return Err(format!("format: expected {}, got {format}", case["format"]));
            }
            let (ours, theirs) = (&info["options"], &case["options"]);
            for (k, v) in theirs.as_object().unwrap() {
                let want = match (k.as_str(), v) {
                    ("encoding", J::String(e)) => {
                        let got = ours["encoding"].as_str().unwrap_or("");
                        if !same_encoding(e, got) {
                            return Err(format!("encoding: expected {e}, got {got}"));
                        }
                        continue;
                    }
                    ("separator", J::String(_)) if format == "text/line-based" => continue,
                    ("separator", J::String(s)) => json!(s.replace("\\t", "\t")),
                    ("includeFileSources" | "includeArchiveFileName" | "storeBlankColumns" | "forceText" | "sheetRecords", _) => continue,
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
            let cols: Vec<&str> = case["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
            if t.names != cols {
                return Err(format!("columns: expected {cols:?}, got {:?}", t.names));
            }
            let rows = case["rows"].as_array().unwrap();
            if t.rows != rows.len() {
                return Err(format!("row count: expected {}, got {}", rows.len(), t.rows));
            }
            for (r, er) in rows.iter().enumerate() {
                for (c, ec) in er.as_array().unwrap().iter().enumerate() {
                    let got = cell(t.columns[c].get(r));
                    if norm(ec) != norm(&got) {
                        let short = |j: &J| j.to_string().chars().take(160).collect::<String>();
                        return Err(format!("row {r} column '{}': expected {}, got {}", t.names[c], short(ec), short(&got)));
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
    println!("Corpus conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
