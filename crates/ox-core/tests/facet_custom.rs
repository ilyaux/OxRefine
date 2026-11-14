//! These five presets are exercised against a checksum-pinned official OpenRefine 3.10.1 release.
use ox_core::{engine::{facet_counts, filter, EngineConfig}, grel::{self, Value}, ops::Op, Project};
use serde_json::{json, Value as J};
use std::collections::BTreeMap;

fn fixture() -> J { serde_json::from_str(include_str!("../../../tests/facet_custom/fixtures.json")).unwrap() }
fn project(fixture: &J) -> Project {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!("oxrefine-custom-facets-{}-{}.csv", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&path, fixture["corpus"].as_str().unwrap()).unwrap();
    let mut project = Project::open(&path).unwrap();
    for op in fixture["ops"].as_array().unwrap() { project.apply(serde_json::from_value::<Op>(op.clone()).unwrap()).unwrap(); }
    project
}

#[test]
fn preset_expressions_match_typed_null_empty_error_and_log_edges() {
    let fixture = fixture();
    let project = project(&fixture);
    let column = &project.table().columns[1];
    assert!(column.get(7).is_null());
    assert_eq!(column.get(8), &Value::Str(String::new()), "empty string must stay distinct from null");
    assert!(matches!(column.get(11), Value::Error(_)));
    let mut mismatches = vec![];
    for preset in fixture["presets"].as_array().unwrap() {
        let expression = preset["expression"].as_str().unwrap();
        let program = grel::compile(expression).unwrap();
        for (row, expected) in preset["preview"].as_array().unwrap().iter().enumerate() {
            let value = program.eval(column.get(row).clone(), &grel::NoRow);
            let same = if expected.is_object() { matches!(value, Value::Error(_)) }
                else { value.render().as_deref() == expected.as_str() };
            if !same { mismatches.push(format!("{expression}, row {row}: expected {expected}, got {:?}", value.render())); }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

fn choices(panel: &J, reference: bool) -> (BTreeMap<String, u64>, u64, u64) {
    if reference {
        (panel["choices"].as_array().unwrap().iter().map(|choice| (choice["v"]["l"].as_str().unwrap().to_owned(), choice["c"].as_u64().unwrap())).collect(),
            panel["blankChoice"]["c"].as_u64().unwrap_or(0), panel["errorChoice"]["c"].as_u64().unwrap_or(0))
    } else {
        (panel["choices"].as_array().unwrap().iter().map(|choice| (choice[0].as_str().unwrap().to_owned(), choice[1].as_u64().unwrap())).collect(),
            panel["blank"].as_u64().unwrap(), panel["error"].as_u64().unwrap())
    }
}

#[test]
fn preset_facets_and_selected_rows_records_match_reference() {
    let fixture = fixture();
    let project = project(&fixture);
    let mut mismatches = vec![];
    for case in fixture["cases"].as_array().unwrap() {
        let engine: EngineConfig = serde_json::from_value(case["engine"].clone()).unwrap();
        let matching = filter(project.table(), &engine, None).unwrap();
        if json!(matching) != case["matching"] { mismatches.push(format!("{} matching: expected {}, got {matching:?}", case["name"], case["matching"])); }
        let panels = facet_counts(project.table(), &engine, 2000).unwrap();
        for (index, (expected, actual)) in case["facets"].as_array().unwrap().iter().zip(panels).enumerate() {
            if case["engine"]["facets"][index]["type"] == "list" {
                if choices(expected, true) != choices(&actual, false) { mismatches.push(format!("{} facet{index} choices: expected {:?}, got {:?}", case["name"], choices(expected, true), choices(&actual, false))); }
            } else {
                for key in ["numericCount", "nonNumericCount", "blankCount", "errorCount", "baseNumericCount", "baseNonNumericCount", "baseBlankCount", "baseErrorCount", "min", "max", "step", "bins", "baseBins"] {
                    let same = if let (Some(a), Some(b)) = (expected[key].as_f64(), actual[key].as_f64()) { (a-b).abs() < 1e-9 } else { expected[key] == actual[key] };
                    if !same { mismatches.push(format!("{} facet{index} {key}: expected {}, got {}", case["name"], expected[key], actual[key])); }
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
