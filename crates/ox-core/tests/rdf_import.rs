//! RDF imports against OpenRefine 3.10.1 (tests/rdf/gen.mjs). Jena orders rows and columns by its
//! hash tables and names blank nodes randomly, so tables are compared as records: each subject with
//! the multiset of (predicate, value) pairs, blank nodes by count rather than by id. The columns,
//! the number of rows and which rows start a record must be the same.
use ox_core::{grel::Value, import::{import, inspect, ImportOptions}};
use serde_json::{json, Value as J};
use std::{collections::BTreeSet, path::Path};

/// A blank node id: 32 hex digits (Turtle, JSON-LD, OxRefine) or a UUID (Jena's RDF/XML parser)
fn blank(s: &str) -> bool {
    let hex = |s: &str| s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    s.len() == 32 && hex(s) || s.len() == 36 && s.split('-').map(str::len).eq([8, 4, 4, 4, 12]) && hex(&s.replace('-', ""))
}

/// (records, distinct blank nodes, rows that start a record)
fn records(columns: &[String], rows: &[Vec<Option<String>>]) -> (Vec<(String, Vec<(String, String)>)>, usize, Vec<bool>) {
    let mut out: Vec<(String, Vec<(String, String)>)> = vec![];
    let mut blanks = BTreeSet::new();
    let mut norm = |s: &str| if blank(s) { blanks.insert(s.to_string()); "_:".to_string() } else { s.to_string() };
    let starts = rows.iter().map(|r| r[0].is_some()).collect();
    for row in rows {
        if let Some(subject) = &row[0] { out.push((norm(subject), vec![])); }
        let record = out.last_mut().unwrap();
        for (c, cell) in row.iter().enumerate().skip(1) {
            if let Some(v) = cell { record.1.push((columns[c].clone(), norm(v))); }
        }
    }
    for r in &mut out { r.1.sort(); }
    out.sort();
    (out, blanks.len(), starts)
}

#[test]
fn rdf_imports_match_openrefine_records() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/rdf");
    let fixture: J = serde_json::from_str(include_str!("../../../tests/rdf/fixtures.json")).unwrap();
    let base = url::Url::from_directory_path(root.join("inputs").canonicalize().unwrap()).unwrap().to_string();
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let path = root.join("inputs").join(case["file"].as_str().unwrap());
        let imported = import(&path, case["format"].as_str(), &ImportOptions::default());
        if case.get("error").is_some() { assert!(imported.is_err(), "{name} must fail"); continue; }
        let t = imported.unwrap_or_else(|e| panic!("{name}: {e}"));
        let ours: Vec<Vec<Option<String>>> = (0..t.rows).map(|r| t.columns.iter().map(|c| match c.get(r) {
            Value::Null => None,
            Value::Str(s) => Some(s.replace(&base, "<base>/")),
            other => panic!("{name}: unexpected cell {other:?}"),
        }).collect()).collect();
        let columns: Vec<String> = serde_json::from_value(case["columns"].clone()).unwrap();
        let theirs: Vec<Vec<Option<String>>> = case["rows"].as_array().unwrap().iter()
            .map(|r| r.as_array().unwrap().iter().map(|c| c["v"].as_str().map(String::from)).collect()).collect();
        assert_eq!(t.names.iter().collect::<BTreeSet<_>>(), columns.iter().collect::<BTreeSet<_>>(), "{name} columns");
        assert_eq!(t.names[0], "subject");
        assert_eq!(ours.len(), theirs.len(), "{name} rows");
        let (a, b) = (records(&t.names, &ours), records(&columns, &theirs));
        assert_eq!(a.0, b.0, "{name} records");
        assert_eq!(a.1, b.1, "{name} blank nodes");
        assert_eq!(a.2.iter().filter(|s| **s).count(), b.2.iter().filter(|s| **s).count(), "{name} records count");
        checked += 1;
    }
    assert_eq!(checked, 9);
    // the import wizard guesses the format from the name, with OpenRefine's (only) option
    for w in fixture["wizard"].as_array().unwrap() {
        let path = root.join("inputs").join(w["file"].as_str().unwrap());
        let guess = inspect(&path).unwrap();
        let expected = json!({"text/rdf/ttl":"rdf-turtle","text/rdf/nt":"rdf-ntriples","text/rdf/xml":"rdf-xml","text/rdf/ld+json":"json-ld"})[w["format"].as_str().unwrap()].clone();
        assert_eq!(guess["format"], expected, "{}", w["file"]);
        assert!(guess["options"]["encoding"].is_string());
        let options: ImportOptions = serde_json::from_value(guess["options"].clone()).unwrap();
        assert_eq!(import(&path, None, &options).unwrap().rows as u64, w["rows"].as_u64().unwrap(), "{}", w["file"]);
    }
}

#[test]
fn blank_nodes_are_stable_and_shared_between_subject_and_object() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/rdf/inputs/basic.nt");
    let a = import(&path, Some("text/rdf/nt"), &ImportOptions::default()).unwrap();
    let b = import(&path, Some("rdf-ntriples"), &ImportOptions::default()).unwrap();
    let cells = |t: &ox_core::table::Table| (0..t.rows).map(|r| t.columns.iter().map(|c| c.get(r).clone()).collect::<Vec<_>>()).collect::<Vec<_>>();
    assert_eq!(cells(&a), cells(&b), "the same file imports the same way");
    let knows = a.col("http://xmlns.com/foaf/0.1/knows").unwrap();
    let friend = (0..a.rows).map(|r| a.columns[knows].get(r)).find_map(|v| match v { Value::Str(s) if blank(s) => Some(s.clone()), _ => None }).unwrap();
    assert!((0..a.rows).any(|r| a.columns[0].get(r) == &Value::Str(friend.clone())), "the friend's own record uses the same id");
}

#[test]
fn json_ld_loads_a_context_from_a_web_address() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/context.jsonld", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).unwrap();
        let body = r#"{"@context": {"name": "http://xmlns.com/foaf/0.1/name"}}"#;
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let dir = std::env::temp_dir().join(format!("ox-rdf-context-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("remote.jsonld");
    std::fs::write(&path, format!(r#"{{"@context": "{url}", "@id": "http://example.org/x", "name": "X"}}"#)).unwrap();
    let t = import(&path, Some("text/rdf/ld+json"), &ImportOptions::default()).unwrap();
    server.join().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(t.names, ["subject", "http://xmlns.com/foaf/0.1/name"]);
    assert_eq!(t.columns[1].get(0), &Value::Str("X".into()));
}
