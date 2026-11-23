//! tests/facets/scatter-fixtures.json: scatterplot facets let through the rows OpenRefine 3.10.1 lets
//! through (linear and unrotated whatever scale or rotation is asked, as OpenRefine filters), and a
//! recipe keeps a facet's drawing settings as given.
use ox_core::{
    engine::{facet_counts, filter, EngineConfig},
    ops::Op,
    Project,
};
use serde_json::{json, Value as J};

#[test]
fn scatterplot_facets_match_openrefine() {
    let fx: J = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/facets/scatter-fixtures.json")).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("scatter.csv");
    std::fs::write(&csv, fx["csv"].as_str().unwrap()).unwrap();
    let mut p = Project::open(&csv).unwrap();
    for op in fx["ops"].as_array().unwrap() {
        p.apply(serde_json::from_value::<Op>(op.clone()).unwrap()).unwrap();
    }
    let t = p.table();
    let mut wrong = vec![];
    for case in fx["cases"].as_array().unwrap() {
        let engine: EngineConfig = serde_json::from_value(case["engine"].clone()).unwrap();
        let rows = filter(t, &engine, None).unwrap();
        if json!(rows) != case["matching"] {
            wrong.push(format!("{}: expected {}, got {}", case["name"], case["matching"], json!(rows)));
        }
        // the panel plots the rows the other facets let through
        let panel = &facet_counts(t, &engine, 100).unwrap()[0];
        assert!(panel["all"].as_array().is_some_and(|a| !a.is_empty()), "{}: {panel}", case["name"]);
        // the facet goes back into JSON as it came, drawing settings included
        let text = serde_json::to_string(&engine).unwrap();
        assert_eq!(text.matches("\"type\"").count(), engine.facets.len(), "{text}");
        let again: J = serde_json::from_str(&text).unwrap();
        for (k, v) in case["engine"]["facets"][0].as_object().unwrap() {
            let got = &again["facets"][0][k];
            assert!(got == v || got.as_f64().is_some_and(|g| Some(g) == v.as_f64()), "{}: {k} {got} != {v}", case["name"]);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
