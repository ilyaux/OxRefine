//! Every Jython and Clojure expression in tests/langs/fixtures.json must render exactly as OpenRefine
//! 3.10.1 rendered it, except those listed in tests/langs/known_failures.txt (with the reason). A known
//! failure that starts passing fails the test too, so the list only ever shrinks.
use ox_core::grel::{self, Row, Value};
use serde_json::Value as J;
use std::collections::BTreeSet;

struct FixtureRow<'a> {
    index: usize,
    inputs: &'a [J],
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
        Some(cell(&self.inputs[self.index][i]))
    }
}

fn cell(j: &J) -> Value {
    match j.as_str() {
        Some("") | None => Value::Null,
        Some(s) => Value::Str(s.into()),
    }
}

fn same(expected: &J, got: &Value) -> bool {
    match (expected, got) {
        (J::Object(_), Value::Error(_)) => true,
        (J::Null, v) => v.is_null(),
        (J::String(s), v) => v.render().as_deref() == Some(s.as_str()),
        _ => false,
    }
}

/// One line per case in known_failures.txt: `lang:expression`, newlines written as \n.
fn key(lang: &str, expr: &str) -> String {
    format!("{lang}:{}", expr.replace('\n', "\\n"))
}

#[test]
fn jython_and_clojure_match_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/langs/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let inputs = fx["inputs"].as_array().unwrap();
    let (mut checked, mut failing, mut report) = (0, BTreeSet::new(), vec![]);
    for c in fx["cases"].as_array().unwrap() {
        let (lang, expr) = (c["lang"].as_str().unwrap(), c["expr"].as_str().unwrap());
        let k = key(lang, expr);
        checked += 1;
        let program = grel::compile(&format!("{lang}:{expr}"));
        let mut bad = vec![];
        match (&c["parseError"], program) {
            (J::String(_), Err(_)) => {}
            (J::String(m), Ok(_)) => bad.push(format!("expected parse error {m:?}, compiled")),
            (_, Err(e)) => bad.push(format!("parse error {e}")),
            (_, Ok(p)) => {
                let rows: Vec<usize> = if c["rows"] == "all" { (0..inputs.len()).collect() } else { vec![0] };
                for (i, want) in rows.iter().zip(c["results"].as_array().unwrap()) {
                    let row = FixtureRow { index: *i, inputs };
                    let got = p.eval(cell(&inputs[*i][0]), &row);
                    if !same(want, &got) {
                        bad.push(format!("row {i}: want {want}, got {got:?}"));
                    }
                }
            }
        }
        if !bad.is_empty() {
            failing.insert(k.clone());
            if !known.contains(&k) {
                report.push(format!("{k}\n    {}", bad.join("\n    ")));
            }
        }
    }
    let fixed: Vec<_> = known.iter().filter(|k| !failing.contains(*k)).collect();
    println!("{checked} expressions, {} failing ({} known)", failing.len(), known.len());
    assert!(report.is_empty(), "{} new failures:\n{}", report.len(), report.join("\n"));
    assert!(fixed.is_empty(), "now passing, remove from known_failures.txt: {fixed:?}");
}
