//! Media uploads to Commons against OpenRefine 3.10.1 (tests/wikibase/media-gen.mjs): both log in
//! to the same mock MediaWiki API (never a real wiki), upload a new file with its captions and
//! statements, and edit an existing file and its wikitext.
use ox_core::{import::{import, ImportOptions}, recon::Judgment, wikibase, Project};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The mock Commons of media-gen.mjs; it records each request, a file as its name, size and hash.
fn mock(existing: Value) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = format!("http://{}/w/api.php", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    std::thread::spawn(move || {
        let (mut page, mut uploaded) = (2000u64, std::collections::HashMap::<String, u64>::new());
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            if reader.read_line(&mut first).is_err() { continue; }
            let (mut length, mut boundary) = (0, None);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() { break; }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") { length = v.trim().parse().unwrap(); }
                if lower.starts_with("content-type:") { boundary = line.split("boundary=").nth(1).map(|b| b.trim().to_string()); }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let mut q = serde_json::Map::new();
            let target = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            for (k, v) in url::form_urlencoded::parse(target.split_once('?').map_or("", |(_, q)| q).as_bytes()) { q.insert(k.into_owned(), json!(v.into_owned())); }
            match &boundary {
                Some(b) => {
                    let mark = format!("--{b}").into_bytes();
                    let mut rest = &body[..];
                    while let Some(at) = find(rest, &mark) {
                        rest = &rest[at + mark.len()..];
                        let Some(head_end) = find(rest, b"\r\n\r\n") else { break };
                        let head = String::from_utf8_lossy(&rest[..head_end]).to_string();
                        let end = find(&rest[head_end..], &mark).map_or(rest.len(), |e| head_end + e);
                        let content = &rest[head_end + 4..end.saturating_sub(2).max(head_end + 4)];
                        let name = head.split("name=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("").to_string();
                        match head.split("filename=\"").nth(1).and_then(|s| s.split('"').next()) {
                            Some(file) => { q.insert(name, json!({ "filename": file, "size": content.len(), "sha256": format!("{:x}", Sha256::digest(content)) })); }
                            None => { q.insert(name, json!(String::from_utf8_lossy(content))); }
                        }
                    }
                }
                None => for (k, v) in url::form_urlencoded::parse(&body) { q.insert(k.into_owned(), json!(v.into_owned())); },
            }
            let text = |k: &str| q.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let reply = match (text("action").as_str(), text("meta").as_str()) {
                ("query", m) if m.contains("tokens") => json!({ "batchcomplete": "", "query": { "tokens": if text("type") == "login" { json!({ "logintoken": "logintoken+\\" }) } else { json!({ "csrftoken": "csrftoken+\\" }) } } }),
                ("login", _) => json!({ "login": { "result": "Success", "lguserid": 7, "lgusername": "Tester" } }),
                ("query", "wbcontentlanguages") => json!({ "query": { "wbcontentlanguages": { "en": { "code": "en" } } } }),
                ("upload", _) => {
                    let id = page; page += 1;
                    uploaded.insert(text("filename"), id);
                    json!({ "upload": { "result": "Success", "filename": text("filename"), "pageid": id } })
                }
                ("query", _) => {
                    let title = text("titles");
                    let id = uploaded.get(title.trim_start_matches("File:")).copied().unwrap_or(0);
                    json!({ "query": { "pages": { id.to_string(): { "pageid": id, "ns": 6, "title": title } } } })
                }
                ("wbgetentities", _) => {
                    let mut entities = serde_json::Map::new();
                    for id in text("ids").split('|').filter(|s| !s.is_empty()) {
                        let mut doc = json!({ "id": id, "type": "mediainfo", "pageid": id[1..].parse::<u64>().unwrap_or(0), "ns": 6, "lastrevid": 700, "descriptions": {} });
                        if let Some(e) = existing.get(id) { for (k, v) in e.as_object().unwrap() { doc[k] = v.clone(); } }
                        entities.insert(id.to_string(), doc);
                    }
                    json!({ "entities": entities, "success": 1 })
                }
                ("wbeditentity", _) => { let rev = 800 + page; page += 1; json!({ "entity": { "id": text("id"), "type": "mediainfo", "lastrevid": rev }, "success": 1 }) }
                ("purge", _) => json!({ "purge": [{ "ns": 6, "pageid": text("pageids").parse::<u64>().unwrap_or(0), "purged": "" }] }),
                ("edit", _) => { let rev = 900 + page; page += 1; json!({ "edit": { "result": "Success", "newrevid": rev } }) }
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

/// What both send, the free parts set aside: the edit group batch id, OpenRefine's tracking
/// category in the uploaded page (OxRefine adds none), and the order of statements in an edit.
fn normalized(mut params: Value) -> Value {
    let obj = params.as_object_mut().unwrap();
    for key in ["summary", "comment"] {
        if let Some(s) = obj.get(key).and_then(Value::as_str) {
            let s = match s.split_once("/b/OR/") { Some((head, tail)) => format!("{head}/b/OR/{{BATCH}}{}", &tail[tail.find('|').unwrap_or(tail.len())..]), None => s.to_string() };
            obj.insert(key.into(), json!(s));
        }
    }
    if let Some(t) = obj.get("text").and_then(Value::as_str) {
        let t = t.trim_end_matches("\n[[Category:Uploaded with OpenRefine]]").to_string();
        obj.insert("text".into(), json!(t));
    }
    if let Some(data) = obj.remove("data") {
        let mut data: Value = serde_json::from_str(data.as_str().unwrap()).unwrap();
        if let Some(claims) = data.get_mut("claims").and_then(Value::as_array_mut) { claims.sort_by_key(|c| c.to_string()); }
        obj.insert("data".into(), data);
    }
    params
}

#[test]
fn commons_uploads_send_what_openrefine_sends() {
    let f: Value = serde_json::from_slice(&std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/wikibase/media.json")).unwrap()).unwrap();
    let (api, seen) = mock(f["existing"].clone());
    let host = api.trim_end_matches("/w/api.php").to_string();
    // the files, and the project naming them
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in f["files"].as_object().unwrap() {
        let bytes: Vec<u8> = body.as_str().unwrap().chars().map(|c| c as u8).collect();
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let folder = dir.path().to_str().unwrap().to_string();
    let csv = dir.path().join("media.csv");
    std::fs::write(&csv, f["csv"].as_str().unwrap().replace("{FILES}", &folder)).unwrap();
    let mut p = Project::new("media", import(&csv, Some("csv"), &ImportOptions::default()).unwrap());
    let (commons, wikidata) = ("https://commons.wikimedia.org/entity/", "http://www.wikidata.org/entity/");
    let only = |column: &str, value: &str| json!({ "mode": "row-based", "facets": [{ "type": "list", "name": column, "columnName": column, "expression": "value", "omitBlank": false, "omitError": false, "selection": [{ "v": { "v": value, "l": value } }], "selectBlank": false, "selectError": false, "invert": false }] });
    let setup = [
        json!({ "op": "core/recon-mark-new-topics", "engineConfig": only("media", "NEW1"), "columnName": "media", "shareNewTopics": false, "service": "https://commonsreconcile.toolforge.org/en/api", "identifierSpace": commons, "schemaSpace": commons }),
        json!({ "op": "core/recon-match-specific-topic-to-cells", "engineConfig": only("media", "M100"), "columnName": "media", "match": { "id": "M100", "name": "Existing test.pdf", "types": [] }, "identifierSpace": commons, "schemaSpace": commons }),
        json!({ "op": "core/recon-match-specific-topic-to-cells", "engineConfig": only("depicts", "Q145"), "columnName": "depicts", "match": { "id": "Q145", "name": "United Kingdom", "types": [] }, "identifierSpace": wikidata, "schemaSpace": "http://www.wikidata.org/prop/direct/" }),
        json!({ "op": "core/recon-match-specific-topic-to-cells", "engineConfig": only("depicts", "Q30"), "columnName": "depicts", "match": { "id": "Q30", "name": "United States of America", "types": [] }, "identifierSpace": wikidata, "schemaSpace": "http://www.wikidata.org/prop/direct/" }),
    ];
    for op in setup { p.apply(serde_json::from_value(op).unwrap()).unwrap(); }
    let schema: Value = serde_json::from_str(&f["schema"].to_string().replace("{API}", &api)).unwrap();
    p.apply(serde_json::from_value(json!({ "op": "wikidata/save-wikibase-schema", "schema": schema })).unwrap()).unwrap();
    let original = p.table().clone();
    wikibase::login(&api, &json!({ "username": "Tester", "password": "test-bot-password" })).unwrap();
    seen.lock().unwrap().clear();
    p.apply(serde_json::from_value(f["operation"]["operation"].clone()).unwrap()).unwrap();

    // the requests, token fetches aside (OpenRefine asks for one before each kind of edit)
    let skip = |r: &&Value| r["type"] == "csrf" || r["meta"] == "wbcontentlanguages";
    let ours: Vec<Value> = seen.lock().unwrap().iter().filter(|r| !skip(r)).cloned().map(normalized).collect();
    let theirs: Vec<Value> = f["requests"].as_array().unwrap().iter().map(|r| r["params"].clone()).filter(|r| !skip(&r)).map(normalized).collect();
    assert_eq!(ours.len(), theirs.len(), "{ours:#?}");
    for (a, b) in ours.iter().zip(&theirs) { assert_eq!(a, b); }

    // the new file matched in its cell, each row's result
    let t = p.table();
    assert_eq!(json!(t.names), f["columns"]);
    let media = t.col("media").unwrap();
    let results = t.col("Wikibase editing results").unwrap();
    for (row, cells) in f["rows"].as_array().unwrap().iter().enumerate() {
        let r = t.columns[media].recon(row).unwrap();
        assert_eq!(json!({ "j": if r.judgment == Judgment::Matched { "matched" } else { "new" }, "m": r.matched.as_ref().map(|m| m.id.clone()) }), cells[media]["r"], "row {row}");
        assert_eq!(t.columns[results].get(row).as_str(), cells[results]["v"].as_str().map(|s| s.replace("{HOST}", &host)).as_deref(), "row {row} result");
    }
    let before = seen.lock().unwrap().len();
    Project::new("replay", original).apply(p.steps().last().unwrap().clone()).unwrap();
    assert_eq!(seen.lock().unwrap().len(), before, "replaying never uploads again");
    wikibase::logout(&api);
}
