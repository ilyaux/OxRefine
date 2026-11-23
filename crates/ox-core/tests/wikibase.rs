//! OpenRefine 3.10.1's wikibase extension (tests/wikibase/gen.mjs): on one reconciled project, the
//! schema each case saves, its schema export and its QuickStatements export.
use ox_core::{import::{import, ImportOptions}, ops::Op, recon::Judgment, wikibase, Project};
use serde_json::{json, Value};

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../../../tests/wikibase/fixtures.json")).unwrap()
}

/// The project every case starts from: the CSV, then the same Recon steps OpenRefine ran.
fn project(f: &Value) -> Project {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ox-wikibase-{}-{n}.csv", std::process::id()));
    std::fs::write(&path, f["csv"].as_str().unwrap()).unwrap();
    let mut p = Project::new("wikibase", import(&path, Some("csv"), &ImportOptions::default()).unwrap());
    let _ = std::fs::remove_file(&path);
    for op in f["setup"].as_array().unwrap() {
        p.apply(serde_json::from_value(op.clone()).unwrap()).unwrap();
    }
    p
}

/// Where OxRefine writes QuickStatements differently on purpose (docs/src/differences.md): an
/// "add" statement is an addition, not a removal, and year -44 is -0044.
fn deliberate(case: &str, text: &str) -> String {
    match case {
        "statement modes" => text.lines().map(|l| match l.strip_prefix("- ") {
            Some(rest) if rest.split('\t').nth(1) == Some("P27") => rest.to_string(),
            _ => l.to_string(),
        }).collect::<Vec<_>>().join("\n") + "\n",
        "constant formats" => text.replace("+-044-", "-0044-"),
        _ => text.to_string(),
    }
}

#[test]
fn reconciled_cells_match_openrefine() {
    let f = fixtures();
    let p = project(&f);
    let t = p.table();
    let expected = f["cases"][0]["table"].as_array().unwrap();
    assert_eq!(expected.len(), t.rows);
    for (row, cells) in expected.iter().enumerate() {
        for column in ["item", "country"] {
            let c = t.col(column).unwrap();
            let theirs = &cells[c]["r"];
            let ours = t.columns[c].recon(row).map(|r| json!({
                "j": match r.judgment { Judgment::Matched => "matched", Judgment::New => "new", Judgment::None => "none", Judgment::Error => "error" },
                "m": r.matched.as_ref().map(|m| m.id.clone()),
            })).unwrap_or(Value::Null);
            assert_eq!(&ours, theirs, "row {row} {column}");
        }
    }
}

#[test]
fn schemas_and_exports_match_openrefine() {
    let f = fixtures();
    let languages: Vec<String> = serde_json::from_value(f["languages"].clone()).unwrap();
    let known = |code: &str| languages.iter().any(|l| l == code);
    let mut checked = 0;
    for case in f["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut p = project(&f);
        let op: Op = serde_json::from_value(json!({ "op": "wikidata/save-wikibase-schema", "schema": case["schema"] })).unwrap();
        let saved = p.apply(op);
        if case["saved"] == "error" {
            let error = saved.unwrap_err();
            for e in case["preview"]["errors"].as_array().unwrap() {
                assert!(error.contains(e["message"].as_str().unwrap()), "{name}: {error}");
            }
            assert_eq!(p.overlay("wikibaseSchema"), None);
            continue;
        }
        saved.unwrap_or_else(|e| panic!("{name}: {e}"));
        let overlay = p.overlay("wikibaseSchema").unwrap();
        assert_eq!(overlay, &case["overlay"], "{name}: stored schema");
        assert_eq!(overlay.to_string(), case["exports"]["wikibase-schema"].as_str().unwrap(), "{name}: schema export");
        let schema = wikibase::Schema::parse(overlay).unwrap();
        let rows: Vec<u32> = (0..p.table().rows as u32).collect();
        let qs = wikibase::quickstatements(&schema, p.table(), &rows, &known);
        match case["exports"]["quickstatements"].as_str() {
            Some(text) => assert_eq!(qs.unwrap_or_else(|e| panic!("{name}: {e}")), deliberate(name, text), "{name}: QuickStatements"),
            // OpenRefine fails (NullPointerException on media info); OxRefine says why
            None => assert!(qs.is_err(), "{name}"),
        }
        // undo removes the schema; redo brings it back
        p.goto(p.steps().len() - 1).unwrap();
        assert_eq!(p.overlay("wikibaseSchema"), None);
        p.goto(p.steps().len()).unwrap();
        assert_eq!(p.overlay("wikibaseSchema"), Some(&case["overlay"]));
        checked += 1;
    }
    assert_eq!(checked, 14);
}

