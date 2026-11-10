use ox_core::{cluster, dispatch, Project};
use serde_json::json;

#[test]
fn recipe_facets_cluster_history() {
    let dir = std::env::temp_dir().join("oxrefine-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("in.csv");
    std::fs::write(&csv, "\u{feff}name;city;city\n  alice smith ;New York\nBOB;new york ;x\nCarol;New-York\n;Boston\n").unwrap();

    let mut slot: Option<Project> = None;
    let mut call = |cmd: &str, args: serde_json::Value| dispatch(&mut slot, cmd, args).unwrap()["data"].clone();

    let s = call("open", json!({ "path": csv }));
    assert_eq!(s["columns"], json!(["name", "city", "city 2"]), "BOM stripped, ';' sniffed, dup header renamed");
    assert_eq!(s["rows"], 4);

    // Recipe exactly as OpenRefine's "Extract operation history" emits it (subset of fields).
    let city_facet = json!({"facets": [{"type": "list", "name": "city", "columnName": "city", "expression": "value",
        "selection": [{"v": {"v": "Boston", "l": "Boston"}}], "selectBlank": false, "invert": true}], "mode": "row-based"});
    call("apply_recipe", json!({ "ops": [
        {"op": "core/text-transform", "engineConfig": {"facets": [], "mode": "row-based"}, "columnName": "name",
         "expression": "grel:value.replace(/\\s+/, ' ').trim().toTitlecase()", "onError": "keep-original", "repeat": false, "repeatCount": 10},
        {"op": "core/text-transform", "engineConfig": city_facet, "columnName": "name", "expression": "value.toUppercase()"},
        {"op": "core/column-addition", "baseColumnName": "city", "newColumnName": "key", "columnInsertIndex": 2,
         "expression": "grel:value.fingerprint()"},
        {"op": "core/column-removal", "columnName": "city 2"},
    ]}));
    let rows = call("rows", json!({ "start": 0, "limit": 10 }));
    assert_eq!(rows["rows"][0], json!([0, ["ALICE SMITH", "New York", "new york"]]));
    assert_eq!(rows["rows"][3], json!([3, ["", "Boston", "boston"]]), "facet-excluded row untouched");

    // ngram clusters all three spellings; fingerprint misses New-York (punctuation is removed, not split)
    let c = call("cluster", json!({ "column": "city", "method": "ngram-fingerprint" }));
    assert_eq!(c[0].as_array().unwrap().len(), 3);
    let c = call("cluster", json!({ "column": "city", "method": "fingerprint" }));
    assert_eq!(c[0].as_array().unwrap().len(), 2);
    assert_eq!(cluster::fingerprint(" Café, de PARIS café "), "cafe de paris");

    call("apply", json!({ "op": {"op": "core/mass-edit", "columnName": "city", "expression": "value",
        "edits": [{"from": ["new york ", "New-York"], "fromBlank": false, "fromError": false, "to": "New York"}]}}));
    let f = call("facets", json!({ "engine": {"facets": [{"type": "list", "columnName": "city", "selection": []},
        {"type": "text", "columnName": "name", "query": "o"}]} }));
    assert_eq!(f[0]["choices"], json!([["New York", 2]]), "list counts respect the text facet (BOB, CAROL)");

    let s = call("goto", json!({ "index": 1 }));
    assert_eq!(s["history"].as_array().unwrap().len(), 5);
    assert_eq!(call("rows", json!({ "limit": 1 }))["rows"][0][1][0], "Alice Smith", "undo back to step 1");
    assert_eq!(call("export_recipe", json!({})).as_array().unwrap().len(), 1);

    let bad = dispatch(&mut slot, "preview", json!({ "column": "name", "expression": "value.split(',')" }));
    assert!(bad.unwrap_err().contains("unsupported function `split`"));
}
