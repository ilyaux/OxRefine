use ox_core::{dispatch, grel::Value, import::{import, inspect, ImportOptions}, jobs, Project, Session};
use serde_json::{json, Value as J};
use std::{fs, io::Write, path::{Path, PathBuf}, time::{Duration, Instant}};

fn fixture() -> J { serde_json::from_str(include_str!("../../../tests/marc/fixtures.json")).unwrap() }
fn source(name: &str) -> PathBuf { Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/marc")).join(name) }
fn cell(value: &Value) -> J {
    match value {
        Value::Null => J::Null, Value::Str(s) => json!({ "v": s }), Value::Int(n) => json!({ "v": n }), Value::Num(n) => json!({ "v": n }),
        Value::Bool(b) => json!({ "v": b }), other => panic!("unexpected imported value: {}", other.display()),
    }
}

#[test]
fn marc_matches_real_wizard_for_utf8_marc8_scripts_combining_entities_and_options() {
    for case in fixture()["cases"].as_array().unwrap().iter().filter(|c| c["reject"] != true) {
        let options: ImportOptions = serde_json::from_value(case["options"].clone()).unwrap();
        let table = import(&source(case["file"].as_str().unwrap()), Some("text/marc"), &options).unwrap_or_else(|e| panic!("{}: {e}", case["name"]));
        let columns: Vec<String> = serde_json::from_value(case["columns"].clone()).unwrap();
        assert_eq!(table.names, columns, "{} columns", case["name"]);
        let mut expected = case["rows"].clone();
        if let Some(correct) = case["locUnicode"].as_str() {
            let value_column = columns.iter().position(|c| c.ends_with(" - marc:subfield")).unwrap();
            assert_ne!(expected[0][value_column]["v"], correct, "reference's u16 truncation is retained as evidence");
            expected[0][value_column] = json!({ "v": correct });
        }
        let actual: Vec<J> = (0..table.rows).map(|r| J::Array(table.columns.iter().map(|c| cell(c.get(r))).collect())).collect();
        assert_eq!(json!(actual), expected, "{} rows", case["name"]);
    }
}

#[test]
fn malformed_records_fail_with_context_without_replacing_the_open_project() {
    let fixtures = fixture();
    let initial = import(&source("ascii.mrc"), None, &ImportOptions::default()).unwrap();
    let mut session = Session { project: Some(Project::new("keep this project", initial)), store: None };
    let before = dispatch(&mut session, "summary", json!({})).unwrap();
    for case in fixtures["cases"].as_array().unwrap().iter().filter(|c| c["reject"] == true) {
        let result = dispatch(&mut session, "open", json!({ "path": source(case["file"].as_str().unwrap()), "format": "marc" }));
        match result { Err(message) => assert!(message.contains("MARC record 1 at byte 0"), "{}: {message}", case["name"]), Ok(_) => panic!("{} must be rejected", case["name"]) }
        assert_eq!(dispatch(&mut session, "summary", json!({})).unwrap()["data"], before["data"], "{} keeps the previous project", case["name"]);
    }
}

#[test]
fn inspect_defaults_use_qualified_marcxml_paths_and_ignore_text_encoding() {
    let info = inspect(&source("multiple-records.mrc")).unwrap();
    assert_eq!(info["format"], "marc");
    assert_eq!(info["options"]["recordPath"], json!(["marc:collection", "marc:record"]));
    assert_eq!(info["options"]["encoding"], "");
    assert!(info["elements"].as_array().unwrap().iter().any(|e| e["path"] == json!(["marc:collection", "marc:record"]) && e["count"] == 2));
    let options: ImportOptions = serde_json::from_value(info["options"].clone()).unwrap();
    let table = import(&source("multiple-records.mrc"), None, &options).unwrap();
    assert_eq!(table.rows, 8);
    let ignored = ImportOptions { encoding: "not-a-text-encoding".into(), ..options };
    assert_eq!(import(&source("multiple-records.mrc"), None, &ignored).unwrap().rows, table.rows);
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self { Self(std::env::temp_dir().join(format!("oxrefine-marc-{}-{}.mrc", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))) }
}
impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }

#[test]
fn concatenated_records_validate_all_frames_and_empty_input_is_an_error() {
    let temp = Temp::new();
    fs::write(&temp.0, []).unwrap();
    match import(&temp.0, None, &ImportOptions::default()) { Err(e) => assert!(e.contains("no ISO2709 records")), Ok(_) => panic!("empty MARC must report no records") }
    let record = fs::read(source("utf8.mrc")).unwrap();
    fs::write(&temp.0, record.repeat(200)).unwrap();
    assert_eq!(import(&temp.0, None, &ImportOptions::default()).unwrap().rows, 200);
    let mut broken = fs::read(source("ascii.mrc")).unwrap(); broken.extend(&record[..15]);
    fs::write(&temp.0, broken).unwrap();
    let options = ImportOptions { limit: 1, ..Default::default() };
    match import(&temp.0, None, &options) { Err(e) => assert!(e.contains("MARC record 2")), Ok(_) => panic!("conversion validates the full stream before the XML row limit") }
}

#[test]
fn conversion_can_be_cancelled_between_records_without_mixing_progress_units() {
    let temp = Temp::new();
    let record = fs::read(source("ascii.mrc")).unwrap();
    let mut file = fs::File::create(&temp.0).unwrap();
    for _ in 0..50_000 { file.write_all(&record).unwrap(); }
    drop(file);
    let owner = temp.0.file_name().unwrap().to_string_lossy().into_owned();
    let task_owner = owner.clone();
    let path = temp.0.clone();
    let handle = std::thread::spawn(move || jobs::with_owner(Some(&task_owner), || jobs::run_result("MARC conversion test", || import(&path, None, &ImportOptions::default()))));
    let started = Instant::now();
    let id = loop {
        let list = jobs::list_owned(&owner);
        if let Some(job) = list.as_array().unwrap().iter().find(|j| j["label"] == "Converting MARC records" && j["done"].as_u64().unwrap_or(0) > 0 && j["state"] == "running") {
            assert_eq!(job["unit"], "bytes");
            assert!(job["done"].as_u64().unwrap() <= job["total"].as_u64().unwrap());
            break job["id"].as_u64().unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(10), "MARC conversion did not enter its byte stage");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(jobs::cancel_owned(&owner, id));
    match handle.join().unwrap() { Err(e) => assert_eq!(e, jobs::CANCELLED), Ok(_) => panic!("conversion must stop after cancellation") }
    let list = jobs::list_owned(&owner);
    assert_eq!(list.as_array().unwrap().iter().find(|j| j["id"] == id).unwrap()["state"], "cancelled");
}
