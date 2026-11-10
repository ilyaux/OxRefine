//! Every expression in tests/grel/fixtures.json must render exactly as OpenRefine 3.10.1 rendered it,
//! except those listed in tests/grel/known_failures.txt (each with the reason). A known failure that
//! starts passing fails the test too, so the list only ever shrinks.
use ox_core::grel::{self, Row, Value};
use serde_json::Value as J;
use std::collections::BTreeSet;

struct FixtureRow<'a> {
    index: usize,
    cells: &'a [J],
}
impl Row for FixtureRow<'_> {
    fn index(&self) -> usize {
        self.index
    }
    fn columns(&self) -> Vec<String> {
        vec!["v".into(), "w".into()]
    }
    fn cell(&self, column: &str) -> Option<Value> {
        let i = ["v", "w"].iter().position(|c| *c == column)?;
        Some(cell(&self.cells[i]))
    }
}

fn cell(j: &J) -> Value {
    match j.as_str() {
        Some("") | None => Value::Null,
        Some(s) => Value::Str(s.into()),
    }
}

/// OpenRefine's preview output: a string, null, or {"message"} for an error.
fn same(expected: &J, got: &Value) -> bool {
    match (expected, got) {
        (J::Object(_), Value::Error(_)) => true,
        (J::Null, v) => v.is_null(),
        (J::String(s), v) => v.render().as_deref() == Some(s.replace("\r\n", "\n").as_str()),
        _ => false,
    }
}

#[test]
fn grel_matches_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/grel/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let inputs = fx["inputs"].as_array().unwrap();
    let (mut checked, mut failing, mut report) = (0, BTreeSet::new(), vec![]);
    for case in fx["cases"].as_array().unwrap() {
        let expr = case["expr"].as_str().unwrap();
        checked += 1;
        let program = grel::compile(expr);
        let mut bad: Option<String> = None;
        match (case.get("parseError"), &program) {
            (Some(_), Err(_)) => {}
            (Some(e), Ok(_)) => bad = Some(format!("expected parse error {e}")),
            (None, Err(e)) => bad = Some(format!("parse failed: {e}")),
            (None, Ok(p)) => {
                for (i, exp) in case["results"].as_array().unwrap().iter().enumerate() {
                    let row = FixtureRow { index: i, cells: inputs[i].as_array().unwrap() };
                    let got = p.eval(cell(&inputs[i][0]), &row);
                    if !same(exp, &got) {
                        bad = Some(format!("row {i}: expected {exp}, got {}", match &got {
                            Value::Error(e) => format!("ERROR({e})"),
                            v => format!("{:?}", v.render()),
                        }));
                        break;
                    }
                }
            }
        }
        if let Some(why) = bad {
            failing.insert(expr.to_string());
            if !known.contains(expr) {
                report.push(format!("NEW FAILURE  {expr}\n    {why}"));
            }
        } else if known.contains(expr) {
            report.push(format!("NOW PASSES (remove from known_failures.txt)  {expr}"));
        }
    }
    println!("GREL conformance: {}/{checked} expressions match OpenRefine 3.10.1 ({} known failures)", checked - failing.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
