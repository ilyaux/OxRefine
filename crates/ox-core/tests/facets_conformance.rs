//! Every case in tests/facets/fixtures.json must give the same facet panels (choices and counts,
//! histograms) and the same matching rows as OpenRefine 3.10.1. Exceptions live in known_failures.txt.
use ox_core::{
    engine::{facet_counts, filter, EngineConfig},
    ops::Op,
    Project,
};
use serde_json::{json, Value as J};
use std::collections::{BTreeMap, BTreeSet};

fn key(j: &J) -> String {
    match j {
        J::String(s) => s.clone(),
        J::Number(n) => n.as_i64().map_or_else(|| ox_core::grel::value::render_num(n.as_f64().unwrap()), |i| i.to_string()),
        other => other.to_string(),
    }
}

/// OpenRefine's list facet as {choice text: count} plus blank and error counts.
fn openrefine_list(f: &J) -> (BTreeMap<String, u64>, u64, u64) {
    let choices = f["choices"].as_array().unwrap().iter().map(|c| (key(&c["v"]["v"]), c["c"].as_u64().unwrap())).collect();
    (choices, f["blankChoice"]["c"].as_u64().unwrap_or(0), f["errorChoice"]["c"].as_u64().unwrap_or(0))
}

fn ours_list(f: &J) -> (BTreeMap<String, u64>, u64, u64) {
    let choices = f["choices"].as_array().unwrap().iter().map(|c| (c[0].as_str().unwrap().to_string(), c[1].as_u64().unwrap())).collect();
    (choices, f["blank"].as_u64().unwrap(), f["error"].as_u64().unwrap())
}

fn close(a: &J, b: &J) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

#[test]
fn facets_match_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}facets/fixtures.json")).unwrap()).unwrap();
    let ops_fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}ops/fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}facets/known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let corpus = std::env::temp_dir().join("oxrefine-facets-corpus.csv");
    std::fs::write(&corpus, ops_fx["corpus"].as_str().unwrap()).unwrap();

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut p = Project::open(&corpus).unwrap();
        for op in case["ops"].as_array().into_iter().flatten() {
            p.apply(serde_json::from_value::<Op>(op.clone()).unwrap()).unwrap();
        }
        let engine: EngineConfig = serde_json::from_value(case["engine"].clone()).unwrap();
        let t = p.table();
        let mut why = None;
        let rows: Vec<u64> = filter(t, &engine, None).unwrap().into_iter().map(u64::from).collect();
        let want: Vec<u64> = case["matching"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap()).collect();
        if rows != want {
            why = Some(format!("matching rows: expected {want:?}, got {rows:?}"));
        }
        let limit = case["prefs"]["ui.browsing.listFacet.limit"].as_u64().map_or(ox_core::engine::FACET_LIMIT, |n| n as usize);
        let panels = facet_counts(t, &engine, limit).unwrap();
        for (i, (exp, got)) in case["facets"].as_array().unwrap().iter().zip(&panels).enumerate() {
            if why.is_some() {
                break;
            }
            let kind = case["engine"]["facets"][i]["type"].as_str().unwrap();
            if kind == "list" && exp["error"] == "Too many choices" {
                // past the choice limit OpenRefine lists none and says how many there were
                if got["tooMany"] != true || got["distinct"] != exp["choiceCount"] {
                    why = Some(format!("facet {i}: expected too many choices ({}), got {got}", exp["choiceCount"]));
                }
            } else if kind == "list" {
                let (e, g) = (openrefine_list(exp), ours_list(got));
                if e != g {
                    why = Some(format!("facet {i}: expected {e:?}, got {g:?}"));
                }
            } else if kind == "timerange" {
                let keys = [
                    "timeCount", "nonTimeCount", "blankCount", "errorCount", "baseTimeCount", "baseNonTimeCount", "baseBlankCount", "baseErrorCount",
                    "min", "max", "step", "from", "to", "bins", "baseBins",
                ];
                for k in keys {
                    let ok = match (&exp[k], &got[k]) {
                        (J::Array(a), J::Array(b)) => a == b,
                        // OpenRefine's Long.MIN_VALUE arrives as a rounded double
                        (a, b) => close(a, b) || a.as_f64().zip(b.as_f64()).is_some_and(|(x, y)| (x - y).abs() <= x.abs() * 1e-15),
                    };
                    if !ok && why.is_none() {
                        why = Some(format!("facet {i} {k}: expected {}, got {}", exp[k], got[k]));
                    }
                }
            } else if kind == "range" {
                let fields = ["numericCount", "nonNumericCount", "blankCount", "errorCount", "baseNumericCount", "baseNonNumericCount", "baseBlankCount", "baseErrorCount"];
                let mut keys: Vec<&str> = fields.to_vec();
                if exp.get("error").is_some() {
                    if got.get("error").is_none() {
                        why = Some(format!("facet {i}: expected error {}", exp["error"]));
                    }
                } else {
                    keys.extend(["min", "max", "step", "bins", "baseBins"]);
                }
                for k in keys {
                    let ok = match (&exp[k], &got[k]) {
                        (J::Array(a), J::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| close(x, y)),
                        (a, b) => close(a, b),
                    };
                    if !ok && why.is_none() {
                        why = Some(format!("facet {i} {k}: expected {}, got {}", exp[k], got[k]));
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
    let _ = json!(null);
    println!("Facet conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
