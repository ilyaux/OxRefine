use ox_core::{cluster, dispatch, Session};
use serde_json::json;

#[test]
fn recipe_facets_cluster_history() {
    let dir = std::env::temp_dir().join("oxrefine-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("in.csv");
    std::fs::write(&csv, "\u{feff}name;city;city\n  alice smith ;New York\nBOB;new york ;x\nCarol;New-York\n;Boston\n").unwrap();

    let mut slot = Session::default();
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
    assert_eq!(rows["rows"][0], json!([0, ["ALICE SMITH", "New York", "new york"], false, false]));
    assert_eq!(rows["rows"][3], json!([3, [null, "Boston", "boston"], false, false]), "facet-excluded row untouched; empty cells are null");

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
    assert_eq!(f[0]["choices"], json!([["New York", 2, "New York"]]), "list counts respect the text facet (BOB, CAROL)");

    let s = call("goto", json!({ "index": 1 }));
    assert_eq!(s["history"].as_array().unwrap().len(), 5);
    assert_eq!(call("rows", json!({ "limit": 1 }))["rows"][0][1][0], "Alice Smith", "undo back to step 1");
    assert_eq!(call("export_recipe", json!({})).as_array().unwrap().len(), 1);

    let bad = dispatch(&mut slot, "preview", json!({ "column": "name", "expression": "value.nope()" }));
    assert!(bad.unwrap_err().contains("Unknown function nope"));
}

#[test]
fn typed_cells_row_context_and_on_error() {
    let dir = std::env::temp_dir().join("oxrefine-typed");
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("in.csv");
    std::fs::write(&csv, "qty,price,note\n3,2.5,a\n10,,b\nx,4,c\n").unwrap();
    let mut slot = Session::default();
    let mut call = |cmd: &str, args: serde_json::Value| dispatch(&mut slot, cmd, args).unwrap()["data"].clone();
    call("open", json!({ "path": csv }));
    let t = |col: &str, expr: &str, on_error: &str| json!({ "op": {"op": "core/text-transform", "columnName": col, "expression": expr, "onError": on_error} });

    call("apply", t("qty", "value.toNumber()", "keep-original"));
    call("apply", t("price", "value.toNumber()", "set-to-blank"));
    // per-row expression reading another column; numbers keep Long/Double semantics (10 * null -> null)
    call("apply", json!({ "op": {"op": "core/column-addition", "baseColumnName": "qty", "newColumnName": "total", "columnInsertIndex": 3,
        "expression": "grel:value * cells['price'].value", "onError": "set-to-blank"} }));
    let rows = call("rows", json!({ "limit": 10 }))["rows"].clone();
    assert_eq!(rows[0][1], json!(["3", "2.5", "a", "7.5"]), "3 * 2.5");
    assert_eq!(rows[1][1], json!(["10", null, "b", null]), "blank price stays null");
    assert_eq!(rows[2][1], json!(["x", "4", "c", null]), "keep-original kept 'x'; 'x' * 4 is null");
    // an array result can't be stored: store-error keeps the error in the cell
    call("apply", t("note", "value.split('')", "store-error"));
    assert_eq!(call("rows", json!({ "limit": 1 }))["rows"][0][1][2], json!("array value not storable"));
    let p = call("preview", json!({ "column": "note", "expression": "isError(value)" }));
    assert_eq!(p[0][1], json!("true"), "type(error) would propagate the error, as in OpenRefine");
    // rowIndex makes the expression run per row, not once per distinct value
    call("apply", t("note", "rowIndex + 1", "keep-original"));
    assert_eq!(call("rows", json!({ "limit": 10 }))["rows"][2][1][2], json!("3"));
}

#[test]
fn projects_are_saved_and_reopened() {
    let home = std::env::temp_dir().join(format!("oxrefine-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let csv = home.join("in.csv");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(&csv, "name,n\n alice ,1\nbob,2\n").unwrap();
    let store = ox_core::store::Store::open(&home).unwrap();
    let call = |s: &mut Session, cmd: &str, args: serde_json::Value| dispatch(s, cmd, args).unwrap()["data"].clone();

    let mut s = Session::with_store(store.clone());
    let id = call(&mut s, "open", json!({ "path": csv, "name": "People" }))["id"].as_str().unwrap().to_string();
    call(&mut s, "apply", json!({ "op": {"op": "core/text-transform", "columnName": "name", "expression": "value.trim()"} }));
    call(&mut s, "apply", json!({ "op": {"op": "core/text-transform", "columnName": "n", "expression": "value.toNumber() * 10"} }));
    call(&mut s, "goto", json!({ "index": 1 }));
    drop(s);

    // a new session (an app restart) sees the project and its history, cursor included
    let mut s = Session::with_store(store.clone());
    let list = call(&mut s, "projects", json!({}));
    assert_eq!(list[0]["name"], "People");
    let p = call(&mut s, "open_project", json!({ "id": id }));
    assert_eq!(p["history"].as_array().unwrap().len(), 2);
    assert_eq!(p["cursor"], 1);
    assert_eq!(call(&mut s, "rows", json!({ "limit": 1 }))["rows"][0][1], json!(["alice", "1"]));
    call(&mut s, "goto", json!({ "index": 2 }));
    assert_eq!(call(&mut s, "rows", json!({ "limit": 1 }))["rows"][0][1], json!(["alice", "10"]), "redo works after reopening");

    call(&mut s, "rename_project", json!({ "id": id, "name": "Renamed" }));
    assert_eq!(call(&mut s, "projects", json!({}))[0]["name"], "Renamed");
    assert!(dispatch(&mut s, "delete_project", json!({ "id": "../../etc" })).is_err(), "ids can't escape the store");
    call(&mut s, "delete_project", json!({ "id": id }));
    assert_eq!(call(&mut s, "projects", json!({})), json!([]));
    let _ = std::fs::remove_dir_all(&home);
}
