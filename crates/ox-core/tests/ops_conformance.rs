//! Every case in tests/ops/fixtures.json must leave the table exactly as OpenRefine 3.10.1 did:
//! same columns, same rows in the same order, same typed cells, stars and flags.
//! Cases listed in tests/ops/known_failures.txt may differ (and must, so the list only shrinks).
use ox_core::{grel::Value, ops::Op, Project};
use serde_json::{json, Value as J};
use std::collections::BTreeSet;

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

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut p = Project::open(&corpus).unwrap();
        let mut why = None;
        for (i, op) in case["ops"].as_array().unwrap().iter().enumerate() {
            let parsed: Result<Op, _> = serde_json::from_value(op.clone());
            if let Err(e) = parsed.map_err(|e| e.to_string()).and_then(|o| p.apply(o)) {
                why = Some(format!("step {} ({}) failed: {e}", i + 1, op["op"]));
                break;
            }
        }
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
