use ox_core::{grel::Value, import::{import, wikitext_tables, ImportOptions}, table::Table};
use serde_json::{json, Value as J};
use std::{io::{BufRead, BufReader, Read, Write}, net::{TcpListener, TcpStream}, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ox-wikitext-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, text: &str) -> PathBuf {
        let path = self.0.join("table.wiki");
        std::fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

struct Service {
    url: String,
    seen: Arc<Mutex<Vec<J>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Service {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(vec![]));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_seen, worker_stop) = (seen.clone(), stop.clone());
        let worker = thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if worker_stop.load(Ordering::Relaxed) { break; }
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap().to_string();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap() == 0 || header.trim().is_empty() { break; }
                    if let Some((key, value)) = header.split_once(':') {
                        if key.eq_ignore_ascii_case("Content-Length") { length = value.trim().parse().unwrap(); }
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&body).into_owned().collect();
                let queries: J = serde_json::from_str(&form["queries"]).unwrap();
                let mut result = serde_json::Map::new();
                for (key, query) in queries.as_object().unwrap() {
                    worker_seen.lock().unwrap().push(query.clone());
                    let alpha = query["query"].as_str().unwrap().ends_with("Alpha");
                    result.insert(key.clone(), json!({ "result": [{ "id": if alpha { "Q111" } else { "Q222" },
                        "name": if alpha { "Alpha entity" } else { "Beta entity" }, "type": [], "score": 100, "match": true }] }));
                }
                let (status, body) = match path.as_str() {
                    "/fail" => (500, "{}".to_string()),
                    "/invalid" => (200, "null".to_string()),
                    "/missing-id" => (200, json!({ "q0": { "result": [{ "name": "No ID", "match": true }] } }).to_string()),
                    _ => (200, J::Object(result).to_string()),
                };
                let mut stream = stream;
                let head = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes()));
            }
        });
        Self { url, seen, stop, worker: Some(worker) }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        self.worker.take().unwrap().join().unwrap();
    }
}

fn normalized(v: &J, service: &str) -> J {
    match v {
        J::Number(n) => json!(n.as_f64()),
        J::String(s) => json!(s.replace(service, "{SERVICE}")),
        J::Object(m) => J::Object(m.iter().filter(|(k, _)| *k != "id" || !m.contains_key("identifierSpace")).map(|(k, v)| (k.clone(), normalized(v, service))).collect()),
        J::Array(a) => J::Array(a.iter().map(|v| normalized(v, service)).collect()),
        other => other.clone(),
    }
}

fn rows(table: &Table) -> J {
    json!((0..table.rows).map(|r| table.columns.iter().map(|c| {
        let mut out = match c.get(r) { Value::Null => J::Null, Value::Str(s) => json!({ "v": s }), other => panic!("unexpected typed wiki value {other:?}") };
        if let Some(recon) = c.recon(r) { out["r"] = recon.to_json(false); }
        out
    }).collect::<Vec<_>>()).collect::<Vec<_>>())
}

fn fixtures() -> J {
    serde_json::from_str(include_str!("../../../tests/wikitext/fixtures.json")).unwrap()
}

