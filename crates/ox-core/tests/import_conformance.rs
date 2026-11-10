//! Every case in tests/import/fixtures.json must import exactly as OpenRefine 3.10.1 imported it:
//! same column names and order, same rows, same typed cells. Exceptions live in known_failures.txt.
use ox_core::{
    grel::Value,
    import::{import, ImportOptions},
};
use serde_json::{json, Value as J};
use std::{collections::BTreeSet, path::Path};

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
        J::Object(m) if m.contains_key("e") => json!({ "e": true }),
        other => other.clone(),
    }
}

#[test]
fn imports_match_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/import/");
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
        let opts: ImportOptions = serde_json::from_value(case["options"].clone()).unwrap();
        let path = Path::new(root).join(case["file"].as_str().unwrap());
        let why = match import(&path, case["format"].as_str(), &opts) {
            Err(e) => Some(format!("import failed: {e}")),
            Ok(t) => {
                let cols: Vec<&str> = case["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
                let rows = case["rows"].as_array().unwrap();
                if t.names != cols {
                    Some(format!("columns: expected {cols:?}, got {:?}", t.names))
                } else if t.rows != rows.len() {
                    Some(format!("row count: expected {}, got {}", rows.len(), t.rows))
                } else {
                    rows.iter().enumerate().find_map(|(r, row)| {
                        row.as_array().unwrap().iter().enumerate().find_map(|(c, ec)| {
                            let got = cell(t.columns[c].get(r));
                            (norm(ec) != got).then(|| format!("row {r} column '{}': expected {ec}, got {got}", cols[c]))
                        })
                    })
                }
            }
        };
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
    println!("Import conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
