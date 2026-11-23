//! Steps run in a real OpenRefine through the bridge. It needs an OpenRefine installation:
//! OPENREFINE_HOME, or ../OpenRefine/openrefine-3.10.1 beside the repository (as the fixture
//! generators use). Without one (CI) the test says so and passes.
use ox_core::{import::{import, ImportOptions}, orproject, Project};
use serde_json::json;
use std::path::{Path, PathBuf};

fn openrefine() -> Option<PathBuf> {
    let candidates = [std::env::var_os("OPENREFINE_HOME").map(PathBuf::from), Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../OpenRefine/openrefine-3.10.1"))];
    candidates.into_iter().flatten().find(|p| ox_core::openrefine::home(p).is_ok())
}

#[test]
fn a_java_extension_step_runs_in_openrefine_and_comes_back_with_its_history() {
    let Some(home) = openrefine() else { eprintln!("no OpenRefine installation: skipped"); return };
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("people.csv");
    std::fs::write(&csv, "name,city\nAnn,oslo\nBob,rome\n").unwrap();
    let p = Project::new("bridge", import(&csv, Some("csv"), &ImportOptions::default()).unwrap());
    // Jython is one of OpenRefine's own Java extensions
    let ops = json!([
        { "op": "core/text-transform", "engineConfig": { "facets": [], "mode": "row-based" }, "columnName": "city", "expression": "jython:return value.upper()", "onError": "keep-original", "repeat": false, "repeatCount": 10 },
        { "op": "core/column-rename", "oldColumnName": "city", "newColumnName": "town" },
    ]);
    let out = dir.path().join("result.openrefine.tar.gz");
    ox_core::openrefine::run(&home, &p, &ops, &out).unwrap();
    let (back, report) = orproject::read(&out).unwrap().into_project();
    assert_eq!(report.merged, 0, "{:?}", report.reason);
    assert_eq!(back.steps().len(), 2, "OpenRefine's two steps come back as history");
    let t = back.table();
    assert_eq!(t.names, ["name", "town"]);
    assert_eq!((0..t.rows).map(|r| t.columns[1].get(r).to_java_string()).collect::<Vec<_>>(), ["OSLO", "ROME"]);
    // an unknown operation is OpenRefine's error, passed on
    let bad = ox_core::openrefine::run(&home, &p, &json!([{ "op": "no-such-extension/operation" }]), &dir.path().join("bad.tar.gz"));
    assert!(bad.is_err());
}
