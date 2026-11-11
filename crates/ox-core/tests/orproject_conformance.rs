//! OpenRefine project archives from tests/orproject (made by a real OpenRefine 3.10.1):
//! 1. reading one gives OpenRefine's table at every history step (from its change files alone);
//! 2. as an OxRefine project, every step replays to that same table, with OpenRefine's history;
//! 3. writing the project out and reading it back gives the same tables and history again.
//! Exported archives land in target/orproject-export/ for tests/orproject/check-export.mjs, which
//! opens them in OpenRefine itself. Cases in known_failures.txt may lose history (and must).
use ox_core::{grel::Value, orproject, table::Table, Project};
use serde_json::{json, Value as J};
use std::{collections::BTreeSet, path::Path};

/// Cell as OpenRefine's get-rows shows it (whole doubles as integers, errors only as "an error").
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

fn shown(t: &Table) -> J {
    json!({
        "columns": t.names,
        "rows": (0..t.rows).map(|r| json!({ "starred": t.starred[r], "flagged": t.flagged[r], "cells": t.columns.iter().map(|c| cell(c.get(r))).collect::<Vec<_>>() })).collect::<Vec<_>>(),
    })
}

fn expected(state: &J) -> J {
    let rows: Vec<J> = state["rows"].as_array().unwrap().iter().map(|r| {
        json!({ "starred": r["starred"], "flagged": r["flagged"], "cells": r["cells"].as_array().unwrap().iter().map(norm).collect::<Vec<_>>() })
    }).collect();
    json!({ "columns": state["columns"], "rows": rows })
}

/// Every table of a project and its history descriptions.
fn steps(p: &mut Project) -> (Vec<J>, Vec<String>) {
    let n = p.steps().len();
    let history = p.steps().iter().map(|o| o.description().to_string()).collect();
    let at = (0..=n).map(|i| p.goto(i).map(|_| shown(p.table())).unwrap()).collect();
    (at, history)
}

#[test]
fn openrefine_project_archives() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/orproject/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let out = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/orproject-export"));
    std::fs::create_dir_all(out).unwrap();
    // the project cross() cases look into
    let ops: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}../ops/fixtures.json")).unwrap()).unwrap();
    let lookup_csv = out.join("lookup.csv");
    std::fs::write(&lookup_csv, ops["lookup"].as_str().unwrap()).unwrap();
    let lookup = Project::open(&lookup_csv).unwrap().table().clone();

    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in fx["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let file = case["archive"].as_str().unwrap();
        let states: Vec<J> = case["states"].as_array().unwrap().iter().map(expected).collect();
        let history: Vec<String> = case["history"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
        let cursor = case["cursor"].as_u64().unwrap() as usize;

        // 1. the tables from OpenRefine's change files: any miss here is a reader bug
        let a = orproject::read(&Path::new(root).join("archives").join(file)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((a.first, a.last(), a.cursor), (0, history.len(), cursor), "{name}: history read only partly: {:?}", a.stopped);
        assert_eq!(a.history, history, "{name}: history");
        for (i, s) in states.iter().enumerate() {
            assert_eq!(&shown(&a.table(i).unwrap()), s, "{name}: table after {i} steps, from the change files");
        }

        // 2. as an OxRefine project: replayed steps give OpenRefine's tables
        let lk = lookup.clone();
        let (mut p, r) = ox_core::cross::scope(move |n| if n == "lookup" { vec![lk.clone()] } else { vec![] }, || a.into_project());
        let (tables, descriptions) = steps(&mut p);
        let mut why = None;
        if r.merged > 0 || r.dropped > 0 {
            why = Some(format!("{} steps merged, {} dropped: {}", r.merged, r.dropped, r.reason.clone().unwrap_or_default()));
        }
        assert_eq!(descriptions, history[r.merged..history.len() - r.dropped], "{name}: descriptions");
        for (i, t) in tables.iter().enumerate() {
            assert_eq!(t, &states[r.merged + i], "{name}: OxRefine's table after {} steps", r.merged + i);
        }

        // 3. write it out and read it back
        let path = out.join(file);
        p.goto(cursor - r.merged).unwrap();
        orproject::write(&p, &path).unwrap_or_else(|e| panic!("{name}: writing: {e}"));
        let back = orproject::read(&path).unwrap_or_else(|e| panic!("{name}: reading our archive: {e}"));
        assert_eq!((back.first, back.last(), back.cursor), (0, tables.len() - 1, cursor - r.merged), "{name}: our archive: {:?}", back.stopped);
        let lk = lookup.clone();
        let (mut q, r2) = ox_core::cross::scope(move |n| if n == "lookup" { vec![lk.clone()] } else { vec![] }, || back.into_project());
        assert_eq!((r2.merged, r2.dropped), (0, 0), "{name}: our archive replays: {:?}", r2.reason);
        assert_eq!(steps(&mut q), (tables, descriptions), "{name}: our archive");

        if let Some(w) = why {
            failing.insert(name.to_string());
            if !known.contains(name) {
                report.push(format!("NEW  {name}: {w}"));
            }
        } else if known.contains(name) {
            report.push(format!("FIXED {name}: remove it from known_failures.txt"));
        }
    }
    let n = fx["cases"].as_array().unwrap().len();
    eprintln!("orproject conformance: {}/{n} cases keep their whole history", n - failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
