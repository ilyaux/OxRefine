use ox_core::{grel::Value, import::{import, inspect, ImportOptions}};
use serde_json::{json, Value as J};
use std::path::Path;

fn cell(value: &Value) -> J { match value { Value::Null => J::Null, Value::Str(s) => json!({"v":s}), Value::Int(i) => json!({"v":i}), Value::Num(n) => json!({"v":n}), other => panic!("unexpected imported cell {other:?}") } }

#[test]
fn pcaxis_rows_and_wizard_match_the_real_bundled_openrefine_importer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/pcaxis");
    let fixture: J = serde_json::from_str(include_str!("../../../tests/pcaxis/fixtures.json")).unwrap();
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let path = root.join(case["file"].as_str().unwrap());
        let options: ImportOptions = serde_json::from_value(case["options"].clone()).unwrap();
        let imported = import(&path, Some("text/line-based/pc-axis"), &options);
        if case.get("error").is_some() { assert!(imported.is_err(), "{} must fail", case["name"]); continue; }
        let table = imported.unwrap_or_else(|e| panic!("{}: {e}", case["name"]));
        assert_eq!(json!(table.names), case["columns"], "{} columns", case["name"]);
        let rows = (0..table.rows).map(|r| table.columns.iter().map(|c| cell(c.get(r))).collect::<Vec<_>>()).collect::<Vec<_>>();
        assert_eq!(json!(rows), case["rows"], "{} rows", case["name"]); checked += 1;
    }
    assert_eq!(checked, 24);
    let path = root.join("inputs/cube-dimension-order-missing-markers.px");
    let metadata = inspect(&path).unwrap();
    assert_eq!(metadata["format"], "pcaxis");
    let options: ImportOptions = serde_json::from_value(metadata["options"].clone()).unwrap();
    let table = import(&path, None, &options).unwrap();
    assert_eq!(json!(table.names), fixture["wizard"]["columns"]);
    assert_eq!(json!((0..table.rows).map(|r| table.columns.iter().map(|c| cell(c.get(r))).collect::<Vec<_>>()).collect::<Vec<_>>()), fixture["wizard"]["rows"]);
    for alias in ["pcaxis", "pc-axis", "text/line-based/pc-axis"] {
        assert_eq!(import(&path, Some(alias), &ImportOptions::default()).unwrap().rows, 6);
    }
}

#[test]
fn malformed_pcaxis_never_replaces_the_current_project() {
    use ox_core::{dispatch, Project, Session, table::Table};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/pcaxis/inputs");
    let mut session = Session::default(); session.project = Some(Project::new("Keep me", Table::new(vec![], vec![], 0)));
    for file in ["missing-dimension-order.px", "unclosed-quoted-value.px"] {
        assert!(dispatch(&mut session, "import", json!({"path":root.join(file),"format":"pcaxis"})).is_err());
        assert_eq!(session.project.as_ref().unwrap().name, "Keep me");
    }
}