#[test]
fn issues_match_openrefine_preview_and_wikibase_issues() {
    use std::collections::BTreeSet;
    let f = fixtures();
    let mut checked = 0;
    for case in f["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        if case["saved"] == "error" { continue; }
        let mut p = project(&f);
        p.apply(serde_json::from_value(json!({ "op": "wikidata/save-wikibase-schema", "schema": case["schema"] })).unwrap()).unwrap();
        // the schema preview's warnings: kind, bucket, severity and count (OpenRefine lists them in hash order)
        let schema = wikibase::Schema::parse(p.overlay("wikibaseSchema").unwrap()).unwrap();
        let rows: Vec<u32> = (0..p.table().rows as u32).collect();
        let (_, warnings) = wikibase::inspect(&schema, p.table(), &rows, &wikibase::language_check(&schema));
        let ours: BTreeSet<String> = warnings.iter().map(|w| format!("{} {:?} {} {}", w.kind, w.bucket, w.severity, w.count)).collect();
        let theirs: BTreeSet<String> = case["preview"]["warnings"].as_array().unwrap().iter()
            .map(|w| format!("{} {:?} {} {}", w["type"].as_str().unwrap(), w["bucketId"].as_str(), w["severity"].as_str().unwrap(), w["count"])).collect();
        assert_eq!(ours, theirs, "{name}: preview warnings");
        // wikibaseIssues() on each row, through the expression preview
        let mut session = ox_core::Session { project: Some(p), store: None };
        let preview = ox_core::dispatch(&mut session, "preview", json!({ "column": "name", "expression": "wikibaseIssues()", "limit": 6 })).unwrap();
        for (row, pair) in preview["data"].as_array().unwrap().iter().enumerate() {
            let parse = |v: &Value| -> BTreeSet<String> { serde_json::from_str::<Vec<String>>(v.as_str().unwrap()).unwrap().into_iter().collect() };
            assert_eq!(parse(&pair[1]), parse(&case["issues"][row]), "{name}: row {row} issues");
        }
        checked += 1;
    }
    assert_eq!(checked, 14);
}

/// Constraint checks (tests/wikibase/constraints-gen.mjs): made-up properties each carrying one
/// Wikidata constraint, and a schema that breaks them all.
#[test]
fn constraint_checks_match_openrefine() {
    use std::collections::BTreeSet;
    let f = fixtures();
    let c: Value = serde_json::from_str(include_str!("../../../tests/wikibase/constraints.json")).unwrap();
    let mut p = project(&f);
    p.apply(serde_json::from_value(json!({ "op": "wikidata/save-wikibase-schema", "schema": c["schema"] })).unwrap()).unwrap();
    let schema = wikibase::Schema::parse(p.overlay("wikibaseSchema").unwrap()).unwrap();
    let properties = c["properties"].clone();
    let constraints = move |pid: &str| wikibase::constraints_of(&json!({ "P2302": properties[pid][1] }));
    let languages = |code: &str| ["en", "de", "fr", "mul"].contains(&code);
    let rows: Vec<u32> = (0..p.table().rows as u32).collect();
    let (_, warnings) = wikibase::inspect_with(&schema, p.table(), &rows, &languages, &constraints);
    let ours: BTreeSet<String> = warnings.iter().map(|w| format!("{} {} {}", w.aggregation_id(), w.severity, w.count)).collect();
    let theirs: BTreeSet<String> = c["warnings"].as_array().unwrap().iter().map(|w| format!("{} {} {}", w["aggregationId"].as_str().unwrap(), w["severity"].as_str().unwrap(), w["count"])).collect();
    assert_eq!(ours, theirs);
    for (row, expected) in c["issues"].as_array().unwrap().iter().enumerate() {
        let (edits, warnings) = wikibase::inspect_with(&schema, p.table(), &[row as u32], &languages, &constraints);
        let ours: BTreeSet<String> = if warnings.is_empty() && !edits.is_empty() { ["no-issue-detected".to_string()].into() } else { warnings.iter().map(|w| w.aggregation_id()).collect() };
        let theirs: BTreeSet<String> = serde_json::from_str::<Vec<String>>(expected.as_str().unwrap()).unwrap().into_iter().collect();
        assert_eq!(ours, theirs, "row {row}");
    }
}
