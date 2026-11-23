//! perform-wikibase-edits against OpenRefine 3.10.1 (tests/wikibase/upload-gen.mjs): both log in
//! to the same mock MediaWiki API (a test bot password, never a real wiki) and upload one schema.
//! The requests, the matched cells, the results column and the history operation must agree.
use ox_core::{import::{import, ImportOptions}, ops::Op, recon::Judgment, wikibase, Project};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

fn read(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../").join(path)).unwrap()).unwrap()
}

/// The mock wiki of upload-gen.mjs, answering the same way; it records each request's parameters.
fn mock(existing: Value, datatypes: Value) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = format!("http://{}/w/api.php", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    std::thread::spawn(move || {
        let mut next = 1000u64;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            if reader.read_line(&mut first).is_err() { continue; }
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() { break; }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") { length = v.trim().parse().unwrap(); }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let target = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            let query = target.split_once('?').map_or("", |(_, q)| q).to_string();
            let mut q = serde_json::Map::new();
            for (k, v) in url::form_urlencoded::parse(query.as_bytes()).chain(url::form_urlencoded::parse(&body)) { q.insert(k.into_owned(), json!(v.into_owned())); }
            let text = |k: &str| q.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let reply = match (text("action").as_str(), text("meta").as_str()) {
                ("query", m) if m.contains("tokens") => json!({ "batchcomplete": "", "query": { "tokens": if text("type") == "login" { json!({ "logintoken": "logintoken+\\" }) } else { json!({ "csrftoken": "csrftoken+\\" }) } } }),
                ("login", _) => json!({ "login": { "result": "Success", "lguserid": 7, "lgusername": "Tester" } }),
                ("query", "wbcontentlanguages") => json!({ "batchcomplete": "", "query": { "wbcontentlanguages": { "en": { "code": "en" }, "de": { "code": "de" }, "fr": { "code": "fr" }, "mul": { "code": "mul" } } } }),
                ("wbgetentities", _) => {
                    let mut entities = serde_json::Map::new();
                    for id in text("ids").split('|').filter(|s| !s.is_empty()) {
                        let base = json!({ "id": id, "pageid": 1, "ns": 0, "title": id, "lastrevid": 500, "modified": "2024-01-01T00:00:00Z" });
                        let doc = match existing.get(id) {
                            Some(e) => { let mut d = base; d["type"] = json!("item"); d["sitelinks"] = json!({}); for (k, v) in e.as_object().unwrap() { d[k] = v.clone(); } d }
                            None if id.starts_with('P') => json!({ "id": id, "type": "property", "datatype": datatypes.get(id).cloned().unwrap_or(json!("string")) }),
                            None => json!({ "id": id, "missing": "" }),
                        };
                        entities.insert(id.to_string(), doc);
                    }
                    json!({ "entities": entities, "success": 1 })
                }
                ("wbeditentity", _) => {
                    let data: Value = serde_json::from_str(&text("data")).unwrap_or(json!({}));
                    let id = if q.contains_key("new") { let id = format!("Q{next}"); next += 1; id } else { text("id") };
                    next += 1;
                    let claims = if data["claims"].is_array() { json!({}) } else { data["claims"].clone() };
                    json!({ "entity": { "labels": data["labels"], "claims": claims, "id": id, "type": "item", "lastrevid": 600 + next - 1 }, "success": 1 })
                }
                _ => json!({ "error": { "code": "unknown", "info": "mock" } }),
            };
            let cookie = if text("action") == "login" { "Set-Cookie: mocksession=1; Path=/\r\n" } else { "" };
            log.lock().unwrap().push(Value::Object(q));
            let body = reply.to_string();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    (api, seen)
}

/// A request's parameters as both send them: the edit group batch id and the claims' order are free.
fn normalized(mut params: Value) -> Value {
    let obj = params.as_object_mut().unwrap();
    if let Some(s) = obj.get("summary").and_then(Value::as_str) {
        let s = regex_free_batch(s);
        obj.insert("summary".into(), json!(s));
    }
    if let Some(data) = obj.remove("data") {
        let mut data: Value = serde_json::from_str(data.as_str().unwrap()).unwrap();
        if let Some(claims) = data.get_mut("claims").and_then(Value::as_array_mut) { claims.sort_by_key(|c| c.to_string()); }
        obj.insert("data".into(), data);
    }
    params
}

fn regex_free_batch(s: &str) -> String {
    match s.split_once("/b/OR/") {
        Some((head, tail)) => format!("{head}/b/OR/{{BATCH}}{}", &tail[tail.find('|').unwrap_or(tail.len())..]),
        None => s.to_string(),
    }
}

#[test]
fn login_and_upload_send_what_openrefine_sends() {
    let fixtures = read("tests/wikibase/fixtures.json");
    let upload = read("tests/wikibase/upload.json");
    let (api, seen) = mock(upload["existing"].clone(), upload["datatypes"].clone());
    let host = api.trim_end_matches("/w/api.php").to_string();
    // the reconciled project and the schema
    let path = std::env::temp_dir().join(format!("ox-wikibase-upload-{}.csv", std::process::id()));
    std::fs::write(&path, fixtures["csv"].as_str().unwrap()).unwrap();
    let mut p = Project::new("upload", import(&path, Some("csv"), &ImportOptions::default()).unwrap());
    let _ = std::fs::remove_file(&path);
    for op in fixtures["setup"].as_array().unwrap() { p.apply(serde_json::from_value(op.clone()).unwrap()).unwrap(); }
    let schema: Value = serde_json::from_str(&upload["schema"].to_string().replace("{API}", &api)).unwrap();
    p.apply(serde_json::from_value(json!({ "op": "wikidata/save-wikibase-schema", "schema": schema })).unwrap()).unwrap();
    let original = p.table().clone();

    // uploading needs a login, then logs in as OpenRefine does
    let operation: Op = serde_json::from_value(upload["operation"]["operation"].clone()).unwrap();
    assert!(p.apply(operation.clone()).unwrap_err().contains("Log in"), "no upload without a login");
    assert!(seen.lock().unwrap().is_empty());
    let login = wikibase::login(&api, &json!({ "username": "Tester", "password": "test-bot-password" })).unwrap();
    assert_eq!(login, json!({ "logged_in": true, "username": "Tester" }));
    assert_eq!(*seen.lock().unwrap(), upload["login"].as_array().unwrap().iter().map(|r| r["params"].clone()).collect::<Vec<_>>());

    // the upload
    seen.lock().unwrap().clear();
    p.apply(operation).unwrap();
    let ours: Vec<Value> = seen.lock().unwrap().iter().filter(|r| r["meta"] != "wbcontentlanguages").cloned().map(normalized).collect();
    let theirs: Vec<Value> = upload["requests"].as_array().unwrap().iter().map(|r| normalized(r["params"].clone())).collect();
    assert_eq!(ours.len(), theirs.len(), "{ours:#?}");
    for (a, b) in ours.iter().zip(&theirs) { assert_eq!(a, b); }

    // new items matched in their cells; each row's result
    let t = p.table();
    assert_eq!(json!(t.names), upload["columns"]);
    let item = t.col("item").unwrap();
    let results = t.col("Wikibase editing results").unwrap();
    for (row, cells) in upload["rows"].as_array().unwrap().iter().enumerate() {
        let brief = t.columns[item].recon(row).map(|r| json!({ "j": if r.judgment == Judgment::Matched { "matched" } else { "new" }, "m": r.matched.as_ref().map(|m| m.id.clone()) }));
        assert_eq!(brief.unwrap_or(Value::Null), cells[item]["r"], "row {row} item");
        let expected = cells[results]["v"].as_str().map(|s| s.replace("{HOST}", &host));
        assert_eq!(t.columns[results].get(row).as_str(), expected.as_deref(), "row {row} result");
    }
    // the history step as OpenRefine records it, and a replay that doesn't touch the wiki
    let step = p.steps().last().unwrap();
    assert_eq!(step.recipe_json(), upload["operation"]["operation"]);
    let before = seen.lock().unwrap().len();
    let mut replay = Project::new("replay", original);
    replay.apply(step.clone()).unwrap();
    assert_eq!(seen.lock().unwrap().len(), before, "replaying an upload must not edit again");
    assert_eq!(replay.table().names, t.names);
    assert_eq!(wikibase::logout(&api), json!({ "logged_in": false }));
}