#[test]
fn wikitext_matches_actual_openrefine() {
    let temp = Temp::new();
    let service = Service::new();
    let fx = fixtures();
    let mut failures = vec![];
    for case in fx["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let path = temp.file(case["text"].as_str().unwrap());
        let options: ImportOptions = serde_json::from_str(&case["options"].to_string().replace("{SERVICE}", &service.url)).unwrap();
        service.seen.lock().unwrap().clear();
        let imported = import(&path, Some("text/wiki"), &options);
        if case.get("error").is_some() {
            assert!(imported.is_err(), "{name}: reference importer failed, Ox must report unsupported markup");
            continue;
        }
        let table = match imported { Ok(table) => table, Err(error) => { failures.push(format!("{name}: {error}")); continue; } };
        if json!(table.names) != case["columns"] || normalized(&rows(&table), &service.url) != normalized(&case["rows"], &service.url) {
            failures.push(format!("{name}: expected {} {}, got {:?} {}", case["columns"], case["rows"], table.names, rows(&table)));
            continue;
        }
        assert_eq!(*service.seen.lock().unwrap(), case["queries"].as_array().unwrap().clone(), "{name}: only reference simple URL queries are allowed");
        if case.get("knownDifference").is_none() {
            let column_recon: Vec<_> = table.columns.iter().map(|col| json!({
                "config": col.recon.as_ref().and_then(|r| r.config.clone()),
                "stats": col.recon.as_ref().map(|r| r.stats(col)),
            })).collect();
            if normalized(&json!(column_recon), &service.url) != normalized(&case["columnRecon"], &service.url) {
                failures.push(format!("{name}: column metadata expected {}, got {}", case["columnRecon"], json!(column_recon)));
            }
        } else {
            assert!(table.columns[1].recon.is_none(), "reference URL column must not acquire another column's config");
            assert!(table.columns[2].recon.as_ref().unwrap().config.is_some());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn table_selection_and_inspection_are_local_and_validated() {
    let temp = Temp::new();
    let basic = &fixtures()["cases"][0];
    let text = basic["text"].as_str().unwrap();
    let path = temp.file(&format!("{text}\n{text}"));
    let mut options = ImportOptions::default();
    options.table_indexes = vec![1];
    let inspected = wikitext_tables(&path, &options).unwrap();
    assert_eq!(inspected["tables"].as_array().unwrap().len(), 2);
    assert!(inspected["options"]["wikiUrl"].is_null());
    let selected = import(&path, Some("wiki"), &options).unwrap();
    assert_eq!(json!(selected.names), basic["columns"]);
    assert_eq!(rows(&selected), basic["rows"]);
    options.table_indexes = vec![2];
    assert!(import(&path, Some("wiki"), &options).err().unwrap().contains("index 2"));
    options.table_indexes = vec![0, 0];
    assert!(import(&path, Some("wiki"), &options).err().unwrap().contains("more than once"));
}

#[test]
fn invalid_service_results_and_excessive_spans_fail_the_import() {
    let temp = Temp::new();
    let service = Service::new();
    let path = temp.file("{|\n! A\n|-\n| [[Alpha]]\n|}\n");
    let mut options = ImportOptions::default();
    options.wiki_url = Some("https://en.wikipedia.org/wiki/".into());
    for endpoint in ["fail", "invalid", "missing-id"] {
        options.recon_service = Some(format!("{}/{endpoint}", service.url));
        assert!(import(&path, Some("wiki"), &options).is_err(), "{endpoint}: must not fabricate matches");
    }
    options.wiki_url = None;
    let path = temp.file("{|\n! A\n|-\n| colspan=10001 | Too wide\n|}\n");
    assert!(import(&path, Some("wiki"), &options).err().unwrap().contains("10,000 columns"));
    options.blank_spanning_cells = false;
    let path = temp.file(&format!("{{|\n! A\n|-\n| colspan=10000 | {}\n|}}\n", "x".repeat(16 * 1024)));
    assert!(import(&path, Some("wiki"), &options).err().unwrap().contains("64 MiB"));
    options.blank_spanning_cells = true;
    let path = temp.file(&format!("{{|\n! A\n|-\n| colspan=10000 | Wide\n{}|}}\n", "|-\n| row\n".repeat(1000)));
    assert!(import(&path, Some("wiki"), &options).err().unwrap().contains("padding short rows"));
}

fn saved_cells(table: &Table) -> J {
    json!((0..table.rows).map(|r| table.columns.iter().map(|c| json!({
        "value": c.get(r).to_java_string(), "type": c.get(r).type_name(),
        "recon": c.recon(r).map(|r| r.to_json(true)),
    })).collect::<Vec<_>>()).collect::<Vec<_>>())
}

#[test]
fn reconciled_archive_sources_survive_store_and_openrefine_roundtrips_without_network() {
    use ox_core::{orproject, store::{Source, Store}, Project};
    let temp = Temp::new();
    let service = Service::new();
    let source = &fixtures()["cases"].as_array().unwrap().iter().find(|c| c["name"] == "local reconciliation").unwrap().clone();
    let text = source["text"].as_str().unwrap();
    let mut options: ImportOptions = serde_json::from_str(&source["options"].to_string().replace("{SERVICE}", &service.url)).unwrap();
    options.include_file_sources = true;
    let folder = temp.0.join("input");
    std::fs::create_dir(&folder).unwrap();
    for name in ["one.wiki", "two.wiki"] { std::fs::write(folder.join(name), text).unwrap(); }
    let from_folder = import(&folder, Some("wikitext"), &options).unwrap();
    assert_eq!(from_folder.rows, 4);
    assert!(from_folder.col("File").is_ok());
    let a = from_folder.col("A").unwrap();
    assert_eq!((0..4).filter(|&r| from_folder.columns[a].recon(r).is_some()).count(), 2);
    let zip_path = temp.0.join("input.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    for name in ["one.wiki", "two.wiki"] {
        writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    options.include_archive_file_name = true;
    let imported = import(&zip_path, Some("text/wiki"), &options).unwrap();
    assert_eq!(imported.rows, 4);
    assert!(imported.col("File").is_ok() && imported.col("Archive").is_ok());
    let expected = normalized(&saved_cells(&imported), &service.url);
    let expected_configs: Vec<_> = imported.columns.iter().map(|c| c.recon.as_ref().and_then(|r| r.config.clone())).collect();
    let requests = service.seen.lock().unwrap().len();
    let service_url = service.url.clone();
    let seen = service.seen.clone();
    drop(service);

    let store_dir = temp.0.join("store");
    let store = Store::open(&store_dir).unwrap();
    let meta = store.create("Wiki import", &imported, Some(Source { path: zip_path.to_string_lossy().into(), format: Some("wikitext".into()), options })).unwrap();
    drop(store);
    let (meta, table, history) = Store::open(&store_dir).unwrap().load(&meta.id).unwrap();
    let reopened = Project::restore(meta, table, history).unwrap();
    assert!(reopened.recipe().is_empty());
    assert_eq!(normalized(&saved_cells(reopened.table()), &service_url), expected);
    let output = temp.0.join("wiki.openrefine.tar.gz");
    orproject::write(&reopened, &output).unwrap();
    let roundtrip = orproject::read(&output).unwrap().into_project().0;
    assert_eq!(roundtrip.table().names, imported.names);
    assert_eq!(normalized(&saved_cells(roundtrip.table()), &service_url), expected);
    let actual_configs: Vec<_> = roundtrip.table().columns.iter().map(|c| c.recon.as_ref().and_then(|r| r.config.clone())).collect();
    assert_eq!(actual_configs, expected_configs);
    assert_eq!(seen.lock().unwrap().len(), requests, "saving/reopening must use the imported baseline, without reconciliation requests");
}
