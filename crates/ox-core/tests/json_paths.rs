use ox_core::{grel::Value, import::{import, inspect, json_paths, ImportOptions}};
use serde_json::{json, Value as J};
use std::{fs, path::PathBuf};

struct Source(PathBuf);
impl Source {
    fn new(bytes: &[u8], suffix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("oxrefine-json-test-{}-{}.{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), suffix));
        fs::write(&path, bytes).unwrap(); Self(path)
    }
}
impl Drop for Source { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
fn cell(value: &Value) -> J {
    match value {
        Value::Null => J::Null,
        Value::Str(s) => json!({ "v": s }), Value::Int(v) => json!({ "v": v }),
        Value::Num(v) => json!({ "v": v }), Value::Bool(v) => json!({ "v": v }),
        other => panic!("unexpected fixture value: {}", other.display()),
    }
}

#[test]
fn json_paths_and_streamed_records_match_verified_release_fixtures() {
    let fixtures: J = serde_json::from_str(include_str!("../../../tests/import/json-paths-fixtures.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let lines = case["lines"].as_bool().unwrap_or(false);
        let source = Source::new(case["text"].as_str().unwrap().as_bytes(), if lines { "jsonl" } else { "json" });
        let mut options: ImportOptions = serde_json::from_value(json!({ "recordPath": case["recordPath"], "jsonLinesRoot": lines,
            "limit": case["limit"].as_i64().unwrap_or(-1) })).unwrap();
        for legacy in [false, true].into_iter().take(if case.get("legacyRecordPath").is_some() { 2 } else { 1 }) {
            if legacy { options.json_lines_root = false; options.record_path = serde_json::from_value(case["legacyRecordPath"].clone()).unwrap(); }
            let table = import(&source.0, None, &options).unwrap_or_else(|e| panic!("{}: {e}", case["name"]));
            if case.get("referenceError").is_some() {
                assert_eq!(table.rows, 1, "OxRefine intentionally stops at its global limit before malformed trailing JSONL");
                continue;
            }
            let columns: Vec<String> = serde_json::from_value(case["columns"].clone()).unwrap();
            assert_eq!(table.names, columns, "{} legacy={legacy}", case["name"]);
            let expected: Vec<J> = case["rows"].as_array().unwrap().iter().take(case["oxRows"].as_u64().unwrap_or(u64::MAX) as usize).cloned().collect();
            let rows: Vec<J> = (0..table.rows).map(|r| J::Array(table.columns.iter().map(|c| cell(c.get(r))).collect())).collect();
            assert_eq!(rows, expected, "{} legacy={legacy}", case["name"]);
        }
    }
}

#[test]
fn inspection_suggests_nested_records_and_bounded_counts_are_never_exact() {
    let source = Source::new(include_bytes!("../../../tests/import/nested.json"), "json");
    let info = inspect(&source.0).unwrap();
    assert_eq!(info["options"]["recordPath"], json!(["_", "data", "items", "_"]));
    let options: ImportOptions = serde_json::from_value(info["options"].clone()).unwrap();
    assert_eq!(import(&source.0, None, &options).unwrap().rows, 2);
    let chosen = info["json"]["paths"].as_array().unwrap().iter().find(|p| p["path"] == info["options"]["recordPath"]).unwrap();
    assert_eq!(chosen["count"], 2);
    assert_eq!(info["json"]["sampled"], false);
    let source = Source::new("{\"id\":1}\n".repeat(1000).as_bytes(), "jsonl");
    let meta = json_paths(&source.0, None, &ImportOptions::default()).unwrap();
    assert_eq!(meta["suggestedPath"], json!(["_"]));
    assert_eq!(meta["paths"][0]["count"], 100);
    assert_eq!(meta["sampled"], true);
    let source = Source::new(format!("[{}]", "{\"id\":1},".repeat(10000) + "{}").as_bytes(), "json");
    let meta = json_paths(&source.0, None, &ImportOptions::default()).unwrap();
    assert_eq!(meta["sampled"], true);
    assert!(meta["paths"].as_array().unwrap().len() <= 256);
    assert!(meta["paths"].as_array().unwrap().iter().find(|p| p["path"] == json!(["_", "_"])).unwrap()["count"].as_u64().unwrap() < 10001);
}

#[test]
fn jsonl_limits_decode_streams_and_preserve_the_explicit_legacy_root() {
    let source = Source::new(b"\xef\xbb\xbf{\"id\":1}\nINVALID\n", "jsonl");
    let canonical = ImportOptions { record_path: vec!["_".into()], json_lines_root: true, limit: 1, ..Default::default() };
    assert_eq!(import(&source.0, None, &canonical).unwrap().rows, 1);
    match import(&source.0, None, &ImportOptions { limit: -1, ..canonical.clone() }) {
        Err(e) => assert!(e.contains("line 2")), Ok(_) => panic!("malformed unlimited JSONL must fail"),
    }
    let source = Source::new(b"{\"city\":\"\xe9\"}\n{\"city\":\"ok\"}\n", "jsonl");
    let table = import(&source.0, None, &ImportOptions { encoding: "windows-1252".into(), ..canonical.clone() }).unwrap();
    assert_eq!(table.columns[0].get(0), &Value::Str("é".into()));
    let text = "{\"city\":\"😀京\"}\n{\"city\":\"next\"}\n";
    let bytes: Vec<u8> = [0xff, 0xfe].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let source = Source::new(&bytes, "jsonl");
    let options = ImportOptions { encoding: "UTF-16LE".into(), ..canonical.clone() };
    assert_eq!(import(&source.0, None, &options).unwrap().columns[0].get(0), &Value::Str("😀京".into()));
    let source = Source::new(b"{\"id\":1}\n{\"id\":2}\n", "jsonl");
    let legacy = ImportOptions { json_lines_root: false, ..canonical };
    assert_eq!(import(&source.0, None, &legacy).unwrap().rows, 2, "legacy whole-array path is one record, even with limit 1");
    let source = Source::new(b"[{\"id\":1,\"items\":[{\"x\":\"a\"},{\"x\":\"b\"}]},{\"id\":2}]\n", "jsonl");
    let table = import(&source.0, None, &ImportOptions::default()).unwrap();
    assert_eq!(table.names, vec!["_ - id", "_ - items - x"]);
    assert_eq!(table.rows, 2, "legacy per-line array retains its earlier cross-item alignment");
    assert_eq!(table.columns[0].get(1), &Value::Int(2));
    assert_eq!(table.columns[1].get(1), &Value::Str("b".into()));
}

#[test]
fn discovery_reports_null_empty_wrong_paths_and_rejects_unbounded_malformed_sources() {
    for text in ["null", "{}", "[]"] {
        let source = Source::new(text.as_bytes(), "json");
        let info = inspect(&source.0).unwrap();
        let options: ImportOptions = serde_json::from_value(info["options"].clone()).unwrap();
        assert_eq!(import(&source.0, None, &options).unwrap().rows, 0);
    }
    let source = Source::new(b"{\"items\":[{\"id\":1}]}", "json");
    let options = ImportOptions { record_path: vec!["_".into(), "missing".into()], ..Default::default() };
    assert_eq!(import(&source.0, None, &options).unwrap().rows, 0);
    let source = Source::new(b"{\"items\":[{\"id\":1},]}", "json");
    assert!(json_paths(&source.0, None, &ImportOptions::default()).is_err());
}

#[test]
fn bounded_metadata_never_splits_a_unicode_scalar() {
    let head = "{\"text\":\"";
    for (character, before_limit) in [("😀", 2), ("é", 1)] {
        let text = format!("{}{}{}\"}}", head, "a".repeat(262144 - head.len() - before_limit), character);
        for suffix in ["json", "jsonl"] {
            let source = Source::new(text.as_bytes(), suffix);
            let meta = json_paths(&source.0, None, &ImportOptions::default()).unwrap();
            assert_eq!(meta["sampled"], true, "{suffix} boundary inside {character}");
            assert_eq!(meta["suggestedPath"], json!(["_"]));
        }
    }
}
