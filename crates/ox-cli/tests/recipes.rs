//! The same OpenRefine-generated ecological recipe fixtures exercise CLI, HTTP and saved projects.
use ox_core::{grel::Value as Cell, Project};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn fixtures() -> Value { serde_json::from_str(include_str!("../../../tests/recipes/fixtures.json")).unwrap() }
fn work(label: &str) -> PathBuf {
    let dir = root().join(format!("target/recipe-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn input(dir: &Path, case: &Value) -> PathBuf {
    if case["guessTypes"] == true { return root().join("tests/recipes/typed-input.openrefine.tar.gz") }
    let path = dir.join("input.csv");
    std::fs::write(&path, case["csv"].as_str().unwrap()).unwrap();
    path
}
fn applied(case: &Value) -> usize {
    if !case["oxPreflightError"].is_null() { return 0 }
    case["oxFailureStep"].as_u64().or_else(|| case["failureStep"].as_u64())
        .map_or_else(|| case["ops"].as_array().unwrap().len(), |i| i as usize - 1)
}

// Match the existing operations suite: blank cells agree, error presence agrees, wording may differ.
fn normalize(value: &Value) -> Value {
    match value {
        Value::Object(m) if m.is_empty() => Value::Null,
        Value::Object(m) if m.contains_key("e") => json!({ "e": true }),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), normalize(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        Value::Number(n) if n.as_f64().is_some_and(|v| v.is_finite() && v.fract() == 0.0) => json!(n.as_f64().unwrap() as i64),
        other => other.clone(),
    }
}
fn table(project: &Project) -> Value {
    let t = project.table();
    let rows: Vec<_> = (0..t.rows).map(|r| {
        let cells: Vec<_> = t.columns.iter().map(|c| match c.get(r) {
            Cell::Null => Value::Null,
            Cell::Str(v) => json!({ "v": v }),
            Cell::Int(v) => json!({ "v": v }),
            Cell::Num(v) => json!({ "v": v }),
            Cell::Bool(v) => json!({ "v": v }),
            Cell::Error(v) => json!({ "e": v }),
            other => panic!("unexpected recipe fixture cell: {other:?}"),
        }).collect();
        json!({ "starred": t.starred[r], "flagged": t.flagged[r], "cells": cells })
    }).collect();
    json!({ "columns": t.names, "rows": rows })
}
fn expected(case: &Value, initial: &Project) -> Value {
    let n = applied(case);
    if n == 0 { table(initial) } else { case["steps"][n - 1]["table"].clone() }
}

#[test]
fn cli_reusable_recipes_match_reference_and_protect_output_on_failure() {
    let base = work("cli");
    for (i, case) in fixtures()["cases"].as_array().unwrap().iter().enumerate() {
        let dir = base.join(i.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let input = input(&dir, case);
        let before = std::fs::read(&input).unwrap();
        let recipe = dir.join("recipe.json");
        let output = dir.join("result.openrefine.tar.gz");
        std::fs::write(&recipe, case["ops"].to_string()).unwrap();
        let n = applied(case);
        let failure = n < case["ops"].as_array().unwrap().len();
        if failure { std::fs::write(&output, "existing output").unwrap(); }
        let run = Command::new(env!("CARGO_BIN_EXE_ox")).arg("apply").arg(&recipe).arg(&input).arg("-o").arg(&output).output().unwrap();
        let error = String::from_utf8_lossy(&run.stderr);
        assert_eq!(run.status.success(), !failure, "{}: {error}", case["name"]);
        assert_eq!(std::fs::read(&input).unwrap(), before, "input must remain unchanged");
        if failure {
            assert!(error.contains("step 2"), "{}: {error}", case["name"]);
            if let Some(text) = case["oxPreflightError"].as_str() {
                assert!(error.contains(text) && error.contains("no steps were applied"), "{error}");
            } else {
                assert!(error.contains("output was not written"), "{error}");
            }
            assert_eq!(std::fs::read(&output).unwrap(), b"existing output");
        } else {
            let p = Project::open(&output).unwrap();
            assert_eq!(normalize(&table(&p)), normalize(&expected(case, &Project::open(&input).unwrap())), "{}", case["name"]);
            assert_eq!(p.steps().len(), n);
        }
    }
}

struct Server { child: Child, addr: SocketAddr }
impl Drop for Server { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }
impl Server {
    fn start(data: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_ox")).args(["serve", "--port", &addr.port().to_string(), "--data"])
            .arg(data).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let server = Self { child, addr };
        let until = Instant::now() + Duration::from_secs(15);
        while TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err() {
            assert!(Instant::now() < until, "test server did not start");
            std::thread::sleep(Duration::from_millis(25));
        }
        server
    }
    fn request(addr: SocketAddr, cmd: &str, args: Value) -> (u16, Value) {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
        let body = args.to_string();
        write!(stream, "POST /api/{cmd} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").expect("HTTP response");
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(body).unwrap_or_else(|_| panic!("non-JSON response: {response}")))
    }
    fn call(&self, cmd: &str, args: Value) -> Value {
        let (status, body) = Self::request(self.addr, cmd, args);
        assert_eq!(status, 200, "{cmd}: {body}");
        body["data"].clone()
    }
}

#[test]
fn http_recipe_partial_failures_persist_across_restart_and_undo() {
    let dir = work("http");
    let mut server = Server::start(&dir.join("store"));
    let fixtures = fixtures();
    let cases = fixtures["cases"].as_array().unwrap();
    let mut projects = vec![];
    for (i, case) in cases.iter().enumerate() {
        let csv = dir.join(format!("input-{i}.csv"));
        std::fs::write(&csv, case["csv"].as_str().unwrap()).unwrap();
        let summary = server.call("open", json!({ "path": csv, "name": case["name"], "options": { "guessCellValueTypes": case["guessTypes"] == true } }));
        let id = summary["id"].as_str().unwrap().to_owned();
        let (status, result) = Server::request(server.addr, "apply_recipe", json!({ "ops": case["ops"] }));
        let n = applied(case);
        if n < case["ops"].as_array().unwrap().len() {
            assert_eq!(status, 400, "{}: {result}", case["name"]);
            let error = result.as_str().unwrap();
            assert!(error.contains("step 2"), "{error}");
            if let Some(text) = case["oxPreflightError"].as_str() {
                assert!(error.contains(text) && error.contains("no steps were applied"), "{error}");
            } else {
                assert!(error.contains("1 of ") && error.contains("remain applied"), "{error}");
            }
            if let Some(text) = case["oxError"].as_str() { assert!(error.contains(text), "{error}"); }
        } else { assert_eq!(status, 200, "{}: {result}", case["name"]); }
        let summary = server.call("summary", json!({}));
        assert_eq!(summary["cursor"], n);
        assert_eq!(summary["history"].as_array().unwrap().len(), n);
        projects.push(id);
    }
    drop(server);
    server = Server::start(&dir.join("store"));
    for (i, (case, id)) in cases.iter().zip(projects).enumerate() {
        let n = applied(case);
        let summary = server.call("open_project", json!({ "id": id }));
        assert_eq!(summary["cursor"], n, "{}", case["name"]);
        let output = dir.join(format!("reopened-{i}.openrefine.tar.gz"));
        server.call("export_project", json!({ "path": output }));
        let p = Project::open(&output).unwrap();
        let initial = Project::open(&input(&dir, case)).unwrap();
        assert_eq!(normalize(&table(&p)), normalize(&expected(case, &initial)), "{}", case["name"]);
        server.call("goto", json!({ "index": 0 }));
        assert_eq!(server.call("summary", json!({}))["cursor"], 0);
        server.call("goto", json!({ "index": n }));
        assert_eq!(server.call("summary", json!({}))["history"].as_array().unwrap().len(), n);
    }
}

#[test]
fn cancelling_http_recipe_keeps_only_completed_steps_and_saves_them() {
    let dir = work("cancel");
    let case = fixtures()["cases"][1].clone();
    let csv = input(&dir, &case);
    let server = Server::start(&dir.join("store"));
    let id = server.call("open", json!({ "path": csv }))["id"].as_str().unwrap().to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/measurement", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let endpoint = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut request = [0; 2048];
        socket.read(&mut request).unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(15)).unwrap();
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    });
    let addr = server.addr;
    let first = case["ops"][0].clone();
    let recipe = std::thread::spawn(move || Server::request(addr, "apply_recipe", json!({ "ops": [first, {
        "op": "core/column-addition-by-fetching-urls", "baseColumnName": "genus", "newColumnName": "fetched", "columnInsertIndex": 4,
        "urlExpression": format!("grel:'{url}'"), "onError": "store-error", "delay": 0, "cacheResponses": true, "httpHeadersJson": [],
    }] })));
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let jobs = server.call("processes", json!({}));
    let active = jobs.as_array().unwrap().iter().find(|p| p["label"] == "Fetching URLs" && p["state"] == "running").unwrap();
    let job_id = active["id"].clone();
    server.call("cancel_process", json!({ "id": active["id"] }));
    release_tx.send(()).unwrap();
    endpoint.join().unwrap();
    let (status, error) = recipe.join().unwrap();
    assert_eq!(status, 400);
    let error = error.as_str().unwrap();
    assert!(error.starts_with("step 2: Cancelled; 1 of 2 recipe step(s) remain applied"), "{error}");
    let jobs = server.call("processes", json!({}));
    let outcome = jobs.as_array().unwrap().iter().find(|p| p["id"] == job_id).unwrap();
    assert_eq!(outcome["state"], "cancelled");
    assert!(outcome["stages"].as_array().unwrap().iter().any(|s| s["label"] == "Fetching URLs" && s["state"] == "cancelled"));
    assert_eq!(outcome["stages"].as_array().unwrap().last().unwrap()["state"], "completed", "saving earlier steps succeeded");
    assert_eq!(server.call("summary", json!({}))["cursor"], 1);
    drop(server);
    let server = Server::start(&dir.join("store"));
    let summary = server.call("open_project", json!({ "id": id }));
    assert_eq!(summary["cursor"], 1);
    assert!(!summary["columns"].as_array().unwrap().contains(&json!("fetched")));
    let output = dir.join("cancelled.openrefine.tar.gz");
    server.call("export_project", json!({ "path": output }));
    assert_eq!(normalize(&table(&Project::open(&output).unwrap())), normalize(&case["steps"][0]["table"]));
    server.call("goto", json!({ "index": 0 }));
    server.call("goto", json!({ "index": 1 }));
}
